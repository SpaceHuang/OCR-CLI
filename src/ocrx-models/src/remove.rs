//! Profile removal with shared-asset reference checking (plan §5.6/§7.3).
//!
//! Only the target profile's private assets are deleted. A shared
//! (revision-scope) asset is removed only when no other profile of the same
//! revision is actually installed — judged from the catalog and each
//! profile's real on-disk state, never by counting files with an assumed
//! name.

use std::path::Path;

use ocrx_inference_api::{
    AssetScope, InferenceError, ModelId, ModelManifest, ProfileId, RemoveReport,
};

use crate::cache::asset_cache_path;
use crate::catalog::ModelCatalogV2;
use crate::verify::is_installed;

/// Installed-state predicate (the real check is `verify::is_installed`
/// bound to the removal's cache root; tests inject a cheap fake so
/// shared-asset semantics stay offline).
pub type InstalledPredicate<'a> = &'a dyn Fn(&ModelManifest) -> Result<bool, InferenceError>;

/// Removes one profile and reports which roles were removed or kept.
///
/// # Errors
///
/// Returns [`InferenceError::UnsupportedProfile`] for profiles outside the
/// catalog and cache-layout errors while resolving paths.
pub fn remove_profile(
    catalog: &ModelCatalogV2,
    cache_root: &Path,
    model: &ModelId,
    profile: &ProfileId,
) -> Result<RemoveReport, InferenceError> {
    let installed: InstalledPredicate<'_> =
        &|manifest: &ModelManifest| installed_at(cache_root, manifest);
    remove_profile_with(catalog, cache_root, model, profile, installed)
}

/// Testable core: same semantics with an injectable installed predicate.
pub fn remove_profile_with(
    catalog: &ModelCatalogV2,
    cache_root: &Path,
    model: &ModelId,
    profile: &ProfileId,
    installed: InstalledPredicate<'_>,
) -> Result<RemoveReport, InferenceError> {
    let manifest = catalog.manifest(model, profile)?;
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for asset in &manifest.assets {
        let path = asset_cache_path(cache_root, &manifest, &asset.role)?;
        match asset.scope {
            AssetScope::Profile => {
                if path.is_file() {
                    std::fs::remove_file(&path).map_err(|error| {
                        InferenceError::InvalidManifest(format!(
                            "unable to delete asset {}: {error}",
                            asset.role
                        ))
                    })?;
                }
                removed.push(asset.role.clone());
            }
            AssetScope::Revision => {
                let shared_in_use = shared_asset_still_used(
                    catalog,
                    model,
                    profile,
                    &manifest,
                    &asset.role,
                    installed,
                )?;
                if shared_in_use {
                    kept.push(asset.role.clone());
                } else {
                    if path.is_file() {
                        std::fs::remove_file(&path).map_err(|error| {
                            InferenceError::InvalidManifest(format!(
                                "unable to delete shared asset {}: {error}",
                                asset.role
                            ))
                        })?;
                    }
                    removed.push(asset.role.clone());
                }
            }
        }
    }
    // Drop the now-empty profile directory (and the revision directory when
    // nothing is left under it).
    let profile_dir = asset_cache_path(cache_root, &manifest, &manifest.assets[0].role)?
        .parent()
        .map(std::path::Path::to_path_buf);
    if let Some(directory) = profile_dir
        && directory.is_dir()
        && std::fs::read_dir(&directory).is_ok_and(|mut entries| entries.next().is_none())
    {
        let _ = std::fs::remove_dir(&directory);
        if let Some(revision_dir) = directory.parent()
            && revision_dir.is_dir()
            && std::fs::read_dir(revision_dir).is_ok_and(|mut entries| entries.next().is_none())
        {
            let _ = std::fs::remove_dir(revision_dir);
        }
    }
    Ok(RemoveReport {
        model: model.clone(),
        profile: profile.clone(),
        removed_roles: removed,
        kept_shared_roles: kept,
    })
}

fn installed_at(cache_root: &Path, manifest: &ModelManifest) -> Result<bool, InferenceError> {
    is_installed(cache_root, manifest)
}

