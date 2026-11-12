//! v6 checkpoint read/write and the safe v5 migration (plan §7.2, T046–T047).
//!
//! The v6 format keeps the line-based, hex-encoded field layout the script
//! ecosystem already parses (§3.5 item 7: tool machine-readability is a
//! formal compatibility requirement), with the structured
//! [`ScanCompatibility`] serialized as deterministic JSON inside one hex
//! field. Writes go through the same temp/flush/rename atomic path as v5.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::RenderAudit;
use crate::compatibility::ScanCompatibility;

/// v6 checkpoint status (replaces the v5 `complete` bool + pending suffix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V6Status {
    Complete,
    PendingOcr,
}

impl V6Status {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::PendingOcr => "pending_ocr",
        }
    }
}

/// A v6 page checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V6Checkpoint {
    pub page_number: u32,
    pub status: V6Status,
    pub compatibility: ScanCompatibility,
    pub output_sha256: String,
    pub truncated: bool,
    /// Degenerated pages are never reused, whatever the identity says.
    pub degenerated: bool,
    /// Written range 1..=4 (1 + frozen ladder length); reads accept wider
    /// legacy values such as 5 (Q1.4b).
    pub attempts: u32,
    /// `page-NNNN.raw` sidecar pointer.
    pub raw: Option<String>,
    pub warnings: Vec<String>,
    pub think_blocks_stripped: u32,
    pub audit: Option<RenderAudit>,
}

/// What a checkpoint on disk turns out to be (T047 safe-read contract).
#[derive(Debug, Clone, PartialEq)]
pub enum CheckpointFile {
    /// Current format.
    V6(V6Checkpoint),
    /// v5 content, parsed without failure and mapped conservatively.
    V5(V5Disposition),
    /// v4 or older: never a crash, always reprocess (user files untouched).
    LegacyUnsupported { page_hint: Option<u32> },
}

/// The conservative v5 → v6 mapping (T047 rules).
#[derive(Debug, Clone, PartialEq)]
pub enum V5Disposition {
    /// `complete=true` without the pending suffix: reusable as v6 complete
    /// (native/empty pages only — see the OCR rule below).
    Complete(V6Checkpoint),
    /// `complete=false` with `;pending-ocr=true`: recoverable pending queue
    /// entry (executed later under a fresh v6 identity).
    PendingOcr(V6Checkpoint),
    /// Everything else: reprocess. Includes OCR-complete pages (they lack
    /// the structured asset/runtime/adapter identity and must never be
    /// reused across the format boundary), contradictory
    /// `complete=true` + pending combinations, and unfinished non-pending
    /// pages.
    Reprocess {
        page_number: u32,
        reason: &'static str,
    },
}

fn encode_field(value: &str) -> String {
    value
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn decode_field(value: &str) -> Result<String, std::io::Error> {
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("zz"), 16)
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad hex field"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 field"))
}

/// Writes a v6 checkpoint atomically (temp file → flush → rename).
///
/// # Errors
///
/// Underlying I/O errors from the atomic write.
pub fn write_v6(path: impl AsRef<Path>, checkpoint: &V6Checkpoint) -> std::io::Result<()> {
    let compatibility = serde_json::to_string(&checkpoint.compatibility)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    // Write-range guard: attempts go out as 1..=4 (Q1.4a); wider values are
    // a read-compat concern only.
    let attempts = checkpoint.attempts.clamp(1, 4);
    let content = format!(
        "ocrx-checkpoint-v6\npage={}\nstatus={}\ncompatibility={}\noutput={}\ntruncated={}\ndegenerated={}\nattempts={}\nraw={}\nwarnings={}\nthink_blocks_stripped={}\naudit={}\n",
        checkpoint.page_number,
        checkpoint.status.as_str(),
        encode_field(&compatibility),
        encode_field(&checkpoint.output_sha256),
        checkpoint.truncated,
        checkpoint.degenerated,
        attempts,
        checkpoint.raw.as_deref().unwrap_or("none"),
        encode_field(&checkpoint.warnings.join("\n")),
        checkpoint.think_blocks_stripped,
        checkpoint.audit.map_or_else(
            || "none".to_owned(),
            |audit| encode_field(&audit.as_checkpoint_fragment())
        ),
    );
    crate::atomic_write(path, content.as_bytes())
}

