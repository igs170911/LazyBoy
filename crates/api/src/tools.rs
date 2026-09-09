use std::sync::Mutex;

use lazyboy_contracts::{
    ComputerAction, ComputerMode, ComputerObservation, PointerType, UiElement,
};
use lazyboy_control::{
    ActionDecision, ActionError, ActionRequest, ActionVerdict, AdapterContext, BrowserPage,
    BrowserRequest, ComputerRef, SandboxProvider, ScreenChange, apply_element_targets,
    click_fingerprint, element_id, format_ui_element_lines, format_ui_elements, frame_signature,
    merge_ui_elements, overlay_elements, parse_computer_actions, resolve_bot_workspace_cwd,
    resolve_bot_workspace_path, screen_change_between, should_block_stale_click,
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
    /// Coarse signature of the previous capture. `frame_id` is a sha256, so on
    /// a live desktop every panel clock tick is a "new" frame; the signature is
    /// what makes "nothing actually happened" detectable.
    pub previous_signature: Mutex<Option<Vec<u8>>>,
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
            description: "Capture a fresh desktop screenshot plus numbered targets (page DOM when Chromium is open, otherwise AT-SPI buttons/fields, otherwise windows). Only when the user asked you to do something on the computer. Not for greetings or chat. File tools already return their visible terminal screenshot. The image attaches only if the screen changed.".into(),
            parameters: json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "computer_act".into(),
            description: "Drive native desktop GUI (dialogs, file manager, XFCE) when the user asked you to operate the computer. Prefer element id from the latest observation (AT-SPI, not pixels). Prefer the browser tool for web page elements. For canvas or unsupported browser controls, observe the current screenshot first and use Cua coordinate actions. x,y use the screenshot pixel dimensions.".into(),
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
            description: "Use Cua to type in a visible persistent terminal on the shared VNC desktop. The same session keeps directory, environment and background jobs. Results are screenshots: inspect the prompt before sending another command; do not type while a command is busy. Omit command to inspect the terminal, or send keys C-c to interrupt. Use computer_act to scroll through output. Not for greetings or questions that need no computer.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string","description":"Command to type. Omit it to just read the terminal."},
                    "session":{"type":"string","description":"Terminal name: the same name is the same terminal. Default main."},
                    "wait_ms":{"type":"number","description":"Wait before capturing the terminal, default 1000 ms, max 10000. Longer jobs keep running; poll by omitting command."},
                    "keys":{"type":"string","description":"Keys instead of a command, space separated: \"C-c\", \"q\", \"Escape Enter\"."},
                    "reset":{"type":"boolean","description":"Start this terminal over: drops directory, exports, and jobs."},
                    "cwd":{"type":"string","description":"Directory for this command; the terminal stays there."}
                }
            }),
        },
        ToolDefinition {
            name: "list_files".into(),
            description: "List files in this bot's home when the user asked about workspace files. Not for greetings or chat — do not ls the desktop to start a conversation.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "read_file".into(),
            description: "Read a page of a UTF-8 file through Cua in the visible terminal. Output is a screenshot; use start_line and lines for further pages or scroll the terminal.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer"},"lines":{"type":"integer"}},"required":["path"]}),
        },
        ToolDefinition {
            name: "write_file".into(),
            description: "Write a UTF-8 file by typing a quoted command through Cua in the visible terminal. Inspect the returned screenshot for errors and the written byte count.".into(),
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
            description: "Control Chromium through the page DOM when the user asked you to use the browser. Prefer this over computer_act for anything in the page. Every action returns fresh numbered elements and visible text; screenshots are opt-in with observe:true for visual ambiguity. click/type/navigate by element id or CSS selector. click scrolls off-screen elements into view and, if the control is [disabled], waits up to 45s (waitMs to change) for it to enable before clicking. Ids are renumbered after every page change. The human still sees the live window.".into(),
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
                    "observe":{"type":"boolean","description":"Attach a screenshot only when visual verification is needed; default false"},
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
            description: "Pause the task and ask the user to complete passwords, 2FA, CAPTCHA, or a login wall when no saved account fits. The shared screen already accepts their input; they press Done, continue when finished. Never ask them to paste secrets in chat. Prefer use_saved_login when list_accounts has a matching site.".into(),
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

/// A human holds the desktop, so nothing may touch it or describe it — no
/// matter what the model can see.
fn gui_blocked(ctx: &ToolCtx) -> Option<ToolOutcome> {
    ctx.gui_block
        .lock()
        .unwrap()
        .clone()
        .map(|message| ToolOutcome {
            text: message,
            image: None,
            pause: false,
            blocks: Vec::new(),
        })
}

/// Everything that acts on pixels. Observation deliberately does not use this:
/// an element tree is text, so a text-only model can still read the desktop.
fn vision_guard(ctx: &ToolCtx) -> Option<ToolOutcome> {
    gui_blocked(ctx).or_else(|| {
        (!ctx.vision).then(|| {
            text_outcome(
                "This model cannot see the shared desktop, so it cannot drive it. computer_observe still reports the element tree as text; pick a vision model to click, type, or browse.",
            )
        })
    })
}

/// Controls listed in one observation. A busy native desktop lands near this
/// number and the tail is scrolled-off controls and duplicated windows.
const MAX_LISTED_ELEMENTS: usize = 120;

/// The element list replaces the element array: models address controls by id
/// (see `element_id`), so serializing both doubled every observation.
fn observation_text(note: &str, observation: &ComputerObservation, change: ScreenChange) -> String {
    let label = if observation
        .elements
        .iter()
        .any(|element| matches!(element.kind.as_deref(), Some("dom") | Some("a11y")))
    {
        "Clickable controls"
    } else {
        "Clickable windows"
    };
    // Coverage belongs on the list, not in a separate warning: this is the
    // exact place where a model decides that a control does not exist.
    let coverage = if observation.native_observation_complete {
        ""
    } else {
        " (partial: some windows did not report controls, so a missing entry is not proof)"
    };
    format!(
        "{note}{}\n{label}{coverage}:\n{}\n{}",
        match change {
            ScreenChange::Identical => " (screen unchanged)",
            ScreenChange::Similar => " (no visible change)",
            ScreenChange::Changed => "",
        },
        format_ui_element_lines(&observation.elements, MAX_LISTED_ELEMENTS),
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
    if let Some(blocked) = gui_blocked(ctx) {
        return blocked;
    }
    match ctx
        .sandbox
        .observe(&ctx.computer_ref(), &ctx.adapter())
        .await
    {
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
    if gui_blocked(ctx).is_some() {
        return text_outcome(format!("waited {seconds:.0}s"));
    }
    match ctx
        .sandbox
        .observe(&ctx.computer_ref(), &ctx.adapter())
        .await
    {
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
    let page = browser_snapshot(ctx, false).await;
    let page_elements = page
        .as_ref()
        .map(|page| page.elements.as_slice())
        .unwrap_or(&[]);
    let native: Vec<_> = observation
        .elements
        .iter()
        .filter(|element| element.kind.as_deref() == Some("a11y"))
        .cloned()
        .collect();
    observation
        .elements
        .retain(|element| element.kind.as_deref() != Some("a11y"));
    observation.elements = merge_ui_elements(observation.elements, page_elements, &native);
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

async fn browser_snapshot(ctx: &ToolCtx, ensure: bool) -> Option<BrowserPage> {
    let page = browser_call(ctx, json!({"action": "snapshot", "ensure": ensure})).await;
    if page.ok { Some(page) } else { None }
}

async fn browser_call(ctx: &ToolCtx, request: Value) -> BrowserPage {
    let adapter = ctx.adapter();
    let mut browser_request: BrowserRequest =
        serde_json::from_value(request.clone()).unwrap_or_default();
    if browser_request.action.is_empty() {
        browser_request.action = request
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("snapshot")
            .to_string();
    }
    if let Some(ensure) = request.get("ensure").and_then(Value::as_bool) {
        browser_request.ensure = ensure;
    }
    browser_request.display = adapter.display.clone();
    browser_request.profile_path = adapter.profile_path.clone();
    match ctx
        .sandbox
        .browser(&ctx.computer_ref(), browser_request, &adapter)
        .await
    {
        Ok(page) => page,
        Err(error) => BrowserPage {
            ok: false,
            error: Some(error.to_string()),
            ..BrowserPage::default()
        },
    }
}

fn is_connection_check(page: &BrowserPage) -> bool {
    let text = format!("{} {}", page.title, page.text).to_lowercase();
    page.ok
        && (text.contains("需要確認您的連線是安全")
            || (text.contains("cloudflare")
                && [
                    "verify you are human",
                    "verifying you are human",
                    "checking your browser",
                    "checking if the site connection is secure",
                    "needs to review the security",
                    "驗證您是人類",
                    "验证您是人类",
                    "確認您的連線是安全",
                    "確認您的人類身分",
                ]
                .iter()
                .any(|marker| text.contains(marker))))
}

fn connection_takeover(ctx: &ToolCtx, reason: &str) -> ToolOutcome {
    *ctx.takeover_requested.lock().unwrap() = true;
    ToolOutcome {
        text: reason.into(),
        image: None,
        pause: true,
        blocks: login_blocks(&json!({"reason":reason,"site":"網站連線驗證",
            "why":"完成驗證後，繼續原本的瀏覽任務。"})),
    }
}

async fn connection_check(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let (Some(x), Some(y)) = (
        args.get("x").and_then(Value::as_u64),
        args.get("y").and_then(Value::as_u64),
    ) else {
        return text_outcome(
            "Take a fresh computer_observe and provide the visible checkbox's non-negative screen x/y.",
        );
    };
    if x > i32::MAX as u64 || y > i32::MAX as u64 {
        return text_outcome("Checkbox coordinates are out of range.");
    }
    let page = browser_call(ctx, json!({"action":"snapshot","ensure":false})).await;
    if !is_connection_check(&page) {
        return text_outcome(
            "No supported Cloudflare connection-check page was confirmed. Re-observe; use request_takeover for other CAPTCHA, login or 2FA. No click was sent.",
        );
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
    let result = ctx
        .sandbox
        .act(
            &ctx.computer_ref(),
            ActionRequest {
                actions,
                observe: false,
                settle_ms: 350,
                display: adapter.display.clone(),
                profile_path: adapter.profile_path.clone(),
            },
            &adapter,
        )
        .await;
    if result.is_err() {
        return connection_takeover(ctx, "無法確認驗證點擊是否完成，請接管檢查。");
    }
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let after = browser_call(ctx, json!({"action":"snapshot","ensure":false})).await;
        // Disappearance alone is not proof of success: loading/error pages
        // can also remove the checkbox. Return evidence for the task check.
        if after.ok && !is_connection_check(&after) && !after.text.trim().is_empty() {
            let mut outcome = observe(ctx).await;
            outcome.text = format!(
                "Connection-check markers disappeared. This is NOT proof of success. Confirm the requested content is actually visible before continuing; if a challenge/error remains, request_takeover.\n{}\n{}",
                browser_result_text("snapshot", &after),
                outcome.text
            );
            return outcome;
        }
    }
    connection_takeover(
        ctx,
        "已嘗試一次驗證並等待，仍無法確認通過。請接管完成驗證，之後繼續原任務。",
    )
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
    let page = browser_call(ctx, request).await;
    if page.ok || !page.elements.is_empty() {
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
    if args.get("observe").and_then(Value::as_bool) != Some(true) {
        return ToolOutcome {
            text,
            image: None,
            pause: false,
            blocks: Vec::new(),
        };
    }
    match ctx
        .sandbox
        .observe(&ctx.computer_ref(), &ctx.adapter())
        .await
    {
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

fn browser_result_text(action: &str, page: &BrowserPage) -> String {
    format!(
        "browser {action}\nPage: {} {}\nClickable page elements: {}\nVisible text:\n{}",
        page.title,
        page.url,
        format_ui_elements(&page.elements),
        page.text
    )
}

async fn act(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let mut args = args.clone();
    let elements = ctx.elements.lock().unwrap().clone();
    let actions_value = args.get("actions").cloned().unwrap_or(Value::Null);
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
            let confirmed = result
                .verdict
                .as_ref()
                .is_some_and(|verdict| verdict.decision == ActionDecision::Done);
            let verdict = result.verdict.as_ref().map(verdict_note);
            if let Some(observation) = result.observation {
                let (observation, note) = attach_ui_elements(
                    ctx,
                    observation,
                    &format!("completed {} computer action(s)", result.completed),
                )
                .await;
                let change = screen_change(ctx, &observation);
                let mut outcome = pack_observation(ctx, &note, observation);
                note_click_result(
                    ctx,
                    had_click,
                    // A confirmed effect is a change even when a ticking clock
                    // hid it in the pixels; counting it as a miss would coach
                    // the model away from a click that worked.
                    change != ScreenChange::Changed && !confirmed,
                    &mut outcome,
                );
                with_verdict(&mut outcome, verdict);
                outcome
            } else {
                let mut outcome = ToolOutcome {
                    text: json!({"ok": true, "completed": result.completed}).to_string(),
                    image: None,
                    pause: false,
                    blocks: Vec::new(),
                };
                with_verdict(&mut outcome, verdict);
                outcome
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
        // Snapshot-scoped Cua targets must reach the controller unchanged.
        // Legacy AT-SPI cannot resolve them, and pixel fallback would bypass staleness checks.
        if target.starts_with("cua:") {
            continue;
        }
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
            let page = browser_call(ctx, request.clone()).await;
            if doubled && page.ok {
                browser_call(ctx, request).await.ok
            } else {
                page.ok
            }
        } else {
            // Native references are resolved only by Cua's snapshot cache.
            // Unknown references must fail instead of reaching the removed AT-SPI driver.
            continue;
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
/// `unchanged` is the perceptual comparison on purpose: with a byte-exact one a
/// ticking clock would reset the streak and this advice would never fire.
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

/// What the driver could prove about the actions it just ran, as one line the
/// model can act on. `None` from the driver stays `None` here: inventing
/// "it worked" is exactly the mistake that makes a model retype a field that
/// was already filled.
fn verdict_note(verdict: &ActionVerdict) -> String {
    let detail = verdict
        .effect
        .as_deref()
        .map(|effect| format!(" (driver said: {effect})"))
        .unwrap_or_default();
    match verdict.decision {
        ActionDecision::Done => "verdict: effect confirmed. Do not repeat this action.".into(),
        ActionDecision::VerifyFreshState => format!(
            "verdict: effect not confirmed{detail}. Read the new screenshot before you retry anything, and never repeat input that may already have worked."
        ),
        ActionDecision::Escalate => format!(
            "verdict: the driver reports no effect{detail}. Re-observe, then change the approach (fresh coordinates, focus the window first, another control) instead of repeating the same input."
        ),
    }
}

/// First line of the tool result: the verdict decides whether the model looks
/// at the new frame or fires the same input again, so it goes before the frame.
fn with_verdict(outcome: &mut ToolOutcome, verdict: Option<String>) {
    if let Some(note) = verdict {
        outcome.text = format!("{note}\n{}", outcome.text);
    }
}

fn pack_observation(ctx: &ToolCtx, note: &str, observation: ComputerObservation) -> ToolOutcome {
    let change = screen_change(ctx, &observation);
    let signature = frame_signature(&observation.image);
    *ctx.previous_frame.lock().unwrap() = Some(observation.frame_id.clone());
    *ctx.previous_signature.lock().unwrap() = signature;
    *ctx.elements.lock().unwrap() = observation.elements.clone();
    let mut text = observation_text(note, &observation, change);
    if !ctx.vision {
        // The list is the whole payload for this model, so say so: left unsaid
        // it keeps waiting for a picture that is never coming.
        text.push_str("\n(elements only: this model cannot see the screen)");
    }
    ToolOutcome {
        text,
        // Only a byte-identical frame drops the picture. A change too small to
        // move the signature is still one the model gets to look at.
        image: if ctx.vision && change != ScreenChange::Identical {
            None
        } else {
            Some(overlay_elements(&observation.image, &observation.elements))
        },
        pause: false,
        blocks: Vec::new(),
    }
}

/// How this capture differs from the one the Agent saw last: byte-exact for the
/// screenshot contract, perceptual for advice.
fn screen_change(ctx: &ToolCtx, observation: &ComputerObservation) -> ScreenChange {
    let previous_frame = ctx.previous_frame.lock().unwrap().clone();
    let previous_signature = ctx.previous_signature.lock().unwrap().clone();
    screen_change_between(
        previous_frame.as_deref(),
        previous_signature.as_deref(),
        observation,
    )
}

/// Quote literal shell text typed by Cua into the visible terminal.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

// X11 key injection cannot represent every Unicode character in a terminal.
// Type one ASCII shell literal, then let the visible shell decode its UTF-8
// bytes. This also keeps multiline commands in one deliberate Enter action.
fn terminal_command(command: &str) -> String {
    let mut encoded = String::new();
    for byte in command.bytes() {
        match byte {
            b'\\' => encoded.push_str("\\\\"),
            b'\'' => encoded.push_str("\\'"),
            32..=126 => encoded.push(char::from(byte)),
            _ => encoded.push_str(&format!("\\x{byte:02x}")),
        }
    }
    format!("eval $'{encoded}'")
}

fn terminal_actions(
    args: &Value,
    title: &str,
    exists: bool,
    cwd: Option<&str>,
) -> Result<Vec<ComputerAction>, String> {
    let mut actions = vec![if exists {
        ComputerAction::Focus {
            title: title.into(),
        }
    } else {
        ComputerAction::Launch {
            application: "terminal".into(),
            uri: Some(format!("--title={title}")),
        }
    }];
    if args.get("reset").and_then(Value::as_bool) == Some(true) {
        actions.push(ComputerAction::Key {
            key: "ctrl+c".into(),
            modifiers: None,
        });
        actions.push(ComputerAction::Clipboard {
            text: "exec /usr/local/bin/lazyboy-terminal-reset".into(),
        });
        actions.push(ComputerAction::Key {
            key: "return".into(),
            modifiers: None,
        });
    } else if let Some(keys) = args.get("keys").and_then(Value::as_str) {
        if keys.trim().is_empty() {
            return Err("keys must contain at least one key".into());
        }
        for key in keys.split_whitespace() {
            let key = key.replace("C-", "ctrl+").replace("M-", "alt+");
            actions.push(ComputerAction::Key {
                key,
                modifiers: None,
            });
        }
    } else if let Some(command) = args
        .get("command")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        let command = if let Some(cwd) = cwd {
            let path = if cwd.starts_with('/') {
                cwd.into()
            } else {
                format!("/home/lazyboy/{cwd}")
            };
            format!(
                "cd -- {} && eval {}",
                shell_quote(&path),
                shell_quote(command)
            )
        } else {
            command.to_string()
        };
        actions.push(ComputerAction::Clipboard {
            text: terminal_command(&command),
        });
        actions.push(ComputerAction::Key {
            key: "return".into(),
            modifiers: None,
        });
    }
    if actions.len() > lazyboy_control::MAX_COMPUTER_ACTIONS {
        return Err("too many keys; split the call".into());
    }
    Ok(actions)
}

async fn shell(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    if let Some(blocked) = vision_guard(ctx) {
        return blocked;
    }
    let session = args
        .get("session")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .trim();
    if session.is_empty()
        || session.len() > 80
        || !session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return text_outcome(
            "session must contain 1–80 letters, digits, dots, underscores or hyphens",
        );
    }
    let title = format!("LazyBoy terminal {} {session}", ctx.bot_id);
    let observation = match ctx
        .sandbox
        .observe(&ctx.computer_ref(), &ctx.adapter())
        .await
    {
        Ok(observation) => observation,
        Err(error) => return text_outcome(error.to_string()),
    };
    let exists = observation
        .elements
        .iter()
        .any(|element| element.kind.as_deref() == Some("window") && element.title == title);
    // An existing terminal keeps its working directory unless explicitly changed.
    let cwd = if !exists || args.get("cwd").is_some() {
        match resolve_bot_workspace_cwd(
            ctx.mode,
            &ctx.bot_id,
            args.get("cwd").and_then(Value::as_str),
        ) {
            Ok(cwd) => cwd,
            Err(error) => return text_outcome(error.to_string()),
        }
    } else {
        None
    };
    let actions = match terminal_actions(args, &title, exists, cwd.as_deref()) {
        Ok(actions) => actions,
        Err(error) => return text_outcome(error),
    };
    let wait_ms = args
        .get("wait_ms")
        .and_then(Value::as_u64)
        .unwrap_or(1000)
        .min(10000) as u32;
    match ctx
        .sandbox
        .act(
            &ctx.computer_ref(),
            ActionRequest::new(actions, true, wait_ms),
            &ctx.adapter(),
        )
        .await
    {
        Ok(result) => match result.observation {
            Some(observation) => pack_observation(
                ctx,
                &format!(
                    "Terminal {session} on the shared desktop. Read the screenshot to check output and whether the prompt returned. A running command keeps running; omit command to inspect it again, or use keys C-c to interrupt. No exit status is inferred."
                ),
                observation,
            ),
            None => text_outcome("terminal action completed but no screenshot was returned"),
        },
        Err(error) => text_outcome(error.to_string()),
    }
}

fn visible_file_path(ctx: &ToolCtx, args: &Value, default: &str) -> Result<String, String> {
    let requested = args.get("path").and_then(Value::as_str).unwrap_or(default);
    let stored = resolve_bot_workspace_path(ctx.mode, &ctx.bot_id, requested)
        .map_err(|error| error.to_string())?;
    Ok(format!("/home/lazyboy/{stored}"))
}

async fn list_files(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let path = match visible_file_path(ctx, args, "") {
        Ok(path) => path,
        Err(error) => return text_outcome(error),
    };
    shell(
        ctx,
        &json!({"session":"files", "command":format!("ls -la -- {}", shell_quote(&path))}),
    )
    .await
}

async fn read_file(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let path = match visible_file_path(ctx, args, "") {
        Ok(path) => path,
        Err(error) => return text_outcome(error),
    };
    let start = args
        .get("start_line")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1);
    let count = args
        .get("lines")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 200);
    let end = start.saturating_add(count - 1);
    shell(ctx, &json!({"session":"files", "command":format!("sed -n '{start},{end}p' -- {}", shell_quote(&path))})).await
}

async fn write_file(ctx: &ToolCtx, args: &Value) -> ToolOutcome {
    let path = match visible_file_path(ctx, args, "notes.txt") {
        Ok(path) => path,
        Err(error) => return text_outcome(error),
    };
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let parent = std::path::Path::new(&path)
        .parent()
        .and_then(|path| path.to_str())
        .unwrap_or("/home/lazyboy");
    let command = format!(
        "mkdir -p -- {} && printf %s {} > {} && wc -c -- {}",
        shell_quote(parent),
        shell_quote(content),
        shell_quote(&path),
        shell_quote(&path)
    );
    shell(ctx, &json!({"session":"files", "command":command})).await
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
                text_outcome(
                    "No saved logins. Ask the human to add one under 帳號, or call request_takeover so they can sign in on the screen.",
                )
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
    let page = browser_call(ctx, json!({"action":"snapshot","ensure":false})).await;
    let filled = match fill_login_fields(
        page,
        &account.host,
        username,
        password,
        |selector, text| async move {
            browser_call(
                ctx,
                json!({"action":"type","selector":selector,"text":text,"ensure":false}),
            )
            .await
        },
    )
    .await
    {
        Ok(fields) => fields,
        Err(message) => return text_outcome(message),
    };
    text_outcome(format!(
        "Filled {} for {} through Cua on the shared desktop. Inspect the form before submitting. For a multi-step login, advance the form and call use_saved_login again.",
        filled.join(" and "),
        account.site
    ))
}

async fn fill_login_fields<F, Fut>(
    mut page: BrowserPage,
    host: &str,
    username: String,
    password: String,
    mut type_field: F,
) -> Result<Vec<&'static str>, &'static str>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = BrowserPage>,
{
    let mut filled = Vec::new();
    for (field, value) in [("username", username), ("password", password)] {
        if !login_page_matches(&page, host) {
            return Err(
                "Login fields were not filled: the current page must use HTTPS and exactly match the saved account host.",
            );
        }
        let Some(selector) = login_field(&page, field) else {
            continue;
        };
        page = type_field(selector, value).await;
        if !page.ok {
            // Driver errors can contain the attempted secret. Never echo them.
            return Err(
                "Cua could not fill the login field. Observe the page again before retrying.",
            );
        }
        filled.push(field);
    }
    if filled.is_empty() {
        return Err(
            "No uniquely labelled login fields were found. Open the login form or request human takeover; no credentials were entered.",
        );
    }
    Ok(filled)
}

fn login_page_matches(page: &BrowserPage, host: &str) -> bool {
    page.ok
        && reqwest::Url::parse(&page.url).ok().is_some_and(|url| {
            url.scheme() == "https"
                && url
                    .host_str()
                    .is_some_and(|current| current.eq_ignore_ascii_case(host))
        })
}

fn login_field(page: &BrowserPage, field: &str) -> Option<String> {
    let words: &[&str] = if field == "password" {
        &["password", "密碼", "密码"]
    } else {
        &[
            "username",
            "user name",
            "email",
            "e-mail",
            "帳號",
            "账号",
            "電子郵件",
            "電子郵箱",
            "使用者名稱",
            "用戶名",
        ]
    };
    let mut matches = page.elements.iter().filter(|element| {
        let label = element.title.to_lowercase();
        let editable = element
            .role
            .as_deref()
            .is_some_and(|role| matches!(role, "textbox" | "searchbox" | "entry"));
        editable && words.iter().any(|word| label.contains(word))
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    first.selector.clone()
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
        wakes: crate::state::WakeBus::default(),
    }
}

#[cfg(test)]
mod connection_check_tests {
    use super::*;
    #[test]
    fn recognizes_connection_wall_but_not_cloudflare_footer() {
        for text in [
            "Cloudflare 驗證您是人類",
            "Cloudflare Verify you are human",
            "Dcard 需要確認您的連線是安全的",
        ] {
            assert!(is_connection_check(&BrowserPage {
                ok: true,
                text: text.into(),
                ..Default::default()
            }));
        }
        for text in [
            "Article text. Protected by Cloudflare",
            "Sign in with your password",
            "",
        ] {
            assert!(!is_connection_check(&BrowserPage {
                ok: true,
                text: text.into(),
                ..Default::default()
            }));
        }
        assert!(!is_connection_check(&BrowserPage {
            ok: false,
            text: "Cloudflare Verify you are human".into(),
            ..Default::default()
        }));
    }
}

#[cfg(test)]
mod shell_session_tests {
    use super::*;
    #[test]
    fn terminal_encoding_preserves_utf8_quotes_and_literal_shell_syntax() {
        let text = "中文🙂\nquotes ' \" and literal $HOME `id` $(printf injected) \\ end";
        let command = terminal_command(&format!("printf %s {}", shell_quote(text)));
        assert!(command.is_ascii());
        let result = std::process::Command::new("bash")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, text.as_bytes());
    }

    #[test]
    fn existing_terminal_is_focused_and_poll_does_not_type() {
        let actions = terminal_actions(&json!({}), "session", true, None).unwrap();
        assert_eq!(
            actions,
            vec![ComputerAction::Focus {
                title: "session".into()
            }]
        );
    }
    #[test]
    fn commands_and_interrupts_use_cua_input() {
        let actions = terminal_actions(
            &json!({"command":"echo '中文'"}),
            "session",
            false,
            Some("/tmp/a b"),
        )
        .unwrap();
        assert!(matches!(&actions[0], ComputerAction::Launch { .. }));
        assert!(
            matches!(&actions[1], ComputerAction::Clipboard { text } if text.is_ascii() && text.starts_with("eval $\'cd -- "))
        );
        assert!(matches!(&actions[2], ComputerAction::Key { key, .. } if key == "return"));
        let keys = terminal_actions(&json!({"keys":"C-c"}), "session", true, None).unwrap();
        assert!(matches!(&keys[1], ComputerAction::Key { key, .. } if key == "ctrl+c"));
    }
}

#[cfg(test)]
mod saved_login_tests {
    use super::*;
    // Runs against a disposable desktop with scripts/cua-login-fixture.py on
    // trusted https://localhost:8443. No model/provider or real credentials.
    #[tokio::test]
    #[ignore = "requires CUA_LOGIN_TEST_CONTAINER with a trusted local HTTPS fixture"]
    async fn saved_login_fills_real_cua_fields_without_submitting() {
        fn browser(request: Value) -> BrowserPage {
            use std::io::Write;
            use std::process::{Command, Stdio};
            let container = std::env::var("CUA_LOGIN_TEST_CONTAINER").unwrap();
            let script = "import importlib.machinery,json,sys; a=importlib.machinery.SourceFileLoader('a','/usr/local/bin/lazyboy-cua-adapter-test').load_module(); print(json.dumps(a.api('/browser',json.load(sys.stdin))))";
            let mut child = Command::new("docker")
                .args(["exec", "-i", &container, "python3", "-c", script])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&serde_json::to_vec(&request).unwrap())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "fixture browser request failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
        let page =
            browser(json!({"action":"navigate","url":"https://localhost:8443/","ensure":true}));
        assert!(
            page.text.contains("Saved login verification"),
            "trusted fixture must be visible"
        );
        let filled = fill_login_fields(
            page,
            "localhost",
            "cua@example.test".into(),
            "fixture-only-123".into(),
            |selector, text| async move {
                browser(json!({"action":"type","selector":selector,"text":text,"ensure":false}))
            },
        )
        .await
        .unwrap();
        assert_eq!(filled, ["username", "password"]);
        let page = browser(json!({"action":"snapshot","ensure":false}));
        assert!(
            page.text.contains("Both fields verified"),
            "fixture must verify both exact input values: {}",
            page.text
        );
        assert!(page.text.contains("Not submitted"));
    }

    #[test]
    fn credentials_require_the_exact_https_host() {
        let page = |url: &str| BrowserPage {
            ok: true,
            url: url.into(),
            ..Default::default()
        };
        assert!(login_page_matches(
            &page("https://example.com/login"),
            "example.com"
        ));
        for url in [
            "http://example.com/login",
            "https://example.com.attacker.test",
            "https://example.com@attacker.test",
            "about:blank",
        ] {
            assert!(!login_page_matches(&page(url), "example.com"));
        }
    }
    #[test]
    fn ambiguous_fields_are_never_guessed() {
        let mut page = BrowserPage {
            ok: true,
            elements: vec![UiElement {
                title: "Email".into(),
                role: Some("textbox".into()),
                selector: Some("p1:0".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(login_field(&page, "username").as_deref(), Some("p1:0"));
        assert!(login_field(&page, "password").is_none());
        page.elements.push(page.elements[0].clone());
        assert!(login_field(&page, "username").is_none());
    }
}

#[cfg(test)]
mod observation_text_tests {
    use super::*;
    use lazyboy_control::observation_from_png;

    fn observation(count: usize) -> ComputerObservation {
        let mut observation = observation_from_png(vec![0xFF, 0xD8, 0xFF], 1920, 1080, None, None);
        observation.elements = (1..=count as u32)
            .map(|index| UiElement {
                id: index,
                title: format!("Window {index}"),
                x: index,
                y: index,
                w: 100,
                h: 40,
                kind: Some("window".into()),
                ..UiElement::default()
            })
            .collect();
        observation
    }

    #[test]
    fn lists_each_control_once_and_names_what_was_dropped() {
        let text = observation_text(
            "computer observed",
            &observation(150),
            ScreenChange::Changed,
        );
        // A model clicks by id, so the element array must not ride along with
        // the list: it used to double every observation.
        assert!(!text.contains("\"selector\""));
        assert!(!text.contains("\"elements\""));
        assert_eq!(text.matches("[1]").count(), 1);
        assert!(text.contains("+30 more not listed"));
        // Screen metadata is small and the model needs the frame id.
        assert!(text.contains("\"frameId\""));
        assert!(text.contains("\"width\":1920"));

        let small = observation_text("computer observed", &observation(3), ScreenChange::Changed);
        assert!(!small.contains("more not listed"));
        assert!(small.contains("[3] window \"Window 3\""));
    }

    #[test]
    fn labels_the_three_screen_states_apart() {
        let observed = |change| observation_text("observed", &observation(1), change);
        assert!(observed(ScreenChange::Identical).starts_with("observed (screen unchanged)"));
        assert!(observed(ScreenChange::Similar).starts_with("observed (no visible change)"));
        assert!(observed(ScreenChange::Changed).starts_with("observed\n"));
    }

    #[test]
    fn an_incomplete_sweep_says_a_missing_control_is_not_proof() {
        // `observation_from_png` starts out incomplete, and a window that
        // never answered is not evidence that its buttons are gone.
        let text = observation_text("observed", &observation(1), ScreenChange::Changed);
        assert!(text.contains("Clickable windows (partial: some windows did not report controls"));
        let mut complete = observation(1);
        complete.native_observation_complete = true;
        let complete = observation_text("observed", &complete, ScreenChange::Changed);
        assert!(!complete.contains("partial"));
        assert!(complete.starts_with("observed\nClickable windows:\n"));
    }
}

#[cfg(test)]
mod verdict_tests {
    use super::*;

    fn verdict(decision: ActionDecision, effect: Option<&str>) -> ActionVerdict {
        ActionVerdict {
            decision,
            effect: effect.map(str::to_string),
            verified: None,
            escalation: None,
        }
    }

    #[test]
    fn an_unproven_effect_says_look_first_never_type_again() {
        let note = verdict_note(&verdict(
            ActionDecision::VerifyFreshState,
            Some("unverifiable"),
        ));
        assert!(note.contains("effect not confirmed"));
        assert!(note.contains("never repeat input"));
        assert!(note.contains("driver said: unverifiable"));
    }

    #[test]
    fn a_confirmed_effect_forbids_repeating_the_action() {
        let note = verdict_note(&verdict(ActionDecision::Done, Some("confirmed")));
        assert!(note.starts_with("verdict: effect confirmed"));
        assert!(note.contains("Do not repeat"));
    }

    #[test]
    fn the_verdict_is_the_first_line_and_invents_no_reason() {
        let mut outcome = text_outcome("completed 1 computer action(s)");
        with_verdict(
            &mut outcome,
            Some(verdict_note(&verdict(ActionDecision::Escalate, None))),
        );
        assert!(
            outcome
                .text
                .starts_with("verdict: the driver reports no effect")
        );
        assert!(outcome.text.ends_with("completed 1 computer action(s)"));
        // The driver gave no `effect` field, so nothing may claim one.
        assert!(!outcome.text.contains("driver said"));
    }
}

/// The tool schema rides in front of every request, so its bytes are the
/// prompt cache key. Collapsing the tools into one `action`-tagged schema
/// (hermes-style) would strand the `tool_calls` stored in existing
/// checkpoints; keeping this serialization byte-stable is the half of the
/// cache win that costs nothing.
#[cfg(test)]
mod tool_schema_tests {
    use super::*;

    fn schema(memory_enabled: bool) -> String {
        serde_json::to_string(&tool_definitions(memory_enabled)).expect("tool schema is json")
    }

    #[test]
    fn the_tool_schema_is_byte_stable_across_calls() {
        for memory_enabled in [false, true] {
            assert_eq!(schema(memory_enabled), schema(memory_enabled));
        }
    }

    #[test]
    fn tool_names_are_unique_and_open_with_the_computer_pair() {
        let tools = tool_definitions(true);
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len());
        assert_eq!(&names[..2], ["computer_observe", "computer_act"]);
    }

    #[test]
    fn memory_tools_are_appended_so_the_desktop_schema_never_moves() {
        let desktop = |memory_enabled: bool| -> Vec<String> {
            tool_definitions(memory_enabled)
                .iter()
                .filter(|tool| {
                    !matches!(
                        tool.name.as_str(),
                        "remember" | "recall_memory" | "forget_memory"
                    )
                })
                .map(|tool| serde_json::to_string(tool).expect("tool schema is json"))
                .collect()
        };
        assert_eq!(desktop(false), desktop(true));
    }
}
