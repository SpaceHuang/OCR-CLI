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

use ocrx_output::commit_profile::CommitRecorder;

use crate::state::{PageArtifactResult, RenderedPage};
use crate::test_support::identity;
use crate::window::{
    PageRenderer, TerminalPage, WindowConfig, commit_terminal, run_pages_with_session,
};

/// One programmed per-page response.
#[derive(Debug, Clone)]
pub(crate) struct Program {
    pub(crate) text: String,
    pub(crate) finish: FinishReason,
}

type Shared<T> = Rc<RefCell<T>>;

/// A programmable session core shared by handle clones (tests pass the
/// same core to the loop and inspect it afterwards).
#[derive(Default)]
pub(crate) struct Core {
    queue: RefCell<Vec<(u32, Program)>>,
    captured: RefCell<Vec<TaskRequest>>,
    window_calls: std::cell::Cell<usize>,
    single_calls: std::cell::Cell<usize>,
    fail_next_window: std::cell::Cell<bool>,
    /// Cumulative generated tokens (T1 telemetry accumulation).
    generated_tokens: std::cell::Cell<u64>,
    /// Programmed step-profile snapshot (T1 channel).
    step_profile: std::cell::Cell<Option<ocrx_inference_api::StepProfile>>,
}

impl Core {
    pub(crate) fn push(&self, page_key: u32, program: Program) {
        self.queue.borrow_mut().push((page_key, program));
    }
}

/// A session handle over the shared core.
#[derive(Clone, Default)]
pub(crate) struct FakeSession {
    pub(crate) identity: Option<InferenceIdentity>,
    pub(crate) core: Rc<Core>,
}

impl FakeSession {
    pub(crate) fn with_depth(depth: usize) -> Self {
        let mut id = identity();
        id.batch_depth = depth;
        Self {
            identity: Some(id),
            core: Rc::new(Core::default()),
        }
    }

    pub(crate) fn push(&self, page_key: u32, program: Program) {
        self.core.push(page_key, program);
    }

    pub(crate) fn fail_next_window(&self) {
        self.core.fail_next_window.set(true);
    }

    pub(crate) fn set_step_profile(&self, profile: ocrx_inference_api::StepProfile) {
        self.core.step_profile.set(Some(profile));
    }

    fn submit_impl(
        &self,
        request: TaskRequest,
        cancellation: &CancellationToken,
    ) -> Result<TaskResponse, InferenceError> {
        self.core.single_calls.set(self.core.single_calls.get() + 1);
        self.core
            .generated_tokens
            .set(self.core.generated_tokens.get() + 1);
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
            // Column-split band requests are `page-N-band-M`; the page
            // number is the leading segment (the ladder still keys per page).
            .split('-')
            .next()
            .and_then(|head| head.parse::<u32>().ok())
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
        RuntimeTelemetry {
            generated_tokens: self.core.generated_tokens.get(),
            ..RuntimeTelemetry::default()
        }
    }

    fn step_profile(&self) -> Option<StepProfile> {
        self.core.step_profile.get()
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
        self.core.window_calls.set(self.core.window_calls.get() + 1);
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
        max_output_tokens: 16_384,
        prompt: crate::ocr::prompt::pack_for(
            &ocrx_inference_api::ModelId::new("ovisocr2").expect("built-in id is valid"),
        )
        .prompt,
        boundary_samples_out: None,
        ladder: crate::ladder_for_model(
            &ocrx_inference_api::ModelId::new("ovisocr2").expect("built-in id is valid"),
        ),
        base_image_tokens: 1024,
    }
}

fn config_with_ladder<'a>(compatibility: &'a ScanCompatibility, model: &str) -> WindowConfig<'a> {
    WindowConfig {
        ladder: crate::ladder_for_model(
            &ocrx_inference_api::ModelId::new(model).expect("built-in id is valid"),
        ),
        ..config(compatibility)
    }
}

