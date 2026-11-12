//! PdfiumOwner (migrated from the CLI, plan §8.10/T058): every in-process
//! PDFium call of a scan runs on exactly one
//! dedicated owner thread (plan §6.1); the main thread requests via channels.
//!
//! Invariant (review MAJOR-2/BLK-2, plan v3.10): the main thread never touches
//! pdfium-render types. The [`PdfiumExecutor`] is constructed inside the owner
//! thread; the [`OwnerPageRenderer`] wrapper only holds channel handles, so
//! the invariant holds by construction. An owner hang/timeout/disconnect is
//! fail-loud: the scan aborts with a structured error and keeps completed
//! checkpoints; there is no runtime fallback to main-thread rendering (serial
//! rendering only exists as a code revert).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::state::{RenderAudit, RenderedPage};
use image::RgbImage;
use ocrx_core::{PageClassification, QualityTier};
use ocrx_pdf::{PdfiumRenderer, RenderOptions, fit_pixel_budget};

/// Prefetch depth floor (plan §6.2 froze K=2 for the serial-era pipeline).
/// Phase-4 W2 unfreezes it: the owner prefetches a FULL window ahead (the
/// VRAM-tiered window depth), hiding the next window's render burst inside
/// the current window's decode. The floor keeps tiny-window calls honest.
const PREFETCH_DEPTH_FLOOR: u32 = 2;
/// Synchronous request timeout: a real PDFium page op is well under a second;
/// this only has to catch a deadlocked native call, so it stays generous.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Raw render result handed back by the executor before model-input scaling.
struct RenderInput {
    image: RgbImage,
    original_width: u32,
    original_height: u32,
}

/// The PDFium operations a scan needs, executed on the owner thread only.
///
/// The real implementation wraps [`PdfiumRenderer`]; tests inject fakes to
/// drive the protocol deterministically.
trait PdfiumExecutor: Send + 'static {
    fn page_count(&mut self) -> Result<usize, String>;
    /// Classifies the given 1-based pages in order; the first failure fails
    /// the whole analysis (same as the serial `analyze_selected_pages`).
    fn analyze(
        &mut self,
        pages: &[usize],
        tier: QualityTier,
    ) -> Result<Vec<PageClassification>, String>;
    /// Renders one page (1-based) at the configured options, no scaling; the
    /// perturbation ratio (when any) is applied by [`compose_rendered_page`]
    /// on the owner thread (identical bytes to the serial resample path).
    fn render_raw(&mut self, page: u32) -> Result<RenderInput, String>;
    /// Extracts the native text layer of one page (1-based index).
    fn extract_native(&mut self, page_index: usize) -> Result<ocrx_pdf::NativePage, String>;
    /// Switches the active input document (X1 cross-document protocol,
    /// BLK-2 freeze §3-2). Always called at a window-scheduling boundary
    /// with no render in flight; the owner clears its prefetch state before
    /// forwarding. The returned error is a document-level VALUE (unreadable
    /// PDF and friends — the caller skips the document); owner
    /// timeout/disconnect stays the fail-loud poison path, never this.
    fn switch_document(&mut self, input: &str) -> Result<(), String>;
}

impl PdfiumExecutor for Box<dyn PdfiumExecutor> {
    fn page_count(&mut self) -> Result<usize, String> {
        (**self).page_count()
    }

    fn analyze(
        &mut self,
        pages: &[usize],
        tier: QualityTier,
    ) -> Result<Vec<PageClassification>, String> {
        (**self).analyze(pages, tier)
    }

    fn render_raw(&mut self, page: u32) -> Result<RenderInput, String> {
        (**self).render_raw(page)
    }

    fn extract_native(&mut self, page_index: usize) -> Result<ocrx_pdf::NativePage, String> {
        (**self).extract_native(page_index)
    }

    fn switch_document(&mut self, input: &str) -> Result<(), String> {
        (**self).switch_document(input)
    }
}

enum PdfRequest {
    PageCount,
    ExtractNative {
        /// 0-based page index.
        page_index: usize,
    },
    Analyze {
        pages: Vec<usize>,
        tier: QualityTier,
    },
    Render {
        page: u32,
    },
    RenderPerturbed {
        page: u32,
        numerator: u32,
        denominator: u32,
    },
    /// X1 (BLK-2 §3-2): switch the active document. Sent only at window
    /// scheduling boundaries between documents; the owner clears the
    /// prefetch cache and resets `next_prefetch` before forwarding, so the
    /// next document can never hit the previous document's cached pages.
    SwitchDocument {
        input: PathBuf,
    },
    /// Phase-4 BLK-2: retier the rolling prefetch lookahead after a
    /// GPU->CPU fallback (the 16 tier is a GPU-VRAM decision only).
    SetPrefetchDepth {
        depth: u32,
    },
}

enum PdfResponse {
    PageCount(Result<usize, String>),
    NativeExtracted(Result<ocrx_pdf::NativePage, String>),
    Analyze(Result<Vec<PageClassification>, String>),
    /// T1: the u64 is the owner render work time in microseconds for this
    /// page (demand render or the prefetch that produced a cache hit;
    /// diagnostic only — the caller wait already covers it).
    Render(Result<(RenderedPage, u64), String>),
    /// Document-level switch outcome as a value (§3-4); a `Err` here means
    /// "skip this document", while owner poison travels through the handle.
    Switched(Result<(), String>),
}

