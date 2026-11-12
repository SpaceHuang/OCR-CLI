//! Quality evaluation contracts migrated from `ocrx-pdf` (plan §8.12,
//! T079–T081 copy-first): truth manifest loading/validation, NED + block +
//! bbox + reading-order metrics, route confusion, and the native/performance/
//! release gates. Values and JSON shapes are frozen golden contracts.
//!
//! This module depends only on PDF/core domain types — evaluation consumes
//! facts, it does not touch inference or scanning internals.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ocrx_core::PageType;
use ocrx_pdf::NativeTextBlock;

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
                && ocrx_pdf::bbox_iou(truth.bbox, candidate.bbox) >= 0.5
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
                !used[index] && ocrx_pdf::bbox_iou(truth.bbox, candidate.bbox) >= 0.5
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
                && ocrx_pdf::bbox_iou(truth.bbox, candidate.bbox) >= 0.5
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

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_core::NormalizedBbox;

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
}
