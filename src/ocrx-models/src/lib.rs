//! Model manifest and cache boundary.

// --- Target-architecture modules (plan §4.1/§8.5) --------------------------
// The legacy flat API below stays until T086 deletes it; new production code
// must use these modules and the [`ModelManagerImpl`] only.

pub mod builtin;
pub mod cache;
pub mod catalog;
pub mod install;
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
            let first = self.manifest(
                &model,
                &ocrx_inference_api::ProfileId::new("compact").expect("built-in profile is valid"),
            )?;
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
        let compact = self.manifest(
            model,
            &ocrx_inference_api::ProfileId::new("compact").expect("built-in profile is valid"),
        )?;
        Ok(ocrx_inference_api::ModelDetail {
            summary: ocrx_inference_api::ModelSummary {
                model: model.clone(),
                display: ocrx_inference_api::ModelDisplayMeta {
                    display_name: compact.display_name,
                    family: compact.family,
                    quantization: compact.quantization,
                },
                revision: compact.source_revision,
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
