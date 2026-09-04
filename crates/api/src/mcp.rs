use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use lazyboy_contracts::{McpServer, McpTool, PatchMcpServerInput, UpsertMcpServerInput};
use rig_core::completion::ToolDefinition;
use rmcp::model::{CallToolRequestParams, ClientInfo, Tool};
use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::db::Actor;
use crate::state::AppState;

type ApiError = (StatusCode, Json<Value>);
type LiveClient = RunningService<RoleClient, ClientInfo>;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct McpHub {
    inner: Arc<Mutex<HashMap<String, Live>>>,
    errors: Arc<Mutex<HashMap<String, String>>>,
}

struct Live {
    client: LiveClient,
    tools: Vec<McpTool>,
    defs: Vec<ToolDefinition>,
}

impl Default for McpHub {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            errors: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl McpHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self, rows: Vec<McpRow>) -> Vec<McpServer> {
        let live = self.inner.lock().await;
        let errors = self.errors.lock().await;
        rows.into_iter()
            .map(|row| {
                let (status, error, tools) = if !row.enabled {
                    ("disabled".into(), None, Vec::new())
                } else if let Some(entry) = live.get(&row.id) {
                    ("connected".into(), None, entry.tools.clone())
                } else {
                    (
                        "disconnected".into(),
                        errors.get(&row.id).cloned().or(row.last_error.clone()),
                        Vec::new(),
                    )
                };
                row.into_server(status, error, tools)
            })
            .collect()
    }

    pub async fn definitions(&self) -> Vec<ToolDefinition> {
        let live = self.inner.lock().await;
        live.values().flat_map(|entry| entry.defs.clone()).collect()
    }

    pub async fn call(&self, exposed: &str, args: &Value) -> Result<String, String> {
        let live = self.inner.lock().await;
        for entry in live.values() {
            for tool in &entry.tools {
                if tool.exposed_name != exposed {
                    continue;
                }
                let arguments = match args {
                    Value::Object(map) => map.clone(),
                    Value::Null => Map::new(),
                    other => {
                        let mut map = Map::new();
                        map.insert("value".into(), other.clone());
                        map
                    }
                };
                let params =
                    CallToolRequestParams::new(tool.name.clone()).with_arguments(arguments);
                let result = entry
                    .client
                    .call_tool(params)
                    .await
                    .map_err(|error| error.to_string())?;
                return serde_json::to_string_pretty(&result).map_err(|error| error.to_string());
            }
        }
        Err(format!("unknown MCP tool {exposed}"))
    }

    pub async fn disconnect(&self, id: &str) {
        self.inner.lock().await.remove(id);
    }

    pub async fn forget(&self, id: &str) {
        self.inner.lock().await.remove(id);
        self.errors.lock().await.remove(id);
    }

    pub async fn connect_row(&self, row: &McpRow) -> Result<Vec<McpTool>, String> {
        self.disconnect(&row.id).await;
        if !row.enabled {
            self.errors.lock().await.remove(&row.id);
            return Ok(Vec::new());
        }
        let client = match connect_client(row).await {
            Ok(client) => client,
            Err(error) => {
                self.errors
                    .lock()
                    .await
                    .insert(row.id.clone(), error.clone());
                return Err(error);
            }
        };
        let raw = match client.list_all_tools().await {
            Ok(raw) => raw,
            Err(error) => {
                let message = error.to_string();
                self.errors
                    .lock()
                    .await
                    .insert(row.id.clone(), message.clone());
                return Err(message);
            }
        };
        let slug = slugify(&row.name);
        let tools: Vec<McpTool> = raw
            .iter()
            .map(|tool| McpTool {
                name: tool.name.to_string(),
                exposed_name: exposed_name_for(&slug, tool.name.as_ref()),
                description: tool
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(400)
                    .collect(),
            })
            .collect();
        let defs = raw
            .iter()
            .zip(tools.iter())
            .map(|(tool, meta)| {
                let description = if meta.description.is_empty() {
                    format!("MCP tool from {}", row.name)
                } else {
                    format!("{} (MCP · {})", meta.description, row.name)
                };
                ToolDefinition {
                    name: meta.exposed_name.clone(),
                    description,
                    parameters: schema_value(tool),
                }
            })
            .collect();
        self.inner.lock().await.insert(
            row.id.clone(),
            Live {
                client,
                tools: tools.clone(),
                defs,
            },
        );
        self.errors.lock().await.remove(&row.id);
        Ok(tools)
    }

    pub async fn reconnect_all(&self, pool: &sqlx::PgPool, actor: &Actor) {
        let rows = match load_rows(pool, actor).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!("mcp load failed: {error}");
                return;
            }
        };
        for row in rows.into_iter().filter(|row| row.enabled) {
            if let Err(error) = self.connect_row(&row).await {
                tracing::warn!("mcp {} ({}) failed: {error}", row.name, row.id);
            }
        }
    }
}

