//! OvisOCR2 protocol adapter crate (plan §8.7): only the adapter lives
//! here. Model-transport concerns (image marker placement, prompt
//! passthrough, `<think>` stripping with the frozen Q3.1 count) stay; the
//! OCR business parsing (formulas, tables, bbox, markdown links, page
//! documents) moved to `ocrx-scan` (T087 deletes the legacy copies).

pub mod adapter;

pub const ADAPTER_ID: &str = "ovisocr2";
pub const MAX_OUTPUT_TOKENS: u32 = 16_384;

pub struct ThinkStrip {
    pub text: String,
    /// Complete blocks whose interior had non-whitespace content; these set
    /// the `think_removed` warning.
    pub nonempty: usize,
    /// Complete blocks whose interior was whitespace only; stripped silently
    /// but counted into the result.
    pub empty: usize,
    /// Stray open/close tags removed outside a complete block.
    pub strays: usize,
}

impl ThinkStrip {
    pub fn total(&self) -> usize {
        self.nonempty + self.empty + self.strays
    }
}

/// Q1.1c candidate C (quality plan §4.1, D-10): column-band detection by
/// vertical whitespace-valley projection on the rendered page - the classic
/// layout-analysis pass, no model and no prompt change. Returns x bounds in
/// pixels for 2..=4 columns left to right, or `None` when no reliable
/// valley set exists (callers fall back to a single full-width band).

/// Strips leaked `<think>` reasoning blocks from a raw model output. Runs
/// once at the artifact entry so page .md, page.json and bbox extraction all
/// consume the same stripped text (single-source, plan D-5).
pub fn strip_think_blocks(input: &str) -> ThinkStrip {
    fn is_tag_start(rest: &str) -> Option<(usize, bool)> {
        // Returns (byte offset, is_open_tag) of the next `<think` / `</think`
        // that forms a real tag (`\b`: the char after `think` is a word
        // boundary) and ends at the first `>`.
        let mut search = 0;
        while let Some(rel) = rest[search..].find('<') {
            let start = search + rel;
            let after = &rest[start + 1..];
            let (is_open, word) = if let Some(word) = after.strip_prefix("think") {
                (true, word)
            } else if let Some(word) = after.strip_prefix("/think") {
                (false, word)
            } else {
                search = start + 1;
                continue;
            };
            // `\b` after `think`: next char must not be alphanumeric or `_`.
            let boundary = !word
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !boundary {
                search = start + 1;
                continue;
            }
            let Some(_tag_end) = word.find('>') else {
                // No closing `>`: not a tag, keep scanning.
                search = start + 1;
                continue;
            };
            return Some((start, is_open));
        }
        None
    }

    fn skip_whitespace(input: &str, mut offset: usize) -> usize {
        while let Some(char) = input[offset..].chars().next() {
            if char.is_whitespace() {
                offset += char.len_utf8();
            } else {
                break;
            }
        }
        offset
    }

    let mut output = String::with_capacity(input.len());
    let mut nonempty = 0;
    let mut empty = 0;
    let mut strays = 0;
    let mut rest = input;
    loop {
        let Some((start, is_open)) = is_tag_start(rest) else {
            output.push_str(rest);
            return ThinkStrip {
                text: output,
                nonempty,
                empty,
                strays,
            };
        };
        output.push_str(&rest[..start]);
        let after_tag = &rest[start..];
        let tag_len = after_tag.find('>').map_or(0, |end| end + 1);
        if is_open {
            // Non-greedy block semantics: the block ends at the NEXT close
            // tag, with any interleaved open tags counted as interior text.
            let mut scan = tag_len;
            let mut close_start = None;
            while let Some((rel, opens_again)) = is_tag_start(&after_tag[scan..]) {
                if !opens_again {
                    close_start = Some(scan + rel);
                    break;
                }
                let inner_len = after_tag[scan + rel..].find('>').map_or(0, |end| end + 1);
                scan += rel + inner_len;
            }
            if let Some(close_start) = close_start {
                let close_len = after_tag[close_start..].find('>').map_or(0, |end| end + 1);
                let interior = &after_tag[tag_len..close_start];
                if interior.trim().is_empty() {
                    empty += 1;
                } else {
                    nonempty += 1;
                }
                // close_start is relative to after_tag (= rest[start..]);
                // the skip offset must be absolute in rest.
                let end = skip_whitespace(rest, start + close_start + close_len);
                rest = &rest[end..];
                continue;
            }
            // Unclosed open tag: stray tag, keep the content.
            strays += 1;
            let end = skip_whitespace(rest, start + tag_len);
            rest = &rest[end..];
            continue;
        }
        // Stray close tag.
        strays += 1;
        let end = skip_whitespace(rest, start + tag_len);
        rest = &rest[end..];
    }
}
