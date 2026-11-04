"""Rereview BLK-1/2/3 fixes in main.rs."""
import io

path = r"D:\Develop\OCR-CLI\.worktrees\scan-throughput-phase2\src\ocrx-cli\src\main.rs"
with open(path, "r", encoding="utf-8", newline="") as f:
    s = f.read()

def sub(old, new, count=1):
    global s
    assert old in s, "ANCHOR NOT FOUND: " + old[:90]
    s = s.replace(old, new, count)

# ---- BLK-3: recognizer gains per-document baselines ------------------------
sub(
    """struct OvisRecognizer {
    output: PathBuf,
    max_tokens: u32,
    telemetry: Rc<RefCell<RuntimeTelemetry>>,
    /// Last cumulative step-profile snapshot (T0.1/A1 debug channel); only
    /// populated meaningfully when `OCRX_STEP_PROFILE` opts in.
    step_profile: Rc<RefCell<Option<ocrx_runtime::StepProfile>>>,
}""",
    """struct OvisRecognizer {
    output: PathBuf,
    max_tokens: u32,
    telemetry: Rc<RefCell<RuntimeTelemetry>>,
    /// Last cumulative step-profile snapshot (T0.1/A1 debug channel); only
    /// populated meaningfully when `OCRX_STEP_PROFILE` opts in.
    step_profile: Rc<RefCell<Option<ocrx_runtime::StepProfile>>>,
    /// Rereview BLK-3: the shared actor's telemetry/step-profile accumulate
    /// over its whole lifetime, so each orchestrated document reports
    /// `snapshot - baseline` (captured at this document's start) instead of
    /// the invocation-wide cumulative. Zero for the PerCall path and for the
    /// document that fills the shared slot (fresh runtime = zero base).
    telemetry_base: Rc<RefCell<RuntimeTelemetry>>,
    step_profile_base: Rc<RefCell<Option<ocrx_runtime::StepProfile>>>,
}""",
)

# update the two write sites to deltas
sub(
    """        *self.telemetry.borrow_mut() = model.telemetry();
        *self.step_profile.borrow_mut() = Some(model.step_profile());
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::from_raw(image.width, image.height, image.rgb8.clone())
            .ok_or_else(|| std::io::Error::other("invalid RGB8"))?""",
    """        let snapshot = model.telemetry();
        let base = *self.telemetry_base.borrow();
        *self.telemetry.borrow_mut() = snapshot.delta_from(&base);
        let profile_snapshot = model.step_profile();
        let profile_base = self
            .step_profile_base
            .borrow()
            .unwrap_or_else(ocrx_runtime::StepProfile::default);
        *self.step_profile.borrow_mut() = Some(profile_snapshot.delta_from(&profile_base));
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::from_raw(image.width, image.height, image.rgb8.clone())
            .ok_or_else(|| std::io::Error::other("invalid RGB8"))?""",
)
sub(
    """        *self.telemetry.borrow_mut() = model.telemetry();
        *self.step_profile.borrow_mut() = Some(model.step_profile());
        for ((_page, image), output) in pages.iter().zip(outputs) {""",
    """        let snapshot = model.telemetry();
        let base = *self.telemetry_base.borrow();
        *self.telemetry.borrow_mut() = snapshot.delta_from(&base);
        let profile_snapshot = model.step_profile();
        let profile_base = self
            .step_profile_base
            .borrow()
            .unwrap_or_else(ocrx_runtime::StepProfile::default);
        *self.step_profile.borrow_mut() = Some(profile_snapshot.delta_from(&profile_base));
        for ((_page, image), output) in pages.iter().zip(outputs) {""",
)

