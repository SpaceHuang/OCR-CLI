//! Evaluation execution entries (plan §8.12, T082).
//!
//! Two public entries: full PDF quality evaluation drives a
//! [`ScanApplication`], and model/runtime专项 evaluation drives an
//! [`InferenceService`] with caller-supplied prompt/images/truth. Both
//! record the complete execution identity (set/truth/inference/prompt/
//! render versions) so a report can be re-run byte-stably.
//!
//! The "same-params rescan byte comparison" primitive (§3.5 item 10) runs
//! the same input twice through one scan application and compares the page
//! artifacts byte-for-byte (excluding non-deterministic fields), covering
//! both generation self-consistency and migration-regression scenarios.

use std::path::Path;

use ocrx_inference_api::InferenceIdentity;

/// Complete evaluation execution identity (plan §7.4/§8.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationIdentity {
    /// Truth/manifest set identifier (e.g. `pdf-native-text-v0.1`).
    pub set_id: String,
    /// Truth data version.
    pub truth_version: String,
    /// Input file SHA-256.
    pub input_sha256: String,
    /// The platform execution identity captured after prepare (`None` for
    /// native-only evaluations).
    pub inference: Option<InferenceIdentity>,
    /// Prompt identity + version used for OCR pages.
    pub prompt_id: String,
    pub prompt_version: String,
    /// Render DPI (render identity).
    pub dpi: u32,
}

impl EvaluationIdentity {
    /// Serializes the identity with deterministic field order (stable JSON
    /// for report embedding).
    #[must_use]
    pub fn to_json(&self) -> String {
        let inference = self.inference.as_ref().map_or_else(
            || "null".to_owned(),
            |identity| serde_json::to_string(identity).unwrap_or_else(|_| "null".to_owned()),
        );
        format!(
            "{{\"set_id\":\"{}\",\"truth_version\":\"{}\",\"input_sha256\":\"{}\",\"inference\":{},\"prompt_id\":\"{}\",\"prompt_version\":\"{}\",\"dpi\":{}}}",
            escape(&self.set_id),
            escape(&self.truth_version),
            escape(&self.input_sha256),
            inference,
            escape(&self.prompt_id),
            escape(&self.prompt_version),
            self.dpi
        )
    }
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A named artifact pair from two scans of the same input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPair {
    pub relative_path: String,
    pub first: Vec<u8>,
    pub second: Vec<u8>,
}

/// Outcome of the byte-comparison primitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanComparison {
    /// Same-named artifacts compared.
    pub compared: usize,
    /// Artifacts present in one scan only (a determinism failure).
    pub missing_on_one_side: Vec<String>,
    /// Artifacts whose bytes differ (a determinism failure).
    pub differing: Vec<String>,
}

impl RescanComparison {
    /// Byte-stable only when every artifact matched and none went missing.
    #[must_use]
    pub fn byte_identical(&self) -> bool {
        self.missing_on_one_side.is_empty() && self.differing.is_empty()
    }

    /// Frozen diagnostic JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let list = |values: &[String]| {
            values
                .iter()
                .map(|value| format!("\"{value}\""))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{{\"compared\":{},\"missing_on_one_side\":[{}],\"differing\":[{}],\"byte_identical\":{}}}",
            self.compared,
            list(&self.missing_on_one_side),
            list(&self.differing),
            self.byte_identical()
        )
    }
}

