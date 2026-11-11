//! Window orchestration tests (T063): fake session + fake renderer prove
//! the frozen behaviors — identity-driven depth, attempt-1 batching,
//! batch-level depth-1 fallback, the degeneration ladder (2048 grid rung),
//! mechanism-A zero-commit on cancellation, and per-page artifacts.

#![cfg(test)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use ocrx_inference_api::{
    DecodeEcho, FinishReason, InferenceError, InferenceIdentity, InferenceSession,
    RuntimeTelemetry, StepProfile, TaskRequest, TaskResponse, TokenUsage,
};
use ocrx_output::{PdfIdentity, ScanCompatibility, ScanRoute};
use ocrx_primitives::{CancellationToken, GenerationProgress};

use crate::state::RenderedPage;
use crate::test_support::identity;
use crate::window::{run_pages_with_session, PageRenderer, WindowConfig};

/// One programmed per-page response.
#[derive(Debug, Clone)]
struct Program {
    text: String,
    finish: FinishReason,
}

type Shared<T> = Rc<RefCell<T>>;

/// A programmable session core shared by handle clones (tests pass the
/// same core to the loop and inspect it afterwards).
#[derive(Default)]
struct Core {
    queue: RefCell<Vec<(u32, Program)>>,
    captured: RefCell<Vec<TaskRequest>>,
    window_calls: std::cell::Cell<usize>,
    single_calls: std::cell::Cell<usize>,
    fail_next_window: std::cell::Cell<bool>,
}

impl Core {
    fn push(&self, page_key: u32, program: Program) {
        self.queue.borrow_mut().push((page_key, program));
    }
}

/// A session handle over the shared core.
#[derive(Clone, Default)]
struct FakeSession {
    identity: Option<InferenceIdentity>,
    core: Rc<Core>,
}

impl FakeSession {
    fn with_depth(depth: usize) -> Self {
        let mut id = identity();
        id.batch_depth = depth;
        Self {
            identity: Some(id),
            core: Rc::new(Core::default()),
        }
    }

    fn push(&self, page_key: u32, program: Program) {
        self.core.push(page_key, program);
    }

    fn fail_next_window(&self) {
        self.core.fail_next_window.set(true);
    }

    fn submit_impl(
        &self,
        request: TaskRequest,
        cancellation: &CancellationToken,
    ) -> Result<TaskResponse, InferenceError> {
        self.core.single_calls.set(self.core.single_calls.get() + 1);
        self.core.captured.borrow_mut().push(request);
        if cancellation.is_cancelled() {
            return Err(InferenceError::Cancelled);
        }
        // Fake page identity: the request's image id is `page-N`.
        let slot = self
            .core
            .captured
            .borrow()
            .last()
            .and_then(|request| request.images.first())
            .map(|image| image.id.clone())
            .unwrap_or_default()
            .trim_start_matches("page-")
            .parse::<u32>()
            .unwrap_or(0);
        let index = self
            .core
            .queue
            .borrow()
            .iter()
            .position(|(key, _)| *key == slot)
            .ok_or_else(|| InferenceError::BackendExecution("no program".into()))?;
        let (_, program) = self.core.queue.borrow_mut().remove(index);
        Ok(TaskResponse {
            text: program.text.clone(),
            raw_text: program.text,
            finish_reason: program.finish,
            usage: TokenUsage::default(),
            think_blocks_stripped: 0,
            decode_echo: DecodeEcho::default(),
            warnings: Vec::new(),
        })
    }
}

impl InferenceSession for FakeSession {
    fn identity(&self) -> &InferenceIdentity {
        self.identity.as_ref().expect("identity set")
    }

    fn telemetry(&self) -> RuntimeTelemetry {
        RuntimeTelemetry::default()
    }

    fn step_profile(&self) -> Option<StepProfile> {
        None
    }

