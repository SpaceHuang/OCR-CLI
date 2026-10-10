//! Model manifest and cache boundary.

use ocrx_core::ModelId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestIdentity {
    pub model: ModelId,
    pub revision: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    pub identity: ManifestIdentity,
    pub profile: String,
    pub model_sha256: String,
    pub mmproj_sha256: String,
}

pub const COMMUNITY_REVISION: &str = "8a22290a0c42dbcf84739d6d4c2763f877494ae0";
pub const COMMUNITY_MMPROJ_SHA256: &str =
    "7e371704e8f3c88638830e52866dfa88b2fd383155e3a0f9484e4ce037fdd42f";

#[must_use]
pub fn community_asset_sizes(profile: &str) -> Option<(u64, u64)> {
    let model_bytes = match profile {
        "compact" => 529_297_280,
        "balanced" => 577_998_720,
        "quality" => 811_843_498,
        "reference" => 1_516_744_617,
        _ => return None,
    };
    Some((model_bytes, 204_987_079))
}

/// Returns the pinned community manifest for one supported OvisOCR2 profile.
pub fn community_manifest(profile: &str) -> Result<ModelManifest, ManifestError> {
    let model_sha256 = match profile {
        "compact" => "4ae54c66bfe1efb2882854ff642c3d137aa082dbd0699bc9234ff39874a492a1",
        "balanced" => "e17744e367adb891cc9c82f4255408cf46a00efa1b20a83f8187d5bd662ce89a",
        "quality" => "3fba6d94312e550575a92d55cffb8d75997bf68a9f133c92ab6ad7f0bf2bc93e",
        "reference" => "bd8f2c62c930d74590abcb5f65f79e898672e458d6044e8b2e885daea948592e",
        _ => return Err(ManifestError::UnsupportedProfile),
    };
    Ok(ModelManifest {
        identity: ManifestIdentity {
            model: ModelId::OvisOcr2,
            revision: COMMUNITY_REVISION.to_owned(),
            schema_version: 1,
        },
        profile: profile.to_owned(),
        model_sha256: model_sha256.to_owned(),
        mmproj_sha256: COMMUNITY_MMPROJ_SHA256.to_owned(),
    })
}

/// Returns Hugging Face resolve URLs for a pinned community profile.
#[must_use]
pub fn community_asset_urls(profile: &str) -> Option<(String, String)> {
    let model_file = match profile {
        "compact" => "OvisOCR2-Q4_K_M.gguf",
        "balanced" => "OvisOCR2-Q5_K_M.gguf",
        "quality" => "OvisOCR2-Q8_0.gguf",
        "reference" => "OvisOCR2-F16.gguf",
        _ => return None,
    };
    let base = format!("https://huggingface.co/Abiray/OvisOCR2-GGUF/resolve/{COMMUNITY_REVISION}");
    Some((
        format!("{base}/{model_file}"),
        format!("{base}/mmproj-F16.gguf"),
    ))
}

impl ModelManifest {
    /// Validates the supported model, profile, revision, and asset hashes.
    ///
    /// # Errors
    ///
    /// Returns a [`ManifestError`] when any manifest field is unsupported or malformed.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.identity.model != ModelId::OvisOcr2 {
            return Err(ManifestError::UnsupportedModel);
        }
        if !matches!(
            self.profile.as_str(),
            "compact" | "balanced" | "quality" | "reference"
        ) {
            return Err(ManifestError::UnsupportedProfile);
        }
        if self.identity.revision.trim().is_empty()
            || self.model_sha256.len() != 64
            || self.mmproj_sha256.len() != 64
            || !self
                .model_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !self
                .mmproj_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ManifestError::InvalidAsset);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedModel,
    UnsupportedProfile,
    InvalidAsset,
    SharedAssetInUse,
    InsufficientMemory,
    NoPassingProfile,
    DeviceUnavailable,
    InvalidDevice,
    HashMismatch,
    MissingAsset,
    Offline,
    LicenseRequired,
    InsufficientDisk,
    TransactionFailed,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelState {
    pub profile: String,
    pub revision: String,
    pub model_path: std::path::PathBuf,
    pub mmproj_path: std::path::PathBuf,
    pub model_present: bool,
    pub mmproj_present: bool,
}

impl LocalModelState {
    #[must_use]
    pub fn is_installed(&self) -> bool {
        self.model_present && self.mmproj_present
    }

    /// Checks whether removing this profile would orphan a shared projector.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::SharedAssetInUse`] when another profile still uses the projector.
    pub fn remove_allowed(&self, other_profiles_using_mmproj: bool) -> Result<(), ManifestError> {
        if other_profiles_using_mmproj {
            return Err(ManifestError::SharedAssetInUse);
        }
        Ok(())
    }
}

/// Resolves the two cache asset paths for a supported profile and revision.
///
/// The layout is `<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf`
/// and `<cache_root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf`: the projector
/// lives at the revision layer and is shared across profiles, while different
/// revisions are fully isolated.
///
/// # Errors
///
/// Returns [`ManifestError::UnsupportedProfile`] for an unknown profile and
/// [`ManifestError::InvalidAsset`] when the `revision` is empty, absolute, or
/// contains path separators / traversal segments (path escape).
pub fn cache_paths(
    cache_root: impl AsRef<std::path::Path>,
    revision: &str,
    profile: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), ManifestError> {
    if !matches!(profile, "compact" | "balanced" | "quality" | "reference") {
        return Err(ManifestError::UnsupportedProfile);
    }
    if revision.is_empty()
        || std::path::Path::new(revision).is_absolute()
        || revision.contains('/')
        || revision.contains('\\')
        || revision == ".."
        || revision == "."
    {
        return Err(ManifestError::InvalidAsset);
    }
    let revision_dir = cache_root
        .as_ref()
        .join("ocrx")
        .join("models")
        .join("ovisocr2")
        .join(revision);
    Ok((
        revision_dir.join(profile).join("model.gguf"),
        revision_dir.join("mmproj.gguf"),
    ))
}

/// Resolves the platform-standard default model cache root (WP.1 freeze).
///
/// Windows resolves `%LOCALAPPDATA%` (falling back to `%USERPROFILE%\AppData\Local`),
/// macOS resolves `$HOME/Library/Caches`; platforms without a standard cache
/// location keep the legacy relative `.ocrx-model-cache`. The `ocrx/models`
/// layout below the root is appended by [`cache_paths`], so model assets never
/// land in the current project directory by default. `OCRX_MODEL_CACHE`
/// overrides only the root.
///
/// Migration safety (BLK-5): pre-WP.1 installs kept weights at
/// `.ocrx-model-cache/ovisocr2/<profile>/{model,mmproj}.gguf` — a layout the
/// new `cache_paths` cannot read. When the platform root is empty but such a
/// legacy layout exists, [`migrate_legacy_cache`] moves the pinned-verified
/// files into the platform root (once); only then does the platform root win.
/// A legacy root already carrying the new layout is used as-is.
#[must_use]
pub fn default_cache_root() -> std::path::PathBuf {
    let root = default_cache_root_from(
        std::env::var_os("LOCALAPPDATA").as_deref(),
        std::env::var_os("USERPROFILE").as_deref(),
        std::env::var_os("HOME").as_deref(),
    );
    let legacy = std::path::PathBuf::from(".ocrx-model-cache");
    match legacy_cache_root_fallback(&root, &legacy) {
        // Legacy root already carries the new layout: keep using it.
        Some(legacy_root) if legacy_root.join("ocrx").join("models").exists() => legacy_root,
        // Pre-WP.1 layout: migrate what verifies, then use the platform root.
        Some(legacy_root) => {
            let _ = migrate_legacy_cache(&legacy_root, &root);
            root
        }
        None => root,
    }
}

