//! Local install-state detection and full content verification (plan §8.5).
//!
//! Presence checks never hash large files; only `verify_*` recompute
//! SHA-256 and sizes against the pinned manifest.

use std::path::Path;

use ocrx_inference_api::{AssetRole, InferenceError, ModelManifest};
use ocrx_primitives::sha256_file;

use crate::cache::asset_cache_path;

/// One asset's observed cache state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPresence {
    pub role: AssetRole,
    pub path: std::path::PathBuf,
    pub present: bool,
    pub size_matches: bool,
}

/// Checks which manifest assets exist on disk (no hashing).
///
/// # Errors
///
/// Returns the manifest/path resolution errors from the cache layout.
pub fn presence(
    cache_root: &Path,
    manifest: &ModelManifest,
) -> Result<Vec<AssetPresence>, InferenceError> {
    let mut states = Vec::new();
    for (role, path) in crate::cache::manifest_asset_paths(cache_root, manifest)? {
        let byte_len = manifest.asset(&role).map(|asset| asset.byte_len);
        let size_matches = std::fs::metadata(&path)
            .map(|metadata| Some(metadata.len()) == byte_len)
            .unwrap_or(false);
        states.push(AssetPresence {
            role,
            path,
            present: size_matches,
            size_matches,
        });
    }
    Ok(states)
}

/// Whether every required asset is present with the pinned size (no hash).
///
/// # Errors
///
/// Returns the cache layout errors.
pub fn is_installed(cache_root: &Path, manifest: &ModelManifest) -> Result<bool, InferenceError> {
    Ok(presence(cache_root, manifest)?
        .into_iter()
        .all(|state| state.present))
}

/// Full content verification: required roles present, sizes exact, SHA-256
/// recomputed from disk. Works without any receipt (a legal legacy install
/// verifies purely by content).
///
/// # Errors
///
/// [`InferenceError::MissingAsset`] for absent files and
/// [`InferenceError::HashMismatch`] for a size or digest mismatch.
pub fn verify_content(cache_root: &Path, manifest: &ModelManifest) -> Result<(), InferenceError> {
    for (role, path) in crate::cache::manifest_asset_paths(cache_root, manifest)? {
        let descriptor = manifest
            .asset(&role)
            .ok_or_else(|| InferenceError::InvalidManifest(format!("role {role} vanished")))?;
        let metadata = std::fs::metadata(&path)
            .map_err(|_| InferenceError::MissingAsset(path.to_string_lossy().into_owned()))?;
        if metadata.len() != descriptor.byte_len {
            return Err(InferenceError::HashMismatch(format!(
                "asset {role} size {} != pinned {}",
                metadata.len(),
                descriptor.byte_len
            )));
        }
        let hash = sha256_file(&path).map_err(|_| {
            InferenceError::MissingAsset(path.to_string_lossy().into_owned())
        })?;
        if hash != descriptor.sha256.to_ascii_lowercase() {
            return Err(InferenceError::HashMismatch(format!("asset {role}")));
        }
    }
    Ok(())
}

/// Verifies the manifest binding values through the injected port (RBLK-9):
/// models only call the port; the registry-backed implementation lives in
/// `ocrx-inference`.
pub fn verify_binding(
    manifest: &ModelManifest,
    verifier: &dyn ocrx_inference_api::BindingVerifier,
) -> Result<(), InferenceError> {
    verifier.verify_binding(manifest)
}

/// Full `verify`: content + binding. Never downloads, never loads.
pub fn verify_profile(
    cache_root: &Path,
    manifest: &ModelManifest,
    verifier: &dyn ocrx_inference_api::BindingVerifier,
) -> Result<(), InferenceError> {
    verify_content(cache_root, manifest)?;
    verify_binding(manifest, verifier)
}

