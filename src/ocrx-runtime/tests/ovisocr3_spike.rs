//! OvisOCR3 integration spike (plan Phase 0, O3-T02/T03/T04).
//!
//! Env-gated (never runs in `cargo test` by default): set `OCRX_E2E_MODEL=1`.
//! Requires the OvisOCR3 GGUF assets in the model cache at the frozen layout
//! (`<cache>/ocrx/models/ovisocr3/<revision>/quality/model.gguf` plus the
//! shared `mmproj.gguf`), the vendored PDFium library, and the golden-set
//! composite at `assets/regression/ovisocr2-draft-v1/inputs/source.pdf`.
//!
//! Probes (read-only, no production code changes):
//! - T02: the frozen llama.cpp pin (wrapper/sys 0.1.161, upstream
//!   `d8123504…`) loads the OvisOCR3 GGUF + mmproj through the production
//!   actor and generates on real rendered pages.
//! - T03: output-shape comparison between the frozen `OCR_MARKDOWN_V1`
//!   prompt bytes and the official `document_markdown` prompt bytes —
//!   `<think>` leakage, bbox img tags, HTML tables, LaTeX, degenerate tails.
//!   The record carries the runtime-final MTMD input text (marker + prompt)
//!   for the D-2 branch decision.
//! - T04: `image_tokens_override = 2048` acceptance through the actor
//!   (D-5 decision input).
//!
//! The JSON record goes to `OCRX_SPIKE_OUT` (a file path) or
//! `docs/develop/spike-assets/ovisocr3-spike-record.json` in the repo.

use llama_cpp_2::mtmd::mtmd_default_marker;
use ocrx_pdf::{PdfiumRenderer, RenderOptions};
use ocrx_runtime::{LlamaMtmdRuntime, Runtime};
use std::path::{Path, PathBuf};

/// Frozen OvisOCR2 prompt bytes (`ocrx-scan/src/ocr/prompt.rs`, leading and
/// middle newline; inlined here because ocrx-runtime must not depend on
/// ocrx-scan — byte equality is asserted by the scan crate's own golden).
const OCR_MARKDOWN_V1: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.";

/// Official OvisOCR3 `prompts/document_markdown.txt` bytes (fetched from the
/// model repo at spike time; stored in
/// `docs/develop/spike-assets/ovisocr3-document_markdown.txt`).
const OVISOCR3_DOCUMENT_MARKDOWN: &str = "Extract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown. Preserve the original text without translation or paraphrasing.";

/// Frozen OvisOCR3 GGUF supply chain (O3-T01, michalland/OvisOCR3-GGUF).
const OVISOCR3_REVISION: &str = "7be644a312005c3a55e2c826aee985a5c93d06b2";
const OVISOCR3_MMPROJ_SHA256: &str =
    "698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd";
const OVISOCR3_MMPROJ_BYTES: u64 = 204_987_136;
const OVISOCR3_Q8_SHA256: &str = "11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6";
const OVISOCR3_Q8_BYTES: u64 = 811_843_712;

const PAGE_COUNT: usize = 3;
const DPI: u16 = 150;
/// Probe cap: shape evidence does not need the full 16384 budget (CPU decode).
const PROBE_MAX_TOKENS: u32 = 384;

fn skip_reason() -> Option<&'static str> {
    if std::env::var_os("OCRX_E2E_MODEL").is_none() {
        return Some("set OCRX_E2E_MODEL=1 to run the OvisOCR3 spike");
    }
    None
}

