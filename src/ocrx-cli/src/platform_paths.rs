//! Cross-platform platform directory resolution (home, cache, config).
//!
//! Platform differences live only in this module. The pure `_from_env`
//! helpers take explicit environment values so they are testable without
//! touching the process environment; the thin wrappers read the process env
//! at call time. `cache_paths` and similar pure functions stay cross-platform
//! identical (WP.1) — only the root resolution differs per platform (W1.3).

use std::path::PathBuf;

/// The host platform, as relevant to directory resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Macos,
    Windows,
    Other,
}

impl Platform {
    /// The stable lowercase label used in the doctor JSON report.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Other => "unsupported",
        }
    }
}

/// Detects the host platform.
#[must_use]
pub fn detect() -> Platform {
    #[cfg(target_os = "macos")]
    {
        Platform::Macos
    }
    #[cfg(target_os = "windows")]
    {
        Platform::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Platform::Other
    }
}

/// Resolves the user home directory from explicit environment values.
///
/// Order: `HOME`, then `USERPROFILE`, then `HOMEDRIVE`+`HOMEPATH`. Returns an
/// error when none are set — never falls back to the current directory (B5:
/// never silently write skills into the project directory).
pub fn home_dir_from_env(
    home: Option<&str>,
    userprofile: Option<&str>,
    homedrive: Option<&str>,
    homepath: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(value) = home.filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    if let Some(value) = userprofile.filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    if let (Some(drive), Some(path)) = (
        homedrive.filter(|s| !s.is_empty()),
        homepath.filter(|s| !s.is_empty()),
    ) {
        // HOMEDRIVE is like "C:" and HOMEPATH is like "\Users\name"; concatenate
        // with a single separator, tolerating leading/trailing slashes on either.
        let drive = drive.trim_end_matches(['\\', '/']);
        let path = path.trim_start_matches(['\\', '/']);
        return Ok(PathBuf::from(format!("{drive}\\{path}")));
    }
    Err(
        "could not resolve user home directory (HOME, USERPROFILE, and HOMEDRIVE+HOMEPATH are all unset)"
            .to_owned(),
    )
}

/// Resolves the user home directory from the process environment.
///
/// # Errors
///
/// Returns an error when no home environment variable is set.
pub fn home_dir() -> Result<PathBuf, String> {
    home_dir_from_env(
        var_str("HOME").as_deref(),
        var_str("USERPROFILE").as_deref(),
        var_str("HOMEDRIVE").as_deref(),
        var_str("HOMEPATH").as_deref(),
    )
}

/// Resolves the model cache root from explicit environment values.
///
/// `OCRX_MODEL_CACHE` overrides the root entirely; otherwise the platform
/// standard cache root is used (macOS `~/Library/Caches`, Windows
/// `%LOCALAPPDATA%`). `cache_paths` appends `ocrx/models/...` so model assets
/// never live in the current project directory.
pub fn cache_root_from_env(
    override_root: Option<&str>,
    localappdata: Option<&str>,
    home: Option<&str>,
    platform: Platform,
) -> PathBuf {
    if let Some(root) = override_root.filter(|s| !s.is_empty()) {
        return PathBuf::from(root);
    }
    match platform {
        Platform::Macos => home
            .map(|h| PathBuf::from(h).join("Library/Caches"))
            .unwrap_or_else(|| fallback_root(home)),
        Platform::Windows => localappdata
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_root(home)),
        Platform::Other => fallback_root(home),
    }
}

/// Resolves the model cache root from the process environment.
pub fn cache_root() -> PathBuf {
    cache_root_from_env(
        var_str("OCRX_MODEL_CACHE").as_deref(),
        var_str("LOCALAPPDATA").as_deref(),
        var_str("HOME")
            .or_else(|| var_str("USERPROFILE"))
            .as_deref(),
        detect(),
    )
}

/// Resolves the user configuration directory from explicit environment values.
///
/// macOS `~/Library/Application Support`, Windows `%APPDATA%`. Reserved for
/// config-file reading (v0.1 does not yet read config files); the resolution
/// is implemented and tested here so callers can adopt it without revisiting
/// platform differences.
pub fn config_dir_from_env(
    override_dir: Option<&str>,
    appdata: Option<&str>,
    home: Option<&str>,
    platform: Platform,
) -> PathBuf {
    if let Some(dir) = override_dir.filter(|s| !s.is_empty()) {
        return PathBuf::from(dir);
    }
    match platform {
        Platform::Macos => home
            .map(|h| PathBuf::from(h).join("Library/Application Support"))
            .unwrap_or_else(|| fallback_root(home)),
        Platform::Windows => appdata
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_root(home)),
        Platform::Other => fallback_root(home),
    }
}

