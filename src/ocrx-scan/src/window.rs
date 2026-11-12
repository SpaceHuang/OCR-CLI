//! Windowed OCR orchestration (plan §4.1 scan/window.rs, T063): fixed
//! depth from the session identity, page-ordered fills with no dynamic
//! backfill, resume-page draining, batch-level depth-1 fallback, the
//! degeneration ladder, and mechanism-A window-atomic v6 commits.
//!
//! This is the session-driven port of the legacy `ScanService`
//! `run_pages_with_model` loop; the frozen behavior is identical except
//! that the request/response types come from the public inference API and
//! the checkpoints are v6.

use std::path::Path;

use ocrx_inference_api::InferenceSession;
use ocrx_output::{
    ScanCompatibility, ScanOcrSide, V6Checkpoint, V6Status, atomic_write, cleanup_full_page_images,
    commit_page_v6, read_any, write_render_audit, write_v6,
};
use ocrx_primitives::{CancellationToken, GenerationProgress};

use crate::ocr::artifacts::{RecognizeError, SessionRecognizer};
use crate::state::{
    DEGENERATION_RETRY_LADDER, DegenerationRetryStep, JobSummary, PageArtifactResult,
    PageDiagnostic, RenderAudit, RenderedPage,
};

/// Render port for the OCR pipeline (the legacy `PageRenderer` trait,
/// moved to the scan app): attempt-1 windows render plain; perturbed
/// re-renders stay available for future ladder adjustments.
pub trait PageRenderer {
    /// One-time availability check.
    ///
    /// # Errors
    ///
    /// Renderer-specific setup failure.
    fn preflight(&mut self) -> std::io::Result<()>;

    /// Renders one page at the configured DPI/budget.
    ///
    /// # Errors
    ///
    /// Rendering failure.
    fn render(&mut self, page: u32) -> std::io::Result<RenderedPage>;

    /// Renders a perturbed variant (future ladder rungs; the frozen table
    /// never uses it). Default: plain render.
    ///
    /// # Errors
    ///
    /// Rendering failure.
    fn render_perturbed(
        &mut self,
        page: u32,
        numerator: u32,
        denominator: u32,
    ) -> std::io::Result<RenderedPage> {
        let _ = (numerator, denominator);
        self.render(page)
    }
}

impl<T: PageRenderer + ?Sized> PageRenderer for Box<T> {
    fn preflight(&mut self) -> std::io::Result<()> {
        (**self).preflight()
    }

    fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
        (**self).render(page)
    }
}

/// One window page at its terminal state.
pub(crate) enum TerminalPage {
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
        error: String,
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

enum SerialOutcome {
    RenderFailed { cancelled: bool },
    Terminal(Result<(RenderedPage, PageArtifactResult, u32), (Option<RenderAudit>, String)>),
}

/// Window-loop inputs shared across documents (multi-doc reuses one session
/// and one config; windows stay per-document).
pub struct WindowConfig<'a> {
    pub compatibility: &'a ScanCompatibility,
    pub resume: bool,
    pub keep_pages: bool,
    /// Ladder rung 3 gate (`--column-split-retry`, default on).
    pub column_split_enabled: bool,
    /// BLK-3: the user's output-token budget — rides EVERY request (single,
    /// window, retry, column split), not just the checkpoint identity.
    pub max_output_tokens: u32,
}

