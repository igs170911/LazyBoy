use std::sync::Mutex;

use lazyboy_contracts::{
    ComputerAction, ComputerMode, ComputerObservation, PointerType, UiElement,
};
use lazyboy_control::{
    ActionError, ActionRequest, AdapterContext, CdpPage, CommandRequest, ComputerRef,
    SandboxProvider, apply_element_targets, cdp_command_on, element_id, format_ui_elements,
    frames_match,
    merge_page_elements, overlay_elements, parse_cdp_page, parse_computer_actions,
    resolve_bot_workspace_cwd, resolve_bot_workspace_path,
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
    pub computer: ComputerRef,
    pub context: AdapterContext,
    pub mode: ComputerMode,
    pub bot_id: String,
    pub vision: bool,
    pub gui_block: Option<String>,
    pub previous_frame: Mutex<Option<String>>,
    pub elements: Mutex<Vec<UiElement>>,
    pub miss_streak: Mutex<u32>,
    pub click_misses: Mutex<u32>,
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
            description: "Capture a fresh desktop screenshot plus numbered targets. When Chromium is open this is the page DOM (click those ids); otherwise native windows. The image attaches only if the screen changed.".into(),
            parameters: json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "computer_act".into(),
            description: "Drive native desktop GUI (dialogs, canvas, XFCE). Prefer element id from the latest observation. For Chromium pages use the browser tool instead of clicking the window. x,y are 1280x800 fallback.".into(),
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
            name: "browser".into(),
            description: "Control Chromium through the page DOM. Prefer this over computer_act for anything in the browser. snapshot returns numbered elements and visible text (no screenshot). click/type/navigate by element id or CSS selector. click scrolls off-screen elements into view and, if the control is [disabled], waits up to 45s (waitMs to change) for it to enable before clicking. Ids are renumbered after every page change. The human still sees the live window.".into(),
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
            description: "Launch browser or terminal on the desktop.".into(),
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
            name: "request_takeover".into(),
            description: "Ask the user to take over for passwords, 2FA, CAPTCHA, login walls, or when the right on-screen control cannot be found. Never ask them to paste secrets in chat.".into(),
            parameters: json!({"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"]}),
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
}

