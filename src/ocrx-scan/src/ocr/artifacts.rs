//! Page artifact assembly over the inference API (plan §8.10, T062).
//!
//! The session-driven pipeline replaces the legacy runtime-consuming
//! writers of `ocrx-ovisocr2` (kept there until T087): a `TaskResponse`
//! carries the dual text (cleaned `text` + pre-strip `raw_text`), the think
//! count, and the decode echo; this module turns it into the page
//! artifacts — tail crop, bbox crops, markdown link validation — with the
//! page document built from the scan parser.
//!
//! Pages never cross this boundary: the caller keeps page numbers locally
//! and supplies them per call.

use std::path::Path;

use image::RgbImage;
use ocrx_inference_api::{
    FinishReason, GenerationParams, ImageInput, InferenceError, InferenceSession, RetrySampling,
    TaskRequest, TaskResponse,
};
use ocrx_primitives::{CancellationToken, GenerationProgress};

use super::markdown::{drop_invalid_markdown_image_links, replace_bbox_images};
use super::parser::{DegenerateTailCrop, build_page_document, degenerate_tail_crop};
use crate::state::{DegenerationRetryStep, PageArtifactResult, RenderedPage};
use crate::{ladder, ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS};

/// Why one recognition attempt failed (the window loop answers batch-level
/// failures with a depth-1 resubmission; everything else is per-page).
#[derive(Debug)]
pub enum RecognizeError {
    /// Batch-level window failure (decode/OOM/context overflow): the whole
    /// window reruns single-stream.
    WindowFailed(String),
    /// Per-page failure (includes cancellation via the `cancelled` flag
    /// carried in the message, mirroring the legacy handling).
    Failed(std::io::Error),
}

impl RecognizeError {
    #[must_use]
    pub fn cancelled() -> Self {
        Self::Failed(std::io::Error::other("cancelled"))
    }

    #[must_use]
    pub fn failed(error: std::io::Error) -> Self {
        Self::Failed(error)
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Failed(error) if error.to_string() == "cancelled")
    }
}

impl std::fmt::Display for RecognizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowFailed(detail) => write!(f, "window batch failed: {detail}"),
            Self::Failed(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RecognizeError {}

impl From<std::io::Error> for RecognizeError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error)
    }
}

fn map_inference_error(error: InferenceError) -> RecognizeError {
    match error {
        InferenceError::WindowFailed(detail) => RecognizeError::WindowFailed(detail),
        InferenceError::Cancelled => RecognizeError::cancelled(),
        other => RecognizeError::Failed(std::io::Error::other(other.to_string())),
    }
}

/// One assembled page's artifacts (same fields as the legacy
/// `PageArtifactOutput`, plus the PNG slot for the encode pipeline).
fn assemble(
    page: u32,
    width: u32,
    height: u32,
    rgb8: &[u8],
    response: &TaskResponse,
    output_root: &Path,
) -> Result<PageArtifactResult, std::io::Error> {
    let truncated = response.finish_reason == FinishReason::MaxTokens;
    // Single-source pipeline (§3.2.2/Q3.1): the adapter already stripped
    // `<think>`; the tail crop runs once here and the cropped text drives
    // every downstream artifact.
    let crop = degenerate_tail_crop(&response.text);
    let degenerated = crop.degenerated || response.finish_reason == FinishReason::RepetitionLoop;
    // The adapter owns the nonempty-strip knowledge (single source); the
    // page document reproduces the baseline ThinkRemoved warning from it.
    let think_nonempty = usize::from(response.warnings.iter().any(|w| w == "think_removed"));
    let document = build_page_document(
        page,
        &crop.text,
        response.finish_reason.as_str(),
        degenerated,
        crop.repetition_removed,
        think_nonempty,
    )
    .map_err(|error| std::io::Error::other(format!("adapter: {error:?}")))?;
    let image = RgbImage::from_raw(width, height, rgb8.to_vec()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid RGB8 image shape")
    })?;
    let boxes = document
        .blocks
        .iter()
        .filter_map(|block| match block {
            ocrx_core::PageBlock::VisualRegion { bbox, .. } => Some(*bbox),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (index, bbox) in boxes.iter().copied().enumerate() {
        let figure = u32::try_from(index + 1)
            .map_err(|_| std::io::Error::other("too many visual regions"))?;
        ocrx_pdf::save_bbox_png(&image, bbox, output_root, page, figure)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
    }
    let markdown = replace_bbox_images(&crop.text, page, boxes.len());
    // Q4: a bad model-emitted image link degrades to a link-level removal
    // (warning) instead of failing the whole page.
    let (markdown, dropped_links) = drop_invalid_markdown_image_links(&markdown, output_root);
    let mut warnings = document
        .warnings
        .iter()
        .map(|warning| warning.as_str().to_owned())
        .collect::<Vec<_>>();
    if dropped_links > 0 {
        warnings.push("invalid_image_link_dropped".to_owned());
    }
    Ok(PageArtifactResult {
        png: None,
        markdown,
        page_json: document
            .to_json()
            .map_err(|error| std::io::Error::other(format!("serialize page: {error:?}")))?,
        truncated,
        degenerated,
        warnings,
        think_blocks_stripped: response.think_blocks_stripped,
        raw_output: response.raw_text.clone(),
    })
}

/// Session-driven page recognizer: builds the OCR request per page (the
/// prompt is scan business), submits through the public session trait, and
/// assembles artifacts. Attempt 1 is plain greedy; ladder rungs attach the
/// frozen decode variants.
pub struct SessionRecognizer<'s> {
    session: &'s mut dyn InferenceSession,
    max_output_tokens: u32,
    output_root: std::path::PathBuf,
}

