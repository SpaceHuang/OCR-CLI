//! Internal adapter and backend extension contract (plan §5.4).
//!
//! `ProtocolAdapter` owns how text/images enter the model protocol and how
//! generic responses decode back (including `<think>` stripping). A
//! `RuntimeBackend` executes an already-encoded task. Neither trait leaks
//! llama.cpp types, PDF concepts, or scan business parsing.

use std::path::PathBuf;

use ocrx_primitives::{CancellationToken, GenerationProgress};

use crate::error::InferenceError;
use crate::id::BackendId;
use crate::identity::{AdapterIdentity, RuntimeIdentity};
use crate::task::{DecodeEcho, GenerationParams, ImageInput, RetrySampling, TokenUsage};

/// Where the encoded images sit relative to the prompt.
///
/// Ovis places the MTMD default image marker before the prompt; the adapter
/// declares the placement and the backend inserts the runtime-native marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePlacement {
    BeforePrompt,
}

/// Finish reason as observed natively by the backend (before adapter
/// normalization). Same variant set as the generic [`crate::task::FinishReason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFinishReason {
    Eos,
    StopToken,
    MaxTokens,
    Invalid,
    RepetitionLoop,
}

impl NativeFinishReason {
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

/// A protocol-encoded task handed to the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedTask {
    pub prompt: String,
    pub images: Vec<ImageInput>,
    pub image_placement: ImagePlacement,
    pub generation: GenerationParams,
    /// Execution variants pass through with the request (§5.4): the backend
    /// binds the decode grid / sampler chain from these.
    pub image_tokens_override: Option<u32>,
    pub retry_sampling: Option<RetrySampling>,
}

/// Raw backend output before protocol cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendOutput {
    /// Model raw output (no protocol cleanup of any kind).
    pub text: String,
    pub native_finish_reason: NativeFinishReason,
    pub usage: TokenUsage,
    /// Actually effective decode-variant echo (RBLK-1): the backend fills
    /// the values it really executed — the default serial implementation
    /// echoes the request verbatim; the llama backend fills the actual grid
    /// binding and sampler chain (which may differ on fallback/degradation).
    /// The adapter must pass this through untouched into the
    /// `TaskResponse.decode_echo`.
    pub decode_echo: DecodeEcho,
}

/// Protocol adapter: encode generic requests, decode generic responses.
pub trait ProtocolAdapter {
    fn identity(&self) -> &AdapterIdentity;

    /// Encodes a validated task request.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`]/[`InferenceError::AdapterDecode`]
    /// for protocol-level encoding problems.
    fn encode(&self, request: crate::task::TaskRequest) -> Result<EncodedTask, InferenceError>;

    /// Decodes a backend output (plan §3.4 item 6):
    /// newline-normalize the raw output → strip `<think>` blocks with count →
    /// produce the dual-text `TaskResponse`; pass `decode_echo` through
    /// untouched (RBLK-1 channel: backend → BackendOutput → decode →
    /// TaskResponse).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::AdapterDecode`] for undecodable output.
    fn decode(&self, output: BackendOutput) -> Result<crate::task::TaskResponse, InferenceError>;
}

/// Runtime backend executing encoded tasks.
pub trait RuntimeBackend {
    /// Cumulative telemetry of this backend (load/first-token/tokens/time);
    /// the session proxies it for `InferenceSession::telemetry()` and must
    /// not double-accumulate.
    fn telemetry(&self) -> crate::identity::RuntimeTelemetry {
        crate::identity::RuntimeTelemetry::default()
    }

    /// Optional cumulative step-profile snapshot (T1, env-gated); `None`
    /// when the channel is disabled. The session proxies it through
    /// `InferenceSession::step_profile()`.
    fn step_profile(&self) -> Option<crate::identity::StepProfile> {
        None
    }

