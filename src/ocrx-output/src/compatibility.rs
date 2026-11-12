//! Structured scan compatibility identity (plan §7.1) and the two-phase
//! OCR reuse protocol (RBLK-2/4/6/7/8).
//!
//! The legacy string `CompatibilityKey` and its prefix-matching rules stay
//! in `lib.rs` until T088; the new identity replaces them with named
//! fields. `InferenceIdentity` (including `display`) is stored verbatim for
//! audit, but every matching function explicitly excludes `display` —
//! report-only fields never trigger reprocessing.

use ocrx_inference_api::{InferenceIdentity, StaticInferenceIdentity};
use serde::{Deserialize, Serialize};

/// Which pipeline produced a page's artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanRoute {
    NativeText,
    Empty,
    Ocr,
}

impl ScanRoute {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeText => "native_text",
            Self::Empty => "empty",
            Self::Ocr => "ocr",
        }
    }
}

/// PDF-side extraction identity (shared by every route).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfIdentity {
    pub pdf_mode: String,
    pub quality_tier: String,
    pub classifier_version: String,
    pub extractor_version: String,
    pub platform: String,
}

/// OCR-side execution identity. `inference` is the full platform identity
/// (stored for audit); matching consumes only the execution fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrIdentity {
    pub inference: InferenceIdentity,
    pub prompt_id: String,
    pub prompt_version: String,
    /// Independent numeric field; the display layer reads it directly
    /// (never parsed out of `prompt_version`).
    pub prompt_schema_version: u32,
    pub dpi: u32,
    pub max_image_tokens: u32,
    pub max_output_tokens: u32,
    pub context_size: u32,
    /// Actually effective batch depth (transparent from the identity).
    pub batch_depth: usize,
    /// Decode generation (transparent from the identity).
    pub decode_generation: u32,
    pub pixel_width_budget: u32,
    pub pixel_height_budget: u32,
    pub page_parser_version: String,
}

/// Versioned structured scan compatibility (`schema_version = 1`).
///
/// Serialization is deterministic (declaration-ordered fields, assets
/// role-sorted inside `InferenceIdentity`), so identical configurations
/// produce identical bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCompatibility {
    pub schema_version: u32,
    pub input_sha256: String,
    pub route: ScanRoute,
    pub pdf: PdfIdentity,
    /// `None` for native/empty routes.
    pub ocr: Option<OcrIdentity>,
}

impl ScanCompatibility {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Builds a native/empty identity (dpi and every OCR-only field are
    /// absent by construction).
    #[must_use]
    pub fn native_or_empty(
        input_sha256: impl Into<String>,
        route: ScanRoute,
        pdf: PdfIdentity,
    ) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            input_sha256: input_sha256.into(),
            route,
            pdf,
            ocr: None,
        }
    }

    /// Builds an OCR identity. `batch_depth`/`decode_generation` are read
    /// from the platform identity — never hardcoded by the scan.
    #[must_use]
    pub fn ocr(input_sha256: impl Into<String>, pdf: PdfIdentity, ocr: OcrIdentity) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            input_sha256: input_sha256.into(),
            route: ScanRoute::Ocr,
            pdf,
            ocr: Some(ocr),
        }
    }

    /// Native/empty matching (plan §7.1): exact input/route/PDF fields.
    /// dpi, batch depth, profile, device, token, and context are OCR-only
    /// and never affect native/empty reuse.
    #[must_use]
    pub fn matches_native(&self, expected: &Self) -> bool {
        self.schema_version == expected.schema_version
            && self.input_sha256 == expected.input_sha256
            && self.route == expected.route
            && self.pdf == expected.pdf
    }

    /// Route guard: an OCR expectation never matches a native checkpoint
    /// and vice versa (the legacy cross-route double-collection bug).
    #[must_use]
    pub fn routes_agree(&self, expected: &Self) -> bool {
        self.route == expected.route
    }
}

/// Scan-side OCR fields the platform cannot know (prompt/render/parser
/// versions and generation configuration). Both comparison phases check
/// them; they are constant within one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOcrSide {
    pub prompt_id: String,
    pub prompt_version: String,
    pub prompt_schema_version: u32,
    pub dpi: u32,
    pub max_image_tokens: u32,
    pub max_output_tokens: u32,
    pub context_size: u32,
    pub pixel_width_budget: u32,
    pub pixel_height_budget: u32,
    pub page_parser_version: String,
}

