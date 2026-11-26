//! Model manifest and cache boundary.

// --- Target-architecture modules (plan §4.1/§8.5) --------------------------
// The legacy flat API below stays until T086 deletes it; new production code
// must use these modules and the [`ModelManagerImpl`] only.

pub mod builtin;
pub mod cache;
pub mod catalog;
pub mod install;
pub mod plan;
pub mod layout;
pub mod recommend;
pub mod remove;
pub mod transport;
pub mod verify;

pub use catalog::ModelCatalogV2;
pub use transport::{CurlTransport, FakeTransport};

/// Resolves the platform-standard default model cache root (frozen WP.1
/// rule): Windows `%LOCALAPPDATA%` (falling back to
/// `%USERPROFILE%\AppData\Local`), macOS `$HOME/Library/Caches`, and a
/// temp-dir root for service accounts without a home — never the current
/// directory. `OCRX_MODEL_CACHE` overrides only the root.
#[must_use]
pub fn default_cache_root() -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return std::path::PathBuf::from(local);
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return std::path::PathBuf::from(profile)
                .join("AppData")
                .join("Local");
        }
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join("Library")
                .join("Caches");
        }
    }
    std::env::temp_dir().join("ocrx").join("models")
}

/// Production model manager (plan §8.5): implements the inference API's
/// [`ModelManager`] over the static catalog, cache layout, install
/// transactions, verification, and removal. Binding verification goes
/// through the injected [`ocrx_inference_api::BindingVerifier`] port
/// (RBLK-9) — this crate never reads the inference registry itself and must
/// never depend on `ocrx-inference`.
pub struct ModelManagerImpl {
    catalog: catalog::ModelCatalogV2,
    cache_root: std::path::PathBuf,
    binding_verifier: Box<dyn ocrx_inference_api::BindingVerifier>,
    transport: Box<dyn transport::Transport>,
    /// Free bytes on the cache volume (probe injectable; `None` skips the
    /// install preflight).
    available_bytes: Option<u64>,
}

impl ModelManagerImpl {
    #[must_use]
    pub fn new(
        cache_root: std::path::PathBuf,
        binding_verifier: Box<dyn ocrx_inference_api::BindingVerifier>,
        transport: Box<dyn transport::Transport>,
        available_bytes: Option<u64>,
    ) -> Self {
        Self {
            catalog: catalog::ModelCatalogV2::builtin(),
            cache_root,
            binding_verifier,
            transport,
            available_bytes,
        }
    }

    #[must_use]
    pub fn catalog(&self) -> &catalog::ModelCatalogV2 {
        &self.catalog
    }

    #[must_use]
    pub fn cache_root(&self) -> &std::path::Path {
        &self.cache_root
    }

    /// Content-only verification (no binding port): used by prepare's
    /// asset-readiness step before deciding to install.
    ///
    /// # Errors
    ///
    /// Missing/hash categories from the content verifier.
    pub fn verify_content_only(
        &self,
        manifest: &ocrx_inference_api::ModelManifest,
    ) -> Result<(), ocrx_inference_api::InferenceError> {
        verify::verify_content(&self.cache_root, manifest)
    }

    /// Resolves every manifest asset to its cache path (role-ordered).
    ///
    /// # Errors
    ///
    /// Cache-layout errors.
    pub fn required_asset_paths(
        &self,
        manifest: &ocrx_inference_api::ModelManifest,
    ) -> Result<
        Vec<(ocrx_inference_api::AssetRole, std::path::PathBuf)>,
        ocrx_inference_api::InferenceError,
    > {
        cache::manifest_asset_paths(&self.cache_root, manifest)
    }

    fn manifest(
        &self,
        model: &ocrx_inference_api::ModelId,
        profile: &ocrx_inference_api::ProfileId,
    ) -> Result<ocrx_inference_api::ModelManifest, ocrx_inference_api::InferenceError> {
        self.catalog.manifest(model, profile)
    }
}

