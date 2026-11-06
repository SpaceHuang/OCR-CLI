//! Phase-4 W3: the PNG encode pipeline (phase-3 v1.1 frozen design).
//!
//! The full-page PNG depends only on the rendered pixels - never on the
//! recognition output - so it can be encoded WHILE the window decodes. The
//! pipeline owns the encoder, stamps nothing itself (the scan service
//! assigns generations), spawns one worker per enqueued page (bounded by
//! the window depth), and stores results keyed by render generation. The
//! commit path fetches by the terminal render's generation, so a degenerated
//! retry's bytes can never be the first attempt's.
//!
//! Error surface (frozen, three-way): an encode error is a page-level
//! `recognize` diagnostic (identical wording to the inline path); a worker
//! panic is fail-loud (re-raised on the scan thread at fetch time); a lost
//! result is an internal invariant violation (same).

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Condvar, Mutex};

use crate::RenderedPage;

/// Encodes one page's RGB8 pixels into the PNG bytes that `commit_page`
/// writes. Production: the `image` crate with the exact parameters of the
/// former inline encode (byte-identical output). Fakes: deterministic bytes.
pub trait PagePngEncoder: Send + Sync + 'static {
    /// `generation` lets fakes distinguish attempts (the commit path fetches
    /// by the terminal render's generation, so retry bytes must differ).
    fn encode_png(
        &self,
        page: u32,
        generation: u64,
        width: u32,
        height: u32,
        rgb8: &[u8],
    ) -> std::io::Result<Vec<u8>>;
}

/// A worker failure that is not an encode error: the encoder panicked.
/// Re-raised on the scan thread at fetch time (fail-loud, frozen surface).
struct Panicked(Box<dyn std::any::Any + Send>);

enum EncodeOutcome {
    Done(Result<Vec<u8>, std::io::Error>),
    Panicked(Panicked),
}

struct PipelineInner {
    results: Mutex<BTreeMap<u64, EncodeOutcome>>,
    done: Condvar,
}

/// Clone-safe handle: the scan service holds one, the window trigger uses
/// the clones it needs. Workers store into the shared map and notify.
#[derive(Clone)]
pub(crate) struct EncodePipeline {
    encoder: Arc<Box<dyn PagePngEncoder>>,
    inner: Arc<PipelineInner>,
    generations: Arc<AtomicU64>,
}

impl EncodePipeline {
    pub(crate) fn new(encoder: Box<dyn PagePngEncoder>) -> Self {
        Self {
            encoder: Arc::new(encoder),
            inner: Arc::new(PipelineInner {
                results: Mutex::new(BTreeMap::new()),
                done: Condvar::new(),
            }),
            generations: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Scan-local monotonic render generation.
    pub(crate) fn next_generation(&self) -> u64 {
        self.generations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    /// Spawns one worker for the page (bounded by the window depth: the
    /// trigger is per rendered page, at most one window in flight). A spawn
    /// failure is stored as an encode error - never a lost result.
    pub(crate) fn encode_one(&self, page: u32, image: &RenderedPage) {
        let generation = image.generation;
        let encoder = Arc::clone(&self.encoder);
        let inner = Arc::clone(&self.inner);
        let pixels = image.rgb8.clone();
        let width = image.width;
        let height = image.height;
        let worker = std::thread::Builder::new()
            .name(format!("png-encode-{page}"))
            .spawn(move || {
                let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    encoder.encode_png(page, generation, width, height, &pixels)
                })) {
                    Ok(result) => EncodeOutcome::Done(result),
                    Err(payload) => EncodeOutcome::Panicked(Panicked(payload)),
                };
                let mut results = inner.results.lock().unwrap();
                results.insert(generation, outcome);
                inner.done.notify_all();
            });
        if let Err(spawn_error) = worker {
            let mut results = self.inner.results.lock().unwrap();
            results.insert(
                generation,
                EncodeOutcome::Done(Err(std::io::Error::other(spawn_error))),
            );
            self.inner.done.notify_all();
        }
    }

    /// Fetches the encoded bytes for `generation`, blocking until the worker
    /// stores them (encodes are ~15 ms; the fetch happens after the window's
    /// decode, so the wait is bounded by the worker's own runtime).
    pub(crate) fn take_png(&self, generation: u64) -> std::io::Result<Vec<u8>> {
        let mut results = self.inner.results.lock().unwrap();
        loop {
            match results.remove(&generation) {
                Some(EncodeOutcome::Done(result)) => return result,
                Some(EncodeOutcome::Panicked(payload)) => {
                    // Frozen surface: encoder panic = fail-loud, re-raised.
                    std::panic::resume_unwind(payload.0);
                }
                None => {
                    results = self.inner.done.wait(results).unwrap();
                }
            }
        }
    }
}
