//! Commit-path profiling (phase-3 plan T1): per-page render-wait, PNG
//! encode, commit write and finalize timings on the scan/output side of the
//! commit pipeline.
//!
//! Deliberately separate from `ocrx_runtime::StepProfile` (whose `encode`
//! bucket means mmproj image encoding inside the runtime actor — phase-2
//! review BLK-2): this channel only exists when `OCRX_STEP_PROFILE` opts in,
//! writes at most one debug file (`commit-profile.json`) per document, and
//! every field is opt-in telemetry — no product output shape changes.
//!
//! Indexing contract: one row per (page, render generation). Every render
//! call allocates the page's next generation, so degenerate retries and
//! depth-1 fallbacks each get their own row; `mark_terminal` folds the
//! page's current generation into the totals, so totals only ever count
//! terminal attempts (v2.1 T1 contract).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// One (page, render generation) row of the commit-path profile,
/// microseconds per segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCommitProfile {
    pub page: u32,
    /// Scan-locally monotonic render attempt identity (1-based per page).
    pub generation: u64,
    /// Caller-side wall-clock wait on `PageRenderer::render` — the only
    /// segment that feeds the R/D/E/W/F wall-clock model.
    pub render_wait_us: u64,
    /// PDFium owner working time (diagnostic only; the caller wait already
    /// covers it — never sum the two).
    pub owner_render_us: u64,
    /// CLI-side full-page PNG encode (`RgbImage::write_to`).
    pub png_encode_us: u64,
    /// `commit_page` write span (first atomic write through atomic rename).
    pub commit_write_us: u64,
}

/// Frozen debug-channel snapshot (`commit-profile.json`). Field names are a
/// contract with the phase-3 diagnostic tooling; additions may append,
/// renames/removals are breaking.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitProfile {
    pub pages: Vec<PageCommitProfile>,
    /// Totals over terminal attempts only (pages that reached `mark_terminal`).
    pub pages_total: u64,
    pub render_wait_us: u64,
    pub owner_render_us: u64,
    pub png_encode_us: u64,
    pub commit_write_us: u64,
    /// `finalize_document` + cleanup span, recorded once per scan.
    pub finalize_us: u64,
}

impl CommitProfile {
    /// Frozen debug-channel JSON. The exact shape is locked by the golden
    /// test; `schema_version` may only grow with append-only changes.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\"schema_version\":1,\"pages\":[");
        for (index, page) in self.pages.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"page\":{},\"generation\":{},\"render_wait_us\":{},\"owner_render_us\":{},\"png_encode_us\":{},\"commit_write_us\":{}}}",
                page.page,
                page.generation,
                page.render_wait_us,
                page.owner_render_us,
                page.png_encode_us,
                page.commit_write_us,
            ));
        }
        json.push_str(&format!(
            "],\"totals\":{{\"pages\":{},\"render_wait_us\":{},\"owner_render_us\":{},\"png_encode_us\":{},\"commit_write_us\":{}}},\"finalize_us\":{}}}",
            self.pages_total,
            self.render_wait_us,
            self.owner_render_us,
            self.png_encode_us,
            self.commit_write_us,
            self.finalize_us,
        ));
        json
    }
}

/// Shared recorder sink handed to the renderer call sites (inside
/// `ScanService`), the CLI recognizer (PNG encode) and the commit/finalize
/// paths. A disabled recorder makes every method a no-op; an enabled one is
/// `Arc<Mutex<..>>` so the recognizer's clone and the service see one state.
#[derive(Clone)]
pub struct CommitRecorder {
    inner: Arc<Mutex<RecorderState>>,
}

struct RecorderState {
    enabled: bool,
    /// Next generation per page (1-based; 0 means "no render recorded yet").
    generations: BTreeMap<u32, u64>,
    rows: BTreeMap<(u32, u64), PageCommitProfile>,
    totals: CommitProfile,
}

fn next_generation(state: &mut RecorderState, page: u32) -> u64 {
    let next = state.generations.entry(page).or_insert(0);
    *next += 1;
    *next
}