fn scan_side_matches(ocr: &OcrIdentity, side: &ScanOcrSide) -> bool {
    ocr.prompt_id == side.prompt_id
        && ocr.prompt_version == side.prompt_version
        && ocr.prompt_schema_version == side.prompt_schema_version
        && ocr.dpi == side.dpi
        && ocr.max_image_tokens == side.max_image_tokens
        && ocr.max_output_tokens == side.max_output_tokens
        && ocr.context_size == side.context_size
        && ocr.pixel_width_budget == side.pixel_width_budget
        && ocr.pixel_height_budget == side.pixel_height_budget
        && ocr.page_parser_version == side.page_parser_version
}

/// Phase 1 — static pre-comparison (RBLK-2/RBLK-4, prepare-free fast path).
///
/// Compares the stored OCR identity's execution fields (excluding the
/// actual backend, the actual batch depth, and the report-only `display`)
/// against the read-only [`StaticInferenceIdentity`] plus the scan-side
/// fields. Result only decides whether prepare can be skipped — the phase-2
/// final comparison is authoritative.
#[must_use]
pub fn ocr_pre_compare(
    stored: &ScanCompatibility,
    statik: &StaticInferenceIdentity,
    side: &ScanOcrSide,
) -> bool {
    let Some(ocr) = &stored.ocr else {
        return false;
    };
    if !scan_side_matches(ocr, side) {
        return false;
    }
    // decode generation is part of both the static identity and the stored
    // transparent field; both must agree.
    ocr.decode_generation == statik.decode_generation
        && ocr.decode_generation == ocr.inference.decode_generation
        && ocr.inference.model == statik.model
        && ocr.inference.runtime == statik.runtime
        && ocr.inference.adapter == statik.adapter
}

