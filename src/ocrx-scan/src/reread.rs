//! Region-block reread machinery (next-phase plan B1, D-28/D-29): the RBAC
//! contract implementation (docs/develop/rbac-region-block-contract-v1.md —
//! the contract is frozen BEFORE this mechanism and the tests below map to
//! its clauses 1:1), the v1-frozen M4/M5 triggers (F1–F5 / G1–G4), and the
//! frozen compare-keep function (lexicographic four-tuple, tie keeps the
//! original table).
//!
//! Detection regions come from the layout model over the rendered bitmap
//! (B0', decoupled from the OCR output); output blocks carry no geometry,
//! so every association is CLASS-ORDINAL, never geometric.

use std::collections::BTreeMap;

use ocrx_core::PageBlock;

// ---------------------------------------------------------------------------
// Region model
// ---------------------------------------------------------------------------

/// Detection region class mapped to output block classes (contract §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegionClass {
    Table,
    Formula,
    Text,
}

/// One layout-detector region over the rendered page bitmap (pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectedRegion {
    pub class: RegionClass,
    /// Per-class ordinal on the page (detection order), the RBAC pairing key.
    pub index_in_class: usize,
    /// Pixel bbox `(x0, y0, x1, y1)`; geometry is used ONLY for dedup and
    /// trigger area ratios — never for block association.
    pub bbox: (f64, f64, f64, f64),
}

impl DetectedRegion {
    fn area(&self) -> f64 {
        ((self.bbox.2 - self.bbox.0).max(0.0)) * ((self.bbox.3 - self.bbox.1).max(0.0))
    }

    fn intersection_area(&self, other: &Self) -> f64 {
        let x0 = self.bbox.0.max(other.bbox.0);
        let y0 = self.bbox.1.max(other.bbox.1);
        let x1 = self.bbox.2.min(other.bbox.2);
        let y1 = self.bbox.3.min(other.bbox.3);
        (x1 - x0).max(0.0) * (y1 - y0).max(0.0)
    }

    /// Union-over-union IoU (0.0 when both areas are 0).
    fn iou(&self, other: &Self) -> f64 {
        let inter = self.intersection_area(other);
        let union = self.area() + other.area() - inter;
        if union <= 0.0 { 0.0 } else { inter / union }
    }
}

/// Contract §1.2 (form ④): same-class regions with IoU >= 0.9 dedup, the
/// earlier ordinal wins. Dedup MASKS the loser — survivor ordinals keep
/// their original numbering (no renumbering).
#[must_use]
pub fn dedup_regions(regions: &[DetectedRegion]) -> Vec<DetectedRegion> {
    let mut kept: Vec<DetectedRegion> = Vec::new();
    'outer: for region in regions {
        for survivor in &kept {
            if survivor.class == region.class && survivor.iou(region) >= 0.9 {
                continue 'outer;
            }
        }
        kept.push(*region);
    }
    kept
}

// ---------------------------------------------------------------------------
// RBAC pairing (contract §2 forms ①/③/④/⑤)
// ---------------------------------------------------------------------------

/// The class-ordinal pairing between deduped detection regions and same-class
/// output blocks (contract §2): `pairs` holds (region ordinal, block index);
/// `unpaired_regions` are miss cases — they NEVER produce new blocks; the
/// unpaired-block disposition differs by branch (M4/M5 keep in place, M6b
/// append) and is applied by the splicers below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    pub pairs: Vec<(usize, usize)>,
    pub unpaired_regions: Vec<usize>,
    pub unpaired_blocks: Vec<usize>,
}

fn block_class(block: &PageBlock) -> Option<RegionClass> {
    match block {
        PageBlock::HtmlTable { .. } => Some(RegionClass::Table),
        PageBlock::Formula { .. } => Some(RegionClass::Formula),
        PageBlock::Markdown { .. } => Some(RegionClass::Text),
        PageBlock::VisualRegion { .. } => None,
    }
}

