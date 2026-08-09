use std::env;
use std::path::PathBuf;

use ocrx_pdf::{PdfiumRenderer, RenderOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let library_dir = PathBuf::from(
        args.next()
            .ok_or("usage: pdfium-check <library-dir> <pdf>")?,
    );
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: pdfium-check <library-dir> <pdf>")?,
    );
    let renderer = match env::var("OCRX_PDFIUM_PASSWORD") {
        Ok(password) => PdfiumRenderer::new(library_dir).with_password(password),
        Err(_) => PdfiumRenderer::new(library_dir),
    };
    let save_dir = env::var_os("OCRX_PDFIUM_SAVE_DIR").map(PathBuf::from);
    if let Some(directory) = &save_dir {
        std::fs::create_dir_all(directory)?;
    }
    if let Ok(page) = env::var("OCRX_PDFIUM_PAGE").and_then(|value| {
        value
            .parse::<usize>()
            .map_err(|_| std::env::VarError::NotPresent)
    }) {
        let image = renderer.render_page_rgb8(&input, page, RenderOptions::default())?;
        println!(
            "page={} pixels={}x{} color=RGB8",
            page + 1,
            image.width(),
            image.height()
        );
        if let Some(directory) = &save_dir {
            image.save(directory.join(format!("page-{:04}.png", page + 1)))?;
        }
        return Ok(());
    }
    let delay_ms = env::var("OCRX_PDFIUM_CHECK_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let started = std::time::Instant::now();
    let callback =
        |index: usize, image: image::RgbImage| -> Result<(), Box<dyn std::error::Error>> {
            if index == 0 || index % 25 == 0 {
                println!(
                    "page={} pixels={}x{} color=RGB8",
                    index + 1,
                    image.width(),
                    image.height()
                );
            }
            if let Some(directory) = &save_dir {
                image.save(directory.join(format!("page-{:04}.png", index + 1)))?;
            }
            drop(image);
            if delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            Ok(())
        };
    let pages = if let Ok(segment) = env::var("OCRX_PDFIUM_SEGMENT_PAGES") {
        renderer.render_document_segmented(
            &input,
            RenderOptions::default(),
            segment.parse()?,
            callback,
        )?
    } else if env::var_os("OCRX_PDFIUM_REOPEN_EACH_PAGE").is_some() {
        renderer.render_document_reopen_each_page(&input, RenderOptions::default(), callback)?
    } else {
        renderer.render_document(&input, RenderOptions::default(), callback)?
    };
    println!("pages={pages} elapsed_ms={}", started.elapsed().as_millis());
    Ok(())
}
