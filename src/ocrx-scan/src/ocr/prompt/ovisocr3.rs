//! The OvisOCR3 prompt pack (plan O3-T05/O3-T15): the spike proved the
//! frozen family instruction bytes produce byte-identical outputs to the
//! official `document_markdown` variant, so this pack freezes the same
//! bytes and identity triplet values — the pack module is the per-model
//! freeze point (D-2 branch 1). Constants stay private; consumers read
//! through the pack registry.

use super::PromptPack;

/// The frozen prompt (same family bytes as OvisOCR2; see the spike record
/// `docs/develop/ovisocr3-integration-spike-2026-11.md` for the equivalence
/// evidence).
const OCR_MARKDOWN_V1: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.";

/// Prompt identity/version carried into checkpoint compatibility (§7.1).
const PROMPT_ID: &str = "ocr-markdown";
const PROMPT_VERSION: &str = "ocr-markdown-v1";
const PROMPT_SCHEMA_VERSION: u32 = 1;

/// The frozen OvisOCR3 pack.
pub static PACK: PromptPack = PromptPack {
    prompt: OCR_MARKDOWN_V1,
    id: PROMPT_ID,
    version: PROMPT_VERSION,
    schema_version: PROMPT_SCHEMA_VERSION,
};
