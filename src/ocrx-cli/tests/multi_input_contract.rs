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

#[test]
fn orchestrated_cancellation_stops_the_queue_at_the_next_boundary() {
    // Rereview BLK-1: cancel = whole invocation. With the deterministic seam
    // requesting cancellation after the first document, the second document
    // must not start, the invocation must exit 130, and already-committed
    // documents stay. Pure-native docs exercise exactly the path that used
    // to ignore the token (no ScanService in between).
    let cwd = std::env::temp_dir().join(format!("ocrx-multidoc-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();

    // Three distinct native-text documents (dedupe collapses identical
    // paths, so the fixture is copied per document). The docs live in a
    // `docs/` subdirectory: the output root must stay outside the input
    // directory (output policy rejects an output under the input path).
    let docs_dir = cwd.join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    let src = fixture("normal-1-page.pdf");
    for name in ["doc-a.pdf", "doc-b.pdf", "doc-c.pdf"] {
        std::fs::copy(&src, docs_dir.join(name)).unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .current_dir(&cwd)
        .args([
            "scan",
            docs_dir.to_str().unwrap(),
            "--output-dir",
            "out",
            "--pdf-mode",
            "text",
            "--device",
            "cpu",
            "--offline",
            "--non-interactive",
            "--report",
            "json",
        ])
        .env("OCRX_E2E_CANCEL_AFTER_DOCUMENTS", "1")
        .env_remove("OCRX_PDFIUM_LIBRARY")
        .output()
        .expect("ocrx binary should run");

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    if code == 3 {
        // No pdfium.dll in this checkout (clean CI): page count failed for
        // every document; the seam path itself is covered by the gated
        // real-model runs. Nothing to assert here.
        eprintln!("skipping: pdfium.dll not present in this checkout");
        let _ = std::fs::remove_dir_all(&cwd);
        return;
    }

    assert_eq!(
        code, 130,
        "seam-cancelled invocation must exit 130 (stdout: {stdout})"
    );
    let root = cwd.join("out");
    let summary: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "stdout must be one JSON object: {error}
{stdout}"
        )
    });
    assert!(
        summary["elapsed_ms"].as_u64().is_some(),
        "summary: {summary}"
    );
    let documents = summary["documents"].as_array().expect("documents array");
    assert_eq!(
        documents.len(),
        1,
        "only the first document may be processed: {summary}"
    );
    // The first document's own status depends on pdfium availability in this
    // checkout (success with the vendored DLL, page-count failure without) -
    // the cancellation contract under test holds either way: the seam fired
    // after document 1 and the invocation still stopped with exit 130.
    assert!(
        documents[0]["status"] == "success" || documents[0]["status"] == "failed",
        "summary: {summary}"
    );
    // Documents after the boundary must not have started: no doc-b/doc-c
    // directories may exist (doc-a's own directory exists only when its
    // scan actually committed, i.e. with pdfium present).
    assert!(root.join("scan-summary.json").is_file());
    let on_disk: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        on_disk
            .iter()
            .all(|name| !name.starts_with("doc-b") && !name.starts_with("doc-c")),
        "documents after the cancellation boundary must not be committed: {on_disk:?}"
    );
    assert_eq!(
        on_disk.len(),
        usize::from(documents[0]["status"] == "success"),
        "only the first document may commit: {on_disk:?}"
    );

    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn multi_root_policy_protects_outputs_before_any_document_runs() {
    let cwd = std::env::temp_dir().join(format!("ocrx-root-policy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(cwd.join("docs")).unwrap();
    let first = cwd.join("docs/a.pdf");
    std::fs::create_dir_all(cwd.join("other-docs")).unwrap();
    let second = cwd.join("other-docs/b.pdf");
    std::fs::write(&first, b"invalid PDF").unwrap();
    std::fs::write(&second, b"invalid PDF").unwrap();
    let cache = cwd.join("cache");
    for root in [
        cache.join("ocrx/models"),
        cache.join("ovisocr2"),
        cwd.clone(),
        cwd.join("docs"),
        cwd.join("other-docs"),
        cwd.join("existing"),
    ] {
        std::fs::create_dir_all(&root).unwrap();
        let summary = root.join("scan-summary.json");
        for existing in [false, true] {
            if existing {
                std::fs::write(&summary, b"SENTINEL").unwrap();
            }
            // A fresh dedicated output is allowed; every other case is refused.
            if !existing && root == cwd.join("existing") {
                continue;
            }
            let output = Command::new(env!("CARGO_BIN_EXE_ocrx"))
                .current_dir(&cwd)
                .arg("scan")
                .arg(&first)
                .arg(&second)
                .arg("--output-dir")
                .arg(&root)
                .args([
                    "--pdf-mode",
                    "text",
                    "--offline",
                    "--non-interactive",
                    "--report",
                    "json",
                ])
                .env("OCRX_MODEL_CACHE", &cache)
                .env("OCRX_PDFIUM_LIBRARY", cwd.join("missing-pdfium"))
                .output()
                .unwrap();
            assert!(matches!(output.status.code(), Some(2 | 9)), "{output:?}");
            assert!(
                !String::from_utf8_lossy(&output.stderr).contains("[doc "),
                "{output:?}"
            );
            if existing {
                assert_eq!(std::fs::read(&summary).unwrap(), b"SENTINEL");
                std::fs::remove_file(&summary).unwrap();
            } else {
                assert!(!summary.exists());
            }
        }
    }
    std::fs::remove_dir_all(cwd).unwrap();
}

#[test]
fn owner_disconnect_at_second_document_reports_error_without_panic() {
    let cwd = std::env::temp_dir().join(format!("ocrx-owner-disconnect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(cwd.join("out/committed")).unwrap();
    let committed = cwd.join("out/committed/document.md");
    std::fs::write(&committed, b"committed result").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .current_dir(&cwd)
        .arg("scan")
        .arg(fixture("normal-1-page.pdf"))
        .arg(fixture("native-plus-empty.pdf"))
        .args([
            "--output-dir",
            "out",
            "--pdf-mode",
            "text",
            "--offline",
            "--non-interactive",
            "--report",
            "json",
        ])
        .env("OCRX_E2E_OWNER_DISCONNECT_ON_SWITCH", "2")
        .env("OCRX_PDFIUM_LIBRARY", cwd.join("missing-pdfium"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("pdfium owner failed"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(
        stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|error| error.to_string().contains("pdfium owner failed")),
        "{stderr}"
    );
    assert_eq!(std::fs::read(committed).unwrap(), b"committed result");
    std::fs::remove_dir_all(cwd).unwrap();
}
