//! Skill installation boundary with source-marked, atomic host adapters.

use std::fs;
use std::io;
use std::io::Write;
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
    /// The skill directory under `root`. The layout (`<root>/.codex/skills/ocrx`,
    /// `<root>/.claude/skills/ocrx`) is identical for user and project scope -
    /// only the root differs (decision 5).
    fn directory(self, root: &Path) -> PathBuf {
        match self {
            Self::Codex => root.join(".codex/skills/ocrx"),
            Self::Claude => root.join(".claude/skills/ocrx"),
        }
    }
}

/// Where a skill is installed: under the user home (`User`) or a project root
/// (`Project`). The skill layout is the same for both; the scope only selects
/// which root to install under (decision 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    User,
    Project,
}

impl SkillScope {
    /// Stable lowercase label used in CLI output and doctor reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }

    /// Parses a scope label (`user`/`project`).
    pub fn parse(label: &str) -> io::Result<Self> {
        match label {
            "user" => Ok(Self::User),
            "project" => Ok(Self::Project),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown skill scope: {other}"),
            )),
        }
    }

    /// Selects the installation root: the user home for `User`, the project
    /// directory for `Project`. Each scope errors when its root is not supplied,
    /// so a missing home or project directory is never silently substituted
    /// with the current directory (B5).
    pub fn root(self, home: Option<&Path>, project: Option<&Path>) -> io::Result<PathBuf> {
        match self {
            Self::User => home.map(PathBuf::from).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "user scope requires a resolvable home directory",
                )
            }),
            Self::Project => project.map(PathBuf::from).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "project scope requires a project directory",
                )
            }),
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

