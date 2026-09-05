use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::AppState;

const REGISTRY_URL: &str = "https://registry.modelcontextprotocol.io/v0/servers";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub title: String,
    pub description: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env_keys: Vec<SecretField>,
    pub header_keys: Vec<SecretField>,
    pub source: &'static str,
    pub remote: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecretField {
    pub name: String,
    pub required: bool,
    pub secret: bool,
    pub hint: String,
}

#[derive(Deserialize)]
pub struct CatalogQuery {
    #[serde(default)]
    pub q: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/mcp-catalog", axum::routing::get(list_catalog))
}

async fn list_catalog(Query(query): Query<CatalogQuery>) -> Result<Json<Value>, StatusCode> {
    let q = query.q.trim().to_ascii_lowercase();
    let mut items = featured();
    if !q.is_empty() {
        items.retain(|item| {
            item.title.to_ascii_lowercase().contains(&q)
                || item.description.to_ascii_lowercase().contains(&q)
                || item.id.to_ascii_lowercase().contains(&q)
        });
        if let Ok(remote) = fetch_registry(&query.q).await {
            for entry in remote {
                if items
                    .iter()
                    .any(|item| item.id == entry.id || item.title == entry.title)
                {
                    continue;
                }
                items.push(entry);
            }
        }
    }
    Ok(Json(json!({ "servers": items })))
}

fn featured() -> Vec<CatalogEntry> {
    vec![
        remote(
            "context7",
            "Context7",
            "即時函式庫文件與程式範例，寫程式時查最新 API。",
            "https://mcp.context7.com/mcp",
        ),
        remote(
            "deepwiki",
            "DeepWiki",
            "讀 GitHub 專案的 Wiki 風格說明。",
            "https://mcp.deepwiki.com/mcp",
        ),
        remote(
            "cloudflare",
            "Cloudflare",
            "搜尋 Cloudflare 文件（Workers、DNS、R2 等）。",
            "https://docs.mcp.cloudflare.com/mcp",
        ),
        remote_auth(
            "linear",
            "Linear",
            "查詢與建立 Linear issue、專案與留言。",
            "https://mcp.linear.app/mcp",
            &[("Authorization", true, "Linear API key")],
        ),
        remote_auth(
            "sentry",
            "Sentry",
            "讀取錯誤、issue 與專案狀態。",
            "https://mcp.sentry.dev/mcp",
            &[("Authorization", true, "Sentry auth token")],
        ),
        remote_auth(
            "github",
            "GitHub",
            "搜尋 repo、issue、PR。",
            "https://api.githubcopilot.com/mcp/",
            &[("Authorization", true, "GitHub PAT（repo 權限）")],
        ),
        stdio(
            "notion",
            "Notion",
            "搜尋與讀取 Notion 頁面。",
            "npx",
            &["-y", "@notionhq/notion-mcp-server"],
            &[("NOTION_TOKEN", true, "Notion integration token")],
        ),
        stdio(
            "fetch",
            "Fetch",
            "抓網頁內容給 Agent 讀。",
            "python3",
            &["-m", "mcp_server_fetch"],
            &[],
        ),
        stdio(
            "memory",
            "Memory",
            "給 Agent 一個可持久的知識圖譜。",
            "npx",
            &["-y", "@modelcontextprotocol/server-memory"],
            &[],
        ),
        stdio(
            "sequential-thinking",
            "Sequential Thinking",
            "把推理拆成步驟，適合複雜規劃。",
            "npx",
            &["-y", "@modelcontextprotocol/server-sequential-thinking"],
            &[],
        ),
    ]
}

fn remote(id: &str, title: &str, description: &str, url: &str) -> CatalogEntry {
    CatalogEntry {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        transport: "http".into(),
        command: None,
        args: Vec::new(),
        url: Some(url.into()),
        env_keys: Vec::new(),
        header_keys: Vec::new(),
        source: "featured",
        remote: true,
    }
}

fn remote_auth(
    id: &str,
    title: &str,
    description: &str,
    url: &str,
    headers: &[(&str, bool, &str)],
) -> CatalogEntry {
    let mut entry = remote(id, title, description, url);
    entry.header_keys = headers
        .iter()
        .map(|(name, required, hint)| SecretField {
            name: (*name).into(),
            required: *required,
            secret: true,
            hint: (*hint).into(),
        })
        .collect();
    entry
}

fn stdio(
    id: &str,
    title: &str,
    description: &str,
    command: &str,
    args: &[&str],
    env: &[(&str, bool, &str)],
) -> CatalogEntry {
    CatalogEntry {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        transport: "stdio".into(),
        command: Some(command.into()),
        args: args.iter().map(|value| (*value).to_string()).collect(),
        url: None,
        env_keys: env
            .iter()
            .map(|(name, required, hint)| SecretField {
                name: (*name).into(),
                required: *required,
                secret: true,
                hint: (*hint).into(),
            })
            .collect(),
        header_keys: Vec::new(),
        source: "featured",
        remote: false,
    }
}

async fn fetch_registry(search: &str) -> Result<Vec<CatalogEntry>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(REGISTRY_URL)
        .query(&[("search", search), ("limit", "24")])
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("registry {}", response.status()));
    }
    let body: Value = response.json().await.map_err(|error| error.to_string())?;
    let servers = body
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(servers.iter().filter_map(parse_registry_item).collect())
}

fn parse_registry_item(item: &Value) -> Option<CatalogEntry> {
    let server = item.get("server").unwrap_or(item);
    let id = server.get("name")?.as_str()?.to_string();
    let title = server
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(&id))
        .to_string();
    let description = server
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if let Some(remote) = server
        .get("remotes")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("url").and_then(Value::as_str).is_some())
        })
    {
        let url = remote.get("url")?.as_str()?.to_string();
        let transport = match remote
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("streamable-http")
        {
            "sse" => "sse",
            _ => "http",
        };
        let header_keys = remote
            .get("headers")
            .and_then(Value::as_array)
            .map(|headers| {
                headers
                    .iter()
                    .filter_map(|header| {
                        Some(SecretField {
                            name: header.get("name")?.as_str()?.to_string(),
                            required: header
                                .get("isRequired")
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                            secret: header
                                .get("isSecret")
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                            hint: header
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Some(CatalogEntry {
            id,
            title,
            description,
            transport: transport.into(),
            command: None,
            args: Vec::new(),
            url: Some(url),
            env_keys: Vec::new(),
            header_keys,
            source: "registry",
            remote: true,
        });
    }
    let package = server.get("packages").and_then(Value::as_array)?.first()?;
    let identifier = package.get("identifier")?.as_str()?;
    let registry_type = package
        .get("registryType")
        .and_then(Value::as_str)
        .unwrap_or("npm");
    let (command, args) = match registry_type {
        "npm" => ("npx".into(), vec!["-y".into(), identifier.into()]),
        "pypi" => ("uvx".into(), vec![identifier.into()]),
        _ => return None,
    };
    let extra = package
        .get("runtimeArguments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|arg| arg.get("value").and_then(Value::as_str).map(str::to_string));
    let mut args = args;
    args.extend(extra);
    let env_keys = package
        .get("environmentVariables")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(SecretField {
                        name: item.get("name")?.as_str()?.to_string(),
                        required: item
                            .get("isRequired")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        secret: item
                            .get("isSecret")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                        hint: item
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(CatalogEntry {
        id,
        title,
        description,
        transport: "stdio".into(),
        command: Some(command),
        args,
        url: None,
        env_keys,
        header_keys: Vec::new(),
        source: "registry",
        remote: false,
    })
}