struct OwnerState<E: PdfiumExecutor> {
    executor: E,
    /// Prefetched results keyed by page number, consumed by page order.
    cache: BTreeMap<u32, Result<RenderedPage, String>>,
    /// Next page the prefetch window has not covered yet.
    next_prefetch: u32,
    /// T1: measured render work time (µs) of each prefetched page, carried
    /// to the response when the cached entry is consumed.
    prefetch_us: BTreeMap<u32, u64>,
    /// Phase-4 W2: prefetch lookahead in pages (the VRAM-tiered window
    /// depth). K=2 was the serial-era floor; a full-window lookahead hides
    /// the next window's render burst inside the current window's decode.
    prefetch_depth: u32,
}

impl<E: PdfiumExecutor> OwnerState<E> {
    /// Answers `page` (from cache or direct). Prefetch is a separate step —
    /// review finding 3: topping the window up before answering delayed the
    /// current page's response behind N+1/N+2's renders.
    fn render_page(&mut self, page: u32) -> Result<(RenderedPage, u64), String> {
        match self.cache.remove(&page) {
            // T1: a cache hit carries the prefetch's measured work time.
            Some(Ok(cached)) => Ok((cached, self.prefetch_us.remove(&page).unwrap_or(0))),
            Some(Err(message)) => Err(message),
            None => {
                let started = std::time::Instant::now();
                let rendered = self.render_direct(page, 1, 1)?;
                let owner_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
                Ok((rendered, owner_us))
            }
        }
    }

    /// Tops the prefetch window up to `page + PREFETCH_DEPTH` inclusive
    /// (plan §6.2), called after the current response was already sent: the
    /// owner thread keeps rendering N+1/N+2 while the main thread runs the
    /// page's recognition/inference. Prefetch failures are cached as values
    /// so one bad page never poisons the window.
    fn top_up_prefetch(&mut self, page: u32) {
        self.next_prefetch = self.next_prefetch.max(page.saturating_add(1));
        let top_exclusive = page
            .saturating_add(self.prefetch_depth.max(PREFETCH_DEPTH_FLOOR))
            .saturating_add(1);
        while self.next_prefetch < top_exclusive {
            let prefetch_page = self.next_prefetch;
            self.next_prefetch += 1;
            let started = std::time::Instant::now();
            let rendered = self.render_direct(prefetch_page, 1, 1);
            if rendered.is_ok() {
                // T1: failed prefetches carry no usable timing.
                self.prefetch_us.insert(
                    prefetch_page,
                    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                );
            }
            self.cache.insert(prefetch_page, rendered);
        }
    }

    fn render_direct(
        &mut self,
        page: u32,
        numerator: u32,
        denominator: u32,
    ) -> Result<RenderedPage, String> {
        let input = self.executor.render_raw(page)?;
        compose_rendered_page(input, numerator, denominator).map_err(|e| e.to_string())
    }
}