fn resolve_model_paths() -> Result<(PathBuf, PathBuf, u64, u64), String> {
    let cache = std::env::var_os("OCRX_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(ocrx_models::default_cache_root);
    let revision_dir = cache
        .join("ocrx")
        .join("models")
        .join("ovisocr3")
        .join(OVISOCR3_REVISION);
    let model = revision_dir.join("quality").join("model.gguf");
    let mmproj = revision_dir.join("mmproj.gguf");
    let model_bytes = std::fs::metadata(&model)
        .map_err(|error| format!("model asset missing: {error}"))?
        .len();
    let mmproj_bytes = std::fs::metadata(&mmproj)
        .map_err(|error| format!("mmproj asset missing: {error}"))?
        .len();
    Ok((model, mmproj, model_bytes, mmproj_bytes))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pdfium_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OCRX_PDFIUM_LIB_DIR") {
        return PathBuf::from(dir);
    }
    // The gitignored vendored dll lives in the main checkout root: the repo
    // root itself, or two levels up for a nested worktree. Canonicalize the
    // root first — CARGO_MANIFEST_DIR/../.. keeps literal `..` components,
    // and the renderer refuses parent-dir traversal in library paths.
    let root = std::fs::canonicalize(repo_root()).unwrap_or_else(|_| repo_root());
    let mut candidates = vec![root.join("vendor/pdfium/bin")];
    if let Some(above) = root.parent().and_then(|dir| dir.parent()) {
        candidates.push(above.join("vendor/pdfium/bin"));
    }
    for candidate in candidates {
        let is_dir =
            candidate.join("pdfium.dll").is_file() || candidate.join("libpdfium.dylib").is_file();
        if is_dir {
            return std::fs::canonicalize(&candidate).unwrap_or(candidate);
        }
    }
    root.join("vendor/pdfium/bin")
}

fn render_pages() -> Vec<(u32, u32, Vec<u8>)> {
    let pdfium_dir = pdfium_dir();
    // The non-redistributable composite is gitignored: it lives in the main
    // checkout root (the repo root itself, or two levels up from a nested
    // worktree).
    let root = std::fs::canonicalize(repo_root()).unwrap_or_else(|_| repo_root());
    let mut candidates = vec![root.join("assets/regression/ovisocr2-draft-v1/inputs/source.pdf")];
    if let Some(above) = root.parent().and_then(|dir| dir.parent()) {
        candidates.push(above.join("assets/regression/ovisocr2-draft-v1/inputs/source.pdf"));
    }
    let source = candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .unwrap_or_else(|| candidates.remove(0));
    let renderer = PdfiumRenderer::new(pdfium_dir);
    (0..PAGE_COUNT)
        .map(|index| {
            let (image, _, _) = renderer
                .render_page_rgb8_with_audit(
                    &source,
                    index,
                    RenderOptions {
                        dpi: DPI,
                        ..Default::default()
                    },
                )
                .expect("golden page must render");
            (image.width(), image.height(), image.into_raw())
        })
        .collect()
}

fn sha256_of(path: &Path) -> String {
    ocrx_core::sha256_file(path).expect("asset readable")
}