impl ocrx_inference_api::ModelManager for ModelManagerImpl {
    fn list(
        &self,
    ) -> Result<Vec<ocrx_inference_api::ModelSummary>, ocrx_inference_api::InferenceError> {
        let mut summaries = Vec::new();
        for model in self.catalog.list() {
            let mut profiles = Vec::new();
            for profile in self.catalog.profiles(&model)? {
                let manifest = self.manifest(&model, &profile)?;
                let installed = verify::is_installed(&self.cache_root, &manifest)?;
                profiles.push(ocrx_inference_api::ProfileSummary {
                    profile,
                    quantization: manifest.quantization,
                    installed,
                });
            }
            // D-7: display metadata comes from the declaration-order FIRST
            // available profile (ovisocr2: compact → byte-identical to the
            // historical display source; ovisocr3: quality). A profile set
            // without `compact` no longer makes list/info fail.
            let first_profile = self
                .catalog
                .profiles(&model)?
                .first()
                .cloned()
                .expect("declared models always have at least one profile");
            let first = self.manifest(&model, &first_profile)?;
            summaries.push(ocrx_inference_api::ModelSummary {
                model,
                display: ocrx_inference_api::ModelDisplayMeta {
                    display_name: first.display_name,
                    family: first.family,
                    quantization: first.quantization,
                },
                revision: first.source_revision,
                profiles,
            });
        }
        Ok(summaries)
    }

    fn info(
        &self,
        model: &ocrx_inference_api::ModelId,
    ) -> Result<ocrx_inference_api::ModelDetail, ocrx_inference_api::InferenceError> {
        let mut profiles = Vec::new();
        let mut bindings = None;
        for profile in self.catalog.profiles(model)? {
            let manifest = self.manifest(model, &profile)?;
            let installed = verify::is_installed(&self.cache_root, &manifest)?;
            bindings.get_or_insert((
                manifest.runtime_binding.clone(),
                manifest.adapter_binding.clone(),
                manifest.license_id.clone(),
            ));
            profiles.push(ocrx_inference_api::ProfileSummary {
                profile,
                quantization: manifest.quantization,
                installed,
            });
        }
        let (runtime_binding, adapter_binding, license_id) = bindings.ok_or_else(|| {
            ocrx_inference_api::InferenceError::UnsupportedModel(model.to_string())
        })?;
        // D-7: display metadata from the declaration-order first available
        // profile (not a fixed `compact` — missing-compact sets stay valid).
        let first_profile = self
            .catalog
            .profiles(model)?
            .first()
            .cloned()
            .expect("declared models always have at least one profile");
        let first = self.manifest(model, &first_profile)?;
        Ok(ocrx_inference_api::ModelDetail {
            summary: ocrx_inference_api::ModelSummary {
                model: model.clone(),
                display: ocrx_inference_api::ModelDisplayMeta {
                    display_name: first.display_name,
                    family: first.family,
                    quantization: first.quantization,
                },
                revision: first.source_revision,
                profiles,
            },
            runtime: ocrx_inference_api::RuntimeIdentity {
                runtime: runtime_binding.runtime,
                wrapper_version: runtime_binding.wrapper_version,
                sys_version: runtime_binding.sys_version,
                upstream_revision: runtime_binding.upstream_revision,
            },
            adapter: ocrx_inference_api::AdapterIdentity {
                adapter: adapter_binding.adapter,
                version: adapter_binding.version,
            },
            license_id,
        })
    }

    fn recommend(
        &self,
        model: &ocrx_inference_api::ModelId,
        resources: &ocrx_inference_api::ResourceSnapshot,
    ) -> Result<ocrx_inference_api::ProfileRecommendation, ocrx_inference_api::InferenceError> {
        let profile_ids = self.catalog.profiles(model)?;
        let names: Vec<&str> = profile_ids
            .iter()
            .map(ocrx_inference_api::ProfileId::as_str)
            .collect();
        let profile = recommend::recommend_profile(&names, resources.memory_gb)?;
        Ok(ocrx_inference_api::ProfileRecommendation {
            // The recommend face is per-model by contract (an explicit
            // --model never crosses models here); the implicit-default
            // cross-model fallback lives in
            // recommend::recommend_default_selection and is wired at the
            // CLI entry where the implicit/explicit distinction is known.
            model: model.clone(),
            rationale: format!("frozen memory ladder for {} GiB", resources.memory_gb),
            profile,
        })
    }

