//! Stable domain contracts shared by the OCR pipeline.

use std::io;
use std::path::{Component, Path, PathBuf};

pub mod resource_sampler;

// Cancellation/progress/hash primitives moved to `ocrx-primitives` (plan
// §8.2). Core re-exports them so the legacy import paths keep compiling;
// the duplicate implementations were deleted with the move (T010/T011).
pub use ocrx_primitives::{
    CancellationToken, GenerationProgress, Sha256State, sha256, sha256_file,
};

/// Frozen decode-window depth for multi-page continuous batching (phase-2
/// stage C unfreeze, 2026-11-04: **N = 8**).
///
/// History: the first freeze landed at N = 1 because the integrated N=4
/// measurement regressed ~5–11% on both backends — but that measurement ran
/// a broken window decode (review finding 1: the loop re-fed the seed token
/// every step), so both its outputs and its timings were invalid. After the
/// fix (verified against the real model: window output == single-stream
/// output byte-for-byte), the corrected N=4 gates are: golden 40.9–41.5 s vs
/// 55.5 s single-stream (1.36× end-to-end), decode aggregate 417–430 tok/s
/// vs 283 (1.52×; the R0.6 isolated bench promised 3.21× — the hybrid
/// attention arch's 18/24 per-sequence SSM layers cap the realizable gain),
/// self-consistency byte-identical, page similarity 1.0000, zero failed /
/// truncated / degenerated pages.
///
/// 2026-11-02 post-review freeze re-confirmed N = 4 (phase-1 P4.4).
///
/// 2026-11-04 unfreeze to N = 8 (phase-2 plan §4 stage C): the isolated
/// batched-bench curve gives B=8 = 1527.6 tok/s (4.76× single-stream, +48%
/// over B=4); window `n_ctx` = max page budget × 8 ≈ 139,776 ≤ the model's
/// 262,144 training window; KV ≈ 1.7 GB at the 12 KB/token measure keeps the
/// load inside the 6 GB VRAM assertion. Gates (all measured on the phase-2
/// branch, Vulkan): structural similarity vs the N=4 frozen reference
/// ≥ 0.999 + manual sampling, self-consistency byte-identical across
/// passes, ≥ 5% wall improvement over the post-A baseline (38.2 s), 100-page
/// resource regression clean, Ctrl+C smoke contract updated (window 1 =
/// pages 1–8, mechanism-A evidence pages 9–12). The compatibility key
/// carries `;batch=8` (was `;batch=4`), so pre-unfreeze checkpoints no
/// longer match and `--resume` rescans the whole document from scratch —
/// an expected, documented behavior change. Phase-1 freeze discipline
/// stands: performance numbers must come from a correctness-gated build.
pub const WINDOW_DEPTH: usize = 8;

/// Creates or validates one direct child directory without following a
/// pre-existing symlink. Returns its canonical path for subsequent writes.
///
/// # Errors
///
/// Returns an error if the root or child cannot be inspected, the child is a
/// symlink or non-directory, or the canonical child is outside the root.
pub fn create_contained_child_dir(root: &Path, child: &str) -> io::Result<PathBuf> {
    let mut components = Path::new(child).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child must be one path component",
        ));
    }
    std::fs::create_dir_all(root)?;
    let canonical_root = root.canonicalize()?;
    let child_path = root.join(child);
    match std::fs::symlink_metadata(&child_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "output child directory must not be a symlink",
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output child path is not a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::fs::create_dir(&child_path)?,
        Err(error) => return Err(error),
    }
    let canonical_child = child_path.canonicalize()?;
    if !canonical_child.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "output child directory escapes output root",
        ));
    }
    Ok(canonical_child)
}

/// Phase-4 depth tiering: the window depth is selected per device from the
/// GPU's VRAM capacity. The 4090-class measured basis (2026-11-07 probe,
/// `docs/test/reports/window-depth-curve-2026-11-07-data/`): depth 16 runs
/// the golden set -5.9% same-day paired at a 7.87 GiB VRAM peak with
/// similarity 1.000000 vs the depth-8 reference, while depth 12 hits a
/// kernel-quantization pit (-11%) and is skipped entirely. The 8 tier keeps
/// the original 6-GB-class freeze rule (C stage) for smaller cards.
///
/// `vram_bytes` is the device-local VRAM capacity (`None` = CPU backend or
/// probe unavailable): the 16 tier needs the measured ~8 GiB peak plus
/// desktop headroom, so it opens at 12 GiB. Tier ladder is {8, 16} only.
#[must_use]
pub fn window_depth_for_vram(vram_bytes: Option<u64>) -> usize {
    const TWELVE_GIB: u64 = 12 * 1024 * 1024 * 1024;
    match vram_bytes {
        Some(vram) if vram >= TWELVE_GIB => 16,
        _ => WINDOW_DEPTH,
    }
}

/// Prefix marking a batch-level window failure; the scan side falls back to
/// depth-1 (single-stream) resubmission when it sees it (plan §7.3③).
pub const BATCH_ERROR_PREFIX: &str = "batch inference failed";

/// Stable GPU resource/performance report (W4.4).
///
/// The same schema is used for Vulkan and CUDA. Platform/backend differences
/// are values, not field names, so downstream reporting and golden tests stay
/// backend-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuResourceReport {
    pub backend: String,
    pub vram_peak_bytes: Option<u64>,
    pub unified_memory_peak_bytes: Option<u64>,
    pub load_time_ms: Option<u64>,
    pub first_token_ms: Option<u64>,
    pub output_tokens: u32,
    pub output_tokens_per_sec: f64,
    pub peak_memory_bytes: Option<u64>,
}

