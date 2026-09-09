use image::ImageFormat;
use ocrx_core::{
    GpuResourceReport, ModelId, PageClassification, PageType, ProcessingMethod, QualityTier,
};
use ocrx_models::{
    ModelCatalog, ResourceSnapshot, UrlInstallRequest, community_asset_sizes, community_asset_urls,
    community_manifest, recommend_device, recommend_profile,
};
use ocrx_output::{
    CancellationToken, CheckpointResumeAction, CompatibilityKey, ErrorReport, ModelLoader,
    OcrQueuePage, PageArtifactRecognizer, PageArtifactResult, PageDiagnostic, PageMarkdown,
    PageRenderer, ProgressEvent, ProgressMode, ProgressStage, RenderAudit, RenderedPage,
    ReportFormat, Reporter, ResultReport, ScanService, collect_page_markdowns,
    commit_native_page_and_collect, finalize_document, format_report, merge_ocr_queue,
    read_checkpoint, recover_pending_ocr_queue, remove_for_overwrite,
};
use ocrx_pdf::{
    PdfErrorKind, PdfWorkerScheduler, PdfiumRenderer, RenderOptions, classify_pdf_error,
    fit_pixel_budget, plan_page_checked,
};
use ocrx_runtime::{Backend, LlamaMtmdRuntime, Runtime, RuntimeInfo, RuntimeTelemetry};
use std::cell::RefCell;
use std::io::{Cursor, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
enum CliCommand {
    Version,
    Help(Option<String>),
    Doctor {
        json: bool,
    },
    Scan(ScanOptions),
    Model {
        action: String,
        profile: Option<String>,
        offline: bool,
        non_interactive: bool,
    },
    Skill {
        action: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct ScanOptions {
    input: String,
    output_dir: Option<String>,
    pages: Option<Vec<usize>>,
    model: String,
    profile: String,
    pdf_mode: String,
    quality_tier: String,
    resume: bool,
    overwrite: bool,
    device: String,
    report: String,
    dpi: Option<u32>,
    max_image_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    context_size: Option<u32>,
    progress: String,
    log_level: String,
    offline: bool,
    non_interactive: bool,
    keep_pages: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigSource {
    Cli,
    Environment,
    UserConfig,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedConfig<T> {
    value: T,
    source: ConfigSource,
}

fn resolve_config<T: Clone>(
    cli: Option<T>,
    environment: Option<T>,
    user: Option<T>,
    default: T,
) -> ResolvedConfig<T> {
    if let Some(value) = cli {
        ResolvedConfig {
            value,
            source: ConfigSource::Cli,
        }
    } else if let Some(value) = environment {
        ResolvedConfig {
            value,
            source: ConfigSource::Environment,
        }
    } else if let Some(value) = user {
        ResolvedConfig {
            value,
            source: ConfigSource::UserConfig,
        }
    } else {
        ResolvedConfig {
            value: default,
            source: ConfigSource::Default,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn parse_scan_args(args: &[&str]) -> Result<ScanOptions, String> {
    let Some(input) = args.first().filter(|value| !value.starts_with('-')) else {
        return Err("scan requires an input PDF".to_owned());
    };
    let mut options = ScanOptions {
        input: (*input).to_owned(),
        output_dir: None,
        pages: None,
        model: ModelId::AS_STR.to_owned(),
        profile: "auto".to_owned(),
        pdf_mode: "auto".to_owned(),
        quality_tier: "l2".to_owned(),
        resume: false,
        overwrite: false,
        device: "auto".to_owned(),
        report: "text".to_owned(),
        dpi: None,
        max_image_tokens: None,
        max_output_tokens: None,
        context_size: None,
        progress: "auto".to_owned(),
        log_level: "info".to_owned(),
        offline: false,
        non_interactive: false,
        keep_pages: false,
    };
    let mut index = 1;
    while index < args.len() {
        match args[index] {
            "-o" | "--output-dir" => {
                index += 1;
                options.output_dir = Some(required_value(args, index, "--output-dir")?.to_owned());
            }
            "--pages" => {
                index += 1;
                options.pages = Some(parse_page_selection(required_value(
                    args, index, "--pages",
                )?)?);
            }
            "--model" => {
                index += 1;
                let model = required_value(args, index, "--model")?;
                ModelId::parse(model).map_err(|_| format!("unsupported model: {model}"))?;
                model.clone_into(&mut options.model);
            }
            "--model-profile" => {
                index += 1;
                enum_value(
                    args,
                    index,
                    "--model-profile",
                    &["auto", "compact", "balanced", "quality", "reference"],
                )?
                .clone_into(&mut options.profile);
            }
            "--pdf-mode" => {
                index += 1;
                enum_value(args, index, "--pdf-mode", &["auto", "text", "ocr"])?
                    .clone_into(&mut options.pdf_mode);
            }
            "--quality-tier" => {
                index += 1;
                enum_value(args, index, "--quality-tier", &["l1", "l2"])?
                    .clone_into(&mut options.quality_tier);
            }
            "--device" => {
                index += 1;
                enum_value(
                    args,
                    index,
                    "--device",
                    &["auto", "cpu", "metal", "cuda", "vulkan"],
                )?
                .clone_into(&mut options.device);
            }
            "--report" => {
                index += 1;
                enum_value(args, index, "--report", &["text", "json"])?
                    .clone_into(&mut options.report);
            }
            "--dpi" => {
                index += 1;
                options.dpi = Some(number_value(args, index, "--dpi")?);
            }
            "--max-image-tokens" => {
                index += 1;
                options.max_image_tokens = Some(number_value(args, index, "--max-image-tokens")?);
            }
            "--max-output-tokens" => {
                index += 1;
                options.max_output_tokens = Some(number_value(args, index, "--max-output-tokens")?);
            }
            "--context-size" => {
                index += 1;
                options.context_size = Some(number_value(args, index, "--context-size")?);
            }
            "--progress" => {
                index += 1;
                enum_value(
                    args,
                    index,
                    "--progress",
                    &["auto", "bar", "plain", "jsonl", "off"],
                )?
                .clone_into(&mut options.progress);
            }
            "--log-level" => {
                index += 1;
                enum_value(
                    args,
                    index,
                    "--log-level",
                    &["error", "warn", "info", "debug", "trace"],
                )?
                .clone_into(&mut options.log_level);
            }
            "--offline" => options.offline = true,
            "--non-interactive" => options.non_interactive = true,
            "--keep-pages" => options.keep_pages = true,
            "--resume" => options.resume = true,
            "--overwrite" => options.overwrite = true,
            flag => return Err(format!("unsupported scan option: {flag}")),
        }
        index += 1;
    }
    if options.resume && options.overwrite {
        return Err("--resume and --overwrite are mutually exclusive".to_owned());
    }
    let output_dir = resolve_output_dir(&options.input, options.output_dir.as_deref())?;
    validate_output_policy(Path::new(&options.input), &output_dir, &[])?;
    validate_numeric_options(
        options.dpi,
        options.max_image_tokens,
        options.max_output_tokens,
        options.context_size,
    )?;
    Ok(options)
}

/// Returns whether the planned pages require the OCR resource lifecycle.
/// Native candidates are validated by the execution phase; this helper only
/// answers the model-preparation question from the immutable initial plan.
#[must_use]
fn planned_ocr_pages(mode: &str, pages: &[PageClassification]) -> bool {
    mode == "ocr"
        || pages.iter().any(|page| match page.recommended_method {
            ProcessingMethod::Ocr => !matches!(page.page_type, PageType::Empty),
            _ => false,
        })
}

#[must_use]
fn dynamic_fallback_requires_model_assets(
    initial_ocr_pages: usize,
    final_ocr_pages: usize,
    assets_valid: bool,
) -> bool {
    initial_ocr_pages == 0 && final_ocr_pages > 0 && !assets_valid
}

#[allow(clippy::fn_params_excessive_bools)]
fn prepare_model_if_required(
    needs_ocr: bool,
    assets_valid: bool,
    offline: bool,
    non_interactive: bool,
    profile: &str,
    cache: &Path,
) -> Result<(), String> {
    if !needs_ocr || assets_valid {
        return Ok(());
    }
    if offline {
        return Err("offline scan requires verified local model assets".to_owned());
    }
    let manifest = community_manifest(profile).map_err(|e| format!("model manifest: {e:?}"))?;
    let (model_url, mmproj_url) = community_asset_urls(profile).ok_or("model URLs unavailable")?;
    let request = UrlInstallRequest {
        manifest,
        model_url,
        mmproj_url,
        cache_root: cache.to_path_buf(),
        offline: false,
        non_interactive,
        license_accepted: true,
        available_bytes: None,
    };
    ocrx_models::install_from_urls(&request)
        .map_err(|e| format!("model installation failed: {e:?}"))
}

fn resolve_and_prepare_model(
    needs_ocr: bool,
    requested_profile: &str,
    requested_device: &str,
    offline: bool,
    non_interactive: bool,
    cache: &Path,
) -> Result<(String, String, PathBuf, PathBuf), String> {
    if !needs_ocr {
        return Ok((
            requested_profile.to_owned(),
            requested_device.to_owned(),
            PathBuf::new(),
            PathBuf::new(),
        ));
    }
    let snapshot = resource_snapshot();
    let profile = resolve_scan_profile(requested_profile, snapshot.memory_gb)?;
    let device = recommend_device(snapshot, requested_device)
        .map_err(|error| format!("requested device is unavailable: {error:?}"))?;
    let manifest =
        community_manifest(&profile).map_err(|error| format!("model manifest: {error:?}"))?;
    let (model, mmproj) = ocrx_models::cache_paths(cache, &manifest.identity.revision, &profile)
        .map_err(|error| format!("model path resolution failed: {error:?}"))?;
    let (model_bytes, mmproj_bytes) = community_asset_sizes(&profile)
        .ok_or_else(|| "model asset sizes unavailable".to_owned())?;
    let mut assets_valid = model.is_file()
        && mmproj.is_file()
        && ocrx_models::verify_manifest_files_with_sizes(
            &manifest,
            cache,
            model_bytes,
            mmproj_bytes,
        )
        .and_then(|_| {
            ocrx_models::validate_runtime_adapter_compatibility("0.1.152", "0.1.153", "ovisocr2")
        })
        .is_ok();
    if !assets_valid && offline {
        return Err("offline scan requires verified local model assets".to_owned());
    }
    if !assets_valid {
        if std::io::stdin().is_terminal() && !non_interactive {
            eprintln!(
                "Model profile '{profile}' is missing or invalid; download {} MB of model assets? [y/N]",
                (model_bytes.saturating_add(mmproj_bytes)) / 1_000_000
            );
            let mut answer = String::new();
            let _ = std::io::stdin().read_line(&mut answer);
            if !answer.trim().eq_ignore_ascii_case("y") {
                return Err("model installation not confirmed".to_owned());
            }
        }
        let (model_url, mmproj_url) =
            community_asset_urls(&profile).ok_or_else(|| "model URLs unavailable".to_owned())?;
        let request = UrlInstallRequest {
            manifest,
            model_url,
            mmproj_url,
            cache_root: cache.to_path_buf(),
            offline: false,
            non_interactive,
            license_accepted: true,
            available_bytes: None,
        };
        ocrx_models::install_from_urls(&request)
            .map_err(|error| format!("model installation failed: {error:?}"))?;
        assets_valid = ocrx_models::verify_manifest_files_with_sizes(
            &request.manifest,
            cache,
            model_bytes,
            mmproj_bytes,
        )
        .and_then(|_| {
            ocrx_models::validate_runtime_adapter_compatibility("0.1.152", "0.1.153", "ovisocr2")
        })
        .is_ok();
        if !assets_valid {
            return Err("installed model failed verification".to_owned());
        }
    }
    Ok((profile, device, model, mmproj))
}

fn resolve_effective_ocr_configuration(
    requested_profile: &str,
    requested_device: &str,
) -> Result<(String, String), String> {
    let snapshot = resource_snapshot();
    let profile = resolve_scan_profile(requested_profile, snapshot.memory_gb)?;
    let device = recommend_device(snapshot, requested_device)
        .map_err(|error| format!("requested device is unavailable: {error:?}"))?;
    Ok((profile, device))
}

fn make_ocr_compatibility(
    native_compatibility: &CompatibilityKey,
    profile: &str,
    device: &str,
) -> CompatibilityKey {
    CompatibilityKey {
        profile: profile.to_owned(),
        parameters: format!(
            "{};ocr-profile={profile};ocr-device={device}",
            native_compatibility.parameters
        ),
        ..native_compatibility.clone()
    }
}

fn analyze_selected_pages(
    renderer: &PdfiumRenderer,
    input: &Path,
    pages: &[usize],
    tier: QualityTier,
) -> Result<Vec<PageClassification>, String> {
    pages
        .iter()
        .map(|page| {
            renderer
                .analyze_page(input, page.saturating_sub(1), tier)
                .map_err(|error| format!("{error:?}"))
        })
        .collect()
}

fn initial_ocr_queue(pages: &[usize], classifications: &[PageClassification]) -> Vec<OcrQueuePage> {
    let mut queue: Vec<_> = pages
        .iter()
        .zip(classifications)
        .filter_map(|(page, classification)| {
            (classification.recommended_method == ProcessingMethod::Ocr).then_some(OcrQueuePage {
                page: *page as u32,
                fallback_reason: None,
            })
        })
        .collect();
    queue.sort_by_key(|page| page.page);
    queue.dedup_by_key(|page| page.page);
    queue
}

fn validate_text_mode_classifications(
    classifications: &[PageClassification],
) -> Result<(), &'static str> {
    classifications.iter().try_for_each(|classification| {
        plan_page_checked(ocrx_core::PdfMode::Text, *classification)
            .map(|_| ())
            .map_err(|_| "text mode requires every non-empty page to be reliable native text")
    })
}

fn initial_ocr_queue_for_mode(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
) -> Vec<OcrQueuePage> {
    let mut queue: Vec<_> = pages
        .iter()
        .zip(classifications)
        .filter_map(|(page, classification)| {
            let needs_ocr = !matches!(classification.page_type, PageType::Empty)
                && (mode == "ocr" || classification.recommended_method == ProcessingMethod::Ocr);
            needs_ocr.then_some(OcrQueuePage {
                page: *page as u32,
                fallback_reason: None,
            })
        })
        .collect();
    queue.sort_by_key(|page| page.page);
    queue.dedup_by_key(|page| page.page);
    queue
}

fn native_candidate_pages(pages: &[usize], classifications: &[PageClassification]) -> Vec<u32> {
    let mut candidates: Vec<_> = pages
        .iter()
        .zip(classifications)
        .filter_map(|(page, classification)| {
            (classification.recommended_method == ProcessingMethod::NativeText)
                .then_some(*page as u32)
        })
        .collect();
    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

fn native_candidate_outcome(page: u32, validation: Result<(), String>) -> Result<(), OcrQueuePage> {
    validation.map_err(|reason| OcrQueuePage {
        page,
        fallback_reason: Some(reason),
    })
}

fn pdf_reason_code_from_str(value: &str) -> ocrx_core::PdfReasonCode {
    match value {
        "native_text_reliable" => ocrx_core::PdfReasonCode::NativeTextReliable,
        "no_visible_content" => ocrx_core::PdfReasonCode::NoVisibleContent,
        "image_coverage_high" => ocrx_core::PdfReasonCode::ImageCoverageHigh,
        "mixed_content" => ocrx_core::PdfReasonCode::MixedContent,
        "text_coverage_low" => ocrx_core::PdfReasonCode::TextCoverageLow,
        "unicode_validity_low" => ocrx_core::PdfReasonCode::UnicodeValidityLow,
        "duplicate_text" => ocrx_core::PdfReasonCode::DuplicateText,
        "geometry_anomaly" => ocrx_core::PdfReasonCode::GeometryAnomaly,
        "native_structure_unresolved" => ocrx_core::PdfReasonCode::NativeStructureUnresolved,
        _ => ocrx_core::PdfReasonCode::NativeValidationFailed,
    }
}

fn routing_diagnostic_json(page: u32, classification: PageClassification) -> String {
    let processing = ocrx_core::PageProcessing {
        planned_method: classification.recommended_method,
        actual_method: None,
        fallback_reason: None,
    };
    format!(
        "{{\"page\":{},\"classification\":{},\"processing\":{}}}",
        page,
        classification.to_json(),
        processing.to_json()
    )
}

fn routing_diagnostic_with_processing(
    page: u32,
    classification: PageClassification,
    actual_method: ProcessingMethod,
    fallback_reason: Option<ocrx_core::PdfReasonCode>,
) -> String {
    let processing = ocrx_core::PageProcessing {
        planned_method: classification.recommended_method,
        actual_method: Some(actual_method),
        fallback_reason,
    };
    debug_assert!(processing.validate().is_ok());
    format!(
        "{{\"page\":{},\"classification\":{},\"processing\":{}}}",
        page,
        classification.to_json(),
        processing.to_json()
    )
}

fn planned_method_for_mode(mode: &str, classification: PageClassification) -> ProcessingMethod {
    if classification.page_type == PageType::Empty {
        ProcessingMethod::Empty
    } else if mode == "ocr" {
        ProcessingMethod::Ocr
    } else {
        classification.recommended_method
    }
}

fn try_routing_diagnostic_with_processing(
    page: u32,
    classification: PageClassification,
    actual_method: ProcessingMethod,
    fallback_reason: Option<ocrx_core::PdfReasonCode>,
) -> Result<String, ocrx_core::DomainError> {
    try_routing_diagnostic_with_plan(
        page,
        classification,
        classification.recommended_method,
        actual_method,
        fallback_reason,
    )
}

fn try_routing_diagnostic_with_plan(
    page: u32,
    classification: PageClassification,
    planned_method: ProcessingMethod,
    actual_method: ProcessingMethod,
    fallback_reason: Option<ocrx_core::PdfReasonCode>,
) -> Result<String, ocrx_core::DomainError> {
    let processing = ocrx_core::PageProcessing {
        planned_method,
        actual_method: Some(actual_method),
        fallback_reason,
    };
    processing.validate()?;
    Ok(format!(
        "{{\"page\":{},\"classification\":{},\"processing\":{}}}",
        page,
        classification.to_json(),
        processing.to_json()
    ))
}

fn final_routing_diagnostics(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, ocrx_core::PdfReasonCode)],
) -> Vec<String> {
    final_routing_diagnostics_with_pending(
        mode,
        pages,
        classifications,
        fallback_reasons,
        &std::collections::HashSet::new(),
        &std::collections::HashSet::new(),
    )
}

fn final_routing_diagnostics_with_pending(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, ocrx_core::PdfReasonCode)],
    pending_pages: &std::collections::HashSet<u32>,
    unavailable_pages: &std::collections::HashSet<u32>,
) -> Vec<String> {
    pages
        .iter()
        .zip(classifications)
        .map(|(page, classification)| {
            if pending_pages.contains(&(*page as u32)) {
                let processing = ocrx_core::PageProcessing {
                    planned_method: planned_method_for_mode(mode, *classification),
                    actual_method: None,
                    fallback_reason: None,
                };
                return format!(
                    "{{\"page\":{},\"classification\":{},\"processing\":{}}}",
                    page,
                    classification.to_json(),
                    processing.to_json()
                );
            }
            if unavailable_pages.contains(&(*page as u32)) {
                let processing = ocrx_core::PageProcessing {
                    planned_method: planned_method_for_mode(mode, *classification),
                    actual_method: None,
                    fallback_reason: None,
                };
                return format!(
                    "{{\"page\":{},\"classification\":{},\"processing\":{}}}",
                    page,
                    classification.to_json(),
                    processing.to_json()
                );
            }
            if let Some((_, reason)) = fallback_reasons
                .iter()
                .find(|(number, _)| *number == *page as u32)
            {
                try_routing_diagnostic_with_plan(
                    *page as u32,
                    *classification,
                    classification.recommended_method,
                    ProcessingMethod::Ocr,
                    Some(*reason),
                )
                .unwrap_or_else(|_| routing_diagnostic_json(*page as u32, *classification))
            } else {
                let planned = planned_method_for_mode(mode, *classification);
                try_routing_diagnostic_with_plan(
                    *page as u32,
                    *classification,
                    planned,
                    planned,
                    None,
                )
                .unwrap_or_else(|_| routing_diagnostic_json(*page as u32, *classification))
            }
        })
        .collect()
}

fn write_routing_checkpoint_sidecars(
    output: &Path,
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, ocrx_core::PdfReasonCode)],
) -> std::io::Result<()> {
    write_routing_checkpoint_sidecars_with_states(
        output,
        mode,
        pages,
        classifications,
        fallback_reasons,
        &std::collections::HashSet::new(),
        &std::collections::HashSet::new(),
    )
}

fn write_routing_checkpoint_sidecars_with_states(
    output: &Path,
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, ocrx_core::PdfReasonCode)],
    pending_pages: &std::collections::HashSet<u32>,
    unavailable_pages: &std::collections::HashSet<u32>,
) -> std::io::Result<()> {
    for (page, routing) in pages.iter().zip(final_routing_diagnostics_with_pending(
        mode,
        pages,
        classifications,
        fallback_reasons,
        pending_pages,
        unavailable_pages,
    )) {
        let sidecar =
            format!("{{\"schema\":\"ocrx-routing-checkpoint-v1\",\"routing\":{routing}}}");
        ocrx_output::atomic_write(
            output.join(format!("page-{page:04}.routing.json")),
            sidecar.as_bytes(),
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RoutingCounts {
    native_text: usize,
    ocr: usize,
    empty: usize,
    fallback: usize,
}

fn routing_counts(
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, ocrx_core::PdfReasonCode)],
) -> RoutingCounts {
    let mut counts = RoutingCounts {
        native_text: 0,
        ocr: 0,
        empty: 0,
        fallback: 0,
    };
    for (page, classification) in pages.iter().zip(classifications) {
        if fallback_reasons
            .iter()
            .any(|(number, _)| *number == *page as u32)
        {
            counts.ocr += 1;
            counts.fallback += 1;
        } else {
            match classification.recommended_method {
                ProcessingMethod::NativeText => counts.native_text += 1,
                ProcessingMethod::Ocr => counts.ocr += 1,
                ProcessingMethod::Empty => counts.empty += 1,
            }
        }
    }
    counts
}

fn routing_counts_for_mode(
    mode: &str,
    pages: &[usize],
    classifications: &[PageClassification],
    fallback_reasons: &[(u32, ocrx_core::PdfReasonCode)],
) -> RoutingCounts {
    if mode == "ocr" {
        let empty = classifications
            .iter()
            .filter(|classification| classification.recommended_method == ProcessingMethod::Empty)
            .count();
        return RoutingCounts {
            native_text: 0,
            ocr: pages.len().saturating_sub(empty),
            empty,
            fallback: 0,
        };
    }
    routing_counts(pages, classifications, fallback_reasons)
}

fn format_routing_summary(document_type: ocrx_pdf::DocumentType, counts: RoutingCounts) -> String {
    format!(
        "document_type={} native_text={} ocr={} empty={} fallback={}\n",
        document_type.as_str(),
        counts.native_text,
        counts.ocr,
        counts.empty,
        counts.fallback
    )
}

#[must_use]
fn should_initialize_ocr_runtime(final_ocr_queue_len: usize) -> bool {
    final_ocr_queue_len > 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScanExecutionPlan {
    final_ocr_pages: usize,
    initialize_runtime: bool,
}

impl ScanExecutionPlan {
    #[must_use]
    fn from_final_ocr_queue(final_ocr_pages: usize) -> Self {
        Self {
            final_ocr_pages,
            initialize_runtime: should_initialize_ocr_runtime(final_ocr_pages),
        }
    }

    #[must_use]
    fn native_only(self) -> bool {
        !self.initialize_runtime && self.final_ocr_pages == 0
    }
}

fn with_ocr_runtime<T>(plan: ScanExecutionPlan, prepare: impl FnOnce() -> T) -> Option<T> {
    plan.initialize_runtime.then(prepare)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NativeValidationRoute {
    Native,
    OcrFallback(OcrQueuePage),
    TextFailure(String),
}

fn route_native_validation(
    mode: &str,
    page: u32,
    result: Result<(), String>,
) -> NativeValidationRoute {
    match result {
        Ok(()) => NativeValidationRoute::Native,
        Err(reason) if mode == "auto" => NativeValidationRoute::OcrFallback(OcrQueuePage {
            page,
            fallback_reason: Some(reason),
        }),
        Err(reason) => NativeValidationRoute::TextFailure(reason),
    }
}

fn build_final_ocr_queue(
    initial: &[OcrQueuePage],
    native_routes: &[(u32, NativeValidationRoute)],
) -> Vec<OcrQueuePage> {
    let fallbacks = native_routes
        .iter()
        .filter_map(|(_, route)| match route {
            NativeValidationRoute::OcrFallback(page) => Some(page.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    merge_ocr_queue(initial, &fallbacks)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeFirstPhaseResult {
    committed_native_pages: Vec<u32>,
    fallback_pages: Vec<OcrQueuePage>,
}

fn run_native_first_phase(
    mode: &str,
    candidates: &[u32],
    validate: impl Fn(u32) -> Result<(), String>,
    mut commit: impl FnMut(u32) -> Result<(), String>,
) -> Result<NativeFirstPhaseResult, String> {
    let mut result = NativeFirstPhaseResult {
        committed_native_pages: Vec::new(),
        fallback_pages: Vec::new(),
    };
    for page in candidates {
        match route_native_validation(mode, *page, validate(*page)) {
            NativeValidationRoute::Native => {
                commit(*page)?;
                result.committed_native_pages.push(*page);
            }
            NativeValidationRoute::OcrFallback(queue_page) => {
                result.fallback_pages.push(queue_page)
            }
            NativeValidationRoute::TextFailure(reason) => {
                return Err(format!("page {page}: {reason}"));
            }
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeOnlyDecision {
    Complete,
    ContinueToOcr,
}

fn native_only_decision(phase: &NativeFirstPhaseResult) -> NativeOnlyDecision {
    if phase.fallback_pages.is_empty() {
        NativeOnlyDecision::Complete
    } else {
        NativeOnlyDecision::ContinueToOcr
    }
}

fn ocr_queue_after_native_phase(
    initial: &[OcrQueuePage],
    phase: &NativeFirstPhaseResult,
) -> Vec<OcrQueuePage> {
    merge_ocr_queue(initial, &phase.fallback_pages)
}

fn service_pages_from_ocr_queue(queue: &[OcrQueuePage]) -> Vec<u32> {
    let mut pages = queue.iter().map(|page| page.page).collect::<Vec<_>>();
    pages.sort_unstable();
    pages.dedup();
    pages
}

fn collect_verified_page_markdowns(
    output: &Path,
    page_numbers: &[u32],
    compatibility: &CompatibilityKey,
) -> std::io::Result<Vec<PageMarkdown>> {
    page_numbers
        .iter()
        .filter_map(|page| {
            let checkpoint_path = output.join(format!("page-{page:04}.checkpoint"));
            let checkpoint = read_checkpoint(checkpoint_path).ok()?;
            checkpoint
                .is_reusable_at(compatibility, output, *page)
                .then_some(*page)
        })
        .map(|page| {
            Ok(PageMarkdown {
                page_number: page,
                content: std::fs::read_to_string(output.join(format!("page-{page:04}.md")))?,
            })
        })
        .collect()
}

fn restrict_ocr_queue_to_pages(
    queue: &[OcrQueuePage],
    selected_pages: &[usize],
) -> Vec<OcrQueuePage> {
    let selected = selected_pages
        .iter()
        .map(|page| *page as u32)
        .collect::<std::collections::HashSet<_>>();
    queue
        .iter()
        .filter(|page| selected.contains(&page.page))
        .cloned()
        .collect()
}

fn required_value<'a>(args: &'a [&str], index: usize, option: &str) -> Result<&'a str, String> {
    args.get(index)
        .filter(|value| !value.starts_with('-'))
        .copied()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn enum_value<'a>(
    args: &'a [&str],
    index: usize,
    option: &str,
    allowed: &[&str],
) -> Result<&'a str, String> {
    let value = required_value(args, index, option)?;
    allowed
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("invalid value for {option}: {value}"))
}

fn number_value(args: &[&str], index: usize, option: &str) -> Result<u32, String> {
    let value = required_value(args, index, option)?;
    let number = value
        .parse::<u32>()
        .map_err(|_| format!("invalid number for {option}: {value}"))?;
    (number > 0)
        .then_some(number)
        .ok_or_else(|| format!("{option} must be greater than zero"))
}

fn validate_numeric_options(
    dpi: Option<u32>,
    max_image_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    context_size: Option<u32>,
) -> Result<(), String> {
    if dpi.is_some_and(|value| !(1..=600).contains(&value)) {
        return Err("dpi must be between 1 and 600".to_owned());
    }
    if max_image_tokens.is_some_and(|value| value < 128) {
        return Err("max image tokens must be at least 128".to_owned());
    }
    if max_output_tokens.is_some_and(|value| value < 128) {
        return Err("max output tokens must be at least 128".to_owned());
    }
    if context_size.is_some_and(|value| value < 1024) {
        return Err("context size must be at least 1024".to_owned());
    }
    let image_budget = max_image_tokens.unwrap_or(1024);
    let output_budget = max_output_tokens.unwrap_or(4096);
    let context_budget = context_size.unwrap_or(16_384);
    let required = image_budget
        .saturating_add(output_budget)
        .saturating_add(1024);
    if required > context_budget {
        return Err(format!(
            "context size {context_budget} is too small for image/output/prompt budget {required}"
        ));
    }
    Ok(())
}

fn resolve_output_dir(input: &str, explicit: Option<&str>) -> Result<PathBuf, String> {
    let input_path = Path::new(input);
    let output = match explicit {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        Some(_) => return Err("output directory cannot be empty".to_owned()),
        None => {
            let stem = input_path
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "input filename has no usable stem".to_owned())?;
            input_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(format!("{stem}-ocr"))
        }
    };
    if output.is_absolute() {
        Ok(output)
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(output))
            .map_err(|error| format!("cannot resolve output directory: {error}"))
    }
}

fn validate_output_policy(input: &Path, output: &Path, protected: &[&Path]) -> Result<(), String> {
    if !output.is_absolute() {
        return Err("output directory must be absolute".to_owned());
    }
    if output == input || output.starts_with(input) || input.starts_with(output) {
        return Err("output directory overlaps input".to_owned());
    }
    if protected
        .iter()
        .any(|root| output == *root || output.starts_with(root) || root.starts_with(output))
    {
        return Err("output directory overlaps a protected path".to_owned());
    }
    Ok(())
}

#[allow(dead_code)]
fn format_cli_result(report: &ResultReport, report_format: &str) -> Result<String, String> {
    let format = match report_format {
        "text" => ReportFormat::Text,
        "json" => ReportFormat::Json,
        other => return Err(format!("unsupported report format: {other}")),
    };
    Ok(format_report(report, format))
}

fn parse_command(args: &[&str]) -> Result<CliCommand, String> {
    match args {
        [] | ["version"] => Ok(CliCommand::Version),
        ["help"] => Ok(CliCommand::Help(None)),
        ["help", topic] => Ok(CliCommand::Help(Some((*topic).to_owned()))),
        ["doctor"] => Ok(CliCommand::Doctor { json: false }),
        ["doctor", "--report", "json"] => Ok(CliCommand::Doctor { json: true }),
        ["scan", input, rest @ ..] if !input.is_empty() && !input.starts_with('-') => {
            let mut scan_args = vec![*input];
            scan_args.extend(rest.iter().copied());
            Ok(CliCommand::Scan(parse_scan_args(&scan_args)?))
        }
        ["model", action] if matches!(*action, "list" | "info" | "recommend") => {
            Ok(CliCommand::Model {
                action: (*action).to_owned(),
                profile: None,
                offline: false,
                non_interactive: false,
            })
        }
        ["model", action, rest @ ..]
            if matches!(*action, "info" | "install" | "remove" | "verify") =>
        {
            parse_model_command(action, rest)
        }
        ["skill", action] if matches!(*action, "install" | "uninstall") => Ok(CliCommand::Skill {
            action: (*action).to_owned(),
        }),
        [command, ..] => Err(format!("unsupported or incomplete command: {command}")),
    }
}

fn parse_model_command(action: &str, args: &[&str]) -> Result<CliCommand, String> {
    let mut profile = None;
    let mut offline = false;
    let mut non_interactive = false;
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            flag @ ("--offline" | "--non-interactive") if action == "remove" => {
                return Err(format!("model {action} does not support {flag}"));
            }
            "--offline" => offline = true,
            "--non-interactive" => non_interactive = true,
            value if !value.starts_with('-') => {
                if profile.is_some() {
                    return Err(format!("unexpected model argument: {value}"));
                }
                if !matches!(value, "compact" | "balanced" | "quality" | "reference") {
                    return Err(format!("invalid model profile: {value}"));
                }
                profile = Some(value.to_owned());
            }
            other => return Err(format!("unsupported model flag: {other}")),
        }
        index += 1;
    }
    if matches!(action, "install" | "remove" | "verify") && profile.is_none() {
        return Err(format!("model {action} requires a profile"));
    }
    Ok(CliCommand::Model {
        action: action.to_owned(),
        profile,
        offline,
        non_interactive,
    })
}

fn parse_page_selection(expression: &str) -> Result<Vec<usize>, String> {
    let mut pages = Vec::new();
    for part in expression.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("page expression contains an empty item".to_owned());
        }
        if let Some((start, end)) = part.split_once('-') {
            let start = parse_page_number(start)?;
            let end = parse_page_number(end)?;
            if start > end {
                return Err(format!("page range is reversed: {start}-{end}"));
            }
            pages.extend(start..=end);
        } else {
            pages.push(parse_page_number(part)?);
        }
    }
    pages.sort_unstable();
    pages.dedup();
    Ok(pages)
}

