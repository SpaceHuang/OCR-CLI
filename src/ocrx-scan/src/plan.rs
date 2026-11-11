//! Input planning and preflight validation (plan §4.1 scan/plan.rs, T053).
//!
//! Ported verbatim from the legacy CLI helpers (kept there until T074):
//! multi-input resolution (X1.1), page selection, numeric validation, and
//! the output policy. Pure functions — no I/O beyond the declared input
//! directory reads and canonicalization.

use std::path::{Path, PathBuf};

use ocrx_core::{PageClassification, PageType, ProcessingMethod};

/// Multi-input resolution (X1.1): directory expansion (sorted `*.pdf`, no
/// recursion), file passthrough, canonical dedup, and the frozen usage
/// errors.
///
/// # Errors
///
/// Usage-error strings identical to the legacy CLI (exit code 2).
pub fn resolve_scan_inputs(raw: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut resolved: Vec<PathBuf> = Vec::new();
    for entry in raw {
        let path = PathBuf::from(entry);
        if path.is_dir() {
            let mut pdfs: Vec<PathBuf> = std::fs::read_dir(&path)
                .map_err(|error| format!("cannot read input directory {entry}: {error}"))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|candidate| {
                    candidate.is_file()
                        && candidate
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
                })
                .collect();
            pdfs.sort();
            if pdfs.is_empty() {
                return Err(format!("input directory contains no PDFs: {entry}"));
            }
            resolved.extend(pdfs);
        } else if path.is_file() {
            resolved.push(path);
        } else {
            return Err(format!("input not found: {entry}"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::with_capacity(resolved.len());
    for path in resolved {
        let canonical = std::fs::canonicalize(&path)
            .map_err(|error| format!("cannot resolve input {}: {error}", path.display()))?;
        if seen.insert(canonical) {
            unique.push(path);
        }
    }
    Ok(unique)
}

/// Whether the planned pages require the OCR resource lifecycle.
#[must_use]
pub fn planned_ocr_pages(mode: &str, pages: &[PageClassification]) -> bool {
    mode == "ocr"
        || pages.iter().any(|page| match page.recommended_method {
            ProcessingMethod::Ocr => !matches!(page.page_type, PageType::Empty),
            _ => false,
        })
}

/// Whether a native-fallback OCR queue with invalid assets must fail the
/// dynamic fallback path.
#[must_use]
pub fn dynamic_fallback_requires_model_assets(
    initial_ocr_pages: usize,
    final_ocr_pages: usize,
    assets_valid: bool,
) -> bool {
    initial_ocr_pages == 0 && final_ocr_pages > 0 && !assets_valid
}

/// Numeric option validation (frozen ranges; the over-cap budget fails
/// once before the scan starts, D5.1).
///
/// # Errors
///
/// Usage-error strings identical to the legacy CLI (exit code 2).
pub fn validate_numeric_options(
    dpi: Option<u32>,
    max_image_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    context_size: Option<u32>,
) -> Result<(), String> {
    if dpi.is_some_and(|value| !(1..=600).contains(&value)) {
        return Err("dpi must be between 1 and 600".to_owned());
    }
    if max_image_tokens.is_some_and(|value| value < 128) {
        return Err("max image tokens must be at least 128".to_owned());
    }
    if max_output_tokens.is_some_and(|value| value < 128) {
        return Err("max output tokens must be at least 128".to_owned());
    }
    if max_output_tokens.is_some_and(|value| value > crate::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS) {
        return Err(format!(
            "max output tokens must not exceed {}",
            crate::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS
        ));
    }
    if context_size.is_some_and(|value| value < 1024) {
        return Err("context size must be at least 1024".to_owned());
    }
    let image_budget = max_image_tokens.unwrap_or(1024);
    let output_budget = max_output_tokens.unwrap_or(16_384);
    let context_budget = context_size.unwrap_or(20_480);
    let required = image_budget
        .saturating_add(output_budget)
        .saturating_add(1024);
    if required > context_budget {
        return Err(format!(
            "context size {context_budget} is too small for image/output/prompt budget {required}"
        ));
    }
    Ok(())
}

/// Output directory resolution: explicit or `<stem>-ocr` next to the input.
///
/// # Errors
///
/// Usage-error strings identical to the legacy CLI (exit code 2).
pub fn resolve_output_dir(input: &str, explicit: Option<&str>) -> Result<PathBuf, String> {
    let input_path = Path::new(input);
    let output = match explicit {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        Some(_) => return Err("output directory cannot be empty".to_owned()),
        None => {
            let stem = input_path
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "input filename has no usable stem".to_owned())?;
            input_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(format!("{stem}-ocr"))
        }
    };
    if output.is_absolute() {
        Ok(output)
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(output))
            .map_err(|error| format!("cannot resolve output directory: {error}"))
    }
}

/// Output policy: the shared root validator (absolute/`..`/trailing-dot
/// spellings and protected roots) plus the working-directory rule.
///
/// # Errors
///
/// Usage-error strings identical to the legacy CLI (exit code 2).
pub fn validate_scan_output_policy(
    input: &Path,
    output: &Path,
    protected: &[&Path],
    working_directory: &Path,
) -> Result<(), String> {
    ocrx_output::validate_output_root(input, output, protected).map_err(|error| error.to_string())?;
    ocrx_output::validate_output_not_containing(output, working_directory)
        .map_err(|error| error.to_string())
}

/// Page selection parser (`1,3,5-9`, sorted and deduplicated).
///
/// # Errors
///
/// Usage-error strings identical to the legacy CLI (exit code 2).
pub fn parse_page_selection(expression: &str) -> Result<Vec<usize>, String> {
    let mut pages = Vec::new();
    for part in expression.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("page expression contains an empty item".to_owned());
        }
        if let Some((start, end)) = part.split_once('-') {
            let start = parse_page_number(start)?;
            let end = parse_page_number(end)?;
            if start > end {
                return Err(format!("page range is reversed: {start}-{end}"));
            }
            pages.extend(start..=end);
        } else {
            pages.push(parse_page_number(part)?);
        }
    }
    pages.sort_unstable();
    pages.dedup();
    Ok(pages)
}

