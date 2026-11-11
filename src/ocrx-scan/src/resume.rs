//! Resume plan and the two-phase OCR reuse protocol (plan §6.3 step 3/5,
//! T055; RBLK-2/4/6/7/8).
//!
//! Phase 1 (pre-compare) is a zero-load fast path over
//! [`ocrx_output::ocr_pre_compare`] with the read-only static identity; it
//! only decides whether prepare can be skipped. Phase 2 (final compare) is
//! authoritative and runs only when this document still has pages to
//! decode: the candidate pages are re-checked against the identity of the
//! session about to decode them.

use std::path::Path;

use ocrx_inference_api::{InferenceIdentity, StaticInferenceIdentity};
use ocrx_output::{
    artifacts_intact, ocr_final_compare, ocr_pre_compare, CheckpointFile, PdfIdentity,
    ScanCompatibility, ScanOcrSide, ScanRoute, V5Disposition, V6Checkpoint, V6Status,
};

/// What the resume planner decided for one checkpoint on disk.
#[derive(Debug, Clone, PartialEq)]
pub enum PageDisposition {
    /// Committed artifact reuse (identity-compatible and intact).
    Reuse {
        think_blocks_stripped: u32,
        truncated: bool,
        warnings: Vec<String>,
    },
    /// Recoverable pending OCR (re-queued; executed under a fresh v6
    /// identity later).
    PendingOcr,
    /// Reprocess: identity mismatch, artifact damage, degeneration, or an
    /// unmappable legacy format.
    Reprocess { reason: &'static str },
}

/// Resume inputs shared by every classification call for one document.
#[derive(Debug, Clone)]
pub struct ResumeContext<'a> {
    pub input_sha256: &'a str,
    pub pdf: &'a PdfIdentity,
    pub side: &'a ScanOcrSide,
    /// The static identity (prepare-free); `None` when the caller has no
    /// service at all (pure native-only runs).
    pub statik: Option<&'a StaticInferenceIdentity>,
    /// The identity of the session about to decode this document's pending
    /// pages; `None` until prepare succeeded.
    pub session_identity: Option<&'a InferenceIdentity>,
    /// Whether this document still has pages that will be decoded (RBLK-8:
    /// the final comparison applies only then).
    pub has_pending_decode: bool,
}

/// Classifies one checkpoint file for resume (T055).
#[must_use]
pub fn classify(
    file: &CheckpointFile,
    root: &Path,
    context: &ResumeContext<'_>,
) -> PageDisposition {
    match file {
        CheckpointFile::LegacyUnsupported { .. } => PageDisposition::Reprocess {
            reason: "unsupported checkpoint version",
        },
        CheckpointFile::V5(disposition) => classify_v5(disposition, root, context),
        CheckpointFile::V6(checkpoint) => classify_v6(checkpoint, root, context),
    }
}

fn classify_v5(
    disposition: &V5Disposition,
    root: &Path,
    context: &ResumeContext<'_>,
) -> PageDisposition {
    match disposition {
        V5Disposition::Reprocess { reason, .. } => PageDisposition::Reprocess { reason },
        V5Disposition::Complete(checkpoint) => classify_v6_complete(checkpoint, root, context),
        V5Disposition::PendingOcr(checkpoint) => {
            // Pending pages are recoverable when the public PDF identity is
            // compatible; execution happens under the fresh v6 identity.
            if checkpoint.compatibility.matches_native(&expected_compat(context)) {
                PageDisposition::PendingOcr
            } else {
                PageDisposition::Reprocess {
                    reason: "v5 pending identity mismatch",
                }
            }
        }
    }
}

fn expected_compat(context: &ResumeContext<'_>) -> ScanCompatibility {
    ScanCompatibility::native_or_empty(
        context.input_sha256,
        ScanRoute::NativeText,
        context.pdf.clone(),
    )
}

fn classify_v6(
    checkpoint: &V6Checkpoint,
    root: &Path,
    context: &ResumeContext<'_>,
) -> PageDisposition {
    match checkpoint.status {
        V6Status::PendingOcr => {
            if checkpoint.compatibility.matches_native(&expected_compat(context)) {
                PageDisposition::PendingOcr
            } else {
                PageDisposition::Reprocess {
                    reason: "pending identity mismatch",
                }
            }
        }
        V6Status::Complete => classify_v6_complete(checkpoint, root, context),
    }
}