fn schema_value(tool: &Tool) -> Value {
    let schema = Value::Object((*tool.input_schema).clone());
    if schema.get("type").is_some() {
        schema
    } else {
        json!({"type":"object","properties": schema.get("properties").cloned().unwrap_or(json!({})), "additionalProperties": true})
    }
}

fn slugify(name: &str) -> String {
    let mut slug: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }
    let slug = slug.trim_matches('_').chars().take(24).collect::<String>();
    if slug.is_empty() { "mcp".into() } else { slug }
}

fn exposed_name_for(slug: &str, tool: &str) -> String {
    let tool: String = tool
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("mcp_{slug}_{tool}")
}

async fn connect_client(row: &McpRow) -> Result<LiveClient, String> {
    match row.transport.as_str() {
        "stdio" => {
            let command = row
                .command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "stdio 需要 command".to_string())?;
            let mut cmd = Command::new(command);
            cmd.args(&row.args);
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            for (key, value) in &row.env {
                if let Some(text) = value.as_str() {
                    cmd.env(key, text);
                }
            }
            let transport = TokioChildProcess::new(cmd)
                .map_err(|error| humanize_mcp_error(Some(command), &error.to_string()))?;
            ClientInfo::default()
                .serve(transport)
                .await
                .map_err(|error| humanize_mcp_error(Some(command), &error.to_string()))
        }
        "http" | "sse" => {
            let url = row
                .url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "HTTP MCP 需要 url".to_string())?;
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_string());
            let mut custom = HashMap::new();
            for (key, value) in &row.headers {
                let Some(text) = value.as_str() else { continue };
                if key.eq_ignore_ascii_case("authorization") {
                    let token = text
                        .strip_prefix("Bearer ")
                        .or_else(|| text.strip_prefix("bearer "))
                        .unwrap_or(text);
                    config = config.auth_header(token.to_string());
                    continue;
                }
                if let (Ok(name), Ok(header)) = (
                    http::HeaderName::from_bytes(key.as_bytes()),
                    http::HeaderValue::from_str(text),
                ) {
                    custom.insert(name, header);
                }
            }
            if !custom.is_empty() {
                config = config.custom_headers(custom);
            }
            let transport = StreamableHttpClientTransport::from_config(config);
            ClientInfo::default()
                .serve(transport)
                .await
                .map_err(|error| humanize_mcp_error(None, &error.to_string()))
        }
        other => Err(format!("不支援的 transport：{other}")),
    }
}

fn humanize_mcp_error(command: Option<&str>, error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("auth required") || lower.contains("unauthorized") || lower.contains("401") {
        return "這個 MCP 需要有效金鑰才能連線。請填 token 後再試。".into();
    }
    if lower.contains("no such file or directory") {
        return match command {
            Some(cmd) => format!("找不到指令 `{cmd}`。API 容器沒有這個執行檔。"),
            None => "找不到 MCP 指令。".into(),
        };
    }
    if lower.contains("timed out") || lower.contains("timeout") {
        return "連線逾時。遠端服務沒回應，或第一次下載套件太久。".into();
    }
    error.to_string()
}

#[derive(Clone)]
pub struct McpRow {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Map<String, Value>,
    pub url: Option<String>,
    pub headers: Map<String, Value>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
}