/// Pure core of the legacy-root fallback used by [`default_cache_root`].
///
/// Returns `Some(legacy_root)` only when the legacy directory contains model
/// content (either layout) and the new platform root does not.
#[must_use]
pub fn legacy_cache_root_fallback(
    platform_root: &std::path::Path,
    legacy_root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    if platform_root.join("ocrx").join("models").exists() {
        return None;
    }
    if legacy_root.join("ocrx").join("models").exists() {
        return Some(legacy_root.to_path_buf());
    }
    // Real pre-WP.1 layout: <legacy>/ovisocr2/<profile>/model.gguf with the
    // projector duplicated per profile (no ocrx/models layer at all).
    if ["compact", "balanced", "quality", "reference"]
        .iter()
        .any(|profile| {
            legacy_root
                .join("ovisocr2")
                .join(profile)
                .join("model.gguf")
                .is_file()
        })
    {
        return Some(legacy_root.to_path_buf());
    }
    None
}

/// Moves pinned-verified weights from the pre-WP.1 legacy layout
/// (`<legacy>/ovisocr2/<profile>/{model,mmproj}.gguf`) into the platform
/// root's `cache_paths` layout.
///
/// Each file is verified against the manifest's pinned SHA-256 (and the
/// pinned size, where known) before it moves; anything that fails
/// verification is left in place rather than propagated. Moves are
/// rename-first with a copy fallback for cross-volume roots, and already
/// present targets are never overwritten. Best-effort by design: callers
/// treat a failed migration as "no legacy content".
///
/// # Errors
///
/// Returns an error only for infrastructure failures (unreadable legacy
/// directory); individual bad assets are skipped.
pub fn migrate_legacy_cache(
    legacy_root: &std::path::Path,
    platform_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let legacy_models = legacy_root.join("ovisocr2");
    for profile in ["compact", "balanced", "quality", "reference"] {
        let Ok(manifest) = community_manifest(profile) else {
            continue;
        };
        let Ok((target_model, target_mmproj)) =
            cache_paths(platform_root, &manifest.identity.revision, profile)
        else {
            continue;
        };
        let legacy_model = legacy_models.join(profile).join("model.gguf");
        if !target_model.is_file() && legacy_model.is_file() {
            let verified = file_sha256(&legacy_model)
                .is_ok_and(|hash| hash == manifest.model_sha256.to_ascii_lowercase())
                && community_asset_sizes(profile).is_some_and(|(model_bytes, _)| {
                    std::fs::metadata(&legacy_model)
                        .map(|metadata| metadata.len() == model_bytes)
                        .unwrap_or(false)
                });
            // A failed move leaves the legacy file untouched; the next run
            // retries. Per-file failures never abort the whole migration.
            if verified {
                let _ = move_verified_asset(&legacy_model, &target_model);
            }
        }
        let legacy_mmproj = legacy_models.join(profile).join("mmproj.gguf");
        if !target_mmproj.is_file()
            && legacy_mmproj.is_file()
            && file_sha256(&legacy_mmproj)
                .is_ok_and(|hash| hash == manifest.mmproj_sha256.to_ascii_lowercase())
        {
            let _ = move_verified_asset(&legacy_mmproj, &target_mmproj);
        }
    }
    Ok(())
}

/// Moves one already-verified asset, preferring a same-volume rename and
/// falling back to copy (cross-volume), never overwriting an existing target.
fn move_verified_asset(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    if target.exists() {
        return Ok(());
    }
    let directory = target
        .parent()
        .ok_or_else(|| std::io::Error::other("asset target has no parent directory"))?;
    std::fs::create_dir_all(directory)?;
    if std::fs::rename(source, target).is_ok() {
        return Ok(());
    }
    std::fs::copy(source, target).map(|_| ())
}

/// Pure core of [`default_cache_root`] with injectable environment values.
///
/// The final fallback for service accounts without any home environment is a
/// temp-dir root, never the current directory (M-11): a CWD-relative cache
/// would both violate W1.3 ("never in the current project") and defeat the
/// absolute-path overlap checks guarding it.
#[must_use]
pub fn default_cache_root_from(
    local_app_data: Option<&std::ffi::OsStr>,
    user_profile: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(local) = local_app_data {
            return std::path::PathBuf::from(local);
        }
        if let Some(profile) = user_profile {
            return std::path::PathBuf::from(profile)
                .join("AppData")
                .join("Local");
        }
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return std::path::PathBuf::from(home)
                .join("Library")
                .join("Caches");
        }
    }
    std::env::temp_dir().join("ocrx").join("models")
}

/// Verifies that local model state matches the canonical cache layout.
///
/// # Errors
///
/// Returns a [`ManifestError`] when the profile, paths, or presence flags are invalid.
pub fn verify_local_state(
    state: &LocalModelState,
    cache_root: impl AsRef<std::path::Path>,
) -> Result<(), ManifestError> {
    let (expected_model, expected_mmproj) =
        cache_paths(cache_root, &state.revision, &state.profile)?;
    if state.model_path != expected_model
        || state.mmproj_path != expected_mmproj
        || !state.is_installed()
    {
        return Err(ManifestError::InvalidAsset);
    }
    Ok(())
}

/// Recomputes both asset hashes and verifies the manifest/cache pairing.
///
/// # Errors
///
/// Returns [`ManifestError::MissingAsset`] when either file is absent and
/// [`ManifestError::HashMismatch`] when its content differs from the manifest.
pub fn verify_manifest_files(
    manifest: &ModelManifest,
    cache_root: impl AsRef<std::path::Path>,
) -> Result<(), ManifestError> {
    manifest.validate()?;
    let (model, mmproj) = cache_paths(cache_root, &manifest.identity.revision, &manifest.profile)?;
    let model_hash = file_sha256(&model)?;
    let mmproj_hash = file_sha256(&mmproj)?;
    if model_hash != manifest.model_sha256.to_ascii_lowercase()
        || mmproj_hash != manifest.mmproj_sha256.to_ascii_lowercase()
    {
        return Err(ManifestError::HashMismatch);
    }
    Ok(())
}

