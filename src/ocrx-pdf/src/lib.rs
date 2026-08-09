//! PDFium-backed PDF rendering boundary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use image::{GenericImageView, RgbImage};
use ocrx_core::NormalizedBbox;
use pdfium_render::prelude::*;
use std::fs;
use std::io::Cursor;

fn render_config(width: i32, height: i32) -> PdfRenderConfig {
    let mut config = PdfRenderConfig::new().set_fixed_size(width, height);
    if std::env::var_os("OCRX_PDFIUM_LIMIT_IMAGE_CACHE").is_some() {
        config = config.limit_render_image_cache_size(true);
    }
    if std::env::var_os("OCRX_PDFIUM_CLEAR_BEFORE_RENDER").is_some() {
        config = config.clear_before_rendering(true);
    }
    config
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub dpi: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfErrorKind {
    NonPdf,
    Corrupt,
    Encrypted,
    DynamicLibrary,
    Unknown,
}

/// Maps PDFium boundary failures to stable, user-facing error classes.
#[must_use]
pub fn classify_pdf_error(message: &str) -> PdfErrorKind {
    let message = message.to_ascii_lowercase();
    if message.contains("library")
        || message.contains("dynamic")
        || message.contains("bind_to_library")
    {
        PdfErrorKind::DynamicLibrary
    } else if message.contains("password")
        || message.contains("encrypted")
        || message.contains("security")
    {
        PdfErrorKind::Encrypted
    } else if message.contains("not a pdf")
        || message.contains("invalid pdf")
        || message.contains("file header")
    {
        PdfErrorKind::NonPdf
    } else if message.contains("corrupt")
        || message.contains("malformed")
        || message.contains("damaged")
        || message.contains("parse")
    {
        PdfErrorKind::Corrupt
    } else {
        PdfErrorKind::Unknown
    }
}

#[must_use]
pub fn fit_pixel_budget(width: u32, height: u32, budget: u64) -> (u32, u32, bool) {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= budget || budget == 0 {
        return (width, height, false);
    }
    let scale = (budget as f64 / pixels as f64).sqrt();
    (
        ((f64::from(width) * scale).floor() as u32).max(1),
        ((f64::from(height) * scale).floor() as u32).max(1),
        true,
    )
}

/// Crops a validated normalized bbox without allowing out-of-bounds access.
///
/// # Errors
///
/// Returns an error when the bbox is invalid or maps to an empty crop.
pub fn crop_bbox(image: &RgbImage, bbox: NormalizedBbox) -> Result<RgbImage, &'static str> {
    if !bbox.is_valid() {
        return Err("invalid bbox");
    }
    let left = (u64::from(image.width()) * u64::from(bbox.left) / 1000) as u32;
    let top = (u64::from(image.height()) * u64::from(bbox.top) / 1000) as u32;
    let right = ((u64::from(image.width()) * u64::from(bbox.right) + 999) / 1000)
        .min(u64::from(image.width())) as u32;
    let bottom = ((u64::from(image.height()) * u64::from(bbox.bottom) + 999) / 1000)
        .min(u64::from(image.height())) as u32;
    if left >= right || top >= bottom {
        return Err("empty crop");
    }
    Ok(image::imageops::crop_imm(image, left, top, right - left, bottom - top).to_image())
}

#[must_use]
pub fn bbox_iou(a: NormalizedBbox, b: NormalizedBbox) -> f32 {
    let left = a.left.max(b.left) as f32;
    let top = a.top.max(b.top) as f32;
    let right = a.right.min(b.right) as f32;
    let bottom = a.bottom.min(b.bottom) as f32;
    let intersection = (right - left).max(0.0) * (bottom - top).max(0.0);
    let area_a = f32::from(a.right - a.left) * f32::from(a.bottom - a.top);
    let area_b = f32::from(b.right - b.left) * f32::from(b.bottom - b.top);
    intersection / (area_a + area_b - intersection).max(1.0)
}

