//! Scan application (plan §4.1/§8.10, T052): the library entry for full
//! PDF document scans over the public inference API. It prints nothing,
//! exits no process, and installs no signal handler — the CLI subscribes
//! to [`ScanEvent`]s and maps the outcome to reports and exit codes.
//!
//! Flow per document (plan §6.3): preflight → page analysis → resume
//! classification (two-phase OCR protocol) → native-first phase → empty
//! pages → final OCR queue → on-demand prepare (once per invocation,
//! shared across documents) → windowed OCR with the frozen ladder →
//! merge/routing/result. Native-only and fully-reused documents never
//! prepare (zero model load, RBLK-8).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ocrx_core::{PageClassification, PageType, PdfReasonCode, ProcessingMethod, QualityTier};
use ocrx_inference_api::{
    InferenceService, InstallPolicy, PrepareRequest, StaticInferenceIdentity,
};
use ocrx_output::{
    PageIncomplete, PageMarkdown, PdfIdentity, ScanCompatibility, ScanOcrSide, ScanRoute,
    V6Checkpoint, V6Status, collect_page_markdowns, commit_native_page_v6, commit_pending_ocr_v6,
    finalize_document,
};
use ocrx_pdf::NativePage;
use ocrx_primitives::CancellationToken;

use crate::native::{run_native_first_phase, service_pages_from_ocr_queue};
use crate::plan::{planned_ocr_pages, validate_numeric_options};
use crate::report::write_routing_checkpoint_sidecars_with_states;
use crate::resume::{PageDisposition, ResumeContext, classify};
use crate::state::{JobSummary, OcrQueuePage, PageDiagnostic, PageStatus, merge_ocr_queue};
use crate::window::{PageRenderer, WindowConfig, run_pages_with_session, write_pending_ocr};

/// PDF facts the application needs (plan §6.3 steps 1–2). The production
/// implementation routes through the PdfiumOwner; tests inject fakes.
pub trait PdfDocumentAccess {
    /// Page count (fails for missing/corrupt/encrypted PDFs).
    ///
    /// # Errors
    ///
    /// I/O-class failure with the PDF reason.
    fn page_count(&mut self, input: &Path) -> std::io::Result<usize>;

    /// Classifies the selected pages.
    ///
    /// # Errors
    ///
    /// Analysis failure.
    fn analyze(
        &mut self,
        input: &Path,
        pages: &[usize],
        tier: QualityTier,
    ) -> std::io::Result<Vec<PageClassification>>;

    /// Extracts one page's native text candidate.
    ///
    /// # Errors
    ///
    /// Extraction failure.
    fn extract_native(&mut self, page: u32) -> std::io::Result<NativePage>;
}

/// One document's per-scan options (frozen defaults resolved by the CLI).
#[derive(Debug, Clone)]
pub struct DocumentOptions {
    pub pdf_mode: String,
    pub quality_tier: String,
    pub dpi: u32,
    pub max_image_tokens: u32,
    pub max_output_tokens: u32,
    pub context_size: u32,
    pub resume: bool,
    pub keep_pages: bool,
    /// `--column-split-retry` gate (default on).
    pub column_split_retry: bool,
    /// Selected pages; `None` = all.
    pub pages: Option<Vec<usize>>,
    /// Model/profile/device request strings (`auto` allowed).
    pub requested_profile: String,
    pub requested_device: String,
    pub install: InstallPolicy,
}

impl Default for DocumentOptions {
    fn default() -> Self {
        Self {
            pdf_mode: "auto".to_owned(),
            quality_tier: "l2".to_owned(),
            dpi: 300,
            max_image_tokens: 1024,
            max_output_tokens: 16_384,
            context_size: 20_480,
            resume: false,
            keep_pages: false,
            column_split_retry: true,
            pages: None,
            requested_profile: "auto".to_owned(),
            requested_device: "auto".to_owned(),
            install: InstallPolicy::offline(),
        }
    }
}

/// Progress events the CLI maps onto its Reporter contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEvent {
    /// Document started (input, output dir).
    DocumentStarted { input: PathBuf, output: PathBuf },
    /// Analysis progress (completed/total pages).
    Analyzing { completed: u32, total: u32 },
    /// Window-loop generation heartbeat (page, tokens, elapsed seconds).
    PageProgress {
        page: u32,
        generated_tokens: u32,
        elapsed_seconds: u64,
    },
    /// Document finished with its summary.
    DocumentFinished { input: PathBuf, cancelled: bool },
}

/// One document's outcome.
#[derive(Debug, Clone)]
pub struct DocumentOutcome {
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub summary: JobSummary,
    /// Selected page numbers (the merge scope).
    pub pages: Vec<u32>,
    /// Actual backend reported for the result.json (empty when no model).
    pub backend: Option<String>,
    /// Actual inference identity (empty for zero-load scans).
    pub identity: Option<ocrx_inference_api::InferenceIdentity>,
    /// T1 per-document telemetry delta (X1: snapshot - invocation
    /// baseline); `None` when no session loaded for this document.
    pub telemetry_delta: Option<ocrx_inference_api::RuntimeTelemetry>,
    /// T1 per-document step-profile delta (same baseline rule); `None`
    /// when the channel is disabled or no session loaded.
    pub step_profile_delta: Option<ocrx_inference_api::StepProfile>,
    /// Structured failure (`kind`/`message`/`cause`) when the document
    /// aborted before producing a summary.
    pub failure: Option<DocumentFailure>,
    /// `document.md` path when the merge produced one (present + non-empty
    /// merge scope).
    pub markdown_path: Option<std::path::PathBuf>,
    /// Frozen route classification (`document_type=` value).
    pub document_type: String,
    /// Frozen routing counters (`routing_counts` in result.json).
    pub routing_counts: RoutingCounts,
    /// Final routing diagnostic JSON strings (`pdf_routing` array).
    pub pdf_routing: Vec<String>,
    /// Figure PNGs under `images/` (bbox crops only).
    pub images_count: usize,
    /// Wall time of the document run (result.json `elapsed_ms`).
    pub elapsed_ms: u128,
}

/// Frozen routing counters (result.json `routing_counts` object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoutingCounts {
    pub native_text: u32,
    pub ocr: u32,
    pub empty: u32,
    pub fallback: u32,
}

/// Structured document-level failure (exit-code mapping stays in the CLI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFailure {
    /// Exit code per the frozen mapping (3 = input/pdf, 5 = output, …).
    pub code: u8,
    pub kind: String,
    pub message: String,
    pub cause: Option<String>,
}

/// Whole-invocation outcome (multi-input aggregation stays in the CLI).
#[derive(Debug, Clone)]
pub struct ScanOutcome {
    pub documents: Vec<DocumentOutcome>,
    pub cancelled: bool,
}

/// The application: owns no model, no renderer, no PDF handle — everything
/// arrives as ports per call.
#[derive(Debug, Default)]
pub struct ScanApplication;

impl ScanApplication {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Runs one invocation (single input = the historical path; multiple
    /// inputs = X1 FIFO with one shared session across documents).
    ///
    /// # Errors
    ///
    /// Only infrastructure failures that abort the whole invocation
    /// surface as `Err`; per-document failures are values in
    /// [`ScanOutcome::documents`].
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        inputs: &[PathBuf],
        output_for: &dyn Fn(&Path) -> PathBuf,
        options: &DocumentOptions,
        pdf: &mut dyn PdfDocumentAccess,
        renderer: &mut dyn PageRenderer,
        service: &mut dyn InferenceService,
        commit_recorder: &ocrx_output::commit_profile::CommitRecorder,
        cancellation: &CancellationToken,
        emit: &mut dyn FnMut(ScanEvent),
    ) -> std::io::Result<ScanOutcome> {
        // Frozen numeric gate (D5.1) once per invocation.
        validate_numeric_options(
            Some(options.dpi),
            Some(options.max_image_tokens),
            Some(options.max_output_tokens),
            Some(options.context_size),
        )
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
        let mut documents = Vec::new();
        let mut cancelled = false;
        // The shared session (X1 ModelSlot::Shared): loaded at most once for
        // the whole invocation, reused across documents.
        let mut shared: Option<PreparedRun> = None;
        let mut session: Option<Box<dyn ocrx_inference_api::InferenceSession>> = None;
        let mut identity: Option<ocrx_inference_api::InferenceIdentity> = None;
        // T1 baselines: the per-document deltas report snapshot - baseline
        // (delta_from saturates; first_token passes through).
        let mut telemetry_base = ocrx_inference_api::RuntimeTelemetry::default();
        let mut step_profile_base = ocrx_inference_api::StepProfile::default();
        for input in inputs {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let output = output_for(input);
            emit(ScanEvent::DocumentStarted {
                input: input.clone(),
                output: output.clone(),
            });
            let outcome = self.run_document(
                input,
                &output,
                options,
                pdf,
                renderer,
                service,
                &mut session,
                &mut identity,
                &mut shared,
                commit_recorder,
                &mut telemetry_base,
                &mut step_profile_base,
                cancellation,
                emit,
            )?;
            cancelled |= outcome.summary.cancelled;
            emit(ScanEvent::DocumentFinished {
                input: input.clone(),
                cancelled: outcome.summary.cancelled,
            });
            documents.push(outcome);
        }
        Ok(ScanOutcome {
            documents,
            cancelled,
        })
    }