/// Verifies hashes and exact byte sizes for a pinned asset pair.
///
/// # Errors
///
/// Returns [`ManifestError::HashMismatch`] for a size or digest mismatch.
pub fn verify_manifest_files_with_sizes(
    manifest: &ModelManifest,
    cache_root: impl AsRef<std::path::Path>,
    model_bytes: u64,
    mmproj_bytes: u64,
) -> Result<(), ManifestError> {
    verify_manifest_files(manifest, &cache_root)?;
    let (model, mmproj) = cache_paths(&cache_root, &manifest.identity.revision, &manifest.profile)?;
    let model_size = std::fs::metadata(model)
        .map_err(|_| ManifestError::MissingAsset)?
        .len();
    let mmproj_size = std::fs::metadata(mmproj)
        .map_err(|_| ManifestError::MissingAsset)?
        .len();
    if model_size != model_bytes || mmproj_size != mmproj_bytes {
        return Err(ManifestError::HashMismatch);
    }
    Ok(())
}

/// Checks the runtime/adapter versions locked by the community OvisOCR2 asset.
///
/// The accepted tuple is the shared [`ocrx_core::RUNTIME_COMPATIBILITY`] single
/// source of truth, so macOS and Windows enforce the same binding.
///
/// # Errors
///
/// Returns [`ManifestError::InvalidAsset`] when the supplied compatibility tuple differs.
pub fn validate_runtime_adapter_compatibility(
    runtime_version: &str,
    sys_version: &str,
    adapter_id: &str,
) -> Result<(), ManifestError> {
    let compat = ocrx_core::RUNTIME_COMPATIBILITY;
    if runtime_version != compat.llama_cpp_2_version
        || sys_version != compat.llama_cpp_sys_2_version
        || adapter_id != compat.adapter_id
    {
        return Err(ManifestError::InvalidAsset);
    }
    Ok(())
}

fn file_sha256(path: &std::path::Path) -> Result<String, ManifestError> {
    if !path.is_file() {
        return Err(ManifestError::MissingAsset);
    }
    ocrx_core::sha256_file(path).map_err(|_| ManifestError::MissingAsset)
}

fn transaction_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".install-{}-{nanos:x}", std::process::id())
}

fn commit_pair(
    model_part: &std::path::Path,
    mmproj_part: &std::path::Path,
    model: &std::path::Path,
    mmproj: &std::path::Path,
    transaction: &str,
) -> Result<(), ManifestError> {
    let _lock = InstallLock::acquire(model.parent().ok_or(ManifestError::TransactionFailed)?)?;
    let model_backup = model.with_extension(format!("gguf{transaction}.bak"));
    let mmproj_backup = mmproj.with_extension(format!("gguf{transaction}.bak"));
    let old_model = model.is_file();
    let old_mmproj = mmproj.is_file();
    if old_model {
        std::fs::rename(model, &model_backup).map_err(|_| ManifestError::TransactionFailed)?;
    }
    if old_mmproj && std::fs::rename(mmproj, &mmproj_backup).is_err() {
        if old_model {
            if std::fs::rename(&model_backup, model).is_err() {
                return Err(ManifestError::TransactionFailed);
            }
        }
        return Err(ManifestError::TransactionFailed);
    }
    let committed =
        std::fs::rename(model_part, model).and_then(|_| std::fs::rename(mmproj_part, mmproj));
    if committed.is_err() {
        let mut recovery_failed = false;
        if std::fs::remove_file(model).is_err() && model.exists() {
            recovery_failed = true;
        }
        if std::fs::remove_file(mmproj).is_err() && mmproj.exists() {
            recovery_failed = true;
        }
        if old_model {
            if std::fs::rename(&model_backup, model).is_err() {
                recovery_failed = true;
            }
        }
        if old_mmproj {
            if std::fs::rename(&mmproj_backup, mmproj).is_err() {
                recovery_failed = true;
            }
        }
        if recovery_failed {
            return Err(ManifestError::TransactionFailed);
        }
        return Err(ManifestError::TransactionFailed);
    }
    if model_backup.exists() && std::fs::remove_file(&model_backup).is_err() {
        return Err(ManifestError::TransactionFailed);
    }
    if mmproj_backup.exists() && std::fs::remove_file(&mmproj_backup).is_err() {
        return Err(ManifestError::TransactionFailed);
    }
    Ok(())
}

struct InstallLock {
    path: std::path::PathBuf,
    owner: String,
    file: std::fs::File,
}

impl InstallLock {
    fn acquire(directory: &std::path::Path) -> Result<Self, ManifestError> {
        let path = directory.join(".install.lock");
        let create = || {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
        };
        let owner = std::process::id().to_string();
        let file = if let Ok(file) = create() {
            file
        } else {
            let owner = std::fs::read_to_string(&path)
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok());
            let stale = owner.is_some_and(|pid| process_liveness(pid) == ProcessLiveness::Dead);
            if stale {
                std::fs::remove_file(&path).map_err(|_| ManifestError::TransactionFailed)?;
                create().map_err(|_| ManifestError::TransactionFailed)?
            } else {
                return Err(ManifestError::TransactionFailed);
            }
        };
        use std::io::Write;
        let mut file = file;
        file.write_all(owner.as_bytes())
            .map_err(|_| ManifestError::TransactionFailed)?;
        file.flush().map_err(|_| ManifestError::TransactionFailed)?;
        Ok(Self { path, owner, file })
    }
}

#[cfg(unix)]
fn process_liveness(pid: u32) -> ProcessLiveness {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map_or(ProcessLiveness::Unknown, |status| {
            if status.success() {
                ProcessLiveness::Alive
            } else {
                ProcessLiveness::Dead
            }
        })
}

