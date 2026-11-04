//! Env-gated decode-step profiling for the windowed decode loop (phase-2
//! plan T0.1/A1).
//!
//! The channel is deliberately separate from [`super::RuntimeTelemetry`] and
//! the `OCRX_GPU_REPORT=1` report: it only exists when `OCRX_STEP_PROFILE`
//! is set, writes at most one debug file (`step-profile.json`) per scan, and
//! every field is opt-in telemetry — no product output shape changes.

use std::time::Instant;

/// Enablement variable: set to anything other than `0`/empty to record the
/// per-step segment costs of the windowed decode loop.
pub const STEP_PROFILE_ENV: &str = "OCRX_STEP_PROFILE";

/// Segments of the window decode loop that are measured separately
/// (plan T0.1: batch 构建 / 逐槽 FFI / 采样 / detokenize / 环检测 / 等待 decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Segment {
    /// mmproj image encoding (`eval_chunks`), per slot — the GPU workload a
    /// host-path optimization cannot remove (plan §0.3 ④).
    Encode,
    /// Bitmap creation + tokenize, per slot.
    SlotSetup,
    /// Per-step batch build: the slot scan plus every `LlamaBatch::add` (and
    /// with A2a the batch reuse itself).
    BatchBuild,
    /// The blocking `context.decode` wait per step.
    DecodeWait,
    /// Per-slot `get_logits_ith` FFI reads.
    LogitsFfi,
    /// Per-slot greedy argmax scan over the full vocabulary.
    Argmax,
    /// Per-slot detokenize FFI (`token_to_piece_bytes`) plus output push.
    Detokenize,
    /// Per-slot repetition-detector `push`.
    Ring,
}

const SEGMENT_COUNT: usize = 8;

fn segment_index(segment: Segment) -> usize {
    match segment {
        Segment::Encode => 0,
        Segment::SlotSetup => 1,
        Segment::BatchBuild => 2,
        Segment::DecodeWait => 3,
        Segment::LogitsFfi => 4,
        Segment::Argmax => 5,
        Segment::Detokenize => 6,
        Segment::Ring => 7,
    }
}

/// Aggregated per-step profiling snapshot, in microseconds. Accumulated by
/// the inference worker across every windowed decode of the process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepProfile {
    pub windows: u64,
    pub steps: u64,
    pub tokens: u64,
    /// Total wall time of the windowed decode functions.
    pub window_total_us: u64,
    pub encode_us: u64,
    pub slot_setup_us: u64,
    pub batch_build_us: u64,
    pub decode_wait_us: u64,
    pub logits_ffi_us: u64,
    pub argmax_us: u64,
    pub detokenize_us: u64,
    pub ring_us: u64,
    /// Window wall time not attributed to any measured segment (progress
    /// messaging, allocations outside segments, slot teardown).
    pub residual_us: u64,
}

impl StepProfile {
    /// Whether profiling was requested through the environment.
    #[must_use]
    pub fn env_enabled() -> bool {
        enabled_from(std::env::var_os(STEP_PROFILE_ENV).as_deref())
    }

    /// Per-document delta from a baseline snapshot (X1 shared actor): every
    /// field is additive, so the orchestrated document reports
    /// `snapshot - baseline` (saturating) instead of the actor's cumulative.
    #[must_use]
    pub fn delta_from(&self, base: &Self) -> Self {
        Self {
            windows: self.windows.saturating_sub(base.windows),
            steps: self.steps.saturating_sub(base.steps),
            tokens: self.tokens.saturating_sub(base.tokens),
            window_total_us: self.window_total_us.saturating_sub(base.window_total_us),
            encode_us: self.encode_us.saturating_sub(base.encode_us),
            slot_setup_us: self.slot_setup_us.saturating_sub(base.slot_setup_us),
            batch_build_us: self.batch_build_us.saturating_sub(base.batch_build_us),
            decode_wait_us: self.decode_wait_us.saturating_sub(base.decode_wait_us),
            logits_ffi_us: self.logits_ffi_us.saturating_sub(base.logits_ffi_us),
            argmax_us: self.argmax_us.saturating_sub(base.argmax_us),
            detokenize_us: self.detokenize_us.saturating_sub(base.detokenize_us),
            ring_us: self.ring_us.saturating_sub(base.ring_us),
            residual_us: self.residual_us.saturating_sub(base.residual_us),
        }
    }

