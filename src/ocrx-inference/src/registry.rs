//! Static registry (plan §6.1/§8.8): the single production registration of
//! the Ovis model, Ovis adapter, llama.cpp runtime, and their compatibility
//! binding. Pure data — no loading, no installation, no dynamic discovery.

use ocrx_inference_api::{
    AdapterId, AdapterIdentity, BindingVerifier, InferenceError, ModelManifest, RuntimeId,
    RuntimeIdentity,
};

/// The internal runtime id (new definition, plan §6.1: not a copy of the
/// result.json display string).
pub const REGISTRY_RUNTIME_ID: &str = "llama.cpp";

/// Frozen wrapper/sys/upstream values (Q1.3 pins revision, plan §5.1).
pub const REGISTRY_WRAPPER_VERSION: &str = "0.1.161";
pub const REGISTRY_SYS_VERSION: &str = "0.1.161";
pub const REGISTRY_UPSTREAM_REVISION: &str = "d81235049384534c167caea52b85a694f6103d14";

/// The single registered protocol adapter.
pub const REGISTRY_ADAPTER_ID: &str = "ovisocr2";
pub const REGISTRY_ADAPTER_VERSION: &str = "0.1.0";

/// Asset roles the registry's runtime+adapter pairing supports.
pub const SUPPORTED_ASSET_ROLES: [&str; 2] = ["model", "mmproj"];

/// The registry's authoritative runtime identity.
///
/// # Panics
///
/// Never: the frozen id strings are compile-time constants validated by tests.
#[must_use]
pub fn runtime_identity() -> RuntimeIdentity {
    RuntimeIdentity {
        runtime: RuntimeId::new(REGISTRY_RUNTIME_ID).expect("frozen runtime id is valid"),
        wrapper_version: REGISTRY_WRAPPER_VERSION.to_owned(),
        sys_version: REGISTRY_SYS_VERSION.to_owned(),
        upstream_revision: REGISTRY_UPSTREAM_REVISION.to_owned(),
    }
}

/// The registry's authoritative adapter identity.
///
/// # Panics
///
/// Never: the frozen id strings are compile-time constants validated by tests.
#[must_use]
pub fn adapter_identity() -> AdapterIdentity {
    AdapterIdentity {
        adapter: AdapterId::new(REGISTRY_ADAPTER_ID).expect("frozen adapter id is valid"),
        version: REGISTRY_ADAPTER_VERSION.to_owned(),
    }
}

/// Binding verification backed by the static registry (RBLK-9): pure data
/// comparison shared by `model verify` (via the injected port) and
/// `prepare`. Never creates a runtime and never loads a model.
#[derive(Debug, Default, Clone, Copy)]
pub struct RegistryVerifier;

impl BindingVerifier for RegistryVerifier {
    fn verify_binding(&self, manifest: &ModelManifest) -> Result<(), InferenceError> {
        crate::binding::check_binding(manifest, &runtime_identity(), &adapter_identity())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_test_manifest;

    #[test]
    fn registry_identities_are_the_frozen_values() {
        let runtime = runtime_identity();
        assert_eq!(runtime.runtime.as_str(), "llama.cpp");
        assert_eq!(runtime.wrapper_version, "0.1.161");
        assert_eq!(runtime.sys_version, "0.1.161");
        assert_eq!(
            runtime.upstream_revision,
            "d81235049384534c167caea52b85a694f6103d14"
        );
        let adapter = adapter_identity();
        assert_eq!(adapter.adapter.as_str(), "ovisocr2");
        assert_eq!(adapter.version, "0.1.0");
    }

    #[test]
    fn verifier_accepts_the_registered_triple() {
        let manifest = builtin_test_manifest();
        assert_eq!(RegistryVerifier.verify_binding(&manifest), Ok(()));
    }

    #[test]
    fn verifier_rejects_any_registry_mismatch() {
        let mut manifest = builtin_test_manifest();
        manifest.runtime_binding.sys_version = "0.1.160".to_owned();
        assert!(matches!(
            RegistryVerifier.verify_binding(&manifest),
            Err(InferenceError::IncompatibleBinding(_))
        ));
        let mut manifest = builtin_test_manifest();
        manifest.adapter_binding.version = "0.2.0".to_owned();
        assert!(matches!(
            RegistryVerifier.verify_binding(&manifest),
            Err(InferenceError::IncompatibleBinding(_))
        ));
        let mut manifest = builtin_test_manifest();
        manifest.assets[0].role = ocrx_inference_api::AssetRole::new("encoder").unwrap();
        assert!(matches!(
            RegistryVerifier.verify_binding(&manifest),
            Err(InferenceError::IncompatibleBinding(_))
        ));
    }
}
