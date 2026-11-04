//! Gated X1 cross-document orchestration E2E (phase-2 plan §4 X1.4①/③).
//!
//! Runs the compiled `ocrx` binary so the real PDFium owner (with the
//! SwitchDocument protocol), the real shared inference actor, and the CLI
//! orchestrator are validated together:
//!
//! - ① content equivalence: each document's orchestrated output is
//!   byte-identical (`document.md`) to its individual scan, and `result.json`
//!   matches structurally after removing timing fields and re-pointing the
//!   layout-dependent paths at the orchestrated directories;
//! - ③ document-level failure: an unreadable middle document is skipped as a
//!   value while the surrounding documents complete (aggregate exit 3).
//!
//! They are `#[ignore]`d and require `OCRX_E2E_MODEL=1`, an installed
//! `compact` profile in the platform model cache, and the co-located PDFium
//! library (same contract as `real_scan_fault_injection.rs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
    dir.canonicalize().expect("PDFium dir should canonicalize")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/fixtures/pdf")
        .join(name)
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ocrx-multidoc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// One `ocrx scan` invocation; `inputs` are the positional inputs (the
/// orchestrator is engaged by passing more than one).
fn run_scan(inputs: &[&Path], output: &Path, extra_args: &[&str]) -> i32 {
    require_real_model();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ocrx"));
    command
        .arg("scan")
        .args(inputs.iter().map(|input| input.to_str().unwrap()))
        .args(["--output-dir", output.to_str().unwrap()])
        .args([
            "--pdf-mode",
            "ocr",
            "--model-profile",
            "compact",
            "--device",
            "cpu",
            "--offline",
            "--non-interactive",
            "--report",
            "json",
        ])
        .args(extra_args)
        .env("OCRX_PDFIUM_LIBRARY", pdfium_dir());
    let child = command.output().expect("ocrx binary should run");
    child.status.code().unwrap_or(-1)
}

/// The orchestrated directory for `input` inside `root`:
/// `<stem>-<hash8>` per the X1.3 layout.
fn doc_dir(root: &Path, input: &Path) -> PathBuf {
    let stem = input.file_stem().unwrap().to_string_lossy().to_string();
    let mut entries = std::fs::read_dir(root)
        .unwrap_or_else(|error| panic!("output root {}: {error}", root.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    entries.retain(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(&stem))
    });
    assert_eq!(
        entries.len(),
        1,
        "exactly one doc directory for stem {stem} under {}: {:?}",
        root.display(),
        entries
    );
    entries.remove(0)
}

