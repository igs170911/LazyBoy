use std::sync::Mutex;

use lazyboy_contracts::{
    ComputerAction, ComputerMode, ComputerObservation, PointerType, UiElement,
};
use lazyboy_control::{
    ActionError, ActionRequest, AdapterContext, CdpPage, CommandRequest, ComputerRef,
    SandboxProvider, a11y_command_on, apply_element_targets, browser_gui_block, cdp_command_on,
    click_fingerprint, element_id, format_ui_elements, frames_match, merge_ui_elements,
    cdp_stdin_command_on, overlay_elements, parse_a11y_page, parse_cdp_page, parse_computer_actions,
    resolve_bot_workspace_cwd, resolve_bot_workspace_path, should_block_stale_click,
};
use rig_core::completion::ToolDefinition;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Actor;
use crate::mcp::McpHub;
use crate::memory::{CreateMemoryInput, MemoryService};

pub struct ToolCtx {
    pub sandbox: std::sync::Arc<dyn SandboxProvider>,
    pub computer: Mutex<Option<ComputerRef>>,
    pub context: Mutex<AdapterContext>,
    pub mode: ComputerMode,
    pub bot_id: String,
    pub vision: bool,
    pub gui_block: Mutex<Option<String>>,
    pub previous_frame: Mutex<Option<String>>,
    pub elements: Mutex<Vec<UiElement>>,
    pub miss_streak: Mutex<u32>,
    pub last_click_key: Mutex<Option<String>>,
    pub click_misses: Mutex<u32>,
    pub takeover_requested: Mutex<bool>,
    pub connection_check_attempted: Mutex<bool>,
    pub pool: PgPool,
    pub memory: MemoryService,
    pub actor: Actor,
    pub session_id: String,
    pub run_id: String,
    pub memory_enabled: bool,
    pub mcp: McpHub,
}

impl ToolCtx {
    pub fn computer_ref(&self) -> ComputerRef {
        self.computer
            .lock()
            .unwrap()
            .clone()
            .expect("computer sandbox is not ready")
    }

    pub fn adapter(&self) -> AdapterContext {
        self.context.lock().unwrap().clone()
    }
}