# construction site: add the two base Rc cells
sub(
    """    let telemetry = Rc::new(RefCell::new(RuntimeTelemetry::default()));
    let step_profile = Rc::new(RefCell::new(None));
    let recognizer = OvisRecognizer {
        output: output.to_path_buf(),
        max_tokens: options.max_output_tokens.unwrap_or(16_384),
        telemetry: Rc::clone(&telemetry),
        step_profile: Rc::clone(&step_profile),
    };""",
    """    let telemetry = Rc::new(RefCell::new(RuntimeTelemetry::default()));
    let step_profile = Rc::new(RefCell::new(None));
    // Rereview BLK-3: per-document baselines for the shared actor's
    // cumulative telemetry; captured per document after model resolution.
    let telemetry_base = Rc::new(RefCell::new(RuntimeTelemetry::default()));
    let step_profile_base = Rc::new(RefCell::new(None));
    let recognizer = OvisRecognizer {
        output: output.to_path_buf(),
        max_tokens: options.max_output_tokens.unwrap_or(16_384),
        telemetry: Rc::clone(&telemetry),
        step_profile: Rc::clone(&step_profile),
        telemetry_base: Rc::clone(&telemetry_base),
        step_profile_base: Rc::clone(&step_profile_base),
    };""",
)

# capture baselines after the resolve match (shared slot settled by now)
sub(
    """    let ocr_compatibility = if execution_plan.initialize_runtime {""",
    """    // Rereview BLK-3: capture this document's telemetry baselines now - the
    // shared slot is settled (filled by this document or an earlier one), so
    // `snapshot - baseline` isolates exactly this document's contribution.
    // PerCall and the slot-filling document see a fresh runtime: zero base.
    if let ModelSlot::Shared(Some(shared)) = env.model_slot {
        *telemetry_base.borrow_mut() = shared.runtime.telemetry();
        *step_profile_base.borrow_mut() = Some(shared.runtime.step_profile());
    }
    let ocr_compatibility = if execution_plan.initialize_runtime {""",
)

# ---- BLK-2: gate shared_reuse on this document actually needing OCR --------
sub(
    """    let shared_reuse = match env.model_slot {
        ModelSlot::Shared(Some(shared)) => {
            Some((shared.profile.clone(), shared.device.clone()))
        }
        _ => None,
    };""",
    """    // Rereview BLK-2: only a document that itself needs OCR reuses the
    // shared configuration - a pure-native document keeps the single-scan
    // metadata (`backend` = the requested device, `ocr_model_loaded` false),
    // regardless of whether an earlier document filled the slot.
    let shared_reuse = match env.model_slot {
        ModelSlot::Shared(Some(shared)) if execution_plan.initialize_runtime => {
            Some((shared.profile.clone(), shared.device.clone()))
        }
        _ => None,
    };""",
)

# ---- BLK-1a: scan_document entry check -------------------------------------
sub(
    """    let started = Instant::now();
    let canonical_input = std::fs::canonicalize(input).unwrap_or_else(|_| input.to_path_buf());""",
    """    // Rereview BLK-1: cancel = whole invocation. A signal that lands during
    // the previous document's teardown must stop this document before any
    // work (native documents have no ScanService cancellation checks).
    if cancellation.is_cancelled() {
        reporter.error(
            &ErrorReport {
                code: 130,
                kind: "cancelled".to_owned(),
                message: "scan cancelled by signal".to_owned(),
                cause: None,
                suggestion: Some("resume with --resume".to_owned()),
            },
            report_format,
        );
        emit_reporter(reporter);
        return 130;
    }
    let started = Instant::now();
    let canonical_input = std::fs::canonicalize(input).unwrap_or_else(|_| input.to_path_buf());""",
)

# ---- BLK-1b: native commit closure skips on cancellation -------------------
sub(
    """        |page| {
            let native = pdfium_owner_handle
                .borrow_mut()
                .extract_native(page)
                .map_err(|error| error.to_string())?;
            let classification = pages
                .iter()
                .position(|selected| *selected as u32 == page)
                .and_then(|index| classifications.get(index).copied())
                .ok_or_else(|| "native_classification_missing".to_owned())?;""",
    """        |page| {
            // Rereview BLK-1: after the signal, in-flight native work must
            // not keep committing - skip the page (the document's 130 return
            // below discards its summary, committed pages stay for --resume).
            if cancellation.is_cancelled() {
                return Ok(());
            }
            let native = pdfium_owner_handle
                .borrow_mut()
                .extract_native(page)
                .map_err(|error| error.to_string())?;
            let classification = pages
                .iter()
                .position(|selected| *selected as u32 == page)
                .and_then(|index| classifications.get(index).copied())
                .ok_or_else(|| "native_classification_missing".to_owned())?;""",
)