fn parse_page_number(value: &str) -> Result<usize, String> {
    let page = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid page number: {value}"))?;
    (page > 0)
        .then_some(page)
        .ok_or_else(|| "page numbers start at 1".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_input_resolution_expands_sorts_and_dedups() {
        let root = std::env::temp_dir().join(format!("ocrx-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("dir/b.pdf"), b"").unwrap();
        std::fs::write(root.join("dir/a.pdf"), b"").unwrap();
        std::fs::write(root.join("dir/notes.txt"), b"").unwrap();
        let single = root.join("solo.pdf");
        std::fs::write(&single, b"").unwrap();
        // Directory expansion is sorted; the txt file is ignored.
        let resolved = resolve_scan_inputs(&[root.join("dir").to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(resolved.len(), 2);
        assert!(resolved[0].ends_with("a.pdf"));
        assert!(resolved[1].ends_with("b.pdf"));
        // Single file passthrough + canonical dedup.
        let resolved = resolve_scan_inputs(&[
            single.to_string_lossy().into_owned(),
            single.to_string_lossy().into_owned(),
        ])
        .unwrap();
        assert_eq!(resolved.len(), 1);
        // Missing input is a usage error.
        assert!(resolve_scan_inputs(&["missing.pdf".to_owned()]).is_err());
        // Empty directory is a usage error.
        std::fs::create_dir_all(root.join("empty")).unwrap();
        assert!(resolve_scan_inputs(&[root.join("empty").to_string_lossy().into_owned()]).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn numeric_validation_mirrors_the_frozen_ranges() {
        assert!(validate_numeric_options(None, None, None, None).is_ok());
        assert!(validate_numeric_options(Some(0), None, None, None).is_err());
        assert!(validate_numeric_options(None, Some(127), None, None).is_err());
        assert!(validate_numeric_options(None, None, Some(127), None).is_err());
        assert!(
            validate_numeric_options(None, None, Some(16_385), None).is_err(),
            "over-cap budget fails before the scan starts"
        );
        assert!(validate_numeric_options(None, None, None, Some(1023)).is_err());
        // Budget sum exceeds the context.
        assert!(validate_numeric_options(None, Some(1024), Some(16_384), Some(8_192)).is_err());
    }

    #[test]
    fn page_selection_parses_ranges_and_dedups() {
        assert_eq!(parse_page_selection("3,1,2-4,3").unwrap(), vec![1, 2, 3, 4]);
        assert!(parse_page_selection("2-1").is_err());
        assert!(parse_page_selection("0").is_err());
        assert!(parse_page_selection("x").is_err());
    }

    #[test]
    fn output_dir_defaults_to_the_stem_layout() {
        let resolved = resolve_output_dir("/tmp/doc.pdf", None).unwrap();
        assert!(!resolved.ends_with("doc-ocr.pdf"));
        assert!(resolved.to_string_lossy().ends_with("doc-ocr"));
        assert!(resolve_output_dir("/tmp/doc.pdf", Some(" ")).is_err());
    }

    #[test]
    fn ocr_lifecycle_question_follows_the_initial_plan() {
        use ocrx_core::{PageType, ProcessingMethod, RouteConfidence, PdfReasonCode};
        let classification = |method: ProcessingMethod, page_type: PageType| PageClassification {
            page_type,
            confidence: RouteConfidence::High,
            recommended_method: method,
            reason: PdfReasonCode::NativeTextReliable,
        };
        // Text mode with all-native pages: no OCR lifecycle.
        assert!(!planned_ocr_pages(
            "text",
            &[classification(ProcessingMethod::NativeText, PageType::Text)]
        ));
        // Auto mode with an OCR-recommended page: OCR lifecycle required.
        assert!(planned_ocr_pages(
            "auto",
            &[classification(ProcessingMethod::Ocr, PageType::Image)]
        ));
        // Empty pages never require OCR on their own.
        assert!(!planned_ocr_pages(
            "auto",
            &[classification(ProcessingMethod::Ocr, PageType::Empty)]
        ));
    }
}