pub fn tool_definitions(memory_enabled: bool) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: "computer_observe".into(),
            description: "Capture a fresh desktop screenshot plus numbered targets (page DOM when Chromium is open, otherwise AT-SPI buttons/fields, otherwise windows). Only when the user asked you to do something on the computer. Not for greetings, chat, or listing files — those need no screenshot. The image attaches only if the screen changed.".into(),
            parameters: json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "computer_act".into(),
            description: "Drive native desktop GUI (dialogs, file manager, XFCE) when the user asked you to operate the computer. Prefer element id from the latest observation (AT-SPI, not pixels). For Chromium pages use the browser tool — computer_act clicks on the browser window are rejected. x,y are 1280x800 fallback for canvas / no-tree widgets.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "actions":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "kind":{"type":"string","enum":["click","move","down","up","hover","drag","type","key","scroll","wait","focus"]},
                                "element":{"type":"number"},
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
            description: "Run a command inside this bot's computer when the user asked you to do work there. Not for greetings, small talk, or questions you can answer in text.".into(),
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
            description: "List files in this bot's home when the user asked about workspace files. Not for greetings or chat — do not ls the desktop to start a conversation.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "read_file".into(),
            description: "Read a UTF-8 text file from this bot's home when the user asked about that file. Not for greetings or chat.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        },
        ToolDefinition {
            name: "write_file".into(),
            description: "Write a UTF-8 file into this bot's home when the user asked you to save something there. Not for greetings or chat.".into(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"content":{"type":"string"}},
                "required":["path","content"]
            }),
        },
        ToolDefinition {
            name: "open_path".into(),
            description: "Open a workspace file or http(s) URL on the desktop when the user asked you to open it. Not for greetings or chat.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        },
        ToolDefinition {
            name: "browser".into(),
            description: "Control Chromium through the page DOM when the user asked you to use the browser. Prefer this over computer_act for anything in the page. snapshot returns numbered elements and visible text (no screenshot). click/type/navigate by element id or CSS selector. click scrolls off-screen elements into view and, if the control is [disabled], waits up to 45s (waitMs to change) for it to enable before clicking. Ids are renumbered after every page change. The human still sees the live window.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["snapshot","click","type","press","navigate","wait"]},
                    "element":{"type":"number"},
                    "selector":{"type":"string"},
                    "text":{"type":"string"},
                    "key":{"type":"string"},
                    "url":{"type":"string"},
                    "ms":{"type":"number"},
                    "waitMs":{"type":"number","description":"click only: how long to wait for a disabled control to enable (default 45000, max 120000)"}
                },
                "required":["action"]
            }),
        },
        ToolDefinition {
            name: "launch_app".into(),
            description: "Launch browser or terminal on the desktop when the user asked you to open an app.".into(),
            parameters: json!({
                "type":"object",
                "properties":{"application":{"type":"string"},"uri":{"type":"string"}},
                "required":["application"]
            }),
        },
        ToolDefinition {
            name: "wait".into(),
            description: "Wait for the screen to change on its own (video playing, page loading, a button that enables later), then return a fresh observation. seconds: 1-60.".into(),
            parameters: json!({
                "type":"object",
                "properties":{"seconds":{"type":"number","minimum":1,"maximum":60},"reason":{"type":"string"}},
                "required":["seconds"]
            }),
        },
        ToolDefinition {
            name: "connection_check".into(),
            description: "Try one normal click on a visible Cloudflare connection-check checkbox. First use computer_observe and locate the checkbox in the latest screenshot; pass its screen x/y. Only for a connection-check page, never image/audio puzzles, passwords or 2FA. Waits up to 15 seconds, returns a fresh observation, and requests human takeover if still blocked. Limited to one attempt per run. Never claim success until the requested page content is visible.".into(),
            parameters: json!({"type":"object","properties":{
                "x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0}
            },"required":["x","y"]}),
        },
        ToolDefinition {
            name: "request_takeover".into(),
            description: "Ask the user to take over the live screen for passwords, 2FA, CAPTCHA, or a login wall when no saved account fits. Never ask them to paste secrets in chat. Prefer use_saved_login when list_accounts has a matching site.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "reason":{"type":"string"},
                    "site":{"type":"string","description":"Site or app name, e.g. Gmail"},
                    "why":{"type":"string","description":"What you will do once they have signed in"}
                },
                "required":["reason"]
            }),
        },
        ToolDefinition {
            name: "list_accounts".into(),
            description: "List saved logins for this bot (site + username only, never passwords). Use before filling a login wall. If the site is missing, tell the human to add it under 帳號 — do not ask them to paste a password.".into(),
            parameters: json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "use_saved_login".into(),
            description: "Fill the current Chromium login form with a saved account. Pass only accountId from list_accounts. The password never appears in chat. For a simple Cloudflare checkbox use computer_observe then connection_check once; for other CAPTCHA or 2FA call request_takeover.".into(),
            parameters: json!({
                "type":"object",
                "properties":{"accountId":{"type":"string"}},
                "required":["accountId"]
            }),
        },
        ToolDefinition {
            name: "create_schedule".into(),
            description: "Schedule recurring work for this bot. Use when the user says every day / weekdays / every Monday / from now on at 9. cron is five fields in the bot timezone (default Asia/Taipei), e.g. \"0 9 * * 1-5\".".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "name":{"type":"string"},
                    "cron":{"type":"string"},
                    "instructions":{"type":"string","description":"What to do each time it fires, written to yourself."},
                    "timezone":{"type":"string"}
                },
                "required":["name","cron","instructions"]
            }),
        },
        ToolDefinition {
            name: "list_schedules".into(),
            description: "List this bot's scheduled jobs.".into(),
            parameters: json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "cancel_schedule".into(),
            description: "Disable or delete a scheduled job by id from list_schedules.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "scheduleId":{"type":"string"},
                    "delete":{"type":"boolean"}
                },
                "required":["scheduleId"]
            }),
        },
        ToolDefinition {
            name: "use_skill".into(),
            description: "Load the playbook of a skill the human taught this bot by demonstration (see 'Taught skills' in your instructions). Returns intent, inputs and semantic steps to follow with the normal tools.".into(),
            parameters: json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
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
    pub blocks: Vec<Value>,
}

