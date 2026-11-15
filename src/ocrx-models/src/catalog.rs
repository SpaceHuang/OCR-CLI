//! Static model catalog (plan §5.1/§5.6): list, info, and resolve.
//!
//! The catalog only registers compiled-in manifest data. There is no dynamic
//! registration, no directory scanning, and no plugin loading (plan §2.2).

use ocrx_inference_api::{InferenceError, ModelId, ModelManifest, ProfileId};

use crate::builtin::ovisocr2::{self, OVISOCR2_PROFILES};
use crate::builtin::ovisocr3::{self, OVISOCR3_PROFILES};

/// Static registry of the built-in manifests.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalogV2;

impl ModelCatalogV2 {
    #[must_use]
    pub fn builtin() -> Self {
        Self
    }

    /// The frozen per-model profile sets in declaration order (ascending
    /// size; D-7 variable profile sets — each model declares its actual
    /// available subset).
    fn profile_names(model: &ModelId) -> Result<&'static [&'static str], InferenceError> {
        match model.as_str() {
            "ovisocr2" => Ok(&OVISOCR2_PROFILES),
            "ovisocr3" => Ok(&OVISOCR3_PROFILES),
            _ => Err(InferenceError::UnsupportedModel(model.to_string())),
        }
    }

    /// Lists registered model ids in declaration order (`ovisocr2`, then
    /// `ovisocr3`).
    ///
    /// # Panics
    ///
    /// Never: the built-in ids are compile-time fixed and validated by tests.
    #[must_use]
    pub fn list(&self) -> Vec<ModelId> {
        vec![
            ModelId::new("ovisocr2").expect("built-in model id is valid"),
            ModelId::new("ovisocr3").expect("built-in model id is valid"),
        ]
    }

    /// Lists the frozen profile ids of a registered model.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::UnsupportedModel`] for unregistered models.
    ///
    /// # Panics
    ///
    /// Never: built-in profile ids are compile-time fixed and validated by tests.
    pub fn profiles(&self, model: &ModelId) -> Result<Vec<ProfileId>, InferenceError> {
        Ok(Self::profile_names(model)?
            .iter()
            .map(|profile| ProfileId::new(*profile).expect("built-in profile id is valid"))
            .collect())
    }

    /// Resolves the built-in manifest for one concrete profile.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::UnsupportedModel`] / [`InferenceError::UnsupportedProfile`].
    pub fn manifest(
        &self,
        model: &ModelId,
        profile: &ProfileId,
    ) -> Result<ModelManifest, InferenceError> {
        match model.as_str() {
            "ovisocr2" => ovisocr2::ovisocr2_manifest(profile.as_str()),
            "ovisocr3" => ovisocr3::ovisocr3_manifest(profile.as_str()),
            _ => Err(InferenceError::UnsupportedModel(model.to_string())),
        }
    }

    /// Resolves `auto` to a concrete profile via the frozen memory
    /// recommendation rule (plan §2.3). The same rule backs prepare's actual
    /// resolution (same input → same output; memory may drift between the
    /// two resolutions, which the final comparison closes, RBLK-6).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::UnsupportedModel`], the memory floor error,
    /// or the no-passing-profile error.
    ///
    /// # Panics
    ///
    /// Never: built-in and checked profile ids are compile-time constants.
    pub fn resolve_profile(
        &self,
        model: &ModelId,
        requested: &str,
        memory_gb: u64,
    ) -> Result<ProfileId, InferenceError> {
        let profiles = Self::profile_names(model)?;
        if requested == "auto" {
            crate::recommend::recommend_profile(profiles, memory_gb)
        } else {
            if !profiles.contains(&requested) {
                return Err(InferenceError::UnsupportedProfile(requested.to_owned()));
            }
            Ok(ProfileId::new(requested).expect("checked profile id is valid"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_both_models_in_declaration_order_with_frozen_profiles() {
        let catalog = ModelCatalogV2::builtin();
        let models = catalog.list();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].as_str(), "ovisocr2");
        assert_eq!(models[1].as_str(), "ovisocr3");
        let ovisocr2_profiles = catalog.profiles(&models[0]).unwrap();
        assert_eq!(
            ovisocr2_profiles
                .iter()
                .map(ProfileId::as_str)
                .collect::<Vec<_>>(),
            ["compact", "balanced", "quality", "reference"]
        );
        // D-7: ovisocr3 declares its actual available subset (spike freeze
        // table) — a single quality rung, not the four-rung mirror.
        let ovisocr3_profiles = catalog.profiles(&models[1]).unwrap();
        assert_eq!(
            ovisocr3_profiles
                .iter()
                .map(ProfileId::as_str)
                .collect::<Vec<_>>(),
            ["quality"]
        );
    }

    #[test]
    fn ovisocr3_manifests_resolve_from_the_freeze_table() {
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr3").unwrap();
        let manifest = catalog
            .manifest(&model, &ProfileId::new("quality").unwrap())
            .unwrap();
        assert_eq!(manifest.adapter_binding.adapter.as_str(), "ovisocr3");
        assert_eq!(manifest.quantization, "Q8_0");
        assert_eq!(manifest.display_name, "OvisOCR3");
        // The unavailable mirror rungs are structurally rejected.
        assert!(matches!(
            catalog.manifest(&model, &ProfileId::new("compact").unwrap()),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        assert!(matches!(
            catalog.profiles(&ModelId::new("qwen3.8").unwrap()),
            Err(InferenceError::UnsupportedModel(_))
        ));
        assert!(matches!(
            catalog.manifest(
                &ModelId::new("qwen3.8").unwrap(),
                &ProfileId::new("quality").unwrap()
            ),
            Err(InferenceError::UnsupportedModel(_))
        ));
    }

    #[test]
    fn auto_resolution_for_a_single_rung_set_is_deterministic() {
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr3").unwrap();
        // Below the 6 GiB floor nothing passes (frozen semantics, both models).
        assert!(matches!(
            catalog.resolve_profile(&model, "auto", 4),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        // Every non-failing memory tier lands on the only declared rung
        // (D-7 fallback: declaration-order first available profile).
        assert_eq!(
            catalog.resolve_profile(&model, "auto", 8).unwrap().as_str(),
            "quality"
        );
        assert_eq!(
            catalog
                .resolve_profile(&model, "auto", 64)
                .unwrap()
                .as_str(),
            "quality"
        );
        // The explicit rung passes; foreign profile names are rejected.
        assert_eq!(
            catalog
                .resolve_profile(&model, "quality", 8)
                .unwrap()
                .as_str(),
            "quality"
        );
        assert!(matches!(
            catalog.resolve_profile(&model, "compact", 8),
            Err(InferenceError::UnsupportedProfile(_))
        ));
    }

    #[test]
    fn unknown_models_are_rejected_structurally() {
        let catalog = ModelCatalogV2::builtin();
        let qwen = ModelId::new("qwen3.8").unwrap();
        assert!(matches!(
            catalog.profiles(&qwen),
            Err(InferenceError::UnsupportedModel(_))
        ));
        assert!(matches!(
            catalog.manifest(&qwen, &ProfileId::new("compact").unwrap()),
            Err(InferenceError::UnsupportedModel(_))
        ));
    }

    #[test]
    fn manifest_resolution_rejects_unknown_profiles() {
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr2").unwrap();
        assert!(matches!(
            catalog.manifest(&model, &ProfileId::new("draft").unwrap()),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        assert!(
            catalog
                .manifest(&model, &ProfileId::new("quality").unwrap())
                .is_ok()
        );
    }

    #[test]
    fn auto_resolution_follows_the_frozen_memory_rule() {
        let catalog = ModelCatalogV2::builtin();
        let model = ModelId::new("ovisocr2").unwrap();
        // Below the 6 GiB floor nothing passes.
        assert!(matches!(
            catalog.resolve_profile(&model, "auto", 4),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        // Tier ladder: <12 compact, <24 balanced, else quality.
        assert_eq!(
            catalog.resolve_profile(&model, "auto", 8).unwrap().as_str(),
            "compact"
        );
        assert_eq!(
            catalog
                .resolve_profile(&model, "auto", 16)
                .unwrap()
                .as_str(),
            "balanced"
        );
        assert_eq!(
            catalog
                .resolve_profile(&model, "auto", 64)
                .unwrap()
                .as_str(),
            "quality"
        );
        // Concrete profiles pass through unchanged.
        assert_eq!(
            catalog
                .resolve_profile(&model, "reference", 8)
                .unwrap()
                .as_str(),
            "reference"
        );
        // Unknown profile names are structurally rejected.
        assert!(matches!(
            catalog.resolve_profile(&model, "draft", 8),
            Err(InferenceError::UnsupportedProfile(_))
        ));
    }
}