fn parse_page_number(value: &str) -> Result<usize, String> {
    let page = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid page number: {value}"))?;
    (page > 0)
        .then_some(page)
        .ok_or_else(|| "page numbers start at 1".to_owned())
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let command = parse_command(&arguments).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        std::process::exit(2);
    });
    match command {
        CliCommand::Version => println!("ocrx 0.1.0"),
        CliCommand::Help(topic) => print_help(topic.as_deref()),
        CliCommand::Doctor { json } => {
            let snapshot = resource_snapshot();
            let info = doctor_runtime_info(snapshot);
            if json {
                println!("{}", doctor_json_report(snapshot, info));
            } else {
                println!(
                    "model={} backend={:?} image_token_limit={} memory_gb={} metal_available={}",
                    ModelId::AS_STR,
                    info.backend,
                    info.image_token_limit,
                    snapshot.memory_gb,
                    snapshot.metal_available
                );
            }
        }
        CliCommand::Model {
            action,
            profile,
            offline: _,
            non_interactive: _,
        } if action == "list" || action == "info" || action == "recommend" => {
            let manifests = ["compact", "balanced", "quality", "reference"]
                .into_iter()
                .map(|profile| ocrx_models::community_manifest(profile).expect("pinned manifest"))
                .collect();
            let catalog = ModelCatalog::new(manifests).expect("built-in catalog must be valid");
            if action == "list" {
                println!("model={}", ModelId::AS_STR);
            } else if action == "info" {
                let info = catalog.info(&profile.into_iter().collect::<Vec<_>>());
                println!(
                    "model={} profiles={} installed={}",
                    ModelId::AS_STR,
                    info.profiles.join(","),
                    info.installed_profiles.join(",")
                );
            } else if action == "recommend" {
                let snapshot = resource_snapshot();
                match recommend_profile(snapshot.memory_gb, &["compact", "balanced", "quality"]) {
                    Ok(profile) => println!(
                        "model={} profile={} memory_gb={} device={}",
                        ModelId::AS_STR,
                        profile,
                        snapshot.memory_gb,
                        recommend_device(snapshot, "auto")
                            .unwrap_or_else(|_| "unavailable".to_owned())
                    ),
                    Err(error) => {
                        eprintln!("profile recommendation failed: {error:?}");
                        std::process::exit(6);
                    }
                }
            }
        }
        CliCommand::Model {
            action,
            profile,
            offline,
            non_interactive,
        } => {
            let Some(profile) = profile else {
                eprintln!("model {action} requires a profile");
                std::process::exit(2);
            };
            let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".ocrx-model-cache"));
            let revision = ocrx_models::community_manifest(&profile)
                .unwrap_or_else(|_| unreachable!("profile parsed by CLI"))
                .identity
                .revision;
            let (model_path, _mmproj_path) =
                ocrx_models::cache_paths(&cache_root, &revision, &profile)
                    .unwrap_or_else(|_| unreachable!("profile parsed by CLI"));
            match action.as_str() {
                "verify" => {
                    let manifest = ocrx_models::community_manifest(&profile)
                        .unwrap_or_else(|_| unreachable!("profile parsed by CLI"));
                    let sizes = ocrx_models::community_asset_sizes(&profile)
                        .unwrap_or_else(|| unreachable!("profile parsed by CLI"));
                    match ocrx_models::verify_manifest_files_with_sizes(
                        &manifest,
                        &cache_root,
                        sizes.0,
                        sizes.1,
                    )
                    .and_then(|_| {
                        ocrx_models::validate_runtime_adapter_compatibility(
                            "0.1.152", "0.1.153", "ovisocr2",
                        )
                    }) {
                        Ok(()) => println!(
                            "model=ovisocr2 profile={} revision={} status=installed",
                            profile, manifest.identity.revision
                        ),
                        Err(error) => {
                            eprintln!("model verification failed: {error:?}");
                            std::process::exit(4);
                        }
                    }
                }
                "install" => {
                    let manifest = community_manifest(&profile)
                        .unwrap_or_else(|_| unreachable!("profile parsed by CLI"));
                    let Some((model_url, mmproj_url)) = community_asset_urls(&profile) else {
                        unreachable!("profile parsed by CLI");
                    };
                    // Trusted apache-2.0 community assets may install non-interactively.
                    let request = UrlInstallRequest {
                        manifest: manifest.clone(),
                        model_url,
                        mmproj_url,
                        cache_root: cache_root.clone(),
                        offline,
                        non_interactive,
                        license_accepted: true,
                        available_bytes: None,
                    };
                    match ocrx_models::install_from_urls(&request) {
                        Ok(()) => println!(
                            "model=ovisocr2 profile={} revision={} status=installed",
                            profile, manifest.identity.revision
                        ),
                        Err(ocrx_models::ManifestError::Offline) => {
                            eprintln!("model install blocked: offline mode");
                            std::process::exit(5);
                        }
                        Err(ocrx_models::ManifestError::LicenseRequired) => {
                            eprintln!("model install blocked: license acceptance required");
                            std::process::exit(4);
                        }
                        Err(ocrx_models::ManifestError::InsufficientDisk) => {
                            eprintln!("model install blocked: insufficient disk");
                            std::process::exit(5);
                        }
                        Err(error) => {
                            eprintln!("model install failed: {error:?}");
                            std::process::exit(4);
                        }
                    }
                }
                "remove" => {
                    let directory = model_path
                        .parent()
                        .expect("generated cache path has parent");
                    if directory.exists() {
                        if let Err(error) = std::fs::remove_dir_all(directory) {
                            eprintln!("model removal failed: {error}");
                            std::process::exit(9);
                        }
                    }
                    println!("model=ovisocr2 profile={profile} status=removed");
                }
                _ => unreachable!("model action parsed by CLI"),
            }
        }
        CliCommand::Scan(options) => run_scan(options),
        CliCommand::Skill { action } => {
            let target = std::env::var("OCRX_SKILL_TARGET").unwrap_or_else(|_| "all".to_owned());
            let scope = match ocrx_skill::SkillScope::parse(
                &std::env::var("OCRX_SKILL_SCOPE").unwrap_or_else(|_| "user".to_owned()),
            ) {
                Ok(scope) => scope,
                Err(error) => {
                    eprintln!("skill {action} blocked: {error}");
                    std::process::exit(2);
                }
            };
            let (home, project): (Option<PathBuf>, Option<PathBuf>) = match scope {
                ocrx_skill::SkillScope::User => (
                    std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .map(PathBuf::from),
                    None,
                ),
                ocrx_skill::SkillScope::Project => (
                    None,
                    Some(
                        std::env::var_os("OCRX_SKILL_PROJECT")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| {
                                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                            }),
                    ),
                ),
            };
            let content = include_str!("../../../assets/skills/ocrx/SKILL.md");
            let result = match action.as_str() {
                "install" => ocrx_skill::install(
                    &target,
                    scope,
                    home.as_deref(),
                    project.as_deref(),
                    content,
                )
                .map(|paths| {
                    format!(
                        "skill=ocrx status=installed scope={} targets={} files={}",
                        scope.as_str(),
                        target,
                        paths.len()
                    )
                }),
                "uninstall" => {
                    ocrx_skill::uninstall(&target, scope, home.as_deref(), project.as_deref()).map(
                        |paths| {
                            format!(
                                "skill=ocrx status=uninstalled scope={} targets={} files={}",
                                scope.as_str(),
                                target,
                                paths.len()
                            )
                        },
                    )
                }
                _ => unreachable!("skill action parsed by CLI"),
            };
            match result {
                Ok(message) => println!("{message}"),
                Err(error) => {
                    eprintln!("skill {action} failed: {error}");
                    std::process::exit(9);
                }
            }
        }
    }
}

