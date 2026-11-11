//! Common inference task and response contract (plan §5.3).
//!
//! A `TaskRequest` carries no page number, PDF path, checkpoint, or output
//! directory: requests and pages have no one-to-one relationship (a column-
//! split retry decomposes one page into N sub-requests). The scan app owns
//! pages; the platform owns decode semantics.

use ocrx_primitives::GenerationProgress;
use serde::{Deserialize, Serialize};

use crate::error::InferenceError;
use crate::identity::StepProfile;
use crate::id::BackendId;

/// Frozen floor window depth for multi-page continuous batching (phase-2/4:
/// the low tier of the {8, 16} ladder; also the post-fallback depth).
pub const WINDOW_DEPTH: usize = 8;

/// High-VRAM window depth tier (phase-4 tiering).
pub const WINDOW_DEPTH_HIGH_VRAM: usize = 16;

/// Decode-generation marker for checkpoint compatibility (quality plan
/// Q1.3 / D-15): the wrapper era that produced a page. Generation 1 =
/// llama.cpp wrapper 0.1.152/0.1.153; generation 2 = 0.1.161 (sampler-
/// capable decode). The platform produces the value through the identity
/// API; the scan writes `decode=<gen>` from the identity, never hardcoded.
pub const DECODE_GENERATION: u32 = 2;

/// MTMD token-grid bindings the platform accepts for
/// `image_tokens_override` (frozen ladder uses 2048 only; `None` = the
/// scan-level 1024 budget).
pub const SUPPORTED_IMAGE_TOKEN_GRIDS: &[u32] = &[2048];

/// Phase-4 depth tiering: the window depth is selected per device from the
/// GPU's VRAM capacity (12 GiB threshold opens the 16 tier; CPU or probe
/// unavailable keeps the floor). Tier ladder is {8, 16} only — depth 12
/// measured a kernel-quantization pit and is skipped entirely.
#[must_use]
pub fn window_depth_for_vram(vram_bytes: Option<u64>) -> usize {
    const TWELVE_GIB: u64 = 12 * 1024 * 1024 * 1024;
    match vram_bytes {
        Some(vram) if vram >= TWELVE_GIB => WINDOW_DEPTH_HIGH_VRAM,
        _ => WINDOW_DEPTH,
    }
}

/// One decoded image input. The scan renders RGB8; the platform validates
/// shape and length at the API boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInput {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub rgb8: Vec<u8>,
}

impl ImageInput {
    /// Validates image shape/length and builds the input.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`] for zero dimensions, an empty
    /// id, or an RGB8 length that is not `width * height * 3`.
    pub fn new(
        id: impl Into<String>,
        width: u32,
        height: u32,
        rgb8: Vec<u8>,
    ) -> Result<Self, InferenceError> {
        let id = id.into();
        if id.is_empty() {
            return Err(InferenceError::InvalidTask("empty image id".into()));
        }
        if width == 0 || height == 0 {
            return Err(InferenceError::InvalidTask(
                "image dimensions must be positive".into(),
            ));
        }
        if rgb8.len() != width as usize * height as usize * 3 {
            return Err(InferenceError::InvalidTask(
                "rgb8 length must equal width * height * 3".into(),
            ));
        }
        Ok(Self {
            id,
            width,
            height,
            rgb8,
        })
    }
}

/// Frozen generation parameters (plan §2.3: greedy, thinking off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationParams {
    pub max_output_tokens: u32,
    pub temperature_milli: u32,
    pub thinking: bool,
}

/// Escape-ladder sampling for degeneration retries only (quality plan Q1.2,
/// D-1/D-13): an ordered sampler chain `repeat_penalty → greedy` — the
/// penalty modifies logits FIRST, greedy makes the final pick; implemented
/// the other way around the penalty never takes effect. The chain is
/// deterministic (no random source), so a retry scan stays byte-reproducible
/// under identical inputs. Normal pages never carry this (pure greedy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrySampling {
    /// Repeat penalty in milli-units (1150 = 1.15x), mirroring the
    /// `temperature_milli` style so the request stays `Eq`-able and the
    /// sweep grid is expressed in exact integers.
    pub repeat_penalty_milli: u32,
    pub penalty_window: u32,
}

