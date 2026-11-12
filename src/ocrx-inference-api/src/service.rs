//! Inference service and session traits (plan §5.5).
//!
//! `prepare` returns a fully bound `InferenceSession`; the fixed order is
//! resolve catalog → select profile → verify binding → select device →
//! check assets → install if needed → create runtime → GPU fallback → create
//! adapter/session. A session is created once per scan invocation (shared
//! across documents in the orchestrated path) and handles many requests,
//! none of which carry PDF context.

use ocrx_primitives::{CancellationToken, GenerationProgress};

use crate::asset::{InstallInteraction, InstallPolicy};
use crate::error::InferenceError;
use crate::id::{BackendId, ModelId};
use crate::identity::{InferenceIdentity, RuntimeTelemetry, StaticInferenceIdentity, StepProfile};
use crate::task::{TaskRequest, TaskResponse};

/// One prepare request (plan §5.5). No batch-depth knob exists: the platform
/// tiers the depth by device (§2.2 — no user dial).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRequest {
    pub model: ModelId,
    /// `auto` or a concrete profile id.
    pub requested_profile: String,
    /// `auto` or a concrete backend id.
    pub requested_device: String,
    /// Scan-level image token budget (frozen default 1024).
    pub max_image_tokens: u32,
    /// Single-stream context size (frozen default 20480).
    pub context_size: u32,
    pub install: InstallPolicy,
}

/// One successfully prepared inference unit.
pub struct PreparedInference {
    pub session: Box<dyn InferenceSession>,
    /// The backend requested before any fallback (diagnostics).
    pub requested_backend: BackendId,
    /// Includes the GPU-initialization-failure fallback-to-CPU warning.
    pub warnings: Vec<String>,
}

/// Top-level inference service.
pub trait InferenceService {
    /// Prepares one session (loads at most once per call; any failure leaves
    /// no "seemingly usable" session behind).
    ///
    /// # Errors
    ///
    /// Returns the structured category for whichever fixed step failed.
    fn prepare(
        &mut self,
        request: PrepareRequest,
        interaction: &mut dyn InstallInteraction,
    ) -> Result<PreparedInference, InferenceError>;

    /// Static decode-generation query (D-15): a compile-time constant of the
    /// platform, callable before prepare, never touching assets or loading.
    /// Same source as the static registry.
    fn decode_generation(&self) -> u32;

    /// Read-only static identity query (RBLK-4, the pre-comparison data
    /// source): resolves the catalog (`auto` → concrete profile via the
    /// frozen memory recommendation), reads built-in manifest per-asset
    /// metadata and the static registry's runtime/adapter binding values.
    ///
    /// Hard constraints: no asset-file hash verification, no installation, no
    /// runtime creation, no device initialization. Resource probing happens
    /// only for the `auto` memory recommendation. The result never contains
    /// the actual backend/batch depth (final-comparison-only fields) or
    /// `display` (report-only, RBLK-7).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::UnsupportedModel`]/[`InferenceError::UnsupportedProfile`]
    /// for unknown requests.
    fn static_identity(
        &self,
        model: &ModelId,
        requested_profile: &str,
    ) -> Result<StaticInferenceIdentity, InferenceError>;
}

/// One bound model+adapter+runtime session.
pub trait InferenceSession {
    /// Full execution identity including the actual backend and batch depth.
    fn identity(&self) -> &InferenceIdentity;

    /// Cumulative telemetry (window aggregation included); the scan reads
    /// the current total after each page and differences per document.
    fn telemetry(&self) -> RuntimeTelemetry;

    /// Optional diagnostic snapshot (T1, `OCRX_STEP_PROFILE`-gated):
    /// cumulative structured counts matching the baseline `step-profile.json`
    /// golden; `None` when the channel is disabled. The type and `delta_from`
    /// semantics live in the public API (RBLK-3) — the scan must never
    /// reference the concrete runtime type.
    fn step_profile(&self) -> Option<StepProfile>;

