//! Production composition root (plan §6.1, T069).
//!
//! The frozen order: catalog → probe/device manager + runtime factory →
//! static registry → `BindingVerifier` implementation → inject the verifier
//! into `ModelManagerImpl` → `InferenceServiceImpl` → `ScanApplication`
//! (RBLK-9: registry-backed binding verification is shared by `model
//! verify` and prepare with zero reverse dependency).
//!
//! The CLI is the ONLY production composition point; tests compose their
//! own fakes in their own modules.

use std::path::PathBuf;

use ocrx_inference::{DeviceManagerImpl, InferenceServiceImpl, RealSystemProbe, RegistryVerifier};
use ocrx_inference_api::{DeviceManager, InferenceService};
use ocrx_models::{CurlTransport, ModelManagerImpl};
use ocrx_scan::ScanApplication;

/// One assembled production context (all services behind their ports).
pub struct Composed {
    /// `model`/`doctor` commands drive the manager through the service.
    pub inference: InferenceServiceImpl,
    /// The scan application (port-driven; the CLI wires PDF/render per run).
    pub scan: ScanApplication,
    /// Resolved model cache root (doctor display + install target).
    pub cache_root: PathBuf,
}

/// Assembles the production context (plan §6.1 order; `model_cache`
/// overrides the platform default for tests).
#[must_use]
pub fn compose(model_cache: Option<PathBuf>) -> Composed {
    // 1. Static catalog comes from the models crate (built-in Ovis set).
    // 2. Probe + device manager (compiled ∩ hardware) and the runtime
    //    factory live behind the inference crate's production impls.
    // 3. Static registry → 4. RegistryVerifier (BindingVerifier port).
    // 5. ModelManagerImpl with the verifier injected (models never reads the
    //    registry itself — RBLK-9).
    // 6. InferenceServiceImpl over the manager + devices + factory.
    // 7. ScanApplication (port-driven).
    let cache_root = model_cache.unwrap_or_else(ocrx_models::default_cache_root);
    let manager = ModelManagerImpl::new(
        cache_root.clone(),
        Box::new(RegistryVerifier),
        Box::new(CurlTransport),
        None,
    );
    let devices = DeviceManagerImpl::new(
        Box::new(RealSystemProbe),
        std::env::var("OCRX_MEMORY_GB")
            .ok()
            .and_then(|value| value.parse().ok()),
        cache_root.clone(),
    );
    let inference = InferenceServiceImpl::new(
        manager,
        devices,
        Box::new(ocrx_runtime::LlamaRuntimeFactory),
    );
    let scan = ScanApplication::new();
    Composed {
        inference,
        scan,
        cache_root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::{ModelId, ModelManager};

    #[test]
    fn composition_resolves_the_single_registered_model() {
        // Rooted in a temp cache so the test never touches the user cache.
        let root = std::env::temp_dir().join(format!("ocrx-compose-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let composed = compose(Some(root.clone()));
        // model list through the manager: exactly one model, frozen metadata.
        let summaries = composed.inference.manager().list().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].model.as_str(), "ovisocr2");
        assert_eq!(summaries[0].display.display_name, "OvisOCR2");
        // Device manager reports the doctor overview without a model load.
        let diagnostics = composed.inference.devices().diagnose();
        assert_eq!(diagnostics.profiles.len(), 4);
        // Static identity resolves via the frozen memory ladder.
        let statik = composed
            .inference
            .static_identity(&ModelId::new("ovisocr2").unwrap(), "auto")
            .unwrap();
        assert_eq!(statik.runtime.runtime.as_str(), "llama.cpp");
        assert_eq!(statik.decode_generation, 2);
        let _ = std::fs::remove_dir_all(root);
    }
}