pub async fn dispatch(ctx: &ToolCtx, name: &str, args: &Value) -> ToolOutcome {
    match name {
        "computer_observe" => observe(ctx).await,
        "computer_act" => act(ctx, args).await,
        "wait" => wait_then_observe(ctx, args).await,
        "browser" => browser(ctx, args).await,
        "connection_check" => connection_check(ctx, args).await,
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
                blocks: login_blocks(args),
            }
        }
        "list_accounts" => list_saved_accounts(ctx).await,
        "use_saved_login" => use_saved_login(ctx, args).await,
        "create_schedule" => create_schedule_tool(ctx, args).await,
        "list_schedules" => list_schedules_tool(ctx).await,
        "cancel_schedule" => cancel_schedule_tool(ctx, args).await,
        "use_skill" => {
            let name = args.get("name").and_then(Value::as_str).unwrap_or("");
            let skills = crate::skills::saved_skills(&ctx.pool, &ctx.bot_id).await;
            match crate::skills::find_skill(&skills, name) {
                Some(skill) => text_outcome(crate::skills::format_playbook_for_run(skill)),
                None => text_outcome(format!(
                    "no taught skill named {name:?}. Available: {}",
                    if skills.is_empty() {
                        "none".to_string()
                    } else {
                        skills
                            .iter()
                            .map(|skill| skill.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                )),
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
            blocks: Vec::new(),
        },
    }
}

async fn remember(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if !ctx.memory_enabled {
        return text_outcome("memory is disabled for this agent");
    }
    let input = CreateMemoryInput {
        content: args
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        importance: args
            .get("importance")
            .and_then(Value::as_f64)
            .unwrap_or(0.5) as f32,
        session_id: Some(ctx.session_id.clone()),
        source_run_id: Some(ctx.run_id.clone()),
        source_message_id: None,
    };
    match ctx
        .memory
        .remember(&ctx.pool, &ctx.actor, &ctx.bot_id, input)
        .await
    {
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
    match ctx
        .memory
        .recall(&ctx.pool, &ctx.actor, &ctx.bot_id, query, limit)
        .await
    {
        Ok(items) => text_outcome(serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())),
        Err(error) => text_outcome(error),
    }
}

async fn forget_memory(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if !ctx.memory_enabled {
        return text_outcome("memory is disabled for this agent");
    }
    let Some(id) = args
        .get("memory_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        return text_outcome("memory_id must be a UUID");
    };
    match ctx
        .memory
        .forget(&ctx.pool, &ctx.actor, &ctx.bot_id, id)
        .await
    {
        Ok(deleted) => text_outcome(json!({"ok":deleted}).to_string()),
        Err(error) => text_outcome(error),
    }
}

fn text_outcome(text: impl Into<String>) -> ToolOutcome {
    ToolOutcome {
        text: text.into(),
        image: None,
        pause: false,
        blocks: Vec::new(),
    }
}

fn vision_guard(ctx: &ToolCtx) -> Option<ToolOutcome> {
    if let Some(message) = ctx.gui_block.lock().unwrap().clone() {
        return Some(ToolOutcome {
            text: message,
            image: None,
            pause: false,
            blocks: Vec::new(),
        });
    }
    if ctx.vision {
        None
    } else {
        Some(ToolOutcome {
            text: "This model cannot see the screen. Use shell and file tools, or pick a vision model.".into(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        })
    }
}

fn observation_text(note: &str, observation: &ComputerObservation, unchanged: bool) -> String {
    let label = if observation.elements.iter().any(|element| {
        matches!(element.kind.as_deref(), Some("dom") | Some("a11y"))
    }) {
        "Clickable controls"
    } else {
        "Clickable windows"
    };
    format!(
        "{note}{}\n{label}: {}\n{}",
        if unchanged { " (screen unchanged)" } else { "" },
        format_ui_elements(&observation.elements),
        json!({
            "frameId": observation.frame_id,
            "width": observation.width,
            "height": observation.height,
            "capturedAt": observation.captured_at,
            "cursor": observation.cursor,
            "activeWindow": observation.active_window,
            "elements": observation.elements,
        })
    )
}

async fn observe(ctx: &ToolCtx) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    match ctx.sandbox.observe(&ctx.computer_ref(), &ctx.adapter()).await {
        Ok(observation) => {
            let (observation, note) =
                attach_ui_elements(ctx, observation, "computer observed").await;
            pack_observation(ctx, &note, observation)
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
    }
}

/// Bounded so it always fits inside the 90s per-tool budget with an observe.
const MAX_WAIT_SECS: f64 = 60.0;

async fn wait_then_observe(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let seconds = args
        .get("seconds")
        .and_then(Value::as_f64)
        .unwrap_or(5.0)
        .clamp(1.0, MAX_WAIT_SECS);
    tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
    if vision_guard(ctx).is_some() {
        return text_outcome(format!("waited {seconds:.0}s"));
    }
    match ctx.sandbox.observe(&ctx.computer_ref(), &ctx.adapter()).await {
        Ok(observation) => {
            let note = format!("waited {seconds:.0}s");
            let (observation, note) = attach_ui_elements(ctx, observation, &note).await;
            pack_observation(ctx, &note, observation)
        }
        Err(error) => text_outcome(format!("waited {seconds:.0}s; observe failed: {error}")),
    }
}

async fn attach_ui_elements(
    ctx: &ToolCtx,
    mut observation: ComputerObservation,
    note: &str,
) -> (ComputerObservation, String) {
    let page = cdp_snapshot(ctx, false).await;
    let include_browser = page
        .as_ref()
        .map(|page| !page.ok || page.elements.is_empty())
        .unwrap_or(true);
    let a11y = a11y_snapshot(ctx, include_browser).await;
    let page_elements = page
        .as_ref()
        .map(|page| page.elements.as_slice())
        .unwrap_or(&[]);
    let a11y_elements = a11y
        .as_ref()
        .filter(|page| page.ok)
        .map(|page| page.elements.as_slice())
        .unwrap_or(&[]);
    observation.elements = merge_ui_elements(observation.elements, page_elements, a11y_elements);
    let mut note = note.to_string();
    if let Some(page) = page.as_ref().filter(|page| page.ok) {
        if !page.url.is_empty() || !page.title.is_empty() {
            note.push_str(&format!("\nPage: {} {}", page.title, page.url));
        }
        if !page.text.is_empty() {
            note.push_str("\nVisible text:\n");
            note.push_str(&page.text);
        }
    }
    (observation, note)
}

async fn a11y_snapshot(ctx: &ToolCtx, include_browser: bool) -> Option<lazyboy_control::A11yPage> {
    let page = a11y_call(
        ctx,
        json!({"action": "snapshot", "includeBrowser": include_browser}),
    )
    .await;
    if page.ok {
        Some(page)
    } else {
        None
    }
}

async fn a11y_call(ctx: &ToolCtx, request: serde_json::Value) -> lazyboy_control::A11yPage {
    let adapter = ctx.adapter();
    let display = adapter.display.as_deref().unwrap_or(":1");
    let argv = a11y_command_on(display, &request);
    match ctx
        .sandbox
        .execute(
            &ctx.computer_ref(),
            CommandRequest {
                argv,
                cwd: None,
                timeout_ms: Some(8_000),
                stdin: None,
            },
            &ctx.adapter(),
        )
        .await
    {
        Ok(result) => {
            let raw = if result.stdout.trim().is_empty() {
                result.stderr
            } else {
                result.stdout
            };
            parse_a11y_page(&raw)
        }
        Err(error) => lazyboy_control::A11yPage {
            ok: false,
            error: Some(error.to_string()),
            ..lazyboy_control::A11yPage::default()
        },
    }
}

async fn cdp_snapshot(ctx: &ToolCtx, ensure: bool) -> Option<CdpPage> {
    let page = cdp_call(ctx, json!({"action": "snapshot", "ensure": ensure})).await;
    if page.ok { Some(page) } else { None }
}

async fn cdp_call(ctx: &ToolCtx, request: Value) -> CdpPage {
    let adapter = ctx.adapter();
    let display = adapter.display.as_deref().unwrap_or(":1");
    // Clicks may sit through a page's stay timer (CLICK_WAIT_MS in cdp.py).
    let timeout_ms = if request.get("action").and_then(Value::as_str) == Some("click") {
        let wait = request
            .get("waitMs")
            .and_then(Value::as_u64)
            .unwrap_or(45_000)
            .min(120_000);
        wait + 25_000
    } else {
        20_000
    };
    let argv = cdp_command_on(display, adapter.profile_path.as_deref(), &request);
    match ctx
        .sandbox
        .execute(
            &ctx.computer_ref(),
            CommandRequest {
                argv,
                cwd: None,
                timeout_ms: Some(timeout_ms),
                stdin: None,
            },
            &ctx.adapter(),
        )
        .await
    {
        Ok(result) => {
            let raw = if result.stdout.trim().is_empty() {
                result.stderr
            } else {
                result.stdout
            };
            parse_cdp_page(&raw)
        }
        Err(error) => CdpPage {
            ok: false,
            error: Some(error.to_string()),
            ..CdpPage::default()
        },
    }
}

fn is_connection_check(page: &CdpPage) -> bool {
    let text = format!("{} {}", page.title, page.text).to_lowercase();
    page.ok && (text.contains("需要確認您的連線是安全") || (text.contains("cloudflare") && [
        "verify you are human", "verifying you are human", "checking your browser",
        "checking if the site connection is secure", "needs to review the security",
        "驗證您是人類", "验证您是人类", "確認您的連線是安全", "確認您的人類身分",
    ].iter().any(|marker| text.contains(marker))))
}

fn connection_takeover(ctx: &ToolCtx, reason: &str) -> ToolOutcome {
    *ctx.takeover_requested.lock().unwrap() = true;
    ToolOutcome {
        text: reason.into(), image: None, pause: true,
        blocks: login_blocks(&json!({"reason":reason,"site":"網站連線驗證",
            "why":"完成驗證後，繼續原本的瀏覽任務。"})),
    }
}

async fn connection_check(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) { return blocked; }
    let (Some(x), Some(y)) = (args.get("x").and_then(Value::as_u64), args.get("y").and_then(Value::as_u64)) else {
        return text_outcome("Take a fresh computer_observe and provide the visible checkbox's non-negative screen x/y.");
    };
    if x > i32::MAX as u64 || y > i32::MAX as u64 {
        return text_outcome("Checkbox coordinates are out of range.");
    }
    let page = cdp_call(ctx, json!({"action":"snapshot","ensure":false})).await;
    if !is_connection_check(&page) {
        return text_outcome("No supported Cloudflare connection-check page was confirmed. Re-observe; use request_takeover for other CAPTCHA, login or 2FA. No click was sent.");
    }
    let already_attempted = {
        let mut attempted = ctx.connection_check_attempted.lock().unwrap();
        let previous = *attempted;
        *attempted = true;
        previous
    };
    if already_attempted {
        return connection_takeover(ctx, "這次任務已嘗試過連線驗證，請接管完成驗證。");
    }
    let actions = match parse_computer_actions(&json!([{"kind":"click","x":x,"y":y}])) {
        Ok(actions) => actions,
        Err(error) => return text_outcome(error.to_string()),
    };
    let adapter = ctx.adapter();
    let result = ctx.sandbox.act(&ctx.computer_ref(), ActionRequest {
        actions, observe:false, settle_ms:350,
        display:adapter.display.clone(), profile_path:adapter.profile_path.clone(),
    }, &adapter).await;
    if result.is_err() {
        return connection_takeover(ctx, "無法確認驗證點擊是否完成，請接管檢查。");
    }
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let after = cdp_call(ctx, json!({"action":"snapshot","ensure":false})).await;
        // Disappearance alone is not proof of success: loading/error pages
        // can also remove the checkbox. Return evidence for the task check.
        if after.ok && !is_connection_check(&after) && !after.text.trim().is_empty() {
            let mut outcome = observe(ctx).await;
            outcome.text = format!("Connection-check markers disappeared. This is NOT proof of success. Confirm the requested content is actually visible before continuing; if a challenge/error remains, request_takeover.\n{}\n{}",
                browser_result_text("snapshot", &after), outcome.text);
            return outcome;
        }
    }
    connection_takeover(ctx, "已嘗試一次驗證並等待，仍無法確認通過。請接管完成驗證，之後繼續原任務。")
}