pub async fn dispatch(ctx: &ToolCtx, name: &str, args: &Value) -> ToolOutcome {
    match name {
        "computer_observe" => observe(ctx).await,
        "computer_act" => act(ctx, args).await,
        "wait" => wait_then_observe(ctx, args).await,
        "browser" => browser(ctx, args).await,
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
    }
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
    let label = if observation
        .elements
        .iter()
        .any(|element| element.kind.as_deref() == Some("dom"))
    {
        "Clickable page elements"
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
    match ctx.sandbox.observe(&ctx.computer, &ctx.context).await {
        Ok(observation) => {
            let (observation, note) =
                attach_page_elements(ctx, observation, "computer observed").await;
            pack_observation(ctx, &note, observation)
        }
        Err(error) => ToolOutcome {
            text: error.to_string(),
            image: None,
            pause: false,
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
    match ctx.sandbox.observe(&ctx.computer, &ctx.context).await {
        Ok(observation) => {
            let note = format!("waited {seconds:.0}s");
            let (observation, note) = attach_page_elements(ctx, observation, &note).await;
            pack_observation(ctx, &note, observation)
        }
        Err(error) => text_outcome(format!("waited {seconds:.0}s; observe failed: {error}")),
    }
}

async fn attach_page_elements(
    ctx: &ToolCtx,
    mut observation: ComputerObservation,
    note: &str,
) -> (ComputerObservation, String) {
    let Some(page) = cdp_snapshot(ctx, false).await else {
        return (observation, note.to_string());
    };
    observation.elements = merge_page_elements(observation.elements, &page.elements);
    let mut note = note.to_string();
    if !page.url.is_empty() || !page.title.is_empty() {
        note.push_str(&format!("\nPage: {} {}", page.title, page.url));
    }
    if !page.text.is_empty() {
        note.push_str("\nVisible text:\n");
        note.push_str(&page.text);
    }
    (observation, note)
}

async fn cdp_snapshot(ctx: &ToolCtx, ensure: bool) -> Option<CdpPage> {
    let page = cdp_call(ctx, json!({"action": "snapshot", "ensure": ensure})).await;
    if page.ok { Some(page) } else { None }
}

async fn cdp_call(ctx: &ToolCtx, request: Value) -> CdpPage {
    let display = ctx.context.display.as_deref().unwrap_or(":1");
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
    let argv = cdp_command_on(display, ctx.context.profile_path.as_deref(), &request);
    match ctx
        .sandbox
        .execute(
            &ctx.computer,
            CommandRequest {
                argv,
                cwd: None,
                timeout_ms: Some(timeout_ms),
            },
            &ctx.context,
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
    if request.get("selector").and_then(Value::as_str).is_none() {
        if let Some(id) = element_id(args.get("element").or_else(|| args.get("id"))) {
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
        };
    }
    match ctx.sandbox.observe(&ctx.computer, &ctx.context).await {
        Ok(observation) => {
            let (observation, note) = attach_page_elements(ctx, observation, &text).await;
            pack_observation(ctx, &note, observation)
        }
        Err(_) => ToolOutcome {
            text,
            image: None,
            pause: false,
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
    if let Some(actions) = args.get_mut("actions") {
        if let Err(error) = apply_element_targets(actions, &elements) {
            if let ActionError::UnknownElement(id) = error {
                return pause_unknown_element(ctx, id, &elements);
            }
            return text_outcome(error.to_string());
        }
        if let Some(items) = actions.as_array_mut() {
            for item in items {
                let Some(kind) = item.get("kind").and_then(Value::as_str) else {
                    continue;
                };
                if kind != "click" {
                    continue;
                }
                let Some(id) = element_id(item.get("element")) else {
                    continue;
                };
                let Some(selector) = elements
                    .iter()
                    .find(|element| u64::from(element.id) == id)
                    .and_then(|element| element.selector.clone())
                else {
                    continue;
                };
                let page = cdp_call(
                    ctx,
                    json!({"action":"click","selector":selector,"ensure":false}),
                )
                .await;
                if page.ok {
                    if let Some(object) = item.as_object_mut() {
                        object.insert("kind".into(), json!("wait"));
                        object.insert("ms".into(), json!(80));
                    }
                }
            }
        }
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
    let had_click = actions.iter().any(action_is_click);
    match ctx
        .sandbox
        .act(
            &ctx.computer,
            ActionRequest {
                actions,
                observe: args.get("observe").and_then(Value::as_bool) != Some(false),
                settle_ms: args.get("settle_ms").and_then(Value::as_u64).unwrap_or(350) as u32,
                display: ctx.context.display.clone(),
                profile_path: ctx.context.profile_path.clone(),
            },
            &ctx.context,
        )
        .await
    {
        Ok(result) => {
            if let Some(observation) = result.observation {
                let unchanged =
                    frames_match(ctx.previous_frame.lock().unwrap().as_deref(), &observation);
                let mut outcome = pack_observation(
                    ctx,
                    &format!("completed {} computer action(s)", result.completed),
                    observation,
                );
                note_click_result(ctx, had_click, unchanged, &mut outcome);
                outcome
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

fn action_is_click(action: &ComputerAction) -> bool {
    matches!(
        action,
        ComputerAction::Pointer {
            pointer_type: PointerType::Click | PointerType::Down,
            ..
        }
    )
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
    match ctx
        .sandbox
        .list_files(&ctx.computer, &stored, &ctx.context)
        .await
    {
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
    match ctx
        .sandbox
        .read_file(&ctx.computer, &stored, &ctx.context)
        .await
    {
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
    let application = args
        .get("application")
        .and_then(Value::as_str)
        .unwrap_or("browser");
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
