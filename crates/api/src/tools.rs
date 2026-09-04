use std::sync::Mutex;

use lazyboy_contracts::{ComputerMode, ComputerObservation};
use lazyboy_control::{
    frames_match, parse_computer_actions, resolve_bot_workspace_cwd, resolve_bot_workspace_path,
    ActionRequest, AdapterContext, CommandRequest, ComputerRef, SandboxProvider,
};
use rig_core::completion::ToolDefinition;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Actor;
use crate::mcp::McpHub;
use crate::memory::{CreateMemoryInput, MemoryService};

pub struct ToolCtx {
    pub sandbox: std::sync::Arc<dyn SandboxProvider>,
    pub computer: ComputerRef,
    pub context: AdapterContext,
    pub mode: ComputerMode,
    pub bot_id: String,
    pub vision: bool,
    pub gui_block: Option<String>,
    pub previous_frame: Mutex<Option<String>>,
    pub takeover_requested: Mutex<bool>,
    pub pool: PgPool,
    pub memory: MemoryService,
    pub actor: Actor,
    pub session_id: String,
    pub run_id: String,
    pub memory_enabled: bool,
    pub mcp: McpHub,
}

pub fn tool_definitions(memory_enabled: bool) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: "computer_observe".into(),
            description: "Capture a fresh desktop screenshot. Frame metadata comes back as text; the image is attached to the next model turn.".into(),
            parameters: json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "computer_act".into(),
            description: "Drive the desktop like a person: move to the target, click, type, drag, and scroll. Coordinates are 1280x800 from the top-left. Always include x,y for click/scroll/drag. Batch a whole gesture in one call. The resulting screenshot is attached to the next turn.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "actions":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "kind":{"type":"string","enum":["click","move","down","up","hover","drag","type","key","scroll","wait","focus"]},
                                "title":{"type":"string"},
                                "x2":{"type":"number"},
                                "y2":{"type":"number"},
                                "x":{"type":"number"},
                                "y":{"type":"number"},
                                "text":{"type":"string"},
                                "key":{"type":"string"},
                                "modifiers":{"type":"array","items":{"type":"string"}},
                                "button":{"type":"string","enum":["left","right"]},
                                "double":{"type":"boolean"},
                                "direction":{"type":"string","enum":["up","down"]},
                                "amount":{"type":"number"},
                                "ms":{"type":"number"}
                            },
                            "required":["kind"]
                        }
                    },
                    "observe":{"type":"boolean"},
                    "settle_ms":{"type":"number"}
                },
                "required":["actions"]
            }),
        },
        ToolDefinition {
            name: "shell".into(),
            description: "Run a command inside this bot's computer.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string"},
                    "cwd":{"type":"string"}
                },
                "required":["command"]
            }),
        },
        ToolDefinition {
            name: "list_files".into(),
            description: "List files in this bot's home.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "read_file".into(),
            description: "Read a UTF-8 text file from this bot's home.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        },
        ToolDefinition {
            name: "write_file".into(),
            description: "Write a UTF-8 file into this bot's home.".into(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"content":{"type":"string"}},
                "required":["path","content"]
            }),
        },
        ToolDefinition {
            name: "open_path".into(),
            description: "Open a workspace file or http(s) URL on the desktop.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        },
        ToolDefinition {
            name: "launch_app".into(),
            description: "Launch browser or terminal on the desktop.".into(),
            parameters: json!({
                "type":"object",
                "properties":{"application":{"type":"string"},"uri":{"type":"string"}},
                "required":["application"]
            }),
        },
        ToolDefinition {
            name: "request_takeover".into(),
            description: "Ask the user to take over for passwords, 2FA, CAPTCHA, or protected input. Never ask them to paste secrets in chat.".into(),
            parameters: json!({"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"]}),
        },
    ];
    if memory_enabled {
        definitions.extend([
            ToolDefinition {
                name: "remember".into(),
                description: "Explicitly save a durable user preference or fact for this agent only. Never store passwords, tokens, private keys, or other secrets.".into(),
                parameters: json!({
                    "type":"object",
                    "properties":{
                        "content":{"type":"string"},
                        "importance":{"type":"number","minimum":0,"maximum":1}
                    },
                    "required":["content"]
                }),
            },
            ToolDefinition {
                name: "recall_memory".into(),
                description: "Search durable memories belonging only to this agent.".into(),
                parameters: json!({
                    "type":"object",
                    "properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20}},
                    "required":["query"]
                }),
            },
            ToolDefinition {
                name: "forget_memory".into(),
                description: "Soft-delete one durable memory belonging to this agent by its ID.".into(),
                parameters: json!({
                    "type":"object",
                    "properties":{"memory_id":{"type":"string","format":"uuid"}},
                    "required":["memory_id"]
                }),
            },
        ]);
    }
    definitions
}

pub struct ToolOutcome {
    pub text: String,
    pub image: Option<Vec<u8>>,
    pub pause: bool,
}

