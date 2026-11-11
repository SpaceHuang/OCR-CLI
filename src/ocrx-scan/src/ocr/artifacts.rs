//! Page artifact assembly types (plan §8.10).
//!
//! T037 copies the shared output record; the `TaskResponse`-driven assembly
//! (PNG crops, raw sidecar, page JSON) lands with the single-page OCR flow
//! (T062). The legacy runtime-consuming writers stay in `ocrx-ovisocr2`
//! until T087.

pub struct PageArtifactOutput {
    pub markdown: String,
    pub page_json: String,
    pub truncated: bool,
    pub degenerated: bool,
    pub warnings: Vec<String>,
    /// Number of think removals (complete blocks + stray tags), including
    /// silent empty-block strips (quality plan Q3.1).
    pub think_blocks_stripped: u32,
    /// Pre-crop normalized raw model output, preserved for the
    /// `page-NNNN.raw` sidecar required by requirement §7.4.
    pub raw_output: String,
}


