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

use crate::region_provider::RegionProvider;
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
    /// Detector confidence, recorded per-region into `f6_detail` for later
    /// calibration (M4.2 D-61: phase 1 records, never filters). Regions
    /// from test fixtures may use 0.0.
    pub conf: f64,
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
// R2.2: table-region cropper (D-21: reread decodes at the 1024 grid; the
// crop bitmap itself stays at the raster's native resolution — the mtmd
// grid does the resample)
// ---------------------------------------------------------------------------

/// Crops one pixel-space region from a rendered page. Coordinates clamp to
/// the bitmap; a degenerate (empty) crop is an error. The crop keeps the
/// source audit and generation so the commit path sees the true provenance.
pub fn crop_region(
    page: &crate::state::RenderedPage,
    bbox: (f64, f64, f64, f64),
) -> std::io::Result<crate::state::RenderedPage> {
    let width = page.width;
    let height = page.height;
    // degenerate check BEFORE clamping: an empty/inverted region is a
    // caller contract violation, silently widening it would mask bugs.
    if bbox.2.round() <= bbox.0.round() || bbox.3.round() <= bbox.1.round() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "degenerate crop region",
        ));
    }
    let x0 = bbox.0.round().max(0.0).min(f64::from(width - 1)) as u32;
    let x1 = (bbox.2.round() as u32).clamp(x0 + 1, width);
    let y0 = bbox.1.round().max(0.0).min(f64::from(height - 1)) as u32;
    let y1 = (bbox.3.round() as u32).clamp(y0 + 1, height);
    let cw = usize::try_from(x1 - x0).map_err(|_| std::io::Error::other("crop width"))?;
    let ch = usize::try_from(y1 - y0).map_err(|_| std::io::Error::other("crop height"))?;
    let stride = usize::try_from(width).map_err(|_| std::io::Error::other("stride"))? * 3;
    let row_len = cw * 3;
    let mut rgb8 = Vec::with_capacity(ch * row_len);
    for row in y0..y1 {
        let base = usize::try_from(row).map_err(|_| std::io::Error::other("row"))? * stride
            + usize::try_from(x0).map_err(|_| std::io::Error::other("x0"))? * 3;
        rgb8.extend_from_slice(&page.rgb8[base..base + row_len]);
    }
    crate::state::RenderedPage::new(x1 - x0, y1 - y0, rgb8).map(|mut cropped| {
        cropped.audit = page.audit;
        cropped.generation = page.generation;
        cropped
    })
}

// ---------------------------------------------------------------------------
// R2.4: integral skip verdict + content-identity gate (D-44/D-47 frozen
// contracts, executable reference ported from
// omnidocbench-region-pairing-check.py / omnidocbench-reread-identity-
// calibration.py - the Python harnesses are the calibration + acceptance
// side; this is the production shape that the orchestration calls)
// ---------------------------------------------------------------------------

/// Normalization v1 (D-47): strip tags -> collapse whitespace -> lowercase
/// -> char bigrams. CJK-friendly (bigrams of CJK text carry structure).
pub fn normalized_bigrams(text: &str) -> std::collections::BTreeSet<String> {
    let stripped = strip_tags(text);
    let lowered = stripped.to_lowercase();
    let collapsed: String = lowered.split_whitespace().collect::<Vec<_>>().join("");
    let s = collapsed.as_str();
    if s.chars().count() < 2 {
        return s.chars().map(|c| c.to_string()).collect();
    }
    s.chars()
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| w.iter().collect())
        .collect()
}

/// Frozen min-text floor (D-47): either side below this many unique
/// bigrams is INCONCLUSIVE - fail-closed at the caller.
pub const MIN_TEXT_BIGRAMS: usize = 8;

/// Directional coverage (D-47): (cov_a, cov_b) where cov_a = share of the
/// ORIGINAL covered by the reread (fragment guard) and cov_b = share of
/// the REREAD covered by the original (foreign guard). None when either
/// side is below the floor.
pub fn directional_coverage(a_text: &str, b_text: &str) -> Option<(f64, f64)> {
    let ga = normalized_bigrams(a_text);
    let gb = normalized_bigrams(b_text);
    if ga.len() < MIN_TEXT_BIGRAMS || gb.len() < MIN_TEXT_BIGRAMS {
        return None;
    }
    let inter = ga.intersection(&gb).count();
    Some((
        inter as f64 / ga.len() as f64,
        inter as f64 / gb.len() as f64,
    ))
}

/// Frozen identity thresholds (D-47): T1 guards against FRAGMENTS (the
/// reread missing rows => low cov_a), T2 against FOREIGN content (a
/// different table => low cov_b). Bias to blocking: inconclusive and
/// below-threshold both keep the original.
pub const IDENTITY_T1_COVERAGE_OF_ORIGINAL: f64 = 0.40;
pub const IDENTITY_T2_COVERAGE_OF_REREAD: f64 = 0.25;

/// R2.5/BLK-5: generation of the frozen reread rules (F1-F5 triggers +
/// compare-keep + D-47 normalization/identity). Calibration contract: one
/// calibration round = +1, and any normalization-version switch is a
/// generation bump — `reread_rules_generation` carries it into the
/// checkpoint so artifacts never reuse across rule changes.
/// 1 = the calibrated set adjudicated by the B0'/D-47 work.
/// 2 = M4.1: the F6 missing-table branch with the D-54 double-formula
/// attribution gate (VLM F6 arm — branch WIP, never shipped; M4.2 removes
/// it from the runtime).
/// 3 = M4.2 (current): the calibrated SLANet rules — cov_attempt single-gate
/// attribution (D-29 one-round calibration, cov_reread removed with T0
/// evidence) + the ruling-line pre-check gate. Generation-2 artifacts never
/// reuse against generation-3 runs and vice versa (D-63; T2/T4 regressions
/// cover both directions and both flag paths).
pub const REREAD_RULES_GENERATION: u32 = 3;

/// The generation recorded for old-flag-only runs (`--table-reread on`,
/// F1-F5 VLM reread; F6 disabled in the M4.2 runtime). Equals the M4.1-era
/// value so era artifacts cannot cross-resume with generation-3 runs.
pub const VLM_REREAD_RULES_GENERATION: u32 = 2;

/// Content-identity verdict (D-47): Some(replace_ok) when both coverages
/// clear the floor and thresholds; None = inconclusive (keep original).
/// `replace_ok` means the reread may PROCEED to compare-keep - never a
/// direct replace.
pub fn content_identity_pass(original_html: &str, reread_html: &str) -> Option<bool> {
    let (cov_a, cov_b) = directional_coverage(original_html, reread_html)?;
    Some(cov_a >= IDENTITY_T1_COVERAGE_OF_ORIGINAL && cov_b >= IDENTITY_T2_COVERAGE_OF_REREAD)
}

/// Why a page was skipped (diagnostic; the behavior is identical: keep
/// original output, no reread).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingSkip {
    DedupEvent,
    CountMismatch { detection_n: usize, output_n: usize },
    OrderingAmbiguous,
}