impl<'s> SessionRecognizer<'s> {
    #[must_use]
    pub fn new(
        session: &'s mut dyn InferenceSession,
        output_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            session,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            output_root: output_root.into(),
        }
    }

    fn request(
        &self,
        id: &str,
        width: u32,
        height: u32,
        rgb8: Vec<u8>,
        step: Option<&DegenerationRetryStep>,
    ) -> Result<TaskRequest, std::io::Error> {
        let generation = GenerationParams {
            max_output_tokens: self.max_output_tokens,
            temperature_milli: 0,
            thinking: false,
        };
        let image = ImageInput::new(id, width, height, rgb8).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        let mut request = TaskRequest::new(super::prompt::OCR_MARKDOWN_V1, vec![image], generation)
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?;
        if let Some(step) = step {
            if let Some(grid) = step.image_tokens {
                request = request.with_image_tokens_override(grid).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                })?;
            }
            if let Some(sampling) = step.sampling {
                request = request.with_retry_sampling(sampling);
            }
        }
        Ok(request)
    }

    /// Attempt-1 single-page recognition (plain greedy).
    ///
    /// # Errors
    ///
    /// Per-page failure categories; cancellation surfaces as the cancelled
    /// error.
    pub fn recognize(
        &mut self,
        page: u32,
        image: &RenderedPage,
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(GenerationProgress),
    ) -> Result<PageArtifactResult, RecognizeError> {
        let request = self.request(
            &format!("page-{page}"),
            image.width,
            image.height,
            image.rgb8.clone(),
            None,
        )?;
        let response = self
            .session
            .submit(request, cancellation, heartbeat)
            .map_err(map_inference_error)?;
        if cancellation.is_cancelled() {
            return Err(RecognizeError::cancelled());
        }
        assemble(
            page,
            image.width,
            image.height,
            &image.rgb8,
            &response,
            &self.output_root,
        )
        .map_err(RecognizeError::Failed)
    }

    /// Degeneration-retry recognition (attempts >= 2): the ladder rung's
    /// decode variants ride the request (Q1.2/Q1.4c); the scan owns the
    /// ladder table (§3.4 item 2 — the platform has no retry concept).
    ///
    /// # Errors
    ///
    /// Same surface as [`Self::recognize`].
    pub fn recognize_retry(
        &mut self,
        page: u32,
        image: &RenderedPage,
        step: &DegenerationRetryStep,
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(GenerationProgress),
    ) -> Result<PageArtifactResult, RecognizeError> {
        // The frozen ladder never uses perturbed renders; a perturbed rung
        // would arrive as a re-render, not a decode variant (Q1.4a table).
        let request = self.request(
            &format!("page-{page}"),
            image.width,
            image.height,
            image.rgb8.clone(),
            Some(step),
        )?;
        let response = self
            .session
            .submit(request, cancellation, heartbeat)
            .map_err(map_inference_error)?;
        if cancellation.is_cancelled() {
            return Err(RecognizeError::cancelled());
        }
        assemble(
            page,
            image.width,
            image.height,
            &image.rgb8,
            &response,
            &self.output_root,
        )
        .map_err(RecognizeError::Failed)
    }

    /// Rung 3 (column split): decomposes the page into N band sub-requests
    /// (plain greedy decode per band, depth 1) and merges them per the
    /// §2.3 frozen rules — raw concatenated in band order, think counts
    /// summed, states conservatively merged, warnings appended in band
    /// order, and no figure crops for bands.
    ///
    /// # Errors
    ///
    /// First band failure aborts (mirrors the legacy single-band error
    /// surface); cancellation surfaces as the cancelled error.
    pub fn recognize_column_split(
        &mut self,
        page: u32,
        image: &RenderedPage,
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(GenerationProgress),
    ) -> Result<PageArtifactResult, RecognizeError> {
        let bands = ladder::detect_column_bands(image.width, image.height, &image.rgb8)
            .unwrap_or_else(|| vec![(0, image.width)]);
        let bytes_per_row = image.width as usize * 3;
        let mut merged_text = String::new();
        let mut merged_raw = String::new();
        let mut degenerated = false;
        let mut truncated = false;
        let mut warnings: Vec<String> = Vec::new();
        let mut think_blocks_stripped = 0_u32;
        for (x0, x1) in bands.iter().copied() {
            if cancellation.is_cancelled() {
                return Err(RecognizeError::cancelled());
            }
            let band_width = (x1 - x0) as usize;
            let mut band_rgb8 = Vec::with_capacity(band_width * image.height as usize * 3);
            for row in 0..image.height as usize {
                let start = row * bytes_per_row + x0 as usize * 3;
                band_rgb8.extend_from_slice(&image.rgb8[start..start + band_width * 3]);
            }
            let request = self.request(
                &format!("page-{page}-band-{x0}"),
                x1 - x0,
                image.height,
                band_rgb8,
                None,
            )?;
            let response = self
                .session
                .submit(request, cancellation, heartbeat)
                .map_err(map_inference_error)?;
            if response.finish_reason == FinishReason::MaxTokens {
                truncated = true;
            }
            merged_raw.push_str(&response.raw_text);
            think_blocks_stripped =
                think_blocks_stripped.saturating_add(response.think_blocks_stripped);
            // Band-level crop on the (already stripped) band text.
            let band_crop = degenerate_tail_crop(&response.text);
            if band_crop.degenerated || response.finish_reason == FinishReason::RepetitionLoop {
                degenerated = true;
            }
            if band_crop.repetition_removed {
                warnings.push("repetition_removed".to_owned());
            }
            // Bands carry no figure crops of their own: bbox tags dropped.
            let band_markdown = replace_bbox_images(&band_crop.text, page, 0);
            if !band_markdown.trim().is_empty() {
                if !merged_text.is_empty() {
                    merged_text.push_str("\n\n");
                }
                merged_text.push_str(&band_markdown);
            }
        }
        let crop = DegenerateTailCrop {
            text: merged_text.clone(),
            degenerated,
            repetition_removed: warnings
                .iter()
                .any(|warning| warning == "repetition_removed"),
        };
        let think_nonempty = 0_usize; // band texts were already stripped
        let document = build_page_document(
            page,
            &crop.text,
            "eos",
            degenerated,
            crop.repetition_removed,
            think_nonempty,
        )
        .map_err(|error| std::io::Error::other(format!("adapter: {error:?}")))?;
        let (markdown, dropped_links) = drop_invalid_markdown_image_links(
            &replace_bbox_images(&crop.text, page, 0),
            &self.output_root,
        );
        if dropped_links > 0 {
            warnings.push("invalid_image_link_dropped".to_owned());
        }
        Ok(PageArtifactResult {
            png: None,
            markdown,
            page_json: document
                .to_json()
                .map_err(|error| std::io::Error::other(format!("serialize page: {error:?}")))?,
            truncated,
            degenerated,
            warnings,
            think_blocks_stripped,
            raw_output: merged_raw,
        })
    }

    /// P4.4 window recognition: one `submit_window` for the whole window
    /// (attempt 1, batched). Results are page-ordered; a batch-level
    /// [`RecognizeError::WindowFailed`] fills every entry and the caller
    /// reruns the window single-stream.
    ///
    /// # Errors
    ///
    /// Per-entry categories.
    pub fn recognize_window(
        &mut self,
        pages: &[(u32, RenderedPage)],
        cancellation: &CancellationToken,
        heartbeat: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<PageArtifactResult, RecognizeError>> {
        let mut requests = Vec::with_capacity(pages.len());
        for (page, image) in pages {
            match self.request(
                &format!("page-{page}"),
                image.width,
                image.height,
                image.rgb8.clone(),
                None,
            ) {
                Ok(request) => requests.push(request),
                Err(error) => {
                    // Request construction failure: the window never started.
                    return (0..pages.len())
                        .map(|_| {
                            Err(RecognizeError::Failed(std::io::Error::new(
                                error.kind(),
                                error.to_string(),
                            )))
                        })
                        .collect();
                }
            }
        }
        let responses = self
            .session
            .submit_window(requests, cancellation, heartbeat);
        responses
            .into_iter()
            .zip(pages.iter())
            .map(|(result, (page, image))| {
                result.map_err(map_inference_error).and_then(|response| {
                    if cancellation.is_cancelled() {
                        return Err(RecognizeError::cancelled());
                    }
                    assemble(
                        *page,
                        image.width,
                        image.height,
                        &image.rgb8,
                        &response,
                        &self.output_root,
                    )
                    .map_err(RecognizeError::Failed)
                })
            })
            .collect()
    }
}

/// Frozen retry-sampling spec used by the ladder (re-exported for tests).
#[allow(dead_code)]
fn frozen_rung() -> Option<RetrySampling> {
    crate::state::DEGENERATION_RETRY_LADDER
        .iter()
        .find_map(|step| step.sampling)
}