/// Saves a cropped figure using a program-generated name and atomic rename.
pub fn save_bbox_png(
    image: &RgbImage,
    bbox: NormalizedBbox,
    output_root: impl AsRef<Path>,
    page: u32,
    figure: u32,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if page == 0 || figure == 0 {
        return Err("page and figure must be positive".into());
    }
    let cropped = crop_bbox(image, bbox).map_err(std::io::Error::other)?;
    let root = output_root.as_ref().join("images");
    fs::create_dir_all(&root)?;
    let final_path = root.join(format!("page-{page:04}-figure-{figure:03}.png"));
    let part_path = root.join(format!(".page-{page:04}-figure-{figure:03}.part.png"));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(cropped).write_to(&mut bytes, image::ImageFormat::Png)?;
    fs::write(&part_path, bytes.into_inner())?;
    fs::rename(&part_path, &final_path)?;
    Ok(final_path)
}

/// Validates a worker page segment before it can be committed by the caller.
///
/// # Errors
///
/// Returns an error when the segment is empty, has missing/extra pages, has
/// temporary files, or contains an invalid PNG.
pub fn validate_worker_segment(
    output_root: impl AsRef<Path>,
    start_page: usize,
    end_page: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if start_page >= end_page {
        return Err("worker segment must be non-empty".into());
    }
    let root = output_root.as_ref();
    let mut png_names = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("part")
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(".part."))
        {
            return Err(format!("worker left temporary file: {}", path.display()).into());
        }
        if path.extension().and_then(|value| value.to_str()) == Some("png") {
            png_names.push(path);
        }
    }
    let expected_count = end_page - start_page;
    if png_names.len() != expected_count {
        return Err(format!(
            "worker output count mismatch: {} != {expected_count}",
            png_names.len()
        )
        .into());
    }
    for page in start_page..end_page {
        let path = root.join(format!("page-{:04}.png", page + 1));
        if !path.is_file() {
            return Err(format!("worker output missing: {}", path.display()).into());
        }
        image::open(&path)
            .map_err(|error| format!("invalid worker PNG {}: {error}", path.display()))?;
        let audit_path = root.join(format!("page-{:04}.audit", page + 1));
        let audit = fs::read_to_string(&audit_path)
            .map_err(|error| format!("invalid worker audit {}: {error}", audit_path.display()))?;
        let rendered = audit
            .split_once(";rendered=")
            .and_then(|(_, value)| value.split_once(';').or(Some((value, ""))))
            .ok_or_else(|| format!("invalid worker audit {}", audit_path.display()))?
            .0;
        let (width, height) = rendered.split_once('x').ok_or_else(|| {
            format!(
                "invalid worker rendered dimensions {}",
                audit_path.display()
            )
        })?;
        let metadata_dimensions = (width.parse::<u32>(), height.parse::<u32>());
        let actual_dimensions = image::open(&path)
            .map_err(|error| format!("invalid worker PNG {}: {error}", path.display()))?
            .dimensions();
        if metadata_dimensions != (Ok(actual_dimensions.0), Ok(actual_dimensions.1)) {
            return Err(format!(
                "worker audit dimensions do not match PNG {}",
                path.display()
            )
            .into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSegmentReport {
    pub start_page: usize,
    pub end_page: usize,
    pub pages: usize,
    pub elapsed_millis: u128,
    pub peak_rss_kb: u64,
}

#[derive(Debug, Clone)]
pub struct PdfWorkerScheduler {
    pub worker_executable: PathBuf,
    pub library_dir: PathBuf,
    pub segment_pages: usize,
    pub timeout: Duration,
    pub dpi: u16,
}

impl PdfWorkerScheduler {
    /// Runs all requested pages in isolated worker processes.
    ///
    /// # Errors
    ///
    /// Returns an error when the range, worker, process exit, or page outputs are invalid.
    pub fn run(
        &self,
        input: impl AsRef<Path>,
        output_root: impl AsRef<Path>,
        start_page: usize,
        end_page: usize,
    ) -> Result<Vec<WorkerSegmentReport>, Box<dyn std::error::Error>> {
        self.run_with_cancel(input, output_root, start_page, end_page, || false)
    }

    /// Runs worker segments and stops when the callback requests cancellation.
    ///
    /// # Errors
    ///
    /// Returns an error for cancellation, timeout, worker failure, or invalid output.
    pub fn run_with_cancel(
        &self,
        input: impl AsRef<Path>,
        output_root: impl AsRef<Path>,
        start_page: usize,
        end_page: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Vec<WorkerSegmentReport>, Box<dyn std::error::Error>> {
        if self.segment_pages == 0 || start_page >= end_page {
            return Err("invalid worker range or segment size".into());
        }
        if !self.worker_executable.is_file() {
            return Err("worker executable is missing".into());
        }
        let root = output_root.as_ref();
        fs::create_dir_all(root)?;
        let mut reports = Vec::new();
        let mut start = start_page;
        while start < end_page {
            let end = (start + self.segment_pages).min(end_page);
            let segment_root = root.join(format!(".worker-{start:04}-{end:04}"));
            if segment_root.exists() {
                fs::remove_dir_all(&segment_root)?;
            }
            fs::create_dir_all(&segment_root)?;
            let began = Instant::now();
            let mut child = Command::new(&self.worker_executable)
                .args([
                    self.library_dir.to_string_lossy().as_ref(),
                    input.as_ref().to_string_lossy().as_ref(),
                    &start.to_string(),
                    &end.to_string(),
                    segment_root.to_string_lossy().as_ref(),
                    &self.dpi.to_string(),
                ])
                .spawn()?;
            let mut peak_rss_kb = 0;
            let status = loop {
                if cancelled() {
                    child.kill()?;
                    let _ = child.wait();
                    fs::remove_dir_all(&segment_root)?;
                    return Err("worker cancelled".into());
                }
                if began.elapsed() > self.timeout {
                    child.kill()?;
                    let _ = child.wait();
                    fs::remove_dir_all(&segment_root)?;
                    return Err("worker timed out".into());
                }
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                peak_rss_kb = peak_rss_kb.max(child_rss_kb(child.id()));
                thread::sleep(Duration::from_millis(10));
            };
            if !status.success() {
                fs::remove_dir_all(&segment_root)?;
                return Err(format!("worker failed for [{start},{end}) with {status}").into());
            }
            if let Err(error) = validate_worker_segment(&segment_root, start, end) {
                let _ = fs::remove_dir_all(&segment_root);
                return Err(error);
            }
            for page in start..end {
                if let Err(error) = fs::rename(
                    segment_root.join(format!("page-{:04}.png", page + 1)),
                    root.join(format!("page-{:04}.png", page + 1)),
                ) {
                    let _ = fs::remove_dir_all(&segment_root);
                    return Err(error.into());
                }
                if let Err(error) = fs::rename(
                    segment_root.join(format!("page-{:04}.audit", page + 1)),
                    root.join(format!("page-{:04}.audit", page + 1)),
                ) {
                    let _ = fs::remove_dir_all(&segment_root);
                    return Err(error.into());
                }
            }
            fs::remove_dir_all(&segment_root)?;
            reports.push(WorkerSegmentReport {
                start_page: start,
                end_page: end,
                pages: end - start,
                elapsed_millis: began.elapsed().as_millis(),
                peak_rss_kb,
            });
            start = end;
        }
        Ok(reports)
    }
}

fn child_rss_kb(pid: u32) -> u64 {
    let pid = pid.to_string();
    Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|rss| rss.trim().parse::<u64>().ok())
        .unwrap_or_default()
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { dpi: 300 }
    }
}

#[derive(Debug)]
pub struct PdfiumRenderer {
    library_dir: PathBuf,
    password: Option<String>,
}

// pdfium-render bindings are process-global; bind once and reuse them for a scan job.
fn shared_pdfium(library_dir: &Path) -> Result<&'static Pdfium, Box<dyn std::error::Error>> {
    use std::sync::OnceLock;
    static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    let library = Pdfium::pdfium_platform_library_name_at_path(library_dir);
    let pdfium = Pdfium::new(Pdfium::bind_to_library(library)?);
    Ok(PDFIUM.get_or_init(|| pdfium))
}

impl PdfiumRenderer {
    pub fn new(library_dir: impl Into<PathBuf>) -> Self {
        Self {
            library_dir: library_dir.into(),
            password: None,
        }
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    pub fn render_page_rgb8(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        options: RenderOptions,
    ) -> Result<RgbImage, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        let document = pdfium.load_pdf_from_file(input, self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        let width = (page.width().value * f32::from(options.dpi) / 72.0).round() as i32;
        let height = (page.height().value * f32::from(options.dpi) / 72.0).round() as i32;
        Ok(page
            .render_with_config(&render_config(width, height))?
            .as_image()?
            .into_rgb8())
    }

    /// Renders one page into the RGB8 shape required by the runtime boundary.
    ///
    /// # Errors
    ///
    /// Returns the underlying PDFium or image conversion error.
    pub fn render_page_rgb8_bytes(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        options: RenderOptions,
    ) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
        let image = self.render_page_rgb8(input, page_number, options)?;
        let (width, height) = image.dimensions();
        Ok((width, height, image.into_raw()))
    }

    /// Renders a page and returns its source-at-72-DPI dimensions for audit metadata.
    pub fn render_page_rgb8_with_audit(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
        options: RenderOptions,
    ) -> Result<(RgbImage, u32, u32), Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        let original_width = page.width().value.round().max(1.0) as u32;
        let original_height = page.height().value.round().max(1.0) as u32;
        let width = (page.width().value * f32::from(options.dpi) / 72.0).round() as i32;
        let height = (page.height().value * f32::from(options.dpi) / 72.0).round() as i32;
        let image = page
            .render_with_config(&render_config(width, height))?
            .as_image()?
            .into_rgb8();
        Ok((image, original_width, original_height))
    }

    pub fn page_count(&self, input: impl AsRef<Path>) -> Result<usize, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        Ok(pdfium
            .load_pdf_from_file(input, self.password.as_deref())?
            .pages()
            .len()
            .try_into()?)
    }

    pub fn page_dimensions(
        &self,
        input: impl AsRef<Path>,
        page_number: usize,
    ) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page = document.pages().get(page_number.try_into()?)?;
        Ok((
            page.width().value.round().max(1.0) as u32,
            page.height().value.round().max(1.0) as u32,
        ))
    }

    pub fn render_document(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let page_count = document.pages().len();
        for index in 0..page_count {
            let page = document.pages().get(index)?;
            let width = (page.width().value * f32::from(options.dpi) / 72.0).round() as i32;
            let height = (page.height().value * f32::from(options.dpi) / 72.0).round() as i32;
            let image = {
                let bitmap = page.render_with_config(&render_config(width, height))?;
                let image = bitmap.as_image()?.into_rgb8();
                drop(bitmap);
                image
            };
            drop(page);
            on_page(index.try_into()?, image)?;
        }
        Ok(page_count.try_into()?)
    }

    pub fn render_document_reopen_each_page(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        let page_count: usize = pdfium
            .load_pdf_from_file(input, self.password.as_deref())?
            .pages()
            .len()
            .try_into()?;
        for index in 0..page_count {
            let document = pdfium.load_pdf_from_file(input, self.password.as_deref())?;
            let page = document.pages().get(index.try_into()?)?;
            let width = (page.width().value * f32::from(options.dpi) / 72.0).round() as i32;
            let height = (page.height().value * f32::from(options.dpi) / 72.0).round() as i32;
            let image = {
                let bitmap = page.render_with_config(&render_config(width, height))?;
                let image = bitmap.as_image()?.into_rgb8();
                drop(bitmap);
                image
            };
            drop(page);
            on_page(index, image)?;
        }
        Ok(page_count)
    }

    pub fn render_document_segmented(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        segment_pages: usize,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if segment_pages == 0 {
            return Err("segment_pages must be greater than zero".into());
        }
        let pdfium = shared_pdfium(&self.library_dir)?;
        let input = input.as_ref();
        let page_count: usize = pdfium
            .load_pdf_from_file(input, self.password.as_deref())?
            .pages()
            .len()
            .try_into()?;
        let mut start = 0;
        while start < page_count {
            let document = pdfium.load_pdf_from_file(input, self.password.as_deref())?;
            let end = (start + segment_pages).min(page_count);
            for index in start..end {
                let page = document.pages().get(index.try_into()?)?;
                let width = (page.width().value * f32::from(options.dpi) / 72.0).round() as i32;
                let height = (page.height().value * f32::from(options.dpi) / 72.0).round() as i32;
                let image = {
                    let bitmap = page.render_with_config(&render_config(width, height))?;
                    let image = bitmap.as_image()?.into_rgb8();
                    drop(bitmap);
                    image
                };
                drop(page);
                on_page(index, image)?;
            }
            drop(document);
            start = end;
        }
        Ok(page_count)
    }

    pub fn render_range(
        &self,
        input: impl AsRef<Path>,
        options: RenderOptions,
        start_page: usize,
        end_page: usize,
        mut on_page: impl FnMut(usize, RgbImage) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if start_page >= end_page {
            return Err("render range must be non-empty".into());
        }
        let pdfium = shared_pdfium(&self.library_dir)?;
        let document = pdfium.load_pdf_from_file(input.as_ref(), self.password.as_deref())?;
        let total: usize = document.pages().len().try_into()?;
        let end_page = end_page.min(total);
        for index in start_page..end_page {
            let page = document.pages().get(index.try_into()?)?;
            let width = (page.width().value * f32::from(options.dpi) / 72.0).round() as i32;
            let height = (page.height().value * f32::from(options.dpi) / 72.0).round() as i32;
            let bitmap = page.render_with_config(&render_config(width, height))?;
            let image = bitmap.as_image()?.into_rgb8();
            drop(bitmap);
            drop(page);
            on_page(index, image)?;
        }
        Ok(end_page - start_page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_render_is_300_dpi() {
        assert_eq!(RenderOptions::default().dpi, 300);
    }

    #[test]
    fn pdfium_errors_map_to_stable_classes() {
        assert_eq!(
            classify_pdf_error("file is not a PDF document"),
            PdfErrorKind::NonPdf
        );
        assert_eq!(
            classify_pdf_error("malformed or corrupt cross-reference table"),
            PdfErrorKind::Corrupt
        );
        assert_eq!(
            classify_pdf_error("document is encrypted and requires a password"),
            PdfErrorKind::Encrypted
        );
        assert_eq!(
            classify_pdf_error("failed to load dynamic library"),
            PdfErrorKind::DynamicLibrary
        );
        assert_eq!(
            classify_pdf_error("unexpected failure"),
            PdfErrorKind::Unknown
        );
    }

    #[test]
    fn pixel_budget_scales_down_without_upscaling() {
        assert_eq!(fit_pixel_budget(100, 100, 10_000), (100, 100, false));
        let (width, height, scaled) = fit_pixel_budget(2_000, 1_000, 1_000_000);
        assert!(scaled && u64::from(width) * u64::from(height) <= 1_000_000);
    }

    #[test]
    fn bbox_iou_and_png_output_are_deterministic() {
        let image = RgbImage::from_pixel(100, 100, image::Rgb([255, 0, 0]));
        let bbox = NormalizedBbox::new(100, 100, 500, 500).unwrap();
        assert!(bbox_iou(bbox, bbox) > 0.99);
        let root = std::env::temp_dir().join(format!("ocrx-png-{}", std::process::id()));
        let path = save_bbox_png(&image, bbox, &root, 2, 1).unwrap();
        assert!(path.ends_with("page-0002-figure-001.png"));
        assert!(path.is_file());
        assert!(!root.join("images/.page-0002-figure-001.part.png").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn worker_segment_validation_rejects_missing_and_part_files() {
        let root = std::env::temp_dir().join(format!("ocrx-worker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(validate_worker_segment(&root, 0, 1).is_err());
        fs::write(root.join(".page-0001.part.png"), b"partial").unwrap();
        assert!(validate_worker_segment(&root, 0, 1).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn worker_segment_validation_rejects_extra_png() {
        let root = std::env::temp_dir().join(format!("ocrx-worker-extra-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let image = RgbImage::from_pixel(2, 2, image::Rgb([0, 0, 0]));
        image.save(root.join("page-0001.png")).unwrap();
        image.save(root.join("page-0002.png")).unwrap();
        assert!(validate_worker_segment(&root, 0, 1).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn worker_scheduler_rejects_missing_executable_and_invalid_range() {
        let scheduler = PdfWorkerScheduler {
            worker_executable: PathBuf::from("/missing/pdfium-worker"),
            library_dir: PathBuf::from("/missing"),
            segment_pages: 20,
            timeout: Duration::from_secs(1),
            dpi: 300,
        };
        assert!(
            scheduler
                .run("input.pdf", std::env::temp_dir(), 0, 1)
                .is_err()
        );
        let invalid = PdfWorkerScheduler {
            segment_pages: 0,
            ..scheduler
        };
        assert!(
            invalid
                .run("input.pdf", std::env::temp_dir(), 0, 1)
                .is_err()
        );
    }

    fn fake_worker(name: &str, body: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ocrx-worker-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let script = root.join("worker.sh");
        fs::write(&script, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }
        script
    }

    fn worker_scheduler(worker: PathBuf, timeout: Duration) -> PdfWorkerScheduler {
        PdfWorkerScheduler {
            worker_executable: worker,
            library_dir: PathBuf::from("/library"),
            segment_pages: 2,
            timeout,
            dpi: 300,
        }
    }

    #[test]
    fn worker_scheduler_commits_only_valid_segments_and_reports_resources() {
        let worker = fake_worker(
            "success",
            "out=\"$5\"\nprintf 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=' | base64 -d > \"$out/page-0001.png\"\nprintf 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=' | base64 -d > \"$out/page-0002.png\"\nprintf 'original=1x1;rendered=1x1' > \"$out/page-0001.audit\"\nprintf 'original=1x1;rendered=1x1' > \"$out/page-0002.audit\"",
        );
        let output =
            std::env::temp_dir().join(format!("ocrx-worker-output-{}", std::process::id()));
        let _ = fs::remove_dir_all(&output);
        let reports = worker_scheduler(worker.clone(), Duration::from_secs(10))
            .run("input.pdf", &output, 0, 2)
            .unwrap();
        assert_eq!(reports[0].pages, 2);
        assert!(output.join("page-0001.png").is_file());
        assert!(output.join("page-0002.png").is_file());
        assert!(!output.join(".worker-0000-0002").exists());
        let _ = fs::remove_dir_all(output);
        let _ = fs::remove_dir_all(worker.parent().unwrap());
    }

    #[test]
    fn worker_scheduler_maps_failure_timeout_cancel_and_invalid_png() {
        let failed = fake_worker("failed", "exit 7");
        assert!(
            worker_scheduler(failed.clone(), Duration::from_secs(1))
                .run(
                    "input.pdf",
                    std::env::temp_dir().join("ocrx-worker-fail"),
                    0,
                    1
                )
                .is_err()
        );
        let timed_out = fake_worker("timeout", "sleep 1");
        assert!(
            worker_scheduler(timed_out.clone(), Duration::from_millis(20))
                .run(
                    "input.pdf",
                    std::env::temp_dir().join("ocrx-worker-timeout"),
                    0,
                    1
                )
                .is_err()
        );
        let cancelled = fake_worker("cancel", "sleep 1");
        assert!(
            worker_scheduler(cancelled.clone(), Duration::from_secs(1))
                .run_with_cancel(
                    "input.pdf",
                    std::env::temp_dir().join("ocrx-worker-cancel"),
                    0,
                    1,
                    || true,
                )
                .is_err()
        );
        let invalid = fake_worker("invalid", "touch \"$5/page-0001.png\"");
        let invalid_output = std::env::temp_dir().join("ocrx-worker-invalid");
        let _ = fs::remove_dir_all(&invalid_output);
        assert!(
            worker_scheduler(invalid.clone(), Duration::from_secs(1))
                .run("input.pdf", &invalid_output, 0, 1)
                .is_err()
        );
        assert!(!invalid_output.join(".worker-0000-0001").exists());
        for worker in [failed, timed_out, cancelled, invalid] {
            let _ = fs::remove_dir_all(worker.parent().unwrap());
        }
    }
}
