use async_trait::async_trait;
use base64::Engine;
use lazyboy_contracts::{ActiveWindow, ComputerObservation, CursorPosition, SandboxKind};
use lazyboy_contracts::ComputerCapabilities;
use lazyboy_control::{
    observation_from_png, ActionRequest, ActionResult, AdapterContext, CommandRequest, CommandResult,
    ComputerRef, EnsureScreenRequest, EnsureScreenResult, FileEntry, ProvisionRequest, SandboxError,
    SandboxProvider, ScreenSession,
};
use reqwest::Client;
use serde_json::Value;

pub struct DockerSandbox {
    client: Client,
    base_url: String,
    token: String,
}

impl DockerSandbox {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    fn headers(&self, context: &AdapterContext) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        headers.insert("x-lazyboy-space-id", context.space_id.parse().unwrap());
        if let Some(bot_id) = &context.bot_id {
            headers.insert("x-lazyboy-bot-id", bot_id.parse().unwrap());
        }
        if let Some(display) = &context.display {
            if let Ok(value) = display.parse() {
                headers.insert("x-lazyboy-display", value);
            }
        }
        if let Some(profile) = &context.profile_path {
            if let Ok(value) = profile.parse() {
                headers.insert("x-lazyboy-profile", value);
            }
        }
        if let Some(slot) = context.screen_slot {
            if let Ok(value) = slot.to_string().parse() {
                headers.insert("x-lazyboy-screen-slot", value);
            }
        }
        headers
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

#[async_trait]
impl SandboxProvider for DockerSandbox {
    async fn provision(
        &self,
        request: ProvisionRequest,
        context: &AdapterContext,
    ) -> Result<ComputerRef, SandboxError> {
        let response = self
            .client
            .post(self.url("/computers"))
            .headers(self.headers(context))
            .json(&serde_json::json!({
                "homeKey": request.home_key,
                "homePath": request.home_path,
                "spaceId": context.space_id,
            }))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SandboxError::message(format!("provision failed: {status} {body}")));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let id = body
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| SandboxError::message("missing computer id"))?
            .to_string();
        Ok(ComputerRef {
            id: id.clone(),
            home_key: request.home_key,
            kind: SandboxKind::Docker,
            provider_ref: id,
            fresh: body.get("resumed").and_then(Value::as_bool) != Some(true),
        })
    }

    async fn capabilities(
        &self,
        _computer: &ComputerRef,
        _context: &AdapterContext,
    ) -> Result<ComputerCapabilities, SandboxError> {
        Ok(ComputerCapabilities { multi_screen: true })
    }

    async fn ensure_screen(
        &self,
        computer: &ComputerRef,
        request: EnsureScreenRequest,
        context: &AdapterContext,
    ) -> Result<EnsureScreenResult, SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/screens", computer.id)))
            .headers(self.headers(context))
            .json(&request)
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SandboxError::message(format!("ensure screen failed: {status} {body}")));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(EnsureScreenResult {
            slot: body.get("slot").and_then(Value::as_u64).unwrap_or(request.slot as u64) as u32,
            display: body
                .get("display")
                .and_then(Value::as_str)
                .unwrap_or(":1")
                .to_string(),
            view_port: body
                .get("viewPort")
                .or_else(|| body.get("view_port"))
                .and_then(Value::as_u64)
                .unwrap_or(6080) as u16,
        })
    }

    async fn execute(
        &self,
        computer: &ComputerRef,
        request: CommandRequest,
        context: &AdapterContext,
    ) -> Result<CommandResult, SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/exec", computer.id)))
            .headers(self.headers(context))
            .json(&request)
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))
    }

    async fn observe(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<ComputerObservation, SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/observe", computer.id)))
            .headers(self.headers(context))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        decode_observation(&body)
    }

    async fn act(
        &self,
        computer: &ComputerRef,
        request: ActionRequest,
        context: &AdapterContext,
    ) -> Result<ActionResult, SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/act", computer.id)))
            .headers(self.headers(context))
            .json(&request)
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(ActionResult {
            completed: body.get("completed").and_then(Value::as_u64).unwrap_or(0) as usize,
            observation: if body.get("png_base64").is_some() {
                Some(decode_observation(&body)?)
            } else {
                None
            },
        })
    }

    async fn connect_screen(
        &self,
        computer: &ComputerRef,
        interactive: bool,
        context: &AdapterContext,
    ) -> Result<ScreenSession, SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/screen-mode", computer.id)))
            .headers(self.headers(context))
            .json(&serde_json::json!({
                "interactive": interactive,
                "slot": context.screen_slot.unwrap_or(0),
            }))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(ScreenSession {
            url: body
                .get("screenUrl")
                .and_then(Value::as_str)
                .map(str::to_string),
            interactive,
        })
    }

    async fn list_files(
        &self,
        computer: &ComputerRef,
        path: &str,
        context: &AdapterContext,
    ) -> Result<Vec<FileEntry>, SandboxError> {
        let response = self
            .client
            .get(self.url(&format!("/computers/{}/files", computer.id)))
            .headers(self.headers(context))
            .query(&[("path", path)])
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let Some(items) = body.as_array() else {
            return Ok(Vec::new());
        };
        Ok(items
            .iter()
            .filter_map(|item| {
                Some(FileEntry {
                    path: item.get("path")?.as_str()?.to_string(),
                    kind: item.get("kind")?.as_str()?.to_string(),
                    size: item.get("size")?.as_u64().unwrap_or(0),
                })
            })
            .collect())
    }

    async fn read_file(
        &self,
        computer: &ComputerRef,
        path: &str,
        context: &AdapterContext,
    ) -> Result<Vec<u8>, SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/read", computer.id)))
            .headers(self.headers(context))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(body
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes()
            .to_vec())
    }

    async fn write_file(
        &self,
        computer: &ComputerRef,
        path: &str,
        content: &[u8],
        context: &AdapterContext,
    ) -> Result<(), SandboxError> {
        let response = self
            .client
            .post(self.url(&format!("/computers/{}/files", computer.id)))
            .headers(self.headers(context))
            .json(&serde_json::json!({
                "path": path,
                "content": String::from_utf8_lossy(content),
            }))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(SandboxError::message(format!(
                "write failed: {}",
                response.status()
            )))
        }
    }

    async fn stop(&self, computer: &ComputerRef, context: &AdapterContext) -> Result<(), SandboxError> {
        self.client
            .post(self.url(&format!("/computers/{}/stop", computer.id)))
            .headers(self.headers(context))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(())
    }

    async fn destroy(&self, computer: &ComputerRef, context: &AdapterContext) -> Result<(), SandboxError> {
        self.client
            .delete(self.url(&format!("/computers/{}", computer.id)))
            .headers(self.headers(context))
            .send()
            .await
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(())
    }
}

fn decode_observation(body: &Value) -> Result<ComputerObservation, SandboxError> {
    let encoded = body
        .get("png_base64")
        .and_then(Value::as_str)
        .ok_or_else(|| SandboxError::message("missing png"))?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| SandboxError::message(error.to_string()))?;
    let cursor = body.get("cursor").and_then(|cursor| {
        Some(CursorPosition {
            x: cursor.get("x")?.as_i64()? as i32,
            y: cursor.get("y")?.as_i64()? as i32,
        })
    });
    let window = body
        .get("activeWindow")
        .or_else(|| body.get("active_window"))
        .and_then(|window| {
            let id = window.get("id")?.as_str()?.to_string();
            if id.is_empty() {
                return None;
            }
            Some(ActiveWindow {
                id,
                title: window
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|title| !title.is_empty())
                    .map(str::to_string),
            })
        });
    Ok(observation_from_png(png, 1280, 800, cursor, window))
}
