//! Per-bot login vault. Humans store credentials; the model only sees ids.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use crate::db::Actor;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VaultAccount {
    pub id: String,
    pub bot_id: String,
    pub site: String,
    pub host: String,
    pub username: String,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertAccount {
    pub site: String,
    #[serde(default)]
    pub host: String,
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub notes: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/bots/{bot_id}/accounts",
            get(list_accounts).post(create_account),
        )
        .route(
            "/api/bots/{bot_id}/accounts/{account_id}",
            patch(update_account).delete(delete_account),
        )
}

async fn scoped_actor(state: &AppState, bot_id: &str) -> Result<Actor, StatusCode> {
    let actor = state
        .bootstrap()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .db
        .get_bot(&actor, bot_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(actor)
}

async fn list_accounts(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
) -> Result<Json<Vec<VaultAccount>>, StatusCode> {
    let actor = scoped_actor(&state, &bot_id).await?;
    list(&state, &actor, &bot_id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_account(
    State(state): State<AppState>,
    Path(bot_id): Path<String>,
    Json(input): Json<UpsertAccount>,
) -> Result<Json<VaultAccount>, (StatusCode, Json<Value>)> {
    let actor = scoped_actor(&state, &bot_id)
        .await
        .map_err(|status| (status, Json(json!({"message":"bot not found"}))))?;
    insert(&state, &actor, &bot_id, input)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message": error}))))
}

async fn update_account(
    State(state): State<AppState>,
    Path((bot_id, account_id)): Path<(String, String)>,
    Json(input): Json<UpsertAccount>,
) -> Result<Json<VaultAccount>, (StatusCode, Json<Value>)> {
    let actor = scoped_actor(&state, &bot_id)
        .await
        .map_err(|status| (status, Json(json!({"message":"bot not found"}))))?;
    update_row(&state, &actor, &bot_id, &account_id, input)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"message": error}))))?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"message":"account not found"})),
        ))
}