async fn browser(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("snapshot");
    let mut request = json!({
        "action": action,
        "ensure": true,
    });
    for key in ["url", "text", "key", "ms", "selector", "waitMs"] {
        if let Some(value) = args.get(key) {
            request[key] = value.clone();
        }
    }
    if request.get("selector").and_then(Value::as_str).is_none()
        && let Some(id) = element_id(args.get("element").or_else(|| args.get("id")))
    {
        let elements = ctx.elements.lock().unwrap().clone();
        match elements
            .iter()
            .find(|element| u64::from(element.id) == id)
            .and_then(|element| element.selector.clone())
        {
            Some(selector) => request["selector"] = json!(selector),
            None if matches!(action, "click" | "type") => {
                return pause_unknown_element(ctx, id as u32, &elements);
            }
            None => {}
        }
    }
    if matches!(action, "click") && request.get("selector").and_then(Value::as_str).is_none() {
        let known: Vec<String> = ctx
            .elements
            .lock()
            .unwrap()
            .iter()
            .filter(|element| element.selector.is_some())
            .map(|element| format!("[{}] {}", element.id, element.title))
            .take(12)
            .collect();
        return text_outcome(format!(
            "browser click needs {{\"action\":\"click\",\"element\":N}} with a number from the last snapshot, or a CSS selector. You sent: {}. Known elements: {}",
            serde_json::to_string(args).unwrap_or_default(),
            if known.is_empty() {
                "none yet, call browser snapshot first".to_string()
            } else {
                known.join(", ")
            }
        ));
    }
    let page = cdp_call(ctx, request).await;
    if !page.elements.is_empty() {
        *ctx.elements.lock().unwrap() = page.elements.clone();
    }
    if !page.ok {
        let mut text = page.error.unwrap_or_else(|| "browser failed".into());
        if !page.elements.is_empty() {
            text.push_str(&format!(
                "\nPage: {} {}\nClickable page elements: {}",
                page.title,
                page.url,
                format_ui_elements(&page.elements)
            ));
        }
        return text_outcome(text);
    }
    let mut text = browser_result_text(action, &page);
    if is_connection_check(&page) {
        text.push_str("\nConnection check detected. Take computer_observe; if a simple verification checkbox is visible, use connection_check with its current screen coordinates once. For other puzzles use request_takeover. The requested content has NOT been retrieved.");
    }
    if let Some(seconds) = page.waited_seconds {
        text.push_str(&format!(
            " (the control was disabled; waited {seconds:.0}s for it to enable before clicking)"
        ));
    }
    if action == "snapshot" {
        return ToolOutcome {
            text,
            image: None,
            pause: false,
            blocks: Vec::new(),
        };
    }
    match ctx.sandbox.observe(&ctx.computer_ref(), &ctx.adapter()).await {
        Ok(observation) => {
            let (observation, note) = attach_ui_elements(ctx, observation, &text).await;
            pack_observation(ctx, &note, observation)
        }
        Err(_) => ToolOutcome {
            text,
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
    }
}

fn browser_result_text(action: &str, page: &CdpPage) -> String {
    if matches!(action, "snapshot" | "navigate") {
        format!(
            "browser {action}\nPage: {} {}\nClickable page elements: {}\nVisible text:\n{}",
            page.title,
            page.url,
            format_ui_elements(&page.elements),
            page.text
        )
    } else {
        format!("browser {action} ok")
    }
}

async fn act(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let mut args = args.clone();
    let elements = ctx.elements.lock().unwrap().clone();
    let actions_value = args.get("actions").cloned().unwrap_or(Value::Null);
    if let Some(message) = browser_gui_block(&actions_value, &elements) {
        return text_outcome(message);
    }
    let click_key = click_fingerprint(&actions_value);
    if should_block_stale_click(
        *ctx.miss_streak.lock().unwrap(),
        ctx.last_click_key.lock().unwrap().as_deref(),
        click_key.as_deref(),
    ) {
        return text_outcome(
            "The last clicks changed nothing. Do not repeat the same click. Options: the control may be disabled until a video/loading finishes (use wait, then re-observe); the target may be off (use browser snapshot and click by element id, or pick a different native control); if the page needs login, CAPTCHA or a human decision, call request_takeover.",
        );
    }
    if let Some(actions) = args.get_mut("actions") {
        if let Err(error) = apply_element_targets(actions, &elements) {
            if let ActionError::UnknownElement(id) = error {
                return pause_unknown_element(ctx, id, &elements);
            }
            return text_outcome(error.to_string());
        }
        if let Some(items) = actions.as_array_mut() {
            apply_semantic_actions(ctx, items, &elements).await;
        }
    }
    let actions = match parse_computer_actions(args.get("actions").unwrap_or(&Value::Null)) {
        Ok(actions) => actions,
        Err(error) => {
            return ToolOutcome {
                text: error.to_string(),
                image: None,
                pause: false,
                blocks: Vec::new(),
            };
        }
    };
    let had_click = actions.iter().any(action_is_click) || click_key.is_some();
    if let Some(key) = click_key {
        *ctx.last_click_key.lock().unwrap() = Some(key);
    }
    match ctx
        .sandbox
        .act(
            &ctx.computer_ref(),
            ActionRequest {
                actions,
                observe: args.get("observe").and_then(Value::as_bool) != Some(false),
                settle_ms: args.get("settle_ms").and_then(Value::as_u64).unwrap_or(350) as u32,
                display: ctx.adapter().display.clone(),
                profile_path: ctx.adapter().profile_path.clone(),
            },
            &ctx.adapter(),
        )
        .await
    {
        Ok(result) => {
            if let Some(observation) = result.observation {
                let (observation, note) = attach_ui_elements(
                    ctx,
                    observation,
                    &format!("completed {} computer action(s)", result.completed),
                )
                .await;
                let unchanged =
                    frames_match(ctx.previous_frame.lock().unwrap().as_deref(), &observation);
                let mut outcome = pack_observation(ctx, &note, observation);
                note_click_result(ctx, had_click, unchanged, &mut outcome);
                outcome
            } else {
                ToolOutcome {
                    text: json!({"ok": true, "completed": result.completed}).to_string(),
                    image: None,
                    pause: false,
                    blocks: Vec::new(),
                }
            }
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
    }
}

fn action_is_click(action: &ComputerAction) -> bool {
    matches!(
        action,
        ComputerAction::Pointer {
            pointer_type: PointerType::Click | PointerType::Down,
            ..
        } | ComputerAction::Ref {
            verb: lazyboy_contracts::RefVerb::Click,
            ..
        }
    )
}

async fn apply_semantic_actions(ctx: &ToolCtx, items: &mut [Value], elements: &[UiElement]) {
    for item in items {
        let Some(kind) = item.get("kind").and_then(Value::as_str) else {
            continue;
        };
        if !matches!(kind, "click" | "type") {
            continue;
        }
        let Some(target) = item
            .get("target")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        let ref_kind = item
            .get("refKind")
            .and_then(Value::as_str)
            .unwrap_or("a11y")
            .to_string();
        let doubled = item.get("double").and_then(Value::as_bool) == Some(true) && kind == "click";
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut request = json!({"action": kind, "selector": target, "ensure": false});
        if kind == "type" {
            request["text"] = json!(text);
        }
        let ok = if ref_kind == "dom" {
            let page = cdp_call(ctx, request.clone()).await;
            if doubled && page.ok {
                cdp_call(ctx, request).await.ok
            } else {
                page.ok
            }
        } else {
            let page = a11y_call(ctx, request.clone()).await;
            if doubled && page.ok {
                a11y_call(ctx, request).await.ok
            } else {
                page.ok
            }
        };
        if ok {
            if let Some(object) = item.as_object_mut() {
                object.insert("kind".into(), json!("wait"));
                object.insert("ms".into(), json!(80));
                object.remove("target");
                object.remove("x");
                object.remove("y");
            }
            continue;
        }
        let Some(id) = element_id(item.get("element")) else {
            continue;
        };
        let Some(element) = elements.iter().find(|element| u64::from(element.id) == id) else {
            continue;
        };
        if element.is_offscreen() {
            continue;
        }
        if let Some(object) = item.as_object_mut() {
            let (x, y) = element.center();
            object.insert("x".into(), json!(x));
            object.insert("y".into(), json!(y));
            object.remove("target");
        }
    }
}

/// A stale element id is the model's mistake, not a reason to park the run
/// on the human: tell it what is visible now and let it re-observe.
fn pause_unknown_element(_ctx: &ToolCtx, id: u32, elements: &[UiElement]) -> ToolOutcome {
    ToolOutcome {
        text: format!(
            "element {id} is not on screen any more. Visible now: {}. Call browser snapshot or computer_observe to get fresh ids, then retry.",
            format_ui_elements(elements)
        ),
        image: None,
        pause: false,
        blocks: Vec::new(),
    }
}

/// Clicks that change nothing are common and usually recoverable (disabled
/// button, video still playing, slightly off target). Coach the model instead
/// of pausing; it can still call request_takeover when it is truly stuck.
fn note_click_result(ctx: &ToolCtx, had_click: bool, unchanged: bool, outcome: &mut ToolOutcome) {
    if !had_click {
        return;
    }
    let mut streak = ctx.miss_streak.lock().unwrap();
    if unchanged {
        *streak += 1;
        *ctx.click_misses.lock().unwrap() += 1;
        if *streak >= 2 {
            outcome.text.push_str(
                "\nThe last clicks changed nothing. Do not repeat the same click. Options: the control may be disabled until a video/loading finishes (use wait, then re-observe); the target may be off (use browser snapshot and click by element id, or pick coordinates from a fresh computer_observe); if the page needs login, CAPTCHA or a human decision, call request_takeover.",
            );
        }
    } else {
        *streak = 0;
    }
}

fn pack_observation(ctx: &ToolCtx, note: &str, observation: ComputerObservation) -> ToolOutcome {
    let unchanged = frames_match(ctx.previous_frame.lock().unwrap().as_deref(), &observation);
    *ctx.previous_frame.lock().unwrap() = Some(observation.frame_id.clone());
    *ctx.elements.lock().unwrap() = observation.elements.clone();
    ToolOutcome {
        text: observation_text(note, &observation, unchanged),
        image: if unchanged {
            None
        } else {
            Some(overlay_elements(&observation.image, &observation.elements))
        },
        pause: false,
        blocks: Vec::new(),
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
            &ctx.computer_ref(),
            CommandRequest {
                argv: vec!["bash".into(), "-lc".into(), command.into()],
                cwd,
                timeout_ms: Some(60_000),
                stdin: None,
            },
            &ctx.adapter(),
        )
        .await
    {
        Ok(result) => ToolOutcome {
            text: format!("exit {}\n{}\n{}", result.code, result.stdout, result.stderr),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
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
                blocks: Vec::new(),
            };
        }
    };
    match ctx
        .sandbox
        .list_files(&ctx.computer_ref(), &stored, &ctx.adapter())
        .await
    {
        Ok(entries) => ToolOutcome {
            text: serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
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
                blocks: Vec::new(),
            };
        }
    };
    match ctx
        .sandbox
        .read_file(&ctx.computer_ref(), &stored, &ctx.adapter())
        .await
    {
        Ok(bytes) => ToolOutcome {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
    }
}

async fn write_file(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let requested = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("notes.txt");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let stored = match resolve_bot_workspace_path(ctx.mode, &ctx.bot_id, requested) {
        Ok(path) => path,
        Err(error) => {
            return ToolOutcome {
                text: error.to_string(),
                image: None,
                pause: false,
                blocks: Vec::new(),
            };
        }
    };
    match ctx
        .sandbox
        .write_file(&ctx.computer_ref(), &stored, content.as_bytes(), &ctx.adapter())
        .await
    {
        Ok(()) => ToolOutcome {
            text: json!({"ok": true, "path": requested}).to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
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
            &ctx.computer_ref(),
            ActionRequest {
                actions: vec![lazyboy_contracts::ComputerAction::Open { path: path.into() }],
                observe: true,
                settle_ms: 400,
                display: ctx.adapter().display.clone(),
                profile_path: ctx.adapter().profile_path.clone(),
            },
            &ctx.adapter(),
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
                    blocks: Vec::new(),
                }
            }
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
    }
}