/// Whether any *other* profile of the same revision still uses the shared
/// asset (checked by that profile's real install state).
fn shared_asset_still_used(
    catalog: &ModelCatalogV2,
    model: &ModelId,
    profile: &ProfileId,
    manifest: &ModelManifest,
    role: &ocrx_inference_api::AssetRole,
    installed: InstalledPredicate<'_>,
) -> Result<bool, InferenceError> {
    for other in catalog.profiles(model)? {
        if &other == profile {
            continue;
        }
        let Ok(other_manifest) = catalog.manifest(model, &other) else {
            continue;
        };
        if other_manifest.source_revision != manifest.source_revision {
            continue;
        }
        // Does the other profile declare the role at all?
        if other_manifest.asset(role).is_none() {
            continue;
        }
        if installed(&other_manifest)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ModelCatalogV2;
    use ocrx_inference_api::AssetRole;

    fn root_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ocrx-remove-{tag}-{}", std::process::id()))
    }

    fn write_asset(root: &Path, manifest: &ModelManifest, role: &str, bytes: &[u8]) {
        let path = asset_cache_path(root, manifest, &AssetRole::new(role).unwrap()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// Owned fake predicate handle (dereffed into [`InstalledPredicate`]).
    type FakeInstalled = Box<dyn Fn(&ModelManifest) -> Result<bool, InferenceError>>;

    /// Fake installed predicate: reports installed profiles by name.
    fn fake_installed(installed_profiles: &'static [&'static str]) -> FakeInstalled {
        Box::new(move |manifest: &ModelManifest| {
            Ok(installed_profiles.contains(&manifest.profile.as_str()))
        })
    }

    #[test]
    fn removing_one_profile_keeps_the_shared_mmproj_when_others_use_it() {
        let root = root_dir("shared-kept");
        let _ = std::fs::remove_dir_all(&root);
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr2").unwrap();
        let compact = catalog
            .manifest(&model, &ProfileId::new("compact").unwrap())
            .unwrap();
        write_asset(&root, &compact, "model", b"tiny-model");
        write_asset(&root, &compact, "mmproj", b"tiny-shared");
        // compact + quality are "installed": quality still uses the shared
        // projector, so removal keeps it and deletes only the private model.
        let installed = fake_installed(&["compact", "quality"]);
        let report = remove_profile_with(
            &catalog,
            &root,
            &model,
            &ProfileId::new("compact").unwrap(),
            &*installed,
        )
        .unwrap();
        assert_eq!(report.removed_roles.len(), 1);
        assert_eq!(report.removed_roles[0].as_str(), "model");
        assert_eq!(report.kept_shared_roles.len(), 1);
        assert_eq!(report.kept_shared_roles[0].as_str(), "mmproj");
        assert!(
            !asset_cache_path(&root, &compact, &AssetRole::new("model").unwrap())
                .unwrap()
                .exists()
        );
        assert!(
            asset_cache_path(&root, &compact, &AssetRole::new("mmproj").unwrap())
                .unwrap()
                .exists(),
            "shared projector must survive while quality uses it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn last_profile_removal_reclaims_the_shared_asset_and_empty_directories() {
        let root = root_dir("last");
        let _ = std::fs::remove_dir_all(&root);
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr2").unwrap();
        let reference = catalog
            .manifest(&model, &ProfileId::new("reference").unwrap())
            .unwrap();
        write_asset(&root, &reference, "model", b"tiny");
        write_asset(&root, &reference, "mmproj", b"tiny");
        // Only the removed profile is "installed".
        let installed = fake_installed(&["reference"]);
        let report = remove_profile_with(
            &catalog,
            &root,
            &model,
            &ProfileId::new("reference").unwrap(),
            &*installed,
        )
        .unwrap();
        assert_eq!(report.kept_shared_roles.len(), 0);
        assert_eq!(report.removed_roles.len(), 2);
        let mmproj =
            asset_cache_path(&root, &reference, &AssetRole::new("mmproj").unwrap()).unwrap();
        assert!(!mmproj.exists(), "last user frees the shared asset");
        let revision_dir = mmproj.parent().unwrap();
        assert!(!revision_dir.join("reference").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_profiles_are_rejected_structurally() {
        let root = root_dir("unknown");
        let _ = std::fs::remove_dir_all(&root);
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr2").unwrap();
        assert!(matches!(
            remove_profile(&catalog, &root, &model, &ProfileId::new("draft").unwrap()),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
