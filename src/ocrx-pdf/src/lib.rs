//! PDFium-backed PDF rendering boundary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use image::RgbImage;
use ocrx_core::{
    NormalizedBbox, PageBlock, PageClassification, PageDocument, PageType, PdfPageDocument,
    PdfReasonCode, ProcessingMethod, QualityTier, RouteConfidence, Termination,
};
use pdfium_render::prelude::PdfPageObjectCommon;
use pdfium_render::prelude::*;
use std::fs;
use std::io::Cursor;

fn render_config(width: i32, height: i32) -> PdfRenderConfig {
    let mut config = PdfRenderConfig::new().set_fixed_size(width, height);
    if std::env::var_os("OCRX_PDFIUM_LIMIT_IMAGE_CACHE").is_some() {
        config = config.limit_render_image_cache_size(true);
    }
    if std::env::var_os("OCRX_PDFIUM_CLEAR_BEFORE_RENDER").is_some() {
        config = config.clear_before_rendering(true);
    }
    config
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub dpi: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfErrorKind {
    NonPdf,
    Corrupt,
    Encrypted,
    DynamicLibrary,
    Unknown,
}

/// Object facts collected without rendering a page or initializing OCR.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSignals {
    pub page_width: f32,
    pub page_height: f32,
    pub image_objects: usize,
    pub text_bbox_coverage: f32,
    pub visible_characters: usize,
    pub unicode_validity: f32,
    pub alphanumeric_ratio: f32,
    pub text_coverage: f32,
    pub image_coverage: f32,
    pub duplicate_text: bool,
    pub geometry_anomaly: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentType {
    Text,
    Image,
    Mixed,
    Empty,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityManifestEntry {
    pub source_sha256: String,
    pub page: u32,
    pub expected_type: PageType,
    pub native_allowed: bool,
    pub benchmark_version: String,
    pub truth_version: String,
    pub expected_text: String,
    pub expected_block_count: usize,
    pub expected_bbox_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityPageOutcome {
    pub is_blank: bool,
    pub has_duplicate_text: bool,
    pub failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QualityOutcomeSummary {
    pub blank_pages: usize,
    pub duplicate_pages: usize,
    pub failed_pages: usize,
}

pub fn summarize_quality_outcomes(outcomes: &[QualityPageOutcome]) -> QualityOutcomeSummary {
    QualityOutcomeSummary {
        blank_pages: outcomes.iter().filter(|outcome| outcome.is_blank).count(),
        duplicate_pages: outcomes
            .iter()
            .filter(|outcome| outcome.has_duplicate_text)
            .count(),
        failed_pages: outcomes.iter().filter(|outcome| outcome.failed).count(),
    }
}

/// Validates the structural invariants of a release-quality truth manifest.
pub fn validate_quality_manifest(
    native_set: &[QualityManifestEntry],
    direct_extract_forbidden_set: &[QualityManifestEntry],
) -> Result<(), &'static str> {
    let mut seen = HashSet::new();
    for entry in native_set.iter().chain(direct_extract_forbidden_set) {
        if entry.source_sha256.is_empty()
            || entry.page == 0
            || entry.benchmark_version.is_empty()
            || entry.truth_version.is_empty()
            || (entry.expected_type == PageType::Text && entry.expected_text.is_empty())
            || entry.expected_bbox_count > entry.expected_block_count
        {
            return Err("manifest entry is incomplete");
        }
        if !seen.insert((&entry.source_sha256, entry.page)) {
            return Err("manifest page appears more than once");
        }
    }
    if native_set.iter().any(|entry| !entry.native_allowed) {
        return Err("native set contains a page that is not native-eligible");
    }
    if direct_extract_forbidden_set
        .iter()
        .any(|entry| entry.native_allowed)
    {
        return Err("forbidden set contains a native-eligible page");
    }
    Ok(())
}

pub fn validate_release_manifest(
    native_set: &[QualityManifestEntry],
    direct_extract_forbidden_set: &[QualityManifestEntry],
) -> Result<(), &'static str> {
    validate_quality_manifest(native_set, direct_extract_forbidden_set)?;
    if native_set.len() < 30 {
        return Err("native quality set must contain at least 30 pages");
    }
    if direct_extract_forbidden_set.len() < 20 {
        return Err("direct-extract forbidden set must contain at least 20 pages");
    }
    if native_set
        .iter()
        .any(|entry| !matches!(entry.expected_type, PageType::Text))
    {
        return Err("native quality set contains a non-text expected page");
    }
    Ok(())
}

/// Loads the deterministic TSV manifest used by release automation.
/// Columns: set, source_sha256, page, expected_type, native_allowed,
/// benchmark_version, truth_version, expected_text, block_count, bbox_count.
pub fn load_quality_manifest(
    path: impl AsRef<Path>,
) -> std::io::Result<(Vec<QualityManifestEntry>, Vec<QualityManifestEntry>)> {
    let content = fs::read_to_string(path)?;
    let mut native = Vec::new();
    let mut forbidden = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("set\t") {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 10 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("manifest line {} must have 10 columns", line_number + 1),
            ));
        }
        let parse = |value: &str, name: &str| {
            value.parse().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid {name} on manifest line {}", line_number + 1),
                )
            })
        };
        let entry = QualityManifestEntry {
            source_sha256: fields[1].to_owned(),
            page: parse(fields[2], "page")?,
            expected_type: match fields[3] {
                "text" => PageType::Text,
                "image" => PageType::Image,
                "mixed" => PageType::Mixed,
                "empty" => PageType::Empty,
                "uncertain" => PageType::Uncertain,
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid expected_type",
                    ));
                }
            },
            native_allowed: match fields[4] {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid native_allowed",
                    ));
                }
            },
            benchmark_version: fields[5].to_owned(),
            truth_version: fields[6].to_owned(),
            expected_text: fields[7].to_owned(),
            expected_block_count: fields[8].parse().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid block_count")
            })?,
            expected_bbox_count: fields[9].parse().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid bbox_count")
            })?,
        };
        if entry.source_sha256.len() != 64
            || !entry
                .source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid source_sha256 on manifest line {}", line_number + 1),
            ));
        }
        match fields[0] {
            "native" => native.push(entry),
            "forbidden" => forbidden.push(entry),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid manifest set",
                ));
            }
        }
    }
    validate_quality_manifest(&native, &forbidden).map_err(std::io::Error::other)?;
    Ok((native, forbidden))
}

pub fn load_quality_manifest_for_input(
    manifest_path: impl AsRef<Path>,
    input_path: impl AsRef<Path>,
) -> std::io::Result<(Vec<QualityManifestEntry>, Vec<QualityManifestEntry>)> {
    let (native, forbidden) = load_quality_manifest(manifest_path)?;
    let input_hash = ocrx_core::sha256_file(input_path.as_ref())?;
    if native
        .iter()
        .chain(&forbidden)
        .any(|entry| entry.source_sha256 != input_hash)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manifest source hash does not match input",
        ));
    }
    Ok((native, forbidden))
}

/// Computes character-level normalized edit distance in `[0, 1]`.
pub fn normalized_edit_distance(expected: &str, actual: &str) -> f32 {
    let expected: Vec<char> = expected.chars().collect();
    let actual: Vec<char> = actual.chars().collect();
    if expected.is_empty() && actual.is_empty() {
        return 0.0;
    }
    if expected.is_empty() || actual.is_empty() {
        return 1.0;
    }
    let mut previous: Vec<usize> = (0..=actual.len()).collect();
    for (row, expected_char) in expected.iter().enumerate() {
        let mut current = vec![row + 1; actual.len() + 1];
        for (column, actual_char) in actual.iter().enumerate() {
            current[column + 1] = if expected_char == actual_char {
                previous[column]
            } else {
                1 + previous[column]
                    .min(previous[column + 1])
                    .min(current[column])
            };
        }
        previous = current;
    }
    previous[actual.len()] as f32 / expected.len().max(actual.len()) as f32
}

