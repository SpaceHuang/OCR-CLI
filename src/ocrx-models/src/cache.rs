//! Named-asset cache layout (plan §5.2/§8.5): maps manifest roles/scope to
//! the existing Ovis cache layout byte-for-byte.
//!
//! Layout (frozen): `<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf`
//! for profile-scope roles and `<cache_root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf`
//! for revision-scope roles — the shared projector keeps its revision-layer
//! path so existing verified installs are never re-downloaded.

use std::path::{Path, PathBuf};

use ocrx_inference_api::{AssetRole, AssetScope, ModelManifest};

/// Resolves the cache path for one manifest asset.
///
/// # Errors
///
/// Returns [`InferenceError::InvalidManifest`] when the revision or the
/// manifest-declared relative path would escape the cache root (absolute
/// path, `..`, separators, empty segments).
pub fn asset_cache_path(
    cache_root: &Path,
    manifest: &ModelManifest,
    role: &AssetRole,
) -> Result<PathBuf, ocrx_inference_api::InferenceError> {
    let descriptor = manifest.asset(role).ok_or_else(|| {
        ocrx_inference_api::InferenceError::InvalidManifest(format!(
            "manifest has no asset role {role}"
        ))
    })?;
    validate_revision(&manifest.source_revision)?;
    validate_relative_path(&descriptor.relative_path)?;
    let revision_dir = cache_root
        .join("ocrx")
        .join("models")
        .join(manifest.model.as_str())
        .join(&manifest.source_revision);
    match descriptor.scope {
        AssetScope::Profile => Ok(revision_dir
            .join(manifest.profile.as_str())
            .join(&descriptor.relative_path)),
        AssetScope::Revision => Ok(revision_dir.join(&descriptor.relative_path)),
    }
}

/// Resolves every asset path of a manifest (deterministic manifest order).
pub fn manifest_asset_paths(
    cache_root: &Path,
    manifest: &ModelManifest,
) -> Result<Vec<(AssetRole, PathBuf)>, ocrx_inference_api::InferenceError> {
    manifest
        .assets
        .iter()
        .map(|asset| {
            Ok((
                asset.role.clone(),
                asset_cache_path(cache_root, manifest, &asset.role)?,
            ))
        })
        .collect()
}

/// Validates a revision path segment (same rules as the legacy `cache_paths`).
fn validate_revision(revision: &str) -> Result<(), ocrx_inference_api::InferenceError> {
    if revision.is_empty()
        || Path::new(revision).is_absolute()
        || revision.contains('/')
        || revision.contains('\\')
        || revision == ".."
        || revision == "."
    {
        return Err(ocrx_inference_api::InferenceError::InvalidManifest(format!(
            "unsafe revision segment {revision:?}"
        )));
    }
    Ok(())
}

/// Validates a manifest relative path (single safe segment).
fn validate_relative_path(path: &str) -> Result<(), ocrx_inference_api::InferenceError> {
    if path.is_empty()
        || path == "."
        || path == ".."
        || Path::new(path).is_absolute()
        || path.contains('/')
        || path.contains('\\')
        || path.contains(':')
    {
        return Err(ocrx_inference_api::InferenceError::InvalidManifest(format!(
            "unsafe asset relative path {path:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::ovisocr2::ovisocr2_manifest;

    #[test]
    fn ovis_layout_is_byte_identical_to_the_legacy_paths() {
        let manifest = ovisocr2_manifest("compact").unwrap();
        let root = Path::new("/cache");
        let model = asset_cache_path(root, &manifest, &AssetRole::new("model").unwrap()).unwrap();
        let mmproj = asset_cache_path(root, &manifest, &AssetRole::new("mmproj").unwrap()).unwrap();
        assert_eq!(
            model,
            PathBuf::from("/cache/ocrx/models/ovisocr2/8a22290a0c42dbcf84739d6d4c2763f877494ae0/compact/model.gguf")
        );
        assert_eq!(
            mmproj,
            PathBuf::from("/cache/ocrx/models/ovisocr2/8a22290a0c42dbcf84739d6d4c2763f877494ae0/mmproj.gguf")
        );
    }

    #[test]
    fn profiles_share_revision_scope_assets_but_keep_private_models() {
        let root = Path::new("/cache");
        let compact = ovisocr2_manifest("compact").unwrap();
        let quality = ovisocr2_manifest("quality").unwrap();
        let model = AssetRole::new("model").unwrap();
        let mmproj = AssetRole::new("mmproj").unwrap();
        assert_ne!(
            asset_cache_path(root, &compact, &model).unwrap(),
            asset_cache_path(root, &quality, &model).unwrap()
        );
        assert_eq!(
            asset_cache_path(root, &compact, &mmproj).unwrap(),
            asset_cache_path(root, &quality, &mmproj).unwrap()
        );
    }

    #[test]
    fn path_escape_is_rejected_for_revisions_and_roles() {
        let mut manifest = ovisocr2_manifest("compact").unwrap();
        assert!(asset_cache_path(Path::new("/c"), &manifest, &AssetRole::new("model").unwrap()).is_ok());
        // Role missing from the manifest is a manifest error, not a panic.
        assert!(asset_cache_path(Path::new("/c"), &manifest, &AssetRole::new("tokenizer").unwrap()).is_err());
        // Unsafe revision segment is rejected.
        manifest.source_revision = "../escape".to_owned();
        assert!(asset_cache_path(Path::new("/c"), &manifest, &AssetRole::new("model").unwrap()).is_err());
        // Unsafe relative path is rejected.
        let mut manifest = ovisocr2_manifest("compact").unwrap();
        manifest.assets[0].relative_path = "..".to_owned();
        assert!(asset_cache_path(Path::new("/c"), &manifest, &AssetRole::new("model").unwrap()).is_err());
    }

    #[test]
    fn manifest_asset_paths_cover_every_role_in_manifest_order() {
        let manifest = ovisocr2_manifest("balanced").unwrap();
        let paths = manifest_asset_paths(Path::new("/cache"), &manifest).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].0.as_str(), "model");
        assert_eq!(paths[1].0.as_str(), "mmproj");
        assert!(paths.iter().all(|(_, path)| path.starts_with("/cache/ocrx/models")));
    }
}
