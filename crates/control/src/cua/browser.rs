use lazyboy_contracts::UiElement;
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use super::ListedWindow;
use super::client::CuaClient;
use crate::controller::ControlError;
use crate::process::spawn_detached;
use crate::{BrowserRequest, CdpPage, launch_argv_on};

pub const SESSION: &str = "lazyboy";

#[derive(Debug, Clone)]
pub struct BrowserBind {
    pub pid: u64,
    pub window_id: u64,
    pub target_id: String,
    pub tab_id: String,
    pub page: Option<CdpPage>,
}

pub fn is_cua_ref(selector: &str) -> bool {
    let mut parts = selector.split(':');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(prefix), Some(index), None)
            if prefix.starts_with('p')
                && prefix[1..].chars().all(|ch| ch.is_ascii_digit())
                && !prefix[1..].is_empty()
                && index.chars().all(|ch| ch.is_ascii_digit())
                && !index.is_empty()
    )
}

pub fn allowed_navigate_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("about:")
}

pub fn page_from_semantic(value: &Value) -> CdpPage {
    let page = value.get("page").unwrap_or(value);
    let url = page
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let title = page
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let outline = value.get("outline").and_then(Value::as_str).unwrap_or("");
    let mut elements = Vec::new();
    for (index, item) in value
        .get("refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(element) = element_from_ref(index as u32 + 1, item) else {
            continue;
        };
        elements.push(element);
    }
    let ok = value.get("status").and_then(Value::as_str) != Some("refused")
        && value.get("ok").and_then(Value::as_bool) != Some(false);
    CdpPage {
        ok,
        error: if ok {
            None
        } else {
            value
                .get("message")
                .or_else(|| value.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        },
        url,
        title,
        text: outline.to_string(),
        restarted: false,
        waited_seconds: None,
        elements,
    }
}

fn element_from_ref(id: u32, item: &Value) -> Option<UiElement> {
    let selector = item.get("ref").and_then(Value::as_str)?.to_string();
    if selector.is_empty() {
        return None;
    }
    let name = item
        .get("name")
        .or_else(|| item.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let role = item
        .get("role")
        .and_then(Value::as_str)
        .filter(|role| !role.is_empty())
        .map(str::to_string);
    let visibility = item
        .get("visibility")
        .and_then(Value::as_str)
        .unwrap_or("in_viewport");
    let (x, y, w, h) = match item.get("frame") {
        Some(frame) if frame.is_object() => (
            number(frame, "x").unwrap_or(0),
            number(frame, "y").unwrap_or(0),
            number(frame, "w")
                .or_else(|| number(frame, "width"))
                .unwrap_or(0),
            number(frame, "h")
                .or_else(|| number(frame, "height"))
                .unwrap_or(0),
        ),
        _ if visibility == "in_viewport" => (0, 0, 1, 1),
        _ => (0, 0, 0, 0),
    };
    Some(UiElement {
        id,
        title: if name.is_empty() {
            selector.clone()
        } else {
            name
        },
        x,
        y,
        w,
        h,
        selector: Some(selector),
        kind: Some("dom".into()),
        role,
    })
}

fn number(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_f64)
                .filter(|n| *n >= 0.0)
                .map(|n| n as u64)
        })
        .map(|n| n as u32)
}

pub fn find_ref<'a>(page: &'a CdpPage, selector: &'a str) -> Option<&'a str> {
    if is_cua_ref(selector) {
        return page
            .elements
            .iter()
            .find(|element| element.selector.as_deref() == Some(selector))
            .and_then(|element| element.selector.as_deref());
    }
    if let Ok(id) = selector.parse::<u32>() {
        return page
            .elements
            .iter()
            .find(|element| element.id == id)
            .and_then(|element| element.selector.as_deref());
    }
    // Labels are accepted only when exact and unique; do not reinterpret CSS
    // fragments as substring matches that can click a different control.
    if selector.trim().is_empty() {
        return None;
    }
    let mut matches = page
        .elements
        .iter()
        .filter(|element| element.title == selector);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    first.selector.as_deref()
}