impl CommitRecorder {
    /// A recorder that records nothing (the env-off default).
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecorderState {
                enabled: false,
                generations: BTreeMap::new(),
                rows: BTreeMap::new(),
                totals: CommitProfile::default(),
            })),
        }
    }

    /// Env-gated recorder: enabled iff `OCRX_STEP_PROFILE` opts in (the same
    /// switch as the runtime step profile; one env, two debug files).
    #[must_use]
    pub fn new_gated() -> Self {
        Self::with_enabled(ocrx_runtime::StepProfile::env_enabled())
    }

    #[must_use]
    pub fn with_enabled(enabled: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecorderState {
                enabled,
                generations: BTreeMap::new(),
                rows: BTreeMap::new(),
                totals: CommitProfile::default(),
            })),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.enabled)
            .unwrap_or(false)
    }

    /// Allocates the page's next render generation and starts the caller's
    /// render wait. Pair with [`Self::record_render_wait`].
    pub fn begin_render(&self, page: u32) -> u64 {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return 0,
        };
        if !state.enabled {
            return 0;
        }
        let generation = next_generation(&mut state, page);
        state
            .rows
            .entry((page, generation))
            .or_insert(PageCommitProfile {
                page,
                generation,
                render_wait_us: 0,
                owner_render_us: 0,
                png_encode_us: 0,
                commit_write_us: 0,
            });
        generation
    }

    pub fn record_render_wait(&self, page: u32, generation: u64, micros: u64) {
        self.edit_row(page, generation, |row| row.render_wait_us = micros);
    }

    pub fn record_owner_render(&self, page: u32, micros: u64) {
        self.edit_current(page, |row| row.owner_render_us = micros);
    }

    /// Records a CLI-side PNG encode against the page's current generation.
    pub fn record_png_encode(&self, page: u32, micros: u64) {
        self.edit_current(page, |row| row.png_encode_us = micros);
    }

    pub fn record_commit_write(&self, page: u32, micros: u64) {
        self.edit_current(page, |row| row.commit_write_us = micros);
    }

    /// Folds the page's current generation into the terminal totals.
    pub fn mark_terminal(&self, page: u32) {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if !state.enabled {
            return;
        }
        if let Some(generation) = state.generations.get(&page).copied() {
            if let Some(row) = state.rows.get(&(page, generation)).copied() {
                state.totals.pages_total += 1;
                state.totals.render_wait_us += row.render_wait_us;
                state.totals.owner_render_us += row.owner_render_us;
                state.totals.png_encode_us += row.png_encode_us;
                state.totals.commit_write_us += row.commit_write_us;
            }
        }
    }

    pub fn record_finalize(&self, micros: u64) {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if state.enabled {
            state.totals.finalize_us = micros;
        }
    }

    /// The frozen snapshot (rows sorted by page, then generation).
    #[must_use]
    pub fn snapshot(&self) -> CommitProfile {
        let state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return CommitProfile::default(),
        };
        let mut profile = state.totals.clone();
        profile.pages = state.rows.values().copied().collect();
        profile
    }

    fn edit_row(&self, page: u32, generation: u64, edit: impl FnOnce(&mut PageCommitProfile)) {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if !state.enabled {
            return;
        }
        if let Some(row) = state.rows.get_mut(&(page, generation)) {
            edit(row);
        }
    }

    fn edit_current(&self, page: u32, edit: impl FnOnce(&mut PageCommitProfile)) {
        let generation = match self.inner.lock() {
            Ok(state) => state.generations.get(&page).copied().unwrap_or(0),
            Err(_) => return,
        };
        self.edit_row(page, generation, edit);
    }
}

#[cfg(test)]
mod tests {
    use super::{CommitProfile, CommitRecorder, PageCommitProfile};

