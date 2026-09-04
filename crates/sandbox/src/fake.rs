use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use lazyboy_contracts::{ComputerObservation, SandboxKind};
use lazyboy_control::{
    ActionRequest, ActionResult, AdapterContext, CommandRequest, CommandResult, ComputerRef,
    FileEntry, ProvisionRequest, SandboxError, SandboxProvider, ScreenSession,
    observation_from_png,
};

const EMPTY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[derive(Default)]
pub struct FakeSandbox {
    files: Mutex<HashMap<String, HashMap<String, Vec<u8>>>>,
}

impl FakeSandbox {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SandboxProvider for FakeSandbox {
    async fn provision(
        &self,
        request: ProvisionRequest,
        _context: &AdapterContext,
    ) -> Result<ComputerRef, SandboxError> {
        self.files
            .lock()
            .unwrap()
            .entry(request.home_key.clone())
            .or_default();
        Ok(ComputerRef {
            id: format!("fake-{}", request.home_key),
            home_key: request.home_key,
            kind: SandboxKind::Docker,
            provider_ref: format!("fake-{}", request.home_path),
            fresh: true,
        })
    }

    async fn execute(
        &self,
        computer: &ComputerRef,
        request: CommandRequest,
        _context: &AdapterContext,
    ) -> Result<CommandResult, SandboxError> {
        if request.argv.get(0).map(String::as_str) == Some("mkdir") {
            return Ok(CommandResult {
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            });
        }
        if request.argv.get(0).map(String::as_str) == Some("touch") {
            if let Some(path) = request.argv.get(1) {
                self.files
                    .lock()
                    .unwrap()
                    .entry(computer.home_key.clone())
                    .or_default()
                    .insert(path.clone(), Vec::new());
            }
        }
        Ok(CommandResult {
            stdout: request.argv.join(" "),
            stderr: String::new(),
            code: 0,
        })
    }

    async fn observe(
        &self,
        _computer: &ComputerRef,
        _context: &AdapterContext,
    ) -> Result<ComputerObservation, SandboxError> {
        Ok(observation_from_png(EMPTY_PNG.to_vec(), 1, 1, None, None))
    }

    async fn act(
        &self,
        computer: &ComputerRef,
        request: ActionRequest,
        context: &AdapterContext,
    ) -> Result<ActionResult, SandboxError> {
        Ok(ActionResult {
            completed: request.actions.len(),
            observation: if request.observe {
                Some(self.observe(computer, context).await?)
            } else {
                None
            },
        })
    }

    async fn connect_screen(
        &self,
        computer: &ComputerRef,
        interactive: bool,
        _context: &AdapterContext,
    ) -> Result<ScreenSession, SandboxError> {
        Ok(ScreenSession {
            url: Some(format!("http://127.0.0.1:6080/fake/{}", computer.id)),
            interactive,
        })
    }

    async fn list_files(
        &self,
        computer: &ComputerRef,
        path: &str,
        _context: &AdapterContext,
    ) -> Result<Vec<FileEntry>, SandboxError> {
        let files = self.files.lock().unwrap();
        let Some(home) = files.get(&computer.home_key) else {
            return Ok(Vec::new());
        };
        Ok(home
            .iter()
            .filter(|(file_path, _)| path.is_empty() || file_path.starts_with(path))
            .map(|(file_path, bytes)| FileEntry {
                path: file_path.clone(),
                kind: "file".into(),
                size: bytes.len() as u64,
            })
            .collect())
    }

    async fn read_file(
        &self,
        computer: &ComputerRef,
        path: &str,
        _context: &AdapterContext,
    ) -> Result<Vec<u8>, SandboxError> {
        self.files
            .lock()
            .unwrap()
            .get(&computer.home_key)
            .and_then(|home| home.get(path).cloned())
            .ok_or_else(|| SandboxError::message("not found"))
    }

    async fn write_file(
        &self,
        computer: &ComputerRef,
        path: &str,
        content: &[u8],
        _context: &AdapterContext,
    ) -> Result<(), SandboxError> {
        self.files
            .lock()
            .unwrap()
            .entry(computer.home_key.clone())
            .or_default()
            .insert(path.to_string(), content.to_vec());
        Ok(())
    }

    async fn stop(
        &self,
        _computer: &ComputerRef,
        _context: &AdapterContext,
    ) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn destroy(
        &self,
        computer: &ComputerRef,
        _context: &AdapterContext,
    ) -> Result<(), SandboxError> {
        self.files.lock().unwrap().remove(&computer.home_key);
        Ok(())
    }
}
