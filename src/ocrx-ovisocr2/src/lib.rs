//! OvisOCR2 adapter boundary. Prompt and MTMD implementation follows runtime freeze.

use ocrx_core::{
    CancellationToken, ModelId, NormalizedBbox, PageBlock, PageDocument, PageWarning, Termination,
};
use ocrx_runtime::{GenerationProgress, InferenceRequest, InferenceResponse, Runtime};
use std::path::Path;

pub const ADAPTER_ID: &str = "ovisocr2";
pub const MAX_OUTPUT_TOKENS: u32 = 16_384;

/// Runs one rendered page through runtime, adapter, bbox cropping and atomic PNG output.
///
/// # Errors
///
/// Returns an error when the RGB8 input, inference, adapter, or crop output is invalid.
pub fn write_page_artifacts(
    runtime: &mut impl Runtime,
    page: u32,
    width: u32,
    height: u32,
    rgb8: Vec<u8>,
    max_output_tokens: u32,
    output_root: impl AsRef<Path>,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(write_page_artifacts_with_status(
        runtime,
        page,
        width,
        height,
        rgb8,
        max_output_tokens,
        output_root,
    )?
    .markdown)
}

#[derive(Debug)]
pub struct PageArtifactOutput {
    pub markdown: String,
    pub page_json: String,
    pub truncated: bool,
    pub degenerated: bool,
    pub warnings: Vec<String>,
    /// Pre-crop normalized raw model output, preserved for the
    /// `page-NNNN.raw` sidecar required by requirement §7.4.
    pub raw_output: String,
}

pub fn write_page_artifacts_with_status(
    runtime: &mut impl Runtime,
    page: u32,
    width: u32,
    height: u32,
    rgb8: Vec<u8>,
    max_output_tokens: u32,
    output_root: impl AsRef<Path>,
) -> Result<PageArtifactOutput, Box<dyn std::error::Error>> {
    write_page_artifacts_with_status_cancelled(
        runtime,
        page,
        width,
        height,
        rgb8,
        max_output_tokens,
        output_root,
        &CancellationToken::new(),
        &mut |_| {},
    )
}

/// Cancellation-aware variant of [`write_page_artifacts_with_status`].
///
/// The runtime polls `cancellation` between generated tokens and forwards
/// `progress` (token count + elapsed seconds) to the caller so the CLI can
/// emit the per-second heartbeat contract.
#[allow(clippy::too_many_arguments)]
pub fn write_page_artifacts_with_status_cancelled(
    runtime: &mut impl Runtime,
    page: u32,
    width: u32,
    height: u32,
    rgb8: Vec<u8>,
    max_output_tokens: u32,
    output_root: impl AsRef<Path>,
    cancellation: &CancellationToken,
    progress: &mut dyn FnMut(GenerationProgress),
) -> Result<PageArtifactOutput, Box<dyn std::error::Error>> {
    // D5.1: an over-cap budget fails the page explicitly; the old silent
    // clamp hid that the requested budget was not in effect.
    let request = try_request(max_output_tokens).map_err(std::io::Error::other)?;
    let response = runtime.generate_with_progress(
        InferenceRequest::new(page, width, height, rgb8.clone(), request.max_output_tokens)?,
        cancellation,
        progress,
    )?;
    if cancellation.is_cancelled() {
        return Err(std::io::Error::other("cancelled").into());
    }
    let truncated = response.finish_reason == "max_tokens";
    // Single-source degeneration pipeline (plan §3.2.2): the tail crop runs
    // once here, before bbox stripping, and the cropped text drives every
    // downstream artifact (page .md, page.json, bbox extraction) so the
    // artifacts can never diverge on the loop body.
    let normalized_raw = normalize_line_endings(&response.raw_output);
    let crop = degenerate_tail_crop(&normalized_raw);
    let document = adapt_cropped_response(page, &crop.text, &response.finish_reason, &crop)
        .map_err(|error| format!("adapter: {error:?}"))?;
    let image = image::RgbImage::from_raw(width, height, rgb8).ok_or("invalid RGB8 image shape")?;
    let boxes = document
        .blocks
        .iter()
        .filter_map(|block| match block {
            PageBlock::VisualRegion { bbox, .. } => Some(*bbox),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (index, bbox) in boxes.iter().copied().enumerate() {
        ocrx_pdf::save_bbox_png(
            &image,
            bbox,
            &output_root,
            page,
            u32::try_from(index + 1).map_err(|_| "too many visual regions")?,
        )?;
    }
    let markdown = replace_bbox_images(&crop.text, page, boxes.len());
    validate_markdown_image_links(&markdown, &output_root)?;
    Ok(PageArtifactOutput {
        markdown,
        page_json: document
            .to_json()
            .map_err(|error| format!("serialize page: {error:?}"))?,
        truncated,
        degenerated: document.warnings.contains(&PageWarning::Degenerated),
        warnings: document
            .warnings
            .iter()
            .map(|warning| warning.as_str().to_owned())
            .collect(),
        raw_output: normalized_raw,
    })
}

fn validate_markdown_image_links(
    markdown: &str,
    output_root: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = output_root.as_ref();
    let mut rest = markdown;
    while let Some(start) = rest.find("images/") {
        let candidate = &rest[start..];
        let end = candidate
            .find(|character: char| {
                character.is_whitespace() || character == ')' || character == '"'
            })
            .unwrap_or(candidate.len());
        let relative = &candidate[..end];
        if !relative.starts_with("images/page-") || relative.contains("..") {
            return Err(format!("invalid markdown image link: {relative}").into());
        }
        if !root.join(relative).is_file() {
            return Err(format!("markdown image link is missing: {relative}").into());
        }
        rest = &candidate[end..];
    }
    Ok(())
}

fn replace_bbox_images(raw: &str, page: u32, count: usize) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    let mut index = 0;
    while let Some(start) = rest.find("<img") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let Some(end) = candidate.find('>') else {
            output.push_str(candidate);
            return output;
        };
        let tag = &candidate[..=end];
        if tag.contains("bbox_") && index < count {
            output.push_str(&format!(
                "<img src=\"images/page-{page:04}-figure-{figure:03}.png\" />",
                figure = index + 1
            ));
            index += 1;
        } else if tag.contains("bbox_") {
            // Invalid or excess model placeholders never become filesystem paths.
        } else {
            output.push_str(tag);
        }
        rest = &candidate[end + 1..];
    }
    output.push_str(rest);
    output
}