pub(crate) fn box_iou(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> f64 {
    let ix0 = a.0.max(b.0);
    let iy0 = a.1.max(b.1);
    let ix1 = a.2.min(b.2);
    let iy1 = a.3.min(b.3);
    let inter = (ix1 - ix0).max(0.0) * (iy1 - iy0).max(0.0);
    let area_a = (a.2 - a.0).max(0.0) * (a.3 - a.1).max(0.0);
    let area_b = (b.2 - b.0).max(0.0) * (b.3 - b.1).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn consecutive_y_overlaps(boxes: &[(f64, f64, f64, f64)], frac: f64) -> bool {
    boxes.windows(2).any(|pair| {
        let (a, b) = (pair[0], pair[1]);
        let inter = a.3.min(b.3) - a.1.max(b.1);
        let smaller = (a.3 - a.1).min(b.3 - b.1);
        smaller > 0.0 && inter / smaller >= frac
    })
}

/// Band-clustering order along an axis (0 = x columns, 1 = y rows),
/// mirrored from the Python calibration harness.
fn axis_lo(b: (f64, f64, f64, f64), axis: usize) -> f64 {
    if axis == 0 { b.0 } else { b.1 }
}
fn axis_hi(b: (f64, f64, f64, f64), axis: usize) -> f64 {
    if axis == 0 { b.2 } else { b.3 }
}

fn bands_order(boxes: &[(f64, f64, f64, f64)], axis: usize) -> (bool, Vec<usize>) {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by_key(|i| (axis_lo(boxes[*i], axis) * 1000.0).round() as i64);
    let mut bands: Vec<Vec<usize>> = Vec::new();
    for i in order {
        let fits = match bands.last() {
            Some(last) => {
                let prev = boxes[*last.last().expect("non-empty band")];
                let inter = axis_hi(prev, axis).min(axis_hi(boxes[i], axis))
                    - axis_lo(prev, axis).max(axis_lo(boxes[i], axis));
                let smaller = (axis_hi(prev, axis) - axis_lo(prev, axis))
                    .min(axis_hi(boxes[i], axis) - axis_lo(boxes[i], axis));
                smaller > 0.0 && inter / smaller >= 0.3
            }
            None => false,
        };
        if fits {
            bands.last_mut().expect("last exists").push(i);
        } else {
            bands.push(vec![i]);
        }
    }
    let clean = bands.len() <= 1
        || bands.windows(2).all(|pair| {
            let hi = pair[0]
                .iter()
                .map(|j| axis_hi(boxes[*j], axis))
                .fold(f64::NEG_INFINITY, f64::max);
            let lo = pair[1]
                .iter()
                .map(|j| axis_lo(boxes[*j], axis))
                .fold(f64::INFINITY, f64::min);
            hi <= lo + 1e-6
        });
    bands.sort_by_key(|band| {
        band.iter()
            .map(|j| axis_lo(boxes[*j], axis) * 1000.0)
            .fold(f64::INFINITY, f64::min) as i64
    });
    let mut ordinals = Vec::new();
    for band in bands {
        let mut within = band;
        within.sort_by_key(|j| (axis_lo(boxes[*j], 1 - axis) * 1000.0).round() as i64);
        ordinals.extend(within);
    }
    (clean, ordinals)
}

/// Pairing verdict (D-44 integral skip): regions are the R1b-normalized
/// detection sequence, `output_n` the page OCR html_table block count.
/// SKIP (Err) reasons: dedup event (ordinal holes), count mismatch
/// (miss/multi), dual-hypothesis ordering ambiguity. PASS yields the
/// detection indices in reading order - position k pairs with the k-th
/// output table block by class ordinal.
pub fn pairing_verdict(
    regions: &[DetectedRegion],
    output_n: usize,
) -> Result<Vec<usize>, PairingSkip> {
    let mut kept: Vec<DetectedRegion> = Vec::new();
    let mut deduped = false;
    for region in regions {
        if kept
            .iter()
            .any(|k| k.class == region.class && box_iou(k.bbox, region.bbox) >= 0.9)
        {
            deduped = true;
            continue;
        }
        kept.push(*region);
    }
    if deduped {
        return Err(PairingSkip::DedupEvent);
    }
    if kept.len() != output_n {
        return Err(PairingSkip::CountMismatch {
            detection_n: kept.len(),
            output_n,
        });
    }
    let boxes: Vec<(f64, f64, f64, f64)> = kept.iter().map(|k| k.bbox).collect();
    let (row_clean, row_order) = bands_order(&boxes, 1);
    let (col_clean, col_order) = bands_order(&boxes, 0);
    if !row_clean || !col_clean || row_order != col_order {
        return Err(PairingSkip::OrderingAmbiguous);
    }
    if consecutive_y_overlaps(
        &row_order.iter().map(|i| boxes[*i]).collect::<Vec<_>>(),
        0.30,
    ) {
        return Err(PairingSkip::OrderingAmbiguous);
    }
    Ok(row_order)
}
// ---------------------------------------------------------------------------
// M4.1 F6 missing-table branch (plan v1.2, D-53..D-58): zero-table-block
// pages whose sidecar regions ARE tables - the model emitted no <table>
// block (content degraded to plain text or truncated mid-emission). The
// reread goes from nothing to something; compare-keep has no baseline, so
// adoption is gated by emission shape + structural validity + content
// attribution (raw pages) instead.
// ---------------------------------------------------------------------------

/// The markdown-level emission shape of a zero-table-block page (D-53).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum F6EmissionShape {
    /// Whitespace-only output - the reread tables are appended.
    Empty,
    /// A single unclosed `<table` marker with no non-table prose after it;
    /// carries the marker byte offset (the replacement anchor).
    RawSingle { marker_pos: usize },
    /// Unsafe disposition - v1 does not trigger, does not decode.
    Ineligible(F6SkipReason),
}

/// Why a zero-block page's emission shape disqualifies F6 (D-53 v1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum F6SkipReason {
    /// Table content embedded in prose - no contiguous anchor.
    TextOnlyNoAnchor,
    /// The malformed table attempt is followed by non-table body text -
    /// marker-to-end replacement would delete it.
    RawTableWithTailProse,
    /// Raw marker present but multiple regions share one attempt blob -
    /// per-region attribution cannot be split safely (D-58).
    MultiRegionRaw,
}

/// Classifies the emission shape. `det_n` is the sidecar table-region count
/// for the page (the caller orders/dedups after eligibility).
pub fn f6_emission_shape(markdown: &str, det_n: usize) -> F6EmissionShape {
    if markdown.trim().is_empty() {
        return F6EmissionShape::Empty;
    }
    let Some(marker_pos) = markdown.find("<table") else {
        return F6EmissionShape::Ineligible(F6SkipReason::TextOnlyNoAnchor);
    };
    // Raw marker present: every non-empty line after the marker must be
    // table-ish (<td/<th/<tr/</table fragments or bare numbers) - any
    // prose line means marker-to-end replacement would delete body text.
    let after = &markdown[marker_pos..];
    let tail_prose = after.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        let tableish = ["<td", "<th", "<tr", "</table", "<table"]
            .iter()
            .any(|tag| trimmed.contains(tag));
        !tableish
            && !trimmed
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_punctuation())
    });
    if tail_prose {
        return F6EmissionShape::Ineligible(F6SkipReason::RawTableWithTailProse);
    }
    if det_n != 1 {
        return F6EmissionShape::Ineligible(F6SkipReason::MultiRegionRaw);
    }
    F6EmissionShape::RawSingle { marker_pos }
}

/// Structural validity gate (D-54): parsable, at least 2 rows x 2 cols,
/// not entirely blank. Necessary condition for adoption.
pub fn f6_structural_gate(html: &str) -> bool {
    let shape = parse_table_shape(html);
    shape.parses
        && shape.rows_x_cols.0 >= 2
        && shape.rows_x_cols.1 >= 2
        && shape.cell_fill_ratio > 0.0
}

/// Content attribution gate (M4.2 D-62 calibrated; raw_single arm only):
/// the reread must recover most of the model's own attempt text
/// (cov_attempt >= 0.50 — the attempt is the model's reading of the same
/// region). The cov_reread >= 0.30 clause was REMOVED by the D-29
/// one-round calibration: it has a structural length bias (a correct
/// reread is much longer than a garbled attempt) and rejected objectively
/// good tables (T0: TEDS 0.998 rejected at cov_reread 0.248); fabrication
/// risk is covered by the ruling-line gate + structural gate + the T5
/// stage-A full manual review. Fail-closed: below the 8-bigram floor is a
/// reject. Generation 3 semantics (D-63) — any further threshold change
/// requires generation + 1 (R2.5/D-42).
pub fn f6_attribution_pass(attempt_text: &str, reread_html: &str) -> bool {
    const COV_ATTEMPT_MIN: f64 = 0.50;
    match directional_coverage(attempt_text, reread_html) {
        Some((cov_attempt, _cov_reread)) => cov_attempt >= COV_ATTEMPT_MIN,
        None => false,
    }
}

/// Orders the detected regions for F6 adoption: IoU dedup first, then the
/// D-44 dual-hypothesis reading order (row/column bands must agree;
/// consecutive-y overlap veto applies). `None` = ambiguous - whole-page
/// F6 skip (D-56).
pub fn f6_order_regions(regions: &[DetectedRegion]) -> Option<Vec<DetectedRegion>> {
    let deduped = dedup_regions(regions);
    let boxes: Vec<(f64, f64, f64, f64)> = deduped.iter().map(|r| r.bbox).collect();
    let (row_clean, row_order) = bands_order(&boxes, 1);
    let (col_clean, col_order) = bands_order(&boxes, 0);
    if !row_clean || !col_clean || row_order != col_order {
        return None;
    }
    if consecutive_y_overlaps(
        &row_order.iter().map(|i| boxes[*i]).collect::<Vec<_>>(),
        0.30,
    ) {
        return None;
    }
    Some(row_order.iter().map(|&i| deduped[i]).collect())
}

