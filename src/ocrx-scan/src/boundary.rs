//! Boundary sample instrumentation (root-cause plan SP-T01, v1.2).
//!
//! Records what each decode path (scan / direct) actually feeds to the
//! runtime, with a joinable identity schema, behind an opt-in env gate
//! (`OCRX_BOUNDARY_SAMPLES_OUT` = JSONL path). Zero behavior when unset.
//!
//! Page identity convention (frozen, plan BLOCK-1): `page_number` is
//! 1-based everywhere; `page_index` is the 0-based render index and is
//! recorded alongside — never derived back from the page number.

use serde::Serialize;

/// One model-boundary observation. Both the scan path and the direct
/// harness emit this exact schema; E1 joins on
/// (`page_number`, `attempt = 1`) with a hard pairing precondition.
#[derive(Debug, Clone, Serialize)]
pub struct BoundarySample {
    /// JSONL schema version (v2 adds `base_image_tokens`; v1 records lack
    /// it and the joiner treats the base grid as uncompared for them).
    pub schema_version: u32,
    /// The scanned document's identity (compatibility input hash).
    pub input_sha256: String,
    /// Per-process invocation id (scan run / probe run).
    pub run_id: String,
    /// Which decode path produced the record.
    pub source: &'static str,
    /// 1-based page number (canonical identity).
    pub page_number: u32,
    /// 0-based render index (`page_number - 1`), recorded explicitly.
    pub page_index: u32,
    /// Ladder attempt (1 = plain attempt; the only direct-comparable one).
    pub attempt: u32,
    /// Sequential window id within the run.
    pub window_id: u32,
    /// Slot within the window (serial path = 0).
    pub slot: u32,
    pub rgb8_len: usize,
    pub rgb8_sha256: String,
    pub prompt_sha256: String,
    pub width: u32,
    pub height: u32,
    pub max_output_tokens: u32,
    /// The EFFECTIVE base image-token grid for this attempt (the mtmd
    /// context budget: the per-model resolved scan default, before any
    /// ladder override — root-cause plan review round 6).
    pub base_image_tokens: u32,
    pub image_tokens_override: Option<u32>,
    pub retry_sampling: Option<(u32, u32)>,
    /// Whether the generation produced usable text.
    pub outcome: &'static str,
}

/// JSONL writer (one `BoundarySample` per line, appended).
pub struct BoundaryRecorder {
    out: Option<std::path::PathBuf>,
    run_id: String,
    input_sha256: String,
    window_seq: std::cell::Cell<u32>,
}

impl BoundaryRecorder {
    /// Builds the recorder from the run's document identity and the env
    /// gate. Disabled (`out = None`) when `OCRX_BOUNDARY_SAMPLES_OUT` is
    /// unset — every call site then costs one `Option` check.
    #[must_use]
    pub fn from_env(input_sha256: &str) -> Self {
        let out = std::env::var_os("OCRX_BOUNDARY_SAMPLES_OUT")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from);
        Self::new(out, input_sha256)
    }

    /// Test/可视化 constructor with an explicit sink.
    #[must_use]
    pub fn new(out: Option<std::path::PathBuf>, input_sha256: &str) -> Self {
        Self {
            out,
            run_id: format!(
                "{}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0),
                std::process::id()
            ),
            input_sha256: input_sha256.to_owned(),
            window_seq: std::cell::Cell::new(0),
        }
    }

    /// Claims the next sequential window id.
    pub fn next_window(&self) -> u32 {
        let id = self.window_seq.get();
        self.window_seq.set(id + 1);
        id
    }

    /// Appends one boundary sample (best-effort: instrumentation must
    /// never break a scan; a write failure is reported to stderr once).
    pub fn record(&self, sample: BoundarySample) {
        let Some(out) = &self.out else {
            return;
        };
        let line = match serde_json::to_string(&sample) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[boundary] serialize failed: {error}");
                return;
            }
        };
        use std::io::Write as _;
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = result {
            eprintln!("[boundary] sample write failed: {error}");
        }
    }
}
