//! One-off raw-output probe (root-cause plan SP-T01/T02, direct side):
//! renders pages IN-PROCESS (no pdfium-owner handoff), generates, and
//! records `BoundarySample` JSONL records with the exact schema the scan
//! side emits — the E1 join compares the two files.
//!
//! Page identity convention (plan BLOCK-1, frozen): `OCRX_PROBE_PAGES`
//! takes 1-BASED page numbers (matching scan/checkpoint spellings); the
//! render index is computed as `page_number - 1` and both fields are
//! recorded in every sample.
//!
//! Env-gated (`OCRX_E2E_MODEL=1`, installed ovisocr3 assets). Extra knobs:
//! - `OCRX_PROBE_PAGES`  comma list of 1-based pages (default "4")
//! - `OCRX_PROBE_DPI`    render DPI (default 300)
//! - `OCRX_PROBE_GRID`   mtmd image-token budget (default 2048)
//! - `OCRX_BOUNDARY_SAMPLES_OUT` JSONL sink (default
//!   `docs/develop/spike-assets/boundary-direct.jsonl`)

use llama_cpp_2::mtmd::mtmd_default_marker;
use ocrx_pdf::{PdfiumRenderer, RenderOptions};
use ocrx_runtime::{LlamaMtmdRuntime, Runtime};
use serde::Serialize;
use std::path::PathBuf;

const PROMPT: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.";

/// The direct-side boundary record (schema mirrors the scan side's
/// `ocrx_scan::boundary::BoundarySample`; duplicated here because the
/// runtime test crate must not depend on the scan crate).
#[derive(Debug, Clone, Serialize)]
struct BoundarySample {
    input_sha256: String,
    run_id: String,
    source: &'static str,
    page_number: u32,
    page_index: u32,
    attempt: u32,
    window_id: u32,
    slot: u32,
    rgb8_len: usize,
    rgb8_sha256: String,
    prompt_sha256: String,
    width: u32,
    height: u32,
    max_output_tokens: u32,
    image_tokens_override: Option<u32>,
    retry_sampling: Option<(u32, u32)>,
    outcome: &'static str,
}

/// Builds one direct-side record. `page_number` is 1-based (canonical);
/// the render index is derived here so the -1 conversion lives in exactly
/// one place per side.
#[allow(clippy::too_many_arguments)]
fn boundary_sample(
    input_sha256: &str,
    run_id: &str,
    page_number: u32,
    slot: u32,
    width: u32,
    height: u32,
    rgb8: &[u8],
    max_output_tokens: u32,
    image_tokens_override: Option<u32>,
    outcome_empty: bool,
) -> BoundarySample {
    BoundarySample {
        input_sha256: input_sha256.to_owned(),
        run_id: run_id.to_owned(),
        source: "direct",
        page_number,
        page_index: page_number.saturating_sub(1),
        attempt: 1,
        window_id: 0,
        slot,
        rgb8_len: rgb8.len(),
        rgb8_sha256: ocrx_core::sha256(rgb8),
        prompt_sha256: ocrx_core::sha256(PROMPT.as_bytes()),
        width,
        height,
        max_output_tokens,
        image_tokens_override,
        retry_sampling: None,
        outcome: if outcome_empty { "empty" } else { "nonempty" },
    }
}

