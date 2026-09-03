use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::state::AppState;

const COOKIE_NAME: &str = "lazyboy_session";

#[derive(Clone)]
pub struct AuthConfig {
    token: Option<String>,
    session_value: Option<String>,
    secure_cookie: bool,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let token = std::env::var("LAZYBOY_APP_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let session_value = token.as_ref().map(|value| {
            let mut hasher = Sha256::new();
            hasher.update(b"lazyboy-session-v1:");
            hasher.update(value.as_bytes());
            hex::encode(hasher.finalize())
        });
        let secure_cookie = std::env::var("LAZYBOY_SECURE_COOKIE")
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        Self {
            token,
            session_value,
            secure_cookie,
        }
    }

    pub fn enabled(&self) -> bool {
        self.token.is_some()
    }

    pub fn strong_enough_for_network(&self) -> bool {
        self.token
            .as_ref()
            .map(|token| token.as_bytes().len() >= 32 && token != "dev-token")
            .unwrap_or(false)
    }

    fn valid_token(&self, supplied: &str) -> bool {
        self.token
            .as_ref()
            .map(|expected| constant_time_eq(expected.as_bytes(), supplied.as_bytes()))
            .unwrap_or(true)
    }

    fn valid_session(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = &self.session_value else {
            return true;
        };
        cookie_value(headers, COOKIE_NAME)
            .map(|supplied| constant_time_eq(expected.as_bytes(), supplied.as_bytes()))
            .unwrap_or(false)
    }

    fn session_cookie(&self) -> Option<String> {
        self.session_value.as_ref().map(|value| {
            format!(
                "{COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age=604800{}",
                if self.secure_cookie { "; Secure" } else { "" }
            )
        })
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
        .route("/api/session", axum::routing::get(session).post(login).delete(logout))
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

async fn logout() -> Response {
    let mut response = Json(json!({"ok": true})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "lazyboy_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        ),
    );
    response
}

pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
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