impl GpuResourceReport {
    #[must_use]
    pub fn to_json(&self) -> String {
        let optional =
            |value: Option<u64>| value.map_or_else(|| "null".to_owned(), |value| value.to_string());
        // Non-finite floats have no JSON literal; emit null instead of the
        // bare `NaN`/`inf` a strict parser rejects (review m16).
        let finite = |value: f64| {
            if value.is_finite() {
                format!("{value:.3}")
            } else {
                "null".to_owned()
            }
        };
        format!(
            "{{\"schema_version\":1,\"backend\":\"{}\",\"vram_peak_bytes\":{},\"unified_memory_peak_bytes\":{},\"load_time_ms\":{},\"first_token_ms\":{},\"output_tokens\":{},\"output_tokens_per_sec\":{},\"peak_memory_bytes\":{}}}",
            json_escape(&self.backend),
            optional(self.vram_peak_bytes),
            optional(self.unified_memory_peak_bytes),
            optional(self.load_time_ms),
            optional(self.first_token_ms),
            self.output_tokens,
            finite(self.output_tokens_per_sec),
            optional(self.peak_memory_bytes),
        )
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobId(pub String);

/// The execution policy selected for PDF input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfMode {
    Auto,
    Text,
    Ocr,
}

impl PdfMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Text => "text",
            Self::Ocr => "ocr",
        }
    }
}

/// Minimum content fidelity required from native extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    L1,
    L2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageType {
    Text,
    Image,
    Mixed,
    Empty,
    Uncertain,
}

impl PageType {
    #[must_use]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteConfidence {
    High,
    Medium,
    Low,
}

impl RouteConfidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingMethod {
    NativeText,
    Ocr,
    Empty,
}

impl ProcessingMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeText => "native_text",
            Self::Ocr => "ocr",
            Self::Empty => "empty",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockSource {
    PdfText,
    PdfImage,
    Ocr,
    Derived,
}

impl BlockSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PdfText => "pdf_text",
            Self::PdfImage => "pdf_image",
            Self::Ocr => "ocr",
            Self::Derived => "derived",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfReasonCode {
    NativeTextReliable,
    NoVisibleContent,
    ImageCoverageHigh,
    MixedContent,
    TextCoverageLow,
    UnicodeValidityLow,
    DuplicateText,
    GeometryAnomaly,
    NativeStructureUnresolved,
    NativeValidationFailed,
}

impl PdfReasonCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeTextReliable => "native_text_reliable",
            Self::NoVisibleContent => "no_visible_content",
            Self::ImageCoverageHigh => "image_coverage_high",
            Self::MixedContent => "mixed_content",
            Self::TextCoverageLow => "text_coverage_low",
            Self::UnicodeValidityLow => "unicode_validity_low",
            Self::DuplicateText => "duplicate_text",
            Self::GeometryAnomaly => "geometry_anomaly",
            Self::NativeStructureUnresolved => "native_structure_unresolved",
            Self::NativeValidationFailed => "native_validation_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageClassification {
    pub page_type: PageType,
    pub confidence: RouteConfidence,
    pub recommended_method: ProcessingMethod,
    pub reason: PdfReasonCode,
}