    /// Sums `other` into `self` (the worker accumulates across windows).
    pub fn accumulate(&mut self, other: &Self) {
        self.windows += other.windows;
        self.steps += other.steps;
        self.tokens += other.tokens;
        self.window_total_us += other.window_total_us;
        self.encode_us += other.encode_us;
        self.slot_setup_us += other.slot_setup_us;
        self.batch_build_us += other.batch_build_us;
        self.decode_wait_us += other.decode_wait_us;
        self.logits_ffi_us += other.logits_ffi_us;
        self.argmax_us += other.argmax_us;
        self.detokenize_us += other.detokenize_us;
        self.ring_us += other.ring_us;
        self.residual_us += other.residual_us;
    }

    /// Frozen debug-channel JSON (`step-profile.json`). Field names are a
    /// contract with the phase-2 diagnostic tooling; additions may append,
    /// renames/removals are breaking.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema_version\":1,\"windows\":{},\"steps\":{},\"tokens\":{},\"us\":{{\"window_total\":{},\"encode\":{},\"slot_setup\":{},\"batch_build\":{},\"decode_wait\":{},\"logits_ffi\":{},\"argmax\":{},\"detokenize\":{},\"ring\":{},\"residual\":{}}}}}",
            self.windows,
            self.steps,
            self.tokens,
            self.window_total_us,
            self.encode_us,
            self.slot_setup_us,
            self.batch_build_us,
            self.decode_wait_us,
            self.logits_ffi_us,
            self.argmax_us,
            self.detokenize_us,
            self.ring_us,
            self.residual_us,
        )
    }
}