struct PdfScanRenderer {
    renderer: PdfiumRenderer,
    input: PathBuf,
    options: RenderOptions,
}

struct WorkerScanRenderer {
    scheduler: PdfWorkerScheduler,
    input: PathBuf,
    output: PathBuf,
    page_count: Option<usize>,
    rendered_end: usize,
}

impl PageRenderer for WorkerScanRenderer {
    fn preflight(&mut self) -> std::io::Result<()> {
        if self.page_count.is_none() {
            let direct = PdfiumRenderer::new(self.scheduler.library_dir.clone());
            let count = direct
                .page_count(&self.input)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            self.page_count = Some(count);
        }
        std::fs::create_dir_all(&self.output)?;
        Ok(())
    }

    fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
        let count = self.page_count.unwrap_or_default();
        let page_index = usize::try_from(page).map_err(std::io::Error::other)?;
        if page == 0 || page_index > count {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "worker page is out of range",
            ));
        }
        // Worker pages are 0-based half-open segments; ensure the page is covered.
        while self.rendered_end < page_index {
            let start = self.rendered_end;
            let end = (start + self.scheduler.segment_pages).min(count);
            // Drop previous segment files to avoid linear growth.
            if start > 0 {
                let _ = std::fs::remove_dir_all(&self.output);
                std::fs::create_dir_all(&self.output)?;
            }
            self.scheduler
                .run(&self.input, &self.output, start, end)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            self.rendered_end = end;
        }
        let path = self.output.join(format!("page-{page:04}.png"));
        let image = image::open(path)
            .map_err(std::io::Error::other)?
            .into_rgb8();
        let audit_path = self.output.join(format!("page-{page:04}.audit"));
        let audit_text = std::fs::read_to_string(audit_path).map_err(std::io::Error::other)?;
        let original = audit_text
            .strip_prefix("original=")
            .and_then(|value| value.split_once(";rendered="))
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid worker audit")
            })?;
        let (original_width, original_height) = parse_dimensions(original.0)?;
        let (metadata_rendered_width, metadata_rendered_height) = parse_dimensions(original.1)?;
        if (metadata_rendered_width, metadata_rendered_height) != image.dimensions() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "worker audit dimensions do not match PNG",
            ));
        }
        const PIXEL_BUDGET: u64 = 2880 * 2880;
        let rendered_width = image.width();
        let rendered_height = image.height();
        let (model_width, model_height, scaled) =
            fit_pixel_budget(rendered_width, rendered_height, PIXEL_BUDGET);
        let rgb = if scaled {
            image::imageops::resize(
                &image,
                model_width,
                model_height,
                image::imageops::FilterType::Triangle,
            )
        } else {
            image
        };
        let (scale_ratio_numerator, scale_ratio_denominator) =
            linear_scale_ratio(model_width, rendered_width);
        RenderedPage::new(rgb.width(), rgb.height(), rgb.into_raw()).map(|page| {
            page.with_audit(RenderAudit {
                original_width,
                original_height,
                rendered_width,
                rendered_height,
                model_width,
                model_height,
                scaled,
                scale_ratio_numerator,
                scale_ratio_denominator,
            })
        })
    }
}

fn parse_dimensions(value: &str) -> std::io::Result<(u32, u32)> {
    let (width, height) = value.split_once('x').ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid dimensions")
    })?;
    Ok((
        width.parse().map_err(std::io::Error::other)?,
        height.parse().map_err(std::io::Error::other)?,
    ))
}