/// Installs the skill for every selected target under the scope's root. The
/// write is atomic (`.part` + rename) and source-marked so only this version's
/// files are ever touched; unrelated user files in the directory are preserved.
pub fn install(
    target: &str,
    scope: SkillScope,
    home: Option<&Path>,
    project: Option<&Path>,
    content: &str,
) -> io::Result<Vec<PathBuf>> {
    let root = scope.root(home, project)?;
    let mut paths = Vec::new();
    for target in parse_target(target)? {
        let directory = target.directory(&root);
        fs::create_dir_all(&directory)?;
        let path = directory.join("SKILL.md");
        let marked = format!("{START}{content}{END}");
        let temporary = directory.join(".SKILL.md.part");
        // Write + sync + rename mirrors ocrx-output::atomic_write (review
        // m11): the sync guarantees the marker file survives a crash, and a
        // failed write must not leave the .part corpse behind.
        let result = (|| -> io::Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(marked.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        paths.push(path);
    }
    Ok(paths)
}

/// Removes this version's skill files for every selected target under the
/// scope's root. Unmarked files (e.g. user-edited or other-version skills) are
/// left untouched.
pub fn uninstall(
    target: &str,
    scope: SkillScope,
    home: Option<&Path>,
    project: Option<&Path>,
) -> io::Result<Vec<PathBuf>> {
    let root = scope.root(home, project)?;
    let mut removed = Vec::new();
    for target in parse_target(target)? {
        let directory = target.directory(&root);
        let path = directory.join("SKILL.md");
        if let Ok(content) = fs::read_to_string(&path) {
            if content.starts_with(START) && content.ends_with(END) {
                fs::remove_file(&path)?;
                // Drop the directory we created when it is now empty
                // (review m11): an uninstall used to leave empty
                // .codex/skills/ocrx shells behind.
                let _ = fs::remove_dir(&directory);
                removed.push(path);
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ocrx-skill-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn user_scope_installs_under_home_for_all_targets() {
        let home = fresh_dir("user-home");
        let paths = install("all", SkillScope::User, Some(&home), None, "# skill\n").unwrap();
        assert_eq!(paths.len(), 2);
        assert!(home.join(".codex/skills/ocrx/SKILL.md").is_file());
        assert!(home.join(".claude/skills/ocrx/SKILL.md").is_file());
        cleanup(&home);
    }

    #[test]
    fn project_scope_installs_under_project_root_not_home() {
        let home = fresh_dir("proj-home");
        let project = fresh_dir("proj-root");
        let paths = install(
            "all",
            SkillScope::Project,
            None,
            Some(&project),
            "# skill\n",
        )
        .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(project.join(".codex/skills/ocrx/SKILL.md").is_file());
        assert!(project.join(".claude/skills/ocrx/SKILL.md").is_file());
        // Project scope must not touch the user home.
        assert!(!home.join(".codex/skills/ocrx/SKILL.md").exists());
        assert!(!home.join(".claude/skills/ocrx/SKILL.md").exists());
        // Uninstall from the same scope removes the project files.
        let removed = uninstall("all", SkillScope::Project, None, Some(&project)).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(!project.join(".codex/skills/ocrx/SKILL.md").exists());
        cleanup(&home);
        cleanup(&project);
    }

    #[test]
    fn project_scope_without_project_dir_is_rejected() {
        let home = fresh_dir("noproj-home");
        assert!(install("all", SkillScope::Project, Some(&home), None, "x").is_err());
        assert!(uninstall("all", SkillScope::Project, Some(&home), None).is_err());
        cleanup(&home);
    }

    #[test]
    fn user_scope_without_home_is_rejected() {
        let project = fresh_dir("nohome-project");
        assert!(install("all", SkillScope::User, None, Some(&project), "x").is_err());
        cleanup(&project);
    }

    #[test]
    fn single_target_codex_installs_only_codex() {
        let home = fresh_dir("codex-only");
        let paths = install("codex", SkillScope::User, Some(&home), None, "# skill\n").unwrap();
        assert_eq!(paths.len(), 1);
        assert!(home.join(".codex/skills/ocrx/SKILL.md").is_file());
        assert!(!home.join(".claude/skills/ocrx").exists());
        cleanup(&home);
    }

    #[test]
    fn single_target_claude_installs_only_claude() {
        let home = fresh_dir("claude-only");
        let paths = install("claude", SkillScope::User, Some(&home), None, "# skill\n").unwrap();
        assert_eq!(paths.len(), 1);
        assert!(home.join(".claude/skills/ocrx/SKILL.md").is_file());
        assert!(!home.join(".codex/skills/ocrx").exists());
        cleanup(&home);
    }

    #[test]
    fn install_is_atomic_and_preserves_unrelated_files() {
        let home = fresh_dir("preserve");
        fs::create_dir_all(home.join(".codex/skills/ocrx")).unwrap();
        fs::write(home.join(".codex/skills/ocrx/other.txt"), "keep").unwrap();
        let paths = install("all", SkillScope::User, Some(&home), None, "# test\n").unwrap();
        assert_eq!(paths.len(), 2);
        assert!(home.join(".codex/skills/ocrx/other.txt").is_file());
        assert_eq!(
            uninstall("all", SkillScope::User, Some(&home), None)
                .unwrap()
                .len(),
            2
        );
        assert!(home.join(".codex/skills/ocrx/other.txt").is_file());
        cleanup(&home);
    }

    #[test]
    fn unknown_target_and_unmarked_file_are_safe() {
        let home = fresh_dir("safe");
        assert!(install("unknown", SkillScope::User, Some(&home), None, "x").is_err());
        let directory = home.join(".codex/skills/ocrx");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("SKILL.md"), "user file").unwrap();
        assert!(
            uninstall("codex", SkillScope::User, Some(&home), None)
                .unwrap()
                .is_empty()
        );
        assert!(directory.join("SKILL.md").is_file());
        cleanup(&home);
    }

    #[test]
    fn scope_parse_round_trips_and_rejects_unknown() {
        assert_eq!(SkillScope::parse("user").unwrap(), SkillScope::User);
        assert_eq!(SkillScope::parse("project").unwrap(), SkillScope::Project);
        assert!(SkillScope::parse("global").is_err());
        assert_eq!(SkillScope::User.as_str(), "user");
        assert_eq!(SkillScope::Project.as_str(), "project");
    }
}