impl PageClassification {
    #[must_use]
    pub fn to_json(self) -> String {
        format!(
            "{{\"page_type\":\"{}\",\"confidence\":\"{}\",\"recommended_method\":\"{}\",\"reason\":\"{}\"}}",
            self.page_type.as_str(),
            self.confidence.as_str(),
            self.recommended_method.as_str(),
            self.reason.as_str()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageProcessing {
    pub planned_method: ProcessingMethod,
    pub actual_method: Option<ProcessingMethod>,
    pub fallback_reason: Option<PdfReasonCode>,
}

impl PageProcessing {
    pub fn validate(self) -> Result<(), DomainError> {
        if self.actual_method == Some(ProcessingMethod::Ocr)
            && self.planned_method == ProcessingMethod::NativeText
            && self.fallback_reason.is_none()
        {
            return Err(DomainError::InvalidPage);
        }
        if self.planned_method == ProcessingMethod::Empty
            && self
                .actual_method
                .is_some_and(|method| method != ProcessingMethod::Empty)
        {
            return Err(DomainError::InvalidPage);
        }
        if self.actual_method == Some(ProcessingMethod::NativeText)
            && self.planned_method != ProcessingMethod::NativeText
        {
            return Err(DomainError::InvalidPage);
        }
        if self.fallback_reason.is_some() && self.actual_method != Some(ProcessingMethod::Ocr) {
            return Err(DomainError::InvalidPage);
        }
        Ok(())
    }

    #[must_use]
    pub fn to_json(self) -> String {
        let actual = self.actual_method.map_or_else(
            || "null".to_owned(),
            |method| format!("\"{}\"", method.as_str()),
        );
        let fallback = self.fallback_reason.map_or_else(
            || "null".to_owned(),
            |reason| format!("\"{}\"", reason.as_str()),
        );
        format!(
            "{{\"planned_method\":\"{}\",\"actual_method\":{},\"fallback_reason\":{}}}",
            self.planned_method.as_str(),
            actual,
            fallback
        )
    }
}

#[cfg(test)]
mod pdf_contract_tests {
    use super::{
        BlockSource, DomainError, NormalizedBbox, PageBlock, PageClassification, PageDocument,
        PageProcessing, PageType, PdfBlockProvenance, PdfMode, PdfPageDocument, PdfReasonCode,
        ProcessingMethod, QualityTier, RouteConfidence, Termination,
    };

    #[test]
    fn pdf_diagnostic_strings_are_stable_machine_values() {
        assert_eq!(PageType::Uncertain.as_str(), "uncertain");
        assert_eq!(RouteConfidence::Medium.as_str(), "medium");
        assert_eq!(ProcessingMethod::NativeText.as_str(), "native_text");
        assert_eq!(
            PdfReasonCode::NativeStructureUnresolved.as_str(),
            "native_structure_unresolved"
        );
    }

    #[test]
    fn routing_configuration_enums_round_trip_with_stable_json_values() {
        for (encoded, value) in [
            ("\"auto\"", PdfMode::Auto),
            ("\"text\"", PdfMode::Text),
            ("\"ocr\"", PdfMode::Ocr),
        ] {
            assert_eq!(serde_json::to_string(&value).unwrap(), encoded);
            assert_eq!(serde_json::from_str::<PdfMode>(encoded).unwrap(), value);
        }
        for (encoded, value) in [("\"l1\"", QualityTier::L1), ("\"l2\"", QualityTier::L2)] {
            assert_eq!(serde_json::to_string(&value).unwrap(), encoded);
            assert_eq!(serde_json::from_str::<QualityTier>(encoded).unwrap(), value);
        }
        assert!(serde_json::from_str::<PdfMode>("\"preview\"").is_err());
        assert!(serde_json::from_str::<QualityTier>("\"l3\"").is_err());
    }

    #[test]
    fn page_diagnostic_json_has_stable_shape_and_null_optional_fields() {
        let classification = PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
        assert_eq!(
            classification.to_json(),
            "{\"page_type\":\"text\",\"confidence\":\"high\",\"recommended_method\":\"native_text\",\"reason\":\"native_text_reliable\"}"
        );
        let processing = PageProcessing {
            planned_method: ProcessingMethod::NativeText,
            actual_method: None,
            fallback_reason: None,
        };
        assert_eq!(
            processing.to_json(),
            "{\"planned_method\":\"native_text\",\"actual_method\":null,\"fallback_reason\":null}"
        );
    }

    #[test]
    fn routed_domain_metadata_round_trips_through_serde_with_stable_enum_names() {
        let classification = PageClassification {
            page_type: PageType::Mixed,
            confidence: RouteConfidence::Medium,
            recommended_method: ProcessingMethod::Ocr,
            reason: PdfReasonCode::MixedContent,
        };
        let processing = PageProcessing {
            planned_method: ProcessingMethod::NativeText,
            actual_method: Some(ProcessingMethod::Ocr),
            fallback_reason: Some(PdfReasonCode::NativeValidationFailed),
        };
        let classification_json = serde_json::to_string(&classification).unwrap();
        let processing_json = serde_json::to_string(&processing).unwrap();
        assert_eq!(
            classification_json,
            "{\"page_type\":\"mixed\",\"confidence\":\"medium\",\"recommended_method\":\"ocr\",\"reason\":\"mixed_content\"}"
        );
        assert_eq!(
            processing_json,
            "{\"planned_method\":\"native_text\",\"actual_method\":\"ocr\",\"fallback_reason\":\"native_validation_failed\"}"
        );
        assert_eq!(
            serde_json::from_str::<PageClassification>(&classification_json).unwrap(),
            classification
        );
        assert_eq!(
            serde_json::from_str::<PageProcessing>(&processing_json).unwrap(),
            processing
        );
    }

    #[test]
    fn page_processing_rejects_inconsistent_actual_and_fallback_states() {
        assert_eq!(
            (PageProcessing {
                planned_method: ProcessingMethod::Ocr,
                actual_method: Some(ProcessingMethod::NativeText),
                fallback_reason: None,
            })
            .validate(),
            Err(DomainError::InvalidPage)
        );
        assert_eq!(
            (PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: None,
                fallback_reason: Some(PdfReasonCode::NativeValidationFailed),
            })
            .validate(),
            Err(DomainError::InvalidPage)
        );
    }

    #[test]
    fn page_processing_accepts_pending_and_consistent_terminal_states() {
        for processing in [
            PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: None,
                fallback_reason: None,
            },
            PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: Some(ProcessingMethod::NativeText),
                fallback_reason: None,
            },
            PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: Some(ProcessingMethod::Ocr),
                fallback_reason: Some(PdfReasonCode::NativeValidationFailed),
            },
            PageProcessing {
                planned_method: ProcessingMethod::Ocr,
                actual_method: Some(ProcessingMethod::Ocr),
                fallback_reason: None,
            },
            PageProcessing {
                planned_method: ProcessingMethod::Empty,
                actual_method: Some(ProcessingMethod::Empty),
                fallback_reason: None,
            },
        ] {
            assert_eq!(processing.validate(), Ok(()));
        }
    }

    #[test]
    fn page_processing_validation_rejects_inconsistent_route_states() {
        assert!(
            PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: Some(ProcessingMethod::Ocr),
                fallback_reason: None
            }
            .validate()
            .is_err()
        );
        assert!(
            PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: Some(ProcessingMethod::Ocr),
                fallback_reason: Some(PdfReasonCode::NativeValidationFailed)
            }
            .validate()
            .is_ok()
        );
        assert!(
            PageProcessing {
                planned_method: ProcessingMethod::Empty,
                actual_method: Some(ProcessingMethod::Ocr),
                fallback_reason: None
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn pdf_page_document_v2_round_trips_contract_metadata_and_rejects_wrong_version() {
        let classification = PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
        let document = PdfPageDocument {
            schema_version: PdfPageDocument::SCHEMA_VERSION,
            document: PageDocument {
                schema_version: PageDocument::SCHEMA_VERSION,
                page_number: 1,
                blocks: vec![PageBlock::Markdown {
                    content: "native".into(),
                }],
                termination: Termination::Eos,
                warnings: vec![],
            },
            classification,
            processing: PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: Some(ProcessingMethod::NativeText),
                fallback_reason: None,
            },
            provenance: vec![PdfBlockProvenance {
                source: BlockSource::PdfText,
                bbox: Some(NormalizedBbox::new(1, 1, 100, 100).unwrap()),
                source_order: 1,
            }],
        };

        let json = document.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["schema_version"], 2);
        assert!(json.contains("\"schema_version\":2"));
        assert!(json.contains("\"page_type\":\"text\""));
        assert!(json.contains("\"actual_method\":\"native_text\""));
        assert!(json.contains("\"source\":\"pdf_text\""));
        let decoded = PdfPageDocument::from_json(&json).unwrap();
        assert_eq!(decoded.document, document.document);
        assert_eq!(decoded.classification, document.classification);
        assert_eq!(decoded.processing, document.processing);
        assert_eq!(decoded.provenance, document.provenance);
        assert_eq!(parsed["classification"]["reason"], "native_text_reliable");
        assert_eq!(parsed["processing"]["planned_method"], "native_text");
        assert!(document.validate().is_ok());
        let mut old = document;
        old.schema_version = 1;
        assert_eq!(old.validate(), Err(DomainError::InvalidPage));
    }

    #[test]
    fn serde_metadata_rejects_unknown_reason_and_missing_required_fields() {
        assert!(serde_json::from_str::<PdfReasonCode>("\"future_reason\"").is_err());
        assert!(serde_json::from_str::<PageClassification>("{\"page_type\":\"text\"}").is_err());
        assert!(
            serde_json::from_str::<PdfBlockProvenance>(
                "{\"source\":\"pdf_text\",\"source_order\":0,\"bbox\":null}"
            )
            .is_ok()
        );
    }

    #[test]
    fn native_pdf_page_requires_pdf_text_provenance() {
        let classification = PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
        let document = PdfPageDocument {
            schema_version: PdfPageDocument::SCHEMA_VERSION,
            document: PageDocument {
                schema_version: PageDocument::SCHEMA_VERSION,
                page_number: 1,
                blocks: vec![PageBlock::Markdown {
                    content: "text".into(),
                }],
                termination: Termination::Eos,
                warnings: vec![],
            },
            classification,
            processing: PageProcessing {
                planned_method: ProcessingMethod::NativeText,
                actual_method: Some(ProcessingMethod::NativeText),
                fallback_reason: None,
            },
            provenance: vec![PdfBlockProvenance {
                source: BlockSource::Ocr,
                bbox: None,
                source_order: 1,
            }],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidPage));
    }

    #[test]
    fn native_pdf_page_requires_text_classification() {
        let document = PdfPageDocument {
            schema_version: PdfPageDocument::SCHEMA_VERSION,
            document: PageDocument {
                schema_version: PageDocument::SCHEMA_VERSION,
                page_number: 1,
                blocks: vec![PageBlock::Markdown {
                    content: "text".into(),
                }],
                termination: Termination::Eos,
                warnings: vec![],
            },
            classification: PageClassification {
                page_type: PageType::Image,
                confidence: RouteConfidence::High,
                recommended_method: ProcessingMethod::Ocr,
                reason: PdfReasonCode::ImageCoverageHigh,
            },
            processing: PageProcessing {
                planned_method: ProcessingMethod::Ocr,
                actual_method: Some(ProcessingMethod::NativeText),
                fallback_reason: None,
            },
            provenance: vec![PdfBlockProvenance {
                source: BlockSource::PdfText,
                bbox: None,
                source_order: 1,
            }],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidPage));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDocument {
    pub schema_version: u32,
    pub page_number: u32,
    pub blocks: Vec<PageBlock>,
    pub termination: Termination,
    pub warnings: Vec<PageWarning>,
}

