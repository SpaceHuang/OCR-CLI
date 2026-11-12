//! Routing sidecars and diagnostics (plan §4.1 scan/report.rs, T066):
//! ported verbatim from the legacy CLI helpers (kept there until T074).
//! The JSON shapes are frozen (`ocrx-routing-checkpoint-v1`).

use std::collections::HashSet;
use std::path::Path;

use ocrx_core::{DomainError, PageClassification, PageType, PdfReasonCode, ProcessingMethod};

/// Planned method per mode (empty pages always plan empty).
#[must_use]
pub fn planned_method_for_mode(mode: &str, classification: PageClassification) -> ProcessingMethod {
    if classification.page_type == PageType::Empty {
        ProcessingMethod::Empty
    } else if mode == "ocr" {
        ProcessingMethod::Ocr
    } else {
        classification.recommended_method
    }
}

/// One frozen routing record: `{"page":N,"classification":…,"processing":…}`.
fn routing_record(
    page: u32,
    classification: PageClassification,
    processing: ocrx_core::PageProcessing,
) -> String {
    format!(
        "{{\"page\":{},\"classification\":{},\"processing\":{}}}",
        page,
        classification.to_json(),
        processing.to_json()
    )
}

/// Initial routing diagnostic (planned only).
#[must_use]
pub fn routing_diagnostic_json(page: u32, classification: PageClassification) -> String {
    routing_record(
        page,
        classification,
        ocrx_core::PageProcessing {
            planned_method: classification.recommended_method,
            actual_method: None,
            fallback_reason: None,
        },
    )
}

/// Routing diagnostic with a validated processing record.
///
/// # Errors
///
/// [`DomainError`] when the processing record is invalid.
pub fn try_routing_diagnostic_with_plan(
    page: u32,
    classification: PageClassification,
    planned_method: ProcessingMethod,
    actual_method: ProcessingMethod,
    fallback_reason: Option<PdfReasonCode>,
) -> Result<String, DomainError> {
    let processing = ocrx_core::PageProcessing {
        planned_method,
        actual_method: Some(actual_method),
        fallback_reason,
    };
    processing.validate()?;
    Ok(routing_record(page, classification, processing))
}

/// Final routing diagnostics for every selected page, honoring pending and
/// unavailable page states (plan §6.3: a pending page keeps its planned
/// method with no actual record).
#[must_use]
#[allow(clippy::implicit_hasher)] // the legacy CLI passes std HashSets
pub fn final_routing_diagnostics_with_pending(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, PdfReasonCode)],
    pending_pages: &HashSet<u32>,
    unavailable_pages: &HashSet<u32>,
) -> Vec<String> {
    pages
        .iter()
        .zip(classifications)
        .map(|(page, classification)| {
            if pending_pages.contains(&(*page as u32))
                || unavailable_pages.contains(&(*page as u32))
            {
                return routing_record(
                    *page as u32,
                    *classification,
                    ocrx_core::PageProcessing {
                        planned_method: planned_method_for_mode(mode, *classification),
                        actual_method: None,
                        fallback_reason: None,
                    },
                );
            }
            if let Some((_, reason)) = fallback_reasons
                .iter()
                .find(|(number, _)| *number == *page as u32)
            {
                try_routing_diagnostic_with_plan(
                    *page as u32,
                    *classification,
                    classification.recommended_method,
                    ProcessingMethod::Ocr,
                    Some(*reason),
                )
                .unwrap_or_else(|_| routing_diagnostic_json(*page as u32, *classification))
            } else {
                let planned = planned_method_for_mode(mode, *classification);
                try_routing_diagnostic_with_plan(
                    *page as u32,
                    *classification,
                    planned,
                    planned,
                    None,
                )
                .unwrap_or_else(|_| routing_diagnostic_json(*page as u32, *classification))
            }
        })
        .collect()
}

/// Writes one routing checkpoint sidecar per selected page (atomic).
///
/// # Errors
///
/// Underlying I/O errors.
#[allow(clippy::implicit_hasher)] // the legacy CLI passes std HashSets
pub fn write_routing_checkpoint_sidecars_with_states(
    output: &Path,
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, PdfReasonCode)],
    pending_pages: &HashSet<u32>,
    unavailable_pages: &HashSet<u32>,
) -> std::io::Result<()> {
    for (page, routing) in pages.iter().zip(final_routing_diagnostics_with_pending(
        mode,
        pages,
        classifications,
        fallback_reasons,
        pending_pages,
        unavailable_pages,
    )) {
        let sidecar = format!("{{\"schema\":\"ocrx-routing-checkpoint-v1\",\"routing\":{routing}}}");
        ocrx_output::atomic_write(
            output.join(format!("page-{page:04}.routing.json")),
            sidecar.as_bytes(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_core::{PageType, RouteConfidence};

    fn classification(method: ProcessingMethod, page_type: PageType) -> PageClassification {
        PageClassification {
            page_type,
            confidence: RouteConfidence::High,
            recommended_method: method,
            reason: PdfReasonCode::NativeTextReliable,
        }
    }

    #[test]
    fn sidecars_carry_the_frozen_schema_and_routing_shape() {
        let root = std::env::temp_dir().join(format!("ocrx-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pages = [1_usize, 2];
        let classifications = [
            classification(ProcessingMethod::NativeText, PageType::Text),
            classification(ProcessingMethod::Ocr, PageType::Image),
        ];
        write_routing_checkpoint_sidecars_with_states(
            &root,
            "auto",
            &pages,
            &classifications,
            &[],
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        let first = std::fs::read_to_string(root.join("page-0001.routing.json")).unwrap();
        assert!(first.starts_with("{\"schema\":\"ocrx-routing-checkpoint-v1\",\"routing\":{\"page\":1"));
        assert!(first.contains("\"planned_method\":\"native_text\""));
        let second = std::fs::read_to_string(root.join("page-0002.routing.json")).unwrap();
        assert!(second.contains("\"planned_method\":\"ocr\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fallback_pages_record_the_ocr_actual_with_the_reason() {
        let pages = [1_usize];
        let classifications = [classification(ProcessingMethod::NativeText, PageType::Text)];
        let diagnostics = final_routing_diagnostics_with_pending(
            "auto",
            &pages,
            &classifications,
            &[(1, PdfReasonCode::UnicodeValidityLow)],
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(diagnostics[0].contains("\"actual_method\":\"ocr\""));
        assert!(diagnostics[0].contains("\"fallback_reason\":\"unicode_validity_low\""));
    }

    #[test]
    fn pending_pages_keep_planned_only_records() {
        let pages = [1_usize];
        let classifications = [classification(ProcessingMethod::Ocr, PageType::Image)];
        let mut pending = HashSet::new();
        pending.insert(1_u32);
        let diagnostics = final_routing_diagnostics_with_pending(
            "auto",
            &pages,
            &classifications,
            &[],
            &pending,
            &HashSet::new(),
        );
        // None actual serializes as the explicit JSON null (frozen shape).
        assert!(diagnostics[0].contains("\"actual_method\":null"));
        assert!(diagnostics[0].contains("\"planned_method\":\"ocr\""));
    }
}
