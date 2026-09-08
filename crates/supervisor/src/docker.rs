use std::collections::HashMap;
use std::default::Default;
use std::path::PathBuf;

use base64::Engine;
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::{EndpointSettings, HostConfig, HostConfigLogConfig, PortBinding};
use bollard::network::{ConnectNetworkOptions, CreateNetworkOptions};
use futures_util::StreamExt;
use lazyboy_control::{
    ActionRequest, BrowserRequest, CommandRequest, CommandResult, EnsureScreenRequest,
    EnsureScreenResult, HOME, RecordingRequest, ScreenTarget, TEAM_SCREEN_LIMIT, normalize_display,
    normalize_workspace_path, screen_layout,
};
use tokio::time::{Duration, sleep};

const SCREEN_PORT_COUNT: u16 = TEAM_SCREEN_LIMIT as u16;

pub struct DockerHost {
    docker: Docker,
    image: String,
    control_token: String,
}

pub struct Provisioned {
    pub id: String,
    pub resumed: bool,
    pub screen_url: Option<String>,
}

pub struct ObservePayload {
    pub json: serde_json::Value,
}

impl ObservePayload {
    fn from_json(value: serde_json::Value) -> Result<Self, String> {
        // The screenshot travels inside `json` as base64; only its presence is
        // checked here so a broken capture fails fast instead of returning an
        // empty observation.
        value
            .get("png_base64")
            .and_then(serde_json::Value::as_str)
            .filter(|encoded| !encoded.is_empty())
            .ok_or_else(|| "missing png".to_string())?;
        Ok(Self { json: value })
    }
}

impl DockerHost {
    pub async fn connect(image: String, control_token: String) -> Result<Self, String> {
        let docker = Docker::connect_with_socket_defaults().map_err(|error| error.to_string())?;
        Ok(Self {
            docker,
            image,
            control_token,
        })
    }

