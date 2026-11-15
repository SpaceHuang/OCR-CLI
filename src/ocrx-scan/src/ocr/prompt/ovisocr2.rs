//! The OvisOCR2 prompt pack — the historical frozen bytes migrated verbatim
//! (plan O3-T15: the migration is byte-identity-asserted; these constants
//! are private to this module and only escape through the pack registry).

use super::PromptPack;

/// The frozen prompt (was `llama_mtmd::PROMPT`; moved to the scan app —
/// the OCR prompt is scan business, not model transport).
const OCR_MARKDOWN_V1: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.";

/// Prompt identity/version carried into checkpoint compatibility (§7.1).
const PROMPT_ID: &str = "ocr-markdown";
const PROMPT_VERSION: &str = "ocr-markdown-v1";
const PROMPT_SCHEMA_VERSION: u32 = 1;

/// The frozen OvisOCR2 pack.
pub static PACK: PromptPack = PromptPack {
    prompt: OCR_MARKDOWN_V1,
    id: PROMPT_ID,
    version: PROMPT_VERSION,
    schema_version: PROMPT_SCHEMA_VERSION,
};