/// Turns a raw render into the model input, exactly like the serial
/// `PdfScanRenderer::render_with_scale`: optional perturbation resample,
/// pixel-budget fit, faithful audit. Runs on the owner thread so the overlap
/// window covers "render + scale" (plan §6.2) and the bytes stay identical to
/// the serial path.
fn compose_rendered_page(
    input: RenderInput,
    scale_numerator: u32,
    scale_denominator: u32,
) -> std::io::Result<RenderedPage> {
    const PIXEL_BUDGET: u64 = 2880 * 2880;
    let rendered_width = input.image.width();
    let rendered_height = input.image.height();
    let image = if scale_numerator == scale_denominator {
        input.image
    } else {
        let scaled_width = u32::try_from(
            (u64::from(rendered_width) * u64::from(scale_numerator)) / u64::from(scale_denominator),
        )
        .map_err(|_| std::io::Error::other("scaled width out of range"))?
        .max(1);
        let scaled_height = u32::try_from(
            (u64::from(rendered_height) * u64::from(scale_numerator))
                / u64::from(scale_denominator),
        )
        .map_err(|_| std::io::Error::other("scaled height out of range"))?
        .max(1);
        image::imageops::resize(
            &input.image,
            scaled_width,
            scaled_height,
            image::imageops::FilterType::Triangle,
        )
    };
    let (model_width, model_height, scaled) =
        fit_pixel_budget(image.width(), image.height(), PIXEL_BUDGET);
    let scaled_input_width = image.width();
    let rgb = if scaled {
        image::imageops::resize(
            &image,
            model_width,
            model_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let (scale_ratio_numerator, scale_ratio_denominator) =
        crate::render::linear_scale_ratio(model_width, scaled_input_width);
    RenderedPage::new(rgb.width(), rgb.height(), rgb.into_raw()).map(|page| {
        page.with_audit(RenderAudit {
            original_width: input.original_width,
            original_height: input.original_height,
            rendered_width,
            rendered_height,
            model_width,
            model_height,
            scaled,
            scale_ratio_numerator,
            scale_ratio_denominator,
        })
    })
}

/// Main-thread handle: channel endpoints plus the poisoned flag. Dropping the
/// handle closes the request channel and the owner exits after its current
/// operation (plan §6.2 cancellation contract).
pub struct PdfiumOwnerHandle {
    requests: Option<std::sync::mpsc::Sender<PdfRequest>>,
    responses: std::sync::mpsc::Receiver<PdfResponse>,
    timeout: Duration,
    poison: Option<String>,
    /// T1: owner render work time (µs) stashed by the latest `render_via`
    /// (main-thread side; `Cell` is fine — the handle is `Rc`-held).
    last_render_us: std::cell::Cell<u64>,
}

impl PdfiumOwnerHandle {
    /// Spawns the owner thread with the real PDFium executor. The renderer is
    /// constructed inside the thread: the binding init happens there and the
    /// main thread never sees a pdfium type (plan §6.1).
    pub fn spawn(
        library_dir: PathBuf,
        input: PathBuf,
        options: RenderOptions,
        password: Option<String>,
        prefetch_depth: u32,
    ) -> Self {
        Self::spawn_with_timeout(
            REQUEST_TIMEOUT,
            prefetch_depth,
            Box::new(move || {
                let mut renderer = PdfiumRenderer::new(library_dir);
                if let Some(password) = password.as_deref() {
                    renderer = renderer.with_password(password);
                }
                Box::new(RealPdfiumExecutor {
                    renderer,
                    input,
                    options,
                }) as Box<dyn PdfiumExecutor>
            }),
        )
    }

    fn spawn_with_timeout(
        timeout: Duration,
        prefetch_depth: u32,
        make_executor: Box<dyn FnOnce() -> Box<dyn PdfiumExecutor> + Send>,
    ) -> Self {
        let (requests_tx, requests_rx) = std::sync::mpsc::channel::<PdfRequest>();
        let (responses_tx, responses_rx) = std::sync::mpsc::channel::<PdfResponse>();
        std::thread::Builder::new()
            .name("pdfium-owner".to_owned())
            .spawn(move || {
                let executor = make_executor();
                let mut state = OwnerState {
                    executor,
                    cache: BTreeMap::new(),
                    next_prefetch: 1,
                    prefetch_us: BTreeMap::new(),
                    prefetch_depth,
                };
                for request in requests_rx {
                    let mut pending_prefetch: Option<u32> = None;
                    let response = match request {
                        PdfRequest::PageCount => {
                            PdfResponse::PageCount(state.executor.page_count())
                        }
                        PdfRequest::ExtractNative { page_index } => {
                            PdfResponse::NativeExtracted(state.executor.extract_native(page_index))
                        }
                        PdfRequest::Analyze { pages, tier } => {
                            PdfResponse::Analyze(state.executor.analyze(&pages, tier))
                        }
                        PdfRequest::Render { page } => {
                            let rendered = state.render_page(page);
                            // Answer first (review round 2): the prefetch is
                            // marked here but only runs after the response
                            // reaches the main thread — the owner thread then
                            // renders N+1/N+2 while the main thread runs this
                            // page's recognition. Failures are values — the
                            // window still tops up past them.
                            pending_prefetch = Some(page);
                            PdfResponse::Render(rendered)
                        }
                        PdfRequest::RenderPerturbed {
                            page,
                            numerator,
                            denominator,
                        } => {
                            // Perturbation renders on demand, never through the
                            // prefetch window (plan §6.2); a non-identity ratio
                            // resamples the raw render like the serial path.
                            let started = std::time::Instant::now();
                            let rendered = state.render_direct(page, numerator, denominator);
                            let owner_us =
                                u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
                            PdfResponse::Render(rendered.map(|page| (page, owner_us)))
                        }
                        PdfRequest::SwitchDocument { input } => {
                            // BLK-2 §3-3: switching is only legal at a
                            // scheduling boundary (the orchestrator
                            // serializes), so the prefetch window of the
                            // previous document is discarded wholesale — the
                            // next document starts with `next_prefetch = 1`
                            // and an empty cache.
                            state.cache.clear();
                            state.next_prefetch = 1;
                            PdfResponse::Switched(
                                state.executor.switch_document(&input.to_string_lossy()),
                            )
                        }
                        PdfRequest::SetPrefetchDepth { depth } => {
                            // BLK-2: a GPU->CPU fallback drops the 16 tier to
                            // the floor; the rolling lookahead follows.
                            state.prefetch_depth = depth.max(PREFETCH_DEPTH_FLOOR);
                            PdfResponse::Switched(Ok(()))
                        }
                    };
                    if responses_tx.send(response).is_err() {
                        break; // handle dropped: exit (plan §6.2 Drop contract)
                    }
                    if let Some(page) = pending_prefetch.take() {
                        state.top_up_prefetch(page);
                    }
                }
            })
            .expect("spawn pdfium owner thread");
        Self {
            requests: Some(requests_tx),
            responses: responses_rx,
            timeout,
            poison: None,
            last_render_us: std::cell::Cell::new(0),
        }
    }

    fn request(&mut self, request: PdfRequest) -> Result<PdfResponse, String> {
        if let Some(poison) = &self.poison {
            return Err(poison.clone());
        }
        let Some(requests) = &self.requests else {
            return Err(self.poison_message());
        };
        if requests.send(request).is_err() {
            self.poison = Some("pdfium owner terminated".to_owned());
            return Err(self.poison_message());
        }
        match self.responses.recv_timeout(self.timeout) {
            Ok(response) => Ok(response),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Fail-loud (plan §6.2 BLK-2): a timed-out wait proves nothing
                // about the owner's native call; the main thread must never
                // render concurrently with a possibly-live owner.
                self.poison = Some("pdfium owner request timed out".to_owned());
                Err(self.poison_message())
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.poison = Some("pdfium owner terminated unexpectedly".to_owned());
                Err(self.poison_message())
            }
        }
    }

    fn poison_message(&self) -> String {
        self.poison
            .clone()
            .unwrap_or_else(|| "pdfium owner terminated".to_owned())
    }

    /// The structured failure when the owner is poisoned, if it is.
    pub(crate) fn poisoned(&self) -> Option<String> {
        self.poison.clone()
    }

    /// Phase-4 BLK-2: retiers the rolling prefetch lookahead after a
    /// GPU->CPU fallback (the 16 tier is a GPU-VRAM decision only).
    pub(crate) fn set_prefetch_depth(&mut self, depth: u32) {
        let _ = self.request(PdfRequest::SetPrefetchDepth { depth });
    }

    /// Switches the active document (X1, BLK-2 §3-2/3-4). `Ok(())` means the
    /// owner now serves `input`; an `Err` VALUE means the document is
    /// unusable (skip it, keep going). A poisoned owner fails loud through
    /// the same `Err` — callers disambiguate with [`Self::poisoned`].
    pub(crate) fn switch_document(&mut self, input: &Path) -> Result<(), String> {
        match self.request(PdfRequest::SwitchDocument {
            input: input.to_path_buf(),
        }) {
            Ok(PdfResponse::Switched(result)) => result,
            Ok(_) => unreachable!("switch request answers a switch"),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn page_count(&mut self) -> std::io::Result<usize> {
        match self.request(PdfRequest::PageCount) {
            Ok(PdfResponse::PageCount(value)) => value.map_err(std::io::Error::other),
            Ok(_) => unreachable!("page count request answers page count"),
            Err(error) => Err(std::io::Error::other(error)),
        }
    }

    pub(crate) fn analyze(
        &mut self,
        pages: &[usize],
        tier: QualityTier,
    ) -> Result<Vec<PageClassification>, String> {
        match self.request(PdfRequest::Analyze {
            pages: pages.to_vec(),
            tier,
        }) {
            Ok(PdfResponse::Analyze(value)) => value,
            Ok(_) => unreachable!("analyze request answers classifications"),
            Err(error) => Err(error),
        }
    }

    /// Native text extraction through the owner (review finding 2): the
    /// main thread never touches the process-global pdfium binding.
    pub(crate) fn extract_native(&mut self, page: u32) -> std::io::Result<ocrx_pdf::NativePage> {
        let page_index = usize::try_from(
            page.checked_sub(1)
                .ok_or_else(|| std::io::Error::other("page number must be positive"))?,
        )
        .map_err(std::io::Error::other)?;
        match self.request(PdfRequest::ExtractNative { page_index }) {
            Ok(PdfResponse::NativeExtracted(value)) => value.map_err(std::io::Error::other),
            Ok(_) => unreachable!("extract request answers a native page"),
            Err(error) => Err(std::io::Error::other(error)),
        }
    }

    fn render_via(&mut self, request: PdfRequest) -> std::io::Result<RenderedPage> {
        match self.request(request) {
            Ok(PdfResponse::Render(value)) => {
                let (page, owner_us) = value.map_err(std::io::Error::other)?;
                self.last_render_us.set(owner_us);
                Ok(page)
            }
            Ok(_) => unreachable!("render request answers a page"),
            Err(error) => Err(std::io::Error::other(error)),
        }
    }

    /// T1: the owner render work time (µs) of the last completed render
    /// response; `take` semantics so one page's value never leaks to the
    /// next. 0 after a failed render.
    pub(crate) fn take_last_render_us(&self) -> u64 {
        let us = self.last_render_us.get();
        self.last_render_us.set(0);
        us
    }
}

/// Production [`crate::app::PdfDocumentAccess`] over the owner handle:
/// every PDF fact (page count, classification signals, native extraction)
/// crosses the owner channel; the main thread holds no pdfium type.
pub struct OwnerPdfAccess {
    handle: Rc<RefCell<PdfiumOwnerHandle>>,
    /// The document the owner currently serves; a differing `input` in any
    /// port call triggers the X1 `SwitchDocument` swap (same owner thread).
    current_input: PathBuf,
}

impl OwnerPdfAccess {
    /// Spawns the owner thread for one document.
    #[must_use]
    pub fn spawn(
        library_dir: PathBuf,
        input: PathBuf,
        options: RenderOptions,
        password: Option<String>,
        prefetch_depth: u32,
    ) -> Self {
        Self {
            handle: Rc::new(RefCell::new(PdfiumOwnerHandle::spawn(
                library_dir, input.clone(), options, password, prefetch_depth,
            ))),
            current_input: input,
        }
    }

    /// Builds the window-loop renderer over the SAME owner (shared handle).
    #[must_use]
    pub fn renderer(
        &self,
        commit_recorder: ocrx_output::commit_profile::CommitRecorder,
    ) -> OwnerPageRenderer {
        OwnerPageRenderer::new(Rc::clone(&self.handle), commit_recorder)
    }

    /// X1 SwitchDocument: the SAME owner thread swaps to the new document
    /// (prefetch cache cleared, `next_prefetch` reset; a dead/unresponsive
    /// owner surfaces as an error on the follow-up calls).
    ///
    /// # Errors
    ///
    /// Channel/owner failure.
    pub fn switch_document(&mut self, input: &Path) -> std::io::Result<()> {
        self.handle
            .borrow_mut()
            .switch_document(input)
            .map_err(std::io::Error::other)?;
        self.current_input = input.to_path_buf();
        Ok(())
    }

    /// Ensures the owner serves `input` (no-op when it already does).
    fn ensure_document(&mut self, input: &Path) -> std::io::Result<()> {
        if self.current_input != input {
            self.switch_document(input)?;
        }
        Ok(())
    }
}

impl crate::app::PdfDocumentAccess for OwnerPdfAccess {
    fn page_count(&mut self, input: &Path) -> std::io::Result<usize> {
        self.ensure_document(input)?;
        self.handle.borrow_mut().page_count()
    }

    fn analyze(
        &mut self,
        input: &Path,
        pages: &[usize],
        tier: QualityTier,
    ) -> std::io::Result<Vec<PageClassification>> {
        self.ensure_document(input)?;
        self.handle
            .borrow_mut()
            .analyze(pages, tier)
            .map_err(std::io::Error::other)
    }

    fn extract_native(&mut self, page: u32) -> std::io::Result<ocrx_pdf::NativePage> {
        self.handle.borrow_mut().extract_native(page)
    }
}

impl Drop for PdfiumOwnerHandle {
    fn drop(&mut self) {
        self.requests.take(); // close the channel; the owner exits
    }
}

/// Real executor: owns the [`PdfiumRenderer`] on the owner thread only.
struct RealPdfiumExecutor {
    renderer: PdfiumRenderer,
    input: PathBuf,
    options: RenderOptions,
}

impl PdfiumExecutor for RealPdfiumExecutor {
    fn page_count(&mut self) -> Result<usize, String> {
        self.renderer
            .page_count(&self.input)
            .map_err(|error| error.to_string())
    }

    fn switch_document(&mut self, input: &str) -> Result<(), String> {
        // The swap itself is infallible; an unusable document surfaces as an
        // Err VALUE from the page_count/analyze/render calls that follow
        // (BLK-2 §3-4), which the orchestrator maps to a per-document skip.
        self.input = PathBuf::from(input);
        Ok(())
    }

    fn analyze(
        &mut self,
        pages: &[usize],
        tier: QualityTier,
    ) -> Result<Vec<PageClassification>, String> {
        pages
            .iter()
            .map(|page| {
                self.renderer
                    .analyze_page(&self.input, page.saturating_sub(1), tier)
                    .map_err(|error| format!("{error:?}"))
            })
            .collect()
    }

    fn extract_native(&mut self, page_index: usize) -> Result<ocrx_pdf::NativePage, String> {
        self.renderer
            .extract_native_page(&self.input, page_index)
            .map_err(|error| error.to_string())
    }

    fn render_raw(&mut self, page: u32) -> Result<RenderInput, String> {
        let page_index = usize::try_from(
            page.checked_sub(1)
                .ok_or_else(|| "page number must be positive".to_owned())?,
        )
        .map_err(|_| "page index out of range".to_owned())?;
        let (image, original_width, original_height) = self
            .renderer
            .render_page_rgb8_with_audit(&self.input, page_index, self.options)
            .map_err(|error| error.to_string())?;
        Ok(RenderInput {
            image,
            original_width,
            original_height,
        })
    }
}

/// `PageRenderer` wrapper used by the scan service: forwards every render to
/// the owner and carries the render-failure injection seam. Holds no pdfium
/// type — the §6.1 invariant by construction.
pub struct OwnerPageRenderer {
    handle: Rc<RefCell<PdfiumOwnerHandle>>,
    /// T1: records the owner render work time each response carried; a
    /// clone of the scan's shared commit-path recorder.
    commit_recorder: ocrx_output::commit_profile::CommitRecorder,
}

impl OwnerPageRenderer {
    #[must_use]
    pub fn new(
        handle: Rc<RefCell<PdfiumOwnerHandle>>,
        commit_recorder: ocrx_output::commit_profile::CommitRecorder,
    ) -> Self {
        Self {
            handle,
            commit_recorder,
        }
    }
}

impl crate::window::PageRenderer for OwnerPageRenderer {
    fn preflight(&mut self) -> std::io::Result<()> {
        // Page count and analysis already ran through the owner in run_scan,
        // which validated the input; channel health is all that is left.
        if self.handle.borrow().poisoned().is_some() {
            return Err(std::io::Error::other("pdfium owner is poisoned"));
        }
        Ok(())
    }

    fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
        crate::render::inject_render_failure(page)?;
        let rendered = self
            .handle
            .borrow_mut()
            .render_via(PdfRequest::Render { page });
        // T1: owner work time from the response (0 after a failure;
        // diagnostic only — the caller wait already covers it).
        let owner_us = self.handle.borrow().take_last_render_us();
        self.commit_recorder.record_owner_render(page, owner_us);
        rendered
    }

    fn render_perturbed(
        &mut self,
        page: u32,
        numerator: u32,
        denominator: u32,
    ) -> std::io::Result<RenderedPage> {
        crate::render::inject_render_failure(page)?;
        let rendered = self
            .handle
            .borrow_mut()
            .render_via(PdfRequest::RenderPerturbed {
                page,
                numerator,
                denominator,
            });
        let owner_us = self.handle.borrow().take_last_render_us();
        self.commit_recorder.record_owner_render(page, owner_us);
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::PageRenderer as _;
    use ocrx_core::{PageType, PdfReasonCode, ProcessingMethod, RouteConfidence};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Deterministic fake: 6 pages, page 2 render fails as a value, optional
    /// render delay and panic injection, call counter for prefetch analysis.
    /// X1: document-aware — renders are tagged with the active document's
    /// prefix so cross-document cache bleed is observable, and switches can
    /// fail per document as values.
    struct FakeExecutor {
        calls: Arc<AtomicUsize>,
        panic_on_render: bool,
        render_delay: Duration,
        /// Marker byte prefix of the active document (doc a -> 1, b -> 2...).
        doc_prefix: u8,
        /// Input substrings whose `switch_document` fails as a value.
        switch_fails_for: Vec<String>,
        /// Owner thread id observed at every switch (X1.4 same-owner assert).
        switch_threads: Arc<std::sync::Mutex<Vec<std::thread::ThreadId>>>,
    }

    impl FakeExecutor {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    calls: Arc::clone(&calls),
                    panic_on_render: false,
                    render_delay: Duration::ZERO,
                    doc_prefix: 1,
                    switch_fails_for: Vec::new(),
                    switch_threads: Arc::new(std::sync::Mutex::new(Vec::new())),
                },
                calls,
            )
        }

        fn spawn(self) -> (PdfiumOwnerHandle, Arc<AtomicUsize>) {
            let calls = Arc::clone(&self.calls);
            let handle = PdfiumOwnerHandle::spawn_with_timeout(
                Duration::from_secs(5),
                2,
                Box::new(move || Box::new(self) as Box<dyn PdfiumExecutor>),
            );
            (handle, calls)
        }
    }

    fn fake_image(page: u32) -> RgbImage {
        RgbImage::from_fn(4, 4, |x, y| image::Rgb([page as u8, x as u8, y as u8]))
    }

    impl PdfiumExecutor for FakeExecutor {
        fn page_count(&mut self) -> Result<usize, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(6)
        }

        fn analyze(
            &mut self,
            pages: &[usize],
            _tier: QualityTier,
        ) -> Result<Vec<PageClassification>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(pages
                .iter()
                .map(|_| PageClassification {
                    page_type: PageType::Text,
                    confidence: RouteConfidence::High,
                    recommended_method: ProcessingMethod::NativeText,
                    reason: PdfReasonCode::NativeTextReliable,
                })
                .collect())
        }

        fn extract_native(&mut self, page_index: usize) -> Result<ocrx_pdf::NativePage, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(format!("native extraction not faked (page {page_index})"))
        }

        fn render_raw(&mut self, page: u32) -> Result<RenderInput, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.panic_on_render {
                // The panic must escape the closure: `std::process::abort`
                // would take the whole test process down with it.
                std::panic::panic_any("injected owner panic");
            }
            std::thread::sleep(self.render_delay);
            if page == 2 {
                return Err("render failed for page 2".to_owned());
            }
            // Tag the marker byte with the active document so a cache bleed
            // across a SwitchDocument is observable in the returned pixels.
            let tagged = u32::from(self.doc_prefix.min(25)) * 10 + page.min(9);
            Ok(RenderInput {
                image: fake_image(tagged),
                original_width: 100,
                original_height: 200,
            })
        }

        fn switch_document(&mut self, input: &str) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.switch_threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            if self.switch_fails_for.iter().any(|bad| input.contains(bad)) {
                return Err(format!("document-level failure for {input}"));
            }
            // "doc-a" style names map to consecutive prefixes; unknown names
            // keep the previous prefix (still a fresh, observable document).
            if let Some(position) = input.rfind("doc-") {
                let letter = input.as_bytes().get(position + 4).copied();
                if letter.is_some_and(|byte| byte.is_ascii_lowercase()) {
                    self.doc_prefix = byte_to_prefix(letter.unwrap());
                }
            }
            Ok(())
        }
    }

    fn byte_to_prefix(byte: u8) -> u8 {
        byte - b'a' + 1
    }

    fn page_marker(rendered: &RenderedPage) -> u32 {
        u32::from(rendered.rgb8[0]) // first byte carries the page marker
    }

    #[test]
    fn page_count_and_analyze_roundtrip_through_the_owner() {
        let (executor, calls) = FakeExecutor::new();
        let (mut handle, _calls) = executor.spawn();
        assert_eq!(handle.page_count().unwrap(), 6);
        let pages = handle.analyze(&[1, 2, 3], QualityTier::L2).unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn render_prefetches_two_pages_ahead_and_serves_from_cache() {
        let (executor, _calls) = FakeExecutor::new();
        let (mut handle, calls) = executor.spawn();
        let wait_calls = |calls: &Arc<AtomicUsize>, target: usize| {
            for _ in 0..200 {
                if calls.load(Ordering::SeqCst) >= target {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("prefetch never reached {target} calls");
        };
        let first = handle.render_via(PdfRequest::Render { page: 1 }).unwrap();
        assert_eq!(page_marker(&first), 11); // doc prefix 1, page 1
        // The response is sent BEFORE the prefetch (review finding 3); the
        // window then tops up asynchronously with pages 2 and 3 (page 2
        // fails as a cached value).
        wait_calls(&calls, 3);
        // Page 2 answered from the cached failure without a new call; the
        // window tops up with page 4.
        assert!(handle.render_via(PdfRequest::Render { page: 2 }).is_err());
        wait_calls(&calls, 4);
        // Page 3 served from cache; window tops up with page 5.
        let third = handle.render_via(PdfRequest::Render { page: 3 }).unwrap();
        assert_eq!(page_marker(&third), 13);
        wait_calls(&calls, 5);
        // Page 4 was prefetched during Render(2)'s top-up: answered without a
        // new call beyond the window's own top-up to page 6.
        let fourth = handle.render_via(PdfRequest::Render { page: 4 }).unwrap();
        assert_eq!(page_marker(&fourth), 14);
        wait_calls(&calls, 6); // only page 6 prefetched
    }

    #[test]
    fn render_error_is_a_value_and_the_channel_stays_open() {
        let (executor, _calls) = FakeExecutor::new();
        let (mut handle, _calls) = executor.spawn();
        let error = handle
            .render_via(PdfRequest::Render { page: 2 })
            .unwrap_err();
        assert!(error.to_string().contains("page 2"));
        // A later page still renders: failure continues (plan §6.2).
        assert!(handle.render_via(PdfRequest::Render { page: 3 }).is_ok());
        assert!(handle.poisoned().is_none());
    }

    #[test]
    fn perturbed_render_renders_on_demand_outside_the_window() {
        let (executor, calls) = FakeExecutor::new();
        let (mut handle, _calls) = executor.spawn();
        let page = handle
            .render_via(PdfRequest::RenderPerturbed {
                page: 1,
                numerator: 47,
                denominator: 50,
            })
            .unwrap();
        // Exactly one executor call: no prefetch on the perturbed path
        // (plan §6.2: retry pages are consumed out of the window anyway).
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // 4x4 raw image resampled by 47/50 -> 3x3 model input, audit records
        // the ratio faithfully (identity budget fit, scaled=false).
        assert_eq!(page.width, 3);
        assert_eq!(page.height, 3);
        assert_eq!(page.audit.rendered_width, 4);
        assert_eq!(page.audit.model_width, 3);
        assert_eq!(page.audit.scale_ratio_numerator, 1);
        assert_eq!(page.audit.scale_ratio_denominator, 1);
    }

    #[test]
    fn full_window_prefetch_hides_the_next_window_render() {
        // Phase-4 W2: with K = window depth, requesting page 1 prefetches the
        // whole remaining window (pages 2..=K+1); the subsequent requests are
        // pure cache hits. FakeExecutor's 6-page ceiling bounds the asserts.
        let (executor, calls) = FakeExecutor::new();
        let handle = PdfiumOwnerHandle::spawn_with_timeout(
            Duration::from_secs(5),
            6,
            Box::new(move || Box::new(executor) as Box<dyn PdfiumExecutor>),
        );
        let mut handle = handle;
        let first = handle.render_via(PdfRequest::Render { page: 1 }).unwrap();
        assert_eq!(page_marker(&first), 11);
        // Wait for the K=6 top-up: pages 2..=6 rendered on the owner thread.
        for _ in 0..200 {
            if calls.load(Ordering::SeqCst) >= 6 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            calls.load(Ordering::SeqCst) >= 6,
            "K=6 prefetch must render pages 2..=6 (observed calls={})",
            calls.load(Ordering::SeqCst)
        );
        // Pages 2..=6 were prefetched (calls >= 6 covers page 1 + 2..=6), so
        // consuming them must serve the prefetched values: page 2 is the
        // fake's injected failure carried as a value, 3..=6 carry the right
        // doc-tagged markers. The K-ahead window ROLLS (each answer tops up
        // K more pages beyond it), so background executor calls keep
        // growing for future pages - that is the design, not a leak.
        assert!(handle.render_via(PdfRequest::Render { page: 2 }).is_err());
        for page in 3..=6 {
            let rendered = handle.render_via(PdfRequest::Render { page }).unwrap();
            assert_eq!(page_marker(&rendered), 10 + page.min(9));
        }
        assert!(
            calls.load(Ordering::SeqCst) >= 6,
            "rolling prefetch must keep rendering ahead"
        );
        assert!(handle.poisoned().is_none());
    }

    #[test]
    fn request_timeout_poisons_the_owner_fail_loud() {
        let (mut executor, _calls) = FakeExecutor::new();
        executor.render_delay = Duration::from_millis(300);
        let mut handle = PdfiumOwnerHandle::spawn_with_timeout(
            Duration::from_millis(20),
            2,
            Box::new(move || Box::new(executor) as Box<dyn PdfiumExecutor>),
        );
        assert!(handle.render_via(PdfRequest::Render { page: 1 }).is_err());
        let poison = handle.poisoned().expect("timeout must poison");
        assert!(poison.contains("timed out"));
        // Every later request fails fast with the same structured error; the
        // main thread never renders itself (invariant: no PDFium type here).
        assert!(handle.page_count().is_err());
        assert!(handle.poisoned().is_some());
    }

    #[test]
    fn owner_panic_disconnects_and_fails_loud() {
        let (mut executor, _calls) = FakeExecutor::new();
        executor.panic_on_render = true;
        let mut handle = PdfiumOwnerHandle::spawn_with_timeout(
            Duration::from_secs(5),
            2,
            Box::new(move || Box::new(executor) as Box<dyn PdfiumExecutor>),
        );
        let error = handle
            .render_via(PdfRequest::Render { page: 1 })
            .unwrap_err();
        assert!(error.to_string().contains("terminated"));
        assert!(handle.poisoned().is_some());
    }

    #[test]
    fn owner_page_renderer_forwards_and_holds_no_executor() {
        let (executor, calls) = FakeExecutor::new();
        let (handle, _calls) = executor.spawn();
        let handle = Rc::new(RefCell::new(handle));
        let mut renderer = OwnerPageRenderer::new(
            Rc::clone(&handle),
            ocrx_output::commit_profile::CommitRecorder::disabled(),
        );
        assert!(renderer.preflight().is_ok());
        let page = renderer.render(1).unwrap();
        assert_eq!(page_marker(&page), 11); // doc prefix 1, page 1
        let page = renderer.render_perturbed(1, 47, 50).unwrap();
        assert_eq!(page.width, 3);
        assert!(calls.load(Ordering::SeqCst) >= 2);
        // Poisoning the owner surfaces through preflight (fail-loud, no
        // silent degradation).
        handle.borrow_mut().poison = Some("injected".to_owned());
        let error = renderer.preflight().unwrap_err();
        assert!(error.to_string().contains("poisoned"));
    }

    #[test]
    fn compose_rendered_page_matches_the_serial_audit_math() {
        // 8x2 raw render, ratio 47/50, pixel budget not hit.
        let image = RgbImage::from_fn(8, 2, |x, y| image::Rgb([x as u8, y as u8, 7]));
        let input = RenderInput {
            image,
            original_width: 100,
            original_height: 25,
        };
        let page = compose_rendered_page(input, 47, 50).unwrap();
        // 8*47/50 = 7 (floor), 2*47/50 = 1.
        assert_eq!(page.width, 7);
        assert_eq!(page.height, 1);
        assert_eq!(page.audit.original_width, 100);
        assert_eq!(page.audit.original_height, 25);
        assert_eq!(page.audit.rendered_width, 8);
        assert_eq!(page.audit.rendered_height, 2);
        assert_eq!(page.audit.model_width, 7);
        assert_eq!(page.audit.model_height, 1);
        assert!(!page.audit.scaled);
        assert_eq!(page.audit.scale_ratio_numerator, 1);
        assert_eq!(page.audit.scale_ratio_denominator, 1);
        assert_eq!(page.rgb8.len(), 7 * 3);
    }

    #[test]
    fn compose_rendered_page_identity_ratio_keeps_pixels() {
        let image = RgbImage::from_fn(3, 3, |x, y| image::Rgb([x as u8, y as u8, 9]));
        let input = RenderInput {
            image,
            original_width: 30,
            original_height: 30,
        };
        let page = compose_rendered_page(input, 1, 1).unwrap();
        assert_eq!(page.width, 3);
        assert_eq!(page.audit.rendered_width, 3);
        assert_eq!(page.rgb8[0], 0);
    }

    // --- X1 cross-document protocol (BLK-2 §3-2/3-3/3-4) --------------------

    fn switch_to(handle: &mut PdfiumOwnerHandle, doc: &str) -> Result<(), String> {
        handle.switch_document(std::path::Path::new(doc))
    }

    #[test]
    fn switch_document_clears_prefetch_and_serves_the_new_document() {
        // X1.4②: after the switch, page 1 of document B must be a FRESH
        // render tagged B — never a cache hit of document A's page 1 — and
        // B's prefetch window restarts from page 2.
        let (executor, calls) = FakeExecutor::new();
        let (mut handle, _calls) = executor.spawn();
        let wait_calls = |calls: &Arc<AtomicUsize>, target: usize| {
            for _ in 0..200 {
                if calls.load(Ordering::SeqCst) >= target {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("prefetch never reached {target} calls");
        };
        // Document A: page 1 answered, prefetch window runs to A pages 2-3.
        let a_page1 = handle.render_via(PdfRequest::Render { page: 1 }).unwrap();
        assert_eq!(page_marker(&a_page1), 11);
        wait_calls(&calls, 3);
        // Switch to document B: prefetch state of A must be gone.
        switch_to(&mut handle, "doc-b.pdf").unwrap();
        let b_page1 = handle.render_via(PdfRequest::Render { page: 1 }).unwrap();
        assert_eq!(
            page_marker(&b_page1),
            21,
            "document B page 1 must be a fresh B-tagged render, not A's cache"
        );
        // B's own prefetch window tops up (B page 2 fails as a cached value).
        wait_calls(&calls, 5);
        assert!(handle.render_via(PdfRequest::Render { page: 2 }).is_err());
        // Back to A: page 1 renders fresh again (A's old cache was dropped).
        switch_to(&mut handle, "doc-a.pdf").unwrap();
        let a_page1_again = handle.render_via(PdfRequest::Render { page: 1 }).unwrap();
        assert_eq!(page_marker(&a_page1_again), 11);
        assert!(handle.poisoned().is_none());
    }

    #[test]
    fn switch_document_error_is_a_value_and_the_channel_stays_open() {
        // X1.4③: an unusable document is an Err VALUE on the switch itself;
        // the owner stays healthy and later documents keep working.
        let (mut executor, _calls) = FakeExecutor::new();
        executor.switch_fails_for.push("doc-bad".to_owned());
        let (handle, _calls) = executor.spawn();
        let mut handle = handle;
        switch_to(&mut handle, "doc-a.pdf").unwrap();
        let error = switch_to(&mut handle, "doc-bad.pdf").unwrap_err();
        assert!(error.contains("document-level failure"), "{error}");
        assert!(
            handle.poisoned().is_none(),
            "a document value must not poison"
        );
        // The channel still serves renders and later switches.
        let page = handle.render_via(PdfRequest::Render { page: 1 }).unwrap();
        assert_eq!(page_marker(&page), 11);
        switch_to(&mut handle, "doc-a.pdf").unwrap();
        assert!(handle.poisoned().is_none());
    }

    #[test]
    fn every_switch_runs_on_the_same_single_owner_thread() {
        // X1.4①: all document switches across the invocation are served by
        // exactly one owner thread — no per-document spawn may ever happen.
        let (executor, _calls) = FakeExecutor::new();
        let threads = Arc::clone(&executor.switch_threads);
        let (handle, _calls) = executor.spawn();
        let mut handle = handle;
        for doc in [
            "doc-a.pdf",
            "doc-b.pdf",
            "doc-a.pdf",
            "doc-c.pdf",
            "doc-b.pdf",
        ] {
            switch_to(&mut handle, doc).unwrap();
        }
        let threads = threads.lock().unwrap();
        assert_eq!(threads.len(), 5);
        let first = threads[0];
        assert!(
            threads.iter().all(|&id| id == first),
            "switches observed on multiple owner threads: {threads:?}"
        );
    }
}