/// Runs the windowed OCR pipeline over the prepared session (T063).
///
/// # Errors
///
/// I/O failures from the commit path (per-page recognition failures are
/// recorded as diagnostics and do not abort).
#[allow(clippy::too_many_arguments)]
pub fn run_pages_with_session(
    session: &mut dyn InferenceSession,
    renderer: &mut dyn PageRenderer,
    page_numbers: &[u32],
    output_root: &Path,
    config: &WindowConfig<'_>,
    commit_recorder: &ocrx_output::commit_profile::CommitRecorder,
    cancellation: &CancellationToken,
    heartbeat: &mut dyn FnMut(u32, GenerationProgress),
) -> std::io::Result<JobSummary> {
    let root = output_root;
    std::fs::create_dir_all(root)?;
    // The window depth is the ACTUAL identity value (never a device guess).
    let window_depth = session.identity().batch_depth.max(1);
    let mut completed = 0_u32;
    let mut failed = 0_u32;
    let mut truncated = 0_u32;
    let mut degenerated = 0_u32;
    let mut think_blocks_stripped = 0_u64;
    let mut warnings = std::collections::BTreeSet::new();
    let mut diagnostics = Vec::new();

    let mut terminal: Vec<TerminalPage> = Vec::new();
    let mut cursor = 0usize;
    while cursor < page_numbers.len() {
        if cancellation.is_cancelled() {
            break;
        }
        // Start (or extend) a window: leading resume-skipped pages drain
        // one by one; live pages join until the window is full.
        let mut window: Vec<u32> = Vec::with_capacity(window_depth);
        while cursor < page_numbers.len() && window.len() < window_depth {
            let page = page_numbers[cursor];
            cursor += 1;
            let checkpoint_path = root.join(format!("page-{page:04}.checkpoint"));
            let mut skipped = false;
            if config.resume
                && let Ok(file) = read_any(&checkpoint_path)
            {
                // The window loop runs post-prepare: the final compare uses
                // the executing session identity (RBLK-6/8).
                let context = crate::resume::ResumeContext {
                    input_sha256: &config.compatibility.input_sha256,
                    pdf: &config.compatibility.pdf,
                    side: &resume_side(config.compatibility),
                    statik: None,
                    session_identity: Some(session.identity()),
                    has_pending_decode: true,
                };
                let disposition = crate::resume::classify(&file, root, &context);
                if matches!(disposition, crate::resume::PageDisposition::Reuse { .. }) {
                    let crate::resume::PageDisposition::Reuse {
                        think_blocks_stripped: count,
                        truncated: was_truncated,
                        warnings: page_warnings,
                    } = disposition
                    else {
                        unreachable!("just matched");
                    };
                    if was_truncated {
                        truncated += 1;
                    }
                    think_blocks_stripped += u64::from(count);
                    warnings.extend(page_warnings);
                    diagnostics.push(PageDiagnostic {
                        page,
                        status: "completed".to_owned(),
                        stage: "resume".to_owned(),
                        error_code: None,
                        message: None,
                        audit: None,
                        warnings: Vec::new(),
                    });
                    completed += 1;
                    skipped = true;
                }
            }
            if !skipped {
                window.push(page);
            }
        }
        if window.is_empty() {
            break;
        }

        // Depth 1: the true serial path (numerically identical to the
        // pre-batching loop, no window context allocated).
        if window.len() == 1 {
            let page = window[0];
            terminal.push(
                match serial_attempt_outcome(
                    session,
                    renderer,
                    root,
                    config,
                    commit_recorder,
                    page,
                    1,
                    cancellation,
                    &mut |event| heartbeat(page, event),
                ) {
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
                        Err((audit, error)) => TerminalPage::RecognizeFailed { page, audit, error },
                    },
                },
            );
            if cancellation.is_cancelled() {
                // A cancelled window commits nothing (mechanism A).
                terminal.clear();
                break;
            }
            commit_terminal(
                &terminal,
                root,
                config.compatibility,
                commit_recorder,
                cancellation,
                &mut completed,
                &mut failed,
                &mut truncated,
                &mut degenerated,
                &mut think_blocks_stripped,
                &mut warnings,
                &mut diagnostics,
            )?;
            terminal.clear();
            continue;
        }

        // Attempt-1 renders in page order (a render failure is terminal
        // for that page, exactly like the serial loop).
        let mut images: Vec<(u32, RenderedPage)> = Vec::with_capacity(window.len());
        let mut render_failed: std::collections::BTreeMap<u32, bool> =
            std::collections::BTreeMap::new();
        for &page in &window {
            // T1: one render row per attempt-1 batch member.
            let generation = commit_recorder.begin_render(page);
            let render_started = std::time::Instant::now();
            let outcome = renderer.render(page);
            commit_recorder.record_render_wait(
                page,
                generation,
                u64::try_from(render_started.elapsed().as_micros()).unwrap_or(u64::MAX),
            );
            match outcome {
                Ok(image) => images.push((page, image)),
                Err(_) => {
                    render_failed.insert(page, cancellation.is_cancelled());
                }
            }
        }

        // Batched attempt-1 recognition; results keyed by page.
        let mut batch_results: std::collections::BTreeMap<
            u32,
            Result<PageArtifactResult, RecognizeError>,
        > = std::collections::BTreeMap::new();
        if !images.is_empty() {
            let mut recognizer = SessionRecognizer::new(session, root, config.max_output_tokens);
            let results = recognizer.recognize_window(&images, cancellation, &mut |slot, event| {
                heartbeat(u32::try_from(slot).unwrap_or(u32::MAX), event);
            });
            for ((page, _), result) in images.iter().zip(results) {
                batch_results.insert(*page, result);
            }
        }

        // Resolve the window. Batch-level failures rerun the whole window
        // single-stream; degenerated/truncated pages continue their ladder
        // single-stream; everything else is terminal as-is.
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
                    if matches!(error, RecognizeError::WindowFailed(_)) {
                        // Depth-1 fallback for the whole window: every
                        // still-pending page reruns the serial path from
                        // attempt 1.
                        for pending in window.iter().copied() {
                            if render_failed.contains_key(&pending)
                                || terminal.iter().any(|entry| entry.page() == pending)
                            {
                                continue;
                            }
                            batch_results.remove(&pending);
                            terminal.push(
                                match serial_attempt_outcome(
                                    session,
                                    renderer,
                                    root,
                                    config,
                                    commit_recorder,
                                    pending,
                                    1,
                                    cancellation,
                                    &mut |event| heartbeat(pending, event),
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
                                        Err((audit, msg)) => TerminalPage::RecognizeFailed {
                                            page: pending,
                                            audit,
                                            error: msg,
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
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            if artifact.degenerated || artifact.truncated {
                // Q1.5/D-14: truncated window pages retry through the
                // ladder exactly like degenerated ones.
                match serial_attempt_outcome(
                    session,
                    renderer,
                    root,
                    config,
                    commit_recorder,
                    page,
                    2,
                    cancellation,
                    &mut |event| heartbeat(page, event),
                ) {
                    SerialOutcome::RenderFailed { cancelled } => {
                        terminal.push(TerminalPage::RenderFailed { page, cancelled });
                    }
                    SerialOutcome::Terminal(outcome) => match outcome {
                        Ok((image, retried, attempts)) => terminal.push(TerminalPage::Committed {
                            page,
                            image,
                            artifact: retried,
                            attempts,
                        }),
                        Err((audit, msg)) => terminal.push(TerminalPage::RecognizeFailed {
                            page,
                            audit,
                            error: msg,
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
        // window completed leaves zero commits and resume rescans it whole.
        if cancellation.is_cancelled() {
            terminal.clear();
            break;
        }
        commit_terminal(
            &terminal,
            root,
            config.compatibility,
            commit_recorder,
            cancellation,
            &mut completed,
            &mut failed,
            &mut truncated,
            &mut degenerated,
            &mut think_blocks_stripped,
            &mut warnings,
            &mut diagnostics,
        )?;
        terminal.clear();
    }

    // BLK-5: WITHOUT --keep-pages the full-page PNGs are dropped after the
    // merge (figure crops stay); with it, they are kept.
    if !config.keep_pages {
        let _ = cleanup_full_page_images(root, page_numbers);
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

/// The resume-side snapshot for the window-loop final compare (scan-side
/// fields are constant within the run; derived from the compatibility).
fn resume_side(compatibility: &ScanCompatibility) -> ScanOcrSide {
    match &compatibility.ocr {
        Some(ocr) => ScanOcrSide {
            prompt_id: ocr.prompt_id.clone(),
            prompt_version: ocr.prompt_version.clone(),
            prompt_schema_version: ocr.prompt_schema_version,
            dpi: ocr.dpi,
            max_image_tokens: ocr.max_image_tokens,
            max_output_tokens: ocr.max_output_tokens,
            context_size: ocr.context_size,
            pixel_width_budget: ocr.pixel_width_budget,
            pixel_height_budget: ocr.pixel_height_budget,
            page_parser_version: ocr.page_parser_version.clone(),
        },
        None => ScanOcrSide {
            prompt_id: String::new(),
            prompt_version: String::new(),
            prompt_schema_version: 0,
            dpi: 0,
            max_image_tokens: 0,
            max_output_tokens: 0,
            context_size: 0,
            pixel_width_budget: 0,
            pixel_height_budget: 0,
            page_parser_version: String::new(),
        },
    }
}

/// Single-page attempt machine (Q1.4a): attempt 1 plain greedy; attempts
/// 2..=4 walk the frozen ladder (2048 grid → penalty chain → column split,
/// gated). Truncated pages enter the ladder too (Q1.5/D-14).
#[allow(clippy::too_many_arguments)]
fn serial_attempt_outcome(
    session: &mut dyn InferenceSession,
    renderer: &mut dyn PageRenderer,
    root: &Path,
    config: &WindowConfig<'_>,
    commit_recorder: &ocrx_output::commit_profile::CommitRecorder,
    page: u32,
    start_attempt: u32,
    cancellation: &CancellationToken,
    heartbeat: &mut dyn FnMut(GenerationProgress),
) -> SerialOutcome {
    let mut attempts = start_attempt.saturating_sub(1);
    loop {
        attempts += 1;
        let step: Option<&DegenerationRetryStep> = if attempts == 1 {
            None
        } else {
            let ladder_index = usize::try_from(attempts - 2).unwrap_or(usize::MAX);
            DEGENERATION_RETRY_LADDER.get(ladder_index)
        };
        // T1: the caller-side render wait is the recorder's R input; every
        // attempt (retry perturbations included) gets its own row.
        let generation = commit_recorder.begin_render(page);
        let render_started = std::time::Instant::now();
        let image = match step.and_then(|rung| rung.perturbed) {
            Some((numerator, denominator)) => {
                renderer.render_perturbed(page, numerator, denominator)
            }
            None => renderer.render(page),
        };
        commit_recorder.record_render_wait(
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
        let mut recognizer = SessionRecognizer::new(session, root, config.max_output_tokens);
        // Rung 3 (column split) is gated by the CLI switch; a closed gate
        // exhausts the ladder for the page.
        // `step`/`recognized` are the ladder attempt pair.
        #[allow(clippy::similar_names)]
        let recognized_outcome = match step {
            Some(rung) if rung.column_split && config.column_split_enabled => {
                recognizer.recognize_column_split(page, &image, cancellation, heartbeat)
            }
            Some(rung) if rung.column_split => Err(RecognizeError::Failed(std::io::Error::other(
                "column split disabled",
            ))),
            Some(rung) => recognizer.recognize_retry(page, &image, rung, cancellation, heartbeat),
            None => recognizer.recognize(page, &image, cancellation, heartbeat),
        };
        match recognized_outcome {
            Ok(artifact) => {
                // Q1.5/D-14: pure truncation enters the ladder too.
                let ladder_worthy = artifact.degenerated || artifact.truncated;
                if ladder_worthy && attempts <= DEGENERATION_RETRY_LADDER.len() as u32 {
                    continue;
                }
                return SerialOutcome::Terminal(Ok((image, artifact, attempts)));
            }
            Err(error) => {
                return SerialOutcome::Terminal(Err((Some(image.audit), error.to_string())));
            }
        }
    }
}

/// Mechanism-A window-atomic commit (v6): nothing commits until every
/// window page reached a terminal state; a cancelled window commits zero.
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_terminal(
    terminal: &[TerminalPage],
    root: &Path,
    compatibility: &ScanCompatibility,
    commit_recorder: &ocrx_output::commit_profile::CommitRecorder,
    cancellation: &CancellationToken,
    completed: &mut u32,
    failed: &mut u32,
    truncated: &mut u32,
    degenerated: &mut u32,
    think_blocks_stripped: &mut u64,
    warnings: &mut std::collections::BTreeSet<String>,
    diagnostics: &mut Vec<PageDiagnostic>,
) -> std::io::Result<()> {
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
                // Raw-output sidecar: preserved whenever the page
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
                let checkpoint = V6Checkpoint {
                    page_number: page,
                    status: V6Status::Complete,
                    compatibility: compatibility.clone(),
                    output_sha256: String::new(),
                    truncated: artifact.truncated,
                    degenerated: artifact.degenerated,
                    attempts,
                    raw: raw_pointer,
                    warnings: artifact_warnings.clone(),
                    think_blocks_stripped: artifact.think_blocks_stripped,
                    audit: Some(image.audit),
                };
                // T1: the write span covers the atomic writes through the
                // checkpoint rename inside commit_page_v6.
                let write_started = std::time::Instant::now();
                // BLK-1: the full-page PNG is the terminal render's real
                // bytes — pre-encoded artifact bytes when a fake supplies
                // them, otherwise a synchronous encode of this attempt's
                // rgb8 (a retry commit can never reuse an earlier
                // attempt's image because the terminal image rides the
                // terminal entry).
                // RBLK-4: an encode error is a page-level failure (frozen
                // surface) — never a scan I/O abort; the remaining terminal
                // pages still commit.
                let png_bytes = match artifact.png.as_ref() {
                    Some(bytes) if !bytes.is_empty() => bytes.clone(),
                    _ => match encode_page_png(image) {
                        Ok(bytes) => bytes,
                        Err(encode_error) => {
                            *failed += 1;
                            commit_recorder.mark_terminal(page);
                            diagnostics.push(PageDiagnostic {
                                page,
                                status: "failed".to_owned(),
                                stage: "recognize".to_owned(),
                                error_code: Some("recognition_failed".to_owned()),
                                message: Some(format!("page recognition failed: {encode_error}")),
                                audit: Some(image.audit),
                                warnings: Vec::new(),
                            });
                            continue;
                        }
                    },
                };
                commit_recorder.record_png_encode(
                    page,
                    u64::try_from(write_started.elapsed().as_micros()).unwrap_or(u64::MAX),
                );
                match commit_page_v6(
                    root,
                    page,
                    &png_bytes,
                    &artifact.markdown,
                    &artifact.page_json,
                    &checkpoint,
                ) {
                    Ok(()) => {
                        commit_recorder.record_commit_write(
                            page,
                            u64::try_from(write_started.elapsed().as_micros()).unwrap_or(u64::MAX),
                        );
                        commit_recorder.mark_terminal(page);
                        diagnostics.push(PageDiagnostic {
                            page,
                            status: "completed".to_owned(),
                            stage: "commit".to_owned(),
                            error_code: None,
                            message: None,
                            audit: Some(image.audit),
                            warnings: artifact_warnings.clone(),
                        });
                        *completed += 1;
                    }
                    Err(error) => {
                        *failed += 1;
                        diagnostics.push(PageDiagnostic {
                            page,
                            status: "failed".to_owned(),
                            stage: "commit".to_owned(),
                            error_code: Some("recognition_failed".to_owned()),
                            message: Some(format!("page commit failed: {error}")),
                            audit: Some(image.audit),
                            warnings: Vec::new(),
                        });
                    }
                }
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
                diagnostics.push(PageDiagnostic {
                    page,
                    status: if cancelled {
                        "cancelled".to_owned()
                    } else {
                        "failed".to_owned()
                    },
                    stage: "recognize".to_owned(),
                    error_code: Some("recognition_failed".to_owned()),
                    message: Some(format!("page recognition failed: {error}")),
                    audit,
                    warnings: Vec::new(),
                });
            }
        }
    }
    Ok(())
}

/// Encodes one rendered page's RGB8 into PNG bytes (synchronous commit-path
/// fallback for the retired async encode pipeline; phase-4 W3 semantics:
/// the bytes belong to the terminal render recorded in the entry).
fn encode_page_png(image: &RenderedPage) -> std::io::Result<Vec<u8>> {
    let img = image::RgbImage::from_raw(image.width, image.height, image.rgb8.clone())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid RGB8 page"))?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    img.write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let bytes = cursor.into_inner();
    if bytes.is_empty() {
        return Err(std::io::Error::other("png encoder produced empty output"));
    }
    Ok(bytes)
}

/// Writes a pending-OCR checkpoint from the scan side (native fallback and
/// queue handoff); the checkpoint carries the document's compatibility —
/// the executing identity is written when the page actually decodes.
///
/// # Errors
///
/// Underlying I/O errors.
pub fn write_pending_ocr(
    root: &Path,
    page: u32,
    compatibility: &ScanCompatibility,
    reason: &str,
) -> std::io::Result<()> {
    write_v6(
        root.join(format!("page-{page:04}.checkpoint")),
        &V6Checkpoint {
            page_number: page,
            status: V6Status::PendingOcr,
            compatibility: compatibility.clone(),
            output_sha256: String::new(),
            truncated: false,
            degenerated: false,
            attempts: 1,
            raw: None,
            warnings: vec![reason.to_owned()],
            think_blocks_stripped: 0,
            audit: None,
        },
    )
}
