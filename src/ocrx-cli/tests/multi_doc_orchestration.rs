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
use std::fs;
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
/// orchestrator is engaged by passing more than one). Returns the exit code
/// plus captured stdout/stderr for the stdout-contract and verify-count
/// assertions.
fn run_scan_full(
    inputs: &[&Path],
    output: &Path,
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
) -> (i32, String, String) {
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
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let child = command.output().expect("ocrx binary should run");
    let code = child.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&child.stdout).to_string();
    let stderr = String::from_utf8_lossy(&child.stderr).to_string();
    (code, stdout, stderr)
}

fn run_scan(inputs: &[&Path], output: &Path, extra_args: &[&str]) -> i32 {
    let (code, _stdout, _stderr) = run_scan_full(inputs, output, extra_args, &[]);
    code
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

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn model_is_resolved_and_verified_exactly_once_per_invocation() {
    // Review BLK-2: the shared model slot must skip not only the load but the
    // full asset verification for every document after the first. With the
    // debug channel on, `resolve_and_prepare_model` emits one stderr line per
    // verification - a two-document invocation must show exactly one.
    let doc_a = fixture("normal-1-page.pdf");
    let doc_b = fixture("figure-1-page.pdf");
    let scratch = scratch_dir("verify-once");
    std::fs::create_dir_all(&scratch).unwrap();
    let root = scratch.join("multi");
    let (code, _stdout, stderr) =
        run_scan_full(&[&doc_a, &doc_b], &root, &[], &[("OCRX_STEP_PROFILE", "1")]);
    assert_eq!(code, 0, "orchestrated invocation must succeed");
    let verifications = stderr
        .lines()
        .filter(|line| line.contains("[model] resolved and verified"))
        .count();
    assert_eq!(
        verifications, 1,
        "asset verification must run exactly once per invocation (stderr: {stderr})"
    );
    // Both documents still complete against the shared runtime.
    let summary: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scan-summary.json")).expect("scan-summary.json"),
    )
    .expect("scan-summary.json parses");
    for document in summary["documents"].as_array().expect("documents") {
        assert_eq!(document["status"], "success", "summary: {summary}");
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets, PDFium, and a built pdfium-worker"]
fn worker_mode_two_ocr_documents_complete_without_spontaneous_cancellation() {
    // Review BLK-1: with OCRX_PDF_WORKER set, every document built its own
    // WorkerScanRenderer whose Drop cancelled the SHARED invocation token -
    // document 2 then "cancelled" with exit 130 without any signal. The
    // renderer now tears down through a child token; the invocation must run
    // to completion with every document successful.
    let doc_a = fixture("normal-1-page.pdf");
    let doc_b = fixture("figure-1-page.pdf");
    let worker = Path::new(env!("CARGO_BIN_EXE_ocrx"))
        .parent()
        .map(|dir| {
            let name = if cfg!(target_os = "windows") {
                "pdfium-worker.exe"
            } else {
                "pdfium-worker"
            };
            dir.join(name)
        })
        .expect("ocrx binary path");
    if !worker.is_file() {
        eprintln!(
            "skipped: pdfium-worker not found at {} (build workspace bins first)",
            worker.display()
        );
        return;
    }
    let scratch = scratch_dir("worker-mode");
    std::fs::create_dir_all(&scratch).unwrap();
    let root = scratch.join("multi");
    let worker_env = worker.to_str().expect("worker path");
    let (code, _stdout, stderr) = run_scan_full(
        &[&doc_a, &doc_b],
        &root,
        &[],
        &[("OCRX_PDF_WORKER", worker_env)],
    );
    assert_ne!(code, 130, "spontaneous cancellation (stderr: {stderr})");
    assert_eq!(code, 0, "worker-mode invocation must succeed");
    let summary: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scan-summary.json")).expect("scan-summary.json"),
    )
    .expect("scan-summary.json parses");
    let statuses = summary["documents"]
        .as_array()
        .expect("documents array")
        .iter()
        .map(|document| document["status"].as_str().unwrap_or("").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec!["success", "success"],
        "both documents must be processed in worker mode: {summary}"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn mixed_queue_documents_keep_single_scan_metadata() {
    // Rereview BLK-2: a pure-native document must keep the single-scan
    // metadata (backend = requested device, ocr_model_loaded = false) even
    // when an earlier document filled the shared model slot - under `auto`
    // configuration, where the pollution used to surface as backend=cpu.
    let native_doc = fixture("normal-1-page.pdf");
    let ocr_doc = fixture("figure-1-page.pdf");
    let scratch = scratch_dir("mixed-queue");
    std::fs::create_dir_all(&scratch).unwrap();

    // Single scans under the same auto configuration (`--pdf-mode auto` +
    // `--device auto` override the helper's OCR/cpu base args - the review
    // repro needs the native document to stay native).
    let auto_args = ["--pdf-mode", "auto", "--device", "auto"];
    let single_native = scratch.join("single-native");
    let single_ocr = scratch.join("single-ocr");
    assert_eq!(run_scan(&[&native_doc], &single_native, &auto_args), 0);
    assert_eq!(run_scan(&[&ocr_doc], &single_ocr, &auto_args), 0);

    // Orchestrated, both orders.
    for order_name in ["native-first", "ocr-first"] {
        let order: Vec<&Path> = if order_name == "native-first" {
            vec![native_doc.as_path(), ocr_doc.as_path()]
        } else {
            vec![ocr_doc.as_path(), native_doc.as_path()]
        };
        let root = scratch.join(format!("multi-{order_name}"));
        assert_eq!(
            run_scan(&order, &root, &auto_args),
            0,
            "orchestrated run must succeed"
        );
        for (doc, single) in [(&native_doc, &single_native), (&ocr_doc, &single_ocr)] {
            let orch_dir = doc_dir(&root, doc);
            let left = comparable_result(
                &std::fs::read_to_string(single.join("result.json")).expect("single result.json"),
                single,
            );
            let right = comparable_result(
                &std::fs::read_to_string(orch_dir.join("result.json")).expect("orch result.json"),
                &orch_dir,
            );
            assert_eq!(
                left,
                right,
                "result.json (structure-normalized) diverged for {} in order {}",
                doc.display(),
                order_name
            );
        }
        // Explicit metadata assertions for the native document.
        let native_orch = doc_dir(&root, &native_doc);
        let native_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(native_orch.join("result.json")).expect("result.json"),
        )
        .expect("result.json parses");
        let native_single_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(single_native.join("result.json")).expect("result.json"),
        )
        .expect("result.json parses");
        assert_eq!(
            native_json["backend"], native_single_json["backend"],
            "native backend must match the single scan: {native_json}"
        );
        assert_eq!(
            native_json["ocr_model_loaded"], false,
            "native document must not report a loaded model: {native_json}"
        );
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets, PDFium, and a GPU-capable binary"]
fn gpu_reports_are_per_document_in_orchestrated_runs() {
    // Rereview BLK-3: the shared actor's cumulative telemetry must not leak
    // into later documents' gpu-resource.json / step-profile.json. Each
    // orchestrated document's output_tokens must equal its own single scan's.
    let doc_a = fixture("normal-1-page.pdf");
    let doc_b = fixture("figure-1-page.pdf");
    let scratch = scratch_dir("gpu-per-doc");
    std::fs::create_dir_all(&scratch).unwrap();

    // A CPU-only test binary cannot produce GPU reports: probe with an
    // explicit vulkan request and skip when the device is unavailable.
    let probe = scratch.join("single-a");
    let (probe_code, _stdout, _stderr) = run_scan_full(
        &[&doc_a],
        &probe,
        &["--device", "vulkan"],
        &[("OCRX_GPU_REPORT", "1")],
    );
    if probe_code == 6 {
        eprintln!("skipping: no usable GPU backend in this binary (probe exit 6)");
        let _ = std::fs::remove_dir_all(&scratch);
        return;
    }
    // A debug GPU build may flag the page degenerated (deterministic exit
    // 10); the per-document comparison only needs the GPU reports on disk.
    assert!(
        probe_code == 0 || probe_code == 10,
        "gpu probe scan must complete (exit {probe_code})"
    );
    let probe_gpu: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(probe.join("gpu-resource.json")).expect("probe gpu-resource.json"),
    )
    .expect("probe gpu-resource.json parses");
    assert_eq!(probe_gpu["backend"], "vulkan", "probe: {probe_gpu}");

    // Single scan of doc B under the same configuration.
    let single_b = scratch.join("single-b");
    let single_b_code = run_scan_full(
        &[&doc_b],
        &single_b,
        &["--device", "vulkan"],
        &[("OCRX_GPU_REPORT", "1")],
    )
    .0;
    assert!(
        single_b_code == 0 || single_b_code == 10,
        "single scan of doc B must complete (exit {single_b_code})"
    );

    // Orchestrated run: both documents share one actor.
    let root = scratch.join("multi");
    let (code, _stdout, _stderr) = run_scan_full(
        &[&doc_a, &doc_b],
        &root,
        &["--device", "vulkan"],
        &[("OCRX_GPU_REPORT", "1")],
    );
    assert!(
        code == 0 || code == 10,
        "orchestrated GPU run must complete (exit {code})"
    );

    let orch_a = doc_dir(&root, &doc_a);
    let orch_b = doc_dir(&root, &doc_b);
    for (doc, single, orch) in [(&doc_a, &probe, &orch_a), (&doc_b, &single_b, &orch_b)] {
        let single_gpu: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(single.join("gpu-resource.json"))
                .unwrap_or_else(|error| panic!("{}: {error}", single.display())),
        )
        .expect("gpu-resource.json parses");
        let orch_gpu: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(orch.join("gpu-resource.json"))
                .unwrap_or_else(|error| panic!("{}: {error}", orch.display())),
        )
        .expect("gpu-resource.json parses");
        assert_eq!(
            orch_gpu["output_tokens"],
            single_gpu["output_tokens"],
            "{} output_tokens must equal its own single scan (no cross-document              accumulation): single={} orchestrated={}",
            doc.display(),
            single_gpu,
            orch_gpu
        );
        // Step-profile debug channel: per-document tokens as well.
        if let (Ok(single_sp), Ok(orch_sp)) = (
            std::fs::read_to_string(single.join("step-profile.json")),
            std::fs::read_to_string(orch.join("step-profile.json")),
        ) {
            let single_sp: serde_json::Value =
                serde_json::from_str(&single_sp).expect("step profile");
            let orch_sp: serde_json::Value = serde_json::from_str(&orch_sp).expect("step profile");
            assert_eq!(
                orch_sp["tokens"],
                single_sp["tokens"],
                "{} step-profile tokens must be per-document",
                doc.display()
            );
        }
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn resume_reuses_committed_pages_without_fallback() {
    // Non-fallback baseline for `shared_fallback_resume_counts_truncated_once`:
    // a plain single-document `--device cpu` resume must reuse the committed
    // pages (skip-commit, no re-recognition) without any compat-key rebuild.
    // Assert: pass 1 commits pages, pass 2 skip-commits them (checkpoint
    // timestamps unchanged), document.md has content.
    let doc = fixture("normal-1-page.pdf");
    let output = scratch_dir("fallback-resume");
    std::fs::create_dir_all(&output).unwrap();

    // PASS 1: fresh scan (no --resume). Commits pages with checkpoints.
    // CPU device: the debug binary has no vulkan feature.
    let (code1, _stdout1, _stderr1) = run_scan_full(&[&doc], &output, &["--device", "cpu"], &[]);
    assert!(
        code1 == 0 || code1 == 10,
        "pass 1 must complete (exit {code1})"
    );

    // Snapshot checkpoint timestamps before resume — a re-run would change
    // them (content hash can't detect a deterministic re-write).
    let mut pre_resume_checkpoints = std::collections::HashMap::new();
    for entry in fs::read_dir(&output).unwrap().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".checkpoint") {
            let meta = entry.metadata().unwrap();
            pre_resume_checkpoints.insert(name, meta.modified().unwrap());
        }
    }
    assert!(
        !pre_resume_checkpoints.is_empty(),
        "pass 1 must produce checkpoints"
    );

    // PASS 2: resume from the same directory. The service's per-page
    // checkpoint matching (with the SAME compat key) skip-commits committed
    // pages — the CLI must NOT double-count them.
    let (code2, _stdout2, _stderr2) = run_scan_full(&[&doc], &output, &["--resume"], &[]);
    assert!(
        code2 == 0 || code2 == 10,
        "resume must complete (exit {code2})"
    );

    // Verify: no committed checkpoint was rewritten during resume.
    for (name, before) in &pre_resume_checkpoints {
        let cp = output.join(name);
        let after = fs::metadata(&cp).unwrap().modified().unwrap();
        assert_eq!(
            *before, after,
            "checkpoint {name} was rewritten during resume"
        );
    }

    // Verify: the final document.md has content.
    let doc_md = output.join("document.md");
    assert!(doc_md.is_file(), "document.md must exist after resume");
    let md_content = fs::read_to_string(&doc_md).unwrap();
    assert!(
        !md_content.trim().is_empty(),
        "document.md must have content"
    );

    let _ = fs::remove_dir_all(&output);
}