pub async fn dispatch(ctx: &ToolCtx, name: &str, args: &Value) -> ToolOutcome {
    match name {
        "computer_observe" => observe(ctx).await,
        "computer_act" => act(ctx, args).await,
        "shell" => shell(ctx, args).await,
        "list_files" => list_files(ctx, args).await,
        "read_file" => read_file(ctx, args).await,
        "write_file" => write_file(ctx, args).await,
        "open_path" => open_path(ctx, args).await,
        "launch_app" => launch_app(ctx, args).await,
        "remember" => remember(ctx, args).await,
        "recall_memory" => recall_memory(ctx, args).await,
        "forget_memory" => forget_memory(ctx, args).await,
        "request_takeover" => {
            *ctx.takeover_requested.lock().unwrap() = true;
            ToolOutcome {
                text: args
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("The bot asked you to take control.")
                    .to_string(),
                image: None,
                pause: true,
            }
        }
        other if other.starts_with("mcp_") => match ctx.mcp.call(other, args).await {
            Ok(text) => text_outcome(text),
            Err(error) => text_outcome(format!("MCP 工具失敗：{error}")),
        },
        other => ToolOutcome {
            text: format!("unknown tool {other}"),
            image: None,
            pause: false,
        },
    }
}

async fn remember(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if !ctx.memory_enabled {
        return text_outcome("memory is disabled for this agent");
    }
    let input = CreateMemoryInput {
        content: args.get("content").and_then(Value::as_str).unwrap_or("").to_string(),
        importance: args.get("importance").and_then(Value::as_f64).unwrap_or(0.5) as f32,
        session_id: Some(ctx.session_id.clone()),
        source_run_id: Some(ctx.run_id.clone()),
        source_message_id: None,
    };
    match ctx.memory.remember(&ctx.pool, &ctx.actor, &ctx.bot_id, input).await {
        Ok(item) => text_outcome(json!({"ok":true,"memory":item}).to_string()),
        Err(error) => text_outcome(error),
    }
}

async fn recall_memory(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if !ctx.memory_enabled {
        return text_outcome("memory is disabled for this agent");
    }
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let limit = args.get("limit").and_then(Value::as_i64);
    match ctx.memory.recall(&ctx.pool, &ctx.actor, &ctx.bot_id, query, limit).await {
        Ok(items) => text_outcome(serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())),
        Err(error) => text_outcome(error),
    }
}

async fn forget_memory(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if !ctx.memory_enabled {
        return text_outcome("memory is disabled for this agent");
    }
    let Some(id) = args.get("memory_id").and_then(Value::as_str).and_then(|id| Uuid::parse_str(id).ok()) else {
        return text_outcome("memory_id must be a UUID");
    };
    match ctx.memory.forget(&ctx.pool, &ctx.actor, &ctx.bot_id, id).await {
        Ok(deleted) => text_outcome(json!({"ok":deleted}).to_string()),
        Err(error) => text_outcome(error),
    }
}

fn text_outcome(text: impl Into<String>) -> ToolOutcome {
    ToolOutcome { text: text.into(), image: None, pause: false }
}

fn vision_guard(ctx: &ToolCtx) -> Option<ToolOutcome> {
    if let Some(message) = &ctx.gui_block {
        return Some(ToolOutcome {
            text: message.clone(),
            image: None,
            pause: false,
        });
    }
    if ctx.vision {
        None
    } else {
        Some(ToolOutcome {
            text: "This model cannot see the screen. Use shell and file tools, or pick a vision model.".into(),
            image: None,
            pause: false,
        })
    }
}

fn observation_text(note: &str, observation: &ComputerObservation, unchanged: bool) -> String {
    format!(
        "{note}{}\n{}",
        if unchanged { " (screen unchanged)" } else { "" },
        json!({
            "frameId": observation.frame_id,
            "width": observation.width,
            "height": observation.height,
            "capturedAt": observation.captured_at,
            "cursor": observation.cursor,
            "activeWindow": observation.active_window,
        })
    )
}