impl RetrySampling {
    /// Builds a validated retry-sampling spec.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`] when the penalty is outside
    /// `1000..=2000` milli (1.0x..=2.0x) or the window is zero.
    pub fn new(repeat_penalty_milli: u32, penalty_window: u32) -> Result<Self, InferenceError> {
        if !(1000..=2000).contains(&repeat_penalty_milli) {
            return Err(InferenceError::InvalidTask(
                "retry repeat_penalty must be within 1000..=2000 milli".into(),
            ));
        }
        if penalty_window == 0 {
            return Err(InferenceError::InvalidTask(
                "retry penalty_window must be >= 1".into(),
            ));
        }
        Ok(Self {
            repeat_penalty_milli,
            penalty_window,
        })
    }

    /// The penalty as the f32 factor llama.cpp consumes.
    #[must_use]
    pub fn repeat_penalty(&self) -> f32 {
        self.repeat_penalty_milli as f32 / 1000.0
    }
}

/// A generic, PDF-free inference task (plan §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRequest {
    pub prompt: String,
    pub images: Vec<ImageInput>,
    pub generation: GenerationParams,
    /// Per-decode MTMD token-grid override (Q1.4c); `None` = scan-level
    /// budget. The platform binds that decode's context grid to the value.
    pub image_tokens_override: Option<u32>,
    /// Ordered sampler chain for degeneration retries (Q1.2); `None` = pure
    /// greedy. The platform does not interpret its business meaning.
    pub retry_sampling: Option<RetrySampling>,
}

impl TaskRequest {
    /// Builds a validated task request (plan §5.3 constraints).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`] for an empty prompt, invalid
    /// images, duplicate image ids, an unsupported grid override, or a zero
    /// max-output-token budget.
    pub fn new(
        prompt: impl Into<String>,
        images: Vec<ImageInput>,
        generation: GenerationParams,
    ) -> Result<Self, InferenceError> {
        let request = Self {
            prompt: prompt.into(),
            images,
            generation,
            image_tokens_override: None,
            retry_sampling: None,
        };
        request.validate()?;
        Ok(request)
    }

    /// Attaches the retry grid override (validated value).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`] when the grid is not a
    /// supported MTMD binding value.
    pub fn with_image_tokens_override(mut self, grid: u32) -> Result<Self, InferenceError> {
        if !SUPPORTED_IMAGE_TOKEN_GRIDS.contains(&grid) {
            return Err(InferenceError::InvalidTask(format!(
                "unsupported image token grid {grid}"
            )));
        }
        self.image_tokens_override = Some(grid);
        Ok(self)
    }

    /// Attaches the retry sampler chain (degeneration-retry path only).
    #[must_use]
    pub fn with_retry_sampling(mut self, retry_sampling: RetrySampling) -> Self {
        self.retry_sampling = Some(retry_sampling);
        self
    }

    /// API-level validation shared by the constructor and revalidation.
    ///
    /// # Errors
    ///
    /// See [`TaskRequest::new`].
    pub fn validate(&self) -> Result<(), InferenceError> {
        if self.prompt.trim().is_empty() {
            return Err(InferenceError::InvalidTask("prompt must be non-empty".into()));
        }
        if self.generation.max_output_tokens == 0 {
            return Err(InferenceError::InvalidTask(
                "max_output_tokens must be positive".into(),
            ));
        }
        let mut seen_ids = std::collections::BTreeSet::new();
        for image in &self.images {
            if !seen_ids.insert(image.id.as_str()) {
                return Err(InferenceError::InvalidTask(format!(
                    "duplicate image id {}",
                    image.id
                )));
            }
            if image.width == 0
                || image.height == 0
                || image.rgb8.len() != image.width as usize * image.height as usize * 3
            {
                return Err(InferenceError::InvalidTask(format!(
                    "invalid image shape for id {}",
                    image.id
                )));
            }
        }
        if let Some(grid) = self.image_tokens_override
            && !SUPPORTED_IMAGE_TOKEN_GRIDS.contains(&grid)
        {
            return Err(InferenceError::InvalidTask(format!(
                "unsupported image token grid {grid}"
            )));
        }
        Ok(())
    }
}

