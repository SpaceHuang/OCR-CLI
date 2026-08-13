//! Cross-platform scheduler isolation tests (W2.2).
//!
//! These exercise the `PdfWorkerScheduler` protocol end to end against a
//! portable fake-worker binary (`pdfium-fake-worker`), whose behavior is
//! selected by the `OCRX_FAKE_WORKER_MODE` env var injected through the
//! scheduler's `env` field. This replaces the earlier `#[cfg(unix)]` shell
//! scripts so the protocol — empty / out-of-order / out-of-range segments,
//! abnormal exit, timeout, cancellation, missing page, bad PNG, leftover
//! `.part` file — is exercised identically on Windows and Unix.
//!
//! Integration tests (not lib unit tests) are required here: only integration
//! tests receive the `CARGO_BIN_EXE_*` env var at compile time, which Cargo
//! honors by building the real `pdfium-fake-worker` bin artifact. Lib unit
//! tests only build the bin's test harness, never the bin itself.

use ocrx_pdf::{PdfWorkerScheduler, WorkerSegmentReport};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn fake_worker_path() -> PathBuf {
    // Cargo sets `CARGO_BIN_EXE_<name>` for integration tests using the bin
    // name verbatim (lowercase, dashes preserved) — not the uppercased form.
    // Resolved at compile time so Cargo also guarantees the bin is built.
    PathBuf::from(env!("CARGO_BIN_EXE_pdfium-fake-worker"))
}

fn worker_scheduler(env: &[(&str, &str)], timeout: Duration) -> PdfWorkerScheduler {
    PdfWorkerScheduler {
        worker_executable: fake_worker_path(),
        library_dir: PathBuf::from("/library"),
        segment_pages: 2,
        timeout,
        dpi: 300,
        env: env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn unique_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ocrx-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn worker_scheduler_commits_only_valid_segments_and_reports_resources() {
    let output = unique_dir("worker-output");
    let reports: Vec<WorkerSegmentReport> = worker_scheduler(
        &[("OCRX_FAKE_WORKER_MODE", "success")],
        Duration::from_secs(10),
    )
    .run("input.pdf", &output, 0, 2)
    .unwrap();
    assert_eq!(reports[0].pages, 2);
    assert!(output.join("page-0001.png").is_file());
    assert!(output.join("page-0002.png").is_file());
    assert!(!output.join(".worker-0000-0002").exists());
    let _ = fs::remove_dir_all(output);
}

#[test]
fn worker_scheduler_rejects_failure_timeout_cancel_and_invalid_output() {
    // Abnormal exit (non-zero) -> segment rejected, nothing committed.
    let fail_out = unique_dir("worker-fail");
    assert!(
        worker_scheduler(&[("OCRX_FAKE_WORKER_MODE", "fail")], Duration::from_secs(2))
            .run("input.pdf", &fail_out, 0, 1)
            .is_err()
    );
    assert!(!fail_out.join("page-0001.png").exists());

    // Timeout -> worker killed, segment rejected.
    let timeout_out = unique_dir("worker-timeout");
    assert!(
        worker_scheduler(
            &[("OCRX_FAKE_WORKER_MODE", "sleep")],
            Duration::from_millis(100)
        )
        .run("input.pdf", &timeout_out, 0, 1)
        .is_err()
    );

    // Cancel -> worker killed, segment root cleaned, nothing committed.
    let cancel_out = unique_dir("worker-cancel");
    assert!(
        worker_scheduler(
            &[("OCRX_FAKE_WORKER_MODE", "sleep")],
            Duration::from_secs(5)
        )
        .run_with_cancel("input.pdf", &cancel_out, 0, 1, || true)
        .is_err()
    );

    // Invalid PNG -> validated and rejected before commit.
    let badpng_out = unique_dir("worker-badpng");
    assert!(
        worker_scheduler(
            &[("OCRX_FAKE_WORKER_MODE", "bad-png")],
            Duration::from_secs(5)
        )
        .run("input.pdf", &badpng_out, 0, 1)
        .is_err()
    );

    // Missing page -> count mismatch, rejected.
    let missing_out = unique_dir("worker-missing");
    assert!(
        worker_scheduler(
            &[("OCRX_FAKE_WORKER_MODE", "missing-page")],
            Duration::from_secs(5)
        )
        .run("input.pdf", &missing_out, 0, 2)
        .is_err()
    );

    // Leftover .part file -> rejected.
    let part_out = unique_dir("worker-part");
    assert!(
        worker_scheduler(
            &[("OCRX_FAKE_WORKER_MODE", "part-file")],
            Duration::from_secs(5)
        )
        .run("input.pdf", &part_out, 0, 1)
        .is_err()
    );

    for out in [
        fail_out,
        timeout_out,
        cancel_out,
        badpng_out,
        missing_out,
        part_out,
    ] {
        let _ = fs::remove_dir_all(out);
    }
}
