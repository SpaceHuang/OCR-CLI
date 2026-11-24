//! Scan compatibility construction (plan §7.1, T064): the scan app builds
//! the structured identity from its own configuration plus the platform
//! identity — `batch_depth`/`decode_generation` are read from the
//! identity, never hardcoded, and there is no device-info re-derivation.

use ocrx_inference_api::InferenceIdentity;
use ocrx_output::{OcrIdentity, PdfIdentity, ScanCompatibility, ScanRoute};

/// Scan-side OCR configuration entering the compatibility identity.
/// O3-T15: the prompt identity triplet comes from the selected model's
/// frozen pack (carried in explicitly — no global-constant read sites).
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
    pub prompt_id: String,
    pub prompt_version: String,
    pub prompt_schema_version: u32,
    /// B4.2/D-25: effective region-reread switches + layout model identity
    /// (id + asset hash) entering the compatibility identity. Defaults =
    /// off/empty (no region pipeline).
    pub table_reread: bool,
    pub formula_reread: bool,
    pub layout_model_id: String,
    pub layout_model_assets_sha256: String,
    /// R2.5/BLK-5: reread execution identity (prompt version + rules
    /// generation). Defaults = empty/0 (the off pipeline); the reread-on
    /// caller sets them from the frozen constants.
    pub reread_prompt_version: String,
    pub reread_rules_generation: u32,
    /// M4.2/D-63: the table-structure reread arm identity. Defaults = None
    /// (the new flag is OFF; the field serializes as OMITTED — the OFF
    /// byte-compatibility contract).
    pub table_structure_reread: Option<ocrx_output::StructureRereadIdentity>,
    /// M4.2 BLOCK-1: sha256 of the regions file actually consumed ("" = no
    /// reread arm). A regions swap/regeneration changes the hash and
    /// invalidates reread products on resume.
    pub regions_sha256: String,
}

impl Default for ScanOcrConfig {
    fn default() -> Self {
        // The default face stays the historical (default-model) pack.
        let pack = crate::ocr::prompt::pack_for(
            &ocrx_inference_api::ModelId::new("ovisocr2").expect("built-in id is valid"),
        );
        Self {
            dpi: 300,
            max_image_tokens: 1024,
            max_output_tokens: crate::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS,
            context_size: 20_480,
            pixel_width_budget: 2880,
            pixel_height_budget: 2880,
            page_parser_version: "ovis-page-v1".to_owned(),
            prompt_id: pack.id.to_owned(),
            prompt_version: pack.version.to_owned(),
            prompt_schema_version: pack.schema_version,
            table_reread: false,
            formula_reread: false,
            layout_model_id: String::new(),
            layout_model_assets_sha256: String::new(),
            reread_prompt_version: String::new(),
            reread_rules_generation: 0,
            table_structure_reread: None,
            regions_sha256: String::new(),
        }
    }
}

/// R2.5: the per-document reread configuration entering the identity (the
/// CLI resolves the layout model through the R2.0 manifest chain and pins
/// the asset hash; the scan consumes the resolved triple verbatim).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RereadOptions {
    pub layout_model_id: String,
    pub layout_assets_sha256: String,
    /// Sidecar regions JSON for this document (R1 route A file form).
    pub regions_path: std::path::PathBuf,
}

impl ScanOcrConfig {
    /// Fills the reread identity fields from the effective options — the
    /// single source for BOTH the stored checkpoint identity and the
    /// resume pre-compare side (they must never drift).
    #[must_use]
    pub fn with_reread(mut self, reread: Option<&RereadOptions>) -> Self {
        match reread {
            Some(reread) => {
                self.table_reread = true;
                self.layout_model_id.clone_from(&reread.layout_model_id);
                self.layout_model_assets_sha256
                    .clone_from(&reread.layout_assets_sha256);
                crate::ocr::prompt::reread::TABLE_REREAD_PROMPT_VERSION
                    .clone_into(&mut self.reread_prompt_version);
                // M4.2/D-63: the old flag records the VLM-era generation (2);
                // the SLANet arm (generation 3) comes from
                // with_structure_reread only.
                self.reread_rules_generation = crate::reread::VLM_REREAD_RULES_GENERATION;
            }
            None => {
                // Defaults already are the off pipeline (empty/0); assert
                // the invariant instead of trusting the constructor.
                debug_assert!(!self.table_reread && self.reread_rules_generation == 0);
            }
        }
        self
    }

    /// M4.2/D-63: fills the table-structure arm from the resolved layout
    /// options and the verified handshake identity — the same single-source
    /// contract as [`Self::with_reread`] (checkpoint identity and resume
    /// pre-compare side never drift). Layout identity fields ride the same
    /// slots for audit; `table_reread` stays FALSE (old flag off) and the
    /// reread prompt stays empty.
    #[must_use]
    pub fn with_structure_reread(
        mut self,
        layout: Option<&RereadOptions>,
        engine: Option<&ocrx_output::StructureRereadIdentity>,
    ) -> Self {
        if let Some(layout) = layout {
            self.layout_model_id.clone_from(&layout.layout_model_id);
            self.layout_model_assets_sha256
                .clone_from(&layout.layout_assets_sha256);
        }
        if engine.is_some() {
            self.reread_rules_generation = crate::reread::REREAD_RULES_GENERATION;
        }
        self.table_structure_reread = engine.cloned();
        self
    }

