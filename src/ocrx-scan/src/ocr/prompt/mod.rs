//! Per-model OCR prompt packs (plan D-2/O3-T15).
//!
//! Each registered model owns a frozen pack module (`ovisocr2`/`ovisocr3`)
//! carrying the prompt bytes and the checkpoint identity triplet. The bytes
//! below are the single source of truth: leading newline, the middle newline
//! before "Preserve", and no trailing newline. Tests assert byte equality so
//! the model input can never drift during a refactor.
//!
//! The frozen prompt constants are PRIVATE to the pack modules — every
//! consumer reads through [`pack_for`], so no read point can bypass the
//! per-model registry (O3-T15: no residual global-constant read sites).

mod ovisocr2;
mod ovisocr3;

use ocrx_inference_api::ModelId;

/// Frozen generation defaults (plan §2.3: greedy, thinking off, D5.2 cap).
/// Both registered models share the cap (spike: the official OvisOCR3
/// reasoning settings match the frozen contract).
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;

/// One model's frozen prompt pack: bytes + checkpoint identity triplet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPack {
    pub prompt: &'static str,
    pub id: &'static str,
    pub version: &'static str,
    pub schema_version: u32,
}

/// Resolves the frozen pack for a registered model id.
///
/// # Panics
///
/// For an unregistered model id — callers validate through the catalog /
/// CLI gate first; an unregistered id here is a wiring bug that must fail
/// loudly.
#[must_use]
pub fn pack_for(model: &ModelId) -> &'static PromptPack {
    match model.as_str() {
        "ovisocr2" => &ovisocr2::PACK,
        "ovisocr3" => &ovisocr3::PACK,
        other => panic!("no frozen prompt pack for model {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_model_resolves_a_pack() {
        for model in ["ovisocr2", "ovisocr3"] {
            let pack = pack_for(&ModelId::new(model).unwrap());
            assert!(!pack.prompt.is_empty());
            assert!(!pack.id.is_empty());
            assert!(!pack.version.is_empty());
            assert!(pack.schema_version >= 1);
        }
    }

    #[test]
    #[should_panic(expected = "no frozen prompt pack")]
    fn unregistered_models_have_no_pack() {
        let _ = pack_for(&ModelId::new("qwen3.8").unwrap());
    }

    #[test]
    fn ovisocr2_pack_bytes_are_frozen_including_leading_and_middle_newlines() {
        // T006 golden carried over: byte equality guards the model input.
        let pack = pack_for(&ModelId::new("ovisocr2").unwrap());
        assert!(pack.prompt.starts_with('\n'));
        assert!(
            pack.prompt
                .contains("standard Markdown.\nPreserve the original text")
        );
        assert!(!pack.prompt.ends_with('\n'));
        assert_eq!(
            pack.prompt,
            "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing."
        );
    }

    #[test]
    fn pack_identities_match_the_checkpoint_contract() {
        let pack = pack_for(&ModelId::new("ovisocr2").unwrap());
        assert_eq!(pack.id, "ocr-markdown");
        assert_eq!(pack.version, "ocr-markdown-v1");
        assert_eq!(pack.schema_version, 1);
    }

    #[test]
    fn ovisocr3_pack_freezes_the_spike_proven_bytes_and_identity() {
        // O3-T05 spike: the OvisOCR3 pack freezes the same frozen bytes and
        // the same identity triplet values (outputs proven byte-identical
        // against the official prompt variant); the pack module is the
        // per-model freeze point.
        let ovisocr2 = pack_for(&ModelId::new("ovisocr2").unwrap());
        let ovisocr3 = pack_for(&ModelId::new("ovisocr3").unwrap());
        assert_eq!(ovisocr2.prompt, ovisocr3.prompt);
        assert_eq!(ovisocr2.id, ovisocr3.id);
        assert_eq!(ovisocr2.version, ovisocr3.version);
        assert_eq!(ovisocr2.schema_version, ovisocr3.schema_version);
    }
}