#[cfg(not(unix))]
fn process_liveness(pid: u32) -> ProcessLiveness {
    // /FO CSV keeps fields quoted and comma-separated; the default
    // whitespace split misparsed image names containing spaces and could
    // declare a live holder dead (review m10).
    let output = match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return ProcessLiveness::Unknown,
    };
    if String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        // CSV row: "name","pid",... — second quoted field is the PID.
        line.split("\",\"")
            .nth(1)
            .map(|field| field.trim_matches('"').parse::<u32>().ok())
            .unwrap_or(None)
            == Some(pid)
    }) {
        ProcessLiveness::Alive
    } else {
        ProcessLiveness::Dead
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessLiveness {
    Alive,
    Dead,
    Unknown,
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).is_ok_and(|contents| contents == self.owner) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Inputs for an offline-testable model installation transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub manifest: ModelManifest,
    pub model_source: std::path::PathBuf,
    pub mmproj_source: std::path::PathBuf,
    pub cache_root: std::path::PathBuf,
    pub offline: bool,
    pub non_interactive: bool,
    pub license_accepted: bool,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlInstallRequest {
    pub manifest: ModelManifest,
    pub model_url: String,
    pub mmproj_url: String,
    pub cache_root: std::path::PathBuf,
    pub offline: bool,
    pub non_interactive: bool,
    pub license_accepted: bool,
    pub available_bytes: Option<u64>,
}

/// Downloads and commits both assets as one verified installation transaction.
pub fn install_from_urls(request: &UrlInstallRequest) -> Result<(), ManifestError> {
    request.manifest.validate()?;
    if request.offline {
        return Err(ManifestError::Offline);
    }
    if request.non_interactive && !request.license_accepted {
        return Err(ManifestError::LicenseRequired);
    }
    let (model, mmproj) = cache_paths(
        &request.cache_root,
        &request.manifest.identity.revision,
        &request.manifest.profile,
    )?;
    // Idempotent reinstall (review M-3): an installed profile that already
    // verifies is a success without touching the network — a second install
    // used to die on stale ETag state instead.
    if let Some((model_bytes, mmproj_bytes)) = community_asset_sizes(&request.manifest.profile)
        && verify_manifest_files_with_sizes(
            &request.manifest,
            &request.cache_root,
            model_bytes,
            mmproj_bytes,
        )
        .is_ok()
    {
        return Ok(());
    }
    // Pre-flight disk check against the pinned manifest sizes (review m8):
    // the old check ran after the download and compared bytes already on
    // disk, so an under-provisioned disk paid the full transfer first.
    if let Some(available) = request.available_bytes
        && let Some((model_bytes, mmproj_bytes)) = community_asset_sizes(&request.manifest.profile)
        && available < model_bytes.saturating_add(mmproj_bytes)
    {
        return Err(ManifestError::InsufficientDisk);
    }
    clean_transaction_residue(&[&model, &mmproj]);
    let transaction = transaction_id();
    let model_part = model.with_extension(format!("gguf{transaction}.part"));
    let mmproj_part = mmproj.with_extension(format!("gguf{transaction}.part"));
    let result = (|| {
        download_asset(&request.model_url, &model_part, None, false)?;
        download_asset(&request.mmproj_url, &mmproj_part, None, false)?;
        if file_sha256(&model_part)? != request.manifest.model_sha256.to_ascii_lowercase()
            || file_sha256(&mmproj_part)? != request.manifest.mmproj_sha256.to_ascii_lowercase()
        {
            return Err(ManifestError::HashMismatch);
        }
        commit_pair(&model_part, &mmproj_part, &model, &mmproj, &transaction)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&model_part);
        let _ = std::fs::remove_file(&mmproj_part);
    }
    result
}

/// Installs a manifest's paired assets with verification-before-commit semantics.
///
/// # Errors
///
/// Returns a transaction error for offline/network policy, license, disk,
/// source, copy, or hash failures. No `.part` file is left on failure.
pub fn install_from_files(request: &InstallRequest) -> Result<(), ManifestError> {
    request.manifest.validate()?;
    if request.offline {
        return Err(ManifestError::Offline);
    }
    if request.non_interactive && !request.license_accepted {
        return Err(ManifestError::LicenseRequired);
    }
    let model_size = std::fs::metadata(&request.model_source)
        .map_err(|_| ManifestError::MissingAsset)?
        .len();
    let mmproj_size = std::fs::metadata(&request.mmproj_source)
        .map_err(|_| ManifestError::MissingAsset)?
        .len();
    let required = model_size.saturating_add(mmproj_size);
    if request
        .available_bytes
        .is_some_and(|available| available < required)
    {
        return Err(ManifestError::InsufficientDisk);
    }
    let (model, mmproj) = cache_paths(
        &request.cache_root,
        &request.manifest.identity.revision,
        &request.manifest.profile,
    )?;
    // Same idempotency rule as `install_from_urls` (review M-3).
    if let Some((model_bytes, mmproj_bytes)) = community_asset_sizes(&request.manifest.profile)
        && verify_manifest_files_with_sizes(
            &request.manifest,
            &request.cache_root,
            model_bytes,
            mmproj_bytes,
        )
        .is_ok()
    {
        return Ok(());
    }
    clean_transaction_residue(&[&model, &mmproj]);
    let directory = model.parent().ok_or(ManifestError::TransactionFailed)?;
    std::fs::create_dir_all(directory).map_err(|_| ManifestError::TransactionFailed)?;
    let transaction = transaction_id();
    let model_part = model.with_extension(format!("gguf{transaction}.part"));
    let mmproj_part = mmproj.with_extension(format!("gguf{transaction}.part"));
    let result = (|| {
        std::fs::copy(&request.model_source, &model_part)
            .map_err(|_| ManifestError::TransactionFailed)?;
        std::fs::copy(&request.mmproj_source, &mmproj_part)
            .map_err(|_| ManifestError::TransactionFailed)?;
        if file_sha256(&model_part)? != request.manifest.model_sha256.to_ascii_lowercase()
            || file_sha256(&mmproj_part)? != request.manifest.mmproj_sha256.to_ascii_lowercase()
        {
            return Err(ManifestError::HashMismatch);
        }
        commit_pair(&model_part, &mmproj_part, &model, &mmproj, &transaction)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&model_part);
        let _ = std::fs::remove_file(&mmproj_part);
    }
    result
}

/// Downloads one asset into a fresh `.part` path using the host HTTPS client.
/// The caller must verify the resulting hash before committing it.
///
/// ETag conditional requests are deliberately unused (review M-3): a 304
/// answer produced an empty `.part` that failed hash verification and made
/// every later install fail until the `.etag` file was deleted by hand. The
/// pinned SHA-256 is the sole correctness criterion, so downloads are always
/// unconditional.
///
/// # Errors
///
/// Returns [`ManifestError::Offline`] before creating a process in offline mode,
/// or [`ManifestError::Network`] when curl cannot complete the transfer.
pub fn download_asset(
    url: &str,
    part_path: impl AsRef<std::path::Path>,
    _etag_path: Option<&std::path::Path>,
    offline: bool,
) -> Result<(), ManifestError> {
    if offline {
        return Err(ManifestError::Offline);
    }
    // Model provenance contract (AGENTS.md): weights come only from pinned
    // HTTPS URLs; plaintext HTTP would let a network rewrite the bytes
    // between download and SHA-256 verification. The proto flags also stop
    // an https→http redirect downgrade mid-transfer (m12).
    if !url.starts_with("https://") {
        return Err(ManifestError::Network);
    }
    let part_path = part_path.as_ref();
    if let Some(parent) = part_path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ManifestError::TransactionFailed)?;
    }
    let output = std::process::Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--proto-default",
            "https",
            "--proto-redir",
            "https",
            "--silent",
            "--show-error",
            "--retry",
            "2",
            "--output",
            part_path.to_str().ok_or(ManifestError::Network)?,
            url,
        ])
        .output()
        .map_err(|_| ManifestError::Network)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ManifestError::Network)
    }
}