async fn launch_app(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let application = args
        .get("application")
        .and_then(Value::as_str)
        .unwrap_or("browser");
    let uri = args.get("uri").and_then(Value::as_str).map(str::to_string);
    match ctx
        .sandbox
        .act(
            &ctx.computer_ref(),
            ActionRequest {
                actions: vec![lazyboy_contracts::ComputerAction::Launch {
                    application: application.into(),
                    uri,
                }],
                observe: true,
                settle_ms: 500,
                display: ctx.adapter().display.clone(),
                profile_path: ctx.adapter().profile_path.clone(),
            },
            &ctx.adapter(),
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
                    blocks: Vec::new(),
                }
            }
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
            blocks: Vec::new(),
        },
    }
}

fn login_blocks(args: &Value) -> Vec<Value> {
    let reason = args
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let site = args
        .get("site")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let why = args.get("why").and_then(Value::as_str).unwrap_or("").trim();
    let site = if site.is_empty() { reason } else { site };
    let why = if why.is_empty() { reason } else { why };
    vec![json!({
        "kind": "login",
        "site": site,
        "why": why,
    })]
}

async fn list_saved_accounts(ctx: &ToolCtx) -> ToolOutcome {
    match crate::vault::list_on(&ctx.pool, &ctx.actor, &ctx.bot_id).await {
        Ok(items) => {
            let slim: Vec<Value> = items
                .iter()
                .map(|item| {
                    json!({
                        "accountId": item.id,
                        "site": item.site,
                        "host": item.host,
                        "username": item.username,
                    })
                })
                .collect();
            if slim.is_empty() {
                text_outcome("No saved logins. Ask the human to add one under 帳號, or call request_takeover so they can sign in on the screen.")
            } else {
                text_outcome(json!({"accounts": slim}).to_string())
            }
        }
        Err(error) => text_outcome(error),
    }
}

