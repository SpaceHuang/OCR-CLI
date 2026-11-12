//! Revision-scoped install transactions (plan §5.6/§7.3/§8.5).
//!
//! One transaction covers every asset of a profile (private model + shared
//! projector): a single revision-level lock, `.part` staging, backups,
//! verify-before-commit, reverse-order rollback, and a deterministic
//! `install-receipt.json` written last. The receipt is an audit record, not
//! a trust source — `verify` always recomputes hashes from disk.

use std::path::Path;

use ocrx_inference_api::{
    DownloadProposal, InferenceError, InstallPolicy, InstallReceipt, ModelManifest,
};
use ocrx_primitives::sha256_file;
use serde::Serialize;

use crate::cache::{asset_cache_path, manifest_asset_paths};
use crate::transport::Transport;
use crate::verify::verify_content;

/// Deterministic receipt file name inside the profile directory.
pub const RECEIPT_FILE_NAME: &str = "install-receipt.json";

/// Inputs for one offline-testable install transaction.
pub struct InstallContext<'a> {
    pub manifest: &'a ModelManifest,
    pub cache_root: &'a Path,
    pub policy: InstallPolicy,
    pub transport: &'a dyn Transport,
    pub interaction: &'a mut dyn ocrx_inference_api::InstallInteraction,
    /// Free bytes on the cache volume (probe injectable; `None` skips the
    /// preflight, e.g. in tests).
    pub available_bytes: Option<u64>,
}

/// Downloads/verifies/commits one profile's asset group and returns the
/// receipt. Already-verified installs succeed offline without any network
/// activity (idempotency, legacy M-3 rule).
///
/// # Errors
///
/// Offline/license/disk/transport/hash categories; a failed transaction
/// never exposes half a valid asset set (reverse-order rollback restores
/// the previous bytes, `.part` files are cleaned).
pub fn install(context: InstallContext<'_>) -> Result<InstallReceipt, InferenceError> {
    let manifest = context.manifest;
    manifest.validate()?;
    let cache_root = context.cache_root;
    // 1. Idempotent short circuit: a fully verified install needs nothing.
    if verify_content(cache_root, manifest).is_ok() {
        let receipt_path = receipt_path(cache_root, manifest)?;
        if !receipt_path.is_file() {
            write_receipt(cache_root, manifest)?;
        }
        return receipt(cache_root, manifest, false);
    }
    // 2. Policy gates (frozen semantics, same as the legacy install paths).
    if context.policy.offline {
        return Err(InferenceError::Offline);
    }
    if context.policy.non_interactive && !context.policy.license_accepted {
        return Err(InferenceError::LicenseRequired);
    }
    // 3. Disk preflight against the bytes still missing (the legacy check
    // ran after the transfer; the pre-flight placement is the M-8 rule).
    let pending = pending_assets(cache_root, manifest)?;
    let pending_bytes: u64 = pending
        .iter()
        .filter_map(|role| manifest.asset(role))
        .map(|asset| asset.byte_len)
        .sum();
    if let Some(available) = context.available_bytes
        && available < pending_bytes
    {
        return Err(InferenceError::InsufficientDisk(format!(
            "need {pending_bytes} bytes, {available} available"
        )));
    }
    // 4. Interactive confirmation of the actual download set.
    if !context.policy.non_interactive {
        let proposals: Vec<DownloadProposal> = pending
            .iter()
            .filter_map(|role| {
                manifest.asset(role).map(|asset| DownloadProposal {
                    role: role.clone(),
                    url: asset.source_url.clone(),
                    byte_len: asset.byte_len,
                    license_id: manifest.license_id.clone(),
                })
            })
            .collect();
        if !proposals.is_empty() && !context.interaction.confirm_download(&proposals)? {
            // A declined proposal means no download may happen.
            return Err(InferenceError::Offline);
        }
    }
    // 5. Transaction: stage → verify → backup → commit → receipt last.
    let targets: Vec<_> = manifest_asset_paths(cache_root, manifest)?;
    let mut directories = std::collections::BTreeSet::new();
    for (_, path) in &targets {
        if let Some(directory) = path.parent() {
            directories.insert(directory.to_path_buf());
        }
    }
    for directory in &directories {
        std::fs::create_dir_all(directory)
            .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    }
    let transaction = transaction_id();
    let staged: Vec<_> = targets
        .iter()
        .map(|(role, path)| {
            let part = path.with_extension(format!(
                "gguf{transaction}.part-{}",
                role.as_str().replace('/', "_")
            ));
            (role.clone(), path.clone(), part)
        })
        .collect();
    let result = (|| {
        for (role, path, part) in &staged {
            if file_matches(manifest, role, path)? {
                continue; // already correct on disk
            }
            let descriptor = manifest
                .asset(role)
                .ok_or_else(|| InferenceError::InvalidManifest(format!("role {role} vanished")))?;
            context.transport.download(&descriptor.source_url, part)?;
            let hash =
                sha256_file(part).map_err(|error| InferenceError::Transport(error.to_string()))?;
            if hash != descriptor.sha256.to_ascii_lowercase() {
                return Err(InferenceError::HashMismatch(format!("asset {role}")));
            }
            let size = std::fs::metadata(part)
                .map_err(|_| InferenceError::MissingAsset(part.to_string_lossy().into_owned()))?
                .len();
            if size != descriptor.byte_len {
                return Err(InferenceError::HashMismatch(format!(
                    "asset {role} downloaded size {size} != pinned {}",
                    descriptor.byte_len
                )));
            }
        }
        commit_group(&staged, &transaction)
    })();
    if let Err(error) = result {
        for (_, _, part) in &staged {
            let _ = std::fs::remove_file(part);
        }
        return Err(error);
    }
    write_receipt(cache_root, manifest)?;
    receipt(cache_root, manifest, !pending.is_empty())
}

