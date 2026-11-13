//! Ovis protocol adapter (plan §5.4/§8.7): the only module of this crate in
//! the target architecture. It owns how a generic task enters the Ovis
//! protocol and how a backend output decodes back — nothing else.
//!
//! Decode responsibilities (§3.4 item 6, Q3.1 single source): newline-
//! normalize the raw backend output → strip `<think>` blocks with the Q3.1
//! count → produce the dual-text response. The echo passes through from
//! `BackendOutput` untouched (RBLK-1 channel). Tail cropping, bbox, page
//! documents, and markdown are scan-application business and never appear
//! here.

use ocrx_inference_api::{
    AdapterId, AdapterIdentity, BackendOutput, EncodedTask, FinishReason, ImagePlacement,
    InferenceError, NativeFinishReason, ProtocolAdapter, TaskRequest, TaskResponse,
};

/// Adapter identity (frozen display pair, plan §7.4).
pub const OVIS_ADAPTER_ID: &str = "ovisocr2";
pub const OVIS_ADAPTER_VERSION: &str = "0.1.0";

/// Ovis protocol adapter: images sit before the prompt (the backend inserts
/// the runtime-native MTMD marker); decode strips `<think>`.
#[derive(Debug, Clone)]
pub struct OvisProtocolAdapter {
    identity: AdapterIdentity,
}

impl Default for OvisProtocolAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OvisProtocolAdapter {
    /// Builds the adapter with the frozen identity.
    ///
    /// # Panics
    ///
    /// Never: the frozen adapter id is a compile-time constant validated by
    /// tests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            identity: AdapterIdentity {
                adapter: AdapterId::new(OVIS_ADAPTER_ID).expect("frozen adapter id is valid"),
                version: OVIS_ADAPTER_VERSION.to_owned(),
            },
        }
    }
}

impl ProtocolAdapter for OvisProtocolAdapter {
    fn identity(&self) -> &AdapterIdentity {
        &self.identity
    }

    /// Encodes one validated task. The Ovis protocol carries exactly one
    /// image before the prompt; the prompt passes through byte-identically.
    ///
    /// # Panics
    ///
    /// Never: validation returns errors instead.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`] for a multi-image request or
    /// a non-Ovis image placement.
    fn encode(&self, request: TaskRequest) -> Result<EncodedTask, InferenceError> {
        if request.images.len() != 1 {
            return Err(InferenceError::InvalidTask(format!(
                "ovis protocol carries exactly one image per task, got {}",
                request.images.len()
            )));
        }
        Ok(EncodedTask {
            prompt: request.prompt,
            images: request.images,
            image_placement: ImagePlacement::BeforePrompt,
            generation: request.generation,
            image_tokens_override: request.image_tokens_override,
            retry_sampling: request.retry_sampling,
        })
    }

    /// Decodes one backend output (§5.4):
    /// normalize newlines → strip `<think>` (counting full blocks including
    /// silent empty strips and stray tags) → dual-text response with the
    /// decode echo passed through untouched.
    ///
    /// # Panics
    ///
    /// Never: the strip helper is total over any input.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::AdapterDecode`] for a finish reason the
    /// protocol cannot interpret.
    fn decode(&self, output: BackendOutput) -> Result<TaskResponse, InferenceError> {
        let normalized_raw = normalize_line_endings(&output.text);
        let think = crate::strip_think_blocks(&normalized_raw);
        let stripped_count = u32::try_from(think.total()).unwrap_or(u32::MAX);
        // `think_removed` rides the structured warning channel so the scan's
        // page-document build reproduces the baseline ThinkRemoved warning
        // (the adapter is the single source that knows nonempty vs stray).
        let warnings = if think.nonempty > 0 {
            vec!["think_removed".to_owned()]
        } else {
            Vec::new()
        };
        Ok(TaskResponse {
            // Downstream (md/page.json/bbox) consumes the cleaned text only.
            text: think.text,
            // The raw sidecar data source: normalized, pre-strip, pre-crop.
            raw_text: normalized_raw,
            finish_reason: finish_reason(output.native_finish_reason)?,
            usage: output.usage,
            think_blocks_stripped: stripped_count,
            // RBLK-1: the echo comes from the backend output, never from a
            // request copy and never defaulted.
            decode_echo: output.decode_echo,
            warnings,
        })
    }
}

/// Maps the native finish reason onto the generic set (Ovis protocol: 1:1).
fn finish_reason(native: NativeFinishReason) -> Result<FinishReason, InferenceError> {
    Ok(match native {
        NativeFinishReason::Eos => FinishReason::Eos,
        NativeFinishReason::StopToken => FinishReason::StopToken,
        NativeFinishReason::MaxTokens => FinishReason::MaxTokens,
        NativeFinishReason::Invalid => FinishReason::Invalid,
        NativeFinishReason::RepetitionLoop => FinishReason::RepetitionLoop,
    })
}

