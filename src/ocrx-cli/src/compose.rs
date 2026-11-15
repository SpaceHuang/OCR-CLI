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

use ocrx_inference::{CurlTransport, ModelManagerImpl};
use ocrx_inference::{DeviceManagerImpl, InferenceServiceImpl, RealSystemProbe, RegistryVerifier};
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
    let cache_root = model_cache.unwrap_or_else(ocrx_inference::default_cache_root);
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
    // Test seam (same family as OCRX_MEMORY_GB): when OCRX_E2E_CAPTURE_OUT
    // names a file, the runtime factory is swapped for a capture fake that
    // serves a canned EOS response per request and appends every encoded
    // prompt to that file — the C7 CLI-driven prompt-capture harness (plan
    // O3-T16: the assertion must ride the real subprocess path). Production
    // runs never set the variable and always take the llama factory.
    let factory: Box<dyn ocrx_inference_api::RuntimeFactory> =
        if let Some(out) = std::env::var_os("OCRX_E2E_CAPTURE_OUT") {
            Box::new(CaptureFactory {
                out: PathBuf::from(out),
            })
        } else {
            Box::new(ocrx_inference::LlamaRuntimeFactory)
        };
    let inference = InferenceServiceImpl::new(manager, devices, factory);
    let scan = ScanApplication::new();
    Composed {
        inference,
        scan,
        cache_root,
    }
}

use std::sync::{Arc, Mutex};

/// Capture fake factory (C7 harness): hands out a backend that records the
/// encoded prompt of every task and serves a deterministic EOS response.
struct CaptureFactory {
    out: PathBuf,
}

impl ocrx_inference_api::RuntimeFactory for CaptureFactory {
    fn create(
        &self,
        _request: &ocrx_inference_api::RuntimeLoadRequest,
    ) -> Result<ocrx_inference_api::BackendHandle, ocrx_inference_api::InferenceError> {
        use std::sync::{Arc, Mutex};
        let sink = Arc::new(Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.out)
                .map_err(|error| {
                    ocrx_inference_api::InferenceError::RuntimeInitialization(error.to_string())
                })?,
        ));
        Ok(ocrx_inference_api::BackendHandle {
            backend: Box::new(CaptureBackend { sink }),
            runtime: ocrx_inference::platform_runtime_identity().map_err(|error| {
                ocrx_inference_api::InferenceError::RuntimeInitialization(error.to_string())
            })?,
            backend_id: ocrx_inference_api::BackendId::new("cpu").map_err(|error| {
                ocrx_inference_api::InferenceError::RuntimeInitialization(error.to_string())
            })?,
            image_token_limit: 1024,
        })
    }
}

/// Records every encoded prompt; answers EOS with a fixed markdown body.
struct CaptureBackend {
    sink: Arc<Mutex<std::fs::File>>,
}

impl CaptureBackend {
    fn record(&self, prompt: &str) {
        use std::io::Write as _;
        // Exact framing: the prompt bytes, one newline, then the separator
        // WITH its trailing newline — splitting on the separator line
        // therefore yields exactly the prompt bytes plus one trailing
        // newline (never a stray leading newline on the next segment).
        let mut file = self.sink.lock().unwrap();
        let _ = write!(
            file,
            "{prompt}
---PROMPT---
"
        );
    }

    fn canned(prompt_captured: bool) -> ocrx_inference_api::BackendOutput {
        let _ = prompt_captured;
        ocrx_inference_api::BackendOutput {
            text: "# captured

body"
                .to_owned(),
            native_finish_reason: ocrx_inference_api::NativeFinishReason::Eos,
            usage: ocrx_inference_api::TokenUsage {
                input_tokens: Some(1),
                output_tokens: 2,
            },
            decode_echo: ocrx_inference_api::DecodeEcho::default(),
        }
    }
}

impl ocrx_inference_api::RuntimeBackend for CaptureBackend {
    fn generate(
        &mut self,
        task: ocrx_inference_api::EncodedTask,
        _cancellation: &ocrx_core::CancellationToken,
        _progress: &mut dyn FnMut(ocrx_core::GenerationProgress),
    ) -> Result<ocrx_inference_api::BackendOutput, ocrx_inference_api::InferenceError> {
        self.record(&task.prompt);
        Ok(Self::canned(true))
    }

    fn generate_window(
        &mut self,
        tasks: Vec<ocrx_inference_api::EncodedTask>,
        _cancellation: &ocrx_core::CancellationToken,
        _progress: &mut dyn FnMut(usize, ocrx_core::GenerationProgress),
    ) -> Vec<Result<ocrx_inference_api::BackendOutput, ocrx_inference_api::InferenceError>> {
        tasks
            .iter()
            .map(|task| {
                self.record(&task.prompt);
                Ok(Self::canned(true))
            })
            .collect()
    }

    fn telemetry(&self) -> ocrx_inference_api::RuntimeTelemetry {
        ocrx_inference_api::RuntimeTelemetry::default()
    }

    fn step_profile(&self) -> Option<ocrx_inference_api::StepProfile> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::{
        DeviceManager as _, InferenceService as _, ModelId, ModelManager as _,
    };

    #[test]
    fn composition_resolves_the_registered_models_in_declaration_order() {
        // Rooted in a temp cache so the test never touches the user cache.
        let root = std::env::temp_dir().join(format!("ocrx-compose-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let composed = compose(Some(root.clone()));
        // model list through the manager: ovisocr2 first (declaration
        // order), then the OvisOCR3 addition (§8.3 expected difference ①).
        let summaries = composed.inference.manager().list().unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].model.as_str(), "ovisocr2");
        assert_eq!(summaries[0].display.display_name, "OvisOCR2");
        assert_eq!(summaries[1].model.as_str(), "ovisocr3");
        assert_eq!(summaries[1].display.display_name, "OvisOCR3");
        // Device manager reports the doctor overview without a model load.
        let diagnostics = composed.inference.devices().diagnose();
        assert_eq!(diagnostics.profiles.len(), 4);
        // Static identity resolves via the frozen memory ladder (both
        // models share the frozen runtime pin, D-4 GO branch).
        let statik = composed
            .inference
            .static_identity(&ModelId::new("ovisocr2").unwrap(), "auto")
            .unwrap();
        assert_eq!(statik.runtime.runtime.as_str(), "llama.cpp");
        // Upstream main bumped the decode generation to 3 (token pieces
        // accumulate as bytes before UTF-8 decoding); identity must match.
        assert_eq!(statik.decode_generation, 3);
        // The ovisocr3 static identity resolves over its single-rung set.
        let statik3 = composed
            .inference
            .static_identity(&ModelId::new("ovisocr3").unwrap(), "auto")
            .unwrap();
        assert_eq!(statik3.decode_generation, 3);
        assert_eq!(statik3.model.profile.as_str(), "quality");
        let _ = std::fs::remove_dir_all(root);
    }
}