async fn observe(ctx: &ToolCtx) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    match ctx.sandbox.observe(&ctx.computer, &ctx.context).await {
        Ok(observation) => pack_observation(ctx, "computer observed", observation),
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

async fn act(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let actions = match parse_computer_actions(args.get("actions").unwrap_or(&Value::Null)) {
        Ok(actions) => actions,
        Err(error) => {
            return ToolOutcome {
                text: error.to_string(),
                image: None,
                pause: false,
            };
        }
    };
    match ctx
        .sandbox
        .act(
            &ctx.computer,
            ActionRequest {
                actions,
                observe: args.get("observe").and_then(Value::as_bool) != Some(false),
                settle_ms: args.get("settle_ms").and_then(Value::as_u64).unwrap_or(120) as u32,
                display: ctx.context.display.clone(),
                profile_path: ctx.context.profile_path.clone(),
            },
            &ctx.context,
        )
        .await
    {
        Ok(result) => {
            if let Some(observation) = result.observation {
                pack_observation(
                    ctx,
                    &format!("completed {} computer action(s)", result.completed),
                    observation,
                )
            } else {
                ToolOutcome {
                    text: json!({"ok": true, "completed": result.completed}).to_string(),
                    image: None,
                    pause: false,
                }
            }
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

fn pack_observation(ctx: &ToolCtx, note: &str, observation: ComputerObservation) -> ToolOutcome {
    let unchanged = frames_match(ctx.previous_frame.lock().unwrap().as_deref(), &observation);
    *ctx.previous_frame.lock().unwrap() = Some(observation.frame_id.clone());
    ToolOutcome {
        text: observation_text(note, &observation, unchanged),
        image: if unchanged { None } else { Some(observation.image) },
        pause: false,
    }
}

async fn shell(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let command = args.get("command").and_then(Value::as_str).unwrap_or("");
    let cwd = args.get("cwd").and_then(Value::as_str);
    let cwd = resolve_bot_workspace_cwd(ctx.mode, &ctx.bot_id, cwd)
        .ok()
        .flatten();
    match ctx
        .sandbox
        .execute(
            &ctx.computer,
            CommandRequest {
                argv: vec!["bash".into(), "-lc".into(), command.into()],
                cwd,
                timeout_ms: Some(60_000),
            },
            &ctx.context,
        )
        .await
    {
        Ok(result) => ToolOutcome {
            text: format!("exit {}\n{}\n{}", result.code, result.stdout, result.stderr),
            image: None,
            pause: false,
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

async fn list_files(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let requested = args.get("path").and_then(Value::as_str).unwrap_or("");
    let stored = match resolve_bot_workspace_path(ctx.mode, &ctx.bot_id, requested) {
        Ok(path) => path,
        Err(error) => {
            return ToolOutcome {
                text: error.to_string(),
                image: None,
                pause: false,
            };
        }
    };
    match ctx.sandbox.list_files(&ctx.computer, &stored, &ctx.context).await {
        Ok(entries) => ToolOutcome {
            text: serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()),
            image: None,
            pause: false,
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

async fn read_file(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let requested = args.get("path").and_then(Value::as_str).unwrap_or("");
    let stored = match resolve_bot_workspace_path(ctx.mode, &ctx.bot_id, requested) {
        Ok(path) => path,
        Err(error) => {
            return ToolOutcome {
                text: error.to_string(),
                image: None,
                pause: false,
            };
        }
    };
    match ctx.sandbox.read_file(&ctx.computer, &stored, &ctx.context).await {
        Ok(bytes) => ToolOutcome {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            image: None,
            pause: false,
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

async fn write_file(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let requested = args.get("path").and_then(Value::as_str).unwrap_or("notes.txt");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let stored = match resolve_bot_workspace_path(ctx.mode, &ctx.bot_id, requested) {
        Ok(path) => path,
        Err(error) => {
            return ToolOutcome {
                text: error.to_string(),
                image: None,
                pause: false,
            };
        }
    };
    match ctx
        .sandbox
        .write_file(&ctx.computer, &stored, content.as_bytes(), &ctx.context)
        .await
    {
        Ok(()) => ToolOutcome {
            text: json!({"ok": true, "path": requested}).to_string(),
            image: None,
            pause: false,
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

async fn open_path(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let path = args.get("path").and_then(Value::as_str).unwrap_or("");
    match ctx
        .sandbox
        .act(
            &ctx.computer,
            ActionRequest {
                actions: vec![lazyboy_contracts::ComputerAction::Open { path: path.into() }],
                observe: true,
                settle_ms: 400,
                display: ctx.context.display.clone(),
                profile_path: ctx.context.profile_path.clone(),
            },
            &ctx.context,
        )
        .await
    {
        Ok(result) => {
            if let Some(observation) = result.observation {
                pack_observation(ctx, "opened path", observation)
            } else {
                ToolOutcome {
                    text: "opened".into(),
                    image: None,
                    pause: false,
                }
            }
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}

async fn launch_app(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let application = args.get("application").and_then(Value::as_str).unwrap_or("browser");
    let uri = args.get("uri").and_then(Value::as_str).map(str::to_string);
    match ctx
        .sandbox
        .act(
            &ctx.computer,
            ActionRequest {
                actions: vec![lazyboy_contracts::ComputerAction::Launch {
                    application: application.into(),
                    uri,
                }],
                observe: true,
                settle_ms: 500,
                display: ctx.context.display.clone(),
                profile_path: ctx.context.profile_path.clone(),
            },
            &ctx.context,
        )
        .await
    {
        Ok(result) => {
            if let Some(observation) = result.observation {
                pack_observation(ctx, "launched app", observation)
            } else {
                ToolOutcome {
                    text: "launched".into(),
                    image: None,
                    pause: false,
                }
            }
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
        },
    }
}
