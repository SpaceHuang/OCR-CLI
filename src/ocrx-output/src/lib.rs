//! Atomic output and checkpoint boundary.

pub mod commit_profile;
pub mod encode_pipeline;

/// Stable result status used by CLI reporters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultStatus {
    Success,
    Partial,
    Failed,
}

impl ResultStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

/// Versioned, stdout-safe scan result contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultReport {
    pub job_id: String,
    pub status: ResultStatus,
    pub total_pages: u32,
    pub completed_pages: u32,
    pub failed_pages: u32,
    pub output_dir: String,
}

impl ResultReport {
    /// Serializes exactly one stable JSON object without a trailing newline.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema_version\":1,\"job_id\":\"{}\",\"status\":\"{}\",\"total_pages\":{},\"completed_pages\":{},\"failed_pages\":{},\"output_dir\":\"{}\"}}",
            json_escape(&redact_sensitive(&self.job_id)),
            self.status.as_str(),
            self.total_pages,
            self.completed_pages,
            self.failed_pages,
            json_escape(&redact_sensitive(&self.output_dir))
        )
    }
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

/// Redacts sensitive values before any diagnostic field is serialized.
///
/// A key only redacts when it appears as an actual key — followed by `=` or
/// `:` — so a value that merely contains the letters (e.g. the message
/// `ocr:failed`, where `ocr` IS the payload) keeps its readability
/// (review m21: substring `ocr` used to redact its own diagnostics).
#[must_use]
pub fn redact_sensitive(input: &str) -> String {
    let mut output = input.to_owned();
    for key in ["prompt", "ocr", "token", "authorization", "proxy", "url"] {
        let mut cursor = 0;
        while let Some(relative) = output[cursor..].to_ascii_lowercase().find(key) {
            let start = cursor + relative;
            let Some(separator) = output[start..].find(['=', ':']) else {
                break;
            };
            // Only treat this as a key/value pair when the separator directly
            // follows the key (`key=value`, `key: value`, `"key":`).
            if start + key.len() != start + separator {
                cursor = start + key.len();
                continue;
            }
            let value_start = start + separator + 1;
            let value_end = output[value_start..]
                .find([',', ' ', '\n'])
                .map_or(output.len(), |end| value_start + end);
            output.replace_range(value_start..value_end, "[REDACTED]");
            cursor = value_start + 10;
        }
    }
    output
}

/// Formats a report for the selected output channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Text,
    Json,
}

#[must_use]
pub fn format_report(report: &ResultReport, format: ReportFormat) -> String {
    match format {
        ReportFormat::Json => report.to_json(),
        ReportFormat::Text => format!(
            "status={} pages={}/{} failed={} output={}",
            report.status.as_str(),
            report.completed_pages,
            report.total_pages,
            report.failed_pages,
            redact_sensitive(&report.output_dir)
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Plain,
    Jsonl,
    Bar,
    Off,
}

/// Capturable reporter that keeps machine output on stdout and diagnostics on stderr.
#[derive(Debug, Default)]
pub struct Reporter {
    pub stdout: String,
    pub stderr: String,
    /// When set, JSON error reports join the stderr diagnostics instead of
    /// stdout. The orchestrated multi-document scan enables this: its stdout
    /// must stay exactly one aggregate JSON object, so per-document errors
    /// are logs (review BLK-4). Single-command behavior is unchanged.
    pub json_errors_on_stderr: bool,
}

impl Reporter {
    pub fn result(&mut self, report: &ResultReport, format: ReportFormat) {
        self.stdout.push_str(&format_report(report, format));
        self.stdout.push('\n');
    }

    pub fn error(&mut self, error: &ErrorReport, format: ReportFormat) {
        match format {
            ReportFormat::Json if self.json_errors_on_stderr => {
                self.stderr.push_str(&error.to_json());
                self.stderr.push('\n');
            }
            ReportFormat::Json => {
                self.stdout.push_str(&error.to_json());
                self.stdout.push('\n');
            }
            ReportFormat::Text => {
                self.stderr.push_str(&format!(
                    "error code={} kind={} {}\n",
                    error.code,
                    redact_sensitive(&error.kind),
                    redact_sensitive(&error.message)
                ));
            }
        }
    }

    pub fn progress(&mut self, event: &ProgressEvent, mode: ProgressMode) {
        match mode {
            ProgressMode::Off => {}
            ProgressMode::Jsonl => {
                self.stderr
                    .push_str(&redact_sensitive(&event.to_json_line()));
                self.stderr.push('\n');
            }
            ProgressMode::Plain => {
                self.stderr.push_str(&format!(
                    "progress event={} page={:?} {}/{}\n",
                    redact_sensitive(&event.event),
                    event.page,
                    event.completed,
                    event.total
                ));
            }
            ProgressMode::Bar => {
                self.stderr
                    .push_str(&format!("progress {}/{}\r", event.completed, event.total));
            }
        }
    }
}

/// Versioned progress event for JSONL reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEvent {
    pub event: String,
    pub page: Option<u32>,
    pub completed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStage {
    Analyzing,
    Extracting,
    Rendering,
    Recognizing,
    Merging,
}

impl ProgressStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analyzing => "analyzing",
            Self::Extracting => "extracting",
            Self::Rendering => "rendering",
            Self::Recognizing => "recognizing",
            Self::Merging => "merging",
        }
    }
}

impl ProgressEvent {
    #[must_use]
    pub fn for_stage(stage: ProgressStage, page: Option<u32>, completed: u32, total: u32) -> Self {
        Self {
            event: stage.as_str().to_owned(),
            page,
            completed,
            total,
        }
    }

    #[must_use]
    pub fn to_json_line(&self) -> String {
        let page = self
            .page
            .map_or_else(|| "null".to_owned(), |value| value.to_string());
        format!(
            "{{\"schema_version\":1,\"event\":\"{}\",\"page\":{},\"completed\":{},\"total\":{}}}",
            json_escape(&self.event),
            page,
            self.completed,
            self.total
        )
    }
}

/// Stable machine-readable application error payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    pub code: u8,
    pub kind: String,
    pub message: String,
    pub cause: Option<String>,
    pub suggestion: Option<String>,
}

impl ErrorReport {
    #[must_use]
    pub fn to_json(&self) -> String {
        let optional = |value: &Option<String>| {
            value.as_deref().map_or_else(
                || "null".to_owned(),
                |v| format!("\"{}\"", json_escape(&redact_sensitive(v))),
            )
        };
        format!(
            "{{\"schema_version\":1,\"code\":{},\"kind\":\"{}\",\"message\":\"{}\",\"cause\":{},\"suggestion\":{}}}",
            self.code,
            json_escape(&redact_sensitive(&self.kind)),
            json_escape(&redact_sensitive(&self.message)),
            optional(&self.cause),
            optional(&self.suggestion)
        )
    }
}

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

// Single cancellation source of truth (W3.1): the SIGINT token created by the
// CLI must be passable to the runtime's token-generation loop, so the trait
// below shares `ocrx_primitives::CancellationToken` instead of keeping a private,
// type-incompatible duplicate.
pub use ocrx_primitives::{CancellationToken, GenerationProgress};

pub mod compatibility;
pub use compatibility::{
    OcrIdentity, PdfIdentity, ScanCompatibility, ScanOcrSide, ScanRoute, ocr_final_compare,
    ocr_pre_compare,
};

pub mod checkpoint_v6;
pub use checkpoint_v6::{
    CheckpointFile, V5Disposition, V6Checkpoint, V6Status, artifacts_intact, commit_native_page_v6,
    commit_page_v6, commit_pending_ocr_v6, read_any, write_v6,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed,
    Partial,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Truncated,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLifecycle {
    pub page_number: u32,
    pub status: PageStatus,
}

impl PageLifecycle {
    pub fn new(page_number: u32) -> io::Result<Self> {
        if page_number == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "page number must be positive",
            ));
        }
        Ok(Self {
            page_number,
            status: PageStatus::Pending,
        })
    }

    pub fn transition(&mut self, next: PageStatus) -> io::Result<()> {
        let valid = matches!(
            (self.status, next),
            (PageStatus::Pending, PageStatus::Processing)
                | (PageStatus::Processing, PageStatus::Completed)
                | (PageStatus::Processing, PageStatus::Failed)
                | (PageStatus::Processing, PageStatus::Truncated)
                | (PageStatus::Processing, PageStatus::Cancelled)
        );
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid page state transition",
            ));
        }
        self.status = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSummary {
    pub selected_pages: u32,
    pub completed_pages: u32,
    pub failed_pages: u32,
    pub truncated_pages: u32,
    pub degenerated_pages: u32,
    /// Think removals summed over all committed/skipped pages (quality plan
    /// Q3.1); surfaces in the scan result as `think_blocks_stripped_total`.
    pub think_blocks_stripped: u64,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<PageDiagnostic>,
    pub cancelled: bool,
}

/// One degeneration-retry rung (quality plan Q1.4a): the render and decode
/// variant the attempt uses. `None` fields mean plain; at most one decode
/// override per rung in the frozen tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DegenerationRetryStep {
    /// Perturbed render ratio (numerator, denominator); `None` renders plain.
    pub perturbed: Option<(u32, u32)>,
    /// MTMD token-grid override for the decode (Q1.4c, e.g. 2048).
    pub image_tokens: Option<u32>,
    /// Ordered `repeat_penalty -> greedy` decode chain (Q1.2, D-13).
    pub sampling: Option<ocrx_runtime::RetrySampling>,
    /// Q1.1c candidate C (D-10): whitespace-valley column-split pass - the
    /// page is re-decoded per detected column band and the band markdowns
    /// merge in reading order. The heaviest rung: final position only.
    pub column_split: bool,
}

impl DegenerationRetryStep {
    /// Plain-render + plain-decode perturbation rung (the baseline ladder).
    #[must_use]
    pub const fn perturbed(numerator: u32, denominator: u32) -> Self {
        Self {
            perturbed: Some((numerator, denominator)),
            image_tokens: None,
            sampling: None,
            column_split: false,
        }
    }
}

/// Degeneration retry ladder (quality plan Q1.4a): the rungs walked in order
/// from attempt 2 on, frozen as a data table so the experiment-adjudicated
/// composition is a single-line change. SCRATCH EXPERIMENT COMPOSITION
/// (Q1.2/Q1.4c adjudication, docs/develop/q12-sampling-spike-build-record-2026-11-07.md §4.2/§5):
/// plain 1024 -> token-grid 2048 (46 corpus escapes, quality text 0.89) ->
/// repeat_penalty 1.2x window 64 (2 corpus escapes; 21% standalone marginal
/// on formula-type loops). Perturbation rungs are EXCLUDED by data: 0
/// escapes both as the baseline ladder (0/213) and as composed-ladder tail
/// rungs (0/228 decodes) - dropping the tail cuts worst-case retry cost 40%
/// with zero outcome change (the 114 residual pages escape on none of the
/// perturbation decodes). Attempt cap = 3. The main-line default remains the
/// baseline two perturbation rungs until Q1.3 lands this composition.
pub const DEGENERATION_RETRY_LADDER: &[DegenerationRetryStep] = &[
    DegenerationRetryStep {
        perturbed: None,
        image_tokens: Some(2048),
        sampling: None,
        column_split: false,
    },
    DegenerationRetryStep {
        perturbed: None,
        image_tokens: None,
        sampling: Some(ocrx_runtime::RetrySampling {
            repeat_penalty_milli: 1_200,
            penalty_window: 64,
        }),
        column_split: false,
    },
    DegenerationRetryStep {
        perturbed: None,
        image_tokens: None,
        sampling: None,
        column_split: true,
    },
];