#[derive(serde::Serialize)]
struct PromptProbe {
    prompt_name: String,
    mtmd_input_text: String,
    raw_output: String,
    output_bytes: usize,
    contains_think_open: bool,
    contains_think_close: bool,
    bbox_img_tags: usize,
    html_tables: usize,
    latex_display_blocks: usize,
    ends_with_repeated_tail: bool,
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn probe_prompt(
    runtime: &mut LlamaMtmdRuntime,
    page: (u32, u32, Vec<u8>),
    name: &str,
    prompt: &str,
) -> PromptProbe {
    let request = ocrx_runtime::InferenceRequest::new(1, page.0, page.1, page.2, PROBE_MAX_TOKENS)
        .expect("probe request is valid");
    let response = runtime
        .generate_with_progress(request, &ocrx_core::CancellationToken::new(), &mut |_| {})
        .expect("generation succeeds on the pinned runtime");
    let raw = response.raw_output;
    let tail = raw
        .chars()
        .rev()
        .take(120)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    let trimmed = tail.trim_end();
    // Degenerate repeated-tail heuristic (same shape the scan ladder fights):
    // the last 40 chars repeat at least twice in the final 120 chars.
    let ends_with_repeated_tail = trimmed.len() >= 80 && {
        let unit: String = trimmed
            .chars()
            .rev()
            .take(40)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        trimmed.contains(&unit) && trimmed.matches(&unit).count() >= 2
    };
    PromptProbe {
        prompt_name: name.to_owned(),
        mtmd_input_text: format!("{}{}", mtmd_default_marker(), prompt),
        raw_output: raw.clone(),
        output_bytes: raw.len(),
        contains_think_open: raw.contains("<think>"),
        contains_think_close: raw.contains("</think>"),
        bbox_img_tags: count_occurrences(&raw, "<img src=\"images/bbox_"),
        html_tables: count_occurrences(&raw, "<table"),
        latex_display_blocks: count_occurrences(&raw, "$$"),
        ends_with_repeated_tail,
    }
}

#[test]
#[ignore = "env-gated: OCRX_E2E_MODEL=1 with the OvisOCR3 spike assets installed"]
fn ovisocr3_frozen_pin_load_prompt_shape_and_grid_spike() {
    if let Some(reason) = skip_reason() {
        eprintln!("skipped: {reason}");
        return;
    }
    let (model, mmproj, model_bytes, mmproj_bytes) =
        resolve_model_paths().expect("OvisOCR3 spike assets are installed");
    assert_eq!(model_bytes, OVISOCR3_Q8_BYTES, "model byte size frozen");
    assert_eq!(
        mmproj_bytes, OVISOCR3_MMPROJ_BYTES,
        "mmproj byte size frozen"
    );
    assert_eq!(sha256_of(&model), OVISOCR3_Q8_SHA256, "model sha256 frozen");
    assert_eq!(
        sha256_of(&mmproj),
        OVISOCR3_MMPROJ_SHA256,
        "mmproj sha256 frozen"
    );

    // T02: the frozen pin loads OvisOCR3 through the production actor (CPU).
    let mut runtime = LlamaMtmdRuntime::load(&model, &mmproj, false, 1024, 20_480, 8)
        .expect("OvisOCR3 GGUF+mmproj must load on the frozen pin");
    let load_telemetry = runtime.telemetry();
    let pages = render_pages();

    // T03: prompt-shape comparison on real pages (frozen vs official bytes).
    let mut probes: Vec<(usize, PromptProbe)> = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        probes.push((
            index,
            probe_prompt(
                &mut runtime,
                (page.0, page.1, page.2.clone()),
                "ocr-markdown-v1-frozen",
                OCR_MARKDOWN_V1,
            ),
        ));
        probes.push((
            index,
            probe_prompt(
                &mut runtime,
                (page.0, page.1, page.2.clone()),
                "ovisocr3-document-markdown-official",
                OVISOCR3_DOCUMENT_MARKDOWN,
            ),
        ));
    }

    // T04: grid override 2048 acceptance (D-5 decision input).
    let grid_page = &pages[0];
    let grid_request = ocrx_runtime::InferenceRequest::new(
        1,
        grid_page.0,
        grid_page.1,
        grid_page.2.clone(),
        PROBE_MAX_TOKENS,
    )
    .expect("probe request is valid")
    .with_image_tokens_override(2048);
    let grid_result = runtime.generate_with_progress(
        grid_request,
        &ocrx_core::CancellationToken::new(),
        &mut |_| {},
    );
    let grid_outcome = match grid_result {
        Ok(response) => serde_json::json!({
            "accepted": true,
            "output_bytes": response.raw_output.len(),
        }),
        Err(error) => serde_json::json!({
            "accepted": false,
            "error": error,
        }),
    };

    let record = serde_json::json!({
        "spike": "ovisocr3-frozen-pin",
        "supply_chain": {
            "repo": "michalland/OvisOCR3-GGUF",
            "revision": OVISOCR3_REVISION,
            "profile": "quality",
            "quantization": "Q8_0",
            "model_sha256": OVISOCR3_Q8_SHA256,
            "model_bytes": OVISOCR3_Q8_BYTES,
            "mmproj_sha256": OVISOCR3_MMPROJ_SHA256,
            "mmproj_bytes": OVISOCR3_MMPROJ_BYTES,
        },
        "runtime_pin": {
            "wrapper": "0.1.161",
            "sys": "0.1.161",
            "upstream": "d81235049384534c167caea52b85a694f6103d14",
            "load_ok": true,
            "load_time_ms": load_telemetry.load_time_ms,
        },
        "mtmd_default_marker": mtmd_default_marker(),
        "prompt_probes": probes
            .into_iter()
            .map(|(page, probe)| serde_json::json!({"page": page, "probe": probe}))
            .collect::<Vec<_>>(),
        "grid_override_2048": grid_outcome,
    });
    let out = std::env::var_os("OCRX_SPIKE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo_root().join("docs/develop/spike-assets/ovisocr3-spike-record.json")
        });
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("spike output dir");
    }
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&record).expect("record serializes"),
    )
    .expect("spike record written");
    println!("spike record: {}", out.display());
}

