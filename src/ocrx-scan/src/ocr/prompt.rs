//! The frozen `ocr-markdown-v1` OCR prompt (plan §2.3/T006/T059).
//!
//! The bytes below are the single source of truth: leading newline, the
//! middle newline before "Preserve", and no trailing newline. Tests assert
//! byte equality so the model input can never drift during a refactor.

/// The frozen prompt (was `llama_mtmd::PROMPT`; moved to the scan app —
/// the OCR prompt is scan business, not model transport).
pub const OCR_MARKDOWN_V1: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.";

/// Prompt identity/version carried into checkpoint compatibility (§7.1).
pub const PROMPT_ID: &str = "ocr-markdown";
pub const PROMPT_VERSION: &str = "ocr-markdown-v1";
pub const PROMPT_SCHEMA_VERSION: u32 = 1;

/// Frozen generation defaults (plan §2.3: greedy, thinking off, D5.2 cap).
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_bytes_are_frozen_including_leading_and_middle_newlines() {
        // T006 golden: byte equality guards the model input.
        assert!(OCR_MARKDOWN_V1.starts_with('\n'));
        assert!(OCR_MARKDOWN_V1
            .contains("standard Markdown.\nPreserve the original text"));
        assert!(!OCR_MARKDOWN_V1.ends_with('\n'));
        assert_eq!(
            OCR_MARKDOWN_V1,
            "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing."
        );
    }

    #[test]
    fn prompt_identity_matches_the_checkpoint_contract() {
        assert_eq!(PROMPT_ID, "ocr-markdown");
        assert_eq!(PROMPT_VERSION, "ocr-markdown-v1");
        assert_eq!(PROMPT_SCHEMA_VERSION, 1);
        assert_eq!(DEFAULT_MAX_OUTPUT_TOKENS, 16_384);
    }
}
