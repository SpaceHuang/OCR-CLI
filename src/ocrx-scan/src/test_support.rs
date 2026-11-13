//! Shared test fixtures for the scan crate (cfg(test) only).

#![cfg(test)]

use ocrx_inference_api::{
    AdapterId, AdapterIdentity, AssetIdentity, AssetRole, BackendId, InferenceIdentity, ModelId,
    ModelIdentity, ProfileId, RuntimeId, RuntimeIdentity, StaticInferenceIdentity,
};
use ocrx_output::{
    CheckpointFile, OcrIdentity, PdfIdentity, ScanCompatibility, ScanOcrSide, V5Disposition,
    V6Checkpoint, V6Status,
};

#[must_use]
pub fn pdf_identity() -> PdfIdentity {
    PdfIdentity {
        pdf_mode: "auto".to_owned(),
        quality_tier: "l2".to_owned(),
        classifier_version: "pdf-route-v1".to_owned(),
        extractor_version: "pdf-text-v1".to_owned(),
        platform: "windows".to_owned(),
    }
}

#[must_use]
pub fn side() -> ScanOcrSide {
    ScanOcrSide {
        prompt_id: "ocr-markdown".to_owned(),
        prompt_version: "ocr-markdown-v1".to_owned(),
        prompt_schema_version: 1,
        dpi: 300,
        max_image_tokens: 1024,
        max_output_tokens: 16_384,
        context_size: 20_480,
        pixel_width_budget: 2880,
        pixel_height_budget: 2880,
        page_parser_version: "ovis-page-v1".to_owned(),
    }
}

#[must_use]
pub fn identity() -> InferenceIdentity {
    InferenceIdentity {
        model: ModelIdentity::new(
            ModelId::new("ovisocr2").unwrap(),
            ProfileId::new("compact").unwrap(),
            "rev-1",
            vec![
                AssetIdentity {
                    role: AssetRole::new("mmproj").unwrap(),
                    sha256: "b".to_owned(),
                    byte_len: 10,
                },
                AssetIdentity {
                    role: AssetRole::new("model").unwrap(),
                    sha256: "a".to_owned(),
                    byte_len: 10,
                },
            ],
        )
        .unwrap(),
        runtime: RuntimeIdentity {
            runtime: RuntimeId::new("llama.cpp").unwrap(),
            wrapper_version: "0.1.161".to_owned(),
            sys_version: "0.1.161".to_owned(),
            upstream_revision: "d81235049384534c167caea52b85a694f6103d14".to_owned(),
        },
        adapter: AdapterIdentity {
            adapter: AdapterId::new("ovisocr2").unwrap(),
            version: "0.1.0".to_owned(),
        },
        backend: BackendId::new("cpu").unwrap(),
        decode_generation: 2,
        batch_depth: 8,
        display: ocrx_inference_api::ModelDisplayMeta {
            display_name: "OvisOCR2".to_owned(),
            family: "Ovis".to_owned(),
            quantization: "Q4_K_M".to_owned(),
        },
    }
}

#[must_use]
pub fn statik() -> StaticInferenceIdentity {
    StaticInferenceIdentity::from_execution(&identity())
}

#[must_use]
pub fn ocr_compatibility() -> ScanCompatibility {
    let current = identity();
    ScanCompatibility::ocr(
        "hash",
        pdf_identity(),
        OcrIdentity {
            inference: current,
            prompt_id: "ocr-markdown".to_owned(),
            prompt_version: "ocr-markdown-v1".to_owned(),
            prompt_schema_version: 1,
            dpi: 300,
            max_image_tokens: 1024,
            max_output_tokens: 16_384,
            context_size: 20_480,
            batch_depth: 8,
            decode_generation: 2,
            pixel_width_budget: 2880,
            pixel_height_budget: 2880,
            page_parser_version: "ovis-page-v1".to_owned(),
        },
    )
}

#[must_use]
pub fn v6_ocr(page: u32) -> V6Checkpoint {
    V6Checkpoint {
        page_number: page,
        status: V6Status::Complete,
        compatibility: ocr_compatibility(),
        output_sha256: String::new(),
        truncated: false,
        degenerated: false,
        attempts: 1,
        raw: None,
        warnings: vec![],
        think_blocks_stripped: 0,
        audit: None,
    }
}

#[must_use]
pub fn v5_ocr_complete(page: u32) -> V5Disposition {
    V5Disposition::Reprocess {
        page_number: page,
        reason: "v5 OCR complete page (no structured identity)",
    }
}

#[must_use]
pub fn v5_pending(page: u32) -> V5Disposition {
    V5Disposition::PendingOcr(V6Checkpoint {
        page_number: page,
        status: V6Status::PendingOcr,
        compatibility: ScanCompatibility::native_or_empty(
            "hash",
            ocrx_output::ScanRoute::NativeText,
            pdf_identity(),
        ),
        output_sha256: String::new(),
        truncated: false,
        degenerated: false,
        attempts: 1,
        raw: None,
        warnings: vec![],
        think_blocks_stripped: 0,
        audit: None,
    })
}

/// Silences the unused-type warning for the re-exported checkpoint enum.
#[allow(dead_code)]
pub fn _unused(_: &CheckpointFile) {}