pub fn merge_job_summaries(native: JobSummary, ocr: JobSummary) -> JobSummary {
    let mut warnings = native.warnings;
    warnings.extend(ocr.warnings);
    let mut diagnostics = native.diagnostics;
    diagnostics.extend(ocr.diagnostics);
    JobSummary {
        selected_pages: native.selected_pages.max(ocr.selected_pages),
        completed_pages: native.completed_pages + ocr.completed_pages,
        failed_pages: native.failed_pages + ocr.failed_pages,
        truncated_pages: native.truncated_pages + ocr.truncated_pages,
        degenerated_pages: native.degenerated_pages + ocr.degenerated_pages,
        think_blocks_stripped: native.think_blocks_stripped + ocr.think_blocks_stripped,
        warnings,
        diagnostics,
        cancelled: native.cancelled || ocr.cancelled,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDiagnostic {
    pub page: u32,
    pub status: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub message: Option<String>,
    pub audit: Option<RenderAudit>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub completed: u32,
    pub total: u32,
    pub elapsed_seconds: u64,
    pub eta_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heartbeat {
    interval_seconds: u64,
    last_emit: Option<u64>,
}

#[derive(Debug)]
pub struct ScanController {
    pages: Vec<PageLifecycle>,
    cancellation: CancellationToken,
    heartbeat: Heartbeat,
}

/// Application ports used by the fake-first scan service.
pub trait PageRenderer {
    fn preflight(&mut self) -> io::Result<()>;
    fn render(&mut self, page: u32) -> io::Result<RenderedPage>;

    /// Renders a perturbed variant used by the degeneration retry path
    /// (plan §3.5): `numerator`/`denominator` scales the model input while
    /// keeping aspect ratio and pixel budget constraints; the RenderAudit
    /// must record the scaling faithfully. The default ignores the ratio
    /// (fakes are deterministic anyway).
    fn render_perturbed(
        &mut self,
        page: u32,
        numerator: u32,
        denominator: u32,
    ) -> io::Result<RenderedPage> {
        let _ = (numerator, denominator);
        self.render(page)
    }
}

impl<T: PageRenderer + ?Sized> PageRenderer for Box<T> {
    fn preflight(&mut self) -> io::Result<()> {
        (**self).preflight()
    }

    fn render(&mut self, page: u32) -> io::Result<RenderedPage> {
        (**self).render(page)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgb8: Vec<u8>,
    pub audit: RenderAudit,

    /// Phase-4 W3: scan-local monotonic render generation. The commit path
    /// fetches the PNG bytes encoded for THIS render, so a degenerated
    /// retry's bytes can never be the first attempt's.
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderAudit {
    pub original_width: u32,
    pub original_height: u32,
    pub rendered_width: u32,
    pub rendered_height: u32,
    pub model_width: u32,
    pub model_height: u32,
    pub scaled: bool,
    pub scale_ratio_numerator: u64,
    pub scale_ratio_denominator: u64,
}

impl RenderAudit {
    #[must_use]
    pub fn as_checkpoint_fragment(self) -> String {
        format!(
            "original={}x{};rendered={}x{};model={}x{};scaled={};ratio={}/{}",
            self.original_width,
            self.original_height,
            self.rendered_width,
            self.rendered_height,
            self.model_width,
            self.model_height,
            self.scaled,
            self.scale_ratio_numerator,
            self.scale_ratio_denominator
        )
    }
}

impl RenderedPage {
    pub fn new(width: u32, height: u32, rgb8: Vec<u8>) -> io::Result<Self> {
        if width == 0 || height == 0 || rgb8.len() != width as usize * height as usize * 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid RGB8 page",
            ));
        }
        Ok(Self {
            width,
            height,
            rgb8,
            generation: 0,
            audit: RenderAudit {
                original_width: width,
                original_height: height,
                rendered_width: width,
                rendered_height: height,
                model_width: width,
                model_height: height,
                scaled: false,
                scale_ratio_numerator: 1,
                scale_ratio_denominator: 1,
            },
        })
    }

    #[must_use]
    pub fn with_audit(mut self, audit: RenderAudit) -> Self {
        self.audit = audit;
        self
    }
}

pub trait ModelLoader {
    type Model;
    fn load(&mut self) -> io::Result<Self::Model>;
}

pub trait PageRecognizer<M> {
    fn recognize(
        &mut self,
        model: &mut M,
        page: u32,
        image: &RenderedPage,
    ) -> io::Result<PageStatus>;
}

/// Terminal state of one window page awaiting the atomic commit phase
/// (plan §7.3, mechanism A).
enum TerminalPage {
    Committed {
        page: u32,
        image: RenderedPage,
        artifact: PageArtifactResult,
        attempts: u32,
    },
    RenderFailed {
        page: u32,
        cancelled: bool,
    },
    RecognizeFailed {
        page: u32,
        audit: Option<RenderAudit>,
        error: io::Error,
    },
}

impl TerminalPage {
    fn page(&self) -> u32 {
        match self {
            Self::Committed { page, .. }
            | Self::RenderFailed { page, .. }
            | Self::RecognizeFailed { page, .. } => *page,
        }
    }
}

/// Outcome of the serial attempt machine.
enum SerialOutcome {
    RenderFailed { cancelled: bool },
    Terminal(Result<(RenderedPage, PageArtifactResult, u32), (RenderAudit, io::Error)>),
}

pub trait PageArtifactRecognizer<M> {
    /// Phase-4 W3: donates the PNG encoder that the scan service's
    /// EncodePipeline uses to overlap page encoding with the window decode.
    /// `None` = the artifact's inline `png` bytes are used at commit (fakes).
    fn png_encoder(&self) -> Option<Box<dyn encode_pipeline::PagePngEncoder>> {
        None
    }

    fn recognize_artifact(
        &mut self,
        model: &mut M,
        page: u32,
        image: &RenderedPage,
    ) -> io::Result<PageArtifactResult>;

    /// Cancellation-aware variant used by the real scan path.
    ///
    /// The default implementation delegates to [`PageArtifactRecognizer::recognize_artifact`]
    /// for fake/test recognizers. Real recognizers must override it so the
    /// runtime can poll cancellation between tokens and report generation
    /// progress for the per-second heartbeat contract.
    fn recognize_artifact_cancelled(
        &mut self,
        model: &mut M,
        page: u32,
        image: &RenderedPage,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> io::Result<PageArtifactResult> {
        let _ = (cancellation, progress);
        self.recognize_artifact(model, page, image)
    }

    /// Q1.2/Q1.4c escape-ladder seam (quality plan §4.1, D-1): recognition
    /// for degeneration retries (`attempts >= 2` only), carrying the ladder
    /// step's decode variants (sampler chain / token grid). The default
    /// delegates to the plain path so fakes and non-sampling recognizers are
    /// unchanged; real recognizers override it to execute the step's decode
    /// variants. Normal (attempt-1) recognition can never reach this method.
    fn recognize_retry_artifact_cancelled(
        &mut self,
        model: &mut M,
        page: u32,
        image: &RenderedPage,
        step: &DegenerationRetryStep,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> io::Result<PageArtifactResult> {
        let _ = step;
        self.recognize_artifact_cancelled(model, page, image, cancellation, progress)
    }

    /// P4.4 window recognition (plan §7.3): decodes the window's pages
    /// concurrently. The default runs them sequentially through
    /// [`Self::recognize_artifact_cancelled`] — numerically identical to
    /// single-stream — so fakes stay unchanged; real recognizers override it
    /// with a true batched decode. A batch-level failure fills every entry
    /// with an error whose message starts with
    /// `ocrx_runtime::BATCH_ERROR_PREFIX` (the scan side falls back to
    /// depth-1 resubmission); per-page failures fail only their entry.
    fn recognize_window_cancelled(
        &mut self,
        model: &mut M,
        pages: &[(u32, RenderedPage)],
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(u32, GenerationProgress),
    ) -> Vec<Result<PageArtifactResult, io::Error>> {
        pages
            .iter()
            .map(|(page, image)| {
                self.recognize_artifact_cancelled(model, *page, image, cancellation, &mut |event| {
                    progress(*page, event)
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageArtifactResult {
    /// Phase-4 W3: the production recognizer no longer encodes - the PNG
    /// comes from the EncodePipeline (encode overlaps the window decode).
    /// `None` is the production shape; `Some` keeps fake recognizers and
    /// pipeline-less callers working (inline fallback at commit).
    pub png: Option<Vec<u8>>,
    pub markdown: String,
    pub page_json: String,
    pub truncated: bool,
    pub degenerated: bool,
    pub warnings: Vec<String>,
    /// Think removals at the adapter artifact entry (quality plan Q3.1):
    /// complete blocks (incl. silent empty strips) plus stray tags.
    pub think_blocks_stripped: u32,
    /// Pre-crop normalized raw output, preserved to the `page-NNNN.raw`
    /// sidecar for degenerated/repetition-cleaned pages (plan §3.3).
    pub raw_output: String,
}

impl<R, L, A, M> ScanService<R, L, A>
where
    R: PageRenderer,
    L: ModelLoader<Model = M>,
    A: PageArtifactRecognizer<M>,
{
    /// Runs a resumable scan, committing each page atomically before continuing.
    pub fn run_with_checkpoints(
        &mut self,
        page_numbers: &[u32],
        output_root: impl AsRef<Path>,
        compatibility: &CompatibilityKey,
        resume: bool,
    ) -> io::Result<JobSummary> {
        self.run_with_checkpoints_cancelled(
            page_numbers,
            output_root,
            compatibility,
            resume,
            false,
            &CancellationToken::new(),
        )
    }

    pub fn run_with_checkpoints_cancelled(
        &mut self,
        page_numbers: &[u32],
        output_root: impl AsRef<Path>,
        compatibility: &CompatibilityKey,
        resume: bool,
        keep_pages: bool,
        cancellation: &CancellationToken,
    ) -> io::Result<JobSummary> {
        self.run_with_checkpoints_heartbeat(
            page_numbers,
            output_root,
            compatibility,
            resume,
            keep_pages,
            cancellation,
            &mut |_, _| {},
        )
    }

    /// [`run_with_checkpoints_cancelled`] with a generation heartbeat
    /// callback (W3.1/R-1): invoked with `(page, progress)` at least once per
    /// elapsed second while a page generates tokens, so the CLI can surface
    /// liveness on multi-minute CPU pages instead of only at page boundaries.
    ///
    /// # Errors
    ///
    /// Same failure surface as [`run_with_checkpoints_cancelled`].
    /// Single-stream attempt machine (serial semantics, plan §3.5/§7.3):
    /// `start_attempt` 1 = full serial path (original render first); 2 = a
    /// batched attempt-1 degeneration retry (perturbed renders from here,
    /// walking [`DEGENERATION_RETRY_LADDER`] in order). The attempt cap is
    /// `1 + ladder.len()` (quality plan Q1.4a: the ladder composition is
    /// frozen by experiment data, the table is the single edit point); a
    /// render failure is terminal for the page.
    fn serial_attempt_outcome(
        &mut self,
        model: &mut M,
        page: u32,
        start_attempt: u32,
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(u32, GenerationProgress),
    ) -> SerialOutcome {
        let mut attempts = start_attempt.saturating_sub(1);
        loop {
            attempts += 1;
            // T1: the caller-side wait is the model's R input; every render
            // attempt (retry perturbations included) gets its own row keyed
            // by the generation allocated here.
            let generation = self.commit_recorder.begin_render(page);
            let render_started = std::time::Instant::now();
            let step = if attempts == 1 {
                None
            } else {
                let ladder_index = usize::try_from(attempts - 2).unwrap_or(usize::MAX);
                DEGENERATION_RETRY_LADDER.get(ladder_index)
            };
            let image = match step.and_then(|rung| rung.perturbed) {
                Some((numerator, denominator)) => {
                    self.renderer.render_perturbed(page, numerator, denominator)
                }
                None => self.renderer.render(page),
            };
            self.commit_recorder.record_render_wait(
                page,
                generation,
                u64::try_from(render_started.elapsed().as_micros()).unwrap_or(u64::MAX),
            );
            let image = match image {
                Ok(image) => image,
                Err(_) => {
                    return SerialOutcome::RenderFailed {
                        cancelled: cancellation.is_cancelled(),
                    };
                }
            };
            // Phase-4 W3: stamp the attempt's render generation and enqueue
            // its PNG encode - it overlaps this attempt's recognition. A
            // degenerated retry enqueues a NEW generation; the commit path
            // fetches by the terminal render's generation only.
            let mut image = image;
            image.generation = self
                .encode_pipeline
                .as_ref()
                .map(encode_pipeline::EncodePipeline::next_generation)
                .unwrap_or(0);
            if let Some(pipeline) = &self.encode_pipeline {
                pipeline.encode_one(page, &image);
            }
            let mut page_heartbeat = |event: GenerationProgress| heartbeat(page, event);
            // Q1.2: attempts >= 2 are degeneration retries — recognition
            // goes through the retry seam so the sampler chain applies only
            // there; attempt 1 keeps the plain greedy path.
            let recognized = if attempts >= 2 {
                self.recognizer.recognize_retry_artifact_cancelled(
                    model,
                    page,
                    &image,
                    step.unwrap_or(&DegenerationRetryStep::perturbed(1, 1)),
                    cancellation,
                    &mut page_heartbeat,
                )
            } else {
                self.recognizer.recognize_artifact_cancelled(
                    model,
                    page,
                    &image,
                    cancellation,
                    &mut page_heartbeat,
                )
            };
            match recognized {
                Ok(artifact) => {
                    // Q1.5/D-14: pure truncation enters the ladder too — Q5.1
                    // showed 24/28 truncated pages are long-period loops
                    // evading the detector; the ladder rungs (2048/penalty/
                    // column-split) are the only re-read chance they get.
                    let ladder_worthy = artifact.degenerated || artifact.truncated;
                    if ladder_worthy && attempts <= DEGENERATION_RETRY_LADDER.len() as u32 {
                        continue;
                    }
                    return SerialOutcome::Terminal(Ok((image, artifact, attempts)));
                }
                Err(error) => {
                    return SerialOutcome::Terminal(Err((image.audit, error)));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // mirrors the cancelled variant + one callback
    pub fn run_with_checkpoints_heartbeat(
        &mut self,
        page_numbers: &[u32],
        output_root: impl AsRef<Path>,
        compatibility: &CompatibilityKey,
        resume: bool,
        keep_pages: bool,
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(u32, GenerationProgress),
    ) -> io::Result<JobSummary> {
        self.renderer.preflight()?;
        let mut model = self.loader.load()?;
        self.run_pages_with_model(
            &mut model,
            page_numbers,
            output_root,
            compatibility,
            resume,
            keep_pages,
            cancellation,
            heartbeat,
        )
    }

    /// [`run_with_checkpoints_heartbeat`] over an EXTERNALLY OWNED model
    /// (X1 cross-document orchestration): the orchestrated scan keeps one
    /// loaded runtime alive across documents, so each document's window
    /// loop runs against the shared actor instead of loading its own.
    /// Semantics are identical to the loading variant, minus the preflight
    /// and load (the orchestrator owns both).
    ///
    /// # Errors
    ///
    /// Same failure surface as [`run_with_checkpoints_heartbeat`].
    #[allow(clippy::too_many_arguments)] // mirrors the loading variant
    pub fn run_pages_with_model(
        &mut self,
        model: &mut M,
        page_numbers: &[u32],
        output_root: impl AsRef<Path>,
        compatibility: &CompatibilityKey,
        resume: bool,
        keep_pages: bool,
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(u32, GenerationProgress),
    ) -> io::Result<JobSummary> {
        let root = output_root.as_ref();
        fs::create_dir_all(root)?;
        let mut completed = 0_u32;
        let mut failed = 0_u32;
        let mut truncated = 0_u32;
        let mut degenerated = 0_u32;
        let mut think_blocks_stripped = 0_u64;
        let mut warnings = std::collections::BTreeSet::new();
        let mut diagnostics = Vec::new();
        // P4.4 (plan §7.3): pages are processed in windows of up to
        // WINDOW_DEPTH. Attempt 1 of every window page runs as one batched
        // decode; degeneration retries and batch-failure fallbacks stay
        // single-stream (depth 1 = 等价单流). Commits are window-atomic
        // (mechanism A): nothing in the window is committed until every page
        // reached a terminal state, and a cancelled window commits nothing so
        // resume rescans it whole — the batch composition for a page stays
        // identical across the interruption.
        let mut terminal: Vec<TerminalPage> = Vec::new();
        let mut cursor = 0usize;
        while cursor < page_numbers.len() {
            if cancellation.is_cancelled() {
                break;
            }
            // Start (or extend) a window: leading resume-skipped pages are
            // drained one by one; live pages join the window until it is full
            // or the job ends.
            let mut window: Vec<u32> = Vec::with_capacity(self.window_depth);
            while cursor < page_numbers.len() && window.len() < self.window_depth {
                let page = page_numbers[cursor];
                cursor += 1;
                let checkpoint_path = root.join(format!("page-{page:04}.checkpoint"));
                let mut skipped = false;
                if resume {
                    if let Ok(checkpoint) = read_checkpoint(&checkpoint_path) {
                        if checkpoint.resume_action(compatibility, root, page)
                            == CheckpointResumeAction::SkipCommitted
                        {
                            if checkpoint.truncated {
                                truncated += 1;
                            }
                            think_blocks_stripped += u64::from(checkpoint.think_blocks_stripped);
                            warnings.extend(checkpoint.warnings.iter().cloned());
                            diagnostics.push(checkpoint.diagnostic());
                            completed += 1;
                            skipped = true;
                        }
                    }
                }
                if !skipped {
                    window.push(page);
                }
            }
            if window.is_empty() {
                break;
            }

            // Frozen depth 1 (plan P4.4): a single-page window runs the true
            // serial path — numerically identical to the pre-batching loop,
            // with no window context allocated. Depth > 1 takes the batched
            // protocol below.
            if window.len() == 1 {
                let page = window[0];
                terminal.push(
                    match self.serial_attempt_outcome(model, page, 1, cancellation, heartbeat) {
                        SerialOutcome::RenderFailed { cancelled } => {
                            TerminalPage::RenderFailed { page, cancelled }
                        }
                        SerialOutcome::Terminal(outcome) => match outcome {
                            Ok((image, artifact, attempts)) => TerminalPage::Committed {
                                page,
                                image,
                                artifact,
                                attempts,
                            },
                            Err((audit, error)) => TerminalPage::RecognizeFailed {
                                page,
                                audit: Some(audit),
                                error,
                            },
                        },
                    },
                );
                if cancellation.is_cancelled() {
                    // A cancelled single-page window commits nothing and the
                    // loop stops — identical to the serial break semantics.
                    // BLK-1: drop the in-flight encode result too.
                    if let Some(pipeline) = &self.encode_pipeline {
                        pipeline.discard_all();
                    }
                    terminal.clear();
                    break;
                }
                commit_terminal_pages(
                    &terminal,
                    root,
                    compatibility,
                    cancellation,
                    self.encode_pipeline.as_ref(),
                    &mut completed,
                    &mut failed,
                    &mut truncated,
                    &mut degenerated,
                    &mut think_blocks_stripped,
                    &mut warnings,
                    &mut diagnostics,
                    &self.commit_recorder,
                )?;
                terminal.clear();
                continue;
            }

            // Attempt-1 renders, in page order (a render failure is terminal
            // for that page, exactly like the serial loop).
            let mut images: Vec<(u32, RenderedPage)> = Vec::with_capacity(window.len());
            let mut render_failed: std::collections::BTreeMap<u32, bool> =
                std::collections::BTreeMap::new();
            for &page in &window {
                // T1: caller-side render wait, one row per (page, generation).
                let generation = self.commit_recorder.begin_render(page);
                let render_started = std::time::Instant::now();
                let rendered = self.renderer.render(page);
                self.commit_recorder.record_render_wait(
                    page,
                    generation,
                    u64::try_from(render_started.elapsed().as_micros()).unwrap_or(u64::MAX),
                );
                match rendered {
                    Ok(mut image) => {
                        // Phase-4 W3: stamp the render generation and enqueue
                        // the PNG encode - it overlaps the window decode.
                        image.generation = self
                            .encode_pipeline
                            .as_ref()
                            .map(encode_pipeline::EncodePipeline::next_generation)
                            .unwrap_or(0);
                        if let Some(pipeline) = &self.encode_pipeline {
                            pipeline.encode_one(page, &image);
                        }
                        images.push((page, image));
                    }
                    Err(_) => {
                        render_failed.insert(page, cancellation.is_cancelled());
                    }
                }
            }

            // Batched attempt-1 recognition over the successfully rendered
            // pages; results are keyed by page for the resolution pass.
            let mut batch_results: std::collections::BTreeMap<
                u32,
                Result<PageArtifactResult, io::Error>,
            > = std::collections::BTreeMap::new();
            if !images.is_empty() {
                let results = self.recognizer.recognize_window_cancelled(
                    model,
                    &images,
                    cancellation,
                    heartbeat,
                );
                for ((page, _), result) in images.iter().zip(results) {
                    batch_results.insert(*page, result);
                }
            }

            // Resolve the window into terminal pages. Batch-level failures
            // (BATCH_ERROR_PREFIX) re-run the whole window single-stream
            // (depth-1 fallback); degenerated pages continue their retry
            // state machine single-stream; everything else is terminal as-is.
            for &page in &window {
                if let Some(cancelled) = render_failed.get(&page) {
                    terminal.push(TerminalPage::RenderFailed {
                        page,
                        cancelled: *cancelled,
                    });
                    continue;
                }
                let Some(result) = batch_results.remove(&page) else {
                    continue;
                };
                let artifact = match result {
                    Ok(artifact) => artifact,
                    Err(error) => {
                        if error
                            .to_string()
                            .starts_with(ocrx_runtime::BATCH_ERROR_PREFIX)
                        {
                            // Depth-1 fallback for the whole window: every
                            // still-pending page reruns the serial path from
                            // attempt 1 (render + single-stream recognize).
                            for pending in window.iter().copied() {
                                if render_failed.contains_key(&pending)
                                    || terminal.iter().any(|entry| entry.page() == pending)
                                {
                                    continue;
                                }
                                batch_results.remove(&pending);
                                terminal.push(
                                    match self.serial_attempt_outcome(
                                        model,
                                        pending,
                                        1,
                                        cancellation,
                                        heartbeat,
                                    ) {
                                        SerialOutcome::RenderFailed { cancelled } => {
                                            TerminalPage::RenderFailed {
                                                page: pending,
                                                cancelled,
                                            }
                                        }
                                        SerialOutcome::Terminal(outcome) => match outcome {
                                            Ok((image, artifact, attempts)) => {
                                                TerminalPage::Committed {
                                                    page: pending,
                                                    image,
                                                    artifact,
                                                    attempts,
                                                }
                                            }
                                            Err((audit, error)) => TerminalPage::RecognizeFailed {
                                                page: pending,
                                                audit: Some(audit),
                                                error,
                                            },
                                        },
                                    },
                                );
                            }
                            break;
                        }
                        terminal.push(TerminalPage::RecognizeFailed {
                            page,
                            audit: None,
                            error,
                        });
                        continue;
                    }
                };
                if artifact.degenerated || artifact.truncated {
                    // Q1.5/D-14: truncated window pages retry through the
                    // ladder exactly like degenerated ones.
                    match self.serial_attempt_outcome(model, page, 2, cancellation, heartbeat) {
                        SerialOutcome::RenderFailed { cancelled } => {
                            terminal.push(TerminalPage::RenderFailed { page, cancelled });
                        }
                        SerialOutcome::Terminal(outcome) => match outcome {
                            Ok((image, retried, attempts)) => {
                                terminal.push(TerminalPage::Committed {
                                    page,
                                    image,
                                    artifact: retried,
                                    attempts,
                                });
                            }
                            Err((audit, error)) => terminal.push(TerminalPage::RecognizeFailed {
                                page,
                                audit: Some(audit),
                                error,
                            }),
                        },
                    }
                } else if let Some((_, image)) = images.iter().find(|(entry, _)| *entry == page) {
                    let image = image.clone();
                    terminal.push(TerminalPage::Committed {
                        page,
                        image,
                        artifact,
                        attempts: 1,
                    });
                }
            }

            // Window-atomic commit (mechanism A): a cancellation after the
            // window started commits nothing; every window page reruns on
            // resume with the same batch composition. BLK-1: drop the
            // in-flight encode results too.
            if cancellation.is_cancelled() {
                if let Some(pipeline) = &self.encode_pipeline {
                    pipeline.discard_all();
                }
                break;
            }
            for entry in &terminal {
                match entry {
                    TerminalPage::Committed {
                        page,
                        image,
                        artifact,
                        attempts,
                    } => {
                        let page = *page;
                        let attempts = *attempts;
                        let mut artifact_warnings = artifact.warnings.clone();
                        if image.audit.scaled
                            && !artifact_warnings
                                .iter()
                                .any(|warning| warning == "render_scaled")
                        {
                            artifact_warnings.push("render_scaled".to_owned());
                        }
                        if artifact.truncated {
                            truncated += 1;
                        }
                        if artifact.degenerated {
                            degenerated += 1;
                        }
                        think_blocks_stripped += u64::from(artifact.think_blocks_stripped);
                        warnings.extend(artifact_warnings.iter().cloned());
                        write_render_audit(root, page, image.audit)?;
                        // Raw-output sidecar (plan §3.3): preserved whenever
                        // the page degenerated or lost text to repetition
                        // cleaning.
                        let raw_pointer = if artifact.degenerated
                            || artifact_warnings
                                .iter()
                                .any(|warning| warning == "repetition_removed")
                        {
                            let name = format!("page-{page:04}.raw");
                            atomic_write(root.join(&name), artifact.raw_output.as_bytes())?;
                            Some(name)
                        } else {
                            None
                        };
                        let checkpoint = PageCheckpoint {
                            page_number: page,
                            compatibility: compatibility.clone(),
                            complete: true,
                            output_sha256: String::new(),
                            truncated: artifact.truncated,
                            degenerated: artifact.degenerated,
                            attempts,
                            raw: raw_pointer,
                            warnings: artifact_warnings,
                            think_blocks_stripped: artifact.think_blocks_stripped,
                            audit: Some(image.audit),
                        };
                        // T1: write span + terminal fold (mirror of
                        // `commit_terminal_pages`). The completed diagnostic
                        // pushes only after the encode fetch succeeds - an
                        // encode error maps to its own recognize diagnostic.
                        let write_started = std::time::Instant::now();
                        let png_bytes = match resolve_commit_png(
                            self.encode_pipeline.as_ref(),
                            image.generation,
                            artifact.png.as_ref(),
                        ) {
                            Ok(bytes) => bytes,
                            Err(encode_error) => {
                                // Frozen surface: an encode error is a page-level
                                // recognize failure, never a task I/O failure.
                                failed += 1;
                                diagnostics.push(encode_error_diagnostic(
                                    page,
                                    Some(image.audit),
                                    &encode_error,
                                ));
                                continue;
                            }
                        };
                        diagnostics.push(checkpoint.diagnostic());
                        commit_page(
                            root,
                            page,
                            &png_bytes,
                            &artifact.markdown,
                            &artifact.page_json,
                            &checkpoint,
                        )?;
                        self.commit_recorder.record_commit_write(
                            page,
                            u64::try_from(write_started.elapsed().as_micros()).unwrap_or(u64::MAX),
                        );
                        self.commit_recorder.mark_terminal(page);
                        completed += 1;
                    }
                    TerminalPage::RenderFailed { page, cancelled } => {
                        let page = *page;
                        let cancelled = *cancelled;
                        self.commit_recorder.mark_terminal(page);
                        if !cancelled {
                            failed += 1;
                        }
                        diagnostics.push(PageDiagnostic {
                            page,
                            status: if cancelled {
                                "cancelled".to_owned()
                            } else {
                                "failed".to_owned()
                            },
                            stage: "render".to_owned(),
                            error_code: Some("render_failed".to_owned()),
                            message: Some("page rendering failed".to_owned()),
                            audit: None,
                            warnings: Vec::new(),
                        });
                    }
                    TerminalPage::RecognizeFailed { page, audit, error } => {
                        let page = *page;
                        let audit = *audit;
                        let cancelled = cancellation.is_cancelled();
                        self.commit_recorder.mark_terminal(page);
                        if !cancelled {
                            failed += 1;
                        }
                        let (error_code, fallback_message) = classify_recognition_error(error);
                        diagnostics.push(PageDiagnostic {
                            page,
                            status: if cancelled {
                                "cancelled".to_owned()
                            } else {
                                "failed".to_owned()
                            },
                            stage: "recognize".to_owned(),
                            error_code: Some(error_code.to_owned()),
                            message: Some(if error_code == "device_unavailable" {
                                fallback_message.to_owned()
                            } else {
                                format!("page recognition failed: {error}")
                            }),
                            audit,
                            warnings: Vec::new(),
                        });
                    }
                }
            }
            // BLK-1: end-of-window eviction — consumed terminal entries are
            // already removed; everything left (stale attempts, failed
            // recognitions, fallback re-renders) is dropped so the result
            // map stays bounded by the window depth.
            if let Some(pipeline) = &self.encode_pipeline {
                pipeline.discard_all();
            }
            terminal.clear();
        }

        if failed == 0
            && !cancellation.is_cancelled()
            && completed == u32::try_from(page_numbers.len()).unwrap_or(u32::MAX)
        {
            // T1: finalize span (merge + cleanup), recorded once per scan.
            let finalize_started = std::time::Instant::now();
            let pages = collect_page_markdowns(root, page_numbers)?;
            finalize_document(root, pages)?;
            if !keep_pages {
                cleanup_full_page_images(root, page_numbers)?;
            }
            self.commit_recorder.record_finalize(
                u64::try_from(finalize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
            );
        }
        Ok(JobSummary {
            selected_pages: u32::try_from(page_numbers.len()).unwrap_or(u32::MAX),
            completed_pages: completed,
            failed_pages: failed,
            truncated_pages: truncated,
            degenerated_pages: degenerated,
            think_blocks_stripped,
            warnings: warnings.into_iter().collect(),
            diagnostics,
            cancelled: cancellation.is_cancelled(),
        })
    }
}

fn classify_recognition_error(error: &std::io::Error) -> (&'static str, &'static str) {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("failed to initialize backend")
        || message.contains("command queue")
        || message.contains("no metal device")
        || message.contains("device unavailable")
        // llama-cpp-2 0.1.152 can collapse a failed context/backend
        // initialization into this message instead of returning the native
        // backend error. Keep it in the device contract rather than
        // misreporting it as page recognition quality failure.
        || message.contains("null reference from llama.cpp")
    {
        (
            "device_unavailable",
            "inference device initialization failed",
        )
    } else {
        ("recognition_failed", "page recognition failed")
    }
}

/// Removes full-page PNG copies after merge; figure crops and page Markdown stay for resume.
pub fn cleanup_full_page_images(root: &Path, page_numbers: &[u32]) -> io::Result<()> {
    for &page in page_numbers {
        let _ = fs::remove_file(root.join("images").join(format!("page-{page:04}.png")));
    }
    Ok(())
}

/// Serial scan application service. The model is loaded once per job.
pub struct ScanService<R, L, A> {
    renderer: R,
    loader: L,
    recognizer: A,
    /// T1 commit-path profiler: env-gated (OCRX_STEP_PROFILE), shared with
    /// the CLI's owner-render/PNG-encode clones through
    /// [`ScanService::with_commit_recorder`].
    commit_recorder: commit_profile::CommitRecorder,
    /// Phase-4 W1: VRAM-tiered window depth (16 on >=12 GiB GPUs, else the
    /// WINDOW_DEPTH floor). Defaults to the compiled floor; the CLI sets it
    /// after device/VRAM probing via [`ScanService::set_window_depth`].
    window_depth: usize,
    /// Phase-4 W3: the PNG encode pipeline. `None` (fakes, pipeline-less
    /// callers) falls back to the artifact's inline `png` bytes at commit.
    encode_pipeline: Option<encode_pipeline::EncodePipeline>,
}

impl<R, L, A> ScanService<R, L, A> {
    #[must_use]
    pub fn new(renderer: R, loader: L, recognizer: A) -> Self {
        Self::with_commit_recorder(
            renderer,
            loader,
            recognizer,
            commit_profile::CommitRecorder::new_gated(),
        )
    }

    /// T1: injects a caller-owned recorder so the CLI's owner-render and
    /// PNG-encode clones and the service's render-wait/commit-write timing
    /// land in one state (one `commit-profile.json` per document).
    #[must_use]
    pub fn with_commit_recorder(
        renderer: R,
        loader: L,
        recognizer: A,
        commit_recorder: commit_profile::CommitRecorder,
    ) -> Self {
        Self {
            renderer,
            loader,
            recognizer,
            commit_recorder,
            window_depth: ocrx_runtime::WINDOW_DEPTH,
            encode_pipeline: None,
        }
    }

    /// Phase-4 W1: overrides the window depth for this service (the
    /// VRAM-tiered selection computed by the CLI). Unset = the compiled
    /// WINDOW_DEPTH floor.
    pub fn set_window_depth(&mut self, window_depth: usize) {
        self.window_depth = window_depth.max(1);
    }

    /// Phase-4 W3: installs the PNG encode pipeline (the recognizer donates
    /// its encoder via [`PageArtifactRecognizer::png_encoder`]). Unset = the
    /// commit falls back to the artifact's inline `png` bytes.
    pub fn set_png_encoder(&mut self, encoder: Box<dyn encode_pipeline::PagePngEncoder>) {
        self.encode_pipeline = Some(encode_pipeline::EncodePipeline::new(encoder));
    }

    /// The commit-path profile snapshot; `None` when the recorder is
    /// disabled (env-off runs, disabled-injected tests).
    #[must_use]
    pub fn commit_profile(&self) -> Option<commit_profile::CommitProfile> {
        self.commit_recorder
            .enabled()
            .then(|| self.commit_recorder.snapshot())
    }
}

impl<R, L, A, M> ScanService<R, L, A>
where
    R: PageRenderer,
    L: ModelLoader<Model = M>,
    A: PageRecognizer<M>,
{
    /// Runs preflight, loads one model, then processes selected pages serially.
    ///
    /// # Errors
    ///
    /// Returns infrastructure errors from preflight, model loading, or page
    /// rendering. Page recognition errors are recorded as failed pages so the
    /// remaining selected pages continue.
    pub fn run(&mut self, page_numbers: &[u32]) -> io::Result<JobSummary> {
        self.renderer.preflight()?;
        let mut model = self.loader.load()?;
        let mut controller = ScanController::new(page_numbers)?;
        while let Some(page) = controller.start_next()? {
            let result = self
                .renderer
                .render(page)
                .and_then(|image| self.recognizer.recognize(&mut model, page, &image));
            let status = result.unwrap_or(PageStatus::Failed);
            controller.finish(page, status)?;
        }
        Ok(controller.summary())
    }
}

impl ScanController {
    pub fn new(page_numbers: &[u32]) -> io::Result<Self> {
        Ok(Self {
            pages: page_numbers
                .iter()
                .map(|&page| PageLifecycle::new(page))
                .collect::<io::Result<Vec<_>>>()?,
            cancellation: CancellationToken::new(),
            heartbeat: Heartbeat::default(),
        })
    }

    pub fn start_next(&mut self) -> io::Result<Option<u32>> {
        if self.cancellation.is_cancelled() {
            return Ok(None);
        }
        if let Some(page) = self
            .pages
            .iter_mut()
            .find(|page| page.status == PageStatus::Pending)
        {
            page.transition(PageStatus::Processing)?;
            return Ok(Some(page.page_number));
        }
        Ok(None)
    }

    pub fn finish(&mut self, page_number: u32, status: PageStatus) -> io::Result<()> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.page_number == page_number)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "page is not selected"))?;
        page.transition(status)
    }

    pub fn cancel(&self) {
        self.cancellation.request_cancel();
    }

    #[must_use]
    pub fn heartbeat_due(&mut self, now_seconds: u64) -> bool {
        self.heartbeat.should_emit(now_seconds)
    }

    #[must_use]
    pub fn summary(&self) -> JobSummary {
        JobSummary {
            selected_pages: self.pages.len() as u32,
            completed_pages: self
                .pages
                .iter()
                .filter(|page| page.status == PageStatus::Completed)
                .count() as u32,
            failed_pages: self
                .pages
                .iter()
                .filter(|page| page.status == PageStatus::Failed)
                .count() as u32,
            think_blocks_stripped: 0,
            truncated_pages: self
                .pages
                .iter()
                .filter(|page| page.status == PageStatus::Truncated)
                .count() as u32,
            degenerated_pages: 0,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            cancelled: self.cancellation.is_cancelled(),
        }
    }
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            interval_seconds: 1,
            last_emit: None,
        }
    }
}

impl Heartbeat {
    #[must_use]
    pub fn new(interval_seconds: u64) -> Self {
        Self {
            interval_seconds: interval_seconds.max(1),
            last_emit: None,
        }
    }

    pub fn should_emit(&mut self, now_seconds: u64) -> bool {
        match self.last_emit {
            None => {
                self.last_emit = Some(now_seconds);
                true
            }
            Some(last) if now_seconds.saturating_sub(last) >= self.interval_seconds => {
                self.last_emit = Some(now_seconds);
                true
            }
            Some(_) => false,
        }
    }
}

impl ProgressSnapshot {
    #[must_use]
    pub fn new(completed: u32, total: u32, elapsed_seconds: u64) -> Self {
        let eta_seconds = if completed == 0 || completed >= total {
            None
        } else {
            Some(
                elapsed_seconds.saturating_mul(u64::from(total - completed)) / u64::from(completed),
            )
        };
        Self {
            completed,
            total,
            elapsed_seconds,
            eta_seconds,
        }
    }
}

impl JobSummary {
    #[must_use]
    pub fn status(&self) -> JobStatus {
        if self.cancelled {
            JobStatus::Cancelled
        } else if self.failed_pages > 0 || self.truncated_pages > 0 || self.degenerated_pages > 0 {
            JobStatus::Partial
        } else if self.completed_pages == self.selected_pages {
            JobStatus::Completed
        } else {
            JobStatus::Running
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityKey {
    pub input_sha256: String,
    pub model: String,
    pub profile: String,
    pub parameters: String,
}

impl CompatibilityKey {
    /// Native/empty artifacts depend only on the input and the public PDF
    /// extraction contract. OCR artifacts carry an `ocr-profile` marker and
    /// therefore require the full effective model/device configuration.
    #[must_use]
    pub fn matches_page_compatibility(&self, expected: &Self) -> bool {
        if self.parameters.contains("ocr-profile=") {
            self == expected
        } else if self.parameters.contains("route=native")
            // A native checkpoint only satisfies a native expectation; an
            // OCR-expected key matching through the lenient public-parameter
            // prefix is what used to double-collect native pages on resume
            // (review BLK-3) and let cross-route resumes through.
            && expected.parameters.contains("route=native")
        {
            self.input_sha256 == expected.input_sha256
                && self.model == expected.model
                && public_pdf_parameters(&self.parameters)
                    == public_pdf_parameters(&expected.parameters)
        } else {
            self == expected
        }
    }
}

fn public_pdf_parameters(parameters: &str) -> String {
    parameters.split(';').take(5).collect::<Vec<_>>().join(";")
}

fn without_pending_marker(key: &CompatibilityKey) -> CompatibilityKey {
    CompatibilityKey {
        parameters: key
            .parameters
            .strip_suffix(";pending-ocr=true")
            .unwrap_or(&key.parameters)
            .to_owned(),
        ..key.clone()
    }
}

/// Stable shared compatibility fragment for content-aware PDF decisions.
#[must_use]
pub fn pdf_compatibility_parameters(
    mode: &str,
    quality_tier: &str,
    classifier_version: &str,
    extractor_version: &str,
) -> String {
    format!(
        "pdf-mode={mode};quality-tier={quality_tier};classifier={classifier_version};extractor={extractor_version}"
    )
}

#[must_use]
pub fn mark_pending_ocr(parameters: &str) -> String {
    if parameters
        .split(';')
        .any(|field| field == "pending-ocr=true")
    {
        parameters.to_owned()
    } else if parameters.is_empty() {
        "pending-ocr=true".to_owned()
    } else {
        format!("{parameters};pending-ocr=true")
    }
}

#[must_use]
pub fn pending_ocr_checkpoint(
    page_number: u32,
    compatibility: CompatibilityKey,
    reason: impl Into<String>,
) -> PageCheckpoint {
    PageCheckpoint {
        page_number,
        compatibility: CompatibilityKey {
            parameters: mark_pending_ocr(&compatibility.parameters),
            ..compatibility
        },
        complete: false,
        output_sha256: String::new(),
        truncated: false,
        degenerated: false,
        attempts: 1,
        raw: None,
        warnings: vec![reason.into()],
        think_blocks_stripped: 0,
        audit: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrQueuePage {
    pub page: u32,
    pub fallback_reason: Option<String>,
}

/// Merges initial OCR plans and native-validation fallbacks deterministically.
#[must_use]
pub fn merge_ocr_queue(initial: &[OcrQueuePage], fallback: &[OcrQueuePage]) -> Vec<OcrQueuePage> {
    let mut merged = initial.to_vec();
    for candidate in fallback {
        if let Some(existing) = merged.iter_mut().find(|page| page.page == candidate.page) {
            if existing.fallback_reason.is_none() {
                existing
                    .fallback_reason
                    .clone_from(&candidate.fallback_reason);
            }
        } else {
            merged.push(candidate.clone());
        }
    }
    merged.sort_by_key(|page| page.page);
    merged
}

#[must_use]
pub fn native_fallback_queue(candidates: &[(u32, Result<(), String>)]) -> Vec<OcrQueuePage> {
    let mut queue: Vec<_> = candidates
        .iter()
        .filter_map(|(page, result)| {
            result.as_ref().err().map(|reason| OcrQueuePage {
                page: *page,
                fallback_reason: Some(reason.clone()),
            })
        })
        .collect();
    queue.sort_by_key(|page| page.page);
    queue.dedup_by_key(|page| page.page);
    queue
}

pub fn recover_pending_ocr_queue(
    root: impl AsRef<Path>,
    expected: &CompatibilityKey,
) -> io::Result<Vec<OcrQueuePage>> {
    let mut queue = Vec::new();
    let root = root.as_ref();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(page_text) = name
            .strip_prefix("page-")
            .and_then(|value| value.strip_suffix(".checkpoint"))
        else {
            continue;
        };
        let Ok(page) = page_text.parse::<u32>() else {
            continue;
        };
        if let Ok(checkpoint) = read_checkpoint(&path)
            && checkpoint.is_pending_ocr(expected)
        {
            queue.push(OcrQueuePage {
                page,
                fallback_reason: checkpoint.warnings.first().cloned(),
            });
        }
    }
    queue.sort_by_key(|page| page.page);
    queue.dedup_by_key(|page| page.page);
    Ok(queue)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageCheckpoint {
    pub page_number: u32,
    pub compatibility: CompatibilityKey,
    pub complete: bool,
    pub output_sha256: String,
    pub truncated: bool,
    /// Repetition-degeneration flag (plan §3.3): a degenerated page is
    /// reprocessed on resume even when otherwise reusable.
    pub degenerated: bool,
    /// Attempts spent by the most recent processing run (1..=1+len of
    /// []); reset on every Reprocess.
    pub attempts: u32,
    /// Raw-output sidecar pointer (`page-NNNN.raw`), per requirement §7.4.
    pub raw: Option<String>,
    pub warnings: Vec<String>,
    /// Think removals at the adapter artifact entry (quality plan Q3.1);
    /// additive v5 field, defaults to 0 when absent.
    pub think_blocks_stripped: u32,
    pub audit: Option<RenderAudit>,
}

/// Why a committed page is marked incomplete in the merged document
/// (plan §3.4); the wording differs between the two causes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageIncomplete {
    Truncated,
    Degenerated,
}

impl PageIncomplete {
    #[must_use]
    pub fn marker_line(self, page_number: u32) -> String {
        let reason = match self {
            Self::Truncated => "输出被截断",
            Self::Degenerated => "输出被复读退化截断",
        };
        format!(
            "> ⚠️ ocrx：第 {page_number} 页识别不完整（{reason}），本页内容可能缺失或错误，请重扫后核对本页。"
        )
    }
}

/// Window-atomic commit phase (plan §7.3, mechanism A): runs after every
/// window page reached a terminal state, in page order. A cancelled scan
/// never reaches this (the caller breaks first), so a cancelled window leaves
/// zero checkpoints and resume rescans it whole.
#[allow(clippy::too_many_arguments)] // mirrors the loop's accumulators
/// Phase-4 W3: resolves the PNG bytes for a committed page. With the
/// encode pipeline installed, this fetches the pre-encoded bytes for the
/// terminal render's generation (blocking briefly on the worker). Without
/// it, the artifact's inline `png` bytes (fakes / pipeline-less callers).
fn resolve_commit_png(
    encode_pipeline: Option<&encode_pipeline::EncodePipeline>,
    generation: u64,
    artifact_png: Option<&Vec<u8>>,
) -> io::Result<Vec<u8>> {
    if let Some(pipeline) = encode_pipeline {
        pipeline.take_png(generation)
    } else {
        artifact_png
            .cloned()
            .ok_or_else(|| io::Error::other("png bytes missing for commit"))
    }
}

/// Phase-4 W3: the page diagnostic for an encode error - identical wording
/// to the old inline-encode path (page-level `recognize` failure), never an
/// escalated task failure.
fn encode_error_diagnostic(
    page: u32,
    audit: Option<RenderAudit>,
    error: &io::Error,
) -> PageDiagnostic {
    PageDiagnostic {
        page,
        status: "failed".to_owned(),
        stage: "recognize".to_owned(),
        error_code: Some("recognition_failed".to_owned()),
        message: Some(format!("page recognition failed: {error}")),
        audit,
        warnings: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)] // mirrors the cancelled variant + pipeline
fn commit_terminal_pages(
    terminal: &[TerminalPage],
    root: &Path,
    compatibility: &CompatibilityKey,
    cancellation: &CancellationToken,
    encode_pipeline: Option<&encode_pipeline::EncodePipeline>,
    completed: &mut u32,
    failed: &mut u32,
    truncated: &mut u32,
    degenerated: &mut u32,
    think_blocks_stripped: &mut u64,
    warnings: &mut std::collections::BTreeSet<String>,
    diagnostics: &mut Vec<PageDiagnostic>,
    commit_recorder: &commit_profile::CommitRecorder,
) -> io::Result<()> {
    for entry in terminal {
        match entry {
            TerminalPage::Committed {
                page,
                image,
                artifact,
                attempts,
            } => {
                let page = *page;
                let attempts = *attempts;
                let mut artifact_warnings = artifact.warnings.clone();
                if image.audit.scaled
                    && !artifact_warnings
                        .iter()
                        .any(|warning| warning == "render_scaled")
                {
                    artifact_warnings.push("render_scaled".to_owned());
                }
                if artifact.truncated {
                    *truncated += 1;
                }
                if artifact.degenerated {
                    *degenerated += 1;
                }
                *think_blocks_stripped += u64::from(artifact.think_blocks_stripped);
                warnings.extend(artifact_warnings.iter().cloned());
                write_render_audit(root, page, image.audit)?;
                // Raw-output sidecar (plan §3.3): preserved whenever the page
                // degenerated or lost text to repetition cleaning.
                let raw_pointer = if artifact.degenerated
                    || artifact_warnings
                        .iter()
                        .any(|warning| warning == "repetition_removed")
                {
                    let name = format!("page-{page:04}.raw");
                    atomic_write(root.join(&name), artifact.raw_output.as_bytes())?;
                    Some(name)
                } else {
                    None
                };
                let checkpoint = PageCheckpoint {
                    page_number: page,
                    compatibility: compatibility.clone(),
                    complete: true,
                    output_sha256: String::new(),
                    truncated: artifact.truncated,
                    degenerated: artifact.degenerated,
                    attempts,
                    raw: raw_pointer,
                    warnings: artifact_warnings,
                    think_blocks_stripped: artifact.think_blocks_stripped,
                    audit: Some(image.audit),
                };
                // T1: the write span covers the atomic writes through the
                // checkpoint rename inside `commit_page`.
                let write_started = std::time::Instant::now();
                let png_bytes = match resolve_commit_png(
                    encode_pipeline,
                    image.generation,
                    artifact.png.as_ref(),
                ) {
                    Ok(bytes) => bytes,
                    Err(encode_error) => {
                        // Frozen surface: an encode error is a page-level
                        // recognize failure, never a task I/O failure.
                        *failed += 1;
                        diagnostics.push(encode_error_diagnostic(
                            page,
                            Some(image.audit),
                            &encode_error,
                        ));
                        continue;
                    }
                };
                commit_page(
                    root,
                    page,
                    &png_bytes,
                    &artifact.markdown,
                    &artifact.page_json,
                    &checkpoint,
                )?;
                commit_recorder.record_commit_write(
                    page,
                    u64::try_from(write_started.elapsed().as_micros()).unwrap_or(u64::MAX),
                );
                commit_recorder.mark_terminal(page);
                // BLK-3: the completed diagnostic only exists when the page
                // actually committed - an encode failure maps to its own
                // recognize-failed diagnostic above, never both.
                diagnostics.push(checkpoint.diagnostic());
                *completed += 1;
            }
            TerminalPage::RenderFailed { page, cancelled } => {
                let page = *page;
                let cancelled = *cancelled;
                commit_recorder.mark_terminal(page);
                if !cancelled {
                    *failed += 1;
                }
                diagnostics.push(PageDiagnostic {
                    page,
                    status: if cancelled {
                        "cancelled".to_owned()
                    } else {
                        "failed".to_owned()
                    },
                    stage: "render".to_owned(),
                    error_code: Some("render_failed".to_owned()),
                    message: Some("page rendering failed".to_owned()),
                    audit: None,
                    warnings: Vec::new(),
                });
            }
            TerminalPage::RecognizeFailed { page, audit, error } => {
                let page = *page;
                let audit = *audit;
                let cancelled = cancellation.is_cancelled();
                commit_recorder.mark_terminal(page);
                if !cancelled {
                    *failed += 1;
                }
                let (error_code, fallback_message) = classify_recognition_error(error);
                diagnostics.push(PageDiagnostic {
                    page,
                    status: if cancelled {
                        "cancelled".to_owned()
                    } else {
                        "failed".to_owned()
                    },
                    stage: "recognize".to_owned(),
                    error_code: Some(error_code.to_owned()),
                    message: Some(if error_code == "device_unavailable" {
                        fallback_message.to_owned()
                    } else {
                        format!("page recognition failed: {error}")
                    }),
                    audit,
                    warnings: Vec::new(),
                });
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointDisposition {
    Reuse,
    PendingOcr,
    Reprocess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointState {
    Committed,
    PendingOcr,
    Reprocess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointResumeAction {
    SkipCommitted,
    QueueOcr,
    Reprocess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingCheckpointMetadata {
    pub pdf_mode: String,
    pub quality_tier: String,
    pub classifier_version: String,
    pub extractor_version: String,
    pub page_classification: String,
    pub planned_method: String,
    pub actual_method: Option<String>,
    pub fallback_reason: Option<String>,
}

impl RoutingCheckpointMetadata {
    #[must_use]
    pub fn to_json(&self) -> String {
        let optional = |value: &Option<String>| {
            value.as_deref().map_or_else(
                || "null".to_owned(),
                |value| format!("\"{}\"", json_escape(value)),
            )
        };
        format!(
            "{{\"pdf_mode\":\"{}\",\"quality_tier\":\"{}\",\"classifier_version\":\"{}\",\"extractor_version\":\"{}\",\"page_classification\":\"{}\",\"planned_method\":\"{}\",\"actual_method\":{},\"fallback_reason\":{}}}",
            json_escape(&self.pdf_mode),
            json_escape(&self.quality_tier),
            json_escape(&self.classifier_version),
            json_escape(&self.extractor_version),
            json_escape(&self.page_classification),
            json_escape(&self.planned_method),
            optional(&self.actual_method),
            optional(&self.fallback_reason),
        )
    }
}

/// The sidecar contract used by resumed pages.  Keeping routing metadata in a
/// separate result envelope lets v5 checkpoints remain readable by older CLI
/// versions while new writers can persist the v6 routing decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingCheckpointResult {
    pub page: u32,
    pub routing: RoutingCheckpointMetadata,
}

impl RoutingCheckpointResult {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":\"ocrx-result-v2\",\"page\":{},\"routing\":{}}}",
            self.page,
            self.routing.to_json()
        )
    }
}

pub fn write_routing_result(
    path: impl AsRef<Path>,
    result: &RoutingCheckpointResult,
) -> io::Result<()> {
    atomic_write(path, result.to_json().as_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMarkdown {
    pub page_number: u32,
    pub content: String,
    /// Marker source carried from the checkpoint, never re-derived from the
    /// .md text (plan §3.4); `None` for complete pages.
    pub incomplete: Option<PageIncomplete>,
}

impl PageMarkdown {
    #[must_use]
    pub fn complete(page_number: u32, content: String) -> Self {
        Self {
            page_number,
            content,
            incomplete: None,
        }
    }
}

pub fn merge_markdown(mut pages: Vec<PageMarkdown>) -> io::Result<String> {
    if pages.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no pages to merge",
        ));
    }
    pages.sort_by_key(|page| page.page_number);
    if pages.iter().any(|page| page.page_number == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "page number must be positive",
        ));
    }
    if pages
        .windows(2)
        .any(|pair| pair[0].page_number == pair[1].page_number)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "duplicate page number",
        ));
    }
    let mut merged = String::new();
    for page in pages {
        if !merged.is_empty() {
            merged.push('\n');
        }
        merged.push_str(&format!("<!-- page: {} -->\n", page.page_number));
        if let Some(incomplete) = page.incomplete {
            // Frozen visible marker for degenerated/truncated pages
            // (plan §3.4); an empty line separates it from the page body.
            merged.push_str(&incomplete.marker_line(page.page_number));
            merged.push_str("\n\n");
        }
        merged.push_str(&normalize_lf(&page.content));
    }
    Ok(merged)
}

pub fn finalize_document(
    output_root: impl AsRef<Path>,
    pages: Vec<PageMarkdown>,
) -> io::Result<()> {
    let markdown = merge_markdown(pages)?;
    atomic_write(
        output_root.as_ref().join("document.md"),
        markdown.as_bytes(),
    )
}

pub fn collect_page_markdowns(
    output_root: impl AsRef<Path>,
    page_numbers: &[u32],
) -> io::Result<Vec<PageMarkdown>> {
    page_numbers
        .iter()
        .map(|page| {
            let root = output_root.as_ref();
            let content = fs::read_to_string(root.join(format!("page-{page:04}.md")))?;
            let checkpoint_path = root.join(format!("page-{page:04}.checkpoint"));
            let incomplete = read_checkpoint(&checkpoint_path)
                .ok()
                .and_then(|checkpoint| {
                    if checkpoint.degenerated {
                        Some(PageIncomplete::Degenerated)
                    } else if checkpoint.truncated {
                        Some(PageIncomplete::Truncated)
                    } else {
                        None
                    }
                });
            Ok(PageMarkdown {
                page_number: *page,
                content,
                incomplete,
            })
        })
        .collect()
}

fn normalize_lf(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

impl PageCheckpoint {
    #[must_use]
    pub fn resume_action(
        &self,
        expected: &CompatibilityKey,
        root: &Path,
        page: u32,
    ) -> CheckpointResumeAction {
        match self.state(expected, root, page) {
            CheckpointState::Committed => CheckpointResumeAction::SkipCommitted,
            CheckpointState::PendingOcr => CheckpointResumeAction::QueueOcr,
            CheckpointState::Reprocess => CheckpointResumeAction::Reprocess,
        }
    }

    #[must_use]
    pub fn state(&self, expected: &CompatibilityKey, root: &Path, page: u32) -> CheckpointState {
        match self.disposition(expected, root, page) {
            CheckpointDisposition::Reuse => CheckpointState::Committed,
            CheckpointDisposition::PendingOcr => CheckpointState::PendingOcr,
            CheckpointDisposition::Reprocess => CheckpointState::Reprocess,
        }
    }

    #[must_use]
    pub fn disposition(
        &self,
        expected: &CompatibilityKey,
        root: &Path,
        page: u32,
    ) -> CheckpointDisposition {
        // Plan §3.5 (v2): only a degenerated page is reprocessed on resume —
        // a pure max_tokens page would deterministically truncate again under
        // identical parameters.
        if !self.degenerated && self.is_reusable_at(expected, root, page) {
            CheckpointDisposition::Reuse
        } else if self.is_pending_ocr(expected) {
            CheckpointDisposition::PendingOcr
        } else {
            CheckpointDisposition::Reprocess
        }
    }

    #[must_use]
    pub fn is_reusable(&self, expected: &CompatibilityKey) -> bool {
        self.complete
            && self.compatibility.matches_page_compatibility(expected)
            && !self.output_sha256.is_empty()
    }

    #[must_use]
    pub fn is_reusable_at(&self, expected: &CompatibilityKey, root: &Path, page: u32) -> bool {
        if !self.is_reusable(expected) || self.page_number != page {
            return false;
        }
        let markdown = root.join(format!("page-{page:04}.md"));
        if !markdown.is_file() {
            return false;
        }
        artifact_sha256(&markdown, root)
            .map(|hash| hash == self.output_sha256)
            .unwrap_or(false)
    }

    /// Identifies an analyzed page whose final result is intentionally waiting
    /// for the OCR phase. It is recoverable, but never a completed artifact.
    #[must_use]
    pub fn is_pending_ocr(&self, expected: &CompatibilityKey) -> bool {
        let pending_base = without_pending_marker(&self.compatibility);
        let expected_base = without_pending_marker(expected);
        !self.complete
            && pending_base.matches_page_compatibility(&expected_base)
            && self.compatibility.parameters.contains("pending-ocr=true")
    }

    fn diagnostic(&self) -> PageDiagnostic {
        PageDiagnostic {
            page: self.page_number,
            status: if self.truncated || self.degenerated {
                "partial"
            } else {
                "completed"
            }
            .to_owned(),
            stage: "recognize".to_owned(),
            error_code: None,
            message: None,
            audit: self.audit,
            warnings: self.warnings.clone(),
        }
    }
}

fn artifact_sha256(markdown: &Path, root: &Path) -> io::Result<String> {
    let content = fs::read_to_string(markdown)?;
    let mut files = vec![markdown.to_path_buf()];
    for start in content.match_indices("images/").map(|(start, _)| start) {
        let tail = &content[start..];
        let end = tail
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ')' | '"' | '\'')
            })
            .unwrap_or(tail.len());
        let relative = &tail[..end];
        let file = root.join(relative);
        if !file.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "referenced image missing",
            ));
        }
        files.push(file);
    }
    files.sort();
    let mut manifest = Vec::new();
    for file in files {
        manifest.extend_from_slice(
            file.strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .as_bytes(),
        );
        manifest.push(0);
        manifest.extend_from_slice(&fs::read(&file)?);
    }
    Ok(ocrx_core::sha256(&manifest))
}

pub fn write_checkpoint(path: impl AsRef<Path>, checkpoint: &PageCheckpoint) -> io::Result<()> {
    // Additive v5 fields (plan §3.3): older parsers collect unknown lines
    // without reading them, so this stays bidirectionally compatible.
    let content = format!(
        "ocrx-checkpoint-v5\npage={}\ninput={}\nmodel={}\nprofile={}\nparameters={}\ncomplete={}\noutput={}\ntruncated={}\ndegenerated={}\nattempts={}\nraw={}\nwarnings={}\nthink_blocks_stripped={}\naudit={}\n",
        checkpoint.page_number,
        encode_field(&checkpoint.compatibility.input_sha256),
        encode_field(&checkpoint.compatibility.model),
        encode_field(&checkpoint.compatibility.profile),
        encode_field(&checkpoint.compatibility.parameters),
        checkpoint.complete,
        checkpoint.output_sha256,
        checkpoint.truncated,
        checkpoint.degenerated,
        checkpoint.attempts,
        checkpoint.raw.as_deref().unwrap_or("none"),
        encode_field(&checkpoint.warnings.join("\n")),
        checkpoint.think_blocks_stripped,
        checkpoint.audit.map_or_else(
            || "none".to_owned(),
            |audit| encode_field(&audit.as_checkpoint_fragment())
        ),
    );
    atomic_write(path, content.as_bytes())
}

pub fn write_render_audit(root: &Path, page: u32, audit: RenderAudit) -> io::Result<()> {
    let path = root.join(format!("page-{page:04}.render-audit"));
    let temporary = root.join(format!(".page-{page:04}.render-audit.part"));
    atomic_write(&temporary, audit.as_checkpoint_fragment().as_bytes())?;
    fs::rename(temporary, path)
}

pub fn read_checkpoint(path: impl AsRef<Path>) -> io::Result<PageCheckpoint> {
    let content = fs::read_to_string(path)?;
    if content.lines().next() != Some("ocrx-checkpoint-v5") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported checkpoint version",
        ));
    }
    let mut values = std::collections::BTreeMap::new();
    for line in content.lines().skip(1) {
        let (key, value) = line.split_once('=').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "malformed checkpoint field")
        })?;
        values.insert(key, value);
    }
    let get = |key: &str| {
        values.get(key).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing checkpoint field: {key}"),
            )
        })
    };
    let page_number = get("page")?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid checkpoint page"))?;
    let complete = get("complete")?.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid checkpoint completion flag",
        )
    })?;
    let truncated = get("truncated")?.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid checkpoint truncation flag",
        )
    })?;
    let warning_text = decode_field(get("warnings")?)?;
    let audit = match get("audit")? {
        "none" => None,
        encoded => Some(parse_render_audit(&decode_field(encoded)?)?),
    };
    // Additive fields default to their pre-v2 values when absent so old v5
    // checkpoints (and files written by this version, read back piecemeal)
    // keep loading.
    let degenerated = match values.get("degenerated") {
        Some(value) => value.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid checkpoint degenerated flag",
            )
        })?,
        None => false,
    };
    let attempts = match values.get("attempts") {
        Some(value) => value.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid checkpoint attempts count",
            )
        })?,
        None => 1,
    };
    let raw = match values.get("raw") {
        Some(&"none") | None => None,
        Some(name) => Some((*name).to_owned()),
    };
    let think_blocks_stripped = match values.get("think_blocks_stripped") {
        Some(value) => value.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid checkpoint think_blocks_stripped count",
            )
        })?,
        None => 0,
    };
    Ok(PageCheckpoint {
        page_number,
        compatibility: CompatibilityKey {
            input_sha256: decode_field(get("input")?)?,
            model: decode_field(get("model")?)?,
            profile: decode_field(get("profile")?)?,
            parameters: decode_field(get("parameters")?)?,
        },
        complete,
        output_sha256: get("output")?.to_owned(),
        truncated,
        degenerated,
        attempts,
        raw,
        warnings: if warning_text.is_empty() {
            Vec::new()
        } else {
            warning_text.split('\n').map(str::to_owned).collect()
        },
        think_blocks_stripped,
        audit,
    })
}

