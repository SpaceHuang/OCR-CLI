//! Production inference composition (plan §4.1/§8.8): static registry,
//! binding verification, device management, system probe, and the
//! `InferenceService` implementation that prepares sessions over the
//! llama.cpp backend and the Ovis adapter.

pub mod binding;
pub mod device;
pub mod probe;
pub mod registry;
pub mod service;
pub mod telemetry;

// Runtime factory re-export: the CLI composition root builds the llama
// factory without a direct runtime dependency (plan §8.13).
pub use ocrx_runtime::LlamaRuntimeFactory;

pub use device::DeviceManagerImpl;

#[cfg(test)]
pub(crate) fn builtin_test_manifest() -> ocrx_inference_api::ModelManifest {
    ocrx_models::builtin::ovisocr2::ovisocr2_manifest("compact").unwrap()
}
pub use probe::{FakeSystemProbe, RealSystemProbe, SystemProbe};
pub use registry::RegistryVerifier;
pub use service::{InferenceServiceImpl, InferenceSessionImpl};
pub use telemetry::{start_vram_sampler, stop_vram_sampler};

// Composition-root conveniences: the CLI is the only production root and
// must not depend on the models crate directly (plan §8.13).
pub use ocrx_models::default_cache_root;
pub mod builtin {
    pub use ocrx_models::builtin::ovisocr2::*;
}
pub use ocrx_models::{CurlTransport, ModelManagerImpl, recommend as recommend_module};
pub mod recommend {
    pub use ocrx_models::recommend::*;
}
pub mod verify {
    pub use ocrx_models::verify::*;
}
