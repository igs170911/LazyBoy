use lazyboy_contracts::{ComputerAction, ComputerCapabilities, ComputerObservation, SandboxKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterContext {
    pub operation_id: String,
    pub space_id: String,
    pub user_id: String,
    pub bot_id: Option<String>,
    pub run_id: Option<String>,
    pub screen_lease_id: Option<String>,
    #[serde(default)]
    pub screen_id: Option<String>,
    #[serde(default)]
    pub screen_slot: Option<u32>,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub profile_path: Option<String>,
}

impl Default for AdapterContext {
    fn default() -> Self {
        Self {
            operation_id: String::new(),
            space_id: String::new(),
            user_id: String::new(),
            bot_id: None,
            run_id: None,
            screen_lease_id: None,
            screen_id: None,
            screen_slot: None,
            display: None,
            profile_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerRef {
    pub id: String,
    pub home_key: String,
    pub kind: SandboxKind,
    pub provider_ref: String,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionRequest {
    pub home_key: String,
    pub home_path: String,
    pub provider_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub actions: Vec<ComputerAction>,
    pub observe: bool,
    pub settle_ms: u32,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub profile_path: Option<String>,
}

impl ActionRequest {
    pub fn new(actions: Vec<ComputerAction>, observe: bool, settle_ms: u32) -> Self {
        Self {
            actions,
            observe,
            settle_ms,
            display: None,
            profile_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsureScreenRequest {
    pub slot: u32,
    pub profile_path: String,
    pub bot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsureScreenResult {
    pub slot: u32,
    pub display: String,
    pub view_port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub completed: usize,
    pub observation: Option<ComputerObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub kind: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSession {
    pub url: Option<String>,
    pub interactive: bool,
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("{0}")]
    Message(String),
    #[error("computer is busy")]
    Busy,
    #[error("{0}")]
    MultiScreen(String),
}

impl SandboxError {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

#[async_trait::async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn provision(
        &self,
        request: ProvisionRequest,
        context: &AdapterContext,
    ) -> Result<ComputerRef, SandboxError>;

    async fn prepare(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<(), SandboxError> {
        let _ = (computer, context);
        Ok(())
    }

    async fn capabilities(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<ComputerCapabilities, SandboxError> {
        let _ = (computer, context);
        Ok(ComputerCapabilities { multi_screen: true })
    }

    async fn ensure_screen(
        &self,
        computer: &ComputerRef,
        request: EnsureScreenRequest,
        context: &AdapterContext,
    ) -> Result<EnsureScreenResult, SandboxError> {
        let _ = (computer, context);
        let layout = crate::screen_layout(request.slot)
            .map_err(|error| SandboxError::message(error.to_string()))?;
        Ok(EnsureScreenResult {
            slot: layout.slot,
            display: layout.display,
            view_port: layout.view_port,
        })
    }

    async fn reconnect(
        &self,
        request: ProvisionRequest,
        context: &AdapterContext,
    ) -> Result<ComputerRef, SandboxError> {
        self.provision(request, context).await
    }

    async fn suspend(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<(), SandboxError> {
        self.stop(computer, context).await
    }

    async fn resume(
        &self,
        request: ProvisionRequest,
        context: &AdapterContext,
    ) -> Result<ComputerRef, SandboxError> {
        self.provision(request, context).await
    }

    async fn execute(
        &self,
        computer: &ComputerRef,
        request: CommandRequest,
        context: &AdapterContext,
    ) -> Result<CommandResult, SandboxError>;

    async fn observe(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<ComputerObservation, SandboxError>;

    async fn act(
        &self,
        computer: &ComputerRef,
        request: ActionRequest,
        context: &AdapterContext,
    ) -> Result<ActionResult, SandboxError>;

    async fn connect_screen(
        &self,
        computer: &ComputerRef,
        interactive: bool,
        context: &AdapterContext,
    ) -> Result<ScreenSession, SandboxError>;

    async fn list_files(
        &self,
        computer: &ComputerRef,
        path: &str,
        context: &AdapterContext,
    ) -> Result<Vec<FileEntry>, SandboxError>;

    async fn read_file(
        &self,
        computer: &ComputerRef,
        path: &str,
        context: &AdapterContext,
    ) -> Result<Vec<u8>, SandboxError>;

    async fn write_file(
        &self,
        computer: &ComputerRef,
        path: &str,
        content: &[u8],
        context: &AdapterContext,
    ) -> Result<(), SandboxError>;

    async fn stop(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<(), SandboxError>;

    async fn destroy(
        &self,
        computer: &ComputerRef,
        context: &AdapterContext,
    ) -> Result<(), SandboxError>;
}