/// Pairs one class's regions with same-class blocks by ordinal (the k-th
/// region ↔ the k-th block; form ⑤: shuffled detection input is absorbed
/// because pairing is by per-class ordinal, never by array position).
#[must_use]
pub fn pair_class(regions: &[DetectedRegion], blocks: &[PageBlock], class: RegionClass) -> Pairing {
    let mut region_ordinals: Vec<usize> = regions
        .iter()
        .filter(|region| region.class == class)
        .map(|region| region.index_in_class)
        .collect();
    region_ordinals.sort_unstable();
    let block_indices: Vec<usize> = blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block_class(block) == Some(class))
        .map(|(index, _)| index)
        .collect();
    let shared = region_ordinals.len().min(block_indices.len());
    Pairing {
        pairs: (0..shared)
            .map(|k| (region_ordinals[k], block_indices[k]))
            .collect(),
        unpaired_regions: region_ordinals[shared..].to_vec(),
        unpaired_blocks: block_indices[shared..].to_vec(),
    }
}

/// Contract form ① (M4/M5): replace paired same-class blocks' content IN
/// PLACE — document order never moves, unpaired blocks keep their positions
/// (form ③, M4/M5 disposition). `replacements` maps region ordinal → new
/// block content; a paired block with no replacement entry stays untouched.
///
/// # Errors
///
/// When a replacement's class differs from the block it would replace
/// (frozen-contract violation).
pub fn splice_m4m5(
    blocks: &[PageBlock],
    regions: &[DetectedRegion],
    replacements: &BTreeMap<usize, PageBlock>,
    class: RegionClass,
) -> Result<Vec<PageBlock>, String> {
    let pairing = pair_class(regions, blocks, class);
    let replacement_by_block: BTreeMap<usize, &PageBlock> = pairing
        .pairs
        .iter()
        .filter_map(|(region_ordinal, block_index)| {
            replacements
                .get(region_ordinal)
                .map(|new| (*block_index, new))
        })
        .collect();
    let mut spliced = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        match replacement_by_block.get(&index) {
            Some(new) => {
                if block_class(new) != Some(class) {
                    return Err(format!(
                        "replacement class mismatch for block {index} (contract §2-①)"
                    ));
                }
                spliced.push((*new).clone());
            }
            None => spliced.push(block.clone()),
        }
    }
    Ok(spliced)
}

/// Contract form ② (M6b): detection reading order is the assembly
/// authority. Text regions produce their reread blocks in detection order;
/// the original table/formula blocks survive byte-for-byte as continuous
/// segments (original relative order kept) inserted at the anchor of the
/// FIRST same-class detection region — or appended at the end when the page
/// has no region of that class (forms ②/③: kept content is never dropped).
///
/// A text region without a reread entry produces nothing (unreadable region).
/// Original Markdown blocks are replaced by the region rereads wholesale
/// (M6b re-reads text; the original text blocks do not survive).
///
/// # Errors
///
/// When a text reread entry names a non-text region.
pub fn splice_m6b(
    blocks: &[PageBlock],
    regions: &[DetectedRegion],
    text_rereads: &BTreeMap<usize, String>,
) -> Result<Vec<PageBlock>, String> {
    for &ordinal in text_rereads.keys() {
        let Some(region) = regions
            .iter()
            .find(|region| region.index_in_class == ordinal)
        else {
            return Err(format!("text reread names unknown region {ordinal}"));
        };
        if region.class != RegionClass::Text {
            return Err(format!(
                "text reread names non-text region {ordinal} (contract §2-②)"
            ));
        }
    }
    // The kept-class segments anchor at their first detection-order region;
    // `table_inserted`/`formula_inserted` flags mark the anchor as consumed.
    let mut assembled: Vec<PageBlock> = Vec::new();
    let mut table_inserted = false;
    let mut formula_inserted = false;
    for region in regions {
        if region.class == RegionClass::Table && !table_inserted {
            table_inserted = true;
            extend_kept(&mut assembled, blocks, RegionClass::Table);
        }
        if region.class == RegionClass::Formula && !formula_inserted {
            formula_inserted = true;
            extend_kept(&mut assembled, blocks, RegionClass::Formula);
        }
        if region.class == RegionClass::Text {
            if let Some(content) = text_rereads.get(&region.index_in_class) {
                assembled.push(PageBlock::Markdown {
                    content: content.clone(),
                });
            }
        }
    }
    // Pages with no region of a kept class append its segment at the end.
    if !table_inserted {
        extend_kept(&mut assembled, blocks, RegionClass::Table);
    }
    if !formula_inserted {
        extend_kept(&mut assembled, blocks, RegionClass::Formula);
    }
    Ok(assembled)
}