/// Why generation stopped (same variant set as the scan-side termination
/// enum; `max_tokens` = budget exhausted, `repetition_loop` = ring detected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Eos,
    StopToken,
    MaxTokens,
    Invalid,
    RepetitionLoop,
}

impl FinishReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eos => "eos",
            Self::StopToken => "stop_token",
            Self::MaxTokens => "max_tokens",
            Self::Invalid => "invalid",
            Self::RepetitionLoop => "repetition_loop",
        }
    }
}

/// Token accounting reported by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: u64,
}

/// Actually effective decode-variant echo (RBLK-1): filled by the backend
/// per the values it really executed — never copied from the request and
/// never defaulted to empty by the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecodeEcho {
    pub image_tokens: Option<u32>,
    pub sampling: Option<RetrySampling>,
}

/// Generic task response (plan §5.3 dual-text contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskResponse {
    /// Normalized text for downstream consumption: `<think>` stripped by the
    /// adapter decode (single source, Q3.1).
    pub text: String,
    /// Normalized raw output before stripping and tail-cropping: the sole
    /// data source of the `page-NNNN.raw` sidecar (baseline semantics).
    pub raw_text: String,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
    /// Adapter decode-time cleanup count: full think blocks (including
    /// silent empty strips) + stray tags.
    pub think_blocks_stripped: u32,
    /// Echo of the actually effective decode variants (backend-produced).
    pub decode_echo: DecodeEcho,
    pub warnings: Vec<String>,
}

/// Session-level execution summary consumed through the public API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExecution {
    /// Actually effective batch depth for windowed submissions.
    pub batch_depth: usize,
    /// Actually executed backend.
    pub backend: BackendId,
    /// Cumulative step profile snapshot, present only when the diagnostic
    /// channel is enabled.
    pub step_profile: Option<StepProfile>,
}

/// Window-depth formatter kept for the (input, batch composition, depth)
/// determinism contract diagnostics.
#[must_use]
pub fn window_depth_summary(identity_batch_depth: usize) -> String {
    format!("window-depth={identity_batch_depth}")
}

