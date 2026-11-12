//! Three-way model↔adapter↔runtime compatibility check (plan §8.8).
//!
//! One implementation serves both consumers: the static-registry
//! [`crate::RegistryVerifier`] (the injected `BindingVerifier` port behind
//! `model verify`) and the `prepare` path — identical semantics by
//! construction.

use ocrx_inference_api::{AdapterIdentity, InferenceError, ModelManifest, RuntimeIdentity};

/// Strictly compares the manifest's binding requirements against the
/// registry's actual runtime/adapter identities and the supported asset
/// roles.
///
/// # Errors
///
/// Returns [`InferenceError::IncompatibleBinding`] naming the first
/// mismatched field (runtime id, wrapper, sys, upstream, adapter, version,
/// or an unsupported asset role).
pub fn check_binding(
    manifest: &ModelManifest,
    runtime: &RuntimeIdentity,
    adapter: &AdapterIdentity,
) -> Result<(), InferenceError> {
    let mismatch = |field: &str| Err(InferenceError::IncompatibleBinding(field.to_owned()));
    if manifest.runtime_binding.runtime != runtime.runtime {
        return mismatch("runtime id");
    }
    if manifest.runtime_binding.wrapper_version != runtime.wrapper_version {
        return mismatch("runtime wrapper version");
    }
    if manifest.runtime_binding.sys_version != runtime.sys_version {
        return mismatch("runtime sys version");
    }
    if manifest.runtime_binding.upstream_revision != runtime.upstream_revision {
        return mismatch("runtime upstream revision");
    }
    if manifest.adapter_binding.adapter != adapter.adapter {
        return mismatch("adapter id");
    }
    if manifest.adapter_binding.version != adapter.version {
        return mismatch("adapter version");
    }
    for asset in &manifest.assets {
        if !crate::registry::SUPPORTED_ASSET_ROLES.contains(&asset.role.as_str()) {
            return mismatch("asset role");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_test_manifest;

    #[test]
    fn the_registered_triple_passes() {
        assert_eq!(
            check_binding(
                &builtin_test_manifest(),
                &crate::registry::runtime_identity(),
                &crate::registry::adapter_identity(),
            ),
            Ok(())
        );
    }

    #[test]
    fn every_binding_field_is_checked_individually() {
        let runtime = crate::registry::runtime_identity();
        let adapter = crate::registry::adapter_identity();
        let mut manifest = builtin_test_manifest();
        manifest.runtime_binding.runtime = ocrx_inference_api::RuntimeId::new("other.cpp").unwrap();
        assert!(matches!(
            check_binding(&manifest, &runtime, &adapter),
            Err(InferenceError::IncompatibleBinding(field)) if field == "runtime id"
        ));
        let mut manifest = builtin_test_manifest();
        manifest.runtime_binding.wrapper_version = "0.1.160".into();
        assert!(matches!(
            check_binding(&manifest, &runtime, &adapter),
            Err(InferenceError::IncompatibleBinding(field)) if field == "runtime wrapper version"
        ));
        let mut manifest = builtin_test_manifest();
        manifest.runtime_binding.upstream_revision = "9e3b928".into();
        assert!(matches!(
            check_binding(&manifest, &runtime, &adapter),
            Err(InferenceError::IncompatibleBinding(field)) if field == "runtime upstream revision"
        ));
        let mut manifest = builtin_test_manifest();
        manifest.adapter_binding.adapter = ocrx_inference_api::AdapterId::new("qwen").unwrap();
        assert!(matches!(
            check_binding(&manifest, &runtime, &adapter),
            Err(InferenceError::IncompatibleBinding(field)) if field == "adapter id"
        ));
    }
}