    /// M4.2 BLOCK-1: pins the consumed regions-file identity into the
    /// compatibility (resume isolation across regions changes).
    #[must_use]
    pub fn with_regions_identity(mut self, regions_sha256: &str) -> Self {
        regions_sha256.clone_into(&mut self.regions_sha256);
        self
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
            prompt_id: config.prompt_id.clone(),
            prompt_version: config.prompt_version.clone(),
            prompt_schema_version: config.prompt_schema_version,
            dpi: config.dpi,
            max_image_tokens: config.max_image_tokens,
            max_output_tokens: config.max_output_tokens,
            context_size: config.context_size,
            batch_depth: identity.batch_depth,
            decode_generation: identity.decode_generation,
            pixel_width_budget: config.pixel_width_budget,
            pixel_height_budget: config.pixel_height_budget,
            page_parser_version: config.page_parser_version.clone(),
            table_reread: config.table_reread,
            formula_reread: config.formula_reread,
            layout_model_id: config.layout_model_id.clone(),
            layout_model_assets_sha256: config.layout_model_assets_sha256.clone(),
            reread_prompt_version: config.reread_prompt_version.clone(),
            reread_rules_generation: config.reread_rules_generation,
            table_structure_reread: config.table_structure_reread.clone(),
            regions_sha256: config.regions_sha256.clone(),
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
    fn reread_execution_identity_rides_the_config_into_the_checkpoint() {
        // R2.5/BLK-5: the on configuration carries the frozen prompt version
        // and the current rules generation into the stored identity; the
        // default (off) config stays empty/0.
        let mut config = ScanOcrConfig {
            reread_prompt_version: crate::ocr::prompt::reread::TABLE_REREAD_PROMPT_VERSION
                .to_owned(),
            // M4.2/D-63: an old-flag (VLM F1-F5) config carries the VLM-era
            // generation; the SLANet generation 3 comes only from
            // with_structure_reread.
            reread_rules_generation: crate::reread::VLM_REREAD_RULES_GENERATION,
            ..ScanOcrConfig::default()
        };
        config.table_reread = true;
        let compatibility = ocr_compatibility("hash", pdf(), &identity(), &config);
        let ocr = compatibility.ocr.as_ref().unwrap();
        assert_eq!(ocr.reread_prompt_version, "table-reread-v1");
        assert_eq!(ocr.reread_rules_generation, 2);
        assert!(ocr.table_reread);
        let off = ocr_compatibility("hash", pdf(), &identity(), &ScanOcrConfig::default());
        let off_ocr = off.ocr.as_ref().unwrap();
        assert_eq!(off_ocr.reread_prompt_version, "");
        assert_eq!(off_ocr.reread_rules_generation, 0);
    }

    #[test]
    fn with_reread_sets_the_frozen_identity_and_off_stays_off() {
        // R2.5: the reread-on configuration carries the frozen prompt
        // version + generation + layout identity; the off configuration
        // (None) keeps the empty pipeline even if the caller mutated
        // unrelated fields.
        let on = ScanOcrConfig::default().with_reread(Some(&RereadOptions {
            layout_model_id: "pp-doclayout-v2".to_owned(),
            layout_assets_sha256: "hash-lm".to_owned(),
            regions_path: std::path::PathBuf::from("regions.json"),
        }));
        assert!(on.table_reread);
        assert_eq!(on.layout_model_id, "pp-doclayout-v2");
        assert_eq!(on.layout_model_assets_sha256, "hash-lm");
        assert_eq!(on.reread_prompt_version, "table-reread-v1");
        assert_eq!(on.reread_rules_generation, 2);
        let off = ScanOcrConfig::default().with_reread(None);
        assert!(!off.table_reread);
        assert_eq!(off.reread_prompt_version, "");
        assert_eq!(off.reread_rules_generation, 0);
        // The stored identity and the pre-compare side derive from the same
        // mapping, so a stored on-identity matches its own run's side.
        let identity = serde_json::from_str::<InferenceIdentity>(
            r#"{"model":{"model":"ovisocr2","profile":"compact","source_revision":"rev","assets":[]},"runtime":{"runtime":"llama.cpp","wrapper_version":"0.1.161","sys_version":"0.1.161","upstream_revision":"d81235049384534c167caea52b85a694f6103d14"},"adapter":{"adapter":"ovisocr2","version":"0.1.0"},"backend":"cpu","decode_generation":2,"batch_depth":8,"display":{"display_name":"OvisOCR2","family":"Ovis","quantization":"Q4_K_M"}}"#,
        )
        .unwrap();
        let stored = ocr_compatibility("hash", pdf(), &identity, &on);
        let stored_ocr = stored.ocr.as_ref().unwrap();
        assert_eq!(stored_ocr.reread_prompt_version, on.reread_prompt_version);
        assert_eq!(
            stored_ocr.reread_rules_generation,
            on.reread_rules_generation
        );
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
            compatibility
                .ocr
                .as_ref()
                .unwrap()
                .inference
                .backend
                .as_str(),
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
