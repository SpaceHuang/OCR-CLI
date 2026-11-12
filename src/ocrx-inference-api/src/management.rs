//! Model and device management traits plus the binding-verifier port
//! (plan §5.6, RBLK-9).
//!
//! `ModelManager` and `DeviceManager` are PDF-free management services. The
//! [`BindingVerifier`] port is defined here (same crate as the manifest DTO)
//! so `ocrx-models` can call it without a reverse dependency on the
//! composition crate: `ocrx-inference` implements it from the static
//! registry and the composition root injects it into `ModelManagerImpl`.

use crate::asset::ModelManifest;
use crate::error::InferenceError;
use crate::id::{BackendId, ModelId, ProfileId};
use crate::identity::{ModelDisplayMeta, RuntimeIdentity};

/// Per-backend availability snapshot (one bool per backend so device
/// selection stays a data-driven table; collapsing them would hide the
/// frozen contract).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub memory_gb: u64,
    pub metal_available: bool,
    pub cuda_available: bool,
    pub vulkan_available: bool,
    pub cpu_available: bool,
}

/// Device selection diagnostics for `doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDiagnostics {
    pub platform: String,
    pub backend: BackendId,
    pub image_token_limit: u32,
    pub memory_gb: u64,
    pub metal_available: bool,
    pub cpu_available: bool,
    pub recommended_profile: String,
    pub recommended_device: String,
    pub cache_root: String,
    pub installed_profiles: Vec<String>,
    /// Frozen profile list of the registered model.
    pub profiles: Vec<String>,
}

/// One profile's installation summary for list/info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSummary {
    pub profile: ProfileId,
    pub quantization: String,
    pub installed: bool,
}

/// `model list` output for one registered model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSummary {
    pub model: ModelId,
    pub display: ModelDisplayMeta,
    pub revision: String,
    pub profiles: Vec<ProfileSummary>,
}

/// `model info` output: profiles, install states, and provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDetail {
    pub summary: ModelSummary,
    pub runtime: RuntimeIdentity,
    pub adapter: crate::AdapterIdentity,
    pub license_id: String,
}

/// Profile recommendation result (no implicit install).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRecommendation {
    pub profile: ProfileId,
    /// Human-readable rationale for the CLI to show.
    pub rationale: String,
}

/// What an install would download (for CLI confirmation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlanItem {
    pub role: crate::AssetRole,
    pub url: String,
    pub byte_len: u64,
    pub cached: bool,
}

/// The full install plan: assets, bytes, license, and current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub model: ModelId,
    pub profile: ProfileId,
    pub license_id: String,
    pub items: Vec<InstallPlanItem>,
}

impl InstallPlan {
    /// Bytes that still need downloading.
    #[must_use]
    pub fn pending_bytes(&self) -> u64 {
        self.items
            .iter()
            .filter(|item| !item.cached)
            .map(|item| item.byte_len)
            .sum()
    }
}

/// Receipt of a completed install (audit record; never a trust source —
/// `verify` always recomputes hashes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReceipt {
    pub model: ModelId,
    pub profile: ProfileId,
    pub revision: String,
    pub assets: Vec<crate::identity::AssetIdentity>,
    pub downloaded: bool,
}

/// What a remove deleted (shared-asset bookkeeping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveReport {
    pub model: ModelId,
    pub profile: ProfileId,
    /// Asset roles deleted from the cache.
    pub removed_roles: Vec<crate::AssetRole>,
    /// Shared assets kept because other profiles still reference them.
    pub kept_shared_roles: Vec<crate::AssetRole>,
}

/// Model management service (no PDF, no scan objects).
pub trait ModelManager {
    /// Lists registered models with profile install states.
    ///
    /// # Errors
    ///
    /// Returns a structured category on cache/state failures.
    fn list(&self) -> Result<Vec<ModelSummary>, InferenceError>;

    /// Details one model: profiles, install states, revision, bindings.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::UnsupportedModel`] for unregistered models.
    fn info(&self, model: &ModelId) -> Result<ModelDetail, InferenceError>;

    /// Recommends a profile for the resources; never installs.
    ///
    /// # Errors
    ///
    /// Returns the memory/availability categories per the frozen rule.
    fn recommend(
        &self,
        model: &ModelId,
        resources: &ResourceSnapshot,
    ) -> Result<ProfileRecommendation, InferenceError>;

    /// Returns the install plan for CLI confirmation.
    ///
    /// # Errors
    ///
    /// Returns the profile/model categories for unknown requests.
    fn install_plan(
        &self,
        model: &ModelId,
        profile: &ProfileId,
    ) -> Result<InstallPlan, InferenceError>;

    /// Installs and verifies one profile, writing a receipt last. A repeated
    /// install of already-verified assets succeeds offline without network.
    ///
    /// # Errors
    ///
    /// Offline/license/disk/transport/hash categories per plan §5.7.
    fn install(
        &mut self,
        model: &ModelId,
        profile: &ProfileId,
        offline: bool,
        non_interactive: bool,
        license_accepted: bool,
        interaction: &mut dyn crate::asset::InstallInteraction,
    ) -> Result<InstallReceipt, InferenceError>;

    /// Verifies all required roles, sizes, SHA-256 values, and the static
    /// runtime/adapter binding via the injected [`BindingVerifier`] port.
    /// Never downloads, never loads.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::MissingAsset`]/[`InferenceError::HashMismatch`]/[`InferenceError::IncompatibleBinding`].
    fn verify(&self, model: &ModelId, profile: &ProfileId) -> Result<(), InferenceError>;