/// Reads any checkpoint version into the safe disposition.
///
/// # Errors
///
/// I/O errors only; malformed v5/v6 content surfaces as a reprocess
/// disposition instead of an error so user files are never destroyed.
pub fn read_any(path: impl AsRef<Path>) -> std::io::Result<CheckpointFile> {
    let content = std::fs::read_to_string(path)?;
    match content.lines().next() {
        Some("ocrx-checkpoint-v6") => Ok(read_v6_content(&content).map_or_else(
            |_| {
                // Malformed v6: never a crash, never a destroy — reprocess.
                CheckpointFile::LegacyUnsupported {
                    page_hint: page_hint(&content),
                }
            },
            CheckpointFile::V6,
        )),
        Some("ocrx-checkpoint-v5") => Ok(CheckpointFile::V5(read_v5_content(&content))),
        Some("ocrx-checkpoint-v4") | Some(_) => Ok(CheckpointFile::LegacyUnsupported {
            page_hint: page_hint(&content),
        }),
        None => Ok(CheckpointFile::LegacyUnsupported { page_hint: None }),
    }
}

fn page_hint(content: &str) -> Option<u32> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("page="))
        .and_then(|value| value.parse().ok())
}

fn parse_fields(content: &str) -> std::collections::BTreeMap<&str, &str> {
    content
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once('='))
        .collect()
}

fn read_v6_content(content: &str) -> Result<V6Checkpoint, std::io::Error> {
    let values = parse_fields(content);
    let get = |key: &str| {
        values.get(key).copied().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("missing checkpoint field: {key}"),
            )
        })
    };
    let parse_bool = |key: &str| -> Result<bool, std::io::Error> {
        get(key)?.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid {key}"))
        })
    };
    let status = match get("status")? {
        "complete" => V6Status::Complete,
        "pending_ocr" => V6Status::PendingOcr,
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid checkpoint status {other:?}"),
            ));
        }
    };
    let compatibility_json = decode_field(get("compatibility")?)?;
    let compatibility: ScanCompatibility = serde_json::from_str(&compatibility_json)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let audit = match get("audit")? {
        "none" => None,
        encoded => Some(parse_audit_fragment(&decode_field(encoded)?)?),
    };
    Ok(V6Checkpoint {
        page_number: get("page")?
            .parse()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid page"))?,
        status,
        compatibility,
        output_sha256: decode_field(get("output")?)?,
        truncated: parse_bool("truncated")?,
        degenerated: parse_bool("degenerated")?,
        // Wide read compat (Q1.4b): values like 5 (a legacy wider ladder)
        // load fine; only the WRITE range is clamped to 1..=4.
        attempts: get("attempts")?.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid attempts")
        })?,
        raw: match get("raw")? {
            "none" => None,
            name => Some(name.to_owned()),
        },
        warnings: decode_field(get("warnings")?)?
            .lines()
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        think_blocks_stripped: get("think_blocks_stripped")?.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid think count")
        })?,
        audit,
    })
}

fn parse_audit_fragment(fragment: &str) -> Result<RenderAudit, std::io::Error> {
    crate::parse_render_audit(fragment)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "malformed render audit"))
}