pub fn model_id() -> ModelId {
    ModelId::OvisOcr2
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvisRequest {
    pub temperature_milli: u32,
    pub max_output_tokens: u32,
    pub thinking: bool,
}

#[must_use]
pub fn request(max_output_tokens: u32) -> OvisRequest {
    OvisRequest {
        temperature_milli: 0,
        max_output_tokens,
        thinking: false,
    }
}

/// Builds a request only when the requested token budget is within the locked manifest limit.
pub fn try_request(max_output_tokens: u32) -> Result<OvisRequest, &'static str> {
    if max_output_tokens == 0 || max_output_tokens > MAX_OUTPUT_TOKENS {
        return Err("max-output-tokens exceeds the manifest safety limit");
    }
    Ok(request(max_output_tokens))
}

pub fn adapt_response(
    page_number: u32,
    response: &InferenceResponse,
) -> Result<PageDocument, ocrx_core::DomainError> {
    let normalized = normalize_line_endings(&response.raw_output);
    let crop = degenerate_tail_crop(&normalized);
    adapt_cropped_response(page_number, &crop.text, &response.finish_reason, &crop)
}

/// Adapter entry for already-cropped text: the entry crop metadata is merged
/// into the page warnings verbatim (plan §3.2.2); re-detecting on the cropped
/// text would be a no-op and could not recover the metadata.
///
/// # Errors
///
/// Returns the core validation error when page/bbox data is invalid.
pub fn adapt_cropped_response(
    page_number: u32,
    cropped: &str,
    finish_reason: &str,
    crop: &DegenerateTailCrop,
) -> Result<PageDocument, ocrx_core::DomainError> {
    build_page_document(
        page_number,
        cropped,
        finish_reason,
        crop.degenerated,
        crop.repetition_removed,
    )
}

/// Conservative adapter for the stable markdown-oriented Ovis output.
///
/// # Errors
///
/// Returns the core validation error when page/bbox data is invalid.
pub fn parse_page(
    page_number: u32,
    raw: &str,
    max_tokens: bool,
) -> Result<PageDocument, ocrx_core::DomainError> {
    parse_page_with_termination(
        page_number,
        raw,
        if max_tokens { "max_tokens" } else { "eos" },
    )
}

