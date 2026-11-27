//! C2 machine-readable install plan (dsh-plugin-v0.1 plan stage 2, D-4).
//!
//! `model recommend --report json` resolves one (model, profile) pair —
//! explicit `--model` keeps the per-model memory-ladder recommendation; the
//! no-flag face reports the SAME pair the scan's implicit default would
//! resolve — and derives the download plan from the frozen catalog + cache
//! layout. `target_dir` is the REVISION directory (the shared mmproj lives
//! one level above the profile dir; only the revision root covers both
//! sides, requirement §13-C2). The plugin must never hardcode this table.

use std::path::{Path, PathBuf};

use ocrx_inference_api::{InferenceError, ModelId};

use crate::catalog::ModelCatalogV2;
use crate::recommend::recommend_profile;
use crate::verify;

/// One planned download asset (manifest order).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstallPlanAsset {
    pub role: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Machine-readable install plan, schema_version 1 (plan D-4). Serialized by
/// the CLI face; the DSH plugin consumes the JSON verbatim.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstallPlan {
    pub schema_version: u32,
    pub action: &'static str,
    pub model: String,
    pub profile: String,
    pub default_model: String,
    pub revision: String,
    pub size_bytes: u64,
    /// The REVISION directory (`<root>/ocrx/models/<model>/<rev>`).
    pub target_dir: PathBuf,
    pub assets: Vec<InstallPlanAsset>,
    pub installed: bool,
    pub cache_root: PathBuf,
}

/// Inputs for [`InstallPlan::resolve`].
pub struct PlanRequest<'a> {
    pub model: &'a ModelId,
    /// System physical RAM in GiB (the profile ladder input, same as
    /// `model recommend`).
    pub memory_gb: u64,
    /// The CLI frozen default model spelling (main.rs `DEFAULT_MODEL`); the
    /// plugin uses it for the "only profile explicit" bootstrap combination.
    pub default_model: &'a str,
    pub cache_root: &'a Path,
    /// Explicit profile override (the implicit-default recommend face passes
    /// the D-50 resolved pair so the plan matches the text face exactly).
    /// `None` resolves through the per-model memory ladder.
    pub profile: Option<&'a ocrx_inference_api::ProfileId>,
}