    fn install_plan(
        &self,
        model: &ocrx_inference_api::ModelId,
        profile: &ocrx_inference_api::ProfileId,
    ) -> Result<ocrx_inference_api::InstallPlan, ocrx_inference_api::InferenceError> {
        let manifest = self.manifest(model, profile)?;
        let mut items = Vec::new();
        for asset in &manifest.assets {
            let path = cache::asset_cache_path(&self.cache_root, &manifest, &asset.role)?;
            let cached = path.is_file()
                && std::fs::metadata(&path)
                    .map(|m| m.len() == asset.byte_len)
                    .unwrap_or(false);
            items.push(ocrx_inference_api::InstallPlanItem {
                role: asset.role.clone(),
                url: asset.source_url.clone(),
                byte_len: asset.byte_len,
                cached,
            });
        }
        Ok(ocrx_inference_api::InstallPlan {
            model: model.clone(),
            profile: profile.clone(),
            license_id: manifest.license_id,
            items,
        })
    }

    fn install(
        &mut self,
        model: &ocrx_inference_api::ModelId,
        profile: &ocrx_inference_api::ProfileId,
        offline: bool,
        non_interactive: bool,
        license_accepted: bool,
        interaction: &mut dyn ocrx_inference_api::InstallInteraction,
    ) -> Result<ocrx_inference_api::InstallReceipt, ocrx_inference_api::InferenceError> {
        let manifest = self.manifest(model, profile)?;
        let policy = ocrx_inference_api::InstallPolicy {
            offline,
            non_interactive,
            license_accepted,
        };
        install::install(install::InstallContext {
            manifest: &manifest,
            cache_root: &self.cache_root,
            policy,
            transport: self.transport.as_ref(),
            interaction,
            available_bytes: self.available_bytes,
        })
    }

    fn verify(
        &self,
        model: &ocrx_inference_api::ModelId,
        profile: &ocrx_inference_api::ProfileId,
    ) -> Result<(), ocrx_inference_api::InferenceError> {
        let manifest = self.manifest(model, profile)?;
        verify::verify_profile(&self.cache_root, &manifest, self.binding_verifier.as_ref())
    }