/// Progress event alias re-exported for session trait signatures.
pub type ProgressEvent = GenerationProgress;

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> GenerationParams {
        GenerationParams {
            max_output_tokens: 1024,
            temperature_milli: 0,
            thinking: false,
        }
    }

    fn image(id: &str) -> ImageInput {
        ImageInput::new(id, 2, 2, vec![0; 12]).unwrap()
    }

    #[test]
    fn task_request_has_no_page_or_pdf_fields_on_the_wire() {
        // T015: the wire format proves the common request carries no page
        // number, PDF path, checkpoint, or output directory field.
        let request = TaskRequest::new("prompt", vec![image("img")], params()).unwrap();
        let value = serde_json::to_value(&request.prompt.as_str()).unwrap();
        assert_eq!(value, serde_json::Value::String("prompt".into()));
        // Structural check: the public type's Debug shape must not contain
        // forbidden field names.
        let debug = format!("{request:?}");
        for forbidden in ["page", "pdf", "output_root", "checkpoint", "PageClassification"] {
            assert!(
                !debug.contains(forbidden),
                "TaskRequest must not carry {forbidden}"
            );
        }
    }

    #[test]
    fn task_request_validation_rejects_bad_inputs() {
        assert!(matches!(
            TaskRequest::new("", vec![], params()),
            Err(InferenceError::InvalidTask(_))
        ));
        assert!(matches!(
            TaskRequest::new("p", vec![], GenerationParams {
                max_output_tokens: 0,
                temperature_milli: 0,
                thinking: false
            }),
            Err(InferenceError::InvalidTask(_))
        ));
        // Duplicate image ids are rejected (API-level uniqueness).
        let duplicate = TaskRequest {
            prompt: "p".into(),
            images: vec![image("same"), image("same")],
            generation: params(),
            image_tokens_override: None,
            retry_sampling: None,
        };
        assert!(matches!(duplicate.validate(), Err(InferenceError::InvalidTask(_))));
        // Bad grid override rejected.
        let bad_grid = TaskRequest::new("p", vec![image("a")], params())
            .unwrap()
            .with_image_tokens_override(4096)
            .unwrap_err();
        assert!(matches!(bad_grid, InferenceError::InvalidTask(_)));
        // Supported grid accepted.
        assert!(TaskRequest::new("p", vec![image("a")], params())
            .unwrap()
            .with_image_tokens_override(2048)
            .is_ok());
    }

    #[test]
    fn image_input_rejects_shape_mismatch_and_empty_ids() {
        assert!(ImageInput::new("img", 2, 2, vec![0; 3]).is_err());
        assert!(ImageInput::new("img", 0, 2, vec![]).is_err());
        assert!(ImageInput::new("", 1, 1, vec![0; 3]).is_err());
        assert!(ImageInput::new("img", 1, 1, vec![1, 2, 3]).is_ok());
    }

    #[test]
    fn retry_sampling_validates_penalty_and_window() {
        assert!(RetrySampling::new(1_100, 64).is_ok());
        assert!(RetrySampling::new(1_000, 1).is_ok());
        assert!(RetrySampling::new(2_000, 256).is_ok());
        assert!(matches!(
            RetrySampling::new(900, 64),
            Err(InferenceError::InvalidTask(_))
        ));
        assert!(matches!(
            RetrySampling::new(2_500, 64),
            Err(InferenceError::InvalidTask(_))
        ));
        assert!(matches!(
            RetrySampling::new(1_100, 0),
            Err(InferenceError::InvalidTask(_))
        ));
        let spec = RetrySampling::new(1_150, 256).unwrap();
        assert!((spec.repeat_penalty() - 1.15).abs() < 1e-6);
    }

    #[test]
    fn finish_reason_serializes_to_stable_snake_case_values() {
        assert_eq!(FinishReason::Eos.as_str(), "eos");
        assert_eq!(FinishReason::StopToken.as_str(), "stop_token");
        assert_eq!(FinishReason::MaxTokens.as_str(), "max_tokens");
        assert_eq!(FinishReason::Invalid.as_str(), "invalid");
        assert_eq!(FinishReason::RepetitionLoop.as_str(), "repetition_loop");
        assert_eq!(
            serde_json::to_string(&FinishReason::RepetitionLoop).unwrap(),
            "\"repetition_loop\""
        );
    }

    #[test]
    fn window_depth_tiers_by_vram_and_skips_the_pit() {
        assert_eq!(window_depth_for_vram(None), 8);
        assert_eq!(window_depth_for_vram(Some(0)), 8);
        let twelve_gib = 12_u64 * 1024 * 1024 * 1024;
        assert_eq!(window_depth_for_vram(Some(twelve_gib)), 16);
        assert_eq!(window_depth_for_vram(Some(twelve_gib - 1)), 8);
        assert_eq!(WINDOW_DEPTH, 8);
        assert_eq!(WINDOW_DEPTH_HIGH_VRAM, 16);
    }

    #[test]
    fn decode_echo_defaults_to_none_for_both_variants() {
        assert_eq!(DecodeEcho::default(), DecodeEcho {
            image_tokens: None,
            sampling: None,
        });
        assert_eq!(window_depth_summary(8), "window-depth=8");
    }
}