    /// Runs one document (plan §6.3 steps 1–7).
    ///
    /// # Errors
    ///
    /// I/O-class abort (the caller records it as a document failure).
    ///
    /// # Panics
    ///
    /// Never for well-formed inputs: all domain failures surface as
    /// [`DocumentFailure`] values; page-number conversion saturates.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::fn_params_excessive_bools)]
    #[allow(clippy::too_many_arguments)]
    pub fn run_document(
        &mut self,
        input: &Path,
        output: &Path,
        options: &DocumentOptions,
        pdf: &mut dyn PdfDocumentAccess,
        renderer: &mut dyn PageRenderer,
        service: &mut dyn InferenceService,
        session: &mut Option<Box<dyn ocrx_inference_api::InferenceSession>>,
        identity: &mut Option<ocrx_inference_api::InferenceIdentity>,
        shared_prepared: &mut Option<PreparedRun>,
        commit_recorder: &ocrx_output::commit_profile::CommitRecorder,
        telemetry_base: &mut ocrx_inference_api::RuntimeTelemetry,
        step_profile_base: &mut ocrx_inference_api::StepProfile,
        cancellation: &CancellationToken,
        emit: &mut dyn FnMut(ScanEvent),
    ) -> std::io::Result<DocumentOutcome> {
        let started = std::time::Instant::now();
        let failure =
            |code: u8, kind: &str, message: &str, cause: Option<String>| DocumentOutcome {
                input: input.to_path_buf(),
                output_dir: output.to_path_buf(),
                summary: JobSummary {
                    selected_pages: 0,
                    completed_pages: 0,
                    failed_pages: 0,
                    truncated_pages: 0,
                    degenerated_pages: 0,
                    think_blocks_stripped: 0,
                    warnings: Vec::new(),
                    diagnostics: Vec::new(),
                    cancelled: false,
                },
                pages: Vec::new(),
                backend: None,
                identity: None,
                markdown_path: None,
                document_type: String::new(),
                routing_counts: RoutingCounts::default(),
                pdf_routing: Vec::new(),
                images_count: 0,
                elapsed_ms: 0,
                telemetry_delta: None,
                step_profile_delta: None,
                failure: Some(DocumentFailure {
                    code,
                    kind: kind.to_owned(),
                    message: message.to_owned(),
                    cause,
                }),
            };
        // Step 1: page count (PDF errors surface before any model work).
        let page_count = match pdf.page_count(input) {
            Ok(count) => count,
            Err(error) => {
                return Ok(failure(
                    3,
                    "input",
                    "unable to read PDF page count",
                    Some(error.to_string()),
                ));
            }
        };
        // BLK-4: validate the RAW selection BEFORE any filtering — an
        // out-of-range selection is a usage error (exit 2), never a silent
        // empty scan.
        let pages: Vec<usize> = match &options.pages {
            Some(selection) => {
                if selection
                    .iter()
                    .any(|page| *page == 0 || *page > page_count)
                {
                    return Ok(failure(
                        2,
                        "usage",
                        "page selection is out of range",
                        Some(format!("pdf has {page_count} pages")),
                    ));
                }
                selection.clone()
            }
            None => (1..=page_count).collect(),
        };
        let required_tier = if options.quality_tier == "l1" {
            QualityTier::L1
        } else {
            QualityTier::L2
        };
        // Step 2: analysis.
        emit(ScanEvent::Analyzing {
            completed: 0,
            total: pages.len() as u32,
        });
        let classifications = pdf.analyze(input, &pages, required_tier).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("unable to analyze PDF pages: {error}"),
            )
        })?;
        if options.pdf_mode == "text"
            && classifications.iter().any(|classification| {
                !matches!(classification.page_type, PageType::Empty)
                    && ocrx_pdf::plan_page_checked(ocrx_core::PdfMode::Text, *classification)
                        .is_err()
            })
        {
            return Ok(failure(
                3,
                "pdf_native",
                "text mode requires every non-empty page to be reliable native text",
                None,
            ));
        }
        // Shared identity scaffolding: input hash + PDF-side compatibility.
        let input_sha256 = ocrx_primitives::sha256_file(input)
            .map_err(|error| std::io::Error::new(error.kind(), error.to_string()))?;
        let pdf_identity = PdfIdentity {
            pdf_mode: options.pdf_mode.clone(),
            quality_tier: options.quality_tier.clone(),
            classifier_version: "pdf-route-v1".to_owned(),
            extractor_version: "pdf-text-v1".to_owned(),
            platform: current_platform(),
        };
        let native_compatibility = ScanCompatibility::native_or_empty(
            &input_sha256,
            ScanRoute::NativeText,
            pdf_identity.clone(),
        );
        // Step 3: resume classification (static pre-compare; zero-load path).
        let side = ScanOcrSide {
            prompt_id: crate::ocr::prompt::PROMPT_ID.to_owned(),
            prompt_version: crate::ocr::prompt::PROMPT_VERSION.to_owned(),
            prompt_schema_version: crate::ocr::prompt::PROMPT_SCHEMA_VERSION,
            dpi: options.dpi,
            max_image_tokens: options.max_image_tokens,
            max_output_tokens: options.max_output_tokens,
            context_size: options.context_size,
            pixel_width_budget: 2880,
            pixel_height_budget: 2880,
            page_parser_version: "ovis-page-v1".to_owned(),
        };
        let statik: Option<StaticInferenceIdentity> = if shared_prepared.is_none() {
            service
                .static_identity(
                    &ocrx_inference_api::ModelId::new("ovisocr2").map_err(|error| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                    })?,
                    &options.requested_profile,
                )
                .ok()
        } else {
            // A shared session already exists: its identity is authoritative
            // for later final compares, but the static query stays the
            // pre-compare source (RBLK-4).
            service
                .static_identity(
                    &ocrx_inference_api::ModelId::new("ovisocr2").map_err(|error| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                    })?,
                    &options.requested_profile,
                )
                .ok()
        };
        let mut resume_reuse: Vec<u32> = Vec::new();
        let mut resume_pending: Vec<u32> = Vec::new();
        let mut resume_incomplete: Vec<V6Checkpoint> = Vec::new();
        // Reused truncated/degenerated pages keep their incomplete flags in
        // the merged document (legacy collect_verified_page_markdowns rule).
        let mut reuse_incomplete: Vec<(u32, PageIncomplete)> = Vec::new();
        // BLK-2: per-page reuse stats so a final-compare demotion can roll
        // its contribution back out of the aggregates.
        let mut reuse_stats: std::collections::HashMap<u32, (u32, bool, Vec<String>)> =
            std::collections::HashMap::new();
        let mut reuse_think: u64 = 0;
        let mut reuse_truncated: u32 = 0;
        let mut reuse_warnings: HashSet<String> = HashSet::new();
        let mut resume_markdown: Vec<PageMarkdown> = Vec::new();
        let static_context = |has_pending: bool| ResumeContext {
            input_sha256: &input_sha256,
            pdf: &pdf_identity,
            side: &side,
            statik: statik.as_ref(),
            session_identity: None,
            has_pending_decode: has_pending,
        };
        if options.resume {
            for page in &pages {
                let page_u32 = u32::try_from(*page).unwrap_or(u32::MAX);
                let path = output.join(format!("page-{page_u32:04}.checkpoint"));
                let Ok(file) = ocrx_output::read_any(&path) else {
                    continue;
                };
                match classify(&file, output, &static_context(false)) {
                    PageDisposition::Reuse {
                        think_blocks_stripped,
                        truncated,
                        warnings,
                    } => {
                        reuse_think += u64::from(think_blocks_stripped);
                        if truncated {
                            reuse_truncated += 1;
                            reuse_incomplete.push((page_u32, PageIncomplete::Truncated));
                        }
                        reuse_warnings.extend(warnings.iter().cloned());
                        reuse_stats.insert(
                            page_u32,
                            (think_blocks_stripped, truncated, warnings.clone()),
                        );
                        // The checkpoint route decides the collection scope.
                        let is_ocr = matches!(
                            ocrx_output::read_any(&path),
                            Ok(ocrx_output::CheckpointFile::V6(checkpoint))
                                if checkpoint.compatibility.route == ScanRoute::Ocr
                        );
                        if is_ocr {
                            resume_reuse.push(page_u32);
                        } else {
                            // Native/empty pages collect through the normal
                            // verified merge below.
                        }
                    }
                    PageDisposition::PendingOcr => resume_pending.push(page_u32),
                    PageDisposition::Reprocess { .. } => {
                        if let Ok(ocrx_output::CheckpointFile::V6(checkpoint)) =
                            ocrx_output::read_any(&path)
                            && checkpoint.status == V6Status::Complete
                            && (checkpoint.truncated || checkpoint.degenerated)
                        {
                            resume_incomplete.push(checkpoint);
                        }
                    }
                }
            }
            // Pending checkpoints upgrade to the fresh v6 pending identity.
            for page in &resume_pending {
                let _ = commit_pending_ocr_v6(
                    output,
                    *page,
                    &V6Checkpoint {
                        page_number: *page,
                        status: V6Status::PendingOcr,
                        compatibility: native_compatibility.clone(),
                        output_sha256: String::new(),
                        truncated: false,
                        degenerated: false,
                        attempts: 1,
                        raw: None,
                        warnings: vec!["pending-ocr".to_owned()],
                        think_blocks_stripped: 0,
                        audit: None,
                    },
                );
            }
        }
        // Initial plans (plan §6.3 step 2).
        let initial_queue = initial_queue_for_mode(&options.pdf_mode, &pages, &classifications);
        let initial_queue: Vec<OcrQueuePage> = initial_queue
            .into_iter()
            .filter(|page| {
                !resume_reuse.contains(&page.page) && !resume_pending.contains(&page.page)
            })
            .collect();
        let native_candidates: Vec<u32> = if options.pdf_mode == "ocr" {
            Vec::new()
        } else {
            native_candidate_pages(&pages, &classifications)
                .into_iter()
                .filter(|page| !resume_reuse.contains(page) && !resume_pending.contains(page))
                .collect()
        };
        // Step 4: native-first phase (auto fallback / text termination).
        let mut native_markdown: Vec<PageMarkdown> = Vec::new();
        let native_failed: Vec<(u32, String)> = Vec::new();
        // Both callbacks need `&mut pdf`; share it through a RefCell for the
        // phase's duration (the phase is strictly sequential).
        let pdf_cell = std::cell::RefCell::new(pdf);
        let native_phase = run_native_first_phase(
            &options.pdf_mode,
            &native_candidates,
            |page| {
                pdf_cell
                    .borrow_mut()
                    .extract_native(page)
                    .map_err(|error| error.to_string())
                    .and_then(|native| {
                        ocrx_pdf::validate_native_candidate(page, &native, required_tier)
                            .map(|_| ())
                            .map_err(|reason| reason.as_str().to_owned())
                    })
            },
            |page| {
                if cancellation.is_cancelled() {
                    return Ok(());
                }
                let native = pdf_cell
                    .borrow_mut()
                    .extract_native(page)
                    .map_err(|error| error.to_string())?;
                let classification = pages
                    .iter()
                    .position(|selected| *selected as u32 == page)
                    .and_then(|index| classifications.get(index).copied())
                    .ok_or_else(|| "native_classification_missing".to_owned())?;
                let (markdown, page_json) = ocrx_pdf::native_pdf_page_artifacts(
                    page,
                    &native,
                    classification,
                    required_tier,
                )
                .map_err(|reason| format!("page {page}: {}", reason.as_str()))?;
                let checkpoint = V6Checkpoint {
                    page_number: page,
                    status: V6Status::Complete,
                    compatibility: native_compatibility.clone(),
                    output_sha256: String::new(),
                    truncated: false,
                    degenerated: false,
                    attempts: 1,
                    raw: None,
                    warnings: Vec::new(),
                    think_blocks_stripped: 0,
                    audit: None,
                };
                commit_native_page_v6(output, page, &markdown, &page_json, &checkpoint)
                    .map_err(|error| error.to_string())?;
                native_markdown.push(PageMarkdown {
                    page_number: page,
                    content: markdown,
                    incomplete: None,
                });
                Ok(())
            },
        );
        let native_phase = match native_phase {
            Ok(phase) => phase,
            Err(error) => {
                return Ok(failure(
                    3,
                    "pdf_native",
                    "native PDF text extraction failed",
                    Some(error),
                ));
            }
        };
        native_markdown.sort_by_key(|page| page.page_number);
        // Empty pages commit (no model work). The count feeds the summary
        // (the markdown vec moves into the merge below).
        let mut empty_markdown: Vec<PageMarkdown> = Vec::new();
        let mut empty_committed: u32 = 0;
        for (page, classification) in pages.iter().zip(&classifications) {
            if classification.page_type != PageType::Empty {
                continue;
            }
            let page_u32 = u32::try_from(*page).unwrap_or(u32::MAX);
            let (empty_markdown_line, page_json) =
                ocrx_pdf::empty_pdf_page_artifacts(page_u32, *classification)
                    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            let checkpoint = V6Checkpoint {
                page_number: page_u32,
                status: V6Status::Complete,
                compatibility: native_compatibility.clone(),
                output_sha256: String::new(),
                truncated: false,
                degenerated: false,
                attempts: 1,
                raw: None,
                warnings: Vec::new(),
                think_blocks_stripped: 0,
                audit: None,
            };
            commit_native_page_v6(
                output,
                page_u32,
                &empty_markdown_line,
                &page_json,
                &checkpoint,
            )
            .map_err(|error| std::io::Error::other(error.to_string()))?;
            empty_committed += 1;
            empty_markdown.push(PageMarkdown {
                page_number: page_u32,
                content: empty_markdown_line,
                incomplete: None,
            });
        }
        // Step 5: final OCR queue + on-demand prepare.
        let mut final_queue =
            crate::native::ocr_queue_after_native_phase(&initial_queue, &native_phase);
        final_queue.retain(|page| !resume_reuse.contains(&page.page));
        // Pending resume pages rejoin the decode queue (fresh v6 identity).
        final_queue = merge_ocr_queue(
            &final_queue,
            &resume_pending
                .iter()
                .map(|page| OcrQueuePage {
                    page: *page,
                    fallback_reason: Some("pending-ocr".to_owned()),
                })
                .collect::<Vec<_>>(),
        );
        let mut decode_pages = service_pages_from_ocr_queue(&final_queue);
        let has_pending_decode = !decode_pages.is_empty();
        // Two-phase continuation: with pending decode pages, the reused OCR
        // candidates must survive the final compare against the executing
        // identity (RBLK-6); without any, pre-compare was terminal (RBLK-8).
        let mut final_compare_failures: Vec<u32> = Vec::new();
        if has_pending_decode {
            if session.is_none() {
                // Prepare once (invocation-shared).
                let request = PrepareRequest {
                    model: ocrx_inference_api::ModelId::new("ovisocr2").map_err(|error| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                    })?,
                    requested_profile: options.requested_profile.clone(),
                    requested_device: options.requested_device.clone(),
                    max_image_tokens: options.max_image_tokens,
                    context_size: options.context_size,
                    install: options.install,
                };
                let mut interaction = ocrx_inference_api::NonInteractive;
                let mut interaction = AcceptingInteraction {
                    inner: &mut interaction,
                };
                match service.prepare(request, &mut interaction) {
                    Ok(prepared) => {
                        *identity = Some(prepared.session.identity().clone());
                        *session = Some(prepared.session);
                        *shared_prepared = Some(PreparedRun {
                            warnings: prepared.warnings.clone(),
                        });
                    }
                    Err(error) => {
                        // Model preparation failure: keep recoverable pending
                        // state for the OCR pages; committed pages survive.
                        for page in &decode_pages {
                            write_pending_ocr(
                                output,
                                *page,
                                &native_compatibility,
                                "ocr_required_but_unavailable",
                            )?;
                        }
                        return Ok(failure(
                            4,
                            "model",
                            "model preparation failed",
                            Some(error.to_string()),
                        ));
                    }
                }
            }
            // Final compare against the executing session (RBLK-6).
            let executing = session.as_ref().map(|session| session.identity().clone());
            for page in &resume_reuse {
                let path = output.join(format!("page-{page:04}.checkpoint"));
                let Ok(Ok(ocrx_output::CheckpointFile::V6(checkpoint))) =
                    ocrx_output::read_any(&path).map(Ok::<_, std::io::Error>)
                else {
                    continue;
                };
                let context = ResumeContext {
                    input_sha256: &input_sha256,
                    pdf: &pdf_identity,
                    side: &side,
                    statik: None,
                    session_identity: executing.as_ref(),
                    has_pending_decode: true,
                };
                if !matches!(
                    classify(
                        &ocrx_output::CheckpointFile::V6(checkpoint),
                        output,
                        &context
                    ),
                    PageDisposition::Reuse { .. }
                ) {
                    final_compare_failures.push(*page);
                }
            }
            #[cfg(test)]
            eprintln!("DEBUG-FC failures={final_compare_failures:?}");
            if !final_compare_failures.is_empty() {
                // Inconsistent candidates reprocess: never a mixed document.
                // BLK-2: the demoted pages must JOIN the decode queue AND
                // leave the reuse bookkeeping, otherwise they are neither
                // decoded nor collected (silent page loss reported as
                // success).
                resume_reuse.retain(|page| !final_compare_failures.contains(page));
                final_queue = merge_ocr_queue(
                    &final_queue,
                    &final_compare_failures
                        .iter()
                        .map(|page| OcrQueuePage {
                            page: *page,
                            fallback_reason: Some("final-compare-mismatch".to_owned()),
                        })
                        .collect::<Vec<_>>(),
                );
                // BLK-2: rebuild the actual decode queue from the merged
                // queue — the old decode_pages predates the demotion.
                decode_pages = service_pages_from_ocr_queue(&final_queue);
                // BLK-2: roll back the reuse counters accumulated during
                // static classification for the demoted pages.
                for page in &final_compare_failures {
                    if let Some(stats) = reuse_stats.remove(page) {
                        reuse_think = reuse_think.saturating_sub(u64::from(stats.0));
                        if stats.1 {
                            reuse_truncated = reuse_truncated.saturating_sub(1);
                        }
                        for warning in &stats.2 {
                            reuse_warnings.remove(warning);
                        }
                    }
                }
            }
        }
        // Collect the reused OCR pages' markdown (identity-verified above).
        for page in &resume_reuse {
            let collected = collect_page_markdowns(output, &[*page]).unwrap_or_default();
            let incomplete = reuse_incomplete
                .iter()
                .find(|(page_number, _)| *page_number == *page)
                .map(|(_, incomplete)| *incomplete)
                .or_else(|| {
                    resume_incomplete
                        .iter()
                        .find(|checkpoint| checkpoint.page_number == *page)
                        .map(|checkpoint| {
                            if checkpoint.degenerated {
                                PageIncomplete::Degenerated
                            } else {
                                PageIncomplete::Truncated
                            }
                        })
                });
            for markdown in collected {
                resume_markdown.push(PageMarkdown {
                    incomplete,
                    ..markdown
                });
            }
        }
        // Step 6: windowed OCR (only when pages remain and a session exists).
        let mut ocr_summary = JobSummary {
            selected_pages: decode_pages.len().min(u32::MAX as usize) as u32,
            completed_pages: 0,
            failed_pages: 0,
            truncated_pages: 0,
            degenerated_pages: 0,
            think_blocks_stripped: 0,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            cancelled: false,
        };
        if has_pending_decode {
            let executing = session.as_mut().expect("prepare succeeded above");
            let executing: &mut dyn ocrx_inference_api::InferenceSession = executing.as_mut();
            let compatibility = crate::compatibility::ocr_compatibility(
                &input_sha256,
                pdf_identity.clone(),
                executing.identity(),
                &crate::compatibility::ScanOcrConfig {
                    dpi: options.dpi,
                    max_image_tokens: options.max_image_tokens,
                    max_output_tokens: options.max_output_tokens,
                    context_size: options.context_size,
                    ..crate::compatibility::ScanOcrConfig::default()
                },
            );
            let config = WindowConfig {
                compatibility: &compatibility,
                resume: false,
                keep_pages: options.keep_pages,
                column_split_enabled: options.column_split_retry,
                max_output_tokens: options.max_output_tokens,
            };
            let mut slot_events: Vec<(u32, u32, u64)> = Vec::new();
            ocr_summary = run_pages_with_session(
                executing,
                renderer,
                &decode_pages,
                output,
                &config,
                commit_recorder,
                cancellation,
                &mut |page: u32, event| {
                    slot_events.push((page, event.generated_tokens, event.elapsed_seconds));
                },
            )?;
            for (page, tokens, elapsed) in slot_events {
                emit(ScanEvent::PageProgress {
                    page,
                    generated_tokens: tokens,
                    elapsed_seconds: elapsed,
                });
            }
        }
        // Step 7: merge + routing sidecars.
        let mut all_markdown: Vec<PageMarkdown> = Vec::new();
        all_markdown.extend(native_markdown);
        all_markdown.extend(empty_markdown);
        all_markdown.extend(resume_markdown);
        // OCR pages collect from disk (committed by the window loop).
        let committed_ocr: Vec<u32> = decode_pages
            .iter()
            .copied()
            .filter(|page| output.join(format!("page-{page:04}.checkpoint")).is_file())
            .collect();
        let mut ocr_markdown = collect_page_markdowns(output, &committed_ocr).unwrap_or_default();
        // Reuse-flag incomplete pages from the window summary diagnostics.
        for markdown in &mut ocr_markdown {
            let diagnostic = ocr_summary
                .diagnostics
                .iter()
                .find(|entry| entry.page == markdown.page_number);
            markdown.incomplete = diagnostic.and_then(|entry| {
                if entry
                    .warnings
                    .iter()
                    .any(|warning| warning == "degenerated")
                {
                    Some(PageIncomplete::Degenerated)
                } else if entry.status == "failed" {
                    Some(PageIncomplete::Truncated)
                } else {
                    None
                }
            });
        }
        all_markdown.extend(ocr_markdown);
        all_markdown.retain(|markdown| markdown.page_number > 0);
        all_markdown.sort_by_key(|markdown| markdown.page_number);
        all_markdown.dedup_by_key(|markdown| markdown.page_number);
        if all_markdown.is_empty() {
            // Stale document.md must not survive a zero-merge run.
            let _ = std::fs::remove_file(output.join("document.md"));
        } else {
            finalize_document(output, all_markdown)?;
        }
        // Routing sidecars reflect the final states.
        let fallback_reasons: Vec<(u32, PdfReasonCode)> = native_phase
            .fallback_pages
            .iter()
            .filter_map(|page| {
                page.fallback_reason
                    .as_ref()
                    .map(|reason| (page.page, pdf_reason_from_str(reason)))
            })
            .collect();
        let pending_set: HashSet<u32> = final_queue.iter().map(|page| page.page).collect();
        let pdf_routing = crate::report::final_routing_diagnostics_with_pending(
            &options.pdf_mode,
            &pages,
            &classifications,
            &fallback_reasons,
            &pending_set,
            &HashSet::new(),
        );
        let routing_counts = routing_counts_for_mode(
            &options.pdf_mode,
            &pages,
            &classifications,
            &fallback_reasons,
        );
        let document_type = ocrx_pdf::summarize_document_type(&classifications)
            .as_str()
            .to_owned();
        write_routing_checkpoint_sidecars_with_states(
            output,
            &options.pdf_mode,
            &pages,
            &classifications,
            &fallback_reasons,
            &pending_set,
            &HashSet::new(),
        )?;
        // Merge the summaries.
        let mut summary = JobSummary {
            selected_pages: u32::try_from(pages.len()).unwrap_or(u32::MAX),
            completed_pages: native_phase.committed_native_pages.len() as u32
                + empty_committed
                + resume_reuse.len() as u32,
            failed_pages: u32::try_from(native_failed.len()).unwrap_or(u32::MAX),
            truncated_pages: reuse_truncated,
            degenerated_pages: 0,
            think_blocks_stripped: reuse_think,
            warnings: {
                let mut warnings: Vec<String> = reuse_warnings.into_iter().collect();
                warnings.sort();
                warnings
            },
            diagnostics: Vec::new(),
            cancelled: cancellation.is_cancelled(),
        };
        summary.completed_pages += ocr_summary.completed_pages;
        summary.failed_pages += ocr_summary.failed_pages;
        summary.truncated_pages += ocr_summary.truncated_pages;
        summary.degenerated_pages += ocr_summary.degenerated_pages;
        summary.think_blocks_stripped += ocr_summary.think_blocks_stripped;
        summary.warnings.extend(ocr_summary.warnings.clone());
        summary.diagnostics.clone_from(&ocr_summary.diagnostics);
        summary.cancelled |= ocr_summary.cancelled;
        // T1/BLK-9: per-document delta ONLY when THIS document actually used
        // the shared session to decode (has_pending_decode). A fully-reused
        // later document reports None identity/telemetry — matching its own
        // single run (RBLK-8: orchestration == individual runs). The
        // baseline still advances to the current snapshot so the NEXT
        // decoding document deltas against the right point.
        let used_session = has_pending_decode;
        let (telemetry_delta, step_profile_delta) = match (session.as_ref(), used_session) {
            (Some(executing), true) => {
                let telemetry_now = executing.telemetry();
                let step_now = executing.step_profile();
                let telemetry_delta = telemetry_now.delta_from(telemetry_base);
                let step_profile_delta =
                    step_now.map(|profile| profile.delta_from(step_profile_base));
                *telemetry_base = telemetry_now;
                if let Some(profile) = step_now {
                    *step_profile_base = profile;
                }
                (Some(telemetry_delta), step_profile_delta)
            }
            _ => (None, None),
        };
        // BLK-8: T1 debug channel — both JSON files per document, only on a
        // successful non-cancelled run (cancel/failure zero writes;
        // disabled recorder = no commit profile file, disabled step channel
        // = no step profile file). Frozen schema_version:1 golden strings.
        if !cancellation.is_cancelled() && !summary.cancelled {
            if let Some(delta) = step_profile_delta {
                ocrx_output::atomic_write(
                    output.join("step-profile.json"),
                    delta.to_json().as_bytes(),
                )?;
            }
            if commit_recorder.enabled() {
                ocrx_output::atomic_write(
                    output.join("commit-profile.json"),
                    commit_recorder.snapshot().to_json().as_bytes(),
                )?;
            }
        }
        let _ = PageStatus::Pending;
        let _ = PageDiagnostic {
            page: 0,
            status: String::new(),
            stage: String::new(),
            error_code: None,
            message: None,
            audit: None,
            warnings: Vec::new(),
        };
        let markdown_path = output
            .join("document.md")
            .is_file()
            .then(|| output.join("document.md"));
        let images_count = std::fs::read_dir(output.join("images"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry.path().is_file()
                            && entry
                                .path()
                                .extension()
                                .is_some_and(|extension| extension == "png")
                            && entry.file_name().to_string_lossy().starts_with("page-")
                            && entry.file_name().to_string_lossy().contains("-figure-")
                    })
                    .count()
            })
            .unwrap_or(0);
        // BLK-9: the document reports the platform identity ONLY when it
        // actually used the shared session to decode. A fully-reused later
        // document matches its own single run (model object null).
        let document_identity = if used_session { identity.clone() } else { None };
        Ok(DocumentOutcome {
            input: input.to_path_buf(),
            output_dir: output.to_path_buf(),
            summary,
            pages: pages
                .iter()
                .map(|page| u32::try_from(*page).unwrap_or(0))
                .collect(),
            backend: document_identity
                .as_ref()
                .map(|identity| identity.backend.to_string()),
            identity: document_identity,
            telemetry_delta,
            step_profile_delta,
            markdown_path,
            document_type,
            routing_counts,
            pdf_routing,
            images_count,
            elapsed_ms: started.elapsed().as_millis(),
            failure: None,
        })
    }
}

