//! Gated W3.3 real-model fault-injection E2E.
//!
//! These exercise the compiled `ocrx` binary so the real PDFium renderer, the
//! real LlamaMtmdRuntime, and the CLI page state machine are validated together.
//! They are `#[ignore]`d and require `OCRX_E2E_MODEL=1`, an installed `compact`
//! profile in the platform model cache, and the co-located PDFium library.

use std::path::PathBuf;
use std::process::Command;

fn require_real_model() {
    assert_eq!(
        std::env::var("OCRX_E2E_MODEL").ok().as_deref(),
        Some("1"),
        "enable with OCRX_E2E_MODEL=1, installed compact assets, and PDFium"
    );
}

fn pdfium_dir() -> PathBuf {
    let library_name = if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else {
        "libpdfium.dylib"
    };
    let dir = std::env::var_os("OCRX_PDFIUM_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/bin")
        });
    assert!(
        dir.join(library_name).is_file(),
        "PDFium runtime not present at {}",
        dir.display()
    );
    dir.canonicalize()
        .expect("PDFium dir should be canonicalizable")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/fixtures/pdf")
        .join(name)
}

/// Runs a single-page real scan with the page's fault injected.
///
/// The failing page never reaches OCR, so this stays fast while still exercising
/// the real PDFium renderer, real runtime load, and the CLI page state machine.
fn run_injected_scan(injection_var: &str, page: &str) -> (i32, String) {
    require_real_model();
    let tag = injection_var
        .trim_start_matches("OCRX_E2E_INJECT_")
        .trim_end_matches("_PAGE")
        .to_ascii_lowercase();
    let output_dir = std::env::temp_dir().join(format!(
        "ocrx-real-fault-injection-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&output_dir);

    let child = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .args([
            "scan",
            fixture("normal-1-page.pdf").to_str().unwrap(),
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--pages",
            "1",
            "--model-profile",
            "compact",
            "--device",
            "cpu",
            "--offline",
            "--non-interactive",
            "--report",
            "json",
        ])
        .env("OCRX_PDFIUM_LIBRARY", pdfium_dir())
        .env(injection_var, page)
        .output()
        .expect("ocrx binary should run");

    let code = child.status.code().unwrap_or(-1);
    let result = std::fs::read_to_string(output_dir.join("result.json")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&output_dir);
    (code, result)
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn real_scan_injects_render_failure_and_continues() {
    let (code, result) = run_injected_scan("OCRX_E2E_INJECT_RENDER_FAIL_PAGE", "1");
    assert_eq!(code, 10, "result.json: {result}");
    assert!(
        result.contains("\"status\":\"partial\""),
        "result.json: {result}"
    );
    assert!(result.contains("\"total\":1"), "result.json: {result}");
    assert!(result.contains("\"completed\":0"), "result.json: {result}");
    assert!(result.contains("\"failed\":1"), "result.json: {result}");
    assert!(
        result.contains("\"stage\":\"render\""),
        "result.json: {result}"
    );
    assert!(
        result.contains("\"error_code\":\"render_failed\""),
        "result.json: {result}"
    );
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn real_scan_injects_inference_failure_and_continues() {
    let (code, result) = run_injected_scan("OCRX_E2E_INJECT_INFERENCE_FAIL_PAGE", "1");
    assert_eq!(code, 10, "result.json: {result}");
    assert!(
        result.contains("\"status\":\"partial\""),
        "result.json: {result}"
    );
    assert!(result.contains("\"total\":1"), "result.json: {result}");
    assert!(result.contains("\"completed\":0"), "result.json: {result}");
    assert!(result.contains("\"failed\":1"), "result.json: {result}");
    assert!(
        result.contains("\"stage\":\"recognize\""),
        "result.json: {result}"
    );
    assert!(
        result.contains("\"error_code\":\"recognition_failed\""),
        "result.json: {result}"
    );
}