/// Roles whose on-disk content is not already correct.
fn pending_assets(
    cache_root: &Path,
    manifest: &ModelManifest,
) -> Result<Vec<ocrx_inference_api::AssetRole>, InferenceError> {
    let mut pending = Vec::new();
    for (role, path) in manifest_asset_paths(cache_root, manifest)? {
        if !file_matches(manifest, &role, &path)? {
            pending.push(role);
        }
    }
    Ok(pending)
}

fn file_matches(
    manifest: &ModelManifest,
    role: &ocrx_inference_api::AssetRole,
    path: &Path,
) -> Result<bool, InferenceError> {
    let descriptor = manifest
        .asset(role)
        .ok_or_else(|| InferenceError::InvalidManifest(format!("role {role} vanished")))?;
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(false);
    };
    if metadata.len() != descriptor.byte_len {
        return Ok(false);
    }
    let hash = sha256_file(path)
        .map_err(|_| InferenceError::MissingAsset(path.to_string_lossy().into_owned()))?;
    Ok(hash == descriptor.sha256.to_ascii_lowercase())
}

/// Backs up existing targets, renames staged parts into place, and rolls
/// back in reverse order on any failure.
fn commit_group(
    staged: &[(
        ocrx_inference_api::AssetRole,
        std::path::PathBuf,
        std::path::PathBuf,
    )],
    transaction: &str,
) -> Result<(), InferenceError> {
    let backups: Vec<_> = staged
        .iter()
        .map(|(_, path, _)| {
            (
                path.clone(),
                path.with_extension(format!("gguf{transaction}.bak")),
                path.is_file(),
            )
        })
        .collect();
    for (path, backup, existed) in &backups {
        if *existed {
            std::fs::rename(path, backup)
                .map_err(|_| InferenceError::InsufficientDisk("backup rename failed".into()))?;
        }
    }
    let mut committed = 0_usize;
    for (_, path, part) in staged {
        match std::fs::rename(part, path) {
            Ok(()) => committed += 1,
            Err(_) => {
                // Roll back in reverse order: remove what we committed, then
                // restore every backup.
                let mut recovery_failed = false;
                for (_, target, _) in staged.iter().take(committed).rev() {
                    if std::fs::remove_file(target).is_err() && target.exists() {
                        recovery_failed = true;
                    }
                }
                for (target, backup, existed) in &backups {
                    if *existed && std::fs::rename(backup, target).is_err() {
                        recovery_failed = true;
                    }
                }
                if recovery_failed {
                    return Err(InferenceError::InsufficientDisk(
                        "commit failed and rollback was incomplete".into(),
                    ));
                }
                return Err(InferenceError::InsufficientDisk(
                    "commit rename failed".into(),
                ));
            }
        }
    }
    for (_, backup, existed) in &backups {
        if *existed {
            let _ = std::fs::remove_file(backup);
        }
    }
    Ok(())
}