    pub async fn provision(
        &self,
        home_key: &str,
        home_path: &str,
        space_id: &str,
    ) -> Result<Provisioned, String> {
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into());
        if home_key.is_empty()
            || !home_key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err("invalid home key".into());
        }
        let expected = PathBuf::from(&data_dir).join("homes").join(home_key);
        if *home_path != expected {
            return Err("home path is outside managed homes".into());
        }
        // Work on the container-local path, not the host daemon's bind path.
        tokio::fs::create_dir_all(&expected)
            .await
            .map_err(|e| e.to_string())?;
        let canonical = tokio::fs::canonicalize(&expected)
            .await
            .map_err(|e| e.to_string())?;
        let root = tokio::fs::canonicalize(&data_dir)
            .await
            .map_err(|e| e.to_string())?;
        if canonical != root.join("homes").join(home_key) {
            return Err("symlinked home is not allowed".into());
        }
        #[cfg(unix)]
        if std::env::var("HOST_DATA_DIR").is_ok() {
            std::os::unix::fs::chown(&canonical, Some(1000), Some(1000))
                .map_err(|e| e.to_string())?;
        }
        let home_path = host_bind_path(home_path);
        if let Some(existing) = self.find(home_key).await? {
            if self.container_reusable(&existing).await.unwrap_or(false) {
                self.wake(&existing).await?;
                let screen_url = self.screen_url(&existing, false).await.ok();
                return Ok(Provisioned {
                    id: existing,
                    resumed: true,
                    screen_url,
                });
            }
            let _ = self.destroy(&existing).await;
        }

        let name = container_name(home_key);
        let network = network_name(home_key);
        self.ensure_network(&network).await?;
        let mut labels = HashMap::new();
        labels.insert("lazyboy.homeKey".into(), home_key.to_string());
        labels.insert("lazyboy.spaceId".into(), space_id.to_string());
        labels.insert("lazyboy.controlVersion".into(), "3".into());

        let mut port_bindings = HashMap::new();
        let mut exposed = HashMap::new();
        for slot in 0..SCREEN_PORT_COUNT {
            let port = format!("{}/tcp", 6080 + slot);
            port_bindings.insert(
                port.clone(),
                Some(vec![PortBinding {
                    host_ip: Some("127.0.0.1".into()),
                    host_port: Some("0".into()),
                }]),
            );
            exposed.insert(port, HashMap::new());
        }

        let host_config = HostConfig {
            log_config: Some(HostConfigLogConfig {
                typ: Some("json-file".into()),
                config: Some(HashMap::from([
                    ("max-size".into(), "10m".into()),
                    ("max-file".into(), "3".into()),
                ])),
            }),
            binds: Some(
                std::iter::once(format!("{home_path}:{HOME}"))
                    .chain(lxcfs_binds())
                    .collect(),
            ),
            port_bindings: Some(port_bindings),
            memory: Some(computer_memory_bytes()),
            nano_cpus: Some(computer_nano_cpus()),
            pids_limit: Some(computer_pids_limit()),
            cap_drop: if computer_sudo_enabled() {
                None
            } else {
                Some(vec!["ALL".into()])
            },
            cap_add: if computer_sudo_enabled() {
                None
            } else {
                Some(vec!["SETUID".into(), "SETGID".into()])
            },
            security_opt: if computer_sudo_enabled() {
                None
            } else {
                Some(vec!["no-new-privileges:true".into()])
            },
            privileged: Some(false),
            shm_size: Some(512 * 1024 * 1024),
            network_mode: Some(network),
            ..Default::default()
        };

        let config = Config {
            image: Some(self.image.clone()),
            // The entrypoint starts as root so it can apply the env-only sudo
            // policy, then drops the desktop process to the unprivileged user.
            // Exec requests below still run explicitly as 1000:1000.
            user: None,
            hostname: Some(name.clone()),
            env: Some(vec![
                "DISPLAY=:1".into(),
                format!("HOME={HOME}"),
                format!(
                    "LAZYBOY_CONTROL_TOKEN={}",
                    scoped_control_token(&self.control_token, home_key)
                ),
                format!(
                    "LAZYBOY_COMPUTER_SUDO={}",
                    if computer_sudo_enabled() {
                        "true"
                    } else {
                        "false"
                    }
                ),
                format!(
                    "LAZYBOY_COMPUTER_DRIVER={}",
                    lazyboy_control::ComputerDriver::from_env().as_str()
                ),
            ]),
            labels: Some(labels),
            exposed_ports: Some(exposed),
            host_config: Some(host_config),
            working_dir: Some(HOME.into()),
            ..Default::default()
        };

        let created = match self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: name.clone(),
                    platform: None,
                }),
                config.clone(),
            )
            .await
        {
            Ok(created) => created,
            Err(error) if error.to_string().to_lowercase().contains("already in use") => {
                let _ = self
                    .docker
                    .remove_container(
                        &name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                self.docker
                    .create_container(
                        Some(CreateContainerOptions {
                            name: name.clone(),
                            platform: None,
                        }),
                        config,
                    )
                    .await
                    .map_err(|error| error.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        self.docker
            .start_container(&created.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|error| error.to_string())?;
        self.wait_running(&created.id).await?;
        // The container is reported as running before its primary desktop has
        // finished booting.  Returning sooner lets an API ensure-screen call
        // win the screen lock, which makes the container's own startup time
        // out and exit.  Wait for the entrypoint's readiness marker first.
        self.wait_ready(&created.id).await?;
        let screen_url = self.screen_url(&created.id, false).await.ok();
        Ok(Provisioned {
            id: created.id,
            resumed: false,
            screen_url,
        })
    }

    pub async fn exec_on(
        &self,
        id: &str,
        request: CommandRequest,
        target: &ScreenTarget,
    ) -> Result<CommandResult, String> {
        let cwd = match request.cwd {
            Some(cwd) if PathBuf::from(&cwd).is_absolute() => cwd,
            Some(cwd) => {
                let relative = normalize_workspace_path(&cwd).map_err(|error| error.to_string())?;
                if relative.is_empty() {
                    HOME.to_string()
                } else {
                    format!("{HOME}/{relative}")
                }
            }
            None => HOME.to_string(),
        };
        let timeout_ms = request.timeout_ms.unwrap_or(30_000).clamp(100, 120_000);
        let argv = if request.argv.is_empty() {
            vec!["/bin/echo".into(), "ready".into()]
        } else {
            request.argv
        };
        let mut bounded = vec![
            "timeout".into(),
            "--signal=TERM".into(),
            "--kill-after=2s".into(),
            format!("{}s", timeout_ms as f64 / 1000.0),
        ];
        bounded.extend(argv);
        self.exec_raw_cmd(id, &bounded, Some(&cwd), target, request.stdin)
            .await
    }

    pub async fn observe_payload(
        &self,
        id: &str,
        target: &ScreenTarget,
    ) -> Result<ObservePayload, String> {
        ObservePayload::from_json(self.control_observe_json(id, target).await?)
    }

    pub async fn act(&self, id: &str, request: ActionRequest) -> Result<serde_json::Value, String> {
        let target = ScreenTarget::from_parts(
            request.display.as_deref(),
            request.profile_path.as_deref(),
            None,
        );
        // A transport failure may follow a successful click. Never replay mutations.
        self.control_act(id, &request, &target).await
    }

    pub async fn browser(
        &self,
        id: &str,
        request: BrowserRequest,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        self.control_browser(id, &request, target).await
    }

    pub async fn recording(
        &self,
        id: &str,
        action: &str,
        request: RecordingRequest,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        self.control_recording(id, action, &request, target).await
    }

    pub async fn screen_url(&self, id: &str, interactive: bool) -> Result<String, String> {
        self.screen_url_for(id, 0, interactive).await
    }

    pub async fn screen_url_for(
        &self,
        id: &str,
        slot: u32,
        interactive: bool,
    ) -> Result<String, String> {
        let layout = screen_layout(slot).map_err(|error| error.to_string())?;
        if let Some(network) = screen_network() {
            match self.screen_container_name(id, &network).await {
                Ok(name) => {
                    let authority = format!("{name}:{}", layout.view_port);
                    return Ok(view_url(&authority, interactive));
                }
                Err(error) => tracing::warn!(
                    "screen network {network} unusable for {id}: {error}; using host ports"
                ),
            }
        }
        let port = self.published_host_port(id, layout.view_port).await?;
        Ok(view_url(&format!("127.0.0.1:{port}"), interactive))
    }

    /// Resolves the computer's container name and joins it to the shared screen
    /// network on demand, so computers started before that network existed keep
    /// working without reprovisioning.
    async fn screen_container_name(&self, id: &str, network: &str) -> Result<String, String> {
        for _ in 0..20 {
            let info = self
                .docker
                .inspect_container(id, None)
                .await
                .map_err(|error| error.to_string())?;
            if info.state.as_ref().and_then(|state| state.running) == Some(true) {
                let name = info.name.unwrap_or_default().trim_matches('/').to_string();
                if name.is_empty() {
                    return Err("computer container has no name".into());
                }
                let attached = info
                    .network_settings
                    .as_ref()
                    .and_then(|settings| settings.networks.as_ref())
                    .is_some_and(|networks| networks.contains_key(network));
                if !attached {
                    self.attach_screen_network(&name, network).await?;
                }
                return Ok(name);
            }
            sleep(Duration::from_millis(100)).await;
        }
        Err("computer is not running".into())
    }

    async fn attach_screen_network(&self, name: &str, network: &str) -> Result<(), String> {
        // Refuse a missing network before connect: Docker can otherwise retain
        // a broken attachment that prevents the container's next restart.
        self.docker
            .inspect_network::<String>(network, None)
            .await
            .map_err(|error| format!("screen network {network} is unavailable: {error}"))?;
        let result = self
            .docker
            .connect_network(
                network,
                ConnectNetworkOptions {
                    container: name.to_string(),
                    endpoint_config: EndpointSettings::default(),
                },
            )
            .await;
        if let Err(error) = result {
            let text = error.to_string();
            if !text.to_lowercase().contains("already") {
                return Err(format!("attach {name} to screen network {network}: {text}"));
            }
        }
        Ok(())
    }

    async fn published_host_port(&self, id: &str, view_port: u16) -> Result<String, String> {
        let key = format!("{view_port}/tcp");
        for _ in 0..20 {
            let info = self
                .docker
                .inspect_container(id, None)
                .await
                .map_err(|error| error.to_string())?;
            let running = info.state.as_ref().and_then(|state| state.running) == Some(true);
            if !running {
                let exit = info.state.and_then(|state| state.exit_code).unwrap_or(1);
                return Err(format!("computer is not running (exit {exit})"));
            }
            if let Some(port) = info
                .network_settings
                .and_then(|settings| settings.ports)
                .and_then(|ports| ports.get(&key).cloned())
                .flatten()
                .and_then(|bindings| bindings.into_iter().next())
                .and_then(|binding| binding.host_port)
                .filter(|port| !port.is_empty())
            {
                return Ok(port);
            }
            sleep(Duration::from_millis(100)).await;
        }
        Err(format!("screen port {view_port} is not published"))
    }

    pub async fn ensure_screen(
        &self,
        id: &str,
        request: EnsureScreenRequest,
    ) -> Result<EnsureScreenResult, String> {
        let layout = screen_layout(request.slot).map_err(|error| error.to_string())?;
        let script = format!(
            "lazyboy-screen ensure {} {} {} {}",
            request.slot,
            shell_single_quote(&request.profile_path),
            shell_single_quote(&request.bot_name),
            shell_single_quote(&request.bot_color)
        );
        let result = self
            .exec_argv(
                id,
                &["bash".into(), "-lc".into(), script],
                None,
                &ScreenTarget {
                    display: layout.display.clone(),
                    profile_path: Some(request.profile_path.clone()),
                    slot: request.slot,
                },
            )
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        Ok(EnsureScreenResult {
            slot: layout.slot,
            display: layout.display,
            view_port: layout.view_port,
        })
    }

    pub async fn list_files(&self, id: &str, path: &str) -> Result<Vec<serde_json::Value>, String> {
        let relative = normalize_workspace_path(path).map_err(|error| error.to_string())?;
        let target = if relative.is_empty() {
            HOME.to_string()
        } else {
            format!("{HOME}/{relative}")
        };
        let script = format!(
            r#"python3 - <<'PY'
import json, os
root = {target:?}
entries = []
try:
    names = sorted(os.listdir(root))
except FileNotFoundError:
    names = []
for name in names:
    full = os.path.join(root, name)
    kind = "dir" if os.path.isdir(full) else "file"
    size = os.path.getsize(full) if kind == "file" else 0
    rel = os.path.relpath(full, {home:?})
    entries.append({{"path": rel, "kind": kind, "size": size}})
print(json.dumps(entries))
PY"#,
            target = target,
            home = HOME
        );
        let result = self
            .exec_argv(
                id,
                &["bash".into(), "-lc".into(), script],
                None,
                &ScreenTarget::default(),
            )
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())
    }

    pub async fn read_file(&self, id: &str, relative: &str) -> Result<Vec<u8>, String> {
        let path = format!("{HOME}/{relative}");
        let (stdout, stderr, code) = self
            .exec_raw(
                id,
                &[
                    "python3".into(),
                    "-c".into(),
                    "import sys; sys.stdout.buffer.write(open(sys.argv[1],'rb').read())".into(),
                    path,
                ],
                None,
                &ScreenTarget::default(),
                None,
            )
            .await?;
        if code != 0 {
            return Err(String::from_utf8_lossy(&stderr).into_owned());
        }
        Ok(stdout)
    }

    pub async fn write_file(&self, id: &str, relative: &str, content: &[u8]) -> Result<(), String> {
        let target = format!("{HOME}/{relative}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);
        let script = format!(
            "python3 -c \"import os,base64,sys; p=sys.argv[1]; os.makedirs(os.path.dirname(p) or '.', exist_ok=True); open(p,'wb').write(base64.b64decode(sys.argv[2]))\" {target:?} {encoded:?}"
        );
        let result = self
            .exec_argv(
                id,
                &["bash".into(), "-lc".into(), script],
                None,
                &ScreenTarget::default(),
            )
            .await?;
        if result.code != 0 {
            Err(result.stderr)
        } else {
            Ok(())
        }
    }

    pub async fn stop(&self, id: &str) -> Result<(), String> {
        self.docker
            .stop_container(id, Some(StopContainerOptions { t: 8 }))
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn pause(&self, id: &str) -> Result<(), String> {
        match self.docker.pause_container(id).await {
            Ok(()) => Ok(()),
            Err(error) if docker_already(&error.to_string(), "paused") => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub async fn unpause(&self, id: &str) -> Result<(), String> {
        self.wake(id).await
    }

    pub async fn destroy(&self, id: &str) -> Result<(), String> {
        let info = self.docker.inspect_container(id, None).await.ok();
        let home_key = info
            .as_ref()
            .and_then(|info| info.config.as_ref())
            .and_then(|config| config.labels.as_ref())
            .and_then(|labels| labels.get("lazyboy.homeKey").cloned());
        let _ = self
            .docker
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
        if let Some(home_key) = home_key {
            let _ = self.docker.remove_network(&network_name(&home_key)).await;
        }
        Ok(())
    }

    async fn current_image_id(&self) -> Result<String, String> {
        self.docker
            .inspect_image(&self.image)
            .await
            .map_err(|error| error.to_string())?
            .id
            .ok_or_else(|| "computer image has no id".into())
    }

    async fn container_reusable(&self, id: &str) -> Result<bool, String> {
        let info = self
            .docker
            .inspect_container(id, None)
            .await
            .map_err(|error| error.to_string())?;
        if info
            .config
            .as_ref()
            .and_then(|c| c.labels.as_ref())
            .and_then(|l| l.get("lazyboy.controlVersion"))
            .map(String::as_str)
            != Some("3")
        {
            return Ok(false);
        }
        let wanted = self.current_image_id().await?;
        let have = info.image.unwrap_or_default();
        if !image_ids_match(&wanted, &have) {
            return Ok(false);
        }
        let running = info.state.as_ref().and_then(|state| state.running) == Some(true);
        let paused = info.state.as_ref().and_then(|state| state.paused) == Some(true);
        let exit = info
            .state
            .as_ref()
            .and_then(|state| state.exit_code)
            .unwrap_or(0);
        if paused {
            return Ok(true);
        }
        if !running && exit != 0 {
            return Ok(false);
        }
        Ok(true)
    }

    async fn find(&self, home_key: &str) -> Result<Option<String>, String> {
        let mut filters = HashMap::new();
        filters.insert("label".into(), vec![format!("lazyboy.homeKey={home_key}")]);
        let list = self
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|error| error.to_string())?;
        Ok(list.into_iter().next().and_then(|item| item.id))
    }

    async fn ensure_network(&self, name: &str) -> Result<(), String> {
        let result = self
            .docker
            .create_network(CreateNetworkOptions {
                name: name.to_string(),
                check_duplicate: true,
                driver: "bridge".into(),
                ..Default::default()
            })
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error) if error.to_string().to_lowercase().contains("already") => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    async fn wake(&self, id: &str) -> Result<(), String> {
        let info = self
            .docker
            .inspect_container(id, None)
            .await
            .map_err(|error| error.to_string())?;
        let running = info.state.as_ref().and_then(|state| state.running) == Some(true);
        let paused = info.state.as_ref().and_then(|state| state.paused) == Some(true);
        if paused {
            match self.docker.unpause_container(id).await {
                Ok(()) => {}
                Err(error) if docker_already(&error.to_string(), "not paused") => {}
                Err(error) => return Err(error.to_string()),
            }
            return self.wait_ready_fast(id).await;
        }
        if running {
            return self.wait_ready_fast(id).await;
        }
        self.docker
            .start_container(id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|error| error.to_string())?;
        self.wait_running(id).await?;
        self.wait_ready(id).await
    }

    async fn wait_running(&self, id: &str) -> Result<(), String> {
        for _ in 0..40 {
            let info = self
                .docker
                .inspect_container(id, None)
                .await
                .map_err(|error| error.to_string())?;
            if info.state.and_then(|state| state.running) == Some(true) {
                return Ok(());
            }
            sleep(Duration::from_millis(250)).await;
        }
        Err("container failed to start".into())
    }

    async fn wait_ready(&self, id: &str) -> Result<(), String> {
        self.wait_ready_attempts(id, 160).await
    }

    async fn wait_ready_fast(&self, id: &str) -> Result<(), String> {
        self.wait_ready_attempts(id, 20).await
    }

    async fn wait_ready_attempts(&self, id: &str, attempts: u32) -> Result<(), String> {
        for _ in 0..attempts {
            let info = self
                .docker
                .inspect_container(id, None)
                .await
                .map_err(|error| error.to_string())?;
            if info.state.as_ref().and_then(|state| state.paused) == Some(true) {
                return Err("computer is still paused".into());
            }
            if info.state.as_ref().and_then(|state| state.running) != Some(true) {
                let exit = info.state.and_then(|state| state.exit_code).unwrap_or(1);
                return Err(format!("computer exited during startup with code {exit}"));
            }
            match self
                .exec_argv(
                    id,
                    &["test".into(), "-f".into(), "/tmp/lazyboy/ready".into()],
                    None,
                    &ScreenTarget::default(),
                )
                .await
            {
                Ok(result) if result.code == 0 => return Ok(()),
                Ok(_) | Err(_) => sleep(Duration::from_millis(250)).await,
            }
        }
        Err("computer desktop failed to become ready".into())
    }

    async fn exec_argv(
        &self,
        id: &str,
        argv: &[String],
        cwd: Option<&str>,
        target: &ScreenTarget,
    ) -> Result<CommandResult, String> {
        self.exec_raw_cmd(id, argv, cwd, target, None).await
    }

    async fn exec_raw_cmd(
        &self,
        id: &str,
        argv: &[String],
        cwd: Option<&str>,
        target: &ScreenTarget,
        stdin: Option<String>,
    ) -> Result<CommandResult, String> {
        let (stdout, stderr, code) = self
            .exec_raw(id, argv, cwd, target, stdin.as_deref())
            .await?;
        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            code,
        })
    }

    async fn exec_raw(
        &self,
        id: &str,
        argv: &[String],
        cwd: Option<&str>,
        target: &ScreenTarget,
        stdin: Option<&str>,
    ) -> Result<(Vec<u8>, Vec<u8>, i32), String> {
        let display = normalize_display(&target.display);
        let mut env = vec![
            format!("DISPLAY={display}"),
            format!("HOME={HOME}"),
            "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ];
        if let Some(profile) = &target.profile_path {
            env.push(format!("LAZYBOY_BROWSER_PROFILE={profile}"));
        }
        let exec = self
            .docker
            .create_exec(
                id,
                CreateExecOptions {
                    attach_stdin: Some(stdin.is_some()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(argv.to_vec()),
                    working_dir: cwd.map(str::to_string),
                    env: Some(env),
                    user: Some("1000:1000".into()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let StartExecResults::Attached {
            mut output,
            mut input,
        } = self
            .docker
            .start_exec(
                &exec.id,
                Some(StartExecOptions {
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| error.to_string())?
        {
            if let Some(body) = stdin {
                use tokio::io::AsyncWriteExt;
                input
                    .write_all(body.as_bytes())
                    .await
                    .map_err(|error| error.to_string())?;
                input.shutdown().await.map_err(|error| error.to_string())?;
            }
            while let Some(chunk) = output.next().await {
                match chunk.map_err(|error| error.to_string())? {
                    bollard::container::LogOutput::StdOut { message } => stdout.extend_from_slice(
                        &message[..message
                            .len()
                            .min((16 * 1024 * 1024usize).saturating_sub(stdout.len()))],
                    ),
                    bollard::container::LogOutput::StdErr { message } => stderr.extend_from_slice(
                        &message[..message
                            .len()
                            .min((1024 * 1024usize).saturating_sub(stderr.len()))],
                    ),
                    _ => {}
                }
            }
        }
        let inspect = self
            .docker
            .inspect_exec(&exec.id)
            .await
            .map_err(|error| error.to_string())?;
        Ok((stdout, stderr, inspect.exit_code.unwrap_or(1) as i32))
    }

    pub async fn container_control_token(&self, id: &str) -> Result<String, String> {
        let info = self
            .docker
            .inspect_container(id, None)
            .await
            .map_err(|e| e.to_string())?;
        let labels = info
            .config
            .and_then(|c| c.labels)
            .ok_or("unmanaged container")?;
        let home = labels.get("lazyboy.homeKey").ok_or("unmanaged container")?;
        Ok(scoped_control_token(&self.control_token, home))
    }

    async fn control_observe_json(
        &self,
        id: &str,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        let token = self.container_control_token(id).await?;
        let result = self
            .exec_argv(
                id,
                &[
                    "curl".into(),
                    "-fsS".into(),
                    "--max-time".into(),
                    "20".into(),
                    "-X".into(),
                    "POST".into(),
                    "-H".into(),
                    format!("Authorization: Bearer {token}"),
                    "-H".into(),
                    format!("x-lazyboy-display: {}", target.display),
                    "http://127.0.0.1:7070/observe".into(),
                ],
                None,
                target,
            )
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())
    }

    async fn control_act(
        &self,
        id: &str,
        request: &ActionRequest,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
        let token = self.container_control_token(id).await?;
        let mut argv = vec![
            "curl".into(),
            "-fsS".into(),
            "--max-time".into(),
            "120".into(),
            "-H".into(),
            format!("Authorization: Bearer {token}"),
            "-H".into(),
            format!("x-lazyboy-display: {}", target.display),
            "-H".into(),
            "content-type: application/json".into(),
        ];
        if let Some(profile) = &target.profile_path {
            argv.extend(["-H".into(), format!("x-lazyboy-profile: {profile}")]);
        }
        argv.extend([
            "--data-binary".into(),
            "@-".into(),
            "http://127.0.0.1:7070/act".into(),
        ]);
        let result = self
            .exec_raw_cmd(id, &argv, None, target, Some(payload))
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())
    }

    async fn control_browser(
        &self,
        id: &str,
        request: &BrowserRequest,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
        let token = self.container_control_token(id).await?;
        let timeout = if request.action == "click" {
            "140"
        } else {
            "30"
        };
        let mut argv = vec![
            "curl".into(),
            "-fsS".into(),
            "--max-time".into(),
            timeout.into(),
            "-H".into(),
            format!("Authorization: Bearer {token}"),
            "-H".into(),
            format!("x-lazyboy-display: {}", target.display),
            "-H".into(),
            "content-type: application/json".into(),
        ];
        if let Some(profile) = &target.profile_path {
            argv.extend(["-H".into(), format!("x-lazyboy-profile: {profile}")]);
        }
        argv.extend([
            "--data-binary".into(),
            "@-".into(),
            "http://127.0.0.1:7070/browser".into(),
        ]);
        let result = self
            .exec_raw_cmd(id, &argv, None, target, Some(payload))
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())
    }

    async fn control_recording(
        &self,
        id: &str,
        action: &str,
        request: &RecordingRequest,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        let payload = serde_json::to_string(request).map_err(|error| error.to_string())?;
        let token = self.container_control_token(id).await?;
        let timeout = if action == "collect" { "30" } else { "20" };
        let mut argv = vec![
            "curl".into(),
            "-fsS".into(),
            "--max-time".into(),
            timeout.into(),
            "-H".into(),
            format!("Authorization: Bearer {token}"),
            "-H".into(),
            format!("x-lazyboy-display: {}", target.display),
            "-H".into(),
            "content-type: application/json".into(),
        ];
        if let Some(profile) = &target.profile_path {
            argv.extend(["-H".into(), format!("x-lazyboy-profile: {profile}")]);
        }
        argv.extend([
            "--data-binary".into(),
            "@-".into(),
            format!("http://127.0.0.1:7070/recording/{action}"),
        ]);
        let result = self
            .exec_raw_cmd(id, &argv, None, target, Some(payload))
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())
    }
}

fn image_ids_match(wanted: &str, have: &str) -> bool {
    let wanted = wanted.trim_start_matches("sha256:");
    let have = have.trim_start_matches("sha256:");
    !wanted.is_empty()
        && !have.is_empty()
        && (wanted == have || wanted.starts_with(have) || have.starts_with(wanted))
}

fn host_bind_path(path: &str) -> String {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into());
    let host_dir = std::env::var("HOST_DATA_DIR").unwrap_or_else(|_| data_dir.clone());
    let data_dir = data_dir.trim_end_matches('/');
    let host_dir = host_dir.trim_end_matches('/');
    path.strip_prefix(data_dir)
        .map(|rest| format!("{host_dir}{rest}"))
        .unwrap_or_else(|| path.to_string())
}

fn computer_memory_bytes() -> i64 {
    let mb = std::env::var("LAZYBOY_COMPUTER_MEMORY_MB")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 128)
        .unwrap_or(2048);
    mb * 1024 * 1024
}

fn computer_nano_cpus() -> i64 {
    let cpus = std::env::var("LAZYBOY_COMPUTER_CPUS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(2.0);
    (cpus * 1_000_000_000.0) as i64
}

fn computer_pids_limit() -> i64 {
    std::env::var("LAZYBOY_COMPUTER_PIDS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 64)
        .unwrap_or(2048)
}

fn computer_sudo_enabled() -> bool {
    matches!(
        std::env::var("LAZYBOY_COMPUTER_SUDO").as_deref(),
        Ok("1" | "true" | "yes")
    )
}

/// LXCFS supplies cgroup-aware /proc views so tools such as htop and free
/// report the Agent container's quota instead of the Docker host. It is
/// optional because Docker Desktop (macOS/Windows) does not ship LXCFS.
fn lxcfs_binds() -> impl Iterator<Item = String> {
    let root = std::env::var("LAZYBOY_LXCFS_ROOT").ok();
    ["cpuinfo", "loadavg", "meminfo", "stat", "swaps", "uptime"]
        .into_iter()
        .filter_map(move |name| {
            let root = root.as_deref()?;
            let source = PathBuf::from(root).join("proc").join(name);
            source
                .is_file()
                .then(|| format!("{}:/proc/{name}:ro", source.display()))
        })
}

fn docker_already(error: &str, needle: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("409") || error.contains(needle)
}

fn container_name(home_key: &str) -> String {
    let sanitized: String = home_key
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    format!("lb-{sanitized}")
        .trim_matches('-')
        .chars()
        .take(60)
        .collect()
}

fn network_name(home_key: &str) -> String {
    format!("lbnet-{}", container_name(home_key))
}

/// `LAZYBOY_SCREEN_NETWORK` places the API and the computer containers on one
/// shared Docker network. Without it the desktop proxy must reach published host
/// ports, which bind the host loopback and are unreachable from another container.
fn screen_network() -> Option<String> {
    std::env::var("LAZYBOY_SCREEN_NETWORK")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn view_url(authority: &str, interactive: bool) -> String {
    let view = if interactive { "false" } else { "true" };
    format!("http://{authority}/vnc_lite.html?resize=scale&view_only={view}")
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn scoped_control_token(master: &str, home: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(master.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(b"lazyboy-computer-control-v2:");
    mac.update(home.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    #[test]
    fn computer_credentials_do_not_reveal_or_share_the_master() {
        let master = "test-master-key-at-least-32-characters";
        let a = scoped_control_token(master, "a");
        let b = scoped_control_token(master, "b");
        assert_ne!(a, master);
        assert_ne!(a, b);
        assert_eq!(a, scoped_control_token(master, "a"));
    }
}

#[cfg(test)]
mod screen_url_tests {
    use super::*;

    #[test]
    fn desktop_urls_keep_authority_and_view_mode() {
        assert_eq!(
            view_url("lb-team-local-space:6080", false),
            "http://lb-team-local-space:6080/vnc_lite.html?resize=scale&view_only=true"
        );
        assert_eq!(
            view_url("127.0.0.1:32905", true),
            "http://127.0.0.1:32905/vnc_lite.html?resize=scale&view_only=false"
        );
    }
}