/// Normalizes CRLF and lone CR to LF so parsed documents are stable across
/// platforms regardless of the input's line-ending convention.
fn normalize_line_endings(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

fn parse_page_with_termination(
    page_number: u32,
    raw: &str,
    finish_reason: &str,
) -> Result<PageDocument, ocrx_core::DomainError> {
    let normalized = normalize_line_endings(raw);
    let crop = degenerate_tail_crop(&normalized);
    build_page_document(
        page_number,
        &crop.text,
        finish_reason,
        crop.degenerated,
        crop.repetition_removed,
    )
}

fn build_page_document(
    page_number: u32,
    normalized: &str,
    finish_reason: &str,
    crop_degenerated: bool,
    crop_repetition_removed: bool,
) -> Result<PageDocument, ocrx_core::DomainError> {
    let mut blocks = Vec::new();
    let markdown = remove_bbox_image_placeholders(normalized);
    blocks.extend(parse_structured_blocks(&markdown));
    let mut seen = Vec::new();
    for bbox in extract_bboxes(normalized) {
        if seen.contains(&bbox) {
            continue;
        }
        let order = seen.len() as u32;
        seen.push(bbox);
        blocks.push(PageBlock::VisualRegion {
            bbox,
            source_order: order,
        });
    }
    let termination = match finish_reason {
        "max_tokens" => Termination::MaxTokens,
        "stop" | "stop_token" => Termination::StopToken,
        "repetition_loop" => Termination::RepetitionLoop,
        "eos" => Termination::Eos,
        // Unknown termination reasons must never be amplified into success
        // semantics (plan §3.2.3, G6).
        _ => Termination::Invalid,
    };
    // Invariant (plan §3.2.3): a runtime repetition-loop early stop marks the
    // page degenerated even when the text-level detector finds no croppable
    // periodic tail.
    let degenerated = crop_degenerated || termination == Termination::RepetitionLoop;
    let mut warnings = Vec::new();
    if termination == Termination::MaxTokens {
        warnings.push(PageWarning::Truncated);
    }
    if crop_repetition_removed {
        warnings.push(PageWarning::RepetitionRemoved);
    }
    if degenerated {
        warnings.push(PageWarning::Degenerated);
    }
    let document = PageDocument {
        schema_version: PageDocument::SCHEMA_VERSION,
        page_number,
        blocks,
        termination,
        warnings,
    };
    document.validate()?;
    Ok(document)
}

fn parse_structured_blocks(markdown: &str) -> Vec<PageBlock> {
    let mut blocks = Vec::new();
    let mut remaining = markdown;
    while !remaining.is_empty() {
        let table_position = remaining.find("<table");
        let formula = find_formula_start(remaining);
        let next = match (table_position, formula) {
            (Some(table), Some((formula, _))) if formula < table => Some((formula, false)),
            (Some(table), _) => Some((table, true)),
            (None, Some((formula, _))) => Some((formula, false)),
            (None, None) => None,
        };
        if let Some((start, is_table)) = next {
            if start > 0 {
                blocks.push(PageBlock::Markdown {
                    content: remaining[..start].to_owned(),
                });
            }
            if !is_table {
                if let Some((open_len, close)) = formula_delimiter(&remaining[start..]) {
                    let formula_start = &remaining[start + open_len..];
                    if let Some(end) = formula_start.find(close) {
                        blocks.push(PageBlock::Formula {
                            latex: formula_start[..end].trim().to_owned(),
                            display: remaining[start..].starts_with("$$"),
                        });
                        remaining = &formula_start[end + close.len()..];
                        continue;
                    }
                }
                blocks.push(PageBlock::Markdown {
                    content: remaining.to_owned(),
                });
                break;
            }
            let table_start = &remaining[start..];
            if let Some(end) = table_start.find("</table>") {
                let end = end + "</table>".len();
                blocks.push(PageBlock::HtmlTable {
                    html: table_start[..end].to_owned(),
                });
                remaining = &table_start[end..];
                continue;
            }
            blocks.push(PageBlock::Markdown {
                content: remaining.to_owned(),
            });
            break;
        }
        if !remaining.trim().is_empty() {
            blocks.push(PageBlock::Markdown {
                content: remaining.to_owned(),
            });
        }
        break;
    }
    blocks
}

fn find_formula_start(value: &str) -> Option<(usize, bool)> {
    ["$$", "$", "\\("]
        .into_iter()
        .filter_map(|delimiter| {
            value
                .find(delimiter)
                .map(|index| (index, delimiter == "$$"))
        })
        .min_by_key(|(index, _)| *index)
}

fn formula_delimiter(value: &str) -> Option<(usize, &'static str)> {
    if value.starts_with("$$") {
        Some((2, "$$"))
    } else if value.starts_with("\\(") {
        Some((2, "\\)"))
    } else if value.starts_with('$') {
        Some((1, "$"))
    } else {
        None
    }
}

/// Frozen detector thresholds (plan §3.2.2). Line-level guard: every line
/// taking part in the repeated blocks must be at least this many bytes after
/// trimming. Inline period bounds are in characters of the trailing segment.
const LINE_MIN_BYTES: usize = 8;
const INLINE_MIN_PERIOD: usize = 8;
const INLINE_MAX_PERIOD: usize = 512;
const INLINE_MIN_REPEATS: usize = 3;

/// Result of the single-entry degeneration crop (plan §3.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DegenerateTailCrop {
    /// Text after cropping; identical to the input when nothing matched.
    pub text: String,
    /// True when any degenerate periodic tail was detected (whether or not
    /// bytes were actually removed).
    pub degenerated: bool,
    /// True iff the removed segment still had non-whitespace content after
    /// bbox placeholder stripping (`remove_bbox_image_placeholders(pre) !=
    /// remove_bbox_image_placeholders(post)`).
    pub repetition_removed: bool,
}

/// Detects and crops a periodic degenerate tail from normalized model output,
/// before any bbox stripping (plan §3.2.2 / D-3).
///
/// Two frozen shapes are covered: line-level periods (the trailing block of
/// `p >= 1` lines repeats at least twice; every involved line has at least
/// [`LINE_MIN_BYTES`] non-whitespace bytes) and inline periods (the trailing
/// segment consists of at least [`INLINE_MIN_REPEATS`] repetitions of a
/// period of [`INLINE_MIN_PERIOD`..=[INLINE_MAX_PERIOD]] characters, allowing
/// one incomplete period at the very end). Cropping keeps exactly one full
/// period block; an incomplete trailing period is cropped along with the loop.
///
/// The operation is idempotent: cropped output no longer satisfies the
/// repetition thresholds, so a second call neither deletes more text nor
/// raises the flags again.
#[must_use]
pub fn degenerate_tail_crop(normalized: &str) -> DegenerateTailCrop {
    let (text, degenerated) = match crop_line_level_period(normalized) {
        Some(cropped) => (cropped, true),
        None => match crop_inline_level_period(normalized) {
            Some(cropped) => (cropped, true),
            None => (normalized.to_owned(), false),
        },
    };
    let repetition_removed = degenerated
        && remove_bbox_image_placeholders(normalized) != remove_bbox_image_placeholders(&text);
    DegenerateTailCrop {
        text,
        degenerated,
        repetition_removed,
    }
}

/// Line-level crop: the smallest `p >= 1` whose trailing block of `p` lines is
/// byte-identical to the preceding `p` lines (block repeated `r >= 2` times)
/// wins; the tail is cropped to one full block. Returns `None` when the
/// confidence guard or equality fails at every `p`.
fn crop_line_level_period(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    let lines: Vec<&str> = trimmed.split('\n').collect();
    let n = lines.len();
    for p in 1..=(n / 2) {
        let block = &lines[n - p..];
        if block.iter().any(|line| line.trim().len() < LINE_MIN_BYTES) {
            continue;
        }
        // The block must already repeat twice; count the total number of
        // trailing repeats to keep exactly one.
        let mut repeats = 1_usize;
        while repeats * p * 2 <= n && lines[n - (repeats + 1) * p..n - repeats * p] == *block {
            repeats += 1;
        }
        if repeats < 2 {
            continue;
        }
        let keep_up_to = n - repeats * p;
        let mut kept_lines = lines[..keep_up_to].to_vec();
        kept_lines.extend_from_slice(block);
        let mut output = kept_lines.join("\n");
        output.push('\n');
        return Some(output);
    }
    None
}