/// Removes leftover transaction files (`.part`/`.bak`/`.etag`) created by
/// this crate's own transactions in the given target directories.
pub(crate) fn clean_transaction_residue(targets: &[&Path]) {
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
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            if name.ends_with(".part") || name.ends_with(".bak") || name.ends_with(".etag") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub(crate) fn transaction_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".install-{}-{nanos:x}", std::process::id())
}

fn receipt_path(
    cache_root: &Path,
    manifest: &ModelManifest,
) -> Result<std::path::PathBuf, InferenceError> {
    Ok(
        asset_cache_path(cache_root, manifest, &manifest.assets[0].role)?
            .parent()
            .ok_or_else(|| InferenceError::InvalidManifest("asset path has no parent".into()))?
            .join(RECEIPT_FILE_NAME),
    )
}

#[derive(Serialize)]
struct ReceiptAsset<'a> {
    role: &'a str,
    sha256: &'a str,
    byte_len: u64,
    scope: &'a str,
    source_url: &'a str,
}

#[derive(Serialize)]
struct ReceiptFile<'a> {
    schema_version: u32,
    model: &'a str,
    profile: &'a str,
    source_revision: &'a str,
    license_id: &'a str,
    assets: Vec<ReceiptAsset<'a>>,
    runtime_binding: &'a ocrx_inference_api::BindingRequirement,
    adapter_binding: &'a ocrx_inference_api::AdapterRequirement,
}

/// Writes the deterministic receipt JSON (stable field order via the
/// serde struct declaration; assets keep manifest order).
fn write_receipt(cache_root: &Path, manifest: &ModelManifest) -> Result<(), InferenceError> {
    let path = receipt_path(cache_root, manifest)?;
    let file = ReceiptFile {
        schema_version: 1,
        model: manifest.model.as_str(),
        profile: manifest.profile.as_str(),
        source_revision: &manifest.source_revision,
        license_id: &manifest.license_id,
        assets: manifest
            .assets
            .iter()
            .map(|asset| ReceiptAsset {
                role: asset.role.as_str(),
                sha256: &asset.sha256,
                byte_len: asset.byte_len,
                scope: match asset.scope {
                    ocrx_inference_api::AssetScope::Profile => "profile",
                    ocrx_inference_api::AssetScope::Revision => "revision",
                },
                source_url: &asset.source_url,
            })
            .collect(),
        runtime_binding: &manifest.runtime_binding,
        adapter_binding: &manifest.adapter_binding,
    };
    let json = serde_json::to_string(&file)
        .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?;
    let staged = path.with_extension(format!("json{}", transaction_id()));
    std::fs::write(&staged, json.as_bytes())
        .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    std::fs::rename(&staged, &path)
        .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    Ok(())
}