/// Resolves the on-disk path of one asset (for install/remove bookkeeping).
pub fn asset_path(
    cache_root: &Path,
    manifest: &ModelManifest,
    role: &AssetRole,
) -> Result<std::path::PathBuf, InferenceError> {
    asset_cache_path(cache_root, manifest, role)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::ovisocr2::ovisocr2_manifest;
    use ocrx_inference_api::{
        AdapterId, AdapterRequirement, AssetDescriptor, AssetProvenance, AssetScope,
        BindingRequirement, ModelId, ProfileId, RuntimeId,
    };

    struct AcceptAll;

    impl ocrx_inference_api::BindingVerifier for AcceptAll {
        fn verify_binding(&self, _manifest: &ModelManifest) -> Result<(), InferenceError> {
            Ok(())
        }
    }

    struct RejectAll;

    impl ocrx_inference_api::BindingVerifier for RejectAll {
        fn verify_binding(&self, _manifest: &ModelManifest) -> Result<(), InferenceError> {
            Err(InferenceError::IncompatibleBinding("sys".into()))
        }
    }

    /// Synthetic two-role manifest with tiny pinned sizes (real Ovis assets
    /// are hundreds of MB; the layout rules are what these tests exercise).
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
                    sha256: "0".repeat(64),
                    byte_len: 12,
                    source_url: "https://example.invalid/model.gguf".into(),
                },
                AssetDescriptor {
                    role: AssetRole::new("mmproj").unwrap(),
                    required: true,
                    scope: AssetScope::Revision,
                    relative_path: "mmproj.gguf".into(),
                    sha256: "0".repeat(64),
                    byte_len: 9,
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

    /// Writes correct content for every asset and re-pins the manifest
    /// hashes to the generated bytes.
    fn install_fixture(root: &Path, manifest: &mut ModelManifest, corrupt_model: bool) {
        let paths = crate::cache::manifest_asset_paths(root, manifest).unwrap();
        let mut contents = Vec::new();
        for (role, path) in &paths {
            let byte_len = manifest.asset(role).unwrap().byte_len;
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut bytes = vec![b'x'; byte_len as usize];
            if corrupt_model && role.as_str() == "model" {
                bytes[0] ^= 0xff;
            }
            std::fs::write(path, &bytes).unwrap();
            contents.push((role.clone(), bytes));
        }
        if !corrupt_model {
            for asset in &mut manifest.assets {
                let bytes = contents
                    .iter()
                    .find(|(role, _)| role == &asset.role)
                    .map(|(_, bytes)| bytes)
                    .unwrap();
                asset.sha256 = ocrx_primitives::sha256(bytes);
            }
        }
    }

    #[test]
    fn presence_reports_missing_and_sized_assets_without_hashing() {
        let root = std::env::temp_dir().join(format!("ocrx-models-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        // Nothing installed.
        let states = presence(&root, &manifest).unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|state| !state.present));
        assert!(!is_installed(&root, &manifest).unwrap());
        // Partially installed (model only, wrong size).
        let model_path =
            crate::cache::asset_cache_path(&root, &manifest, &AssetRole::new("model").unwrap())
                .unwrap();
        std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
        std::fs::write(&model_path, b"wrong-size").unwrap();
        let states = presence(&root, &manifest).unwrap();
        let model_state = states.iter().find(|s| s.role.as_str() == "model").unwrap();
        assert!(!model_state.present);
        assert!(!model_state.size_matches);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_content_checks_sizes_hashes_and_passes_receipt_free_installs() {
        let root = std::env::temp_dir().join(format!(
            "ocrx-models-verify-content-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut manifest = tiny_manifest();
        // Build a tiny install whose content defines the pinned hashes.
        install_fixture(&root, &mut manifest, false);
        // No receipt exists — content verification alone must pass.
        assert_eq!(verify_content(&root, &manifest), Ok(()));
        assert_eq!(verify_profile(&root, &manifest, &AcceptAll), Ok(()));
        // Corrupt one byte: hash mismatch.
        install_fixture(&root, &mut manifest, true);
        assert!(matches!(
            verify_content(&root, &manifest),
            Err(InferenceError::HashMismatch(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn real_ovis_manifest_paths_resolve_without_materializing_bytes() {
        // The real four-profile manifests still resolve under the frozen
        // layout (structure only — no bytes are written).
        let root = std::env::temp_dir().join(format!("ocrx-models-verify-ovis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = ovisocr2_manifest("reference").unwrap();
        let states = presence(&root, &manifest).unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|state| !state.present));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_profile_surfaces_missing_assets_and_binding_failures() {
        let root = std::env::temp_dir().join(format!(
            "ocrx-models-verify-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        assert!(matches!(
            verify_content(&root, &manifest),
            Err(InferenceError::MissingAsset(_))
        ));
        // Binding port failure category passes through.
        assert_eq!(
            verify_binding(&manifest, &RejectAll),
            Err(InferenceError::IncompatibleBinding("sys".into()))
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