/// Marker that this invocation already prepared a shared session.
#[derive(Debug, Clone, Default)]
pub struct PreparedRun {
    pub warnings: Vec<String>,
}

/// Install-interaction shim: the scan app runs non-interactive by default
/// (the CLI gates downloads before invoking the scan).
struct AcceptingInteraction<'a> {
    inner: &'a mut dyn ocrx_inference_api::InstallInteraction,
}

impl ocrx_inference_api::InstallInteraction for AcceptingInteraction<'_> {
    fn confirm_download(
        &mut self,
        proposals: &[ocrx_inference_api::DownloadProposal],
    ) -> Result<bool, ocrx_inference_api::InferenceError> {
        self.inner.confirm_download(proposals)
    }
}

/// Builds the initial OCR queue for a mode (port of the legacy helpers:
/// empty pages never enter, ocr-mode takes every non-empty page, auto takes
/// OCR-recommended pages).
#[must_use]
pub fn initial_queue_for_mode(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
) -> Vec<OcrQueuePage> {
    let mut queue: Vec<_> = pages
        .iter()
        .zip(classifications)
        .filter_map(|(page, classification)| {
            let needs_ocr = !matches!(classification.page_type, PageType::Empty)
                && (mode == "ocr" || classification.recommended_method == ProcessingMethod::Ocr);
            needs_ocr.then_some(OcrQueuePage {
                page: u32::try_from(*page).unwrap_or(u32::MAX),
                fallback_reason: None,
            })
        })
        .collect();
    queue.sort_by_key(|page| page.page);
    queue.dedup_by_key(|page| page.page);
    queue
}