# ---- BLK-1c: empty-page commit loop checks cancellation --------------------
sub(
    """    for (page, classification) in pages.iter().zip(&classifications) {
        if resume_skip_pages.contains(&(*page as u32)) {
            continue;
        }
        if classification.page_type != PageType::Empty {
            continue;
        }""",
    """    for (page, classification) in pages.iter().zip(&classifications) {
        // Rereview BLK-1: stop committing empty pages after the signal.
        if cancellation.is_cancelled() {
            break;
        }
        if resume_skip_pages.contains(&(*page as u32)) {
            continue;
        }
        if classification.page_type != PageType::Empty {
            continue;
        }""",
)

# ---- BLK-1d: uniform cancelled check before the summary match --------------
sub(
    """    let vram_peak_bytes = stop_vram_sampler(vram_sampler);""",
    """    let vram_peak_bytes = stop_vram_sampler(vram_sampler);
    // Rereview BLK-1: any cancellation reaches here - the ScanService path
    // via `summary.cancelled`, the native path (which never enters the
    // service) via the token itself. Return 130 before any finalize/commit.
    if cancellation.is_cancelled() {
        reporter.error(
            &ErrorReport {
                code: 130,
                kind: "cancelled".to_owned(),
                message: "scan cancelled by signal".to_owned(),
                cause: None,
                suggestion: Some("resume with --resume".to_owned()),
            },
            report_format,
        );
        emit_reporter(reporter);
        return 130;
    }""",
)

# remove the now-unreachable inner cancelled check in the Ok branch
sub(
    """            if summary.cancelled {
                reporter.error(
                    &ErrorReport {
                        code: 130,
                        kind: "cancelled".to_owned(),
                        message: "scan cancelled by signal".to_owned(),
                        cause: None,
                        suggestion: Some("resume with --resume".to_owned()),
                    },
                    report_format,
                );
                emit_reporter(reporter);
                return 130;
            }
            {
                let mut all_pages = native_markdown;""",
    """            {
                let mut all_pages = native_markdown;""",
)

# ---- BLK-1e: orchestrator boundary check + test seam -----------------------
sub(
    """    let mut documents: Vec<ScanSummaryDocument> = Vec::new();
    let mut cancelled = false;
    for (index, input) in inputs.iter().enumerate() {
        let canonical = std::fs::canonicalize(input).unwrap_or_else(|_| input.clone());""",
    """    let mut documents: Vec<ScanSummaryDocument> = Vec::new();
    let mut cancelled = false;
    // Rereview BLK-1 test seam: deterministic cancellation injection at a
    // document boundary (same pattern as the OCRX_E2E_INJECT_* seams).
    let cancel_after_documents = std::env::var("OCRX_E2E_CANCEL_AFTER_DOCUMENTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    for (index, input) in inputs.iter().enumerate() {
        // Rereview BLK-1: cancel = whole invocation - stop at the boundary,
        // keep the already-committed documents, exit 130.
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        let canonical = std::fs::canonicalize(input).unwrap_or_else(|_| input.clone());""",
)

sub(
    """        documents.push(ScanSummaryDocument {
            input: canonical,
            output_dir: doc_output,
            status: match code {
                0 => "success",
                10 => "partial",
                _ => "failed",
            },
            exit_code: code,
            elapsed_ms: doc_elapsed_ms,
        });
    }""",
    """        documents.push(ScanSummaryDocument {
            input: canonical,
            output_dir: doc_output,
            status: match code {
                0 => "success",
                10 => "partial",
                _ => "failed",
            },
            exit_code: code,
            elapsed_ms: doc_elapsed_ms,
        });
        if let Some(limit) = cancel_after_documents
            && documents.len() >= limit
        {
            cancellation.request_cancel();
        }
    }""",
)

with open(path, "w", encoding="utf-8", newline="") as f:
    f.write(s)
print("rereview fixes applied")
