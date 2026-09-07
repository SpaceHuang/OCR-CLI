use std::env;
use std::path::PathBuf;

use ocrx_core::QualityTier;
use ocrx_pdf::{PdfiumRenderer, RenderOptions, classify_page, validate_native_candidate};

fn page_probe_json(page: usize, width: u32, height: u32) -> String {
    format!(
        "{{\"page\":{},\"width\":{},\"height\":{},\"color\":\"RGB8\"}}",
        page + 1,
        width,
        height
    )
}

fn document_probe_json(pages: usize, elapsed_ms: u128) -> String {
    format!("{{\"pages\":{pages},\"elapsed_ms\":{elapsed_ms}}}")
}

fn json_escape(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '"' => "\\\"".to_owned(),
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            c if c.is_control() => format!("\\u{:04x}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

fn native_candidate_json(page: usize, native: &ocrx_pdf::NativePage) -> String {
    let blocks = native.blocks.iter().map(|block| format!(
        "{{\"text\":\"{}\",\"bbox\":{{\"left\":{},\"top\":{},\"right\":{},\"bottom\":{}}},\"source_order\":{},\"source\":\"pdf_text\"}}",
        json_escape(&block.text), block.bbox.left, block.bbox.top, block.bbox.right,
        block.bbox.bottom, block.order
    )).collect::<Vec<_>>().join(",");
    format!(
        "{{\"schema_version\":2,\"page_number\":{},\"native_text\":\"{}\",\"blocks\":[{}],\"signals\":{{\"page_width\":{},\"page_height\":{},\"image_objects\":{},\"text_bbox_coverage\":{},\"visible_characters\":{},\"unicode_validity\":{},\"text_coverage\":{},\"image_coverage\":{},\"duplicate_text\":{},\"geometry_anomaly\":{}}},\"candidate_status\":\"raw_native_facts_requires_l2_validation\"}}",
        page + 1,
        json_escape(&native.text),
        blocks,
        native.signals.page_width,
        native.signals.page_height,
        native.signals.image_objects,
        native.signals.text_bbox_coverage,
        native.signals.visible_characters,
        native.signals.unicode_validity,
        native.signals.text_coverage,
        native.signals.image_coverage,
        native.signals.duplicate_text,
        native.signals.geometry_anomaly
    )
}

#[allow(clippy::too_many_arguments)]
fn native_probe_json(
    page: usize,
    text_chars: usize,
    blocks: usize,
    visible_chars: usize,
    unicode_validity: f32,
    text_bbox_coverage: f32,
    text_coverage: f32,
    image_coverage: f32,
    image_objects: usize,
    duplicate_text: bool,
    geometry_anomaly: bool,
) -> String {
    format!(
        "{{\"page\":{},\"native_text_chars\":{},\"native_blocks\":{},\"visible_chars\":{},\"unicode_validity\":{unicode_validity:.6},\"text_bbox_coverage\":{text_bbox_coverage:.6},\"text_coverage\":{text_coverage:.6},\"image_coverage\":{image_coverage:.6},\"image_objects\":{},\"duplicate_text_geometry\":{},\"geometry_anomaly\":{}}}",
        page + 1,
        text_chars,
        blocks,
        visible_chars,
        image_objects,
        duplicate_text,
        geometry_anomaly
    )
}

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
    let json = env::var_os("OCRX_PDFIUM_JSON").is_some();
    if let Some(directory) = &save_dir {
        std::fs::create_dir_all(directory)?;
    }
    if env::var_os("OCRX_PDFIUM_COMPARE_NATIVE").is_some() {
        let pages = renderer.page_count(&input)?;
        for page in 0..pages {
            let signals = renderer.analyze_page_signals(&input, page)?;
            let classification = classify_page(signals, QualityTier::L2);
            let native = renderer.extract_native_page(&input, page)?;
            let validation = validate_native_candidate(page as u32 + 1, &native, QualityTier::L2)
                .map(|_| "ok".to_owned())
                .unwrap_or_else(|reason| reason.as_str().to_owned());
            println!(
                "{{\"page\":{},\"recommended_method\":\"{}\",\"validation\":\"{}\",\"visible_characters\":{},\"blocks\":{},\"text_bbox_coverage\":{:.6},\"duplicate_text\":{},\"geometry_anomaly\":{}}}",
                page + 1,
                classification.recommended_method.as_str(),
                validation,
                signals.visible_characters,
                native.blocks.len(),
                signals.text_bbox_coverage,
                signals.duplicate_text,
                signals.geometry_anomaly
            );
        }
        return Ok(());
    }
    if let Ok(page) = env::var("OCRX_PDFIUM_NATIVE_PAGE").and_then(|value| {
        value
            .parse::<usize>()
            .map_err(|_| std::env::VarError::NotPresent)
    }) {
        let native = renderer.extract_native_page(&input, page)?;
        if let Some(directory) = env::var_os("OCRX_PDFIUM_NATIVE_SAVE_DIR") {
            let directory = PathBuf::from(directory);
            std::fs::create_dir_all(&directory)?;
            std::fs::write(
                directory.join(format!("page-{:04}.json", page + 1)),
                native_candidate_json(page, &native),
            )?;
        }
        if json {
            println!(
                "{}",
                native_probe_json(
                    page,
                    native.text.chars().count(),
                    native.blocks.len(),
                    native.signals.visible_characters,
                    native.signals.unicode_validity,
                    native.signals.text_bbox_coverage,
                    native.signals.text_coverage,
                    native.signals.image_coverage,
                    native.signals.image_objects,
                    native.signals.duplicate_text,
                    native.signals.geometry_anomaly
                )
            );
        } else {
            println!(
                "page={} native_text_chars={} native_blocks={} visible_chars={} unicode_validity={:.6} text_bbox_coverage={:.6} text_coverage={:.6} image_coverage={:.6} image_objects={} duplicate_text_geometry={} geometry_anomaly={}",
                page + 1,
                native.text.chars().count(),
                native.blocks.len(),
                native.signals.visible_characters,
                native.signals.unicode_validity,
                native.signals.text_bbox_coverage,
                native.signals.text_coverage,
                native.signals.image_coverage,
                native.signals.image_objects,
                native.signals.duplicate_text,
                native.signals.geometry_anomaly
            );
        }
        return Ok(());
    }
    if let Ok(page) = env::var("OCRX_PDFIUM_PAGE").and_then(|value| {
        value
            .parse::<usize>()
            .map_err(|_| std::env::VarError::NotPresent)
    }) {
        let image = renderer.render_page_rgb8(&input, page, RenderOptions::default())?;
        if json {
            println!("{}", page_probe_json(page, image.width(), image.height()));
        } else {
            println!(
                "page={} pixels={}x{} color=RGB8",
                page + 1,
                image.width(),
                image.height()
            );
        }
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
            if !json && (index == 0 || index % 25 == 0) {
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
    let elapsed_ms = started.elapsed().as_millis();
    if json {
        println!("{}", document_probe_json(pages, elapsed_ms));
    } else {
        println!("pages={pages} elapsed_ms={elapsed_ms}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{document_probe_json, native_probe_json, page_probe_json};

    #[test]
    fn page_probe_json_is_stable_and_one_based() {
        assert_eq!(
            page_probe_json(0, 640, 480),
            "{\"page\":1,\"width\":640,\"height\":480,\"color\":\"RGB8\"}"
        );
    }

    #[test]
    fn document_probe_json_contains_machine_readable_summary() {
        assert_eq!(
            document_probe_json(3, 12),
            "{\"pages\":3,\"elapsed_ms\":12}"
        );
    }

    #[test]
    fn native_probe_json_is_stable_and_one_based() {
        assert_eq!(
            native_probe_json(0, 12, 2, 10, 1.0, 0.8, 0.9, 0.1, 0, false, false),
            "{\"page\":1,\"native_text_chars\":12,\"native_blocks\":2,\"visible_chars\":10,\"unicode_validity\":1.000000,\"text_bbox_coverage\":0.800000,\"text_coverage\":0.900000,\"image_coverage\":0.100000,\"image_objects\":0,\"duplicate_text_geometry\":false,\"geometry_anomaly\":false}"
        );
    }
}