/// Strips the fields that legitimately differ between an individual scan and
/// its orchestrated counterpart (timing + layout-derived paths) and returns
/// the remainder for structural comparison. `own_dir` is the scanned run's
/// own output directory: its occurrences (in the JSON-escaped spelling) are
/// normalized to a placeholder so both sides compare structure, not layout.
fn comparable_result(result_json: &str, own_dir: &Path) -> BTreeMap<String, serde_json::Value> {
    let mut value: serde_json::Value = serde_json::from_str(result_json).expect("result.json");
    let object = value.as_object_mut().expect("result.json object");
    object.remove("elapsed_ms");
    let escaped = own_dir.to_string_lossy().replace('\\', "\\\\");
    for field in object.values_mut() {
        let text = field.to_string();
        if text.contains(escaped.as_str()) {
            *field = serde_json::Value::String(text.replace(escaped.as_str(), "<OUTPUT_DIR>"));
        }
    }
    object
        .into_iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn orchestrated_outputs_match_individual_scans() {
    let doc_a = fixture("normal-1-page.pdf");
    let doc_b = fixture("figure-1-page.pdf");
    let scratch = scratch_dir("equivalence");
    std::fs::create_dir_all(&scratch).unwrap();

    // Individual scans: the pre-orchestration baseline for each document.
    let indiv_a = scratch.join("indiv-a");
    let indiv_b = scratch.join("indiv-b");
    assert_eq!(
        run_scan(&[&doc_a], &indiv_a, &[]),
        0,
        "individual scan of doc A"
    );
    assert_eq!(
        run_scan(&[&doc_b], &indiv_b, &[]),
        0,
        "individual scan of doc B"
    );

    // Orchestrated: both documents in one invocation.
    let root = scratch.join("multi");
    let code = run_scan(&[&doc_a, &doc_b], &root, &[]);
    assert_eq!(code, 0, "orchestrated invocation must succeed");

    let orch_a = doc_dir(&root, &doc_a);
    let orch_b = doc_dir(&root, &doc_b);

    // ① content products: byte-identical per document.
    for (indiv, orch) in [(indiv_a.clone(), &orch_a), (indiv_b.clone(), &orch_b)] {
        let left = std::fs::read(indiv.join("document.md")).expect("individual document.md");
        let right = std::fs::read(orch.join("document.md")).expect("orchestrated document.md");
        assert_eq!(left, right, "document.md diverged for {}", orch.display());
    }

    // ① result.json: structurally identical after removing timing fields and
    // normalizing each run's own output-directory occurrences.
    for (indiv, orch) in [(&indiv_a, &orch_a), (&indiv_b, &orch_b)] {
        let left = comparable_result(
            &std::fs::read_to_string(indiv.join("result.json")).expect("individual result.json"),
            indiv,
        );
        let right = comparable_result(
            &std::fs::read_to_string(orch.join("result.json")).expect("orchestrated result.json"),
            orch,
        );
        assert_eq!(
            left,
            right,
            "result.json (structure-normalized) diverged for {}",
            orch.display()
        );
    }

    // Aggregate summary: two successes, per-document elapsed recorded.
    let summary: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scan-summary.json")).expect("scan-summary.json"),
    )
    .expect("scan-summary.json parses");
    let documents = summary["documents"].as_array().expect("documents array");
    assert_eq!(documents.len(), 2, "summary: {summary}");
    for document in documents {
        assert_eq!(document["status"], "success", "summary: {summary}");
        assert_eq!(document["exit_code"], 0, "summary: {summary}");
        assert!(
            document["elapsed_ms"].as_u64().is_some(),
            "per-document elapsed_ms must be recorded: {summary}"
        );
    }
    assert!(
        summary["elapsed_ms"].as_u64().is_some(),
        "total elapsed must be present: {summary}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn unreadable_document_is_skipped_and_the_rest_continue() {
    let doc_a = fixture("normal-1-page.pdf");
    let doc_b = fixture("figure-1-page.pdf");
    let broken = fixture("not-a-pdf.txt");
    let scratch = scratch_dir("skip");
    std::fs::create_dir_all(&scratch).unwrap();

    let indiv_a = scratch.join("indiv-a");
    let indiv_b = scratch.join("indiv-b");
    assert_eq!(run_scan(&[&doc_a], &indiv_a, &[]), 0);
    assert_eq!(run_scan(&[&doc_b], &indiv_b, &[]), 0);

    let root = scratch.join("multi");
    // The unreadable middle document must not affect its neighbors
    // (X1.4③): both PDFs complete, the invocation still fails non-zero.
    let code = run_scan(&[&doc_a, &broken, &doc_b], &root, &[]);
    assert_eq!(code, 3, "aggregate exit must report the document failure");

    let orch_a = doc_dir(&root, &doc_a);
    let orch_b = doc_dir(&root, &doc_b);
    for (indiv, orch) in [(&indiv_a, &orch_a), (&indiv_b, &orch_b)] {
        let left = std::fs::read(indiv.join("document.md")).expect("individual document.md");
        let right = std::fs::read(orch.join("document.md")).expect("orchestrated document.md");
        assert_eq!(left, right, "document.md diverged for {}", orch.display());
    }

    let summary: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scan-summary.json")).expect("scan-summary.json"),
    )
    .expect("scan-summary.json parses");
    // The real executor's switch is an infallible path swap, so the unreadable
    // document surfaces at page count inside scan_document: status "failed"
    // with the document-level exit code 3 (the owner-level "skipped" label is
    // reserved for the switch-value path). Either way the invocation continues.
    let statuses = summary["documents"]
        .as_array()
        .expect("documents array")
        .iter()
        .map(|document| document["status"].as_str().unwrap_or("").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec!["success", "failed", "success"],
        "summary: {summary}"
    );
    assert_eq!(
        summary["documents"][1]["exit_code"], 3,
        "summary: {summary}"
    );
    // A document that never started must not have left artifacts behind.
    let planned_bad_dir = root.join(
        summary["documents"][1]["output_dir"]
            .as_str()
            .expect("output_dir")
            .rsplit(['\\', '/'])
            .next()
            .expect("dir name"),
    );
    assert!(
        !planned_bad_dir.exists(),
        "unreadable document left an output directory: {}",
        planned_bad_dir.display()
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