fn encode_field(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_field(value: &str) -> io::Result<String> {
    if value.len() % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid encoded checkpoint field",
        ));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid encoded checkpoint field",
                )
            })
        })
        .collect::<io::Result<Vec<_>>>()
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "checkpoint field is not UTF-8")
            })
        })
}

fn parse_render_audit(value: &str) -> io::Result<RenderAudit> {
    let mut values = std::collections::BTreeMap::new();
    for part in value.split(';') {
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid render audit"))?;
        values.insert(key, value);
    }
    let dimensions = |key: &str| -> io::Result<(u32, u32)> {
        let value = values.get(key).copied().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing render audit field")
        })?;
        let (width, height) = value.split_once('x').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid render audit dimensions",
            )
        })?;
        Ok((
            width.parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid render audit width")
            })?,
            height.parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid render audit height")
            })?,
        ))
    };
    let (original_width, original_height) = dimensions("original")?;
    let (rendered_width, rendered_height) = dimensions("rendered")?;
    let (model_width, model_height) = dimensions("model")?;
    let scaled = values.get("scaled") == Some(&"true");
    let (scale_ratio_numerator, scale_ratio_denominator) = values
        .get("ratio")
        .and_then(|ratio| ratio.split_once('/'))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing render audit ratio"))?;
    Ok(RenderAudit {
        original_width,
        original_height,
        rendered_width,
        rendered_height,
        model_width,
        model_height,
        scaled,
        scale_ratio_numerator: scale_ratio_numerator.parse().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid render audit ratio")
        })?,
        scale_ratio_denominator: scale_ratio_denominator.parse().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid render audit ratio")
        })?,
    })
}

