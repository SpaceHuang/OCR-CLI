//! Runtime boundary shared by fake and real inference implementations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    Metal,
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

#[cfg(target_os = "macos")]
mod llama_mtmd;

#[cfg(target_os = "macos")]
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
}
