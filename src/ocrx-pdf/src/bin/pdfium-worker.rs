use std::env;
use std::path::PathBuf;

use ocrx_pdf::{PdfiumRenderer, RenderOptions, validate_worker_segment};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let library_dir = PathBuf::from(
        args.next()
            .ok_or("usage: pdfium-worker <lib> <pdf> <start> <end> <out> <dpi> [password]")?,
    );
    let input = PathBuf::from(args.next().ok_or("missing pdf")?);
    let start: usize = args.next().ok_or("missing start")?.parse()?;
    let end: usize = args.next().ok_or("missing end")?.parse()?;
    let output = PathBuf::from(args.next().ok_or("missing output")?);
    let dpi: u16 = args
        .next()
        .ok_or("missing dpi")?
        .parse()
        .map_err(|_| "invalid dpi")?;
    if dpi == 0 {
        return Err("dpi must be positive".into());
    }
    if start >= end {
        return Err("page range must be non-empty and start < end".into());
    }
    let renderer = match args.next() {
        Some(password) => PdfiumRenderer::new(library_dir).with_password(password),
        None => PdfiumRenderer::new(library_dir),
    };
    std::fs::create_dir_all(&output)?;
    let count =
        renderer.render_range(&input, RenderOptions { dpi }, start, end, |index, image| {
            let path = output.join(format!("page-{:04}.png", index + 1));
            let temp = output.join(format!(".page-{:04}.part.png", index + 1));
            let (original_width, original_height) = renderer.page_dimensions(&input, index)?;
            image.save(&temp)?;
            std::fs::rename(&temp, &path)?;
            std::fs::write(
                output.join(format!("page-{:04}.audit", index + 1)),
                format!(
                    "original={}x{};rendered={}x{}",
                    original_width,
                    original_height,
                    image.width(),
                    image.height()
                ),
            )?;
            // Progress diagnostics belong on stderr: the parent CLI owns
            // stdout for `--report json` / `--progress jsonl` (review MINOR 4).
            eprintln!("page={} path={}", index + 1, path.display());
            Ok(())
        })?;
    validate_worker_segment(&output, start, end)?;
    eprintln!("worker_complete pages={count}");
    Ok(())
}