#[test]
#[ignore = "set OCRX_E2E_MODEL=1 with installed compact assets and PDFium"]
fn shared_fallback_resume_counts_truncated_once() {
    // Rereview BLK-1/BLK-2: the shared multi-input orchestrator must survive
    // a GPU->CPU runtime fallback on BOTH invocations and count a truncated
    // page exactly once on resume. The seam makes the GPU load ATTEMPT
    // itself fail, so each pass flows through the real
    // Err(gpu_error) -> CPU load -> Some(warning) branch: the compat key is
    // rebuilt (batch floor + ocr-device=cpu), the resume checkpoint matching
    // re-runs, and pass 2 skip-commits pass 1's pages instead of
    // re-recognizing them.
    //
    // Truncation premise: --max-output-tokens 128 on the dense fixture cuts
    // the single page at the token budget, committing a truncated=true
    // checkpoint in pass 1.
    //
    // Capability gate: a binary that cannot attempt vulkan (feature compiled
    // out, no ICD/driver) rejects the device request with exit 6 before the
    // runtime (W1.4). That environment cannot exercise the fallback, so the
    // test reports the skip and returns instead of failing.
    let scratch = scratch_dir("shared-fallback-resume");
    std::fs::create_dir_all(&scratch).unwrap();
    let dense = fixture("dense-text-for-max-tokens.pdf");
    // Two documents so the invocation is genuinely orchestrated (Shared
    // slot); distinct stems give distinct <stem>-<hash8> directories.
    let doc_a = scratch.join("dense-a.pdf");
    let doc_b = scratch.join("dense-b.pdf");
    fs::copy(&dense, &doc_a).unwrap();
    fs::copy(&dense, &doc_b).unwrap();
    let output = scratch.join("shared-output");
    let seam_env = [("OCRX_E2E_FORCE_RUNTIME_FALLBACK", "1")];
    let scan_args = ["--device", "vulkan", "--max-output-tokens", "128"];

    // PASS 1: fresh orchestrated scan; the forced GPU failure falls back to
    // CPU and commits truncated checkpoints under each doc directory.
    let (code1, _stdout1, stderr1) =
        run_scan_full(&[&doc_a, &doc_b], &output, &scan_args, &seam_env);
    if code1 == 6 && stderr1.contains("requested device is unavailable") {
        // W1.4 rejected the device request before the runtime: this binary
        // cannot attempt vulkan (feature compiled out, no ICD/driver), so the
        // GPU->CPU fallback cannot run here. Report the skip, don't fail.
        eprintln!(
            "skip: this binary/hardware cannot attempt vulkan, so the \
             GPU->CPU fallback scenario cannot run here"
        );
        let _ = fs::remove_dir_all(&scratch);
        return;
    }
    assert!(
        code1 == 0 || code1 == 10,
        "pass 1 must complete with the fallback (exit {code1})"
    );

    // Premise + reuse evidence: each doc directory holds a truncated=true
    // checkpoint; snapshot the checkpoint mtimes — pass 2 must not rewrite
    // them (a re-recognition would commit and bump the mtime).
    let mut pass1_mtimes = std::collections::BTreeMap::new();
    for doc in [&doc_a, &doc_b] {
        let dir = doc_dir(&output, doc);
        let mut truncated_seen = false;
        let mut checkpoint_count = 0;
        for entry in fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".checkpoint") {
                continue;
            }
            checkpoint_count += 1;
            let content = fs::read_to_string(entry.path()).unwrap();
            truncated_seen |= content.contains("truncated=true");
            pass1_mtimes.insert(entry.path(), entry.metadata().unwrap().modified().unwrap());
        }
        assert!(
            checkpoint_count >= 1,
            "pass 1 must commit checkpoints for {}",
            doc.display()
        );
        assert!(
            truncated_seen,
            "the dense page must commit a truncated=true checkpoint for {}",
            doc.display()
        );
    }

    // PASS 2: the SAME request (--device vulkan + forced GPU failure) with
    // --resume. The post-fallback compat-key rebuild must align with pass 1's
    // checkpoints so they are skip-committed, and the CLI must count the
    // truncated page exactly once.
    let mut resume_args = scan_args.to_vec();
    resume_args.push("--resume");
    let (code2, _stdout2, _stderr2) =
        run_scan_full(&[&doc_a, &doc_b], &output, &resume_args, &seam_env);
    assert!(
        code2 == 0 || code2 == 10,
        "pass 2 resume must complete (exit {code2})"
    );

    // Reuse evidence: no committed checkpoint was rewritten during resume.
    for (path, before) in &pass1_mtimes {
        let after = fs::metadata(path).unwrap().modified().unwrap();
        assert_eq!(
            *before,
            after,
            "checkpoint {} was rewritten during resume (page re-recognized?)",
            path.display()
        );
    }

    // Per-document results. Universal invariants for both documents: the
    // truncated page is counted exactly once, the diagnostics list exactly
    // one partial entry for page 1, and the merged document.md has content.
    for doc in [&doc_a, &doc_b] {
        let dir = doc_dir(&output, doc);
        let result: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("result.json")).unwrap())
                .unwrap_or_else(|error| panic!("result.json for {}: {error}", doc.display()));
        assert_eq!(
            result["status"],
            "partial",
            "truncated page keeps partial status for {}",
            doc.display()
        );
        assert_eq!(
            result["pages"]["truncated"],
            1,
            "truncated page must be counted exactly once for {}",
            doc.display()
        );
        let partials = result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|diagnostic| diagnostic["status"] == "partial")
            .collect::<Vec<_>>();
        assert_eq!(
            partials.len(),
            1,
            "exactly one partial diagnostic for {}: {:?}",
            doc.display(),
            partials
        );
        assert_eq!(partials[0]["page"], 1, "partial diagnostic targets page 1");
        let md = fs::read_to_string(dir.join("document.md")).unwrap();
        assert!(
            !md.trim().is_empty(),
            "document.md must be merged for {}",
            doc.display()
        );
    }

    // Backend label semantics (M-10): the document whose runtime actually ran
    // in this invocation reports the ACTUAL fallback device (cpu) and carries
    // the real fallback branch's warning. The other document's pages were all
    // resume-reused before any runtime work, so it performed no OCR here and
    // keeps the requested-device label (the same metadata its individual-scan
    // counterpart produces); the pages' provenance lives in their checkpoints
    // (ocr-device=cpu), not in this invocation's backend label.
    for (doc, expected_backend, expect_fallback_warning) in
        [(&doc_a, "cpu", true), (&doc_b, "vulkan", false)]
    {
        let dir = doc_dir(&output, doc);
        let result: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("result.json")).unwrap())
                .unwrap_or_else(|error| panic!("result.json for {}: {error}", doc.display()));
        assert_eq!(
            result["backend"],
            expected_backend,
            "backend label for {}",
            doc.display()
        );
        let warnings = result["warnings"].as_array().unwrap();
        assert_eq!(
            warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|text| text.starts_with("gpu init failed; fallback to cpu"))),
            expect_fallback_warning,
            "fallback warning presence for {}",
            doc.display()
        );
    }

    let _ = fs::remove_dir_all(&scratch);
}