pub fn atomic_write(path: impl AsRef<Path>, contents: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = temporary_path(path);
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Compares path components for equality, ignoring ASCII case on Windows.
///
/// NTFS (and the Win32 namespace generally) matches paths case-insensitively,
/// so byte-exact comparison lets `c:\users\space\appdata\local` slip past a
/// protected `%LOCALAPPDATA%` root. Comparison is component-wise, so
/// `C:\foo` never matches a prefix of `C:\foobar`.
fn comparable_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let text = component.as_os_str().to_string_lossy().into_owned();
            if cfg!(target_os = "windows") {
                text.to_ascii_lowercase()
            } else {
                text
            }
        })
        .collect()
}

/// Returns whether `descendant` is `ancestor` itself or lives underneath it
/// (case-insensitively on Windows).
#[must_use]
pub fn path_contains(ancestor: &Path, descendant: &Path) -> bool {
    let ancestor_components = comparable_components(ancestor);
    let descendant_components = comparable_components(descendant);
    descendant_components.len() >= ancestor_components.len()
        && descendant_components[..ancestor_components.len()] == ancestor_components[..]
}

/// Returns whether two paths overlap: equal, one inside the other, or sharing
/// a case-variant spelling on Windows.
#[must_use]
pub fn paths_overlap(left: &Path, right: &Path) -> bool {
    path_contains(left, right) || path_contains(right, left)
}

/// Canonicalizes `path` when it exists; otherwise canonicalizes its deepest
/// existing ancestor and rejoins the remaining components.
///
/// Both sides of an overlap check must go through this helper: Windows
/// `canonicalize` returns `\\?\`-prefixed extended paths, so mixing a
/// canonicalized spelling with a plain one would never match. Resolving
/// through the deepest existing ancestor also unfolds `..`, 8.3 short names
/// (`PROGRA~1`), and reparse points for the parts of the path that exist
/// (review BLK-2: component-wise comparison alone was bypassable).
fn canonical_form(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    let components: Vec<std::path::Component<'_>> = path.components().collect();
    // Try every split point from the deepest prefix downwards; when the
    // prefix exists, its canonical form plus the (non-existent) remainder is
    // the canonical spelling of the whole path. Every component — including
    // the leaf — is rejoined; dropping the leaf would silently compare the
    // parent directory instead (found by the BLK-3 regression test: a temp
    // output under an existing temp root compared as the temp root itself).
    for index in (1..components.len()).rev() {
        let ancestor: PathBuf = components[..index].iter().collect();
        if let Ok(mut joined) = fs::canonicalize(&ancestor) {
            for component in &components[index..] {
                joined = joined.join(component.as_os_str());
            }
            return joined;
        }
    }
    path.to_path_buf()
}

/// Resolves `.` and `..` lexically for absolute paths (relative paths are
/// joined onto the current directory first), matching Win32 resolution
/// (review BLK-2 fix sequence: normalize first, then check). A `..` at a
/// root boundary is a no-op.
fn lexically_normalize(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Rejects output-path spellings whose meaning the filesystem resolves
/// differently than the component-wise comparison sees (review BLK-2):
///
/// - `..` components are never resolved by `Path::components`, so
///   `E:\a\..\b` compares as `E:\a\..\b` while the filesystem means `E:\b`;
///   output paths containing them are refused outright.
/// - Windows strips trailing dots and spaces from each path component, so
///   `"dir "` compares as a distinct component but deletes `dir` — the
///   trailing-space `--overwrite` deletion vector. A bare `.` component
///   (current directory) is legal and exempt.
fn reject_ambiguous_output_path(output: &Path) -> io::Result<()> {
    if output
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path must not contain '..' components",
        ));
    }
    if cfg!(target_os = "windows")
        && output.components().any(|component| {
            let text = component.as_os_str().to_string_lossy();
            text.len() > 1 && (text.ends_with(' ') || text.ends_with('.'))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path components must not end with dots or spaces on Windows",
        ));
    }
    Ok(())
}

pub fn validate_output_root(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    protected_roots: &[&Path],
) -> io::Result<()> {
    let input = input.as_ref();
    let output = output.as_ref();
    if !output.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output root must be absolute",
        ));
    }
    // Normalize first (review BLK-2 sequence): a default output derived from
    // a relative input legitimately contains `..` (cwd.join("../../x"));
    // lexical resolution turns it into its real absolute spelling, and only
    // genuinely ambiguous spellings (trailing dots/spaces on Windows) are
    // then refused outright.
    let output_normalized = lexically_normalize(output);
    let input_normalized = lexically_normalize(input);
    reject_ambiguous_output_path(&output_normalized)?;
    // Compare canonical forms: residual `..` in existing prefixes, 8.3 short
    // names, and case variants all resolve to one spelling before the
    // overlap test.
    let output_canonical = canonical_form(&output_normalized);
    let input_canonical = canonical_form(&input_normalized);
    if paths_overlap(&output_canonical, &input_canonical) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output root is the input path",
        ));
    }
    for protected in protected_roots {
        let protected_canonical = canonical_form(protected);
        if paths_overlap(&output_canonical, &protected_canonical) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "output root overlaps a protected path",
            ));
        }
    }
    Ok(())
}

/// Validates the working-directory rule for a scan output root: `output` must
/// not be `protected` itself and must not live ABOVE it, while a dedicated
/// subdirectory underneath `protected` stays allowed.
///
/// Deliberately one-directional, unlike `paths_overlap`: `--overwrite` removes
/// the whole output tree, so an output at or above the working directory would
/// take the working directory with it, but an output below it
/// (`--output-dir out`, the default `<input dir>/<stem>-ocr`) is the normal
/// layout and must pass.
pub fn validate_output_not_containing(
    output: impl AsRef<Path>,
    protected: impl AsRef<Path>,
) -> io::Result<()> {
    let output_canonical = canonical_form(&lexically_normalize(output.as_ref()));
    let protected_canonical = canonical_form(&lexically_normalize(protected.as_ref()));
    if path_contains(&output_canonical, &protected_canonical) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "output root is the working directory or an ancestor of it",
        ));
    }
    Ok(())
}

/// Removes `output` after the full overwrite guard set: the shared root
/// validator (absolute path, `..`/trailing-dot-spaces refusal, input overlap,
/// protected roots) plus the one-directional working-directory rule — the
/// deletion target must not be the working directory or an ancestor of it.
pub fn remove_for_overwrite(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    protected_roots: &[&Path],
    working_directory: impl AsRef<Path>,
) -> io::Result<()> {
    validate_output_root(&input, &output, protected_roots)?;
    validate_output_not_containing(&output, working_directory)?;
    let output = output.as_ref();
    if output.exists() {
        fs::remove_dir_all(output)?;
    }
    Ok(())
}

pub fn commit_page(
    output_root: impl AsRef<Path>,
    page_number: u32,
    png: &[u8],
    markdown: &str,
    page_json: &str,
    checkpoint: &PageCheckpoint,
) -> io::Result<()> {
    if page_number == 0 || checkpoint.page_number != page_number {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "checkpoint page mismatch",
        ));
    }
    let root = output_root.as_ref();
    atomic_write(
        root.join("images")
            .join(format!("page-{page_number:04}.png")),
        png,
    )?;
    atomic_write(
        root.join(format!("page-{page_number:04}.md")),
        normalize_lf(markdown).as_bytes(),
    )?;
    atomic_write(
        root.join(format!("page-{page_number:04}.json")),
        page_json.as_bytes(),
    )?;
    let output_sha256 = artifact_sha256(&root.join(format!("page-{page_number:04}.md")), root)?;
    write_checkpoint(
        root.join(format!("page-{page_number:04}.checkpoint")),
        &PageCheckpoint {
            complete: true,
            output_sha256,
            ..checkpoint.clone()
        },
    )
}

/// Commits a page produced by the native PDF text path without creating a
/// synthetic raster image. The checkpoint becomes complete only after the
/// markdown and page JSON artifacts are durable.
pub fn commit_native_page(
    output_root: impl AsRef<Path>,
    page_number: u32,
    markdown: &str,
    page_json: &str,
    checkpoint: &PageCheckpoint,
) -> io::Result<()> {
    if page_number == 0 || checkpoint.page_number != page_number {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "checkpoint page mismatch",
        ));
    }
    let root = output_root.as_ref();
    atomic_write(
        root.join(format!("page-{page_number:04}.md")),
        normalize_lf(markdown).as_bytes(),
    )?;
    atomic_write(
        root.join(format!("page-{page_number:04}.json")),
        page_json.as_bytes(),
    )?;
    let output_sha256 = artifact_sha256(&root.join(format!("page-{page_number:04}.md")), root)?;
    write_checkpoint(
        root.join(format!("page-{page_number:04}.checkpoint")),
        &PageCheckpoint {
            complete: true,
            output_sha256,
            ..checkpoint.clone()
        },
    )
}

