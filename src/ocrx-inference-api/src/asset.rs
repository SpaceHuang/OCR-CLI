//! Generic asset manifest v2, install policy, and the universal install
//! interaction port (plan §5.2).
//!
//! The manifest carries named asset roles instead of two fixed hash fields:
//! roles are unique per manifest, required roles must all be present, and the
//! Ovis layout (`model` = profile scope, `mmproj` = revision scope) keeps the
//! existing shared-projector cache layout byte-identical.

use crate::error::InferenceError;
use crate::id::{AssetRole, ModelId, ProfileId, RuntimeId};
use serde::{Deserialize, Serialize};

/// Whether an asset is per-profile or shared across the revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetScope {
    /// Lives under `<revision>/<profile>/` (Ovis `model`).
    Profile,
    /// Lives under `<revision>/` and is shared by all profiles (Ovis `mmproj`).
    Revision,
}

/// One named asset inside a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetDescriptor {
    pub role: AssetRole,
    pub required: bool,
    pub scope: AssetScope,
    /// Cache-relative file name. Must be a safe single path segment (no
    /// absolute path, `..`, separators, or drive letters).
    pub relative_path: String,
    /// Pinned lowercase SHA-256 of the file content.
    pub sha256: String,
    pub byte_len: u64,
    /// Pinned HTTPS source URL.
    pub source_url: String,
}

/// Runtime binding a model requires (frozen wrapper/sys/upstream values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingRequirement {
    pub runtime: RuntimeId,
    pub wrapper_version: String,
    pub sys_version: String,
    pub upstream_revision: String,
}

/// Adapter binding a model requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterRequirement {
    pub adapter: crate::AdapterId,
    pub version: String,
}

/// Where the pinned assets come from (audit metadata).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetProvenance {
    /// Distribution channel, e.g. `huggingface`.
    pub provider: String,
    /// Provenance landing page (not a download URL).
    pub url: String,
}

/// Generic asset manifest v2 (`schema_version = 2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub schema_version: u32,
    pub model: ModelId,
    pub profile: ProfileId,
    /// Report-facing name (feeds `ModelDisplayMeta`, never matching).
    pub display_name: String,
    pub family: String,
    pub quantization: String,
    pub source_revision: String,
    pub assets: Vec<AssetDescriptor>,
    pub runtime_binding: BindingRequirement,
    pub adapter_binding: AdapterRequirement,
    pub license_id: String,
    pub provenance: AssetProvenance,
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Validates a manifest asset path segment: a safe relative single segment.
fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path != "."
        && path != ".."
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(char::is_whitespace)
}

impl ModelManifest {
    /// Current manifest schema version.
    pub const SCHEMA_VERSION: u32 = 2;

    #[must_use]
    pub fn required_roles(&self) -> Vec<&AssetRole> {
        self.assets
            .iter()
            .filter(|asset| asset.required)
            .map(|asset| &asset.role)
            .collect()
    }

    #[must_use]
    pub fn asset(&self, role: &AssetRole) -> Option<&AssetDescriptor> {
        self.assets.iter().find(|asset| &asset.role == role)
    }

