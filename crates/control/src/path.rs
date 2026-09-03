use lazyboy_contracts::ComputerMode;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PathError {
    #[error("path escapes the computer workspace")]
    EscapesWorkspace,
}

pub fn normalize_workspace_path(value: &str) -> Result<String, PathError> {
    let normalized = value.replace('\\', "/").trim_start_matches('/').to_string();
    let segments: Vec<&str> = normalized.split('/').filter(|segment| !segment.is_empty()).collect();
    if segments.iter().any(|segment| *segment == "." || *segment == "..") {
        return Err(PathError::EscapesWorkspace);
    }
    Ok(segments.join("/"))
}

pub fn team_bot_workspace_directory(bot_id: &str) -> Result<String, PathError> {
    Ok(format!("bots/{}", normalize_workspace_path(bot_id)?))
}

/// Map a bot-facing path onto the stored workspace path.
///
/// Team computers prefix relative work into `bots/<botId>/`.
/// `shared/...` and `bots/...` address the shared root. Dedicated computers
/// treat the requested path as already rooted at the bot home.
pub fn resolve_bot_workspace_path(
    mode: ComputerMode,
    bot_id: &str,
    requested_path: &str,
) -> Result<String, PathError> {
    if mode != ComputerMode::Team {
        return normalize_workspace_path(requested_path);
    }
    let explicit_root = strip_virtual_workspace_root(requested_path);
    let normalized = normalize_workspace_path(explicit_root.unwrap_or(requested_path))?;
    if explicit_root.is_some() || is_team_root_path(&normalized) {
        return Ok(normalized);
    }
    let home = team_bot_workspace_directory(bot_id)?;
    if normalized.is_empty() {
        Ok(home)
    } else {
        Ok(format!("{home}/{normalized}"))
    }
}

pub fn resolve_bot_workspace_cwd(
    mode: ComputerMode,
    bot_id: &str,
    requested_cwd: Option<&str>,
) -> Result<Option<String>, PathError> {
    if mode != ComputerMode::Team {
        return Ok(requested_cwd.map(str::to_string));
    }
    match requested_cwd {
        None | Some("") | Some(".") => Ok(Some(team_bot_workspace_directory(bot_id)?)),
        Some(cwd) if cwd.starts_with('/') => Ok(Some(cwd.to_string())),
        Some(cwd) => Ok(Some(resolve_bot_workspace_path(mode, bot_id, cwd)?)),
    }
}

pub fn display_bot_workspace_path(
    mode: ComputerMode,
    bot_id: &str,
    requested_path: &str,
    stored_path: &str,
) -> Result<String, PathError> {
    if mode != ComputerMode::Team || !is_bot_relative_request(requested_path) {
        return Ok(stored_path.to_string());
    }
    let normalized = normalize_workspace_path(stored_path)?;
    let bot_directory = team_bot_workspace_directory(bot_id)?;
    if normalized == bot_directory {
        return Ok(String::new());
    }
    let prefix = format!("{bot_directory}/");
    if let Some(rest) = normalized.strip_prefix(&prefix) {
        Ok(rest.to_string())
    } else {
        Ok(normalized)
    }
}

fn is_bot_relative_request(value: &str) -> bool {
    strip_virtual_workspace_root(value).is_none()
        && !is_team_root_path(&normalize_workspace_path(value).unwrap_or_default())
}

fn strip_virtual_workspace_root(value: &str) -> Option<&str> {
    let trimmed = value.trim_start_matches('/');
    trimmed.strip_prefix("workspace/").or_else(|| {
        if trimmed == "workspace" {
            Some("")
        } else {
            None
        }
    })
}

fn is_team_root_path(value: &str) -> bool {
    value == "shared"
        || value.starts_with("shared/")
        || value == "bots"
        || value.starts_with("bots/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_segments() {
        assert!(matches!(
            normalize_workspace_path("../etc/passwd"),
            Err(PathError::EscapesWorkspace)
        ));
        assert!(matches!(
            normalize_workspace_path("foo/./bar"),
            Err(PathError::EscapesWorkspace)
        ));
    }

    #[test]
    fn dedicated_paths_stay_at_home_root() {
        let path = resolve_bot_workspace_path(ComputerMode::Dedicated, "bot-1", "notes/a.txt").unwrap();
        assert_eq!(path, "notes/a.txt");
    }

    #[test]
    fn team_relative_paths_live_in_bot_folder() {
        let path = resolve_bot_workspace_path(ComputerMode::Team, "bot-1", "notes/a.txt").unwrap();
        assert_eq!(path, "bots/bot-1/notes/a.txt");
        let cwd = resolve_bot_workspace_cwd(ComputerMode::Team, "bot-1", None)
            .unwrap()
            .unwrap();
        assert_eq!(cwd, "bots/bot-1");
    }

    #[test]
    fn team_shared_and_bots_address_the_root() {
        assert_eq!(
            resolve_bot_workspace_path(ComputerMode::Team, "bot-1", "shared/plan.md").unwrap(),
            "shared/plan.md"
        );
        assert_eq!(
            resolve_bot_workspace_path(ComputerMode::Team, "bot-1", "bots/bot-2/x").unwrap(),
            "bots/bot-2/x"
        );
    }

    #[test]
    fn display_strips_bot_prefix_for_relative_requests() {
        let shown = display_bot_workspace_path(
            ComputerMode::Team,
            "bot-1",
            "notes/a.txt",
            "bots/bot-1/notes/a.txt",
        )
        .unwrap();
        assert_eq!(shown, "notes/a.txt");
        let shared = display_bot_workspace_path(
            ComputerMode::Team,
            "bot-1",
            "shared/plan.md",
            "shared/plan.md",
        )
        .unwrap();
        assert_eq!(shared, "shared/plan.md");
    }

    #[test]
    fn private_homes_are_not_the_team_home() {
        let team = lazyboy_contracts::computer_home_key(ComputerMode::Team, "space-1", None).unwrap();
        let private = lazyboy_contracts::computer_home_key(
            ComputerMode::Dedicated,
            "space-1",
            Some("bot-private"),
        )
        .unwrap();
        assert_ne!(team, private);
        assert_eq!(
            resolve_bot_workspace_path(ComputerMode::Dedicated, "bot-private", "shared/secret.txt").unwrap(),
            "shared/secret.txt"
        );
    }
}
