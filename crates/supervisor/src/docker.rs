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
use bollard::models::{HostConfig, PortBinding};
use bollard::network::CreateNetworkOptions;
use futures_util::StreamExt;
use lazyboy_control::{
    ActionRequest, CommandRequest, CommandResult, EnsureScreenRequest, EnsureScreenResult, HOME,
    ScreenTarget, TEAM_SCREEN_LIMIT, action_pause_ms, launch_argv_on, normalize_display,
    normalize_workspace_path, open_argv_on, pointer_state_command_on, screen_layout,
    screenshot_command_on, window_list_command_on, xdotool_argv_on,
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
    pub png: Vec<u8>,
    pub json: serde_json::Value,
}

impl ObservePayload {
    fn from_json(value: serde_json::Value) -> Result<Self, String> {
        let encoded = value
            .get("png_base64")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "missing png".to_string())?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| error.to_string())?;
        Ok(Self { png, json: value })
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
        let home_path = host_bind_path(home_path);
        tokio::fs::create_dir_all(&home_path)
            .await
            .map_err(|error| error.to_string())?;
        let _ = tokio::process::Command::new("chown")
            .args(["-R", "1000:1000", &home_path])
            .status()
            .await;
        if let Some(existing) = self.find(home_key).await? {
            if self.container_reusable(&existing).await.unwrap_or(false) {
                self.docker
                    .start_container(&existing, None::<StartContainerOptions<String>>)
                    .await
                    .ok();
                self.wait_running(&existing).await?;
                self.wait_ready(&existing).await?;
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
            binds: Some(vec![format!("{home_path}:{HOME}")]),
            port_bindings: Some(port_bindings),
            memory: Some(computer_memory_bytes()),
            nano_cpus: Some(computer_nano_cpus()),
            pids_limit: Some(computer_pids_limit()),
            cap_drop: Some(vec!["ALL".into()]),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            privileged: Some(false),
            shm_size: Some(512 * 1024 * 1024),
            network_mode: Some(network),
            ..Default::default()
        };

        let config = Config {
            image: Some(self.image.clone()),
            user: Some("1000:1000".into()),
            hostname: Some(name.clone()),
            env: Some(vec![
                "DISPLAY=:1".into(),
                format!("HOME={HOME}"),
                format!("LAZYBOY_CONTROL_TOKEN={}", self.control_token),
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

    pub async fn exec(&self, id: &str, request: CommandRequest) -> Result<CommandResult, String> {
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
        let argv = if request.argv.is_empty() {
            vec!["/bin/echo".into(), "ready".into()]
        } else {
            request.argv
        };
        self.exec_argv(id, &argv, Some(&cwd), &ScreenTarget::default())
            .await
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
        let argv = if request.argv.is_empty() {
            vec!["/bin/echo".into(), "ready".into()]
        } else {
            request.argv
        };
        self.exec_argv(id, &argv, Some(&cwd), target).await
    }

    pub async fn observe(&self, id: &str) -> Result<Vec<u8>, String> {
        Ok(self
            .observe_payload(id, &ScreenTarget::default())
            .await?
            .png)
    }

    pub async fn observe_payload(
        &self,
        id: &str,
        target: &ScreenTarget,
    ) -> Result<ObservePayload, String> {
        let mut body = if let Ok(value) = self.control_observe_json(id, target).await {
            value
        } else {
            let (stdout, stderr, code) = self
                .exec_raw(id, &screenshot_command_on(&target.display), None, target)
                .await?;
            if code != 0 {
                return Err(String::from_utf8_lossy(&stderr).into_owned());
            }
            let mut body = serde_json::json!({
                "png_base64": base64::engine::general_purpose::STANDARD.encode(&stdout)
            });
            if let Ok(meta) = self.pointer_state(id, target).await {
                if let serde_json::Value::Object(map) = meta {
                    if let Some(obj) = body.as_object_mut() {
                        if let (Some(x), Some(y)) = (map.get("x"), map.get("y")) {
                            obj.insert("cursor".into(), serde_json::json!({ "x": x, "y": y }));
                        }
                        if map
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|id| !id.is_empty())
                        {
                            obj.insert(
                                "activeWindow".into(),
                                serde_json::json!({ "id": map.get("id"), "title": map.get("title") }),
                            );
                        }
                    }
                }
            }
            body
        };
        self.attach_window_elements(id, target, &mut body).await;
        ObservePayload::from_json(body)
    }

    async fn attach_window_elements(
        &self,
        id: &str,
        target: &ScreenTarget,
        body: &mut serde_json::Value,
    ) {
        if body
            .get("elements")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| !items.is_empty())
        {
            return;
        }
        let Ok(result) = self
            .exec_argv(id, &window_list_command_on(&target.display), None, target)
            .await
        else {
            return;
        };
        if result.code != 0 {
            return;
        }
        if let Ok(elements) = serde_json::from_str::<serde_json::Value>(&result.stdout) {
            body["elements"] = elements;
        }
    }

    async fn pointer_state(
        &self,
        id: &str,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        let result = self
            .exec_argv(id, &pointer_state_command_on(&target.display), None, target)
            .await?;
        if result.code != 0 {
            return Err(result.stderr);
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())
    }

    pub async fn act(&self, id: &str, request: ActionRequest) -> Result<serde_json::Value, String> {
        let target = ScreenTarget::from_parts(
            request.display.as_deref(),
            request.profile_path.as_deref(),
            None,
        );
        if let Ok(body) = self.control_act(id, &request, &target).await {
            return Ok(body);
        }
        let mut completed = 0usize;
        for action in &request.actions {
            match action {
                lazyboy_contracts::ComputerAction::Wait { ms } => {
                    sleep(Duration::from_millis(*ms as u64)).await;
                }
                lazyboy_contracts::ComputerAction::Open { path } => {
                    let _ = self
                        .exec_argv(
                            id,
                            &open_argv_on(&target.display, target.profile_path.as_deref(), path),
                            None,
                            &target,
                        )
                        .await?;
                }
                lazyboy_contracts::ComputerAction::Launch { application, uri } => {
                    let argv = launch_argv_on(
                        &target.display,
                        target.profile_path.as_deref(),
                        application,
                        uri.as_deref(),
                    )
                    .ok_or_else(|| "unknown application".to_string())?;
                    let _ = self.exec_argv(id, &argv, None, &target).await?;
                }
                other => {
                    let argv = xdotool_argv_on(&target.display, other)
                        .ok_or_else(|| "unsupported action".to_string())?;
                    let result = self.exec_argv(id, &argv, None, &target).await?;
                    if result.code != 0 {
                        return Err(result.stderr);
                    }
                }
            }
            let pause = action_pause_ms(action);
            if pause > 0 {
                sleep(Duration::from_millis(pause)).await;
            }
            completed += 1;
        }
        if request.settle_ms > 0 {
            sleep(Duration::from_millis(request.settle_ms as u64)).await;
        }
        let mut body = serde_json::json!({ "completed": completed });
        if request.observe {
            let payload = self.observe_payload(id, &target).await?;
            if let serde_json::Value::Object(map) = payload.json {
                if let Some(object) = body.as_object_mut() {
                    object.extend(map);
                }
            }
        }
        Ok(body)
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
        let port = self.published_host_port(id, layout.view_port).await?;
        let view = if interactive { "false" } else { "true" };
        Ok(format!(
            "http://127.0.0.1:{port}/vnc_lite.html?resize=scale&view_only={view}"
        ))
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
            "lazyboy-screen ensure {} {}",
            request.slot,
            shell_single_quote(&request.profile_path)
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
        let wanted = self.current_image_id().await?;
        let have = info.image.unwrap_or_default();
        if !image_ids_match(&wanted, &have) {
            return Ok(false);
        }
        let running = info.state.as_ref().and_then(|state| state.running) == Some(true);
        let exit = info
            .state
            .as_ref()
            .and_then(|state| state.exit_code)
            .unwrap_or(0);
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
        for _ in 0..160 {
            let info = self
                .docker
                .inspect_container(id, None)
                .await
                .map_err(|error| error.to_string())?;
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
        let (stdout, stderr, code) = self.exec_raw(id, argv, cwd, target).await?;
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
        if let StartExecResults::Attached { mut output, .. } = self
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
            while let Some(chunk) = output.next().await {
                match chunk.map_err(|error| error.to_string())? {
                    bollard::container::LogOutput::StdOut { message } => {
                        stdout.extend_from_slice(&message)
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        stderr.extend_from_slice(&message)
                    }
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

    async fn control_observe_json(
        &self,
        id: &str,
        target: &ScreenTarget,
    ) -> Result<serde_json::Value, String> {
        let script = format!(
            "curl -fsS -H 'Authorization: Bearer {}' -H 'x-lazyboy-display: {}' http://127.0.0.1:7070/observe",
            self.control_token, target.display
        );
        let result = self
            .exec_argv(id, &["bash".into(), "-lc".into(), script], None, target)
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
        let profile_header = target
            .profile_path
            .as_deref()
            .map(|profile| format!(" -H 'x-lazyboy-profile: {profile}'"))
            .unwrap_or_default();
        let script = format!(
            "curl -fsS -H 'Authorization: Bearer {}' -H 'x-lazyboy-display: {}'{profile_header} -H 'content-type: application/json' -d {} http://127.0.0.1:7070/act",
            self.control_token,
            target.display,
            shell_single_quote(&payload)
        );
        let result = self
            .exec_argv(id, &["bash".into(), "-lc".into(), script], None, target)
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

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}
