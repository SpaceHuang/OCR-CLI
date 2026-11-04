//! Ungated multi-input stdout/layout contract tests (review BLK-3/BLK-4).
//!
//! These run the compiled `ocrx` binary against tracked native-text fixtures
//! with `--pdf-mode text`. Whether PDFium is available decides only the exit
//! path (0 = both documents scanned natively, 3 = each document failed at
//! page count); the contract assertions hold on either side:
//!
//! - a relative `--output-dir` is resolved against the process cwd (BLK-3),
//!   so every document passes the absolute-root output policy;
//! - `--report json` stdout is exactly ONE parseable JSON object (the
//!   aggregate summary), and the `[doc i/N]` boundary logs go to stderr
//!   (BLK-4).

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/fixtures/pdf")
        .join(name)
}

#[test]
fn multi_input_relative_output_root_and_json_stdout_contract() {
    let cwd = std::env::temp_dir().join(format!("ocrx-multidoc-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();

    let doc_a = fixture("normal-1-page.pdf");
    let doc_b = fixture("native-plus-empty.pdf");
    let output = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .current_dir(&cwd)
        .args([
            "scan",
            doc_a.to_str().unwrap(),
            doc_b.to_str().unwrap(),
            "--output-dir",
            "relative-root",
            "--pdf-mode",
            "text",
            "--device",
            "cpu",
            "--offline",
            "--non-interactive",
            "--report",
            "json",
        ])
        .env_remove("OCRX_PDFIUM_LIBRARY")
        .output()
        .expect("ocrx binary should run");

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // BLK-3: the relative root resolved under the process cwd, so the run
    // passed the absolute-root output policy (either scanning successfully or
    // failing later at PDFium - never with the policy's exit 9).
    assert_ne!(
        code, 9,
        "relative output root hit the policy (stdout: {stdout})"
    );
    let root = cwd.join("relative-root");
    assert!(
        root.join("scan-summary.json").is_file(),
        "scan-summary.json must land in the resolved root (exit {code}, stdout: {stdout})"
    );

    // BLK-4: stdout is exactly one JSON object - the aggregate summary.
    let summary: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("stdout must be one JSON object: {error}\n{stdout}"));
    assert_eq!(summary["mode"], "multi", "stdout: {stdout}");
    assert_eq!(summary["schema_version"], 1, "stdout: {stdout}");
    let documents = summary["documents"].as_array().expect("documents array");
    assert_eq!(documents.len(), 2, "stdout: {stdout}");
    // Boundary logs went to stderr, not stdout (already proven by the strict
    // parse above - one stray "[doc" line would break the JSON parse).
    assert!(
        stderr.contains("[doc 1/2"),
        "doc boundary log expected on stderr: {stderr}"
    );
    if code == 0 {
        for document in documents {
            assert_eq!(document["status"], "success", "summary: {summary}");
            let dir = PathBuf::from(document["output_dir"].as_str().unwrap());
            assert!(dir.join("document.md").is_file(), "{}", dir.display());
        }
    } else {
        // PDFium unavailable: every document failed at page count.
        assert_eq!(code, 3, "unexpected exit (stdout: {stdout})");
        for document in documents {
            assert_eq!(document["exit_code"], 3, "summary: {summary}");
        }
    }

    let _ = std::fs::remove_dir_all(&cwd);
}
