//! Chat attachments: bytes go to the model once, then we drop them.
//!
//! The chat row only stores name/mime/size. If the bot's computer needs a copy
//! (open a PDF, upload a spreadsheet), we write it under `inbox/` on the bind-
//! mounted home and delete anything older than [`INBOX_TTL`].

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine;
use lazyboy_contracts::SessionAttachment;
use lazyboy_control::resolve_bot_workspace_path;
use rig_core::completion::message::{ImageDetail, ImageMediaType, UserContent};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::fs;

use crate::computer;
use crate::db::{Actor, parse_mode};
use crate::state::AppState;

pub const INBOX_TTL: Duration = Duration::from_secs(2 * 60 * 60);
pub const MAX_COUNT: usize = 4;
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 20 * 1024 * 1024;
const TEXT_INLINE_CHARS: usize = 80_000;
const NAME_MAX: usize = 80;

pub type IncomingAttachment = SessionAttachment;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAttachment {
    pub kind: &'static str,
    pub name: String,
    pub mime_type: String,
    pub size: usize,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct DecodedAttachment {
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub fn decode_incoming(items: &[IncomingAttachment]) -> Result<Vec<DecodedAttachment>, String> {
    if items.len() > MAX_COUNT {
        return Err(format!("最多 {MAX_COUNT} 個附件"));
    }
    let mut out = Vec::with_capacity(items.len());
    let mut total = 0usize;
    for item in items {
        let name = safe_name(&item.name)?;
        let mime = normalize_mime(&item.mime_type, &name);
        if !allowed_mime(&mime) {
            return Err(format!("不支援的檔案類型：{name}"));
        }
        let bytes = decode_base64(&item.content)?;
        if bytes.is_empty() {
            return Err(format!("空檔案：{name}"));
        }
        if bytes.len() > MAX_BYTES {
            return Err(format!("{name} 超過 10 MB"));
        }
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL_BYTES {
            return Err("附件合計超過 20 MB".into());
        }
        out.push(DecodedAttachment {
            name,
            mime_type: mime,
            bytes,
        });
    }
    Ok(out)
}

pub fn stored_blocks(files: &[DecodedAttachment]) -> Vec<Value> {
    files
        .iter()
        .map(|file| {
            json!({
                "kind": if is_image(&file.mime_type) { "image" } else { "file" },
                "name": file.name,
                "mimeType": file.mime_type,
                "size": file.bytes.len(),
                "path": format!("inbox/{}", file.name),
            })
        })
        .collect()
}

pub fn caption_for_title(text: &str, files: &[DecodedAttachment]) -> String {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    match files.first() {
        Some(file) => format!("附件 {}", file.name),
        None => String::new(),
    }
}

/// Write copies into each bot's `inbox/` so the desktop can open them.
pub async fn stage_for_bots(
    state: &AppState,
    actor: &Actor,
    bot_ids: &[String],
    files: &[DecodedAttachment],
) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    for bot_id in bot_ids {
        let Some(dir) = inbox_dir_for_bot(state, actor, bot_id).await? else {
            continue;
        };
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| error.to_string())?;
        for file in files {
            let path = unique_path(&dir, &file.name).await;
            fs::write(&path, &file.bytes)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub async fn llm_parts(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    blocks: &[Value],
    vision: bool,
) -> Vec<UserContent> {
    let files = load_from_blocks(state, actor, bot_id, blocks).await;
    if files.is_empty() && blocks.is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut notes = Vec::new();
    if !blocks.is_empty() {
        notes.push(format!(
            "User attached {} file(s). Chat history does not keep the bytes. Copies live in inbox/ on this computer and expire after 2 hours.",
            blocks.len()
        ));
    }
    for (stored, bytes) in files {
        notes.push(format!(
            "- {} ({}, {} bytes) at inbox/{}",
            stored.name, stored.mime_type, stored.size, stored.name
        ));
        if is_image(&stored.mime_type) {
            if vision {
                if let Some(bytes) = bytes {
                    parts.push(UserContent::image_base64(
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                        Some(image_media(&stored.mime_type)),
                        Some(ImageDetail::High),
                    ));
                } else {
                    notes.push("  (image missing from inbox; it may have expired)".into());
                }
            }
        } else if is_text_mime(&stored.mime_type) {
            if let Some(bytes) = bytes {
                if let Some(text) = utf8_preview(&bytes) {
                    parts.push(UserContent::text(format!(
                        "Contents of {}:\n```\n{text}\n```",
                        stored.name
                    )));
                }
            }
        } else {
            notes.push(
                "  Open this with open_path or the desktop if you need the original file.".into(),
            );
        }
    }
    if !notes.is_empty() {
        parts.insert(0, UserContent::text(notes.join("\n")));
    }
    parts
}

pub async fn sweep_all_inboxes(data_dir: &str) {
    let homes = PathBuf::from(data_dir).join("homes");
    let Ok(mut spaces) = fs::read_dir(&homes).await else {
        return;
    };
    while let Ok(Some(entry)) = spaces.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        sweep_dir(&path.join("inbox")).await;
        let bots = path.join("bots");
        let Ok(mut bots_dir) = fs::read_dir(&bots).await else {
            continue;
        };
        while let Ok(Some(bot)) = bots_dir.next_entry().await {
            sweep_dir(&bot.path().join("inbox")).await;
        }
    }
}

async fn inbox_dir_for_bot(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(bot) = state
        .db
        .get_bot(actor, bot_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(computer_id) = bot.computer_id.as_deref() else {
        return Ok(None);
    };
    let Some(computer) = state
        .db
        .get_computer(computer_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let home = computer::home_path(&state.data_dir, &computer.home_key);
    let relative = resolve_bot_workspace_path(parse_mode(&computer.scope), bot_id, "inbox")
        .unwrap_or_else(|_| "inbox".into());
    Ok(Some(home.join(relative)))
}

async fn load_from_blocks(
    state: &AppState,
    actor: &Actor,
    bot_id: &str,
    blocks: &[Value],
) -> Vec<(StoredAttachment, Option<Vec<u8>>)> {
    let Some(dir) = inbox_dir_for_bot(state, actor, bot_id).await.ok().flatten() else {
        return blocks
            .iter()
            .filter_map(parse_stored)
            .map(|file| (file, None))
            .collect();
    };
    let mut out = Vec::new();
    for block in blocks {
        let Some(file) = parse_stored(block) else {
            continue;
        };
        let path = dir.join(&file.name);
        let bytes = fs::read(&path).await.ok();
        out.push((file, bytes));
    }
    out
}

fn parse_stored(value: &Value) -> Option<StoredAttachment> {
    let kind = value.get("kind").and_then(Value::as_str)?;
    if kind != "file" && kind != "image" {
        return None;
    }
    let name = value.get("name").and_then(Value::as_str)?;
    Some(StoredAttachment {
        kind: if kind == "image" { "image" } else { "file" },
        name: name.to_string(),
        mime_type: value
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string(),
        size: value.get("size").and_then(Value::as_u64).unwrap_or(0) as usize,
        path: value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

async fn sweep_dir(dir: &Path) {
    let Ok(mut entries) = fs::read_dir(dir).await else {
        return;
    };
    let cutoff = SystemTime::now() - INBOX_TTL;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path).await else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < cutoff {
            let _ = fs::remove_file(&path).await;
        }
    }
}

async fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if fs::metadata(&candidate).await.is_err() {
        return candidate;
    }
    let (stem, ext) = split_ext(name);
    for n in 2..1000 {
        let next = dir.join(format!("{stem}-{n}{ext}"));
        if fs::metadata(&next).await.is_err() {
            return next;
        }
    }
    dir.join(format!("{stem}-{}.bin", uuid::Uuid::new_v4()))
}

fn split_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(index) if index > 0 => (name[..index].to_string(), name[index..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

pub fn safe_name(name: &str) -> Result<String, String> {
    let file = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim();
    let mut out = String::new();
    for ch in file.chars() {
        if ch.is_control() || "/\\:".contains(ch) {
            continue;
        }
        out.push(ch);
        if out.chars().count() >= NAME_MAX {
            break;
        }
    }
    let out = out.trim_matches('.').trim().to_string();
    if out.is_empty() || out == "." || out == ".." {
        return Err("檔名無效".into());
    }
    Ok(out)
}

fn decode_base64(value: &str) -> Result<Vec<u8>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("附件內容是空的".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(trimmed.replace('\n', "")))
        .map_err(|_| "附件不是有效的 base64".into())
}

fn normalize_mime(mime: &str, name: &str) -> String {
    let mime = mime.trim().to_ascii_lowercase();
    if !mime.is_empty() && mime != "application/octet-stream" {
        return mime.split(';').next().unwrap_or(&mime).trim().to_string();
    }
    match Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
    .into()
}

fn allowed_mime(mime: &str) -> bool {
    is_image(mime)
        || is_text_mime(mime)
        || matches!(
            mime,
            "application/pdf"
                | "application/xml"
                | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        )
}

fn is_image(mime: &str) -> bool {
    matches!(
        mime,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/") || matches!(mime, "application/json" | "application/xml")
}

fn image_media(mime: &str) -> ImageMediaType {
    if mime == "image/jpeg" {
        ImageMediaType::JPEG
    } else {
        ImageMediaType::PNG
    }
}

fn utf8_preview(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let preview: String = text.chars().take(TEXT_INLINE_CHARS).collect();
    if text.chars().count() > TEXT_INLINE_CHARS {
        Some(format!("{preview}\n…(truncated)"))
    } else {
        Some(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_in_name() {
        assert_eq!(safe_name("../../etc/passwd").unwrap(), "passwd");
        assert!(safe_name("...").is_err());
    }

    #[test]
    fn guesses_png_mime() {
        assert_eq!(normalize_mime("", "shot.PNG"), "image/png");
        assert!(allowed_mime("image/png"));
        assert!(!allowed_mime("application/x-msdownload"));
    }

    #[test]
    fn stored_blocks_drop_bytes() {
        let files = [DecodedAttachment {
            name: "a.png".into(),
            mime_type: "image/png".into(),
            bytes: vec![1, 2, 3],
        }];
        let blocks = stored_blocks(&files);
        assert_eq!(blocks[0]["kind"], "image");
        assert_eq!(blocks[0]["path"], "inbox/a.png");
        assert!(blocks[0].get("content").is_none());
    }

    #[test]
    fn decode_limits_count() {
        let item = IncomingAttachment {
            name: "a.txt".into(),
            mime_type: "text/plain".into(),
            content: base64::engine::general_purpose::STANDARD.encode("hi"),
        };
        let many = vec![item; 5];
        assert!(decode_incoming(&many).is_err());
    }
}
