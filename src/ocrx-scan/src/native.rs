//! Native-first routing and the final OCR queue (plan §6.3 steps 2/4/5,
//! T056/T057): ported verbatim from the legacy CLI helpers (kept there
//! until T074). Pure planning functions — commits go through caller
//! callbacks so the application entry owns the output I/O.

use crate::state::{merge_ocr_queue, OcrQueuePage};

/// Why a native candidate did not commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeValidationRoute {
    /// Validation passed; the page commits native.
    Native,
    /// Validation failed in auto mode: the page falls back to OCR.
    OcrFallback(OcrQueuePage),
    /// Validation failed in text mode: the whole scan terminates.
    TextFailure(String),
}

/// Routes one native-validation outcome per the frozen mode semantics.
#[must_use]
pub fn route_native_validation(
    mode: &str,
    page: u32,
    result: Result<(), String>,
) -> NativeValidationRoute {
    match result {
        Ok(()) => NativeValidationRoute::Native,
        Err(reason) if mode == "auto" => NativeValidationRoute::OcrFallback(OcrQueuePage {
            page,
            fallback_reason: Some(reason),
        }),
        Err(reason) => NativeValidationRoute::TextFailure(reason),
    }
}

/// Merges the initial OCR queue with the native-fallback pages.
#[must_use]
pub fn build_final_ocr_queue(
    initial: &[OcrQueuePage],
    native_routes: &[(u32, NativeValidationRoute)],
) -> Vec<OcrQueuePage> {
    let fallbacks = native_routes
        .iter()
        .filter_map(|(_, route)| match route {
            NativeValidationRoute::OcrFallback(page) => Some(page.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    merge_ocr_queue(initial, &fallbacks)
}

/// Outcome of the native-first phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFirstPhaseResult {
    pub committed_native_pages: Vec<u32>,
    pub fallback_pages: Vec<OcrQueuePage>,
}

/// Runs native candidates first: validation failures fall back in auto
/// mode and terminate in text mode; committed pages go through the caller
/// callback (which performs the atomic native commit).
///
/// # Errors
///
/// The text-mode failure message (page + reason).
pub fn run_native_first_phase(
    mode: &str,
    candidates: &[u32],
    validate: impl Fn(u32) -> Result<(), String>,
    mut commit: impl FnMut(u32) -> Result<(), String>,
) -> Result<NativeFirstPhaseResult, String> {
    let mut result = NativeFirstPhaseResult {
        committed_native_pages: Vec::new(),
        fallback_pages: Vec::new(),
    };
    for page in candidates {
        match route_native_validation(mode, *page, validate(*page)) {
            NativeValidationRoute::Native => {
                commit(*page)?;
                result.committed_native_pages.push(*page);
            }
            NativeValidationRoute::OcrFallback(queue_page) => {
                result.fallback_pages.push(queue_page)
            }
            NativeValidationRoute::TextFailure(reason) => {
                return Err(format!("page {page}: {reason}"));
            }
        }
    }
    Ok(result)
}

/// Whether the scan can end after the native phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOnlyDecision {
    Complete,
    ContinueToOcr,
}

/// A native-only scan completes when nothing fell back.
#[must_use]
pub fn native_only_decision(phase: &NativeFirstPhaseResult) -> NativeOnlyDecision {
    if phase.fallback_pages.is_empty() {
        NativeOnlyDecision::Complete
    } else {
        NativeOnlyDecision::ContinueToOcr
    }
}

/// The OCR queue after the native phase (initial + fallbacks, merged).
#[must_use]
pub fn ocr_queue_after_native_phase(
    initial: &[OcrQueuePage],
    phase: &NativeFirstPhaseResult,
) -> Vec<OcrQueuePage> {
    merge_ocr_queue(initial, &phase.fallback_pages)
}

/// Plain page numbers from a queue, sorted and deduplicated.
#[must_use]
pub fn service_pages_from_ocr_queue(queue: &[OcrQueuePage]) -> Vec<u32> {
    let mut pages = queue.iter().map(|page| page.page).collect::<Vec<_>>();
    pages.sort_unstable();
    pages.dedup();
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_mode_failure_terminates_and_auto_falls_back() {
        // auto: validation failure -> OCR fallback.
        match route_native_validation("auto", 3, Err("low coverage".into())) {
            NativeValidationRoute::OcrFallback(page) => {
                assert_eq!(page.page, 3);
                assert_eq!(page.fallback_reason.as_deref(), Some("low coverage"));
            }
            other => panic!("expected fallback, got {other:?}"),
        }
        // text: validation failure -> termination.
        match route_native_validation("text", 4, Err("bad unicode".into())) {
            NativeValidationRoute::TextFailure(reason) => {
                assert!(reason.contains("bad unicode"));
            }
            other => panic!("expected text failure, got {other:?}"),
        }
        // Passing validation commits native in both modes.
        assert!(matches!(
            route_native_validation("text", 1, Ok(())),
            NativeValidationRoute::Native
        ));
    }

    #[test]
    fn native_first_phase_commits_falls_back_and_terminates() {
        // auto mode: page 1 passes, page 2 falls back.
        let phase = run_native_first_phase(
            "auto",
            &[1, 2],
            |page| if page == 2 {
                Err("fallback reason".into())
            } else {
                Ok(())
            },
            |_page| Ok(()),
        )
        .unwrap();
        assert_eq!(phase.committed_native_pages, [1]);
        assert_eq!(phase.fallback_pages.len(), 1);
        assert_eq!(native_only_decision(&phase), NativeOnlyDecision::ContinueToOcr);

        // text mode: a failure aborts (the commit callback never runs past it).
        let error = run_native_first_phase(
            "text",
            &[1, 2],
            |page| if page == 2 {
                Err("no text".into())
            } else {
                Ok(())
            },
            |_page| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("page 2"));

        // All-pass phase completes natively.
        let phase =
            run_native_first_phase("auto", &[1], |_| Ok(()), |_| Ok(())).unwrap();
        assert_eq!(native_only_decision(&phase), NativeOnlyDecision::Complete);
    }

    #[test]
    fn final_queue_merges_initial_and_fallback_pages_sorted() {
        let initial = vec![OcrQueuePage {
            page: 5,
            fallback_reason: None,
        }];
        let routes = vec![
            (
                2_u32,
                NativeValidationRoute::OcrFallback(OcrQueuePage {
                    page: 2,
                    fallback_reason: Some("low".into()),
                }),
            ),
            (1_u32, NativeValidationRoute::Native),
        ];
        let queue = build_final_ocr_queue(&initial, &routes);
        let pages: Vec<u32> = queue.iter().map(|page| page.page).collect();
        assert_eq!(pages, [2, 5]);
        assert_eq!(service_pages_from_ocr_queue(&queue), [2, 5]);
        // The queue after the native phase is the same merge.
        let phase = run_native_first_phase(
            "auto",
            &[1, 2],
            |page| if page == 2 {
                Err("low".into())
            } else {
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();
        let queue = ocr_queue_after_native_phase(&initial, &phase);
        assert_eq!(service_pages_from_ocr_queue(&queue), [2, 5]);
    }
}
