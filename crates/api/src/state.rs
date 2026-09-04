use std::sync::Arc;

use lazyboy_control::SandboxProvider;
use lazyboy_sandbox::{DockerSandbox, FakeSandbox};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::db::{Actor, Db};
use crate::auth::AuthConfig;
use crate::mcp::McpHub;
use crate::memory::MemoryService;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub sandbox: Arc<dyn SandboxProvider>,
    pub data_dir: String,
    pub auth: AuthConfig,
    pub memory: MemoryService,
    pub mcp: McpHub,
}

impl AppState {
    pub async fn connect(database_url: &str) -> Result<Self, String> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(|error| error.to_string())?;
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .map_err(|error| error.to_string())?;
        let sandbox = sandbox_from_env();
        Ok(Self {
            db: Db { pool },
            sandbox,
            data_dir: std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into()),
            auth: AuthConfig::from_env(),
            memory: MemoryService::from_env(),
            mcp: McpHub::new(),
        })
    }

    pub async fn bootstrap(&self) -> Result<Actor, sqlx::Error> {
        self.db.ensure_local_actor().await
    }

    pub fn pool(&self) -> &PgPool {
        &self.db.pool
    }
}

fn sandbox_from_env() -> Arc<dyn SandboxProvider> {
    match std::env::var("SANDBOX_PROVIDER").unwrap_or_else(|_| "docker".into()).as_str() {
        "fake" => Arc::new(FakeSandbox::new()),
        _ => {
            let url = std::env::var("SANDBOX_SUPERVISOR_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:7091".into());
            let token = std::env::var("SANDBOX_SUPERVISOR_TOKEN")
                .expect("SANDBOX_SUPERVISOR_TOKEN must be set");
            assert!(
                token.len() >= 32 && token != "dev-token",
                "SANDBOX_SUPERVISOR_TOKEN must be a non-default value of at least 32 characters"
            );
            Arc::new(DockerSandbox::new(url, token))
        }
    }
}