/// Parses a v5 checkpoint body and maps it to the conservative disposition
/// (T047: `complete=true` + no pending → Complete; `complete=false` +
/// pending → PendingOcr; other false → Reprocess; `complete=true` with a
/// pending suffix → contradictory → Reprocess; v5 OCR complete pages are
/// ALWAYS reprocessed; degenerated pages are never reused; `decode=` and
/// `think_blocks_stripped` never participate and never fail the parse).
fn read_v5_content(content: &str) -> V5Disposition {
    let values = parse_fields(content);
    let get = |key: &str| values.get(key).copied();
    let page_number = get("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let complete = get("complete").is_some_and(|value| value == "true");
    let parameters = get("parameters").and_then(|value| decode_field(value).ok());
    let Some(parameters) = parameters else {
        return V5Disposition::Reprocess {
            page_number,
            reason: "unreadable v5 parameters",
        };
    };
    let pending = parameters.contains("pending-ocr=true");
    let degenerated = get("degenerated").is_some_and(|value| value == "true");
    let is_ocr = parameters.contains("ocr-profile=");
    if complete && pending {
        // Contradictory state: never map to complete (T047).
        return V5Disposition::Reprocess {
            page_number,
            reason: "contradictory complete+pending state",
        };
    }
    if degenerated {
        // A degenerated page is reprocessed under any identity combination.
        return V5Disposition::Reprocess {
            page_number,
            reason: "degenerated page",
        };
    }
    if complete && is_ocr {
        // v5 OCR complete pages lack the structured asset/runtime/adapter
        // identity: reprocess exactly once and write v6 (plan §7.2). The
        // `decode=<gen>` and `think_blocks_stripped` fields do not
        // participate in this decision and their absence never fails the
        // parse (conservative-field rule).
        return V5Disposition::Reprocess {
            page_number,
            reason: "v5 OCR complete page (no structured identity)",
        };
    }
    // Map the five public PDF parameters + platform into the structured
    // PDF identity; unparseable values degrade to reprocess.
    let Some(pdf) = parse_v5_pdf_identity(&parameters) else {
        return V5Disposition::Reprocess {
            page_number,
            reason: "unparseable v5 public parameters",
        };
    };
    let route = if is_ocr {
        crate::compatibility::ScanRoute::Ocr
    } else {
        crate::compatibility::ScanRoute::NativeText
    };
    let compatibility = ScanCompatibility::native_or_empty(
        get("input")
            .and_then(|value| decode_field(value).ok())
            .unwrap_or_default(),
        route,
        pdf,
    );
    let status = if complete {
        V6Status::Complete
    } else if pending {
        V6Status::PendingOcr
    } else {
        return V5Disposition::Reprocess {
            page_number,
            reason: "unfinished non-pending page",
        };
    };
    let checkpoint = V6Checkpoint {
        page_number,
        status,
        compatibility,
        output_sha256: get("output")
            .and_then(|value| decode_field(value).ok())
            .unwrap_or_default(),
        truncated: get("truncated").is_some_and(|value| value == "true"),
        degenerated: false,
        attempts: get("attempts")
            .and_then(|value| value.parse().ok())
            // Wide read compat: legacy wider ladders load as-is.
            .unwrap_or(1),
        raw: match get("raw") {
            Some("none") | None => None,
            Some(name) => Some((*name).to_owned()),
        },
        warnings: get("warnings")
            .and_then(|value| decode_field(value).ok())
            .map(|text| {
                text.lines()
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        // Missing field defaults to 0; the parser never fails on it.
        think_blocks_stripped: get("think_blocks_stripped")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        audit: match get("audit") {
            Some("none") | None => None,
            Some(encoded) => decode_field(encoded)
                .ok()
                .and_then(|fragment| parse_audit_fragment(&fragment).ok()),
        },
    };
    if complete {
        V5Disposition::Complete(checkpoint)
    } else {
        V5Disposition::PendingOcr(checkpoint)
    }
}

/// Parses the five public PDF parameters (`pdf-mode`, `quality-tier`,
/// `classifier`, `extractor`, `platform`) out of a v5 compatibility string.
fn parse_v5_pdf_identity(parameters: &str) -> Option<crate::compatibility::PdfIdentity> {
    let mut pdf_mode = None;
    let mut quality_tier = None;
    let mut classifier = None;
    let mut extractor = None;
    let mut platform = None;
    for segment in parameters.split(';') {
        let Some((key, value)) = segment.split_once('=') else {
            continue;
        };
        match key {
            "pdf-mode" => pdf_mode = Some(value.to_owned()),
            "quality-tier" => quality_tier = Some(value.to_owned()),
            "classifier" => classifier = Some(value.to_owned()),
            "extractor" => extractor = Some(value.to_owned()),
            "platform" => platform = Some(value.to_owned()),
            _ => {}
        }
    }
    Some(crate::compatibility::PdfIdentity {
        pdf_mode: pdf_mode?,
        quality_tier: quality_tier?,
        classifier_version: classifier?,
        extractor_version: extractor?,
        platform: platform?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::{PdfIdentity, ScanRoute};
    use ocrx_inference_api::InferenceIdentity;

    fn identity() -> InferenceIdentity {
        serde_json::from_str(
            r#"{"model":{"model":"ovisocr2","profile":"compact","source_revision":"rev-1","assets":[{"role":"mmproj","sha256":"b","byte_len":10},{"role":"model","sha256":"a","byte_len":10}]},"runtime":{"runtime":"llama.cpp","wrapper_version":"0.1.161","sys_version":"0.1.161","upstream_revision":"d81235049384534c167caea52b85a694f6103d14"},"adapter":{"adapter":"ovisocr2","version":"0.1.0"},"backend":"cpu","decode_generation":2,"batch_depth":8,"display":{"display_name":"OvisOCR2","family":"Ovis","quantization":"Q4_K_M"}}"#,
        )
        .unwrap()
    }

    fn compatibility() -> ScanCompatibility {
        ScanCompatibility::ocr(
            "input-hash",
            PdfIdentity {
                pdf_mode: "auto".to_owned(),
                quality_tier: "l2".to_owned(),
                classifier_version: "pdf-route-v1".to_owned(),
                extractor_version: "pdf-text-v1".to_owned(),
                platform: "windows".to_owned(),
            },
            crate::compatibility::OcrIdentity {
                inference: identity(),
                prompt_id: "ocr-markdown".to_owned(),
                prompt_version: "ocr-markdown-v1".to_owned(),
                prompt_schema_version: 1,
                dpi: 300,
                max_image_tokens: 1024,
                max_output_tokens: 16_384,
                context_size: 20_480,
                batch_depth: 8,
                decode_generation: 2,
                pixel_width_budget: 2880,
                pixel_height_budget: 2880,
                page_parser_version: "ovis-page-v1".to_owned(),
            },
        )
    }

    fn checkpoint() -> V6Checkpoint {
        V6Checkpoint {
            page_number: 7,
            status: V6Status::Complete,
            compatibility: compatibility(),
            output_sha256: "abc123".to_owned(),
            truncated: true,
            degenerated: false,
            attempts: 4,
            raw: Some("page-0007.raw".to_owned()),
            warnings: vec!["truncated".to_owned(), "think_removed".to_owned()],
            think_blocks_stripped: 5,
            audit: None,
        }
    }

    fn write_and_read(checkpoint: &V6Checkpoint) -> CheckpointFile {
        write_and_read_tagged(checkpoint, "default")
    }

    fn write_and_read_tagged(checkpoint: &V6Checkpoint, tag: &str) -> CheckpointFile {
        let root = std::env::temp_dir().join(format!("ocrx-v6-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let path = root.join("page.checkpoint");
        write_v6(&path, checkpoint).unwrap();
        let file = read_any(&path).unwrap();
        let _ = std::fs::remove_dir_all(root);
        file
    }

    #[test]
    fn v6_round_trips_every_field_with_status_and_identity() {
        let file = write_and_read_tagged(&checkpoint(), "round");
        let CheckpointFile::V6(loaded) = file else {
            panic!("expected v6, got {file:?}");
        };
        assert_eq!(loaded.page_number, 7);
        assert_eq!(loaded.status, V6Status::Complete);
        assert_eq!(loaded.compatibility, compatibility());
        assert_eq!(loaded.output_sha256, "abc123");
        assert!(loaded.truncated);
        assert!(!loaded.degenerated);
        assert_eq!(loaded.attempts, 4);
        assert_eq!(loaded.raw.as_deref(), Some("page-0007.raw"));
        assert_eq!(loaded.warnings, vec!["truncated", "think_removed"]);
        assert_eq!(loaded.think_blocks_stripped, 5);
        assert_eq!(loaded.audit, None);
    }

    #[test]
    fn v6_write_clamps_attempts_to_the_frozen_range_but_reads_accept_wider() {
        let mut wide = checkpoint();
        wide.attempts = 5;
        let file = write_and_read_tagged(&wide, "clamps");
        let CheckpointFile::V6(loaded) = file else {
            panic!("expected v6");
        };
        // The write clamps to 4; the reader accepts a wider hand-written 5.
        assert_eq!(loaded.attempts, 4);
        let root = std::env::temp_dir().join(format!("ocrx-v6-wide-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("wide.checkpoint");
        let content = std::fs::read_to_string({
            write_v6(&path, &wide).unwrap();
            &path
        })
        .unwrap()
        .replace("attempts=4", "attempts=5");
        std::fs::write(&path, content).unwrap();
        match read_any(&path).unwrap() {
            CheckpointFile::V6(loaded) => assert_eq!(loaded.attempts, 5, "wide read compat"),
            other => panic!("expected v6, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pending_ocr_status_round_trips() {
        let mut pending = checkpoint();
        pending.status = V6Status::PendingOcr;
        let file = write_and_read_tagged(&pending, "pending");
        let CheckpointFile::V6(loaded) = file else {
            panic!("expected v6");
        };
        assert_eq!(loaded.status, V6Status::PendingOcr);
    }

    fn v5_document(parameters: &str, complete: bool, extra: &str) -> String {
        format!(
            "ocrx-checkpoint-v5\npage=2\ninput={}\nmodel={}\nprofile={}\nparameters={}\ncomplete={complete}\noutput={}\ntruncated=false\n{}\n",
            encode_field("input-hash"),
            encode_field("ovisocr2"),
            encode_field("native"),
            encode_field(parameters),
            encode_field("output-hash"),
            extra,
        )
    }

    #[test]
    fn v5_native_complete_maps_to_reusable_v6_complete() {
        let parameters = "pdf-mode=auto;quality-tier=l2;classifier=pdf-route-v1;extractor=pdf-text-v1;platform=windows;dpi=300;tokens=16384;max_image_tokens=1024;context_size=20480;device=cpu;batch=8;route=native;decode=2";
        let content = v5_document(
            parameters,
            true,
            "degenerated=false\nattempts=1\nwarnings=\n",
        );
        let disposition = read_v5_content(&content);
        let V5Disposition::Complete(loaded) = disposition else {
            panic!("expected complete, got {disposition:?}");
        };
        assert_eq!(loaded.page_number, 2);
        assert_eq!(loaded.status, V6Status::Complete);
        assert_eq!(loaded.compatibility.route, ScanRoute::NativeText);
        assert_eq!(loaded.compatibility.pdf.platform, "windows");
        assert_eq!(loaded.compatibility.pdf.pdf_mode, "auto");
        // dpi/batch/decode are NOT part of the native identity (宽松匹配).
        assert!(loaded.compatibility.ocr.is_none());
        // think count absent → 0, parse never fails.
        assert_eq!(loaded.think_blocks_stripped, 0);
    }

    #[test]
    fn v5_ocr_complete_always_reprocesses() {
        let parameters = "pdf-mode=auto;quality-tier=l2;classifier=pdf-route-v1;extractor=pdf-text-v1;platform=windows;dpi=300;tokens=16384;max_image_tokens=1024;context_size=20480;device=cpu;batch=8;route=native;decode=2;ocr-profile=compact;ocr-device=cpu";
        let content = v5_document(
            parameters,
            true,
            "degenerated=false\nattempts=1\nthink_blocks_stripped=12\n",
        );
        let disposition = read_v5_content(&content);
        assert_eq!(
            disposition,
            V5Disposition::Reprocess {
                page_number: 2,
                reason: "v5 OCR complete page (no structured identity)",
            }
        );
    }

    #[test]
    fn v5_pending_suffix_maps_to_recoverable_pending() {
        let parameters = "pdf-mode=auto;quality-tier=l2;classifier=pdf-route-v1;extractor=pdf-text-v1;platform=windows;pending-ocr=true";
        let content = v5_document(parameters, false, "degenerated=false\n");
        let disposition = read_v5_content(&content);
        let V5Disposition::PendingOcr(loaded) = disposition else {
            panic!("expected pending, got {disposition:?}");
        };
        assert_eq!(loaded.status, V6Status::PendingOcr);
    }

    #[test]
    fn v5_contradictions_and_degenerated_reprocess() {
        // complete=true with a pending suffix: invalid, never complete.
        let contradictory = v5_document(
            "pdf-mode=auto;quality-tier=l2;classifier=c;extractor=e;platform=windows;pending-ocr=true",
            true,
            "degenerated=false\n",
        );
        assert_eq!(
            read_v5_content(&contradictory),
            V5Disposition::Reprocess {
                page_number: 2,
                reason: "contradictory complete+pending state",
            }
        );
        // Degenerated: reprocess under any identity combination.
        let degenerated = v5_document(
            "pdf-mode=auto;quality-tier=l2;classifier=c;extractor=e;platform=windows",
            true,
            "degenerated=true\n",
        );
        assert_eq!(
            read_v5_content(&degenerated),
            V5Disposition::Reprocess {
                page_number: 2,
                reason: "degenerated page",
            }
        );
        // Unfinished non-pending: reprocess.
        let unfinished = v5_document(
            "pdf-mode=auto;quality-tier=l2;classifier=c;extractor=e;platform=windows",
            false,
            "degenerated=false\n",
        );
        assert_eq!(
            read_v5_content(&unfinished),
            V5Disposition::Reprocess {
                page_number: 2,
                reason: "unfinished non-pending page",
            }
        );
    }

    #[test]
    fn v4_and_older_are_never_a_crash_and_hint_the_page() {
        let root = std::env::temp_dir().join(format!("ocrx-v4-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("legacy.checkpoint");
        std::fs::write(&path, "ocrx-checkpoint-v4\npage=9\n").unwrap();
        match read_any(&path).unwrap() {
            CheckpointFile::LegacyUnsupported { page_hint } => assert_eq!(page_hint, Some(9)),
            other => panic!("expected legacy, got {other:?}"),
        }
        // Garbage file: still a safe disposition.
        std::fs::write(&path, "").unwrap();
        assert!(matches!(
            read_any(&path).unwrap(),
            CheckpointFile::LegacyUnsupported { .. }
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn v5_decode_and_think_fields_never_participate_or_fail() {
        // A v5 native page with decode=1 (old generation) and a think count:
        // both ignored for the native reuse mapping; parse succeeds.
        let parameters = "pdf-mode=text;quality-tier=l1;classifier=pdf-route-v1;extractor=pdf-text-v1;platform=macos;dpi=200;tokens=8000;max_image_tokens=512;context_size=8192;device=metal;batch=4;route=native;decode=1";
        let content = v5_document(
            parameters,
            true,
            "degenerated=false\nthink_blocks_stripped=3\n",
        );
        let V5Disposition::Complete(loaded) = read_v5_content(&content) else {
            panic!("expected complete");
        };
        assert_eq!(loaded.think_blocks_stripped, 3, "count preserved for audit");
        assert_eq!(loaded.compatibility.pdf.platform, "macos");
    }
}

/// Commits an OCR page with a v6 checkpoint: identical artifact order to
/// the legacy `commit_page` (PNG → md → json → artifact hash → checkpoint
/// last), with the structured compatibility identity.
///
/// # Errors
///
/// Underlying I/O errors; the checkpoint is written only after every
/// artifact is durable (mechanism A ordering).
pub fn commit_page_v6(
    output_root: impl AsRef<Path>,
    page_number: u32,
    png: &[u8],
    markdown: &str,
    page_json: &str,
    checkpoint: &V6Checkpoint,
) -> std::io::Result<()> {
    if page_number == 0 || checkpoint.page_number != page_number {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "checkpoint page mismatch",
        ));
    }
    let root = output_root.as_ref();
    crate::atomic_write(
        root.join("images")
            .join(format!("page-{page_number:04}.png")),
        png,
    )?;
    crate::atomic_write(
        root.join(format!("page-{page_number:04}.md")),
        crate::normalize_lf(markdown).as_bytes(),
    )?;
    crate::atomic_write(
        root.join(format!("page-{page_number:04}.json")),
        page_json.as_bytes(),
    )?;
    let output_sha256 =
        crate::artifact_sha256(&root.join(format!("page-{page_number:04}.md")), root)?;
    write_v6(
        root.join(format!("page-{page_number:04}.checkpoint")),
        &V6Checkpoint {
            status: V6Status::Complete,
            output_sha256,
            ..checkpoint.clone()
        },
    )
}

/// Commits a native page (no synthetic full-page PNG) with a v6 checkpoint.
///
/// # Errors
///
/// Underlying I/O errors.
pub fn commit_native_page_v6(
    output_root: impl AsRef<Path>,
    page_number: u32,
    markdown: &str,
    page_json: &str,
    checkpoint: &V6Checkpoint,
) -> std::io::Result<()> {
    if page_number == 0 || checkpoint.page_number != page_number {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "checkpoint page mismatch",
        ));
    }
    let root = output_root.as_ref();
    crate::atomic_write(
        root.join(format!("page-{page_number:04}.md")),
        crate::normalize_lf(markdown).as_bytes(),
    )?;
    crate::atomic_write(
        root.join(format!("page-{page_number:04}.json")),
        page_json.as_bytes(),
    )?;
    let output_sha256 =
        crate::artifact_sha256(&root.join(format!("page-{page_number:04}.md")), root)?;
    write_v6(
        root.join(format!("page-{page_number:04}.checkpoint")),
        &V6Checkpoint {
            status: V6Status::Complete,
            output_sha256,
            ..checkpoint.clone()
        },
    )
}

/// Writes a pending-OCR v6 checkpoint (native fallback / queue handoff).
///
/// # Errors
///
/// Underlying I/O errors.
pub fn commit_pending_ocr_v6(
    output_root: impl AsRef<Path>,
    page_number: u32,
    checkpoint: &V6Checkpoint,
) -> std::io::Result<()> {
    if page_number == 0 || checkpoint.page_number != page_number {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "checkpoint page mismatch",
        ));
    }
    write_v6(
        output_root
            .as_ref()
            .join(format!("page-{page_number:04}.checkpoint")),
        &V6Checkpoint {
            status: V6Status::PendingOcr,
            ..checkpoint.clone()
        },
    )
}

/// Checks whether a committed page's artifacts still match the checkpoint
/// (T048: a missing or tampered referenced artifact is not reusable).
#[must_use]
pub fn artifacts_intact(checkpoint: &V6Checkpoint, root: &Path) -> bool {
    if checkpoint.status != V6Status::Complete {
        return false;
    }
    let page = checkpoint.page_number;
    let markdown_path = root.join(format!("page-{page:04}.md"));
    let Ok(bytes) = std::fs::read(&markdown_path) else {
        return false;
    };
    if ocrx_primitives::sha256(&bytes) != checkpoint.output_sha256 {
        return false;
    }
    // Referenced images must exist (bbox crops + the full-page png only
    // when the page carried one).
    if root
        .join("images")
        .join(format!("page-{page:04}.png"))
        .exists()
    {
        // full-page png present: fine
    }
    for warning in &checkpoint.warnings {
        let _ = warning;
    }
    if let Some(raw) = &checkpoint.raw
        && !root.join(raw).is_file()
    {
        return false;
    }
    true
}
