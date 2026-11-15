//! Static registry (plan §6.1/§8.8): the single production registration of
//! the Ovis model, Ovis adapter, llama.cpp runtime, and their compatibility
//! binding. Pure data — no loading, no installation, no dynamic discovery.

use ocrx_inference_api::{
    AdapterId, AdapterIdentity, BindingVerifier, InferenceError, ModelId, ModelManifest, RuntimeId,
    RuntimeIdentity,
};

/// The internal runtime id (new definition, plan §6.1: not a copy of the
/// result.json display string). O3-T13: aliases of the single-source
/// platform constants in the inference API.
pub const REGISTRY_RUNTIME_ID: &str = ocrx_inference_api::PLATFORM_RUNTIME_ID;

/// Frozen wrapper/sys/upstream values (Q1.3 pins revision, plan §5.1).
pub const REGISTRY_WRAPPER_VERSION: &str = ocrx_inference_api::PLATFORM_WRAPPER_VERSION;
pub const REGISTRY_SYS_VERSION: &str = ocrx_inference_api::PLATFORM_SYS_VERSION;
pub const REGISTRY_UPSTREAM_REVISION: &str = ocrx_inference_api::PLATFORM_UPSTREAM_REVISION;

/// The single registered protocol adapter.
pub const REGISTRY_ADAPTER_ID: &str = "ovisocr2";
/// The OvisOCR3 adapter id registered alongside (D-3 per-model mapping;
/// both adapters share the frozen version line).
pub const REGISTRY_ADAPTER_ID_OVISOCR3: &str = "ovisocr3";
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

/// The registered adapter identity for one model id (D-3 per-model
/// mapping). Unknown model ids are themselves a binding mismatch: the
/// registry cannot verify a pairing it never declared.
///
/// # Errors
///
/// [`InferenceError::IncompatibleBinding`] for an unregistered model.
///
/// # Panics
///
/// Never: the frozen adapter ids are compile-time constants validated by
/// tests.
pub fn adapter_identity_for(model: &ModelId) -> Result<AdapterIdentity, InferenceError> {
    let adapter = match model.as_str() {
        "ovisocr2" => REGISTRY_ADAPTER_ID,
        "ovisocr3" => REGISTRY_ADAPTER_ID_OVISOCR3,
        other => {
            return Err(InferenceError::IncompatibleBinding(format!(
                "model id {other} is not registered"
            )));
        }
    };
    Ok(AdapterIdentity {
        adapter: AdapterId::new(adapter).expect("frozen adapter id is valid"),
        version: REGISTRY_ADAPTER_VERSION.to_owned(),
    })
}

/// Binding verification backed by the static registry (RBLK-9): pure data
/// comparison shared by `model verify` (via the injected port) and
/// `prepare`. Never creates a runtime and never loads a model.
#[derive(Debug, Default, Clone, Copy)]
pub struct RegistryVerifier;

impl BindingVerifier for RegistryVerifier {
    fn verify_binding(&self, manifest: &ModelManifest) -> Result<(), InferenceError> {
        crate::binding::check_binding(
            manifest,
            &runtime_identity(),
            &adapter_identity_for(&manifest.model)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_test_manifest;

    #[test]
    fn registry_and_runtime_factory_share_one_identity_source() {
        // O3-T13: the backend factory's reported identity and the registry
        // binding constants derive from the same inference-api platform
        // constants — a bump can never update one side only.
        assert_eq!(
            ocrx_runtime::platform_runtime_identity().unwrap(),
            runtime_identity()
        );
        assert_eq!(
            REGISTRY_WRAPPER_VERSION,
            ocrx_inference_api::PLATFORM_WRAPPER_VERSION
        );
    }

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
        // D-3: the per-model mapping resolves each registered model.
        assert_eq!(
            adapter_identity_for(&ModelId::new("ovisocr2").unwrap())
                .unwrap()
                .adapter
                .as_str(),
            "ovisocr2"
        );
        let ovisocr3 = adapter_identity_for(&ModelId::new("ovisocr3").unwrap()).unwrap();
        assert_eq!(ovisocr3.adapter.as_str(), "ovisocr3");
        assert_eq!(ovisocr3.version, "0.1.0");
        assert!(adapter_identity_for(&ModelId::new("qwen3.8").unwrap()).is_err());
    }

    #[test]
    fn verifier_accepts_the_registered_triple() {
        let manifest = builtin_test_manifest();
        assert_eq!(RegistryVerifier.verify_binding(&manifest), Ok(()));
    }

    #[test]
    fn verifier_accepts_both_registered_model_bindings() {
        // O3-T10 (D-3): the per-model mapping accepts each registered
        // model's own adapter pairing (this flips the Phase-1 C1
        // prerequisite discriminator from O3-T09).
        let ovisocr2 = crate::builtin_test_manifest();
        assert_eq!(RegistryVerifier.verify_binding(&ovisocr2), Ok(()));
        let ovisocr3 = ocrx_models::builtin::ovisocr3::ovisocr3_manifest("quality").unwrap();
        assert_eq!(RegistryVerifier.verify_binding(&ovisocr3), Ok(()));
    }

    #[test]
    fn verifier_rejects_every_wrong_pairing_in_the_error_matrix() {
        // C1: ovisocr3 assets x ovisocr2 adapter, wrong runtime identity,
        // wrong adapter version — each lands on IncompatibleBinding.
        let mut cross_adapter =
            ocrx_models::builtin::ovisocr3::ovisocr3_manifest("quality").unwrap();
        cross_adapter.adapter_binding.adapter =
            ocrx_inference_api::AdapterId::new("ovisocr2").unwrap();
        assert!(matches!(
            RegistryVerifier.verify_binding(&cross_adapter),
            Err(InferenceError::IncompatibleBinding(field)) if field == "adapter id"
        ));
        let mut wrong_runtime =
            ocrx_models::builtin::ovisocr3::ovisocr3_manifest("quality").unwrap();
        wrong_runtime.runtime_binding.sys_version = "0.1.160".to_owned();
        assert!(matches!(
            RegistryVerifier.verify_binding(&wrong_runtime),
            Err(InferenceError::IncompatibleBinding(field)) if field == "runtime sys version"
        ));
        let mut wrong_version =
            ocrx_models::builtin::ovisocr3::ovisocr3_manifest("quality").unwrap();
        wrong_version.adapter_binding.version = "0.2.0".to_owned();
        assert!(matches!(
            RegistryVerifier.verify_binding(&wrong_version),
            Err(InferenceError::IncompatibleBinding(field)) if field == "adapter version"
        ));
        // An unregistered model id is itself a binding mismatch.
        let mut unknown = crate::builtin_test_manifest();
        unknown.model = ocrx_inference_api::ModelId::new("qwen3.8").unwrap();
        assert!(matches!(
            RegistryVerifier.verify_binding(&unknown),
            Err(InferenceError::IncompatibleBinding(_))
        ));
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
