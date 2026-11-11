//! OCR render pipeline (plan §4.1 scan/render.rs, T058): the worker-segment
//! renderer with one-segment prefetch (P3), the shared `SegmentRenderer`
//! port, dimension/scale helpers, and the W3.3 fault-injection seams.
//! The in-process PdfiumOwner lives in [`pdfium_owner`].

pub mod pdfium_owner;

pub use pdfium_owner::OwnerPageRenderer;

use std::path::Path;
use ocrx_pdf::PdfWorkerScheduler;
use ocrx_primitives::CancellationToken;

/// Renders one 0-based half-open worker segment, honouring cancellation
/// (plan §6.3). [`PdfWorkerScheduler`] is the production implementation; tests
/// inject fakes that write segment files directly.
pub trait SegmentRenderer: Send + Sync + 'static {
    fn run_segment(
        &self,
        input: &Path,
        output: &Path,
        start: usize,
        end: usize,
        cancellation: &CancellationToken,
    ) -> Result<(), String>;
}

impl SegmentRenderer for PdfWorkerScheduler {
    fn run_segment(
        &self,
        input: &Path,
        output: &Path,
        start: usize,
        end: usize,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        self.run_with_cancel(input, output, start, end, || cancellation.is_cancelled())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub fn parse_dimensions(value: &str) -> std::io::Result<(u32, u32)> {
    let (width, height) = value.split_once('x').ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid dimensions")
    })?;
    Ok((
        width.parse().map_err(std::io::Error::other)?,
        height.parse().map_err(std::io::Error::other)?,
    ))
}

pub fn linear_scale_ratio(model_width: u32, rendered_width: u32) -> (u64, u64) {
    let numerator = u64::from(model_width);
    let denominator = u64::from(rendered_width);
    let divisor = gcd(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

pub fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

/// Parses the page number from a W3.3 test-only fault-injection env value.
///
/// The value is `None` when the env var is unset. Zero and non-numeric values are
/// rejected so a misconfigured test never injects a fault into page 0.
#[must_use]
pub fn parse_injected_failure_page(value: Option<&str>) -> Option<u32> {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|page| *page > 0)
}

/// Returns an error when the injected page matches, otherwise `Ok(())`.
///
/// This is the deterministic test seam for W3.3 real-model fault injection.
/// Production scans leave the env vars unset, so the seam is a no-op in normal use.
pub fn injected_failure(value: Option<&str>, page: u32, description: &str) -> std::io::Result<()> {
    if parse_injected_failure_page(value) == Some(page) {
        return Err(std::io::Error::other(description.to_owned()));
    }
    Ok(())
}

/// Injects a real render failure for `page` when `OCRX_E2E_INJECT_RENDER_FAIL_PAGE` matches.
pub fn inject_render_failure(page: u32) -> std::io::Result<()> {
    injected_failure(
        std::env::var("OCRX_E2E_INJECT_RENDER_FAIL_PAGE")
            .ok()
            .as_deref(),
        page,
        "injected render failure",
    )
}

/// Injects a real inference failure for `page` when `OCRX_E2E_INJECT_INFERENCE_FAIL_PAGE` matches.
pub fn inject_inference_failure(page: u32) -> std::io::Result<()> {
    injected_failure(
        std::env::var("OCRX_E2E_INJECT_INFERENCE_FAIL_PAGE")
            .ok()
            .as_deref(),
        page,
        "injected inference failure",
    )
}