/// The F6 orchestration: zero-table-block pages only (the caller gates on
/// the block count). Regions are ordered (D-56), each crop decoded, and
/// adoption decided per arm:
/// - raw_single: structural gate + attribution gate (cov_attempt/cov_reread)
/// - empty: structural gate only (no anchor - the weakest link, per plan
///   D-54 v1.2; the spike's 100% manual review is the content-safety pass)
///
/// Adopted tables land as appended `HtmlTable` blocks in reading order and
/// are written into the markdown per the emission shape (marker-to-end
/// replacement for raw pages, append for empty pages). Zero text deletion.
///
/// Returns the new blocks, the new markdown, and the stats.
pub fn orchestrate_f6(
    blocks: &[PageBlock],
    markdown: &str,
    page_number: u32,
    page: &crate::state::RenderedPage,
    provider: &mut dyn RegionProvider,
    engine: &mut dyn crate::structure_engine::StructureTableEngine,
) -> std::io::Result<(Vec<PageBlock>, String, RereadStats)> {
    let mut stats = RereadStats::default();
    let regions = provider.regions_for_page(page_number);
    if regions.is_empty() {
        stats.f6_skipped_shape = true;
        return Ok((blocks.to_vec(), markdown.to_owned(), stats));
    }
    let shape = f6_emission_shape(markdown, regions.len());
    let (markdown_anchor, attempt_text) = match &shape {
        F6EmissionShape::RawSingle { marker_pos } => {
            (Some(*marker_pos), Some(markdown[*marker_pos..].to_owned()))
        }
        F6EmissionShape::Empty => (None, None),
        F6EmissionShape::Ineligible(_) => {
            stats.f6_skipped_shape = true;
            return Ok((blocks.to_vec(), markdown.to_owned(), stats));
        }
    };
    let Some(ordered) = f6_order_regions(&regions) else {
        stats.f6_ordering_ambiguous = true;
        return Ok((blocks.to_vec(), markdown.to_owned(), stats));
    };
    stats.f6_triggered = ordered.len();
    let mut adopted: Vec<PageBlock> = Vec::new();
    let mut adopted_html: Vec<String> = Vec::new();
    for (ordinal, region) in ordered.iter().enumerate() {
        let crop = crop_region(page, region.bbox)?;
        // M4.2: the engine verdict replaces the VLM decode. Runtime faults
        // are per-region dispositions (D-63 (3)/(4)), never a page failure.
        let verdict = match crate::window::encode_page_png(&crop) {
            Ok(png) => engine.recognize(&png, page_number, ordinal),
            Err(error) => Err(crate::structure_engine::EngineErrorKind::EngineError(
                error.to_string(),
            )),
        };
        let mut outcome = F6RegionOutcome {
            region_ordinal: ordinal,
            decoded: true,
            adopted: false,
            reject_reason: None,
            conf: region.conf,
            engine: crate::structure_engine::EXPECTED_ENGINE,
        };
        let reread_html = match verdict {
            Ok(crate::structure_engine::StructureOutcome::Html(html)) => Some(html),
            Ok(crate::structure_engine::StructureOutcome::Reject(reject)) => {
                outcome.reject_reason = match reject {
                    crate::structure_engine::StructureReject::NoRulingLines => {
                        Some("no_ruling_lines")
                    }
                    crate::structure_engine::StructureReject::EmptyOutput => Some("empty_output"),
                };
                None
            }
            Err(kind) => {
                outcome.reject_reason = Some(kind.reject_reason());
                None
            }
        };
        if let Some(reread_html) = reread_html {
            if f6_structural_gate(&reread_html) {
                let attribution_ok = match &attempt_text {
                    Some(attempt) => f6_attribution_pass(attempt, &reread_html),
                    None => true, // empty arm: no anchor - structural gate only
                };
                if attribution_ok {
                    adopted.push(PageBlock::HtmlTable {
                        html: reread_html.clone(),
                    });
                    adopted_html.push(reread_html);
                    outcome.adopted = true;
                } else {
                    outcome.reject_reason = Some("attribution");
                }
            } else {
                outcome.reject_reason = Some("structural");
            }
        }
        if outcome.adopted {
            stats.f6_emitted += 1;
        } else {
            stats.f6_rejected += 1;
        }
        stats.f6_detail.push(outcome);
    }
    // Persistence (D-54): markdown per emission shape; blocks appended in
    // reading order. Zero deletion of existing content.
    // D-54 zero-deletion rule: when NOTHING was adopted (all gates
    // rejected), the page keeps its original markdown and blocks exactly —
    // the raw attempt text is never deleted by a failed reread.
    if adopted.is_empty() {
        return Ok((blocks.to_vec(), markdown.to_owned(), stats));
    }
    let new_markdown = match markdown_anchor {
        Some(marker_pos) => {
            // raw_single: replace marker-to-end with the adopted tables. The
            // trigger qualification guarantees no non-table prose after the
            // marker. Multiple adopted tables concatenate (single-region
            // arm: exactly one).
            let mut updated = markdown[..marker_pos].to_owned();
            for html in &adopted_html {
                updated.push_str(html);
                updated.push('\n');
            }
            updated
        }
        None => {
            // empty: the whole markdown is the adopted tables.
            let mut updated = String::new();
            for html in &adopted_html {
                updated.push_str(html);
                updated.push('\n');
            }
            updated
        }
    };
    let mut new_blocks = blocks.to_vec();
    new_blocks.extend(adopted);
    Ok((new_blocks, new_markdown, stats))
}

// ---------------------------------------------------------------------------
// R2.4 orchestration: the per-page table-reread flow. Decode side is a
// port (TableRereadDecoder) so fakes drive tests and SessionRecognizer
// implements it in production (TABLE_REREAD_V1 prompt, 1024 grid).
// ---------------------------------------------------------------------------

/// Decoder port: reread one cropped table region, return its HTML.
pub trait TableRereadDecoder {
    fn reread_table(&mut self, crop: &crate::state::RenderedPage) -> std::io::Result<String>;
}

/// Per-page reread outcome counters (result-schema inputs, R2.6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RereadStats {
    pub pairing_skipped: bool,
    pub triggered: usize,
    pub decoded: usize,
    pub identity_blocked: usize,
    pub compare_rejected: usize,
    pub replaced: usize,
    /// BLK-6 (review 2026-11-18): per-table outcomes in reading order —
    /// page-level counters cannot attribute multi-table pages (the B2
    /// adjudicator consumes the persisted form).
    pub detail: Vec<RereadTableOutcome>,
    /// M4.1 (F6 missing-table branch): per-table region outcomes on
    /// zero-table-block pages, in region reading order.
    pub f6_detail: Vec<F6RegionOutcome>,
    /// F6 regions considered (post-ordering) on zero-table-block pages.
    pub f6_triggered: usize,
    /// F6 regions adopted (structural + attribution gates passed).
    pub f6_emitted: usize,
    /// F6 regions rejected by a gate.
    pub f6_rejected: usize,
    /// F6 emission shape was ineligible — the page keeps its text and no
    /// decode ran (D-53 v1.2).
    pub f6_skipped_shape: bool,
    /// F6 region ordering was ambiguous — whole-page skip (D-56).
    pub f6_ordering_ambiguous: bool,
}

/// One F6 region's outcome (region reading order; the frozen-population
/// spike adjudicates per region). M4.2: carries the detector confidence
/// (D-61 audit) and the engine id (D-63 identity chain).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct F6RegionOutcome {
    pub region_ordinal: usize,
    pub decoded: bool,
    pub adopted: bool,
    /// Frozen vocabulary: structural | attribution | empty_output |
    /// no_ruling_lines | engine_error | engine_down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<&'static str>,
    /// Detector confidence (D-61: recorded, never filtered in phase 1).
    pub conf: f64,
    /// Engine id from the verified handshake identity.
    pub engine: &'static str,
}

/// One table's reread outcome (reading-order ordinal; persisted as the
/// per-page reread detail artifact for exact B2 gate attribution).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RereadTableOutcome {
    pub table_ordinal: usize,
    pub triggered: bool,
    pub decoded: bool,
    pub replaced: bool,
}

/// Per-page orchestration (normal pages; the caller gates on the
/// `--table-reread` flag - off means this fn is never called). The provider
/// is scoped to THIS page by the caller.
///
/// Flow (plan R2.4): regions -> pairing verdict (integral skip) ->
/// F1-F5 trigger per block -> crop -> decode -> content identity (D-47) ->
/// compare-keep (strictly better) -> splice. Any skip = original blocks
/// unchanged.
pub fn orchestrate_table_reread(
    blocks: &[PageBlock],
    page_number: u32,
    page: &crate::state::RenderedPage,
    provider: &mut dyn RegionProvider,
    decoder: &mut dyn TableRereadDecoder,
    always_trigger: bool,
) -> std::io::Result<(Vec<PageBlock>, RereadStats)> {
    let mut stats = RereadStats::default();
    let regions = provider.regions_for_page(page_number);
    let output_n = blocks
        .iter()
        .filter(|b| block_class(b) == Some(RegionClass::Table))
        .count();
    // BLK-2 (review 2026-11-18): the verdict's READING ORDER must drive
    // crop AND splice — cropping the raw detection sequence pairs the k-th
    // output block with whichever region the detector emitted first, which
    // on multi-table pages re-reads a DIFFERENT table than the block.
    let table_regions: Vec<DetectedRegion> = match pairing_verdict(&regions, output_n) {
        Ok(order) => order.into_iter().map(|index| regions[index]).collect(),
        Err(_) => {
            stats.pairing_skipped = true;
            return Ok((blocks.to_vec(), stats));
        }
    };
    let page_area = f64::from(page.width) * f64::from(page.height);
    let mut replacements: std::collections::BTreeMap<usize, PageBlock> =
        std::collections::BTreeMap::new();
    // BLK-3: the ordinal cursor advances for EVERY table block
    // unconditionally — a skipped/identity-blocked/compare-rejected table
    // still consumed its region slot; a stale cursor mis-pairs every
    // following table.
    let mut table_block_position = 0usize;
    for block in blocks {
        if block_class(block) != Some(RegionClass::Table) {
            continue;
        }
        let html = match block {
            PageBlock::HtmlTable { html } => html,
            _ => unreachable!("block_class filtered"),
        };
        let region = &table_regions[table_block_position];
        let ordinal = table_block_position;
        table_block_position += 1;
        let mut outcome = RereadTableOutcome {
            table_ordinal: ordinal,
            triggered: false,
            decoded: false,
            replaced: false,
        };
        let area_ratio = region.area() / page_area.max(1.0);
        // BLK-4 (plan D-18 frozen): a BAD page (retry-committed,
        // attempts >= 2) always triggers the reread — the F1-F5
        // heuristics only gate the attempt-1 path. Identity and
        // compare-keep still guard every replacement.
        if always_trigger || table_reread_candidate(html, area_ratio) {
            outcome.triggered = true;
            stats.triggered += 1;
            let crop = crop_region(page, region.bbox)?;
            let reread_html = decoder.reread_table(&crop)?;
            outcome.decoded = true;
            stats.decoded += 1;
            match content_identity_pass(html, &reread_html) {
                Some(true) => {}
                _ => {
                    stats.identity_blocked += 1;
                    stats.detail.push(outcome);
                    continue;
                }
            }
            let original_shape = parse_table_shape(html);
            let reread_shape = parse_table_shape(&reread_html);
            if keep_original(&original_shape, &reread_shape) {
                stats.compare_rejected += 1;
                stats.detail.push(outcome);
                continue;
            }
            // keyed by the TABLE-BLOCK ordinal (position among table blocks
            // = region ordinal under the pairing one-to-one contract) - NOT
            // the all-block enumerate index, which shifts when non-table
            // blocks precede the first table.
            replacements.insert(ordinal, PageBlock::HtmlTable { html: reread_html });
            outcome.replaced = true;
            stats.replaced += 1;
        }
        stats.detail.push(outcome);
    }
    // splice with the ORDERED regions: replacement keys are reading-order
    // ordinals, so the pairing must walk the same order.
    let spliced = splice_m4m5(blocks, &table_regions, &replacements, RegionClass::Table)
        .map_err(std::io::Error::other)?;
    Ok((spliced, stats))
}