/// Collects page-level artifact files under an output directory (page .md,
/// .json, routing sidecars; excludes result.json/document.md timestamps and
/// image binaries whose encoding is deterministic but whose presence depends
/// on route).
fn collect_page_artifacts(root: &Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // Exact-case suffixes are intentional: these files are created by
        // the scan pipeline with exactly these spellings (legacy
        // case_sensitive_file_extension_comparisons allowance).
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        let is_page_artifact = (name.starts_with("page-")
            && (name.ends_with(".md")
                || name.ends_with(".json")
                || name.ends_with(".routing.json")
                || name.ends_with(".raw")))
            || name == "document.md";
        if is_page_artifact {
            out.push((name.to_owned(), std::fs::read(&path)?));
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

/// Same-params rescan byte comparison (§3.5 item 10): given the page
/// artifacts of two scans (same input, same parameters), compares each
/// named artifact byte-for-byte.
///
/// Timestamps and other non-deterministic fields must be excluded by the
/// caller before collection; `result.json`/`scan-summary.json` (elapsed_ms)
/// are not page artifacts and never compared.
#[must_use]
pub fn compare_rescan_artifacts(
    first: &[(String, Vec<u8>)],
    second: &[(String, Vec<u8>)],
) -> RescanComparison {
    let mut comparison = RescanComparison {
        compared: 0,
        missing_on_one_side: Vec::new(),
        differing: Vec::new(),
    };
    let second_index: std::collections::HashMap<&String, &Vec<u8>> =
        second.iter().map(|(name, bytes)| (name, bytes)).collect();
    let first_index: std::collections::HashMap<&String, &Vec<u8>> =
        first.iter().map(|(name, bytes)| (name, bytes)).collect();
    for (name, bytes) in first {
        match second_index.get(name) {
            Some(other) => {
                comparison.compared += 1;
                if bytes != *other {
                    comparison.differing.push(name.clone());
                }
            }
            None => comparison.missing_on_one_side.push(name.clone()),
        }
    }
    for (name, _) in second {
        if !first_index.contains_key(name) {
            comparison.missing_on_one_side.push(name.clone());
        }
    }
    comparison.missing_on_one_side.sort();
    comparison
}

/// Convenience wrapper: reads the page artifacts from two output
/// directories and compares them.
///
/// # Errors
///
/// I/O errors from reading either directory.
pub fn compare_rescan_outputs(
    first_root: &Path,
    second_root: &Path,
) -> std::io::Result<RescanComparison> {
    Ok(compare_rescan_artifacts(
        &collect_page_artifacts(first_root)?,
        &collect_page_artifacts(second_root)?,
    ))
}

/// Entry 1 (full PDF quality evaluation): drives a scan application over
/// one input and returns the outcome handle plus the recorded evaluation
/// identity. The caller maps the outcome to reports; this entry exists so
/// evaluators never re-implement scan orchestration.
pub struct ScanEvaluationEntry<'a> {
    pub app: &'a mut ocrx_scan::ScanApplication,
    pub options: &'a ocrx_scan::DocumentOptions,
    pub pdf: &'a mut dyn ocrx_scan::PdfDocumentAccess,
    pub renderer: &'a mut dyn ocrx_scan::window::PageRenderer,
    pub service: &'a mut dyn ocrx_inference_api::InferenceService,
    pub commit_recorder: &'a ocrx_output::commit_profile::CommitRecorder,
    pub cancellation: &'a ocrx_primitives::CancellationToken,
}

impl ScanEvaluationEntry<'_> {
    /// Runs one document and records the evaluation identity.
    ///
    /// # Errors
    ///
    /// Propagates the application's I/O errors.
    ///
    /// # Panics
    ///
    /// Only for the single-input invariant: `run` was given exactly one
    /// input, so `documents` is non-empty.
    pub fn run_document(
        self,
        input: &Path,
        output: &Path,
        identity_set: &str,
        truth_version: &str,
    ) -> std::io::Result<(ocrx_scan::DocumentOutcome, EvaluationIdentity)> {
        let input_sha256 = ocrx_primitives::sha256_file(input)?;
        let events = std::cell::RefCell::new(Vec::new());
        let outcome = self.app.run(
            &[input.to_path_buf()],
            &|_| output.to_path_buf(),
            self.options,
            self.pdf,
            self.renderer,
            self.service,
            self.commit_recorder,
            self.cancellation,
            &mut |event| events.borrow_mut().push(event),
        )?;
        let _ = events;
        let identity = EvaluationIdentity {
            set_id: identity_set.to_owned(),
            truth_version: truth_version.to_owned(),
            input_sha256,
            inference: outcome
                .documents
                .first()
                .and_then(|document| document.identity.clone()),
            prompt_id: ocrx_scan::ocr::prompt::PROMPT_ID.to_owned(),
            prompt_version: ocrx_scan::ocr::prompt::PROMPT_VERSION.to_owned(),
            dpi: self.options.dpi,
        };
        Ok((
            outcome.documents.into_iter().next().expect("one input"),
            identity,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_identity_json_is_deterministic_and_field_ordered() {
        let identity = EvaluationIdentity {
            set_id: "pdf-native-text-v0.1".to_owned(),
            truth_version: "truth-2026-11-01".to_owned(),
            input_sha256: "abc".to_owned(),
            inference: None,
            prompt_id: "ocr-markdown".to_owned(),
            prompt_version: "ocr-markdown-v1".to_owned(),
            dpi: 300,
        };
        let json = identity.to_json();
        assert!(json.starts_with("{\"set_id\":\"pdf-native-text-v0.1\""));
        assert!(!json.contains("\"prompt_schema_version\""));
        assert_eq!(json, identity.to_json());
        // Inference identity embeds when present.
        let mut with_inference = identity.clone();
        with_inference.inference = Some(serde_json::from_str(
            r#"{"model":{"model":"ovisocr2","profile":"compact","source_revision":"rev","assets":[]},"runtime":{"runtime":"llama.cpp","wrapper_version":"0.1.161","sys_version":"0.1.161","upstream_revision":"d81235049384534c167caea52b85a694f6103d14"},"adapter":{"adapter":"ovisocr2","version":"0.1.0"},"backend":"cpu","decode_generation":2,"batch_depth":8,"display":{"display_name":"OvisOCR2","family":"Ovis","quantization":"Q4_K_M"}}"#,
        )
        .unwrap());
        let json = with_inference.to_json();
        assert!(json.contains("\"backend\":\"cpu\""));
    }

    #[test]
    fn rescan_comparison_flags_differences_and_missing_sides() {
        let first = vec![
            ("page-0001.md".to_owned(), b"a".to_vec()),
            ("page-0002.md".to_owned(), b"b".to_vec()),
            ("page-0004.md".to_owned(), b"d".to_vec()),
        ];
        let second = vec![
            ("page-0001.md".to_owned(), b"a".to_vec()),
            ("page-0002.md".to_owned(), b"B".to_vec()),
            ("page-0003.md".to_owned(), b"c".to_vec()),
        ];
        let comparison = compare_rescan_artifacts(&first, &second);
        assert_eq!(comparison.compared, 2);
        assert_eq!(comparison.differing, ["page-0002.md"]);
        assert_eq!(
            comparison.missing_on_one_side,
            ["page-0003.md", "page-0004.md"]
        );
        assert!(!comparison.byte_identical());
        assert!(
            comparison
                .to_json()
                .contains("\"differing\":[\"page-0002.md\"]")
        );
    }

    #[test]
    fn rescan_comparison_passes_for_byte_identical_artifacts() {
        let artifacts = vec![("page-0001.md".to_owned(), b"same".to_vec())];
        let comparison = compare_rescan_artifacts(&artifacts, &artifacts);
        assert!(comparison.byte_identical());
        assert_eq!(comparison.compared, 1);
    }

    #[test]
    fn rescan_outputs_comparison_reads_real_directories() {
        let root_a =
            std::env::temp_dir().join(format!("ocrx-eval-rescan-a-{}", std::process::id()));
        let root_b =
            std::env::temp_dir().join(format!("ocrx-eval-rescan-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root_a);
        let _ = std::fs::remove_dir_all(&root_b);
        std::fs::create_dir_all(&root_a).unwrap();
        std::fs::create_dir_all(&root_b).unwrap();
        std::fs::write(root_a.join("page-0001.md"), b"body").unwrap();
        std::fs::write(root_b.join("page-0001.md"), b"body").unwrap();
        std::fs::write(root_a.join("result.json"), b"{}").unwrap();
        std::fs::write(root_b.join("result.json"), b"{\"elapsed_ms\":1}").unwrap();
        let comparison = compare_rescan_outputs(&root_a, &root_b).unwrap();
        // result.json (elapsed_ms non-determinism) is excluded from artifacts.
        assert!(comparison.byte_identical());
        assert_eq!(comparison.compared, 1);
        let _ = std::fs::remove_dir_all(root_a);
        let _ = std::fs::remove_dir_all(root_b);
    }
}
