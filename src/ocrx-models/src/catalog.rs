//! Static model catalog (plan §5.1/§5.6): list, info, and resolve.
//!
//! The catalog only registers compiled-in manifest data. There is no dynamic
//! registration, no directory scanning, and no plugin loading (plan §2.2).

use ocrx_inference_api::{InferenceError, ModelId, ModelManifest, ProfileId};

use crate::builtin::ovisocr2::{self, OVISOCR2_PROFILES};

/// Static registry of the built-in manifests.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalogV2;

impl ModelCatalogV2 {
    #[must_use]
    pub fn builtin() -> Self {
        Self
    }

    /// Lists registered model ids (currently only `ovisocr2`).
    ///
    /// # Panics
    ///
    /// Never: the built-in id is compile-time fixed and validated by tests.
    #[must_use]
    pub fn list(&self) -> Vec<ModelId> {
        vec![ModelId::new("ovisocr2").expect("built-in model id is valid")]
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
        Self::require_registered(model)?;
        Ok(OVISOCR2_PROFILES
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
        Self::require_registered(model)?;
        ovisocr2::ovisocr2_manifest(profile.as_str())
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
        Self::require_registered(model)?;
        if requested == "auto" {
            crate::recommend::recommend_profile(&OVISOCR2_PROFILES, memory_gb)
        } else {
            if !OVISOCR2_PROFILES.contains(&requested) {
                return Err(InferenceError::UnsupportedProfile(requested.to_owned()));
            }
            Ok(ProfileId::new(requested).expect("checked profile id is valid"))
        }
    }

    fn require_registered(model: &ModelId) -> Result<(), InferenceError> {
        if model.as_str() != "ovisocr2" {
            return Err(InferenceError::UnsupportedModel(model.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_only_ovisocr2_with_frozen_profiles() {
        let catalog = ModelCatalogV2::builtin();
        let models = catalog.list();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].as_str(), "ovisocr2");
        let profiles = catalog.profiles(&models[0]).unwrap();
        assert_eq!(
            profiles.iter().map(ProfileId::as_str).collect::<Vec<_>>(),
            ["compact", "balanced", "quality", "reference"]
        );
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