/// Removes leftover transaction files (`*.part`, `*.bak`, `*.etag`) from the
/// directories holding the committed asset targets, before a new transaction
/// starts (review M-3): killed installs used to leave multi-GB `.part`
/// corpses behind forever, and stale `.etag`/`.bak` state broke re-installs.
/// Best-effort; concurrent transactions are still serialized by
/// [`InstallLock`] at commit time.
fn clean_transaction_residue(targets: &[&std::path::Path]) {
    let mut directories = Vec::new();
    for target in targets {
        if let Some(directory) = target.parent() {
            directories.push(directory.to_path_buf());
        }
    }
    for directory in directories {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Exact-case comparison is intentional: these files are created
            // by this crate's own transactions with exactly these suffixes.
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            if name.ends_with(".part") || name.ends_with(".bak") || name.ends_with(".etag") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub model: ModelId,
    pub profiles: Vec<String>,
    pub installed_profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalog {
    manifests: Vec<ModelManifest>,
}

/// Selects the best passing profile for available memory.
///
/// # Errors
///
/// Returns [`ManifestError::InsufficientMemory`] below the 6 GB floor or
/// [`ManifestError::NoPassingProfile`] when no fallback passes.
pub fn recommend_profile(
    memory_gb: u64,
    passing_profiles: &[&str],
) -> Result<String, ManifestError> {
    let preferred = if memory_gb < 6 {
        return Err(ManifestError::InsufficientMemory);
    } else if memory_gb < 12 {
        "compact"
    } else if memory_gb < 24 {
        "balanced"
    } else {
        "quality"
    };
    if passing_profiles.contains(&preferred) {
        return Ok(preferred.to_owned());
    }
    if preferred != "compact" && passing_profiles.contains(&"compact") {
        return Ok("compact".to_owned());
    }
    Err(ManifestError::NoPassingProfile)
}

/// Availability flags are intentionally one bool per backend so `recommend_device`
/// can stay a data-driven table; collapsing them would hide the frozen contract.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub memory_gb: u64,
    pub metal_available: bool,
    pub cuda_available: bool,
    pub vulkan_available: bool,
    pub cpu_available: bool,
}

/// Selects a usable device, honoring an explicit request.
///
/// `auto` follows the frozen platform priority `metal > cuda > vulkan > cpu`,
/// picking the first backend whose availability flag is set. An explicitly
/// requested backend that is unavailable is rejected with
/// [`ManifestError::DeviceUnavailable`] (exit 6) rather than silently falling
/// back, so a missing GPU is always observable.
///
/// # Errors
///
/// Returns [`ManifestError::InvalidDevice`] for unknown requests or
/// [`ManifestError::DeviceUnavailable`] when the requested backend is unavailable.
pub fn recommend_device(
    snapshot: ResourceSnapshot,
    requested: &str,
) -> Result<String, ManifestError> {
    // Data-driven priority table: platform differences are expressed purely via
    // the availability flags, never via platform branching in the matcher.
    let auto_priority: [(&str, bool); 4] = [
        ("metal", snapshot.metal_available),
        ("cuda", snapshot.cuda_available),
        ("vulkan", snapshot.vulkan_available),
        ("cpu", snapshot.cpu_available),
    ];
    match requested {
        "auto" => auto_priority
            .into_iter()
            .find_map(|(device, available)| available.then_some(device.to_owned()))
            .ok_or(ManifestError::DeviceUnavailable),
        "metal" if snapshot.metal_available => Ok("metal".to_owned()),
        "cuda" if snapshot.cuda_available => Ok("cuda".to_owned()),
        "vulkan" if snapshot.vulkan_available => Ok("vulkan".to_owned()),
        "cpu" if snapshot.cpu_available => Ok("cpu".to_owned()),
        "metal" | "cuda" | "vulkan" | "cpu" => Err(ManifestError::DeviceUnavailable),
        _ => Err(ManifestError::InvalidDevice),
    }
}

impl ModelCatalog {
    /// Creates a catalog after validating every manifest.
    ///
    /// # Errors
    ///
    /// Returns the first validation error found in the supplied manifests.
    pub fn new(manifests: Vec<ModelManifest>) -> Result<Self, ManifestError> {
        for manifest in &manifests {
            manifest.validate()?;
        }
        Ok(Self { manifests })
    }

    #[must_use]
    pub fn list(&self) -> Vec<ModelId> {
        vec![ModelId::OvisOcr2]
    }

    #[must_use]
    pub fn info(&self, installed_profiles: &[String]) -> ModelInfo {
        let mut profiles: Vec<String> = self
            .manifests
            .iter()
            .map(|manifest| manifest.profile.clone())
            .collect();
        profiles.sort();
        profiles.dedup();
        ModelInfo {
            model: ModelId::OvisOcr2,
            profiles,
            installed_profiles: installed_profiles.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMMUNITY_MMPROJ_SHA256, COMMUNITY_REVISION, community_asset_sizes, community_asset_urls,
        community_manifest,
    };
    use super::{
        InstallRequest, LocalModelState, ManifestError, ManifestIdentity, ModelCatalog,
        ModelManifest, ResourceSnapshot, UrlInstallRequest, cache_paths, default_cache_root_from,
        download_asset, file_sha256, install_from_files, install_from_urls,
        legacy_cache_root_fallback, move_verified_asset, recommend_device, recommend_profile,
        validate_runtime_adapter_compatibility, verify_local_state, verify_manifest_files,
    };
    use ocrx_core::ModelId;

    fn manifest() -> ModelManifest {
        ModelManifest {
            identity: ManifestIdentity {
                model: ModelId::OvisOcr2,
                revision: "rev-1".to_owned(),
                schema_version: 1,
            },
            profile: "compact".to_owned(),
            model_sha256: "a".repeat(64),
            mmproj_sha256: "b".repeat(64),
        }
    }

    #[test]
    fn community_manifest_is_pinned_for_all_profiles() {
        for profile in ["compact", "balanced", "quality", "reference"] {
            let manifest = community_manifest(profile).unwrap();
            assert_eq!(manifest.identity.revision, COMMUNITY_REVISION);
            assert_eq!(manifest.mmproj_sha256, COMMUNITY_MMPROJ_SHA256);
            assert!(community_asset_sizes(profile).is_some());
            assert!(manifest.validate().is_ok());
            let urls = community_asset_urls(profile).unwrap();
            assert!(urls.0.contains("huggingface.co"));
            assert!(urls.1.contains("mmproj-F16.gguf"));
        }
        assert_eq!(
            community_manifest("experimental"),
            Err(ManifestError::UnsupportedProfile)
        );
        assert!(community_asset_urls("experimental").is_none());
    }

    #[test]
    fn verify_manifest_recomputes_both_asset_hashes() {
        let root = std::env::temp_dir().join(format!("ocrx-model-verify-{}", std::process::id()));
        // Layout: <root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf and
        // <root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf (shared projector).
        let revision_dir = root.join("ocrx/models/ovisocr2/rev-1");
        let profile_dir = revision_dir.join("compact");
        let model_path = profile_dir.join("model.gguf");
        let mmproj_path = revision_dir.join("mmproj.gguf");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(&model_path, b"model").unwrap();
        std::fs::write(&mmproj_path, b"projector").unwrap();
        let mut valid = manifest();
        valid.model_sha256 = ocrx_core::sha256_file(&model_path).unwrap();
        valid.mmproj_sha256 = ocrx_core::sha256_file(&mmproj_path).unwrap();
        assert!(verify_manifest_files(&valid, &root).is_ok());
        valid.mmproj_sha256 = "a".repeat(64);
        assert_eq!(
            verify_manifest_files(&valid, &root),
            Err(ManifestError::HashMismatch)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_transaction_verifies_hashes_and_cleans_parts() {
        let root = std::env::temp_dir().join(format!("ocrx-model-install-{}", std::process::id()));
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        let model = source.join("model");
        let mmproj = source.join("mmproj");
        std::fs::write(&model, b"model").unwrap();
        std::fs::write(&mmproj, b"projector").unwrap();
        let hash = |path: &std::path::Path| ocrx_core::sha256_file(path).unwrap();
        let mut valid = manifest();
        valid.model_sha256 = hash(&model);
        valid.mmproj_sha256 = hash(&mmproj);
        let request = InstallRequest {
            manifest: valid.clone(),
            model_source: model.clone(),
            mmproj_source: mmproj.clone(),
            cache_root: root.join("cache"),
            offline: false,
            non_interactive: true,
            license_accepted: true,
            available_bytes: Some(100),
        };
        assert!(install_from_files(&request).is_ok());
        assert!(verify_manifest_files(&valid, &request.cache_root).is_ok());
        let mut bad = request;
        bad.manifest.model_sha256 = "b".repeat(64);
        assert_eq!(install_from_files(&bad), Err(ManifestError::HashMismatch));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_policy_rejects_offline_license_and_disk_failures() {
        let request = InstallRequest {
            manifest: manifest(),
            model_source: "missing".into(),
            mmproj_source: "missing".into(),
            cache_root: "cache".into(),
            offline: true,
            non_interactive: true,
            license_accepted: false,
            available_bytes: Some(0),
        };
        assert_eq!(install_from_files(&request), Err(ManifestError::Offline));
        let mut request = request;
        request.offline = false;
        assert_eq!(
            install_from_files(&request),
            Err(ManifestError::LicenseRequired)
        );
        request.license_accepted = true;
        assert_eq!(
            install_from_files(&request),
            Err(ManifestError::MissingAsset)
        );
    }

    #[test]
    fn download_policy_blocks_network_before_process_creation() {
        let root = std::env::temp_dir().join(format!("ocrx-download-{}", std::process::id()));
        assert_eq!(
            download_asset(
                "https://example.invalid/model.gguf",
                root.join("model.part"),
                None,
                true
            ),
            Err(ManifestError::Offline)
        );
        assert_eq!(
            download_asset("file:///tmp/model", root.join("model.part"), None, false),
            Err(ManifestError::Network)
        );
        // HTTPS-only provenance: plaintext http is refused like any other
        // non-pinned scheme (MINOR 12).
        assert_eq!(
            download_asset(
                "http://example.invalid/model.gguf",
                root.join("model.part"),
                None,
                false
            ),
            Err(ManifestError::Network)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn url_install_keeps_offline_and_invalid_url_transactions_side_effect_free() {
        let root = std::env::temp_dir().join(format!("ocrx-url-install-{}", std::process::id()));
        let request = UrlInstallRequest {
            manifest: manifest(),
            model_url: "https://example.invalid/model.gguf".to_owned(),
            mmproj_url: "https://example.invalid/mmproj.gguf".to_owned(),
            cache_root: root.clone(),
            offline: true,
            non_interactive: true,
            license_accepted: true,
            available_bytes: None,
        };
        assert_eq!(install_from_urls(&request), Err(ManifestError::Offline));
        let mut request = request;
        request.offline = false;
        request.model_url = "file:///not-supported".to_owned();
        assert_eq!(install_from_urls(&request), Err(ManifestError::Network));
        assert!(!root.exists());
    }

    #[test]
    fn manifest_accepts_only_valid_ovisocr2_profile_assets() {
        assert_eq!(manifest().validate(), Ok(()));
        let mut invalid = manifest();
        invalid.profile = "experimental".to_owned();
        assert_eq!(invalid.validate(), Err(ManifestError::UnsupportedProfile));
    }

    #[test]
    fn manifest_rejects_missing_revision_or_invalid_hashes() {
        let mut invalid = manifest();
        invalid.identity.revision.clear();
        assert_eq!(invalid.validate(), Err(ManifestError::InvalidAsset));
        let mut invalid = manifest();
        invalid.model_sha256 = "not-a-hash".to_owned();
        assert_eq!(invalid.validate(), Err(ManifestError::InvalidAsset));
    }

    #[test]
    fn runtime_and_adapter_compatibility_is_strictly_locked() {
        assert_eq!(
            validate_runtime_adapter_compatibility("0.1.152", "0.1.153", "ovisocr2"),
            Ok(())
        );
        assert_eq!(
            validate_runtime_adapter_compatibility("0.1.151", "0.1.153", "ovisocr2"),
            Err(ManifestError::InvalidAsset)
        );
        assert_eq!(
            validate_runtime_adapter_compatibility("0.1.152", "0.1.153", "other"),
            Err(ManifestError::InvalidAsset)
        );
    }

    #[test]
    fn runtime_compatibility_is_a_shared_single_source_of_truth() {
        // The shared compatibility tuple lives in ocrx_core so macOS and Windows
        // share one source of truth; validate_runtime_adapter_compatibility must
        // agree with it exactly (no scattered platform-specific constants).
        let compat = ocrx_core::RUNTIME_COMPATIBILITY;
        assert!(!compat.llama_cpp_2_version.is_empty());
        assert!(!compat.llama_cpp_sys_2_version.is_empty());
        assert!(!compat.adapter_id.is_empty());
        assert_eq!(
            validate_runtime_adapter_compatibility(
                compat.llama_cpp_2_version,
                compat.llama_cpp_sys_2_version,
                compat.adapter_id,
            ),
            Ok(())
        );
        // Any deviation from the shared tuple is rejected.
        assert_eq!(
            validate_runtime_adapter_compatibility(
                "0.0.0",
                compat.llama_cpp_sys_2_version,
                compat.adapter_id,
            ),
            Err(ManifestError::InvalidAsset)
        );
    }

    #[test]
    fn local_state_requires_model_and_mmproj_and_protects_shared_asset() {
        let state = LocalModelState {
            profile: "compact".to_owned(),
            revision: "rev-1".to_owned(),
            model_path: "compact.gguf".into(),
            mmproj_path: "shared-mmproj.gguf".into(),
            model_present: true,
            mmproj_present: true,
        };
        assert!(state.is_installed());
        assert_eq!(
            state.remove_allowed(true),
            Err(ManifestError::SharedAssetInUse)
        );
        assert_eq!(state.remove_allowed(false), Ok(()));
        assert!(
            !LocalModelState {
                model_present: true,
                mmproj_present: false,
                ..state
            }
            .is_installed()
        );
    }

    #[test]
    fn cache_paths_are_program_generated_and_verify_requires_a_complete_pair() {
        let root = std::path::Path::new("/cache");
        let (model, mmproj) = cache_paths(root, COMMUNITY_REVISION, "compact").unwrap();
        let state = LocalModelState {
            profile: "compact".to_owned(),
            revision: COMMUNITY_REVISION.to_owned(),
            model_path: model,
            mmproj_path: mmproj,
            model_present: true,
            mmproj_present: true,
        };
        assert_eq!(verify_local_state(&state, root), Ok(()));
        assert!(cache_paths(root, "../escape", "compact").is_err());
        assert!(
            verify_local_state(
                &LocalModelState {
                    model_path: "/tmp/model.gguf".into(),
                    ..state
                },
                root
            )
            .is_err()
        );
    }

    #[test]
    fn cache_paths_lay_out_assets_under_revision_with_shared_mmproj() {
        let root = std::path::Path::new("/cache");
        let revision = "8a22290a0c42dbcf84739d6d4c2763f877494ae0";
        let (model, mmproj) = cache_paths(root, revision, "compact").unwrap();
        assert_eq!(
            model,
            std::path::Path::new(
                "/cache/ocrx/models/ovisocr2/8a22290a0c42dbcf84739d6d4c2763f877494ae0/compact/model.gguf"
            )
        );
        assert_eq!(
            mmproj,
            std::path::Path::new(
                "/cache/ocrx/models/ovisocr2/8a22290a0c42dbcf84739d6d4c2763f877494ae0/mmproj.gguf"
            )
        );
    }

    #[test]
    fn cache_paths_share_mmproj_across_profiles_but_isolate_revisions() {
        let root = std::path::Path::new("/cache");
        let (compact_model, compact_mmproj) = cache_paths(root, "rev-a", "compact").unwrap();
        let (quality_model, quality_mmproj) = cache_paths(root, "rev-a", "quality").unwrap();
        // mmproj lives at the revision layer and is shared across profiles.
        assert_eq!(compact_mmproj, quality_mmproj);
        // model is per-profile.
        assert_ne!(compact_model, quality_model);
        // different revisions produce independent directories.
        let (compact_b_model, compact_b_mmproj) = cache_paths(root, "rev-b", "compact").unwrap();
        assert_ne!(compact_model, compact_b_model);
        assert_ne!(compact_mmproj, compact_b_mmproj);
    }

    #[test]
    fn cache_paths_rejects_path_escape_and_unknown_profiles() {
        let root = std::path::Path::new("/cache");
        assert!(cache_paths(root, "../escape", "compact").is_err());
        assert!(cache_paths(root, "/absolute/revision", "compact").is_err());
        assert!(cache_paths(root, "rev/with/slash", "compact").is_err());
        assert!(cache_paths(root, "", "compact").is_err());
        assert!(cache_paths(root, "rev-1", "unknown-profile").is_err());
    }

    #[test]
    fn default_cache_root_prefers_local_app_data_on_windows() {
        if !cfg!(target_os = "windows") {
            return;
        }
        let local = std::path::PathBuf::from("C:\\Users\\u\\AppData\\Local");
        assert_eq!(
            default_cache_root_from(
                Some(local.as_os_str()),
                Some(std::ffi::OsStr::new("C:\\Users\\u")),
                None,
            ),
            local
        );
    }

    #[test]
    fn default_cache_root_falls_back_to_user_profile_on_windows() {
        if !cfg!(target_os = "windows") {
            return;
        }
        assert_eq!(
            default_cache_root_from(None, Some(std::ffi::OsStr::new("C:\\Users\\u")), None),
            std::path::PathBuf::from("C:\\Users\\u")
                .join("AppData")
                .join("Local")
        );
    }

    #[test]
    fn default_cache_root_uses_library_caches_on_macos() {
        if !cfg!(target_os = "macos") {
            return;
        }
        assert_eq!(
            default_cache_root_from(None, None, Some(std::ffi::OsStr::new("/Users/u"))),
            std::path::PathBuf::from("/Users/u/Library/Caches")
        );
    }

    #[test]
    fn default_cache_root_never_falls_back_to_cwd_relative_dir() {
        // M-11: without any home environment (service accounts) the cache
        // root degrades to a temp-dir root — never the CWD-relative
        // `.ocrx-model-cache`, which would violate W1.3 and defeat the
        // absolute-path overlap checks.
        if cfg!(target_os = "windows") {
            assert_ne!(
                default_cache_root_from(None, None, Some(std::ffi::OsStr::new("/Users/u"))),
                std::path::PathBuf::from(".ocrx-model-cache")
            );
            assert!(
                default_cache_root_from(None, None, None).is_absolute(),
                "service-account fallback must be an absolute temp-dir root"
            );
            return;
        }
        if cfg!(target_os = "macos") {
            assert_ne!(
                default_cache_root_from(
                    Some(std::ffi::OsStr::new("C:\\Local")),
                    Some(std::ffi::OsStr::new("C:\\Users\\u")),
                    None,
                ),
                std::path::PathBuf::from(".ocrx-model-cache")
            );
            return;
        }
        assert!(
            default_cache_root_from(None, None, None).is_absolute(),
            "service-account fallback must be an absolute temp-dir root"
        );
    }

    #[test]
    fn legacy_cache_root_fallback_prefers_populated_legacy_over_empty_platform_root() {
        // BLK-5 regression guard: the fallback must detect the REAL pre-WP.1
        // layout (`<legacy>/ovisocr2/<profile>/model.gguf`, no `ocrx/models`
        // layer) — the previous probe looked for `ocrx/models` under the
        // legacy root, which no existing install ever had, so the guard never
        // fired and upgraded users' weights were orphaned.
        let base = std::env::temp_dir().join(format!("ocrx-legacy-cache-{}", std::process::id()));
        let platform = base.join("platform");
        let legacy = base.join("legacy");
        let _ = std::fs::remove_dir_all(&base);

        // Real old layout: per-profile directories with model+mmproj pairs.
        let compact = legacy.join("ovisocr2").join("compact");
        std::fs::create_dir_all(&compact).unwrap();
        std::fs::write(compact.join("model.gguf"), b"legacy-compact-model").unwrap();
        std::fs::write(compact.join("mmproj.gguf"), b"legacy-mmproj").unwrap();

        // Empty platform root + old-layout legacy root -> legacy detected.
        assert_eq!(
            legacy_cache_root_fallback(&platform, &legacy),
            Some(legacy.clone())
        );
        // Populated platform root always wins.
        std::fs::create_dir_all(platform.join("ocrx").join("models")).unwrap();
        assert_eq!(legacy_cache_root_fallback(&platform, &legacy), None);
        // Neither populated -> no fallback (fresh install uses platform root).
        let empty_legacy = base.join("empty-legacy");
        assert_eq!(legacy_cache_root_fallback(&platform, &empty_legacy), None);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_legacy_cache_moves_verified_weights_and_skips_tampered_ones() {
        // BLK-5: migration must (a) move hash-verified legacy files into the
        // new layout, (b) never move a file whose bytes do not match the
        // pinned manifest hash, and (c) work against the real pre-WP.1 shape.
        // The seam below mirrors migrate_legacy_cache with an injected
        // manifest so the test does not need multi-GB real weights.
        let base = std::env::temp_dir().join(format!("ocrx-legacy-migrate-{}", std::process::id()));
        let platform = base.join("platform");
        let legacy = base.join("legacy");
        let _ = std::fs::remove_dir_all(&base);

        let model_bytes = b"tiny legacy compact model bytes".to_vec();
        let mmproj_bytes = b"tiny legacy mmproj bytes".to_vec();
        let manifest = ModelManifest {
            identity: ManifestIdentity {
                model: ModelId::OvisOcr2,
                revision: COMMUNITY_REVISION.to_owned(),
                schema_version: 1,
            },
            profile: "compact".to_owned(),
            model_sha256: ocrx_core::sha256(&model_bytes),
            mmproj_sha256: ocrx_core::sha256(&mmproj_bytes),
        };
        let compact = legacy.join("ovisocr2").join("compact");
        std::fs::create_dir_all(&compact).unwrap();
        std::fs::write(compact.join("model.gguf"), &model_bytes).unwrap();
        std::fs::write(compact.join("mmproj.gguf"), &mmproj_bytes).unwrap();

        let legacy_models = legacy.join("ovisocr2");
        let (target_model, target_mmproj) =
            cache_paths(&platform, &manifest.identity.revision, "compact").unwrap();
        // Verified model moves.
        assert_eq!(
            file_sha256(&compact.join("model.gguf")).unwrap(),
            manifest.model_sha256
        );
        move_verified_asset(
            &legacy_models.join("compact").join("model.gguf"),
            &target_model,
        )
        .unwrap();
        move_verified_asset(
            &legacy_models.join("compact").join("mmproj.gguf"),
            &target_mmproj,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(&target_model).unwrap(),
            model_bytes,
            "moved model keeps its bytes"
        );
        assert_eq!(
            std::fs::read(&target_mmproj).unwrap(),
            mmproj_bytes,
            "moved mmproj keeps its bytes"
        );
        assert!(
            !compact.join("model.gguf").exists(),
            "rename moved the source"
        );
        // A tampered sibling fails migrate_legacy_cache's hash gate, so it is
        // never even offered to the move seam: assert the gate predicate
        // itself (the gate, not the mover, is what protects tampered files).
        let balanced = legacy.join("ovisocr2").join("balanced");
        std::fs::create_dir_all(&balanced).unwrap();
        std::fs::write(balanced.join("model.gguf"), b"tampered").unwrap();
        assert_ne!(
            file_sha256(&balanced.join("model.gguf")).unwrap(),
            manifest.model_sha256,
            "tampered bytes must fail the pinned-hash gate"
        );
        assert!(balanced.join("model.gguf").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn catalog_lists_only_ovisocr2_and_deduplicates_profiles() {
        let catalog = ModelCatalog::new(vec![
            manifest(),
            ModelManifest {
                profile: "quality".to_owned(),
                ..manifest()
            },
        ])
        .unwrap();
        assert_eq!(catalog.list(), vec![ModelId::OvisOcr2]);
        assert_eq!(
            catalog.info(&["compact".to_owned()]).profiles,
            vec!["compact", "quality"]
        );
        assert_eq!(
            catalog.info(&["compact".to_owned()]).installed_profiles,
            vec!["compact"]
        );
    }

    #[test]
    fn profile_recommendation_respects_memory_floor_and_quality_state() {
        assert_eq!(
            recommend_profile(4, &["compact"]),
            Err(ManifestError::InsufficientMemory)
        );
        assert_eq!(
            recommend_profile(8, &["compact", "quality"]),
            Ok("compact".to_owned())
        );
        assert_eq!(
            recommend_profile(16, &["balanced", "quality"]),
            Ok("balanced".to_owned())
        );
        assert_eq!(
            recommend_profile(24, &["quality"]),
            Ok("quality".to_owned())
        );
        assert_eq!(
            recommend_profile(16, &["compact"]),
            Ok("compact".to_owned())
        );
        assert_eq!(
            recommend_profile(16, &[]),
            Err(ManifestError::NoPassingProfile)
        );
    }

    #[test]
    #[allow(clippy::similar_names)] // platform-prefixed fixture names are intentionally parallel
    fn recommend_device_uses_platform_aware_priority_and_rejects_unavailable_devices() {
        // Snapshots model each platform/device situation via the availability
        // flags rather than branching on the platform string, so the priority
        // table stays data-driven.
        let macos = ResourceSnapshot {
            memory_gb: 16,
            metal_available: true,
            cuda_available: false,
            vulkan_available: false,
            cpu_available: true,
        };
        let windows_gpu = ResourceSnapshot {
            memory_gb: 24,
            metal_available: false,
            cuda_available: true,
            vulkan_available: true,
            cpu_available: true,
        };
        let windows_vulkan_only = ResourceSnapshot {
            cuda_available: false,
            ..windows_gpu
        };
        let windows_cpu = ResourceSnapshot {
            cuda_available: false,
            vulkan_available: false,
            ..windows_gpu
        };

        // `auto` follows the frozen priority: metal > cuda > vulkan > cpu.
        assert_eq!(recommend_device(macos, "auto"), Ok("metal".to_owned()));
        assert_eq!(recommend_device(windows_gpu, "auto"), Ok("cuda".to_owned()));
        assert_eq!(
            recommend_device(windows_vulkan_only, "auto"),
            Ok("vulkan".to_owned())
        );
        assert_eq!(recommend_device(windows_cpu, "auto"), Ok("cpu".to_owned()));
        assert_eq!(
            recommend_device(
                ResourceSnapshot {
                    metal_available: false,
                    cuda_available: false,
                    vulkan_available: false,
                    cpu_available: false,
                    ..macos
                },
                "auto"
            ),
            Err(ManifestError::DeviceUnavailable)
        );

        // Explicit device honors availability; an explicitly requested but
        // unavailable GPU backend is rejected (exit 6), never silently
        // falling back to another backend.
        assert_eq!(recommend_device(macos, "metal"), Ok("metal".to_owned()));
        assert_eq!(recommend_device(windows_gpu, "cuda"), Ok("cuda".to_owned()));
        assert_eq!(
            recommend_device(windows_gpu, "vulkan"),
            Ok("vulkan".to_owned())
        );
        assert_eq!(recommend_device(windows_cpu, "cpu"), Ok("cpu".to_owned()));
        assert_eq!(
            recommend_device(windows_cpu, "cuda"),
            Err(ManifestError::DeviceUnavailable)
        );
        assert_eq!(
            recommend_device(windows_cpu, "vulkan"),
            Err(ManifestError::DeviceUnavailable)
        );
        assert_eq!(
            recommend_device(
                ResourceSnapshot {
                    metal_available: false,
                    ..macos
                },
                "metal"
            ),
            Err(ManifestError::DeviceUnavailable)
        );

        // Unknown device string is a parse-level rejection, not availability.
        assert_eq!(
            recommend_device(macos, "tpu"),
            Err(ManifestError::InvalidDevice)
        );
    }
}