async fn use_saved_login(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let Some(account_id) = args.get("accountId").and_then(Value::as_str) else {
        return text_outcome("accountId is required");
    };
    let secret =
        match crate::vault::get_secret_on(&ctx.pool, &ctx.actor, &ctx.bot_id, account_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return text_outcome("that saved account was not found"),
            Err(error) => return text_outcome(error),
        };
    let (account, username, password) = secret;
    let adapter = ctx.adapter();
    let display = adapter.display.as_deref().unwrap_or(":1");
    let request = json!({
        "action": "fill_login",
        "expectedHost": account.host,
        "ensure": false,
        "username": username,
        "password": password,
        "display": display,
        "port": lazyboy_control::devtools_port(display),
        "profile": adapter.profile_path.clone().unwrap_or_default(),
    });
    let argv = cdp_stdin_command_on(display, adapter.profile_path.as_deref());
    let raw = match ctx
        .sandbox
        .execute(
            &ctx.computer_ref(),
            CommandRequest {
                argv,
                cwd: None,
                timeout_ms: Some(20_000),
                stdin: Some(request.to_string()),
            },
            &ctx.adapter(),
        )
        .await
    {
        Ok(result) => {
            if result.stdout.trim().is_empty() {
                result.stderr
            } else {
                result.stdout
            }
        }
        Err(error) => {
            return text_outcome(format!("could not fill login: {error}"));
        }
    };
    let page = parse_cdp_page(&raw);
    if !page.ok {
        return text_outcome(format!(
            "could not fill {} login: {}",
            account.site,
            page.error.unwrap_or_else(|| "unknown error".into())
        ));
    }
    let submitted = serde_json::from_str::<Value>(raw.trim())
        .ok()
        .and_then(|value| value.get("submitted").and_then(Value::as_bool))
        .unwrap_or(false);
    text_outcome(format!(
        "Filled {} as {}. {} For a simple Cloudflare checkbox, observe and try connection_check once; for other CAPTCHA or 2FA, call request_takeover.",
        account.site,
        account.username,
        if submitted {
            "Submitted the form."
        } else {
            "Username and password are in the fields; click Sign in if needed."
        }
    ))
}

