use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::state::AppState;

const COOKIE_NAME: &str = "lazyboy_session";

#[derive(Clone)]
pub struct AuthConfig {
    token: Option<String>,
    sessions: Arc<Mutex<HashMap<String, Instant>>>,
    secure_cookie: bool,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let token = std::env::var("LAZYBOY_APP_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let secure_cookie = std::env::var("LAZYBOY_SECURE_COOKIE")
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        Self {
            token,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            secure_cookie,
        }
    }

    pub fn enabled(&self) -> bool {
        self.token.is_some()
    }

    pub fn strong_enough_for_network(&self) -> bool {
        self.token
            .as_ref()
            .map(|token| token.len() >= 32 && token != "dev-token")
            .unwrap_or(false)
    }

    fn valid_token(&self, supplied: &str) -> bool {
        self.token
            .as_ref()
            .map(|expected| constant_time_eq(expected.as_bytes(), supplied.as_bytes()))
            .unwrap_or(true)
    }

    fn valid_session(&self, headers: &HeaderMap) -> bool {
        if !self.enabled() {
            return true;
        }
        let Some(value) = cookie_value(headers, COOKIE_NAME) else {
            return false;
        };
        let key = hex::encode(Sha256::digest(value.as_bytes()));
        self.sessions
            .lock()
            .unwrap()
            .get(&key)
            .is_some_and(|expires| *expires > Instant::now())
    }

    fn session_cookie(&self) -> Option<String> {
        if !self.enabled() {
            return None;
        }
        let value = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let key = hex::encode(Sha256::digest(value.as_bytes()));
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, expires| *expires > Instant::now());
        if sessions.len() >= 4096
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, t)| **t)
                .map(|(k, _)| k.clone())
        {
            sessions.remove(&oldest);
        }
        sessions.insert(key, Instant::now() + Duration::from_secs(604800));
        Some(format!(
            "{COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age=604800{}",
            if self.secure_cookie { "; Secure" } else { "" }
        ))
    }

    fn revoke_session(&self, headers: &HeaderMap) {
        if let Some(value) = cookie_value(headers, COOKIE_NAME) {
            self.sessions
                .lock()
                .unwrap()
                .remove(&hex::encode(Sha256::digest(value.as_bytes())));
        }
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

#[derive(Deserialize)]
struct LoginInput {
    token: String,
}

pub fn public_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/session",
            axum::routing::get(session).post(login).delete(logout),
        )
        .with_state(state)
}

async fn session(State(state): State<AppState>, headers: HeaderMap) -> Json<serde_json::Value> {
    Json(json!({
        "authenticated": state.auth.valid_session(&headers),
        "required": state.auth.enabled()
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginInput>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    if !state.auth.valid_token(&input.token) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"message": "存取 token 不正確"})),
        ));
    }
    let mut response = Json(json!({"ok": true})).into_response();
    if let Some(cookie) = state.auth.session_cookie() {
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"message": "無法建立 session"})),
                )
            })?,
        );
    }
    Ok(response)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    state.auth.revoke_session(&headers);
    let mut response = Json(json!({"ok": true})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("lazyboy_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
    );
    response
}

pub async fn require_auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if state.auth.valid_session(request.headers()) {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"message": "請先登入"})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn token_comparison_requires_exact_value() {
        assert!(constant_time_eq(b"correct", b"correct"));
        assert!(!constant_time_eq(b"correct", b"wrong"));
        assert!(!constant_time_eq(b"correct", b"correct-longer"));
    }
}

/// SameSite cookies do not replace Origin checks (including WebSockets).
pub async fn browser_boundary(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !allowed_browser_request(request.headers(), state.auth.enabled()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"message":"cross-origin request rejected"})),
        )
            .into_response();
    }
    let private =
        request.uri().path().starts_with("/api/") || request.uri().path().starts_with("/view/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    if private {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

fn allowed_browser_request(headers: &HeaderMap, authenticated_mode: bool) -> bool {
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return false;
    }
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Ok(destination) = reqwest::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    if !authenticated_mode
        && !matches!(
            destination.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]")
        )
    {
        return false;
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let Ok(origin) = origin
            .to_str()
            .ok()
            .and_then(|s| reqwest::Url::parse(s).ok())
            .ok_or(())
        else {
            return false;
        };
        let Ok(expected) = reqwest::Url::parse(&format!("{}://{host}", origin.scheme())) else {
            return false;
        };
        if !matches!(origin.scheme(), "http" | "https") || origin.origin() != expected.origin() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn blocks_cross_origin_and_dns_rebinding() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "localhost:3101".parse().unwrap());
        assert!(allowed_browser_request(&headers, false));
        headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!allowed_browser_request(&headers, true));
        headers.insert(header::ORIGIN, "http://localhost:3101".parse().unwrap());
        assert!(allowed_browser_request(&headers, true));
        headers.remove(header::ORIGIN);
        headers.insert(header::HOST, "rebinding.example".parse().unwrap());
        assert!(!allowed_browser_request(&headers, false));
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    #[test]
    fn sessions_are_distinct_expire_and_are_revoked_on_logout() {
        let config = AuthConfig {
            token: Some("secret".into()),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            secure_cookie: true,
        };
        let first = config.session_cookie().unwrap();
        let second = config.session_cookie().unwrap();
        assert_ne!(first, second);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            first.split(';').next().unwrap().parse().unwrap(),
        );
        assert!(config.valid_session(&headers));
        config.revoke_session(&headers);
        assert!(!config.valid_session(&headers));
        headers.insert(
            header::COOKIE,
            second.split(';').next().unwrap().parse().unwrap(),
        );
        assert!(config.valid_session(&headers));
        for expiry in config.sessions.lock().unwrap().values_mut() {
            *expiry = Instant::now() - Duration::from_secs(1);
        }
        assert!(!config.valid_session(&headers));
    }
}