    #[test]
    fn commit_profile_json_schema_is_frozen() {
        // T1: the debug channel's field names are a contract with the
        // phase-3 diagnostic tooling. This golden string locks them; new
        // fields may append, renames or removals are breaking changes.
        let profile = CommitProfile {
            pages: vec![
                PageCommitProfile {
                    page: 1,
                    generation: 1,
                    render_wait_us: 120_000,
                    owner_render_us: 100_000,
                    png_encode_us: 45_000,
                    commit_write_us: 12_000,
                },
                PageCommitProfile {
                    page: 2,
                    generation: 3,
                    render_wait_us: 80_000,
                    owner_render_us: 60_000,
                    png_encode_us: 30_000,
                    commit_write_us: 9_000,
                },
            ],
            pages_total: 1,
            render_wait_us: 80_000,
            owner_render_us: 60_000,
            png_encode_us: 30_000,
            commit_write_us: 9_000,
            finalize_us: 5_000,
        };
        assert_eq!(
            profile.to_json(),
            "{\"schema_version\":1,\"pages\":[{\"page\":1,\"generation\":1,\"render_wait_us\":120000,\"owner_render_us\":100000,\"png_encode_us\":45000,\"commit_write_us\":12000},{\"page\":2,\"generation\":3,\"render_wait_us\":80000,\"owner_render_us\":60000,\"png_encode_us\":30000,\"commit_write_us\":9000}],\"totals\":{\"pages\":1,\"render_wait_us\":80000,\"owner_render_us\":60000,\"png_encode_us\":30000,\"commit_write_us\":9000},\"finalize_us\":5000}"
        );
        assert_eq!(
            CommitProfile::default().to_json(),
            "{\"schema_version\":1,\"pages\":[],\"totals\":{\"pages\":0,\"render_wait_us\":0,\"owner_render_us\":0,\"png_encode_us\":0,\"commit_write_us\":0},\"finalize_us\":0}"
        );
    }

    #[test]
    fn recorder_keys_rows_by_generation_and_totals_only_terminal() {
        let recorder = CommitRecorder::with_enabled(true);
        // Page 1 renders twice (degenerate retry): two rows, two generations.
        let first = recorder.begin_render(1);
        recorder.record_render_wait(1, first, 100);
        recorder.record_png_encode(1, 10);
        let second = recorder.begin_render(1);
        assert_eq!(second, 2, "render attempt identity is 1-based monotonic");
        recorder.record_render_wait(1, second, 150);
        recorder.record_png_encode(1, 20);
        recorder.record_commit_write(1, 5);
        recorder.mark_terminal(1);
        // Page 2 renders once and fails: a row exists but never joins totals.
        let only = recorder.begin_render(2);
        recorder.record_render_wait(2, only, 70);
        recorder.record_finalize(500);
        let profile = recorder.snapshot();
        assert_eq!(profile.pages.len(), 3);
        assert_eq!(profile.pages[0].page, 1);
        assert_eq!(profile.pages[0].generation, 1);
        assert_eq!(profile.pages[0].render_wait_us, 100);
        assert_eq!(
            profile.pages[0].png_encode_us, 10,
            "encode lands on its own row"
        );
        assert_eq!(profile.pages[1].generation, 2);
        assert_eq!(profile.pages[1].render_wait_us, 150);
        assert_eq!(profile.pages[1].commit_write_us, 5);
        assert_eq!(profile.pages[2].page, 2);
        assert_eq!(profile.pages[2].render_wait_us, 70);
        // Totals only count the terminal page-1 generation.
        assert_eq!(profile.pages_total, 1);
        assert_eq!(profile.render_wait_us, 150);
        assert_eq!(profile.png_encode_us, 20);
        assert_eq!(profile.commit_write_us, 5);
        assert_eq!(profile.finalize_us, 500);
    }

    #[test]
    fn disabled_recorder_records_nothing() {
        let recorder = CommitRecorder::with_enabled(false);
        let generation = recorder.begin_render(1);
        assert_eq!(generation, 0);
        recorder.record_render_wait(1, generation, 100);
        recorder.record_png_encode(1, 10);
        recorder.record_commit_write(1, 5);
        recorder.mark_terminal(1);
        recorder.record_finalize(500);
        assert_eq!(recorder.snapshot(), CommitProfile::default());
        assert!(!recorder.enabled());
    }

    #[test]
    fn clones_share_one_state() {
        let recorder = CommitRecorder::with_enabled(true);
        let sink = recorder.clone();
        let generation = recorder.begin_render(1);
        sink.record_render_wait(1, generation, 42);
        recorder.record_png_encode(1, 7);
        let profile = sink.snapshot();
        assert_eq!(profile.pages.len(), 1);
        assert_eq!(profile.pages[0].render_wait_us, 42);
        assert_eq!(profile.pages[0].png_encode_us, 7);
    }
}