/// Phase 2 — final comparison (RBLK-6, authoritative; only run when this
/// document has pages to decode and a prepared session identity).
///
/// Full execution-field comparison against the identity of the session
/// about to decode: model (resolved profile + per-asset hashes), runtime
/// (wrapper/sys/upstream), adapter, decode generation, actual backend, and
/// actual batch depth. `display` is explicitly excluded (RBLK-7): display
/// copy or quantization-label changes never reprocess.
#[must_use]
pub fn ocr_final_compare(stored: &ScanCompatibility, identity: &InferenceIdentity) -> bool {
    let Some(ocr) = &stored.ocr else {
        return false;
    };
    let statik = StaticInferenceIdentity::from_execution(identity);
    // Static subset first (model/runtime/adapter/generation) …
    ocr_pre_compare(
        stored,
        &statik,
        &ScanOcrSide {
            prompt_id: ocr.prompt_id.clone(),
            prompt_version: ocr.prompt_version.clone(),
            prompt_schema_version: ocr.prompt_schema_version,
            dpi: ocr.dpi,
            max_image_tokens: ocr.max_image_tokens,
            max_output_tokens: ocr.max_output_tokens,
            context_size: ocr.context_size,
            pixel_width_budget: ocr.pixel_width_budget,
            pixel_height_budget: ocr.pixel_height_budget,
            page_parser_version: ocr.page_parser_version.clone(),
        },
    ) && ocr.inference.backend == identity.backend
        && ocr.inference.batch_depth == identity.batch_depth
        // RBLK-7 explicit exclusion: display may differ freely.
        && true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrx_inference_api::{
        AdapterId, AdapterIdentity, AssetIdentity, AssetRole, BackendId, ModelId, ModelIdentity,
        ProfileId, RuntimeId, RuntimeIdentity,
    };

    fn asset(role: &str, sha: &str) -> AssetIdentity {
        AssetIdentity {
            role: AssetRole::new(role).unwrap(),
            sha256: sha.to_owned(),
            byte_len: 10,
        }
    }

    fn runtime() -> RuntimeIdentity {
        RuntimeIdentity {
            runtime: RuntimeId::new("llama.cpp").unwrap(),
            wrapper_version: "0.1.161".to_owned(),
            sys_version: "0.1.161".to_owned(),
            upstream_revision: "d81235049384534c167caea52b85a694f6103d14".to_owned(),
        }
    }

    fn identity(backend: &str, batch_depth: usize, quant: &str) -> InferenceIdentity {
        InferenceIdentity {
            model: ModelIdentity::new(
                ModelId::new("ovisocr2").unwrap(),
                ProfileId::new("compact").unwrap(),
                "rev-1",
                vec![asset("mmproj", "b"), asset("model", "a")],
            )
            .unwrap(),
            runtime: runtime(),
            adapter: AdapterIdentity {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".to_owned(),
            },
            backend: BackendId::new(backend).unwrap(),
            decode_generation: 2,
            batch_depth,
            display: ModelDisplay {
                display_name: "OvisOCR2".to_owned(),
                family: "Ovis".to_owned(),
                quantization: quant.to_owned(),
            },
        }
    }

    // Local alias so the test stays readable (the real type is
    // ocrx_inference_api::ModelDisplayMeta).
    type ModelDisplay = ocrx_inference_api::ModelDisplayMeta;

    fn side() -> ScanOcrSide {
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

    fn stored(identity: &InferenceIdentity) -> ScanCompatibility {
        ScanCompatibility::ocr(
            "input-hash",
            PdfIdentity {
                pdf_mode: "auto".to_owned(),
                quality_tier: "l2".to_owned(),
                classifier_version: "pdf-route-v1".to_owned(),
                extractor_version: "pdf-text-v1".to_owned(),
                platform: "windows".to_owned(),
            },
            OcrIdentity {
                inference: identity.clone(),
                prompt_id: "ocr-markdown".to_owned(),
                prompt_version: "ocr-markdown-v1".to_owned(),
                prompt_schema_version: 1,
                dpi: 300,
                max_image_tokens: 1024,
                max_output_tokens: 16_384,
                context_size: 20_480,
                batch_depth: identity.batch_depth,
                decode_generation: identity.decode_generation,
                pixel_width_budget: 2880,
                pixel_height_budget: 2880,
                page_parser_version: "ovis-page-v1".to_owned(),
            },
        )
    }

    #[test]
    fn native_matching_ignores_ocr_only_changes_and_routes_never_cross() {
        let pdf = |platform: &str| PdfIdentity {
            pdf_mode: "auto".to_owned(),
            quality_tier: "l2".to_owned(),
            classifier_version: "pdf-route-v1".to_owned(),
            extractor_version: "pdf-text-v1".to_owned(),
            platform: platform.to_owned(),
        };
        let native =
            ScanCompatibility::native_or_empty("in", ScanRoute::NativeText, pdf("windows"));
        assert!(native.matches_native(&native));
        // OCR-only fields do not exist on native identities at all.
        let empty = ScanCompatibility::native_or_empty("in", ScanRoute::Empty, pdf("windows"));
        assert!(!native.matches_native(&empty), "route must not cross");
        // A platform change invalidates native reuse (M-1 rule).
        let mac = ScanCompatibility::native_or_empty("in", ScanRoute::NativeText, pdf("macos"));
        assert!(!native.matches_native(&mac));
    }

    #[test]
    fn pre_compare_passes_when_the_static_identity_agrees() {
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        assert!(ocr_pre_compare(&stored(&current), &statik, &side()));
    }

    #[test]
    fn pre_compare_ignores_actual_backend_and_batch_depth() {
        // Stored page was written by a CPU/depth-8 run; the static identity
        // of a would-be GPU/16 run agrees on every static field → the
        // pre-comparison passes (actual values are final-comparison-only).
        let stored_page = identity("cpu", 8, "Q4_K_M");
        let mut gpu_static_model = stored_page.model.clone();
        gpu_static_model.profile = ProfileId::new("compact").unwrap();
        let statik = StaticInferenceIdentity {
            model: gpu_static_model,
            runtime: runtime(),
            adapter: stored_page.adapter.clone(),
            decode_generation: 2,
        };
        assert!(ocr_pre_compare(&stored(&stored_page), &statik, &side()));
    }

    #[test]
    fn pre_compare_fails_on_any_static_execution_change() {
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        // Profile drift (auto re-resolution).
        let mut drifted = identity("cpu", 8, "Q4_K_M");
        drifted.model.profile = ProfileId::new("balanced").unwrap();
        assert!(!ocr_pre_compare(&stored(&drifted), &statik, &side()));
        // Asset hash change.
        let mut asset_drift = identity("cpu", 8, "Q4_K_M");
        asset_drift.model.assets[0].sha256 = "z".to_owned();
        assert!(!ocr_pre_compare(&stored(&asset_drift), &statik, &side()));
        // Runtime binding change.
        let mut runtime_drift = identity("cpu", 8, "Q4_K_M");
        runtime_drift.runtime.wrapper_version = "0.1.160".to_owned();
        assert!(!ocr_pre_compare(&stored(&runtime_drift), &statik, &side()));
        // Adapter change.
        let mut adapter_drift = identity("cpu", 8, "Q4_K_M");
        adapter_drift.adapter.version = "0.2.0".to_owned();
        assert!(!ocr_pre_compare(&stored(&adapter_drift), &statik, &side()));
        // Decode generation change.
        let mut generation_drift = identity("cpu", 8, "Q4_K_M");
        generation_drift.decode_generation = 3;
        assert!(!ocr_pre_compare(
            &stored(&generation_drift),
            &statik,
            &side()
        ));
        // Scan-side change (dpi).
        let mut dpi_side = side();
        dpi_side.dpi = 200;
        assert!(!ocr_pre_compare(&stored(&current), &statik, &dpi_side));
    }

    #[test]
    fn final_compare_requires_the_actual_backend_and_batch_depth() {
        let current = identity("cpu", 8, "Q4_K_M");
        // Same actual execution → reuse.
        assert!(ocr_final_compare(&stored(&current), &current));
        // Different actual backend (GPU run while stored was CPU) → reprocess.
        let gpu = identity("cuda", 16, "Q4_K_M");
        assert!(!ocr_final_compare(&stored(&current), &gpu));
        // Different actual batch depth → reprocess (cross-depth rule).
        let depth16 = identity("cpu", 16, "Q4_K_M");
        assert!(!ocr_final_compare(&stored(&current), &depth16));
    }

    #[test]
    fn display_changes_never_trigger_reprocessing_rblk7() {
        // Only the display metadata differs (display name copy + quantization
        // label): stored page stays reusable — the discriminating test.
        let current = identity("cpu", 8, "Q4_K_M");
        let mut relabeled = identity("cpu", 8, "Q4_K_M");
        relabeled.display.display_name = "OvisOCR2 (renamed)".to_owned();
        relabeled.display.quantization = "Q4_K_X".to_owned();
        assert!(ocr_final_compare(&stored(&current), &relabeled));
        assert!(ocr_final_compare(&stored(&relabeled), &current));
    }

    #[test]
    fn serialization_is_deterministic_and_stores_display_for_audit() {
        let current = identity("cpu", 8, "Q4_K_M");
        let compatibility = stored(&current);
        let json = serde_json::to_string(&compatibility).unwrap();
        assert_eq!(json, serde_json::to_string(&compatibility).unwrap());
        // display stored verbatim (audit) …
        assert!(json.contains("\"display\":{\"display_name\":\"OvisOCR2\""));
        // … with the transparent batch/decode fields …
        assert!(json.contains("\"batch_depth\":8"));
        assert!(json.contains("\"decode_generation\":2"));
        // … and round-trips.
        let round: ScanCompatibility = serde_json::from_str(&json).unwrap();
        assert_eq!(round, compatibility);
    }

    #[test]
    fn native_identities_do_not_match_ocr_expectations() {
        let current = identity("cpu", 8, "Q4_K_M");
        let ocr_compat = stored(&current);
        let pdf = PdfIdentity {
            pdf_mode: "auto".to_owned(),
            quality_tier: "l2".to_owned(),
            classifier_version: "pdf-route-v1".to_owned(),
            extractor_version: "pdf-text-v1".to_owned(),
            platform: "windows".to_owned(),
        };
        let native_compat =
            ScanCompatibility::native_or_empty("input-hash", ScanRoute::NativeText, pdf);
        assert!(!ocr_final_compare(&native_compat, &current));
        assert!(native_compat.routes_agree(&native_compat));
        assert!(!ocr_compat.routes_agree(&native_compat));
    }
}