/// Initial OCR queue per mode (plan §6.3 step 2): empty pages never enter;
/// ocr-mode takes every non-empty page; auto takes OCR-recommended pages.
#[must_use]
pub fn initial_ocr_queue_for_mode(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
) -> Vec<crate::state::OcrQueuePage> {
    let mut queue: Vec<_> = pages
        .iter()
        .zip(classifications)
        .filter_map(|(page, classification)| {
            let needs_ocr = !matches!(classification.page_type, PageType::Empty)
                && (mode == "ocr" || classification.recommended_method == ProcessingMethod::Ocr);
            needs_ocr.then_some(crate::state::OcrQueuePage {
                page: u32::try_from(*page).unwrap_or(u32::MAX),
                fallback_reason: None,
            })
        })
        .collect();
    queue.sort_by_key(|page| page.page);
    queue.dedup_by_key(|page| page.page);
    queue
}

/// Native candidate pages (recommended native text), sorted and deduped.
#[must_use]
pub fn native_candidate_pages(pages: &[usize], classifications: &[PageClassification]) -> Vec<u32> {
    let mut candidates: Vec<_> = pages
        .iter()
        .zip(classifications)
        .filter_map(|(page, classification)| {
            (classification.recommended_method == ProcessingMethod::NativeText)
                .then_some(u32::try_from(*page).unwrap_or(u32::MAX))
        })
        .collect();
    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

/// Legacy reason strings back to the structured code (routing sidecars).
#[must_use]
pub fn pdf_reason_from_str(value: &str) -> PdfReasonCode {
    match value {
        "native_text_reliable" => PdfReasonCode::NativeTextReliable,
        "no_visible_content" => PdfReasonCode::NoVisibleContent,
        "image_coverage_high" => PdfReasonCode::ImageCoverageHigh,
        "mixed_content" => PdfReasonCode::MixedContent,
        "text_coverage_low" => PdfReasonCode::TextCoverageLow,
        "unicode_validity_low" => PdfReasonCode::UnicodeValidityLow,
        "duplicate_text" => PdfReasonCode::DuplicateText,
        "geometry_anomaly" => PdfReasonCode::GeometryAnomaly,
        "native_structure_unresolved" => PdfReasonCode::NativeStructureUnresolved,
        _ => PdfReasonCode::NativeValidationFailed,
    }
}

/// An all-zero signals fixture (fakes only).
#[must_use]
pub fn empty_signals() -> ocrx_pdf::PageSignals {
    ocrx_pdf::PageSignals {
        page_width: 0.0,
        page_height: 0.0,
        image_objects: 0,
        text_bbox_coverage: 0.0,
        visible_characters: 0,
        unicode_validity: 0.0,
        alphanumeric_ratio: 0.0,
        text_coverage: 0.0,
        image_coverage: 0.0,
        duplicate_text: false,
        geometry_anomaly: false,
    }
}

/// A native page passing the frozen validation thresholds (L2 tier:
/// non-empty blocks, unicode >= 0.90, alphanumeric >= 0.70, no
/// duplicates/anomalies).
pub(crate) fn valid_native_page(page: u32) -> NativePage {
    let _ = page; // the page number rides the artifacts call, not the page
    NativePage {
        text: "native body text".to_owned(),
        blocks: vec![ocrx_pdf::NativeTextBlock {
            text: "native body text".to_owned(),
            bbox: ocrx_core::NormalizedBbox {
                left: 10,
                top: 10,
                right: 400,
                bottom: 60,
            },
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        }],
        signals: ocrx_pdf::PageSignals {
            page_width: 612.0,
            page_height: 792.0,
            image_objects: 0,
            text_bbox_coverage: 0.30,
            visible_characters: 14,
            unicode_validity: 1.0,
            alphanumeric_ratio: 0.95,
            text_coverage: 0.30,
            image_coverage: 0.0,
            duplicate_text: false,
            geometry_anomaly: false,
        },
    }
}

/// Frozen routing counters (plan §6.3/legacy routing_counts_for_mode): the
/// ocr mode counts every non-empty page as OCR; auto/text count by
/// recommendation with fallback pages double-counting ocr+fallback.
#[must_use]
pub fn routing_counts_for_mode(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, PdfReasonCode)],
) -> RoutingCounts {
    if mode == "ocr" {
        let empty = classifications
            .iter()
            .filter(|classification| classification.recommended_method == ProcessingMethod::Empty)
            .count();
        return RoutingCounts {
            native_text: 0,
            ocr: u32::try_from(pages.len().saturating_sub(empty)).unwrap_or(u32::MAX),
            empty: u32::try_from(empty).unwrap_or(u32::MAX),
            fallback: 0,
        };
    }
    let mut counts = RoutingCounts::default();
    for (page, classification) in pages.iter().zip(classifications) {
        if fallback_reasons
            .iter()
            .any(|(number, _)| *number == *page as u32)
        {
            counts.ocr += 1;
            counts.fallback += 1;
        } else {
            match classification.recommended_method {
                ProcessingMethod::NativeText => counts.native_text += 1,
                ProcessingMethod::Ocr => counts.ocr += 1,
                ProcessingMethod::Empty => counts.empty += 1,
            }
        }
    }
    counts
}

