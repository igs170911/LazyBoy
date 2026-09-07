//! Read-only, workspace-wide SKILL.md commands.
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSkill {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing)]
    pub instructions: String,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "goal"
        && name != "skills"
        && name != "help"
        && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn parse_file(path: PathBuf, name: String) -> Option<FileSkill> {
    let file_type = std::fs::symlink_metadata(&path).ok()?.file_type();
    if file_type.is_symlink() || !file_type.is_file() {
        return None;
    }
    if std::fs::metadata(&path).ok()?.len() > 512 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let (frontmatter, instructions) = if let Some(rest) = text.strip_prefix("---\n") {
        let (header, body) = rest.split_once("\n---")?;
        (
            header,
            body.trim_start_matches(['\n', '\r']).trim().to_string(),
        )
    } else {
        ("", text.trim().to_string())
    };
    let description = frontmatter.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "description")
            .then(|| value.trim().trim_matches(|ch| ch == '"' || ch == '\'').to_string())
    }).unwrap_or_else(|| instructions.lines().next().unwrap_or("自訂技能").chars().take(120).collect());
    (!instructions.is_empty()).then_some(FileSkill { name, description, instructions })
}

pub fn list(data_dir: &str) -> Vec<FileSkill> {
    let root = Path::new(data_dir).join("skills");
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut skills = entries.filter_map(Result::ok).filter_map(|entry| {
        let file_type = entry.file_type().ok()?;
        if !file_type.is_dir() { return None; }
        let name = entry.file_name().to_string_lossy().to_string();
        valid_name(&name).then(|| parse_file(entry.path().join("SKILL.md"), name)).flatten()
    }).collect::<Vec<_>>();
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

pub fn slash(prompt: &str, data_dir: &str) -> Option<(FileSkill, String)> {
    let mut words = prompt.trim().splitn(2, char::is_whitespace);
    let command = words.next()?.strip_prefix('/')?;
    if !valid_name(command) { return None; }
    let skill = list(data_dir).into_iter().find(|skill| skill.name == command)?;
    Some((skill, words.next().unwrap_or("").trim().to_string()))
}

pub fn index(data_dir: &str) -> String {
    let skills = list(data_dir);
    if skills.is_empty() { return String::new(); }
    let lines = skills.iter().map(|skill| format!("/{:<18} {}", skill.name, skill.description)).collect::<Vec<_>>().join("\n");
    format!("可用的檔案技能（唯讀）：\n{lines}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn loads_only_safe_skill_names_and_resolves_arguments() {
        let root = std::env::temp_dir().join(format!("lazyboy-file-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("skills/open-site")).unwrap();
        fs::write(root.join("skills/open-site/SKILL.md"), "---\ndescription: Open a site\n---\nUse the browser.\n").unwrap();
        fs::create_dir_all(root.join("skills/goal")).unwrap();
        let root_text = root.to_str().unwrap();
        assert_eq!(slash("/open-site example.com", root_text).unwrap().1, "example.com");
        assert!(slash("/goal do it", root_text).is_none());
        assert!(slash("/missing", root_text).is_none());
        let _ = fs::remove_dir_all(root);
    }
}
