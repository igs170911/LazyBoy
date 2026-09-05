use axum::extract::ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::computer;
use crate::state::AppState;

pub async fn view_root(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
    req: Request,
) -> Response {
    proxy(state, bot_id, String::new(), req).await
}

pub async fn view_path(
    State(state): State<AppState>,
    Path((bot_id, rest)): Path<(String, String)>,
    req: Request,
) -> Response {
    proxy(state, bot_id, rest, req).await
}

async fn proxy(state: AppState, bot_id: String, rest: String, req: Request) -> Response {
    let upgrade = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    if !upgrade {
        if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        if is_viewer_page(&rest) {
            return viewer_page().await;
        }
        return trusted_asset(&rest).await;
    }
    if rest != "websockify" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let ensure = upgrade || is_viewer_page(&rest) || rest.contains("websockify");
    let port = match upstream_port(&state, &bot_id, ensure).await {
        Ok(port) => port,
        Err(status) => return status.into_response(),
    };
    if upgrade {
        return match WebSocketUpgrade::from_request(req, &state).await {
            Ok(ws) => ws
                .on_upgrade(move |socket| proxy_socket(socket, port, rest))
                .into_response(),
            Err(error) => error.into_response(),
        };
    }
    if is_viewer_page(&rest) {
        return viewer_page().await;
    }
    StatusCode::NOT_FOUND.into_response()
}

fn is_viewer_page(rest: &str) -> bool {
    rest.is_empty()
        || rest == "vnc_lite.html"
        || rest == "vnc.html"
        || rest == "index.html"
        || rest == "embed.html"
}

async fn viewer_page() -> Response {
    let dir = std::env::var("LAZYBOY_WEB_DIR").unwrap_or_else(|_| "apps/web".into());
    let path = std::path::Path::new(&dir).join("vnc.html");
    let html = match tokio::fs::read_to_string(&path).await {
        Ok(body) => body,
        Err(_) => include_str!("../../../apps/web/vnc.html").to_string(),
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    (StatusCode::OK, headers, html).into_response()
}

async fn upstream_port(state: &AppState, bot_id: &str, ensure: bool) -> Result<u16, StatusCode> {
    let actor = state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bot = state
        .db
        .get_bot(&actor, bot_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let computer = state
        .db
        .get_computer(bot.computer_id.as_deref().unwrap_or(""))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let computer_ref = computer::computer_ref(&computer).ok_or(StatusCode::NOT_FOUND)?;
    let screen = if ensure {
        match computer::ensure_bot_screen(state, &actor, bot_id, &computer, None).await {
            Ok(bound) => bound.row,
            Err(_) => state
                .db
                .get_screen(&computer.id, bot_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        }
    } else {
        state
            .db
            .get_screen(&computer.id, bot_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    let session = state
        .sandbox
        .connect_screen(
            &computer_ref,
            computer::user_has_screen_control(&computer, screen.as_ref(), bot_id),
            &computer::adapter_context_for(&actor, bot_id, "view", screen.as_ref(), None),
        )
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let url = session.url.ok_or(StatusCode::NOT_FOUND)?;
    let rewritten = rewrite_upstream(&url);
    let uri: Uri = rewritten.parse().map_err(|_| StatusCode::BAD_GATEWAY)?;
    uri.port_u16().ok_or(StatusCode::BAD_GATEWAY)
}

fn rewrite_upstream(url: &str) -> String {
    let host = std::env::var("LAZYBOY_SCREEN_UPSTREAM").unwrap_or_else(|_| "127.0.0.1".into());
    url.replace("127.0.0.1", &host).replace("localhost", &host)
}

fn safe_asset(rest: &str) -> bool {
    (rest.starts_with("core/") || rest.starts_with("vendor/"))
        && rest.ends_with(".js")
        && rest
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
}

async fn trusted_asset(rest: &str) -> Response {
    if !safe_asset(rest) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = std::env::var("LAZYBOY_NOVNC_DIR").unwrap_or_else(|_| {
        let web = std::env::var("LAZYBOY_WEB_DIR").unwrap_or_else(|_| "apps/web".into());
        if std::path::Path::new(&web).join("novnc").is_dir() {
            format!("{web}/novnc")
        } else {
            "apps/web/node_modules/@novnc/novnc".into()
        }
    });
    match tokio::fs::read(std::path::Path::new(&root).join(rest)).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn proxy_socket(mut client: WebSocket, port: u16, rest: String) {
    let host = std::env::var("LAZYBOY_SCREEN_UPSTREAM").unwrap_or_else(|_| "127.0.0.1".into());
    let path = if rest.is_empty() {
        "websockify".into()
    } else {
        rest
    };
    let url = format!("ws://{host}:{port}/{path}");
    let Ok((upstream, _)) = tokio_tungstenite::connect_async(url).await else {
        let _ = client.send(AxumMessage::Close(None)).await;
        return;
    };
    let (mut up_write, mut up_read) = upstream.split();
    loop {
        tokio::select! {
            incoming = client.recv() => {
                match incoming {
                    Some(Ok(AxumMessage::Binary(data))) => {
                        if up_write.send(WsMessage::Binary(data)).await.is_err() { break; }
                    }
                    Some(Ok(AxumMessage::Text(text))) => {
                        if up_write.send(WsMessage::Text(text.to_string().into())).await.is_err() { break; }
                    }
                    Some(Ok(AxumMessage::Ping(data))) => {
                        if up_write.send(WsMessage::Ping(data)).await.is_err() { break; }
                    }
                    Some(Ok(AxumMessage::Pong(data))) => {
                        if up_write.send(WsMessage::Pong(data)).await.is_err() { break; }
                    }
                    Some(Ok(AxumMessage::Close(_))) | None => break,
                    Some(Err(_)) => break,
                }
            }
            incoming = up_read.next() => {
                match incoming {
                    Some(Ok(WsMessage::Binary(data))) => {
                        if client.send(AxumMessage::Binary(data)).await.is_err() { break; }
                    }
                    Some(Ok(WsMessage::Text(text))) => {
                        if client.send(AxumMessage::Text(text.to_string().into())).await.is_err() { break; }
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        if client.send(AxumMessage::Ping(data.into())).await.is_err() { break; }
                    }
                    Some(Ok(WsMessage::Pong(data))) => {
                        if client.send(AxumMessage::Pong(data.into())).await.is_err() { break; }
                    }
                    Some(Ok(WsMessage::Close(_))) | Some(Ok(WsMessage::Frame(_))) | None => break,
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod asset_tests {
    use super::*;
    #[test]
    fn only_trusted_novnc_scripts_are_served() {
        assert!(safe_asset("core/rfb.js"));
        for path in [
            "../secret.js",
            "core/../../secret.js",
            "core/%2e%2e/x.js",
            "evil.html",
            "/core/rfb.js",
        ] {
            assert!(!safe_asset(path));
        }
    }
}
