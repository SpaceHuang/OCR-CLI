//! OvisOCR2 adapter boundary. Prompt and MTMD implementation follows runtime freeze.

use ocrx_core::{ModelId, NormalizedBbox, PageBlock, PageDocument, PageWarning, Termination};
use ocrx_runtime::InferenceResponse;
use ocrx_runtime::{InferenceRequest, Runtime};
use std::path::Path;

pub const ADAPTER_ID: &str = "ovisocr2";
pub const MAX_OUTPUT_TOKENS: u32 = 4096;

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

pub struct PageArtifactOutput {
    pub markdown: String,
    pub page_json: String,
    pub truncated: bool,
    pub warnings: Vec<String>,
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
    let request = request(max_output_tokens);
    let response = runtime.generate(InferenceRequest::new(
        page,
        width,
        height,
        rgb8.clone(),
        request.max_output_tokens,
    )?)?;
    let truncated = response.finish_reason == "max_tokens";
    let document =
        adapt_response(page, &response).map_err(|error| format!("adapter: {error:?}"))?;
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
    let markdown = replace_bbox_images(&response.raw_output, page, boxes.len());
    validate_markdown_image_links(&markdown, &output_root)?;
    Ok(PageArtifactOutput {
        markdown,
        page_json: document
            .to_json()
            .map_err(|error| format!("serialize page: {error:?}"))?,
        truncated,
        warnings: document
            .warnings
            .iter()
            .map(|warning| warning.as_str().to_owned())
            .collect(),
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
        max_output_tokens: max_output_tokens.min(MAX_OUTPUT_TOKENS),
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
    parse_page_with_termination(page_number, &response.raw_output, &response.finish_reason)
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

fn parse_page_with_termination(
    page_number: u32,
    raw: &str,
    finish_reason: &str,
) -> Result<PageDocument, ocrx_core::DomainError> {
    let mut blocks = Vec::new();
    let markdown = remove_bbox_image_placeholders(raw);
    let (markdown, repetition_removed) = remove_confident_tail_repetition(&markdown);
    blocks.extend(parse_structured_blocks(&markdown));
    let mut seen = Vec::new();
    for bbox in extract_bboxes(raw) {
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
        _ => Termination::Eos,
    };
    let mut warnings = Vec::new();
    if termination == Termination::MaxTokens {
        warnings.push(PageWarning::Truncated);
    }
    if repetition_removed {
        warnings.push(PageWarning::RepetitionRemoved);
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

fn remove_confident_tail_repetition(markdown: &str) -> (String, bool) {
    let trimmed = markdown.trim_end();
    let Some((body, last)) = trimmed.rsplit_once('\n') else {
        return (markdown.to_owned(), false);
    };
    let last = last.trim();
    if last.len() < 8 {
        return (markdown.to_owned(), false);
    }
    let Some((prefix, previous)) = body.rsplit_once('\n') else {
        return (markdown.to_owned(), false);
    };
    if previous.trim() != last {
        return (markdown.to_owned(), false);
    }
    (format!("{}\n{}\n", prefix.trim_end(), last), true)
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
        assert_eq!(
            request(MAX_OUTPUT_TOKENS + 1).max_output_tokens,
            MAX_OUTPUT_TOKENS
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

    #[cfg(target_os = "macos")]
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
            .unwrap_or_else(|| std::path::PathBuf::from(".ocrx-model-cache"));
        let model = cache.join("ovisocr2/compact/model.gguf");
        let mmproj = cache.join("ovisocr2/compact/mmproj.gguf");
        assert!(
            model.is_file() && mmproj.is_file(),
            "compact profile assets must be installed under {cache:?}"
        );
        let mut runtime = ocrx_runtime::LlamaMtmdRuntime::load(&model, &mmproj, true, 1024, 16_384)
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
}