/// T04 deep probe (D-5): does the 2048 grid rung produce usable output for
/// OvisOCR3, compared against the no-override baseline and the OvisOCR2
/// control on the same pages? Each request mirrors the production ladder's
/// rung-2 shape (grid override + retry sampling attached).
#[test]
#[ignore = "env-gated: OCRX_E2E_MODEL=1 with the OvisOCR3 spike assets installed"]
fn ovisocr3_grid_2048_rung_behavior_probe() {
    if let Some(reason) = skip_reason() {
        eprintln!("skipped: {reason}");
        return;
    }
    let (model, mmproj, _, _) = resolve_model_paths().expect("OvisOCR3 spike assets");
    let pages = render_pages();
    let mut record = serde_json::json!({ "spike": "ovisocr3-grid-2048" });

    for (name, model_path, mmproj_path) in [
        ("ovisocr3-quality", model.clone(), mmproj.clone()),
        (
            "ovisocr2-reference-control",
            ovisocr2_reference_paths().0,
            ovisocr2_reference_paths().1,
        ),
    ] {
        let mut runtime = LlamaMtmdRuntime::load(&model_path, &mmproj_path, false, 1024, 20_480, 8)
            .unwrap_or_else(|error| panic!("{name} must load: {error}"));
        let mut rows = Vec::new();
        for (index, page) in pages.iter().enumerate() {
            // Baseline: plain greedy (no override).
            let plain = ocrx_runtime::InferenceRequest::new(
                1,
                page.0,
                page.1,
                page.2.clone(),
                PROBE_MAX_TOKENS,
            )
            .expect("probe request is valid");
            let plain_out: serde_json::Value = runtime
                .generate_with_progress(plain, &ocrx_core::CancellationToken::new(), &mut |_| {})
                .map(|response| serde_json::json!(response.raw_output.len()))
                .unwrap_or_else(|error| serde_json::json!(format!("ERR: {error}")));
            // Rung-2 shape: 2048 grid + retry sampling.
            let rung = ocrx_runtime::InferenceRequest::new(
                1,
                page.0,
                page.1,
                page.2.clone(),
                PROBE_MAX_TOKENS,
            )
            .expect("probe request is valid")
            .with_image_tokens_override(2048)
            .with_retry_sampling(
                ocrx_runtime::RetrySampling::new(1_200, 64).expect("frozen sampling"),
            );
            let rung_out: serde_json::Value = runtime
                .generate_with_progress(rung, &ocrx_core::CancellationToken::new(), &mut |_| {})
                .map(|response| serde_json::json!(response.raw_output.len()))
                .unwrap_or_else(|error| serde_json::json!(format!("ERR: {error}")));
            rows.push(serde_json::json!({
                "page": index,
                "plain_bytes": plain_out,
                "grid2048_bytes": rung_out,
            }));
        }
        record[name] = serde_json::json!(rows);
    }

    let out = std::env::var_os("OCRX_SPIKE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("docs/develop/spike-assets/ovisocr3-grid-record.json"));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("spike output dir");
    }
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&record).expect("record serializes"),
    )
    .expect("spike record written");
    println!("grid record: {}", out.display());
}

/// OvisOCR2 control paths (the installed reference profile in the default
/// cache — spike-only helper, mirrors the multi-sequence spike).
fn ovisocr2_reference_paths() -> (PathBuf, PathBuf) {
    let cache = std::env::var_os("OCRX_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(ocrx_models::default_cache_root);
    let manifest =
        ocrx_models::builtin::ovisocr2::ovisocr2_manifest("quality").expect("ovisocr2 manifest");
    let root = cache
        .join("ocrx")
        .join("models")
        .join("ovisocr2")
        .join(manifest.source_revision);
    (
        root.join("quality").join("model.gguf"),
        root.join("mmproj.gguf"),
    )
}
