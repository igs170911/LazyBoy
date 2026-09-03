mod auth;
mod computer;
mod db;
mod memory;
mod routes;
mod runs;
mod screen_proxy;
mod sessions;
mod state;
mod tools;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use state::AppState;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.env"),
    );
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();
    if std::env::var("XAI_API_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        tracing::warn!("XAI_API_KEY is not set; chat will fail until it is");
    }

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://lazyboy:lazyboy@127.0.0.1:5434/lazyboy".into());
    let state = AppState::connect(&database_url)
        .await
        .expect("database");
    state.bootstrap().await.expect("bootstrap");

    let worker_state = state.clone();
    tokio::spawn(async move {
        runs::worker_loop(worker_state).await;
    });
    let idle_state = state.clone();
    tokio::spawn(async move {
        computer::idle_loop(idle_state).await;
    });

    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "127.0.0.1:3101".into());
    let addr: SocketAddr = bind.parse().expect("API_BIND");
    if !addr.ip().is_loopback() && !state.auth.strong_enough_for_network() {
        panic!("LAZYBOY_APP_TOKEN must be set to at least 32 characters when API_BIND is not loopback");
    }

    let web_dir = std::env::var("LAZYBOY_WEB_DIR").unwrap_or_else(|_| "apps/web".into());
    let app = Router::new()
        .route("/api/health", axum::routing::get(|| async { axum::Json(serde_json::json!({"ok": true})) }))
        .merge(auth::public_router(state.clone()))
        .merge(routes::router(state))
        .fallback_service(ServeDir::new(web_dir));

    tracing::info!("api listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

fn _keep_arc() {
    let _: Option<Arc<()>> = None;
}