async fn create_schedule_tool(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let name = args.get("name").and_then(Value::as_str).unwrap_or("");
    let cron = args.get("cron").and_then(Value::as_str).unwrap_or("");
    let instructions = args
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or("");
    let timezone = args
        .get("timezone")
        .and_then(Value::as_str)
        .unwrap_or("Asia/Taipei");
    let state = schedule_state(ctx);
    match crate::schedules::create(
        &state,
        &ctx.actor,
        &ctx.bot_id,
        crate::schedules::CreateSchedule {
            name: name.to_string(),
            cron: cron.to_string(),
            timezone: timezone.to_string(),
            instructions: instructions.to_string(),
            thread_id: Some(ctx.session_id.clone()),
            enabled: true,
        },
    )
    .await
    {
        Ok(row) => {
            let human = crate::schedules::describe_cron(&row.cron);
            ToolOutcome {
                text: format!("Scheduled 「{}」 ({human}).", row.name),
                image: None,
                pause: false,
                blocks: vec![json!({
                    "kind": "schedule",
                    "scheduleId": row.id,
                    "name": row.name,
                    "cron": row.cron,
                    "human": human,
                })],
            }
        }
        Err(error) => text_outcome(error),
    }
}

async fn list_schedules_tool(ctx: &ToolCtx) -> ToolOutcome {
    let state = schedule_state(ctx);
    match crate::schedules::list(&state, &ctx.actor, &ctx.bot_id).await {
        Ok(rows) => text_outcome(
            json!({
                "schedules": rows.into_iter().map(crate::schedules::public_json).collect::<Vec<_>>()
            })
            .to_string(),
        ),
        Err(error) => text_outcome(error),
    }
}

