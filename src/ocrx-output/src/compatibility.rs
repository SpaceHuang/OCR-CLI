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

/// M4.2/D-63: the table-structure reread engine identity, from the
/// verified sidecar handshake. Stored on `OcrIdentity` as `Option` —
/// `None` serializes as an OMITTED field (OFF products stay byte-identical),
/// `Some` carries the full supply-chain identity (exact-equality compare on
/// resume).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StructureRereadIdentity {
    pub engine: String,
    /// D-72 split: model-dir freeze (manifest-keyed, `bae6e5f8`).
    pub models_revision: String,
    /// D-72 split: worker behavior generation (`proto2` = P0 no-table-found fix).
    pub protocol_revision: String,
    pub paddleocr: String,
    pub paddlex: String,
    pub paddlepaddle: String,
    /// Model-dir audit hashes from the hello frame (sorted map, D-63).
    #[serde(default)]
    pub models_sha256: std::collections::BTreeMap<String, String>,
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
    /// B4.2/D-25: effective region-reread switches and the layout model
    /// identity (id + asset hash) — compatibility fields on the
    /// `pixel_width_budget` precedent; `#[serde(default)]` keeps older
    /// checkpoints readable (off/empty = the default pipeline).
    #[serde(default)]
    pub table_reread: bool,
    #[serde(default)]
    pub formula_reread: bool,
    #[serde(default)]
    pub layout_model_id: String,
    #[serde(default)]
    pub layout_model_assets_sha256: String,
    /// R2.5/BLK-5: the reread prompt identity (empty = no reread prompt).
    /// A prompt switch must reprocess — the reread output rides this bytes.
    #[serde(default)]
    pub reread_prompt_version: String,
    /// R2.5/BLK-5: generation of the reread trigger/compare/normalization
    /// rules (one calibration round = +1; a normalization-version switch is
    /// a generation bump by contract). 0 = no reread rules.
    #[serde(default)]
    pub reread_rules_generation: u32,
    /// M4.2/D-63: the SLANet table-structure reread arm. `skip_serializing_if`
    /// is the OFF byte-compatibility contract: an OFF product must NOT carry
    /// the field at all (diff-verified by T2's byte-equality test).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_structure_reread: Option<StructureRereadIdentity>,
    /// M4.2 BLOCK-1: sha256 of the regions file consumed by the reread
    /// arms ("" = none). Omitted when empty — OFF products stay
    /// byte-identical; a regions change invalidates reread products.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub regions_sha256: String,
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
    /// B4.2/D-25 — see [`OcrIdentity`] (defaults = off/empty pipeline).
    pub table_reread: bool,
    pub formula_reread: bool,
    pub layout_model_id: String,
    pub layout_model_assets_sha256: String,
    /// R2.5/BLK-5 — see [`OcrIdentity`] (defaults = off/empty pipeline).
    pub reread_prompt_version: String,
    pub reread_rules_generation: u32,
    /// M4.2/D-63 — see [`OcrIdentity`] (None = the new flag is OFF).
    pub table_structure_reread: Option<StructureRereadIdentity>,
    /// M4.2 BLOCK-1 — see [`OcrIdentity`].
    pub regions_sha256: String,
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
        && ocr.table_reread == side.table_reread
        && ocr.formula_reread == side.formula_reread
        && ocr.layout_model_id == side.layout_model_id
        && ocr.layout_model_assets_sha256 == side.layout_model_assets_sha256
        && ocr.reread_prompt_version == side.reread_prompt_version
        && ocr.reread_rules_generation == side.reread_rules_generation
        && ocr.table_structure_reread == side.table_structure_reread
        && ocr.regions_sha256 == side.regions_sha256
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
            table_reread: ocr.table_reread,
            formula_reread: ocr.formula_reread,
            layout_model_id: ocr.layout_model_id.clone(),
            layout_model_assets_sha256: ocr.layout_model_assets_sha256.clone(),
            reread_prompt_version: ocr.reread_prompt_version.clone(),
            reread_rules_generation: ocr.reread_rules_generation,
            table_structure_reread: ocr.table_structure_reread.clone(),
            regions_sha256: ocr.regions_sha256.clone(),
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
            table_reread: false,
            formula_reread: false,
            layout_model_id: String::new(),
            layout_model_assets_sha256: String::new(),
            reread_prompt_version: String::new(),
            reread_rules_generation: 0,
            regions_sha256: String::new(),
            table_structure_reread: None,
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
                table_reread: false,
                formula_reread: false,
                layout_model_id: String::new(),
                layout_model_assets_sha256: String::new(),
                reread_prompt_version: String::new(),
                reread_rules_generation: 0,
                regions_sha256: String::new(),
                table_structure_reread: None,
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
    fn cross_model_checkpoint_reuse_is_vetoed_in_both_directions() {
        // G3/O3-T17 (iron rule 2): a checkpoint written under one model must
        // never be reused by a scan requesting the other — the structured
        // identity's model field (id/profile/revision/assets) carries the
        // veto in both directions, at both the static pre-compare and the
        // authoritative final compare.
        let ovisocr2 = identity("cpu", 8, "Q4_K_M");
        let mut ovisocr3 = identity("cpu", 8, "Q4_K_M");
        ovisocr3.model = ModelIdentity::new(
            ModelId::new("ovisocr3").unwrap(),
            ProfileId::new("quality").unwrap(),
            "rev-3",
            vec![asset("mmproj", "b"), asset("model", "a")],
        )
        .unwrap();
        ovisocr3.adapter = AdapterIdentity {
            adapter: AdapterId::new("ovisocr3").unwrap(),
            version: "0.1.0".to_owned(),
        };
        // ovisocr2 checkpoint x ovisocr3 request: vetoed.
        let statik3 = StaticInferenceIdentity::from_execution(&ovisocr3);
        assert!(
            !ocr_pre_compare(&stored(&ovisocr2), &statik3, &side()),
            "ovisocr2 checkpoint must not pass an ovisocr3 pre-compare"
        );
        assert!(!ocr_final_compare(&stored(&ovisocr2), &ovisocr3));
        // ovisocr3 checkpoint x ovisocr2 request: vetoed (reverse direction).
        let statik2 = StaticInferenceIdentity::from_execution(&ovisocr2);
        assert!(!ocr_pre_compare(&stored(&ovisocr3), &statik2, &side()));
        assert!(!ocr_final_compare(&stored(&ovisocr3), &ovisocr2));
        // Same-model reuse still passes (the veto is cross-model only).
        assert!(ocr_pre_compare(&stored(&ovisocr3), &statik3, &side()));
        assert!(ocr_final_compare(&stored(&ovisocr3), &ovisocr3));
        // Mixed-profile same-model reuse keeps its existing veto semantics.
        let mut ovisocr3_other_profile = ovisocr3.clone();
        ovisocr3_other_profile.model = ModelIdentity::new(
            ModelId::new("ovisocr3").unwrap(),
            ProfileId::new("quality").unwrap(),
            "rev-other",
            vec![asset("mmproj", "b"), asset("model", "a")],
        )
        .unwrap();
        assert!(!ocr_pre_compare(
            &stored(&ovisocr3),
            &StaticInferenceIdentity::from_execution(&ovisocr3_other_profile),
            &side()
        ));
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
    fn reread_switches_and_layout_model_identity_gate_reuse() {
        // B4.2/D-25: the effective reread switches and the layout model
        // identity (id + asset hash) are compatibility fields — mode = the
        // pixel_width_budget precedent. A stored reread-on page never
        // reuses against an off configuration and vice versa; a layout
        // model switch or asset-hash drift reprocesses too.
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        let mut stored_compat = stored(&current);
        let ocr = stored_compat.ocr.as_mut().unwrap();
        ocr.table_reread = true;
        ocr.formula_reread = true;
        ocr.layout_model_id = "pp-doclayout-v2".to_owned();
        ocr.layout_model_assets_sha256 = "hash-lm".to_owned();
        // Off side → mismatch (on→off switch must not reuse).
        assert!(!ocr_pre_compare(&stored_compat, &statik, &side()));
        let mut matching_side = side();
        matching_side.table_reread = true;
        matching_side.formula_reread = true;
        matching_side.layout_model_id = "pp-doclayout-v2".to_owned();
        matching_side.layout_model_assets_sha256 = "hash-lm".to_owned();
        assert!(ocr_pre_compare(&stored_compat, &statik, &matching_side));
        // Layout model switch → mismatch.
        let mut model_switch = matching_side.clone();
        model_switch.layout_model_id = "mineru-layout".to_owned();
        assert!(!ocr_pre_compare(&stored_compat, &statik, &model_switch));
        // Layout asset hash drift → mismatch.
        let mut hash_drift = matching_side.clone();
        hash_drift.layout_model_assets_sha256 = "other-hash".to_owned();
        assert!(!ocr_pre_compare(&stored_compat, &statik, &hash_drift));
        // Off stored / on side → mismatch (mixed configuration reprocess).
        let fresh = stored(&current);
        assert!(!ocr_pre_compare(&fresh, &statik, &matching_side));
    }

    /// R2.5 fixture: stored identity + matching side with the reread
    /// pipeline on at the frozen prompt/generation (`table-reread-v1`, 1).
    fn reread_on(current: &InferenceIdentity) -> (ScanCompatibility, ScanOcrSide) {
        let mut compat = stored(current);
        let ocr = compat.ocr.as_mut().unwrap();
        ocr.table_reread = true;
        ocr.reread_prompt_version = "table-reread-v1".to_owned();
        ocr.reread_rules_generation = 1;
        let mut matching = side();
        matching.table_reread = true;
        matching.reread_prompt_version = "table-reread-v1".to_owned();
        matching.reread_rules_generation = 1;
        (compat, matching)
    }

    #[test]
    fn reread_prompt_switch_never_reuses() {
        // R2.5/BLK-5 regression 1: a reread prompt change (v1 -> v2) is a
        // bytes-visible output change — the stored page must reprocess in
        // BOTH directions.
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        let (compat, matching) = reread_on(&current);
        assert!(
            ocr_pre_compare(&compat, &statik, &matching.clone()),
            "baseline on"
        );
        let mut switched = matching.clone();
        switched.reread_prompt_version = "table-reread-v2".to_owned();
        assert!(!ocr_pre_compare(&compat, &statik, &switched));
        // Reverse direction: stored v2 page, run downgrades to v1.
        let mut stored_v2 = compat;
        stored_v2.ocr.as_mut().unwrap().reread_prompt_version = "table-reread-v2".to_owned();
        assert!(!ocr_pre_compare(&stored_v2, &statik, &matching));
    }

    #[test]
    fn reread_rules_generation_bump_never_reuses() {
        // R2.5/BLK-5 regression 2: one calibration round = generation +1
        // (D-29/D-47 thresholds live in the rules); old artifacts carry the
        // old rules' verdicts and must reprocess.
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        let (compat, matching) = reread_on(&current);
        let mut recalibrated = matching.clone();
        recalibrated.reread_rules_generation = 2;
        assert!(!ocr_pre_compare(&compat, &statik, &recalibrated));
        // Reverse: stored under generation 2, run is still on 1.
        let mut stored_new = compat;
        stored_new.ocr.as_mut().unwrap().reread_rules_generation = 2;
        assert!(!ocr_pre_compare(&stored_new, &statik, &matching));
    }

    #[test]
    fn m42_structure_identity_generation_and_flag_paths() {
        // M4.2 D-63/D-64/D-68 EXECUTABLE GENERATION MATRIX (v1.6):
        // - 2 <-> 3: old-flag era vs first SLANet generation (historical);
        // - 3 <-> 4: SAME SLANet engine identity, ONLY the generation
        //   differs (D-68 ordering change) — bidirectional non-reuse. This
        //   is the core guard: the engine identity must not substitute for
        //   the generation gate;
        // - 2 <-> 4: old-flag gen 2 vs current SLANet gen 4, bidirectional.
        // Every mismatched pair fails; identical identity + generation
        // reuses.
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        let (compat, matching) = reread_on(&current); // generation-2 stored/run
        let engine = |models_revision: &str| StructureRereadIdentity {
            engine: "slanet-plus".to_owned(),
            models_revision: models_revision.to_owned(),
            protocol_revision: "proto2".to_owned(),
            paddleocr: "3.7.0".to_owned(),
            paddlex: "3.7.2".to_owned(),
            paddlepaddle: "3.4.0".to_owned(),
            models_sha256: std::collections::BTreeMap::new(),
        };

        // --- 2 <-> 3 (historical isolation) ---------------------------------
        let mut gen3_run = matching.clone();
        gen3_run.reread_rules_generation = 3;
        gen3_run.table_structure_reread = Some(engine("bae6e5f8"));
        assert!(!ocr_pre_compare(&compat, &statik, &gen3_run));
        let mut stored_gen3 = compat.clone();
        stored_gen3.ocr.as_mut().unwrap().reread_rules_generation = 3;
        stored_gen3.ocr.as_mut().unwrap().table_structure_reread = Some(engine("bae6e5f8"));
        assert!(!ocr_pre_compare(&stored_gen3, &statik, &matching));

        // --- 3 <-> 4: SAME engine identity, ONLY generation differs --------
        let mut stored_gen3_same_engine = compat.clone();
        stored_gen3_same_engine
            .ocr
            .as_mut()
            .unwrap()
            .reread_rules_generation = 3;
        stored_gen3_same_engine
            .ocr
            .as_mut()
            .unwrap()
            .table_structure_reread = Some(engine("bae6e5f8"));
        let mut gen4_run = matching.clone();
        gen4_run.reread_rules_generation = 4;
        gen4_run.table_structure_reread = Some(engine("bae6e5f8"));
        assert!(
            !ocr_pre_compare(&stored_gen3_same_engine, &statik, &gen4_run),
            "gen-3 empty-skip products must NEVER reuse under the gen-4 run"
        );
        // Reverse: stored gen 4, run is gen 3.
        assert!(
            !ocr_pre_compare(&stored_gen4_fixture(&current), &statik, &gen3_run),
            "gen-4 products must never reuse under a gen-3 run"
        );

        // --- 2 <-> 4 (old flag vs current SLANet) ---------------------------
        assert!(!ocr_pre_compare(&compat, &statik, &gen4_run));
        assert!(!ocr_pre_compare(
            &stored_gen4_fixture(&current),
            &statik,
            &matching
        ));

        // Engine-identity mismatch at the SAME generation: no reuse either.
        let mut stored_diff_engine = compat.clone();
        stored_diff_engine
            .ocr
            .as_mut()
            .unwrap()
            .reread_rules_generation = 4;
        stored_diff_engine
            .ocr
            .as_mut()
            .unwrap()
            .table_structure_reread = Some(engine("other-revision"));
        assert!(!ocr_pre_compare(&stored_diff_engine, &statik, &gen4_run));

        // Identical identity + generation: reuse stays possible.
        let mut stored_match = compat.clone();
        stored_match.ocr.as_mut().unwrap().reread_rules_generation = 4;
        stored_match.ocr.as_mut().unwrap().table_structure_reread = Some(engine("bae6e5f8"));
        assert!(ocr_pre_compare(&stored_match, &statik, &gen4_run));
    }

    /// A stored compatibility carrying the CURRENT generation (4) with the
    /// exact SLANet engine identity — the 3<->4 fixture's counterpart.
    fn stored_gen4_fixture(current: &InferenceIdentity) -> ScanCompatibility {
        let mut stored = stored(current);
        let ocr = stored.ocr.as_mut().unwrap();
        ocr.reread_rules_generation = 4;
        ocr.table_structure_reread = Some(StructureRereadIdentity {
            engine: "slanet-plus".to_owned(),
            models_revision: "bae6e5f8".to_owned(),
            protocol_revision: "proto2".to_owned(),
            paddleocr: "3.7.0".to_owned(),
            paddlex: "3.7.2".to_owned(),
            paddlepaddle: "3.4.0".to_owned(),
            models_sha256: std::collections::BTreeMap::new(),
        });
        stored
    }

    #[test]
    fn m42_regions_sha256_change_invalidates_reread_products() {
        // M4.2 BLOCK-1: swapping or regenerating the regions file changes
        // the recorded sha256 — reread products never reuse across that
        // change (either direction). Identical sha reuses.
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        let mut stored_base = stored(&current);
        {
            let ocr = stored_base.ocr.as_mut().unwrap();
            ocr.table_reread = true;
            ocr.reread_rules_generation = 2;
            ocr.regions_sha256 = "regions-sha-A".to_owned();
        }
        let mut run = side();
        run.table_reread = true;
        run.reread_rules_generation = 2;
        run.regions_sha256 = "regions-sha-A".to_owned();
        assert!(
            ocr_pre_compare(&stored_base, &statik, &run),
            "same regions reuses"
        );
        run.regions_sha256 = "regions-sha-B".to_owned();
        assert!(
            !ocr_pre_compare(&stored_base, &statik, &run),
            "swapped regions must reprocess"
        );
        // Reverse direction too.
        let mut stored_b = stored(&current);
        stored_b.ocr.as_mut().unwrap().regions_sha256 = "regions-sha-B".to_owned();
        let mut run_a = side();
        run_a.table_reread = true;
        run_a.reread_rules_generation = 2;
        run_a.regions_sha256 = "regions-sha-A".to_owned();
        assert!(!ocr_pre_compare(&stored_b, &statik, &run_a));
        // Pre-M4.2 products carry NO sha (empty); a run WITH one is a
        // different regions view -> reprocess (fail-closed).
        assert!(!ocr_pre_compare(
            &stored_base,
            &statik,
            &side_with_regions("x")
        ));
    }

    fn side_with_regions(sha: &str) -> ScanOcrSide {
        let mut s = side();
        s.table_reread = true;
        s.reread_rules_generation = 2;
        s.regions_sha256 = sha.to_owned();
        s
    }

    #[test]
    fn m42_off_omits_the_structure_field_serialization_contract() {
        // OFF byte-compatibility (D-63 skip_serializing_if): an OFF product
        // must NOT carry the field at all — byte-identical to pre-M4.2
        // output. ON products DO carry it.
        let current = identity("cpu", 8, "Q4_K_M");
        let off = stored(&current);
        let json = serde_json::to_string(&off).unwrap();
        assert!(
            !json.contains("table_structure_reread"),
            "OFF identity must omit the structure field: {json}"
        );
        let mut on = stored(&current);
        on.ocr.as_mut().unwrap().table_structure_reread = Some(StructureRereadIdentity {
            engine: "slanet-plus".to_owned(),
            models_revision: "bae6e5f8".to_owned(),
            protocol_revision: "proto2".to_owned(),
            paddleocr: "3.7.0".to_owned(),
            paddlex: "3.7.2".to_owned(),
            paddlepaddle: "3.4.0".to_owned(),
            models_sha256: std::collections::BTreeMap::new(),
        });
        let json_on = serde_json::to_string(&on).unwrap();
        assert!(json_on.contains("table_structure_reread"));
        // Round-trip preserves the identity (checkpoint resume contract).
        let parsed: ScanCompatibility = serde_json::from_str(&json_on).unwrap();
        assert_eq!(
            parsed.ocr.as_ref().unwrap().table_structure_reread,
            on.ocr.as_ref().unwrap().table_structure_reread
        );
        // Pre-M4.2 products (no field) deserialize to None.
        let parsed_off: ScanCompatibility = serde_json::from_str(&json).unwrap();
        assert!(
            parsed_off
                .ocr
                .as_ref()
                .unwrap()
                .table_structure_reread
                .is_none()
        );
    }

    #[test]
    fn normalization_version_switch_rides_the_rules_generation() {
        // R2.5/BLK-5 regression 3 (contract test): the D-47 normalization
        // has no dedicated identity field BY DESIGN — a normalization
        // version switch manifests as `reread_rules_generation` +1 (frozen
        // rule: any trigger/compare/normalization change bumps it). Same
        // generation = same normalization = reuse stays possible; a switch
        // with the bump honored never reuses.
        let current = identity("cpu", 8, "Q4_K_M");
        let statik = StaticInferenceIdentity::from_execution(&current);
        let (compat, matching) = reread_on(&current);
        // Sanity: identical generation (v1 == v1) reuses.
        assert!(ocr_pre_compare(&compat, &statik, &matching));
        // Normalization v1 -> v2 run with the contract honored.
        let mut v2_run = matching.clone();
        v2_run.reread_rules_generation += 1;
        assert!(!ocr_pre_compare(&compat, &statik, &v2_run));
    }

    #[test]
    fn old_identities_without_reread_fields_deserialize_with_defaults() {
        // Additive compat (pixel_width_budget precedent): a compatibility
        // written before B4.2 has no reread/layout fields; it deserializes
        // with off/empty defaults and stays reusable against an off run.
        let current = identity("cpu", 8, "Q4_K_M");
        let json = serde_json::to_string(&stored(&current)).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let ocr = value.get_mut("ocr").unwrap().as_object_mut().unwrap();
        for field in [
            "table_reread",
            "formula_reread",
            "layout_model_id",
            "layout_model_assets_sha256",
            "reread_prompt_version",
            "reread_rules_generation",
        ] {
            ocr.remove(field).unwrap();
        }
        let stripped = serde_json::to_string(&value).unwrap();
        let parsed: ScanCompatibility = serde_json::from_str(&stripped).unwrap();
        let ocr = parsed.ocr.as_ref().unwrap();
        assert!(!ocr.table_reread);
        assert!(!ocr.formula_reread);
        assert_eq!(ocr.layout_model_id, "");
        assert_eq!(ocr.layout_model_assets_sha256, "");
        assert_eq!(ocr.reread_prompt_version, "");
        assert_eq!(ocr.reread_rules_generation, 0);
        let statik = StaticInferenceIdentity::from_execution(&current);
        assert!(ocr_pre_compare(&parsed, &statik, &side()));
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