impl McpRow {
    fn into_server(self, status: String, error: Option<String>, tools: Vec<McpTool>) -> McpServer {
        McpServer {
            id: self.id,
            name: self.name,
            transport: self.transport,
            command: self.command,
            args: self.args,
            env: self.env,
            url: self.url,
            headers: self.headers,
            enabled: self.enabled,
            status,
            error,
            tools,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

type RowTuple = (
    String,
    String,
    String,
    Option<String>,
    Value,
    Value,
    Option<String>,
    Value,
    bool,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn row_from(tuple: RowTuple) -> McpRow {
    McpRow {
        id: tuple.0,
        name: tuple.1,
        transport: tuple.2,
        command: tuple.3,
        args: value_to_strings(&tuple.4),
        env: value_to_map(&tuple.5),
        url: tuple.6,
        headers: value_to_map(&tuple.7),
        enabled: tuple.8,
        created_at: tuple.9,
        updated_at: tuple.10,
        last_error: None,
    }
}

fn value_to_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn value_to_map(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

async fn load_rows(pool: &sqlx::PgPool, actor: &Actor) -> Result<Vec<McpRow>, sqlx::Error> {
    let rows: Vec<RowTuple> = sqlx::query_as(
        "SELECT id, name, transport, command, args, env, url, headers, enabled, created_at, updated_at
         FROM mcp_servers WHERE space_id=$1 AND user_id=$2 ORDER BY created_at, name",
    )
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_from).collect())
}

async fn load_row(
    pool: &sqlx::PgPool,
    actor: &Actor,
    id: &str,
) -> Result<Option<McpRow>, sqlx::Error> {
    let row: Option<RowTuple> = sqlx::query_as(
        "SELECT id, name, transport, command, args, env, url, headers, enabled, created_at, updated_at
         FROM mcp_servers WHERE id=$1 AND space_id=$2 AND user_id=$3",
    )
    .bind(id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_from))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/mcp-servers", get(list_servers).post(create_server))
        .route(
            "/api/mcp-servers/{id}",
            get(get_server).patch(update_server).delete(delete_server),
        )
        .route(
            "/api/mcp-servers/{id}/reconnect",
            axum::routing::post(reconnect_server),
        )
        .merge(crate::mcp_catalog::router())
}

fn internal(message: String) -> ApiError {
    tracing::error!("mcp: {message}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"message":"internal error"})),
    )
}

fn bad(message: &str) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(json!({"message": message})))
}

async fn rollback_server(state: &AppState, id: &str) {
    state.mcp.forget(id).await;
    let _ = sqlx::query("DELETE FROM mcp_servers WHERE id=$1")
        .bind(id)
        .execute(state.pool())
        .await;
}

async fn actor(state: &AppState) -> Result<Actor, ApiError> {
    state
        .bootstrap()
        .await
        .map_err(|error| internal(error.to_string()))
}

async fn present(state: &AppState, _actor: &Actor, mut row: McpRow) -> McpServer {
    let live = state.mcp.inner.lock().await;
    if !row.enabled {
        return row.into_server("disabled".into(), None, Vec::new());
    }
    if let Some(entry) = live.get(&row.id) {
        return row.into_server("connected".into(), None, entry.tools.clone());
    }
    let error = row.last_error.take();
    row.into_server("disconnected".into(), error, Vec::new())
}

async fn list_servers(State(state): State<AppState>) -> Result<Json<Vec<McpServer>>, ApiError> {
    let actor = actor(&state).await?;
    let rows = load_rows(state.pool(), &actor)
        .await
        .map_err(|error| internal(error.to_string()))?;
    Ok(Json(state.mcp.snapshot(rows).await))
}

fn validate_input(input: &UpsertMcpServerInput) -> Result<(), ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(bad("需要名稱"));
    }
    match input.transport.as_str() {
        "stdio" => {
            if input.command.as_deref().unwrap_or("").trim().is_empty() {
                return Err(bad("stdio 需要 command，例如 npx 或 uvx"));
            }
        }
        "http" | "sse" => {
            if input.url.as_deref().unwrap_or("").trim().is_empty() {
                return Err(bad("HTTP / SSE 需要 url"));
            }
        }
        _ => return Err(bad("transport 只能是 stdio、http 或 sse")),
    }
    Ok(())
}

async fn create_server(
    State(state): State<AppState>,
    Json(input): Json<UpsertMcpServerInput>,
) -> Result<(StatusCode, Json<McpServer>), ApiError> {
    validate_input(&input)?;
    let actor = actor(&state).await?;
    let id = Uuid::new_v4().to_string();
    let name = input.name.trim().chars().take(80).collect::<String>();
    sqlx::query(
        "INSERT INTO mcp_servers (id,space_id,user_id,name,transport,command,args,env,url,headers,enabled)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .bind(&name)
    .bind(&input.transport)
    .bind(input.command.as_deref().map(str::trim).filter(|value| !value.is_empty()))
    .bind(json!(input.args))
    .bind(Value::Object(input.env.clone()))
    .bind(input.url.as_deref().map(str::trim).filter(|value| !value.is_empty()))
    .bind(Value::Object(input.headers.clone()))
    .bind(input.enabled)
    .execute(state.pool())
    .await
    .map_err(|error| {
        if error.to_string().contains("mcp_servers_space_id_user_id_name_key")
            || error.to_string().contains("duplicate key")
        {
            bad("已有同名 MCP")
        } else {
            internal(error.to_string())
        }
    })?;
    let mut row = load_row(state.pool(), &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or_else(|| internal("missing row".into()))?;
    let mut status = if row.enabled {
        "disconnected"
    } else {
        "disabled"
    }
    .to_string();
    let mut tools = Vec::new();
    if row.enabled {
        match tokio::time::timeout(CONNECT_TIMEOUT, state.mcp.connect_row(&row)).await {
            Ok(Ok(connected)) => {
                status = "connected".into();
                tools = connected;
            }
            Ok(Err(message)) => {
                rollback_server(&state, &id).await;
                return Err((StatusCode::BAD_GATEWAY, Json(json!({"message": message}))));
            }
            Err(_) => {
                rollback_server(&state, &id).await;
                return Err(bad("連線逾時（60 秒）"));
            }
        }
    }
    Ok((
        StatusCode::CREATED,
        Json(row.into_server(status, None, tools)),
    ))
}

async fn get_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<McpServer>, ApiError> {
    let actor = actor(&state).await?;
    let row = load_row(state.pool(), &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))))?;
    Ok(Json(present(&state, &actor, row).await))
}