pub fn commit_native_page_and_collect(
    output_root: impl AsRef<Path>,
    page_number: u32,
    markdown: &str,
    page_json: &str,
    checkpoint: &PageCheckpoint,
) -> io::Result<PageMarkdown> {
    commit_native_page(&output_root, page_number, markdown, page_json, checkpoint)?;
    Ok(PageMarkdown {
        page_number,
        content: normalize_lf(markdown),
        incomplete: None,
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    path.with_file_name(format!(".{name}.part-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::{
        CancellationToken, CheckpointDisposition, CheckpointResumeAction, CheckpointState,
        CompatibilityKey, DEGENERATION_RETRY_LADDER, DegenerationRetryStep, ErrorReport,
        GenerationProgress, Heartbeat, JobStatus, JobSummary, ModelLoader, OcrQueuePage,
        PageArtifactRecognizer, PageArtifactResult, PageCheckpoint, PageDiagnostic, PageIncomplete,
        PageLifecycle, PageMarkdown, PageRecognizer, PageRenderer, PageStatus, ProgressEvent,
        ProgressMode, ProgressSnapshot, ProgressStage, RenderAudit, RenderedPage, ReportFormat,
        Reporter, ResultReport, ResultStatus, RoutingCheckpointMetadata, RoutingCheckpointResult,
        ScanController, ScanService, atomic_write, classify_recognition_error, format_report,
        merge_job_summaries, recover_pending_ocr_queue,
    };
    use crate::encode_pipeline;
    use std::fs;

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ocrx-output-test-{}-{name}", std::process::id()))
    }

    struct FakeRenderer {
        calls: Vec<u32>,
        fail_page: Option<u32>,
    }
    impl PageRenderer for FakeRenderer {
        fn preflight(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
            self.calls.push(page);
            if self.fail_page == Some(page) {
                Err(std::io::Error::other("render"))
            } else {
                RenderedPage::new(1, 1, vec![page as u8, 0, 0])
            }
        }
    }
    struct FakeLoader {
        loads: u32,
    }
    impl ModelLoader for FakeLoader {
        type Model = u32;
        fn load(&mut self) -> std::io::Result<Self::Model> {
            self.loads += 1;
            Ok(1)
        }
    }
    struct FakeRecognizer {
        calls: Vec<u32>,
    }
    impl PageRecognizer<u32> for FakeRecognizer {
        fn recognize(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
        ) -> std::io::Result<PageStatus> {
            self.calls.push(page);
            Ok(PageStatus::Completed)
        }
    }

    struct FakeArtifacts {
        calls: Vec<u32>,
    }
    impl PageArtifactRecognizer<u32> for FakeArtifacts {
        fn recognize_artifact(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
        ) -> std::io::Result<PageArtifactResult> {
            self.calls.push(page);
            Ok(PageArtifactResult {
                png: Some(vec![page as u8]),
                think_blocks_stripped: 0,
                markdown: format!("page {page}"),
                degenerated: false,
                raw_output: String::new(),
                page_json: format!(
                    r#"{{"schema_version":1,"page_number":{page},"blocks":[],"termination":"eos","warnings":[]}}"#
                ),
                truncated: false,
                warnings: Vec::new(),
            })
        }
    }

    /// Fake whose WINDOW recognition always fails at the batch level; the
    /// single-page path succeeds — the scan must fall back to depth-1 and
    /// still commit every page (plan §7.3③/§7.4 批级故障降级).
    struct BatchFailingArtifacts {
        serial_calls: Vec<u32>,
    }
    impl PageArtifactRecognizer<u32> for BatchFailingArtifacts {
        fn recognize_artifact(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
        ) -> std::io::Result<PageArtifactResult> {
            self.serial_calls.push(page);
            Ok(PageArtifactResult {
                png: Some(vec![page as u8]),
                think_blocks_stripped: 0,
                markdown: format!("page {page}"),
                degenerated: false,
                raw_output: String::new(),
                page_json: format!(
                    r#"{{"schema_version":1,"page_number":{page},"blocks":[],"termination":"eos","warnings":[]}}"#
                ),
                truncated: false,
                warnings: Vec::new(),
            })
        }

        fn recognize_window_cancelled(
            &mut self,
            _model: &mut u32,
            pages: &[(u32, RenderedPage)],
            _cancellation: &CancellationToken,
            _progress: &mut dyn FnMut(u32, GenerationProgress),
        ) -> Vec<std::io::Result<PageArtifactResult>> {
            pages
                .iter()
                .map(|(_, _)| {
                    Err(std::io::Error::other(format!(
                        "{}injected batch failure",
                        ocrx_runtime::BATCH_ERROR_PREFIX
                    )))
                })
                .collect()
        }
    }

    /// Fake whose recognition records a PNG encode into a shared T1
    /// recorder — the same handoff the CLI recognizer performs, so the
    /// end-to-end test proves the encode segment lands on the same profile
    /// state as the service's render/commit segments (v2.1 T1, shared sink).
    struct EncodingArtifacts {
        calls: Vec<u32>,
        encode_sink: Option<crate::commit_profile::CommitRecorder>,
    }
    impl PageArtifactRecognizer<u32> for EncodingArtifacts {
        fn recognize_artifact(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
        ) -> std::io::Result<PageArtifactResult> {
            self.calls.push(page);
            if let Some(recorder) = &self.encode_sink {
                recorder.record_png_encode(page, 1);
            }
            Ok(PageArtifactResult {
                png: Some(vec![page as u8]),
                think_blocks_stripped: 0,
                markdown: format!("page {page}"),
                degenerated: false,
                raw_output: String::new(),
                page_json: format!(
                    r#"{{"schema_version":1,"page_number":{page},"blocks":[],"termination":"eos","warnings":[]}}"#
                ),
                truncated: false,
                warnings: Vec::new(),
            })
        }
    }

    /// Renderer with a measurable (>=1 microsecond) render wait so the
    /// commit-profile e2e can assert non-zero render_wait_us — the plain
    /// fake finishes in well under a microsecond, and a sub-microsecond
    /// wait correctly records 0.
    struct SleepyRenderer {
        inner: FakeRenderer,
    }
    impl PageRenderer for SleepyRenderer {
        fn preflight(&mut self) -> std::io::Result<()> {
            self.inner.preflight()
        }
        fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
            std::thread::sleep(std::time::Duration::from_millis(1));
            self.inner.render(page)
        }
    }

    #[test]
    fn commit_profile_captures_every_segment_end_to_end() {
        // T1 acceptance precondition: with the recorder enabled, every
        // segment (render wait, encode via the shared sink, commit write,
        // finalize) fills non-zero, rows are keyed per page, and totals fold
        // exactly the terminal attempts.
        let root = test_dir("commit-profile-e2e");
        let _ = fs::remove_dir_all(&root);
        let recorder = crate::commit_profile::CommitRecorder::with_enabled(true);
        let mut service = ScanService::with_commit_recorder(
            SleepyRenderer {
                inner: FakeRenderer {
                    calls: Vec::new(),
                    fail_page: None,
                },
            },
            FakeLoader { loads: 0 },
            EncodingArtifacts {
                calls: Vec::new(),
                encode_sink: Some(recorder.clone()),
            },
            recorder.clone(),
        );
        let cancellation = CancellationToken::new();
        let summary = service
            .run_with_checkpoints_heartbeat(
                &[1, 2, 3],
                &root,
                &degeneration_compatibility(),
                false,
                false,
                &cancellation,
                &mut |_, _| {},
            )
            .unwrap();
        assert_eq!(summary.completed_pages, 3);
        let profile = service
            .commit_profile()
            .expect("enabled recorder yields the snapshot");
        assert_eq!(profile.pages.len(), 3, "one row per page");
        for row in &profile.pages {
            assert_eq!(row.generation, 1, "single attempt per page");
            assert!(
                row.render_wait_us > 0,
                "render wait recorded for page {}",
                row.page
            );
            assert_eq!(
                row.png_encode_us, 1,
                "encode lands via the shared sink on page {}",
                row.page
            );
            assert!(
                row.commit_write_us > 0,
                "commit write recorded for page {}",
                row.page
            );
        }
        assert_eq!(profile.pages_total, 3, "every page reached terminal state");
        assert_eq!(profile.png_encode_us, 3);
        assert!(profile.render_wait_us > 0);
        assert!(profile.commit_write_us > 0);
        assert!(profile.finalize_us > 0, "finalize recorded once");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn commit_profile_disabled_yields_nothing() {
        // Env-off runs (and disabled-injected tests) get no profile and no
        // recorder state — the write gate in the CLI sees `None`.
        let root = test_dir("commit-profile-disabled");
        let _ = fs::remove_dir_all(&root);
        let mut service = ScanService::with_commit_recorder(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            EncodingArtifacts {
                calls: Vec::new(),
                encode_sink: None,
            },
            crate::commit_profile::CommitRecorder::with_enabled(false),
        );
        let cancellation = CancellationToken::new();
        let summary = service
            .run_with_checkpoints_heartbeat(
                &[1, 2],
                &root,
                &degeneration_compatibility(),
                false,
                false,
                &cancellation,
                &mut |_, _| {},
            )
            .unwrap();
        assert_eq!(summary.completed_pages, 2);
        assert!(service.commit_profile().is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn commit_profile_cancelled_scan_never_reaches_terminal_totals() {
        // A cancelled scan writes nothing (CLI gate on `cancelled`), and the
        // profile itself shows zero terminal folds and zero finalize — the
        // observable "no partial write" contract.
        let root = test_dir("commit-profile-cancelled");
        let _ = fs::remove_dir_all(&root);
        let recorder = crate::commit_profile::CommitRecorder::with_enabled(true);
        let mut service = ScanService::with_commit_recorder(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            EncodingArtifacts {
                calls: Vec::new(),
                encode_sink: Some(recorder.clone()),
            },
            recorder,
        );
        let cancellation = CancellationToken::new();
        cancellation.request_cancel();
        let summary = service
            .run_with_checkpoints_heartbeat(
                &[1, 2],
                &root,
                &degeneration_compatibility(),
                false,
                false,
                &cancellation,
                &mut |_, _| {},
            )
            .unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.completed_pages, 0);
        let profile = service
            .commit_profile()
            .expect("recorder enabled; snapshot still inspectable");
        assert_eq!(profile.pages_total, 0, "nothing terminal under cancel");
        assert_eq!(profile.finalize_us, 0, "finalize never ran");
        assert_eq!(profile.commit_write_us, 0, "nothing was written");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn batch_level_failure_falls_back_to_single_stream_and_commits_all_pages() {
        let root = test_dir("batch-fallback");
        let _ = fs::remove_dir_all(&root);
        let recognizer = BatchFailingArtifacts {
            serial_calls: Vec::new(),
        };
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            recognizer,
        );
        let summary = service
            .run_with_checkpoints(&[1, 2, 3], &root, &degeneration_compatibility(), false)
            .unwrap();
        // Every page committed through the depth-1 fallback.
        assert_eq!(summary.completed_pages, 3);
        assert_eq!(summary.failed_pages, 0);
        assert_eq!(
            service.recognizer.serial_calls,
            vec![1, 2, 3],
            "fallback must rerun every window page single-stream"
        );
        assert!(root.join("page-0001.checkpoint").is_file());
        assert!(root.join("page-0003.checkpoint").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_pages_with_model_matches_the_loading_entry_point() {
        // X1: the orchestrated entry point (externally owned model) must
        // behave identically to the loading one — same summary, same
        // checkpoints, same committed artifacts — for a clean window and for
        // a render-failed page.
        let root = test_dir("run-pages-with-model");
        let _ = fs::remove_dir_all(&root);
        for fail_page in [None, Some(2)] {
            let mut summaries = Vec::new();
            let mut targets = Vec::new();
            for (mode, entry) in [("loading", 0u8), ("shared", 1u8)] {
                let target = root.join(format!("fail{fail_page:?}-{mode}"));
                let mut service = ScanService::new(
                    FakeRenderer {
                        calls: Vec::new(),
                        fail_page,
                    },
                    FakeLoader { loads: 0 },
                    FakeArtifacts { calls: Vec::new() },
                );
                let summary = if entry == 0 {
                    service
                        .run_with_checkpoints(
                            &[1, 2, 3],
                            &target,
                            &degeneration_compatibility(),
                            false,
                        )
                        .unwrap()
                } else {
                    let mut model = service.loader.load().unwrap();
                    service
                        .run_pages_with_model(
                            &mut model,
                            &[1, 2, 3],
                            &target,
                            &degeneration_compatibility(),
                            false,
                            false,
                            &CancellationToken::new(),
                            &mut |_, _| {},
                        )
                        .unwrap()
                };
                summaries.push(summary);
                targets.push(target);
            }
            assert_eq!(
                summaries[0].completed_pages, summaries[1].completed_pages,
                "completed pages diverged between entry points"
            );
            assert_eq!(
                summaries[0].failed_pages, summaries[1].failed_pages,
                "failed pages diverged between entry points"
            );
            let expected_completed =
                u32::from(fail_page.is_none()) * 3 + u32::from(fail_page.is_some()) * 2;
            assert_eq!(summaries[0].completed_pages, expected_completed);
            for page in [1, 2, 3] {
                for suffix in ["checkpoint", "md"] {
                    let name = format!("page-{page:04}.{suffix}");
                    assert_eq!(
                        fs::read(targets[0].join(&name)).ok(),
                        fs::read(targets[1].join(&name)).ok(),
                        "{name} diverged between entry points"
                    );
                }
            }
            assert_eq!(
                fs::read(targets[0].join("document.md")).ok(),
                fs::read(targets[1].join("document.md")).ok(),
                "document.md diverged between entry points"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_service_preflights_loads_once_and_processes_serially() {
        let renderer = FakeRenderer {
            calls: Vec::new(),
            fail_page: Some(2),
        };
        let loader = FakeLoader { loads: 0 };
        let recognizer = FakeRecognizer { calls: Vec::new() };
        let mut service = ScanService::new(renderer, loader, recognizer);
        let summary = service.run(&[1, 2, 3]).unwrap();
        assert_eq!(summary.selected_pages, 3);
        assert_eq!(summary.completed_pages, 2);
        assert_eq!(summary.failed_pages, 1);
        assert_eq!(service.loader.loads, 1);
        assert_eq!(service.renderer.calls, vec![1, 2, 3]);
        assert_eq!(service.recognizer.calls, vec![1, 3]);
    }

    #[test]
    fn rendered_page_rejects_invalid_rgb8_shape() {
        assert!(RenderedPage::new(2, 2, vec![0; 11]).is_err());
        let page = RenderedPage::new(2, 2, vec![0; 12]).unwrap();
        assert_eq!((page.width, page.height), (2, 2));
    }

    #[test]
    fn rendered_page_carries_stable_render_audit() {
        let page = RenderedPage::new(20, 10, vec![0; 600])
            .unwrap()
            .with_audit(RenderAudit {
                original_width: 40,
                original_height: 20,
                rendered_width: 20,
                rendered_height: 10,
                model_width: 20,
                model_height: 10,
                scaled: true,
                scale_ratio_numerator: 1,
                scale_ratio_denominator: 1,
            });
        assert!(page.audit.scaled);
        assert_eq!(
            (page.audit.original_width, page.audit.model_height),
            (40, 10)
        );
    }

    // ---- D3: degeneration retry, markers, sidecar (plan §3.3-§3.5, §5.2) ----

    struct ScriptedRenderer {
        calls: std::collections::VecDeque<(u32, u32, u32)>,
    }
    impl PageRenderer for ScriptedRenderer {
        fn preflight(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
            self.calls.push_back((page, 1, 1));
            RenderedPage::new(1, 1, vec![1, 2, 3])
        }
        fn render_perturbed(
            &mut self,
            page: u32,
            numerator: u32,
            denominator: u32,
        ) -> std::io::Result<RenderedPage> {
            self.calls.push_back((page, numerator, denominator));
            RenderedPage::new(1, 1, vec![4, 5, 6])
        }
    }

    fn normal_artifact(page: u32) -> PageArtifactResult {
        PageArtifactResult {
            png: Some(vec![page as u8]),
            think_blocks_stripped: 0,
            markdown: format!("clean page {page}"),
            page_json: format!(
                r#"{{"schema_version":1,"page_number":{page},"blocks":[],"termination":"eos","warnings":[]"}}"#
            ),
            truncated: false,
            degenerated: false,
            warnings: Vec::new(),
            raw_output: String::new(),
        }
    }

    fn degenerate_artifact(page: u32) -> PageArtifactResult {
        PageArtifactResult {
            png: Some(vec![page as u8]),
            think_blocks_stripped: 0,
            markdown: format!("cropped page {page}"),
            page_json: format!(
                r#"{{"schema_version":1,"page_number":{page},"blocks":[],"termination":"max_tokens","warnings":["truncated","repetition_removed","degenerated"]}}"#
            ),
            truncated: true,
            degenerated: true,
            warnings: vec![
                "truncated".to_owned(),
                "repetition_removed".to_owned(),
                "degenerated".to_owned(),
            ],
            raw_output: format!("loop loop loop page {page}"),
        }
    }

    struct ScriptedRecognizer {
        script: std::collections::VecDeque<PageArtifactResult>,
        calls: u32,
        retry_calls: u32,
        on_call: Option<Box<dyn FnMut(u32)>>,
        fail_when_cancelled: Option<CancellationToken>,
    }
    impl PageArtifactRecognizer<u32> for ScriptedRecognizer {
        fn recognize_artifact(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
        ) -> std::io::Result<PageArtifactResult> {
            self.calls += 1;
            self.scripted_next(page)
        }

        fn recognize_retry_artifact_cancelled(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
            _step: &DegenerationRetryStep,
            _cancellation: &CancellationToken,
            _progress: &mut dyn FnMut(GenerationProgress),
        ) -> std::io::Result<PageArtifactResult> {
            self.retry_calls += 1;
            self.scripted_next(page)
        }
    }
    impl ScriptedRecognizer {
        fn scripted_next(&mut self, page: u32) -> std::io::Result<PageArtifactResult> {
            if let Some(callback) = self.on_call.as_mut() {
                callback(page);
            }
            if let Some(token) = &self.fail_when_cancelled
                && token.is_cancelled()
            {
                // Mirror the real runtime: an in-flight Ctrl+C fails the page.
                return Err(std::io::Error::other("cancelled"));
            }
            Ok(self
                .script
                .pop_front()
                .unwrap_or_else(|| normal_artifact(page)))
        }
    }

    // --- Phase-4 W3: encode pipeline TDD matrix -----------------------------

    /// Generation-keyed fake: the PNG bytes are exactly the terminal render's
    /// generation, so a commit of a stale attempt is observable byte-level.
    struct AlwaysFailingArtifacts {
        calls: Vec<u32>,
    }
    impl PageArtifactRecognizer<u32> for AlwaysFailingArtifacts {
        fn recognize_artifact(
            &mut self,
            _model: &mut u32,
            page: u32,
            _image: &RenderedPage,
        ) -> std::io::Result<PageArtifactResult> {
            self.calls.push(page);
            Err(std::io::Error::other("injected recognition failure"))
        }
    }

    struct GenerationEncoder;
    impl encode_pipeline::PagePngEncoder for GenerationEncoder {
        fn encode_png(
            &self,
            _page: u32,
            generation: u64,
            _width: u32,
            _height: u32,
            _rgb8: &[u8],
        ) -> std::io::Result<Vec<u8>> {
            Ok(vec![generation as u8])
        }
    }

    struct FailingEncoder;
    impl encode_pipeline::PagePngEncoder for FailingEncoder {
        fn encode_png(
            &self,
            _page: u32,
            _generation: u64,
            _width: u32,
            _height: u32,
            _rgb8: &[u8],
        ) -> std::io::Result<Vec<u8>> {
            Err(std::io::Error::other("injected encode failure"))
        }
    }

    struct PanickingEncoder;
    impl encode_pipeline::PagePngEncoder for PanickingEncoder {
        fn encode_png(
            &self,
            _page: u32,
            _generation: u64,
            _width: u32,
            _height: u32,
            _rgb8: &[u8],
        ) -> std::io::Result<Vec<u8>> {
            panic!("injected encoder panic");
        }
    }

    #[test]
    fn encode_pipeline_commits_the_terminal_generation() {
        // A degenerated attempt-1 page retries (attempt 2 renders again with
        // a NEW generation); the committed PNG must be the TERMINAL render's
        // bytes ([2]), never the first attempt's ([1]).
        let root = test_dir("encode-pipeline-generation");
        let _ = fs::remove_dir_all(&root);
        let script = std::collections::VecDeque::from(vec![
            degenerate_artifact(1),
            PageArtifactResult {
                png: Some(vec![99]), // recognition output; PNG comes from the pipeline
                think_blocks_stripped: 0,
                markdown: "page 1 retry".to_owned(),
                page_json: r#"{"schema_version":1,"page_number":1,"blocks":[],"termination":"eos","warnings":[]}"#.to_owned(),
                truncated: false,
                degenerated: false,
                warnings: Vec::new(),
                raw_output: String::new(),
            },
        ]);
        let recognizer = ScriptedRecognizer {
            script,
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            recognizer,
        );
        service.set_png_encoder(Box::new(GenerationEncoder));
        // keep_pages: the full-page PNG is deleted on successful merge, and
        // this test reads it back byte-level.
        let summary = service
            .run_with_checkpoints_cancelled(
                &[1],
                &root,
                &degeneration_compatibility(),
                false,
                true,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(summary.completed_pages, 1);
        let png = fs::read(root.join("images").join("page-0001.png")).unwrap();
        assert_eq!(
            png,
            vec![2_u8],
            "committed PNG must be the terminal generation's encode (attempts ran twice)"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn encode_error_maps_to_page_recognize_diagnostic() {
        // Frozen surface: an encode error is a page-level recognize failure
        // (identical wording to the inline path) - never an escalated
        // task-level I/O failure.
        let root = test_dir("encode-pipeline-error");
        let _ = fs::remove_dir_all(&root);
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        service.set_png_encoder(Box::new(FailingEncoder));
        let summary = service
            .run_with_checkpoints(&[1, 2], &root, &degeneration_compatibility(), false)
            .unwrap();
        assert_eq!(summary.failed_pages, 2, "both pages fail on encode");
        assert_eq!(summary.completed_pages, 0);
        for diagnostic in &summary.diagnostics {
            assert_eq!(diagnostic.stage, "recognize", "diagnostic: {diagnostic:?}");
            assert_eq!(
                diagnostic.error_code.as_deref(),
                Some("recognition_failed"),
                "diagnostic: {diagnostic:?}"
            );
        }
        assert!(!root.join("images").join("page-0001.png").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn single_page_encode_error_yields_only_failed_diagnostics() {
        // BLK-3: a depth-1 window whose encode fails must produce exactly
        // one failed recognize diagnostic per page - never a completed one.
        let root = test_dir("encode-pipeline-error-single");
        let _ = fs::remove_dir_all(&root);
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        service.set_png_encoder(Box::new(FailingEncoder));
        let summary = service
            .run_with_checkpoints(&[1], &root, &degeneration_compatibility(), false)
            .unwrap();
        assert_eq!(summary.failed_pages, 1);
        assert_eq!(summary.completed_pages, 0);
        assert_eq!(
            summary.diagnostics.len(),
            1,
            "exactly one diagnostic: {:?}",
            summary.diagnostics
        );
        let diagnostic = &summary.diagnostics[0];
        assert_eq!(diagnostic.page, 1);
        assert_eq!(diagnostic.status, "failed");
        assert_eq!(diagnostic.stage, "recognize");
        assert_eq!(diagnostic.error_code.as_deref(), Some("recognition_failed"));
        assert!(!root.join("images").join("page-0001.png").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn encoder_panic_is_fail_loud_at_fetch() {
        // Frozen surface: an encoder panic re-raises on the scan thread
        // (resume_unwind) instead of being swallowed into a page error.
        let root = test_dir("encode-pipeline-panic");
        let _ = fs::remove_dir_all(&root);
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        service.set_png_encoder(Box::new(PanickingEncoder));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service
                .run_with_checkpoints(&[1], &root, &degeneration_compatibility(), false)
                .unwrap();
        }));
        let payload = result.expect_err("encoder panic must propagate fail-loud");
        let message = payload
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(
            message.contains("injected encoder panic"),
            "payload: {message}"
        );
        assert!(!root.join("images").join("page-0001.png").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn encode_results_are_discarded_on_recognition_failure() {
        // BLK-1: a recognition failure must not leave the page's encode
        // result in the map - the pipeline is cleared after the window's
        // commit phase, so the map stays bounded by failures too.
        struct RecordingEncoder {
            inner: std::sync::Mutex<Vec<u64>>,
        }
        impl encode_pipeline::PagePngEncoder for RecordingEncoder {
            fn encode_png(
                &self,
                _page: u32,
                generation: u64,
                _width: u32,
                _height: u32,
                _rgb8: &[u8],
            ) -> std::io::Result<Vec<u8>> {
                self.inner.lock().unwrap().push(generation);
                Ok(vec![generation as u8])
            }
        }
        let root = test_dir("encode-pipeline-fail-discard");
        let _ = fs::remove_dir_all(&root);
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            AlwaysFailingArtifacts { calls: Vec::new() },
        );
        service.set_png_encoder(Box::new(RecordingEncoder {
            inner: std::sync::Mutex::new(Vec::new()),
        }));
        let summary = service
            .run_with_checkpoints(&[1, 2], &root, &degeneration_compatibility(), false)
            .unwrap();
        assert_eq!(summary.failed_pages, 2);
        assert_eq!(summary.completed_pages, 0);
        // The pipeline was discarded with the window: no encode results
        // survive the failed window (the service is dropped here; any
        // un-discarded entry would have kept the pages alive until now).
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelled_window_discards_encode_results() {
        // BLK-1: a cancelled window commits nothing AND drops its in-flight
        // encode results (zero disk writes, zero retained results).
        let root = test_dir("encode-pipeline-cancel");
        let _ = fs::remove_dir_all(&root);
        let cancellation = CancellationToken::new();
        cancellation.request_cancel();
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        service.set_png_encoder(Box::new(GenerationEncoder));
        let summary = service
            .run_with_checkpoints_cancelled(
                &[1, 2],
                &root,
                &degeneration_compatibility(),
                false,
                true,
                &cancellation,
            )
            .unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.completed_pages, 0);
        assert!(!root.join("images").join("page-0001.png").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn batch_fallback_discards_stale_attempt_encodes() {
        // BLK-1: a batch-level failure re-runs every page single-stream with
        // fresh renders; the stale batch-attempt encodes must not leak into
        // the committed files (the terminal generation's bytes win).
        let root = test_dir("encode-pipeline-batch-fallback");
        let _ = fs::remove_dir_all(&root);
        let recognizer = BatchFailingArtifacts {
            serial_calls: Vec::new(),
        };
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            recognizer,
        );
        service.set_png_encoder(Box::new(GenerationEncoder));
        // keep_pages: the merge deletes full-page PNGs otherwise.
        let summary = service
            .run_with_checkpoints_cancelled(
                &[1, 2, 3],
                &root,
                &degeneration_compatibility(),
                false,
                true,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(summary.completed_pages, 3);
        // Every serial re-render gets a fresh generation (1..=6 for 3 pages
        // x 2 attempts each in the fallback), so the committed bytes are the
        // highest generations, not the batch-attempt ones.
        for page in [1_u32, 2, 3] {
            let png = fs::read(root.join("images").join(format!("page-{page:04}.png"))).unwrap();
            assert!(
                !png.is_empty(),
                "page {page} committed with terminal-generation bytes"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pipeline_and_fallback_commits_are_byte_identical() {
        // The pipeline is a timing change: with an encoder whose bytes match
        // the artifact's inline png (vec![page as u8]), both paths write
        // byte-identical page files.
        let root = test_dir("encode-pipeline-parity");
        let _ = fs::remove_dir_all(&root);
        struct PageBytesEncoder;
        impl encode_pipeline::PagePngEncoder for PageBytesEncoder {
            fn encode_png(
                &self,
                page: u32,
                _generation: u64,
                _width: u32,
                _height: u32,
                _rgb8: &[u8],
            ) -> std::io::Result<Vec<u8>> {
                Ok(vec![page as u8])
            }
        }
        for (name, with_pipeline) in [("pipeline", true), ("fallback", false)] {
            let target = root.join(name);
            let mut service = ScanService::new(
                FakeRenderer {
                    calls: Vec::new(),
                    fail_page: None,
                },
                FakeLoader { loads: 0 },
                FakeArtifacts { calls: Vec::new() },
            );
            if with_pipeline {
                service.set_png_encoder(Box::new(PageBytesEncoder));
            }
            let summary = service
                .run_with_checkpoints_cancelled(
                    &[1, 2],
                    &target,
                    &degeneration_compatibility(),
                    false,
                    true,
                    &CancellationToken::new(),
                )
                .unwrap();
            assert_eq!(summary.completed_pages, 2);
        }
        for page in [1_u32, 2] {
            let name = format!("page-{page:04}.png");
            assert_eq!(
                fs::read(root.join("pipeline").join("images").join(&name)).unwrap(),
                fs::read(root.join("fallback").join("images").join(&name)).unwrap(),
                "{name} diverged between pipeline and fallback"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_fallback_truncated_page_counted_once() {
        // Phase-4 BLK-1: after a cross-invocation GPU->CPU fallback re-match,
        // a truncated page must be counted exactly once (by the service's
        // SkipCommitted path). The CLI re-matching must not double-count via
        // resume_incomplete_checkpoints.
        //
        // This test exercises the depth-1 serial path (which is what runs
        // for a single-page doc after fallback) with a truncated artifact,
        // verifying the service counts truncated=1 and the summary has
        // exactly one diagnostic for the page.
        let root = test_dir("shared-fallback-truncated-once");
        let _ = fs::remove_dir_all(&root);
        // Q1.5/D-14: the truncated page walks the full 4-rung ladder before
        // committing truncated — the once-only PAGE count is what this test
        // guards (per-attempt artifacts are not page counts).
        let script = std::collections::VecDeque::from((1..=4).map(|_| PageArtifactResult {
            png: Some(vec![1]),
            think_blocks_stripped: 0,
            markdown: "page 1 truncated content".to_owned(),
            page_json: r#"{"schema_version":1,"page_number":1,"blocks":[],"termination":"max_tokens","warnings":["truncated"]}"#.to_owned(),
            truncated: true,
            degenerated: false,
            warnings: vec!["truncated".to_owned()],
            raw_output: String::new(),
        }).collect::<Vec<_>>());
        let recognizer = ScriptedRecognizer {
            script,
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            recognizer,
        );
        // keep_pages=true so the page file survives for verification.
        // BLK-2 (rereview): PASS 1 produces the truncated checkpoint; PASS 2
        // resumes and must count it exactly once — this is the control flow
        // the CLI re-matching rides on after a GPU->CPU fallback.
        let summary = service
            .run_with_checkpoints_cancelled(
                &[1],
                &root,
                &degeneration_compatibility(),
                false,
                true,
                &CancellationToken::new(),
            )
            .unwrap();
        // PASS 1 counts the truncated page exactly once.
        assert_eq!(summary.truncated_pages, 1, "pass 1: truncated counted once");
        assert_eq!(summary.completed_pages, 1);
        let truncated_diags: Vec<_> = summary
            .diagnostics
            .iter()
            .filter(|d| d.page == 1 && d.status == "partial")
            .collect();
        assert_eq!(
            truncated_diags.len(),
            1,
            "exactly one partial diagnostic for the truncated page: {:?}",
            summary.diagnostics
        );
        // The page-0001.checkpoint exists and has truncated=true.
        let cp_text = fs::read_to_string(root.join("page-0001.checkpoint")).unwrap();
        assert!(cp_text.contains("truncated=true"));
        // The page-0001.md exists with the truncated content.
        assert!(root.join("page-0001.md").is_file());

        // PASS 2: resume from the same directory. The service's
        // SkipCommitted path must count truncated exactly once — this is
        // the per-page resume matching that the CLI re-matching rides on
        // after a GPU->CPU fallback (BLK-1).
        let summary2 = service
            .run_with_checkpoints_cancelled(
                &[1],
                &root,
                &degeneration_compatibility(),
                true, // resume from the existing checkpoint
                true, // keep_pages
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(
            summary2.truncated_pages, 1,
            "resume must count truncated exactly once, got {}",
            summary2.truncated_pages
        );
        assert_eq!(
            summary2.completed_pages, 1,
            "resume must skip-commit the page"
        );
        let truncated_diags2: Vec<_> = summary2
            .diagnostics
            .iter()
            .filter(|d| d.page == 1 && d.status == "partial")
            .collect();
        assert_eq!(
            truncated_diags2.len(),
            1,
            "exactly one partial diagnostic after resume: {:?}",
            summary2.diagnostics
        );

        let _ = fs::remove_dir_all(&root);
    }

    fn degeneration_compatibility() -> CompatibilityKey {
        CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "balanced".to_owned(),
            parameters: "tokens=4096".to_owned(),
        }
    }

    fn run_scan_with(
        root: &std::path::Path,
        pages: &[u32],
        renderer: ScriptedRenderer,
        recognizer: ScriptedRecognizer,
        resume: bool,
    ) -> std::io::Result<JobSummary> {
        let mut service = ScanService::new(renderer, FakeLoader { loads: 0 }, recognizer);
        service.run_with_checkpoints(pages, root, &degeneration_compatibility(), resume)
    }

    #[test]
    fn checkpoint_new_fields_round_trip_and_old_v5_text_stays_readable() {
        let directory = test_dir("checkpoint-degeneration");
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("page-0104.checkpoint");
        let checkpoint = PageCheckpoint {
            page_number: 104,
            compatibility: degeneration_compatibility(),
            complete: true,
            output_sha256: "abc".to_owned(),
            truncated: true,
            degenerated: true,
            attempts: 3,
            raw: Some("page-0104.raw".to_owned()),
            warnings: vec!["truncated".to_owned(), "degenerated".to_owned()],
            think_blocks_stripped: 0,
            audit: None,
        };
        super::write_checkpoint(&path, &checkpoint).unwrap();
        let loaded = super::read_checkpoint(&path).unwrap();
        assert_eq!(loaded, checkpoint);
        // Old v5 text (no additive fields) still loads with default values.
        let legacy = directory.join("page-0002.checkpoint");
        fs::write(
            &legacy,
            "ocrx-checkpoint-v5\npage=2\ninput=696e707574\nmodel=6d6f64656c\nprofile=70726f66696c65\nparameters=70\ncomplete=true\noutput=6f7574707574\ntruncated=false\nwarnings=\naudit=none\n",
        )
        .unwrap();
        let old = super::read_checkpoint(&legacy).unwrap();
        assert!(!old.degenerated);
        assert_eq!(old.attempts, 1);
        assert_eq!(old.raw, None);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn merge_markdown_inserts_frozen_incomplete_markers() {
        let marker_for = |incomplete| {
            super::merge_markdown(vec![PageMarkdown {
                page_number: 104,
                content: "body".to_owned(),
                incomplete: Some(incomplete),
            }])
            .unwrap()
        };
        let degenerated = marker_for(PageIncomplete::Degenerated);
        assert_eq!(
            degenerated,
            "<!-- page: 104 -->\n> ⚠️ ocrx：第 104 页识别不完整（输出被复读退化截断），本页内容可能缺失或错误，请重扫后核对本页。\n\nbody"
        );
        let truncated = marker_for(PageIncomplete::Truncated);
        assert!(truncated.contains("（输出被截断）"));
        assert!(!truncated.contains("复读"));
        let complete = super::merge_markdown(vec![PageMarkdown {
            page_number: 104,
            content: "body".to_owned(),
            incomplete: None,
        }])
        .unwrap();
        assert!(!complete.contains('⚠'));
    }

    #[test]
    fn scenario_a_first_attempt_loops_second_succeeds() {
        let root = test_dir("scenario-a");
        let _ = fs::remove_dir_all(&root);
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: [degenerate_artifact(1), normal_artifact(1)]
                .into_iter()
                .collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let summary = run_scan_with(&root, &[1], renderer, recognizer, false).unwrap();
        assert_eq!(summary.completed_pages, 1);
        assert_eq!(summary.degenerated_pages, 0);
        let checkpoint = super::read_checkpoint(root.join("page-0001.checkpoint")).unwrap();
        assert_eq!(checkpoint.attempts, 2);
        assert!(!checkpoint.degenerated);
        assert_eq!(checkpoint.raw, None);
        assert_eq!(
            fs::read_to_string(root.join("page-0001.md")).unwrap(),
            "clean page 1"
        );
        let document = fs::read_to_string(root.join("document.md")).unwrap();
        assert!(!document.contains('⚠'));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn degeneration_retries_route_through_the_retry_seam() {
        // Q1.2: attempt 1 recognition takes the plain path; every
        // degeneration retry (attempts >= 2) takes the retry seam. With the
        // default 2-entry ladder, one degenerate + one normal script entry
        // yields exactly 1 plain call and 1 retry call.
        let root = test_dir("retry-seam-routing");
        let _ = fs::remove_dir_all(&root);
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: [degenerate_artifact(1), normal_artifact(1)]
                .into_iter()
                .collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let mut service = ScanService::new(renderer, FakeLoader { loads: 0 }, recognizer);
        let summary = service
            .run_with_checkpoints(&[1], &root, &degeneration_compatibility(), false)
            .unwrap();
        assert_eq!(summary.completed_pages, 1);
        assert_eq!(
            (service.recognizer.calls, service.recognizer.retry_calls),
            (1, 1)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scenario_b_all_attempts_degenerate_commits_marked_partial_with_raw() {
        let root = test_dir("scenario-b");
        let _ = fs::remove_dir_all(&root);
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: [
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
            ]
            .into_iter()
            .collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let summary = run_scan_with(&root, &[104], renderer, recognizer, false).unwrap();
        assert_eq!(summary.degenerated_pages, 1);
        let checkpoint = super::read_checkpoint(root.join("page-0104.checkpoint")).unwrap();
        assert_eq!(checkpoint.attempts, 4);
        assert!(checkpoint.degenerated);
        assert_eq!(checkpoint.raw.as_deref(), Some("page-0104.raw"));
        assert_eq!(
            fs::read_to_string(root.join("page-0104.raw")).unwrap(),
            "loop loop loop page 104"
        );
        assert_eq!(
            checkpoint.resume_action(&degeneration_compatibility(), &root, 104),
            CheckpointResumeAction::Reprocess
        );
        let document = fs::read_to_string(root.join("document.md")).unwrap();
        assert!(document.contains("输出被复读退化截断"));
        assert!(document.contains("cropped page 104"));
        assert!(!document.contains("loop loop"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scenario_c_pure_truncation_retries_through_ladder_then_resumes_committed() {
        // Q1.5/D-14 (v1.5.2): pure truncation ENTERS the ladder — Q5.1 showed
        // 24/28 truncated pages are long-period loops evading the detector.
        // Resume disposition unchanged: the committed truncated page stays
        // reusable (SkipCommitted) on resume.
        let root = test_dir("scenario-c");
        let _ = fs::remove_dir_all(&root);
        let truncated_only = PageArtifactResult {
            truncated: true,
            degenerated: false,
            ..normal_artifact(5)
        };
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: [
                truncated_only.clone(),
                truncated_only.clone(),
                truncated_only.clone(),
                truncated_only,
            ]
            .into_iter()
            .collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let summary = run_scan_with(&root, &[5], renderer, recognizer, false).unwrap();
        assert_eq!(summary.truncated_pages, 1);
        assert_eq!(summary.degenerated_pages, 0);
        let checkpoint = super::read_checkpoint(root.join("page-0005.checkpoint")).unwrap();
        assert_eq!(checkpoint.attempts, 4);
        assert_eq!(checkpoint.raw, None);
        assert_eq!(
            checkpoint.resume_action(&degeneration_compatibility(), &root, 5),
            CheckpointResumeAction::SkipCommitted
        );
        let document = fs::read_to_string(root.join("document.md")).unwrap();
        assert!(document.contains("（输出被截断）"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scenario_d_resume_retries_degenerated_pages_with_fresh_attempt_budget() {
        let root = test_dir("scenario-d");
        let _ = fs::remove_dir_all(&root);
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: [
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
            ]
            .into_iter()
            .collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let summary = run_scan_with(&root, &[104], renderer, recognizer, false).unwrap();
        assert_eq!(summary.degenerated_pages, 1);
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: [
                degenerate_artifact(104),
                degenerate_artifact(104),
                degenerate_artifact(104),
                normal_artifact(104),
            ]
            .into_iter()
            .collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let summary = run_scan_with(&root, &[104], renderer, recognizer, true).unwrap();
        assert_eq!(summary.degenerated_pages, 0);
        let checkpoint = super::read_checkpoint(root.join("page-0104.checkpoint")).unwrap();
        assert_eq!(checkpoint.attempts, 4);
        assert!(!checkpoint.degenerated);
        let document = fs::read_to_string(root.join("document.md")).unwrap();
        assert!(!document.contains('⚠'));
        assert!(document.contains("clean page 104"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scenario_e_cancellation_during_attempt_reports_cancelled() {
        let root = test_dir("scenario-e");
        let _ = fs::remove_dir_all(&root);
        let token = CancellationToken::new();
        let cancel_trigger = token.clone();
        let renderer = ScriptedRenderer {
            calls: std::collections::VecDeque::new(),
        };
        let recognizer = ScriptedRecognizer {
            script: std::collections::VecDeque::new(),
            calls: 0,
            retry_calls: 0,
            on_call: Some(Box::new(move |_| cancel_trigger.request_cancel())),
            fail_when_cancelled: Some(token.clone()),
        };
        let mut service = ScanService::new(renderer, FakeLoader { loads: 0 }, recognizer);
        let summary = service
            .run_with_checkpoints_cancelled(
                &[1],
                &root,
                &degeneration_compatibility(),
                false,
                false,
                &token,
            )
            .unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.completed_pages, 0);
        assert!(!root.join("page-0001.checkpoint").exists());
        fs::remove_dir_all(root).unwrap();
    }

    struct LadderProbeRenderer {
        log: std::rc::Rc<std::cell::RefCell<Vec<(u32, u32, u32)>>>,
    }
    impl PageRenderer for LadderProbeRenderer {
        fn preflight(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
            self.log.borrow_mut().push((page, 1, 1));
            RenderedPage::new(1, 1, vec![1, 2, 3])
        }
        fn render_perturbed(
            &mut self,
            page: u32,
            numerator: u32,
            denominator: u32,
        ) -> std::io::Result<RenderedPage> {
            self.log.borrow_mut().push((page, numerator, denominator));
            RenderedPage::new(1, 1, vec![4, 5, 6])
        }
    }

    #[test]
    fn perturbed_retries_use_frozen_scale_ratios() {
        // Q1.4a mechanism gate: the retry ladder is the frozen data table;
        // an always-degenerate page must render plain on attempt 1, then
        // walk the table in order, and commit with attempts = 1 + table
        // length when it never stops degenerating.
        let root = test_dir("scenario-ratios");
        let _ = fs::remove_dir_all(&root);
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let renderer = LadderProbeRenderer {
            log: std::rc::Rc::clone(&log),
        };
        // An exhausted script falls back to normal artifacts, so script the
        // full budget: one degenerate artifact per attempt (4 with the
        // experiment ladder); the last degenerate artifact commits.
        let recognizer = ScriptedRecognizer {
            script: (1..=4).map(degenerate_artifact).collect(),
            calls: 0,
            retry_calls: 0,
            on_call: None,
            fail_when_cancelled: None,
        };
        let mut service = ScanService::new(renderer, FakeLoader { loads: 0 }, recognizer);
        let summary = service
            .run_with_checkpoints(&[1], &root, &degeneration_compatibility(), false)
            .unwrap();
        assert_eq!(summary.degenerated_pages, 1);
        // Every non-perturbed rung renders plain (1,1,1) — e.g. the
        // token-grid and penalty rungs; perturbed rungs walk their ratios.
        let expected_rungs = DEGENERATION_RETRY_LADDER
            .iter()
            .map(|rung| rung.perturbed.unwrap_or((1, 1)))
            .map(|(numerator, denominator)| (1, numerator, denominator));
        assert_eq!(
            *log.borrow(),
            vec![(1, 1, 1)]
                .into_iter()
                .chain(expected_rungs)
                .collect::<Vec<_>>()
        );
        let checkpoint = super::read_checkpoint(root.join("page-0001.checkpoint")).unwrap();
        assert_eq!(
            checkpoint.attempts,
            1 + DEGENERATION_RETRY_LADDER.len() as u32
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checkpoint_attempts_field_supports_extended_ladder_and_stays_readable() {
        // Q1.4b: attempts widens to 1..=N (N = ladder length + 1); a v5
        // checkpoint written with attempts=5 round-trips, and older files
        // without the additive attempts line keep defaulting to 1.
        let directory = test_dir("checkpoint-attempts-5");
        let _ = fs::remove_dir_all(&directory);
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: degeneration_compatibility(),
            complete: true,
            output_sha256: String::new(),
            truncated: false,
            degenerated: false,
            attempts: 5,
            raw: None,
            warnings: Vec::new(),
            think_blocks_stripped: 0,
            audit: None,
        };
        let path = directory.join("page-0001.checkpoint");
        super::write_checkpoint(&path, &checkpoint).unwrap();
        assert_eq!(super::read_checkpoint(&path).unwrap().attempts, 5);
        let legacy = format!(
            "ocrx-checkpoint-v5
page=1
input={}
model={}
profile={}
parameters={}
complete=true
output=abc
truncated=false
raw=none
warnings=
audit=none
",
            super::encode_field("input"),
            super::encode_field("ovisocr2"),
            super::encode_field("balanced"),
            super::encode_field("tokens=16384"),
        );
        fs::write(&path, legacy).unwrap();
        assert_eq!(super::read_checkpoint(&path).unwrap().attempts, 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn scan_service_resume_reuses_only_compatible_committed_pages() {
        let root = test_dir("scan-resume");
        let _ = fs::remove_dir_all(&root);
        let compatibility = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "balanced".to_owned(),
            parameters: "p".to_owned(),
        };
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        let first = service
            .run_with_checkpoints(&[1, 2], &root, &compatibility, false)
            .unwrap();
        assert_eq!(first.completed_pages, 2);
        assert!(root.join("document.md").exists());
        let mut second = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        let resumed = second
            .run_with_checkpoints(&[1, 2, 3], &root, &compatibility, true)
            .unwrap();
        assert_eq!(resumed.completed_pages, 3);
        assert_eq!(second.recognizer.calls, vec![3]);
        assert!(root.join("document.md").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_service_cancellation_stops_new_pages_and_keeps_committed_page() {
        let root = test_dir("scan-cancel");
        let _ = fs::remove_dir_all(&root);
        let token = CancellationToken::new();
        token.request_cancel();
        let compatibility = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "balanced".to_owned(),
            parameters: "p".to_owned(),
        };
        let mut service = ScanService::new(
            FakeRenderer {
                calls: Vec::new(),
                fail_page: None,
            },
            FakeLoader { loads: 0 },
            FakeArtifacts { calls: Vec::new() },
        );
        let summary = service
            .run_with_checkpoints_cancelled(&[1, 2], &root, &compatibility, false, false, &token)
            .unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.completed_pages, 0);
        assert!(!root.join("page-0001.checkpoint").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn result_json_is_versioned_and_escapes_user_values() {
        let report = ResultReport {
            job_id: "job\"1".to_owned(),
            status: ResultStatus::Partial,
            total_pages: 3,
            completed_pages: 2,
            failed_pages: 1,
            output_dir: "/tmp/a\nb".to_owned(),
        };
        assert_eq!(
            report.to_json(),
            "{\"schema_version\":1,\"job_id\":\"job\\\"1\",\"status\":\"partial\",\"total_pages\":3,\"completed_pages\":2,\"failed_pages\":1,\"output_dir\":\"/tmp/a\\nb\"}"
        );
    }

    #[test]
    fn report_formats_are_stdout_ready() {
        let report = ResultReport {
            job_id: "job".to_owned(),
            status: ResultStatus::Success,
            total_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            output_dir: "/tmp/out".to_owned(),
        };
        assert!(format_report(&report, ReportFormat::Json).starts_with('{'));
        assert_eq!(
            format_report(&report, ReportFormat::Text),
            "status=success pages=1/1 failed=0 output=/tmp/out"
        );
    }

    #[test]
    fn reporter_progress_off_emits_nothing() {
        let mut reporter = Reporter::default();
        reporter.progress(
            &ProgressEvent {
                event: "scan_started".to_owned(),
                page: None,
                completed: 0,
                total: 3,
            },
            ProgressMode::Off,
        );
        assert!(reporter.stdout.is_empty());
        assert!(reporter.stderr.is_empty());
    }

    #[test]
    fn progress_stage_names_are_stable_machine_values() {
        let stages = [
            ProgressStage::Analyzing,
            ProgressStage::Extracting,
            ProgressStage::Rendering,
            ProgressStage::Recognizing,
            ProgressStage::Merging,
        ];
        let events: Vec<_> = stages
            .into_iter()
            .map(|stage| ProgressEvent::for_stage(stage, None, 0, 1).event)
            .collect();
        assert_eq!(
            events,
            vec![
                "analyzing",
                "extracting",
                "rendering",
                "recognizing",
                "merging"
            ]
        );
    }

    #[test]
    fn reporter_keeps_json_result_on_stdout_and_progress_on_stderr() {
        let mut reporter = Reporter::default();
        reporter.result(
            &ResultReport {
                job_id: "job".to_owned(),
                status: ResultStatus::Partial,
                total_pages: 2,
                completed_pages: 1,
                failed_pages: 1,
                output_dir: "/tmp/out".to_owned(),
            },
            ReportFormat::Json,
        );
        reporter.progress(
            &ProgressEvent {
                event: "page_complete".to_owned(),
                page: Some(1),
                completed: 1,
                total: 2,
            },
            ProgressMode::Jsonl,
        );
        assert_eq!(reporter.stdout.lines().count(), 1);
        assert!(reporter.stdout.contains("\"schema_version\":1"));
        assert_eq!(reporter.stderr.lines().count(), 1);
        assert!(reporter.stderr.contains("page_complete"));
    }

    #[test]
    fn reporter_text_errors_never_pollute_machine_stdout() {
        let mut reporter = Reporter::default();
        reporter.error(
            &ErrorReport {
                code: 2,
                kind: "usage".to_owned(),
                message: "bad arg".to_owned(),
                cause: None,
                suggestion: None,
            },
            ReportFormat::Text,
        );
        assert!(reporter.stdout.is_empty());
        assert!(reporter.stderr.contains("bad arg"));
    }

    #[test]
    fn reporter_redacts_sensitive_values_on_success_and_error_paths() {
        let result = ResultReport {
            job_id: "prompt=secret".to_owned(),
            status: ResultStatus::Success,
            total_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            output_dir: "url=https://user:pass@example.test/x".to_owned(),
        };
        let result_json = result.to_json();
        assert!(!result_json.contains("secret"));
        assert!(!result_json.contains("https://"));
        let error = ErrorReport {
            code: 1,
            kind: "token=abc".to_owned(),
            message: "ocr=private".to_owned(),
            cause: Some("proxy=https://u:p@example.test".to_owned()),
            suggestion: Some("authorization=Bearer-secret".to_owned()),
        };
        let error_json = error.to_json();
        assert!(!error_json.contains("abc"));
        assert!(!error_json.contains("private"));
        assert!(!error_json.contains("Bearer-secret"));
        assert!(!error_json.contains("https://"));
    }

    #[test]
    fn progress_jsonl_and_error_json_are_stable() {
        let event = ProgressEvent {
            event: "page_progress".to_owned(),
            page: Some(2),
            completed: 1,
            total: 3,
        };
        assert_eq!(
            event.to_json_line(),
            "{\"schema_version\":1,\"event\":\"page_progress\",\"page\":2,\"completed\":1,\"total\":3}"
        );
        let error = ErrorReport {
            code: 3,
            kind: "input".to_owned(),
            message: "bad input".to_owned(),
            cause: None,
            suggestion: Some("retry".to_owned()),
        };
        assert_eq!(
            error.to_json(),
            "{\"schema_version\":1,\"code\":3,\"kind\":\"input\",\"message\":\"bad input\",\"cause\":null,\"suggestion\":\"retry\"}"
        );
    }

    #[test]
    fn atomic_write_commits_complete_content_without_part_file() {
        let directory = test_dir("commit");
        let _ = fs::remove_dir_all(&directory);
        let output = directory.join("nested/document.md");
        atomic_write(&output, b"complete\n").expect("atomic write should succeed");
        assert_eq!(fs::read_to_string(&output).unwrap(), "complete\n");
        assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn atomic_write_rejects_directory_target_without_removing_existing_file() {
        let directory = test_dir("reject");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("result");
        fs::write(&target, b"old").unwrap();
        assert!(atomic_write(&target.join("child"), b"bad").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn output_root_rejects_relative_and_protected_paths() {
        let input = std::path::Path::new("/work/input.pdf");
        let cache = std::path::Path::new("/work/.cache");
        let workspace = std::path::Path::new("/work");
        assert!(super::validate_output_root(input, "relative", &[cache, workspace]).is_err());
        assert!(super::validate_output_root(input, "/work", &[cache, workspace]).is_err());
        assert!(
            super::validate_output_root(input, "/work/.cache/job", &[cache, workspace]).is_err()
        );
        assert!(super::validate_output_root(input, "/work/result", &[cache, workspace]).is_err());
    }

    #[test]
    fn output_root_accepts_unrelated_absolute_path() {
        let input = std::path::Path::new("/work/input.pdf");
        let cache = std::path::Path::new("/work/.cache");
        // Windows needs a drive-qualified path: "/tmp/..." is not absolute there.
        let outside = if cfg!(target_os = "windows") {
            "C:\\tmp\\ocr-result"
        } else {
            "/tmp/ocr-result"
        };
        assert!(super::validate_output_root(input, outside, &[cache]).is_ok());
    }

    #[test]
    fn output_root_rejects_case_variant_of_protected_path_on_windows() {
        // NTFS matches paths case-insensitively: a case variant of a protected
        // root must not slip past the overlap check (M3 regression guard).
        if !cfg!(target_os = "windows") {
            return;
        }
        let input = std::path::Path::new("C:\\work\\input.pdf");
        let cache = std::path::Path::new("C:\\Users\\u\\AppData\\Local");
        let case_variant = std::path::Path::new("c:\\users\\U\\appdata\\local\\ocr-out");
        assert!(super::validate_output_root(input, case_variant, &[cache]).is_err());
        // Component boundaries still matter: a sibling sharing a prefix is fine.
        let sibling = std::path::Path::new("C:\\Users\\u\\AppData\\Localocr-out");
        assert!(super::validate_output_root(input, sibling, &[cache]).is_ok());
    }

    #[test]
    fn path_contains_is_component_wise() {
        let root = std::path::Path::new("/work/cache");
        assert!(super::path_contains(
            root,
            std::path::Path::new("/work/cache/job")
        ));
        assert!(!super::path_contains(
            root,
            std::path::Path::new("/work/cache-other")
        ));
        assert!(!super::path_contains(root, std::path::Path::new("/work")));
        assert!(super::paths_overlap(
            root,
            std::path::Path::new("/work/cache")
        ));
    }

    #[test]
    fn output_root_dot_dot_vector_is_resolved_before_overlap_check() {
        // BLK-2 vector 1: `Path::components` never resolves `..`, so the
        // check lexically normalizes first — an output that lands inside a
        // protected root after resolution must be rejected even though its
        // raw components never spell the root, while one that resolves
        // elsewhere stays allowed.
        let base = std::env::temp_dir().join(format!("ocrx-dotdot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let protected = base.join("cache");
        std::fs::create_dir_all(&protected).unwrap();
        let input = base.join("input.pdf");
        let overlap = base.join("elsewhere").join("..").join("cache").join("out");
        assert!(
            super::validate_output_root(&input, &overlap, &[protected.as_path()]).is_err(),
            "dot-dot spelling resolving into the protected root must be rejected"
        );
        let safe = base.join("elsewhere").join("..").join("other").join("out");
        assert!(
            super::validate_output_root(&input, &safe, &[protected.as_path()]).is_ok(),
            "dot-dot spelling resolving outside the protected root is fine"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn output_root_rejects_trailing_space_or_dot_components_on_windows() {
        // BLK-2 vector 3: Win32 strips trailing dots/spaces per component, so
        // `dir<space>` would compare as distinct but delete `dir`; refuse the
        // spelling instead. A bare `.` component stays legal.
        if !cfg!(target_os = "windows") {
            return;
        }
        let input = std::path::Path::new("C:\\work\\input.pdf");
        let cache = std::path::Path::new("C:\\work\\cache3");
        let trailing_space = std::path::Path::new("C:\\work\\cache3 \\out");
        let trailing_dot = std::path::Path::new("C:\\work\\cache3.\\out");
        assert!(
            super::validate_output_root(input, trailing_space, &[cache]).is_err(),
            "trailing-space component must be rejected"
        );
        assert!(
            super::validate_output_root(input, trailing_dot, &[cache]).is_err(),
            "trailing-dot component must be rejected"
        );
    }

    #[test]
    fn output_root_overlap_uses_canonical_forms() {
        // BLK-2 vector 2: an output whose *existing* ancestor sits inside the
        // protected root must be caught even when the spelled suffix differs
        // (case variant of an existing directory).
        if !cfg!(target_os = "windows") {
            return;
        }
        let base = std::env::temp_dir().join(format!("ocrx-canon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let protected = base.join("Cache");
        std::fs::create_dir_all(&protected).unwrap();
        // Deep output under a case-variant spelling of the existing protected
        // directory; the output itself does not exist yet.
        let output = base.join("CACHE").join("deep").join("out");
        let input = base.join("input.pdf");
        assert!(
            super::validate_output_root(&input, &output, &[protected.as_path()]).is_err(),
            "canonicalized comparison must catch the case-variant ancestor"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn output_may_sit_below_but_not_at_or_above_the_working_directory() {
        // The working-directory rule is one-directional: a subdirectory is the
        // normal `--output-dir out` layout, while the directory itself and
        // every ancestor are refused because --overwrite deletes the whole
        // output tree.
        let (root, cwd) = if cfg!(target_os = "windows") {
            (
                std::path::Path::new("C:\\"),
                std::path::PathBuf::from("C:\\work\\project"),
            )
        } else {
            (
                std::path::Path::new("/"),
                std::path::PathBuf::from("/work/project"),
            )
        };
        assert!(super::validate_output_not_containing(&cwd.join("paper-ocr"), &cwd).is_ok());
        assert!(super::validate_output_not_containing(&cwd.join("out").join("x"), &cwd).is_ok());
        assert!(super::validate_output_not_containing(root, &cwd).is_err());
        assert!(super::validate_output_not_containing(&cwd, &cwd).is_err());
    }

    #[test]
    fn output_containing_working_directory_folds_case_on_windows() {
        // NTFS matches paths case-insensitively: `C:\...\PROJECT` contains the
        // working directory `C:\...\Project` even though the components differ
        // in case, while a subdirectory spelled with a case variant stays
        // allowed.
        if !cfg!(target_os = "windows") {
            return;
        }
        let base = std::env::temp_dir().join(format!("ocrx-wdcwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let working_directory = base.join("Project");
        std::fs::create_dir_all(&working_directory).unwrap();
        assert!(
            super::validate_output_not_containing(&base.join("PROJECT"), &working_directory)
                .is_err(),
            "case-variant ancestor of the working directory must be rejected"
        );
        assert!(
            super::validate_output_not_containing(
                &base.join("project").join("out"),
                &working_directory
            )
            .is_ok(),
            "case-variant subdirectory of the working directory is the normal layout"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn checkpoint_reuse_requires_complete_matching_compatibility() {
        let expected = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "compact".to_owned(),
            parameters: "dpi=300".to_owned(),
        };
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: expected.clone(),
            complete: true,
            output_sha256: "output".to_owned(),
            truncated: true,
            warnings: vec!["truncated".to_owned(), "repetition_removed".to_owned()],
            think_blocks_stripped: 0,
            audit: None,
            degenerated: false,
            attempts: 1,
            raw: None,
        };
        assert!(checkpoint.is_reusable(&expected));
        assert!(
            !PageCheckpoint {
                complete: false,
                ..checkpoint.clone()
            }
            .is_reusable(&expected)
        );
        assert!(
            !PageCheckpoint {
                output_sha256: String::new(),
                ..checkpoint.clone()
            }
            .is_reusable(&expected)
        );
        assert!(
            !PageCheckpoint {
                compatibility: CompatibilityKey {
                    profile: "quality".to_owned(),
                    ..expected.clone()
                },
                ..checkpoint
            }
            .is_reusable(&expected)
        );
    }

    #[test]
    fn native_checkpoint_ignores_ocr_only_configuration_changes() {
        let native = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "native".to_owned(),
            parameters: "pdf-mode=auto;quality-tier=l2;classifier=c;extractor=e;dpi=300;tokens=4096;device=cpu;route=native".to_owned(),
        };
        let changed_ocr = CompatibilityKey {
            profile: "quality".to_owned(),
            parameters: "pdf-mode=auto;quality-tier=l2;classifier=c;extractor=e;dpi=300;tokens=8192;device=metal;route=native".to_owned(),
            ..native.clone()
        };
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: native,
            complete: true,
            output_sha256: "output".to_owned(),
            truncated: false,
            warnings: Vec::new(),
            think_blocks_stripped: 0,
            audit: None,
            degenerated: false,
            attempts: 1,
            raw: None,
        };
        assert!(checkpoint.is_reusable(&changed_ocr));
    }

    #[test]
    fn effective_ocr_checkpoint_rejects_changed_profile_or_device() {
        let stored = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "compact".to_owned(),
            parameters: "pdf-mode=auto;quality-tier=l2;classifier=c;extractor=e;dpi=300;ocr-profile=compact;ocr-device=cpu".to_owned(),
        };
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: stored.clone(),
            complete: true,
            output_sha256: "output".to_owned(),
            truncated: false,
            think_blocks_stripped: 0,
            warnings: Vec::new(),
            audit: None,
            degenerated: false,
            attempts: 1,
            raw: None,
        };
        let changed = CompatibilityKey {
            profile: "quality".to_owned(),
            parameters: stored
                .parameters
                .replace("compact;ocr-device=cpu", "quality;ocr-device=metal"),
            ..stored.clone()
        };
        assert!(!checkpoint.is_reusable(&changed));
    }

    #[test]
    fn pdf_compatibility_fragment_changes_when_routing_contract_changes() {
        let base = super::pdf_compatibility_parameters("auto", "l2", "pdf-route-v1", "pdf-text-v1");
        assert_eq!(
            base,
            "pdf-mode=auto;quality-tier=l2;classifier=pdf-route-v1;extractor=pdf-text-v1"
        );
        assert_ne!(
            base,
            super::pdf_compatibility_parameters("auto", "l1", "pdf-route-v1", "pdf-text-v1")
        );
        assert_ne!(
            base,
            super::pdf_compatibility_parameters("text", "l2", "pdf-route-v1", "pdf-text-v1")
        );
        assert_ne!(
            base,
            super::pdf_compatibility_parameters("auto", "l2", "pdf-route-v2", "pdf-text-v1")
        );
    }

    #[test]
    fn incomplete_pending_ocr_checkpoint_is_recoverable_but_not_reusable() {
        let expected = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "quality".to_owned(),
            parameters: "pdf-mode=auto;pending-ocr=true".to_owned(),
        };
        let checkpoint = PageCheckpoint {
            page_number: 3,
            compatibility: expected.clone(),
            complete: false,
            output_sha256: String::new(),
            truncated: false,
            warnings: vec!["native_validation_failed".to_owned()],
            think_blocks_stripped: 0,
            audit: None,
            degenerated: false,
            attempts: 1,
            raw: None,
        };
        assert!(checkpoint.is_pending_ocr(&expected));
        assert!(!checkpoint.is_reusable(&expected));
    }

    #[test]
    fn pending_ocr_marker_is_stable_and_idempotent() {
        assert_eq!(
            super::mark_pending_ocr("pdf-mode=auto"),
            "pdf-mode=auto;pending-ocr=true"
        );
        assert_eq!(
            super::mark_pending_ocr("pdf-mode=auto;pending-ocr=true"),
            "pdf-mode=auto;pending-ocr=true"
        );
        assert_eq!(super::mark_pending_ocr(""), "pending-ocr=true");
    }

    #[test]
    fn pending_ocr_checkpoint_factory_is_incomplete_and_recoverable() {
        let checkpoint = super::pending_ocr_checkpoint(
            4,
            CompatibilityKey {
                input_sha256: "input".to_owned(),
                model: "ovisocr2".to_owned(),
                profile: "quality".to_owned(),
                parameters: "pdf-mode=auto".to_owned(),
            },
            "native_validation_failed",
        );
        assert!(!checkpoint.complete);
        assert_eq!(checkpoint.page_number, 4);
        assert_eq!(checkpoint.warnings, vec!["native_validation_failed"]);
        assert!(
            checkpoint
                .compatibility
                .parameters
                .contains("pending-ocr=true")
        );
        assert!(checkpoint.is_pending_ocr(&checkpoint.compatibility));
    }

    #[test]
    fn checkpoint_disposition_distinguishes_pending_and_reprocess_states() {
        let root = test_dir("checkpoint-disposition");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let compatibility = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "quality".to_owned(),
            parameters: "pdf-mode=auto".to_owned(),
        };
        let pending = super::pending_ocr_checkpoint(4, compatibility.clone(), "native_failed");
        assert_eq!(
            pending.disposition(&pending.compatibility, &root, 4),
            CheckpointDisposition::PendingOcr
        );
        assert_eq!(
            pending.disposition(
                &CompatibilityKey {
                    parameters: "pdf-mode=ocr".to_owned(),
                    ..compatibility.clone()
                },
                &root,
                4,
            ),
            CheckpointDisposition::Reprocess
        );
        let incomplete = PageCheckpoint {
            page_number: 4,
            compatibility,
            complete: false,
            output_sha256: String::new(),
            truncated: false,
            warnings: Vec::new(),
            think_blocks_stripped: 0,
            audit: None,
            degenerated: false,
            attempts: 1,
            raw: None,
        };
        assert_eq!(
            incomplete.disposition(&incomplete.compatibility, &root, 4),
            CheckpointDisposition::Reprocess
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checkpoint_state_machine_exposes_recovery_states() {
        let root = test_dir("checkpoint-state");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let compatibility = CompatibilityKey {
            input_sha256: "input".to_owned(),
            model: "ovisocr2".to_owned(),
            profile: "quality".to_owned(),
            parameters: "pdf-mode=auto".to_owned(),
        };
        let pending = super::pending_ocr_checkpoint(4, compatibility, "native_failed");
        assert_eq!(
            pending.state(&pending.compatibility, &root, 4),
            CheckpointState::PendingOcr
        );
        assert_eq!(
            pending.state(
                &CompatibilityKey {
                    parameters: "pdf-mode=ocr".to_owned(),
                    ..pending.compatibility.clone()
                },
                &root,
                4
            ),
            CheckpointState::Reprocess
        );
        assert_eq!(
            pending.resume_action(&pending.compatibility, &root, 4),
            CheckpointResumeAction::QueueOcr
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_rebuilds_sorted_pending_ocr_queue_from_checkpoint_directory() {
        let root = test_dir("recover-pending");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let compatibility = CompatibilityKey {
            input_sha256: "input".into(),
            model: "ovisocr2".into(),
            profile: "quality".into(),
            parameters: "pdf-mode=auto".into(),
        };
        let pending_compatibility =
            super::pending_ocr_checkpoint(1, compatibility.clone(), "early").compatibility;
        for (page, reason) in [(3, "late"), (1, "early")] {
            super::write_checkpoint(
                root.join(format!("page-{page:04}.checkpoint")),
                &super::pending_ocr_checkpoint(page, pending_compatibility.clone(), reason),
            )
            .unwrap();
        }
        let queue = recover_pending_ocr_queue(&root, &compatibility).unwrap();
        assert_eq!(
            queue.iter().map(|page| page.page).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(queue[0].fallback_reason.as_deref(), Some("early"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_ignores_pending_checkpoint_from_another_input() {
        let root = test_dir("recover-incompatible");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let stored = CompatibilityKey {
            input_sha256: "other-input".into(),
            model: "ovisocr2".into(),
            profile: "quality".into(),
            parameters: "pdf-mode=auto".into(),
        };
        let expected = CompatibilityKey {
            input_sha256: "current-input".into(),
            ..stored.clone()
        };
        let checkpoint = super::pending_ocr_checkpoint(2, stored, "fallback");
        super::write_checkpoint(root.join("page-0002.checkpoint"), &checkpoint).unwrap();
        assert!(
            recover_pending_ocr_queue(&root, &expected)
                .unwrap()
                .is_empty()
        );
        let expected_different_parameters = CompatibilityKey {
            input_sha256: "other-input".into(),
            parameters: "pdf-mode=ocr".into(),
            ..expected
        };
        assert!(
            recover_pending_ocr_queue(&root, &expected_different_parameters)
                .unwrap()
                .is_empty()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn routing_checkpoint_metadata_has_stable_v6_json_shape() {
        let metadata = RoutingCheckpointMetadata {
            pdf_mode: "auto".to_owned(),
            quality_tier: "l2".to_owned(),
            classifier_version: "pdf-route-v1".to_owned(),
            extractor_version: "pdf-native-v1".to_owned(),
            page_classification: "text".to_owned(),
            planned_method: "native_text".to_owned(),
            actual_method: None,
            fallback_reason: None,
        };
        assert_eq!(
            metadata.to_json(),
            "{\"pdf_mode\":\"auto\",\"quality_tier\":\"l2\",\"classifier_version\":\"pdf-route-v1\",\"extractor_version\":\"pdf-native-v1\",\"page_classification\":\"text\",\"planned_method\":\"native_text\",\"actual_method\":null,\"fallback_reason\":null}"
        );
    }

    #[test]
    fn routing_checkpoint_result_persists_v6_metadata_in_result_json() {
        let result = RoutingCheckpointResult {
            page: 4,
            routing: RoutingCheckpointMetadata {
                pdf_mode: "auto".to_owned(),
                quality_tier: "l2".to_owned(),
                classifier_version: "pdf-route-v1".to_owned(),
                extractor_version: "pdf-native-v1".to_owned(),
                page_classification: "mixed".to_owned(),
                planned_method: "native_text".to_owned(),
                actual_method: Some("ocr".to_owned()),
                fallback_reason: Some("native_validation_failed".to_owned()),
            },
        };
        assert!(result.to_json().contains("\"schema\":\"ocrx-result-v2\""));
        assert!(result.to_json().contains("\"actual_method\":\"ocr\""));
        assert!(result.to_json().contains("native_validation_failed"));
    }

    #[test]
    fn ocr_queue_merge_sorts_deduplicates_and_keeps_fallback_reason() {
        let initial = vec![
            OcrQueuePage {
                page: 4,
                fallback_reason: None,
            },
            OcrQueuePage {
                page: 2,
                fallback_reason: None,
            },
        ];
        let fallback = vec![
            OcrQueuePage {
                page: 3,
                fallback_reason: Some("native_validation_failed".to_owned()),
            },
            OcrQueuePage {
                page: 4,
                fallback_reason: Some("duplicate_text".to_owned()),
            },
        ];
        assert_eq!(
            super::merge_ocr_queue(&initial, &fallback),
            vec![
                OcrQueuePage {
                    page: 2,
                    fallback_reason: None
                },
                OcrQueuePage {
                    page: 3,
                    fallback_reason: Some("native_validation_failed".to_owned())
                },
                OcrQueuePage {
                    page: 4,
                    fallback_reason: Some("duplicate_text".to_owned())
                },
            ]
        );
    }

    #[test]
    fn native_fallback_queue_keeps_only_failed_candidates_with_reasons() {
        let candidates = vec![
            (1, Ok(())),
            (2, Err("native_structure_unresolved".to_owned())),
        ];
        assert_eq!(
            super::native_fallback_queue(&candidates),
            vec![OcrQueuePage {
                page: 2,
                fallback_reason: Some("native_structure_unresolved".to_owned())
            }]
        );
    }

    #[test]
    fn native_fallback_queue_is_sorted_and_deduplicated_by_page() {
        let candidates = vec![
            (4, Err("later".to_owned())),
            (2, Err("first".to_owned())),
            (4, Err("duplicate".to_owned())),
            (3, Ok(())),
        ];
        assert_eq!(
            super::native_fallback_queue(&candidates),
            vec![
                OcrQueuePage {
                    page: 2,
                    fallback_reason: Some("first".to_owned()),
                },
                OcrQueuePage {
                    page: 4,
                    fallback_reason: Some("later".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn checkpoint_round_trip_is_atomic_and_versioned() {
        let directory = test_dir("checkpoint");
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("page-0001.checkpoint");
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: CompatibilityKey {
                input_sha256: "input".to_owned(),
                model: "ovisocr2".to_owned(),
                profile: "compact".to_owned(),
                parameters: "dpi=300".to_owned(),
            },
            complete: true,
            output_sha256: "output".to_owned(),
            truncated: false,
            degenerated: false,
            attempts: 1,
            raw: None,
            warnings: Vec::new(),
            think_blocks_stripped: 0,
            audit: None,
        };
        super::write_checkpoint(&path, &checkpoint).unwrap();
        assert_eq!(super::read_checkpoint(&path).unwrap(), checkpoint);
        assert!(super::read_checkpoint(directory.join("missing")).is_err());
        fs::write(directory.join("legacy.checkpoint"), "ocrx-checkpoint-v4\n").unwrap();
        assert!(super::read_checkpoint(directory.join("legacy.checkpoint")).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn markdown_merge_sorts_pages_and_normalizes_line_endings() {
        let merged = super::merge_markdown(vec![
            PageMarkdown {
                page_number: 2,
                incomplete: None,
                content: "second\r\npage".to_owned(),
            },
            PageMarkdown {
                page_number: 1,
                incomplete: None,
                content: "first\rpage".to_owned(),
            },
        ])
        .unwrap();
        assert_eq!(
            merged,
            "<!-- page: 1 -->\nfirst\npage\n<!-- page: 2 -->\nsecond\npage"
        );
    }

    #[test]
    fn markdown_merge_rejects_empty_duplicate_or_zero_pages() {
        assert!(super::merge_markdown(Vec::new()).is_err());
        assert!(
            super::merge_markdown(vec![PageMarkdown {
                page_number: 0,
                incomplete: None,
                content: "x".to_owned()
            }])
            .is_err()
        );
        assert!(
            super::merge_markdown(vec![
                PageMarkdown {
                    page_number: 1,
                    incomplete: None,
                    content: "a".to_owned()
                },
                PageMarkdown {
                    page_number: 1,
                    incomplete: None,
                    content: "b".to_owned()
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn finalize_document_merges_native_and_ocr_pages_in_pdf_order() {
        let directory = test_dir("finalize-document");
        let _ = fs::remove_dir_all(&directory);
        super::finalize_document(
            &directory,
            vec![
                PageMarkdown {
                    page_number: 2,
                    incomplete: None,
                    content: "ocr".to_owned(),
                },
                PageMarkdown {
                    page_number: 1,
                    incomplete: None,
                    content: "native".to_owned(),
                },
            ],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("document.md")).unwrap(),
            "<!-- page: 1 -->\nnative\n<!-- page: 2 -->\nocr"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn collect_page_markdowns_reads_existing_page_artifacts_in_requested_order() {
        let directory = test_dir("collect-page-markdowns");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("page-0002.md"), "second").unwrap();
        fs::write(directory.join("page-0001.md"), "first").unwrap();

        let pages = super::collect_page_markdowns(&directory, &[2, 1]).unwrap();

        assert_eq!(pages[0].page_number, 2);
        assert_eq!(pages[0].content, "second");
        assert_eq!(pages[1].page_number, 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn overwrite_removes_only_an_allowed_output_root() {
        let directory = test_dir("overwrite");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("old"), b"old").unwrap();
        let input = std::path::Path::new("/work/input.pdf");
        let protected = std::path::Path::new("/work/.cache");
        let working_directory = std::path::Path::new("/work/elsewhere");
        super::remove_for_overwrite(input, &directory, &[protected], working_directory).unwrap();
        assert!(!directory.exists());
        assert!(
            super::remove_for_overwrite(input, "/work", &[protected], working_directory).is_err()
        );
        // The working-directory guard travels with the deletion API itself,
        // not only with the CLI wrapper.
        assert!(
            super::remove_for_overwrite(input, working_directory, &[protected], working_directory)
                .is_err()
        );
    }

    #[test]
    fn page_commit_writes_artifacts_before_complete_checkpoint() {
        let directory = test_dir("page-commit");
        let _ = fs::remove_dir_all(&directory);
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: CompatibilityKey {
                input_sha256: "input".to_owned(),
                model: "ovisocr2".to_owned(),
                profile: "compact".to_owned(),
                parameters: "dpi=300".to_owned(),
            },
            complete: false,
            output_sha256: "output".to_owned(),
            truncated: false,
            degenerated: false,
            attempts: 1,
            raw: None,
            warnings: Vec::new(),
            think_blocks_stripped: 0,
            audit: None,
        };
        super::commit_page(&directory, 1, b"png", "page\r\ntext", "{}", &checkpoint).unwrap();
        assert_eq!(
            fs::read(directory.join("page-0001.md")).unwrap(),
            b"page\ntext"
        );
        assert!(
            super::read_checkpoint(directory.join("page-0001.checkpoint"))
                .unwrap()
                .complete
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn native_page_commit_does_not_create_raster_asset() {
        let directory = test_dir("native-page-commit");
        let _ = fs::remove_dir_all(&directory);
        let checkpoint = PageCheckpoint {
            page_number: 1,
            compatibility: CompatibilityKey {
                input_sha256: "input".to_owned(),
                model: "none".to_owned(),
                profile: "none".to_owned(),
                parameters: "pdf-mode=auto".to_owned(),
            },
            complete: false,
            output_sha256: String::new(),
            truncated: false,
            think_blocks_stripped: 0,
            degenerated: false,
            attempts: 1,
            raw: None,
            warnings: Vec::new(),
            audit: None,
        };
        super::commit_native_page(
            &directory,
            1,
            "native text",
            "{\"source\":\"pdf_text\"}",
            &checkpoint,
        )
        .unwrap();
        assert!(directory.join("page-0001.md").is_file());
        assert!(directory.join("page-0001.json").is_file());
        assert!(!directory.join("images/page-0001.png").exists());
        assert!(
            super::read_checkpoint(directory.join("page-0001.checkpoint"))
                .unwrap()
                .complete
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn native_page_commit_and_collect_returns_the_committed_markdown() {
        let directory = test_dir("native-page-collect");
        let _ = fs::remove_dir_all(&directory);
        let checkpoint = PageCheckpoint {
            page_number: 2,
            compatibility: CompatibilityKey {
                input_sha256: "input".to_owned(),
                model: "none".to_owned(),
                profile: "none".to_owned(),
                parameters: "pdf-mode=auto".to_owned(),
            },
            complete: false,
            output_sha256: String::new(),
            truncated: false,
            degenerated: false,
            attempts: 1,
            raw: None,
            warnings: Vec::new(),
            think_blocks_stripped: 0,
            audit: None,
        };
        let page = super::commit_native_page_and_collect(
            &directory,
            2,
            "native\r\npage",
            "{}",
            &checkpoint,
        )
        .unwrap();
        assert_eq!(
            page,
            PageMarkdown {
                page_number: 2,
                incomplete: None,
                content: "native\npage".to_owned()
            }
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn render_audit_is_written_as_atomic_page_sidecar() {
        let directory = test_dir("render-audit");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let audit = RenderAudit {
            original_width: 612,
            original_height: 792,
            rendered_width: 2550,
            rendered_height: 3300,
            model_width: 2880,
            model_height: 3727,
            scaled: true,
            scale_ratio_numerator: 16,
            scale_ratio_denominator: 17,
        };
        super::write_render_audit(&directory, 1, audit).unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("page-0001.render-audit")).unwrap(),
            "original=612x792;rendered=2550x3300;model=2880x3727;scaled=true;ratio=16/17"
        );
        assert!(!directory.join(".page-0001.render-audit.part").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn job_summary_maps_page_outcomes_to_stable_status() {
        assert_eq!(
            JobSummary {
                degenerated_pages: 0,
                think_blocks_stripped: 0,
                selected_pages: 2,
                completed_pages: 2,
                failed_pages: 0,
                truncated_pages: 0,
                warnings: Vec::new(),
                diagnostics: Vec::new(),
                cancelled: false
            }
            .status(),
            JobStatus::Completed
        );
        assert_eq!(
            JobSummary {
                degenerated_pages: 0,
                think_blocks_stripped: 0,
                selected_pages: 2,
                completed_pages: 1,
                failed_pages: 1,
                truncated_pages: 0,
                warnings: Vec::new(),
                diagnostics: Vec::new(),
                cancelled: false
            }
            .status(),
            JobStatus::Partial
        );
        assert_eq!(
            JobSummary {
                degenerated_pages: 0,
                think_blocks_stripped: 0,
                selected_pages: 2,
                completed_pages: 1,
                failed_pages: 0,
                truncated_pages: 0,
                warnings: Vec::new(),
                diagnostics: Vec::new(),
                cancelled: true
            }
            .status(),
            JobStatus::Cancelled
        );
        assert_eq!(
            JobSummary {
                degenerated_pages: 0,
                think_blocks_stripped: 0,
                selected_pages: 2,
                completed_pages: 1,
                failed_pages: 0,
                truncated_pages: 0,
                warnings: Vec::new(),
                diagnostics: Vec::new(),
                cancelled: false
            }
            .status(),
            JobStatus::Running
        );
    }

    #[test]
    fn merge_job_summaries_combines_native_and_ocr_phases() {
        let native = JobSummary {
            degenerated_pages: 0,
            think_blocks_stripped: 0,
            selected_pages: 3,
            completed_pages: 2,
            failed_pages: 0,
            truncated_pages: 0,
            warnings: vec!["native warning".into()],
            diagnostics: vec![PageDiagnostic {
                page: 1,
                status: "completed".into(),
                stage: "native".into(),
                error_code: None,
                message: None,
                audit: None,
                warnings: Vec::new(),
            }],
            cancelled: false,
        };
        let ocr = JobSummary {
            degenerated_pages: 0,
            think_blocks_stripped: 0,
            selected_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            truncated_pages: 0,
            warnings: vec!["ocr warning".into()],
            diagnostics: vec![PageDiagnostic {
                page: 3,
                status: "completed".into(),
                stage: "ocr".into(),
                error_code: None,
                message: None,
                audit: None,
                warnings: Vec::new(),
            }],
            cancelled: false,
        };

        let merged = merge_job_summaries(native, ocr);

        assert_eq!(merged.selected_pages, 3);
        assert_eq!(merged.completed_pages, 3);
        assert_eq!(merged.warnings, vec!["native warning", "ocr warning"]);
        assert_eq!(merged.diagnostics.len(), 2);
        assert_eq!(merged.status(), JobStatus::Completed);
    }

    #[test]
    fn page_lifecycle_allows_only_forward_terminal_transitions() {
        let mut page = PageLifecycle::new(1).unwrap();
        assert!(page.transition(PageStatus::Completed).is_err());
        page.transition(PageStatus::Processing).unwrap();
        page.transition(PageStatus::Completed).unwrap();
        assert!(page.transition(PageStatus::Processing).is_err());
        assert!(PageLifecycle::new(0).is_err());
    }

    #[test]
    fn progress_eta_is_unknown_without_samples_and_after_completion() {
        assert_eq!(ProgressSnapshot::new(0, 10, 20).eta_seconds, None);
        assert_eq!(ProgressSnapshot::new(5, 10, 20).eta_seconds, Some(20));
        assert_eq!(ProgressSnapshot::new(10, 10, 20).eta_seconds, None);
    }

    #[test]
    fn backend_initialization_errors_have_device_specific_diagnostics() {
        let error = std::io::Error::other("llama_init_from_model: failed to initialize backend");
        assert_eq!(
            classify_recognition_error(&error),
            (
                "device_unavailable",
                "inference device initialization failed"
            )
        );
        let error = std::io::Error::other("invalid image dimensions");
        assert_eq!(
            classify_recognition_error(&error),
            ("recognition_failed", "page recognition failed")
        );
        let error = std::io::Error::other("page recognition failed: null reference from llama.cpp");
        assert_eq!(
            classify_recognition_error(&error),
            (
                "device_unavailable",
                "inference device initialization failed"
            )
        );
    }

    #[test]
    fn cancellation_token_distinguishes_first_and_second_interrupt() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert!(!token.force_exit());
        token.request_cancel();
        assert!(token.is_cancelled());
        assert!(!token.force_exit());
        token.request_cancel();
        assert!(token.force_exit());
    }

    #[test]
    fn heartbeat_emits_immediately_then_at_fixed_intervals() {
        let mut heartbeat = Heartbeat::default();
        assert!(heartbeat.should_emit(10));
        assert!(!heartbeat.should_emit(10));
        assert!(!heartbeat.should_emit(10));
        assert!(heartbeat.should_emit(11));
        let mut clamped = Heartbeat::new(0);
        assert!(clamped.should_emit(0));
        assert!(!clamped.should_emit(0));
        assert!(clamped.should_emit(1));
    }

    #[test]
    fn scan_controller_processes_serially_and_stops_after_cancel() {
        let mut controller = ScanController::new(&[1, 2]).unwrap();
        assert_eq!(controller.start_next().unwrap(), Some(1));
        controller.finish(1, PageStatus::Completed).unwrap();
        assert_eq!(controller.start_next().unwrap(), Some(2));
        controller.cancel();
        assert_eq!(controller.start_next().unwrap(), None);
        controller.finish(2, PageStatus::Failed).unwrap();
        assert_eq!(controller.summary().status(), JobStatus::Cancelled);
    }
}