/// Inline-level crop on the trailing segment (after the last newline): finds a
/// period `p` in [`INLINE_MIN_PERIOD`, [`INLINE_MAX_PERIOD`]] whose full
/// repetitions (at least [`INLINE_MIN_REPEATS`]) end at the segment tail,
/// tolerating one incomplete period at the very end; keeps exactly one period
/// and drops the incomplete tail.
///
/// All periodicity comparisons run on raw bytes — a multi-byte period never
/// gets sliced at an unverified byte offset, so no UTF-8 input can panic the
/// crop (review BLK-1). The trailing partial must be a byte prefix of the
/// period (a genuinely truncated next period); an unrelated tail after the
/// repeats means this is normal text and must not be cropped (review round 2).
fn crop_inline_level_period(text: &str) -> Option<String> {
    // Trailing newlines belong to no content line; the detector inspects the
    // trimmed tail so a loop line followed by a final newline still matches.
    let trimmed = text.trim_end();
    let (head, segment) = match trimmed.rsplit_once('\n') {
        Some((head, segment)) => (Some(head), segment),
        None => (None, trimmed),
    };
    let bytes = segment.as_bytes();
    let len = bytes.len();
    if len < INLINE_MIN_PERIOD * INLINE_MIN_REPEATS {
        return None;
    }
    // All periodicity comparisons run on raw bytes — a multi-byte period is
    // never sliced at an unverified byte offset, so no UTF-8 input can panic
    // the crop (review BLK-1).
    //
    // Candidates are anchored at the tail: a run of complete periods ends at
    // `end = len - partial`, preceded by arbitrary normal line content, and
    // optionally followed by a truncated next period (`partial` bytes that
    // must be a byte prefix of the period — an unrelated tail means this is
    // normal text and must not be cropped, review round 2).
    //
    // A rotation-aligned candidate can describe the same bytes with a shifted
    // period, folding loop content into the kept prefix or an unrelated tail
    // into the period (`abcdefgh` x4 read as run `bcdefgha` x3 + tail
    // `bcdefgh`; `abcdefgh` x3 + `XYZ` read as period `defghabc`). Two guards
    // keep the honest reading: a candidate is rejected when its kept prefix
    // itself continues the period (left-maximality, the `k` loop below), and
    // among surviving candidates the one covering the most periods — then the
    // leftmost run start — wins.
    let mut best: Option<(usize, usize, usize, usize)> = None; // (repeats, start, end, p)
    for p in INLINE_MIN_PERIOD..=INLINE_MAX_PERIOD {
        if p * INLINE_MIN_REPEATS > len {
            break;
        }
        let max_partial = (p - 1).min(len - p * INLINE_MIN_REPEATS);
        for partial in 0..=max_partial {
            let end = len - partial;
            let period = &bytes[end - p..end];
            if bytes[end - 2 * p..end - p] != *period || bytes[end - 3 * p..end - 2 * p] != *period
            {
                continue;
            }
            if partial > 0 && !bytes[end..].starts_with(&period[..partial]) {
                continue;
            }
            // Count the full trailing repeats and locate the run start.
            let mut repeats = INLINE_MIN_REPEATS;
            while end >= (repeats + 1) * p
                && bytes[end - (repeats + 1) * p..end - repeats * p] == *period
            {
                repeats += 1;
            }
            let start = end - repeats * p;
            // Left-maximality: the kept prefix must not itself continue the
            // period, otherwise the run really starts further left and this
            // shifted reading would keep loop content as "pre-loop text".
            let mut continues = false;
            for k in 1..=start.min(p) {
                if bytes[start - k..start] == period[p - k..p] {
                    continues = true;
                    break;
                }
            }
            if continues {
                continue;
            }
            // The output slices are only taken at verified char boundaries;
            // when a boundary falls inside a multi-byte character this
            // candidate cannot be cropped exactly, so try the next one
            // instead of panicking.
            if !segment.is_char_boundary(start) || !segment.is_char_boundary(end - p) {
                continue;
            }
            let better = match best {
                None => true,
                Some((best_repeats, best_start, _, _)) => {
                    repeats > best_repeats || (repeats == best_repeats && start < best_start)
                }
            };
            if better {
                best = Some((repeats, start, end, p));
            }
        }
    }
    let (_, start, end, p) = best?;
    Some(join_head_and_period(
        head,
        &segment[..start],
        &segment[end - p..end],
    ))
}

/// Assembles the cropped text: pre-loop content, then exactly one full period.
fn join_head_and_period(head: Option<&str>, pre: &str, period: &str) -> String {
    let mut output = String::with_capacity(pre.len() + period.len() + 2);
    if let Some(head) = head {
        output.push_str(head);
        output.push('\n');
    }
    output.push_str(pre);
    output.push_str(period);
    output
}

fn remove_bbox_image_placeholders(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<img") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let Some(end) = candidate.find('>') else {
            output.push_str(candidate);
            return output;
        };
        let tag = &candidate[..=end];
        if tag.contains("bbox_") {
            rest = &candidate[end + 1..];
        } else {
            output.push_str(tag);
            rest = &candidate[end + 1..];
        }
    }
    output.push_str(rest);
    output
}

