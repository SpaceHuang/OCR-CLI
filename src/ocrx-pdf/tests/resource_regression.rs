//! W0.3 resource regression sampling skeleton.
//!
//! Spawns the `pdfium-check` worker over the 100-page fixture and samples its
//! RSS curve with the cross-platform [`ocrx_core::resource_sampler`] probe,
//! mirroring the macOS `macos-pdf-rss-check.sh` baseline. This skeleton asserts
//! *sampling availability* on Windows (the macOS sandbox failure mode) and a
//! clean worker exit; the non-linear-growth gate itself belongs to the W0.3/W6
//! baseline measurement on the 16 GB CPU-only reference device.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use ocrx_core::resource_sampler::{ProcessMemorySampler, RealProcessMemorySampler};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/fixtures/pdf")
        .join(name)
}

/// Vendor PDFium runtime directory, when present (see W2.1 layout).
fn pdfium_lib_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/bin");
    std::fs::canonicalize(&dir).ok()
}

#[test]
fn pdfium_resource_regression_collects_rss_curve() {
    let Some(lib_dir) = pdfium_lib_dir() else {
        eprintln!(
            "note: pdfium runtime not present at vendor/pdfium/bin; \
             skipping resource regression sampling"
        );
        return;
    };
    let input = fixture("normal-100-pages.pdf");
    if !input.is_file() {
        eprintln!("note: 100-page fixture missing; skipping resource regression sampling");
        return;
    }

    let started = std::time::Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pdfium-check"))
        .arg(&lib_dir)
        .arg(&input)
        .env("OCRX_PDFIUM_CHECK_DELAY_MS", "20")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("pdfium-check must spawn");

    let probe = RealProcessMemorySampler;
    let pid = child.id();
    let mut rss_kb_samples: Vec<u64> = Vec::new();
    loop {
        match child.try_wait().expect("worker must stay waitable") {
            Some(status) => {
                assert!(status.success(), "worker must exit cleanly: {status:?}");
                break;
            }
            None => {
                if let Some(sample) = probe.sample_rss(pid) {
                    rss_kb_samples.push(sample.rss_kb);
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let elapsed_ms = started.elapsed().as_millis();

    // Sampling availability gate (macOS `rss-check=PASS` equivalent): enough
    // non-zero samples were taken to interpret the trend later.
    assert!(
        rss_kb_samples.len() >= 3,
        "insufficient RSS samples ({}); sampler is not usable for the baseline",
        rss_kb_samples.len()
    );
    assert!(
        rss_kb_samples.iter().all(|&rss_kb| rss_kb > 0),
        "every sample must carry a real working-set value, not zero"
    );
    let min_kb = rss_kb_samples.iter().min().copied().unwrap_or_default();
    let max_kb = rss_kb_samples.iter().max().copied().unwrap_or_default();
    let last_kb = rss_kb_samples[rss_kb_samples.len() - 1];
    println!(
        "rss-check=PASS samples={} min_kb={min_kb} max_kb={max_kb} last_kb={last_kb} elapsed_ms={elapsed_ms}",
        rss_kb_samples.len()
    );
}