// ---------------------------------------------------------------------------
// R2.6 golden gate: the single post-processing artifact function. The
// window loop calls this ONLY when the pipeline is armed; the off path
// never constructs it (zero call by construction). R2.7's acceptance
// criteria (untouched bytes / diff exactly the target table / exporter
// readable / checkpoint hash) ride this fn.
// ---------------------------------------------------------------------------

/// Region provider scoped to one page: the window loop owns the document
/// provider; the orchestration expects the CURRENT page's regions.
pub(crate) struct PageScopedProvider<'a> {
    inner: &'a mut dyn RegionProvider,
    page: u32,
}

impl<'a> PageScopedProvider<'a> {
    pub(crate) fn new(inner: &'a mut dyn RegionProvider, page: u32) -> Self {
        Self { inner, page }
    }
}

impl RegionProvider for PageScopedProvider<'_> {
    fn regions_for_page(&mut self, page: u32) -> Vec<DetectedRegion> {
        debug_assert_eq!(page, self.page, "provider must be scoped to one page");
        self.inner.regions_for_page(self.page)
    }
}

/// The (table ordinal, reread html) pairs where the orchestration actually
/// replaced a block — the markdown sync inputs.
fn extract_replacements(old: &[PageBlock], new: &[PageBlock]) -> Vec<(usize, String)> {
    let mut replacements = Vec::new();
    let mut ordinal = 0usize;
    for (old_block, new_block) in old.iter().zip(new.iter()) {
        if let (PageBlock::HtmlTable { html: old_html }, PageBlock::HtmlTable { html: new_html }) =
            (old_block, new_block)
        {
            if old_html != new_html {
                replacements.push((ordinal, new_html.clone()));
            }
            ordinal += 1;
        }
    }
    replacements
}

/// Targeted markdown update (R2.7/D-41): replaces ONLY the `ordinal`-th
/// `<table>...</table>` fragment (nesting-aware scan; the frozen v1 model
/// contract emits no nested tables). `None` when the fragment does not
/// exist or is unterminated — the caller keeps the markdown and warns.
fn replace_table_fragment(markdown: &str, ordinal: usize, html: &str) -> Option<String> {
    let mut depth = 0usize;
    let mut fragment_start = None;
    let mut seen_tables = 0usize;
    let bytes = markdown.as_bytes();
    let mut fragment_end = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<table") {
            if depth == 0 {
                if seen_tables == ordinal {
                    fragment_start = Some(i);
                }
                seen_tables += 1;
            }
            depth += 1;
            i += 6;
            continue;
        }
        if bytes[i..].starts_with(b"</table>") {
            depth = depth.saturating_sub(1);
            if depth == 0 && fragment_start.is_some() {
                fragment_end = Some(i + b"</table>".len());
                break;
            }
            i += 8;
            continue;
        }
        i += 1;
    }
    let start = fragment_start?;
    let end = fragment_end?;
    let mut updated = String::with_capacity(markdown.len() + html.len());
    updated.push_str(&markdown[..start]);
    updated.push_str(html);
    updated.push_str(&markdown[end..]);
    Some(updated)
}