fn classify_v6_complete(
    checkpoint: &V6Checkpoint,
    root: &Path,
    context: &ResumeContext<'_>,
) -> PageDisposition {
    // Degenerated pages are never reused (any identity combination).
    if checkpoint.degenerated {
        return PageDisposition::Reprocess {
            reason: "degenerated page",
        };
    }
    // Route-scoped identity check first: native/empty use the frozen
    // five-field scope; OCR pages go through the two-phase protocol.
    if checkpoint.compatibility.route != ScanRoute::Ocr {
        if checkpoint.compatibility.matches_native(&expected_compat(context))
            && artifacts_intact(checkpoint, root)
        {
            return PageDisposition::Reuse {
                think_blocks_stripped: checkpoint.think_blocks_stripped,
                truncated: checkpoint.truncated,
                warnings: checkpoint.warnings.clone(),
            };
        }
        return PageDisposition::Reprocess {
            reason: "native identity mismatch or damaged artifacts",
        };
    }
    // --- Two-phase OCR protocol ---
    let Some(statik) = context.statik else {
        return PageDisposition::Reprocess {
            reason: "no static identity for OCR pre-compare",
        };
    };
    // Phase 1: static pre-compare (zero-load fast path).
    if !ocr_pre_compare(&checkpoint.compatibility, statik, context.side) {
        return PageDisposition::Reprocess {
            reason: "static pre-compare failed",
        };
    }
    // Artifact integrity gate (independent of identity).
    if !artifacts_intact(checkpoint, root) {
        return PageDisposition::Reprocess {
            reason: "damaged OCR artifacts",
        };
    }
    // Phase 2: final comparison — authoritative, ONLY when this document
    // will decode pages (RBLK-8). A fully-reused document keeps the stored
    // identity as terminal: zero decoding, no session required, and a
    // shared session from an earlier document changes nothing.
    if !context.has_pending_decode {
        return PageDisposition::Reuse {
            think_blocks_stripped: checkpoint.think_blocks_stripped,
            truncated: checkpoint.truncated,
            warnings: checkpoint.warnings.clone(),
        };
    }
    match context.session_identity {
        Some(identity) => {
            if ocr_final_compare(&checkpoint.compatibility, identity) {
                PageDisposition::Reuse {
                    think_blocks_stripped: checkpoint.think_blocks_stripped,
                    truncated: checkpoint.truncated,
                    warnings: checkpoint.warnings.clone(),
                }
            } else {
                // Includes the auto-drift case: the pre-compare resolved a
                // different profile than prepare did — every inconsistent
                // candidate reprocesses, never a mixed-profile document.
                PageDisposition::Reprocess {
                    reason: "final execution identity mismatch",
                }
            }
        }
        None => PageDisposition::Reprocess {
            reason: "pending decode without a prepared session",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{identity, pdf_identity, side, v5_ocr_complete, v5_pending, v6_ocr};
    use std::path::PathBuf;

    fn context<'a>(
        statik: Option<&'a StaticInferenceIdentity>,
        session: Option<&'a InferenceIdentity>,
        has_pending: bool,
    ) -> ResumeContext<'a> {
        // The pdf/side fixtures live as OnceLock singletons for the test
        // lifetime so the borrows stay valid.
        use std::sync::OnceLock;
        static PDF: OnceLock<PdfIdentity> = OnceLock::new();
        static SIDE: OnceLock<ScanOcrSide> = OnceLock::new();
        let pdf = PDF.get_or_init(pdf_identity);
        let side = SIDE.get_or_init(side);
        ResumeContext {
            input_sha256: "hash",
            pdf,
            side,
            statik,
            session_identity: session,
            has_pending_decode: has_pending,
        }
    }

    fn v6_native(complete: bool) -> V6Checkpoint {
        let compatibility =
            ScanCompatibility::native_or_empty("hash", ScanRoute::NativeText, pdf_identity());
        V6Checkpoint {
            page_number: 1,
            status: if complete {
                V6Status::Complete
            } else {
                V6Status::PendingOcr
            },
            compatibility,
            output_sha256: String::new(),
            truncated: false,
            degenerated: false,
            attempts: 1,
            raw: None,
            warnings: vec![],
            think_blocks_stripped: 0,
            audit: None,
        }
    }

    #[test]
    fn native_complete_reuses_when_intact() {
        let root = std::env::temp_dir().join(format!("ocrx-resume-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("page-0001.md"), "body").unwrap();
        let mut checkpoint = v6_native(true);
        checkpoint.output_sha256 = ocrx_primitives::sha256(b"body");
        let current = identity();
        let statik = StaticInferenceIdentity::from_execution(&current);
        // Native reuse ignores the OCR session entirely.
        let disposition =
            classify(&CheckpointFile::V6(checkpoint), &root, &context(Some(&statik), None, false));
        assert!(matches!(disposition, PageDisposition::Reuse { .. }));
        // Damaged artifact (hash mismatch) → reprocess.
        std::fs::write(root.join("page-0001.md"), "tampered").unwrap();
        let context = context(Some(&statik), None, false);
        let disposition = classify(&CheckpointFile::V6(v6_native(true)), &root, &context);
        assert!(matches!(disposition, PageDisposition::Reprocess { .. }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn degenerated_pages_never_reuse() {
        let root = PathBuf::from("nowhere");
        let mut checkpoint = v6_native(true);
        checkpoint.degenerated = true;
        let current = identity();
        let statik = StaticInferenceIdentity::from_execution(&current);
        assert!(matches!(
            classify(
                &CheckpointFile::V6(checkpoint),
                &root,
                &context(None, None, false)
            ),
            PageDisposition::Reprocess { reason: "degenerated page" }
        ));
    }

    #[test]
    fn ocr_reuse_without_pending_decode_needs_no_session_rblk8() {
        // Fully-reused document: pre-compare passes, no pending decode →
        // reuse with ZERO prepare/load, even though no session exists.
        let root = std::env::temp_dir().join(format!("ocrx-resume-ocr-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("page-0002.md"), "ocr body").unwrap();
        let mut checkpoint = v6_ocr(2);
        checkpoint.output_sha256 = ocrx_primitives::sha256(b"ocr body");
        let current = identity();
        let statik = StaticInferenceIdentity::from_execution(&current);
        let context = context(Some(&statik), None, false);
        let disposition = classify(&CheckpointFile::V6(checkpoint), &root, &context);
        assert!(matches!(disposition, PageDisposition::Reuse { .. }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ocr_with_pending_decode_requires_the_final_compare() {
        let root = std::env::temp_dir().join(format!("ocrx-resume-fin-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("page-0002.md"), "ocr body").unwrap();
        let mut checkpoint = v6_ocr(2);
        checkpoint.output_sha256 = ocrx_primitives::sha256(b"ocr body");
        let current = identity();
        let statik = StaticInferenceIdentity::from_execution(&current);
        // Pending decode but no session yet → reprocess (cannot authorize).
        assert!(matches!(
            classify(&CheckpointFile::V6(checkpoint.clone()), &root, &context(Some(&statik), None, true)),
            PageDisposition::Reprocess { .. }
        ));
        // Matching session identity → reuse.
        assert!(matches!(
            classify(&CheckpointFile::V6(checkpoint.clone()), &root, &context(Some(&statik), Some(&current), true)),
            PageDisposition::Reuse { .. }
        ));
        // Actual execution change (batch depth 16 run) → reprocess (RBLK-6).
        let mut drifted = identity();
        drifted.batch_depth = 16;
        assert!(matches!(
            classify(&CheckpointFile::V6(checkpoint.clone()), &root, &context(Some(&statik), Some(&drifted), true)),
            PageDisposition::Reprocess { reason: "final execution identity mismatch" }
        ));
        // Display-only change → still reuse (RBLK-7 discriminating test).
        let mut relabeled = identity();
        relabeled.display.quantization = "Q4_K_X".to_owned();
        assert!(matches!(
            classify(&CheckpointFile::V6(checkpoint), &root, &context(Some(&statik), Some(&relabeled), true)),
            PageDisposition::Reuse { .. }
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn v5_ocr_complete_pages_reprocess_and_pending_recovers() {
        let root = PathBuf::from("nowhere");
        assert!(matches!(
            classify(
                &CheckpointFile::V5(v5_ocr_complete(3)),
                &root,
                &context(None, None, true)),
            PageDisposition::Reprocess { .. }
        ));
        assert!(matches!(
            classify(
                &CheckpointFile::V5(v5_pending(4)),
                &root,
                &context(None, None, true)),
            PageDisposition::PendingOcr
        ));
    }

    #[test]
    fn legacy_versions_are_never_reused() {
        let root = PathBuf::from("nowhere");
        assert!(matches!(
            classify(
                &CheckpointFile::LegacyUnsupported { page_hint: Some(1) },
                &root,
                &context(None, None, false)
            ),
            PageDisposition::Reprocess { reason: "unsupported checkpoint version" }
        ));
    }
}