fn extract_bboxes(raw: &str) -> Vec<NormalizedBbox> {
    let mut result = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find("bbox_") {
        let candidate = &rest[start + 5..];
        let mut values = candidate.split(['_', '.', '"', '\'']);
        let parsed = (0..4)
            .map(|_| values.next()?.parse::<u16>().ok())
            .collect::<Option<Vec<_>>>();
        if let Some(values) = parsed {
            if let Ok(bbox) = NormalizedBbox::new(values[0], values[1], values[2], values[3]) {
                result.push(bbox);
            }
        }
        rest = &candidate[candidate.len().min(1)..];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_runtime::{Backend, RuntimeInfo};

    #[test]
    fn request_disables_thinking_and_uses_zero_temperature() {
        assert_eq!(
            request(512),
            OvisRequest {
                temperature_milli: 0,
                max_output_tokens: 512,
                thinking: false
            }
        );
    }

    #[test]
    fn request_rejects_tokens_above_manifest_limit() {
        assert!(try_request(MAX_OUTPUT_TOKENS).is_ok());
        assert!(try_request(MAX_OUTPUT_TOKENS + 1).is_err());
    }

    #[test]
    fn over_cap_budget_fails_production_path_explicitly() {
        let error = write_page_artifacts_with_status_cancelled(
            &mut FixtureRuntime,
            1,
            10,
            10,
            vec![255; 10 * 10 * 3],
            MAX_OUTPUT_TOKENS + 1,
            std::env::temp_dir(),
            &CancellationToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exceeds the manifest safety limit")
        );
    }

    #[test]
    fn parser_removes_only_high_confidence_tail_repetition() {
        let page = parse_page(1, "intro\nrepeated tail\nrepeated tail", false).unwrap();
        assert!(page.warnings.contains(&PageWarning::RepetitionRemoved));
        let uncertain = parse_page(1, "intro\nshort\nshort", false).unwrap();
        assert!(!uncertain.warnings.contains(&PageWarning::RepetitionRemoved));
    }

    #[test]
    fn parser_preserves_markdown_and_marks_truncation() {
        let page = parse_page(1, "# heading", true).unwrap();
        assert_eq!(page.termination, Termination::MaxTokens);
        assert_eq!(page.warnings, vec![PageWarning::Truncated]);
        assert!(matches!(page.blocks[0], PageBlock::Markdown { .. }));
    }

    #[test]
    fn parser_maps_formula_and_html_table_to_core_blocks() {
        let page = parse_page(
            1,
            "intro\n$$x^2 + 1$$\n<table><tr><td>A</td></tr></table>",
            false,
        )
        .unwrap();
        assert!(page.blocks.iter().any(|block| matches!(
            block,
            PageBlock::Formula { latex, display: true } if latex == "x^2 + 1"
        )));
        assert!(page.blocks.iter().any(|block| matches!(
            block,
            PageBlock::HtmlTable { html } if html.contains("<td>A</td>")
        )));
    }

    #[test]
    fn parser_maps_inline_formula_delimiters_to_formula_blocks() {
        let page = parse_page(1, "before $\\angle A=30^\\circ$ after \\(x^2\\)", false).unwrap();
        let formulas = page
            .blocks
            .iter()
            .filter_map(|block| match block {
                PageBlock::Formula { latex, display } => Some((latex.as_str(), *display)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            formulas,
            vec![(r"\angle A=30^\circ", false), ("x^2", false)]
        );
    }

    #[test]
    fn adapter_preserves_unclosed_formula_and_maps_stop_token() {
        let page = adapt_response(
            1,
            &InferenceResponse {
                raw_output: "before $$unclosed".to_owned(),
                finish_reason: "stop".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(page.termination, Termination::StopToken);
        assert!(matches!(page.blocks[0], PageBlock::Markdown { .. }));
    }

    #[test]
    fn parser_maps_bbox_image_tags_to_visual_regions() {
        let page =
            parse_page(1, "text <img src=\"images/bbox_10_20_300_400.jpg\">", false).unwrap();
        assert!(page.blocks.iter().any(|block| matches!(block, PageBlock::VisualRegion { bbox, .. } if bbox.left == 10 && bbox.bottom == 400)));
    }

    #[test]
    fn parser_removes_valid_bbox_image_placeholders_from_markdown() {
        let page = parse_page(
            1,
            "before <img src=\"images/bbox_10_20_300_400.jpg\" /> after",
            false,
        )
        .unwrap();
        let markdown = page
            .blocks
            .iter()
            .find_map(|block| match block {
                PageBlock::Markdown { content } => Some(content.as_str()),
                _ => None,
            })
            .unwrap();
        assert_eq!(markdown, "before  after");
        assert_eq!(
            page.blocks
                .iter()
                .filter(|block| matches!(block, PageBlock::VisualRegion { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn parser_deduplicates_repeated_bbox_regions_in_source_order() {
        let page = parse_page(
            1,
            "<img src=\"images/bbox_10_20_300_400.jpg\"><img src=\"images/bbox_10_20_300_400.jpg\"><img src=\"images/bbox_500_600_700_800.jpg\">",
            false,
        )
        .unwrap();
        let regions = page
            .blocks
            .iter()
            .filter_map(|block| match block {
                PageBlock::VisualRegion {
                    bbox, source_order, ..
                } => Some((*bbox, *source_order)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].1, 0);
        assert_eq!(regions[1].1, 1);
    }

    #[test]
    fn adapter_consumes_runtime_response_without_exposing_runtime_types_downstream() {
        let response = InferenceResponse {
            raw_output: "recognized".to_owned(),
            finish_reason: "eos".to_owned(),
        };
        let page = adapt_response(3, &response).unwrap();
        assert_eq!(page.page_number, 3);
        assert_eq!(page.termination, Termination::Eos);
    }

    struct FixtureRuntime;
    impl Runtime for FixtureRuntime {
        fn info(&self) -> RuntimeInfo {
            RuntimeInfo {
                backend: Backend::Cpu,
                image_token_limit: 1024,
            }
        }

        fn generate(&mut self, _request: InferenceRequest) -> Result<InferenceResponse, String> {
            Ok(InferenceResponse {
                raw_output: "before <img src=\"images/bbox_100_100_900_900.jpg\" /> after"
                    .to_owned(),
                finish_reason: "eos".to_owned(),
            })
        }
    }

    #[test]
    fn reference_raw_fixture_maps_to_stable_page_document_golden() {
        let raw = include_str!("../tests/fixtures/reference/page-01.raw.md");
        let expected = include_str!("../tests/fixtures/reference/page-01.page.json").trim();
        let page = parse_page(1, raw, false).unwrap();
        assert_eq!(page.to_json().unwrap(), expected);
        assert_eq!(PageDocument::from_json(expected).unwrap(), page);
        assert!(page.blocks.iter().any(|block| matches!(
            block,
            PageBlock::Formula { latex, .. } if latex == "E = mc^2"
        )));
        assert!(page.blocks.iter().any(|block| matches!(
            block,
            PageBlock::VisualRegion { bbox, source_order: 0 }
                if bbox.left == 100 && bbox.bottom == 400
        )));
    }

    #[test]
    fn parser_handles_empty_output_and_rejects_invalid_bbox_shapes() {
        let empty = parse_page(1, "", false).unwrap();
        assert!(empty.blocks.is_empty());
        assert_eq!(empty.termination, Termination::Eos);

        let page = parse_page(
            1,
            "ok <img src=\"images/bbox_10_10_10_20.jpg\"> <img src=\"images/bbox_0_0_1000_10.jpg\"> <img src=\"images/bbox_1_2_3_4.jpg\">",
            false,
        )
        .unwrap();
        assert_eq!(
            page.blocks
                .iter()
                .filter(|block| matches!(block, PageBlock::VisualRegion { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn adapter_preserves_unclosed_table_as_markdown() {
        let page = parse_page(1, "start <table><tr><td>open", false).unwrap();
        assert!(
            !page
                .blocks
                .iter()
                .any(|block| matches!(block, PageBlock::HtmlTable { .. }))
        );
        assert!(page.blocks.iter().any(|block| matches!(
            block,
            PageBlock::Markdown { content } if content.contains("<table>")
        )));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    #[ignore = "set OCRX_E2E_MODEL=1 with installed community compact assets"]
    fn gated_real_model_writes_at_least_one_figure_png() {
        assert_eq!(
            std::env::var("OCRX_E2E_MODEL").ok().as_deref(),
            Some("1"),
            "enable with OCRX_E2E_MODEL=1"
        );
        let cache = std::env::var_os("OCRX_MODEL_CACHE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let manifest =
            ocrx_models::community_manifest("compact").expect("compact manifest is pinned");
        let (model, mmproj) =
            ocrx_models::cache_paths(&cache, &manifest.identity.revision, &manifest.profile)
                .expect("compact cache paths");
        assert!(
            model.is_file() && mmproj.is_file(),
            "compact profile assets must be installed under {cache:?}"
        );
        let use_gpu = cfg!(target_os = "macos");
        let mut runtime =
            ocrx_runtime::LlamaMtmdRuntime::load(&model, &mmproj, use_gpu, 1024, 16_384)
                .expect("real runtime load");
        let root = std::env::temp_dir().join(format!("ocrx-e2e-figure-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Minimal RGB page; real model may or may not emit a bbox — assert pipeline completes.
        let width = 64_u32;
        let height = 64_u32;
        let rgb8 = vec![240_u8; (width * height * 3) as usize];
        let markdown = write_page_artifacts(&mut runtime, 1, width, height, rgb8, 64, &root)
            .expect("real page artifacts");
        assert!(!markdown.is_empty() || root.join("images").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn page_artifacts_write_stable_bbox_png_and_replace_markdown_link() {
        let root = std::env::temp_dir().join(format!("ocrx-ovis-artifacts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let markdown = write_page_artifacts(
            &mut FixtureRuntime,
            2,
            10,
            10,
            vec![255; 10 * 10 * 3],
            16,
            &root,
        )
        .unwrap();
        assert!(markdown.contains("images/page-0002-figure-001.png"));
        assert!(root.join("images/page-0002-figure-001.png").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_bbox_placeholder_cannot_escape_images_directory() {
        assert_eq!(
            replace_bbox_images("x <img src=\"../../secret\"> y", 1, 0),
            "x <img src=\"../../secret\"> y"
        );
        assert_eq!(
            replace_bbox_images("x <img src=\"images/bbox_3_3_3_3.jpg\"> y", 1, 0),
            "x  y"
        );
    }

    #[test]
    fn markdown_image_links_require_existing_output_artifacts() {
        let root = std::env::temp_dir().join(format!("ocrx-ovis-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("images")).unwrap();
        assert!(
            validate_markdown_image_links("![figure](images/page-0001-figure-001.png)", &root)
                .is_err()
        );
        std::fs::write(root.join("images/page-0001-figure-001.png"), b"png").unwrap();
        assert!(
            validate_markdown_image_links("![figure](images/page-0001-figure-001.png)", &root)
                .is_ok()
        );
        assert!(validate_markdown_image_links("![x](images/../secret)", &root).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    // ---- D1: degenerate tail crop (plan §3.2.2, §5.1) ----

    #[test]
    fn crop_line_level_period_keeps_one_block_and_respects_byte_guard() {
        // p=1, r=2 regression of the previous two-line behavior.
        let crop = degenerate_tail_crop("intro\nrepeated tail\nrepeated tail");
        assert_eq!(crop.text, "intro\nrepeated tail\n");
        assert!(crop.degenerated && crop.repetition_removed);
        // Multi-line period p=2 with r=2.
        let crop = degenerate_tail_crop("head\nA line one\nB line two\nA line one\nB line two");
        assert_eq!(crop.text, "head\nA line one\nB line two\n");
        assert!(crop.degenerated);
        // 8-byte guard boundary: 8 bytes triggers, 7 does not.
        let eight = degenerate_tail_crop("x\n12345678\n12345678");
        assert!(eight.degenerated);
        let seven = degenerate_tail_crop("x\n1234567\n1234567");
        assert!(!seven.degenerated);
        assert_eq!(seven.text, "x\n1234567\n1234567");
    }

    #[test]
    fn crop_inline_level_period_handles_incident_shape_and_partial_tail() {
        let period = "依次按键 $\\tan^{1}F$ 2 2 1 8";
        // Exact six repeats: crop keeps one period.
        let exact = degenerate_tail_crop(&format!("real content\n{period}"));
        assert!(!exact.degenerated);
        let repeated = degenerate_tail_crop(&format!("real content\n{period}{period}{period}"));
        assert!(repeated.degenerated && repeated.repetition_removed);
        assert_eq!(repeated.text, format!("real content\n{period}"));
        // Trailing incomplete period (a true prefix of the period) is cropped
        // together with the loop; exactly one period is kept.
        let partial: String = period.chars().take(6).collect();
        let with_partial =
            degenerate_tail_crop(&format!("real content\n{period}{period}{period}{partial}"));
        assert!(with_partial.degenerated);
        assert_eq!(with_partial.text, format!("real content\n{period}"));
        // bbox tag loop: degenerated, but the stripped text did not change.
        let bbox_tag = "<img src=\"images/bbox_10_20_300_400.jpg\">";
        let bbox_loop = degenerate_tail_crop(&format!("text\n{bbox_tag}{bbox_tag}{bbox_tag}"));
        assert!(bbox_loop.degenerated);
        assert!(!bbox_loop.repetition_removed);
        assert_eq!(bbox_loop.text, format!("text\n{bbox_tag}"));
    }

    #[test]
    fn non_ascii_period_with_different_partial_tail_does_not_panic() {
        // BLK-1 regression: any multi-byte input must return a verdict, never
        // panic — including unverified byte-offset slices (review round 1/2).
        // The 'a' tail is not a prefix of the period: normal text, uncropped.
        let crop = degenerate_tail_crop("循环片段循环片段循环片段a");
        assert!(!crop.degenerated);
        assert_eq!(crop.text, "循环片段循环片段循环片段a");
        // A Chinese truncated period prefix is detected and cropped.
        let crop = degenerate_tail_crop("循环片段循环片段循环片段循环");
        assert!(crop.degenerated);
        assert_eq!(crop.text, "循环片段");
        // Re-cropping is idempotent.
        let second = degenerate_tail_crop(&crop.text);
        assert_eq!(second.text, crop.text);
        assert!(!second.degenerated);
        // Production path with the same shape must not panic either.
        let page = adapt_response(
            1,
            &InferenceResponse {
                raw_output: "正文
循环片段循环片段循环片段a"
                    .to_owned(),
                finish_reason: "max_tokens".to_owned(),
            },
        )
        .unwrap();
        assert!(!page.warnings.contains(&PageWarning::Degenerated));
    }

    #[test]
    fn unrelated_tail_after_repeats_is_never_cropped() {
        // Review round 2 false-positive: three ASCII repeats followed by an
        // unrelated tail must stay byte-identical.
        let crop = degenerate_tail_crop("abcdefghabcdefghabcdefghXYZ");
        assert!(!crop.degenerated);
        assert_eq!(crop.text, "abcdefghabcdefghabcdefghXYZ");
        // The same body ending on a true period boundary is a real loop.
        let crop = degenerate_tail_crop("abcdefghabcdefghabcdefghabcdefgh");
        assert!(crop.degenerated);
        assert_eq!(crop.text, "abcdefgh");
    }

    #[test]
    fn loop_after_same_line_prefix_is_detected() {
        // Review round 3: the run is anchored at the tail, so a loop that
        // starts mid-line after normal content must still be detected.
        // Boundary case: run ends exactly at end-of-segment.
        let crop = degenerate_tail_crop("正文abcdefghabcdefghabcdefgh");
        assert!(crop.degenerated);
        assert_eq!(crop.text, "正文abcdefgh");
        // Truncated next period (true prefix) is cropped with the loop.
        let crop = degenerate_tail_crop("正文abcdefghabcdefghabcdefghabc");
        assert!(crop.degenerated);
        assert_eq!(crop.text, "正文abcdefgh");
        // Same shapes with the incident Chinese period.
        let period = "依次按键 $\\tan^{1}F$ 2 2 1 8";
        let crop = degenerate_tail_crop(&format!("正文{period}{period}{period}"));
        assert!(crop.degenerated);
        assert_eq!(crop.text, format!("正文{period}"));
        // Re-cropping is idempotent.
        let second = degenerate_tail_crop(&crop.text);
        assert_eq!(second.text, crop.text);
        assert!(!second.degenerated);
    }

    #[test]
    fn crop_ignores_short_legitimate_repetitions() {
        // Adjacent identical table cells, "(1)…(2)…" enumeration, ellipsis —
        // none of these are periodic tails under the frozen thresholds.
        for sample in [
            "| 3 | 3 | 3 |\n| 4 | 4 | 4 |",
            "(1) first\n(2) second\n(3) third",
            "……\n……",
            "single line only",
            "",
        ] {
            let crop = degenerate_tail_crop(sample);
            assert!(!crop.degenerated, "false positive on {sample:?}");
            assert_eq!(crop.text, sample);
        }
    }

    #[test]
    fn crop_is_idempotent_in_text_and_flags() {
        let period = "依次按键 $\\tan^{1}F$ 2 2 1 8";
        let input = format!("real content\n{period}{period}{period}");
        let first = degenerate_tail_crop(&input);
        let second = degenerate_tail_crop(&first.text);
        assert_eq!(second.text, first.text);
        assert!(!second.degenerated);
        assert!(!second.repetition_removed);
    }

    #[test]
    fn incident_fixture_crops_loop_and_keeps_sources_consistent() {
        let raw = include_str!("../tests/fixtures/degeneration/inline-loop.raw.md");
        let crop = degenerate_tail_crop(raw);
        assert!(crop.degenerated && crop.repetition_removed);
        let period = "依次按键 $\\tan^{1}F$ 2 2 1 8";
        let expected_head = "### 例 3\n(2) 依次按键\n";
        assert!(
            crop.text.starts_with(expected_head),
            "kept text must start with the pre-loop content"
        );
        assert!(
            !crop.text.contains(&period.repeat(2)),
            "cropped text must not retain the loop body"
        );
        assert!(crop.text.len() < raw.len());

        let page = parse_page(104, raw, true).unwrap();
        assert_eq!(page.termination, Termination::MaxTokens);
        assert!(page.warnings.contains(&PageWarning::Degenerated));
        assert!(page.warnings.contains(&PageWarning::RepetitionRemoved));
        // Single-source consistency: the .md is exactly replace_bbox_images of
        // the cropped text (no bbox tags in this fixture), and every markdown
        // block parsed into page.json is carried by that same .md — neither
        // artifact retains the loop body.
        let markdown = replace_bbox_images(&crop.text, 104, 0);
        assert_eq!(markdown, crop.text);
        for block in &page.blocks {
            if let PageBlock::Markdown { content } = block {
                assert!(markdown.contains(content.as_str()));
                assert!(!content.contains(&period.repeat(2)));
            }
        }
    }

    #[test]
    fn bbox_loop_fixture_flags_degenerated_without_repetition_removed() {
        let raw = include_str!("../tests/fixtures/degeneration/bbox-loop.raw.md");
        let page = parse_page(1, raw, false).unwrap();
        assert!(page.warnings.contains(&PageWarning::Degenerated));
        assert!(!page.warnings.contains(&PageWarning::RepetitionRemoved));
    }

    #[test]
    fn termination_mapping_accepts_repetition_loop_and_rejects_unknown() {
        let response = |reason: &str| InferenceResponse {
            raw_output: "text".to_owned(),
            finish_reason: reason.to_owned(),
        };
        assert_eq!(
            adapt_response(1, &response("repetition_loop"))
                .unwrap()
                .termination,
            Termination::RepetitionLoop
        );
        assert_eq!(
            adapt_response(1, &response("mystery")).unwrap().termination,
            Termination::Invalid
        );
        assert_eq!(
            adapt_response(1, &response("eos")).unwrap().termination,
            Termination::Eos
        );
        assert_eq!(
            adapt_response(1, &response("stop_token"))
                .unwrap()
                .termination,
            Termination::StopToken
        );
    }

    #[test]
    fn repetition_loop_implies_degenerated_even_without_text_hit() {
        // A p=1 token loop of "。" may not present a croppable text tail, yet
        // the invariant (plan §3.2.3) must still mark the page degenerated.
        let page = adapt_response(
            1,
            &InferenceResponse {
                raw_output: "正文\n……。".to_owned(),
                finish_reason: "repetition_loop".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(page.termination, Termination::RepetitionLoop);
        assert!(page.warnings.contains(&PageWarning::Degenerated));
        assert!(!page.warnings.contains(&PageWarning::Truncated));
    }

    struct LoopRuntime;
    impl Runtime for LoopRuntime {
        fn info(&self) -> RuntimeInfo {
            RuntimeInfo {
                backend: Backend::Cpu,
                image_token_limit: 1024,
            }
        }

        fn generate(&mut self, _request: InferenceRequest) -> Result<InferenceResponse, String> {
            let period = "依次按键 $\\tan^{1}F$ 2 2 1 8";
            Ok(InferenceResponse {
                raw_output: format!("real content\n{period}{period}{period}"),
                finish_reason: "max_tokens".to_owned(),
            })
        }
    }

    #[test]
    fn production_path_merges_entry_crop_metadata_into_page_json() {
        let root = std::env::temp_dir().join(format!("ocrx-ovis-crop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let output = write_page_artifacts_with_status_cancelled(
            &mut LoopRuntime,
            104,
            10,
            10,
            vec![255; 10 * 10 * 3],
            64,
            &root,
            &CancellationToken::new(),
            &mut |_| {},
        )
        .unwrap();
        // The page .md is cropped (single source) — no loop body remains.
        let period = "依次按键 $\\tan^{1}F$ 2 2 1 8";
        assert!(!output.markdown.contains(&period.repeat(2)));
        assert!(output.degenerated);
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning == "degenerated")
        );
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning == "repetition_removed")
        );
        assert!(output.truncated);
        // page.json carries the entry crop metadata; artifacts are persisted
        // by the output layer, so the in-process json is the contract here.
        let document = PageDocument::from_json(&output.page_json).unwrap();
        assert!(document.warnings.contains(&PageWarning::Degenerated));
        assert!(document.warnings.contains(&PageWarning::RepetitionRemoved));
        assert_eq!(document.termination, Termination::MaxTokens);
        // The raw sidecar payload (pre-crop normalized output) is available.
        assert_eq!(
            output.raw_output,
            format!("real content\n{period}{period}{period}")
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