fn repo_root() -> PathBuf {
    std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

#[test]
fn boundary_sample_records_both_page_fields_with_explicit_conversion() {
    // SP-T01 (plan BLOCK-1): the record must carry the 1-based canonical
    // page number AND the derived 0-based render index — the v1.0 probe's
    // index/number confusion is exactly what this pins down.
    let sample = boundary_sample(
        "input-hash",
        "run-1",
        4,
        0,
        2531,
        3276,
        &[1, 2, 3],
        16_384,
        Some(2048),
        false,
    );
    assert_eq!(sample.page_number, 4);
    assert_eq!(sample.page_index, 3);
    assert_eq!(sample.source, "direct");
    assert_eq!(sample.attempt, 1);
    assert_eq!(sample.rgb8_sha256.len(), 64);
    let json = serde_json::to_string(&sample).unwrap();
    assert!(json.contains("\"page_number\":4"));
    assert!(json.contains("\"page_index\":3"));
    assert!(json.contains("\"outcome\":\"nonempty\""));
    // Empty outcome spelling.
    let sample = boundary_sample(
        "input-hash",
        "run-1",
        4,
        0,
        1,
        1,
        &[1, 2, 3],
        16_384,
        None,
        true,
    );
    assert_eq!(sample.outcome, "empty");
    assert_eq!(sample.image_tokens_override, None);
}

#[test]
#[ignore = "env-gated: OCRX_E2E_MODEL=1 with installed ovisocr3 assets"]
fn raw_output_probe_for_empty_pages() {
    if std::env::var_os("OCRX_E2E_MODEL").is_none() {
        eprintln!("skipped");
        return;
    }
    let dpi: u16 = std::env::var("OCRX_PROBE_DPI")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let grid: u32 = std::env::var("OCRX_PROBE_GRID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2048);
    let pages: Vec<u32> = std::env::var("OCRX_PROBE_PAGES")
        .or_else(|_| std::env::var("OCRX_PROBE_PAGE"))
        .map(|value| {
            value
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect()
        })
        .unwrap_or_else(|_| vec![4]);

    let root = repo_root();
    let relative = "assets/regression/ovisocr2-draft-v1/inputs/source.pdf";
    let mut candidates = vec![root.join(relative)];
    if let Some(above) = root.parent().and_then(|d| d.parent()) {
        candidates.push(above.join(relative));
    }
    let source = candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .expect("golden composite available");
    let pdfium = if let Some(dir) = std::env::var_os("OCRX_PDFIUM_LIB_DIR") {
        PathBuf::from(dir)
    } else {
        let vendored = root.join("vendor/pdfium/bin");
        if vendored.join("pdfium.dll").is_file() {
            vendored
        } else {
            root.parent()
                .and_then(|d| d.parent())
                .map(|p| p.join("vendor/pdfium/bin"))
                .filter(|p| p.join("pdfium.dll").is_file())
                .expect("pdfium")
        }
    };
    let renderer = PdfiumRenderer::new(std::fs::canonicalize(pdfium).unwrap());
    let cache = ocrx_models::default_cache_root();
    let rev = "7be644a312005c3a55e2c826aee985a5c93d06b2";
    let dir = cache.join("ocrx/models/ovisocr3").join(rev);
    let mut runtime = LlamaMtmdRuntime::load(
        dir.join("quality/model.gguf"),
        dir.join("mmproj.gguf"),
        false,
        grid,
        20_480,
        8,
    )
    .expect("load");

    let run_id = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        std::process::id()
    );
    let input_sha256 = ocrx_core::sha256_file(&source).expect("source readable");
    let sink = std::env::var_os("OCRX_BOUNDARY_SAMPLES_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("docs/develop/spike-assets/boundary-direct.jsonl"));

    for (slot, page_number) in pages.iter().enumerate() {
        // FROZEN convention: page_number is 1-based; the render index is
        // page_number - 1 (explicit, recorded in the sample).
        let page_index = page_number.saturating_sub(1);
        let (image, _, _) = renderer
            .render_page_rgb8_with_audit(&source, page_index as usize, RenderOptions { dpi })
            .unwrap_or_else(|error| panic!("render page {page_number}: {error}"));
        let width = image.width();
        let height = image.height();
        let rgb8 = image.into_raw();
        let request =
            ocrx_runtime::InferenceRequest::new(*page_number, width, height, rgb8.clone(), 16_384)
                .unwrap();
        let response = runtime
            .generate_with_progress(request, &ocrx_core::CancellationToken::new(), &mut |_| {})
            .expect("generate");
        let sample = boundary_sample(
            &input_sha256,
            &run_id,
            *page_number,
            u32::try_from(slot).unwrap_or(u32::MAX),
            width,
            height,
            &rgb8,
            16_384,
            None,
            response.raw_output.trim().is_empty(),
        );
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink)
            .expect("boundary sink");
        writeln!(file, "{}", serde_json::to_string(&sample).unwrap()).expect("sample write");
        println!(
            "PROBE dpi={dpi} page={page_number} grid={grid} finish={} raw_len={} rgb8_sha256={}",
            response.finish_reason,
            response.raw_output.len(),
            sample.rgb8_sha256
        );
        println!("==== RAW START ====");
        println!("{}", response.raw_output);
        println!("==== RAW END ====");
        let _ = mtmd_default_marker();
    }
}