    /// Generates one response while polling cancellation and reporting
    /// progress at least once per elapsed second (one event when blocking
    /// image encoding starts).
    ///
    /// # Errors
    ///
    /// Returns a structured category for backend execution failure or
    /// cancellation ([`InferenceError::Cancelled`]).
    fn generate(
        &mut self,
        task: EncodedTask,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<BackendOutput, InferenceError>;

    /// Optional window method (P4.2 §⑤ minimal interface, mapping the
    /// current multi-slot window decode). Results are slot-ordered like
    /// `tasks`; each entry is that slot's output or its per-slot error. A
    /// batch-level failure (decode/OOM/context overflow) fills every entry
    /// with [`InferenceError::WindowFailed`] so the scan side falls back to
    /// depth-1 resubmission. The default implementation runs tasks
    /// sequentially through [`RuntimeBackend::generate`] — numerically
    /// identical to single-stream decoding. Real backends override it with a
    /// true batched decode and forward progress per slot index (never a page
    /// number).
    ///
    /// Determinism contract (P4 frozen): identical (inputs, batch
    /// composition, depth) reproduces byte-identical outputs; different
    /// compositions may diverge.
    fn generate_window(
        &mut self,
        tasks: Vec<EncodedTask>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<BackendOutput, InferenceError>> {
        let mut results = Vec::with_capacity(tasks.len());
        for (slot, task) in tasks.into_iter().enumerate() {
            results.push(self.generate(task, cancellation, &mut |event| {
                progress(slot, event)
            }));
        }
        results
    }
}

/// Parameters a runtime factory needs to load one backend (model paths come
/// from the models layer; the API keeps them as opaque paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLoadRequest {
    pub model_path: PathBuf,
    pub mmproj_path: PathBuf,
    pub device: BackendId,
    /// Single-stream context size (frozen default 20480); window contexts
    /// are created independently by the backend.
    pub context_size: u32,
    /// Scan-level image token budget (frozen default 1024).
    pub max_image_tokens: u32,
    /// Expected window depth for capacity planning (actual identity value).
    pub batch_depth: usize,
}

/// One successfully created backend plus its authoritative identities.
#[allow(dead_code)] // consumed by the inference composition (stage D)
pub struct BackendHandle {
    pub backend: Box<dyn RuntimeBackend>,
    /// Runtime identity (runtime id + wrapper/sys/upstream).
    pub runtime: RuntimeIdentity,
    /// Actually created backend id (may differ from the request after a
    /// fallback — handles only carry what really loaded).
    pub backend_id: BackendId,
    /// The image token limit the loaded model supports.
    pub image_token_limit: u32,
}

impl std::fmt::Debug for BackendHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendHandle")
            .field("runtime", &self.runtime)
            .field("backend_id", &self.backend_id)
            .field("image_token_limit", &self.image_token_limit)
            .finish_non_exhaustive()
    }
}

/// Creates runtime backends for a prepared model (plan §8.6: the factory
/// returns the actual backend and runtime identity; it never touches the
/// model catalog or installation).
pub trait RuntimeFactory {
    /// Loads one backend.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::RuntimeInitialization`] on load failure.
    fn create(&self, request: &RuntimeLoadRequest) -> Result<BackendHandle, InferenceError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{FinishReason, TaskRequest, TaskResponse};
    use crate::{AdapterId, ImageInput};

    fn params() -> GenerationParams {
        GenerationParams {
            max_output_tokens: 64,
            temperature_milli: 0,
            thinking: false,
        }
    }

    fn image() -> ImageInput {
        ImageInput::new("img", 1, 1, vec![1, 2, 3]).unwrap()
    }

    /// A passthrough adapter proving the RBLK-1 echo channel: decode passes
    /// BackendOutput.decode_echo through untouched.
    struct PassthroughAdapter {
        id: AdapterIdentity,
    }

    impl ProtocolAdapter for PassthroughAdapter {
        fn identity(&self) -> &AdapterIdentity {
            &self.id
        }

        fn encode(&self, request: TaskRequest) -> Result<EncodedTask, InferenceError> {
            Ok(EncodedTask {
                prompt: request.prompt,
                images: request.images,
                image_placement: ImagePlacement::BeforePrompt,
                generation: request.generation,
                image_tokens_override: request.image_tokens_override,
                retry_sampling: request.retry_sampling,
            })
        }

        fn decode(&self, output: BackendOutput) -> Result<TaskResponse, InferenceError> {
            Ok(TaskResponse {
                text: output.text.clone(),
                raw_text: output.text,
                finish_reason: FinishReason::Eos,
                usage: output.usage,
                think_blocks_stripped: 0,
                decode_echo: output.decode_echo,
                warnings: Vec::new(),
            })
        }
    }