async fn delete_account(
    State(state): State<AppState>,
    Path((bot_id, account_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let actor = scoped_actor(&state, &bot_id).await?;
    let deleted = sqlx::query(
        "DELETE FROM vault_accounts
         WHERE id=$1 AND bot_id=$2 AND space_id=$3 AND user_id=$4",
    )
    .bind(&account_id)
    .bind(&bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .execute(state.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if deleted.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<Vec<VaultAccount>, String> {
    list_on(state.pool(), actor, bot_id).await
}

pub async fn list_on(
    pool: &sqlx::PgPool,
    actor: &Actor,
    bot_id: &str,
) -> Result<Vec<VaultAccount>, String> {
    sqlx::query_as(
        "SELECT id, bot_id, site, host, username, notes, created_at, updated_at
         FROM vault_accounts
         WHERE bot_id=$1 AND space_id=$2 AND user_id=$3
         ORDER BY updated_at DESC",
    )
    .bind(bot_id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())
}

pub async fn get_secret(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    account_id: &str,
) -> Result<Option<(VaultAccount, String, String)>, String> {
    get_secret_on(state.pool(), actor, bot_id, account_id).await
}

pub async fn get_secret_on(
    pool: &sqlx::PgPool,
    actor: &Actor,
    bot_id: &str,
    account_id: &str,
) -> Result<Option<(VaultAccount, String, String)>, String> {
    let row: Option<(String, String, String, String, String, String, DateTime<Utc>, DateTime<Utc>, String)> =
        sqlx::query_as(
            "SELECT id, bot_id, site, host, username, notes, created_at, updated_at, password_ciphertext
             FROM vault_accounts
             WHERE id=$1 AND bot_id=$2 AND space_id=$3 AND user_id=$4",
        )
        .bind(account_id)
        .bind(bot_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let password = decrypt(&row.8)?;
    Ok(Some((
        VaultAccount {
            id: row.0,
            bot_id: row.1,
            site: row.2,
            host: row.3,
            username: row.4.clone(),
            notes: row.5,
            created_at: row.6,
            updated_at: row.7,
        },
        row.4,
        password,
    )))
}

async fn insert(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    input: UpsertAccount,
) -> Result<VaultAccount, String> {
    let site = clean_site(&input.site)?;
    let username = clean_username(&input.username)?;
    if input.password.is_empty() {
        return Err("password is required".into());
    }
    let host = normalize_host(&input.host, &site);
    let id = Uuid::new_v4().to_string();
    let ciphertext = encrypt(&input.password)?;
    sqlx::query_as(
        "INSERT INTO vault_accounts
            (id, space_id, user_id, bot_id, site, host, username, password_ciphertext, notes)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         RETURNING id, bot_id, site, host, username, notes, created_at, updated_at",
    )
    .bind(&id)
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .bind(bot_id)
    .bind(site)
    .bind(host)
    .bind(username)
    .bind(ciphertext)
    .bind(input.notes.trim())
    .fetch_one(state.pool())
    .await
    .map_err(|error| error.to_string())
}

async fn update_row(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    account_id: &str,
    input: UpsertAccount,
) -> Result<Option<VaultAccount>, String> {
    let site = clean_site(&input.site)?;
    let username = clean_username(&input.username)?;
    let host = normalize_host(&input.host, &site);
    if input.password.is_empty() {
        sqlx::query_as(
            "UPDATE vault_accounts
             SET site=$1, host=$2, username=$3, notes=$4, updated_at=now()
             WHERE id=$5 AND bot_id=$6 AND space_id=$7 AND user_id=$8
             RETURNING id, bot_id, site, host, username, notes, created_at, updated_at",
        )
        .bind(site)
        .bind(host)
        .bind(username)
        .bind(input.notes.trim())
        .bind(account_id)
        .bind(bot_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(state.pool())
        .await
        .map_err(|error| error.to_string())
    } else {
        let ciphertext = encrypt(&input.password)?;
        sqlx::query_as(
            "UPDATE vault_accounts
             SET site=$1, host=$2, username=$3, notes=$4, password_ciphertext=$5, updated_at=now()
             WHERE id=$6 AND bot_id=$7 AND space_id=$8 AND user_id=$9
             RETURNING id, bot_id, site, host, username, notes, created_at, updated_at",
        )
        .bind(site)
        .bind(host)
        .bind(username)
        .bind(input.notes.trim())
        .bind(ciphertext)
        .bind(account_id)
        .bind(bot_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(state.pool())
        .await
        .map_err(|error| error.to_string())
    }
}

fn clean_site(site: &str) -> Result<String, String> {
    let value = site.trim();
    if value.is_empty() || value.chars().count() > 80 {
        return Err("site name must be 1–80 characters".into());
    }
    Ok(value.to_string())
}

fn clean_username(username: &str) -> Result<String, String> {
    let value = username.trim();
    if value.is_empty() || value.chars().count() > 200 {
        return Err("username must be 1–200 characters".into());
    }
    Ok(value.to_string())
}

fn normalize_host(host: &str, site: &str) -> String {
    let raw = host.trim();
    if raw.is_empty() {
        return site.to_lowercase();
    }
    raw.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_lowercase()
}

fn vault_key() -> Result<[u8; 32], String> {
    let material = std::env::var("LAZYBOY_VAULT_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("LAZYBOY_APP_TOKEN").ok().filter(|v| !v.is_empty()))
        .ok_or_else(|| "set LAZYBOY_VAULT_KEY or LAZYBOY_APP_TOKEN to encrypt saved passwords".to_string())?;
    let digest = Sha256::digest(material.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    Ok(key)
}

fn encrypt(plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(&vault_key()?).map_err(|error| error.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut packed = Vec::with_capacity(12 + encrypted.len());
    packed.extend_from_slice(&nonce_bytes);
    packed.extend_from_slice(&encrypted);
    Ok(hex::encode(packed))
}

fn decrypt(packed: &str) -> Result<String, String> {
    let bytes = hex::decode(packed).map_err(|error| error.to_string())?;
    if bytes.len() < 13 {
        return Err("corrupt vault entry".into());
    }
    let cipher = Aes256Gcm::new_from_slice(&vault_key()?).map_err(|error| error.to_string())?;
    let nonce = Nonce::from_slice(&bytes[..12]);
    let plain = cipher
        .decrypt(nonce, &bytes[12..])
        .map_err(|_| "could not decrypt vault entry".to_string())?;
    String::from_utf8(plain).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{decrypt, encrypt, normalize_host};

    #[test]
    fn round_trips_a_password() {
        unsafe { std::env::set_var("LAZYBOY_VAULT_KEY", "test-vault-key-for-unit-tests") };
        let packed = encrypt("s3cret!").unwrap();
        assert!(!packed.contains("s3cret"));
        assert_eq!(decrypt(&packed).unwrap(), "s3cret!");
    }

    #[test]
    fn host_strips_urls() {
        assert_eq!(normalize_host("https://mail.google.com/inbox", "Gmail"), "mail.google.com");
        assert_eq!(normalize_host("", "Gmail"), "gmail");
    }
}