/// Resolves the user configuration directory from the process environment.
#[allow(dead_code)] // reserved for config-file reading (W1.3 resolution; not yet consumed)
pub fn config_dir() -> PathBuf {
    config_dir_from_env(
        var_str("OCRX_CONFIG_DIR").as_deref(),
        var_str("APPDATA").as_deref(),
        var_str("HOME")
            .or_else(|| var_str("USERPROFILE"))
            .as_deref(),
        detect(),
    )
}

/// Last-resort root: a per-user `.ocrx` directory, or a temp dir if no home is
/// resolvable. Never the current project directory.
fn fallback_root(home: Option<&str>) -> PathBuf {
    home.map(|h| PathBuf::from(h).join(".ocrx"))
        .unwrap_or_else(|| std::env::temp_dir().join("ocrx"))
}

fn var_str(key: &str) -> Option<String> {
    std::env::var_os(key).and_then(|s| s.into_string().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_prefers_home_then_userprofile_then_drive_pair() {
        assert_eq!(
            home_dir_from_env(Some("/Users/a"), None, None, None).unwrap(),
            PathBuf::from("/Users/a")
        );
        // HOME wins over USERPROFILE.
        assert_eq!(
            home_dir_from_env(Some("/Users/a"), Some("C:\\Users\\a"), None, None).unwrap(),
            PathBuf::from("/Users/a")
        );
        // USERPROFILE fallback.
        assert_eq!(
            home_dir_from_env(None, Some("C:\\Users\\a"), None, None).unwrap(),
            PathBuf::from("C:\\Users\\a")
        );
        // HOMEDRIVE + HOMEPATH fallback, tolerating leading backslash.
        assert_eq!(
            home_dir_from_env(None, None, Some("C:"), Some("\\Users\\a")).unwrap(),
            PathBuf::from("C:\\Users\\a")
        );
        assert_eq!(
            home_dir_from_env(None, None, Some("C:\\"), Some("Users\\a")).unwrap(),
            PathBuf::from("C:\\Users\\a")
        );
    }

    #[test]
    fn home_dir_errors_when_no_variable_is_set() {
        let err = home_dir_from_env(None, None, None, None).unwrap_err();
        assert!(err.contains("could not resolve user home directory"));
        // Empty strings are treated as unset.
        assert!(home_dir_from_env(Some(""), Some(""), None, None).is_err());
    }

    #[test]
    fn cache_root_override_wins_and_platform_defaults_follow_standard_dirs() {
        // OCRX_MODEL_CACHE override wins on every platform.
        assert_eq!(
            cache_root_from_env(
                Some("/custom"),
                Some("C:\\LA"),
                Some("/h"),
                Platform::Windows
            ),
            PathBuf::from("/custom")
        );
        // macOS default -> ~/Library/Caches.
        assert_eq!(
            cache_root_from_env(None, None, Some("/Users/a"), Platform::Macos),
            PathBuf::from("/Users/a/Library/Caches")
        );
        // Windows default -> %LOCALAPPDATA%.
        assert_eq!(
            cache_root_from_env(
                None,
                Some("C:\\Users\\a\\AppData\\Local"),
                None,
                Platform::Windows
            ),
            PathBuf::from("C:\\Users\\a\\AppData\\Local")
        );
        // Windows with no LOCALAPPDATA falls back to per-user .ocrx under home.
        assert_eq!(
            cache_root_from_env(None, None, Some("C:\\Users\\a"), Platform::Windows),
            PathBuf::from("C:\\Users\\a\\.ocrx")
        );
    }

    #[test]
    fn config_dir_resolves_platform_standard_config_dirs() {
        // Override wins.
        assert_eq!(
            config_dir_from_env(Some("/cfg"), Some("C:\\AD"), Some("/h"), Platform::Windows),
            PathBuf::from("/cfg")
        );
        // macOS -> ~/Library/Application Support.
        assert_eq!(
            config_dir_from_env(None, None, Some("/Users/a"), Platform::Macos),
            PathBuf::from("/Users/a/Library/Application Support")
        );
        // Windows -> %APPDATA%.
        assert_eq!(
            config_dir_from_env(
                None,
                Some("C:\\Users\\a\\AppData\\Roaming"),
                None,
                Platform::Windows
            ),
            PathBuf::from("C:\\Users\\a\\AppData\\Roaming")
        );
    }

    #[test]
    fn platform_label_is_stable() {
        assert_eq!(Platform::Macos.as_str(), "macos");
        assert_eq!(Platform::Windows.as_str(), "windows");
        assert_eq!(Platform::Other.as_str(), "unsupported");
    }
}
