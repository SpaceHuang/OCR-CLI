//! Page lifecycle, queues, progress, and the degeneration ladder table
//! (plan §4.1 scan/state.rs + ladder table Q1.4a).
//!
//! Copied from the legacy `ocrx-output` scan-service layer (T051
//! copy-first): the state machine, queue merges, and the frozen ladder are
//! scan-application data. The legacy originals stay in `ocrx-output` until
//! T088. `RetrySampling` comes from the inference API — the scan never
//! references runtime types.

use ocrx_inference_api::RetrySampling;
use ocrx_primitives::CancellationToken;
use std::io;

// The render audit is an output-persistence type (checkpoint fragment);
// the scan consumes it by value.
pub use ocrx_output::RenderAudit;

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
    pub sampling: Option<RetrySampling>,
    /// Q1.1c candidate C (D-10): whitespace-valley column-split pass - the
    /// page is re-decoded per detected column band and the band markdowns
    /// merge in reading order. The heaviest rung: final position only.
    pub column_split: bool,
}

impl DegenerationRetryStep {
    /// Plain-render + plain-decode perturbation rung (kept for future
    /// ladder adjustments; not part of the frozen table).
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

/// Degeneration retry ladder (Q1.4a frozen composition; the perturbation
/// rungs were excluded by measured data: 0 escapes as baseline and as tail
/// rungs, dropping them cuts worst-case retry cost 40% with zero outcome
/// change). Rungs in order from attempt 2 on; attempt cap = 1 + len = 4.
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
        sampling: Some(RetrySampling {
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

/// Frozen attempts range written to checkpoints: 1 + ladder length (Q1.4a).
pub const ATTEMPTS_MAX: u32 = 1 + DEGENERATION_RETRY_LADDER.len() as u32;

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
    /// Creates a pending lifecycle entry.
    ///
    /// # Errors
    ///
    /// [`io::Error`] for a zero page number.
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

    /// Applies one state transition.
    ///
    /// # Errors
    ///
    /// [`io::Error`] for an invalid transition.
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
pub struct PageDiagnostic {
    pub page: u32,
    pub status: String,
    pub stage: String,
    pub error_code: Option<String>,
    pub message: Option<String>,
    pub audit: Option<RenderAudit>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSummary {
    pub selected_pages: u32,
    pub completed_pages: u32,
    pub failed_pages: u32,
    pub truncated_pages: u32,
    pub degenerated_pages: u32,
    /// Think removals summed over all committed/skipped pages (Q3.1).
    pub think_blocks_stripped: u64,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<PageDiagnostic>,
    pub cancelled: bool,
}

#[must_use]
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

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            interval_seconds: 1,
            last_emit: None,
        }
    }
}

/// Cooperative scan controller: page lifecycles, shared cancellation, and
/// the heartbeat cadence. It owns no model, no renderer, and no I/O.
#[derive(Debug)]
pub struct ScanController {
    pages: Vec<PageLifecycle>,
    cancellation: CancellationToken,
    heartbeat: Heartbeat,
}

impl ScanController {
    /// Builds a controller over the selected pages.
    ///
    /// # Errors
    ///
    /// [`io::Error`] when any page number is zero.
    pub fn new(page_numbers: &[u32]) -> io::Result<Self> {
        let mut pages = Vec::with_capacity(page_numbers.len());
        for page in page_numbers {
            pages.push(PageLifecycle::new(*page)?);
        }
        Ok(Self {
            pages,
            cancellation: CancellationToken::new(),
            heartbeat: Heartbeat::default(),
        })
    }

    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    #[must_use]
    pub fn pages(&self) -> &[PageLifecycle] {
        &self.pages
    }

    /// Applies one transition to a page.
    ///
    /// # Errors
    ///
    /// [`io::Error`] for unknown pages or invalid transitions.
    pub fn transition(&mut self, page: u32, next: PageStatus) -> io::Result<()> {
        let lifecycle = self
            .pages
            .iter_mut()
            .find(|lifecycle| lifecycle.page_number == page)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown page"))?;
        lifecycle.transition(next)
    }