pub fn chromium_window(windows: &[ListedWindow]) -> Option<&ListedWindow> {
    windows.iter().find(|window| {
        let blob = format!("{} {}", window.title, window.app_name).to_ascii_lowercase();
        blob.contains("chrom")
    })
}

fn ids_from(value: &Value) -> Option<(String, String)> {
    let mut target_id = None;
    let mut tab_id = None;
    let mut nodes = Vec::new();
    super::client::walk(value, &mut nodes);
    for node in nodes {
        if target_id.is_none() {
            target_id = node
                .get("target_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if tab_id.is_none() {
            tab_id = node
                .get("tab_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(tabs) = node.get("tabs").and_then(Value::as_array)
            && let Some(first) = tabs.first()
            && tab_id.is_none()
        {
            tab_id = first
                .get("tab_id")
                .or_else(|| first.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }
    Some((target_id?, tab_id?))
}

pub async fn ensure_bind(
    client: &CuaClient,
    display: &str,
    profile: Option<&str>,
    ensure: bool,
    windows: &[ListedWindow],
) -> Result<BrowserBind, ControlError> {
    let mut listed = windows.to_vec();
    if chromium_window(&listed).is_none() && ensure {
        if let Some(argv) = launch_argv_on(display, profile, "browser", None) {
            spawn_detached(&argv)
                .await
                .map_err(ControlError::internal)?;
        }
        for _ in 0..24 {
            sleep(Duration::from_millis(250)).await;
            listed = super::CuaController::list_windows_now(client, display).await?;
            if chromium_window(&listed).is_some() {
                break;
            }
        }
    }
    let window = chromium_window(&listed)
        .cloned()
        .ok_or(ControlError::BrowserUnavailable)?;
    attach(client, display, &window).await
}

async fn attach(
    client: &CuaClient,
    display: &str,
    window: &ListedWindow,
) -> Result<BrowserBind, ControlError> {
    client
        .call(
            display,
            "start_session",
            &json!({ "session": SESSION }),
            &[],
        )
        .await?;
    let pid = window.pid;
    let window_id = window.id;
    let prepare = client
        .call(
            display,
            "browser_prepare",
            &json!({
                "pid": pid,
                "window_id": window_id,
                "session": SESSION,
                "strategy": { "kind": "existing_profile" },
                "allow_launch": false,
            }),
            &[],
        )
        .await;
    if let Err(error) = &prepare {
        let text = error.to_string();
        if !text.contains("consent") && !text.contains("prepare") && !text.contains("grant") {
            tracing::warn!(error = %error, "browser_prepare failed");
        }
    }
    let state = client
        .call(
            display,
            "get_browser_state",
            &json!({
                "pid": pid,
                "window_id": window_id,
                "session": SESSION,
                "include_screenshot": false,
            }),
            &[],
        )
        .await?;
    let (target_id, tab_id) = ids_from(&state).ok_or(ControlError::BrowserUnavailable)?;
    Ok(BrowserBind {
        pid,
        window_id,
        target_id,
        tab_id,
        page: None,
    })
}

pub async fn snapshot(
    client: &CuaClient,
    display: &str,
    bind: &BrowserBind,
) -> Result<CdpPage, ControlError> {
    let value = client
        .call(
            display,
            "get_browser_state",
            &json!({
                "target_id": bind.target_id,
                "tab_id": bind.tab_id,
                "session": SESSION,
                "snapshot_format": "semantic_v2",
                "include_screenshot": false,
            }),
            &[],
        )
        .await?;
    Ok(page_from_semantic(&value))
}

pub async fn run(
    client: &CuaClient,
    display: &str,
    profile: Option<&str>,
    request: &BrowserRequest,
    windows: &[ListedWindow],
    bind: &mut Option<BrowserBind>,
) -> Result<CdpPage, ControlError> {
    if request.action == "probe" {
        return Ok(CdpPage {
            ok: chromium_window(windows).is_some(),
            ..CdpPage::default()
        });
    }
    let attached = match bind.as_ref() {
        Some(current) => current.clone(),
        None => {
            let attached = ensure_bind(client, display, profile, request.ensure, windows).await?;
            *bind = Some(attached.clone());
            attached
        }
    };
    if request.action == "ensure" {
        return Ok(CdpPage {
            ok: true,
            ..CdpPage::default()
        });
    }
    match request.action.as_str() {
        "snapshot" => snapshot(client, display, &attached).await,
        "wait" => {
            let ms = request.ms.unwrap_or(400).min(5000);
            sleep(Duration::from_millis(ms)).await;
            snapshot(client, display, &attached).await
        }
        "navigate" => {
            let url = request.url.as_deref().unwrap_or("");
            if !allowed_navigate_url(url) {
                return Err(ControlError::InvalidAction(
                    "browser navigate only accepts http, https, or about URLs".into(),
                ));
            }
            client
                .call(
                    display,
                    "browser_navigate",
                    &json!({
                        "target_id": attached.target_id,
                        "tab_id": attached.tab_id,
                        "session": SESSION,
                        "url": url,
                    }),
                    &[],
                )
                .await?;
            sleep(Duration::from_millis(800)).await;
            snapshot(client, display, &attached).await
        }
        "click" => click(client, display, &attached, request).await,
        "type" => type_into(client, display, &attached, request).await,
        "press" => {
            let key = map_press_key(request.key.as_deref().unwrap_or("return"));
            client
                .call(
                    display,
                    "press_key",
                    &json!({
                        "key": key,
                        "pid": attached.pid,
                        "window_id": attached.window_id,
                        "session": SESSION,
                    }),
                    &[],
                )
                .await?;
            sleep(Duration::from_millis(200)).await;
            snapshot(client, display, &attached).await
        }
        other => Err(ControlError::InvalidAction(format!(
            "unsupported browser action {other}"
        ))),
    }
}

async fn click(
    client: &CuaClient,
    display: &str,
    bind: &BrowserBind,
    request: &BrowserRequest,
) -> Result<CdpPage, ControlError> {
    let selector = request
        .selector
        .as_deref()
        .ok_or_else(|| ControlError::InvalidAction("browser click needs a selector".into()))?;
    let page = bind.page.as_ref().ok_or(ControlError::StaleReference)?;
    let Some(r#ref) = find_ref(&page, selector) else {
        return Ok(CdpPage {
                ok: false,
                error: Some(
                    "element gone: the page changed and ids were renumbered. Use the fresh element list in this result."
                        .into(),
                ),
                url: page.url.clone(),
                title: page.title.clone(),
                text: page.text.clone(),
                elements: page.elements.clone(),
                ..CdpPage::default()
            });
    };
    let r#ref = r#ref.to_string();
    client
        .call(
            display,
            "browser_click",
            &json!({
                "target_id": bind.target_id,
                "tab_id": bind.tab_id,
                "session": SESSION,
                "ref": r#ref,
                "input_route": "dom_event",
            }),
            &[],
        )
        .await?;
    sleep(Duration::from_millis(250)).await;
    snapshot(client, display, bind).await
}

async fn type_into(
    client: &CuaClient,
    display: &str,
    bind: &BrowserBind,
    request: &BrowserRequest,
) -> Result<CdpPage, ControlError> {
    let text = request.text.clone().unwrap_or_default();
    tracing::info!(backend = "cua", tool = "browser_type", length = text.len());
    if let Some(selector) = request.selector.as_deref() {
        let page = bind.page.as_ref().ok_or(ControlError::StaleReference)?;
        let Some(r#ref) = find_ref(&page, selector) else {
            return Ok(CdpPage {
                ok: false,
                error: Some("target field is unavailable; no text inserted".into()),
                url: page.url.clone(),
                title: page.title.clone(),
                text: page.text.clone(),
                elements: page.elements.clone(),
                ..CdpPage::default()
            });
        };
        let r#ref = r#ref.to_string();
        client
            .call(
                display,
                "browser_type",
                &json!({
                    "target_id": bind.target_id,
                    "tab_id": bind.tab_id,
                    "session": SESSION,
                    "ref": r#ref,
                    "text": text,
                    "replace": true,
                }),
                &[],
            )
            .await?;
    } else if !text.is_empty() {
        client
            .call(
                display,
                "type_text",
                &json!({
                    "text": text,
                    "pid": bind.pid,
                    "window_id": bind.window_id,
                    "session": SESSION,
                }),
                &[],
            )
            .await?;
    }
    sleep(Duration::from_millis(200)).await;
    snapshot(client, display, bind).await
}

fn map_press_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => "return".into(),
        "esc" | "escape" => "escape".into(),
        other => other.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_snapshot_scoped_refs() {
        assert!(is_cua_ref("p1:1"));
        assert!(is_cua_ref("p28:12"));
        assert!(!is_cua_ref("#submit"));
        assert!(!is_cua_ref("button.primary"));
        assert!(!is_cua_ref("p:1"));
    }

    #[test]
    fn semantic_snapshot_becomes_cdp_page() {
        let raw = json!({
            "status": "ok",
            "outline": "- button \"Smoke Click\"\n- textbox \"Smoke Entry\"",
            "page": { "title": "LazyBoy Cua Smoke", "url": "http://127.0.0.1:8765/cua-smoke.html" },
            "content_refs": [{ "ref": "p1:0", "name": "Page heading", "role": "heading" }],
            "refs": [
                { "name": "Smoke Click", "ref": "p1:1", "role": "button", "frame": "main", "visibility": "in_viewport" },
                { "name": "Smoke Entry", "ref": "p1:2", "role": "textbox", "visibility": "in_viewport" }
            ]
        });
        let page = page_from_semantic(&raw);
        assert!(page.ok);
        assert_eq!(page.title, "LazyBoy Cua Smoke");
        assert_eq!(page.elements.len(), 2);
        assert_eq!(page.elements[0].id, 1);
        assert_eq!(page.elements[0].selector.as_deref(), Some("p1:1"));
        assert_eq!(page.elements[0].kind.as_deref(), Some("dom"));
        assert!(!page.elements[0].is_offscreen());
        assert_eq!(find_ref(&page, "1"), Some("p1:1"));
        assert_eq!(find_ref(&page, "p1:1"), Some("p1:1"));
        assert_eq!(find_ref(&page, "p9:1"), None);
        assert_eq!(find_ref(&page, ""), None);
        assert_eq!(find_ref(&page, "#"), None);
        assert_eq!(find_ref(&page, "Smoke Entry"), Some("p1:2"));
    }

    #[test]
    fn navigate_accepts_http_https_about_only() {
        assert!(allowed_navigate_url("https://example.com"));
        assert!(allowed_navigate_url("http://127.0.0.1:8765/cua-smoke.html"));
        assert!(allowed_navigate_url("about:blank"));
        assert!(!allowed_navigate_url("file:///tmp/x.html"));
        assert!(!allowed_navigate_url("javascript:alert(1)"));
        assert!(!allowed_navigate_url(""));
    }

    #[test]
    fn chromium_window_matches_title_or_app() {
        let chrome = ListedWindow {
            id: 9,
            pid: 334,
            title: "LazyBoy Cua Smoke - Chromium".into(),
            app_name: "Chromium".into(),
            x: 0,
            y: 0,
            w: 1280,
            h: 800,
            z: 2,
        };
        let terminal = ListedWindow {
            id: 3,
            pid: 20,
            title: "終端機".into(),
            app_name: "xfce4-terminal".into(),
            x: 10,
            y: 10,
            w: 400,
            h: 300,
            z: 1,
        };
        assert!(chromium_window(&[terminal.clone(), chrome.clone()]).is_some());
        assert!(chromium_window(&[terminal]).is_none());
    }
}