    /// Validates the manifest structure (plan §5.2 rules).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidManifest`] for a wrong schema
    /// version, empty identity/display fields, duplicate roles, malformed
    /// hashes, unsafe paths, non-HTTPS sources, or empty bindings.
    pub fn validate(&self) -> Result<(), InferenceError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(InferenceError::InvalidManifest(format!(
                "unsupported manifest schema version {}",
                self.schema_version
            )));
        }
        if self.source_revision.trim().is_empty() {
            return Err(InferenceError::InvalidManifest(
                "empty source revision".into(),
            ));
        }
        if self.display_name.trim().is_empty()
            || self.family.trim().is_empty()
            || self.quantization.trim().is_empty()
            || self.license_id.trim().is_empty()
        {
            return Err(InferenceError::InvalidManifest(
                "empty display/family/quantization/license field".into(),
            ));
        }
        if self.assets.is_empty() {
            return Err(InferenceError::InvalidManifest(
                "manifest must declare at least one asset".into(),
            ));
        }
        let mut seen_roles = std::collections::BTreeSet::new();
        for asset in &self.assets {
            if !seen_roles.insert(asset.role.as_str()) {
                return Err(InferenceError::InvalidManifest(format!(
                    "duplicate asset role {}",
                    asset.role
                )));
            }
            if !is_sha256_hex(&asset.sha256) {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} has a malformed sha256",
                    asset.role
                )));
            }
            if asset.byte_len == 0 {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} declares zero bytes",
                    asset.role
                )));
            }
            if !is_safe_relative_path(&asset.relative_path) {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} has an unsafe relative path {:?}",
                    asset.role, asset.relative_path
                )));
            }
            if !asset.source_url.starts_with("https://") {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} source must be pinned HTTPS",
                    asset.role
                )));
            }
        }
        if self.runtime_binding.wrapper_version.trim().is_empty()
            || self.runtime_binding.sys_version.trim().is_empty()
            || self.runtime_binding.upstream_revision.trim().is_empty()
        {
            return Err(InferenceError::InvalidManifest(
                "empty runtime binding".into(),
            ));
        }
        if self.adapter_binding.version.trim().is_empty() {
            return Err(InferenceError::InvalidManifest(
                "empty adapter binding".into(),
            ));
        }
        Ok(())
    }
}

/// Download proposal handed to the install interaction (plan §5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProposal {
    pub role: AssetRole,
    pub url: String,
    pub byte_len: u64,
    pub license_id: String,
}

/// Policy flags controlling prepare/install downloads (plan §5.5).
///
/// `offline` forbids any download; `non_interactive` replaces terminal
/// questions with the frozen policy answer (license gate below); downloading
/// without an accepted license is a [`InferenceError::LicenseRequired`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallPolicy {
    pub offline: bool,
    pub non_interactive: bool,
    pub license_accepted: bool,
}

impl InstallPolicy {
    #[must_use]
    pub const fn offline() -> Self {
        Self {
            offline: true,
            non_interactive: true,
            license_accepted: false,
        }
    }

    #[must_use]
    pub const fn interactive_licensed() -> Self {
        Self {
            offline: false,
            non_interactive: false,
            license_accepted: false,
        }
    }
}

/// Universal install interaction port (plan §5.5).
///
/// The port only expresses "here is what would be downloaded, from where,
/// under which license — may we proceed". The CLI implements the terminal
/// question; tests inject fakes; prepare/install are the only callers.
pub trait InstallInteraction {
    /// Asks whether the proposed downloads may proceed. Returning `Ok(false)`
    /// aborts the install without partial state.
    ///
    /// # Errors
    ///
    /// Implementations may surface an interaction-layer failure verbatim.
    fn confirm_download(&mut self, proposals: &[DownloadProposal]) -> Result<bool, InferenceError>;

    /// Optional transfer progress tick (bytes completed so far).
    fn download_progress(&mut self, _completed_bytes: u64, _total_bytes: u64) {}
}

/// Non-interactive interaction: every proposal is declined by the port.
///
/// Callers combine this with [`InstallPolicy::non_interactive`] + license
/// acceptance for unattended installs; a declined proposal surfaces as
/// [`InferenceError::Offline`] (nothing may be downloaded unattended without
/// an explicit license acceptance).
#[derive(Debug, Default, Clone, Copy)]
pub struct NonInteractive;

impl InstallInteraction for NonInteractive {
    fn confirm_download(&mut self, _proposals: &[DownloadProposal]) -> Result<bool, InferenceError> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(role: &str, path: &str, scope: AssetScope) -> AssetDescriptor {
        AssetDescriptor {
            role: AssetRole::new(role).unwrap(),
            required: true,
            scope,
            relative_path: path.to_owned(),
            sha256: "a".repeat(64),
            byte_len: 100,
            source_url: "https://example.invalid/a.gguf".to_owned(),
        }
    }

