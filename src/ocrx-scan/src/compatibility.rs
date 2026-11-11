//! Scan compatibility construction (plan §7.1, T064): the scan app builds
//! the structured identity from its own configuration plus the platform
//! identity — `batch_depth`/`decode_generation` are read from the
//! identity, never hardcoded, and there is no device-info re-derivation.

use ocrx_inference_api::InferenceIdentity;
use ocrx_output::{OcrIdentity, PdfIdentity, ScanCompatibility, ScanRoute};

/// Scan-side OCR configuration entering the compatibility identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOcrConfig {
    pub dpi: u32,
    pub max_image_tokens: u32,
    pub max_output_tokens: u32,
    pub context_size: u32,
    /// Frozen model pixel budget (§2.3: 2880 × 2880).
    pub pixel_width_budget: u32,
    pub pixel_height_budget: u32,
    pub page_parser_version: String,
}

impl Default for ScanOcrConfig {
    fn default() -> Self {
        Self {
            dpi: 300,
            max_image_tokens: 1024,
            max_output_tokens: crate::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS,
            context_size: 20_480,
            pixel_width_budget: 2880,
            pixel_height_budget: 2880,
            page_parser_version: "ovis-page-v1".to_owned(),
        }
    }
}

/// Builds the native/empty compatibility (dpi and every OCR-only field are
/// absent by construction — the frozen native matching scope).
#[must_use]
pub fn native_compatibility(
    input_sha256: &str,
    route: ScanRoute,
    pdf: PdfIdentity,
) -> ScanCompatibility {
    ScanCompatibility::native_or_empty(input_sha256, route, pdf)
}

/// Builds the OCR compatibility from the actual platform identity (T064:
/// the identity is the only source for batch depth/decode generation and
/// every platform field; the scan never fabricates them).
#[must_use]
pub fn ocr_compatibility(
    input_sha256: &str,
    pdf: PdfIdentity,
    identity: &InferenceIdentity,
    config: &ScanOcrConfig,
) -> ScanCompatibility {
    ScanCompatibility::ocr(
        input_sha256,
        pdf,
        OcrIdentity {
            inference: identity.clone(),
            prompt_id: crate::ocr::prompt::PROMPT_ID.to_owned(),
            prompt_version: crate::ocr::prompt::PROMPT_VERSION.to_owned(),
            prompt_schema_version: crate::ocr::prompt::PROMPT_SCHEMA_VERSION,
            dpi: config.dpi,
            max_image_tokens: config.max_image_tokens,
            max_output_tokens: config.max_output_tokens,
            context_size: config.context_size,
            batch_depth: identity.batch_depth,
            decode_generation: identity.decode_generation,
            pixel_width_budget: config.pixel_width_budget,
            pixel_height_budget: config.pixel_height_budget,
            page_parser_version: config.page_parser_version.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> InferenceIdentity {
        serde_json::from_str(
            r#"{"model":{"model":"ovisocr2","profile":"compact","source_revision":"rev","assets":[]},"runtime":{"runtime":"llama.cpp","wrapper_version":"0.1.161","sys_version":"0.1.161","upstream_revision":"d81235049384534c167caea52b85a694f6103d14"},"adapter":{"adapter":"ovisocr2","version":"0.1.0"},"backend":"cpu","decode_generation":2,"batch_depth":8,"display":{"display_name":"OvisOCR2","family":"Ovis","quantization":"Q4_K_M"}}"#,
        )
        .unwrap()
    }

    #[test]
    fn ocr_compatibility_reads_batch_and_generation_from_the_identity() {
        let config = ScanOcrConfig::default();
        let compatibility = ocr_compatibility("hash", pdf(), &identity(), &config);
        let ocr = compatibility.ocr.as_ref().unwrap();
        // Transparent fields come from the identity (no scan-side copies).
        assert_eq!(ocr.batch_depth, 8);
        assert_eq!(ocr.decode_generation, 2);
        assert_eq!(ocr.prompt_version, "ocr-markdown-v1");
        assert_eq!(ocr.prompt_schema_version, 1);
        assert_eq!(ocr.dpi, 300);
        assert_eq!(ocr.page_parser_version, "ovis-page-v1");
    }

    #[test]
    fn fallback_depth_lands_in_the_checkpoint_identity() {
        // GPU->CPU fallback: the identity already carries the re-selected
        // floor depth; the scan writes exactly what the platform produced.
        let mut fallback = identity();
        fallback.backend = ocrx_inference_api::BackendId::new("cpu").unwrap();
        fallback.batch_depth = 8;
        let compatibility = ocr_compatibility("hash", pdf(), &fallback, &ScanOcrConfig::default());
        assert_eq!(compatibility.ocr.as_ref().unwrap().batch_depth, 8);
        assert_eq!(
            compatibility.ocr.as_ref().unwrap().inference.backend.as_str(),
            "cpu"
        );
    }

    fn pdf() -> PdfIdentity {
        PdfIdentity {
            pdf_mode: "auto".to_owned(),
            quality_tier: "l2".to_owned(),
            classifier_version: "pdf-route-v1".to_owned(),
            extractor_version: "pdf-text-v1".to_owned(),
            platform: "windows".to_owned(),
        }
    }
}