    /// A backend whose echo deliberately differs from any request variant —
    /// the receipt must come from the backend, not a request copy.
    struct DivergentEchoBackend;

    impl RuntimeBackend for DivergentEchoBackend {
        fn generate(
            &mut self,
            _task: EncodedTask,
            _cancellation: &CancellationToken,
            _progress: &mut dyn FnMut(GenerationProgress),
        ) -> Result<BackendOutput, InferenceError> {
            Ok(BackendOutput {
                text: "raw".to_owned(),
                native_finish_reason: NativeFinishReason::Eos,
                usage: TokenUsage {
                    input_tokens: Some(3),
                    output_tokens: 1,
                },
                decode_echo: DecodeEcho {
                    image_tokens: Some(2048),
                    sampling: Some(RetrySampling {
                        repeat_penalty_milli: 1_200,
                        penalty_window: 64,
                    }),
                },
            })
        }
    }

    #[test]
    fn encoded_task_carries_placement_and_variants_without_business_types() {
        let adapter = PassthroughAdapter {
            id: AdapterIdentity {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".to_owned(),
            },
        };
        let request = TaskRequest::new("prompt", vec![image()], params())
            .unwrap()
            .with_retry_sampling(RetrySampling::new(1_200, 64).unwrap());
        let encoded = adapter.encode(request).unwrap();
        assert_eq!(encoded.image_placement, ImagePlacement::BeforePrompt);
        assert!(encoded.retry_sampling.is_some());
        let debug = format!("{encoded:?}");
        for forbidden in ["page", "pdf", "PageDocument", "bbox"] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn decode_echo_flows_from_backend_output_through_decode() {
        let adapter = PassthroughAdapter {
            id: AdapterIdentity {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".to_owned(),
            },
        };
        let mut backend = DivergentEchoBackend;
        let output = backend
            .generate(
                EncodedTask {
                    prompt: "p".into(),
                    images: vec![image()],
                    image_placement: ImagePlacement::BeforePrompt,
                    generation: params(),
                    image_tokens_override: None,
                    retry_sampling: None,
                },
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        // The backend echo (2048/penalty) differs from the request variants
        // (None) — the response must carry the backend value (RBLK-1).
        let response = adapter.decode(output).unwrap();
        assert_eq!(response.decode_echo.image_tokens, Some(2048));
        assert_eq!(
            response.decode_echo.sampling,
            Some(RetrySampling {
                repeat_penalty_milli: 1_200,
                penalty_window: 64,
            })
        );
    }

    #[test]
    fn default_generate_window_is_sequential_and_slot_indexed() {
        struct RecordingBackend {
            calls: Vec<String>,
        }
        impl RuntimeBackend for RecordingBackend {
            fn generate(
                &mut self,
                task: EncodedTask,
                _cancellation: &CancellationToken,
                progress: &mut dyn FnMut(GenerationProgress),
            ) -> Result<BackendOutput, InferenceError> {
                progress(GenerationProgress {
                    generated_tokens: 1,
                    elapsed_seconds: 0,
                });
                self.calls.push(task.prompt.clone());
                Ok(BackendOutput {
                    text: task.prompt,
                    native_finish_reason: NativeFinishReason::Eos,
                    usage: TokenUsage::default(),
                    decode_echo: DecodeEcho::default(),
                })
            }
        }
        let mut backend = RecordingBackend { calls: Vec::new() };
        let tasks = vec![
            EncodedTask {
                prompt: "a".into(),
                images: vec![],
                image_placement: ImagePlacement::BeforePrompt,
                generation: params(),
                image_tokens_override: None,
                retry_sampling: None,
            },
            EncodedTask {
                prompt: "b".into(),
                images: vec![],
                image_placement: ImagePlacement::BeforePrompt,
                generation: params(),
                image_tokens_override: None,
                retry_sampling: None,
            },
        ];
        let mut slots = Vec::new();
        let results = backend.generate_window(tasks, &CancellationToken::new(), &mut |slot, _| {
            slots.push(slot);
        });
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(backend.calls, vec!["a", "b"]);
        assert_eq!(slots, vec![0, 1], "progress carries slot indexes");
    }
}
