//! The frozen table-reread prompt (m4-table-reopen plan R2.3/D-36).
//!
//! The reread prompt is NOT a per-model OCR pack: it is the M4 pipeline's
//! frozen input for any registered model (v1 ships exactly one). The bytes
//! are the single source of truth; the test asserts byte equality so the
//! model input can never drift during a refactor. Its identity rides the
//! reread execution identity (`reread_prompt_version`, R2.5).

/// Frozen table-reread prompt bytes (no leading/trailing newline).
pub const TABLE_REREAD_V1: &str = "Transcribe the table shown in the image as a single complete HTML <table> element. Preserve every row and column, using thead/th for header rows, rowspan/colspan for merged cells, and empty <td></td> for empty cells. Output only the <table> element with no commentary.";
pub const TABLE_REREAD_PROMPT_ID: &str = "table-reread";
pub const TABLE_REREAD_PROMPT_VERSION: &str = "table-reread-v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_reread_prompt_bytes_are_frozen() {
        assert!(!TABLE_REREAD_V1.starts_with('\n'));
        assert!(!TABLE_REREAD_V1.ends_with('\n'));
        assert!(TABLE_REREAD_V1.starts_with("Transcribe the table"));
        assert!(TABLE_REREAD_V1.contains("<table>"));
        assert_eq!(TABLE_REREAD_PROMPT_ID, "table-reread");
        assert_eq!(TABLE_REREAD_PROMPT_VERSION, "table-reread-v1");
    }
}
