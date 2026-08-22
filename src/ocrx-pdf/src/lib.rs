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
///
/// `pdfium-render`'s `Display` impl prints the `Debug` representation, so error
/// strings contain variant names like `LoadLibraryError` (a dynamic-library
/// *loading* failure) and `PdfiumLibraryInternalError(FormatError)` (a content
/// failure). The broad substring "library" appears in both, so we match the
/// specific loading variants rather than "library" to avoid misclassifying a
/// format/password error as a dynamic-library failure.
#[must_use]
pub fn classify_pdf_error(message: &str) -> PdfErrorKind {
    let message = message.to_ascii_lowercase();
    if message.contains("loadlibraryerror")
        || message.contains("loadlibraryfunctionnameerror")
        || message.contains("dlopen")
        || message.contains("bind_to_library")
        || message.contains("dynamic library")
    {
        PdfErrorKind::DynamicLibrary
    } else if message.contains("passworderror")
        || message.contains("securityerror")
        || message.contains("password")
        || message.contains("encrypted")
        || message.contains("security")
    {
        PdfErrorKind::Encrypted
    } else if message.contains("formaterror")
        || message.contains("not a pdf")
        || message.contains("invalid pdf")
        || message.contains("file header")
    {
        // pdfium returns FormatError for both non-PDF headers and truncated
        // structures; without a separate corrupt signal we map both to NonPdf.
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
    /// Extra environment variables applied to each spawned worker process.
    /// Production scans leave this empty; tests use it to drive a fake worker's
    /// behavior without touching the process-wide environment (edition 2024
    /// forbids `std::env::set_var`).
    pub env: Vec<(String, String)>,
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
            let mut command = Command::new(&self.worker_executable);
            command.args([
                self.library_dir.to_string_lossy().as_ref(),
                input.as_ref().to_string_lossy().as_ref(),
                &start.to_string(),
                &end.to_string(),
                segment_root.to_string_lossy().as_ref(),
                &self.dpi.to_string(),
            ]);
            for (key, value) in &self.env {
                command.env(key, value);
            }
            let mut child = command.spawn()?;
            let mut peak_rss_kb = 0;
            let mut tick = 0u32;
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
                tick = tick.wrapping_add(1);
                // Sample RSS every ~100ms rather than every 10ms poll: spawning
                // `ps`/`wmic` each tick dominates the loop on Windows, and the
                // peak is captured fine at this cadence.
                if tick % 10 == 0 {
                    peak_rss_kb = peak_rss_kb.max(child_rss_kb(child.id()));
                }
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
    if cfg!(target_os = "windows") {
        // `wmic` reports WorkingSetSize in bytes; best-effort (returns 0 when
        // wmic is absent, e.g. on Windows builds that ship without it).
        Command::new("wmic")
            .args([
                "process",
                "where",
                &format!("ProcessId={pid}"),
                "get",
                "WorkingSetSize",
                "/value",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|stdout| {
                stdout.lines().find_map(|line| {
                    line.trim()
                        .strip_prefix("WorkingSetSize=")?
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
            })
            .map(|bytes| bytes / 1024)
            .unwrap_or_default()
    } else {
        Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|rss| rss.trim().parse::<u64>().ok())
            .unwrap_or_default()
    }
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
// The bind happens inside `OnceLock::get_or_init` so concurrent callers never race
// on `Pdfium::new` (which asserts bindings are uninitialized); the closure runs at
// most once and the result - success or failure - is cached for the process.

/// Resolves the platform PDFium library file path under `library_dir`, rejecting
/// path-escape (`..` components) so a crafted `library_dir` can never traverse
/// out to load an unintended library. Pure: builds a path only, never loads.
///
/// # Errors
///
/// Returns [`PdfErrorKind::DynamicLibrary`] when `library_dir` contains a
/// parent-dir (`..`) component.
pub fn pdfium_library_path(library_dir: &Path) -> Result<PathBuf, PdfErrorKind> {
    if library_dir
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(PdfErrorKind::DynamicLibrary);
    }
    Ok(Pdfium::pdfium_platform_library_name_at_path(library_dir))
}

fn shared_pdfium(library_dir: &Path) -> Result<&'static Pdfium, Box<dyn std::error::Error>> {
    use std::sync::OnceLock;
    static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    let result = PDFIUM.get_or_init(|| {
        // OCRX_PDFIUM_LIBRARY may point at the library file itself or its
        // containing directory; accept both forms at this boundary.
        let library = if library_dir.is_file() {
            library_dir.to_path_buf()
        } else {
            match pdfium_library_path(library_dir) {
                Ok(path) => path,
                Err(_) => {
                    return Err(
                        "library_dir contains parent-dir (..) traversal; refused to load"
                            .to_owned(),
                    );
                }
            }
        };
        Pdfium::bind_to_library(library)
            .map(Pdfium::new)
            .map_err(|error| error.to_string())
    });
    result.as_ref().map_err(|error| error.clone().into())
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
        // Synthetic user-facing messages.
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
            classify_pdf_error("PdfiumLibraryInternalError(PasswordError)"),
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
        // Real pdfium-render error variants (Display prints {:#?}). The
        // PdfiumLibraryInternalError variants must NOT match the dynamic-library
        // class even though they contain the substring "library".
        assert_eq!(
            classify_pdf_error("LoadLibraryError(LoadLibraryExW { source: 126 })"),
            PdfErrorKind::DynamicLibrary
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(FormatError)"),
            PdfErrorKind::NonPdf
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(PasswordError)"),
            PdfErrorKind::Encrypted
        );
        assert_eq!(
            classify_pdf_error("PdfiumLibraryInternalError(SecurityError)"),
            PdfErrorKind::Encrypted
        );
    }

    #[test]
    fn pixel_budget_scales_down_without_upscaling() {
        assert_eq!(fit_pixel_budget(100, 100, 10_000), (100, 100, false));
        let (width, height, scaled) = fit_pixel_budget(2_000, 1_000, 1_000_000);
        assert!(scaled && u64::from(width) * u64::from(height) <= 1_000_000);
    }

    // --- W2.1/W0.2: PDFium library path resolution & loading guard ---------
    //
    // A crafted library_dir must never traverse out (via `..`) to load an
    // unintended DLL. The resolver is pure (builds a path only, never loads),
    // so the guard is testable in-process without touching the process-global
    // Pdfium binding.

    #[test]
    fn pdfium_library_path_rejects_parent_dir_traversal() {
        // A clean directory resolves to the platform library file beside it.
        let clean = pdfium_library_path(Path::new("/vendor/pdfium/bin")).unwrap();
        assert!(
            clean
                .file_name()
                .map(|name| name.to_string_lossy().contains("pdfium"))
                .unwrap_or(false),
            "resolved path should be the platform pdfium library, got {clean:?}"
        );
        // Parent-dir traversal anywhere in library_dir is rejected (path escape
        // must not silently succeed - W0.2).
        assert_eq!(
            pdfium_library_path(Path::new("/vendor/pdfium/bin/../../evil")),
            Err(PdfErrorKind::DynamicLibrary)
        );
        assert_eq!(
            pdfium_library_path(Path::new("../evil")),
            Err(PdfErrorKind::DynamicLibrary)
        );
        assert_eq!(
            pdfium_library_path(Path::new("/opt/../evil")),
            Err(PdfErrorKind::DynamicLibrary)
        );
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

    // --- W3.2: bbox/crop mapping cross-platform contract tests ---------------
    //
    // These pin the pixel-mapping contract: the near edge uses floor (so the
    // crop never starts inside the requested region) and the far edge uses ceil
    // (so the crop never loses the requested region), the result is clamped to
    // the image bounds (no out-of-bounds access), a valid bbox always yields a
    // non-empty crop (the area threshold), and an invalid bbox is rejected
    // before any file is written. Pure logic - runs identically on Windows.

    #[test]
    fn crop_bbox_uses_floor_for_near_edge_and_ceil_for_far_edge() {
        // 10x10 image; bbox (150,150,850,850) maps to:
        //   left  = floor(10*150/1000) = floor(1.5) = 1
        //   right = ceil (10*850/1000) = ceil (8.5) = 9
        // so the crop spans columns 1..9 (8 px) - proving floor on the near
        // edge and ceil on the far edge.
        let image = RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0]));
        let bbox = NormalizedBbox::new(150, 150, 850, 850).unwrap();
        let crop = crop_bbox(&image, bbox).unwrap();
        assert_eq!(
            crop.width(),
            8,
            "near edge should floor, far edge should ceil"
        );
        assert_eq!(crop.height(), 8);
    }

    #[test]
    fn crop_bbox_fits_within_image_bounds_at_the_far_edge_without_overflow() {
        // bbox reaching the far corner (999) on a 10x10 image maps right/bottom
        // to 10 (== width/height) and must not overflow or panic.
        let image = RgbImage::from_pixel(10, 10, image::Rgb([9, 9, 9]));
        let bbox = NormalizedBbox::new(0, 0, 999, 999).unwrap();
        let crop = crop_bbox(&image, bbox).unwrap();
        assert_eq!(crop.width(), 10);
        assert_eq!(crop.height(), 10);
        assert_eq!(crop.get_pixel(9, 9), &image::Rgb([9, 9, 9]));
    }

    #[test]
    fn crop_bbox_rejects_invalid_bbox_and_valid_bbox_is_always_nonempty() {
        // An invalid bbox (zero-width) is rejected before cropping.
        let image = RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0]));
        let invalid = NormalizedBbox {
            left: 50,
            top: 50,
            right: 50,
            bottom: 90,
        };
        assert!(crop_bbox(&image, invalid).is_err());
        // The area threshold: every valid bbox yields a non-empty crop, even on
        // a tiny image where the normalized region is sub-pixel-wide.
        let small = RgbImage::from_pixel(3, 3, image::Rgb([0, 0, 0]));
        for (left, top, right, bottom) in [
            (0, 0, 1, 1),
            (1, 1, 2, 2),
            (0, 0, 999, 999),
            (500, 500, 501, 501),
        ] {
            let bbox = NormalizedBbox::new(left, top, right, bottom).unwrap();
            let crop = crop_bbox(&small, bbox)
                .unwrap_or_else(|e| panic!("valid bbox {bbox:?} should crop: {e}"));
            assert!(
                crop.width() >= 1 && crop.height() >= 1,
                "empty crop for {bbox:?}"
            );
        }
    }

    #[test]
    fn save_bbox_png_rejects_invalid_bbox_without_writing_any_file() {
        let image = RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0]));
        let root = std::env::temp_dir().join(format!("ocrx-png-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let invalid = NormalizedBbox {
            left: 10,
            top: 10,
            right: 10,
            bottom: 20,
        };
        assert!(save_bbox_png(&image, invalid, &root, 1, 1).is_err());
        // No images directory or figure file is left behind for an invalid bbox.
        assert!(!root.join("images").exists());
        assert!(!root.exists() || fs::read_dir(&root).unwrap().count() == 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bbox_iou_is_zero_for_disjoint_one_for_identical_and_expected_for_overlap() {
        let disjoint_a = NormalizedBbox::new(0, 0, 100, 100).unwrap();
        let disjoint_b = NormalizedBbox::new(200, 200, 300, 300).unwrap();
        assert!(bbox_iou(disjoint_a, disjoint_b).abs() < 1e-6);
        let identical = NormalizedBbox::new(0, 0, 200, 200).unwrap();
        assert!((bbox_iou(identical, identical) - 1.0).abs() < 1e-6);
        // a=(0,0,200,200) area 40000, b=(100,100,300,300) area 40000,
        // intersection (100,100,200,200) area 10000 -> IoU 10000/70000.
        let partial = NormalizedBbox::new(100, 100, 300, 300).unwrap();
        assert!((bbox_iou(identical, partial) - 10_000.0_f32 / 70_000.0).abs() < 1e-6);
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
            env: Vec::new(),
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

    // The portable scheduler-isolation tests live in `tests/worker_scheduler.rs`
    // as integration tests: only integration tests get the `CARGO_BIN_EXE_*`
    // env at compile time, which `cargo test` resolves by building the actual
    // `pdfium-fake-worker` bin artifact (lib unit tests only build the bin's
    // test harness, never the bin itself). See `tests/worker_scheduler.rs`.

    // --- W2.1: real-PDFium component tests ------------------------------------
    //
    // These exercise the frozen pdfium runtime (pdfium-render 0.9.3 +
    // `pdfium_7881` binding the chromium/7961 native library) against the
    // committed PDF fixtures. They skip when the native library is absent
    // (CI without the runtime artifact) so `cargo test` stays green; on a
    // machine with the library present they verify the Windows loading path,
    // 300 DPI RGB8 rendering, audit sizes, page selection, pixel-budget
    // scaling, and real error classification. See
    // docs/develop/pdfium-windows-release-baseline-2026-08-11.md.

    /// Directory holding the co-located `pdfium.dll`/`libpdfium.dylib`, or
    /// `None` when the runtime artifact is not present. The path is canonicalized
    /// so it has no `..` components: the `pdfium_library_path` guard rejects
    /// parent-dir traversal (W0.2), and the application is expected to pass a
    /// canonical `library_dir` (the production default is `.`).
    fn pdfium_lib_dir() -> Option<PathBuf> {
        let dir = std::env::var_os("OCRX_PDFIUM_LIB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/bin")
            });
        if !Pdfium::pdfium_platform_library_name_at_path(&dir).is_file() {
            return None;
        }
        std::fs::canonicalize(&dir).ok()
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../benchmarks/fixtures/pdf")
            .join(name)
    }

    fn ensure_pdfium() -> Option<PathBuf> {
        let dir = pdfium_lib_dir();
        if dir.is_none() {
            eprintln!(
                "note: pdfium runtime not present at vendor/pdfium/bin; \
                 skipping real-PDF component test"
            );
        }
        dir
    }

    #[test]
    fn real_pdf_page_counts_match_fixtures() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        assert_eq!(
            renderer.page_count(fixture("normal-1-page.pdf")).unwrap(),
            1
        );
        assert_eq!(
            renderer
                .page_count(fixture("complex-20-pages.pdf"))
                .unwrap(),
            20
        );
        assert_eq!(
            renderer
                .page_count(fixture("normal-100-pages.pdf"))
                .unwrap(),
            100
        );
    }

    #[test]
    fn real_pdf_renders_rgb8_at_300_dpi_with_audit_sizes() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        let (image, original_width, original_height) = renderer
            .render_page_rgb8_with_audit(fixture("normal-1-page.pdf"), 0, RenderOptions::default())
            .unwrap();
        // RGB8: exactly 3 bytes per pixel.
        assert_eq!(
            image.as_raw().len(),
            usize::try_from(image.width()).unwrap() * usize::try_from(image.height()).unwrap() * 3
        );
        // 300 DPI render = source 72-DPI points * 300/72 (within rounding).
        let dpi = f32::from(RenderOptions::default().dpi);
        assert_eq!(
            image.width(),
            (original_width as f32 * dpi / 72.0).round() as u32
        );
        assert_eq!(
            image.height(),
            (original_height as f32 * dpi / 72.0).round() as u32
        );
    }

    #[test]
    fn real_pdf_page_selection_maps_to_the_requested_index() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        // Page 5 (0-indexed 4) renders and differs from page 1, proving the
        // selection index maps to the right page rather than always page 0.
        let first = renderer
            .render_page_rgb8(fixture("complex-20-pages.pdf"), 0, RenderOptions::default())
            .unwrap();
        let fifth = renderer
            .render_page_rgb8(fixture("complex-20-pages.pdf"), 4, RenderOptions::default())
            .unwrap();
        assert_ne!(first.as_raw(), fifth.as_raw());
    }

    #[test]
    fn real_render_pixel_budget_scales_proportionally_without_cropping() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        let (image, _, _) = renderer
            .render_page_rgb8_with_audit(fixture("normal-1-page.pdf"), 0, RenderOptions::default())
            .unwrap();
        // Force a downscale below the rendered pixel count; the budget must
        // shrink both dimensions proportionally (no cropping) and stay within
        // budget, never upscaling.
        let pixels = u64::from(image.width()) * u64::from(image.height());
        let budget = pixels / 2;
        let (width, height, scaled) = fit_pixel_budget(image.width(), image.height(), budget);
        assert!(scaled);
        assert!(u64::from(width) * u64::from(height) <= budget);
        assert!(width <= image.width() && height <= image.height());
        // Aspect ratio preserved within floor rounding tolerance.
        let original_ratio = f64::from(image.width()) / f64::from(image.height());
        let scaled_ratio = f64::from(width) / f64::from(height);
        assert!((scaled_ratio - original_ratio).abs() < 0.05);
    }

    #[test]
    fn real_pdfium_errors_classify_to_stable_content_categories() {
        let Some(lib) = ensure_pdfium() else {
            return;
        };
        let renderer = PdfiumRenderer::new(lib);
        // A non-PDF file must classify as NonPdf (the library loaded fine, so
        // this is a content error, not a DynamicLibrary error).
        let non_pdf = std::env::temp_dir().join(format!("ocrx-notpdf-{}.txt", std::process::id()));
        fs::write(&non_pdf, "this is not a pdf document").unwrap();
        let error = renderer.page_count(&non_pdf).unwrap_err();
        assert_eq!(classify_pdf_error(&error.to_string()), PdfErrorKind::NonPdf);
        let _ = fs::remove_file(&non_pdf);

        // A truncated/corrupt fixture must classify as a content error
        // (NonPdf or Corrupt), never as a DynamicLibrary failure.
        let error = renderer
            .page_count(fixture("corrupt-truncated.pdf"))
            .unwrap_err();
        let kind = classify_pdf_error(&error.to_string());
        assert!(
            matches!(kind, PdfErrorKind::NonPdf | PdfErrorKind::Corrupt),
            "expected content error for corrupt fixture, got {kind:?}"
        );
    }
}
