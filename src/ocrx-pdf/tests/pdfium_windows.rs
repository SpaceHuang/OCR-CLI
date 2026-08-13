//! W2.1/W0.2 PDFium loading-failure integration tests.
//!
//! These cover the loading-failure half of the W2.1/W0.2 contract: a missing
//! library and a path-escaping `library_dir` must fail (non-zero exit) rather
//! than silently succeed. They run the `pdfium-check` bin as a subprocess so
//! each case gets a fresh process - the pdfium-render binding is process-global
//! and can only bind once, so a failure case cannot share a process with a
//! successful bind.
//!
//! The positive rendering validation (page count, 300 DPI RGB8, audit sizes,
//! page selection, pixel budget, content-error classification) lives in the
//! `real_pdf_*` lib unit tests, which bind the real library in-process. This
//! file holds only the failure cases that need a separate process.
//!
//! `CARGO_BIN_EXE_pdfium-check` is set only for integration tests, so these
//! cases cannot live in the lib unit-test module.

use std::path::PathBuf;
use std::process::Command;

fn pdfium_check() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pdfium-check"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/fixtures/pdf")
        .join(name)
}

#[test]
fn missing_library_does_not_silently_succeed() {
    // A directory with no pdfium library must fail (non-zero exit), not
    // silently render. Runs as a fresh pdfium-check process so the
    // process-global binding is unaffected.
    let empty = std::env::temp_dir().join(format!("ocrx-empty-lib-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&empty);
    let output = Command::new(pdfium_check())
        .args([
            empty.to_str().unwrap(),
            fixture("normal-1-page.pdf").to_str().unwrap(),
        ])
        .output()
        .expect("pdfium-check should run");
    assert!(
        !output.status.success(),
        "a missing pdfium library must not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    assert!(
        stderr.contains("library") || stderr.contains("pdfium"),
        "missing-library failure should mention the library, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn path_escape_library_dir_is_rejected() {
    // A library_dir containing `..` must be rejected by the pdfium_library_path
    // guard, never traversing out to load an unintended library (W0.2). The
    // pdfium-check process exits non-zero instead of silently loading.
    let output = Command::new(pdfium_check())
        .args(["./../..", fixture("normal-1-page.pdf").to_str().unwrap()])
        .output()
        .expect("pdfium-check should run");
    assert!(
        !output.status.success(),
        "a path-escaping library_dir must not silently succeed"
    );
}
