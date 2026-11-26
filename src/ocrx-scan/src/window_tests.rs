//! Window orchestration tests (T063): fake session + fake renderer prove
//! the frozen behaviors — identity-driven depth, attempt-1 batching,
//! batch-level depth-1 fallback, the degeneration ladder (2048 grid rung),
//! mechanism-A zero-commit on cancellation, and per-page artifacts.

#![cfg(test)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
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
    budgets: RefCell<Vec<(u32, u32, u32)>>,
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

    fn render_budgeted(
        &mut self,
        page: u32,
        pixel_width_budget: u32,
        pixel_height_budget: u32,
    ) -> std::io::Result<RenderedPage> {
        self.renders.borrow_mut().push(page);
        self.budgets
            .borrow_mut()
            .push((page, pixel_width_budget, pixel_height_budget));
        let mut image = RenderedPage::new(1, 1, vec![200; 3])?;
        image.generation = u64::from(page);
        Ok(image)
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
        structure_identity_slot: None,
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
        structure_identity_slot: None,
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

fn committed_attempts(root: &Path, page: u32) -> (u32, bool) {
    let file = ocrx_output::read_any(root.join(format!("page-{page:04}.checkpoint")))
        .expect("v6 checkpoint committed");
    let ocrx_output::CheckpointFile::V6(checkpoint) = file else {
        panic!("expected a v6 checkpoint");
    };
    (checkpoint.attempts, checkpoint.degenerated)
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

const TRIGGERED_TABLE: &str = "<table><tr><td>alpha beta</td><td></td></tr><tr><td>gamma delta</td><td></td></tr><tr><td>epsilon zeta</td><td></td></tr></table>";
const REREAD_TABLE: &str = "<table><tr><th>alpha beta</th><th>gamma delta</th></tr><tr><td>epsilon zeta</td><td>eta theta</td><td>iota kappa</td></tr></table>";

/// R2.6 test provider: one table region exactly covering the 1x1 page.
struct GateRegionProvider {
    regions: Vec<usize>,
}
impl crate::region_provider::RegionProvider for GateRegionProvider {
    fn regions_for_page(&mut self, _page: u32) -> Vec<crate::reread::DetectedRegion> {
        self.regions
            .iter()
            .map(|i| crate::reread::DetectedRegion {
                class: crate::reread::RegionClass::Table,
                index_in_class: *i,
                bbox: (0.0, 0.0, 1.0, 1.0),
                conf: 0.9,
            })
            .collect()
    }
}

#[test]
fn reread_gate_off_never_constructs_the_pipeline() {
    // Off path (provider None): zero reread requests — every captured
    // request is the plain OCR prompt, counters stay 0.
    let root = temp_root("gate-off");
    let session = FakeSession::with_depth(1);
    session.push(
        1,
        Program {
            text: TRIGGERED_TABLE.to_owned(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.tables_reread_total, 0);
    assert_eq!(summary.replaced_total, 0);
    let captured = session.core.captured.borrow();
    assert!(
        captured.iter().all(|r| r.prompt
            == crate::ocr::prompt::pack_for(
                &ocrx_inference_api::ModelId::new("ovisocr2").unwrap()
            )
            .prompt),
        "off path must never issue a reread request"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reread_gate_on_replaces_and_counts_through_the_production_decoder() {
    let root = temp_root("gate-on");
    let session = FakeSession::with_depth(1);
    // Program 1 = the page OCR; program 2 = the reread decode response.
    session.push(
        1,
        Program {
            text: TRIGGERED_TABLE.to_owned(),
            finish: FinishReason::Eos,
        },
    );
    session.push(
        1,
        Program {
            text: REREAD_TABLE.to_owned(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let mut provider = GateRegionProvider { regions: vec![0] };
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: Some(&mut provider as &mut dyn crate::region_provider::RegionProvider),
            vlm_armed: true,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.tables_reread_total, 1, "one table reread");
    assert_eq!(summary.replaced_total, 1, "and replaced");
    // The production decoder rode the SAME session with the FROZEN prompt.
    let captured = session.core.captured.borrow();
    let reread_request = captured
        .iter()
        .find(|r| r.prompt == crate::ocr::prompt::reread::TABLE_REREAD_V1)
        .expect("a reread request exists");
    assert!(
        reread_request
            .images
            .first()
            .is_some_and(|image| image.id == "page-1-table-reread"),
        "the decode targets the cropped region"
    );
    drop(captured);
    // page_json AND markdown carry the reread table.
    let page_json = std::fs::read_to_string(root.join("page-0001.json")).unwrap();
    assert!(page_json.contains("<th>alpha beta</th>"), "{page_json}");
    let markdown = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
    assert!(markdown.contains("<th>alpha beta</th>"), "{markdown}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reread_gate_skip_leaves_artifacts_untouched() {
    // Count mismatch (2 regions vs 1 table) => integral skip: zero reread
    // decodes, byte-identical page, counters 0.
    let root = temp_root("gate-skip");
    let session = FakeSession::with_depth(1);
    session.push(
        1,
        Program {
            text: TRIGGERED_TABLE.to_owned(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let mut provider = GateRegionProvider {
        regions: vec![0, 1],
    };
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: Some(&mut provider as &mut dyn crate::region_provider::RegionProvider),
            vlm_armed: true,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.tables_reread_total, 0);
    assert_eq!(summary.replaced_total, 0);
    let captured = session.core.captured.borrow();
    assert!(
        captured.iter().all(|r| r.prompt
            == crate::ocr::prompt::pack_for(
                &ocrx_inference_api::ModelId::new("ovisocr2").unwrap()
            )
            .prompt),
        "skipped pages never issue reread requests"
    );
    drop(captured);
    let markdown = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
    assert_eq!(markdown, TRIGGERED_TABLE);
    let _ = std::fs::remove_dir_all(root);
}

const HEALTHY_TABLE_PAGE: &str =
    "<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>";

/// R2.7 acceptance ①: armed but never triggered -> committed artifacts are
/// BYTE-IDENTICAL to the off run (and zero decode calls happened).
#[test]
fn r2_7_armed_but_not_triggered_is_byte_identical() {
    let run = |tag: &str, armed: bool| {
        let root = temp_root(tag);
        let session = FakeSession::with_depth(1);
        session.push(
            1,
            Program {
                text: HEALTHY_TABLE_PAGE.to_owned(),
                finish: FinishReason::Eos,
            },
        );
        if armed {
            session.push(
                1,
                Program {
                    text: REREAD_TABLE.to_owned(),
                    finish: FinishReason::Eos,
                },
            );
        }
        let mut renderer = FakeRenderer::new();
        let compatibility = compatibility();
        let mut provider = GateRegionProvider { regions: vec![0] };
        let summary = run_pages_with_session(
            &mut session.clone(),
            &mut renderer,
            &[1],
            &root,
            &config(&compatibility),
            &CommitRecorder::disabled(),
            &CancellationToken::new(),
            &mut |_, _| {},
            &mut crate::app::RereadArms {
                provider: if armed {
                    Some(&mut provider as &mut dyn crate::region_provider::RegionProvider)
                } else {
                    None
                },
                vlm_armed: armed,
                structure: None,
            },
        )
        .unwrap();
        assert_eq!(summary.completed_pages, 1);
        let md = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
        let json = std::fs::read_to_string(root.join("page-0001.json")).unwrap();
        let reread_requests = session
            .core
            .captured
            .borrow()
            .iter()
            .filter(|r| r.prompt == crate::ocr::prompt::reread::TABLE_REREAD_V1)
            .count();
        let _ = std::fs::remove_dir_all(&root);
        (md, json, summary.tables_reread_total, reread_requests)
    };
    let (off_md, off_json, _, _) = run("r27-off-baseline", false);
    let (on_md, on_json, reread_total, reread_requests) = run("r27-on-healthy", true);
    assert_eq!(on_md, off_md, "markdown byte-identical");
    assert_eq!(on_json, off_json, "page_json byte-identical");
    assert_eq!(reread_total, 0);
    assert_eq!(reread_requests, 0, "trigger never fired -> zero decodes");
}

/// R2.7 acceptance ②: after a replacement the committed md/json agree and
/// the diff is EXACTLY the target table (everything else byte-identical).
#[test]
fn r2_7_replacement_diff_is_exactly_the_target_table() {
    let run = |tag: &str, armed: bool| {
        let root = temp_root(tag);
        let session = FakeSession::with_depth(1);
        session.push(
            1,
            Program {
                text: TRIGGERED_TABLE.to_owned(),
                finish: FinishReason::Eos,
            },
        );
        if armed {
            session.push(
                1,
                Program {
                    text: REREAD_TABLE.to_owned(),
                    finish: FinishReason::Eos,
                },
            );
        }
        let mut renderer = FakeRenderer::new();
        let compatibility = compatibility();
        let mut provider = GateRegionProvider { regions: vec![0] };
        let _summary = run_pages_with_session(
            &mut session.clone(),
            &mut renderer,
            &[1],
            &root,
            &config(&compatibility),
            &CommitRecorder::disabled(),
            &CancellationToken::new(),
            &mut |_, _| {},
            &mut crate::app::RereadArms {
                provider: if armed {
                    Some(&mut provider as &mut dyn crate::region_provider::RegionProvider)
                } else {
                    None
                },
                vlm_armed: armed,
                structure: None,
            },
        )
        .unwrap();
        let md = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
        let json = std::fs::read_to_string(root.join("page-0001.json")).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        (md, json)
    };
    let (off_md, off_json) = run("r27-diff-off", false);
    let (on_md, on_json) = run("r27-diff-on", true);
    // md: exactly the table fragment changed.
    assert_eq!(
        on_md,
        off_md.replace(TRIGGERED_TABLE, REREAD_TABLE),
        "markdown diff = the target table only"
    );
    // json: parse both and compare structure-wise.
    let off_doc = ocrx_core::PageDocument::from_json(&off_json).unwrap();
    let on_doc = ocrx_core::PageDocument::from_json(&on_json).unwrap();
    assert_eq!(off_doc.schema_version, on_doc.schema_version);
    assert_eq!(off_doc.page_number, on_doc.page_number);
    assert_eq!(off_doc.termination, on_doc.termination);
    assert_eq!(off_doc.warnings, on_doc.warnings);
    assert_eq!(off_doc.blocks.len(), on_doc.blocks.len());
    for (off_block, on_block) in off_doc.blocks.iter().zip(on_doc.blocks.iter()) {
        match (off_block, on_block) {
            (
                ocrx_core::PageBlock::HtmlTable { html: off_html },
                ocrx_core::PageBlock::HtmlTable { html: on_html },
            ) => {
                assert_eq!(off_html, TRIGGERED_TABLE);
                assert_eq!(on_html, REREAD_TABLE);
            }
            _ => assert_eq!(off_block, on_block, "non-table blocks byte-identical"),
        }
    }
}

/// R2.7 acceptance ④: the checkpoint output hash corresponds to the NEW
/// markdown (the gate runs before commit, so the hash rides the spliced
/// bytes).
#[test]
fn r2_7_checkpoint_hash_matches_the_replaced_markdown() {
    let root = temp_root("r27-hash");
    let session = FakeSession::with_depth(1);
    session.push(
        1,
        Program {
            text: TRIGGERED_TABLE.to_owned(),
            finish: FinishReason::Eos,
        },
    );
    session.push(
        1,
        Program {
            text: REREAD_TABLE.to_owned(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let mut provider = GateRegionProvider { regions: vec![0] };
    run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: Some(&mut provider as &mut dyn crate::region_provider::RegionProvider),
            vlm_armed: true,
            structure: None,
        },
    )
    .unwrap();
    let markdown = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
    assert!(markdown.contains("<th>alpha beta</th>"));
    let checkpoint = ocrx_output::read_any(&root.join("page-0001.checkpoint")).unwrap();
    let expected = ocrx_output::artifact_sha256(&root.join("page-0001.md"), &root).unwrap();
    let v6 = match checkpoint {
        ocrx_output::CheckpointFile::V6(v6) => v6,
        #[allow(unreachable_patterns)]
        _ => panic!("expected a v6 checkpoint"),
    };
    assert_eq!(v6.output_sha256, expected, "hash rides the splice");
    let _ = std::fs::remove_dir_all(root);
}

// --- review 2026-11-18 BLK-4/5 window-level regressions ---------------------

#[test]
fn blk5_ovisocr3_retries_keep_the_plain_render_never_budgeted() {
    // BLK-5: the render budget comes from the CURRENT rung's config —
    // ovisocr3's rungs carry render_budget None, so every retry render is
    // the plain one main froze (never a 4096² supersample).
    let root = temp_root("blk5-ovisocr3-plain");
    let session = FakeSession::with_depth(1);
    for _ in 0..4 {
        session.push(
            1,
            Program {
                text: "循环片段".repeat(9),
                finish: FinishReason::RepetitionLoop,
            },
        );
    }
    session.push(
        1,
        Program {
            text: "ovisocr3 rescue".into(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config_with_ladder(&compatibility, "ovisocr3"),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    // The ovisocr3 ladder walked its 3 rungs (attempts 2..=4) — every one
    // a PLAIN render.
    assert!(
        renderer.budgets.borrow().is_empty(),
        "ovisocr3 retries must never call render_budgeted"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn blk4_bad_page_reread_always_triggers_but_identity_still_guards() {
    // BLK-4 end-to-end: a retry-committed page (attempts >= 2) whose table
    // is HEALTHY (no F-gate) still triggers the reread — and the identity
    // gate still blocks a foreign reread (恒触发不替换).
    let root = temp_root("blk4-bad-page-trigger");
    let session = FakeSession::with_depth(1);
    // Attempt 1 degenerates; attempt 2 rescues — the page commits with
    // attempts = 2 (the bad-page path).
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
            // HEALTHY table: no F1-F5 trigger on the normal path.
            text: "<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>"
                .to_owned(),
            finish: FinishReason::Eos,
        },
    );
    // The reread decode returns FOREIGN content (identity blocks it).
    session.push(
        1,
        Program {
            text: "<table><tr><td>foreign one</td><td>foreign two</td></tr></table>".to_owned(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let mut provider = GateRegionProvider { regions: vec![0] };
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: Some(&mut provider as &mut dyn crate::region_provider::RegionProvider),
            vlm_armed: true,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.tables_reread_total, 1, "bad page always triggers");
    assert_eq!(summary.replaced_total, 0, "identity guard still holds");
    let captured = session.core.captured.borrow();
    assert!(
        captured
            .iter()
            .any(|r| r.prompt == crate::ocr::prompt::reread::TABLE_REREAD_V1),
        "the reread decode happened despite the healthy table"
    );
    drop(captured);
    let markdown = std::fs::read_to_string(root.join("page-0001.md")).unwrap();
    assert!(
        !markdown.contains("foreign"),
        "the identity guard kept the original table"
    );
    let _ = std::fs::remove_dir_all(root);
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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

/// Reads the committed v6 checkpoint's attempts value for `page`.

#[test]
fn m2_rung_renders_budgeted_4096_and_escapes_with_the_2048_grid() {
    // B-M2.1 (D-17): the M2 rung 2048@4096² sits after penalty, before the
    // column split; its decode rides the 2048 grid. B-M2.3 diag: the
    // spike's raised raster applies to the WHOLE retry walk (attempts >= 2).
    let root = temp_root("m2-rung-escape");
    let session = FakeSession::with_depth(1);
    for _ in 0..3 {
        session.push(
            1,
            Program {
                text: "循环片段".repeat(9),
                finish: FinishReason::RepetitionLoop,
            },
        );
    }
    session.push(
        1,
        Program {
            text: "m2 rescue".into(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.degenerated_pages, 0, "escaped on the M2 rung");
    // B-M2.3 diag: the spike's raised raster applies to the WHOLE retry
    // walk — every retry render (attempts 2..=4) is budgeted.
    assert_eq!(
        renderer.budgets.borrow().as_slice(),
        &[(1, 4096, 4096), (1, 4096, 4096), (1, 4096, 4096)]
    );
    // The ladder walked in order: plain → 2048-grid → penalty → M2(2048).
    let captured = session.core.captured.borrow();
    assert_eq!(captured.len(), 4);
    // Walk-wide retry grid (B-M2.3 diag): every RETRY decode rides 2048;
    // attempt 1 stays plain 1024.
    assert_eq!(captured[0].image_tokens_override, None);
    for request in &captured[1..] {
        assert_eq!(request.image_tokens_override, Some(2048));
    }
    assert!(captured[1].retry_sampling.is_none());
    assert!(captured[2].retry_sampling.is_some());
    assert!(captured[3].retry_sampling.is_none());
    drop(captured);
    assert_eq!(committed_attempts(&root, 1), (4, false));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn m2_rung_stays_unused_when_the_penalty_rung_escapes() {
    // Escaping at the penalty rung: retry renders stay budgeted, the M2
    // rung's decode never fires.
    let root = temp_root("m2-rung-untouched");
    let session = FakeSession::with_depth(1);
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
            text: "循环片段".repeat(9),
            finish: FinishReason::RepetitionLoop,
        },
    );
    session.push(
        1,
        Program {
            text: "penalty rescue".into(),
            finish: FinishReason::Eos,
        },
    );
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.degenerated_pages, 0);
    // Raised raster on the two retry attempts; only ONE 2048-grid decode
    // (rung 1) — the M2 rung's decode never fires on an attempt-3 escape.
    assert_eq!(
        renderer.budgets.borrow().as_slice(),
        &[(1, 4096, 4096), (1, 4096, 4096)]
    );
    let captured = session.core.captured.borrow();
    assert_eq!(captured.len(), 3);
    assert!(captured[2].retry_sampling.is_some());
    assert_eq!(
        captured
            .iter()
            .filter(|r| r.image_tokens_override == Some(2048))
            .count(),
        2,
        "walk-wide retry grid: retry decodes (2,3) at 2048, attempt 1 plain"
    );
    drop(captured);
    assert_eq!(committed_attempts(&root, 1), (3, false));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exhausted_five_attempt_ladder_commits_the_m2_terminal_attempt_marked() {
    // 5-attempt exhaustion: plain → 2048@2880² → penalty → 2048@4096² →
    // column split; the terminal (5th) attempt commits with attempts = 5
    // and the degenerated marker (D-17 cap 4→5).
    let root = temp_root("m2-rung-exhaust");
    let session = FakeSession::with_depth(1);
    for _ in 0..5 {
        session.push(
            1,
            Program {
                text: "循环片段".repeat(9),
                finish: FinishReason::RepetitionLoop,
            },
        );
    }
    let mut renderer = FakeRenderer::new();
    let compatibility = compatibility();
    let summary = run_pages_with_session(
        &mut session.clone(),
        &mut renderer,
        &[1],
        &root,
        &config(&compatibility),
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut |_, _| {},
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.completed_pages, 1);
    assert_eq!(summary.degenerated_pages, 1, "exhaustion commits marked");
    // Every retry render (attempts 2..=5) budgeted.
    assert_eq!(renderer.budgets.borrow().as_slice(), &[(1, 4096, 4096); 4]);
    assert_eq!(committed_attempts(&root, 1), (5, true));
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        trigger_state_json: String::new(),
        png: None,
        markdown: body.to_owned(),
        page_json: format!(r#"{{"schema_version":1,"page_number":{page},"blocks":[]}}"#),
        truncated: false,
        degenerated: false,
        warnings: Vec::new(),
        think_blocks_stripped: 0,
        raw_output: String::new(),
        reread_tables: 0,
        reread_replaced: 0,
        reread_detail_json: String::new(),
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
    let mut reread_tables = 0_u64;
    let mut reread_replaced = 0_u64;
    let mut warnings = std::collections::BTreeSet::new();
    let mut diagnostics = Vec::new();
    let result = commit_terminal(
        &terminal,
        &root,
        &compatibility(),
        None,
        &CommitRecorder::disabled(),
        &CancellationToken::new(),
        &mut completed,
        &mut failed,
        &mut truncated,
        &mut degenerated,
        &mut think_blocks_stripped,
        &mut reread_tables,
        &mut reread_replaced,
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
    // the ladder (5 attempts: plain + the 4 frozen ovisocr2 rungs incl. the
    // M2 rung) and commit as DEGENERATED — never as a clean success that
    // --resume would silently reuse.
    let root = temp_root("empty-ladder");
    let mut session = FakeSession::with_depth(1);
    for _ in 0..5 {
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert!(
        root.join("page-0001.checkpoint").is_file() || root.join("page-0001.md").is_file(),
        "committed page must write artifacts"
    );
    // 5 submits: attempt-1 + the four frozen rungs.
    assert_eq!(session.core.single_calls.get(), 5);
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
    for _ in 0..5 {
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    assert_eq!(summary.degenerated_pages, 1);
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&boundary)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 5, "one record per attempt");
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
        // Audit metadata: the recorder's run id reaches every record.
        assert!(
            record["run_id"].as_str().unwrap_or_default().len() >= 6,
            "run_id must be populated: {}",
            record["run_id"]
        );
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
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
        &mut crate::app::RereadArms {
            provider: None,
            vlm_armed: false,
            structure: None,
        },
    )
    .unwrap();
    let records: Vec<serde_json::Value> = std::fs::read_to_string(&boundary)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 4);
    // Attempt 2 = 2048-grid rung; attempt 3 = sampling chain; attempt 4 =
    // the M2 budget rung — walk-wide (B-M2.3): every retry carries the
    // 2048 grid override.
    assert_eq!(records[1]["image_tokens_override"], 2048);
    assert_eq!(records[1]["retry_sampling"], serde_json::Value::Null);
    assert_eq!(records[2]["retry_sampling"], serde_json::json!([1200, 64]));
    assert_eq!(records[2]["image_tokens_override"], 2048);
    assert_eq!(records[3]["retry_sampling"], serde_json::Value::Null);
    assert_eq!(records[3]["image_tokens_override"], 2048);
    let _ = std::fs::remove_dir_all(root);
}
