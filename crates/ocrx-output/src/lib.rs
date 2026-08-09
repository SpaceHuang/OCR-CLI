//! Atomic output and checkpoint boundary.

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
}

impl Reporter {
    pub fn result(&mut self, report: &ResultReport, format: ReportFormat) {
        self.stdout.push_str(&format_report(report, format));
        self.stdout.push('\n');
    }

    pub fn error(&mut self, error: &ErrorReport, format: ReportFormat) {
        match format {
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

impl ProgressEvent {
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
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

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
    pub warnings: Vec<String>,
    pub diagnostics: Vec<PageDiagnostic>,
    pub cancelled: bool,
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

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    requests: Arc<AtomicU8>,
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

pub trait PageArtifactRecognizer<M> {
    fn recognize_artifact(
        &mut self,
        model: &mut M,
        page: u32,
        image: &RenderedPage,
    ) -> io::Result<PageArtifactResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageArtifactResult {
    pub png: Vec<u8>,
    pub markdown: String,
    pub truncated: bool,
    pub warnings: Vec<String>,
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
        self.renderer.preflight()?;
        let mut model = self.loader.load()?;
        let root = output_root.as_ref();
        fs::create_dir_all(root)?;
        let mut completed = 0_u32;
        let mut failed = 0_u32;
        let mut truncated = 0_u32;
        let mut warnings = std::collections::BTreeSet::new();
        let mut diagnostics = Vec::new();
        for &page in page_numbers {
            if cancellation.is_cancelled() {
                break;
            }
            let checkpoint_path = root.join(format!("page-{page:04}.checkpoint"));
            if resume {
                if let Ok(checkpoint) = read_checkpoint(&checkpoint_path)
                    && checkpoint.is_reusable_at(compatibility, root, page)
                {
                    if checkpoint.truncated {
                        truncated += 1;
                    }
                    warnings.extend(checkpoint.warnings.iter().cloned());
                    diagnostics.push(checkpoint.diagnostic());
                    completed += 1;
                    continue;
                }
            }
            let image = match self.renderer.render(page) {
                Ok(image) => image,
                Err(_) => {
                    failed += 1;
                    diagnostics.push(PageDiagnostic {
                        page,
                        status: "failed".to_owned(),
                        stage: "render".to_owned(),
                        error_code: Some("render_failed".to_owned()),
                        message: Some("page rendering failed".to_owned()),
                        audit: None,
                        warnings: Vec::new(),
                    });
                    continue;
                }
            };
            match self.recognizer.recognize_artifact(&mut model, page, &image) {
                Ok(artifact) => {
                    let mut artifact_warnings = artifact.warnings;
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
                    warnings.extend(artifact_warnings.iter().cloned());
                    write_render_audit(root, page, image.audit)?;
                    let checkpoint = PageCheckpoint {
                        page_number: page,
                        compatibility: compatibility.clone(),
                        complete: true,
                        output_sha256: String::new(),
                        truncated: artifact.truncated,
                        warnings: artifact_warnings,
                        audit: Some(image.audit),
                    };
                    diagnostics.push(checkpoint.diagnostic());
                    commit_page(root, page, &artifact.png, &artifact.markdown, &checkpoint)?;
                    completed += 1;
                }
                Err(_) => {
                    failed += 1;
                    diagnostics.push(PageDiagnostic {
                        page,
                        status: "failed".to_owned(),
                        stage: "recognize".to_owned(),
                        error_code: Some("recognition_failed".to_owned()),
                        message: Some("page recognition failed".to_owned()),
                        audit: Some(image.audit),
                        warnings: Vec::new(),
                    });
                }
            }
        }
        if failed == 0
            && !cancellation.is_cancelled()
            && completed == u32::try_from(page_numbers.len()).unwrap_or(u32::MAX)
        {
            let pages = page_numbers
                .iter()
                .map(|&page| {
                    Ok(PageMarkdown {
                        page_number: page,
                        content: fs::read_to_string(root.join(format!("page-{page:04}.md")))?,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            atomic_write(root.join("document.md"), merge_markdown(pages)?.as_bytes())?;
            if !keep_pages {
                cleanup_full_page_images(root, page_numbers)?;
            }
        }
        Ok(JobSummary {
            selected_pages: u32::try_from(page_numbers.len()).unwrap_or(u32::MAX),
            completed_pages: completed,
            failed_pages: failed,
            truncated_pages: truncated,
            warnings: warnings.into_iter().collect(),
            diagnostics,
            cancelled: cancellation.is_cancelled(),
        })
    }
}

/// Removes full-page PNG copies after merge; figure crops and page Markdown stay for resume.
fn cleanup_full_page_images(root: &Path, page_numbers: &[u32]) -> io::Result<()> {
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
}

impl<R, L, A> ScanService<R, L, A> {
    #[must_use]
    pub fn new(renderer: R, loader: L, recognizer: A) -> Self {
        Self {
            renderer,
            loader,
            recognizer,
        }
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
            truncated_pages: self
                .pages
                .iter()
                .filter(|page| page.status == PageStatus::Truncated)
                .count() as u32,
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

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_cancel(&self) {
        self.requests
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_add(1))
            })
            .ok();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.requests.load(Ordering::SeqCst) >= 1
    }

    #[must_use]
    pub fn force_exit(&self) -> bool {
        self.requests.load(Ordering::SeqCst) >= 2
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
        } else if self.failed_pages > 0 || self.truncated_pages > 0 {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageCheckpoint {
    pub page_number: u32,
    pub compatibility: CompatibilityKey,
    pub complete: bool,
    pub output_sha256: String,
    pub truncated: bool,
    pub warnings: Vec<String>,
    pub audit: Option<RenderAudit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMarkdown {
    pub page_number: u32,
    pub content: String,
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
        merged.push_str(&format!(
            "<!-- page: {} -->\n{}",
            page.page_number,
            normalize_lf(&page.content)
        ));
    }
    Ok(merged)
}

fn normalize_lf(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

impl PageCheckpoint {
    #[must_use]
    pub fn is_reusable(&self, expected: &CompatibilityKey) -> bool {
        self.complete && &self.compatibility == expected && !self.output_sha256.is_empty()
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

    fn diagnostic(&self) -> PageDiagnostic {
        PageDiagnostic {
            page: self.page_number,
            status: if self.truncated {
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
    let content = format!(
        "ocrx-checkpoint-v4\npage={}\ninput={}\nmodel={}\nprofile={}\nparameters={}\ncomplete={}\noutput={}\ntruncated={}\nwarnings={}\naudit={}\n",
        checkpoint.page_number,
        encode_field(&checkpoint.compatibility.input_sha256),
        encode_field(&checkpoint.compatibility.model),
        encode_field(&checkpoint.compatibility.profile),
        encode_field(&checkpoint.compatibility.parameters),
        checkpoint.complete,
        checkpoint.output_sha256,
        checkpoint.truncated,
        encode_field(&checkpoint.warnings.join("\n")),
        checkpoint.audit.map_or_else(
            || "none".to_owned(),
            |audit| encode_field(&audit.as_checkpoint_fragment())
        ),
    );
    atomic_write(path, content.as_bytes())
}

fn write_render_audit(root: &Path, page: u32, audit: RenderAudit) -> io::Result<()> {
    let path = root.join(format!("page-{page:04}.render-audit"));
    let temporary = root.join(format!(".page-{page:04}.render-audit.part"));
    atomic_write(&temporary, audit.as_checkpoint_fragment().as_bytes())?;
    fs::rename(temporary, path)
}

pub fn read_checkpoint(path: impl AsRef<Path>) -> io::Result<PageCheckpoint> {
    let content = fs::read_to_string(path)?;
    if content.lines().next() != Some("ocrx-checkpoint-v4") {
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
        warnings: if warning_text.is_empty() {
            Vec::new()
        } else {
            warning_text.split('\n').map(str::to_owned).collect()
        },
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
    if output == input {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output root is the input path",
        ));
    }
    for protected in protected_roots {
        if output == *protected || output.starts_with(protected) || protected.starts_with(output) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "output root overlaps a protected path",
            ));
        }
    }
    Ok(())
}

pub fn remove_for_overwrite(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    protected_roots: &[&Path],
) -> io::Result<()> {
    validate_output_root(&input, &output, protected_roots)?;
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
        CancellationToken, CompatibilityKey, ErrorReport, Heartbeat, JobStatus, JobSummary,
        ModelLoader, PageArtifactRecognizer, PageArtifactResult, PageCheckpoint, PageLifecycle,
        PageMarkdown, PageRecognizer, PageRenderer, PageStatus, ProgressEvent, ProgressMode,
        ProgressSnapshot, RenderAudit, RenderedPage, ReportFormat, Reporter, ResultReport,
        ResultStatus, ScanController, ScanService, atomic_write, format_report,
    };
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
                png: vec![page as u8],
                markdown: format!("page {page}"),
                truncated: false,
                warnings: Vec::new(),
            })
        }
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
        assert!(super::validate_output_root(input, "/tmp/ocr-result", &[cache]).is_ok());
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
            audit: None,
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
            warnings: Vec::new(),
            audit: None,
        };
        super::write_checkpoint(&path, &checkpoint).unwrap();
        assert_eq!(super::read_checkpoint(&path).unwrap(), checkpoint);
        assert!(super::read_checkpoint(directory.join("missing")).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn markdown_merge_sorts_pages_and_normalizes_line_endings() {
        let merged = super::merge_markdown(vec![
            PageMarkdown {
                page_number: 2,
                content: "second\r\npage".to_owned(),
            },
            PageMarkdown {
                page_number: 1,
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
                content: "x".to_owned()
            }])
            .is_err()
        );
        assert!(
            super::merge_markdown(vec![
                PageMarkdown {
                    page_number: 1,
                    content: "a".to_owned()
                },
                PageMarkdown {
                    page_number: 1,
                    content: "b".to_owned()
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn overwrite_removes_only_an_allowed_output_root() {
        let directory = test_dir("overwrite");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("old"), b"old").unwrap();
        let input = std::path::Path::new("/work/input.pdf");
        let protected = std::path::Path::new("/work/.cache");
        super::remove_for_overwrite(input, &directory, &[protected]).unwrap();
        assert!(!directory.exists());
        assert!(super::remove_for_overwrite(input, "/work", &[protected]).is_err());
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
            warnings: Vec::new(),
            audit: None,
        };
        super::commit_page(&directory, 1, b"png", "page\r\ntext", &checkpoint).unwrap();
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