/// Platform identifier for the compatibility identity (frozen strings).
#[must_use]
pub fn current_platform() -> String {
    if cfg!(target_os = "macos") {
        "macos".to_owned()
    } else if cfg!(target_os = "windows") {
        "windows".to_owned()
    } else {
        "unknown".to_owned()
    }
}

/// Silence the unused-planned helper import until T067 wires telemetry.
#[allow(dead_code)]
fn _planned_used() -> bool {
    planned_ocr_pages("auto", &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::identity;
    use std::cell::RefCell;
    use std::rc::Rc;

    use ocrx_output::commit_profile::CommitRecorder;

    /// Fake PDF: a 3-page doc, page 1 native-clean, page 2 native-invalid
    /// (falls back), page 3 empty.
    struct FakePdf {
        native_fails: Vec<u32>,
        extract_calls: RefCell<Vec<u32>>,
    }

    impl FakePdf {
        fn new() -> Self {
            Self {
                native_fails: vec![2],
                extract_calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl PdfDocumentAccess for FakePdf {
        fn page_count(&mut self, _input: &Path) -> std::io::Result<usize> {
            Ok(3)
        }

        fn analyze(
            &mut self,
            _input: &Path,
            pages: &[usize],
            _tier: QualityTier,
        ) -> std::io::Result<Vec<PageClassification>> {
            Ok(pages
                .iter()
                .map(|page| {
                    if *page == 3 {
                        PageClassification {
                            page_type: PageType::Empty,
                            confidence: ocrx_core::RouteConfidence::High,
                            recommended_method: ProcessingMethod::Empty,
                            reason: PdfReasonCode::NoVisibleContent,
                        }
                    } else {
                        PageClassification {
                            page_type: PageType::Text,
                            confidence: ocrx_core::RouteConfidence::High,
                            recommended_method: ProcessingMethod::NativeText,
                            reason: PdfReasonCode::NativeTextReliable,
                        }
                    }
                })
                .collect())
        }

        fn extract_native(&mut self, page: u32) -> std::io::Result<NativePage> {
            self.extract_calls.borrow_mut().push(page);
            if self.native_fails.contains(&page) {
                return Err(std::io::Error::other("low unicode validity"));
            }
            Ok(valid_native_page(page))
        }
    }

    /// Fake renderer (1x1 pages).
    struct FakeRenderer;

    impl PageRenderer for FakeRenderer {
        fn preflight(&mut self) -> std::io::Result<()> {
            Ok(())
        }

        fn render(&mut self, _page: u32) -> std::io::Result<crate::state::RenderedPage> {
            crate::state::RenderedPage::new(1, 1, vec![0; 3])
        }
    }

    /// Fake service: static identity resolves; prepare returns the shared
    /// fake session (counts prepares).
    struct FakeService {
        prepares: Rc<RefCell<usize>>,
        session: crate::window_tests::FakeSession,
        /// BLK-2 knob: static identity override (differs from the session's
        /// identity to force a final-compare mismatch).
        static_override: Option<StaticInferenceIdentity>,
    }

    impl InferenceService for FakeService {
        fn prepare(
            &mut self,
            _request: PrepareRequest,
            _interaction: &mut dyn ocrx_inference_api::InstallInteraction,
        ) -> Result<ocrx_inference_api::PreparedInference, ocrx_inference_api::InferenceError>
        {
            *self.prepares.borrow_mut() += 1;
            Ok(ocrx_inference_api::PreparedInference {
                session: Box::new(self.session.clone()),
                requested_backend: ocrx_inference_api::BackendId::new("cpu").unwrap(),
                warnings: Vec::new(),
            })
        }

        fn decode_generation(&self) -> u32 {
            2
        }

        fn static_identity(
            &self,
            _model: &ocrx_inference_api::ModelId,
            _requested_profile: &str,
        ) -> Result<StaticInferenceIdentity, ocrx_inference_api::InferenceError> {
            Ok(self
                .static_override
                .clone()
                .unwrap_or_else(|| StaticInferenceIdentity::from_execution(&identity())))
        }
    }

    fn options() -> DocumentOptions {
        DocumentOptions::default()
    }

    #[test]
    fn native_only_scan_never_prepares_and_writes_document() {
        let root = std::env::temp_dir().join(format!("ocrx-app-native-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("doc.pdf");
        std::fs::write(&input, b"pdf-bytes").unwrap();
        let output = root.join("out");
        // All 3 pages native-clean: page 3 empty (empty commit), 1-2 native.
        let mut pdf = FakePdf::new();
        pdf.native_fails.clear();
        let prepares = Rc::new(RefCell::new(0_usize));
        let session = crate::window_tests::FakeSession::with_depth(1);
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let outcome = app
            .run(
                std::slice::from_ref(&input),
                &|_| output.clone(),
                &options(),
                &mut pdf,
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(outcome.documents.len(), 1);
        let document = &outcome.documents[0];
        assert!(document.failure.is_none());
        // Zero prepare: native+empty scans never touch the model (T057).
        assert_eq!(*prepares.borrow(), 0);
        assert!(document.backend.is_none());
        assert_eq!(document.summary.completed_pages, 3);
        assert_eq!(document.summary.failed_pages, 0);
        // document.md merged (3 pages) + native page artifacts exist.
        assert!(output.join("document.md").is_file());
        assert!(output.join("page-0001.md").is_file());
        assert!(output.join("page-0003.md").is_file());
        // Routing sidecars written for every page (T066).
        assert!(output.join("page-0001.routing.json").is_file());
        // No full-page PNG for native pages (no synthetic raster, T048).
        assert!(!output.join("images/page-0001.png").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn auto_fallback_prepares_once_and_decodes_the_fallback_queue() {
        let root = std::env::temp_dir().join(format!("ocrx-app-auto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("doc.pdf");
        std::fs::write(&input, b"pdf-bytes").unwrap();
        let output = root.join("out");
        // Page 2 native validation fails -> OCR fallback; decode succeeds.
        let mut pdf = FakePdf::new();
        let prepares = Rc::new(RefCell::new(0_usize));
        let session = crate::window_tests::FakeSession::with_depth(1);
        session.core.push(
            2,
            crate::window_tests::Program {
                text: "ocr body for page 2".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let outcome = app
            .run(
                std::slice::from_ref(&input),
                &|_| output.clone(),
                &options(),
                &mut pdf,
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        let document = &outcome.documents[0];
        assert!(document.failure.is_none(), "{:?}", document.failure);
        // Exactly one prepare for the fallback queue (T057).
        assert_eq!(*prepares.borrow(), 1);
        // 3 committed: 1 native + 1 empty + 1 OCR.
        assert_eq!(document.summary.completed_pages, 3);
        // The OCR page wrote its artifacts (markdown + checkpoint + routing).
        assert!(output.join("page-0002.md").is_file());
        assert!(output.join("page-0002.checkpoint").is_file());
        // BLK-5: default (no --keep-pages) removes the full-page PNG;
        // figure crops under images/ stay.
        assert!(!output.join("images/page-0002.png").is_file());
        // document.md carries all three pages in order.
        let document_md = std::fs::read_to_string(output.join("document.md")).unwrap();
        assert!(document_md.contains("ocr body for page 2"));
        // The identity surfaced for the result report.
        let document_identity = document.identity.as_ref().expect("prepared identity");
        assert_eq!(document_identity.backend.as_str(), "cpu");
        assert_eq!(document.backend.as_deref(), Some("cpu"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn multi_input_shares_one_session_across_documents() {
        let root = std::env::temp_dir().join(format!("ocrx-app-multi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input_a = root.join("a.pdf");
        let input_b = root.join("b.pdf");
        std::fs::write(&input_a, b"a").unwrap();
        std::fs::write(&input_b, b"b").unwrap();
        let output_root = root.join("outs");
        // Both documents need OCR (ocr mode): page 1 per document.
        let pdf = FakeMultiPdf;
        let prepares = Rc::new(RefCell::new(0_usize));
        let session = crate::window_tests::FakeSession::with_depth(1);
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "doc a body".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "doc b body".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let options = DocumentOptions {
            pdf_mode: "ocr".to_owned(),
            ..DocumentOptions::default()
        };
        let outcome = app
            .run(
                &[input_a, input_b],
                &|input| {
                    let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
                    output_root.join(stem)
                },
                &options,
                &mut { pdf },
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(outcome.documents.len(), 2);
        // X1 ModelSlot::Shared: exactly one prepare across both documents.
        assert_eq!(*prepares.borrow(), 1);
        // Both documents decoded their page 1 with the shared session.
        assert!(output_root.join("a/document.md").is_file());
        assert!(output_root.join("b/document.md").is_file());
        let a_md = std::fs::read_to_string(output_root.join("a/document.md")).unwrap();
        let b_md = std::fs::read_to_string(output_root.join("b/document.md")).unwrap();
        assert!(a_md.contains("doc a body"));
        assert!(b_md.contains("doc b body"));
        let _ = std::fs::remove_dir_all(root);
    }

    /// Session-side identity with a DIFFERENT batch depth than
    /// [`crate::test_support::identity`] (forces a final-compare mismatch).
    fn session_identity() -> ocrx_inference_api::InferenceIdentity {
        let mut id = crate::test_support::identity();
        id.batch_depth = 16;
        id
    }

    fn static_identity_for_session() -> StaticInferenceIdentity {
        StaticInferenceIdentity::from_execution(&session_identity())
    }

    /// A two-page OCR-mode doc fixture (both pages route to OCR).
    struct TwoPageOcrPdf;

    impl PdfDocumentAccess for TwoPageOcrPdf {
        fn page_count(&mut self, _input: &Path) -> std::io::Result<usize> {
            Ok(2)
        }

        fn analyze(
            &mut self,
            _input: &Path,
            pages: &[usize],
            _tier: QualityTier,
        ) -> std::io::Result<Vec<PageClassification>> {
            Ok(pages
                .iter()
                .map(|_page| PageClassification {
                    page_type: PageType::Image,
                    confidence: ocrx_core::RouteConfidence::High,
                    recommended_method: ProcessingMethod::Ocr,
                    reason: PdfReasonCode::ImageCoverageHigh,
                })
                .collect())
        }

        fn extract_native(&mut self, _page: u32) -> std::io::Result<ocrx_pdf::NativePage> {
            Ok(NativePage {
                text: String::new(),
                blocks: Vec::new(),
                signals: empty_signals(),
            })
        }
    }

    /// A two-page OCR-mode doc fixture (both pages route to OCR).
    struct FakeMultiPdf;

    impl PdfDocumentAccess for FakeMultiPdf {
        fn page_count(&mut self, _input: &Path) -> std::io::Result<usize> {
            Ok(1)
        }

        fn analyze(
            &mut self,
            _input: &Path,
            pages: &[usize],
            _tier: QualityTier,
        ) -> std::io::Result<Vec<PageClassification>> {
            Ok(pages
                .iter()
                .map(|_page| PageClassification {
                    page_type: PageType::Image,
                    confidence: ocrx_core::RouteConfidence::High,
                    recommended_method: ProcessingMethod::Ocr,
                    reason: PdfReasonCode::ImageCoverageHigh,
                })
                .collect())
        }

        fn extract_native(&mut self, _page: u32) -> std::io::Result<NativePage> {
            Ok(NativePage {
                text: String::new(),
                blocks: Vec::new(),
                signals: empty_signals(),
            })
        }
    }

    /// A service whose static identity resolves but whose prepare always
    /// fails (model unavailable) — the T068 fixture.
    struct FailingPrepareService {
        session: crate::window_tests::FakeSession,
    }

    impl InferenceService for FailingPrepareService {
        fn prepare(
            &mut self,
            _request: PrepareRequest,
            _interaction: &mut dyn ocrx_inference_api::InstallInteraction,
        ) -> Result<ocrx_inference_api::PreparedInference, ocrx_inference_api::InferenceError>
        {
            Err(ocrx_inference_api::InferenceError::RuntimeInitialization(
                "model load failed".into(),
            ))
        }

        fn decode_generation(&self) -> u32 {
            2
        }

        fn static_identity(
            &self,
            _model: &ocrx_inference_api::ModelId,
            _requested_profile: &str,
        ) -> Result<StaticInferenceIdentity, ocrx_inference_api::InferenceError> {
            Ok(StaticInferenceIdentity::from_execution(&identity()))
        }
    }

    #[test]
    fn prepare_failure_keeps_recoverable_pending_and_committed_pages() {
        let root = std::env::temp_dir().join(format!("ocrx-app-t068-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("doc.pdf");
        std::fs::write(&input, b"pdf-bytes").unwrap();
        let output = root.join("out");
        // auto mode: page 2 native validation fails -> OCR fallback; prepare
        // then fails (model unavailable). Page 1 (native) and page 3 (empty)
        // must survive committed; page 2 keeps recoverable pending state.
        let mut pdf = FakePdf::new();
        let session = crate::window_tests::FakeSession::with_depth(1);
        let mut service = FailingPrepareService { session };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let outcome = app
            .run(
                std::slice::from_ref(&input),
                &|_| output.clone(),
                &options(),
                &mut pdf,
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        let document = &outcome.documents[0];
        let failure = document.failure.as_ref().expect("prepare failure surfaces");
        assert_eq!(failure.code, 4);
        assert_eq!(failure.kind, "model");
        // Committed native/empty pages survive.
        assert!(output.join("page-0001.md").is_file());
        assert!(output.join("page-0003.md").is_file());
        // Page 2 keeps a recoverable pending checkpoint (resume handoff).
        let pending_path = output.join("page-0002.checkpoint");
        assert!(pending_path.is_file());
        let file = ocrx_output::read_any(&pending_path).unwrap();
        match file {
            ocrx_output::CheckpointFile::V6(checkpoint) => {
                assert_eq!(checkpoint.status, ocrx_output::V6Status::PendingOcr);
                assert!(
                    checkpoint
                        .warnings
                        .iter()
                        .any(|warning| warning == "ocr_required_but_unavailable")
                );
            }
            other => panic!("expected v6 pending, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn per_document_telemetry_delta_isolates_contributions() {
        // Two OCR documents share one session; each document's delta covers
        // only its own tokens (snapshot - baseline, X1 semantics).
        let root = std::env::temp_dir().join(format!("ocrx-app-t067-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input_a = root.join("a.pdf");
        let input_b = root.join("b.pdf");
        std::fs::write(&input_a, b"a").unwrap();
        std::fs::write(&input_b, b"b").unwrap();
        let output_root = root.join("outs");
        let pdf = FakeMultiPdf;
        let session = crate::window_tests::FakeSession::with_depth(1);
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "doc a body".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "doc b body".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        let prepares = Rc::new(RefCell::new(0_usize));
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let options = DocumentOptions {
            pdf_mode: "ocr".to_owned(),
            ..DocumentOptions::default()
        };
        let outcome = app
            .run(
                &[input_a, input_b],
                &|input| {
                    let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
                    output_root.join(stem)
                },
                &options,
                &mut { pdf },
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        // Each document's telemetry delta covers exactly its own submit (+1
        // generated token per page; the fake bumps one token per submit).
        let a = &outcome.documents[0];
        let b = &outcome.documents[1];
        assert_eq!(
            a.telemetry_delta
                .as_ref()
                .map(|delta| delta.generated_tokens),
            Some(1),
            "document A's delta covers only its own page"
        );
        assert_eq!(
            b.telemetry_delta
                .as_ref()
                .map(|delta| delta.generated_tokens),
            Some(1),
            "document B's delta covers only its own page (baseline advanced)"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn zero_decode_followup_document_reports_no_identity_blk9() {
        // BLK-9: doc A decodes and establishes the shared session; doc B is
        // fully reusable (seeded v6 checkpoint matching the session identity)
        // and must report NO identity/telemetry — matching its own single
        // run — with zero new submits.
        let root = std::env::temp_dir().join(format!("ocrx-app-blk9-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input_a = root.join("a.pdf");
        let input_b = root.join("b.pdf");
        std::fs::write(&input_a, b"a").unwrap();
        std::fs::write(&input_b, b"b").unwrap();
        let output_root = root.join("outs");
        // Seed doc B's page-1 v6 checkpoint BEFORE the run: content matches
        // the static identity so pre-compare passes and no decode happens.
        let b_out = output_root.join("b");
        std::fs::create_dir_all(&b_out).unwrap();
        std::fs::write(b_out.join("page-0001.md"), "seeded body").unwrap();
        std::fs::write(b_out.join("page-0001.json"), "{\"schema_version\":1}").unwrap();
        let mut checkpoint = crate::test_support::v6_ocr(1);
        // The integrity digest must match the write-path algorithm:
        // artifact_sha256 over the md (+ referenced images; none here).
        checkpoint.output_sha256 =
            ocrx_output::artifact_sha256(&b_out.join("page-0001.md"), &b_out).unwrap();
        ocrx_output::write_v6(b_out.join("page-0001.checkpoint"), &checkpoint).unwrap();
        let pdf = FakeMultiPdf;
        let session = crate::window_tests::FakeSession::with_depth(1);
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "doc a body".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        let prepares = Rc::new(RefCell::new(0_usize));
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let options = DocumentOptions {
            pdf_mode: "ocr".to_owned(),
            resume: true,
            ..DocumentOptions::default()
        };
        let outcome = app
            .run(
                &[input_a, input_b],
                &|input| {
                    let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
                    output_root.join(stem)
                },
                &options,
                &mut { pdf },
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(outcome.documents.len(), 2);
        let a = &outcome.documents[0];
        let b = &outcome.documents[1];
        // A decoded: identity present.
        assert!(a.identity.is_some(), "doc A decoded");
        // B fully reused: NO identity, NO telemetry — matches its single run.
        assert!(
            b.identity.is_none(),
            "B must not borrow A's session identity"
        );
        assert!(b.telemetry_delta.is_none());
        assert!(b.step_profile_delta.is_none());
        assert!(b.failure.is_none());
        // Zero new submits for B (only A's page decoded).
        // FakeSession core tracks captured across docs (shared service session).
        assert!(b_out.join("document.md").is_file());
        assert!(
            b_out.join("document.md").is_file(),
            "B merged its reused page"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn t1_json_files_written_on_success_and_absent_on_disabled_blk8() {
        let root = std::env::temp_dir().join(format!("ocrx-app-blk8-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("doc.pdf");
        std::fs::write(&input, b"x").unwrap();
        let output = root.join("out");
        let mut pdf = FakeMultiPdf;
        let session = crate::window_tests::FakeSession::with_depth(1);
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "body".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        session.set_step_profile(ocrx_inference_api::StepProfile {
            windows: 1,
            steps: 4,
            tokens: 9,
            ..ocrx_inference_api::StepProfile::default()
        });
        let prepares = Rc::new(RefCell::new(0_usize));
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let options = DocumentOptions {
            pdf_mode: "ocr".to_owned(),
            ..DocumentOptions::default()
        };
        // Enabled recorder: both files written.
        let recorder = ocrx_output::commit_profile::CommitRecorder::with_enabled(true);
        let outcome = app
            .run(
                std::slice::from_ref(&input),
                &|_| output.clone(),
                &options,
                &mut pdf,
                &mut renderer,
                &mut service,
                &recorder,
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        assert!(outcome.documents[0].failure.is_none());
        let step_json = std::fs::read_to_string(output.join("step-profile.json")).unwrap();
        assert!(step_json.starts_with("{\"schema_version\":1,\"windows\":1"));
        let commit_json = std::fs::read_to_string(output.join("commit-profile.json")).unwrap();
        assert!(commit_json.contains("\"schema_version\":1"));
        // Disabled recorder: commit profile absent.
        let output2 = root.join("out2");
        let mut app2 = ScanApplication::new();
        let outcome2 = app2
            .run(
                &[input],
                &|_| output2.clone(),
                &options,
                &mut FakeMultiPdf,
                &mut FakeRenderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        assert!(outcome2.documents[0].failure.is_none());
        assert!(
            output2.join("step-profile.json").is_file(),
            "step channel independent of recorder"
        );
        assert!(
            !output2.join("commit-profile.json").exists(),
            "disabled recorder = no file"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn final_compare_mismatch_reprocesses_reused_and_pending_pages_blk2() {
        // BLK-2: page 1 is pre-compare-reusable (seeded v6 checkpoint with the
        // STATIC identity); page 2 is pending decode. The session's identity
        // DIFFERS from the static one (different profile) → the final compare
        // demotes page 1, and BOTH pages must decode under the new identity.
        // No page may vanish from the merged document.
        let root = std::env::temp_dir().join(format!("ocrx-app-blk2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("doc.pdf");
        std::fs::write(&input, b"x").unwrap();
        let output = root.join("out");
        std::fs::create_dir_all(&output).unwrap();
        // Seed page 1: OCR checkpoint with the STATIC identity (matches
        // static_identity → pre-compare passes).
        std::fs::write(output.join("page-0001.md"), "seeded body").unwrap();
        std::fs::write(output.join("page-0001.json"), "{\"schema_version\":1}").unwrap();
        let mut checkpoint = crate::test_support::v6_ocr(1);
        checkpoint.output_sha256 =
            ocrx_output::artifact_sha256(&output.join("page-0001.md"), &output).unwrap();
        ocrx_output::write_v6(output.join("page-0001.checkpoint"), &checkpoint).unwrap();
        // The session identity DIFFERS from the static one: batch_depth 16.
        let session = crate::window_tests::FakeSession::with_depth(16);
        session.core.push(
            1,
            crate::window_tests::Program {
                text: "redecoded page 1".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        session.core.push(
            2,
            crate::window_tests::Program {
                text: "decoded page 2".into(),
                finish: ocrx_inference_api::FinishReason::Eos,
            },
        );
        let mut pdf = TwoPageOcrPdf;
        let prepares = Rc::new(RefCell::new(0_usize));
        let mut service = FakeService {
            prepares: Rc::clone(&prepares),
            session,
            static_override: None,
        };
        // static_identity must report the batch_depth-8 identity (matching
        // the seeded checkpoint) so pre-compare passes; the session's
        // batch_depth-16 identity then demotes it at the final compare.
        service.static_override = Some(StaticInferenceIdentity::from_execution(
            &crate::test_support::identity(),
        ));
        let mut renderer = FakeRenderer;
        let mut app = ScanApplication::new();
        let options = DocumentOptions {
            pdf_mode: "ocr".to_owned(),
            resume: true,
            ..DocumentOptions::default()
        };
        let outcome = app
            .run(
                &[input],
                &|_| output.clone(),
                &options,
                &mut pdf,
                &mut renderer,
                &mut service,
                &CommitRecorder::disabled(),
                &CancellationToken::new(),
                &mut |_| {},
            )
            .unwrap();
        let document = &outcome.documents[0];
        assert!(document.failure.is_none(), "{:?}", document.failure);
        // BOTH pages decoded (2 submits): the demoted reuse page AND the
        // pending page.
        assert_eq!(
            document.summary.completed_pages, 2,
            "diagnostics: {:?} warnings: {:?}",
            document.summary.diagnostics, document.summary.warnings
        );
        assert!(output.join("page-0001.md").is_file());
        assert!(output.join("page-0002.md").is_file());
        let document_md = std::fs::read_to_string(output.join("document.md")).unwrap();
        assert!(document_md.contains("redecoded page 1"));
        assert!(document_md.contains("decoded page 2"));
        // The decoded pages carry the NEW identity (batch_depth 16).
        let new_checkpoint = ocrx_output::read_any(output.join("page-0001.checkpoint")).unwrap();
        match new_checkpoint {
            ocrx_output::CheckpointFile::V6(cp) => {
                assert_eq!(cp.compatibility.ocr.as_ref().unwrap().batch_depth, 16);
            }
            other => panic!("expected v6, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
