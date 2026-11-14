//! One-off raw-output probe (O3-T26 attribution): render one composite page
//! and print the RAW (pre-strip) model output — env-gated.

use ocrx_pdf::{PdfiumRenderer, RenderOptions};
use ocrx_runtime::{LlamaMtmdRuntime, Runtime};
use std::path::PathBuf;

const PROMPT: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.";

fn repo_root() -> PathBuf {
    std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
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
    let page: usize = std::env::var("OCRX_PROBE_PAGE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let grid: u32 = std::env::var("OCRX_PROBE_GRID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2048);

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
    let (image, _, _) = renderer
        .render_page_rgb8_with_audit(&source, page, RenderOptions { dpi })
        .expect("render");
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
    let request = ocrx_runtime::InferenceRequest::new(
        1,
        image.width(),
        image.height(),
        image.into_raw(),
        16_384,
    )
    .unwrap();
    let response = runtime
        .generate_with_progress(request, &ocrx_core::CancellationToken::new(), &mut |_| {})
        .expect("generate");
    println!(
        "PROBE dpi={dpi} page={page} grid={grid} finish={} raw_len={}",
        response.finish_reason,
        response.raw_output.len()
    );
    println!("==== RAW START ====");
    println!("{}", response.raw_output);
    println!("==== RAW END ====");
    // Context-reuse leg: generate a SECOND page on the SAME runtime to
    // expose cross-page SSM/KV state pollution (O3-T26 attribution).
    let second: usize = std::env::var("OCRX_PROBE_PAGE2")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(page);
    let (image2, _, _) = renderer
        .render_page_rgb8_with_audit(&source, second, RenderOptions { dpi })
        .expect("render 2");
    let request2 = ocrx_runtime::InferenceRequest::new(
        2,
        image2.width(),
        image2.height(),
        image2.into_raw(),
        16_384,
    )
    .unwrap();
    let response2 = runtime
        .generate_with_progress(request2, &ocrx_core::CancellationToken::new(), &mut |_| {})
        .expect("generate 2");
    println!(
        "PROBE2 dpi={dpi} page={second} grid={grid} finish={} raw_len={}",
        response2.finish_reason,
        response2.raw_output.len()
    );
    println!("==== RAW2 START ====");
    println!("{}", response2.raw_output);
    println!("==== RAW2 END ====");
}