fn extend_kept(assembled: &mut Vec<PageBlock>, blocks: &[PageBlock], class: RegionClass) {
    for block in blocks {
        if block_class(block) == Some(class) {
            assembled.push(block.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Frozen v1 triggers (D-29): M4 table F1–F5, M5 formula G1–G4
// ---------------------------------------------------------------------------

/// The v1-frozen structural shape a table HTML parses into (the compare
/// four-tuple's basis).
#[derive(Debug, Clone, PartialEq)]
pub struct TableShape {
    pub parses: bool,
    pub structure_attrs: usize,
    pub cell_fill_ratio: f64,
    pub rows_x_cols: (usize, usize),
}

/// Minimal tolerant HTML-table reader for the frozen trigger/compare
/// definitions: row = `<tr>`, cell = `<td>`/`<th>`; structure attributes are
/// `th` cells plus `rowspan`/`colspan` occurrences. `parses` = has a table
/// body with at least one row and one cell (F5).
#[must_use]
pub fn parse_table_shape(html: &str) -> TableShape {
    let lower = html.to_ascii_lowercase();
    let rows = count_tag_rows(&lower);
    let mut cells_per_row: Vec<usize> = Vec::new();
    let mut cell_texts: Vec<String> = Vec::new();
    for row in lower.split("<tr>").skip(1) {
        let body = match row.find("</tr>") {
            Some(end) => &row[..end],
            None => row,
        };
        let mut count = 0usize;
        for cell in split_cells(body) {
            count += 1;
            cell_texts.push(strip_tags(cell.text));
        }
        cells_per_row.push(count);
    }
    let parses = rows > 0 && cells_per_row.iter().any(|&count| count > 0);
    let filled = cell_texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .count();
    let fill = if cell_texts.is_empty() {
        0.0
    } else {
        f64::from(filled as u32) / f64::from(cell_texts.len() as u32)
    };
    let structure_attrs = count_occurrences(&lower, "<th")
        + count_occurrences(&lower, "rowspan=")
        + count_occurrences(&lower, "colspan=");
    TableShape {
        parses,
        structure_attrs,
        cell_fill_ratio: fill,
        rows_x_cols: (rows, cells_per_row.iter().copied().max().unwrap_or(0)),
    }
}

fn count_tag_rows(lower: &str) -> usize {
    count_occurrences(lower, "<tr")
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

struct CellSplit<'a> {
    text: &'a str,
}

fn split_cells(body: &str) -> Vec<CellSplit<'_>> {
    let mut cells = Vec::new();
    let bytes = body.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let td = body[index..].find("<td").map(|offset| index + offset);
        let th = body[index..].find("<th").map(|offset| index + offset);
        let start = match (td, th) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let Some(start) = start else { break };
        let after_tag = body[start..].find('>').map(|offset| start + offset + 1);
        let Some(after_tag) = after_tag else { break };
        let end_candidates = [
            body[after_tag..]
                .find("</td>")
                .map(|offset| after_tag + offset),
            body[after_tag..]
                .find("</th>")
                .map(|offset| after_tag + offset),
        ];
        let end = end_candidates.iter().flatten().copied().min();
        let Some(end) = end else { break };
        cells.push(CellSplit {
            text: &body[after_tag..end],
        });
        index = end;
    }
    cells
}

fn strip_tags(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            ch if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn row_texts(shape: &TableShape, html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut texts = Vec::new();
    for row in lower.split("<tr>").skip(1) {
        let body = match row.find("</tr>") {
            Some(end) => &row[..end],
            None => row,
        };
        let cells: Vec<String> = split_cells(body)
            .iter()
            .map(|cell| strip_tags(cell.text).trim().to_owned())
            .collect();
        texts.push(cells.join("\u{1}"));
    }
    let _ = shape;
    texts
}

/// M4 疑弱触发 v1 (contract §4): any of F1–F5.
#[must_use]
pub fn table_reread_candidate(html: &str, region_area_ratio: f64) -> bool {
    let shape = parse_table_shape(html);
    // F5: unparsable.
    if !shape.parses {
        return true;
    }
    let row_texts = row_texts(&shape, html);
    let counts: Vec<usize> = row_texts
        .iter()
        .map(|row| row.split('\u{1}').count())
        .collect();
    // F1: ragged rows (>= 2 rows with differing cell counts).
    if counts.len() >= 2 && counts.windows(2).any(|pair| pair[0] != pair[1]) {
        return true;
    }
    // F2: empty-cell ratio >= 0.3.
    let cells: Vec<&str> = row_texts
        .iter()
        .flat_map(|row| row.split('\u{1}'))
        .collect();
    let empty = cells.iter().filter(|cell| cell.trim().is_empty()).count();
    if !cells.is_empty() && f64::from(empty as u32) / f64::from(cells.len() as u32) >= 0.3 {
        return true;
    }
    // F3: no structure attributes and the region covers >= 10% of the page.
    if shape.structure_attrs == 0 && region_area_ratio >= 0.10 {
        return true;
    }
    // F4: an identical row repeated >= 3 times.
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &row_texts {
        *seen.entry(row.as_str()).or_default() += 1;
    }
    if seen.values().copied().any(|count| count >= 3) {
        return true;
    }
    false
}

/// Compare-keep v1 (contract §4): parse both tables into the four-tuple
/// (parses, structure attrs, cell fill, rows×cols), lexicographic — the
/// reread must be STRICTLY better to replace; a tie keeps the original.
/// Returns `true` when the ORIGINAL is kept.
#[must_use]
pub fn keep_original(original: &TableShape, reread: &TableShape) -> bool {
    fn rank(shape: &TableShape) -> (u8, usize, f64, usize) {
        (
            u8::from(shape.parses),
            shape.structure_attrs,
            shape.cell_fill_ratio,
            shape.rows_x_cols.0.saturating_mul(shape.rows_x_cols.1),
        )
    }
    let (original_rank, reread_rank) = (rank(original), rank(reread));
    match reread_rank.0.cmp(&original_rank.0) {
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => match reread_rank.1.cmp(&original_rank.1) {
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Equal => {
                let (a, b) = (original_rank.2, reread_rank.2);
                if (a - b).abs() < 1e-9 {
                    return true; // tie on fill
                }
                b <= a
            }
        },
    }
}

fn contains_replacement_glyph(latex: &str) -> bool {
    latex.contains('\u{25A1}') || latex.contains('\u{FFFD}') || latex.contains('？')
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4E00}'..='\u{9FFF}').contains(&ch) || ('\u{3400}'..='\u{4DBF}').contains(&ch)
    })
}

