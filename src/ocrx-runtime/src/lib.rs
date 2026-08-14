//! Runtime boundary shared by fake and real inference implementations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    Metal,
    Cuda,
    Vulkan,
}

impl Backend {
    /// Stable lowercase identifier used in doctor JSON and runtime reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Backend::Cpu => "Cpu",
            Backend::Metal => "Metal",
            Backend::Cuda => "Cuda",
            Backend::Vulkan => "Vulkan",
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
    /// Generates one deterministic response for a validated image request.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is invalid or inference fails.
    fn generate(&mut self, request: InferenceRequest) -> Result<InferenceResponse, String>;
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
        if request.width == 0
            || request.height == 0
            || request.image_rgb8.len() != (request.width as usize * request.height as usize * 3)
            || request.max_output_tokens == 0
        {
            return Err("invalid inference request".to_owned());
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
}