pub fn block_precision_recall_f1(
    expected: &[NativeTextBlock],
    actual: &[NativeTextBlock],
) -> (f32, f32, f32) {
    let mut matched_expected = vec![false; expected.len()];
    let mut matches = 0usize;
    for candidate in actual {
        if let Some((index, _)) = expected.iter().enumerate().find(|(index, truth)| {
            !matched_expected[*index]
                && truth.text == candidate.text
                && bbox_iou(truth.bbox, candidate.bbox) >= 0.5
        }) {
            matched_expected[index] = true;
            matches += 1;
        }
    }
    let precision = if actual.is_empty() {
        0.0
    } else {
        matches as f32 / actual.len() as f32
    };
    let recall = if expected.is_empty() {
        0.0
    } else {
        matches as f32 / expected.len() as f32
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    (precision, recall, f1)
}

pub fn bbox_validity_rate(blocks: &[NativeTextBlock]) -> f32 {
    if blocks.is_empty() {
        return 1.0;
    }
    blocks.iter().filter(|block| block.bbox.is_valid()).count() as f32 / blocks.len() as f32
}

pub fn bbox_recall(expected: &[NativeTextBlock], actual: &[NativeTextBlock]) -> f32 {
    if expected.is_empty() {
        return 1.0;
    }
    let mut used = vec![false; actual.len()];
    let matched = expected
        .iter()
        .filter(|truth| {
            if let Some(index) = actual.iter().enumerate().position(|(index, candidate)| {
                !used[index] && bbox_iou(truth.bbox, candidate.bbox) >= 0.5
            }) {
                used[index] = true;
                true
            } else {
                false
            }
        })
        .count();
    matched as f32 / expected.len() as f32
}

pub fn reading_order_accuracy(expected: &[NativeTextBlock], actual: &[NativeTextBlock]) -> f32 {
    let mut actual_indices = Vec::new();
    let mut used = vec![false; expected.len()];
    for candidate in actual {
        if let Some((index, _)) = expected.iter().enumerate().find(|(index, truth)| {
            !used[*index]
                && truth.text == candidate.text
                && bbox_iou(truth.bbox, candidate.bbox) >= 0.5
        }) {
            used[index] = true;
            actual_indices.push(index);
        }
    }
    if actual_indices.len() < 2 {
        return if actual_indices.is_empty() { 0.0 } else { 1.0 };
    }
    let total = actual_indices.len() * (actual_indices.len() - 1) / 2;
    let inversions = actual_indices
        .iter()
        .enumerate()
        .flat_map(|(left, value)| {
            actual_indices
                .iter()
                .skip(left + 1)
                .map(move |right| (*value, *right))
        })
        .filter(|(left, right)| left > right)
        .count();
    1.0 - inversions as f32 / total as f32
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteConfusionMatrix {
    pub counts: [[u32; 5]; 5],
}

fn page_type_index(page_type: PageType) -> usize {
    match page_type {
        PageType::Text => 0,
        PageType::Image => 1,
        PageType::Mixed => 2,
        PageType::Empty => 3,
        PageType::Uncertain => 4,
    }
}

pub fn route_confusion_matrix(expected: &[PageType], actual: &[PageType]) -> RouteConfusionMatrix {
    let mut matrix = RouteConfusionMatrix::default();
    for (expected, actual) in expected.iter().zip(actual) {
        matrix.counts[page_type_index(*expected)][page_type_index(*actual)] += 1;
    }
    matrix
}

pub fn route_accuracy(matrix: &RouteConfusionMatrix) -> f32 {
    let total: u32 = matrix.counts.iter().flatten().sum();
    if total == 0 {
        return 1.0;
    }
    let correct: u32 = (0..matrix.counts.len())
        .map(|index| matrix.counts[index][index])
        .sum();
    correct as f32 / total as f32
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeReleaseMetrics {
    pub total_pages: usize,
    pub native_pages: usize,
    pub fallback_pages: usize,
    pub ocr_pages: usize,
    pub failed_pages: usize,
    pub text_ned: f32,
    pub ocr_baseline_text_ned: f32,
    pub reading_order_accuracy: f32,
    pub ocr_baseline_reading_order_accuracy: f32,
    pub block_precision: f32,
    pub block_recall: f32,
    pub block_f1: f32,
    pub bbox_validity_rate: f32,
    pub bbox_recall: f32,
    pub duplicate_pages: usize,
    pub blank_pages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReleaseGateReport {
    pub passed: bool,
    pub failure: Option<&'static str>,
    pub failures: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceGateInput {
    pub auto_elapsed_ms: Vec<u64>,
    pub ocr_elapsed_ms: Vec<u64>,
    pub auto_all_native: bool,
    pub model_loaded: bool,
}

pub fn validate_performance_gate(input: &PerformanceGateInput) -> Result<(), &'static str> {
    if input.auto_elapsed_ms.len() < 5 || input.ocr_elapsed_ms.len() < 5 {
        return Err("performance gate requires at least five measurements per mode");
    }
    if !input.auto_all_native {
        return Err("performance gate requires every auto run to finish as native_text");
    }
    if input.model_loaded {
        return Err("performance gate requires auto runs without model loading");
    }
    let median = |values: &[u64]| {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    };
    if median(&input.auto_elapsed_ms) * 2 > median(&input.ocr_elapsed_ms) {
        return Err("auto median elapsed time exceeds fifty percent of OCR");
    }
    Ok(())
}

impl NativeReleaseGateReport {
    #[must_use]
    pub fn to_json(&self) -> String {
        let failure = self
            .failure
            .map_or_else(|| "null".to_owned(), |value| format!("\"{value}\""));
        let failures = self
            .failures
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"passed\":{},\"failure\":{failure},\"failures\":[{failures}]}}",
            self.passed
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseReport {
    pub gate: NativeReleaseGateReport,
    pub native_manifest_pages: usize,
    pub forbidden_manifest_pages: usize,
    pub performance_status: String,
}

impl ReleaseReport {
    #[must_use]
    pub fn to_json(&self) -> String {
        let failures = self
            .gate
            .failures
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":\"ocrx-release-report-v1\",\"passed\":{},\"failures\":[{failures}],\"native_manifest_pages\":{},\"forbidden_manifest_pages\":{},\"performance_status\":\"{}\"}}",
            self.gate.passed,
            self.native_manifest_pages,
            self.forbidden_manifest_pages,
            self.performance_status
        )
    }
}

#[must_use]
pub fn generate_release_report(
    metrics: NativeReleaseMetrics,
    native_manifest_pages: usize,
    forbidden_manifest_pages: usize,
    performance_status: &str,
) -> ReleaseReport {
    ReleaseReport {
        gate: native_release_gate_report(metrics),
        native_manifest_pages,
        forbidden_manifest_pages,
        performance_status: performance_status.to_owned(),
    }
}

pub fn native_release_gate_report(metrics: NativeReleaseMetrics) -> NativeReleaseGateReport {
    let mut failures = Vec::new();
    if metrics.total_pages == 0 {
        failures.push("native release metrics have no valid denominator");
    }
    if metrics.native_pages != metrics.total_pages {
        failures.push("native route coverage is below 100%");
    }
    if metrics.fallback_pages + metrics.ocr_pages + metrics.failed_pages != 0 {
        failures.push("native release contains fallback, OCR, or failed pages");
    }
    if metrics.text_ned < 0.99 {
        failures.push("text NED is below release threshold");
    }
    if metrics.text_ned + 0.005 < metrics.ocr_baseline_text_ned {
        failures.push("text NED regresses against OCR baseline");
    }
    if metrics.reading_order_accuracy < 0.98 {
        failures.push("reading order accuracy is below release threshold");
    }
    if metrics.reading_order_accuracy + 0.01 < metrics.ocr_baseline_reading_order_accuracy {
        failures.push("reading order accuracy regresses against OCR baseline");
    }
    if metrics.block_precision < 0.97 || metrics.block_recall < 0.97 || metrics.block_f1 < 0.97 {
        failures.push("block score is below release threshold");
    }
    if metrics.bbox_validity_rate < 0.99 || metrics.bbox_recall < 0.97 {
        failures.push("bbox score is below release threshold");
    }
    if metrics.duplicate_pages + metrics.blank_pages != 0 {
        failures.push("native release contains duplicate or blank pages");
    }
    NativeReleaseGateReport {
        passed: failures.is_empty(),
        failure: failures.first().copied(),
        failures,
    }
}

pub fn validate_native_release_metrics(metrics: NativeReleaseMetrics) -> Result<(), &'static str> {
    native_release_gate_report(metrics)
        .failure
        .map_or(Ok(()), Err)
}

impl DocumentType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Mixed => "mixed",
            Self::Empty => "empty",
            Self::Uncertain => "uncertain",
        }
    }
}

/// Reduces page facts to a stable document-level classification.
pub fn summarize_document_type(pages: &[PageClassification]) -> DocumentType {
    if pages.is_empty() {
        return DocumentType::Empty;
    }
    let mut has_text = false;
    let mut has_image = false;
    let mut has_empty = false;
    let mut has_uncertain = false;
    for page in pages {
        match page.page_type {
            PageType::Text => has_text = true,
            PageType::Image => has_image = true,
            PageType::Mixed => {
                has_text = true;
                has_image = true;
            }
            PageType::Empty => has_empty = true,
            PageType::Uncertain => has_uncertain = true,
        }
    }
    if has_uncertain {
        DocumentType::Uncertain
    } else if has_text && has_image {
        DocumentType::Mixed
    } else if has_text {
        DocumentType::Text
    } else if has_image {
        DocumentType::Image
    } else if has_empty {
        DocumentType::Empty
    } else {
        DocumentType::Uncertain
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeTextBlock {
    pub text: String,
    pub bbox: NormalizedBbox,
    pub order: u32,
    pub source: ocrx_core::BlockSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativePage {
    pub text: String,
    pub blocks: Vec<NativeTextBlock>,
    pub signals: PageSignals,
}

/// Coalesces adjacent character facts that belong to the same visual line.
/// The input must already be in PDF text order.
pub fn coalesce_native_text_blocks(blocks: &[NativeTextBlock]) -> Vec<NativeTextBlock> {
    let mut result = Vec::new();
    for block in blocks {
        let Some(previous) = result.last_mut() else {
            result.push(block.clone());
            continue;
        };
        let previous_center = u32::from(previous.bbox.top + previous.bbox.bottom) / 2;
        let current_center = u32::from(block.bbox.top + block.bbox.bottom) / 2;
        let previous_height = u32::from(previous.bbox.bottom - previous.bbox.top);
        let current_height = u32::from(block.bbox.bottom - block.bbox.top);
        let center_delta = previous_center.abs_diff(current_center);
        if center_delta <= previous_height.max(current_height) / 2
            && block.bbox.left.saturating_sub(previous.bbox.right) <= 40
            && !previous.text.ends_with(['\n', '\r'])
            && !block.text.starts_with(['\n', '\r'])
            && previous.source == block.source
        {
            previous.text.push_str(&block.text);
            previous.bbox = NormalizedBbox::new(
                previous.bbox.left.min(block.bbox.left),
                previous.bbox.top.min(block.bbox.top),
                previous.bbox.right.max(block.bbox.right),
                previous.bbox.bottom.max(block.bbox.bottom),
            )
            .unwrap_or(previous.bbox);
        } else {
            result.push(block.clone());
        }
    }
    for (index, block) in result.iter_mut().enumerate() {
        block.order = index as u32 + 1;
    }
    result
}

fn has_duplicate_text_geometry(blocks: &[NativeTextBlock]) -> bool {
    let mut seen = HashSet::new();
    blocks.iter().any(|block| {
        !seen.insert((
            block.text.as_str(),
            block.bbox.left,
            block.bbox.top,
            block.bbox.right,
            block.bbox.bottom,
        ))
    })
}

fn has_geometry_anomaly(blocks: &[NativeTextBlock]) -> bool {
    blocks.iter().enumerate().any(|(index, left)| {
        blocks.iter().skip(index + 1).any(|right| {
            let intersection_width = left
                .bbox
                .right
                .min(right.bbox.right)
                .saturating_sub(left.bbox.left.max(right.bbox.left));
            let intersection_height = left
                .bbox
                .bottom
                .min(right.bbox.bottom)
                .saturating_sub(left.bbox.top.max(right.bbox.top));
            let intersection = u32::from(intersection_width) * u32::from(intersection_height);
            // Widen before multiplying (M-9): in u16 the products overflow at
            // ~300-unit squares, panicking debug builds and silently wrapping
            // in release — the overlap detector then randomly passes
            // overlapping text blocks as valid native text.
            let left_area = u32::from(left.bbox.right - left.bbox.left)
                * u32::from(left.bbox.bottom - left.bbox.top);
            let right_area = u32::from(right.bbox.right - right.bbox.left)
                * u32::from(right.bbox.bottom - right.bbox.top);
            let smaller = left_area.min(right_area);
            left.text != right.text && smaller > 0 && intersection * 100 >= smaller * 80
        })
    })
}

/// Converts native PDF text facts into the existing page document contract.
pub fn native_page_document(
    page_number: u32,
    page: &NativePage,
) -> Result<ocrx_core::PageDocument, ocrx_core::DomainError> {
    let document = ocrx_core::PageDocument {
        schema_version: ocrx_core::PageDocument::SCHEMA_VERSION,
        page_number,
        blocks: vec![ocrx_core::PageBlock::Markdown {
            content: page.text.clone(),
        }],
        termination: ocrx_core::Termination::Eos,
        warnings: Vec::new(),
    };
    document.validate()?;
    Ok(document)
}

pub fn native_pdf_page_document(
    page_number: u32,
    page: &NativePage,
    classification: ocrx_core::PageClassification,
) -> Result<ocrx_core::PdfPageDocument, ocrx_core::DomainError> {
    let document = native_page_document(page_number, page)?;
    let routed = ocrx_core::PdfPageDocument {
        schema_version: ocrx_core::PdfPageDocument::SCHEMA_VERSION,
        document,
        classification,
        processing: ocrx_core::PageProcessing {
            planned_method: classification.recommended_method,
            actual_method: Some(ocrx_core::ProcessingMethod::NativeText),
            fallback_reason: None,
        },
        provenance: page
            .blocks
            .iter()
            .map(|block| ocrx_core::PdfBlockProvenance {
                source: block.source,
                bbox: Some(block.bbox),
                source_order: block.order,
            })
            .collect(),
    };
    routed.validate()?;
    Ok(routed)
}

#[must_use]
pub fn native_page_markdown(document: &ocrx_core::PageDocument) -> String {
    document
        .blocks
        .iter()
        .filter_map(|block| match block {
            ocrx_core::PageBlock::Markdown { content } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn validate_native_candidate(
    page_number: u32,
    page: &NativePage,
    tier: QualityTier,
) -> Result<ocrx_core::PageDocument, PdfReasonCode> {
    validate_native_page(tier, page.signals, &page.blocks)?;
    native_page_document(page_number, page).map_err(|_| PdfReasonCode::NativeValidationFailed)
}

pub fn prepare_native_page(
    renderer: &PdfiumRenderer,
    input: impl AsRef<Path>,
    page_number: usize,
    tier: QualityTier,
) -> Result<ocrx_core::PageDocument, PdfReasonCode> {
    let page = renderer
        .extract_native_page(input, page_number)
        .map_err(|_| PdfReasonCode::NativeValidationFailed)?;
    validate_native_candidate(page_number as u32 + 1, &page, tier)
}

pub fn native_page_artifacts(
    page_number: u32,
    page: &NativePage,
    tier: QualityTier,
) -> Result<(String, String), PdfReasonCode> {
    let document = validate_native_candidate(page_number, page, tier)?;
    let markdown = native_page_markdown(&document);
    let json = document
        .to_json()
        .map_err(|_| PdfReasonCode::NativeValidationFailed)?;
    Ok((markdown, json))
}

pub fn native_pdf_page_artifacts(
    page_number: u32,
    page: &NativePage,
    classification: ocrx_core::PageClassification,
    tier: QualityTier,
) -> Result<(String, String), PdfReasonCode> {
    let native = validate_native_candidate(page_number, page, tier)?;
    let routed = native_pdf_page_document(page_number, page, classification)
        .map_err(|_| PdfReasonCode::NativeValidationFailed)?;
    let markdown = native_page_markdown(&native);
    let json = routed
        .to_json()
        .map_err(|_| PdfReasonCode::NativeValidationFailed)?;
    Ok((markdown, json))
}

/// Builds the stable routed artifact for a page with no visible PDF content.
/// Empty pages are still selected-page results: they receive JSON, Markdown,
/// and a complete checkpoint just like native and OCR pages.
pub fn empty_pdf_page_artifacts(
    page_number: u32,
    classification: PageClassification,
) -> Result<(String, String), ocrx_core::DomainError> {
    if classification.page_type != PageType::Empty
        || classification.recommended_method != ProcessingMethod::Empty
    {
        return Err(ocrx_core::DomainError::InvalidPage);
    }
    let routed = PdfPageDocument {
        schema_version: PdfPageDocument::SCHEMA_VERSION,
        document: PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION,
            page_number,
            blocks: vec![PageBlock::Markdown {
                content: String::new(),
            }],
            termination: Termination::Eos,
            warnings: Vec::new(),
        },
        classification,
        processing: ocrx_core::PageProcessing {
            planned_method: ProcessingMethod::Empty,
            actual_method: Some(ProcessingMethod::Empty),
            fallback_reason: None,
        },
        provenance: Vec::new(),
    };
    routed.validate()?;
    let json = routed.to_json()?;
    Ok((String::new(), json))
}

/// Rejects native candidates that cannot safely satisfy the requested tier.
pub fn validate_native_page(
    tier: QualityTier,
    analysis: PageSignals,
    blocks: &[NativeTextBlock],
) -> Result<(), PdfReasonCode> {
    if [
        analysis.page_width,
        analysis.page_height,
        analysis.text_bbox_coverage,
        analysis.unicode_validity,
        analysis.alphanumeric_ratio,
        analysis.text_coverage,
        analysis.image_coverage,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return Err(PdfReasonCode::NativeValidationFailed);
    }
    if blocks.is_empty() || analysis.visible_characters == 0 {
        return Err(PdfReasonCode::NativeValidationFailed);
    }
    if analysis.duplicate_text
        || analysis.geometry_anomaly
        || analysis.unicode_validity < 0.90
        || analysis.alphanumeric_ratio < 0.70
    {
        return Err(PdfReasonCode::NativeValidationFailed);
    }
    let block_character_count = blocks
        .iter()
        .flat_map(|block| block.text.chars())
        .filter(|character| !character.is_whitespace())
        .count();
    let has_exact_duplicate = has_duplicate_text_geometry(blocks);
    if blocks.iter().any(|block| {
        block.text.is_empty()
            || !block.bbox.is_valid()
            || block.source != ocrx_core::BlockSource::PdfText
    }) || blocks.iter().any(|block| block.order == 0)
        || blocks.windows(2).any(|pair| pair[0].order >= pair[1].order)
        || has_exact_duplicate
        || block_character_count != analysis.visible_characters
    {
        return Err(PdfReasonCode::NativeValidationFailed);
    }
    if matches!(tier, QualityTier::L2) && analysis.text_coverage < 0.20 {
        return Err(PdfReasonCode::NativeStructureUnresolved);
    }
    if matches!(tier, QualityTier::L2) && analysis.text_bbox_coverage < 0.0001 {
        return Err(PdfReasonCode::NativeStructureUnresolved);
    }
    Ok(())
}

impl Default for PageSignals {
    fn default() -> Self {
        Self {
            page_width: 0.0,
            page_height: 0.0,
            image_objects: 0,
            text_bbox_coverage: 0.0,
            visible_characters: 0,
            unicode_validity: 1.0,
            alphanumeric_ratio: 1.0,
            text_coverage: 0.0,
            image_coverage: 0.0,
            duplicate_text: false,
            geometry_anomaly: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassifierConfig {
    pub version: &'static str,
    pub image_coverage_threshold: f32,
    pub mixed_image_threshold: f32,
    pub text_coverage_threshold: f32,
    pub min_unicode_validity: f32,
    pub min_alphanumeric_ratio: f32,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            version: "pdf-route-v1",
            image_coverage_threshold: 0.55,
            mixed_image_threshold: 0.10,
            text_coverage_threshold: 0.20,
            min_unicode_validity: 0.90,
            // Separation measured on the W0.4 truth composite: real prose
            // pages sit at ~0.84 (CJK ideographs are alphanumeric), broken
            // ToUnicode garbage at <=0.55. A false positive only sends the
            // page to OCR, so the threshold leans high rather than risk
            // garbage native output.
            min_alphanumeric_ratio: 0.70,
        }
    }
}

/// Deterministic, versioned P0 page router. Thresholds are intentionally not user configurable.
#[must_use]
pub fn classify_page(signals: PageSignals, _tier: QualityTier) -> PageClassification {
    classify_page_with_config(signals, ClassifierConfig::default())
}

#[must_use]
pub fn classify_pages(signals: &[PageSignals], tier: QualityTier) -> Vec<PageClassification> {
    classify_pages_with_config(signals, tier, ClassifierConfig::default())
}

#[must_use]
pub fn classify_pages_with_config(
    signals: &[PageSignals],
    _tier: QualityTier,
    config: ClassifierConfig,
) -> Vec<PageClassification> {
    signals
        .iter()
        .copied()
        .map(|page| classify_page_with_config(page, config))
        .collect()
}

#[must_use]
pub fn classify_page_with_config(
    signals: PageSignals,
    config: ClassifierConfig,
) -> PageClassification {
    if [
        signals.page_width,
        signals.page_height,
        signals.text_bbox_coverage,
        signals.unicode_validity,
        signals.alphanumeric_ratio,
        signals.text_coverage,
        signals.image_coverage,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return PageClassification {
            page_type: PageType::Uncertain,
            confidence: RouteConfidence::Low,
            recommended_method: ProcessingMethod::Ocr,
            reason: PdfReasonCode::GeometryAnomaly,
        };
    }
    if signals.visible_characters == 0 && signals.image_coverage < 0.01 {
        return PageClassification {
            page_type: PageType::Empty,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::Empty,
            reason: PdfReasonCode::NoVisibleContent,
        };
    }
    if signals.image_coverage >= config.image_coverage_threshold
        && signals.text_coverage < config.text_coverage_threshold
    {
        return PageClassification {
            page_type: PageType::Image,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::Ocr,
            reason: PdfReasonCode::ImageCoverageHigh,
        };
    }
    // A text layer can be structurally valid (real glyphs, sane geometry) yet
    // carry a broken ToUnicode CMap: every char maps to arbitrary punctuation,
    // which passes the unicode-validity ratio. Real text in any script is
    // dominated by letters/digits (CJK ideographs are alphanumeric), so a
    // near-zero ratio means the extracted text cannot be trusted and the page
    // must go to OCR instead. The reason reuses the stable
    // `unicode_validity_low` code: a broken CMap is a text-mapping failure.
    if signals.duplicate_text
        || signals.geometry_anomaly
        || signals.unicode_validity < config.min_unicode_validity
        || (signals.visible_characters > 0
            && signals.alphanumeric_ratio < config.min_alphanumeric_ratio)
    {
        return PageClassification {
            page_type: PageType::Uncertain,
            confidence: RouteConfidence::Low,
            recommended_method: ProcessingMethod::Ocr,
            reason: if signals.duplicate_text {
                PdfReasonCode::DuplicateText
            } else if signals.geometry_anomaly {
                PdfReasonCode::GeometryAnomaly
            } else {
                PdfReasonCode::UnicodeValidityLow
            },
        };
    }
    if signals.image_coverage >= config.mixed_image_threshold && signals.visible_characters > 0 {
        return PageClassification {
            page_type: PageType::Mixed,
            confidence: RouteConfidence::Medium,
            recommended_method: ProcessingMethod::Ocr,
            reason: PdfReasonCode::MixedContent,
        };
    }
    if signals.visible_characters >= 1 && signals.text_coverage >= config.text_coverage_threshold {
        return PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
    }
    PageClassification {
        page_type: PageType::Uncertain,
        confidence: RouteConfidence::Low,
        recommended_method: ProcessingMethod::Ocr,
        reason: PdfReasonCode::TextCoverageLow,
    }
}

/// Converts classification facts into the immutable first execution plan.
#[must_use]
pub fn plan_page(mode: ocrx_core::PdfMode, classification: PageClassification) -> ProcessingMethod {
    match mode {
        ocrx_core::PdfMode::Ocr => match classification.page_type {
            PageType::Empty => ProcessingMethod::Empty,
            _ => ProcessingMethod::Ocr,
        },
        ocrx_core::PdfMode::Text => match classification.page_type {
            PageType::Empty => ProcessingMethod::Empty,
            _ => ProcessingMethod::NativeText,
        },
        ocrx_core::PdfMode::Auto => classification.recommended_method,
    }
}

pub fn plan_page_checked(
    mode: ocrx_core::PdfMode,
    classification: PageClassification,
) -> Result<ProcessingMethod, PdfReasonCode> {
    if mode == ocrx_core::PdfMode::Text
        && !matches!(classification.page_type, PageType::Text | PageType::Empty)
    {
        return Err(PdfReasonCode::NativeValidationFailed);
    }
    Ok(plan_page(mode, classification))
}

/// Maps PDFium boundary failures to stable, user-facing error classes.
///
/// `pdfium-render`'s `Display` impl prints the `Debug` representation, so error
/// strings contain variant names like `LoadLibraryError` (a dynamic-library
/// *loading* failure) and `PdfiumLibraryInternalError(FormatError)` (a content
/// failure). The broad substring "library" appears in both, so we match the
/// specific loading variants rather than "library" to avoid misclassifying a
/// format/password error as a dynamic-library failure.
#[must_use]
pub fn classify_pdf_error(message: &str) -> PdfErrorKind {
    let message = message.to_ascii_lowercase();
    if message.contains("loadlibraryerror")
        || message.contains("loadlibraryfunctionnameerror")
        || message.contains("dlopen")
        || message.contains("bind_to_library")
        || message.contains("dynamic library")
    {
        PdfErrorKind::DynamicLibrary
    } else if message.contains("passworderror")
        || message.contains("securityerror")
        || message.contains("password")
        || message.contains("encrypted")
        || message.contains("security")
    {
        PdfErrorKind::Encrypted
    } else if message.contains("formaterror")
        || message.contains("not a pdf")
        || message.contains("invalid pdf")
        || message.contains("file header")
    {
        // pdfium returns FormatError for both non-PDF headers and truncated
        // structures; without a separate corrupt signal we map both to NonPdf.
        PdfErrorKind::NonPdf
    } else if message.contains("corrupt")
        || message.contains("malformed")
        || message.contains("damaged")
        || message.contains("parse")
    {
        PdfErrorKind::Corrupt
    } else {
        PdfErrorKind::Unknown
    }
}

#[must_use]
pub fn fit_pixel_budget(width: u32, height: u32, budget: u64) -> (u32, u32, bool) {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= budget || budget == 0 {
        return (width, height, false);
    }
    let scale = (budget as f64 / pixels as f64).sqrt();
    (
        ((f64::from(width) * scale).floor() as u32).max(1),
        ((f64::from(height) * scale).floor() as u32).max(1),
        true,
    )
}

/// Crops a validated normalized bbox without allowing out-of-bounds access.
///
/// # Errors
///
/// Returns an error when the bbox is invalid or maps to an empty crop.
pub fn crop_bbox(image: &RgbImage, bbox: NormalizedBbox) -> Result<RgbImage, &'static str> {
    if !bbox.is_valid() {
        return Err("invalid bbox");
    }
    let left = (u64::from(image.width()) * u64::from(bbox.left) / 1000) as u32;
    let top = (u64::from(image.height()) * u64::from(bbox.top) / 1000) as u32;
    let right = ((u64::from(image.width()) * u64::from(bbox.right) + 999) / 1000)
        .min(u64::from(image.width())) as u32;
    let bottom = ((u64::from(image.height()) * u64::from(bbox.bottom) + 999) / 1000)
        .min(u64::from(image.height())) as u32;
    if left >= right || top >= bottom {
        return Err("empty crop");
    }
    Ok(image::imageops::crop_imm(image, left, top, right - left, bottom - top).to_image())
}

#[must_use]
pub fn bbox_iou(a: NormalizedBbox, b: NormalizedBbox) -> f32 {
    let left = a.left.max(b.left) as f32;
    let top = a.top.max(b.top) as f32;
    let right = a.right.min(b.right) as f32;
    let bottom = a.bottom.min(b.bottom) as f32;
    let intersection = (right - left).max(0.0) * (bottom - top).max(0.0);
    let area_a = f32::from(a.right - a.left) * f32::from(a.bottom - a.top);
    let area_b = f32::from(b.right - b.left) * f32::from(b.bottom - b.top);
    intersection / (area_a + area_b - intersection).max(1.0)
}

/// Saves a cropped figure using a program-generated name and atomic rename.
pub fn save_bbox_png(
    image: &RgbImage,
    bbox: NormalizedBbox,
    output_root: impl AsRef<Path>,
    page: u32,
    figure: u32,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if page == 0 || figure == 0 {
        return Err("page and figure must be positive".into());
    }
    let cropped = crop_bbox(image, bbox).map_err(std::io::Error::other)?;
    let root = output_root.as_ref().join("images");
    fs::create_dir_all(&root)?;
    let final_path = root.join(format!("page-{page:04}-figure-{figure:03}.png"));
    let part_path = root.join(format!(".page-{page:04}-figure-{figure:03}.part.png"));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(cropped).write_to(&mut bytes, image::ImageFormat::Png)?;
    fs::write(&part_path, bytes.into_inner())?;
    fs::rename(&part_path, &final_path)?;
    Ok(final_path)
}

/// Validates a worker page segment before it can be committed by the caller.
///
/// # Errors
///
/// Returns an error when the segment is empty, has missing/extra pages, has
/// temporary files, or contains an invalid PNG.
pub fn validate_worker_segment(
    output_root: impl AsRef<Path>,
    start_page: usize,
    end_page: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if start_page >= end_page {
        return Err("worker segment must be non-empty".into());
    }
    let root = output_root.as_ref();
    let mut png_names = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("part")
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(".part."))
        {
            return Err(format!("worker left temporary file: {}", path.display()).into());
        }
        if path.extension().and_then(|value| value.to_str()) == Some("png") {
            png_names.push(path);
        }
    }
    let expected_count = end_page - start_page;
    if png_names.len() != expected_count {
        return Err(format!(
            "worker output count mismatch: {} != {expected_count}",
            png_names.len()
        )
        .into());
    }
    for page in start_page..end_page {
        let path = root.join(format!("page-{:04}.png", page + 1));
        if !path.is_file() {
            return Err(format!("worker output missing: {}", path.display()).into());
        }
        // Header-only dimension read (review m19): validating the audit
        // dimensions does not need the pixels, and each full decode of a
        // 300-DPI page PNG costs real time in the scan hot path.
        let actual_dimensions = image::image_dimensions(&path)
            .map_err(|error| format!("invalid worker PNG {}: {error}", path.display()))?;
        let audit_path = root.join(format!("page-{:04}.audit", page + 1));
        let audit = fs::read_to_string(&audit_path)
            .map_err(|error| format!("invalid worker audit {}: {error}", audit_path.display()))?;
        let rendered = audit
            .split_once(";rendered=")
            .and_then(|(_, value)| value.split_once(';').or(Some((value, ""))))
            .ok_or_else(|| format!("invalid worker audit {}", audit_path.display()))?
            .0;
        let (width, height) = rendered.split_once('x').ok_or_else(|| {
            format!(
                "invalid worker rendered dimensions {}",
                audit_path.display()
            )
        })?;
        let metadata_dimensions = (width.parse::<u32>(), height.parse::<u32>());
        if metadata_dimensions != (Ok(actual_dimensions.0), Ok(actual_dimensions.1)) {
            return Err(format!(
                "worker audit dimensions do not match PNG {}",
                path.display()
            )
            .into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSegmentReport {
    pub start_page: usize,
    pub end_page: usize,
    pub pages: usize,
    pub elapsed_millis: u128,
    pub peak_rss_kb: u64,
}

#[derive(Debug, Clone)]
pub struct PdfWorkerScheduler {
    pub worker_executable: PathBuf,
    pub library_dir: PathBuf,
    pub segment_pages: usize,
    pub timeout: Duration,
    pub dpi: u16,
    /// Extra environment variables applied to each spawned worker process.
    /// Production scans leave this empty; tests use it to drive a fake worker's
    /// behavior without touching the process-wide environment (edition 2024
    /// forbids `std::env::set_var`).
    pub env: Vec<(String, String)>,
    /// Password forwarded to the worker as its optional 7th protocol argument
    /// so encrypted PDFs render in worker mode exactly like in-process.
    pub password: Option<String>,
}

impl PdfWorkerScheduler {
    /// Sets the password passed to every worker (encrypted PDF support).
    #[must_use]
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }
}

impl PdfWorkerScheduler {
    /// Runs all requested pages in isolated worker processes.
    ///
    /// # Errors
    ///
    /// Returns an error when the range, worker, process exit, or page outputs are invalid.
    pub fn run(
        &self,
        input: impl AsRef<Path>,
        output_root: impl AsRef<Path>,
        start_page: usize,
        end_page: usize,
    ) -> Result<Vec<WorkerSegmentReport>, Box<dyn std::error::Error>> {
        self.run_with_cancel(input, output_root, start_page, end_page, || false)
    }

    /// Runs worker segments and stops when the callback requests cancellation.
    ///
    /// # Errors
    ///
    /// Returns an error for cancellation, timeout, worker failure, or invalid output.
    pub fn run_with_cancel(
        &self,
        input: impl AsRef<Path>,
        output_root: impl AsRef<Path>,
        start_page: usize,
        end_page: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Vec<WorkerSegmentReport>, Box<dyn std::error::Error>> {
        if self.segment_pages == 0 || start_page >= end_page {
            return Err("invalid worker range or segment size".into());
        }
        if !self.worker_executable.is_file() {
            return Err("worker executable is missing".into());
        }
        let root = output_root.as_ref();
        fs::create_dir_all(root)?;
        let mut reports = Vec::new();
        let mut start = start_page;
        while start < end_page {
            let end = (start + self.segment_pages).min(end_page);
            let segment_root = root.join(format!(".worker-{start:04}-{end:04}"));
            if segment_root.exists() {
                fs::remove_dir_all(&segment_root)?;
            }
            fs::create_dir_all(&segment_root)?;
            let began = Instant::now();
            let mut command = Command::new(&self.worker_executable);
            command.args([
                self.library_dir.to_string_lossy().as_ref(),
                input.as_ref().to_string_lossy().as_ref(),
                &start.to_string(),
                &end.to_string(),
                segment_root.to_string_lossy().as_ref(),
                &self.dpi.to_string(),
            ]);
            if let Some(password) = &self.password {
                command.arg(password);
            }
            for (key, value) in &self.env {
                command.env(key, value);
            }
            // Worker diagnostics go to the parent's stderr, never stdout: the
            // CLI owns stdout for `--report json` / `--progress jsonl`, and a
            // chatty worker printing `page=N` progress lines must not
            // interleave with those contracts (review MINOR 4).
            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::inherit());
            let mut child = command.spawn()?;
            if let Some(stdout) = child.stdout.take() {
                std::thread::spawn(move || {
                    let mut stdout = stdout;
                    let _ = std::io::copy(&mut stdout, &mut std::io::stderr());
                });
            }
            let mut peak_rss_kb = 0;
            let mut tick = 0u32;
            let status = loop {
                if cancelled() {
                    // Best-effort cleanup (review m1): a transient
                    // remove_dir_all failure (AV/indexer lock) must not
                    // replace the cancelled semantics with an io error.
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_dir_all(&segment_root);
                    return Err("worker cancelled".into());
                }
                if began.elapsed() > self.timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_dir_all(&segment_root);
                    return Err("worker timed out".into());
                }
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                tick = tick.wrapping_add(1);
                // Sample RSS every ~100ms rather than every 10ms poll: spawning
                // `ps`/`wmic` each tick dominates the loop on Windows, and the
                // peak is captured fine at this cadence.
                if tick % 10 == 0 {
                    peak_rss_kb = peak_rss_kb.max(child_rss_kb(child.id()));
                }
                thread::sleep(Duration::from_millis(10));
            };
            if !status.success() {
                let _ = fs::remove_dir_all(&segment_root);
                return Err(format!("worker failed for [{start},{end}) with {status}").into());
            }
            if let Err(error) = validate_worker_segment(&segment_root, start, end) {
                let _ = fs::remove_dir_all(&segment_root);
                return Err(error);
            }
            for page in start..end {
                if let Err(error) = fs::rename(
                    segment_root.join(format!("page-{:04}.png", page + 1)),
                    root.join(format!("page-{:04}.png", page + 1)),
                ) {
                    let _ = fs::remove_dir_all(&segment_root);
                    return Err(error.into());
                }
                if let Err(error) = fs::rename(
                    segment_root.join(format!("page-{:04}.audit", page + 1)),
                    root.join(format!("page-{:04}.audit", page + 1)),
                ) {
                    let _ = fs::remove_dir_all(&segment_root);
                    return Err(error.into());
                }
            }
            // All pages of the segment are committed by now: cleanup is
            // best-effort, and a locked leftover directory must not fail an
            // otherwise complete segment (review m1).
            let _ = fs::remove_dir_all(&segment_root);
            reports.push(WorkerSegmentReport {
                start_page: start,
                end_page: end,
                pages: end - start,
                elapsed_millis: began.elapsed().as_millis(),
                peak_rss_kb,
            });
            start = end;
        }
        Ok(reports)
    }
}

fn child_rss_kb(pid: u32) -> u64 {
    // Delegates to the shared W0.3 sampler: on Windows it reads the working
    // set through `tasklist`, because `wmic` is removed by default on
    // Windows 11 24H2+ and used to silently return 0 on those machines (M5).
    // Best-effort: a refused sample counts as 0, matching the peak-tracking
    // semantics of the scheduler loop.
    use ocrx_core::resource_sampler::{ProcessMemorySampler, RealProcessMemorySampler};

    RealProcessMemorySampler
        .sample_rss(pid)
        .map(|sample| sample.rss_kb)
        .unwrap_or_default()
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { dpi: 300 }
    }
}

#[derive(Debug)]
pub struct PdfiumRenderer {
    library_dir: PathBuf,
    password: Option<String>,
}

// pdfium-render bindings are process-global; bind once and reuse them for a
// scan job. Successes are cached in a `OnceLock` so every later call reuses
// the same binding; bind attempts are serialized by a mutex because
// `Pdfium::new` asserts process-global uninitialized bindings. A failed bind
// is deliberately NOT cached: a transiently wrong `OCRX_PDFIUM_LIBRARY` must
// not poison every later call in the process — the next call retries.

/// Resolves the platform PDFium library file path under `library_dir`, rejecting
/// path-escape (`..` components) so a crafted `library_dir` can never traverse
/// out to load an unintended library. Pure: builds a path only, never loads.
///
/// # Errors
///
/// Returns [`PdfErrorKind::DynamicLibrary`] when `library_dir` contains a
/// parent-dir (`..`) component.
pub fn pdfium_library_path(library_dir: &Path) -> Result<PathBuf, PdfErrorKind> {
    if library_dir
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(PdfErrorKind::DynamicLibrary);
    }
    Ok(Pdfium::pdfium_platform_library_name_at_path(library_dir))
}

fn shared_pdfium(library_dir: &Path) -> Result<&'static Pdfium, Box<dyn std::error::Error>> {
    use std::sync::{Mutex, OnceLock};
    static PDFIUM: OnceLock<&'static Pdfium> = OnceLock::new();
    static BIND_MUTEX: Mutex<()> = Mutex::new(());

    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    let _bind_guard = BIND_MUTEX
        .lock()
        .map_err(|_| "pdfium bind mutex poisoned")?;
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    // OCRX_PDFIUM_LIBRARY may point at the library file itself or its
    // containing directory. Policy is applied to the canonical location, not
    // the spelling (review m18): a path that resolves on disk loads from its
    // real canonical location — the same `..`-containing spelling therefore
    // behaves identically whether it names the file or the directory, and
    // cannot name anywhere else than where it resolves. Only a spelling that
    // cannot be resolved (non-existent) falls back to the strict lexical
    // parent-dir rejection.
    let library = match std::fs::canonicalize(library_dir) {
        Ok(canonical) if canonical.is_file() => canonical,
        Ok(canonical_directory) => {
            // Canonical location exists and is a directory: resolve the
            // platform library file inside it.
            Pdfium::pdfium_platform_library_name_at_path(&canonical_directory)
        }
        Err(error) => {
            if library_dir
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(
                    "pdfium library path contains parent-dir (..) traversal; refused to load"
                        .to_owned()
                        .into(),
                );
            }
            return Err(format!("pdfium library path unavailable: {error}").into());
        }
    };
    let leaked: &'static Pdfium = Box::leak(Box::new(
        Pdfium::bind_to_library(library)
            .map(Pdfium::new)
            .map_err(|error| error.to_string())?,
    ));
    Ok(PDFIUM.get_or_init(|| leaked))
}

