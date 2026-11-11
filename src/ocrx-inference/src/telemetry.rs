//! Generic telemetry and optional GPU sampling (plan §4.1/§8.8).
//!
//! The VRAM sampler is the `OCRX_GPU_REPORT` support primitive migrated
//! from the CLI: a background thread polls the driver for used VRAM and
//! reports the observed peak when stopped. Purely observational — no scan,
//! PDF, or model concepts.

/// Starts a best-effort VRAM peak sampler (Windows/nvidia-smi only; any
/// other platform returns `None` and the report simply omits the peak).
#[must_use]
pub fn start_vram_sampler() -> Option<(
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<u64>,
)> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_worker = std::sync::Arc::clone(&done);
    let handle = std::thread::Builder::new()
        .name("vram-sampler".to_owned())
        .spawn(move || {
            let mut peak_mb = 0_u64;
            while !done_worker.load(std::sync::atomic::Ordering::SeqCst) {
                if let Ok(output) = std::process::Command::new("nvidia-smi")
                    .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
                    .output()
                    && output.status.success()
                    && let Some(first) = String::from_utf8_lossy(&output.stdout).lines().next()
                    && let Ok(mb) = first.trim().parse::<u64>()
                {
                    peak_mb = peak_mb.max(mb);
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            peak_mb.saturating_mul(1024 * 1024)
        })
        .ok()?;
    Some((done, handle))
}

/// Stops the sampler and returns the observed peak (`None` when nothing
/// meaningful was observed or no sampler was running).
#[must_use]
pub fn stop_vram_sampler(
    sampler: Option<(
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<u64>,
    )>,
) -> Option<u64> {
    let (done, handle) = sampler?;
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let bytes = handle.join().unwrap_or(0);
    (bytes > 0).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_a_missing_sampler_is_none() {
        assert_eq!(stop_vram_sampler(None), None);
    }

    #[test]
    fn sampler_round_trip_is_available_on_windows() {
        if !cfg!(target_os = "windows") {
            // Non-Windows platforms start nothing; stopping still resolves.
            assert!(start_vram_sampler().is_none());
            return;
        }
        let sampler = start_vram_sampler();
        // A short run may or may not observe a driver; both are valid — the
        // contract is that stopping never hangs and never panics.
        let _peak = stop_vram_sampler(sampler);
    }
}
