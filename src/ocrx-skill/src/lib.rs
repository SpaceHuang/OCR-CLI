//! Skill installation boundary with source-marked, atomic host adapters.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const SKILL_ID: &str = "ocrx";
pub const SKILL_VERSION: &str = "0.1.0";
const START: &str = "<!-- ocrx-skill:begin version=0.1.0 -->\n";
const END: &str = "\n<!-- ocrx-skill:end -->\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTarget {
    Codex,
    Claude,
}

impl SkillTarget {
    fn directory(self, home: &Path) -> PathBuf {
        match self {
            Self::Codex => home.join(".codex/skills/ocrx"),
            Self::Claude => home.join(".claude/skills/ocrx"),
        }
    }
}

pub fn parse_target(target: &str) -> io::Result<Vec<SkillTarget>> {
    match target {
        "codex" => Ok(vec![SkillTarget::Codex]),
        "claude" => Ok(vec![SkillTarget::Claude]),
        "all" => Ok(vec![SkillTarget::Codex, SkillTarget::Claude]),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown skill target",
        )),
    }
}

pub fn install(target: &str, home: impl AsRef<Path>, content: &str) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for target in parse_target(target)? {
        let directory = target.directory(home.as_ref());
        fs::create_dir_all(&directory)?;
        let path = directory.join("SKILL.md");
        let marked = format!("{START}{content}{END}");
        let temporary = directory.join(".SKILL.md.part");
        fs::write(&temporary, marked)?;
        fs::rename(&temporary, &path)?;
        paths.push(path);
    }
    Ok(paths)
}

pub fn uninstall(target: &str, home: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for target in parse_target(target)? {
        let path = target.directory(home.as_ref()).join("SKILL.md");
        if let Ok(content) = fs::read_to_string(&path) {
            if content.starts_with(START) && content.ends_with(END) {
                fs::remove_file(&path)?;
                removed.push(path);
            }
        }
    }
    Ok(removed)
}

#[must_use]
pub fn redact_sensitive(input: &str) -> String {
    let mut output = input.to_owned();
    for key in ["prompt", "ocr", "token", "authorization", "proxy", "url"] {
        let mut cursor = 0;
        while let Some(relative) = output[cursor..].to_ascii_lowercase().find(key) {
            let start = cursor + relative;
            let Some(separator) = output[start..].find(['=', ':']) else {
                break;
            };
            let value_start = start + separator + 1;
            let value_end = output[value_start..]
                .find([',', ' ', '\n'])
                .map_or(output.len(), |end| value_start + end);
            output.replace_range(value_start..value_end, "[REDACTED]");
            cursor = value_start + 10;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_all_targets_atomically_and_preserves_unrelated_files() {
        let home = std::env::temp_dir().join(format!("ocrx-skill-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let content = "# test\n";
        fs::create_dir_all(home.join(".codex/skills/ocrx")).unwrap();
        fs::write(home.join(".codex/skills/ocrx/other.txt"), "keep").unwrap();
        let paths = install("all", &home, content).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(home.join(".codex/skills/ocrx/other.txt").is_file());
        assert!(uninstall("all", &home).unwrap().len() == 2);
        assert!(home.join(".codex/skills/ocrx/other.txt").is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn unknown_target_and_unmarked_file_are_safe() {
        let home = std::env::temp_dir().join(format!("ocrx-skill-safe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        assert!(install("unknown", &home, "x").is_err());
        let directory = home.join(".codex/skills/ocrx");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("SKILL.md"), "user file").unwrap();
        assert!(uninstall("codex", &home).unwrap().is_empty());
        assert!(directory.join("SKILL.md").is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn redaction_removes_sensitive_values() {
        let output =
            redact_sensitive("prompt=secret token:abc proxy=https://u:p@example/a ocr=text");
        assert!(!output.contains("secret"));
        assert!(!output.contains("abc"));
        assert!(!output.contains("https://"));
        assert!(!output.contains("text"));
    }
}