/// Newline normalization shared with the legacy entry (`\r\n` and stray
/// `\r` fold to `\n`); the raw sidecar is this normalization result.
pub(crate) fn normalize_line_endings(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::{DecodeEcho, GenerationParams, ImageInput, RetrySampling, TokenUsage};

    fn adapter() -> OvisProtocolAdapter {
        OvisProtocolAdapter::new()
    }

    fn request(prompt: &str) -> TaskRequest {
        TaskRequest::new(
            prompt,
            vec![ImageInput::new("img", 1, 1, vec![1, 2, 3]).unwrap()],
            GenerationParams {
                max_output_tokens: 1024,
                temperature_milli: 0,
                thinking: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn identity_is_the_frozen_display_pair() {
        let adapter = adapter();
        assert_eq!(adapter.identity().adapter.as_str(), "ovisocr2");
        assert_eq!(adapter.identity().version, "0.1.0");
    }

    #[test]
    fn encode_places_the_image_before_an_untouched_prompt() {
        let adapter = adapter();
        // T006 golden: the prompt bytes pass through unchanged (leading and
        // middle newlines preserved).
        let frozen_prompt = "\nExtract all readable content from the image\nPreserve the original text without translation or paraphrasing.";
        let encoded = adapter.encode(request(frozen_prompt)).unwrap();
        assert_eq!(encoded.prompt, frozen_prompt);
        assert_eq!(encoded.image_placement, ImagePlacement::BeforePrompt);
        assert_eq!(encoded.images.len(), 1);
        assert_eq!(encoded.generation.max_output_tokens, 1024);
        assert_eq!(encoded.generation.temperature_milli, 0);
        assert!(!encoded.generation.thinking);
        // Retry variants ride along untouched.
        let mut retry = request("p");
        retry.image_tokens_override = Some(2048);
        retry.retry_sampling = Some(RetrySampling::new(1_200, 64).unwrap());
        let encoded = adapter.encode(retry).unwrap();
        assert_eq!(encoded.image_tokens_override, Some(2048));
        assert!(encoded.retry_sampling.is_some());
    }

    #[test]
    fn encode_rejects_multi_image_requests() {
        let mut multi = request("p");
        multi
            .images
            .push(ImageInput::new("img2", 1, 1, vec![4, 5, 6]).unwrap());
        assert!(matches!(
            adapter().encode(multi),
            Err(InferenceError::InvalidTask(_))
        ));
    }

    #[test]
    fn decode_strips_full_blocks_and_counts_silent_empty_strips() {
        let adapter = adapter();
        // Full non-empty block + silent empty block (frozen Q3.1 semantics:
        // empty interiors are stripped silently but counted).
        let raw = "before<think>reasoning</think>mid<think>\n </think>tail";
        let output = BackendOutput {
            text: raw.to_owned(),
            native_finish_reason: NativeFinishReason::Eos,
            usage: TokenUsage::default(),
            decode_echo: DecodeEcho::default(),
        };
        let response = adapter.decode(output).unwrap();
        assert_eq!(response.text, "beforemidtail");
        // raw_text is the normalized backend output, byte-identical (no
        // \r in this input).
        assert_eq!(response.raw_text, raw);
        assert_eq!(response.think_blocks_stripped, 2);
        assert_eq!(response.finish_reason, FinishReason::Eos);
    }

    #[test]
    fn decode_counts_stray_tags_without_eating_content() {
        let adapter = adapter();
        let response = adapter
            .decode(BackendOutput {
                text: "A</think>B".to_owned(),
                native_finish_reason: NativeFinishReason::Eos,
                usage: TokenUsage::default(),
                decode_echo: DecodeEcho::default(),
            })
            .unwrap();
        assert_eq!(response.text, "AB");
        assert_eq!(response.think_blocks_stripped, 1);
    }

    #[test]
    fn decode_raw_text_is_the_normalized_prestrip_output() {
        let adapter = adapter();
        let output = BackendOutput {
            text: "a\r\nb\rc".to_owned(),
            native_finish_reason: NativeFinishReason::MaxTokens,
            usage: TokenUsage::default(),
            decode_echo: DecodeEcho::default(),
        };
        let response = adapter.decode(output).unwrap();
        assert_eq!(response.raw_text, "a\nb\nc");
        assert_eq!(response.text, "a\nb\nc");
        assert_eq!(response.finish_reason, FinishReason::MaxTokens);
    }

    #[test]
    fn decode_passes_the_backend_echo_through_untouched() {
        let adapter = adapter();
        let echo = DecodeEcho {
            image_tokens: Some(2048),
            sampling: Some(RetrySampling::new(1_200, 64).unwrap()),
        };
        let output = BackendOutput {
            text: "raw".to_owned(),
            native_finish_reason: NativeFinishReason::RepetitionLoop,
            usage: TokenUsage {
                input_tokens: Some(9),
                output_tokens: 4,
            },
            decode_echo: echo,
        };
        let response = adapter.decode(output).unwrap();
        // The receipt equals the backend value even though a request copy
        // would have been None — proving the RBLK-1 channel direction.
        assert_eq!(response.decode_echo, echo);
        assert_eq!(response.finish_reason, FinishReason::RepetitionLoop);
        assert_eq!(
            response.usage,
            TokenUsage {
                input_tokens: Some(9),
                output_tokens: 4,
            }
        );
    }

    #[test]
    fn decode_counts_word_boundary_protection_and_is_idempotent() {
        let adapter = adapter();
        // `<thinking>` is not a think tag (word boundary).
        let output = BackendOutput {
            text: "<thinking>keep</thinking> body".to_owned(),
            native_finish_reason: NativeFinishReason::Eos,
            usage: TokenUsage::default(),
            decode_echo: DecodeEcho::default(),
        };
        let response = adapter.decode(output).unwrap();
        assert_eq!(response.text, "<thinking>keep</thinking> body");
        assert_eq!(response.think_blocks_stripped, 0);
        // Stripping again changes nothing (idempotent).
        let second = adapter
            .decode(BackendOutput {
                text: response.text.clone(),
                native_finish_reason: NativeFinishReason::Eos,
                usage: TokenUsage::default(),
                decode_echo: DecodeEcho::default(),
            })
            .unwrap();
        assert_eq!(second.text, response.text);
        assert_eq!(second.think_blocks_stripped, 0);
    }
}