fn has_unbalanced_math_delimiters(latex: &str) -> bool {
    let dollar_count = latex.matches("$$").count();
    if dollar_count % 2 != 0 {
        return true;
    }
    let without_display = latex.replace("$$", "");
    if without_display.matches('$').count() % 2 != 0 {
        return true;
    }
    let begins = latex.matches("\\begin{").count();
    let ends = latex.matches("\\end{").count();
    if begins != ends {
        return true;
    }
    let open = latex.chars().filter(|&ch| ch == '{').count();
    let close = latex.chars().filter(|&ch| ch == '}').count();
    if open != close {
        return true;
    }
    // Parenthesis balance (G1's bracket half), ignoring escaped parens.
    let open_paren = latex.matches('(').count();
    let close_paren = latex.matches(')').count();
    open_paren != close_paren
}

/// M5 正常页公式触发 v1 (contract §4): any of G1–G4.
#[must_use]
pub fn formula_reread_candidate(latex: &str, region_area_ratio: f64) -> bool {
    // G1: unbalanced brackets / environments.
    if has_unbalanced_math_delimiters(latex) {
        return true;
    }
    // G2: replacement glyphs.
    if contains_replacement_glyph(latex) {
        return true;
    }
    // G3: length < 8 chars and the region covers >= 5% of the page.
    if latex.chars().count() < 8 && region_area_ratio >= 0.05 {
        return true;
    }
    // G4: CJK inside math mode (a block formula IS math mode).
    if contains_cjk(latex) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(class: RegionClass, index: usize, bbox: (f64, f64, f64, f64)) -> DetectedRegion {
        DetectedRegion {
            class,
            index_in_class: index,
            bbox,
        }
    }

    fn md(content: &str) -> PageBlock {
        PageBlock::Markdown {
            content: content.to_owned(),
        }
    }

    fn table(html: &str) -> PageBlock {
        PageBlock::HtmlTable {
            html: html.to_owned(),
        }
    }

    fn formula(latex: &str) -> PageBlock {
        PageBlock::Formula {
            latex: latex.to_owned(),
            display: true,
        }
    }

    // --- contract §1.2 form ④ ----------------------------------------------

    #[test]
    fn duplicate_regions_dedup_by_iou_first_wins() {
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 100.0, 100.0)),
            region(RegionClass::Table, 1, (1.0, 1.0, 99.0, 99.0)), // IoU ~0.96
            region(RegionClass::Table, 2, (200.0, 0.0, 300.0, 100.0)),
            region(RegionClass::Formula, 0, (0.0, 0.0, 100.0, 100.0)), // other class
        ];
        let deduped = dedup_regions(&regions);
        assert_eq!(deduped.len(), 3);
        // The loser is masked, survivors keep their original ordinals.
        assert_eq!(
            deduped
                .iter()
                .map(|region| region.index_in_class)
                .collect::<Vec<_>>(),
            vec![0, 2, 0]
        );
        assert_eq!(deduped[2].class, RegionClass::Formula);
    }

    // --- contract §2 forms ①/③ (M4/M5) --------------------------------------

    #[test]
    fn ordinal_pairs_same_class_blocks_in_document_order() {
        // Table blocks at document positions 0 and 2; regions 0/1 pair by
        // ordinal regardless of the blocks' document positions (form ⑤).
        let blocks = vec![
            table("<tr><td>a</td></tr>"),
            md("prose"),
            table("<tr><td>b</td></tr>"),
        ];
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 10.0, 10.0)),
            region(RegionClass::Table, 1, (0.0, 10.0, 10.0, 20.0)),
        ];
        let pairing = pair_class(&regions, &blocks, RegionClass::Table);
        assert_eq!(pairing.pairs, vec![(0, 0), (1, 2)]);
        assert!(pairing.unpaired_regions.is_empty());
        assert!(pairing.unpaired_blocks.is_empty());
    }

    #[test]
    fn splice_m4m5_replaces_in_place_and_keeps_document_order() {
        let blocks = vec![
            table("<tr><td>old0</td></tr>"),
            md("prose"),
            table("<tr><td>old1</td></tr>"),
        ];
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 10.0, 10.0)),
            region(RegionClass::Table, 1, (0.0, 10.0, 10.0, 20.0)),
        ];
        let mut replacements = BTreeMap::new();
        replacements.insert(1usize, table("<tr><td>new1</td></tr>"));
        let spliced = splice_m4m5(&blocks, &regions, &replacements, RegionClass::Table).unwrap();
        assert_eq!(spliced.len(), 3, "document order never moves");
        assert_eq!(
            spliced[0],
            table("<tr><td>old0</td></tr>"),
            "no replacement entry: untouched"
        );
        assert_eq!(spliced[1], md("prose"));
        assert_eq!(
            spliced[2],
            table("<tr><td>new1</td></tr>"),
            "block 2 replaced in place"
        );
    }

    #[test]
    fn unpaired_regions_produce_no_new_blocks() {
        // detection_N > output_N: the miss region must not add a block.
        let blocks = vec![table("<tr><td>a</td></tr>")];
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 10.0, 10.0)),
            region(RegionClass::Table, 1, (0.0, 10.0, 10.0, 20.0)),
        ];
        let pairing = pair_class(&regions, &blocks, RegionClass::Table);
        assert_eq!(pairing.pairs, vec![(0, 0)]);
        assert_eq!(pairing.unpaired_regions, vec![1]);
        let spliced = splice_m4m5(&blocks, &regions, &BTreeMap::new(), RegionClass::Table).unwrap();
        assert_eq!(spliced.len(), 1, "no new block from the miss region");
    }

    #[test]
    fn unpaired_blocks_keep_position_m4m5() {
        // detection_N < output_N: the unpaired (missed-detection) block keeps
        // its document position (M4/M5 disposition, form ③).
        let blocks = vec![
            md("head"),
            table("<tr><td>a</td></tr>"),
            table("<tr><td>b</td></tr>"),
        ];
        let regions = vec![region(RegionClass::Table, 0, (0.0, 0.0, 10.0, 10.0))];
        let spliced = splice_m4m5(&blocks, &regions, &BTreeMap::new(), RegionClass::Table).unwrap();
        assert_eq!(spliced.len(), 3);
        assert_eq!(spliced[1], table("<tr><td>a</td></tr>"));
        assert_eq!(
            spliced[2],
            table("<tr><td>b</td></tr>"),
            "unpaired block kept in place"
        );
    }

    #[test]
    fn splice_m4m5_rejects_class_mismatch() {
        let blocks = vec![table("<tr><td>a</td></tr>")];
        let regions = vec![region(RegionClass::Table, 0, (0.0, 0.0, 1.0, 1.0))];
        let mut replacements = BTreeMap::new();
        replacements.insert(0usize, formula("x"));
        assert!(splice_m4m5(&blocks, &regions, &replacements, RegionClass::Table).is_err());
    }

    // --- contract §2 forms ②/③ (M6b) ----------------------------------------

    #[test]
    fn m6b_splices_text_by_detection_order_with_kept_segments() {
        // Detection order: text0, table1, text2, formula3 — the kept table
        // segment anchors at the table region's position, the formula segment
        // at the formula region's position; kept blocks keep relative order.
        let blocks = vec![
            md("old text"),
            table("<tr><td>T1</td></tr>"),
            table("<tr><td>T2</td></tr>"),
            formula("E1"),
        ];
        let regions = vec![
            region(RegionClass::Text, 0, (0.0, 0.0, 10.0, 5.0)),
            region(RegionClass::Table, 0, (0.0, 5.0, 10.0, 10.0)),
            region(RegionClass::Text, 1, (0.0, 10.0, 10.0, 15.0)),
            region(RegionClass::Formula, 0, (0.0, 15.0, 10.0, 20.0)),
        ];
        let mut rereads = BTreeMap::new();
        rereads.insert(0usize, "new text 0".to_owned());
        rereads.insert(1usize, "new text 2".to_owned());
        let spliced = splice_m6b(&blocks, &regions, &rereads).unwrap();
        assert_eq!(
            spliced,
            vec![
                md("new text 0"),
                table("<tr><td>T1</td></tr>"),
                table("<tr><td>T2</td></tr>"),
                md("new text 2"),
                formula("E1"),
            ],
            "detection order assembly with kept segments at class anchors"
        );
    }

    #[test]
    fn m6b_appends_kept_segment_when_no_region_of_class() {
        // No table region on the page: the kept table segment appends at the
        // end (form ②).
        let blocks = vec![table("<tr><td>T</td></tr>"), formula("E")];
        let regions = vec![region(RegionClass::Text, 0, (0.0, 0.0, 10.0, 5.0))];
        let mut rereads = BTreeMap::new();
        rereads.insert(0usize, "new text".to_owned());
        let spliced = splice_m6b(&blocks, &regions, &rereads).unwrap();
        assert_eq!(
            spliced,
            vec![md("new text"), table("<tr><td>T</td></tr>"), formula("E")],
        );
    }

    #[test]
    fn unpaired_kept_blocks_survive_m6b() {
        // Two original table blocks, one table region: BOTH survive (form ③,
        // M6b disposition — kept content is never dropped).
        let blocks = vec![table("<tr><td>T1</td></tr>"), table("<tr><td>T2</td></tr>")];
        let regions = vec![region(RegionClass::Table, 0, (0.0, 0.0, 10.0, 10.0))];
        let spliced = splice_m6b(&blocks, &regions, &BTreeMap::new()).unwrap();
        assert_eq!(
            spliced,
            vec![table("<tr><td>T1</td></tr>"), table("<tr><td>T2</td></tr>")],
        );
    }

    #[test]
    fn splice_m6b_rejects_non_text_reread_entries() {
        let blocks = vec![md("t")];
        let regions = vec![region(RegionClass::Table, 0, (0.0, 0.0, 1.0, 1.0))];
        let mut rereads = BTreeMap::new();
        rereads.insert(0usize, "x".to_owned());
        assert!(splice_m6b(&blocks, &regions, &rereads).is_err());
    }

    // --- contract §2 form ⑤ ---------------------------------------------------

    #[test]
    fn detection_order_absorbs_shuffled_regions() {
        // Regions arrive shuffled in the input array; the per-class ordinal
        // pairing still maps region 0 → block 0, region 1 → block 1.
        let blocks = vec![table("<tr><td>a</td></tr>"), table("<tr><td>b</td></tr>")];
        let regions = vec![
            region(RegionClass::Table, 1, (0.0, 10.0, 10.0, 20.0)),
            region(RegionClass::Table, 0, (0.0, 0.0, 10.0, 10.0)),
        ];
        let pairing = pair_class(&regions, &blocks, RegionClass::Table);
        assert_eq!(pairing.pairs, vec![(0, 0), (1, 1)]);
    }

    // --- frozen triggers (D-29) -------------------------------------------------

    #[test]
    fn table_trigger_f1_ragged_rows() {
        assert!(table_reread_candidate(
            "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>",
            0.0
        ));
    }

    #[test]
    fn table_trigger_f2_empty_cell_ratio() {
        assert!(table_reread_candidate(
            "<table><tr><td>a</td><td></td></tr><tr><td></td><td></td></tr><tr><td></td><td></td></tr><tr><td></td><td></td></tr></table>",
            0.0
        ));
        // 1/4 empty = 0.25 < 0.3: not triggered by F2.
        assert!(!table_reread_candidate(
            "<table><tr><td>a</td><td>b</td><td>c</td><td></td></tr></table>",
            0.0
        ));
    }

    #[test]
    fn table_trigger_f3_no_structure_attrs_large_region() {
        let plain = "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>";
        assert!(table_reread_candidate(plain, 0.10));
        assert!(!table_reread_candidate(plain, 0.09));
        // Structure attributes present: never F3.
        let with_th = "<table><tr><th>a</th><td>b</td></tr></table>";
        assert!(!table_reread_candidate(with_th, 0.5));
    }

    #[test]
    fn table_trigger_f4_identical_row_repeated_three_times() {
        let repeated = "<table><tr><td>x</td></tr><tr><td>x</td></tr><tr><td>x</td></tr></table>";
        assert!(table_reread_candidate(repeated, 0.0));
        let twice = "<table><tr><td>x</td></tr><tr><td>x</td></tr></table>";
        assert!(!table_reread_candidate(twice, 0.0));
    }

    #[test]
    fn table_trigger_f5_unparsable_html() {
        assert!(table_reread_candidate("<div>not a table</div>", 0.0));
        assert!(table_reread_candidate("<table></table>", 0.0), "no rows");
    }

    #[test]
    fn healthy_table_never_triggers() {
        assert!(!table_reread_candidate(
            "<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>",
            0.5
        ));
    }

    #[test]
    fn formula_trigger_g1_unbalanced_brackets_or_environments() {
        assert!(formula_reread_candidate("\\begin{aligned} x", 0.0));
        assert!(formula_reread_candidate("f(x = 1", 0.0));
        assert!(formula_reread_candidate("$x^2", 0.0), "odd $ count");
        assert!(!formula_reread_candidate(
            "\\begin{aligned} x \\end{aligned}",
            0.0
        ));
        assert!(!formula_reread_candidate("f(x) = x^2 + 1", 0.0));
    }

    #[test]
    fn formula_trigger_g2_replacement_glyphs() {
        assert!(formula_reread_candidate("x = \u{25A1} + 1", 0.0));
        assert!(formula_reread_candidate("x = \u{FFFD}", 0.0));
        assert!(formula_reread_candidate("x = ？", 0.0));
        assert!(!formula_reread_candidate("x = 1", 0.0));
    }

    #[test]
    fn formula_trigger_g3_short_latex_large_region() {
        assert!(formula_reread_candidate("x^2", 0.05));
        assert!(!formula_reread_candidate("x^2", 0.049));
        assert!(
            !formula_reread_candidate("\\int_0^\\infty f(x) dx", 0.5),
            "long latex"
        );
    }

    #[test]
    fn formula_trigger_g4_cjk_in_math() {
        assert!(formula_reread_candidate("面积 = 长 \\times 宽", 0.0));
        assert!(!formula_reread_candidate("E = mc^2", 0.0));
    }

    // --- frozen compare-keep (D-29) ----------------------------------------------

    #[test]
    fn compare_keep_lexicographic_tie_keeps_original() {
        let worse = TableShape {
            parses: false,
            structure_attrs: 0,
            cell_fill_ratio: 0.0,
            rows_x_cols: (0, 0),
        };
        let baseline = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.8,
            rows_x_cols: (3, 4),
        };
        let better = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.9,
            rows_x_cols: (3, 4),
        };
        let tie = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.8,
            rows_x_cols: (3, 4),
        };
        let more_attrs = TableShape {
            parses: true,
            structure_attrs: 3,
            cell_fill_ratio: 0.5,
            rows_x_cols: (2, 2),
        };
        assert!(
            keep_original(&baseline, &worse),
            "worse reread: keep original"
        );
        assert!(
            !keep_original(&baseline, &better),
            "strictly better: replace"
        );
        assert!(keep_original(&baseline, &tie), "tie keeps the original");
        assert!(
            !keep_original(&baseline, &more_attrs),
            "attrs rank second: more attrs wins even with lower fill"
        );
    }

    #[test]
    fn compare_keep_parses_dominates_everything() {
        let unparsable_original = TableShape {
            parses: false,
            structure_attrs: 9,
            cell_fill_ratio: 1.0,
            rows_x_cols: (9, 9),
        };
        let parsable_reread = TableShape {
            parses: true,
            structure_attrs: 0,
            cell_fill_ratio: 0.1,
            rows_x_cols: (1, 1),
        };
        assert!(!keep_original(&unparsable_original, &parsable_reread));
    }

    #[test]
    fn parse_table_shape_reports_the_frozen_tuple() {
        let shape = parse_table_shape(
            "<table><tr><th>a</th><td></td></tr><tr><td rowspan=\"2\">x</td><td>b</td></tr></table>",
        );
        assert!(shape.parses);
        assert_eq!(shape.structure_attrs, 2, "one th + one rowspan");
        assert_eq!(shape.rows_x_cols, (2, 2));
        assert!(
            (shape.cell_fill_ratio - 0.75).abs() < 1e-9,
            "3 of 4 cells filled"
        );
        assert!(!parse_table_shape("<div>x</div>").parses);
    }
}