fn enabled_from(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// Per-window segment recorder. Disabled instances make every call a no-op
/// branch (the profiling gate must cost nothing in normal scans).
pub(crate) struct StepProfiler {
    enabled: bool,
    open: [Option<Instant>; SEGMENT_COUNT],
    profile: StepProfile,
    window_open: bool,
}

impl StepProfiler {
    /// Env-gated constructor used by the decode loop.
    pub(crate) fn new() -> Self {
        Self::with_enabled(StepProfile::env_enabled())
    }

    pub(crate) fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            open: [None; SEGMENT_COUNT],
            profile: StepProfile::default(),
            window_open: false,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    /// Starts a wall-clock window (called once per `generate_window_pages`).
    pub(crate) fn open_window(&mut self) {
        if !self.enabled {
            return;
        }
        self.window_open = true;
        self.profile.windows += 1;
    }

    /// Closes the window with its total wall time and books the residual
    /// (total minus every measured segment).
    pub(crate) fn close_window(&mut self, total: std::time::Duration) {
        if !self.enabled || !self.window_open {
            return;
        }
        self.window_open = false;
        for open in &mut self.open {
            *open = None;
        }
        let measured = self.profile.encode_us
            + self.profile.slot_setup_us
            + self.profile.batch_build_us
            + self.profile.decode_wait_us
            + self.profile.logits_ffi_us
            + self.profile.argmax_us
            + self.profile.detokenize_us
            + self.profile.ring_us;
        let total_us = u64::try_from(total.as_micros()).unwrap_or(u64::MAX);
        self.profile.window_total_us += total_us;
        self.profile.residual_us += total_us.saturating_sub(measured);
    }

    pub(crate) fn begin(&mut self, segment: Segment) {
        if !self.enabled {
            return;
        }
        let index = segment_index(segment);
        if self.open[index].is_none() {
            self.open[index] = Some(Instant::now());
        }
    }

    pub(crate) fn end(&mut self, segment: Segment) {
        if !self.enabled {
            return;
        }
        let index = segment_index(segment);
        if let Some(started) = self.open[index].take() {
            let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
            let bucket = match segment {
                Segment::Encode => &mut self.profile.encode_us,
                Segment::SlotSetup => &mut self.profile.slot_setup_us,
                Segment::BatchBuild => &mut self.profile.batch_build_us,
                Segment::DecodeWait => &mut self.profile.decode_wait_us,
                Segment::LogitsFfi => &mut self.profile.logits_ffi_us,
                Segment::Argmax => &mut self.profile.argmax_us,
                Segment::Detokenize => &mut self.profile.detokenize_us,
                Segment::Ring => &mut self.profile.ring_us,
            };
            *bucket += micros;
        }
    }

    /// One shared decode step executed (a batch was decoded).
    pub(crate) fn count_step(&mut self) {
        if self.enabled {
            self.profile.steps += 1;
        }
    }

    /// One token appended to a slot's output.
    pub(crate) fn count_token(&mut self) {
        if self.enabled {
            self.profile.tokens += 1;
        }
    }

    /// The accumulated snapshot (safe to take mid-window).
    pub(crate) fn snapshot(&self) -> StepProfile {
        self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::{Segment, StepProfile, StepProfiler, enabled_from};
    use std::time::Duration;

    #[test]
    fn step_profile_json_schema_is_frozen() {
        // A1: the debug channel's field names are a contract with the T0.1
        // diagnostic tooling. This golden string locks them; new fields may
        // append, renames or removals are breaking changes.
        let profile = StepProfile {
            windows: 2,
            steps: 33,
            tokens: 128,
            window_total_us: 1_000_000,
            encode_us: 400_000,
            slot_setup_us: 5_000,
            batch_build_us: 12_000,
            decode_wait_us: 500_000,
            logits_ffi_us: 40_000,
            argmax_us: 30_000,
            detokenize_us: 8_000,
            ring_us: 2_000,
            residual_us: 3_000,
        };
        assert_eq!(
            profile.to_json(),
            "{\"schema_version\":1,\"windows\":2,\"steps\":33,\"tokens\":128,\"us\":{\"window_total\":1000000,\"encode\":400000,\"slot_setup\":5000,\"batch_build\":12000,\"decode_wait\":500000,\"logits_ffi\":40000,\"argmax\":30000,\"detokenize\":8000,\"ring\":2000,\"residual\":3000}}"
        );
        assert_eq!(
            StepProfile::default().to_json(),
            "{\"schema_version\":1,\"windows\":0,\"steps\":0,\"tokens\":0,\"us\":{\"window_total\":0,\"encode\":0,\"slot_setup\":0,\"batch_build\":0,\"decode_wait\":0,\"logits_ffi\":0,\"argmax\":0,\"detokenize\":0,\"ring\":0,\"residual\":0}}"
        );
    }

    #[test]
    fn env_enablement_treats_unset_zero_and_empty_as_off() {
        assert!(!enabled_from(None));
        assert!(!enabled_from(Some(std::ffi::OsStr::new("0"))));
        assert!(!enabled_from(Some(std::ffi::OsStr::new(""))));
        assert!(enabled_from(Some(std::ffi::OsStr::new("1"))));
        assert!(enabled_from(Some(std::ffi::OsStr::new("true"))));
    }

    #[test]
    fn profiler_accumulates_segments_counts_and_residual() {
        let mut profiler = StepProfiler::with_enabled(true);
        profiler.open_window();
        profiler.begin(Segment::Encode);
        std::thread::sleep(Duration::from_millis(2));
        profiler.end(Segment::Encode);
        profiler.begin(Segment::BatchBuild);
        profiler.end(Segment::BatchBuild);
        profiler.count_step();
        profiler.count_token();
        profiler.close_window(Duration::from_millis(10));
        let profile = profiler.snapshot();
        assert_eq!(profile.windows, 1);
        assert_eq!(profile.steps, 1);
        assert_eq!(profile.tokens, 1);
        assert_eq!(profile.window_total_us, 10_000);
        assert!(
            profile.encode_us >= 1_000,
            "encode bucket must record sleep"
        );
        assert!(
            profile.window_total_us
                >= profile.encode_us
                    + profile.slot_setup_us
                    + profile.batch_build_us
                    + profile.decode_wait_us
                    + profile.logits_ffi_us
                    + profile.argmax_us
                    + profile.detokenize_us
                    + profile.ring_us,
        );
        assert!(
            profile.residual_us > 0,
            "sleep-heavy window leaves residual"
        );
        // A second window accumulates on top of the first.
        profiler.open_window();
        profiler.close_window(Duration::from_millis(5));
        assert_eq!(profiler.snapshot().windows, 2);
        assert_eq!(profiler.snapshot().window_total_us, 15_000);
    }

    #[test]
    fn disabled_profiler_records_nothing() {
        let mut profiler = StepProfiler::with_enabled(false);
        profiler.open_window();
        profiler.begin(Segment::DecodeWait);
        profiler.end(Segment::DecodeWait);
        profiler.count_step();
        profiler.count_token();
        profiler.close_window(Duration::from_millis(10));
        assert_eq!(profiler.snapshot(), StepProfile::default());
    }

    #[test]
    fn unbalanced_begin_is_ignored_and_close_window_cancels_open_segments() {
        let mut profiler = StepProfiler::with_enabled(true);
        // An `end` without `begin` must not panic or record.
        profiler.end(Segment::Ring);
        assert_eq!(profiler.snapshot().ring_us, 0);
        // A segment left open when the window closes is cancelled (early
        // error paths skip the balanced end) and does not poison the next
        // window.
        profiler.open_window();
        profiler.begin(Segment::SlotSetup);
        profiler.close_window(Duration::from_millis(4));
        assert_eq!(profiler.snapshot().slot_setup_us, 0);
        profiler.open_window();
        profiler.begin(Segment::SlotSetup);
        std::thread::sleep(Duration::from_millis(1));
        profiler.end(Segment::SlotSetup);
        profiler.close_window(Duration::from_millis(4));
        assert!(profiler.snapshot().slot_setup_us >= 500);
    }

    #[test]
    fn step_profile_delta_from_isolates_per_document_contribution() {
        // Rereview BLK-3: the shared actor's step profile accumulates across
        // documents; the orchestrated document reports snapshot - baseline
        // (saturating) in every additive field.
        let base = StepProfile {
            windows: 2,
            steps: 10,
            tokens: 100,
            window_total_us: 1_000_000,
            encode_us: 100_000,
            logits_ffi_us: 200_000,
            ..StepProfile::default()
        };
        let snapshot = StepProfile {
            windows: 3,
            steps: 15,
            tokens: 160,
            window_total_us: 1_600_000,
            encode_us: 160_000,
            logits_ffi_us: 260_000,
            argmax_us: 50_000,
            ..StepProfile::default()
        };
        let delta = snapshot.delta_from(&base);
        assert_eq!(delta.windows, 1);
        assert_eq!(delta.steps, 5);
        assert_eq!(delta.tokens, 60);
        assert_eq!(delta.window_total_us, 600_000);
        assert_eq!(delta.encode_us, 60_000);
        assert_eq!(delta.logits_ffi_us, 60_000);
        assert_eq!(delta.argmax_us, 50_000);
        assert_eq!(snapshot.delta_from(&StepProfile::default()), snapshot);
    }

    #[test]
    fn step_profile_accumulate_sums_every_field() {
        let base = StepProfile {
            windows: 1,
            steps: 2,
            tokens: 3,
            window_total_us: 100,
            encode_us: 10,
            slot_setup_us: 11,
            batch_build_us: 12,
            decode_wait_us: 13,
            logits_ffi_us: 14,
            argmax_us: 15,
            detokenize_us: 16,
            ring_us: 17,
            residual_us: 18,
        };
        let mut total = base;
        total.accumulate(&base);
        assert_eq!(
            total,
            StepProfile {
                windows: 2,
                steps: 4,
                tokens: 6,
                window_total_us: 200,
                encode_us: 20,
                slot_setup_us: 22,
                batch_build_us: 24,
                decode_wait_us: 26,
                logits_ffi_us: 28,
                argmax_us: 30,
                detokenize_us: 32,
                ring_us: 34,
                residual_us: 36,
            }
        );
    }

    // --- A1: the debug channel must not touch the OCRX_GPU_REPORT schema. --

    /// A1 schema-compat lock: the GPU report's exact JSON shape is unchanged
    /// by the existence of the step-profile channel. The two channels share
    /// no state, but this golden test pins the product schema the phase-2
    /// gates (MAJOR-1 structured comparison) depend on.
    #[test]
    fn gpu_resource_report_schema_stays_untouched_by_step_channel() {
        let report = ocrx_core::GpuResourceReport {
            backend: "Vulkan".to_owned(),
            vram_peak_bytes: Some(4_500_000_000),
            unified_memory_peak_bytes: None,
            load_time_ms: Some(1_300),
            first_token_ms: Some(118),
            output_tokens: 12_839,
            output_tokens_per_sec: 424.0,
            peak_memory_bytes: None,
        };
        assert_eq!(
            report.to_json(),
            "{\"schema_version\":1,\"backend\":\"Vulkan\",\"vram_peak_bytes\":4500000000,\"unified_memory_peak_bytes\":null,\"load_time_ms\":1300,\"first_token_ms\":118,\"output_tokens\":12839,\"output_tokens_per_sec\":424.000,\"peak_memory_bytes\":null}"
        );
    }
}