fn linear_scale_ratio(model_width: u32, rendered_width: u32) -> (u64, u64) {
    let numerator = u64::from(model_width);
    let denominator = u64::from(rendered_width);
    let divisor = gcd(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}
impl PageRenderer for PdfScanRenderer {
    fn preflight(&mut self) -> std::io::Result<()> {
        self.renderer
            .page_count(&self.input)
            .map(|_| ())
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
    fn render(&mut self, page: u32) -> std::io::Result<RenderedPage> {
        const PIXEL_BUDGET: u64 = 2880 * 2880;
        let (image, original_width, original_height) = self
            .renderer
            .render_page_rgb8_with_audit(
                &self.input,
                usize::try_from(page - 1).map_err(std::io::Error::other)?,
                self.options,
            )
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let rendered_width = image.width();
        let rendered_height = image.height();
        let (model_width, model_height, scaled) =
            fit_pixel_budget(rendered_width, rendered_height, PIXEL_BUDGET);
        let rgb = if scaled {
            image::imageops::resize(
                &image,
                model_width,
                model_height,
                image::imageops::FilterType::Triangle,
            )
        } else {
            image
        };
        let (scale_ratio_numerator, scale_ratio_denominator) =
            linear_scale_ratio(model_width, rendered_width);
        RenderedPage::new(rgb.width(), rgb.height(), rgb.into_raw()).map(|page| {
            page.with_audit(RenderAudit {
                original_width,
                original_height,
                rendered_width,
                rendered_height,
                model_width,
                model_height,
                scaled,
                scale_ratio_numerator,
                scale_ratio_denominator,
            })
        })
    }
}

struct RuntimeLoader {
    model: PathBuf,
    mmproj: PathBuf,
    gpu: bool,
    max_image_tokens: u32,
    context_size: u32,
}
impl ModelLoader for RuntimeLoader {
    type Model = LlamaMtmdRuntime;
    fn load(&mut self) -> std::io::Result<Self::Model> {
        LlamaMtmdRuntime::load(
            &self.model,
            &self.mmproj,
            self.gpu,
            self.max_image_tokens,
            self.context_size,
        )
        .map_err(std::io::Error::other)
    }
}

/// Sample peak Windows GPU VRAM (via `nvidia-smi`) while a scan runs so the
/// W4.4 GPU resource report can record a real `vram_peak_bytes`. Returns `None`
/// on non-Windows targets or when `nvidia-smi` is unavailable.
fn start_vram_sampler() -> Option<(
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<u64>,
)> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_worker = std::sync::Arc::clone(&done);
    let handle = std::thread::spawn(move || {
        let mut peak_mb = 0_u64;
        while !done_worker.load(std::sync::atomic::Ordering::SeqCst) {
            if let Ok(output) = std::process::Command::new("nvidia-smi")
                .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
                .output()
                && output.status.success()
            {
                if let Some(first) = String::from_utf8_lossy(&output.stdout).lines().next()
                    && let Ok(mb) = first.trim().parse::<u64>()
                {
                    peak_mb = peak_mb.max(mb);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        peak_mb.saturating_mul(1024 * 1024)
    });
    Some((done, handle))
}

fn stop_vram_sampler(
    sampler: Option<(
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<u64>,
    )>,
) -> Option<u64> {
    let (done, handle) = sampler?;
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let bytes = handle.join().unwrap_or(0);
    (bytes > 0).then_some(bytes)
}

struct OvisRecognizer {
    output: PathBuf,
    max_tokens: u32,
    telemetry: Rc<RefCell<RuntimeTelemetry>>,
}
impl PageArtifactRecognizer<LlamaMtmdRuntime> for OvisRecognizer {
    fn recognize_artifact(
        &mut self,
        model: &mut LlamaMtmdRuntime,
        page: u32,
        image: &RenderedPage,
    ) -> std::io::Result<PageArtifactResult> {
        let artifact = ocrx_ovisocr2::write_page_artifacts_with_status(
            model,
            page,
            image.width,
            image.height,
            image.rgb8.clone(),
            self.max_tokens,
            &self.output,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        let previous = *self.telemetry.borrow();
        *self.telemetry.borrow_mut() = previous.merge(model.telemetry());
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::from_raw(image.width, image.height, image.rgb8.clone())
            .ok_or_else(|| std::io::Error::other("invalid RGB8"))?
            .write_to(&mut png, ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        Ok(PageArtifactResult {
            png: png.into_inner(),
            markdown: artifact.markdown,
            page_json: artifact.page_json,
            truncated: artifact.truncated,
            warnings: artifact.warnings,
        })
    }
}

fn run_scan(options: ScanOptions) -> ! {
    let started = Instant::now();
    let report_format = if options.report == "json" {
        ReportFormat::Json
    } else {
        ReportFormat::Text
    };
    let progress_mode = resolve_progress_mode(&options.progress, &options.log_level);
    let mut reporter = Reporter::default();
    let input = PathBuf::from(&options.input);
    let canonical_input = std::fs::canonicalize(&input).unwrap_or_else(|_| input.clone());
    let output = match resolve_output_dir(&options.input, options.output_dir.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 2,
                    kind: "usage".to_owned(),
                    message: "output directory resolution failed".to_owned(),
                    cause: Some(error),
                    suggestion: Some("pass an explicit --output-dir".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(2);
        }
    };
    let library = std::env::var_os("OCRX_PDFIUM_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    // Validate the input before resolving or downloading model assets. Invalid,
    // encrypted, and corrupt PDFs are input errors even on a machine with no
    // installed OCR model.
    let page_count = match PdfiumRenderer::new(library.clone()).page_count(&input) {
        Ok(count) => count,
        Err(error) => {
            let error_kind = match classify_input_error(&input, &error.to_string()) {
                PdfErrorKind::NonPdf => "pdf_non_pdf",
                PdfErrorKind::Corrupt => "pdf_corrupt",
                PdfErrorKind::Encrypted => "pdf_encrypted",
                PdfErrorKind::DynamicLibrary => "pdf_dynamic_library",
                PdfErrorKind::Unknown => "input",
            };
            reporter.error(
                &ErrorReport {
                    code: 3,
                    kind: error_kind.to_owned(),
                    message: "unable to read PDF page count".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: Some("check the input PDF path".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(3);
        }
    };
    let pages = options
        .pages
        .clone()
        .unwrap_or_else(|| (1..=page_count).collect());
    if pages.iter().any(|page| *page == 0 || *page > page_count) {
        reporter.error(
            &ErrorReport {
                code: 2,
                kind: "usage".to_owned(),
                message: "page selection is out of range".to_owned(),
                cause: Some(format!("pdf has {page_count} pages")),
                suggestion: Some("adjust --pages".to_owned()),
            },
            report_format,
        );
        print_reporter(reporter);
        std::process::exit(2);
    }
    let required_tier = if options.quality_tier == "l1" {
        QualityTier::L1
    } else {
        QualityTier::L2
    };
    let pdf = PdfiumRenderer::new(library.clone());
    reporter.progress(
        &ProgressEvent::for_stage(ProgressStage::Analyzing, None, 0, pages.len() as u32),
        progress_mode,
    );
    let classifications = match analyze_selected_pages(&pdf, &input, &pages, required_tier) {
        Ok(classifications) => classifications,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 3,
                    kind: "pdf_analysis".to_owned(),
                    message: "unable to analyze PDF pages".to_owned(),
                    cause: Some(error),
                    suggestion: Some("check the input PDF and PDFium installation".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(3);
        }
    };
    if options.pdf_mode == "text"
        && let Err(error) = validate_text_mode_classifications(&classifications)
    {
        reporter.error(
            &ErrorReport {
                code: 3,
                kind: "pdf_routing".to_owned(),
                message: "text mode cannot process non-text PDF pages".to_owned(),
                cause: Some(error.to_owned()),
                suggestion: Some("use --pdf-mode auto or --pdf-mode ocr".to_owned()),
            },
            report_format,
        );
        print_reporter(reporter);
        std::process::exit(3);
    }
    let initial_queue = initial_ocr_queue_for_mode(&options.pdf_mode, &pages, &classifications);
    let cache = std::env::var_os("OCRX_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".ocrx-model-cache"));
    if output.exists()
        && (output.join("document.md").exists() || output.join("result.json").exists())
        && !options.resume
        && !options.overwrite
    {
        reporter.error(
            &ErrorReport {
                code: 9,
                kind: "output".to_owned(),
                message: "output directory already contains results".to_owned(),
                cause: Some(output.display().to_string()),
                suggestion: Some("use --resume or --overwrite".to_owned()),
            },
            report_format,
        );
        print_reporter(reporter);
        std::process::exit(9);
    }
    if options.overwrite {
        let cache_root = cache.clone();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let protected = [input.as_path(), cache_root.as_path(), cwd.as_path()];
        if let Err(error) = remove_for_overwrite(&input, &output, &protected) {
            reporter.error(
                &ErrorReport {
                    code: 9,
                    kind: "output".to_owned(),
                    message: "overwrite refused for unsafe output path".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: Some("choose a dedicated output directory".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(9);
        }
    }
    let pages = options
        .pages
        .clone()
        .unwrap_or_else(|| (1..=page_count).collect());
    if pages.iter().any(|page| *page == 0 || *page > page_count) {
        reporter.error(
            &ErrorReport {
                code: 2,
                kind: "usage".to_owned(),
                message: "page selection is out of range".to_owned(),
                cause: Some(format!("pdf has {page_count} pages")),
                suggestion: Some("adjust --pages".to_owned()),
            },
            report_format,
        );
        print_reporter(reporter);
        std::process::exit(2);
    }
    reporter.progress(
        &ProgressEvent::for_stage(ProgressStage::Extracting, None, 0, pages.len() as u32),
        progress_mode,
    );
    let dpi = options.dpi.unwrap_or(300).try_into().unwrap_or(300);
    let renderer: Box<dyn PageRenderer> = if let Some(worker) = std::env::var_os("OCRX_PDF_WORKER")
    {
        let worker_output =
            std::env::temp_dir().join(format!("ocrx-worker-render-{}", std::process::id()));
        Box::new(WorkerScanRenderer {
            scheduler: PdfWorkerScheduler {
                worker_executable: PathBuf::from(worker),
                library_dir: library.clone(),
                segment_pages: 20,
                timeout: Duration::from_secs(20 * 60),
                dpi,
                env: Vec::new(),
            },
            input: input.clone(),
            output: worker_output,
            page_count: Some(page_count),
            rendered_end: 0,
        })
    } else {
        Box::new(PdfScanRenderer {
            renderer: PdfiumRenderer::new(library),
            input: input.clone(),
            options: RenderOptions { dpi },
        })
    };
    let telemetry = Rc::new(RefCell::new(RuntimeTelemetry::default()));
    let recognizer = OvisRecognizer {
        output: output.clone(),
        max_tokens: options.max_output_tokens.unwrap_or(4096),
        telemetry: Rc::clone(&telemetry),
    };
    // Keep compatibility metadata stable without touching model resources.
    // Actual profile/device resolution is deferred until the final OCR queue
    // exists below.
    let mut profile = options.profile.clone();
    let mut device = options.device.clone();
    let mut model = PathBuf::new();
    let mut mmproj = PathBuf::new();
    let input_sha256 = match file_sha256(&input) {
        Ok(hash) => hash,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 3,
                    kind: "input".to_owned(),
                    message: "unable to hash input PDF".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: None,
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(3);
        }
    };
    let compatibility = CompatibilityKey {
        input_sha256,
        model: "ovisocr2".to_owned(),
        profile: "native".to_owned(),
        parameters: format!(
            "{};dpi={dpi};tokens={:?};max_image_tokens={};context_size={};device={};route=native",
            ocrx_output::pdf_compatibility_parameters(
                &options.pdf_mode,
                &options.quality_tier,
                "pdf-route-v1",
                "pdf-text-v1",
            ),
            options.max_output_tokens,
            options.max_image_tokens.unwrap_or(1024),
            options.context_size.unwrap_or(16_384),
            options.device
        ),
    };
    let mut initial_queue = if options.resume {
        recover_pending_ocr_queue(&output, &compatibility)
            .map(|recovered| {
                let recovered = restrict_ocr_queue_to_pages(&recovered, &pages);
                merge_ocr_queue(&initial_queue, &recovered)
            })
            .unwrap_or(initial_queue)
    } else {
        initial_queue
    };
    let mut resume_committed_markdown = Vec::new();
    let mut resume_skip_pages = std::collections::HashSet::new();
    let mut resume_pending_pages = std::collections::HashSet::new();
    let resume_effective_configuration = if options.resume {
        resolve_effective_ocr_configuration(&options.profile, &options.device).ok()
    } else {
        None
    };
    let resume_ocr_compatibility = resume_effective_configuration
        .as_ref()
        .map(|(profile, device)| make_ocr_compatibility(&compatibility, profile, device));
    if options.resume {
        for page in &pages {
            let path = output.join(format!("page-{page:04}.checkpoint"));
            let Ok(checkpoint) = read_checkpoint(&path) else {
                continue;
            };
            match checkpoint.resume_action(&compatibility, &output, *page as u32) {
                CheckpointResumeAction::SkipCommitted => {
                    if let Ok(mut markdown) = collect_page_markdowns(&output, &[*page as u32]) {
                        resume_committed_markdown.append(&mut markdown);
                        resume_skip_pages.insert(*page as u32);
                    }
                }
                CheckpointResumeAction::QueueOcr => {
                    resume_pending_pages.insert(*page as u32);
                }
                CheckpointResumeAction::Reprocess => {}
            }
            if let Some(expected_ocr) = &resume_ocr_compatibility {
                if checkpoint.resume_action(expected_ocr, &output, *page as u32)
                    == CheckpointResumeAction::SkipCommitted
                {
                    if let Ok(mut markdown) = collect_page_markdowns(&output, &[*page as u32]) {
                        resume_committed_markdown.append(&mut markdown);
                        resume_skip_pages.insert(*page as u32);
                    }
                }
            }
        }
    }
    initial_queue.retain(|page| !resume_skip_pages.contains(&page.page));
    let native_candidates = if options.pdf_mode == "ocr" {
        Vec::new()
    } else {
        native_candidate_pages(&pages, &classifications)
            .into_iter()
            .filter(|page| {
                !resume_skip_pages.contains(page) && !resume_pending_pages.contains(page)
            })
            .collect()
    };
    let resumed_completed_count = resume_committed_markdown.len() as u32;
    let mut native_markdown = resume_committed_markdown;
    let native_phase = run_native_first_phase(
        &options.pdf_mode,
        &native_candidates,
        |page| {
            let native = pdf
                .extract_native_page(&input, page.saturating_sub(1) as usize)
                .map_err(|error| error.to_string())?;
            ocrx_pdf::validate_native_candidate(page, &native, required_tier)
                .map(|_| ())
                .map_err(|reason| reason.as_str().to_owned())
        },
        |page| {
            let native = pdf
                .extract_native_page(&input, page.saturating_sub(1) as usize)
                .map_err(|error| error.to_string())?;
            let classification = pages
                .iter()
                .position(|selected| *selected as u32 == page)
                .and_then(|index| classifications.get(index).copied())
                .ok_or_else(|| "native_classification_missing".to_owned())?;
            let (markdown, page_json) =
                ocrx_pdf::native_pdf_page_artifacts(page, &native, classification, required_tier)
                    .map_err(|reason| reason.as_str().to_owned())?;
            let checkpoint = ocrx_output::PageCheckpoint {
                page_number: page,
                compatibility: compatibility.clone(),
                complete: true,
                output_sha256: String::new(),
                truncated: false,
                warnings: Vec::new(),
                audit: None,
            };
            let committed =
                commit_native_page_and_collect(&output, page, &markdown, &page_json, &checkpoint)
                    .map_err(|error| error.to_string())?;
            native_markdown.push(committed);
            Ok(())
        },
    );
    let native_phase = match native_phase {
        Ok(phase) => phase,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 3,
                    kind: "pdf_native".to_owned(),
                    message: "native PDF text extraction failed".to_owned(),
                    cause: Some(error),
                    suggestion: Some("use --pdf-mode auto to allow OCR fallback".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(3);
        }
    };
    let mut empty_markdown = Vec::new();
    let mut empty_pages_committed = 0usize;
    for (page, classification) in pages.iter().zip(&classifications) {
        if resume_skip_pages.contains(&(*page as u32)) {
            continue;
        }
        if classification.page_type != PageType::Empty {
            continue;
        }
        let (markdown, page_json) =
            match ocrx_pdf::empty_pdf_page_artifacts(*page as u32, *classification) {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    reporter.error(
                        &ErrorReport {
                            code: 5,
                            kind: "output".to_owned(),
                            message: "unable to build empty page artifact".to_owned(),
                            cause: Some(format!("{error:?}")),
                            suggestion: Some("inspect the output directory".to_owned()),
                        },
                        report_format,
                    );
                    print_reporter(reporter);
                    std::process::exit(5);
                }
            };
        let checkpoint = ocrx_output::PageCheckpoint {
            page_number: *page as u32,
            compatibility: compatibility.clone(),
            complete: true,
            output_sha256: String::new(),
            truncated: false,
            warnings: Vec::new(),
            audit: None,
        };
        match commit_native_page_and_collect(
            &output,
            *page as u32,
            &markdown,
            &page_json,
            &checkpoint,
        ) {
            Ok(committed) => {
                empty_markdown.push(committed);
                empty_pages_committed += 1;
            }
            Err(error) => {
                reporter.error(
                    &ErrorReport {
                        code: 5,
                        kind: "output".to_owned(),
                        message: "unable to commit empty page artifact".to_owned(),
                        cause: Some(error.to_string()),
                        suggestion: Some("inspect the output directory".to_owned()),
                    },
                    report_format,
                );
                print_reporter(reporter);
                std::process::exit(5);
            }
        }
    }
    for fallback in &native_phase.fallback_pages {
        let reason = fallback
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "native_validation_failed".to_owned());
        let checkpoint =
            ocrx_output::pending_ocr_checkpoint(fallback.page, compatibility.clone(), reason);
        if let Err(error) = ocrx_output::write_checkpoint(
            output.join(format!("page-{:04}.checkpoint", fallback.page)),
            &checkpoint,
        ) {
            reporter.error(
                &ErrorReport {
                    code: 5,
                    kind: "output".to_owned(),
                    message: "unable to persist OCR fallback checkpoint".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: Some("inspect the output directory".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(5);
        }
    }
    let final_ocr_queue = ocr_queue_after_native_phase(&initial_queue, &native_phase);
    let ocr_service_pages = service_pages_from_ocr_queue(&final_ocr_queue);
    let execution_plan = ScanExecutionPlan::from_final_ocr_queue(ocr_service_pages.len());
    if execution_plan.initialize_runtime {
        match resolve_and_prepare_model(
            true,
            &options.profile,
            &options.device,
            options.offline,
            options.non_interactive,
            &cache,
        ) {
            Ok((resolved_profile, resolved_device, resolved_model, resolved_mmproj)) => {
                profile = resolved_profile;
                device = resolved_device;
                model = resolved_model;
                mmproj = resolved_mmproj;
            }
            Err(error) => {
                let unavailable_reason = "ocr_required_but_unavailable".to_owned();
                for page in &ocr_service_pages {
                    let checkpoint = ocrx_output::pending_ocr_checkpoint(
                        *page,
                        compatibility.clone(),
                        unavailable_reason.clone(),
                    );
                    if let Err(write_error) = ocrx_output::write_checkpoint(
                        output.join(format!("page-{page:04}.checkpoint")),
                        &checkpoint,
                    ) {
                        reporter.error(
                            &ErrorReport {
                                code: 5,
                                kind: "output".to_owned(),
                                message: "unable to persist partial checkpoint".to_owned(),
                                cause: Some(write_error.to_string()),
                                suggestion: None,
                            },
                            report_format,
                        );
                        print_reporter(reporter);
                        std::process::exit(5);
                    }
                }
                let fallback_reasons = native_phase
                    .fallback_pages
                    .iter()
                    .filter_map(|page| {
                        page.fallback_reason
                            .as_deref()
                            .map(|reason| (page.page, pdf_reason_code_from_str(reason)))
                    })
                    .collect::<Vec<_>>();
                let mut fallback_reasons = fallback_reasons;
                for queued in &final_ocr_queue {
                    if let Some(reason) = queued.fallback_reason.as_deref()
                        && !fallback_reasons
                            .iter()
                            .any(|(page, _)| *page == queued.page)
                    {
                        fallback_reasons.push((queued.page, pdf_reason_code_from_str(reason)));
                    }
                }
                let unavailable_pages = ocr_service_pages.iter().copied().collect();
                if let Err(write_error) = write_routing_checkpoint_sidecars_with_states(
                    &output,
                    &options.pdf_mode,
                    &pages,
                    &classifications,
                    &fallback_reasons,
                    &std::collections::HashSet::new(),
                    &unavailable_pages,
                ) {
                    reporter.error(
                        &ErrorReport {
                            code: 5,
                            kind: "output".to_owned(),
                            message: "unable to persist partial routing sidecars".to_owned(),
                            cause: Some(write_error.to_string()),
                            suggestion: None,
                        },
                        report_format,
                    );
                    print_reporter(reporter);
                    std::process::exit(5);
                }
                let total_pages = pages.len() as u32;
                let completed_pages = native_phase.committed_native_pages.len() as u32
                    + empty_pages_committed as u32
                    + resumed_completed_count;
                let result_json = serialize_scan_result(&ScanResultFields {
                    status: "partial",
                    input: &canonical_input,
                    markdown_path: None,
                    output_dir: &output,
                    profile: &profile,
                    required_quality_tier: &options.quality_tier,
                    document_type: ocrx_pdf::summarize_document_type(&classifications).as_str(),
                    ocr_model_loaded: false,
                    backend: &device,
                    total_pages,
                    completed_pages,
                    failed_pages: ocr_service_pages.len() as u32,
                    truncated_pages: 0,
                    warnings: vec![unavailable_reason],
                    diagnostics: ocr_service_pages
                        .iter()
                        .map(|page| PageDiagnostic {
                            page: *page,
                            status: "pending".to_owned(),
                            stage: "model".to_owned(),
                            error_code: Some("ocr_required_but_unavailable".to_owned()),
                            message: Some("OCR model preparation failed".to_owned()),
                            audit: None,
                            warnings: vec!["ocr_required_but_unavailable".to_owned()],
                        })
                        .collect(),
                    pdf_routing: final_routing_diagnostics_with_pending(
                        &options.pdf_mode,
                        &pages,
                        &classifications,
                        &fallback_reasons,
                        &std::collections::HashSet::new(),
                        &unavailable_pages,
                    ),
                    routing_counts: routing_counts_for_mode(
                        &options.pdf_mode,
                        &pages,
                        &classifications,
                        &fallback_reasons,
                    ),
                    image_dir: &output.join("images"),
                    image_count: 0,
                    elapsed_ms: started.elapsed().as_millis(),
                });
                if let Err(write_error) =
                    ocrx_output::atomic_write(output.join("result.json"), result_json.as_bytes())
                {
                    reporter.error(
                        &ErrorReport {
                            code: 5,
                            kind: "output".to_owned(),
                            message: "unable to persist partial result".to_owned(),
                            cause: Some(write_error.to_string()),
                            suggestion: None,
                        },
                        report_format,
                    );
                    print_reporter(reporter);
                    std::process::exit(5);
                }
                reporter.error(
                    &ErrorReport {
                        code: 4,
                        kind: "model".to_owned(),
                        message: error,
                        cause: None,
                        suggestion: Some("install or repair the selected profile first".to_owned()),
                    },
                    report_format,
                );
                print_reporter(reporter);
                std::process::exit(4);
            }
        }
    }
    let ocr_compatibility = if execution_plan.initialize_runtime {
        make_ocr_compatibility(&compatibility, &profile, &device)
    } else {
        compatibility.clone()
    };
    if execution_plan.initialize_runtime {
        reporter.progress(
            &ProgressEvent::for_stage(
                ProgressStage::Rendering,
                None,
                0,
                ocr_service_pages.len() as u32,
            ),
            progress_mode,
        );
        reporter.progress(
            &ProgressEvent::for_stage(
                ProgressStage::Recognizing,
                None,
                0,
                ocr_service_pages.len() as u32,
            ),
            progress_mode,
        );
    }
    let cancellation = CancellationToken::new();
    install_sigint_handler(cancellation.clone());
    // W4.4 GPU report: opt-in so a normal scan does not spawn a background
    // sampler or write an extra file by default. Enabled only when OCR actually
    // runs on a GPU backend and OCRX_GPU_REPORT is explicitly set.
    let gpu_report_enabled = std::env::var_os("OCRX_GPU_REPORT").is_some() && device != "cpu";
    let vram_sampler = if gpu_report_enabled && execution_plan.initialize_runtime {
        start_vram_sampler()
    } else {
        None
    };
    let summary = if execution_plan.initialize_runtime {
        let loader = RuntimeLoader {
            model,
            mmproj,
            gpu: device != "cpu",
            max_image_tokens: options.max_image_tokens.unwrap_or(1024),
            context_size: options.context_size.unwrap_or(16_384),
        };
        let mut service = ScanService::new(renderer, loader, recognizer);
        service.run_with_checkpoints_cancelled(
            &ocr_service_pages,
            &output,
            &ocr_compatibility,
            options.resume,
            options.keep_pages,
            &cancellation,
        )
    } else {
        Ok(ocrx_output::JobSummary {
            selected_pages: 0,
            completed_pages: 0,
            failed_pages: 0,
            truncated_pages: 0,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            cancelled: false,
        })
    };
    let vram_peak_bytes = stop_vram_sampler(vram_sampler);
    if gpu_report_enabled && execution_plan.initialize_runtime && device != "cpu" {
        let scan_telemetry = *telemetry.borrow();
        let gpu_report = GpuResourceReport {
            backend: device.clone(),
            vram_peak_bytes,
            unified_memory_peak_bytes: None,
            load_time_ms: Some(scan_telemetry.load_time_ms),
            first_token_ms: Some(scan_telemetry.first_token_ms),
            output_tokens: u32::try_from(scan_telemetry.generated_tokens).unwrap_or(u32::MAX),
            output_tokens_per_sec: scan_telemetry.output_tokens_per_sec(),
            peak_memory_bytes: None,
        };
        if let Err(error) = ocrx_output::atomic_write(
            output.join("gpu-resource.json"),
            gpu_report.to_json().as_bytes(),
        ) {
            reporter.error(
                &ErrorReport {
                    code: 6,
                    kind: "output".to_owned(),
                    message: "gpu-resource.json commit failed".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: None,
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(6);
        }
    }
    match summary {
        Ok(summary) => {
            let mut terminal_pending_pages = resume_pending_pages.clone();
            for diagnostic in &summary.diagnostics {
                if diagnostic.status == "completed"
                    || diagnostic.status == "partial"
                    || diagnostic.status == "failed"
                {
                    terminal_pending_pages.remove(&diagnostic.page);
                }
            }
            if summary.failed_pages == 0 && summary.truncated_pages == 0 {
                for page in &ocr_service_pages {
                    terminal_pending_pages.remove(page);
                }
            }
            if summary.cancelled {
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
                print_reporter(reporter);
                std::process::exit(130);
            }
            {
                let mut all_pages = native_markdown;
                all_pages.extend(empty_markdown);
                if !ocr_service_pages.is_empty() {
                    match collect_verified_page_markdowns(
                        &output,
                        &ocr_service_pages,
                        &ocr_compatibility,
                    ) {
                        Ok(mut ocr_pages) => all_pages.append(&mut ocr_pages),
                        Err(error) => {
                            reporter.error(
                                &ErrorReport {
                                    code: 5,
                                    kind: "output".to_owned(),
                                    message: "unable to collect page artifacts".to_owned(),
                                    cause: Some(error.to_string()),
                                    suggestion: Some("inspect the output directory".to_owned()),
                                },
                                report_format,
                            );
                            print_reporter(reporter);
                            std::process::exit(5);
                        }
                    }
                }
                if all_pages.is_empty() {
                    let document = output.join("document.md");
                    if document.is_file() {
                        if let Err(error) = std::fs::remove_file(&document) {
                            reporter.error(
                                &ErrorReport {
                                    code: 5,
                                    kind: "output".to_owned(),
                                    message: "unable to remove stale document".to_owned(),
                                    cause: Some(error.to_string()),
                                    suggestion: Some("inspect the output directory".to_owned()),
                                },
                                report_format,
                            );
                            print_reporter(reporter);
                            std::process::exit(5);
                        }
                    }
                } else if let Err(error) = finalize_document(&output, all_pages) {
                    reporter.error(
                        &ErrorReport {
                            code: 5,
                            kind: "output".to_owned(),
                            message: "unable to finalize document".to_owned(),
                            cause: Some(error.to_string()),
                            suggestion: Some("inspect the output directory".to_owned()),
                        },
                        report_format,
                    );
                    print_reporter(reporter);
                    std::process::exit(5);
                }
            }
            let native_completed_pages = native_phase.committed_native_pages.len() as u32;
            let total_pages = pages.len() as u32;
            let completed_pages = native_completed_pages
                + empty_pages_committed as u32
                + resumed_completed_count
                + summary.completed_pages;
            let report = ResultReport {
                job_id: "scan".to_owned(),
                status: if summary.failed_pages == 0 && summary.truncated_pages == 0 {
                    ocrx_output::ResultStatus::Success
                } else {
                    ocrx_output::ResultStatus::Partial
                },
                total_pages,
                completed_pages,
                failed_pages: summary.failed_pages,
                output_dir: output.display().to_string(),
            };
            let fallback_reasons = native_phase
                .fallback_pages
                .iter()
                .filter_map(|page| {
                    page.fallback_reason
                        .as_deref()
                        .map(|reason| (page.page, pdf_reason_code_from_str(reason)))
                })
                .collect::<Vec<_>>();
            let mut fallback_reasons = fallback_reasons;
            for queued in &final_ocr_queue {
                if let Some(reason) = queued.fallback_reason.as_deref()
                    && !fallback_reasons
                        .iter()
                        .any(|(page, _)| *page == queued.page)
                {
                    fallback_reasons.push((queued.page, pdf_reason_code_from_str(reason)));
                }
            }
            if let Err(error) = write_routing_checkpoint_sidecars_with_states(
                &output,
                &options.pdf_mode,
                &pages,
                &classifications,
                &fallback_reasons,
                &terminal_pending_pages,
                &std::collections::HashSet::new(),
            ) {
                reporter.error(
                    &ErrorReport {
                        code: 6,
                        kind: "output".to_owned(),
                        message: "routing checkpoint commit failed".to_owned(),
                        cause: Some(error.to_string()),
                        suggestion: None,
                    },
                    report_format,
                );
                print_reporter(reporter);
                std::process::exit(6);
            }
            let markdown_output = output
                .join("document.md")
                .is_file()
                .then(|| output.join("document.md"));
            let result_json = serialize_scan_result(&ScanResultFields {
                status: if summary.failed_pages == 0 && summary.truncated_pages == 0 {
                    "success"
                } else {
                    "partial"
                },
                input: &canonical_input,
                markdown_path: markdown_output.as_deref(),
                output_dir: &output,
                profile: &profile,
                required_quality_tier: &options.quality_tier,
                document_type: ocrx_pdf::summarize_document_type(&classifications).as_str(),
                ocr_model_loaded: execution_plan.initialize_runtime,
                backend: &device,
                total_pages,
                completed_pages,
                failed_pages: summary.failed_pages,
                truncated_pages: summary.truncated_pages,
                warnings: summary.warnings.clone(),
                diagnostics: summary.diagnostics.clone(),
                pdf_routing: final_routing_diagnostics_with_pending(
                    &options.pdf_mode,
                    &pages,
                    &classifications,
                    &fallback_reasons,
                    &terminal_pending_pages,
                    &std::collections::HashSet::new(),
                ),
                routing_counts: routing_counts_for_mode(
                    &options.pdf_mode,
                    &pages,
                    &classifications,
                    &fallback_reasons,
                ),
                image_dir: &output.join("images"),
                image_count: std::fs::read_dir(output.join("images"))
                    .map(|entries| {
                        entries
                            .filter_map(Result::ok)
                            .filter(|entry| {
                                entry.path().is_file()
                                    && entry.path().extension().is_some_and(|ext| ext == "png")
                                    && entry.file_name().to_string_lossy().starts_with("page-")
                                    && entry.file_name().to_string_lossy().contains("-figure-")
                            })
                            .count()
                    })
                    .unwrap_or(0),
                elapsed_ms: started.elapsed().as_millis(),
            });
            if let Err(error) =
                ocrx_output::atomic_write(output.join("result.json"), result_json.as_bytes())
            {
                reporter.error(
                    &ErrorReport {
                        code: 6,
                        kind: "output".to_owned(),
                        message: "result.json commit failed".to_owned(),
                        cause: Some(error.to_string()),
                        suggestion: None,
                    },
                    report_format,
                );
                print_reporter(reporter);
                std::process::exit(6);
            }
            if report_format == ReportFormat::Json {
                reporter.stdout.push_str(&result_json);
                reporter.stdout.push('\n');
            } else {
                reporter.result(&report, report_format);
                let counts = routing_counts_for_mode(
                    &options.pdf_mode,
                    &pages,
                    &classifications,
                    &fallback_reasons,
                );
                reporter.stdout.push_str(&format_routing_summary(
                    ocrx_pdf::summarize_document_type(&classifications),
                    counts,
                ));
            }
            reporter.progress(
                &ProgressEvent::for_stage(
                    ProgressStage::Merging,
                    None,
                    summary.completed_pages,
                    summary.selected_pages,
                ),
                progress_mode,
            );
            print_reporter(reporter);
            std::process::exit(
                if summary.failed_pages == 0 && summary.truncated_pages == 0 {
                    0
                } else {
                    10
                },
            );
        }
        Err(error) => {
            let error_kind = match classify_pdf_error(&error.to_string()) {
                PdfErrorKind::NonPdf => "pdf_non_pdf",
                PdfErrorKind::Corrupt => "pdf_corrupt",
                PdfErrorKind::Encrypted => "pdf_encrypted",
                PdfErrorKind::DynamicLibrary => "pdf_dynamic_library",
                PdfErrorKind::Unknown => "runtime",
            };
            reporter.error(
                &ErrorReport {
                    code: 6,
                    kind: error_kind.to_owned(),
                    message: "scan initialization or execution failed".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: Some("check doctor output and runtime assets".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(6);
        }
    }
}

fn classify_input_error(input: &Path, message: &str) -> PdfErrorKind {
    let classified = classify_pdf_error(message);
    if classified != PdfErrorKind::Unknown || !message.to_ascii_lowercase().contains("formaterror")
    {
        return classified;
    }
    let mut header = [0_u8; 1024];
    let bytes_read = std::fs::File::open(input)
        .and_then(|mut file| file.read(&mut header))
        .unwrap_or_default();
    if header[..bytes_read]
        .windows(b"%PDF-".len())
        .any(|window| window == b"%PDF-")
    {
        PdfErrorKind::Corrupt
    } else {
        PdfErrorKind::NonPdf
    }
}

fn json_escape_path(path: &Path) -> String {
    let mut escaped = String::new();
    for character in path.to_string_lossy().chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn json_quote(value: &str) -> String {
    format!("\"{}\"", json_escape_path(Path::new(value)))
}

struct ScanResultFields<'a> {
    status: &'a str,
    input: &'a Path,
    markdown_path: Option<&'a Path>,
    output_dir: &'a Path,
    profile: &'a str,
    required_quality_tier: &'a str,
    document_type: &'a str,
    ocr_model_loaded: bool,
    backend: &'a str,
    total_pages: u32,
    completed_pages: u32,
    failed_pages: u32,
    truncated_pages: u32,
    warnings: Vec<String>,
    diagnostics: Vec<PageDiagnostic>,
    pdf_routing: Vec<String>,
    routing_counts: RoutingCounts,
    image_dir: &'a Path,
    image_count: usize,
    elapsed_ms: u128,
}

fn serialize_scan_result(fields: &ScanResultFields<'_>) -> String {
    let markdown = fields.markdown_path.map_or_else(
        || "null".to_owned(),
        |path| format!("\"{}\"", json_escape_path(path)),
    );
    let quantization = match fields.profile {
        "compact" => "Q4_K_M",
        "balanced" => "Q5_K_M",
        "quality" => "Q8_0",
        _ => "BF16",
    };
    let model = if fields.ocr_model_loaded {
        format!(
            "{{\"id\":\"ovisocr2\",\"name\":\"OvisOCR2\",\"family\":\"Ovis\",\"profile\":\"{}\",\"revision\":\"{}\",\"mmproj_revision\":\"{}\",\"adapter\":\"ovisocr2\",\"adapter_version\":\"0.1.0\",\"prompt_schema_version\":1,\"runtime\":\"llama.cpp@0.1.152\",\"quantization\":\"{}\"}}",
            fields.profile,
            ocrx_models::COMMUNITY_REVISION,
            ocrx_models::COMMUNITY_REVISION,
            quantization
        )
    } else {
        "null".to_owned()
    };
    let warnings = format!(
        "[{}]",
        fields
            .warnings
            .iter()
            .map(|warning| json_quote(warning))
            .collect::<Vec<_>>()
            .join(",")
    );
    let diagnostics = fields
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let audit = diagnostic.audit.map_or_else(|| "null".to_owned(), |audit| format!("{{\"original\":{{\"width\":{},\"height\":{}}},\"rendered\":{{\"width\":{},\"height\":{}}},\"model\":{{\"width\":{},\"height\":{}}},\"scaled\":{},\"scale_ratio\":{{\"numerator\":{},\"denominator\":{}}}}}", audit.original_width, audit.original_height, audit.rendered_width, audit.rendered_height, audit.model_width, audit.model_height, audit.scaled, audit.scale_ratio_numerator, audit.scale_ratio_denominator));
            let error_code = diagnostic.error_code.as_deref().map_or_else(|| "null".to_owned(), json_quote);
            let message = diagnostic.message.as_deref().map_or_else(|| "null".to_owned(), json_quote);
            let page_warnings = diagnostic.warnings.iter().map(|warning| json_quote(warning)).collect::<Vec<_>>().join(",");
            format!("{{\"page\":{},\"status\":{},\"stage\":{},\"error_code\":{},\"message\":{},\"audit\":{},\"warnings\":[{}]}}", diagnostic.page, json_quote(&diagnostic.status), json_quote(&diagnostic.stage), error_code, message, audit, page_warnings)
        })
        .collect::<Vec<_>>()
        .join(",");
    let pdf_routing = fields.pdf_routing.join(",");
    format!(
        "{{\"schema_version\":3,\"job_id\":\"scan\",\"status\":\"{}\",\"input\":\"{}\",\"markdown_path\":{},\"output_dir\":\"{}\",\"required_quality_tier\":\"{}\",\"document_type\":\"{}\",\"ocr_model_loaded\":{},\"model\":{},\"backend\":\"{}\",\"pages\":{{\"total\":{},\"completed\":{},\"failed\":{},\"truncated\":{}}},\"routing_counts\":{{\"native_text\":{},\"ocr\":{},\"empty\":{},\"fallback\":{}}},\"images\":{{\"directory\":\"{}\",\"count\":{}}},\"elapsed_ms\":{},\"warnings\":{},\"diagnostics\":[{}],\"pdf_routing\":[{}],\"routing_metadata\":[{}]}}",
        fields.status,
        json_escape_path(fields.input),
        markdown,
        json_escape_path(fields.output_dir),
        fields.required_quality_tier,
        fields.document_type,
        fields.ocr_model_loaded,
        model,
        fields.backend,
        fields.total_pages,
        fields.completed_pages,
        fields.failed_pages,
        fields.truncated_pages,
        fields.routing_counts.native_text,
        fields.routing_counts.ocr,
        fields.routing_counts.empty,
        fields.routing_counts.fallback,
        json_escape_path(fields.image_dir),
        fields.image_count,
        fields.elapsed_ms,
        warnings,
        diagnostics,
        pdf_routing,
        pdf_routing
    )
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    ocrx_core::sha256_file(path)
}

fn resolve_scan_profile(profile: &str, memory_gb: u64) -> Result<String, String> {
    if profile == "auto" {
        recommend_profile(memory_gb, &["compact", "balanced", "quality", "reference"])
            .map_err(|error| format!("{error:?}"))
    } else {
        Ok(profile.to_owned())
    }
}

fn resolve_progress_mode(progress: &str, log_level: &str) -> ProgressMode {
    if matches!(log_level, "error") && progress != "jsonl" {
        return ProgressMode::Off;
    }
    match progress {
        "jsonl" => ProgressMode::Jsonl,
        "bar" => ProgressMode::Bar,
        "plain" => ProgressMode::Plain,
        "off" => ProgressMode::Off,
        _ => {
            if std::io::stderr().is_terminal() {
                ProgressMode::Bar
            } else {
                ProgressMode::Plain
            }
        }
    }
}

fn install_sigint_handler(token: CancellationToken) {
    let _ = ctrlc::set_handler(move || {
        token.request_cancel();
        if token.force_exit() {
            std::process::exit(130);
        }
    });
}

fn doctor_runtime_info(snapshot: ResourceSnapshot) -> RuntimeInfo {
    RuntimeInfo {
        backend: if snapshot.metal_available {
            Backend::Metal
        } else {
            Backend::Cpu
        },
        image_token_limit: 1024,
    }
}

fn print_reporter(reporter: Reporter) {
    print!("{}", reporter.stdout);
    eprint!("{}", reporter.stderr);
}

fn print_help(topic: Option<&str>) {
    match topic {
        None => println!("ocrx <version|scan|doctor|model|skill|help>"),
        Some("scan") => {
            println!("ocrx scan <input.pdf> [options]");
            println!("  -o, --output-dir DIR");
            println!("  --pages SPEC                 page range, e.g. 1,3-5");
            println!("  --model ovisocr2");
            println!("  --model-profile auto|compact|balanced|quality|reference");
            println!("  --pdf-mode auto|text|ocr");
            println!("  --quality-tier l1|l2 (default l2)");
            println!("  --device auto|cpu|metal|cuda|vulkan");
            println!("  --report text|json");
            println!("  --dpi DPI                    render resolution (1-600, default 300)");
            println!("  --max-image-tokens N         image token budget (>=128, default 1024)");
            println!("  --max-output-tokens N        output token budget (>=128, default 4096)");
            println!("  --context-size N             context size (>=1024, default 16384)");
            println!("  --progress auto|bar|plain|jsonl|off");
            println!("  --log-level error|warn|info|debug|trace");
            println!("  --offline                    do not download");
            println!("  --non-interactive            disable interactive prompts");
            println!("  --keep-pages                 keep full-page PNGs after merge");
            println!("  --resume | --overwrite       mutually exclusive");
        }
        Some("doctor") => println!("ocrx doctor [--report json]"),
        Some("model") => {
            println!("ocrx model list");
            println!("ocrx model info [PROFILE]");
            println!("ocrx model recommend");
            println!("ocrx model install PROFILE [--offline] [--non-interactive]");
            println!("ocrx model remove PROFILE");
            println!("ocrx model verify PROFILE [--offline] [--non-interactive]");
        }
        Some("skill") => println!("ocrx skill <install|uninstall>"),
        Some(topic) => eprintln!("unknown help topic: {topic}"),
    }
}

fn resource_snapshot() -> ResourceSnapshot {
    let detected_memory = if cfg!(target_os = "macos") {
        std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(|bytes| bytes / 1024 / 1024 / 1024)
    } else {
        None
    };
    let memory_gb = resolve_config(
        std::env::var("OCRX_MEMORY_GB")
            .ok()
            .and_then(|value| value.parse().ok()),
        detected_memory,
        None,
        16,
    )
    .value;
    ResourceSnapshot {
        memory_gb,
        metal_available: cfg!(target_os = "macos"),
        cuda_available: ocrx_runtime::Backend::Cuda.is_compiled(),
        vulkan_available: ocrx_runtime::Backend::Vulkan.is_compiled(),
        cpu_available: true,
    }
}

fn doctor_json_report(snapshot: ResourceSnapshot, info: RuntimeInfo) -> String {
    let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".ocrx-model-cache"));
    doctor_json_report_with_cache(snapshot, info, cache_root)
}

fn doctor_json_report_with_cache(
    snapshot: ResourceSnapshot,
    info: RuntimeInfo,
    cache_root: PathBuf,
) -> String {
    doctor_json_report_with_paths(snapshot, info, cache_root, None)
}

fn doctor_json_report_with_paths(
    snapshot: ResourceSnapshot,
    info: RuntimeInfo,
    cache_root: PathBuf,
    home: Option<&Path>,
) -> String {
    let profile = recommend_profile(snapshot.memory_gb, &["compact", "balanced", "quality"])
        .unwrap_or_else(|_| "unavailable".to_owned());
    let device = recommend_device(snapshot, "auto").unwrap_or_else(|_| "unavailable".to_owned());
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unsupported"
    };
    let backend = match info.backend {
        Backend::Metal => "Metal",
        Backend::Cuda => "Cuda",
        Backend::Vulkan => "Vulkan",
        Backend::Cpu => "Cpu",
    };
    let installed_profiles = ["compact", "balanced", "quality", "reference"]
        .into_iter()
        .filter(|profile| {
            ocrx_models::community_manifest(profile)
                .and_then(|manifest| {
                    ocrx_models::cache_paths(&cache_root, &manifest.identity.revision, profile)
                })
                .map(|(model, mmproj)| model.is_file() && mmproj.is_file())
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let environment_home = std::env::var_os("HOME").map(PathBuf::from);
    let skill_installed = home
        .map(|home| home.join(".codex/skills/ocrx/SKILL.md").is_file())
        .or_else(|| {
            environment_home
                .as_deref()
                .map(|home| home.join(".codex/skills/ocrx/SKILL.md").is_file())
        })
        .unwrap_or(false);
    format!(
        "{{\"schema_version\":2,\"platform\":\"{platform}\",\"model\":\"ovisocr2\",\"backend\":\"{backend}\",\"image_token_limit\":{},\"memory_gb\":{},\"metal_available\":{},\"cpu_available\":{},\"cache_root\":\"{}\",\"installed_profiles\":[{}],\"recommended_profile\":\"{profile}\",\"recommended_device\":\"{device}\",\"skill_installed\":{},\"profiles\":[\"compact\",\"balanced\",\"quality\",\"reference\"]}}",
        info.image_token_limit,
        snapshot.memory_gb,
        snapshot.metal_available,
        snapshot.cpu_available,
        cache_root.display(),
        installed_profiles
            .iter()
            .map(|profile| format!("\"{profile}\""))
            .collect::<Vec<_>>()
            .join(","),
        skill_installed
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{
        CliCommand, ConfigSource, NativeFirstPhaseResult, NativeOnlyDecision,
        NativeValidationRoute, ResolvedConfig, ResourceSnapshot, RoutingCounts, ScanExecutionPlan,
        ScanOptions, ScanResultFields, build_final_ocr_queue, doctor_json_report,
        dynamic_fallback_requires_model_assets, final_routing_diagnostics, format_routing_summary,
        initial_ocr_queue, initial_ocr_queue_for_mode, native_candidate_outcome,
        native_candidate_pages, native_only_decision, ocr_queue_after_native_phase, parse_command,
        parse_page_selection, parse_scan_args, pdf_reason_code_from_str, planned_ocr_pages,
        prepare_model_if_required, resolve_config, restrict_ocr_queue_to_pages,
        route_native_validation, routing_counts, routing_counts_for_mode, routing_diagnostic_json,
        routing_diagnostic_with_processing, run_native_first_phase, service_pages_from_ocr_queue,
        should_initialize_ocr_runtime, try_routing_diagnostic_with_processing,
        validate_text_mode_classifications, with_ocr_runtime, write_routing_checkpoint_sidecars,
    };
    use ocrx_core::{
        PageClassification, PageType, PdfReasonCode, ProcessingMethod, RouteConfidence,
    };
    use ocrx_output::{OcrQueuePage, PageDiagnostic, RenderAudit, ResultReport, ResultStatus};

    #[test]
    fn scan_parser_builds_typed_options_and_rejects_other_models() {
        assert_eq!(
            parse_scan_args(&["input.pdf", "-o", "out", "--pages", "3,1-2", "--resume"]),
            Ok(ScanOptions {
                input: "input.pdf".to_owned(),
                output_dir: Some("out".to_owned()),
                pages: Some(vec![1, 2, 3]),
                model: "ovisocr2".to_owned(),
                profile: "auto".to_owned(),
                pdf_mode: "auto".to_owned(),
                quality_tier: "l2".to_owned(),
                resume: true,
                overwrite: false,
                device: "auto".to_owned(),
                report: "text".to_owned(),
                dpi: None,
                max_image_tokens: None,
                max_output_tokens: None,
                context_size: None,
                progress: "auto".to_owned(),
                log_level: "info".to_owned(),
                offline: false,
                non_interactive: false,
                keep_pages: false,
            })
        );
        assert!(parse_scan_args(&["input.pdf", "--model", "other"]).is_err());
    }

    #[test]
    fn text_mode_classification_validation_rejects_non_text_and_allows_empty() {
        let classification = |page_type, method| PageClassification {
            page_type,
            confidence: RouteConfidence::High,
            recommended_method: method,
            reason: PdfReasonCode::NativeTextReliable,
        };
        assert!(
            validate_text_mode_classifications(&[
                classification(PageType::Text, ProcessingMethod::NativeText),
                classification(PageType::Empty, ProcessingMethod::Empty),
            ])
            .is_ok()
        );
        assert!(
            validate_text_mode_classifications(&[classification(
                PageType::Image,
                ProcessingMethod::Ocr,
            )])
            .is_err()
        );
    }

    #[test]
    fn scan_parser_rejects_resume_overwrite_and_missing_values() {
        assert!(parse_scan_args(&["input.pdf", "--resume", "--overwrite"]).is_err());
        assert!(parse_scan_args(&["input.pdf", "--pages"]).is_err());
    }

    #[test]
    fn scan_parser_accepts_pdf_routing_options_and_rejects_unknown_values() {
        let parsed =
            parse_scan_args(&["input.pdf", "--pdf-mode", "text", "--quality-tier", "l1"]).unwrap();
        assert_eq!(parsed.pdf_mode, "text");
        assert_eq!(parsed.quality_tier, "l1");
        assert!(parse_scan_args(&["input.pdf", "--pdf-mode", "fast"]).is_err());
        assert!(parse_scan_args(&["input.pdf", "--quality-tier", "l3"]).is_err());
    }

    #[test]
    fn model_preparation_is_required_only_for_planned_ocr_pages() {
        let text = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let scan = ocrx_core::PageClassification {
            page_type: PageType::Image,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::Ocr,
            reason: ocrx_core::PdfReasonCode::ImageCoverageHigh,
        };
        assert!(!planned_ocr_pages("auto", &[text]));
        assert!(planned_ocr_pages("auto", &[text, scan]));
        assert!(planned_ocr_pages("ocr", &[text]));
    }

    #[test]
    fn dynamic_fallback_requires_assets_only_after_new_ocr_work_appears() {
        assert!(!dynamic_fallback_requires_model_assets(0, 0, false));
        assert!(!dynamic_fallback_requires_model_assets(0, 1, true));
        assert!(dynamic_fallback_requires_model_assets(0, 1, false));
        assert!(!dynamic_fallback_requires_model_assets(1, 1, false));
    }

    #[test]
    fn prepare_model_if_required_is_noop_without_ocr_or_with_valid_assets() {
        assert!(
            prepare_model_if_required(false, false, true, true, "balanced", Path::new("/missing"))
                .is_ok()
        );
        assert!(
            prepare_model_if_required(true, true, true, true, "balanced", Path::new("/missing"))
                .is_ok()
        );
    }

    #[test]
    fn initial_ocr_queue_preserves_selected_page_numbers_and_skips_native_pages() {
        let native = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let ocr = ocrx_core::PageClassification {
            page_type: PageType::Image,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::Ocr,
            reason: ocrx_core::PdfReasonCode::ImageCoverageHigh,
        };
        assert_eq!(
            initial_ocr_queue(&[4, 9], &[native, ocr]),
            vec![ocrx_output::OcrQueuePage {
                page: 9,
                fallback_reason: None
            }]
        );
    }

    #[test]
    fn ocr_mode_places_all_non_empty_selected_pages_in_initial_queue() {
        let text = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let empty = ocrx_core::PageClassification {
            page_type: PageType::Empty,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::Empty,
            reason: ocrx_core::PdfReasonCode::NoVisibleContent,
        };

        let queue = initial_ocr_queue_for_mode("ocr", &[1, 2], &[text, empty]);

        assert_eq!(
            queue.iter().map(|page| page.page).collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn native_candidate_outcome_turns_validation_failure_into_a_reasoned_fallback() {
        assert!(native_candidate_outcome(1, Ok(())).is_ok());
        assert_eq!(
            native_candidate_outcome(8, Err("native_structure_unresolved".to_owned())),
            Err(OcrQueuePage {
                page: 8,
                fallback_reason: Some("native_structure_unresolved".to_owned())
            })
        );
    }

    #[test]
    fn pdf_reason_code_mapping_preserves_stable_fallback_values() {
        assert_eq!(
            pdf_reason_code_from_str("native_structure_unresolved"),
            ocrx_core::PdfReasonCode::NativeStructureUnresolved
        );
        assert_eq!(
            pdf_reason_code_from_str("unknown_reason"),
            ocrx_core::PdfReasonCode::NativeValidationFailed
        );
    }

    #[test]
    fn routing_diagnostic_includes_classification_and_planned_processing() {
        let classification = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let json = routing_diagnostic_json(4, classification);
        assert!(json.contains("\"page\":4"));
        assert!(json.contains("\"page_type\":\"text\""));
        assert!(json.contains("\"planned_method\":\"native_text\""));
        assert!(json.contains("\"actual_method\":null"));
    }

    #[test]
    fn routing_diagnostic_records_actual_method_and_fallback_reason() {
        let classification = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let native = routing_diagnostic_with_processing(
            1,
            classification,
            ProcessingMethod::NativeText,
            None,
        );
        assert!(native.contains("\"actual_method\":\"native_text\""));
        let fallback = routing_diagnostic_with_processing(
            1,
            classification,
            ProcessingMethod::Ocr,
            Some(ocrx_core::PdfReasonCode::NativeStructureUnresolved),
        );
        assert!(fallback.contains("\"actual_method\":\"ocr\""));
        assert!(fallback.contains("\"fallback_reason\":\"native_structure_unresolved\""));
    }

    #[test]
    fn final_routing_diagnostics_assigns_actual_methods_and_dynamic_fallbacks() {
        let native = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let scan = ocrx_core::PageClassification {
            page_type: PageType::Image,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::Ocr,
            reason: ocrx_core::PdfReasonCode::ImageCoverageHigh,
        };
        let result = final_routing_diagnostics(
            "auto",
            &[2, 5],
            &[native, scan],
            &[(2, ocrx_core::PdfReasonCode::NativeStructureUnresolved)],
        );
        assert!(result[0].contains("\"actual_method\":\"ocr\""));
        assert!(result[0].contains("native_structure_unresolved"));
        assert!(result[1].contains("\"actual_method\":\"ocr\""));
    }

    #[test]
    fn routing_checkpoint_sidecars_are_written_for_each_selected_page() {
        let root =
            std::env::temp_dir().join(format!("ocrx-routing-sidecars-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let classification = PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
        write_routing_checkpoint_sidecars(&root, "auto", &[2], &[classification], &[]).unwrap();
        let json = std::fs::read_to_string(root.join("page-0002.routing.json")).unwrap();
        assert!(json.contains("\"schema\":\"ocrx-routing-checkpoint-v1\""));
        assert!(json.contains("\"page\":2"));
        assert!(json.contains("\"processing\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn forced_ocr_routing_records_ocr_as_planned_and_actual_method() {
        let classification = PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
        let result = final_routing_diagnostics("ocr", &[1], &[classification], &[]);
        assert!(result[0].contains("\"recommended_method\":\"native_text\""));
        assert!(result[0].contains("\"planned_method\":\"ocr\""));
        assert!(result[0].contains("\"actual_method\":\"ocr\""));
        assert!(result[0].contains("\"page_type\":\"text\""));
    }

    #[test]
    fn routing_counts_separate_native_ocr_empty_and_fallback_pages() {
        let classifications = vec![
            PageClassification {
                page_type: PageType::Text,
                confidence: RouteConfidence::High,
                recommended_method: ProcessingMethod::NativeText,
                reason: PdfReasonCode::NativeTextReliable,
            },
            PageClassification {
                page_type: PageType::Image,
                confidence: RouteConfidence::High,
                recommended_method: ProcessingMethod::Ocr,
                reason: PdfReasonCode::ImageCoverageHigh,
            },
            PageClassification {
                page_type: PageType::Empty,
                confidence: RouteConfidence::High,
                recommended_method: ProcessingMethod::Empty,
                reason: PdfReasonCode::NoVisibleContent,
            },
        ];
        let counts = routing_counts(
            &[1, 2, 3],
            &classifications,
            &[(1, PdfReasonCode::NativeValidationFailed)],
        );
        assert_eq!(
            counts,
            RoutingCounts {
                native_text: 0,
                ocr: 2,
                empty: 1,
                fallback: 1,
            }
        );
    }

    #[test]
    fn forced_ocr_counts_non_empty_pages_as_ocr() {
        let classifications = vec![
            PageClassification {
                page_type: PageType::Text,
                confidence: RouteConfidence::High,
                recommended_method: ProcessingMethod::NativeText,
                reason: PdfReasonCode::NativeTextReliable,
            },
            PageClassification {
                page_type: PageType::Empty,
                confidence: RouteConfidence::High,
                recommended_method: ProcessingMethod::Empty,
                reason: PdfReasonCode::NoVisibleContent,
            },
        ];
        let counts = routing_counts_for_mode("ocr", &[5, 6], &classifications, &[]);
        assert_eq!(counts.native_text, 0);
        assert_eq!(counts.ocr, 1);
        assert_eq!(counts.empty, 1);
    }

    #[test]
    fn text_routing_summary_is_machine_stable() {
        assert_eq!(
            format_routing_summary(
                ocrx_pdf::DocumentType::Mixed,
                RoutingCounts {
                    native_text: 2,
                    ocr: 3,
                    empty: 1,
                    fallback: 1,
                }
            ),
            "document_type=mixed native_text=2 ocr=3 empty=1 fallback=1\n"
        );
    }

    #[test]
    fn runtime_initialization_depends_only_on_final_ocr_queue() {
        assert!(!should_initialize_ocr_runtime(0));
        assert!(should_initialize_ocr_runtime(1));
        assert!(should_initialize_ocr_runtime(7));
    }

    #[test]
    fn execution_plan_distinguishes_native_only_from_ocr_work() {
        let native = ScanExecutionPlan::from_final_ocr_queue(0);
        assert!(native.native_only());
        assert!(!native.initialize_runtime);
        let mixed = ScanExecutionPlan::from_final_ocr_queue(2);
        assert!(!mixed.native_only());
        assert!(mixed.initialize_runtime);
    }

    #[test]
    fn runtime_preparation_callback_is_not_called_for_native_only_plan() {
        let native = ScanExecutionPlan::from_final_ocr_queue(0);
        let mut called = false;
        assert_eq!(
            with_ocr_runtime(native, || {
                called = true;
                7
            }),
            None
        );
        assert!(!called);
        let mixed = ScanExecutionPlan::from_final_ocr_queue(1);
        assert_eq!(with_ocr_runtime(mixed, || 7), Some(7));
    }

    #[test]
    fn native_validation_route_respects_auto_fallback_and_text_failure() {
        assert_eq!(
            route_native_validation("auto", 3, Ok(())),
            NativeValidationRoute::Native
        );
        assert_eq!(
            route_native_validation("auto", 3, Err("bad_structure".to_owned())),
            NativeValidationRoute::OcrFallback(OcrQueuePage {
                page: 3,
                fallback_reason: Some("bad_structure".to_owned())
            })
        );
        assert_eq!(
            route_native_validation("text", 3, Err("bad_structure".to_owned())),
            NativeValidationRoute::TextFailure("bad_structure".to_owned())
        );
    }

    #[test]
    fn final_ocr_queue_merges_only_auto_fallbacks_in_page_order() {
        let initial = vec![OcrQueuePage {
            page: 5,
            fallback_reason: None,
        }];
        let routes = vec![
            (2, NativeValidationRoute::Native),
            (
                3,
                NativeValidationRoute::OcrFallback(OcrQueuePage {
                    page: 3,
                    fallback_reason: Some("bad".to_owned()),
                }),
            ),
            (4, NativeValidationRoute::TextFailure("fail".to_owned())),
            (
                5,
                NativeValidationRoute::OcrFallback(OcrQueuePage {
                    page: 5,
                    fallback_reason: Some("late".to_owned()),
                }),
            ),
        ];
        assert_eq!(
            build_final_ocr_queue(&initial, &routes),
            vec![
                OcrQueuePage {
                    page: 3,
                    fallback_reason: Some("bad".to_owned())
                },
                OcrQueuePage {
                    page: 5,
                    fallback_reason: Some("late".to_owned())
                }
            ]
        );
    }

    #[test]
    fn routing_queues_sort_and_deduplicate_pages_at_their_boundary() {
        let ocr = ocrx_core::PageClassification {
            page_type: PageType::Image,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::Ocr,
            reason: PdfReasonCode::ImageCoverageHigh,
        };
        let native = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: PdfReasonCode::NativeTextReliable,
        };
        assert_eq!(
            initial_ocr_queue_for_mode("auto", &[9, 4, 9], &[ocr, native, ocr])
                .into_iter()
                .map(|page| page.page)
                .collect::<Vec<_>>(),
            vec![9]
        );
        assert_eq!(
            native_candidate_pages(&[9, 4, 9], &[ocr, native, ocr]),
            vec![4]
        );
        assert!(initial_ocr_queue_for_mode("auto", &[2], &[native]).is_empty());
    }

    #[test]
    fn native_first_phase_commits_success_and_collects_auto_fallbacks() {
        let mut committed = Vec::new();
        let result = run_native_first_phase(
            "auto",
            &[1, 2],
            |page| {
                if page == 2 {
                    Err("bad_structure".to_owned())
                } else {
                    Ok(())
                }
            },
            |page| {
                committed.push(page);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(committed, vec![1]);
        assert_eq!(result.committed_native_pages, vec![1]);
        assert_eq!(
            result.fallback_pages,
            vec![OcrQueuePage {
                page: 2,
                fallback_reason: Some("bad_structure".to_owned())
            }]
        );
    }

    #[test]
    fn fake_runtime_auto_pipeline_runs_native_then_ocr_for_fallback_pages() {
        let initial = initial_ocr_queue_for_mode(
            "auto",
            &[1, 2, 3],
            &[
                PageClassification {
                    page_type: PageType::Text,
                    confidence: RouteConfidence::High,
                    recommended_method: ProcessingMethod::NativeText,
                    reason: PdfReasonCode::NativeTextReliable,
                },
                PageClassification {
                    page_type: PageType::Image,
                    confidence: RouteConfidence::High,
                    recommended_method: ProcessingMethod::Ocr,
                    reason: PdfReasonCode::ImageCoverageHigh,
                },
                PageClassification {
                    page_type: PageType::Text,
                    confidence: RouteConfidence::High,
                    recommended_method: ProcessingMethod::NativeText,
                    reason: PdfReasonCode::NativeTextReliable,
                },
            ],
        );
        let mut native = Vec::new();
        let phase = run_native_first_phase(
            "auto",
            &[1, 3],
            |page| {
                (page != 3)
                    .then_some(())
                    .ok_or_else(|| "bad_structure".to_owned())
            },
            |page| {
                native.push(page);
                Ok(())
            },
        )
        .unwrap();
        let routes = vec![
            (1, NativeValidationRoute::Native),
            (
                3,
                NativeValidationRoute::OcrFallback(phase.fallback_pages[0].clone()),
            ),
        ];
        let queue = build_final_ocr_queue(&initial, &routes);
        assert_eq!(native, vec![1]);
        assert_eq!(
            queue.iter().map(|page| page.page).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(should_initialize_ocr_runtime(queue.len()));
        assert_eq!(service_pages_from_ocr_queue(&queue), vec![2, 3]);
    }

    #[test]
    fn native_first_phase_text_failure_stops_without_commit() {
        let mut committed = Vec::new();
        let error = run_native_first_phase(
            "text",
            &[2],
            |_| Err("bad_structure".to_owned()),
            |page| {
                committed.push(page);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("page 2"));
        assert!(committed.is_empty());
    }

    #[test]
    fn native_only_decision_only_continues_when_fallback_pages_exist() {
        assert_eq!(
            native_only_decision(&NativeFirstPhaseResult {
                committed_native_pages: vec![1, 2],
                fallback_pages: Vec::new()
            }),
            NativeOnlyDecision::Complete
        );
        assert_eq!(
            native_only_decision(&NativeFirstPhaseResult {
                committed_native_pages: vec![1],
                fallback_pages: vec![OcrQueuePage {
                    page: 2,
                    fallback_reason: Some("bad".to_owned())
                }]
            }),
            NativeOnlyDecision::ContinueToOcr
        );
    }

    #[test]
    fn ocr_queue_after_native_phase_merges_initial_and_dynamic_pages() {
        let phase = NativeFirstPhaseResult {
            committed_native_pages: vec![1],
            fallback_pages: vec![OcrQueuePage {
                page: 3,
                fallback_reason: Some("bad".to_owned()),
            }],
        };
        assert_eq!(
            ocr_queue_after_native_phase(
                &[OcrQueuePage {
                    page: 2,
                    fallback_reason: None
                }],
                &phase
            ),
            vec![
                OcrQueuePage {
                    page: 2,
                    fallback_reason: None
                },
                OcrQueuePage {
                    page: 3,
                    fallback_reason: Some("bad".to_owned())
                }
            ]
        );
    }

    #[test]
    fn native_candidate_pages_select_only_native_recommendations() {
        let native = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        let ocr = ocrx_core::PageClassification {
            page_type: PageType::Image,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::Ocr,
            reason: ocrx_core::PdfReasonCode::ImageCoverageHigh,
        };
        assert_eq!(native_candidate_pages(&[3, 7], &[native, ocr]), vec![3]);
    }

    #[test]
    fn service_pages_are_sorted_and_deduplicated_from_queue_only() {
        let queue = vec![
            OcrQueuePage {
                page: 8,
                fallback_reason: None,
            },
            OcrQueuePage {
                page: 2,
                fallback_reason: Some("bad".to_owned()),
            },
            OcrQueuePage {
                page: 8,
                fallback_reason: Some("duplicate".to_owned()),
            },
        ];
        assert_eq!(service_pages_from_ocr_queue(&queue), vec![2, 8]);
    }

    #[test]
    fn recovered_ocr_queue_is_restricted_to_selected_pages() {
        let queue = vec![
            OcrQueuePage {
                page: 1,
                fallback_reason: None,
            },
            OcrQueuePage {
                page: 2,
                fallback_reason: Some("native_failed".to_owned()),
            },
            OcrQueuePage {
                page: 4,
                fallback_reason: None,
            },
        ];
        let restricted = restrict_ocr_queue_to_pages(&queue, &[1, 4]);
        assert_eq!(
            restricted.iter().map(|page| page.page).collect::<Vec<_>>(),
            vec![1, 4]
        );
    }

    #[test]
    fn routing_diagnostic_validation_rejects_ocr_without_fallback_reason() {
        let classification = ocrx_core::PageClassification {
            page_type: PageType::Text,
            confidence: ocrx_core::RouteConfidence::High,
            recommended_method: ProcessingMethod::NativeText,
            reason: ocrx_core::PdfReasonCode::NativeTextReliable,
        };
        assert!(
            try_routing_diagnostic_with_processing(1, classification, ProcessingMethod::Ocr, None)
                .is_err()
        );
        assert!(
            try_routing_diagnostic_with_processing(
                1,
                classification,
                ProcessingMethod::Ocr,
                Some(ocrx_core::PdfReasonCode::NativeValidationFailed)
            )
            .is_ok()
        );
    }

    #[test]
    fn scan_parser_validates_enum_and_positive_numeric_options() {
        let parsed = parse_scan_args(&[
            "input.pdf",
            "--device",
            "metal",
            "--dpi",
            "300",
            "--report",
            "json",
        ])
        .unwrap();
        assert_eq!(parsed.device, "metal");
        assert_eq!(parsed.dpi, Some(300));
        assert_eq!(parsed.report, "json");
        assert!(parse_scan_args(&["input.pdf", "--device", "quantum"]).is_err());
        assert!(parse_scan_args(&["input.pdf", "--dpi", "0"]).is_err());
    }

    fn run_cli(args: &[&str]) -> std::process::Output {
        if let Ok(binary) = std::env::var("CARGO_BIN_EXE_ocrx") {
            return Command::new(binary)
                .args(args)
                .output()
                .expect("CLI test binary should run");
        }
        Command::new("cargo")
            .args(["run", "--quiet", "--bin", "ocrx", "--"])
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo should launch the CLI test binary")
    }

    #[test]
    fn cli_json_success_has_exactly_one_stdout_object() {
        let output = run_cli(&["doctor", "--report", "json"]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1);
        assert!(stdout.starts_with('{') && stdout.ends_with("}\n"));
        assert!(String::from_utf8(output.stderr).unwrap().is_empty());
    }

    #[test]
    fn cli_parameter_error_has_no_stdout_and_stable_exit_code() {
        let output = run_cli(&["scan", "input.pdf", "--model", "other"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("ovisocr2") || stderr.contains("model"));
    }

    #[test]
    fn cli_model_profile_commands_require_a_profile() {
        for action in ["install", "remove", "verify"] {
            let output = run_cli(&["model", action]);
            assert_eq!(output.status.code(), Some(2), "model {action}");
            assert!(output.stdout.is_empty(), "model {action}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.contains("requires a profile"),
                "model {action}: {stderr}"
            );
        }
    }

    #[test]
    fn cli_model_remove_rejects_unsupported_flags() {
        for args in [
            &["model", "remove", "compact", "--offline"][..],
            &["model", "remove", "compact", "--non-interactive"][..],
        ] {
            let output = run_cli(args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            assert!(output.stdout.is_empty(), "{args:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains("does not support"), "{args:?}: {stderr}");
        }
    }

    #[test]
    fn cli_initialization_error_is_one_json_error_object() {
        let output = run_cli(&["scan", "input.pdf", "--report", "json"]);
        assert_eq!(output.status.code(), Some(3));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1);
        assert!(stdout.contains("\"schema_version\":1"));
        assert!(!stdout.contains("input.pdf"));
    }

    #[test]
    fn scan_parser_validates_model_profiles() {
        let parsed = parse_scan_args(&["input.pdf", "--model-profile", "quality"]).unwrap();
        assert_eq!(parsed.profile, "quality");
        assert!(parse_scan_args(&["input.pdf", "--model-profile", "experimental"]).is_err());
    }

    #[test]
    fn config_resolution_uses_declared_precedence_and_audits_source() {
        assert_eq!(
            resolve_config(Some("cli"), Some("env"), Some("user"), "default"),
            ResolvedConfig {
                value: "cli",
                source: ConfigSource::Cli
            }
        );
        assert_eq!(
            resolve_config(None, Some("env"), Some("user"), "default"),
            ResolvedConfig {
                value: "env",
                source: ConfigSource::Environment
            }
        );
        assert_eq!(
            resolve_config(None, None, Some("user"), "default"),
            ResolvedConfig {
                value: "user",
                source: ConfigSource::UserConfig
            }
        );
        assert_eq!(
            resolve_config::<&str>(None, None, None, "default").source,
            ConfigSource::Default
        );
    }

    #[test]
    fn output_directory_defaults_to_input_stem_and_becomes_absolute() {
        let output = super::resolve_output_dir("docs/paper.pdf", None).unwrap();
        assert!(output.is_absolute());
        assert!(output.ends_with("docs/paper-ocr"));

        let explicit = super::resolve_output_dir("paper.pdf", Some("out")).unwrap();
        assert!(explicit.is_absolute());
        assert!(explicit.ends_with("out"));
        assert!(super::resolve_output_dir("paper.pdf", Some(" ")).is_err());
    }

    #[test]
    fn output_policy_rejects_input_and_protected_overlap() {
        let input = Path::new("/work/input.pdf");
        let cache = Path::new("/work/cache");
        assert!(super::validate_output_policy(input, Path::new("relative"), &[cache]).is_err());
        assert!(
            super::validate_output_policy(input, Path::new("/work/input.pdf/out"), &[cache])
                .is_err()
        );
        assert!(
            super::validate_output_policy(input, Path::new("/work/cache/job"), &[cache]).is_err()
        );
        assert!(super::validate_output_policy(input, Path::new("/tmp/result"), &[cache]).is_ok());
    }

    #[test]
    fn cli_result_report_uses_the_stable_reporter_contract() {
        let report = ResultReport {
            job_id: "job-1".to_owned(),
            status: ResultStatus::Success,
            total_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            output_dir: "/tmp/result".to_owned(),
        };
        assert!(
            super::format_cli_result(&report, "json")
                .unwrap()
                .contains("schema_version")
        );
        assert!(super::format_cli_result(&report, "csv").is_err());
    }

    #[test]
    fn scan_result_json_preserves_truncation_and_escapes_paths() {
        let input = Path::new("/tmp/input\tname\n.pdf");
        let json = super::serialize_scan_result(&ScanResultFields {
            status: "partial",
            input,
            markdown_path: None,
            output_dir: Path::new("/tmp/output"),
            profile: "balanced",
            required_quality_tier: "l2",
            document_type: "mixed",
            ocr_model_loaded: true,
            backend: "cpu",
            total_pages: 2,
            completed_pages: 2,
            failed_pages: 0,
            truncated_pages: 1,
            warnings: vec!["truncated".to_owned()],
            diagnostics: Vec::new(),
            pdf_routing: Vec::new(),
            routing_counts: RoutingCounts {
                native_text: 1,
                ocr: 1,
                empty: 0,
                fallback: 1,
            },
            image_dir: Path::new("/tmp/output/images"),
            image_count: 3,
            elapsed_ms: 42,
        });

        assert!(json.contains("\"status\":\"partial\""));
        assert!(json.contains("\"truncated\":1"));
        assert!(json.contains("\"document_type\":\"mixed\""));
        assert!(json.contains("\"warnings\":[\"truncated\"]"));
        assert!(json.contains(
            "\"routing_counts\":{\"native_text\":1,\"ocr\":1,\"empty\":0,\"fallback\":1}"
        ));
        assert!(json.contains("/tmp/input\\tname\\n.pdf"));
    }

    #[test]
    fn scan_result_json_exposes_routing_metadata_alias() {
        let routing = "{\"page\":3,\"classification\":{},\"processing\":{\"planned_method\":\"native_text\",\"actual_method\":\"ocr\",\"fallback_reason\":\"native_validation_failed\"}}".to_owned();
        let json = super::serialize_scan_result(&ScanResultFields {
            status: "partial",
            input: Path::new("/tmp/input.pdf"),
            markdown_path: None,
            output_dir: Path::new("/tmp/output"),
            profile: "quality",
            required_quality_tier: "l2",
            document_type: "mixed",
            ocr_model_loaded: true,
            backend: "cpu",
            total_pages: 1,
            completed_pages: 0,
            failed_pages: 1,
            truncated_pages: 0,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            pdf_routing: vec![routing],
            routing_counts: RoutingCounts {
                native_text: 0,
                ocr: 1,
                empty: 0,
                fallback: 1,
            },
            image_dir: Path::new("/tmp/output/images"),
            image_count: 0,
            elapsed_ms: 1,
        });
        assert!(json.contains("\"routing_metadata\":[{\"page\":3"));
        assert!(json.contains("native_validation_failed"));
    }

    #[test]
    fn scan_result_json_preserves_page_diagnostic_warnings_and_audit() {
        let json = super::serialize_scan_result(&ScanResultFields {
            status: "partial",
            input: Path::new("/tmp/input.pdf"),
            markdown_path: None,
            output_dir: Path::new("/tmp/output"),
            profile: "balanced",
            required_quality_tier: "l2",
            document_type: "text",
            ocr_model_loaded: false,
            backend: "cpu",
            total_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            truncated_pages: 1,
            warnings: vec!["render_scaled".to_owned(), "truncated".to_owned()],
            diagnostics: vec![PageDiagnostic {
                page: 1,
                status: "partial".to_owned(),
                stage: "recognize".to_owned(),
                error_code: None,
                message: Some("warning \"detail\"\\safe\n".to_owned()),
                audit: Some(RenderAudit {
                    original_width: 100,
                    original_height: 200,
                    rendered_width: 80,
                    rendered_height: 160,
                    model_width: 64,
                    model_height: 128,
                    scaled: true,
                    scale_ratio_numerator: 4,
                    scale_ratio_denominator: 5,
                }),
                warnings: vec!["truncated".to_owned(), "render_scaled".to_owned()],
            }],
            pdf_routing: Vec::new(),
            routing_counts: RoutingCounts {
                native_text: 0,
                ocr: 0,
                empty: 1,
                fallback: 0,
            },
            image_dir: Path::new("/tmp/output/images"),
            image_count: 0,
            elapsed_ms: 1,
        });

        assert!(json.contains("\"warnings\":[\"truncated\",\"render_scaled\"]"));
        assert!(json.contains("\"ocr_model_loaded\":false"));
        assert!(json.contains("\"model\":null"));
        assert!(json.contains("\"scaled\":true"));
        assert!(json.contains("warning \\\"detail\\\"\\\\safe\\n"));
        assert!(json.contains("\"scale_ratio\":{\"numerator\":4,\"denominator\":5}"));
    }

    #[test]
    fn linear_scale_ratio_uses_reduced_linear_width_ratio() {
        assert_eq!(super::linear_scale_ratio(64, 80), (4, 5));
        assert_eq!(super::linear_scale_ratio(80, 80), (1, 1));
        assert_eq!(super::linear_scale_ratio(63, 80), (63, 80));
    }

    #[test]
    fn numeric_options_enforce_runtime_safe_ranges() {
        assert!(
            super::validate_numeric_options(Some(300), Some(1024), Some(4096), Some(8192)).is_ok()
        );
        assert!(super::validate_numeric_options(Some(601), None, None, None).is_err());
        assert!(super::validate_numeric_options(None, Some(64), None, None).is_err());
        assert!(super::validate_numeric_options(None, None, None, Some(512)).is_err());
    }

    #[test]
    fn doctor_json_report_has_stable_golden_shape() {
        let empty = std::env::temp_dir().join(format!("ocrx-doctor-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        let report = super::doctor_json_report_with_paths(
            ResourceSnapshot {
                memory_gb: 16,
                metal_available: true,
                cuda_available: false,
                vulkan_available: false,
                cpu_available: true,
            },
            ocrx_runtime::RuntimeInfo {
                backend: ocrx_runtime::Backend::Metal,
                image_token_limit: 1024,
            },
            empty.clone(),
            Some(&empty),
        );
        let _ = std::fs::remove_dir_all(&empty);
        assert_eq!(
            report,
            format!(
                "{{\"schema_version\":2,\"platform\":\"macos\",\"model\":\"ovisocr2\",\"backend\":\"Metal\",\"image_token_limit\":1024,\"memory_gb\":16,\"metal_available\":true,\"cpu_available\":true,\"cache_root\":\"{}\",\"installed_profiles\":[],\"recommended_profile\":\"balanced\",\"recommended_device\":\"metal\",\"skill_installed\":false,\"profiles\":[\"compact\",\"balanced\",\"quality\",\"reference\"]}}",
                empty.display()
            )
        );
    }

    #[test]
    fn scan_parser_accepts_runtime_and_execution_flags() {
        let parsed = parse_scan_args(&[
            "input.pdf",
            "--max-image-tokens",
            "1024",
            "--max-output-tokens",
            "4096",
            "--context-size",
            "8192",
            "--progress",
            "jsonl",
            "--offline",
            "--non-interactive",
            "--keep-pages",
        ])
        .unwrap();
        assert_eq!(parsed.max_image_tokens, Some(1024));
        assert_eq!(parsed.max_output_tokens, Some(4096));
        assert_eq!(parsed.context_size, Some(8192));
        assert_eq!(parsed.progress, "jsonl");
        assert!(parsed.offline && parsed.non_interactive && parsed.keep_pages);
    }

    #[test]
    fn scan_parser_accepts_log_level() {
        let parsed = parse_scan_args(&["input.pdf", "--log-level", "error"]).unwrap();
        assert_eq!(parsed.log_level, "error");
        assert!(parse_scan_args(&["input.pdf", "--log-level", "verbose"]).is_err());
    }

    #[test]
    fn doctor_json_report_redacts_sensitive_cache_paths() {
        let report = doctor_json_report(
            ResourceSnapshot {
                memory_gb: 16,
                metal_available: true,
                cuda_available: false,
                vulkan_available: false,
                cpu_available: true,
            },
            ocrx_runtime::RuntimeInfo {
                backend: ocrx_runtime::Backend::Metal,
                image_token_limit: 1024,
            },
        );
        assert!(!report.to_ascii_lowercase().contains("authorization="));
        assert!(!report.to_ascii_lowercase().contains("token:"));
        assert!(report.contains("\"backend\":\"Metal\""));
    }

    #[test]
    fn command_parser_accepts_declared_command_roots() {
        assert_eq!(parse_command(&["version"]), Ok(CliCommand::Version));
        assert_eq!(
            parse_command(&["doctor"]),
            Ok(CliCommand::Doctor { json: false })
        );
        assert_eq!(
            parse_command(&["doctor", "--report", "json"]),
            Ok(CliCommand::Doctor { json: true })
        );
        assert_eq!(
            parse_command(&["scan", "input.pdf"]),
            Ok(CliCommand::Scan(ScanOptions {
                input: "input.pdf".to_owned(),
                output_dir: None,
                pages: None,
                model: "ovisocr2".to_owned(),
                profile: "auto".to_owned(),
                pdf_mode: "auto".to_owned(),
                quality_tier: "l2".to_owned(),
                resume: false,
                overwrite: false,
                device: "auto".to_owned(),
                report: "text".to_owned(),
                dpi: None,
                max_image_tokens: None,
                max_output_tokens: None,
                context_size: None,
                progress: "auto".to_owned(),
                log_level: "info".to_owned(),
                offline: false,
                non_interactive: false,
                keep_pages: false,
            }))
        );
        assert_eq!(
            parse_command(&["model", "info"]),
            Ok(CliCommand::Model {
                action: "info".to_owned(),
                profile: None,
                offline: false,
                non_interactive: false,
            })
        );
        assert_eq!(
            parse_command(&["skill", "install"]),
            Ok(CliCommand::Skill {
                action: "install".to_owned(),
            })
        );
    }

    #[test]
    fn command_parser_rejects_unknown_or_missing_commands() {
        assert!(parse_command(&["clean"]).is_err());
        assert!(parse_command(&["scan"]).is_err());
        assert!(parse_command(&["model", "unknown"]).is_err());
        assert!(parse_command(&["model", "install"]).is_err());
        assert!(parse_command(&["model", "remove"]).is_err());
        assert!(parse_command(&["model", "verify"]).is_err());
        assert!(parse_command(&["model", "remove", "compact", "--offline"]).is_err());
        assert!(parse_command(&["model", "remove", "compact", "--non-interactive"]).is_err());
        assert!(parse_command(&["model", "install", "compact", "--offline"]).is_ok());
        assert!(parse_command(&["model", "verify", "compact", "--offline"]).is_ok());
        assert!(parse_command(&["model", "verify", "experimental"]).is_err());
        assert!(parse_command(&["model", "verify", "compact"]).is_ok());
    }

    #[test]
    fn page_selection_expands_sorts_and_deduplicates() {
        assert_eq!(
            parse_page_selection("3, 1-2, 2, 5-6"),
            Ok(vec![1, 2, 3, 5, 6])
        );
    }

    #[test]
    fn page_selection_rejects_zero_reversed_and_empty_items() {
        assert!(parse_page_selection("0").is_err());
        assert!(parse_page_selection("4-2").is_err());
        assert!(parse_page_selection("1,,3").is_err());
    }
}