async fn update_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<PatchMcpServerInput>,
) -> Result<Json<McpServer>, ApiError> {
    let actor = actor(&state).await?;
    let current = load_row(state.pool(), &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))))?;
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&current.name)
        .chars()
        .take(40)
        .collect::<String>();
    let transport = input.transport.unwrap_or(current.transport.clone());
    let command = input
        .command
        .or(current.command.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let args = input.args.unwrap_or(current.args);
    let env = input.env.unwrap_or(current.env);
    let url = input
        .url
        .or(current.url)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let headers = input.headers.unwrap_or(current.headers);
    let enabled = input.enabled.unwrap_or(current.enabled);
    validate_input(&UpsertMcpServerInput {
        name: name.clone(),
        transport: transport.clone(),
        command: command.clone(),
        args: args.clone(),
        env: env.clone(),
        url: url.clone(),
        headers: headers.clone(),
        enabled,
    })?;
    if !matches!(transport.as_str(), "stdio" | "http" | "sse") {
        return Err(bad("transport 只能是 stdio、http 或 sse"));
    }
    sqlx::query(
        "UPDATE mcp_servers
         SET name=$2, transport=$3, command=$4, args=$5, env=$6, url=$7, headers=$8, enabled=$9, updated_at=now()
         WHERE id=$1 AND space_id=$10 AND user_id=$11",
    )
    .bind(&id)
    .bind(&name)
    .bind(&transport)
    .bind(&command)
    .bind(json!(args))
    .bind(Value::Object(env))
    .bind(&url)
    .bind(Value::Object(headers))
    .bind(enabled)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .execute(state.pool())
    .await
    .map_err(|error| internal(error.to_string()))?;
    let mut row = load_row(state.pool(), &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))))?;
    if row.enabled {
        state.mcp.disconnect(&id).await;
    } else {
        state.mcp.forget(&id).await;
    }
    let mut status = if row.enabled {
        "disconnected"
    } else {
        "disabled"
    }
    .to_string();
    let mut error = None;
    let mut tools = Vec::new();
    if row.enabled {
        match tokio::time::timeout(CONNECT_TIMEOUT, state.mcp.connect_row(&row)).await {
            Ok(Ok(connected)) => {
                status = "connected".into();
                tools = connected;
            }
            Ok(Err(message)) => error = Some(message),
            Err(_) => error = Some("連線逾時（60 秒）".into()),
        }
    }
    row.last_error = error.clone();
    Ok(Json(row.into_server(status, error, tools)))
}

async fn delete_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state).await?;
    state.mcp.forget(&id).await;
    let deleted = sqlx::query("DELETE FROM mcp_servers WHERE id=$1 AND space_id=$2 AND user_id=$3")
        .bind(&id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .execute(state.pool())
        .await
        .map_err(|error| internal(error.to_string()))?;
    if deleted.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn reconnect_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<McpServer>, ApiError> {
    let actor = actor(&state).await?;
    let mut row = load_row(state.pool(), &actor, &id)
        .await
        .map_err(|error| internal(error.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))))?;
    if !row.enabled {
        state.mcp.disconnect(&id).await;
        return Ok(Json(row.into_server("disabled".into(), None, Vec::new())));
    }
    match tokio::time::timeout(CONNECT_TIMEOUT, state.mcp.connect_row(&row)).await {
        Ok(Ok(tools)) => Ok(Json(row.into_server("connected".into(), None, tools))),
        Ok(Err(message)) => {
            state.mcp.disconnect(&id).await;
            row.last_error = Some(message.clone());
            Ok(Json(row.into_server(
                "disconnected".into(),
                Some(message),
                Vec::new(),
            )))
        }
        Err(_) => {
            state.mcp.disconnect(&id).await;
            Ok(Json(row.into_server(
                "disconnected".into(),
                Some("連線逾時（60 秒）".into()),
                Vec::new(),
            )))
        }
    }
}