    fn submit(
        &mut self,
        request: TaskRequest,
        cancellation: &CancellationToken,
        _progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<TaskResponse, InferenceError> {
        self.submit_impl(request, cancellation)
    }

    fn submit_window(
        &mut self,
        requests: Vec<TaskRequest>,
        cancellation: &CancellationToken,
        _progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<TaskResponse, InferenceError>> {
        self.core
            .window_calls
            .set(self.core.window_calls.get() + 1);
        if self.core.fail_next_window.replace(false) {
            return std::iter::repeat_n(
                Err(InferenceError::WindowFailed("ctx".into())),
                requests.len(),
            )
            .collect();
        }
        let mut results = Vec::with_capacity(requests.len());
        for request in requests {
            results.push(self.submit_impl(request, cancellation));
        }
        results
    }
}

/// A fake renderer producing a fixed 1x1 gray page, failing configured pages.
#[derive(Debug, Default)]
struct FakeRenderer {
    renders: RefCell<Vec<u32>>,
    fail_pages: RefCell<Vec<u32>>,
}

impl FakeRenderer {
    fn new() -> Self {
        Self::default()
    }

    fn fail_page(&mut self, page: u32) {
        self.fail_pages.borrow_mut().push(page);
    }
}

impl PageRenderer for FakeRenderer {
    fn preflight(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
        self.renders.borrow_mut().push(page);
        if self.fail_pages.borrow().contains(&page) {
            return Err(std::io::Error::other("render boom"));
        }
        let mut image = RenderedPage::new(1, 1, vec![200; 3])?;
        image.generation = u64::from(page);
        Ok(image)
    }
}

fn compatibility() -> ScanCompatibility {
    ScanCompatibility::native_or_empty(
        "hash",
        ScanRoute::Ocr,
        PdfIdentity {
            pdf_mode: "auto".to_owned(),
            quality_tier: "l2".to_owned(),
            classifier_version: "pdf-route-v1".to_owned(),
            extractor_version: "pdf-text-v1".to_owned(),
            platform: "windows".to_owned(),
        },
    )
}

fn config(compatibility: &ScanCompatibility) -> WindowConfig<'_> {
    WindowConfig {
        compatibility,
        resume: false,
        keep_pages: false,
        column_split_enabled: true,
    }
}

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ocrx-window-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn eos(page: u32) -> Program {
    Program {
        text: format!("body {page}"),
        finish: FinishReason::Eos,
    }
}

#[test]
fn window_fills_use_the_identity_batch_depth_without_backfill() {
    let root = temp_root("depth");
    let session = FakeSession::with_depth(2);
    for page in 1..=3_u32 {
        session.push(page, eos(page));
    }
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1, 2, 3],
        &root,
        &config(&compatibility),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    // Depth 2 over [1,2,3] → one full window + one single-page window.
    assert_eq!(session.core.window_calls.get(), 1);
    // 3 submits total: 2 inside the window batch + 1 serial for page 3.
    assert_eq!(session.core.single_calls.get(), 3);
    assert_eq!(summary.completed_pages, 3);
    assert_eq!(summary.failed_pages, 0);
    for page in 1..=3_u32 {
        assert!(root.join(format!("page-{page:04}.md")).is_file());
        assert!(root.join(format!("page-{page:04}.checkpoint")).is_file());
        assert!(
            root.join("images")
                .join(format!("page-{page:04}.png"))
                .is_file()
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn render_failures_record_diagnostics_and_continue() {
    let root = temp_root("render-fail");
    let session = FakeSession::with_depth(1);
    session.push(1, eos(1));
    session.push(2, eos(2));
    let mut renderer = FakeRenderer::new();
    renderer.fail_page(1);
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1, 2],
        &root,
        &config(&compatibility),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.failed_pages, 1);
    assert!(summary
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.stage == "render"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cancelled_windows_commit_nothing_mechanism_a() {
    let root = temp_root("cancel");
    let session = FakeSession::with_depth(1);
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let cancellation = CancellationToken::new();
    cancellation.request_cancel();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &cancellation,
        &mut |_, _| {},
    )
    .unwrap();
    // Zero commits: no checkpoint, no artifacts.
    assert_eq!(summary.completed_pages, 0);
    assert!(summary.cancelled);
    assert!(!root.join("page-0001.checkpoint").exists());
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        0,
        "cancelled window leaves zero commits"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn degenerated_pages_retry_through_the_frozen_ladder() {
    let root = temp_root("ladder");
    let session = FakeSession::with_depth(1);
    // Page 1: degenerates on attempt 1, escapes with the 2048-grid rung.
    session.push(
        1,
        Program {
            text: "循环片段".repeat(9),
            finish: FinishReason::RepetitionLoop,
        },
    );
    session.push(
        1,
        Program {
            text: "recovered body".into(),
            finish: FinishReason::Eos,
        },
    );
    session.push(2, eos(2));
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1, 2],
        &root,
        &config(&compatibility),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 2);
    assert_eq!(summary.degenerated_pages, 0, "recovered on rung 1");
    // The retry request carried the 2048 grid override (frozen rung 1).
    let captured = session.core.captured.borrow();
    let retry_request = captured
        .iter()
        .find(|request| request.image_tokens_override == Some(2048))
        .expect("the 2048 grid rung must ride the retry request");
    assert_eq!(retry_request.generation.max_output_tokens, 16_384);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn window_batch_failure_falls_back_to_depth_one() {
    let root = temp_root("fallback");
    let session = FakeSession::with_depth(2);
    for page in 1..=2_u32 {
        session.push(page, eos(page));
    }
    session.fail_next_window();
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1, 2],
        &root,
        &config(&compatibility),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 2, "depth-1 rerun recovers both");
    assert_eq!(session.core.window_calls.get(), 1);
    assert_eq!(session.core.single_calls.get(), 2);
    let _ = std::fs::remove_dir_all(root);
}