/// Pre-rasterization pixel budget shared by every render entry point: a page
/// whose MediaBox at the requested DPI would exceed this is rasterized at a
/// proportionally reduced size instead of allocating an unbounded bitmap
/// (a hostile `200000x200000` pt MediaBox at 300 DPI used to allocate ~200 GB
/// before any scaling could happen downstream).
const MAX_RENDER_PIXELS: u64 = 2880 * 2880;

/// Computes the rasterization bitmap size for one page: points → pixels at
/// the requested DPI, NaN/negative guarded to at least 1 px, then clamped to
/// [`MAX_RENDER_PIXELS`] by proportional downscale. Every render entry point
/// must go through this helper so no spelling of a hostile MediaBox can reach
/// `set_fixed_size` with an unbounded size (review BLK-4: the worker-mode
/// `render_range` path used to bypass the clamp).
fn clamped_bitmap_size(
    page_width: f32,
    page_height: f32,
    dpi: u16,
) -> Result<(i32, i32), Box<dyn std::error::Error>> {
    let raw_width = (page_width * f32::from(dpi) / 72.0).round().max(1.0) as u32;
    let raw_height = (page_height * f32::from(dpi) / 72.0).round().max(1.0) as u32;
    let (width, height, _) = fit_pixel_budget(raw_width, raw_height, MAX_RENDER_PIXELS);
    Ok((i32::try_from(width)?, i32::try_from(height)?))
}