    fn remove(
        &mut self,
        model: &ocrx_inference_api::ModelId,
        profile: &ocrx_inference_api::ProfileId,
    ) -> Result<ocrx_inference_api::RemoveReport, ocrx_inference_api::InferenceError> {
        remove::remove_profile(&self.catalog, &self.cache_root, model, profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::{InferenceError, ModelManager};

    struct AcceptAllVerifier;

    impl ocrx_inference_api::BindingVerifier for AcceptAllVerifier {
        fn verify_binding(
            &self,
            _manifest: &ocrx_inference_api::ModelManifest,
        ) -> Result<(), InferenceError> {
            Ok(())
        }
    }

    #[test]
    fn list_and_info_derive_display_from_the_first_available_profile() {
        // D-7/C15: the display source is the declaration-order first
        // available profile — ovisocr2 keeps the historical compact display
        // (byte-identical), ovisocr3 (single quality rung) displays Q8_0
        // instead of failing on a missing compact manifest.
        let root =
            std::env::temp_dir().join(format!("ocrx-manager-display-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp cache root");
        let manager = ModelManagerImpl::new(
            root.clone(),
            Box::new(AcceptAllVerifier),
            Box::new(crate::transport::FakeTransport::new()),
            None,
        );
        let summaries = manager.list().expect("list works for both models");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].model.as_str(), "ovisocr2");
        assert_eq!(summaries[0].display.quantization, "Q4_K_M");
        assert_eq!(summaries[1].model.as_str(), "ovisocr3");
        assert_eq!(summaries[1].display.quantization, "Q8_0");
        assert_eq!(summaries[1].display.display_name, "OvisOCR3");
        assert_eq!(
            summaries[1]
                .profiles
                .iter()
                .map(|profile| profile.profile.as_str())
                .collect::<Vec<_>>(),
            ["quality"]
        );

        let model = ocrx_inference_api::ModelId::new("ovisocr3").unwrap();
        let detail = manager.info(&model).expect("info works for ovisocr3");
        assert_eq!(detail.summary.display.quantization, "Q8_0");
        assert_eq!(detail.adapter.adapter.as_str(), "ovisocr3");
        assert_eq!(detail.runtime.wrapper_version, "0.1.161");
        assert_eq!(detail.license_id, "apache-2.0");
        // Unknown models still fail loudly.
        assert!(
            manager
                .info(&ocrx_inference_api::ModelId::new("qwen3.8").unwrap())
                .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_plans_cover_both_models_from_their_freeze_tables() {
        // O3-T14 management face (via the manager, not the CLI): each
        // model's install plan carries its own URL base, byte sizes, and
        // cache paths; nothing is hardcoded to one model.
        let root = std::env::temp_dir().join(format!("ocrx-manager-plan-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp cache root");
        let manager = ModelManagerImpl::new(
            root.clone(),
            Box::new(AcceptAllVerifier),
            Box::new(crate::transport::FakeTransport::new()),
            None,
        );
        let plan2 = manager
            .install_plan(
                &ocrx_inference_api::ModelId::new("ovisocr2").unwrap(),
                &ocrx_inference_api::ProfileId::new("compact").unwrap(),
            )
            .unwrap();
        assert_eq!(plan2.items.len(), 2);
        assert!(
            plan2.items.iter().all(|item| item
                .url
                .starts_with("https://huggingface.co/Abiray/OvisOCR2-GGUF/resolve/")),
            "ovisocr2 urls: {:?}",
            plan2.items
        );
        let plan3 = manager
            .install_plan(
                &ocrx_inference_api::ModelId::new("ovisocr3").unwrap(),
                &ocrx_inference_api::ProfileId::new("quality").unwrap(),
            )
            .unwrap();
        assert_eq!(plan3.items.len(), 2);
        assert_eq!(plan3.license_id, "apache-2.0");
        let model_item = plan3
            .items
            .iter()
            .find(|item| item.role.as_str() == "model")
            .unwrap();
        assert_eq!(model_item.byte_len, 811_843_712);
        assert!(
            model_item
                .url
                .starts_with("https://huggingface.co/michalland/OvisOCR3-GGUF/resolve/"),
            "ovisocr3 url: {}",
            model_item.url
        );
        assert!(!model_item.cached, "empty cache: nothing pre-cached");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recommendation_goes_through_the_generalized_fallback_per_model() {
        // C15: `recommend` (manager face) and catalog auto resolve share the
        // same generalized rule — the single-rung ovisocr3 set resolves
        // deterministically at every non-floor memory tier.
        let root =
            std::env::temp_dir().join(format!("ocrx-manager-recommend-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp cache root");
        let manager = ModelManagerImpl::new(
            root.clone(),
            Box::new(AcceptAllVerifier),
            Box::new(crate::transport::FakeTransport::new()),
            None,
        );
        let model = ocrx_inference_api::ModelId::new("ovisocr3").unwrap();
        for memory_gb in [6, 8, 16, 24, 128] {
            let recommendation = manager
                .recommend(
                    &model,
                    &ocrx_inference_api::ResourceSnapshot {
                        memory_gb,
                        metal_available: false,
                        cuda_available: false,
                        vulkan_available: false,
                        cpu_available: true,
                    },
                )
                .expect("recommend works for a non-empty profile set");
            assert_eq!(recommendation.profile.as_str(), "quality");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
