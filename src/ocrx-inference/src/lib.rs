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

pub use device::DeviceManagerImpl;

#[cfg(test)]
pub(crate) fn builtin_test_manifest() -> ocrx_inference_api::ModelManifest {
    ocrx_models::builtin::ovisocr2::ovisocr2_manifest("compact").unwrap()
}
pub use probe::{FakeSystemProbe, RealSystemProbe, SystemProbe};
pub use registry::RegistryVerifier;
pub use service::{InferenceServiceImpl, InferenceSessionImpl};
pub use telemetry::{start_vram_sampler, stop_vram_sampler};