    /// Emits a heartbeat when the interval elapsed (progress contract).
    #[must_use]
    pub fn heartbeat(&mut self, snapshot: ProgressSnapshot) -> Option<ProgressSnapshot> {
        let elapsed = snapshot.elapsed_seconds;
        let due = self
            .heartbeat
            .last_emit
            .is_none_or(|last| elapsed.saturating_sub(last) >= self.heartbeat.interval_seconds);
        if due {
            self.heartbeat.last_emit = Some(elapsed);
            Some(snapshot)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgb8: Vec<u8>,
    pub audit: RenderAudit,

    /// Scan-local monotonic render generation: the commit path fetches the
    /// PNG bytes encoded for THIS render, so a degenerated retry's bytes
    /// can never be the first attempt's (phase-4 W3 semantics).
    pub generation: u64,
}

impl RenderedPage {
    /// Validates and builds a rendered page.
    ///
    /// # Errors
    ///
    /// [`io::Error`] for zero dimensions or an RGB8 length mismatch.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageArtifactResult {
    /// The production recognizer does not encode: the PNG comes from the
    /// encode pipeline (overlapped with the window decode). `None` is the
    /// production shape; `Some` keeps fakes and pipeline-less callers
    /// working (inline fallback at commit).
    pub png: Option<Vec<u8>>,
    pub markdown: String,
    pub page_json: String,
    pub truncated: bool,
    pub degenerated: bool,
    pub warnings: Vec<String>,
    /// Think removals at the adapter artifact entry (Q3.1).
    pub think_blocks_stripped: u32,
    /// Pre-crop normalized raw output for the `page-NNNN.raw` sidecar.
    pub raw_output: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_ladder_has_three_rungs_in_the_q14a_order() {
        assert_eq!(DEGENERATION_RETRY_LADDER.len(), 3);
        assert_eq!(ATTEMPTS_MAX, 4);
        // Rung 1: 2048 grid.
        assert_eq!(DEGENERATION_RETRY_LADDER[0].image_tokens, Some(2048));
        assert!(DEGENERATION_RETRY_LADDER[0].sampling.is_none());
        // Rung 2: penalty 1200 milli / window 64.
        assert_eq!(
            DEGENERATION_RETRY_LADDER[1].sampling,
            Some(RetrySampling {
                repeat_penalty_milli: 1_200,
                penalty_window: 64,
            })
        );
        // Rung 3: column split, final position only.
        assert!(DEGENERATION_RETRY_LADDER[2].column_split);
        // No perturbation rungs survive in the frozen table.
        assert!(
            DEGENERATION_RETRY_LADDER
                .iter()
                .all(|rung| rung.perturbed.is_none())
        );
    }

    #[test]
    fn lifecycle_transitions_follow_the_state_machine() {
        let mut page = PageLifecycle::new(1).unwrap();
        assert!(PageLifecycle::new(0).is_err());
        assert!(page.transition(PageStatus::Completed).is_err());
        page.transition(PageStatus::Processing).unwrap();
        page.transition(PageStatus::Truncated).unwrap();
        assert!(page.transition(PageStatus::Completed).is_err());
    }

    #[test]
    fn queue_merges_are_sorted_deduplicated_and_keep_first_reasons() {
        let initial = vec![
            OcrQueuePage {
                page: 3,
                fallback_reason: None,
            },
            OcrQueuePage {
                page: 1,
                fallback_reason: None,
            },
        ];
        let fallback = vec![
            OcrQueuePage {
                page: 2,
                fallback_reason: Some("low coverage".to_owned()),
            },
            OcrQueuePage {
                page: 3,
                fallback_reason: Some("late reason".to_owned()),
            },
        ];
        let merged = merge_ocr_queue(&initial, &fallback);
        let pages: Vec<u32> = merged.iter().map(|page| page.page).collect();
        assert_eq!(pages, [1, 2, 3]);
        // A None initial reason is filled by the fallback's reason (the
        // legacy merge semantics: fallbacks complete, never override).
        assert_eq!(merged[2].fallback_reason.as_deref(), Some("late reason"));
        // Fallback-only entries carry their reason.
        assert_eq!(merged[1].fallback_reason.as_deref(), Some("low coverage"));

        let candidates = vec![(5, Err("no text".to_owned())), (5, Ok(())), (4, Ok(()))];
        let queue = native_fallback_queue(&candidates);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].page, 5);
    }

    #[test]
    fn controller_gates_transitions_and_heartbeats() {
        let mut controller = ScanController::new(&[1, 2]).unwrap();
        assert!(controller.transition(3, PageStatus::Processing).is_err());
        controller.transition(1, PageStatus::Processing).unwrap();
        assert_eq!(controller.pages()[0].status, PageStatus::Processing);
        // First heartbeat always emits; a same-second one does not.
        let snapshot = ProgressSnapshot {
            completed: 1,
            total: 2,
            elapsed_seconds: 5,
            eta_seconds: Some(5),
        };
        assert!(controller.heartbeat(snapshot).is_some());
        assert!(
            controller
                .heartbeat(ProgressSnapshot {
                    elapsed_seconds: 5,
                    ..snapshot
                })
                .is_none()
        );
        assert!(
            controller
                .heartbeat(ProgressSnapshot {
                    elapsed_seconds: 6,
                    ..snapshot
                })
                .is_some()
        );
    }

    #[test]
    fn rendered_page_validates_rgb8_and_carries_the_audit() {
        assert!(RenderedPage::new(2, 2, vec![0; 3]).is_err());
        let page = RenderedPage::new(2, 2, vec![0; 12]).unwrap();
        assert_eq!(page.generation, 0);
        assert_eq!(page.audit.scale_ratio_numerator, 1);
        let audit = page.audit;
        let audited = page.with_audit(RenderAudit {
            original_width: 100,
            ..audit
        });
        assert_eq!(audited.audit.original_width, 100);
        assert!(
            audited
                .audit
                .as_checkpoint_fragment()
                .starts_with("original=100x")
        );
    }

    #[test]
    fn job_summary_merge_adds_counts_and_unions_warnings() {
        let native = JobSummary {
            selected_pages: 2,
            completed_pages: 2,
            failed_pages: 0,
            truncated_pages: 0,
            degenerated_pages: 0,
            think_blocks_stripped: 3,
            warnings: vec!["a".to_owned()],
            diagnostics: vec![],
            cancelled: false,
        };
        let ocr = JobSummary {
            selected_pages: 3,
            completed_pages: 1,
            failed_pages: 1,
            truncated_pages: 1,
            degenerated_pages: 1,
            think_blocks_stripped: 5,
            warnings: vec!["b".to_owned()],
            diagnostics: vec![],
            cancelled: true,
        };
        let merged = merge_job_summaries(native, ocr);
        assert_eq!(merged.selected_pages, 3);
        assert_eq!(merged.completed_pages, 3);
        assert_eq!(merged.failed_pages, 1);
        assert_eq!(merged.truncated_pages, 1);
        assert_eq!(merged.degenerated_pages, 1);
        assert_eq!(merged.think_blocks_stripped, 8);
        assert_eq!(merged.warnings, vec!["a", "b"]);
        assert!(merged.cancelled);
    }
}