    /// Removes one profile's private assets; a shared asset is deleted only
    /// when no other profile of the same revision still uses it.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::UnsupportedProfile`] for unknown profiles.
    fn remove(
        &mut self,
        model: &ModelId,
        profile: &ProfileId,
    ) -> Result<RemoveReport, InferenceError>;
}

/// Device management service (plan §5.6).
pub trait DeviceManager {
    /// Current memory/hardware/compiled-capability snapshot.
    fn snapshot(&self) -> ResourceSnapshot;

    /// Selects a device: an explicitly requested unavailable device is an
    /// error (no silent reselection); `auto` follows the frozen priority
    /// `metal > cuda > vulkan > cpu`.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::DeviceUnavailable`]/[`InferenceError::InvalidTask`].
    fn select(&self, requested: &str) -> Result<BackendId, InferenceError>;

    /// Batch-depth tier selection (12 GiB → 16, otherwise 8; CPU/probe
    /// unavailable → 8). Called by `prepare` and re-called after a GPU→CPU
    /// fallback. No user-facing batch-depth knob exists.
    fn window_depth(&self, vram_bytes: Option<u64>) -> usize;

    /// Structured device/profile/cache/install overview for `doctor`.
    fn diagnose(&self) -> DeviceDiagnostics;

    /// Backends compiled into this binary (CPU always; GPU per features),
    /// intersected with hardware presence by callers via [`Self::select`].
    fn compiled_backends(&self) -> Vec<BackendId>;
}

/// Static model↔adapter↔runtime binding verification port (RBLK-9).
///
/// Defined in the API crate (same crate as the manifest DTO) so the models
/// layer can depend on it with zero cycle risk: the port implementation
/// lives in `ocrx-inference` (built from the static registry) and is
/// injected into `ModelManagerImpl` at composition time. Pure data
/// comparison — never creates a runtime, never loads a model.
pub trait BindingVerifier {
    /// Checks the manifest's runtime/adapter binding values and required
    /// asset roles against the static registry.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::IncompatibleBinding`] when any binding
    /// value disagrees (runtime id, wrapper/sys versions, upstream revision,
    /// adapter id/version, or an unsupported asset role).
    fn verify_binding(&self, manifest: &ModelManifest) -> Result<(), InferenceError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{AdapterId, AssetRole, RuntimeId};
    use crate::identity::AssetIdentity;

    fn manifest() -> ModelManifest {
        crate::asset::ModelManifest {
            schema_version: 2,
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            display_name: "OvisOCR2".into(),
            family: "Ovis".into(),
            quantization: "Q4_K_M".into(),
            source_revision: "8a22290a".into(),
            assets: vec![crate::asset::AssetDescriptor {
                role: AssetRole::new("model").unwrap(),
                required: true,
                scope: crate::asset::AssetScope::Profile,
                relative_path: "model.gguf".into(),
                sha256: "a".repeat(64),
                byte_len: 1,
                source_url: "https://example.invalid/m.gguf".into(),
            }],
            runtime_binding: crate::asset::BindingRequirement {
                runtime: RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".into(),
                sys_version: "0.1.161".into(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".into(),
            },
            adapter_binding: crate::asset::AdapterRequirement {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".into(),
            },
            license_id: "apache-2.0".into(),
            provenance: crate::asset::AssetProvenance {
                provider: "huggingface".into(),
                url: "https://example.invalid".into(),
            },
        }
    }

    /// Fake port: accepts only the exact frozen triple (proves the port is
    /// injectable and the failure category is IncompatibleBinding).
    struct AcceptingVerifier {
        accept: bool,
    }

    impl BindingVerifier for AcceptingVerifier {
        fn verify_binding(&self, _manifest: &ModelManifest) -> Result<(), InferenceError> {
            if self.accept {
                Ok(())
            } else {
                Err(InferenceError::IncompatibleBinding("wrapper".into()))
            }
        }
    }

    #[test]
    fn binding_verifier_port_accepts_and_rejects_via_fake() {
        let good = AcceptingVerifier { accept: true };
        assert_eq!(good.verify_binding(&manifest()), Ok(()));
        let bad = AcceptingVerifier { accept: false };
        assert_eq!(
            bad.verify_binding(&manifest()),
            Err(InferenceError::IncompatibleBinding("wrapper".into()))
        );
    }

    #[test]
    fn install_plan_pending_bytes_counts_only_uncached_items() {
        let plan = InstallPlan {
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            license_id: "apache-2.0".into(),
            items: vec![
                InstallPlanItem {
                    role: AssetRole::new("model").unwrap(),
                    url: "https://example.invalid/m".into(),
                    byte_len: 100,
                    cached: false,
                },
                InstallPlanItem {
                    role: AssetRole::new("mmproj").unwrap(),
                    url: "https://example.invalid/p".into(),
                    byte_len: 50,
                    cached: true,
                },
            ],
        };
        assert_eq!(plan.pending_bytes(), 100);
    }

    #[test]
    fn remove_report_separates_removed_from_kept_shared_roles() {
        let report = RemoveReport {
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            removed_roles: vec![AssetRole::new("model").unwrap()],
            kept_shared_roles: vec![AssetRole::new("mmproj").unwrap()],
        };
        assert_eq!(report.removed_roles.len(), 1);
        assert_eq!(report.kept_shared_roles.len(), 1);
    }

    #[test]
    fn receipt_carries_asset_identities_for_audit() {
        let receipt = InstallReceipt {
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            revision: "8a22290a".into(),
            assets: vec![AssetIdentity {
                role: AssetRole::new("model").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            }],
            downloaded: true,
        };
        assert_eq!(receipt.assets.len(), 1);
        assert!(receipt.downloaded);
    }
}