    /// Submits one task; blocks until the response or an error.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Cancelled`] on cooperative cancellation.
    fn submit(
        &mut self,
        request: TaskRequest,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<TaskResponse, InferenceError>;

    /// Optional window entry (P4.2 §⑤ minimal interface): submits a batch of
    /// requests. The default implementation submits sequentially in order —
    /// numerically equivalent to single-stream decoding; real backends
    /// override with multi-slot continuous batching. Progress callbacks carry
    /// the window slot index (never a page number); results are request-
    /// ordered. A batch-level failure fills every entry with
    /// [`InferenceError::WindowFailed`]; the scan resubmits the window at
    /// depth 1.
    fn submit_window(
        &mut self,
        requests: Vec<TaskRequest>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<TaskResponse, InferenceError>> {
        let mut results = Vec::with_capacity(requests.len());
        for (slot, request) in requests.into_iter().enumerate() {
            results.push(self.submit(request, cancellation, &mut |event| progress(slot, event)));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::ProfileId;
    use crate::identity::{AdapterIdentity, ModelDisplayMeta, ModelIdentity, RuntimeIdentity};
    use crate::task::{DecodeEcho, FinishReason, ImageInput, TokenUsage};
    use std::cell::RefCell;

    fn identity(batch_depth: usize) -> InferenceIdentity {
        InferenceIdentity {
            model: ModelIdentity::new(
                ModelId::new("ovisocr2").unwrap(),
                ProfileId::new("compact").unwrap(),
                "rev",
                vec![],
            )
            .unwrap(),
            runtime: RuntimeIdentity {
                runtime: crate::RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".into(),
                sys_version: "0.1.161".into(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".into(),
            },
            adapter: AdapterIdentity {
                adapter: crate::AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".into(),
            },
            backend: BackendId::new("cpu").unwrap(),
            decode_generation: 2,
            batch_depth,
            display: ModelDisplayMeta {
                display_name: "OvisOCR2".into(),
                family: "Ovis".into(),
                quantization: "Q4_K_M".into(),
            },
        }
    }

    /// Minimal fake session: one controlled identity, per-submit queue,
    /// cumulative telemetry, optional step profile (env-gated semantics are
    /// the implementation's business — the fake just mirrors the contract).
    struct FakeSession {
        id: InferenceIdentity,
        queue: RefCell<Vec<Result<String, InferenceError>>>,
        submits: RefCell<usize>,
        telemetry: RefCell<RuntimeTelemetry>,
        step_profile: Option<StepProfile>,
    }

    impl FakeSession {
        fn with_queue(id: InferenceIdentity, texts: &[&str]) -> Self {
            Self {
                id,
                queue: RefCell::new(texts.iter().map(|t| Ok((*t).to_owned())).collect()),
                submits: RefCell::new(0),
                telemetry: RefCell::new(RuntimeTelemetry::default()),
                step_profile: None,
            }
        }
    }

    impl InferenceSession for FakeSession {
        fn identity(&self) -> &InferenceIdentity {
            &self.id
        }

        fn telemetry(&self) -> RuntimeTelemetry {
            *self.telemetry.borrow()
        }

        fn step_profile(&self) -> Option<StepProfile> {
            self.step_profile
        }

        fn submit(
            &mut self,
            _request: TaskRequest,
            cancellation: &CancellationToken,
            progress: &mut dyn FnMut(GenerationProgress),
        ) -> Result<TaskResponse, InferenceError> {
            if cancellation.is_cancelled() {
                return Err(InferenceError::Cancelled);
            }
            progress(GenerationProgress {
                generated_tokens: 1,
                elapsed_seconds: 0,
            });
            *self.submits.borrow_mut() += 1;
            self.telemetry.borrow_mut().generated_tokens += 1;
            let next = self.queue.borrow_mut().pop();
            match next {
                Some(Ok(text)) => Ok(TaskResponse {
                    text,
                    raw_text: String::new(),
                    finish_reason: FinishReason::Eos,
                    usage: TokenUsage::default(),
                    think_blocks_stripped: 0,
                    decode_echo: DecodeEcho::default(),
                    warnings: Vec::new(),
                }),
                Some(Err(error)) => Err(error),
                None => Ok(TaskResponse {
                    text: "queued-fallback".into(),
                    raw_text: String::new(),
                    finish_reason: FinishReason::Eos,
                    usage: TokenUsage::default(),
                    think_blocks_stripped: 0,
                    decode_echo: DecodeEcho::default(),
                    warnings: Vec::new(),
                }),
            }
        }
    }

    fn request() -> TaskRequest {
        TaskRequest::new(
            "p",
            vec![ImageInput::new("img", 1, 1, vec![1, 2, 3]).unwrap()],
            crate::task::GenerationParams {
                max_output_tokens: 8,
                temperature_milli: 0,
                thinking: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn fake_session_serves_multiple_submits_with_cumulative_telemetry() {
        let mut session = FakeSession::with_queue(identity(8), &["a", "b"]);
        let token = CancellationToken::new();
        let first = session.submit(request(), &token, &mut |_| {}).unwrap();
        let second = session.submit(request(), &token, &mut |_| {}).unwrap();
        assert_eq!(first.text, "b");
        assert_eq!(second.text, "a");
        assert_eq!(*session.submits.borrow(), 2);
        assert_eq!(
            session.telemetry().generated_tokens,
            2,
            "no double counting"
        );
        assert_eq!(session.step_profile(), None);
        assert_eq!(session.identity().batch_depth, 8);
        assert_eq!(session.identity().decode_generation, 2);
    }

    #[test]
    fn default_submit_window_runs_sequentially_with_slot_progress() {
        let mut session = FakeSession::with_queue(identity(8), &["x", "y", "z"]);
        let mut slots = Vec::new();
        let results = session.submit_window(
            vec![request(), request(), request()],
            &CancellationToken::new(),
            &mut |slot, _| slots.push(slot),
        );
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(slots, vec![0, 1, 2]);
    }

    #[test]
    fn cancelled_submit_maps_to_the_cancelled_category() {
        let mut session = FakeSession::with_queue(identity(8), &[]);
        let token = CancellationToken::new();
        token.request_cancel();
        assert_eq!(
            session.submit(request(), &token, &mut |_| {}).unwrap_err(),
            InferenceError::Cancelled
        );
    }
}