fn config_with_boundary<'a>(
    compatibility: &'a ScanCompatibility,
    out: &std::path::Path,
) -> WindowConfig<'a> {
    WindowConfig {
        boundary_samples_out: Some(out.to_path_buf()),
        ..config(compatibility)
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

fn empty_eos() -> Program {
    // Review BLOCK fix: the immediate-EOS empty result the OvisOCR3
    // 30-page evidence produced.
    Program {
        text: String::new(),
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
        &CommitRecorder::disabled(),
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
    }
    // BLK-5: keep_pages=false drops the full-page PNGs (figure crops stay);
    // the committed bytes were real PNGs before cleanup.
    for page in 1..=3_u32 {
        assert!(
            !root
                .join("images")
                .join(format!("page-{page:04}.png"))
                .is_file()
        );
    }
    assert!(root.join("images").is_dir(), "images dir still exists");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn keep_pages_true_keeps_full_page_pngs_non_empty_and_decodable() {
    let root = temp_root("keep");
    let session = FakeSession::with_depth(1);
    for page in 1..=2_u32 {
        session.push(page, eos(page));
    }
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let mut config = config(&compatibility);
    config.keep_pages = true;
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1, 2],
        &root,
        &config,
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 2);
    // BLK-1: the committed full-page PNG is real (non-empty, decodable PNG).
    for page in 1..=2_u32 {
        let path = root.join("images").join(format!("page-{page:04}.png"));
        let bytes = std::fs::read(&path).unwrap_or_else(|error| {
            panic!("page-{page} png must exist: {error}");
        });
        assert!(!bytes.is_empty(), "page-{page} png must not be empty");
        let decoded = image::load_from_memory(&bytes)
            .unwrap_or_else(|error| panic!("page-{page} png must decode: {error}"));
        assert_eq!(
            (decoded.width(), decoded.height()),
            (1, 1),
            "render was 1x1"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn max_output_tokens_rides_every_request_blk3() {
    let root = temp_root("budget");
    let session = FakeSession::with_depth(1);
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let mut cfg = config(&compatibility);
    cfg.max_output_tokens = 128; // non-default budget
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &cfg,
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    let captured = session.core.captured.borrow();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0].generation.max_output_tokens, 128,
        "the user budget must reach the actual request, not just the checkpoint"
    );
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
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.failed_pages, 1);
    assert!(
        summary
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.stage == "render")
    );
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
        &CommitRecorder::disabled(),
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
        &CommitRecorder::disabled(),
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
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 2, "depth-1 rerun recovers both");
    assert_eq!(session.core.window_calls.get(), 1);
    assert_eq!(session.core.single_calls.get(), 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn png_encode_failure_is_page_level_and_remaining_pages_commit_rblk4() {
    // RBLK-4: a page whose PNG encode fails (rgb8 length no longer matches
    // the dims — the shape an encode failure has at the commit path) is a
    // page-level recognize failure: commit_terminal keeps returning Ok and
    // the remaining terminal pages still commit.
    let root = temp_root("png-encode-fail");
    let mut broken_image = RenderedPage::new(1, 1, vec![200; 3]).unwrap();
    broken_image.generation = 1;
    // Corrupt AFTER construction: width no longer matches the rgb8 length.
    broken_image.width = 2;
    let mut good_image = RenderedPage::new(1, 1, vec![200; 3]).unwrap();
    good_image.generation = 2;
    let artifact = |page: u32, body: &str| PageArtifactResult {
        png: None,
        markdown: body.to_owned(),
        page_json: format!(r#"{{"schema_version":1,"page_number":{page},"blocks":[]}}"#),
        truncated: false,
        degenerated: false,
        warnings: Vec::new(),
        think_blocks_stripped: 0,
        raw_output: String::new(),
    };
    let terminal = vec![
        TerminalPage::Committed {
            page: 1,
            image: broken_image,
            artifact: artifact(1, "body 1"),
            attempts: 1,
        },
        TerminalPage::Committed {
            page: 2,
            image: good_image,
            artifact: artifact(2, "body 2"),
            attempts: 1,
        },
    ];
    let mut completed = 0_u32;
    let mut failed = 0_u32;
    let mut truncated = 0_u32;
    let mut degenerated = 0_u32;
    let mut think_blocks_stripped = 0_u64;
    let mut warnings = std::collections::BTreeSet::new();
    let mut diagnostics = Vec::new();
    let result = commit_terminal(
        &terminal,
        &root,
        &compatibility(),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut completed,
        &mut failed,
        &mut truncated,
        &mut degenerated,
        &mut think_blocks_stripped,
        &mut warnings,
        &mut diagnostics,
    );
    assert!(
        result.is_ok(),
        "an encode failure must not abort the commit: {result:?}"
    );
    assert_eq!(completed, 1, "only the healthy page completed");
    assert_eq!(failed, 1, "the broken encode is a page-level failure");
    let diagnostic = diagnostics.iter().find(|d| d.page == 1).unwrap();
    assert_eq!(diagnostic.status, "failed");
    assert_eq!(diagnostic.stage, "recognize");
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("recognition failed"),
        "encode failure surfaces as the frozen recognize diagnostic"
    );
    // The remaining page still committed its full artifact set.
    assert!(root.join("page-0002.md").is_file());
    assert!(root.join("page-0002.json").is_file());
    let png = std::fs::read(root.join("images/page-0002.png")).unwrap();
    assert!(!png.is_empty(), "page 2 committed a real PNG");
    assert!(image::load_from_memory(&png).is_ok(), "page 2 png decodes");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resume_reuses_candidate_pages_even_with_pending_decode() {
    // Real-model T106 regression: the window-loop resume classification built
    // its ResumeContext with statik: None, so every OCR candidate died at the
    // phase-1 gate ("no static identity for OCR pre-compare") and a resume
    // with ANY pending page re-decoded the whole document (observed on real
    // assets: 20/20 pages re-decoded on --resume). Seed page 1 as a reusable
    // checkpoint under THIS run's compatibility; page 2 is pending — page 1
    // must be reused (no decode, checkpoint untouched), page 2 decoded.
    let root = temp_root("resume-candidate-pending");
    let compatibility = crate::test_support::ocr_compatibility();
    let mut checkpoint = crate::test_support::v6_ocr(1);
    std::fs::write(root.join("page-0001.md"), "seeded body").unwrap();
    std::fs::write(
        root.join("page-0001.json"),
        "{\"schema_version\":1,\"blocks\":[]}",
    )
    .unwrap();
    checkpoint.output_sha256 =
        ocrx_output::artifact_sha256(&root.join("page-0001.md"), &root).unwrap();
    ocrx_output::write_v6(root.join("page-0001.checkpoint"), &checkpoint).unwrap();
    let before = std::fs::metadata(root.join("page-0001.checkpoint"))
        .unwrap()
        .modified()
        .unwrap();

    // Depth 8 matches the seeded identity's batch_depth — a mismatching
    // session identity would (correctly) fail phase 2 and reprocess.
    let session = FakeSession::with_depth(8);
    // Only page 2 has a program: a page-1 decode would panic the queue
    // lookup — the submit counts below are the precise assertion.
    session.push(2, eos(2));
    let mut renderer = FakeRenderer::new();
    let mut config = config(&compatibility);
    config.resume = true;
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1, 2],
        &root,
        &config,
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 2);
    assert_eq!(
        session.core.single_calls.get() + session.core.window_calls.get(),
        1,
        "only the pending page may decode — the candidate must reuse"
    );
    let after = std::fs::metadata(root.join("page-0001.checkpoint"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        before, after,
        "a reused page's checkpoint must not be rewritten"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn empty_eos_output_enters_the_ladder_and_lands_degenerated() {
    // Review BLOCK fix: an immediate-EOS empty OCR page must retry through
    // the ladder (4 attempts: plain + 3 rungs) and commit as DEGENERATED —
    // never as a clean success that --resume would silently reuse.
    let root = temp_root("empty-ladder");
    let mut session = FakeSession::with_depth(1);
    for _ in 0..4 {
        session.push(1, empty_eos());
    }
    let mut renderer = FakeRenderer::new();
    let summary = run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility()),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(
        root.join("page-0001.checkpoint").is_file() || root.join("page-0001.md").is_file(),
        "committed page must write artifacts"
    );
    // 4 submits: attempt-1 + the three frozen rungs.
    assert_eq!(session.core.single_calls.get(), 4);
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(
        summary.degenerated_pages, 1,
        "empty output counts degenerated"
    );
    assert_eq!(summary.truncated_pages, 0);
    // The checkpoint must carry the degenerated flag (resume never reuses
    // degenerated pages) and the page json must explain why.
    let checkpoint = std::fs::read_to_string(root.join("page-0001.checkpoint")).unwrap();
    assert!(
        checkpoint.contains("degenerated=true"),
        "checkpoint: {checkpoint}"
    );
    let page_json = std::fs::read_to_string(root.join("page-0001.json")).unwrap();
    assert!(
        page_json.contains("empty_ocr_output") || page_json.contains("degenerated"),
        "page json: {page_json}"
    );
    let checkpoint = std::fs::read_to_string(root.join("page-0001.checkpoint")).unwrap();
    assert!(
        checkpoint.contains("degenerated=true"),
        "checkpoint: {checkpoint}"
    );
    let page_json = std::fs::read_to_string(root.join("page-0001.json")).unwrap();
    assert!(
        page_json.contains("empty_ocr_output") || page_json.contains("degenerated"),
        "page json: {page_json}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn empty_output_recovered_by_a_rung_commits_clean() {
    // A rung that produces real content finishes the ladder early: the
    // page commits with attempts=2 and NO degenerated flag.
    let root = temp_root("empty-recovered");
    let mut session = FakeSession::with_depth(1);
    session.push(1, empty_eos());
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let summary = run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility()),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(session.core.single_calls.get(), 2);
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.degenerated_pages, 0);
    let md = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
    assert_eq!(md, "body 1");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn boundary_samples_record_one_line_per_serial_attempt() {
    // SP-T01: with the boundary sink configured, every serial attempt
    // appends exactly one record with the frozen identity schema.
    let root = temp_root("boundary-serial");
    let boundary = root.join("boundary.jsonl");
    let mut session = FakeSession::with_depth(1);
    for _ in 0..4 {
        session.push(1, empty_eos());
    }
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &config_with_boundary(&compatibility, &boundary),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.degenerated_pages, 1);
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&boundary)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 4, "one record per attempt");
    for (index, record) in lines.iter().enumerate() {
        assert_eq!(record["source"], "scan");
        assert_eq!(record["page_number"], 1);
        assert_eq!(record["page_index"], 0);
        assert_eq!(record["attempt"], index as u64 + 1);
        assert_eq!(record["window_id"], 0);
        assert_eq!(record["slot"], 0);
        assert_eq!(record["outcome"], "empty");
        // Same rendered bytes for every attempt (fake render is stable).
        let hash = record["rgb8_sha256"].as_str().unwrap();
        assert_eq!(hash, lines[0]["rgb8_sha256"]);
        assert!(record["prompt_sha256"].as_str().unwrap().len() == 64);
        assert_eq!(record["max_output_tokens"], 16_384);
        assert_eq!(record["base_image_tokens"], 1024);
        assert_eq!(record["schema_version"], 2);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn boundary_samples_record_the_recovery_outcome() {
    let root = temp_root("boundary-recovery");
    let boundary = root.join("boundary.jsonl");
    let mut session = FakeSession::with_depth(1);
    session.push(1, empty_eos());
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &config_with_boundary(&compatibility, &boundary),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.degenerated_pages, 0);
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&boundary)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["outcome"], "empty");
    assert_eq!(lines[1]["outcome"], "nonempty");
    assert_eq!(lines[1]["attempt"], 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn boundary_disabled_by_default_writes_nothing() {
    let root = temp_root("boundary-off");
    let mut session = FakeSession::with_depth(1);
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    // No boundary.jsonl in the output root (sink unset -> zero writes).
    assert!(!root.join("boundary.jsonl").is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ovisocr3_ladder_runs_the_sampling_rung_first() {
    // Option-a mitigation: with the sampling rung leading, the FIRST retry
    // of an empty page carries the repeat-penalty chain (no grid override)
    // and the SECOND retry carries the 2048 grid — read straight from the
    // boundary records (SP-T01), which log each attempt's rung fields.
    let root = temp_root("ladder-order-ovisocr3");
    let boundary = root.join("boundary.jsonl");
    let mut session = FakeSession::with_depth(1);
    session.push(1, empty_eos());
    session.push(1, empty_eos());
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &WindowConfig {
            boundary_samples_out: Some(boundary.clone()),
            ..config_with_ladder(&compatibility, "ovisocr3")
        },
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    let records: Vec<serde_json::Value> = std::fs::read_to_string(&boundary)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 3);
    // Attempt 2 = sampling rung (penalty chain, no grid override).
    assert_eq!(records[1]["attempt"], 2);
    assert_eq!(records[1]["retry_sampling"], serde_json::json!([1200, 64]));
    assert_eq!(records[1]["image_tokens_override"], serde_json::Value::Null);
    // Attempt 3 = 2048-grid rung.
    assert_eq!(records[2]["attempt"], 3);
    assert_eq!(records[2]["image_tokens_override"], 2048);
    assert_eq!(records[2]["retry_sampling"], serde_json::Value::Null);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ovisocr2_ladder_keeps_the_frozen_rung_order() {
    // Same script under the ovisocr2 ladder: the 2048-grid rung is the
    // FIRST retry and the sampling chain the second — the mirror image of
    // the ovisocr3 order above.
    let root = temp_root("ladder-order-ovisocr2");
    let boundary = root.join("boundary.jsonl");
    let mut session = FakeSession::with_depth(1);
    session.push(1, empty_eos());
    session.push(1, empty_eos());
    session.push(1, eos(1));
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    run_pages_with_session(
        &mut session,
        &mut renderer,
        &[1],
        &root,
        &WindowConfig {
            boundary_samples_out: Some(boundary.clone()),
            ..config_with_ladder(&compatibility, "ovisocr2")
        },
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    let records: Vec<serde_json::Value> = std::fs::read_to_string(&boundary)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 3);
    // Attempt 2 = 2048-grid rung; attempt 3 = sampling chain.
    assert_eq!(records[1]["image_tokens_override"], 2048);
    assert_eq!(records[1]["retry_sampling"], serde_json::Value::Null);
    assert_eq!(records[2]["retry_sampling"], serde_json::json!([1200, 64]));
    assert_eq!(records[2]["image_tokens_override"], serde_json::Value::Null);
    let _ = std::fs::remove_dir_all(root);
}