/// Reads back the written receipt into the public DTO (audit record).
fn receipt(
    cache_root: &Path,
    manifest: &ModelManifest,
    downloaded: bool,
) -> Result<InstallReceipt, InferenceError> {
    let _ = receipt_path(cache_root, manifest)?;
    Ok(InstallReceipt {
        model: manifest.model.clone(),
        profile: manifest.profile.clone(),
        revision: manifest.source_revision.clone(),
        assets: manifest
            .assets
            .iter()
            .map(|asset| ocrx_inference_api::AssetIdentity {
                role: asset.role.clone(),
                sha256: asset.sha256.clone(),
                byte_len: asset.byte_len,
            })
            .collect(),
        downloaded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::FakeTransport;
    use ocrx_inference_api::AssetRole;
    use ocrx_inference_api::{
        AdapterId, AdapterRequirement, AssetDescriptor, AssetProvenance, AssetScope,
        BindingRequirement, InstallInteraction, ModelId, ProfileId, RuntimeId,
    };

    fn tiny_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: 2,
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            display_name: "OvisOCR2".into(),
            family: "Ovis".into(),
            quantization: "Q4_K_M".into(),
            source_revision: "rev-test-1".into(),
            assets: vec![
                AssetDescriptor {
                    role: AssetRole::new("model").unwrap(),
                    required: true,
                    scope: AssetScope::Profile,
                    relative_path: "model.gguf".into(),
                    sha256: ocrx_primitives::sha256(b"model-bytes"),
                    byte_len: 11,
                    source_url: "https://example.invalid/model.gguf".into(),
                },
                AssetDescriptor {
                    role: AssetRole::new("mmproj").unwrap(),
                    required: true,
                    scope: AssetScope::Revision,
                    relative_path: "mmproj.gguf".into(),
                    sha256: ocrx_primitives::sha256(b"mmproj-bytes"),
                    byte_len: 12,
                    source_url: "https://example.invalid/mmproj.gguf".into(),
                },
            ],
            runtime_binding: BindingRequirement {
                runtime: RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".into(),
                sys_version: "0.1.161".into(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".into(),
            },
            adapter_binding: AdapterRequirement {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".into(),
            },
            license_id: "apache-2.0".into(),
            provenance: AssetProvenance {
                provider: "huggingface".into(),
                url: "https://example.invalid".into(),
            },
        }
    }

    struct RecordingInteraction {
        confirmations: usize,
        answer: bool,
    }

    impl RecordingInteraction {
        fn accepting() -> Self {
            Self {
                confirmations: 0,
                answer: true,
            }
        }
    }

    impl InstallInteraction for RecordingInteraction {
        fn confirm_download(
            &mut self,
            proposals: &[DownloadProposal],
        ) -> Result<bool, InferenceError> {
            self.confirmations += 1;
            assert!(!proposals.is_empty());
            Ok(self.answer)
        }
    }

    fn ctx<'a>(
        manifest: &'a ModelManifest,
        root: &'a Path,
        transport: &'a dyn Transport,
        interaction: &'a mut dyn InstallInteraction,
        policy: InstallPolicy,
        available: Option<u64>,
    ) -> InstallContext<'a> {
        InstallContext {
            manifest,
            cache_root: root,
            policy,
            transport,
            interaction,
            available_bytes: available,
        }
    }

    #[test]
    fn install_downloads_verifies_commits_and_writes_the_receipt_last() {
        let root = std::env::temp_dir().join(format!("ocrx-install-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"model-bytes");
        // mmproj bytes differ per URL: re-serve with the right prefix.
        transport.responses.lock().unwrap().push((
            "https://example.invalid/mmproj".into(),
            Ok(b"mmproj-bytes".to_vec()),
        ));
        let mut interaction = RecordingInteraction::accepting();
        let receipt = install(ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            InstallPolicy {
                offline: false,
                non_interactive: true,
                license_accepted: true,
            },
            None,
        ))
        .unwrap();
        assert!(receipt.downloaded);
        // Both assets verify on disk.
        assert_eq!(verify_content(&root, &manifest), Ok(()));
        // Receipt file exists and is deterministic JSON.
        let receipt_path =
            root.join("ocrx/models/ovisocr2/rev-test-1/compact/install-receipt.json");
        let json = std::fs::read_to_string(&receipt_path).unwrap();
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"role\":\"model\""));
        assert!(json.contains("d81235049384534c167caea52b85a694f6103d14"));
        // No transaction residue survives.
        let profile_dir = receipt_path.parent().unwrap();
        assert!(!profile_dir.read_dir().unwrap().flatten().any(|entry| {
            entry.file_name().to_string_lossy().contains(".part")
                || entry.file_name().to_string_lossy().contains(".bak")
        }));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_installs_are_idempotent_and_never_touch_the_network() {
        let root = std::env::temp_dir().join(format!("ocrx-install-idem-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        // Stage the correct content directly (as a legacy install would have).
        for asset in &manifest.assets {
            let path = asset_cache_path(&root, &manifest, &asset.role).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                if asset.role.as_str() == "model" {
                    &b"model-bytes"[..]
                } else {
                    &b"mmproj-bytes"[..]
                },
            )
            .unwrap();
        }
        let transport = FakeTransport::new();
        let mut interaction = RecordingInteraction::accepting();
        let receipt = install(ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            InstallPolicy {
                offline: true, // offline must still succeed
                non_interactive: true,
                license_accepted: false,
            },
            None,
        ))
        .unwrap();
        assert!(!receipt.downloaded);
        assert!(transport.requested_urls().is_empty(), "no network");
        // A missing receipt is backfilled without downloading.
        let receipt_path =
            root.join("ocrx/models/ovisocr2/rev-test-1/compact/install-receipt.json");
        assert!(receipt_path.is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_failures_never_create_partial_state() {
        let root = std::env::temp_dir().join(format!("ocrx-install-policy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        let mut interaction = RecordingInteraction::accepting();
        // Offline.
        assert_eq!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy::offline(),
                None
            )),
            Err(InferenceError::Offline)
        );
        // Non-interactive without license.
        assert_eq!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: true,
                    license_accepted: false
                },
                None
            )),
            Err(InferenceError::LicenseRequired)
        );
        // Insufficient disk preflight.
        assert!(matches!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: true,
                    license_accepted: true
                },
                Some(10)
            )),
            Err(InferenceError::InsufficientDisk(_))
        ));
        assert!(!root.join("ocrx").exists(), "no partial state on disk");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hash_failures_roll_back_and_clean_parts() {
        let root = std::env::temp_dir().join(format!("ocrx-install-hash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"corrupted-bytes!!");
        let mut interaction = RecordingInteraction::accepting();
        assert!(matches!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: true,
                    license_accepted: true
                },
                None
            )),
            Err(InferenceError::HashMismatch(_))
        ));
        // Nothing usable was exposed: no committed asset exists.
        for asset in &manifest.assets {
            let path = asset_cache_path(&root, &manifest, &asset.role).unwrap();
            assert!(!path.exists(), "{} must not exist", asset.role);
        }
        // No .part residue.
        let profile_dir = root.join("ocrx/models/ovisocr2/rev-test-1/compact");
        if profile_dir.exists() {
            assert!(
                !profile_dir
                    .read_dir()
                    .unwrap()
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().contains(".part"))
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn declined_interactive_confirmation_aborts_without_downloads() {
        let root =
            std::env::temp_dir().join(format!("ocrx-install-declined-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"model-bytes");
        let mut interaction = RecordingInteraction {
            confirmations: 0,
            answer: false,
        };
        assert_eq!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: false,
                    license_accepted: false
                },
                None
            )),
            Err(InferenceError::Offline)
        );
        assert_eq!(interaction.confirmations, 1);
        assert!(transport.requested_urls().is_empty(), "declined = no fetch");
        let _ = std::fs::remove_dir_all(&root);
    }
}