/// R2.6/R2.7: the ONE post-processing artifact fn — parse the artifact's
/// page_json, run the per-page orchestration, splice the document, sync
/// page_json, and apply the targeted markdown update. Only the target
/// table fragments change; everything else is byte-identical by
/// construction. Counters land on the artifact (result-schema inputs).
///
/// # Errors
///
/// page_json parse/serialize failures (a corrupted artifact is a page
/// error, never a silent skip).
pub fn apply_table_reread(
    rendered: &crate::state::RenderedPage,
    artifact: &mut crate::state::PageArtifactResult,
    page_number: u32,
    arms: &mut crate::app::RereadArms<'_>,
    vlm_decoder: Option<&mut crate::ocr::artifacts::SessionTableDecoder<'_>>,
    always_trigger: bool,
) -> std::io::Result<RereadStats> {
    // M4.2: the provider reborrow happens HERE (single arms borrow) — the
    // page scope serves both the F6 arm and the F1-F5 arm.
    let Some(provider) = arms.provider.as_deref_mut() else {
        // No regions sidecar: nothing can arm (the CLI requires the layout
        // inputs for either switch; this is the degraded-empty read path).
        return Ok(RereadStats::default());
    };
    let mut scoped = PageScopedProvider::new(provider, page_number);
    let mut document =
        ocrx_core::PageDocument::from_json(&artifact.page_json).map_err(|error| {
            std::io::Error::other(format!("reread: page_json parse failed: {error:?}"))
        })?;
    let original_blocks = document.blocks.clone();
    let page_number = document.page_number;
    let table_block_count = original_blocks
        .iter()
        .filter(|b| block_class(b) == Some(RegionClass::Table))
        .count();
    if table_block_count == 0 {
        // M4.2 F6: zero-table-block pages — the missing-table branch, run
        // ONLY when the structure engine is armed (D-64: old-flag-only runs
        // do NOT trigger F6 — the VLM F6 arm was removed with generation 3).
        // The emission-shape qualification and adoption gates live in
        // orchestrate_f6; the returned markdown replaces the artifact's.
        let Some(engine) = arms.structure.as_deref_mut() else {
            let mut stats = RereadStats::default();
            stats.f6_skipped_shape = true;
            return Ok(stats);
        };
        let regions = scoped.regions_for_page(page_number);
        let mut stats = RereadStats::default();
        if regions.is_empty() {
            stats.f6_skipped_shape = true;
            return Ok(stats);
        }
        let (new_blocks, new_markdown, f6_stats) = orchestrate_f6(
            &original_blocks,
            &artifact.markdown,
            page_number,
            rendered,
            &mut scoped,
            engine,
        )?;
        artifact.reread_detail_json = serde_json::to_string(&f6_stats.f6_detail)
            .map_err(|error| std::io::Error::other(format!("reread detail serialize: {error}")))?;
        if f6_stats.f6_emitted > 0 {
            document.blocks = new_blocks;
            artifact.page_json = document.to_json().map_err(|error| {
                std::io::Error::other(format!("reread: page_json write failed: {error:?}"))
            })?;
            artifact.markdown = new_markdown;
        }
        return Ok(f6_stats);
    }
    let Some(decoder) = vlm_decoder.map(|decoder| decoder as &mut dyn TableRereadDecoder) else {
        // F1-F5 disarmed (old flag off) with table blocks present: keep the
        // original blocks unchanged (byte-identical off path by construction).
        return Ok(RereadStats::default());
    };
    let (spliced, stats) = orchestrate_table_reread(
        &original_blocks,
        page_number,
        rendered,
        &mut scoped,
        decoder,
        always_trigger,
    )?;
    artifact.reread_tables = stats.decoded;
    artifact.reread_replaced = stats.replaced;
    if stats.pairing_skipped {
        return Ok(stats);
    }
    if stats.replaced == 0 {
        return Ok(stats);
    }
    document.blocks = spliced;
    artifact.page_json = document.to_json().map_err(|error| {
        std::io::Error::other(format!("reread: page_json write failed: {error:?}"))
    })?;
    for (ordinal, html) in extract_replacements(&original_blocks, &document.blocks) {
        match replace_table_fragment(&artifact.markdown, ordinal, &html) {
            Some(updated) => artifact.markdown = updated,
            None => artifact
                .warnings
                .push("reread_markdown_fragment_missing".to_owned()),
        }
    }
    Ok(stats)
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
    if reread_rank.0 != original_rank.0 {
        return reread_rank.0 < original_rank.0;
    }
    if reread_rank.1 != original_rank.1 {
        return reread_rank.1 < original_rank.1;
    }
    if (reread_rank.2 - original_rank.2).abs() >= 1e-9 {
        return reread_rank.2 < original_rank.2;
    }
    // Fill tie (within epsilon): the FOURTH element decides (review BLK-3 —
    // the previous implementation returned "keep" here, never comparing
    // rows×cols, contradicting the frozen lexicographic contract).
    reread_rank.3 <= original_rank.3
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
    // --- M4.1 F6 missing-table branch (plan v1.2) ---------------------------

    fn f6_region(ordinal: usize, bbox: (f64, f64, f64, f64)) -> DetectedRegion {
        DetectedRegion {
            class: RegionClass::Table,
            index_in_class: ordinal,
            bbox,
            conf: 0.9,
        }
    }

    fn f6_page() -> crate::state::RenderedPage {
        crate::state::RenderedPage::new(400, 400, vec![200; 400 * 400 * 3]).unwrap()
    }

    /// F6 engine fake: scripted HTML verdicts; records crop sizes decoded
    /// from the PNG IHDR (the production path sends PNG bytes).
    struct F6Engine {
        responses: Vec<String>,
        calls: std::cell::RefCell<Vec<(f64, f64)>>,
    }
    impl crate::structure_engine::StructureTableEngine for F6Engine {
        fn recognize(
            &mut self,
            png: &[u8],
            _page: u32,
            _ordinal: usize,
        ) -> Result<
            crate::structure_engine::StructureOutcome,
            crate::structure_engine::EngineErrorKind,
        > {
            let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
            let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
            self.calls.borrow_mut().push((f64::from(width), f64::from(height)));
            let i = self.calls.borrow().len() - 1;
            Ok(crate::structure_engine::StructureOutcome::Html(
                self.responses[i].clone(),
            ))
        }
    }

    struct F6Provider {
        regions: Vec<DetectedRegion>,
    }
    impl RegionProvider for F6Provider {
        fn regions_for_page(&mut self, _page: u32) -> Vec<DetectedRegion> {
            self.regions.clone()
        }
    }

    const GOOD_TABLE: &str =
        "<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>";

    #[test]
    fn f6_emission_shape_classifies_the_four_shapes() {
        // empty
        assert_eq!(f6_emission_shape("  \n ", 2), F6EmissionShape::Empty);
        // raw_single clean (marker, no tail prose, det 1)
        assert_eq!(
            f6_emission_shape("header\n<table><tr><td>1", 1),
            F6EmissionShape::RawSingle { marker_pos: 7 }
        );
        // tail prose
        assert_eq!(
            f6_emission_shape("header\n<table><tr><td>1\nSome trailing prose", 1),
            F6EmissionShape::Ineligible(F6SkipReason::RawTableWithTailProse)
        );
        // text only
        assert_eq!(
            f6_emission_shape("just prose\nmore prose", 1),
            F6EmissionShape::Ineligible(F6SkipReason::TextOnlyNoAnchor)
        );
        // multi-region raw
        assert_eq!(
            f6_emission_shape("<table><tr><td>1", 2),
            F6EmissionShape::Ineligible(F6SkipReason::MultiRegionRaw)
        );
    }

    #[test]
    fn f6_structural_gate_rejects_trivial_and_blank_tables() {
        assert!(f6_structural_gate(GOOD_TABLE));
        // 1x1: below the 2x2 floor
        assert!(!f6_structural_gate("<table><tr><td>a</td></tr></table>"));
        // unparsable
        assert!(!f6_structural_gate("<table>not a table"));
        // all-blank cells
        assert!(!f6_structural_gate(
            "<table><tr><td></td><td> </td></tr><tr><td>  </td><td></td></tr></table>"
        ));
    }

    #[test]
    fn f6_attribution_gate_calibrated_single_formula() {
        let attempt = "<table><tr><td>alpha beta gamma</td><td>delta epsilon</td></tr></table>";
        // A faithful reread recovers the attempt content and adds structure.
        let faithful = "<table><tr><th>alpha beta gamma</th><th>delta epsilon</th></tr></table>";
        assert!(f6_attribution_pass(attempt, faithful));
        // Generation-3 calibration: a CORRECT but much longer reread passes
        // (cov_attempt dominates; the removed cov_reread clause rejected
        // objectively good tables — T0 evidence, D-62).
        let long_faithful = "<table><tr><th>alpha beta gamma</th><th>delta epsilon</th></tr>                             <tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr>                             <tr><td>5</td><td>6</td></tr><tr><td>7</td><td>8</td></tr>                             <tr><td>9</td><td>10</td></tr></table>";
        assert!(f6_attribution_pass(attempt, long_faithful));
        // A hallucinated table shares nothing with the attempt.
        assert!(!f6_attribution_pass(
            attempt,
            "<table><tr><td>unrelated words entirely</td></tr></table>"
        ));
        // Below the 8-bigram floor: fail-closed.
        assert!(!f6_attribution_pass(
            "<table><tr><td>x</td></tr></table>",
            GOOD_TABLE
        ));
    }

    #[test]
    fn f6_order_regions_dedups_and_orders_by_reading_position() {
        let regions = vec![
            f6_region(0, (0.0, 200.0, 400.0, 400.0)),
            f6_region(1, (0.0, 0.0, 400.0, 200.0)),
            f6_region(2, (1.0, 201.0, 400.0, 400.0)), // dup of region 0
        ];
        let ordered = f6_order_regions(&regions).unwrap();
        assert_eq!(ordered.len(), 2);
        // reading order: the top region first
        assert!(ordered[0].bbox.1 == 0.0);
        assert!((ordered[1].bbox.1 - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn f6_order_regions_returns_none_on_ambiguity() {
        // two regions with overlapping y-bands (consecutive-y veto)
        let regions = vec![
            f6_region(0, (0.0, 0.0, 400.0, 300.0)),
            f6_region(1, (0.0, 100.0, 400.0, 400.0)),
        ];
        assert!(f6_order_regions(&regions).is_none());
    }

    #[test]
    fn f6_orchestration_empty_md_appends_adopted_tables() {
        let blocks: Vec<PageBlock> = vec![md("prose before")];
        let page = f6_page();
        let mut provider = F6Provider {
            regions: vec![f6_region(0, (0.0, 0.0, 400.0, 400.0))],
        };
        let mut engine = F6Engine {
            responses: vec![GOOD_TABLE.to_owned()],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let (new_blocks, new_md, stats) =
            orchestrate_f6(&blocks, "", 0, &page, &mut provider, &mut engine).unwrap();
        assert_eq!(stats.f6_triggered, 1);
        assert_eq!(stats.f6_emitted, 1);
        assert_eq!(stats.f6_rejected, 0);
        assert_eq!(new_blocks.len(), 2, "prose block + appended table");
        assert!(matches!(&new_blocks[1], PageBlock::HtmlTable { html } if html == GOOD_TABLE));
        assert_eq!(new_md.trim(), GOOD_TABLE, "empty md -> the table is the md");
        assert_eq!(stats.f6_detail.len(), 1);
        assert!(stats.f6_detail[0].adopted);
    }

    #[test]
    fn f6_orchestration_raw_single_replaces_marker_to_end() {
        let blocks: Vec<PageBlock> = vec![md("header prose")];
        let page = f6_page();
        let mut provider = F6Provider {
            regions: vec![f6_region(0, (0.0, 0.0, 400.0, 400.0))],
        };
        let attempt = "<table><tr><td>alpha beta gamma</td><td>delta epsilon</td></tr></table>";
        let mut engine = F6Engine {
            responses: vec![
                "<table><tr><th>alpha beta gamma</th><th>delta epsilon</th></tr><tr><td>1</td><td>2</td></tr></table>"
                    .to_owned(),
            ],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let md = format!("header prose\n{attempt}");
        let (new_blocks, new_md, stats) =
            orchestrate_f6(&blocks, &md, 0, &page, &mut provider, &mut engine).unwrap();
        assert_eq!(stats.f6_triggered, 1);
        assert_eq!(stats.f6_emitted, 1);
        // header prose preserved, marker-to-end replaced
        assert!(new_md.starts_with("header prose"));
        assert!(
            new_md.contains("<th>alpha beta gamma</th>"),
            "reread adopted"
        );
        assert!(
            !new_md.contains("<td>alpha beta gamma</td>"),
            "malformed attempt replaced"
        );
        assert_eq!(new_blocks.len(), 2);
        // 0-crop calls: 1 region -> 1 decode
        assert_eq!(engine.calls.borrow().len(), 1);
    }

    #[test]
    fn f6_orchestration_multi_region_raw_is_skipped() {
        let blocks: Vec<PageBlock> = vec![md("prose")];
        let page = f6_page();
        let mut provider = F6Provider {
            regions: vec![
                f6_region(0, (0.0, 0.0, 400.0, 200.0)),
                f6_region(1, (0.0, 200.0, 400.0, 400.0)),
            ],
        };
        let mut engine = F6Engine {
            responses: vec![],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let md = "<table><tr><td>1"; // raw marker but det_n = 2
        let (new_blocks, new_md, stats) =
            orchestrate_f6(&blocks, md, 0, &page, &mut provider, &mut engine).unwrap();
        assert!(stats.f6_skipped_shape);
        assert_eq!(stats.f6_triggered, 0);
        assert_eq!(new_blocks.len(), 1, "unchanged");
        assert_eq!(new_md, md, "markdown unchanged");
        assert!(engine.calls.borrow().is_empty(), "no decode ran");
    }

    #[test]
    fn f6_orchestration_structural_reject_keeps_text() {
        let blocks: Vec<PageBlock> = vec![md("prose")];
        let page = f6_page();
        let mut provider = F6Provider {
            regions: vec![f6_region(0, (0.0, 0.0, 400.0, 400.0))],
        };
        let mut engine = F6Engine {
            responses: vec!["<table><tr><td>x</td></tr></table>".to_owned()],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let (new_blocks, new_md, stats) =
            orchestrate_f6(&blocks, "", 0, &page, &mut provider, &mut engine).unwrap();
        assert_eq!(stats.f6_rejected, 1);
        assert_eq!(stats.f6_emitted, 0);
        assert_eq!(new_md, "", "rejected reread -> empty md kept");
        assert_eq!(new_blocks.len(), 1, "only the prose block");
        assert_eq!(stats.f6_detail[0].reject_reason, Some("structural"));
    }

    #[test]
    fn f6_orchestration_attribution_reject_keeps_text_raw_arm() {
        let blocks: Vec<PageBlock> = vec![md("header")];
        let page = f6_page();
        let mut provider = F6Provider {
            regions: vec![f6_region(0, (0.0, 0.0, 400.0, 400.0))],
        };
        let attempt = "<table><tr><td>alpha beta gamma</td><td>delta epsilon</td></tr></table>";
        let mut engine = F6Engine {
            responses: vec![
                "<table><tr><td>wholly unrelated</td><td>invented words</td></tr><tr><td>more invention</td><td>even more</td></tr></table>"
                    .to_owned(),
            ],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let md = format!("header\n{attempt}");
        let (new_blocks, new_md, stats) =
            orchestrate_f6(&blocks, &md, 0, &page, &mut provider, &mut engine).unwrap();
        assert_eq!(stats.f6_rejected, 1);
        assert_eq!(stats.f6_emitted, 0);
        assert_eq!(stats.f6_detail[0].reject_reason, Some("attribution"));
        assert_eq!(new_md, md, "raw arm: rejected -> attempt text kept");
        assert_eq!(new_blocks.len(), 1);
    }

    #[test]
    fn f6_orchestration_multi_region_empty_orders_by_reading_position() {
        let blocks: Vec<PageBlock> = vec![];
        let page = f6_page();
        let mut provider = F6Provider {
            regions: vec![
                f6_region(0, (0.0, 200.0, 400.0, 400.0)),
                f6_region(1, (0.0, 0.0, 400.0, 200.0)),
            ],
        };
        let bottom_table =
            "<table><tr><th>bottom</th><th>b2</th></tr><tr><td>1</td><td>2</td></tr></table>";
        let top_table =
            "<table><tr><th>top</th><th>t2</th></tr><tr><td>3</td><td>4</td></tr></table>";
        // responses arrive in decode order = reading order (top first)
        let mut engine = F6Engine {
            responses: vec![top_table.to_owned(), bottom_table.to_owned()],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let (new_blocks, new_md, stats) =
            orchestrate_f6(&blocks, "", 0, &page, &mut provider, &mut engine).unwrap();
        assert_eq!(stats.f6_triggered, 2);
        assert_eq!(stats.f6_emitted, 2);
        // reading order: top (y=0) decoded and emitted first
        let first_cells = new_md.find("top").unwrap();
        let bottom_cells = new_md.find("bottom").unwrap();
        assert!(first_cells < bottom_cells, "top table first in md");
        assert_eq!(new_blocks.len(), 2);
    }

    #[test]
    fn f6_apply_table_reread_dispatches_zero_block_pages() {
        // end-to-end through apply_table_reread: a zero-table-block page with
        // an eligible shape goes through the F6 branch, not F1-F5 pairing.
        let page = f6_page();
        let document = ocrx_core::PageDocument {
            schema_version: 1,
            page_number: 1,
            blocks: vec![md("prose before")],
            termination: ocrx_core::Termination::Eos,
            warnings: vec![],
        };
        let mut artifact = crate::state::PageArtifactResult {
            png: None,
            markdown: String::new(),
            page_json: document.to_json().unwrap(),
            truncated: false,
            degenerated: false,
            warnings: vec![],
            think_blocks_stripped: 0,
            raw_output: String::new(),
            reread_tables: 0,
            reread_replaced: 0,
            reread_detail_json: String::new(),
        };
        let mut provider = F6Provider {
            regions: vec![f6_region(0, (0.0, 0.0, 400.0, 400.0))],
        };
        let mut engine = F6Engine {
            responses: vec![GOOD_TABLE.to_owned()],
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let stats =
            super::apply_table_reread(
                &page,
                &mut artifact,
                1,
                &mut crate::app::RereadArms {
                    provider: Some(&mut provider),
                    vlm_armed: false,
                    structure: Some(&mut engine),
                },
                None,
                false,
            )
                .unwrap();
        assert_eq!(stats.f6_emitted, 1);
        assert!(artifact.page_json.contains(GOOD_TABLE));
        assert!(artifact.markdown.contains(GOOD_TABLE));
        assert!(artifact.reread_detail_json.contains("region_ordinal"));
    }

    use super::*;
    use crate::region_provider::EmptyRegionProvider;

    fn region(class: RegionClass, index: usize, bbox: (f64, f64, f64, f64)) -> DetectedRegion {
        DetectedRegion {
            class,
            index_in_class: index,
            bbox,
            conf: 0.9,
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
    fn compare_keep_fourth_element_decides_on_fill_tie() {
        // Review BLK-3: the frozen lexicographic contract compares rows×cols
        // when parses/attrs/fill tie — larger scale replaces, smaller keeps,
        // equal keeps (tie preserves the original).
        let base = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.8,
            rows_x_cols: (3, 4),
        };
        let larger = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.8,
            rows_x_cols: (4, 5),
        };
        let smaller = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.8,
            rows_x_cols: (2, 3),
        };
        let equal = TableShape {
            parses: true,
            structure_attrs: 2,
            cell_fill_ratio: 0.8,
            rows_x_cols: (3, 4),
        };
        assert!(
            !keep_original(&base, &larger),
            "fill tie + larger scale: replace"
        );
        assert!(
            keep_original(&base, &smaller),
            "fill tie + smaller scale: keep"
        );
        assert!(keep_original(&base, &equal), "full tie: keep original");
    }

    #[test]
    fn pairing_verdict_passes_clean_two_table_page() {
        // two vertically disjoint tables -> ordinals [0, 1]
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 100.0, 50.0)),
            region(RegionClass::Table, 1, (0.0, 200.0, 100.0, 250.0)),
        ];
        assert_eq!(pairing_verdict(&regions, 2), Ok(vec![0, 1]));
    }

    #[test]
    fn pairing_verdict_skips_count_mismatch() {
        // detection missed one of the two output tables
        let regions = vec![region(RegionClass::Table, 0, (0.0, 0.0, 100.0, 50.0))];
        assert_eq!(
            pairing_verdict(&regions, 2),
            Err(PairingSkip::CountMismatch {
                detection_n: 1,
                output_n: 2
            })
        );
    }

    #[test]
    fn pairing_verdict_skips_dedup_event() {
        // near-duplicate detections: ordinal holes => integral skip
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 100.0, 50.0)),
            region(RegionClass::Table, 1, (1.0, 1.0, 99.0, 49.0)),
        ];
        assert_eq!(pairing_verdict(&regions, 2), Err(PairingSkip::DedupEvent));
    }

    #[test]
    fn pairing_verdict_skips_when_row_and_column_orders_disagree() {
        // 2x2 grid: row-major [0,1,2,3] vs column-major [0,2,1,3] differ
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 45.0, 100.0)),
            region(RegionClass::Table, 1, (55.0, 0.0, 100.0, 100.0)),
            region(RegionClass::Table, 2, (0.0, 200.0, 45.0, 300.0)),
            region(RegionClass::Table, 3, (55.0, 200.0, 100.0, 300.0)),
        ];
        assert_eq!(
            pairing_verdict(&regions, 4),
            Err(PairingSkip::OrderingAmbiguous)
        );
    }

    #[test]
    fn pairing_verdict_skips_consecutive_y_overlap_veto() {
        // side-by-side tables whose y-ranges overlap (hairline stagger):
        // relative order is x-derived => ambiguous => skip
        let regions = vec![
            region(RegionClass::Table, 0, (0.0, 0.0, 100.0, 100.0)),
            region(RegionClass::Table, 1, (150.0, 5.0, 250.0, 105.0)),
        ];
        assert_eq!(
            pairing_verdict(&regions, 2),
            Err(PairingSkip::OrderingAmbiguous)
        );
    }

    #[test]
    fn content_identity_gate_separates_same_and_foreign() {
        let original = "<table><tr><td>alpha beta</td><td>gamma delta</td></tr><tr><td>epsilon zeta</td><td>eta theta</td></tr></table>";
        // near-identical reread (legit): proceeds to compare-keep
        let reread = "<table><tr><td>alpha beta</td><td>gamma delta</td></tr><tr><td>epsilon zeta</td><td>eta theta</td></tr></table>";
        assert_eq!(content_identity_pass(original, reread), Some(true));
        // fragment: only one cell survives (cov_a low => keep original)
        let fragment = "<table><tr><td>alpha beta</td></tr></table>";
        assert_eq!(content_identity_pass(original, fragment), Some(false));
        // foreign: disjoint content (both covs low => keep original)
        let foreign = "<table><tr><td>omega sigma</td><td>tau upsilon</td></tr></table>";
        assert_eq!(content_identity_pass(original, foreign), Some(false));
        // inconclusive: below the min-text floor => keep original
        assert_eq!(
            content_identity_pass(original, "<table><tr><td>x</td></tr></table>"),
            None
        );
    }

    #[test]
    fn normalized_bigrams_strips_tags_and_is_cjk_friendly() {
        let g = normalized_bigrams("<table><tr><td>面积</td></tr></table>");
        assert!(
            g.contains("面积") || !g.is_empty(),
            "CJK text produces bigrams"
        );
        let g2 = normalized_bigrams("<td>abc</td><td>abc</td>");
        assert!(g2.contains("ab") && g2.contains("bc"));
    }

    // --- R2.4 orchestration --------------------------------------------------

    struct StubDecoder {
        response: String,
    }
    impl TableRereadDecoder for StubDecoder {
        fn reread_table(&mut self, _crop: &crate::state::RenderedPage) -> std::io::Result<String> {
            Ok(self.response.clone())
        }
    }

    struct SingleTableProvider;
    impl RegionProvider for SingleTableProvider {
        fn regions_for_page(&mut self, _page: u32) -> Vec<DetectedRegion> {
            vec![region(RegionClass::Table, 0, (0.0, 0.0, 400.0, 100.0))]
        }
    }

    fn orchestration_page() -> crate::state::RenderedPage {
        crate::state::RenderedPage::new(400, 100, vec![200; 400 * 100 * 3]).unwrap()
    }

    fn triggered_block() -> PageBlock {
        // F2 fires: empty-cell ratio 3/6 = 0.5 >= 0.3; content is rich
        // enough for the identity gate (>= 8 unique bigrams after norm).
        PageBlock::HtmlTable {
            html: "<table><tr><td>alpha beta</td><td></td></tr><tr><td>gamma delta</td><td></td></tr><tr><td>epsilon zeta</td><td></td></tr></table>"
                .to_owned(),
        }
    }

    fn healthy_block() -> PageBlock {
        PageBlock::HtmlTable {
            html: "<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>"
                .to_owned(),
        }
    }

    // --- review 2026-11-18 BLK-2/3/4 regressions -----------------------------

    /// Provider returning regions in an explicit (possibly non-reading)
    /// order — the sidecar emits DETECTOR order.
    struct OrderedProvider {
        regions: Vec<DetectedRegion>,
    }
    impl RegionProvider for OrderedProvider {
        fn regions_for_page(&mut self, _page: u32) -> Vec<DetectedRegion> {
            self.regions.clone()
        }
    }

    /// Decoder recording each crop's (width, height) and returning the
    /// scripted responses in order.
    struct ScriptedDecoder {
        responses: Vec<String>,
        crops: std::cell::RefCell<Vec<(u32, u32)>>,
    }
    impl TableRereadDecoder for ScriptedDecoder {
        fn reread_table(&mut self, crop: &crate::state::RenderedPage) -> std::io::Result<String> {
            self.crops.borrow_mut().push((crop.width, crop.height));
            let index = self.crops.borrow().len() - 1;
            Ok(self.responses[index].clone())
        }
    }

    fn top_region() -> DetectedRegion {
        region(RegionClass::Table, 0, (0.0, 0.0, 400.0, 40.0))
    }
    fn bottom_region() -> DetectedRegion {
        region(RegionClass::Table, 1, (0.0, 40.0, 400.0, 100.0))
    }
    fn two_table_page() -> crate::state::RenderedPage {
        crate::state::RenderedPage::new(400, 100, vec![200; 400 * 100 * 3]).unwrap()
    }
    fn top_triggered_block() -> PageBlock {
        // F2 fires; text distinct from the bottom table (identity gate).
        PageBlock::HtmlTable {
            html: "<table><tr><td>top alpha</td><td></td></tr><tr><td>top beta</td><td></td></tr><tr><td>top gamma</td><td></td></tr></table>".to_owned(),
        }
    }
    fn bottom_triggered_block() -> PageBlock {
        PageBlock::HtmlTable {
            html: "<table><tr><td>bot alpha</td><td></td></tr><tr><td>bot beta</td><td></td></tr><tr><td>bot gamma</td><td></td></tr></table>".to_owned(),
        }
    }
    fn better_shape(content_a: &str, content_b: &str) -> String {
        format!(
            "<table><tr><th>{content_a}</th><th>{content_b}</th></tr><tr><td>r1</td><td>r2</td></tr></table>"
        )
    }

    #[test]
    fn blk2_reading_order_drives_crop_and_splice_not_detector_order() {
        // Detection order = [bottom, top]; blocks in reading order = [top,
        // bottom]. The pairing verdict must reorder: block 0 pairs the TOP
        // region (crop 400x40) and block 1 the BOTTOM region (400x60), and
        // the replacement keys follow the same order.
        let blocks = vec![top_triggered_block(), bottom_triggered_block()];
        let page = two_table_page();
        let mut provider = OrderedProvider {
            regions: vec![bottom_region(), top_region()],
        };
        let mut decoder = ScriptedDecoder {
            responses: vec![
                better_shape("top alpha", "top beta"),
                better_shape("bot alpha", "bot beta"),
            ],
            crops: std::cell::RefCell::new(Vec::new()),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.replaced, 2);
        // Crops in READING order: top region first (height 40).
        let crops = decoder.crops.borrow();
        assert_eq!(crops.len(), 2);
        assert_eq!(crops[0], (400, 40), "first crop = the TOP region");
        assert_eq!(crops[1], (400, 60), "second crop = the BOTTOM region");
        // Each block got ITS OWN reread content (never the other's).
        assert!(
            matches!(&spliced[0], PageBlock::HtmlTable { html } if html.contains("top alpha") && html.contains("<th>")),
            "block 0 keeps its own reread"
        );
        assert!(
            matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("bot alpha") && html.contains("<th>")),
            "block 1 keeps its own reread"
        );
        // Per-table detail: ordinals follow reading order.
        assert_eq!(
            stats.detail,
            vec![
                RereadTableOutcome {
                    table_ordinal: 0,
                    triggered: true,
                    decoded: true,
                    replaced: true
                },
                RereadTableOutcome {
                    table_ordinal: 1,
                    triggered: true,
                    decoded: true,
                    replaced: true
                },
            ]
        );
    }

    #[test]
    fn blk3_healthy_first_never_consumes_the_second_tables_reread() {
        // The reviewer's minimal repro: table 0 healthy (no trigger), table
        // 1 triggered. The cursor must still advance for table 0 — with the
        // stale-cursor bug table 1 read table 0's region AND table 0's
        // block was replaced with table 1's reread.
        let blocks = vec![healthy_block(), bottom_triggered_block()];
        let page = two_table_page();
        let mut provider = OrderedProvider {
            regions: vec![top_region(), bottom_region()],
        };
        let mut decoder = ScriptedDecoder {
            responses: vec![better_shape("bot alpha", "bot beta")],
            crops: std::cell::RefCell::new(Vec::new()),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.replaced, 1);
        assert!(!stats.detail[0].triggered);
        assert!(stats.detail[1].replaced);
        // The HEALTHY table stays byte-identical; the TRIGGERED table got
        // the reread.
        assert_eq!(spliced[0], blocks[0], "healthy table untouched");
        assert!(
            matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("bot alpha") && html.contains("<th>")),
            "triggered table replaced"
        );
        // The single crop is the BOTTOM region (400x60), not the top.
        assert_eq!(decoder.crops.borrow().as_slice(), &[(400, 60)]);
    }

    #[test]
    fn blk3_identity_blocked_first_still_advances_the_cursor() {
        let blocks = vec![top_triggered_block(), bottom_triggered_block()];
        let page = two_table_page();
        let mut provider = OrderedProvider {
            regions: vec![top_region(), bottom_region()],
        };
        // First response = FOREIGN content (identity gate blocks it).
        let mut decoder = ScriptedDecoder {
            responses: vec![
                better_shape("foreign one", "foreign two"),
                better_shape("bot alpha", "bot beta"),
            ],
            crops: std::cell::RefCell::new(Vec::new()),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.identity_blocked, 1);
        assert_eq!(stats.replaced, 1);
        assert_eq!(spliced[0], blocks[0], "identity-blocked table untouched");
        assert!(
            matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("bot alpha")),
            "second table still replaced"
        );
        // Both crops happened, in reading order.
        assert_eq!(decoder.crops.borrow().as_slice(), &[(400, 40), (400, 60)]);
    }

    #[test]
    fn blk3_compare_rejected_first_still_advances_the_cursor() {
        let blocks = vec![top_triggered_block(), bottom_triggered_block()];
        let page = two_table_page();
        let mut provider = OrderedProvider {
            regions: vec![top_region(), bottom_region()],
        };
        // First response = same-or-worse shape (compare-keep rejects).
        let mut decoder = ScriptedDecoder {
            responses: vec![
                "<table><tr><td>top alpha</td><td></td></tr><tr><td>top beta</td><td></td></tr><tr><td>top gamma</td><td></td></tr></table>".to_owned(),
                better_shape("bot alpha", "bot beta"),
            ],
            crops: std::cell::RefCell::new(Vec::new()),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.compare_rejected, 1);
        assert_eq!(stats.replaced, 1);
        assert_eq!(spliced[0], blocks[0], "compare-rejected table untouched");
        assert!(
            matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("bot alpha")),
            "second table still replaced"
        );
    }

    #[test]
    fn blk4_always_trigger_bypasses_f_gates_but_keeps_identity_and_compare() {
        // Bad-page contract (attempts >= 2): the HEALTHY table triggers too
        // — yet a compare-keep rejection still keeps the original (恒触发
        // 不替换 is a legal outcome).
        let blocks = vec![healthy_block(), bottom_triggered_block()];
        let page = two_table_page();
        let mut provider = OrderedProvider {
            regions: vec![top_region(), bottom_region()],
        };
        let mut decoder = ScriptedDecoder {
            responses: vec![
                // Same shape as the original healthy table → compare keeps.
                "<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>"
                    .to_owned(),
                better_shape("bot alpha", "bot beta"),
            ],
            crops: std::cell::RefCell::new(Vec::new()),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, true).unwrap();
        assert_eq!(stats.triggered, 2, "healthy table also triggers");
        assert_eq!(stats.decoded, 2);
        // The healthy table's reread is IDENTITY-blocked (its normalized
        // text has < 8 unique bigrams → inconclusive → keep); the guard
        // chain stays fully armed on the bad-page path.
        assert_eq!(stats.identity_blocked, 1);
        assert_eq!(stats.compare_rejected, 0);
        assert_eq!(stats.replaced, 1);
        assert_eq!(spliced[0], blocks[0]);
        assert!(matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("bot alpha")));
    }

    #[test]
    fn blk4_without_always_trigger_the_f_gates_still_gate() {
        let blocks = vec![healthy_block(), bottom_triggered_block()];
        let page = two_table_page();
        let mut provider = OrderedProvider {
            regions: vec![top_region(), bottom_region()],
        };
        let mut decoder = ScriptedDecoder {
            responses: vec![better_shape("bot alpha", "bot beta")],
            crops: std::cell::RefCell::new(Vec::new()),
        };
        let (_, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.triggered, 1, "healthy table does not trigger");
        assert_eq!(stats.decoded, 1);
    }

    #[test]
    fn orchestration_replaces_triggered_table_passing_all_gates() {
        let blocks = vec![md("prose"), triggered_block(), md("tail")];
        let page = orchestration_page();
        let mut provider = SingleTableProvider;
        let mut decoder = StubDecoder {
            response: "<table><tr><th>alpha beta</th><th>gamma delta</th></tr><tr><td>epsilon zeta</td><td>eta theta</td><td>iota kappa</td></tr></table>".to_owned(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.triggered, 1);
        assert_eq!(stats.decoded, 1);
        assert_eq!(stats.identity_blocked, 0);
        assert_eq!(stats.compare_rejected, 0);
        assert_eq!(stats.replaced, 1);
        assert_eq!(spliced.len(), 3);
        assert!(
            matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("<th>alpha beta</th>")),
            "position 1 replaced with the reread table"
        );
        assert_eq!(spliced[0], md("prose"), "non-table blocks byte-identical");
        assert_eq!(spliced[2], md("tail"));
    }

    #[test]
    fn orchestration_untouched_when_trigger_never_fires() {
        let blocks = vec![healthy_block()];
        let page = orchestration_page();
        let mut provider = SingleTableProvider;
        let mut decoder = StubDecoder {
            response: "<table></table>".to_owned(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.triggered, 0);
        assert_eq!(stats.decoded, 0);
        assert_eq!(spliced, blocks, "healthy table: original preserved");
    }

    #[test]
    fn orchestration_pairs_count_mismatch_to_whole_page_skip() {
        // provider has 2 regions, page has 1 table block => count mismatch
        struct TwoRegionProvider;
        impl RegionProvider for TwoRegionProvider {
            fn regions_for_page(&mut self, _page: u32) -> Vec<DetectedRegion> {
                vec![
                    region(RegionClass::Table, 0, (0.0, 0.0, 400.0, 40.0)),
                    region(RegionClass::Table, 1, (0.0, 60.0, 400.0, 99.0)),
                ]
            }
        }
        let blocks = vec![triggered_block()];
        let page = orchestration_page();
        let mut provider = TwoRegionProvider;
        let mut decoder = StubDecoder {
            response: "<table><tr><td>z</td></tr></table>".to_owned(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert!(stats.pairing_skipped);
        assert_eq!(stats.decoded, 0, "pairing skip: decode never runs");
        assert_eq!(spliced, blocks);
    }

    #[test]
    fn orchestration_identity_gate_blocks_foreign_reread() {
        let blocks = vec![triggered_block()];
        let page = orchestration_page();
        let mut provider = SingleTableProvider;
        let mut decoder = StubDecoder {
            response: "<table><tr><td>omega sigma</td><td>tau upsilon</td><td>phi chi</td><td>psi omega</td></tr></table>".to_owned(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.triggered, 1);
        assert_eq!(stats.identity_blocked, 1);
        assert_eq!(stats.replaced, 0);
        assert_eq!(spliced, blocks);
    }

    #[test]
    fn orchestration_compare_reject_worse_reread() {
        // triggered original; reread is a FRAGMENT (identity passes because
        // the fragment is a subset of the original, cov_a high) but the
        // four-tuple is smaller => compare keeps the original
        let blocks = vec![triggered_block()];
        let page = orchestration_page();
        let mut provider = SingleTableProvider;
        let mut decoder = StubDecoder {
            response: "<table><tr><td>alpha beta</td><td></td></tr><tr><td>gamma delta</td><td></td></tr></table>".to_owned(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.triggered, 1);
        assert_eq!(stats.decoded, 1);
        assert_eq!(
            stats.identity_blocked, 0,
            "subset passes identity (cov_a >= 0.4)"
        );
        assert_eq!(
            stats.compare_rejected, 1,
            "smaller rows x cols => keep original"
        );
        assert_eq!(stats.replaced, 0);
        assert_eq!(spliced, blocks);
    }

    #[test]
    fn orchestration_keys_replacements_by_table_ordinal_not_block_index() {
        // mixed page: markdown BEFORE the table => table block_index 1 but
        // table ordinal 0. The replacement must land on the table (this
        // regression guards the v1.5-keying bug: keying by all-block index
        // shifted when non-table blocks preceded).
        let blocks = vec![md("lead"), triggered_block()];
        let page = orchestration_page();
        let mut provider = SingleTableProvider;
        let mut decoder = StubDecoder {
            response: "<table><tr><th>alpha beta</th><th>gamma delta</th></tr><tr><td>epsilon zeta</td><td>eta theta</td><td>iota kappa</td></tr></table>".to_owned(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert_eq!(stats.replaced, 1);
        assert!(
            matches!(&spliced[1], PageBlock::HtmlTable { html } if html.contains("<th>alpha beta</th>")),
            "mixed page: the TABLE block is replaced, not shifted"
        );
        assert_eq!(spliced[0], md("lead"));
    }

    #[test]
    fn orchestration_provider_degradation_keeps_original() {
        let blocks = vec![triggered_block()];
        let page = orchestration_page();
        let mut provider = EmptyRegionProvider;
        let mut decoder = StubDecoder {
            response: String::new(),
        };
        let (spliced, stats) =
            orchestrate_table_reread(&blocks, 0, &page, &mut provider, &mut decoder, false)
                .unwrap();
        assert!(
            stats.pairing_skipped,
            "count mismatch (0 regions vs 1 block)"
        );
        assert_eq!(spliced, blocks);
    }

    #[test]
    fn crop_region_exacts_geometry_and_content() {
        // 4x3 page, unique byte per pixel: rows differ, columns differ.
        let mut rgb8 = Vec::new();
        for y in 0..3_u32 {
            for x in 0..4_u32 {
                rgb8.push(u8::try_from(y * 10 + x).unwrap());
                rgb8.push(0);
                rgb8.push(0);
            }
        }
        let page = crate::state::RenderedPage::new(4, 3, rgb8).unwrap();
        let crop = crop_region(&page, (1.0, 1.0, 3.0, 3.0)).unwrap();
        assert_eq!((crop.width, crop.height), (2, 2));
        assert_eq!(crop.rgb8, vec![11, 0, 0, 12, 0, 0, 21, 0, 0, 22, 0, 0]);
        assert_eq!(crop.generation, page.generation);
        assert_eq!(crop.audit, page.audit);
    }

    #[test]
    fn crop_region_clamps_and_rejects_degenerate() {
        let page = crate::state::RenderedPage::new(4, 3, vec![7; 4 * 3 * 3]).unwrap();
        // clamped full page
        let crop = crop_region(&page, (-5.0, -5.0, 99.0, 99.0)).unwrap();
        assert_eq!((crop.width, crop.height), (4, 3));
        // degenerate: x0 == x1 after clamp
        assert!(crop_region(&page, (2.0, 0.0, 2.0, 3.0)).is_err());
        // fully outside on the low side clamps to a 1px column (not degenerate)
        let crop = crop_region(&page, (-9.0, 0.0, -3.0, 3.0));
        assert!(crop.is_ok() && crop.unwrap().width == 1);
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
