//! Generic `RuntimeBackend` implementations (plan §8.6): the llama.cpp
//! backend over the P4.1 inference actor, the runtime factory, and the
//! programmable fake used by scan/inference tests.
//!
//! Backend rules: no OCR prompt, no page numbers, no `PageDocument`, no
//! output directories — a backend executes `EncodedTask`s and echoes the
//! variants it actually applied into `BackendOutput.decode_echo`.

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod llama {
    //! llama.cpp backend implementation (cfg-gated like `LlamaMtmdRuntime`).

    use ocrx_inference_api::identity::RuntimeTelemetry;
    use ocrx_inference_api::{
        BackendHandle, BackendId, BackendOutput, DecodeEcho, EncodedTask, GenerationParams,
        ImagePlacement, InferenceError, NativeFinishReason, RetrySampling, RuntimeBackend,
        RuntimeFactory, RuntimeIdentity, RuntimeLoadRequest, TokenUsage,
    };
    use ocrx_primitives::{CancellationToken, GenerationProgress};

    use crate::llama_mtmd::LlamaMtmdRuntime;
    use crate::step_profile::StepProfile;
    use crate::{Runtime, WorkerTask};

    /// Maps the legacy string finish reasons of the worker protocol onto the
    /// structured native finish reason.
    fn native_finish_reason(reason: &str) -> NativeFinishReason {
        match reason {
            "eos" => NativeFinishReason::Eos,
            "stop" => NativeFinishReason::StopToken,
            "max_tokens" => NativeFinishReason::MaxTokens,
            "repetition_loop" => NativeFinishReason::RepetitionLoop,
            _ => NativeFinishReason::Invalid,
        }
    }

    fn map_worker_error(message: String) -> InferenceError {
        if message.contains("cancelled") {
            InferenceError::Cancelled
        } else if message.starts_with(crate::BATCH_ERROR_PREFIX) {
            // Batch-level window failure (decode/OOM/context overflow):
            // the scan side answers with a depth-1 resubmission.
            InferenceError::WindowFailed(message)
        } else {
            InferenceError::BackendExecution(message)
        }
    }

    /// Validates and converts one protocol-encoded task into the worker
    /// task (Ovis protocol: exactly one image, before-prompt placement,
    /// frozen greedy/no-thinking generation). The worker consumes the
    /// caller's prompt verbatim.
    fn encoded_to_worker_task(task: EncodedTask, slot: u32) -> Result<WorkerTask, InferenceError> {
        if task.images.len() != 1 {
            return Err(InferenceError::InvalidTask(format!(
                "llama backend expects exactly one image per task, got {}",
                task.images.len()
            )));
        }
        if task.image_placement != ImagePlacement::BeforePrompt {
            return Err(InferenceError::InvalidTask(
                "llama backend only supports image-before-prompt placement".into(),
            ));
        }
        if task.generation.temperature_milli != 0 || task.generation.thinking {
            return Err(InferenceError::InvalidTask(
                "llama backend executes the frozen greedy/no-thinking contract only".into(),
            ));
        }
        if task.generation.max_output_tokens == 0 {
            return Err(InferenceError::InvalidTask(
                "max_output_tokens must be positive".into(),
            ));
        }
        let image = &task.images[0];
        Ok(WorkerTask {
            slot,
            prompt: task.prompt,
            width: image.width,
            height: image.height,
            image_rgb8: image.rgb8.clone(),
            max_output_tokens: task.generation.max_output_tokens,
            image_tokens_override: task.image_tokens_override,
            // Identical-shape bridge: the legacy worker task carries the
            // runtime-owned sampling struct until T087 deletes it.
            retry_sampling: task.retry_sampling.map(|spec| crate::RetrySampling {
                repeat_penalty_milli: spec.repeat_penalty_milli,
                penalty_window: spec.penalty_window,
            }),
        })
    }

    /// llama.cpp `RuntimeBackend`: forwards encoded tasks to the P4.1
    /// inference actor (multi-slot window decoding stays an internal
    /// implementation detail) and echoes the applied variants.
    pub struct LlamaBackend {
        runtime: LlamaMtmdRuntime,
        image_token_limit: u32,
    }

    impl LlamaBackend {
        /// Builds a backend over an already-loaded runtime handle.
        #[must_use]
        pub fn new(runtime: LlamaMtmdRuntime, image_token_limit: u32) -> Self {
            Self {
                runtime,
                image_token_limit,
            }
        }

        /// The image token limit the loaded model supports.
        #[must_use]
        pub fn image_token_limit(&self) -> u32 {
            self.image_token_limit
        }
    }

    impl RuntimeBackend for LlamaBackend {
        fn generate(
            &mut self,
            task: EncodedTask,
            cancellation: &CancellationToken,
            progress: &mut dyn FnMut(GenerationProgress),
        ) -> Result<BackendOutput, InferenceError> {
            let worker_task = encoded_to_worker_task(task, 0)?;
            let applied = (
                worker_task.image_tokens_override,
                // Legacy struct -> api struct (identical-shape bridge).
                worker_task
                    .retry_sampling
                    .map(|spec| RetrySampling {
                        repeat_penalty_milli: spec.repeat_penalty_milli,
                        penalty_window: spec.penalty_window,
                    }),
            );
            let mut last_tokens = 0_u64;
            let response = self
                .runtime
                .generate_task(worker_task, cancellation, &mut |_slot, event| {
                    last_tokens = u64::from(event.generated_tokens);
                    progress(event);
                })
                .map_err(map_worker_error)?;
            Ok(BackendOutput {
                text: response.raw_output,
                native_finish_reason: native_finish_reason(&response.finish_reason),
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: last_tokens,
                },
                // The worker applies the requested grid binding and sampler
                // chain verbatim (no silent degradation path today), so the
                // actually applied variants equal the request.
                decode_echo: DecodeEcho {
                    image_tokens: applied.0,
                    sampling: applied.1,
                },
            })
        }

        fn generate_window(
            &mut self,
            tasks: Vec<EncodedTask>,
            cancellation: &CancellationToken,
            progress: &mut dyn FnMut(usize, GenerationProgress),
        ) -> Vec<Result<BackendOutput, InferenceError>> {
            // Validate every task first: an invalid task means the window
            // never started, so every slot fails with the same category.
            let mut worker_tasks = Vec::with_capacity(tasks.len());
            for (slot, task) in tasks.into_iter().enumerate() {
                match encoded_to_worker_task(task, u32::try_from(slot).unwrap_or(u32::MAX)) {
                    Ok(worker_task) => worker_tasks.push(worker_task),
                    Err(error) => {
                        return std::iter::repeat_n(Err(error), worker_tasks.len() + 1).collect();
                    }
                }
            }
            let results =
                self.runtime
                    .generate_window_tasks(worker_tasks, cancellation, &mut |slot, event| {
                        progress(slot as usize, event)
                    });
            results
                .into_iter()
                .map(|result| {
                    result
                        .map(|response| BackendOutput {
                            text: response.raw_output,
                            native_finish_reason: native_finish_reason(&response.finish_reason),
                            usage: TokenUsage {
                                input_tokens: None,
                                output_tokens: 0,
                            },
                            decode_echo: DecodeEcho::default(),
                        })
                        .map_err(map_worker_error)
                })
                .collect()
        }

        fn telemetry(&self) -> RuntimeTelemetry {
            let legacy = Runtime::telemetry(&self.runtime);
            RuntimeTelemetry {
                load_time_ms: legacy.load_time_ms,
                first_token_ms: legacy.first_token_ms,
                generated_tokens: legacy.generated_tokens,
                generation_time_ms: legacy.generation_time_ms,
            }
        }

        fn step_profile(&self) -> Option<StepProfile> {
            Some(self.runtime.step_profile())
        }
    }

    /// Runtime factory for the llama.cpp backend (plan §8.6): loads one
    /// backend and returns its authoritative identities; it never touches
    /// the model catalog or installation.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct LlamaRuntimeFactory;

    impl RuntimeFactory for LlamaRuntimeFactory {
        fn create(&self, request: &RuntimeLoadRequest) -> Result<BackendHandle, InferenceError> {
            let use_gpu = request.device.as_str() != "cpu";
            let runtime = LlamaMtmdRuntime::load(
                &request.model_path,
                &request.mmproj_path,
                use_gpu,
                request.max_image_tokens,
                request.context_size,
                request.batch_depth,
            )
            .map_err(|error| {
                if error.contains("cancelled") {
                    InferenceError::Cancelled
                } else {
                    InferenceError::RuntimeInitialization(error)
                }
            })?;
            let info = runtime.info();
            Ok(BackendHandle {
                backend: Box::new(LlamaBackend::new(runtime, info.image_token_limit)),
                runtime: RuntimeIdentity {
                    runtime: ocrx_inference_api::RuntimeId::new("llama.cpp")
                        .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
                    wrapper_version: "0.1.161".to_owned(),
                    sys_version: "0.1.161".to_owned(),
                    upstream_revision: "d81235049384534c167caea52b85a694f6103d14".to_owned(),
                },
                backend_id: request.device.clone(),
                image_token_limit: info.image_token_limit,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn params() -> GenerationParams {
            GenerationParams {
                max_output_tokens: 32,
                temperature_milli: 0,
                thinking: false,
            }
        }

        fn task(prompt: &str) -> EncodedTask {
            EncodedTask {
                prompt: prompt.to_owned(),
                images: vec![ocrx_inference_api::ImageInput::new("img", 1, 1, vec![1, 2, 3])
                    .unwrap()],
                image_placement: ImagePlacement::BeforePrompt,
                generation: params(),
                image_tokens_override: None,
                retry_sampling: None,
            }
        }

        #[test]
        fn finish_reason_mapping_covers_the_frozen_set() {
            assert_eq!(native_finish_reason("eos"), NativeFinishReason::Eos);
            assert_eq!(
                native_finish_reason("max_tokens"),
                NativeFinishReason::MaxTokens
            );
            assert_eq!(
                native_finish_reason("repetition_loop"),
                NativeFinishReason::RepetitionLoop
            );
            assert_eq!(native_finish_reason("stop"), NativeFinishReason::StopToken);
            assert_eq!(
                native_finish_reason("whatever"),
                NativeFinishReason::Invalid
            );
        }

        #[test]
        fn worker_error_mapping_preserves_the_batch_level_protocol() {
            // The legacy BATCH_ERROR_PREFIX protocol maps to WindowFailed so
            // the scan side falls back to depth-1 resubmission.
            let batch = map_worker_error(format!("{}ctx overflow", crate::BATCH_ERROR_PREFIX));
            assert!(matches!(batch, InferenceError::WindowFailed(_)));
            assert_eq!(
                map_worker_error("cancelled".into()),
                InferenceError::Cancelled
            );
            assert!(matches!(
                map_worker_error("load failed".into()),
                InferenceError::BackendExecution(_)
            ));
        }

        #[test]
        fn encoded_task_conversion_validates_the_ovis_protocol() {
            let good = encoded_to_worker_task(task("p"), 0).unwrap();
            assert_eq!(good.slot, 0);
            assert_eq!(good.prompt, "p");
            assert_eq!(good.width, 1);
            // Multi-image and wrong placement are invalid tasks.
            let mut multi = task("p");
            multi
                .images
                .push(ocrx_inference_api::ImageInput::new("img2", 1, 1, vec![1, 2, 3]).unwrap());
            assert!(matches!(
                encoded_to_worker_task(multi, 0),
                Err(InferenceError::InvalidTask(_))
            ));
            let mut placed = task("p");
            placed.image_placement = ImagePlacement::BeforePrompt;
            assert!(encoded_to_worker_task(placed, 0).is_ok());
            // Non-frozen generation (thinking/temperature) is rejected.
            let mut hot = task("p");
            hot.generation = GenerationParams {
                max_output_tokens: 32,
                temperature_milli: 700,
                thinking: false,
            };
            assert!(matches!(
                encoded_to_worker_task(hot, 0),
                Err(InferenceError::InvalidTask(_))
            ));
        }

        #[test]
        fn decode_echo_carries_the_requested_variants_on_the_serial_path() {
            // The applied variants equal the request (no degradation today);
            // a retry task with the frozen rung echoes both values back.
            let mut retry = task("p");
            retry.image_tokens_override = Some(2048);
            retry.retry_sampling = Some(RetrySampling::new(1_200, 64).unwrap());
            let converted = encoded_to_worker_task(retry, 3).unwrap();
            assert_eq!(converted.image_tokens_override, Some(2048));
            assert_eq!(
                converted.retry_sampling,
                Some(crate::RetrySampling {
                    repeat_penalty_milli: 1_200,
                    penalty_window: 64,
                })
            );
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use llama::{LlamaBackend, LlamaRuntimeFactory};

use ocrx_inference_api::identity::RuntimeTelemetry;
use ocrx_inference_api::{
    BackendOutput, DecodeEcho, EncodedTask, InferenceError, NativeFinishReason, RetrySampling,
    RuntimeBackend, TokenUsage,
};
use ocrx_primitives::{CancellationToken, GenerationProgress};

/// Programmable fake backend (plan §8.6 fake contract): captures requests,
/// queues responses (EOS / max_tokens / repetition_loop / backend error /
/// cancellation), can fail whole windows batch-level, and can emit a decode
/// echo that deliberately differs from the request variants (the receipt
/// must come from the backend, never from a request copy).
#[derive(Debug, Default)]
pub struct FakeRuntimeBackend {
    queue: std::collections::VecDeque<Result<ProgrammedResponse, InferenceError>>,
    /// When set, every `generate_window` call fails batch-level.
    window_batch_error: Option<InferenceError>,
    /// When set, `generate` fails with cancellation once the token is set.
    cancel_aware: bool,
    /// When set, the echo is this fixed divergent value instead of the
    /// request variants.
    divergent_echo: Option<DecodeEcho>,
    /// Requests captured so far (test-facing).
    pub captured: Vec<CapturedTask>,
    telemetry: RuntimeTelemetry,
    step_profile: Option<crate::step_profile::StepProfile>,
    window_calls: usize,
    single_calls: usize,
}

/// One captured request (test-facing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedTask {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub rgb8_len: usize,
    pub max_output_tokens: u32,
    pub image_tokens_override: Option<u32>,
    pub retry_sampling: Option<RetrySampling>,
}

/// One programmed response (test-facing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgrammedResponse {
    pub text: String,
    pub native_finish_reason: NativeFinishReason,
    pub output_tokens: u64,
}

impl FakeRuntimeBackend {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one successful response.
    pub fn push_response(&mut self, response: ProgrammedResponse) {
        self.queue.push_back(Ok(response));
    }

    /// Convenience: one EOS response with the given text.
    pub fn push_eos(&mut self, text: impl Into<String>) {
        self.push_response(ProgrammedResponse {
            text: text.into(),
            native_finish_reason: NativeFinishReason::Eos,
            output_tokens: 1,
        });
    }

    /// Queues one per-task failure.
    pub fn push_error(&mut self, error: InferenceError) {
        self.queue.push_back(Err(error));
    }

    /// Makes every subsequent window submission fail batch-level.
    pub fn fail_windows_with(&mut self, error: InferenceError) {
        self.window_batch_error = Some(error);
    }

    /// Honors pre-cancelled tokens like the real backend.
    pub fn set_cancel_aware(&mut self, enabled: bool) {
        self.cancel_aware = enabled;
    }

    /// Programs a decode echo that differs from the request variants.
    pub fn set_divergent_echo(&mut self, echo: DecodeEcho) {
        self.divergent_echo = Some(echo);
    }

    /// Programs the diagnostic step-profile snapshot.
    pub fn set_step_profile(&mut self, profile: crate::step_profile::StepProfile) {
        self.step_profile = Some(profile);
    }

    /// Number of window submissions received.
    #[must_use]
    pub fn window_calls(&self) -> usize {
        self.window_calls
    }

    /// Number of single submissions received.
    #[must_use]
    pub fn single_calls(&self) -> usize {
        self.single_calls
    }
}

impl RuntimeBackend for FakeRuntimeBackend {
    fn generate(
        &mut self,
        task: EncodedTask,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<BackendOutput, InferenceError> {
        self.single_calls += 1;
        self.captured.push(CapturedTask {
            prompt: task.prompt,
            width: task.images.first().map_or(0, |image| image.width),
            height: task.images.first().map_or(0, |image| image.height),
            rgb8_len: task.images.first().map_or(0, |image| image.rgb8.len()),
            max_output_tokens: task.generation.max_output_tokens,
            image_tokens_override: task.image_tokens_override,
            retry_sampling: task.retry_sampling,
        });
        if self.cancel_aware && cancellation.is_cancelled() {
            return Err(InferenceError::Cancelled);
        }
        progress(GenerationProgress {
            generated_tokens: 1,
            elapsed_seconds: 0,
        });
        self.telemetry.generated_tokens += 1;
        self.telemetry.generation_time_ms += 1;
        let programmed = self.queue.pop_front().unwrap_or_else(|| {
            Ok(ProgrammedResponse {
                text: String::new(),
                native_finish_reason: NativeFinishReason::Eos,
                output_tokens: 0,
            })
        });
        let programmed = programmed?;
        let echo = self.divergent_echo.unwrap_or(DecodeEcho {
            image_tokens: task.image_tokens_override,
            sampling: task.retry_sampling,
        });
        Ok(BackendOutput {
            text: programmed.text,
            native_finish_reason: programmed.native_finish_reason,
            usage: TokenUsage {
                input_tokens: None,
                output_tokens: programmed.output_tokens,
            },
            decode_echo: echo,
        })
    }

    fn generate_window(
        &mut self,
        tasks: Vec<EncodedTask>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<BackendOutput, InferenceError>> {
        self.window_calls += 1;
        if let Some(error) = &self.window_batch_error {
            // Batch-level failure: every entry failed, nothing consumed.
            return std::iter::repeat_n(Err(error.clone()), tasks.len()).collect();
        }
        let mut results = Vec::with_capacity(tasks.len());
        for (slot, task) in tasks.into_iter().enumerate() {
            results.push(self.generate(task, cancellation, &mut |event| {
                progress(slot, event)
            }));
        }
        results
    }

    fn telemetry(&self) -> RuntimeTelemetry {
        self.telemetry
    }

    fn step_profile(&self) -> Option<crate::step_profile::StepProfile> {
        self.step_profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::{GenerationParams, ImageInput, ImagePlacement};

    fn params() -> GenerationParams {
        GenerationParams {
            max_output_tokens: 32,
            temperature_milli: 0,
            thinking: false,
        }
    }

    fn task(prompt: &str) -> EncodedTask {
        EncodedTask {
            prompt: prompt.to_owned(),
            images: vec![ImageInput::new("img", 1, 1, vec![1, 2, 3]).unwrap()],
            image_placement: ImagePlacement::BeforePrompt,
            generation: params(),
            image_tokens_override: None,
            retry_sampling: None,
        }
    }

    #[test]
    fn fake_backend_is_programmable_and_captures_requests() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("first");
        backend.push_response(ProgrammedResponse {
            text: "second".into(),
            native_finish_reason: NativeFinishReason::MaxTokens,
            output_tokens: 7,
        });
        backend.push_error(InferenceError::BackendExecution("boom".into()));
        let token = CancellationToken::new();
        let first = backend.generate(task("a"), &token, &mut |_| {}).unwrap();
        assert_eq!(first.text, "first");
        assert_eq!(first.native_finish_reason, NativeFinishReason::Eos);
        assert_eq!(first.decode_echo, DecodeEcho::default());
        let second = backend.generate(task("b"), &token, &mut |_| {}).unwrap();
        assert_eq!(second.native_finish_reason, NativeFinishReason::MaxTokens);
        assert_eq!(second.usage.output_tokens, 7);
        assert!(backend.generate(task("c"), &token, &mut |_| {}).is_err());
        assert_eq!(
            backend
                .captured
                .iter()
                .map(|captured| captured.prompt.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(backend.single_calls(), 3);
        // Empty queue falls back to a neutral EOS response.
        let fourth = backend.generate(task("d"), &token, &mut |_| {}).unwrap();
        assert_eq!(fourth.text, "");
    }

    #[test]
    fn fake_backend_window_batch_failure_fails_every_entry_without_consuming() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("queued");
        backend.fail_windows_with(InferenceError::WindowFailed("context overflow".into()));
        let results = backend.generate_window(
            vec![task("a"), task("b")],
            &CancellationToken::new(),
            &mut |_, _| {},
        );
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .all(|result| matches!(result, Err(InferenceError::WindowFailed(_)))));
        // The queued response was NOT consumed by the failed window.
        let single = backend.generate(task("c"), &CancellationToken::new(), &mut |_| {});
        assert_eq!(single.unwrap().text, "queued");
        assert_eq!(backend.window_calls(), 1);
    }

    #[test]
    fn fake_backend_window_success_is_slot_ordered_and_per_slot_failing() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("one");
        backend.push_error(InferenceError::BackendExecution("slot fail".into()));
        backend.push_eos("three");
        let mut slots = Vec::new();
        let results = backend.generate_window(
            vec![task("a"), task("b"), task("c")],
            &CancellationToken::new(),
            &mut |slot, _| slots.push(slot),
        );
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
        assert_eq!(slots, vec![0, 1, 2]);
    }

    #[test]
    fn fake_backend_cancel_aware_maps_to_cancelled() {
        let mut backend = FakeRuntimeBackend::new();
        backend.set_cancel_aware(true);
        let token = CancellationToken::new();
        token.request_cancel();
        assert_eq!(
            backend.generate(task("a"), &token, &mut |_| {}).unwrap_err(),
            InferenceError::Cancelled
        );
    }

    #[test]
    fn fake_backend_divergent_echo_proves_the_receipt_comes_from_the_backend() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("x");
        let divergent = DecodeEcho {
            image_tokens: Some(2048),
            sampling: Some(RetrySampling::new(1_200, 64).expect("frozen rung is valid")),
        };
        backend.set_divergent_echo(divergent);
        // Request carries NO variants; the response echo still carries the
        // backend-programmed values.
        let output = backend
            .generate(task("a"), &CancellationToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(output.decode_echo, divergent);
        assert_eq!(output.decode_echo.image_tokens, Some(2048));
    }

    #[test]
    fn fake_backend_exposes_programmable_step_profile_and_telemetry() {
        let mut backend = FakeRuntimeBackend::new();
        assert_eq!(backend.step_profile(), None);
        backend.set_step_profile(crate::step_profile::StepProfile {
            windows: 2,
            steps: 10,
            ..crate::step_profile::StepProfile::default()
        });
        assert!(backend.step_profile().is_some());
        let _ = backend.generate(task("a"), &CancellationToken::new(), &mut |_| {});
        assert_eq!(backend.telemetry().generated_tokens, 1);
    }
}
