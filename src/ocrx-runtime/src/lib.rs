//! Runtime boundary shared by fake and real inference implementations.

use ocrx_core::CancellationToken;
pub use ocrx_core::GenerationProgress;

/// Runtime telemetry used by the W4.4 GPU/performance report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeTelemetry {
    pub load_time_ms: u64,
    pub first_token_ms: u64,
    pub generated_tokens: u64,
    pub generation_time_ms: u64,
}

impl RuntimeTelemetry {
    #[must_use]
    pub fn output_tokens_per_sec(&self) -> f64 {
        if self.generation_time_ms == 0 {
            0.0
        } else {
            self.generated_tokens as f64 / (self.generation_time_ms as f64 / 1_000.0)
        }
    }

    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            load_time_ms: self.load_time_ms.max(other.load_time_ms),
            first_token_ms: if self.first_token_ms == 0 {
                other.first_token_ms
            } else if other.first_token_ms == 0 {
                self.first_token_ms
            } else {
                self.first_token_ms.min(other.first_token_ms)
            },
            generated_tokens: self.generated_tokens.saturating_add(other.generated_tokens),
            generation_time_ms: self
                .generation_time_ms
                .saturating_add(other.generation_time_ms),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    Metal,
    Cuda,
    Vulkan,
}

impl Backend {
    /// Stable backend identifier used in doctor JSON and runtime reports
    /// (title case, e.g. `Cpu`); the doctor JSON consumes this directly so
    /// the spelling lives in exactly one place.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Backend::Cpu => "Cpu",
            Backend::Metal => "Metal",
            Backend::Cuda => "Cuda",
            Backend::Vulkan => "Vulkan",
        }
    }

    /// Whether this backend is compiled into the current binary (W1.1).
    ///
    /// The compiled set is decided by cargo features, not by hardware: CPU is
    /// always compiled in; Metal ships in every llama.cpp Apple build (the
    /// `metal` feature only forces the sys flag, so macOS behavior is
    /// unchanged); CUDA/Vulkan require their opt-in features. Availability
    /// probing must intersect hardware presence with this set - `auto` may
    /// never select a backend that is not compiled in.
    #[must_use]
    pub const fn is_compiled(self) -> bool {
        match self {
            Backend::Cpu => true,
            Backend::Metal => cfg!(target_os = "macos"),
            Backend::Cuda => cfg!(feature = "cuda"),
            Backend::Vulkan => cfg!(feature = "vulkan"),
        }
    }

    /// First compiled-in GPU backend in the frozen priority order
    /// `metal > cuda > vulkan`, or `None` on a CPU-only build.
    #[must_use]
    pub const fn first_compiled_gpu() -> Option<Self> {
        if Backend::Metal.is_compiled() {
            Some(Backend::Metal)
        } else if Backend::Cuda.is_compiled() {
            Some(Backend::Cuda)
        } else if Backend::Vulkan.is_compiled() {
            Some(Backend::Vulkan)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeInfo {
    pub backend: Backend,
    pub image_token_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceRequest {
    pub page: u32,
    pub width: u32,
    pub height: u32,
    pub image_rgb8: Vec<u8>,
    pub max_output_tokens: u32,
}

impl InferenceRequest {
    /// Builds a validated RGB8 request for a rendered page.
    ///
    /// # Errors
    ///
    /// Returns an error when dimensions, RGB8 length, or token limit is invalid.
    pub fn new(
        page: u32,
        width: u32,
        height: u32,
        image_rgb8: Vec<u8>,
        max_output_tokens: u32,
    ) -> Result<Self, String> {
        if width == 0
            || height == 0
            || image_rgb8.len() != width as usize * height as usize * 3
            || max_output_tokens == 0
        {
            return Err("invalid inference request".to_owned());
        }
        Ok(Self {
            page,
            width,
            height,
            image_rgb8,
            max_output_tokens,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceResponse {
    pub raw_output: String,
    pub finish_reason: String,
}

pub trait Runtime {
    fn info(&self) -> RuntimeInfo;

    /// Returns aggregated generation telemetry collected by this runtime.
    ///
    /// Fake/test runtimes use the default empty value; the real runtime keeps
    /// load, first-token, token-count, and generation-time counters updated
    /// across the pages it generates.
    fn telemetry(&self) -> RuntimeTelemetry {
        RuntimeTelemetry::default()
    }

    /// Generates one deterministic response for a validated image request.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is invalid or inference fails.
    fn generate(&mut self, request: InferenceRequest) -> Result<InferenceResponse, String>;

    /// Generates one response while polling `cancellation` and reporting
    /// `progress` at least once per elapsed second.
    ///
    /// The default implementation delegates to [`Runtime::generate`] and is
    /// suitable for fake runtimes that cannot observe cancellation. Real
    /// runtimes must override this so Ctrl+C stops token generation promptly
    /// instead of only at page boundaries.
    fn generate_with_progress(
        &mut self,
        request: InferenceRequest,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<InferenceResponse, String> {
        let _ = (cancellation, progress);
        self.generate(request)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod llama_mtmd;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use llama_mtmd::LlamaMtmdRuntime;

#[derive(Debug, Default)]
pub struct FakeRuntime;

impl Runtime for FakeRuntime {
    fn info(&self) -> RuntimeInfo {
        RuntimeInfo {
            backend: Backend::Cpu,
            image_token_limit: 1024,
        }
    }

    fn generate(&mut self, request: InferenceRequest) -> Result<InferenceResponse, String> {
        self.generate_with_progress(request, &CancellationToken::new(), &mut |_| {})
    }

    fn generate_with_progress(
        &mut self,
        request: InferenceRequest,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<InferenceResponse, String> {
        if request.width == 0
            || request.height == 0
            || request.image_rgb8.len() != (request.width as usize * request.height as usize * 3)
            || request.max_output_tokens == 0
        {
            return Err("invalid inference request".to_owned());
        }
        progress(GenerationProgress {
            generated_tokens: 1,
            elapsed_seconds: 0,
        });
        if cancellation.is_cancelled() {
            return Ok(InferenceResponse {
                raw_output: format!("page {}", request.page),
                finish_reason: "cancelled".to_owned(),
            });
        }
        Ok(InferenceResponse {
            raw_output: format!("page {}", request.page),
            finish_reason: "eos".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B1 guard (moved here from W0.1): the Windows target must declare the
    /// same pinned llama.cpp bindings as macOS, each with the `mtmd` feature.
    /// A Windows build without `mtmd` cannot compile `LlamaMtmdRuntime`, so a
    /// Cargo.toml that drops either crate or the feature is a build break the
    /// tests must catch before CI does.
    #[test]
    fn windows_target_declares_pinned_llama_bindings_with_mtmd() {
        let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .expect("ocrx-runtime Cargo.toml must be readable");
        let windows_section = manifest
            .split("[target.'cfg(target_os = \"windows\")'.dependencies]")
            .nth(1)
            .map(|rest| {
                rest.lines()
                    // The section ends at the next table header, not at any
                    // `[` (inline arrays like features = [...] also use one).
                    .take_while(|line| !line.trim_start().starts_with('['))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        for crate_name in ["llama-cpp-2", "llama-cpp-sys-2"] {
            let declaration = windows_section
                .lines()
                .find(|line| line.starts_with(crate_name))
                .unwrap_or_else(|| panic!("Windows target must declare {crate_name} (B1)"));
            assert!(
                declaration.contains("mtmd"),
                "{crate_name} must enable the mtmd feature on Windows"
            );
            assert!(
                !declaration.contains("workspace = true"),
                "{crate_name} stays a direct pinned dependency, not workspace-inherited"
            );
        }
    }

    #[test]
    fn fake_runtime_has_deterministic_inference_contract() {
        let mut runtime = FakeRuntime;
        let response = runtime
            .generate(InferenceRequest::new(2, 1, 1, vec![1, 2, 3], 128).unwrap())
            .unwrap();
        assert_eq!(response.raw_output, "page 2");
        assert_eq!(response.finish_reason, "eos");
        assert!(
            runtime
                .generate(InferenceRequest {
                    page: 2,
                    width: 1,
                    height: 1,
                    image_rgb8: Vec::new(),
                    max_output_tokens: 128,
                })
                .is_err()
        );
    }

    #[test]
    fn generate_with_progress_honors_cancellation() {
        let mut runtime = FakeRuntime;
        let cancellation = CancellationToken::new();
        cancellation.request_cancel();
        let response = runtime
            .generate_with_progress(
                InferenceRequest::new(1, 1, 1, vec![1, 2, 3], 128).unwrap(),
                &cancellation,
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(response.finish_reason, "cancelled");
    }

    #[test]
    fn generate_with_progress_emits_progress_observer() {
        let mut runtime = FakeRuntime;
        let mut progress = Vec::new();
        let response = runtime
            .generate_with_progress(
                InferenceRequest::new(1, 1, 1, vec![1, 2, 3], 128).unwrap(),
                &CancellationToken::new(),
                &mut |event| progress.push(event),
            )
            .unwrap();
        assert_eq!(response.finish_reason, "eos");
        assert_eq!(
            progress,
            vec![GenerationProgress {
                generated_tokens: 1,
                elapsed_seconds: 0,
            }]
        );
    }

    #[test]
    fn request_constructor_rejects_rgb8_dimension_mismatch() {
        assert!(InferenceRequest::new(1, 2, 2, vec![0; 3], 16).is_err());
    }

    #[test]
    fn backend_serializes_to_stable_lowercase_identifiers() {
        // Frozen golden: doctor JSON and runtime reports depend on these exact
        // strings. Adding a backend variant must extend, never rename, this set.
        assert_eq!(Backend::Cpu.as_str(), "Cpu");
        assert_eq!(Backend::Metal.as_str(), "Metal");
        assert_eq!(Backend::Cuda.as_str(), "Cuda");
        assert_eq!(Backend::Vulkan.as_str(), "Vulkan");
    }

    #[test]
    fn backend_compiled_set_is_cargo_feature_gated() {
        // W1.1: the backends compiled into the binary are decided by cargo
        // features, not by hardware presence. The workspace test gate builds
        // with default features, so this binary is CPU-only on Windows.
        assert!(Backend::Cpu.is_compiled(), "CPU is always compiled in");
        // Metal ships in every llama.cpp Apple build; the explicit `metal`
        // feature only forces the sys flag, so macOS keeps reporting it.
        assert_eq!(Backend::Metal.is_compiled(), cfg!(target_os = "macos"));
        assert!(
            !Backend::Cuda.is_compiled(),
            "cuda requires the cuda cargo feature"
        );
        assert!(
            !Backend::Vulkan.is_compiled(),
            "vulkan requires the vulkan cargo feature"
        );
    }

    #[test]
    fn first_compiled_gpu_backend_respects_feature_gates() {
        // Default (CPU-only) build: no GPU backend on Windows, Metal kept on
        // macOS so v0.1 Metal-only behavior is unchanged.
        if cfg!(target_os = "macos") {
            assert_eq!(Backend::first_compiled_gpu(), Some(Backend::Metal));
        } else {
            assert_eq!(Backend::first_compiled_gpu(), None);
        }
    }
}