impl InstallPlan {
    /// Resolves the plan for one model via the frozen memory ladder.
    ///
    /// # Errors
    ///
    /// Unknown model, memory below the profile floor, or cache-layout
    /// rejection.
    pub fn resolve(request: PlanRequest<'_>) -> Result<Self, InferenceError> {
        let catalog = ModelCatalogV2::builtin();
        let profile = match request.profile {
            Some(explicit) => explicit.clone(),
            None => {
                let profiles = catalog.profiles(request.model)?;
                let names: Vec<&str> = profiles
                    .iter()
                    .map(ocrx_inference_api::ProfileId::as_str)
                    .collect();
                recommend_profile(&names, request.memory_gb)?
            }
        };
        let manifest = catalog.manifest(request.model, &profile)?;
        let mut assets = Vec::new();
        let mut size_bytes = 0_u64;
        for asset in &manifest.assets {
            let path = crate::cache::asset_cache_path(request.cache_root, &manifest, &asset.role)?;
            size_bytes = size_bytes.saturating_add(asset.byte_len);
            assets.push(InstallPlanAsset {
                role: asset.role.as_str().to_owned(),
                path,
                size_bytes: asset.byte_len,
            });
        }
        // `installed` uses the same size+sha256 face as install's idempotent
        // short circuit (verify_content), so the plan and the installer can
        // never disagree about readiness.
        let installed = verify::verify_content(request.cache_root, &manifest).is_ok();
        let revision_dir = request
            .cache_root
            .join("ocrx")
            .join("models")
            .join(manifest.model.as_str())
            .join(&manifest.source_revision);
        Ok(Self {
            schema_version: 1,
            action: "recommend",
            model: manifest.model.as_str().to_owned(),
            profile: manifest.profile.as_str().to_owned(),
            default_model: request.default_model.to_owned(),
            revision: manifest.source_revision.clone(),
            size_bytes,
            target_dir: revision_dir,
            assets,
            installed,
            cache_root: request.cache_root.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::ProfileId;

    fn plan_for(model: &str, memory_gb: u64, cache_root: &Path) -> InstallPlan {
        InstallPlan::resolve(PlanRequest {
            model: &ModelId::new(model).unwrap(),
            memory_gb,
            default_model: "ovisocr3",
            cache_root,
            profile: None,
        })
        .unwrap()
    }

    #[test]
    fn ovisocr3_quality_plan_covers_both_sides_of_the_revision_layout() {
        // Plan §6 阶段 2: ovisocr3 only has the quality rung; the frozen byte
        // totals are 811,843,712 (model) + 204,987,136 (mmproj), and
        // target_dir is the REVISION directory so the mmproj side (~205 MB)
        // is never dropped from the consent surface.
        let root = std::env::temp_dir().join(format!("ocrx-plan-o3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let plan = plan_for("ovisocr3", 32, &root);
        assert_eq!(plan.schema_version, 1);
        assert_eq!(plan.action, "recommend");
        assert_eq!(plan.model, "ovisocr3");
        assert_eq!(plan.profile, "quality");
        assert_eq!(plan.default_model, "ovisocr3");
        assert_eq!(plan.revision, crate::builtin::ovisocr3::OVISOCR3_REVISION);
        assert_eq!(plan.size_bytes, 811_843_712 + 204_987_136);
        assert_eq!(
            plan.target_dir,
            root.join("ocrx")
                .join("models")
                .join("ovisocr3")
                .join(crate::builtin::ovisocr3::OVISOCR3_REVISION)
        );
        assert_eq!(plan.assets.len(), 2);
        let model_asset = plan.assets.iter().find(|a| a.role == "model").unwrap();
        assert_eq!(model_asset.size_bytes, 811_843_712);
        assert_eq!(
            model_asset.path,
            plan.target_dir.join("quality").join("model.gguf"),
            "the model asset lives inside the profile dir under the revision root"
        );
        let mmproj_asset = plan.assets.iter().find(|a| a.role == "mmproj").unwrap();
        assert_eq!(mmproj_asset.size_bytes, 204_987_136);
        assert_eq!(
            mmproj_asset.path,
            plan.target_dir.join("mmproj.gguf"),
            "the shared projector lives directly in the revision dir"
        );
        assert!(!plan.installed, "empty cache: nothing installed");
        assert_eq!(plan.cache_root, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ovisocr2_plan_follows_the_memory_ladder_and_reports_default_model() {
        let root = std::env::temp_dir().join(format!("ocrx-plan-o2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let balanced = plan_for("ovisocr2", 16, &root);
        assert_eq!(balanced.profile, "balanced");
        assert_eq!(balanced.default_model, "ovisocr3", "the CLI frozen default");
        let compact = plan_for("ovisocr2", 8, &root);
        assert_eq!(compact.profile, "compact");
        // Both profile rungs live in the SAME revision (the shared mmproj is
        // revision-scoped), so target_dir is identical — the profile-scoped
        // model asset path is what differs.
        assert_eq!(balanced.target_dir, compact.target_dir);
        assert_ne!(
            balanced
                .assets
                .iter()
                .find(|a| a.role == "model")
                .unwrap()
                .path,
            compact
                .assets
                .iter()
                .find(|a| a.role == "model")
                .unwrap()
                .path
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn installed_flag_rides_the_same_verify_content_face_as_install() {
        // The builtin manifests pin real asset hashes, so a full installed=true
        // fixture is not forgeable here; the contract under test is the
        // WIRING: plan.installed must equal install's idempotency face
        // (verify_content) for the same cache state — empty and wrong-bytes
        // legs both stay false and never diverge from the direct call.
        let root = std::env::temp_dir().join(format!("ocrx-plan-inst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let plan = plan_for("ovisocr2", 8, &root);
        let manifest = crate::builtin::ovisocr2::ovisocr2_manifest("compact").unwrap();
        assert_eq!(
            plan.installed,
            verify::verify_content(&root, &manifest).is_ok()
        );
        for asset in &manifest.assets {
            let path = crate::cache::asset_cache_path(&root, &manifest, &asset.role).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"placeholder-wrong-bytes").unwrap();
        }
        let wrong = plan_for("ovisocr2", 8, &root);
        assert!(
            !wrong.installed,
            "wrong bytes must not count as installed (sha256 face)"
        );
        assert_eq!(
            wrong.installed,
            verify::verify_content(&root, &manifest).is_ok()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn memory_below_the_floor_is_an_error_not_a_panicked_plan() {
        let root = std::env::temp_dir().join(format!("ocrx-plan-floor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let error = InstallPlan::resolve(PlanRequest {
            model: &ModelId::new("ovisocr3").unwrap(),
            memory_gb: 4,
            default_model: "ovisocr3",
            profile: None,
            cache_root: &root,
        })
        .unwrap_err();
        assert!(matches!(error, InferenceError::UnsupportedProfile(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_profile_override_plans_the_requested_manifest_not_the_ladder() {
        // BLOCK-7: `model recommend --report json <profile>` must plan the
        // REQUESTED rung - a non-ladder profile (32 GiB ladder = quality,
        // request = compact) gets compact's exact manifest bytes, so the
        // consent copy carries the real size for the requested target.
        let root = std::env::temp_dir().join(format!("ocrx-plan-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ladder = plan_for("ovisocr2", 32, &root);
        assert_eq!(ladder.profile, "quality", "ladder face at 32 GiB");
        let pinned = InstallPlan::resolve(PlanRequest {
            model: &ModelId::new("ovisocr2").unwrap(),
            memory_gb: 32,
            default_model: "ovisocr3",
            cache_root: &root,
            profile: Some(&ProfileId::new("compact").unwrap()),
        })
        .unwrap();
        assert_eq!(pinned.profile, "compact");
        assert_ne!(pinned.profile, ladder.profile);
        let manifest = crate::builtin::ovisocr2::ovisocr2_manifest("compact").unwrap();
        let expected: u64 = manifest.assets.iter().map(|asset| asset.byte_len).sum();
        assert_eq!(pinned.size_bytes, expected);
        assert_ne!(pinned.size_bytes, ladder.size_bytes, "non-ladder rung differs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_profiles_never_escape_the_catalog() {
        // The implicit-default plan face and `ProfileId` spelling agree.
        let root = std::env::temp_dir().join(format!("ocrx-plan-prof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let plan = plan_for("ovisocr2", 32, &root);
        ProfileId::new(&plan.profile).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