impl PdfiumRenderer {
    pub fn new(library_dir: impl Into<PathBuf>) -> Self {
        Self {
            library_dir: library_dir.into(),
            password: None,
        }
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    pub fn render_page_rgb8(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        options: RenderOptions,
    ) -> Result<RgbImage, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        let document = pdfium.load_pdf_from_file(input, self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        let (width, height) =
            clamped_bitmap_size(page.width().value, page.height().value, options.dpi)?;
        Ok(page
            .render_with_config(&render_config(width, height))?
            .as_image()?
            .into_rgb8())
    }

    /// Renders one page into the RGB8 shape required by the runtime boundary.
    ///
    /// # Errors
    ///
    /// Returns the underlying PDFium or image conversion error.
    pub fn render_page_rgb8_bytes(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        options: RenderOptions,
    ) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
        let image = self.render_page_rgb8(input, page_number, options)?;
        let (width, height) = image.dimensions();
        Ok((width, height, image.into_raw()))
    }

    /// Renders a page and returns its source-at-72-DPI dimensions for audit metadata.
    ///
    /// The bitmap size is clamped to [`MAX_RENDER_PIXELS`] before allocation;
    /// `original_*` stays the true page size at 72 DPI so the audit still
    /// reports the unscaled geometry.
    pub fn render_page_rgb8_with_audit(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        options: RenderOptions,
    ) -> Result<(RgbImage, u32, u32), Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        let original_width = page.width().value.round().max(1.0) as u32;
        let original_height = page.height().value.round().max(1.0) as u32;
        let (width, height) =
            clamped_bitmap_size(page.width().value, page.height().value, options.dpi)?;
        let image = page
            .render_with_config(&render_config(width, height))?
            .as_image()?
            .into_rgb8();
        Ok((image, original_width, original_height))
    }

    pub fn page_count(&self, input: impl AsRef<Path>) -> Result<usize, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        Ok(pdfium
            .load_pdf_from_file(input, self.password.as_deref())?
            .pages()
            .len()
            .try_into()?)
    }

    /// Reads PDFium's text layer without rasterizing the page.
    pub fn extract_native_page(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
    ) -> Result<NativePage, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        let width = page.width().value.max(1.0);
        let height = page.height().value.max(1.0);
        let text_page = page.text()?;
        let image_area: f32 = page
            .objects()
            .iter()
            .filter(|object| object.as_image_object().is_some())
            .filter_map(|object| object.bounds().ok())
            .map(|bounds| {
                bounds.to_rect().width().value.max(0.0) * bounds.to_rect().height().value.max(0.0)
            })
            .sum();
        let image_objects = page
            .objects()
            .iter()
            .filter(|object| object.as_image_object().is_some())
            .count();
        let mut text = String::new();
        let mut blocks = Vec::new();
        let mut valid = 0usize;
        let mut alphanumeric = 0usize;
        let text_chars = text_page.chars();
        let chars: Vec<_> = text_chars.iter().collect();
        for (index, character) in chars.iter().enumerate() {
            let Some(value) = character.unicode_char() else {
                continue;
            };
            if value.is_control() && !matches!(value, '\n' | '\t') {
                continue;
            }
            text.push(value);
            if !value.is_whitespace() {
                // Validity counts characters with a real mapping (review m20):
                // pdfium surfaces unmapped glyphs as U+FFFD, and counting them
                // as valid made the ratio identically 1.0 - a dead signal.
                valid += usize::from(value != char::REPLACEMENT_CHARACTER);
                alphanumeric += usize::from(value.is_alphanumeric());
            }
            if let Ok(bounds) = character.tight_bounds() {
                let left = (bounds.left().value.max(0.0) / width * 1000.0)
                    .round()
                    .clamp(0.0, 999.0) as u16;
                let right = (bounds.right().value.max(0.0) / width * 1000.0)
                    .round()
                    .clamp(1.0, 1000.0) as u16;
                // Keep sub-page precision until after scaling. Rounding the
                // normalized ratio before multiplying by 1000 collapses most
                // glyph boxes to a one-unit strip and makes line coalescing
                // incorrectly merge an entire page into one coarse block.
                let top = ((1.0_f32 - bounds.top().value.max(0.0) / height) * 1000.0)
                    .round()
                    .clamp(0.0, 999.0) as u16;
                let bottom = ((1.0_f32 - bounds.bottom().value.max(0.0) / height) * 1000.0)
                    .round()
                    .clamp(1.0, 1000.0) as u16;
                if let Ok(bbox) =
                    NormalizedBbox::new(left, top.min(999), right, bottom.max(top + 1).min(1000))
                {
                    blocks.push(NativeTextBlock {
                        text: value.to_string(),
                        bbox,
                        order: index as u32 + 1,
                        source: ocrx_core::BlockSource::PdfText,
                    });
                }
            }
        }
        let character_count = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count();
        let blocks = coalesce_native_text_blocks(&blocks);
        let duplicate_text = has_duplicate_text_geometry(&blocks);
        let geometry_anomaly = has_geometry_anomaly(&blocks);
        let text_bbox_coverage = blocks
            .iter()
            .map(|block| {
                f32::from(block.bbox.right - block.bbox.left)
                    * f32::from(block.bbox.bottom - block.bbox.top)
                    / 1_000_000.0
            })
            .sum::<f32>()
            .clamp(0.0, 1.0);
        let has_blocks = !blocks.is_empty();
        Ok(NativePage {
            text,
            blocks,
            signals: PageSignals {
                page_width: width,
                page_height: height,
                image_objects,
                text_bbox_coverage,
                visible_characters: character_count,
                unicode_validity: if character_count == 0 {
                    1.0
                } else {
                    valid as f32 / character_count as f32
                },
                alphanumeric_ratio: if character_count == 0 {
                    1.0
                } else {
                    alphanumeric as f32 / character_count as f32
                },
                text_coverage: if has_blocks { 1.0 } else { 0.0 },
                image_coverage: (image_area / (width * height)).clamp(0.0, 1.0),
                duplicate_text,
                geometry_anomaly,
            },
        })
    }

    /// Analyzes one page's PDF objects without rasterizing it.
    pub fn analyze_page(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        tier: QualityTier,
    ) -> Result<PageClassification, Box<dyn std::error::Error>> {
        let signals = self.analyze_page_signals(input, page_number)?;
        Ok(classify_page(signals, tier))
    }

    /// Analyzes one page and preserves the raw, non-rendered PDF signals.
    pub fn analyze_page_signals(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
    ) -> Result<PageSignals, Box<dyn std::error::Error>> {
        // Keep routing analysis independent from the later native document
        // construction.  The auto pipeline deliberately has two phases:
        // analysis/classification first, then native extraction/validation.
        // Calling `extract_native_page` here would make those phases share
        // the same derived object and could hide a validation-only failure.
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        let width = page.width().value.max(1.0);
        let height = page.height().value.max(1.0);
        let text_page = page.text()?;
        let image_area: f32 = page
            .objects()
            .iter()
            .filter(|object| object.as_image_object().is_some())
            .filter_map(|object| object.bounds().ok())
            .map(|bounds| {
                bounds.to_rect().width().value.max(0.0) * bounds.to_rect().height().value.max(0.0)
            })
            .sum();
        let image_objects = page
            .objects()
            .iter()
            .filter(|object| object.as_image_object().is_some())
            .count();

        let mut valid = 0usize;
        let mut alphanumeric = 0usize;
        let mut visible_characters = 0usize;
        let mut analysis_blocks = Vec::new();
        for (index, character) in text_page.chars().iter().enumerate() {
            let Some(value) = character.unicode_char() else {
                continue;
            };
            if value.is_control() && !matches!(value, '\n' | '\t') {
                continue;
            }
            if !value.is_whitespace() {
                // Same non-replacement validity semantics as extract_native_page
                // (review m20).
                valid += usize::from(value != char::REPLACEMENT_CHARACTER);
                alphanumeric += usize::from(value.is_alphanumeric());
                visible_characters += 1;
            }
            if let Ok(bounds) = character.tight_bounds() {
                let left = (bounds.left().value.max(0.0) / width * 1000.0)
                    .round()
                    .clamp(0.0, 999.0) as u16;
                let right = (bounds.right().value.max(0.0) / width * 1000.0)
                    .round()
                    .clamp(1.0, 1000.0) as u16;
                let top = ((1.0_f32 - bounds.top().value.max(0.0) / height) * 1000.0)
                    .round()
                    .clamp(0.0, 999.0) as u16;
                let bottom = ((1.0_f32 - bounds.bottom().value.max(0.0) / height) * 1000.0)
                    .round()
                    .clamp(1.0, 1000.0) as u16;
                if let Ok(bbox) =
                    NormalizedBbox::new(left, top.min(999), right, bottom.max(top + 1).min(1000))
                {
                    analysis_blocks.push(NativeTextBlock {
                        text: value.to_string(),
                        bbox,
                        order: index as u32 + 1,
                        source: ocrx_core::BlockSource::PdfText,
                    });
                }
            }
        }
        let analysis_blocks = coalesce_native_text_blocks(&analysis_blocks);
        let has_blocks = !analysis_blocks.is_empty();
        let text_bbox_coverage = analysis_blocks
            .iter()
            .map(|block| {
                f32::from(block.bbox.right - block.bbox.left)
                    * f32::from(block.bbox.bottom - block.bbox.top)
                    / 1_000_000.0
            })
            .sum::<f32>()
            .clamp(0.0, 1.0);
        Ok(PageSignals {
            page_width: width,
            page_height: height,
            image_objects,
            text_bbox_coverage,
            visible_characters,
            unicode_validity: if visible_characters == 0 {
                1.0
            } else {
                valid as f32 / visible_characters as f32
            },
            alphanumeric_ratio: if visible_characters == 0 {
                1.0
            } else {
                alphanumeric as f32 / visible_characters as f32
            },
            text_coverage: if has_blocks { 1.0 } else { 0.0 },
            image_coverage: (image_area / (width * height)).clamp(0.0, 1.0),
            duplicate_text: has_duplicate_text_geometry(&analysis_blocks),
            geometry_anomaly: has_geometry_anomaly(&analysis_blocks),
        })
    }

    pub fn page_dimensions(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
    ) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        Ok((
            page.width().value.round().max(1.0) as u32,
            page.height().value.round().max(1.0) as u32,
        ))
    }

    pub fn render_document(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page_count = document.pages().len();
        for index in 0..page_count {
            let page = document.pages().get(index)?;
            let (width, height) =
                clamped_bitmap_size(page.width().value, page.height().value, options.dpi)?;
            let image = {
                let bitmap = page.render_with_config(&render_config(width, height))?;
                let image = bitmap.as_image()?.into_rgb8();
                drop(bitmap);
                image
            };
            drop(page);
            on_page(index.try_into()?, image)?;
        }
        Ok(page_count.try_into()?)
    }

    pub fn render_document_reopen_each_page(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        let page_count: usize = pdfium
            .load_pdf_from_file(input, self.password.as_deref())?
            .pages()
            .len()
            .try_into()?;
        for index in 0..page_count {
            let document = pdfium.load_pdf_from_file(input, self.password.as_deref())?;
            let page = document.pages().get(index.try_into()?)?;
            let (width, height) =
                clamped_bitmap_size(page.width().value, page.height().value, options.dpi)?;
            let image = {
                let bitmap = page.render_with_config(&render_config(width, height))?;
                let image = bitmap.as_image()?.into_rgb8();
                drop(bitmap);
                image
            };
            drop(page);
            on_page(index, image)?;
        }
        Ok(page_count)
    }

    pub fn render_document_segmented(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        segment_pages: usize,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if segment_pages == 0 {
            return Err("segment_pages must be greater than zero".into());
        }
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        let page_count: usize = pdfium
            .load_pdf_from_file(input, self.password.as_deref())?
            .pages()
            .len()
            .try_into()?;
        let mut start = 0;
        while start < page_count {
            let document = pdfium.load_pdf_from_file(input, self.password.as_deref())?;
            let end = (start + segment_pages).min(page_count);
            for index in start..end {
                let page = document.pages().get(index.try_into()?)?;
                let (width, height) =
                    clamped_bitmap_size(page.width().value, page.height().value, options.dpi)?;
                let image = {
                    let bitmap = page.render_with_config(&render_config(width, height))?;
                    let image = bitmap.as_image()?.into_rgb8();
                    drop(bitmap);
                    image
                };
                drop(page);
                on_page(index, image)?;
            }
            drop(document);
            start = end;
        }
        Ok(page_count)
    }

    pub fn render_range(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        start_page: usize,
        end_page: usize,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if start_page >= end_page {
            return Err("render range must be non-empty".into());
        }
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let total: usize = document.pages().len().try_into()?;
        let end_page = end_page.min(total);
        for index in start_page..end_page {
            let page = document.pages().get(index.try_into()?)?;
            let (width, height) =
                clamped_bitmap_size(page.width().value, page.height().value, options.dpi)?;
            let bitmap = page.render_with_config(&render_config(width, height))?;
            let image = bitmap.as_image()?.into_rgb8();
            drop(bitmap);
            drop(page);
            on_page(index, image)?;
        }
        Ok(end_page - start_page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage0_pdf_fixtures_are_present_and_hash_locked() {
        let fixtures = [
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/normal-1-page.pdf") as &[u8],
                "c248518d126bd5634b91d3d3238d68235c33a9690bd459ed9eed9ae0e9d835df",
            ),
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/varied-page-sizes.pdf") as &[u8],
                "7bdd586be8dd909ef0148b77a010f705d46794732e9ce9ad9730c01b4bc80929",
            ),
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/normal-100-pages.pdf") as &[u8],
                "2935510b108a68f1cde90a4db1471988b2d62af10ddfd366c79076c7ef5a2971",
            ),
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/complex-20-pages.pdf") as &[u8],
                "391b4f1a24e984c21a638f07af68019a50c62bc08a797eca21c64c3bac75a17d",
            ),
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/corrupt-truncated.pdf") as &[u8],
                "734f6fd295bc4b7d615749554fd3dbf0e7db012a0fbe159b17b7e654ad5003f8",
            ),
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/encrypted-password-required.pdf")
                    as &[u8],
                "46a0e3e6064691fb05636f61729920f6026639943477498e96ab58a0c0a13ffb",
            ),
            (
                include_bytes!("../../../benchmarks/fixtures/pdf/not-a-pdf.txt") as &[u8],
                "a34e5ab1e4947f849dd9ff5dc326f713dac2390064c4209fafae7da31d225654",
            ),
        ];
        for (fixture, expected) in fixtures {
            assert_eq!(ocrx_core::sha256(fixture), expected);
        }
    }

    #[test]
    fn stage0_real_pdfium_fixture_reports_text_and_geometry_when_library_is_available() {
        run_pdfium_probe(
            "tests::pdfium_probe_stage0_real_pdfium_fixture_reports_text_and_geometry",
        );
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_stage0_real_pdfium_fixture_reports_text_and_geometry() {
        let library = std::env::var_os("OCRX_PDFIUM_LIBRARY_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("OCRX_PDFIUM_LIBRARY").map(|path| {
                    let path = PathBuf::from(path);
                    if path.is_file() {
                        path.parent().unwrap_or(&path).to_owned()
                    } else {
                        path
                    }
                })
            })
            .unwrap_or_else(|| {
                PathBuf::from("/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib")
            });
        let library_binary = if cfg!(target_os = "macos") {
            library.join("libpdfium.dylib")
        } else if cfg!(target_os = "windows") {
            library.join("pdfium.dll")
        } else {
            library.join("libpdfium.so")
        };
        if !library_binary.is_file() {
            eprintln!("skipping PDFium object probe: {}", library.display());
            return;
        }
        let renderer = PdfiumRenderer::new(library);
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../benchmarks/fixtures/pdf/normal-1-page.pdf");
        let page = renderer
            .extract_native_page(fixture.clone(), 0)
            .expect("PDFium fixture should be readable");
        assert!(page.signals.visible_characters > 0);
        assert!(page.signals.page_width > 0.0);
        assert!(page.signals.page_height > 0.0);
        assert!(page.signals.text_bbox_coverage > 0.0);
        assert!(
            page.blocks
                .iter()
                .all(|block| block.bbox.right > block.bbox.left
                    && block.bbox.bottom > block.bbox.top)
        );
        assert!(page.signals.unicode_validity > 0.0);
        let signals = renderer
            .analyze_page_signals(fixture, 0)
            .expect("raw page signals should be available");
        assert_eq!(signals.visible_characters, page.signals.visible_characters);
    }

    #[test]
    fn default_render_is_300_dpi() {
        assert_eq!(RenderOptions::default().dpi, 300);
    }

    #[test]
    fn pdfium_errors_map_to_stable_classes() {
        // Synthetic user-facing messages.
        assert_eq!(
            classify_pdf_error("file is not a PDF document"),
            PdfErrorKind::NonPdf
        );
        assert_eq!(
            classify_pdf_error("malformed or corrupt cross-reference table"),
            PdfErrorKind::Corrupt
        );
        assert_eq!(
            classify_pdf_error("document is encrypted and requires a password"),
            PdfErrorKind::Encrypted
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(PasswordError)"),
            PdfErrorKind::Encrypted
        );
        assert_eq!(
            classify_pdf_error("failed to load dynamic library"),
            PdfErrorKind::DynamicLibrary
        );
        assert_eq!(
            classify_pdf_error("unexpected failure"),
            PdfErrorKind::Unknown
        );
        // Real pdfium-render error variants (Display prints {:#?}). The
        // PdfiumLibraryInternalError variants must NOT match the dynamic-library
        // class even though they contain the substring "library".
        assert_eq!(
            classify_pdf_error("LoadLibraryError(LoadLibraryExW { source: 126 })"),
            PdfErrorKind::DynamicLibrary
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(FormatError)"),
            PdfErrorKind::NonPdf
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(PasswordError)"),
            PdfErrorKind::Encrypted
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(SecurityError)"),
            PdfErrorKind::Encrypted
        );
    }

    #[test]
    fn pixel_budget_scales_down_without_upscaling() {
        assert_eq!(fit_pixel_budget(100, 100, 10_000), (100, 100, false));
        let (width, height, scaled) = fit_pixel_budget(2_000, 1_000, 1_000_000);
        assert!(scaled && u64::from(width) * u64::from(height) <= 1_000_000);
    }

    // --- W2.1/W0.2: PDFium library path resolution & loading guard ---------
    //
    // A crafted library_dir must never traverse out (via `..`) to load an
    // unintended DLL. The resolver is pure (builds a path only, never loads),
    // so the guard is testable in-process without touching the process-global
    // Pdfium binding.

    #[test]
    fn pdfium_library_path_rejects_parent_dir_traversal() {
        // A clean directory resolves to the platform library file beside it.
        let clean = pdfium_library_path(Path::new("/vendor/pdfium/bin")).unwrap();
        assert!(
            clean
                .file_name()
                .map(|name| name.to_string_lossy().contains("pdfium"))
                .unwrap_or(false),
            "resolved path should be the platform pdfium library, got {clean:?}"
        );
        // Parent-dir traversal anywhere in library_dir is rejected (path escape
        // must not silently succeed - W0.2).
        assert_eq!(
            pdfium_library_path(Path::new("/vendor/pdfium/bin/../../evil")),
            Err(PdfErrorKind::DynamicLibrary)
        );
        assert_eq!(
            pdfium_library_path(Path::new("../evil")),
            Err(PdfErrorKind::DynamicLibrary)
        );
        assert_eq!(
            pdfium_library_path(Path::new("/opt/../evil")),
            Err(PdfErrorKind::DynamicLibrary)
        );
    }

    #[test]
    fn shared_pdfium_rejects_parent_dir_in_file_form_library_path() {
        // The `..` guard must also apply when the library variable names the
        // library file itself, not only when it names a directory (MINOR 7).
        // The check runs before any binding, so no pdfium.dll is required.
        let renderer = PdfiumRenderer::new("../evil/libpdfium.dll");
        let error = renderer
            .render_page_rgb8("does-not-matter.pdf", 0, RenderOptions::default())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("parent-dir"),
            "expected traversal rejection, got: {error}"
        );
    }

    #[test]
    fn pre_raster_pixel_budget_clamps_before_bitmap_allocation() {
        // MINOR 17: the render entry points clamp the computed bitmap size to
        // MAX_RENDER_PIXELS via fit_pixel_budget before allocation, so a
        // hostile MediaBox cannot allocate an unbounded bitmap. Pin the math
        // the entry points rely on (a 200000 pt square at 300 DPI).
        let raw_width = (200_000.0_f32 * 300.0 / 72.0).round() as u32;
        let raw_height = raw_width;
        let (width, height, scaled) = super::fit_pixel_budget(raw_width, raw_height, 2880 * 2880);
        assert!(scaled);
        assert!(
            u64::from(width) * u64::from(height) <= 2880 * 2880,
            "clamped bitmap must fit the budget: {width}x{height}"
        );
        assert_eq!(
            super::fit_pixel_budget(1000, 1000, 2880 * 2880),
            (1000, 1000, false)
        );
    }

    #[test]
    fn huge_mediabox_fixture_is_hash_locked() {
        let data = include_bytes!("../../../benchmarks/fixtures/pdf/huge-mediabox-1-page.pdf");
        assert_eq!(
            ocrx_core::sha256(data),
            "080463987ae54d0719cede53517c23b5458345c5f5fbf69fe045831fcf01391b"
        );
    }

    #[test]
    fn real_huge_mediabox_renders_clamped_through_worker_path() {
        run_pdfium_probe("tests::pdfium_probe_huge_mediabox_renders_clamped");
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_huge_mediabox_renders_clamped() {
        // BLK-4 regression vector: `render_range` (the OCRX_PDF_WORKER path)
        // must clamp the bitmap to MAX_RENDER_PIXELS instead of handing a
        // 20000x20000 pt MediaBox at 300 DPI (~83k px per side) to PDFium.
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        let mut rendered_sizes = Vec::new();
        let count = renderer
            .render_range(
                fixture("huge-mediabox-1-page.pdf"),
                RenderOptions { dpi: 300 },
                0,
                1,
                |_index, image| {
                    rendered_sizes.push(image.dimensions());
                    Ok(())
                },
            )
            .expect("huge-mediabox page should render clamped");
        assert_eq!(count, 1);
        let (width, height) = rendered_sizes[0];
        assert!(
            u64::from(width) * u64::from(height) <= 2880 * 2880,
            "worker-path render must fit the pixel budget, got {width}x{height}"
        );
        assert!(width > 1 && height > 1, "clamped render is still real work");
    }

    #[test]
    fn geometry_anomaly_detects_overlapping_large_blocks_without_u16_overflow() {
        // M-9 regression: ~300-unit squares overflowed in the u16 multiply,
        // panicking debug builds and wrapping in release, so overlapping
        // cover-page blocks passed validation as successful native text.
        let block = |text: &str| NativeTextBlock {
            text: text.to_owned(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        // Two 500x500 (=250000 > u16::MAX) identical-geometry blocks overlap:
        // the anomaly detector must flag them, not panic or wrap.
        assert!(has_geometry_anomaly(&[block("a"), block("b")]));
        // Distinct far-apart blocks stay clean.
        let mut apart = block("a");
        apart.bbox = NormalizedBbox::new(0, 0, 100, 100).unwrap();
        let mut other = block("b");
        other.bbox = NormalizedBbox::new(500, 500, 600, 600).unwrap();
        assert!(!has_geometry_anomaly(&[apart, other]));
    }

    #[test]
    fn bbox_iou_and_png_output_are_deterministic() {
        let image = RgbImage::from_pixel(100, 100, image::Rgb([255, 0, 0]));
        let bbox = NormalizedBbox::new(100, 100, 500, 500).unwrap();
        assert!(bbox_iou(bbox, bbox) > 0.99);
        let root = std::env::temp_dir().join(format!("ocrx-png-{}", std::process::id()));
        let path = save_bbox_png(&image, bbox, &root, 2, 1).unwrap();
        assert!(path.ends_with("page-0002-figure-001.png"));
        assert!(path.is_file());
        assert!(!root.join("images/.page-0002-figure-001.part.png").exists());
        let _ = fs::remove_dir_all(root);
    }

    // --- W3.2: bbox/crop mapping cross-platform contract tests ---------------
    //
    // These pin the pixel-mapping contract: the near edge uses floor (so the
    // crop never starts inside the requested region) and the far edge uses ceil
    // (so the crop never loses the requested region), the result is clamped to
    // the image bounds (no out-of-bounds access), a valid bbox always yields a
    // non-empty crop (the area threshold), and an invalid bbox is rejected
    // before any file is written. Pure logic - runs identically on Windows.

    #[test]
    fn crop_bbox_uses_floor_for_near_edge_and_ceil_for_far_edge() {
        // 10x10 image; bbox (150,150,850,850) maps to:
        //   left  = floor(10*150/1000) = floor(1.5) = 1
        //   right = ceil (10*850/1000) = ceil (8.5) = 9
        // so the crop spans columns 1..9 (8 px) - proving floor on the near
        // edge and ceil on the far edge.
        let image = RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0]));
        let bbox = NormalizedBbox::new(150, 150, 850, 850).unwrap();
        let crop = crop_bbox(&image, bbox).unwrap();
        assert_eq!(
            crop.width(),
            8,
            "near edge should floor, far edge should ceil"
        );
        assert_eq!(crop.height(), 8);
    }

    #[test]
    fn crop_bbox_fits_within_image_bounds_at_the_far_edge_without_overflow() {
        // bbox reaching the far corner (999) on a 10x10 image maps right/bottom
        // to 10 (== width/height) and must not overflow or panic.
        let image = RgbImage::from_pixel(10, 10, image::Rgb([9, 9, 9]));
        let bbox = NormalizedBbox::new(0, 0, 999, 999).unwrap();
        let crop = crop_bbox(&image, bbox).unwrap();
        assert_eq!(crop.width(), 10);
        assert_eq!(crop.height(), 10);
        assert_eq!(crop.get_pixel(9, 9), &image::Rgb([9, 9, 9]));
    }

    #[test]
    fn crop_bbox_rejects_invalid_bbox_and_valid_bbox_is_always_nonempty() {
        // An invalid bbox (zero-width) is rejected before cropping.
        let image = RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0]));
        let invalid = NormalizedBbox {
            left: 50,
            top: 50,
            right: 50,
            bottom: 90,
        };
        assert!(crop_bbox(&image, invalid).is_err());
        // The area threshold: every valid bbox yields a non-empty crop, even on
        // a tiny image where the normalized region is sub-pixel-wide.
        let small = RgbImage::from_pixel(3, 3, image::Rgb([0, 0, 0]));
        for (left, top, right, bottom) in [
            (0, 0, 1, 1),
            (1, 1, 2, 2),
            (0, 0, 999, 999),
            (500, 500, 501, 501),
        ] {
            let bbox = NormalizedBbox::new(left, top, right, bottom).unwrap();
            let crop = crop_bbox(&small, bbox)
                .unwrap_or_else(|e| panic!("valid bbox {bbox:?} should crop: {e}"));
            assert!(
                crop.width() >= 1 && crop.height() >= 1,
                "empty crop for {bbox:?}"
            );
        }
    }

    #[test]
    fn save_bbox_png_rejects_invalid_bbox_without_writing_any_file() {
        let image = RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0]));
        let root = std::env::temp_dir().join(format!("ocrx-png-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let invalid = NormalizedBbox {
            left: 10,
            top: 10,
            right: 10,
            bottom: 20,
        };
        assert!(save_bbox_png(&image, invalid, &root, 1, 1).is_err());
        // No images directory or figure file is left behind for an invalid bbox.
        assert!(!root.join("images").exists());
        assert!(!root.exists() || fs::read_dir(&root).unwrap().count() == 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bbox_iou_is_zero_for_disjoint_one_for_identical_and_expected_for_overlap() {
        let disjoint_a = NormalizedBbox::new(0, 0, 100, 100).unwrap();
        let disjoint_b = NormalizedBbox::new(200, 200, 300, 300).unwrap();
        assert!(bbox_iou(disjoint_a, disjoint_b).abs() < 1e-6);
        let identical = NormalizedBbox::new(0, 0, 200, 200).unwrap();
        assert!((bbox_iou(identical, identical) - 1.0).abs() < 1e-6);
        // a=(0,0,200,200) area 40000, b=(100,100,300,300) area 40000,
        // intersection (100,100,200,200) area 10000 -> IoU 10000/70000.
        let partial = NormalizedBbox::new(100, 100, 300, 300).unwrap();
        assert!((bbox_iou(identical, partial) - 10_000.0_f32 / 70_000.0).abs() < 1e-6);
    }

    #[test]
    fn worker_segment_validation_rejects_missing_and_part_files() {
        let root = std::env::temp_dir().join(format!("ocrx-worker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(validate_worker_segment(&root, 0, 1).is_err());
        fs::write(root.join(".page-0001.part.png"), b"partial").unwrap();
        assert!(validate_worker_segment(&root, 0, 1).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn worker_segment_validation_rejects_extra_png() {
        let root = std::env::temp_dir().join(format!("ocrx-worker-extra-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let image = RgbImage::from_pixel(2, 2, image::Rgb([0, 0, 0]));
        image.save(root.join("page-0001.png")).unwrap();
        image.save(root.join("page-0002.png")).unwrap();
        assert!(validate_worker_segment(&root, 0, 1).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn worker_scheduler_rejects_missing_executable_and_invalid_range() {
        let scheduler = PdfWorkerScheduler {
            worker_executable: PathBuf::from("/missing/pdfium-worker"),
            library_dir: PathBuf::from("/missing"),
            segment_pages: 20,
            timeout: Duration::from_secs(1),
            dpi: 300,
            env: Vec::new(),
            password: None,
        };
        assert!(
            scheduler
                .run("input.pdf", std::env::temp_dir(), 0, 1)
                .is_err()
        );
        let invalid = PdfWorkerScheduler {
            segment_pages: 0,
            ..scheduler
        };
        assert!(
            invalid
                .run("input.pdf", std::env::temp_dir(), 0, 1)
                .is_err()
        );
    }

    // The portable scheduler-isolation tests live in `tests/worker_scheduler.rs`
    // as integration tests: only integration tests get the `CARGO_BIN_EXE_*`
    // env at compile time, which `cargo test` resolves by building the actual
    // `pdfium-fake-worker` bin artifact (lib unit tests only build the bin's
    // test harness, never the bin itself). See `tests/worker_scheduler.rs`.

    // --- W2.1: real-PDFium component tests ------------------------------------
    //
    // These exercise the frozen pdfium runtime (pdfium-render 0.9.3 +
    // `pdfium_7881` binding the chromium/7961 native library) against the
    // committed PDF fixtures. They skip when the native library is absent
    // (CI without the runtime artifact) so `cargo test` stays green; on a
    // machine with the library present they verify the Windows loading path,
    // 300 DPI RGB8 rendering, audit sizes, page selection, pixel-budget
    // scaling, and real error classification. See
    // docs/develop/pdfium-windows-release-baseline-2026-08-11.md.

    /// Directory holding the co-located `pdfium.dll`/`libpdfium.dylib`, or
    /// `None` when the runtime artifact is not present. The path is canonicalized
    /// so it has no `..` components: the `pdfium_library_path` guard rejects
    /// parent-dir traversal (W0.2), and the application is expected to pass a
    /// canonical `library_dir` (the production default is `.`).
    fn pdfium_lib_dir() -> Option<PathBuf> {
        let dir = std::env::var_os("OCRX_PDFIUM_LIB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/bin")
            });
        if !Pdfium::pdfium_platform_library_name_at_path(&dir).is_file() {
            return None;
        }
        std::fs::canonicalize(&dir).ok()
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../benchmarks/fixtures/pdf")
            .join(name)
    }

    // pdfium-render's `thread_safe` feature only marks `Pdfium` as Send/Sync,
    // and the native library keeps process-global state. Running several
    // in-process PDFium workloads back to back in one process has been
    // observed to corrupt the heap on Windows (flaky segfaults /
    // STATUS_HEAP_CORRUPTION under parallel harness threads), while each
    // workload in a fresh process is empirically stable. The real-PDF
    // component tests therefore delegate to `#[ignore]`d probe tests executed
    // in a fresh subprocess each — the same process isolation the production
    // `pdfium-worker` architecture applies (W2.2).
    fn run_pdfium_probe(probe: &str) {
        let exe = std::env::current_exe().expect("test binary path should resolve");
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                probe,
                "--ignored",
                "--test-threads=1",
                "--nocapture",
            ])
            .output()
            .expect("test subprocess should start");
        // libtest exits 0 even when the exact filter matches zero tests (e.g.
        // after a probe rename), so success alone proves nothing: require the
        // subprocess's own `test result:` summary to show one passing test in
        // an ok run (M6).
        let stdout = String::from_utf8_lossy(&output.stdout);
        let summary = stdout
            .lines()
            .rev()
            .find(|line| line.starts_with("test result:"))
            .unwrap_or_default();
        assert!(
            summary.contains("ok.") && summary.contains("1 passed"),
            "pdfium probe {probe} did not run cleanly; subprocess summary: `{summary}` (stdout: `{}`)",
            stdout.trim()
        );
    }

    fn ensure_pdfium() -> Option<PathBuf> {
        let dir = pdfium_lib_dir();
        if dir.is_none() {
            eprintln!(
                "note: pdfium runtime not present at vendor/pdfium/bin; \
                 skipping real-PDF component test"
            );
        }
        dir
    }

    #[test]
    fn real_pdf_page_counts_match_fixtures() {
        run_pdfium_probe("tests::pdfium_probe_page_counts_match_fixtures");
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_page_counts_match_fixtures() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        assert_eq!(
            renderer.page_count(fixture("normal-1-page.pdf")).unwrap(),
            1
        );
        assert_eq!(
            renderer
                .page_count(fixture("complex-20-pages.pdf"))
                .unwrap(),
            20
        );
        assert_eq!(
            renderer
                .page_count(fixture("normal-100-pages.pdf"))
                .unwrap(),
            100
        );
    }

    #[test]
    fn real_pdf_renders_rgb8_at_300_dpi_with_audit_sizes() {
        run_pdfium_probe("tests::pdfium_probe_renders_rgb8_at_300_dpi_with_audit_sizes");
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_renders_rgb8_at_300_dpi_with_audit_sizes() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        let (image, original_width, original_height) = renderer
            .render_page_rgb8_with_audit(fixture("normal-1-page.pdf"), 0, RenderOptions::default())
            .unwrap();
        // RGB8: exactly 3 bytes per pixel.
        assert_eq!(
            image.as_raw().len(),
            usize::try_from(image.width()).unwrap() * usize::try_from(image.height()).unwrap() * 3
        );
        // 300 DPI render = source 72-DPI points * 300/72 (within rounding).
        let dpi = f32::from(RenderOptions::default().dpi);
        assert_eq!(
            image.width(),
            (original_width as f32 * dpi / 72.0).round() as u32
        );
        assert_eq!(
            image.height(),
            (original_height as f32 * dpi / 72.0).round() as u32
        );
    }

    #[test]
    fn real_pdf_page_selection_maps_to_the_requested_index() {
        run_pdfium_probe("tests::pdfium_probe_page_selection_maps_to_the_requested_index");
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_page_selection_maps_to_the_requested_index() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        // Page 5 (0-indexed 4) renders and differs from page 1, proving the
        // selection index maps to the right page rather than always page 0.
        let first = renderer
            .render_page_rgb8(fixture("complex-20-pages.pdf"), 0, RenderOptions::default())
            .unwrap();
        let fifth = renderer
            .render_page_rgb8(fixture("complex-20-pages.pdf"), 4, RenderOptions::default())
            .unwrap();
        assert_ne!(first.as_raw(), fifth.as_raw());
    }

    #[test]
    fn real_render_pixel_budget_scales_proportionally_without_cropping() {
        run_pdfium_probe("tests::pdfium_probe_render_pixel_budget_scales_proportionally");
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_render_pixel_budget_scales_proportionally() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        let (image, _, _) = renderer
            .render_page_rgb8_with_audit(fixture("normal-1-page.pdf"), 0, RenderOptions::default())
            .unwrap();
        // Force a downscale below the rendered pixel count; the budget must
        // shrink both dimensions proportionally (no cropping) and stay within
        // budget, never upscaling.
        let pixels = u64::from(image.width()) * u64::from(image.height());
        let budget = pixels / 2;
        let (width, height, scaled) = fit_pixel_budget(image.width(), image.height(), budget);
        assert!(scaled);
        assert!(u64::from(width) * u64::from(height) <= budget);
        assert!(width <= image.width() && height <= image.height());
        // Aspect ratio preserved within floor rounding tolerance.
        let original_ratio = f64::from(image.width()) / f64::from(image.height());
        let scaled_ratio = f64::from(width) / f64::from(height);
        assert!((scaled_ratio - original_ratio).abs() < 0.05);
    }

    #[test]
    fn real_pdfium_errors_classify_to_stable_content_categories() {
        run_pdfium_probe("tests::pdfium_probe_errors_classify_to_stable_content_categories");
    }

    #[ignore = "probe runs in an isolated subprocess via run_pdfium_probe"]
    #[test]
    fn pdfium_probe_errors_classify_to_stable_content_categories() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        // A non-PDF file must classify as NonPdf (the library loaded fine, so
        // this is a content error, not a DynamicLibrary error).
        let non_pdf = std::env::temp_dir().join(format!("ocrx-notpdf-{}.txt", std::process::id()));
        fs::write(&non_pdf, "this is not a pdf document").unwrap();
        let error = renderer.page_count(&non_pdf).unwrap_err();
        assert_eq!(classify_pdf_error(&error.to_string()), PdfErrorKind::NonPdf);
        let _ = fs::remove_file(&non_pdf);

        // A truncated/corrupt fixture must classify as a content error
        // (NonPdf or Corrupt), never as a DynamicLibrary failure.
        let error = renderer
            .page_count(fixture("corrupt-truncated.pdf"))
            .unwrap_err();
        let kind = classify_pdf_error(&error.to_string());
        assert!(
            matches!(kind, PdfErrorKind::NonPdf | PdfErrorKind::Corrupt),
            "expected content error for corrupt fixture, got {kind:?}"
        );
    }

    #[test]
    fn classifier_routes_deterministically_at_p0_boundaries() {
        let text = classify_page(
            PageSignals {
                visible_characters: 20,
                text_coverage: 0.20,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(text.page_type, PageType::Text);
        assert_eq!(text.recommended_method, ProcessingMethod::NativeText);
        let scan = classify_page(
            PageSignals {
                image_coverage: 0.55,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(scan.recommended_method, ProcessingMethod::Ocr);
        let mixed = classify_page(
            PageSignals {
                visible_characters: 20,
                text_coverage: 0.3,
                image_coverage: 0.1,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(mixed.page_type, PageType::Mixed);
        let empty = classify_page(PageSignals::default(), QualityTier::L2);
        assert_eq!(empty.recommended_method, ProcessingMethod::Empty);
    }

    #[test]
    fn batch_classifier_preserves_page_order_and_empty_inputs() {
        assert!(classify_pages(&[], QualityTier::L2).is_empty());
        let classifications = classify_pages(
            &[
                PageSignals {
                    visible_characters: 8,
                    text_coverage: 0.3,
                    ..Default::default()
                },
                PageSignals {
                    image_coverage: 0.7,
                    ..Default::default()
                },
                PageSignals::default(),
            ],
            QualityTier::L2,
        );
        assert_eq!(classifications.len(), 3);
        assert_eq!(classifications[0].page_type, PageType::Text);
        assert_eq!(classifications[1].page_type, PageType::Image);
        assert_eq!(classifications[2].page_type, PageType::Empty);
    }

    #[test]
    fn batch_classifier_applies_one_versioned_config_to_every_page() {
        let config = ClassifierConfig {
            text_coverage_threshold: 0.1,
            ..ClassifierConfig::default()
        };
        let classifications = classify_pages_with_config(
            &[
                PageSignals {
                    visible_characters: 2,
                    text_coverage: 0.1,
                    ..Default::default()
                },
                PageSignals {
                    visible_characters: 3,
                    text_coverage: 0.1,
                    ..Default::default()
                },
            ],
            QualityTier::L2,
            config,
        );
        assert_eq!(classifications.len(), 2);
        assert!(
            classifications
                .iter()
                .all(|classification| classification.page_type == PageType::Text)
        );
    }

    #[test]
    fn document_summary_is_stable_and_conservative() {
        let page = |page_type| PageClassification {
            page_type,
            confidence: RouteConfidence::High,
            recommended_method: match page_type {
                PageType::Text => ProcessingMethod::NativeText,
                PageType::Image | PageType::Mixed | PageType::Uncertain => ProcessingMethod::Ocr,
                PageType::Empty => ProcessingMethod::Empty,
            },
            reason: PdfReasonCode::NoVisibleContent,
        };
        assert_eq!(summarize_document_type(&[]), DocumentType::Empty);
        assert_eq!(
            summarize_document_type(&[page(PageType::Text), page(PageType::Image)]),
            DocumentType::Mixed
        );
        assert_eq!(
            summarize_document_type(&[page(PageType::Image), page(PageType::Empty)]),
            DocumentType::Image
        );
        assert_eq!(
            summarize_document_type(&[page(PageType::Text), page(PageType::Uncertain)]),
            DocumentType::Uncertain
        );
    }

    #[test]
    fn native_validation_allows_l1_but_rejects_insufficient_l2_structure() {
        let blocks = [NativeTextBlock {
            text: "忠实文字".to_owned(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        }];
        let analysis = PageSignals {
            visible_characters: 4,
            text_coverage: 0.10,
            ..Default::default()
        };
        assert!(validate_native_page(QualityTier::L1, analysis, &blocks).is_ok());
        assert_eq!(
            validate_native_page(QualityTier::L2, analysis, &blocks),
            Err(PdfReasonCode::NativeStructureUnresolved)
        );
    }

    #[test]
    fn native_validation_rejects_l2_text_with_insufficient_bbox_coverage() {
        let blocks = [NativeTextBlock {
            text: "窄文字".to_owned(),
            bbox: NormalizedBbox::new(0, 0, 40, 40).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        }];
        let analysis = PageSignals {
            visible_characters: 3,
            text_coverage: 1.0,
            text_bbox_coverage: 0.000_016,
            ..Default::default()
        };
        assert!(validate_native_page(QualityTier::L1, analysis, &blocks).is_ok());
        assert_eq!(
            validate_native_page(QualityTier::L2, analysis, &blocks),
            Err(PdfReasonCode::NativeStructureUnresolved)
        );
    }

    #[test]
    fn native_validation_rejects_exact_duplicate_text_geometry() {
        let bbox = NormalizedBbox::new(0, 0, 500, 500).unwrap();
        let blocks = [
            NativeTextBlock {
                text: "重复".to_owned(),
                bbox,
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "重复".to_owned(),
                bbox,
                order: 2,
                source: ocrx_core::BlockSource::PdfText,
            },
        ];
        let result = validate_native_page(
            QualityTier::L1,
            PageSignals {
                visible_characters: 2,
                text_coverage: 1.0,
                ..Default::default()
            },
            &blocks,
        );
        assert_eq!(result, Err(PdfReasonCode::NativeValidationFailed));
    }

    #[test]
    fn native_validation_rejects_non_pdf_text_block_sources() {
        let blocks = [NativeTextBlock {
            text: "错误来源".to_owned(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::Ocr,
        }];
        let result = validate_native_page(
            QualityTier::L1,
            PageSignals {
                visible_characters: 4,
                text_coverage: 1.0,
                ..Default::default()
            },
            &blocks,
        );
        assert_eq!(result, Err(PdfReasonCode::NativeValidationFailed));
    }

    #[test]
    fn native_validation_rejects_character_count_mismatch() {
        let blocks = [NativeTextBlock {
            text: "少一个".to_owned(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        }];
        let result = validate_native_page(
            QualityTier::L1,
            PageSignals {
                visible_characters: 4,
                text_coverage: 1.0,
                ..Default::default()
            },
            &blocks,
        );
        assert_eq!(result, Err(PdfReasonCode::NativeValidationFailed));
    }

    #[test]
    fn native_validation_rejects_zero_source_order() {
        let blocks = [NativeTextBlock {
            text: "顺序".to_owned(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 0,
            source: ocrx_core::BlockSource::PdfText,
        }];
        let result = validate_native_page(
            QualityTier::L1,
            PageSignals {
                visible_characters: 2,
                text_coverage: 1.0,
                ..Default::default()
            },
            &blocks,
        );
        assert_eq!(result, Err(PdfReasonCode::NativeValidationFailed));
    }

    #[test]
    fn native_validation_rejects_non_finite_analysis_signals() {
        let block = NativeTextBlock {
            text: "稳定文字".to_owned(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        let result = validate_native_page(
            QualityTier::L1,
            PageSignals {
                visible_characters: 4,
                text_coverage: f32::INFINITY,
                ..Default::default()
            },
            &[block],
        );
        assert_eq!(result, Err(PdfReasonCode::NativeValidationFailed));
    }

    #[test]
    fn page_plan_obeys_explicit_mode_and_preserves_empty_pages() {
        let text = classify_page(
            PageSignals {
                visible_characters: 20,
                text_coverage: 0.2,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(
            plan_page(ocrx_core::PdfMode::Auto, text),
            ProcessingMethod::NativeText
        );
        assert_eq!(
            plan_page(ocrx_core::PdfMode::Ocr, text),
            ProcessingMethod::Ocr
        );
        let image = classify_page(
            PageSignals {
                image_coverage: 0.8,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(
            plan_page(ocrx_core::PdfMode::Text, image),
            ProcessingMethod::NativeText
        );
        assert_eq!(
            plan_page_checked(ocrx_core::PdfMode::Text, image),
            Err(PdfReasonCode::NativeValidationFailed)
        );
        assert_eq!(
            plan_page_checked(ocrx_core::PdfMode::Ocr, image),
            Ok(ProcessingMethod::Ocr)
        );
        let empty = classify_page(PageSignals::default(), QualityTier::L2);
        assert_eq!(
            plan_page(ocrx_core::PdfMode::Ocr, empty),
            ProcessingMethod::Empty
        );
    }

    #[test]
    fn checked_page_plan_matrix_keeps_empty_and_routes_uncertain_pages_conservatively() {
        let empty = classify_page(PageSignals::default(), QualityTier::L2);
        assert_eq!(
            plan_page_checked(ocrx_core::PdfMode::Text, empty),
            Ok(ProcessingMethod::Empty)
        );
        let uncertain = classify_page(
            PageSignals {
                visible_characters: 4,
                text_coverage: 0.4,
                unicode_validity: 0.5,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(uncertain.page_type, PageType::Uncertain);
        assert_eq!(
            plan_page_checked(ocrx_core::PdfMode::Auto, uncertain),
            Ok(ProcessingMethod::Ocr)
        );
        assert_eq!(
            plan_page_checked(ocrx_core::PdfMode::Text, uncertain),
            Err(PdfReasonCode::NativeValidationFailed)
        );
    }

    #[test]
    fn native_page_maps_to_existing_page_document_json_and_markdown_shape() {
        let page = NativePage {
            text: "第一行\n第二行".to_owned(),
            blocks: Vec::new(),
            signals: PageSignals {
                visible_characters: 6,
                text_coverage: 1.0,
                text_bbox_coverage: 0.01,
                ..Default::default()
            },
        };
        let document = native_page_document(2, &page).unwrap();
        let json = document.to_json().unwrap();
        assert!(json.contains("第一行\\n第二行"));
        assert_eq!(document.page_number, 2);
    }

    #[test]
    fn native_pdf_page_document_carries_classification_and_block_provenance() {
        let page = NativePage {
            text: "native".to_owned(),
            blocks: vec![NativeTextBlock {
                text: "native".to_owned(),
                bbox: NormalizedBbox::new(10, 20, 100, 120).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            }],
            signals: PageSignals {
                visible_characters: 6,
                text_coverage: 1.0,
                text_bbox_coverage: 0.01,
                ..Default::default()
            },
        };
        let classification = classify_page(page.signals, QualityTier::L2);
        let document = native_pdf_page_document(2, &page, classification).unwrap();
        let json = document.to_json().unwrap();

        assert!(json.contains("\"schema_version\":2"));
        assert!(json.contains("\"source\":\"pdf_text\""));
        assert!(json.contains("\"source_order\":1"));
        assert!(document.validate().is_ok());
    }

    #[test]
    fn native_pdf_page_artifacts_emit_routed_schema_v2_json() {
        let page = NativePage {
            text: "native".to_owned(),
            blocks: vec![NativeTextBlock {
                text: "native".to_owned(),
                bbox: NormalizedBbox::new(10, 20, 100, 120).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            }],
            signals: PageSignals {
                visible_characters: 6,
                text_coverage: 1.0,
                text_bbox_coverage: 0.01,
                ..Default::default()
            },
        };
        let classification = classify_page(page.signals, QualityTier::L2);
        assert_eq!(
            validate_native_page(QualityTier::L2, page.signals, &page.blocks),
            Ok(())
        );
        let (markdown, json) =
            native_pdf_page_artifacts(2, &page, classification, QualityTier::L2).unwrap();

        assert_eq!(markdown, "native");
        assert!(json.contains("\"schema_version\":2"));
        assert!(json.contains("\"actual_method\":\"native_text\""));
        assert!(json.contains("\"source\":\"pdf_text\""));
    }

    #[test]
    fn classifier_config_freezes_version_and_controls_only_versioned_thresholds() {
        let config = ClassifierConfig::default();
        assert_eq!(config.version, "pdf-route-v1");
        let relaxed = ClassifierConfig {
            text_coverage_threshold: 0.10,
            ..config
        };
        let result = classify_page_with_config(
            PageSignals {
                visible_characters: 2,
                text_coverage: 0.10,
                ..Default::default()
            },
            relaxed,
        );
        assert_eq!(result.page_type, PageType::Text);
    }

    #[test]
    fn classifier_uses_conservative_reason_precedence_for_unreliable_text() {
        let classification = classify_page(
            PageSignals {
                visible_characters: 10,
                text_coverage: 0.8,
                unicode_validity: 0.5,
                duplicate_text: true,
                geometry_anomaly: true,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(classification.page_type, PageType::Uncertain);
        assert_eq!(classification.confidence, RouteConfidence::Low);
        assert_eq!(classification.recommended_method, ProcessingMethod::Ocr);
        assert_eq!(classification.reason, PdfReasonCode::DuplicateText);
    }

    #[test]
    fn classifier_routes_non_finite_page_signals_to_low_confidence_ocr() {
        let classification = classify_page(
            PageSignals {
                text_coverage: f32::NAN,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(classification.page_type, PageType::Uncertain);
        assert_eq!(classification.confidence, RouteConfidence::Low);
        assert_eq!(classification.recommended_method, ProcessingMethod::Ocr);
        assert_eq!(classification.reason, PdfReasonCode::GeometryAnomaly);
    }

    #[test]
    fn classifier_routes_broken_cmap_punctuation_text_to_ocr() {
        // A broken ToUnicode CMap maps every glyph to arbitrary punctuation
        // and stray Latin-1 code points: unicode validity stays 1.0 while the
        // alphanumeric ratio lands well under real prose (measured separation
        // on the W0.4 composite: garbage <=0.55, real text ~0.84).
        let classification = classify_page(
            PageSignals {
                visible_characters: 120,
                unicode_validity: 1.0,
                alphanumeric_ratio: 0.37,
                text_coverage: 0.8,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(classification.page_type, PageType::Uncertain);
        assert_eq!(classification.confidence, RouteConfidence::Low);
        assert_eq!(classification.recommended_method, ProcessingMethod::Ocr);
        assert_eq!(classification.reason, PdfReasonCode::UnicodeValidityLow);
    }

    #[test]
    fn classifier_keeps_symbol_light_but_legible_text_on_native_path() {
        // Pure-CJK prose is alphanumeric under Unicode (ideographs are
        // letters), so ordinary Chinese text must keep routing to native.
        let classification = classify_page(
            PageSignals {
                visible_characters: 500,
                unicode_validity: 1.0,
                alphanumeric_ratio: 0.84,
                text_coverage: 0.8,
                ..Default::default()
            },
            QualityTier::L2,
        );
        assert_eq!(classification.page_type, PageType::Text);
        assert_eq!(classification.confidence, RouteConfidence::High);
        assert_eq!(
            classification.recommended_method,
            ProcessingMethod::NativeText
        );
        assert_eq!(classification.reason, PdfReasonCode::NativeTextReliable);
    }

    #[test]
    fn native_validation_rejects_punctuation_only_text_layers() {
        let signals = PageSignals {
            page_width: 541.0,
            page_height: 754.0,
            visible_characters: 100,
            unicode_validity: 1.0,
            alphanumeric_ratio: 0.48,
            text_coverage: 1.0,
            ..Default::default()
        };
        let blocks = vec![NativeTextBlock {
            text: "!\"#$%&'()".to_owned(),
            bbox: NormalizedBbox::new(10, 10, 900, 900).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        }];
        assert_eq!(
            validate_native_page(QualityTier::L2, signals, &blocks),
            Err(PdfReasonCode::NativeValidationFailed)
        );
    }

    #[test]
    fn native_candidate_validation_is_tier_aware_and_preserves_page_number() {
        let page = NativePage {
            text: "忠实文字".to_owned(),
            blocks: vec![NativeTextBlock {
                text: "忠实文字".to_owned(),
                bbox: NormalizedBbox::new(10, 10, 500, 500).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            }],
            signals: PageSignals {
                visible_characters: 4,
                text_coverage: 0.1,
                ..Default::default()
            },
        };
        assert_eq!(
            validate_native_candidate(7, &page, QualityTier::L1)
                .unwrap()
                .page_number,
            7
        );
        assert_eq!(
            validate_native_candidate(7, &page, QualityTier::L2),
            Err(PdfReasonCode::NativeStructureUnresolved)
        );
    }

    #[test]
    fn native_page_markdown_is_deterministic_and_excludes_non_markdown_assets() {
        let page = NativePage {
            text: "标题\n正文".to_owned(),
            blocks: Vec::new(),
            signals: PageSignals::default(),
        };
        let document = native_page_document(1, &page).unwrap();
        assert_eq!(native_page_markdown(&document), "标题\n正文");
    }

    #[test]
    fn native_text_blocks_coalesce_same_line_and_preserve_reading_order() {
        let blocks = vec![
            NativeTextBlock {
                text: "一".into(),
                bbox: NormalizedBbox::new(10, 100, 20, 140).unwrap(),
                order: 9,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "行".into(),
                bbox: NormalizedBbox::new(21, 102, 31, 142).unwrap(),
                order: 10,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "二".into(),
                bbox: NormalizedBbox::new(10, 300, 20, 340).unwrap(),
                order: 11,
                source: ocrx_core::BlockSource::PdfText,
            },
        ];
        let result = coalesce_native_text_blocks(&blocks);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "一行");
        assert_eq!(result[0].order, 1);
        assert_eq!(result[1].text, "二");
        assert_eq!(result[1].order, 2);
    }

    #[test]
    fn native_text_blocks_do_not_merge_distant_same_line_columns() {
        let blocks = vec![
            NativeTextBlock {
                text: "左栏".into(),
                bbox: NormalizedBbox::new(10, 100, 100, 140).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "右栏".into(),
                bbox: NormalizedBbox::new(600, 102, 700, 142).unwrap(),
                order: 2,
                source: ocrx_core::BlockSource::PdfText,
            },
        ];
        let result = coalesce_native_text_blocks(&blocks);
        assert_eq!(
            result
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["左栏", "右栏"]
        );
    }

    #[test]
    fn native_text_blocks_treat_newlines_as_hard_line_breaks() {
        let blocks = vec![
            NativeTextBlock {
                text: "第一行\n".into(),
                bbox: NormalizedBbox::new(10, 100, 100, 140).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "第二行".into(),
                bbox: NormalizedBbox::new(10, 102, 100, 142).unwrap(),
                order: 2,
                source: ocrx_core::BlockSource::PdfText,
            },
        ];
        assert_eq!(coalesce_native_text_blocks(&blocks).len(), 2);
    }

    #[test]
    fn duplicate_text_geometry_is_detected_deterministically() {
        let bbox = NormalizedBbox::new(10, 10, 100, 100).unwrap();
        let blocks = vec![
            NativeTextBlock {
                text: "重复".into(),
                bbox,
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "重复".into(),
                bbox,
                order: 2,
                source: ocrx_core::BlockSource::PdfText,
            },
        ];
        assert!(has_duplicate_text_geometry(&blocks));
    }

    #[test]
    fn high_overlap_with_different_text_is_a_geometry_anomaly() {
        let blocks = vec![
            NativeTextBlock {
                text: "甲".into(),
                bbox: NormalizedBbox::new(10, 10, 100, 100).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            },
            NativeTextBlock {
                text: "乙".into(),
                bbox: NormalizedBbox::new(15, 15, 95, 95).unwrap(),
                order: 2,
                source: ocrx_core::BlockSource::PdfText,
            },
        ];
        assert!(has_geometry_anomaly(&blocks));
    }

    #[test]
    fn quality_manifest_rejects_incomplete_overlap_and_wrong_membership() {
        let entry = QualityManifestEntry {
            source_sha256: "a".into(),
            page: 1,
            expected_type: PageType::Text,
            native_allowed: true,
            benchmark_version: "v1".into(),
            truth_version: "truth-v1".into(),
            expected_text: "正文".into(),
            expected_block_count: 1,
            expected_bbox_count: 1,
        };
        assert!(validate_quality_manifest(std::slice::from_ref(&entry), &[]).is_ok());
        let duplicate = entry.clone();
        let wrong_membership = entry.clone();
        assert_eq!(
            validate_quality_manifest(&[entry], &[duplicate]),
            Err("manifest page appears more than once")
        );
        assert_eq!(
            validate_quality_manifest(
                &[],
                &[QualityManifestEntry {
                    native_allowed: true,
                    ..wrong_membership.clone()
                }]
            ),
            Err("forbidden set contains a native-eligible page")
        );
        assert_eq!(
            validate_quality_manifest(
                &[QualityManifestEntry {
                    truth_version: String::new(),
                    ..wrong_membership
                }],
                &[]
            ),
            Err("manifest entry is incomplete")
        );
    }

    #[test]
    fn quality_manifest_loader_parses_and_validates_tsv() {
        let path = std::env::temp_dir().join(format!("ocrx-manifest-{}.tsv", std::process::id()));
        std::fs::write(&path, "set\tsource_sha256\tpage\texpected_type\tnative_allowed\tbenchmark_version\ttruth_version\texpected_text\tblock_count\tbbox_count\nnative\t0000000000000000000000000000000000000000000000000000000000000000\t1\ttext\ttrue\tv1\ttruth-v1\t正文\t1\t1\nforbidden\t0000000000000000000000000000000000000000000000000000000000000000\t2\timage\tfalse\tv1\ttruth-v1\t\t0\t0\n").unwrap();
        let (native, forbidden) = super::load_quality_manifest(&path).unwrap();
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].expected_text, "正文");
        assert_eq!(forbidden.len(), 1);
        assert!(!forbidden[0].native_allowed);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn quality_manifest_loader_rejects_manifest_for_another_input() {
        let root = std::env::temp_dir().join(format!("ocrx-manifest-hash-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let input = root.join("input.pdf");
        let manifest = root.join("manifest.tsv");
        std::fs::write(&input, b"fixture").unwrap();
        std::fs::write(
            &manifest,
            "native\t1111111111111111111111111111111111111111111111111111111111111111\t1\ttext\ttrue\tv1\ttruth-v1\t正文\t1\t1\n",
        )
        .unwrap();
        assert_eq!(
            super::load_quality_manifest_for_input(&manifest, &input)
                .unwrap_err()
                .to_string(),
            "manifest source hash does not match input"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quality_manifest_loader_rejects_malformed_sha256() {
        let path =
            std::env::temp_dir().join(format!("ocrx-manifest-sha-{}.tsv", std::process::id()));
        std::fs::write(
            &path,
            "native\ttoo-short\t1\ttext\ttrue\tv1\ttruth-v1\t正文\t1\t1\n",
        )
        .unwrap();
        let error = super::load_quality_manifest(&path).unwrap_err();
        assert!(error.to_string().contains("invalid source_sha256"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn quality_outcome_summary_counts_blank_duplicate_and_failed_pages() {
        assert_eq!(
            summarize_quality_outcomes(&[]),
            QualityOutcomeSummary::default()
        );
        let summary = summarize_quality_outcomes(&[
            QualityPageOutcome {
                is_blank: true,
                has_duplicate_text: false,
                failed: false,
            },
            QualityPageOutcome {
                is_blank: false,
                has_duplicate_text: true,
                failed: true,
            },
            QualityPageOutcome {
                is_blank: false,
                has_duplicate_text: false,
                failed: false,
            },
        ]);
        assert_eq!(
            summary,
            QualityOutcomeSummary {
                blank_pages: 1,
                duplicate_pages: 1,
                failed_pages: 1,
            }
        );
    }

    #[test]
    fn release_manifest_requires_frozen_quality_set_sizes() {
        let entry = QualityManifestEntry {
            source_sha256: "a".into(),
            page: 1,
            expected_type: PageType::Text,
            native_allowed: true,
            benchmark_version: "v1".into(),
            truth_version: "truth-v1".into(),
            expected_text: "正文".into(),
            expected_block_count: 1,
            expected_bbox_count: 1,
        };
        assert_eq!(
            validate_release_manifest(std::slice::from_ref(&entry), &[]),
            Err("native quality set must contain at least 30 pages")
        );
    }

    #[test]
    fn release_report_json_keeps_gate_failures_and_inputs() {
        let report = super::generate_release_report(
            NativeReleaseMetrics {
                total_pages: 1,
                native_pages: 0,
                fallback_pages: 1,
                ocr_pages: 0,
                failed_pages: 0,
                text_ned: 0.9,
                ocr_baseline_text_ned: 0.9,
                reading_order_accuracy: 1.0,
                ocr_baseline_reading_order_accuracy: 1.0,
                block_precision: 1.0,
                block_recall: 1.0,
                block_f1: 1.0,
                bbox_validity_rate: 1.0,
                bbox_recall: 1.0,
                duplicate_pages: 0,
                blank_pages: 0,
            },
            30,
            20,
            "not_run",
        );
        let json = report.to_json();
        assert!(json.contains("ocrx-release-report-v1"));
        assert!(json.contains("native route coverage is below 100%"));
        assert!(json.contains("\"native_manifest_pages\":30"));
        assert!(json.contains("\"performance_status\":\"not_run\""));
    }

    #[test]
    fn normalized_edit_distance_is_unicode_safe_and_bounded() {
        assert!(normalized_edit_distance("", "").abs() < f32::EPSILON);
        assert!(normalized_edit_distance("abc", "abc").abs() < f32::EPSILON);
        assert!((normalized_edit_distance("", "字") - 1.0).abs() < f32::EPSILON);
        assert!((normalized_edit_distance("甲乙", "甲丙") - 0.5).abs() < f32::EPSILON);
        let distance = normalized_edit_distance("abc", "xyz");
        assert!((0.0..=1.0).contains(&distance));
        assert!((normalized_edit_distance("abc", "xyz") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn block_scores_match_text_and_bbox_once() {
        let truth = NativeTextBlock {
            text: "段落".into(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        let actual = NativeTextBlock {
            bbox: NormalizedBbox::new(10, 10, 490, 490).unwrap(),
            ..truth.clone()
        };
        let scores = block_precision_recall_f1(&[truth], &[actual]);
        assert!((scores.0 - 1.0).abs() < f32::EPSILON);
        assert!((scores.1 - 1.0).abs() < f32::EPSILON);
        assert!((scores.2 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn bbox_validity_rate_covers_empty_valid_and_invalid_sets() {
        assert!((bbox_validity_rate(&[]) - 1.0).abs() < f32::EPSILON);
        let valid = NativeTextBlock {
            text: "有效".into(),
            bbox: NormalizedBbox::new(0, 0, 500, 500).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        assert!((bbox_validity_rate(std::slice::from_ref(&valid)) - 1.0).abs() < f32::EPSILON);
        let invalid = NativeTextBlock {
            bbox: NormalizedBbox {
                left: 500,
                top: 0,
                right: 500,
                bottom: 500,
            },
            ..valid.clone()
        };
        assert!((bbox_validity_rate(&[valid, invalid]) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn bbox_recall_is_geometry_only_and_handles_empty_truth() {
        assert!((bbox_recall(&[], &[]) - 1.0).abs() < f32::EPSILON);
        let first = NativeTextBlock {
            text: "真值一".into(),
            bbox: NormalizedBbox::new(0, 0, 400, 400).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        let second = NativeTextBlock {
            text: "真值二".into(),
            bbox: NormalizedBbox::new(600, 0, 900, 400).unwrap(),
            order: 2,
            source: ocrx_core::BlockSource::PdfText,
        };
        let actual = NativeTextBlock {
            text: "不同文字仍可命中几何框".into(),
            bbox: NormalizedBbox::new(10, 10, 390, 390).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        assert!((bbox_recall(&[first, second], &[actual]) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn reading_order_accuracy_scores_perfect_and_reversed_orders() {
        let first = NativeTextBlock {
            text: "一".into(),
            bbox: NormalizedBbox::new(0, 0, 100, 100).unwrap(),
            order: 1,
            source: ocrx_core::BlockSource::PdfText,
        };
        let second = NativeTextBlock {
            text: "二".into(),
            bbox: NormalizedBbox::new(200, 0, 300, 100).unwrap(),
            order: 2,
            source: ocrx_core::BlockSource::PdfText,
        };
        assert!(
            (reading_order_accuracy(
                &[first.clone(), second.clone()],
                &[first.clone(), second.clone()]
            ) - 1.0)
                .abs()
                < f32::EPSILON
        );
        assert!(
            reading_order_accuracy(&[first.clone(), second.clone()], &[second, first.clone()])
                .abs()
                < f32::EPSILON
        );
        assert!((reading_order_accuracy(&[first], &[])).abs() < f32::EPSILON);
    }

    #[test]
    fn route_confusion_matrix_uses_stable_page_type_axes() {
        let matrix = route_confusion_matrix(
            &[PageType::Text, PageType::Image, PageType::Empty],
            &[PageType::Text, PageType::Mixed, PageType::Empty],
        );
        assert_eq!(matrix.counts[0][0], 1);
        assert_eq!(matrix.counts[1][2], 1);
        assert_eq!(matrix.counts[3][3], 1);
        assert_eq!(matrix.counts.iter().flatten().sum::<u32>(), 3);
    }

    #[test]
    fn route_accuracy_is_deterministic_for_empty_perfect_and_mixed_routes() {
        assert!((route_accuracy(&RouteConfusionMatrix::default()) - 1.0).abs() < f32::EPSILON);
        let perfect = route_confusion_matrix(
            &[PageType::Text, PageType::Image],
            &[PageType::Text, PageType::Image],
        );
        assert!((route_accuracy(&perfect) - 1.0).abs() < f32::EPSILON);
        let mixed = route_confusion_matrix(
            &[PageType::Text, PageType::Image],
            &[PageType::Text, PageType::Mixed],
        );
        assert!((route_accuracy(&mixed) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn native_release_metrics_enforce_all_p0_thresholds() {
        let passing = NativeReleaseMetrics {
            total_pages: 30,
            native_pages: 30,
            fallback_pages: 0,
            ocr_pages: 0,
            failed_pages: 0,
            text_ned: 0.99,
            ocr_baseline_text_ned: 0.99,
            reading_order_accuracy: 0.98,
            ocr_baseline_reading_order_accuracy: 0.98,
            block_precision: 0.97,
            block_recall: 0.97,
            block_f1: 0.97,
            bbox_validity_rate: 0.99,
            bbox_recall: 0.97,
            duplicate_pages: 0,
            blank_pages: 0,
        };
        assert!(validate_native_release_metrics(passing).is_ok());
        let report = native_release_gate_report(passing);
        assert_eq!(
            report.to_json(),
            "{\"passed\":true,\"failure\":null,\"failures\":[]}"
        );
        assert_eq!(
            validate_native_release_metrics(NativeReleaseMetrics {
                text_ned: 0.989,
                ..passing
            }),
            Err("text NED is below release threshold")
        );
        let failed = native_release_gate_report(NativeReleaseMetrics {
            native_pages: 29,
            ..passing
        });
        assert!(!failed.passed);
        assert_eq!(
            failed.to_json(),
            "{\"passed\":false,\"failure\":\"native route coverage is below 100%\",\"failures\":[\"native route coverage is below 100%\"]}"
        );
        assert_eq!(
            validate_native_release_metrics(NativeReleaseMetrics {
                native_pages: 29,
                ..passing
            }),
            Err("native route coverage is below 100%")
        );
        assert_eq!(
            validate_native_release_metrics(NativeReleaseMetrics {
                ocr_baseline_text_ned: 0.996,
                ..passing
            }),
            Err("text NED regresses against OCR baseline")
        );
        assert_eq!(
            validate_native_release_metrics(NativeReleaseMetrics {
                ocr_baseline_reading_order_accuracy: 0.991,
                ..passing
            }),
            Err("reading order accuracy regresses against OCR baseline")
        );
        let multiple = native_release_gate_report(NativeReleaseMetrics {
            native_pages: 29,
            text_ned: 0.8,
            ..passing
        });
        assert_eq!(multiple.failures.len(), 3);
    }

    #[test]
    fn performance_gate_requires_five_runs_native_only_and_half_median() {
        let passing = PerformanceGateInput {
            auto_elapsed_ms: vec![40, 50, 45, 55, 50],
            ocr_elapsed_ms: vec![100, 110, 105, 120, 100],
            auto_all_native: true,
            model_loaded: false,
        };
        assert!(validate_performance_gate(&passing).is_ok());
        assert_eq!(
            validate_performance_gate(&PerformanceGateInput {
                auto_elapsed_ms: vec![40; 4],
                ..passing.clone()
            }),
            Err("performance gate requires at least five measurements per mode")
        );
        assert_eq!(
            validate_performance_gate(&PerformanceGateInput {
                auto_all_native: false,
                ..passing.clone()
            }),
            Err("performance gate requires every auto run to finish as native_text")
        );
        assert_eq!(
            validate_performance_gate(&PerformanceGateInput {
                model_loaded: true,
                ..passing.clone()
            }),
            Err("performance gate requires auto runs without model loading")
        );
        assert_eq!(
            validate_performance_gate(&PerformanceGateInput {
                auto_elapsed_ms: vec![60; 5],
                ..passing
            }),
            Err("auto median elapsed time exceeds fifty percent of OCR")
        );
    }

    #[test]
    fn native_page_artifacts_validate_and_return_matching_markdown_and_json() {
        let page = NativePage {
            text: "正文".to_owned(),
            blocks: vec![NativeTextBlock {
                text: "正文".to_owned(),
                bbox: NormalizedBbox::new(10, 10, 500, 500).unwrap(),
                order: 1,
                source: ocrx_core::BlockSource::PdfText,
            }],
            signals: PageSignals {
                visible_characters: 2,
                text_coverage: 1.0,
                text_bbox_coverage: 0.24,
                ..Default::default()
            },
        };
        let (markdown, json) = native_page_artifacts(6, &page, QualityTier::L2).unwrap();
        assert_eq!(markdown, "正文");
        assert!(json.contains("\"page_number\":6"));
    }

    #[test]
    fn empty_pdf_page_artifacts_are_a_complete_empty_route() {
        let classification = PageClassification {
            page_type: PageType::Empty,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::Empty,
            reason: PdfReasonCode::NoVisibleContent,
        };
        let (markdown, json) = empty_pdf_page_artifacts(4, classification).unwrap();
        assert!(markdown.is_empty());
        assert!(json.contains("\"page\":4"));
        assert!(json.contains("\"actual_method\":\"empty\""));
        assert!(json.contains("\"blocks\":[{\"content\":\"\",\"type\":\"markdown\"}]"));
    }
}