async fn cancel_schedule_tool(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let Some(id) = args.get("scheduleId").and_then(Value::as_str) else {
        return text_outcome("scheduleId is required");
    };
    let delete = args.get("delete").and_then(Value::as_bool).unwrap_or(false);
    if delete {
        let result = sqlx::query(
            "DELETE FROM schedules WHERE id=$1 AND bot_id=$2 AND space_id=$3 AND user_id=$4",
        )
        .bind(id)
        .bind(&ctx.bot_id)
        .bind(&ctx.actor.space_id)
        .bind(&ctx.actor.user_id)
        .execute(&ctx.pool)
        .await;
        return match result {
            Ok(done) if done.rows_affected() == 1 => text_outcome("schedule deleted"),
            Ok(_) => text_outcome("schedule not found"),
            Err(error) => text_outcome(error.to_string()),
        };
    }
    let result = sqlx::query(
        "UPDATE schedules SET enabled=false, next_run_at=NULL, updated_at=now()
         WHERE id=$1 AND bot_id=$2 AND space_id=$3 AND user_id=$4",
    )
    .bind(id)
    .bind(&ctx.bot_id)
    .bind(&ctx.actor.space_id)
    .bind(&ctx.actor.user_id)
    .execute(&ctx.pool)
    .await;
    match result {
        Ok(done) if done.rows_affected() == 1 => text_outcome("schedule paused"),
        Ok(_) => text_outcome("schedule not found"),
        Err(error) => text_outcome(error.to_string()),
    }
}

fn schedule_state(ctx: &ToolCtx) -> crate::state::AppState {
    crate::state::AppState {
        db: crate::db::Db {
            pool: ctx.pool.clone(),
        },
        sandbox: ctx.sandbox.clone(),
        data_dir: String::new(),
        auth: crate::auth::AuthConfig::from_env(),
        memory: ctx.memory.clone(),
        mcp: ctx.mcp.clone(),
        calls: crate::state::CallRegistry::default(),
    }
}

#[cfg(test)]
mod connection_check_tests {
    use super::*;
    #[test]
    fn recognizes_connection_wall_but_not_cloudflare_footer() {
        for text in ["Cloudflare 驗證您是人類", "Cloudflare Verify you are human", "Dcard 需要確認您的連線是安全的"] {
            assert!(is_connection_check(&CdpPage { ok:true, text:text.into(), ..Default::default() }));
        }
        for text in ["Article text. Protected by Cloudflare", "Sign in with your password", ""] {
            assert!(!is_connection_check(&CdpPage { ok:true, text:text.into(), ..Default::default() }));
        }
        assert!(!is_connection_check(&CdpPage { ok:false, text:"Cloudflare Verify you are human".into(), ..Default::default() }));
    }
}