    fn manifest(assets: Vec<AssetDescriptor>) -> ModelManifest {
        ModelManifest {
            schema_version: 2,
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            display_name: "OvisOCR2".to_owned(),
            family: "Ovis".to_owned(),
            quantization: "Q4_K_M".to_owned(),
            source_revision: "8a22290a".to_owned(),
            assets,
            runtime_binding: BindingRequirement {
                runtime: RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".to_owned(),
                sys_version: "0.1.161".to_owned(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".to_owned(),
            },
            adapter_binding: AdapterRequirement {
                adapter: crate::AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".to_owned(),
            },
            license_id: "apache-2.0".to_owned(),
            provenance: AssetProvenance {
                provider: "huggingface".to_owned(),
                url: "https://huggingface.co/Abiray/OvisOCR2-GGUF".to_owned(),
            },
        }
    }

    #[test]
    fn valid_multi_role_manifest_passes_validation() {
        let valid = manifest(vec![
            descriptor("model", "model.gguf", AssetScope::Profile),
            descriptor("mmproj", "mmproj.gguf", AssetScope::Revision),
        ]);
        assert_eq!(valid.validate(), Ok(()));
        assert_eq!(valid.required_roles().len(), 2);
        assert!(valid.asset(&AssetRole::new("mmproj").unwrap()).is_some());
    }

    #[test]
    fn manifest_rejects_duplicate_roles_and_unsafe_paths() {
        let duplicate = manifest(vec![
            descriptor("model", "model.gguf", AssetScope::Profile),
            descriptor("model", "other.gguf", AssetScope::Profile),
        ]);
        assert!(matches!(
            duplicate.validate(),
            Err(InferenceError::InvalidManifest(_))
        ));
        for bad_path in ["../escape.gguf", "/abs/model.gguf", "a/b.gguf", "", "a\\b.gguf"] {
            let invalid = manifest(vec![descriptor("model", bad_path, AssetScope::Profile)]);
            assert!(matches!(
                invalid.validate(),
                Err(InferenceError::InvalidManifest(_))
            ));
        }
    }

    #[test]
    fn manifest_rejects_wrong_schema_non_https_and_bad_hashes() {
        let mut invalid = manifest(vec![descriptor("model", "model.gguf", AssetScope::Profile)]);
        invalid.schema_version = 1;
        assert!(matches!(
            invalid.validate(),
            Err(InferenceError::InvalidManifest(_))
        ));
        let mut invalid = manifest(vec![descriptor("model", "model.gguf", AssetScope::Profile)]);
        invalid.assets[0].source_url = "http://example.invalid/a.gguf".to_owned();
        assert!(matches!(
            invalid.validate(),
            Err(InferenceError::InvalidManifest(_))
        ));
        let mut invalid = manifest(vec![descriptor("model", "model.gguf", AssetScope::Profile)]);
        invalid.assets[0].sha256 = "not-a-hash".to_owned();
        assert!(matches!(
            invalid.validate(),
            Err(InferenceError::InvalidManifest(_))
        ));
    }

    #[test]
    fn optional_roles_are_reported_separately_from_required_roles() {
        let mut optional = descriptor("tokenizer", "tokenizer.gguf", AssetScope::Profile);
        optional.required = false;
        let valid = manifest(vec![
            descriptor("model", "model.gguf", AssetScope::Profile),
            optional,
        ]);
        assert_eq!(valid.validate(), Ok(()));
        assert_eq!(valid.required_roles().len(), 1);
    }

    #[test]
    fn non_interactive_port_declines_every_proposal() {
        let mut interaction = NonInteractive;
        let proposals = vec![DownloadProposal {
            role: AssetRole::new("model").unwrap(),
            url: "https://example.invalid/model.gguf".to_owned(),
            byte_len: 1,
            license_id: "apache-2.0".to_owned(),
        }];
        assert_eq!(interaction.confirm_download(&proposals), Ok(false));
    }
}
