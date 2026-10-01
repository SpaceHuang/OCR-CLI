//! Test-fixture worker mimicking the `pdfium-worker` protocol for scheduler
//! isolation tests. Behavior is selected by the `OCRX_FAKE_WORKER_MODE`
//! environment variable so a single portable binary covers success, failure,
//! timeout, cancel, and invalid-output scenarios on every platform — replacing
//! the platform-specific shell scripts used previously. This binary is a dev
//! fixture and is not shipped in releases.
//!
//! Protocol (positional, matching what `PdfWorkerScheduler` spawns):
//! `<library-dir> <input> <start> <end> <output> <dpi> [password]`

use std::env;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let _library_dir = args.next();
    let _input = args.next();
    let start: usize = args.next().ok_or("missing start".to_owned())?.parse()?;
    let end: usize = args.next().ok_or("missing end".to_owned())?.parse()?;
    let output_arg = args.next().ok_or("missing output".to_owned())?;
    let output = Path::new(&output_arg);
    let _dpi = args.next();
    let mode = env::var("OCRX_FAKE_WORKER_MODE").unwrap_or_else(|_| "success".to_owned());
    fs::create_dir_all(output)?;
    match mode.as_str() {
        "fail" => std::process::exit(7),
        // Records the exact argv it received (one argument per line) at the
        // path given by OCRX_FAKE_ARGS_OUT before producing a valid segment,
        // so tests can pin the spawn protocol — including the optional 7th
        // password argument. The segment directory itself is wiped by the
        // scheduler after validation, hence the out-of-band path.
        "args" => {
            let Some(args_out) = std::env::var_os("OCRX_FAKE_ARGS_OUT") else {
                return Err("args mode requires OCRX_FAKE_ARGS_OUT".into());
            };
            fs::write(args_out, std::env::args().collect::<Vec<_>>().join("\n"))?;
        }
        "sleep" => {
            // Run longer than any scheduler timeout used in tests so the
            // scheduler exercises its kill-on-timeout / kill-on-cancel paths.
            thread::sleep(Duration::from_secs(60));
            return Ok(());
        }
        "bad-png" => {
            for page in start..end {
                fs::write(
                    output.join(format!("page-{:04}.png", page + 1)),
                    b"not a png",
                )?;
                fs::write(
                    output.join(format!("page-{:04}.audit", page + 1)),
                    "original=1x1;rendered=1x1",
                )?;
            }
            return Ok(());
        }
        "missing-page" => {
            for page in start..end {
                if page == start {
                    continue;
                }
                write_valid_page(output, page)?;
            }
            return Ok(());
        }
        "part-file" => {
            for page in start..end {
                write_valid_page(output, page)?;
            }
            fs::write(output.join(".page-0001.part.png"), b"leftover")?;
            return Ok(());
        }
        _ => {}
    }
    for page in start..end {
        write_valid_page(output, page)?;
    }
    Ok(())
}

fn write_valid_page(output: &Path, page: usize) -> Result<(), Box<dyn std::error::Error>> {
    let image = image::RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]));
    image.save(output.join(format!("page-{:04}.png", page + 1)))?;
    fs::write(
        output.join(format!("page-{:04}.audit", page + 1)),
        "original=1x1;rendered=1x1",
    )?;
    Ok(())
}