#[derive(Serialize)]
struct PageDocumentSerde<'a> {
    schema_version: u32,
    page_number: u32,
    blocks: Vec<PageBlockSerde<'a>>,
    termination: &'a str,
    warnings: Vec<&'a str>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum PageBlockSerde<'a> {
    #[serde(rename = "markdown")]
    Markdown { content: &'a str },
    #[serde(rename = "formula")]
    Formula { latex: &'a str, display: bool },
    #[serde(rename = "html_table")]
    HtmlTable { html: &'a str },
    #[serde(rename = "visual_region")]
    VisualRegion {
        bbox: NormalizedBbox,
        source_order: u32,
    },
}

fn page_block_serde(block: &PageBlock) -> PageBlockSerde<'_> {
    match block {
        PageBlock::Markdown { content } => PageBlockSerde::Markdown { content },
        PageBlock::Formula { latex, display } => PageBlockSerde::Formula {
            latex,
            display: *display,
        },
        PageBlock::HtmlTable { html } => PageBlockSerde::HtmlTable { html },
        PageBlock::VisualRegion { bbox, source_order } => PageBlockSerde::VisualRegion {
            bbox: *bbox,
            source_order: *source_order,
        },
    }
}

impl PageDocument {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Validates the stable page contract before it can be persisted or rendered.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidPage`] for an unsupported schema or page
    /// number, and [`DomainError::InvalidBbox`] for an invalid visual region.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != Self::SCHEMA_VERSION || self.page_number == 0 {
            return Err(DomainError::InvalidPage);
        }
        if self.blocks.iter().any(|block| {
            matches!(
                block,
                PageBlock::VisualRegion { bbox, .. } if !bbox.is_valid()
            )
        }) {
            return Err(DomainError::InvalidBbox);
        }
        Ok(())
    }

    /// Serializes the validated domain document into the stable JSON shape.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the document violates the domain invariants.
    pub fn to_json(&self) -> Result<String, DomainError> {
        self.validate()?;
        let value = PageDocumentSerde {
            schema_version: self.schema_version,
            page_number: self.page_number,
            blocks: self.blocks.iter().map(page_block_serde).collect(),
            termination: self.termination.as_str(),
            warnings: self.warnings.iter().map(PageWarning::as_str).collect(),
        };
        serde_json::to_string(&value).map_err(|_| DomainError::InvalidJson)
    }

    /// Deserializes the stable JSON shape emitted by [`Self::to_json`].
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidJson`] for malformed or unsupported JSON.
    pub fn from_json(value: &str) -> Result<Self, DomainError> {
        let root: serde_json::Value =
            serde_json::from_str(value).map_err(|_| DomainError::InvalidJson)?;
        let schema_version = root
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(DomainError::InvalidJson)?;
        let page_number = root
            .get("page_number")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(DomainError::InvalidJson)?;
        let termination = match root
            .get("termination")
            .and_then(serde_json::Value::as_str)
            .ok_or(DomainError::InvalidJson)?
        {
            "eos" => Termination::Eos,
            "stop_token" => Termination::StopToken,
            "max_tokens" => Termination::MaxTokens,
            "invalid" => Termination::Invalid,
            "repetition_loop" => Termination::RepetitionLoop,
            _ => return Err(DomainError::InvalidJson),
        };
        let warnings = root
            .get("warnings")
            .and_then(serde_json::Value::as_array)
            .ok_or(DomainError::InvalidJson)?
            .iter()
            .map(|warning| match warning.as_str() {
                Some("truncated") => Ok(PageWarning::Truncated),
                Some("repetition_removed") => Ok(PageWarning::RepetitionRemoved),
                Some("render_scaled") => Ok(PageWarning::RenderScaled),
                Some("degenerated") => Ok(PageWarning::Degenerated),
                Some("think_removed") => Ok(PageWarning::ThinkRemoved),
                _ => Err(DomainError::InvalidJson),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let blocks = root
            .get("blocks")
            .and_then(serde_json::Value::as_array)
            .ok_or(DomainError::InvalidJson)?
            .iter()
            .map(|block| parse_block(&block.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let document = Self {
            schema_version,
            page_number,
            blocks,
            termination,
            warnings,
        };
        document.validate()?;
        Ok(document)
    }
}

/// Versioned envelope for PDF content-aware pages.
///
/// The existing [`PageDocument`] remains the OCR adapter contract. PDF pages
/// use this envelope so routing metadata cannot be silently dropped when a
/// page is persisted or resumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPageDocument {
    pub schema_version: u32,
    pub document: PageDocument,
    pub classification: PageClassification,
    pub processing: PageProcessing,
    pub provenance: Vec<PdfBlockProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfBlockProvenance {
    pub source: BlockSource,
    pub bbox: Option<NormalizedBbox>,
    pub source_order: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct PdfPageMetadataSerde {
    schema_version: u32,
    page: u32,
    classification: PageClassification,
    processing: PageProcessing,
    provenance: Vec<PdfBlockProvenance>,
}

#[derive(Debug, Serialize)]
struct PdfPageEnvelopeSerde {
    schema_version: u32,
    page: u32,
    classification: PageClassification,
    processing: PageProcessing,
    provenance: Vec<PdfBlockProvenance>,
    document: serde_json::Value,
}

impl PdfPageDocument {
    pub const SCHEMA_VERSION: u32 = 2;

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(DomainError::InvalidPage);
        }
        self.document.validate()?;
        self.processing.validate()?;
        if self.processing.actual_method == Some(ProcessingMethod::NativeText)
            && (self.classification.page_type != PageType::Text
                || self.classification.recommended_method != ProcessingMethod::NativeText)
        {
            return Err(DomainError::InvalidPage);
        }
        if self.processing.actual_method == Some(ProcessingMethod::NativeText)
            && (self.provenance.is_empty()
                || self
                    .provenance
                    .iter()
                    .any(|block| block.source != BlockSource::PdfText))
        {
            return Err(DomainError::InvalidPage);
        }
        let mut orders = self
            .provenance
            .iter()
            .map(|block| block.source_order)
            .collect::<Vec<_>>();
        if orders.contains(&0) {
            return Err(DomainError::InvalidPage);
        }
        orders.sort_unstable();
        if orders.windows(2).any(|pair| pair[0] == pair[1])
            || self
                .provenance
                .iter()
                .any(|block| block.bbox.is_some_and(|bbox| !bbox.is_valid()))
        {
            return Err(DomainError::InvalidPage);
        }
        if self.processing.planned_method != self.classification.recommended_method
            && self.processing.planned_method != ProcessingMethod::Ocr
        {
            return Err(DomainError::InvalidPage);
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, DomainError> {
        self.validate()?;
        let document = serde_json::from_str(&self.document.to_json()?)
            .map_err(|_| DomainError::InvalidJson)?;
        serde_json::to_string(&PdfPageEnvelopeSerde {
            schema_version: Self::SCHEMA_VERSION,
            page: self.document.page_number,
            classification: self.classification,
            processing: self.processing,
            provenance: self.provenance.clone(),
            document,
        })
        .map_err(|_| DomainError::InvalidJson)
    }

    pub fn from_json(value: &str) -> Result<Self, DomainError> {
        Self::from_json_serde(value)
    }

    fn from_json_serde(value: &str) -> Result<Self, DomainError> {
        let root: serde_json::Value =
            serde_json::from_str(value).map_err(|_| DomainError::InvalidJson)?;
        let metadata: PdfPageMetadataSerde =
            serde_json::from_value(root.clone()).map_err(|_| DomainError::InvalidJson)?;
        let document_value = root.get("document").ok_or(DomainError::InvalidJson)?;
        let document_json =
            serde_json::to_string(document_value).map_err(|_| DomainError::InvalidJson)?;
        let document = PageDocument::from_json(&document_json)?;
        if metadata.page != document.page_number {
            return Err(DomainError::InvalidPage);
        }
        let result = Self {
            schema_version: metadata.schema_version,
            document,
            classification: metadata.classification,
            processing: metadata.processing,
            provenance: metadata.provenance,
        };
        result.validate()?;
        Ok(result)
    }
}

fn json_number(value: &str, key: &str) -> Result<u32, DomainError> {
    value
        .split_once(&format!("\"{key}\":"))
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|number| number.parse().ok())
        .ok_or(DomainError::InvalidJson)
}

fn json_string(value: &str, key: &str) -> Result<String, DomainError> {
    value
        .split_once(&format!("\"{key}\":\""))
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(string, _)| {
            string
                .replace("\\n", "\n")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
        })
        .ok_or(DomainError::InvalidJson)
}

fn parse_block(value: &str) -> Result<PageBlock, DomainError> {
    let value = value.trim_matches('{').trim_matches('}');
    let kind = json_string(value, "type")?;
    match kind.as_str() {
        "markdown" => Ok(PageBlock::Markdown {
            content: json_string(value, "content")?,
        }),
        "formula" => Ok(PageBlock::Formula {
            latex: json_string(value, "latex")?,
            display: value.contains("\"display\":true"),
        }),
        "html_table" => Ok(PageBlock::HtmlTable {
            html: json_string(value, "html")?,
        }),
        "visual_region" => {
            let bbox = value
                .split_once("\"bbox\":{")
                .and_then(|(_, rest)| rest.split_once('}'))
                .ok_or(DomainError::InvalidJson)?
                .0;
            let number =
                |key| u16::try_from(json_number(bbox, key)?).map_err(|_| DomainError::InvalidJson);
            Ok(PageBlock::VisualRegion {
                bbox: NormalizedBbox::new(
                    number("left")?,
                    number("top")?,
                    number("right")?,
                    number("bottom")?,
                )?,
                source_order: json_number(value, "source_order")?,
            })
        }
        _ => Err(DomainError::InvalidJson),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageBlock {
    Markdown {
        content: String,
    },
    Formula {
        latex: String,
        display: bool,
    },
    HtmlTable {
        html: String,
    },
    VisualRegion {
        bbox: NormalizedBbox,
        source_order: u32,
    },
}

impl PageBlock {
    fn to_json(&self) -> String {
        match self {
            Self::Markdown { content } => format!(
                "{{\"type\":\"markdown\",\"content\":\"{}\"}}",
                json_escape(content)
            ),
            Self::Formula { latex, display } => format!(
                "{{\"type\":\"formula\",\"latex\":\"{}\",\"display\":{display}}}",
                json_escape(latex)
            ),
            Self::HtmlTable { html } => format!(
                "{{\"type\":\"html_table\",\"html\":\"{}\"}}",
                json_escape(html)
            ),
            Self::VisualRegion { bbox, source_order } => format!(
                "{{\"type\":\"visual_region\",\"bbox\":{{\"left\":{},\"top\":{},\"right\":{},\"bottom\":{}}},\"source_order\":{source_order}}}",
                bbox.left, bbox.top, bbox.right, bbox.bottom
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedBbox {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl NormalizedBbox {
    /// Creates a normalized bounding box after validating its half-open bounds.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidBbox`] when the box has zero area or uses
    /// coordinates outside the `[0, 1000)` coordinate space.
    pub fn new(left: u16, top: u16, right: u16, bottom: u16) -> Result<Self, DomainError> {
        let bbox = Self {
            left,
            top,
            right,
            bottom,
        };
        bbox.is_valid()
            .then_some(bbox)
            .ok_or(DomainError::InvalidBbox)
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.left < self.right && self.top < self.bottom && self.right < 1000 && self.bottom < 1000
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    Eos,
    StopToken,
    MaxTokens,
    Invalid,
    RepetitionLoop,
}

impl Termination {
    fn as_str(self) -> &'static str {
        match self {
            Self::Eos => "eos",
            Self::StopToken => "stop_token",
            Self::MaxTokens => "max_tokens",
            Self::Invalid => "invalid",
            Self::RepetitionLoop => "repetition_loop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageWarning {
    Truncated,
    RepetitionRemoved,
    RenderScaled,
    Degenerated,
    ThinkRemoved,
}

impl PageWarning {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::RepetitionRemoved => "repetition_removed",
            Self::RenderScaled => "render_scaled",
            Self::Degenerated => "degenerated",
            Self::ThinkRemoved => "think_removed",
        }
    }
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Structural JSON syntax validator used by CLI contract tests.
///
/// The project emits hand-assembled JSON strings; this walks the emitted
/// bytes (balanced containers, correctly quoted strings, valid escape
/// sequences, well-formed literals and numbers) without adding a JSON parser
/// dependency. It validates syntax only, never schema. Whitespace around the
/// top-level value is tolerated, matching lenient parsers.
#[must_use]
pub fn is_valid_json(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(end) = parse_json_value(bytes, skip_json_ws(bytes, 0)) else {
        return false;
    };
    skip_json_ws(bytes, end) == bytes.len()
}

fn skip_json_ws(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\n' | b'\r' | b'\t') {
        index += 1;
    }
    index
}

/// Parses one JSON value starting at `index`; returns the index just past it.
fn parse_json_value(bytes: &[u8], mut index: usize) -> Option<usize> {
    index = skip_json_ws(bytes, index);
    match bytes.get(index)? {
        b'{' => {
            index = skip_json_ws(bytes, index + 1);
            if bytes.get(index) == Some(&b'}') {
                return Some(index + 1);
            }
            loop {
                index = skip_json_ws(bytes, index);
                if bytes.get(index) != Some(&b'"') {
                    return None;
                }
                index = parse_json_string(bytes, index)?;
                index = skip_json_ws(bytes, index);
                if bytes.get(index) != Some(&b':') {
                    return None;
                }
                index = parse_json_value(bytes, index + 1)?;
                index = skip_json_ws(bytes, index);
                match bytes.get(index) {
                    Some(&b',') => index += 1,
                    Some(&b'}') => return Some(index + 1),
                    _ => return None,
                }
            }
        }
        b'[' => {
            index = skip_json_ws(bytes, index + 1);
            if bytes.get(index) == Some(&b']') {
                return Some(index + 1);
            }
            loop {
                index = parse_json_value(bytes, index)?;
                index = skip_json_ws(bytes, index);
                match bytes.get(index) {
                    Some(&b',') => index += 1,
                    Some(&b']') => return Some(index + 1),
                    _ => return None,
                }
            }
        }
        b'"' => parse_json_string(bytes, index),
        b't' => consume_json_literal(bytes, index, b"true"),
        b'f' => consume_json_literal(bytes, index, b"false"),
        b'n' => consume_json_literal(bytes, index, b"null"),
        b'-' | b'0'..=b'9' => parse_json_number(bytes, index),
        _ => None,
    }
}

/// Parses a quoted string starting at the opening quote; returns the index
/// just past the closing quote.
fn parse_json_string(bytes: &[u8], mut index: usize) -> Option<usize> {
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    index += 1;
    loop {
        match bytes.get(index)? {
            b'"' => return Some(index + 1),
            b'\\' => {
                index += 1;
                match bytes.get(index)? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => index += 1,
                    b'u' => {
                        for _ in 0..4 {
                            index += 1;
                            if !bytes.get(index)?.is_ascii_hexdigit() {
                                return None;
                            }
                        }
                        index += 1;
                    }
                    // An unescaped Windows path like "C:\Users" lands here.
                    _ => return None,
                }
            }
            byte if *byte < 0x20 => return None, // raw control characters
            _ => index += 1,
        }
    }
}

fn consume_json_literal(bytes: &[u8], index: usize, literal: &[u8]) -> Option<usize> {
    bytes
        .get(index..index + literal.len())?
        .eq_ignore_ascii_case(literal)
        .then(|| index + literal.len())
}

fn parse_json_number(bytes: &[u8], mut index: usize) -> Option<usize> {
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    let digits_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if index == digits_start {
        return None;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return None;
        }
    }
    if matches!(bytes.get(index), Some(&b'e') | Some(&b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(&b'+') | Some(&b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return None;
        }
    }
    Some(index)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCompatibility {
    /// `llama-cpp-2` wrapper crate version.
    pub llama_cpp_2_version: &'static str,
    /// `llama-cpp-sys-2` sys crate version.
    pub llama_cpp_sys_2_version: &'static str,
    /// Embedded `llama.cpp` upstream commit (short SHA), recorded by the
    /// WP.2 build freeze (`docs/develop/runtime-windows-alignment-*.md`).
    pub llama_cpp_commit: &'static str,
    /// Adapter id this runtime binding serves (`ovisocr2`).
    pub adapter_id: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    UnsupportedModel,
    InvalidBbox,
    InvalidPage,
    InvalidJson,
    InvalidError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Success,
    Usage,
    Input,
    Model,
    Download,
    Runtime,
    Parse,
    Integrity,
    Output,
    Partial,
    Cancelled,
}

impl ErrorKind {
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Usage => 2,
            Self::Input => 3,
            Self::Model => 4,
            Self::Download => 5,
            Self::Runtime => 6,
            Self::Parse => 7,
            Self::Integrity => 8,
            Self::Output => 9,
            Self::Partial => 10,
            Self::Cancelled => 130,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub kind: ErrorKind,
    pub code: u8,
    pub message: String,
    pub cause: Option<String>,
    pub suggestion: Option<String>,
}

impl AppError {
    pub fn new(kind: ErrorKind, code: u8, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            cause: None,
            suggestion: None,
        }
    }

    #[must_use]
    pub fn from_kind(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self::new(kind, kind.exit_code(), message)
    }

    #[must_use]
    pub fn with_context(mut self, cause: impl Into<String>, suggestion: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Checks that the exit code matches the stable mapping for [`Self::kind`].
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidError`] when the code and kind disagree.
    pub fn validate(&self) -> Result<(), DomainError> {
        (self.code == self.kind.exit_code())
            .then_some(())
            .ok_or(DomainError::InvalidError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- BLK-1: child tokens scope subsystem lifecycle teardown. -----------

    #[test]
    fn child_observes_ancestors_but_its_cancel_stays_local() {
        let parent = CancellationToken::new();
        let child = parent.child();
        // Without any request, neither side is cancelled.
        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());
        // Cancelling the child (renderer teardown) must NOT touch the parent.
        child.request_cancel();
        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled(), "child cancel leaked to the parent");
        assert!(!parent.force_exit());
        // Cancelling the parent (user Ctrl+C) must cancel every descendant.
        let other_child = parent.child();
        parent.request_cancel();
        assert!(other_child.is_cancelled());
        assert!(
            !other_child.force_exit(),
            "single parent cancel is graceful"
        );
        // Escalation on the parent is observed by descendants too.
        parent.request_cancel();
        assert!(parent.force_exit());
        assert!(other_child.force_exit());
    }

    #[test]
    fn child_chain_propagates_through_nesting() {
        let root = CancellationToken::new();
        let mid = root.child();
        let leaf = mid.child();
        assert!(!leaf.is_cancelled());
        // The leaf's own double request escalates only itself.
        leaf.request_cancel();
        leaf.request_cancel();
        assert!(leaf.force_exit());
        assert!(!mid.is_cancelled());
        assert!(!mid.force_exit());
        // Ancestor single cancel reaches the deepest descendant gracefully; a
        // sibling that self-escalated keeps its own stronger state.
        let leaf2 = mid.child();
        root.request_cancel();
        assert!(leaf.is_cancelled());
        assert!(leaf2.is_cancelled());
        assert!(!leaf2.force_exit(), "one ancestor cancel stays graceful");
        assert!(leaf.force_exit(), "self-escalated leaf keeps its own state");
    }

    #[test]
    fn child_keeps_observing_after_the_parent_handle_is_dropped() {
        // The child keeps the ancestor chain alive through its own Arc, so a
        // dropped parent handle leaves its recorded state observable.
        let child = {
            let parent = CancellationToken::new();
            let child = parent.child();
            parent.request_cancel();
            child
        };
        assert!(child.is_cancelled());
        assert!(!child.force_exit());
    }

    #[test]
    fn window_depth_tiers_by_vram_and_skips_the_pit() {
        // Phase-4 W1: {8, 16} only - depth 12 measured -11% (kernel
        // quantization pit), so the ladder must skip it entirely.
        assert_eq!(window_depth_for_vram(None), 8, "CPU / probe failure = 8");
        assert_eq!(window_depth_for_vram(Some(0)), 8);
        assert_eq!(window_depth_for_vram(Some(6 * 1024 * 1024 * 1024)), 8);
        assert_eq!(window_depth_for_vram(Some(8 * 1024 * 1024 * 1024)), 8);
        assert_eq!(
            window_depth_for_vram(Some(12 * 1024 * 1024 * 1024)),
            16,
            "12 GiB opens the 16 tier"
        );
        assert_eq!(window_depth_for_vram(Some(24 * 1024 * 1024 * 1024)), 16);
    }

    #[test]
    fn json_validator_accepts_escaped_windows_paths_and_balanced_containers() {
        assert!(is_valid_json(
            r#"{"cache_root":"C:\\Users\\u\\AppData\\Local","n":1,"list":[true,false,null]}"#
        ));
        assert!(is_valid_json("  {\"a\":\"b\"}  \n"));
        assert!(is_valid_json("-12.5e+3"));
    }

    #[test]
    fn json_validator_rejects_unescaped_backslash_and_structural_errors() {
        // The BLK-1 regression: a raw Windows path inside a JSON string.
        assert!(!is_valid_json(r#"{"cache_root":"C:\Users\Space"}"#));
        assert!(!is_valid_json(r#"{"a":"\u12g0"}"#));
        assert!(!is_valid_json(r#"{"a":}"#));
        assert!(!is_valid_json(r#"{"a":1,}"#));
        assert!(!is_valid_json(r"[1,2"));
        assert!(!is_valid_json("{\"a\":\"b\"} trailing"));
        assert!(!is_valid_json(""));
    }

    #[test]
    fn gpu_resource_report_json_has_stable_schema_golden() {
        let report = GpuResourceReport {
            backend: "vulkan".to_owned(),
            vram_peak_bytes: Some(862_000_000),
            unified_memory_peak_bytes: None,
            load_time_ms: Some(1_234),
            first_token_ms: Some(2_345),
            output_tokens: 128,
            output_tokens_per_sec: 12.5,
            peak_memory_bytes: Some(456_000_000),
        };
        assert_eq!(
            report.to_json(),
            "{\"schema_version\":1,\"backend\":\"vulkan\",\"vram_peak_bytes\":862000000,\"unified_memory_peak_bytes\":null,\"load_time_ms\":1234,\"first_token_ms\":2345,\"output_tokens\":128,\"output_tokens_per_sec\":12.500,\"peak_memory_bytes\":456000000}"
        );
    }

    #[test]
    fn bbox_uses_half_open_1000_space() {
        assert!(
            NormalizedBbox {
                left: 0,
                top: 0,
                right: 999,
                bottom: 999
            }
            .is_valid()
        );
        assert!(
            !NormalizedBbox {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 999
            }
            .is_valid()
        );
        assert!(
            !NormalizedBbox {
                left: 10,
                top: 0,
                right: 10,
                bottom: 2
            }
            .is_valid()
        );
    }

    #[test]
    fn bbox_constructor_rejects_invalid_values() {
        assert_eq!(
            NormalizedBbox::new(0, 0, 999, 999),
            Ok(NormalizedBbox {
                left: 0,
                top: 0,
                right: 999,
                bottom: 999
            })
        );
        assert_eq!(
            NormalizedBbox::new(4, 2, 4, 8),
            Err(DomainError::InvalidBbox)
        );
    }

    #[test]
    fn app_error_keeps_three_part_user_context() {
        let error = AppError::new(ErrorKind::Input, 2, "invalid page").with_context(
            "page 0 is outside the document",
            "use a page number from 1 onward",
        );
        assert_eq!(error.code, 2);
        assert!(error.cause.is_some());
        assert!(error.suggestion.is_some());
    }

    #[test]
    fn error_kinds_have_stable_exit_codes() {
        let cases = [
            (ErrorKind::Success, 0),
            (ErrorKind::Usage, 2),
            (ErrorKind::Input, 3),
            (ErrorKind::Model, 4),
            (ErrorKind::Download, 5),
            (ErrorKind::Runtime, 6),
            (ErrorKind::Parse, 7),
            (ErrorKind::Integrity, 8),
            (ErrorKind::Output, 9),
            (ErrorKind::Partial, 10),
            (ErrorKind::Cancelled, 130),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.exit_code(), expected);
            assert_eq!(AppError::from_kind(kind, "failure").code, expected);
        }
        assert!(
            AppError::from_kind(ErrorKind::Parse, "failure")
                .validate()
                .is_ok()
        );
        assert_eq!(
            AppError::new(ErrorKind::Parse, 6, "failure").validate(),
            Err(DomainError::InvalidError)
        );
    }

    #[test]
    fn page_document_validation_rejects_unknown_schema_and_zero_page() {
        let document = PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION + 1,
            page_number: 1,
            blocks: vec![],
            termination: Termination::Eos,
            warnings: vec![],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidPage));

        let document = PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION,
            page_number: 0,
            blocks: vec![],
            termination: Termination::Eos,
            warnings: vec![],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidPage));
    }

    #[test]
    fn page_document_json_has_stable_schema_golden() {
        let document = PageDocument {
            schema_version: 1,
            page_number: 2,
            blocks: vec![
                PageBlock::Markdown {
                    content: "Hello\nworld".to_owned(),
                },
                PageBlock::Formula {
                    latex: "x^2".to_owned(),
                    display: true,
                },
                PageBlock::HtmlTable {
                    html: "<table><tr><td>A</td></tr></table>".to_owned(),
                },
                PageBlock::VisualRegion {
                    bbox: NormalizedBbox::new(1, 2, 10, 20).unwrap(),
                    source_order: 3,
                },
            ],
            termination: Termination::StopToken,
            warnings: vec![PageWarning::RenderScaled],
        };
        assert_eq!(
            document.to_json().unwrap(),
            "{\"schema_version\":1,\"page_number\":2,\"blocks\":[{\"type\":\"markdown\",\"content\":\"Hello\\nworld\"},{\"type\":\"formula\",\"latex\":\"x^2\",\"display\":true},{\"type\":\"html_table\",\"html\":\"<table><tr><td>A</td></tr></table>\"},{\"type\":\"visual_region\",\"bbox\":{\"left\":1,\"top\":2,\"right\":10,\"bottom\":20},\"source_order\":3}],\"termination\":\"stop_token\",\"warnings\":[\"render_scaled\"]}"
        );
        let encoded = document.to_json().unwrap();
        assert_eq!(PageDocument::from_json(&encoded).unwrap(), document);
    }

    #[test]
    fn page_document_empty_and_warning_variants_round_trip() {
        for (termination, warnings) in [
            (Termination::Eos, vec![]),
            (Termination::MaxTokens, vec![PageWarning::Truncated]),
            (
                Termination::Invalid,
                vec![PageWarning::RepetitionRemoved, PageWarning::RenderScaled],
            ),
            (Termination::RepetitionLoop, vec![PageWarning::Degenerated]),
        ] {
            let document = PageDocument {
                schema_version: 1,
                page_number: 1,
                blocks: vec![],
                termination,
                warnings,
            };
            let encoded = document.to_json().unwrap();
            assert_eq!(PageDocument::from_json(&encoded).unwrap(), document);
        }
    }

    #[test]
    fn page_document_validation_rejects_invalid_visual_region() {
        let document = PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION,
            page_number: 1,
            blocks: vec![PageBlock::VisualRegion {
                bbox: NormalizedBbox {
                    left: 10,
                    top: 10,
                    right: 10,
                    bottom: 20,
                },
                source_order: 0,
            }],
            termination: Termination::Eos,
            warnings: vec![],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidBbox));
    }
}
