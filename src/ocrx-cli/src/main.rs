use ocrx_core::{PageClassification, PageType, ProcessingMethod};
use ocrx_output::{
    CancellationToken, CompatibilityKey, ErrorReport, OcrQueuePage, PageDiagnostic, PageIncomplete,
    PageMarkdown, ProgressEvent, ProgressMode, ProgressStage, ReportFormat, Reporter, ResultReport,
    format_report, merge_ocr_queue, read_checkpoint,
};
use ocrx_scan::RenderOptions;
use ocrx_scan::{PdfErrorKind, classify_pdf_error};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use ocrx_models::recommend as model_recommend;

mod compose;

use ocrx_inference_api::{DeviceManager as _, ModelManager as _};

/// Trusted apache-2.0 community assets may install non-interactively: the
/// frozen policy accepts every confirmed download (legacy install semantics).
struct TrustedInstallInteraction;

impl ocrx_inference_api::InstallInteraction for TrustedInstallInteraction {
    fn confirm_download(
        &mut self,
        _proposals: &[ocrx_inference_api::DownloadProposal],
    ) -> Result<bool, ocrx_inference_api::InferenceError> {
        Ok(true)
    }
}
mod platform_paths;
use platform_paths::home_dir;

/// The only registered model id (frozen; the inference-api value type is
/// authoritative — this spelling backs CLI defaults/display).
/// D-48/T1 (ovisocr3-default-switch plan): the frozen default model. The
/// IMPLICIT default further resolves through the D-50 three-segment
/// memory table at scan/doctor entry — this constant is the >=8 GiB face
/// and the explicit-selector default.
const DEFAULT_MODEL: &str = "ovisocr3";

/// Registered model ids the `--model` selector accepts (D-6: the default
/// stays `ovisocr2`; unknown ids share the scan error spelling).
const SUPPORTED_MODELS: [&str; 2] = ["ovisocr2", "ovisocr3"];

#[derive(Debug, PartialEq, Eq)]
enum CliCommand {
    Version,
    Help(Option<String>),
    Doctor {
        json: bool,
        /// `--model` selector (O3-T21/D-11); `None` = the frozen default
        /// (default-model readiness) shape.
        model: Option<String>,
    },
    Scan(ScanOptions),
    Model {
        action: String,
        /// `--model` selector; `None` = the historical default shape.
        model: Option<String>,
        profile: Option<String>,
        offline: bool,
        non_interactive: bool,
        /// C2/D-4: machine-readable face, accepted on recommend/install only.
        report_json: bool,
    },
    Skill {
        action: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct ScanOptions {
    /// Positional inputs (X1.1): one PDF per document scan; a multi-input
    /// invocation is the X1 orchestrated mode. Raw spellings — directory
    /// expansion and dedup happen in `resolve_scan_inputs` at scan start.
    inputs: Vec<String>,
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
    /// PDF open password (--password). Sensitive: never printed or logged.
    password: Option<String>,
    /// Q1.1c/D-10 (O-5): enable the column-split retry rung. Default on;
    /// formula-dense document sets measured harmful - pass --column-split-retry off.
    column_split_retry: bool,
    /// D-50 scope flag: true when the user passed `--model` explicitly —
    /// the implicit-default cross-model fallback NEVER applies to an
    /// explicit selection.
    model_explicit: bool,
    /// R2.5: the table-reread pipeline switch (`--table-reread on`, off
    /// default). Requires `--layout-model pp-doclayout-v2` + `--layout-regions`.
    table_reread: bool,
    /// M4.2/D-64 + M4.3/D-71: the table-structure reread mode.
    /// `--table-structure-reread on` = strict (engine failures are hard
    /// errors, D-63 (2)); `off` = off; ABSENT = best-effort ON (P2b default
    /// flip): regions auto-generate when not supplied and any engine-side
    /// unavailability degrades the document to off with a warning.
    structure_reread: StructureRereadMode,
    /// R2.5: layout model id (only `pp-doclayout-v2` in frozen v1).
    layout_model: Option<String>,
    /// R2.5: sidecar regions JSON path for the document (R1 route A file form).
    layout_regions: Option<String>,
}

/// M4.3/D-71 three-state structure-reread switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructureRereadMode {
    /// Explicit `--table-structure-reread off`.
    Off,
    /// Flag ABSENT: best-effort ON (auto regions + degrade on failure).
    DefaultBestEffort,
    /// Explicit `--table-structure-reread on`: strict (D-63 (2) semantics).
    On,
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
        inputs: vec![(*input).to_owned()],
        output_dir: None,
        pages: None,
        model: DEFAULT_MODEL.to_owned(),
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
        password: None,
        column_split_retry: true,
        model_explicit: false,
        table_reread: false,
        structure_reread: StructureRereadMode::DefaultBestEffort,
        layout_model: None,
        layout_regions: None,
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
                // O3-T16: the scan gate opens to the registered models
                // (DEFAULT_MODEL unchanged; unknown ids share the frozen
                // error spelling).
                if !SUPPORTED_MODELS.contains(&model) {
                    return Err(format!("unsupported model: {model}"));
                }
                model.clone_into(&mut options.model);
                options.model_explicit = true;
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
            "--password" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--password requires a value".to_owned());
                };
                options.password = Some((*value).to_owned());
                index += 1;
            }
            "--resume" => options.resume = true,
            "--overwrite" => options.overwrite = true,
            // Q1.1c/O-5: the column-split rung switch (documented; the
            // parse arm was missing - passing it used to be a usage error).
            "--column-split-retry" => {
                index += 1;
                let value =
                    enum_value(args, index, "--column-split-retry", &["on", "off"])?.to_owned();
                options.column_split_retry = value == "on";
            }
            // R2.5: the table-reread pipeline switch + its frozen inputs.
            "--table-reread" => {
                index += 1;
                let value = enum_value(args, index, "--table-reread", &["on", "off"])?.to_owned();
                options.table_reread = value == "on";
            }
            // M4.2/D-64 + M4.3/D-71: the F6 SLANet arm mode (absent =
            // best-effort ON — the P2b default flip).
            "--table-structure-reread" => {
                index += 1;
                let value =
                    enum_value(args, index, "--table-structure-reread", &["on", "off"])?.to_owned();
                options.structure_reread = if value == "on" {
                    StructureRereadMode::On
                } else {
                    StructureRereadMode::Off
                };
            }
            "--layout-model" => {
                index += 1;
                let value = required_value(args, index, "--layout-model")?;
                options.layout_model = Some(value.to_owned());
            }
            "--layout-regions" => {
                index += 1;
                let value = required_value(args, index, "--layout-regions")?;
                options.layout_regions = Some(value.to_owned());
            }
            // X1.1: an additional positional is another input, not an
            // unknown flag — options and inputs may interleave.
            flag if !flag.starts_with('-') => options.inputs.push((*flag).to_owned()),
            flag => return Err(format!("unsupported scan option: {flag}")),
        }
        index += 1;
    }
    if options.resume && options.overwrite {
        return Err("--resume and --overwrite are mutually exclusive".to_owned());
    }
    validate_reread(&options)?;
    if options.inputs.len() > 1 && options.output_dir.is_none() {
        // X1.3: multiple documents fan out under an explicit output root;
        // there is no silent per-cwd default for the orchestrated mode.
        return Err("scanning multiple inputs requires --output-dir <output-root>".to_owned());
    }
    let output_dir = resolve_output_dir(&options.inputs[0], options.output_dir.as_deref())?;
    validate_output_policy(Path::new(&options.inputs[0]), &output_dir, &[])?;
    validate_numeric_options(
        options.dpi,
        Some(
            options
                .max_image_tokens
                .unwrap_or_else(|| default_max_image_tokens(&options.model)),
        ),
        options.max_output_tokens,
        options.context_size,
    )?;
    Ok(options)
}

/// O3-T18/D-5 revision: the scan-level image-token budget DEFAULT is
/// per-model. OvisOCR3's projector needs the 2048 grid on dense pages (the
/// 1024 OvisOCR2 default leaves them effectively unread — real-scan
/// evidence in the implementation log); the OvisOCR2 default stays 1024
/// byte-for-byte (iron rule 1).
#[must_use]
/// D-50/T1: the implicit-default (model, profile) resolution — the ONLY
/// place a scan may cross models. Frozen three-segment matrix: <6 GiB
/// errors (no install, no load), 6-7 GiB falls back to ovisocr2
/// balanced, >=8 GiB keeps ovisocr3 quality. An explicit `--model`
/// (or any explicit profile) is FROZEN — returned unchanged.
/// The implicit-default `(model, profile)` pair (D-50 three-segment
/// table). `None` = the selection is explicit (a `--model` or an explicit
/// profile) — the caller must not cross models.
/// The implicit-default `(model, profile)` pair (D-50 three-segment
/// table). `None` = the selection is explicit (`model_explicit` or a
/// non-auto profile) — the caller must not cross models.
fn resolve_implicit_default_pair(
    model_explicit: bool,
    requested_profile: &str,
    memory_gb: u64,
) -> Option<Result<(String, String), ocrx_inference_api::InferenceError>> {
    if model_explicit || requested_profile != "auto" {
        return None;
    }
    Some(
        model_recommend::recommend_default_selection(memory_gb)
            .map(|(model, profile)| (model.to_owned(), profile.to_owned())),
    )
}

/// The parsed pair, or the frozen floor-error exit (exit 6 — the same face
/// as the profile-recommendation failure).
fn resolve_implicit_default_selection_or_exit(options: &ScanOptions) -> (String, String) {
    let memory_gb = compose::compose(None)
        .inference
        .devices()
        .snapshot()
        .memory_gb;
    match resolve_implicit_default_pair(options.model_explicit, &options.profile, memory_gb) {
        Some(Ok(pair)) => pair,
        Some(Err(error)) => {
            eprintln!("profile recommendation failed: {error:?}");
            std::process::exit(6);
        }
        None => (options.model.clone(), options.profile.clone()),
    }
}

fn default_max_image_tokens(model: &str) -> u32 {
    if model == "ovisocr3" { 2048 } else { 1024 }
}

/// Resolves the X1.1 multi-input set: directories expand to their `*.pdf`
/// entries (sorted by path, non-recursive), files pass through, anything
/// missing is a usage error (exit 2 via the caller), and duplicates collapse
/// by canonical path. The single-input path never calls this — its
/// nonexistent-input behavior (page-count failure, exit 3) is unchanged.
fn resolve_scan_inputs(raw: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut resolved: Vec<PathBuf> = Vec::new();
    for entry in raw {
        let path = PathBuf::from(entry);
        if path.is_dir() {
            let mut pdfs: Vec<PathBuf> = std::fs::read_dir(&path)
                .map_err(|error| format!("cannot read input directory {entry}: {error}"))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|candidate| {
                    candidate.is_file()
                        && candidate
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
                })
                .collect();
            pdfs.sort();
            if pdfs.is_empty() {
                return Err(format!("input directory contains no PDFs: {entry}"));
            }
            resolved.extend(pdfs);
        } else if path.is_file() {
            resolved.push(path);
        } else {
            return Err(format!("input not found: {entry}"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::with_capacity(resolved.len());
    for path in resolved {
        let canonical = std::fs::canonicalize(&path)
            .map_err(|error| format!("cannot resolve input {}: {error}", path.display()))?;
        if seen.insert(canonical) {
            unique.push(path);
        }
    }
    Ok(unique)
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
                .then_some((*page, checkpoint))
        })
        .map(|(page, checkpoint)| {
            Ok(PageMarkdown {
                page_number: page,
                content: std::fs::read_to_string(output.join(format!("page-{page:04}.md")))?,
                incomplete: if checkpoint.degenerated {
                    Some(PageIncomplete::Degenerated)
                } else if checkpoint.truncated {
                    Some(PageIncomplete::Truncated)
                } else {
                    None
                },
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

/// R2.5 reread-switch validation: `on` needs the frozen layout model id and
/// the sidecar regions file; the model/regions flags are meaningless (and
/// misleading for the identity) without the switch.
fn validate_reread(options: &ScanOptions) -> Result<(), String> {
    // M4.2/D-64 + M4.3/D-71: the OLD flag keeps the strict layout-input
    // requirement (no auto-generation for the F1-F5 arm). The structure
    // switch auto-generates regions (P2b) and only constrains the model id.
    if options.table_reread {
        let model = options.layout_model.as_deref().unwrap_or_default();
        if model.is_empty() {
            return Err("--table-reread on requires --layout-model pp-doclayout-v2".to_owned());
        }
        if model != ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID {
            return Err(format!(
                "--layout-model must be {} in frozen v1 (got {model})",
                ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID
            ));
        }
        let regions = options.layout_regions.as_deref().unwrap_or_default();
        if regions.is_empty() {
            return Err("--table-reread on requires --layout-regions <regions.json>".to_owned());
        }
        let regions_path = Path::new(regions);
        if !regions_path.is_file() && !regions_path.is_dir() {
            return Err(format!("--layout-regions path not found: {regions}"));
        }
    }
    if options.structure_reread != StructureRereadMode::Off {
        if let Some(model) = options.layout_model.as_deref() {
            if model != ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID {
                return Err(format!(
                    "--layout-model must be {} in frozen v1 (got {model})",
                    ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID
                ));
            }
        }
    }
    if options.table_reread || options.structure_reread != StructureRereadMode::Off {
        // layout flags are consumed by an armed arm — fine. Only an all-off
        // invocation with layout flags is an error.
    } else if options.layout_model.is_some() || options.layout_regions.is_some() {
        return Err(
            "--layout-model/--layout-regions require --table-reread on or --table-structure-reread on"
                .to_owned(),
        );
    }
    Ok(())
}

/// R2.5: resolves the effective reread configuration — the layout model is
/// resolved through the R2.0 manifest chain and the identity asset hash is
/// the FROZEN rule sha256 over the manifest-order `role:sha256` lines.
fn reread_options(
    options: &ScanOptions,
) -> Result<Option<ocrx_scan::compatibility::RereadOptions>, String> {
    layout_reread_options(options, options.table_reread)
}

/// M4.2/D-64: the shared layout resolution, parameterized by which switch
/// is armed (the OLD flag and the structure flag carry identical layout
/// inputs; each gets its own resolved RereadOptions).
fn layout_reread_options(
    options: &ScanOptions,
    enabled: bool,
) -> Result<Option<ocrx_scan::compatibility::RereadOptions>, String> {
    // D-71: an EXPLICIT --layout-model is honored; when the structure arm
    // is enabled without one, the frozen v1 model id is implied (the
    // auto-generation path resolves the same installed assets).
    let model = options
        .layout_model
        .clone()
        .unwrap_or_else(|| ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID.to_owned());
    if !enabled {
        return Ok(None);
    }
    let manifest = ocrx_models::layout::paddlex_layout_manifest(&model, None)
        .map_err(|error| format!("layout model {model} is not resolvable: {error}"))?;
    let mut canonical = String::new();
    for asset in &manifest.assets {
        canonical.push_str(&format!(
            "{}:{}
",
            asset.role, asset.sha256
        ));
    }
    Ok(Some(ocrx_scan::compatibility::RereadOptions {
        layout_model_id: manifest.model.as_str().to_owned(),
        layout_assets_sha256: ocrx_output::sha256(canonical.as_bytes()),
        regions_path: PathBuf::from(options.layout_regions.as_deref().unwrap_or_default()),
    }))
}

/// R2.5/BLK-5 result identity sync: result.json reflects the effective
/// reread execution identity (frozen prompt version + rules generation).
fn reread_result_identity(
    reread: Option<&ocrx_scan::compatibility::RereadOptions>,
    structure_json: &str,
    tables_reread_total: u64,
    replaced_total: u64,
) -> String {
    // M4.2 BLOCK-3: the structure arm reports ITS OWN section — a
    // structure-only run must never render as enabled:false. Empty json =
    // the arm did not run (the blob keeps its exact pre-M4.2 shape).
    let structure_part = if structure_json.is_empty() {
        String::new()
    } else {
        let rules = ocrx_scan::reread::REREAD_RULES_GENERATION;
        format!(
            ",\"table_structure_reread\":{{\"enabled\":true,\"engine_identity\":{structure_json},\"rules_generation\":{rules}}}"
        )
    };
    let (_enabled, model_part) = match reread {
        Some(reread) => (
            true,
            format!(
                "\"enabled\":true,\"layout_model\":\"{}\",\"layout_assets_sha256\":\"{}\",\"prompt_version\":\"{}\",\"rules_generation\":{}",
                reread.layout_model_id,
                reread.layout_assets_sha256,
                ocrx_scan::ocr::prompt::reread::TABLE_REREAD_PROMPT_VERSION,
                // M4.2/D-63: old-flag-only ON records the VLM-era generation
                // (2). The SLANet arm's generation lives in the checkpoint
                // compatibility identity, not this display blob.
                ocrx_scan::reread::VLM_REREAD_RULES_GENERATION,
            ),
        ),
        None => (
            false,
            "\"enabled\":false,\"prompt_version\":\"\",\"rules_generation\":0".to_owned(),
        ),
    };
    format!(
        "{{{model_part}{structure_part},\"tables_reread_total\":{tables_reread_total},\"replaced_total\":{replaced_total}}}"
    )
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
    // Mirror the adapter cap (plan §3.7/D5.1): an over-cap budget must fail
    // once before the scan starts, never as a per-page adapter error.
    if max_output_tokens
        .is_some_and(|value| value > ocrx_scan::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS)
    {
        return Err(format!(
            "max output tokens must not exceed {}",
            ocrx_scan::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS
        ));
    }
    if context_size.is_some_and(|value| value < 1024) {
        return Err("context size must be at least 1024".to_owned());
    }
    Ok(())
}

/// BLOCK-2: the model-dependent half of the numeric gate — the context
/// must fit the image/output/prompt budget of the RESOLVED model. Runs
/// after the D-50 default resolution (the pre-parse gate used the
/// unresolved model's budget and rejected legal fallback configurations).
fn validate_context_gate(options: &ScanOptions) -> Result<(), String> {
    let image_budget = options
        .max_image_tokens
        .unwrap_or_else(|| default_max_image_tokens(&options.model));
    let output_budget = options.max_output_tokens.unwrap_or(16_384);
    let context_budget = options.context_size.unwrap_or(20_480);
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
    // Delegates to the shared output-root validator so every scan gets the
    // full protection set in one place (review BLK-2): absolute-path check,
    // refusal of `..` and Windows trailing dot/space spellings, and
    // case-folded comparison through canonicalized forms (which also unfolds
    // 8.3 short names and reparse points on existing prefixes).
    ocrx_output::validate_output_root(input, output, protected).map_err(|error| error.to_string())
}

/// Full output policy for a scan: the shared root validator plus the
/// working-directory rule. The working directory is protected
/// one-directionally — an output at or above it is refused (`--overwrite`
/// deletes the whole output tree), while a subdirectory of it is the normal
/// layout and must pass.
fn validate_scan_output_policy(
    input: &Path,
    output: &Path,
    protected: &[&Path],
    working_directory: &Path,
) -> Result<(), String> {
    validate_output_policy(input, output, protected)?;
    ocrx_output::validate_output_not_containing(output, working_directory)
        .map_err(|error| error.to_string())
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
        ["doctor", rest @ ..] => {
            let mut json = false;
            let mut model = None;
            let mut index = 0;
            while index < rest.len() {
                match rest[index] {
                    "--report" => {
                        index += 1;
                        if rest.get(index) != Some(&"json") {
                            return Err("unsupported or incomplete command: doctor".to_owned());
                        }
                        json = true;
                    }
                    "--model" => {
                        index += 1;
                        let Some(&value) = rest.get(index) else {
                            return Err("unsupported or incomplete command: doctor".to_owned());
                        };
                        if !SUPPORTED_MODELS.contains(&value) {
                            return Err(format!("unsupported model: {value}"));
                        }
                        model = Some(value.to_owned());
                    }
                    _ => return Err("unsupported or incomplete command: doctor".to_owned()),
                }
                index += 1;
            }
            Ok(CliCommand::Doctor { json, model })
        }
        ["scan", input, rest @ ..] if !input.is_empty() && !input.starts_with('-') => {
            let mut scan_args = vec![*input];
            scan_args.extend(rest.iter().copied());
            Ok(CliCommand::Scan(parse_scan_args(&scan_args)?))
        }
        ["model", action] if matches!(*action, "list" | "info" | "recommend") => {
            Ok(CliCommand::Model {
                action: (*action).to_owned(),
                model: None,
                profile: None,
                offline: false,
                non_interactive: false,
                report_json: false,
            })
        }
        ["model", action, rest @ ..]
            if matches!(
                *action,
                "list" | "info" | "recommend" | "install" | "remove" | "verify"
            ) =>
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
    let mut model = None;
    let mut profile = None;
    let mut offline = false;
    let mut non_interactive = false;
    // C2/D-4: `--report json` is the machine-readable face; exactly the
    // recommend and install subcommands accept it (plan §6 阶段 2 — the
    // list/info/verify and --progress/--model-profile bans stay frozen).
    let report_allowed = matches!(action, "recommend" | "install");
    let mut report_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            flag @ ("--offline" | "--non-interactive") if action == "remove" => {
                return Err(format!("model {action} does not support {flag}"));
            }
            "--model" => {
                index += 1;
                let value = required_value(args, index, "--model")?;
                if !SUPPORTED_MODELS.contains(&value) {
                    return Err(format!("unsupported model: {value}"));
                }
                model = Some(value.to_owned());
            }
            "--report" => {
                index += 1;
                let value = required_value(args, index, "--report")?;
                if !report_allowed {
                    return Err("unsupported model flag: --report".to_owned());
                }
                if value != "json" {
                    return Err(format!("unsupported model report format: {value}"));
                }
                report_json = true;
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
        model,
        profile,
        offline,
        non_interactive,
        report_json,
    })
}

/// C2/D-5: maps install errors onto (exit code, failure-object kind, human
/// message). Exit codes are frozen (offline/disk = 5, everything else = 4);
/// the kind vocabulary is transport/hash/lock/offline/disk (+ license for
/// the policy gate the trusted-install path cannot normally reach).
fn classify_install_error(
    error: &ocrx_inference_api::InferenceError,
) -> (u8, &'static str, String) {
    use ocrx_inference_api::InferenceError as E;
    match error {
        E::Offline => (
            5,
            "offline",
            "model install blocked: offline mode".to_owned(),
        ),
        E::InsufficientDisk(_) => (
            5,
            "disk",
            "model install blocked: insufficient disk".to_owned(),
        ),
        E::LicenseRequired => (
            4,
            "license",
            "model install blocked: license acceptance required".to_owned(),
        ),
        E::InstallLocked(_) => (4, "lock", format!("model install failed: {error}")),
        E::HashMismatch(_) => (4, "hash", format!("model install failed: {error}")),
        E::Transport(_) => (4, "transport", format!("model install failed: {error}")),
        other => (4, "install", format!("model install failed: {other:?}")),
    }
}

/// C2/D-5: renders install events as human-readable stderr progress lines
/// (process visibility only — the machine-readable statistics ride the
/// final run report, never these lines).
fn eprint_install_progress(event: &ocrx_models::install::InstallEvent<'_>) {
    use ocrx_models::install::InstallEvent as E;
    match event {
        E::LockAcquired { pid } => eprintln!("install lock-acquired pid={pid}"),
        E::DownloadStart {
            role,
            url,
            byte_len,
            attempt,
        } => eprintln!(
            "install download-start role={role} url={url} bytes={byte_len} attempt={attempt}"
        ),
        E::DownloadProgress {
            role,
            bytes,
            total_bytes,
            rate_bps,
        } => eprintln!(
            "install download-progress role={role} bytes={bytes}/{total_bytes} rate_bps={rate_bps:.0}"
        ),
        E::Verify {
            role,
            sha256_ok,
            size_ok,
        } => eprintln!("install verify role={role} sha256_ok={sha256_ok} size_ok={size_ok}"),
        E::Commit => eprintln!("install commit"),
        E::Done => eprintln!("install done"),
    }
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
        CliCommand::Version => println!("ocrx {}", env!("CARGO_PKG_VERSION")),
        CliCommand::Help(topic) => print_help(topic.as_deref()),
        CliCommand::Doctor { json, model } => {
            // T073/T077: doctor flows through the composed DeviceManager;
            // the snapshot is the manager's probe (hardware ∩ compiled).
            // D-11/O3-T21: no-flag = the DEFAULT-MODEL readiness face; the
            // implicit default resolves through the D-50 three-segment
            // table (same as scan — doctor and scan must agree on the
            // model a user will actually get); `--model` selects the
            // explicit readiness face and never crosses models. The
            // implicit default resolves the FULL (model, profile) pair and
            // BOTH ride the readiness face — recomputing the profile with
            // the single-model ladder contradicted the scan's resolved
            // pair at 6-7 GiB (review BLOCK-1).
            let composed = compose::compose(None);
            let snapshot = composed.inference.devices().snapshot();
            // The resolver only reads model_explicit/profile; build the
            // minimal probe face from the parse results.
            let (selected, resolved_profile) = match model {
                Some(explicit) => (explicit, None),
                None => match resolve_implicit_default_pair(false, "auto", snapshot.memory_gb) {
                    Some(Ok(pair)) => (pair.0, Some(pair.1)),
                    Some(Err(_)) | None => (DEFAULT_MODEL.to_owned(), None),
                },
            };
            let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(ocrx_inference::default_cache_root);
            let diagnostics = doctor_diagnostics_from_snapshot(
                &snapshot,
                &cache_root,
                &selected,
                resolved_profile.as_deref(),
            );
            let backend_display = doctor_backend_display(&diagnostics);
            if json {
                println!(
                    "{}",
                    doctor_json_report(snapshot, &selected, resolved_profile.as_deref())
                );
            } else {
                println!(
                    "model={} backend={} image_token_limit={} memory_gb={} metal_available={}",
                    selected,
                    backend_display,
                    diagnostics.image_token_limit,
                    snapshot.memory_gb,
                    snapshot.metal_available
                );
            }
        }
        CliCommand::Model {
            action,
            model,
            profile,
            offline: _,
            non_interactive: _,
            report_json,
        } if action == "list" || action == "info" || action == "recommend" => {
            // T071: everything flows through the production ModelManager
            // (compose root); the default output strings stay byte-identical
            // (O3-T09: an explicit `--model` narrows the face; the no-flag
            // `list` walks every registered model in declaration order, SS83 item 1).
            let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(ocrx_inference::default_cache_root);
            let composed = compose::compose(Some(cache_root));
            let narrowed = model.is_some();
            let snapshot = composed.inference.devices().snapshot();
            // D-50/BLOCK-1: the no-flag recommend face reports the SAME
            // (model, profile) the scan would resolve; an explicit --model
            // keeps the per-model recommendation (never crosses models).
            // D-50 scope (v1.2/re-review): the implicit pair rides ONLY the
            // recommend face (scan-preparation advice). list/info are
            // CATALOG faces — the frozen catalog default (ovisocr3) plus
            // any explicit selector, machine-independent.
            let (selected, implicit_profile) = if action == "recommend" && !narrowed {
                match resolve_implicit_default_pair(false, "auto", snapshot.memory_gb) {
                    Some(Ok(pair)) => (pair.0, Some(pair.1)),
                    _ => (DEFAULT_MODEL.to_owned(), None),
                }
            } else {
                (
                    model.clone().unwrap_or_else(|| DEFAULT_MODEL.to_owned()),
                    None,
                )
            };
            let model = ocrx_inference_api::ModelId::new(&selected)
                .unwrap_or_else(|_| unreachable!("model id validated by the CLI parser"));
            // C2/D-4 machine-readable plan face: the plan must match the
            // text recommend face exactly — the implicit-default pair rides
            // in as an explicit profile override; an explicit --model keeps
            // the per-model memory ladder (resolve inside the plan).
            if action == "recommend" && report_json {
                let cache_root_for_plan = std::env::var_os("OCRX_MODEL_CACHE")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(ocrx_inference::default_cache_root);
                let plan =
                    ocrx_models::plan::InstallPlan::resolve(ocrx_models::plan::PlanRequest {
                        model: &model,
                        memory_gb: snapshot.memory_gb,
                        default_model: DEFAULT_MODEL,
                        cache_root: &cache_root_for_plan,
                        profile: implicit_profile
                            .as_deref()
                            .map(ocrx_inference_api::ProfileId::new)
                            .transpose()
                            .unwrap_or_else(|_| unreachable!("implicit profile is a ladder name"))
                            .as_ref(),
                    });
                match plan {
                    Ok(plan) => {
                        println!(
                            "{}",
                            serde_json::to_string(&plan)
                                .unwrap_or_else(|_| unreachable!("plan serializes"))
                        );
                    }
                    Err(error) => {
                        eprintln!("model recommend failed: {error:?}");
                        std::process::exit(6);
                    }
                }
                return;
            }
            if action == "list" {
                if narrowed {
                    // Explicit selector: list exactly that model's line
                    // (frozen shape on creation, SS83 item 8).
                    println!("model={}", model.as_str());
                } else {
                    match composed.inference.manager().list() {
                        Ok(summaries) => {
                            for summary in summaries {
                                println!("model={}", summary.model);
                            }
                        }
                        Err(error) => {
                            eprintln!("model list failed: {error:?}");
                            std::process::exit(4);
                        }
                    }
                }
            } else if action == "info" {
                let detail = match composed.inference.manager().info(&model) {
                    Ok(detail) => detail,
                    Err(error) => {
                        eprintln!("model info failed: {error:?}");
                        std::process::exit(4);
                    }
                };
                // Frozen display: profiles sorted; the optional profile
                // argument rides the installed slot (historical shape).
                let mut profiles: Vec<String> = detail
                    .summary
                    .profiles
                    .iter()
                    .map(|summary| summary.profile.to_string())
                    .collect();
                profiles.sort();
                let installed: Vec<String> = profile.into_iter().collect();
                println!(
                    "model={} profiles={} installed={}",
                    selected,
                    profiles.join(","),
                    installed.join(",")
                );
            } else {
                let snapshot = composed.inference.devices().snapshot();
                match composed.inference.manager().recommend(&model, &snapshot) {
                    Ok(recommendation) => println!(
                        "model={} profile={} memory_gb={} device={}",
                        selected,
                        implicit_profile.unwrap_or_else(|| recommendation.profile.to_string()),
                        snapshot.memory_gb,
                        composed
                            .inference
                            .devices()
                            .select("auto")
                            .map(|backend| backend.to_string())
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
            model,
            profile,
            offline,
            non_interactive,
            report_json,
        } => {
            let Some(profile) = profile else {
                eprintln!("model {action} requires a profile");
                std::process::exit(2);
            };
            // O3-T09: the selector defaults to the historical DEFAULT_MODEL
            // so the default success strings stay byte-identical.
            let selected = model.unwrap_or_else(|| DEFAULT_MODEL.to_owned());
            let revision = match selected.as_str() {
                "ovisocr2" => ocrx_inference::builtin::OVISOCR2_REVISION,
                "ovisocr3" => ocrx_inference::builtin::ovisocr3::OVISOCR3_REVISION,
                other => unreachable!("model selector validated by the CLI parser: {other}"),
            };
            let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(ocrx_inference::default_cache_root);
            let mut composed = compose::compose(Some(cache_root));
            match action.as_str() {
                "verify" => {
                    // T072: verify = content + registry binding via the
                    // injected port (never downloads, never loads).
                    let model = ocrx_inference_api::ModelId::new(&selected)
                        .unwrap_or_else(|_| unreachable!("pinned model id"));
                    let profile_id = ocrx_inference_api::ProfileId::new(&profile)
                        .unwrap_or_else(|_| unreachable!("profile parsed by CLI"));
                    match composed.inference.manager().verify(&model, &profile_id) {
                        Ok(()) => println!(
                            "model={selected} profile={profile} revision={revision} status=installed"
                        ),
                        Err(error) => {
                            eprintln!("model verification failed: {error:?}");
                            std::process::exit(4);
                        }
                    }
                }
                "install" => {
                    // T072: install via the manager (revision-scoped group
                    // transaction; trusted apache-2.0 community assets may
                    // install non-interactively, so the interaction accepts).
                    // C2/D-5: `--report json` switches the output face — the
                    // stderr stage/progress lines stay human-readable process
                    // visibility; the final summary (success) or failure
                    // object (schema v1, stdout) is the machine-readable
                    // contract. Exit codes are unchanged in both faces.
                    let model = ocrx_inference_api::ModelId::new(&selected)
                        .unwrap_or_else(|_| unreachable!("pinned model id"));
                    let profile_id = ocrx_inference_api::ProfileId::new(&profile)
                        .unwrap_or_else(|_| unreachable!("profile parsed by CLI"));
                    let mut interaction = TrustedInstallInteraction;
                    let progress_closure = |event: &ocrx_models::install::InstallEvent<'_>| {
                        eprint_install_progress(event);
                    };
                    let observer: Option<ocrx_models::install::InstallObserver<'_>> = if report_json
                    {
                        let observer: &(
                             dyn Fn(&ocrx_models::install::InstallEvent<'_>) + Send + Sync
                         ) = &progress_closure;
                        Some(observer)
                    } else {
                        None
                    };
                    match composed.inference.manager_mut().install_reported(
                        &model,
                        &profile_id,
                        offline,
                        non_interactive,
                        true,
                        &mut interaction,
                        observer,
                    ) {
                        Ok(report) => {
                            println!(
                                "model={selected} profile={profile} revision={revision} status=installed"
                            );
                            if report_json {
                                println!(
                                    "{}",
                                    serde_json::to_string(&report)
                                        .unwrap_or_else(|_| unreachable!("run report serializes"))
                                );
                            }
                        }
                        Err(error) => {
                            let (code, kind, message) = classify_install_error(&error);
                            if report_json {
                                println!(
                                    "{}",
                                    ErrorReport {
                                        code,
                                        kind: kind.to_owned(),
                                        message,
                                        cause: Some(error.to_string()),
                                        suggestion: None,
                                    }
                                    .to_json()
                                );
                            } else {
                                eprintln!("{message}");
                            }
                            std::process::exit(i32::from(code));
                        }
                    }
                }
                "remove" => {
                    // T072: removal via the manager (per-asset deletion with
                    // the shared-projector reference check, review m9).
                    let model = ocrx_inference_api::ModelId::new(&selected)
                        .unwrap_or_else(|_| unreachable!("pinned model id"));
                    let profile_id = ocrx_inference_api::ProfileId::new(&profile)
                        .unwrap_or_else(|_| unreachable!("profile parsed by CLI"));
                    match composed.inference.manager_mut().remove(&model, &profile_id) {
                        Ok(_report) => {
                            println!("model={selected} profile={profile} status=removed")
                        }
                        Err(error) => {
                            eprintln!("model removal failed: {error:?}");
                            std::process::exit(9);
                        }
                    }
                }
                _ => unreachable!("model action parsed by CLI"),
            }
        }
        CliCommand::Scan(options) => {
            // T074: single- and multi-input invocations both route through
            // ScanApplication::run (the app owns the FIFO orchestration and
            // the historical page-count failure / exit 3 gate).
            run_scan_via_application(options)
        }
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
                ocrx_skill::SkillScope::User => (home_dir().ok(), None),
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

/// Parses the page number from a W3.3 test-only fault-injection env value.
///
/// The value is `None` when the env var is unset. Zero and non-numeric values are
/// rejected so a misconfigured test never injects a fault into page 0.
#[must_use]
fn parse_injected_failure_page(value: Option<&str>) -> Option<u32> {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|page| *page > 0)
}

/// Returns an error when the injected page matches, otherwise `Ok(())`.
///
/// This is the deterministic test seam for W3.3 real-model fault injection.
/// Production scans leave the env vars unset, so the seam is a no-op in normal use.
fn injected_failure(value: Option<&str>, page: u32, description: &str) -> std::io::Result<()> {
    if parse_injected_failure_page(value) == Some(page) {
        return Err(std::io::Error::other(description.to_owned()));
    }
    Ok(())
}

/// Injects a real render failure for `page` when `OCRX_E2E_INJECT_RENDER_FAIL_PAGE` matches.
fn inject_render_failure(page: u32) -> std::io::Result<()> {
    injected_failure(
        std::env::var("OCRX_E2E_INJECT_RENDER_FAIL_PAGE")
            .ok()
            .as_deref(),
        page,
        "injected render failure",
    )
}

/// Injects a real inference failure for `page` when `OCRX_E2E_INJECT_INFERENCE_FAIL_PAGE` matches.
fn inject_inference_failure(page: u32) -> std::io::Result<()> {
    injected_failure(
        std::env::var("OCRX_E2E_INJECT_INFERENCE_FAIL_PAGE")
            .ok()
            .as_deref(),
        page,
        "injected inference failure",
    )
}

fn probe_vram_bytes(device: &str) -> Option<u64> {
    match device {
        "vulkan" => probe_vulkan_vram_bytes(),
        "cuda" => probe_cuda_vram_bytes(),
        _ => None,
    }
}

#[cfg(feature = "vulkan")]
fn probe_vulkan_vram_bytes() -> Option<u64> {
    use vulkano::VulkanLibrary;
    use vulkano::instance::{Instance, InstanceCreateInfo};
    use vulkano::memory::MemoryHeapFlags;

    let library = VulkanLibrary::new().ok()?;
    let instance = Instance::new(library, InstanceCreateInfo::default()).ok()?;
    instance
        .enumerate_physical_devices()
        .ok()?
        .filter_map(|physical| {
            let properties = physical.memory_properties();
            properties
                .memory_heaps
                .iter()
                .filter(|heap| heap.flags.contains(MemoryHeapFlags::DEVICE_LOCAL))
                .map(|heap| heap.size)
                .max()
        })
        .max()
}

#[cfg(not(feature = "vulkan"))]
fn probe_vulkan_vram_bytes() -> Option<u64> {
    None
}

fn probe_cuda_vram_bytes() -> Option<u64> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?;
    let mb = first.trim().parse::<u64>().ok()?;
    Some(mb.saturating_mul(1024 * 1024))
}

/// Phase-4 BLK-2: the batch depth that actually takes effect. The 16 tier
/// is a GPU-VRAM decision; a CPU runtime (requested or GPU->CPU fallback)
/// always runs the compiled floor.
/// Resolves the PDFium library location (upstream hardening, main 2a415a4):
/// the explicit `OCRX_PDFIUM_LIBRARY` override wins; otherwise the library
/// shipped next to the installed executable (Windows `pdfium.dll`, macOS
/// `../lib/libpdfium.dylib`, Linux `../lib/libpdfium.so`) is used so installs
/// keep working after the vendored copy is laid out by the install scripts;
/// the historical `"."` fallback comes last.
fn pdfium_library_from_environment() -> PathBuf {
    std::env::var_os("OCRX_PDFIUM_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(default_pdfium_library)
}

fn default_pdfium_library() -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        let bundled = bundled_pdfium_path(&executable);
        if bundled.is_file() {
            return bundled;
        }
    }
    PathBuf::from(".")
}

fn bundled_pdfium_path(executable: &Path) -> PathBuf {
    let directory = executable.parent().unwrap_or_else(|| Path::new("."));
    #[cfg(target_os = "macos")]
    let bundled = directory.join("../lib/libpdfium.dylib");
    #[cfg(target_os = "windows")]
    let bundled = directory.join("pdfium.dll");
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let bundled = directory.join("../lib/libpdfium.so");
    bundled
}

fn resolve_window_depth(requested_device: &str) -> usize {
    // The composed device manager owns select + VRAM probe + tiering.
    let composed = compose::compose(None);
    let device = composed
        .inference
        .devices()
        .select(requested_device)
        .unwrap_or_else(|_| ocrx_inference_api::BackendId::new("cpu").expect("cpu is valid"));
    let vram = composed.inference.devices().vram_bytes(&device);
    composed.inference.devices().window_depth(vram)
}

/// Per-invocation shared state for one document scan (X1.2). `owner` is
/// `None` on the single-input path: the scan then spawns the
/// invocation's owner at the historical spawn point, byte-identically. The
/// orchestrated path passes the invocation-wide owner and switches it
/// between documents. `model_slot` decides whether the runtime is loaded per
/// scan (PerCall, historical) or once per invocation and reused across
/// documents (Shared).
fn emit_reporter(reporter: &mut Reporter) {
    let drained = std::mem::take(reporter);
    // `mem::take` resets to `Reporter::default()`, which would silently drop
    // the orchestrated stdout policy mid-invocation - carry it across drains.
    reporter.json_errors_on_stderr = drained.json_errors_on_stderr;
    print_reporter(drained);
}

/// Single-input scan (historical contract): identical behavior to the
/// pre-orchestrator `run_scan` - the output directory resolves up front, the
/// owner spawns at the historical point, the runtime loads per invocation,
/// and the document's exit code becomes the process exit code.
/// Maps CLI scan options onto the scan application (T074/T075): builds the
/// PDF/render ports over PdfiumOwner, drives the compose-root service, and
/// serializes each outcome into the frozen result.json shape. Exit codes:
/// cancel 130 > document failure 3 > partial 10 > 0; multi-input adds
/// `scan-summary.json` and (JSON mode) exactly one stdout aggregate.
#[allow(clippy::too_many_lines)]
fn run_scan_via_application(options: ScanOptions) -> ! {
    let started = std::time::Instant::now();
    // D-50/T1: the IMPLICIT default (no --model, auto profile) resolves
    // through the frozen three-segment memory table BEFORE anything
    // consumes options.model — vision-token defaults, service identity,
    // result/checkpoint identity all follow the RESOLVED (model, profile).
    // An explicit --model is frozen (never crosses models); <6 GiB keeps
    // the frozen floor error.
    let (resolved_model, resolved_profile) = resolve_implicit_default_selection_or_exit(&options);
    let mut options = options;
    options.model = resolved_model;
    options.profile = resolved_profile;
    // BLOCK-2 (review 2026-11-19): the model-dependent context gate runs
    // AFTER the resolution — the pre-parse gate used the UNRESOLVED
    // model's token budget and rejected legal fallback configurations
    // (6-7 GiB: ovisocr2's 1024 budget makes --context-size 18432 legal,
    // but the ovisocr3 pre-gate computed 19456 and errored). Model-
    // independent checks stay in the parser.
    if let Err(error) = validate_context_gate(&options) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
    let progress_mode = resolve_progress_mode(&options.progress, &options.log_level);
    let report_format = match options.report.as_str() {
        "json" => ReportFormat::Json,
        _ => ReportFormat::Text,
    };
    let multi = options.inputs.len() > 1
        || Path::new(options.inputs.first().unwrap_or(&String::new())).is_dir();
    // BLK-4: orchestrated invocations keep stdout to exactly one aggregate
    // object - per-document errors are logs on stderr. Single input keeps
    // the historical stdout error contract.
    let mut reporter = Reporter {
        json_errors_on_stderr: multi,
        ..Reporter::default()
    };
    // Plain single-file scans skip input resolution entirely: their
    // historical missing-file behavior is the page-count failure (exit 3),
    // produced by the app's page-count gate (kind=input).
    // R2.5: the reread configuration resolves once up front (manifest
    // chain + frozen asset-hash rule); identity and result.json share it.
    let reread = match reread_options(&options) {
        Ok(reread) => reread,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    };
    let resolved_inputs = if multi {
        match resolve_scan_inputs(&options.inputs) {
            Ok(inputs) => inputs,
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(2);
            }
        }
    } else {
        options.inputs.iter().map(PathBuf::from).collect()
    };
    if multi && options.output_dir.is_none() {
        eprintln!("error: scanning multiple inputs requires --output-dir <output-root>");
        std::process::exit(2);
    }
    // Multi-input: the relative output root resolves against the current
    // directory up front (the X1 golden uses a relative spelling).
    let output_root = options.output_dir.clone().map(|spelling| {
        let root = PathBuf::from(spelling);
        if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&root))
                .unwrap_or(root)
        }
    });
    let output_for = |input: &Path| -> PathBuf {
        if multi {
            output_dir_for_doc(output_root.as_deref().unwrap_or(Path::new(".")), input)
        } else {
            let input_str = input.to_string_lossy().into_owned();
            resolve_output_dir(&input_str, options.output_dir.as_deref()).unwrap_or_else(|error| {
                eprintln!("error: {error}");
                std::process::exit(2);
            })
        }
    };
    // M4.3 P2b (D-71): the structure arm resolution + auto regions
    // generation. Best-effort (default) degrades to off on ANY engine-side
    // failure with a warning; explicit `on` hard-errors (D-63 (2)).
    let structure_mode_active = options.structure_reread != StructureRereadMode::Off;
    let mut structure_degraded: Option<String> = None;
    let mut structure_reread = if structure_mode_active {
        match layout_reread_options(&options, true) {
            Ok(resolved) => resolved,
            Err(error) => {
                if options.structure_reread == StructureRereadMode::On {
                    eprintln!("error: {error}");
                    std::process::exit(2);
                }
                structure_degraded = Some(format!("layout model unresolvable: {error}"));
                None
            }
        }
    } else {
        None
    };
    if structure_mode_active && structure_reread.is_some() && options.layout_regions.is_none() {
        // D-71 (2): generate the regions files for every input into
        // `<output>/regions/` (route A per-stem files) when absent.
        let root: PathBuf = if multi {
            output_root
                .clone()
                .expect("multi requires output dir")
                .join("regions")
        } else {
            output_for(resolved_inputs.first().expect("single doc input")).join("regions")
        };
        let generation = (|| -> Result<(), String> {
            let mut generator =
                ocrx_scan::structure_engine::StructureEngineClient::spawn_from_env()?;
            let model_dir = ocrx_inference::default_cache_root()
                .join("ocrx")
                .join("models")
                .join(ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID)
                .join(ocrx_models::layout::PADDLE_LAYOUT_REVISION);
            let result = (|| {
                for input in &resolved_inputs {
                    let stem = input
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let out_path = root.join(format!("{stem}.regions.json"));
                    if out_path.exists() {
                        continue;
                    }
                    generator.generate_regions(input, &model_dir, &out_path)?;
                }
                Ok(())
            })();
            if result.is_err() {
                // Drop kills the child (Drop impl on StructureEngineClient).
                drop(generator);
            }
            result
        })();
        match generation {
            Ok(()) => {
                if let Some(opts) = structure_reread.as_mut() {
                    opts.regions_path = root;
                }
            }
            Err(error) => {
                if options.structure_reread == StructureRereadMode::On {
                    eprintln!("error: regions generation failed: {error}");
                    std::process::exit(2);
                }
                structure_degraded = Some(format!("regions generation failed: {error}"));
                structure_reread = None;
            }
        }
    }
    if let Some(warning) = &structure_degraded {
        eprintln!("warn: table-structure reread degraded to off (default best-effort): {warning}");
    }
    // Ctrl+C: first request graceful, second forced 130 (frozen contract).
    let cancellation = CancellationToken::new();
    install_sigint_handler(cancellation.clone());
    // Frozen preflight (§6.3 step 1): an explicit unavailable device fails
    // diagnosably (exit 6 kind runtime) even for native-only documents.
    if options.device != "auto" {
        let composed_gate = compose::compose(None);
        if let Err(device_error) = composed_gate.inference.devices().select(&options.device) {
            reporter.error(
                &ErrorReport {
                    code: 6,
                    kind: "runtime".to_owned(),
                    message: "requested device is unavailable".to_owned(),
                    cause: Some(format!("{device_error:?}")),
                    suggestion: Some(
                        "run `ocrx doctor --report json` to see available backends".to_owned(),
                    ),
                },
                report_format,
            );
            emit_reporter(&mut reporter);
            std::process::exit(6);
        }
    }
    // Output policy: the output tree may not overlap the input, the model
    // cache, or the working directory (exit 9 kind output).
    let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(ocrx_inference::default_cache_root);
    let working_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for input in &resolved_inputs {
        let output = output_for(input);
        if let Ok(canonical_input) = input.canonicalize() {
            // W2.3/m2: the protected cache paths are the MODEL subtrees
            // (`<root>/ocrx/models`, `<root>/ovisocr2`), not the whole cache
            // root — on Windows the default root is %LOCALAPPDATA%, which
            // also contains %TEMP%; protecting the root would outlaw every
            // temp-dir output. An output AT the cache root is still refused
            // (ancestor of the protected subtree).
            let protected_cache_roots = [
                cache_root.join("ocrx").join("models"),
                cache_root.join("ovisocr2"),
            ];
            let protected = [
                canonical_input.as_path(),
                protected_cache_roots[0].as_path(),
                protected_cache_roots[1].as_path(),
            ];
            if let Err(error) = validate_scan_output_policy(
                &canonical_input,
                &output,
                &protected,
                &working_directory,
            ) {
                reporter.error(
                    &ErrorReport {
                        code: 9,
                        kind: "output".to_owned(),
                        message: "output directory refused by the output policy".to_owned(),
                        cause: Some(error),
                        suggestion: Some("choose a dedicated output directory".to_owned()),
                    },
                    report_format,
                );
                emit_reporter(&mut reporter);
                std::process::exit(9);
            }
        }
    }
    // Multi-input aggregate policy (upstream hardening, main 4376 hunk): the
    // aggregate ROOT is validated against every input BEFORE the owner or any
    // document starts — child-directory checks cannot protect
    // scan-summary.json on failure paths — and an existing summary requires
    // an explicit --resume or --overwrite.
    if multi && let Some(root) = &output_root {
        let policy_error = (|| -> Option<String> {
            for input in &resolved_inputs {
                if let Err(error) = validate_scan_output_policy(
                    input,
                    root,
                    &[
                        cache_root.join("ocrx").join("models").as_path(),
                        cache_root.join("ovisocr2").as_path(),
                    ],
                    &working_directory,
                ) {
                    return Some(error);
                }
            }
            if root.join("scan-summary.json").exists() && !options.overwrite && !options.resume {
                return Some(
                    "scan-summary.json already exists; use --resume or --overwrite".to_owned(),
                );
            }
            None
        })();
        if let Some(error) = policy_error {
            reporter.error(
                &ErrorReport {
                    code: 9,
                    kind: "output".to_owned(),
                    message: "output directory refused by the output policy".to_owned(),
                    cause: Some(error),
                    suggestion: Some(
                        "choose a dedicated output directory; use --resume or --overwrite for existing results"
                            .to_owned(),
                    ),
                },
                report_format,
            );
            emit_reporter(&mut reporter);
            std::process::exit(9);
        }
    }
    let dpi = u16::try_from(options.dpi.unwrap_or(300)).unwrap_or(300);
    let window_depth = resolve_window_depth(&options.device);
    let pdfium_dir = pdfium_library_from_environment();
    let mut composed = compose::compose(None);
    let app_options = ocrx_scan::DocumentOptions {
        pdf_mode: options.pdf_mode.clone(),
        quality_tier: options.quality_tier.clone(),
        dpi: options.dpi.unwrap_or(300),
        max_image_tokens: options
            .max_image_tokens
            .unwrap_or_else(|| default_max_image_tokens(&options.model)),
        max_output_tokens: options.max_output_tokens.unwrap_or(16_384),
        context_size: options.context_size.unwrap_or(20_480),
        resume: options.resume,
        keep_pages: options.keep_pages,
        column_split_retry: options.column_split_retry,
        pages: options.pages.clone(),
        requested_profile: options.profile.clone(),
        requested_device: options.device.clone(),
        model: options.model.clone(),
        install: ocrx_inference_api::InstallPolicy {
            offline: options.offline,
            non_interactive: options.non_interactive,
            license_accepted: true,
        },
        reread: reread.clone(),
        structure_reread: structure_reread.clone(),
        structure_strict: options.structure_reread == StructureRereadMode::On,
    };
    // One owner for the whole invocation (X1): the first input spawns it,
    // later documents SwitchDocument inside the port.
    let first_input = resolved_inputs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("missing.pdf"));
    let mut pdf_access = ocrx_scan::render::pdfium_owner::OwnerPdfAccess::spawn(
        pdfium_dir.clone(),
        first_input,
        RenderOptions {
            dpi,
            ..Default::default()
        },
        options.password.clone(),
        u32::try_from(window_depth * 2).unwrap_or(2),
    );
    let commit_recorder = ocrx_output::commit_profile::CommitRecorder::new_gated();
    let renderer = pdf_access.renderer(commit_recorder.clone());
    let mut renderer_box: Box<dyn ocrx_scan::window::PageRenderer> = Box::new(renderer);
    let events = std::cell::RefCell::new(Vec::new());
    let outcome = composed.scan.run(
        &resolved_inputs,
        &output_for,
        &app_options,
        &mut pdf_access,
        renderer_box.as_mut(),
        &mut composed.inference,
        &commit_recorder,
        &cancellation,
        &mut |event| events.borrow_mut().push(event),
    );
    // Emit buffered events through the Reporter contract. Multi-doc
    // boundary lines land on stderr (X1 golden "[doc i/n input]").
    let mut doc_index = 0_usize;
    for event in events.into_inner() {
        match event {
            ocrx_scan::ScanEvent::DocumentStarted { input, .. } => {
                doc_index += 1;
                if multi {
                    eprintln!(
                        "[doc {doc_index}/{} {}]",
                        resolved_inputs.len(),
                        input.display()
                    );
                }
            }
            ocrx_scan::ScanEvent::Analyzing { completed, total } => {
                reporter.progress(
                    &ProgressEvent::for_stage(ProgressStage::Analyzing, None, completed, total),
                    progress_mode,
                );
            }
            ocrx_scan::ScanEvent::PageProgress {
                page,
                generated_tokens,
                elapsed_seconds,
            } => {
                reporter.progress(
                    &ProgressEvent::for_stage(
                        ProgressStage::Recognizing,
                        Some(page),
                        generated_tokens,
                        generated_tokens,
                    ),
                    progress_mode,
                );
                let _ = elapsed_seconds;
            }
            ocrx_scan::ScanEvent::DocumentFinished { .. } => {}
        }
    }
    // X1.2 owner contract (fail-loud): a poisoned owner — missing library,
    // terminated or timed-out worker — aborts the whole invocation with the
    // structured "pdfium owner failed" error, it never degrades into
    // per-document skips.
    if let Some(cause) = pdf_access.poisoned() {
        reporter.error(
            &ErrorReport {
                code: 3,
                kind: "pdf".to_owned(),
                message: "scan aborted: pdfium owner failed".to_owned(),
                cause: Some(cause),
                suggestion: Some(
                    "check the PDFium library location (OCRX_PDFIUM_LIBRARY or the bundled copy)"
                        .to_owned(),
                ),
            },
            report_format,
        );
        emit_reporter(&mut reporter);
        std::process::exit(3);
    }
    let Ok(outcome) = outcome else {
        // Whole-invocation infrastructure failure (rare: I/O abort).
        eprintln!("error: scan invocation failed");
        std::process::exit(3);
    };
    let cancelled = outcome.cancelled;
    let mut doc_results: Vec<(PathBuf, i32)> = Vec::new();
    for document in &outcome.documents {
        if let Some(failure) = &document.failure {
            // Document-level failure: an Err VALUE (the orchestrator skips
            // it and keeps going, X1.2).
            reporter.error(
                &ErrorReport {
                    code: failure.code,
                    kind: failure.kind.clone(),
                    message: failure.message.clone(),
                    cause: failure.cause.clone(),
                    suggestion: None,
                },
                report_format,
            );
            doc_results.push((document.input.clone(), i32::from(failure.code)));
            continue;
        }
        let result_json = serialize_scan_result(&ScanResultFields {
            model_identity: document.identity.as_ref(),
            status: if document.summary.failed_pages == 0
                && document.summary.truncated_pages == 0
                && document.summary.degenerated_pages == 0
            {
                "success"
            } else {
                "partial"
            },
            input: &document.input,
            markdown_path: document.markdown_path.as_deref(),
            output_dir: &document.output_dir,
            profile: &options.profile,
            required_quality_tier: &options.quality_tier,
            document_type: &document.document_type,
            ocr_model_loaded: document.identity.is_some(),
            backend: document.backend.as_deref().unwrap_or("cpu"),
            total_pages: document.summary.selected_pages,
            completed_pages: document.summary.completed_pages,
            failed_pages: document.summary.failed_pages,
            truncated_pages: document.summary.truncated_pages,
            degenerated_pages: document.summary.degenerated_pages,
            think_blocks_stripped_total: document.summary.think_blocks_stripped,
            warnings: document.summary.warnings.clone(),
            diagnostics: document.summary.diagnostics.clone(),
            pdf_routing: document.pdf_routing.clone(),
            routing_counts: app_counts(&document.routing_counts),
            image_dir: &document.output_dir.join("images"),
            image_count: document.images_count,
            elapsed_ms: document.elapsed_ms,
            reread_json: reread_result_identity(
                reread.as_ref(),
                &document.summary.structure_reread_json,
                document.summary.tables_reread_total,
                document.summary.replaced_total,
            ),
        });
        if let Err(error) = ocrx_output::atomic_write(
            document.output_dir.join("result.json"),
            result_json.as_bytes(),
        ) {
            reporter.error(
                &ErrorReport {
                    code: 5,
                    kind: "output".to_owned(),
                    message: "result.json commit failed".to_owned(),
                    cause: Some(error.to_string()),
                    suggestion: None,
                },
                report_format,
            );
            doc_results.push((document.input.clone(), 5));
            continue;
        }
        if report_format == ReportFormat::Json {
            if !multi {
                reporter.stdout.push_str(&result_json);
                reporter.stdout.push('\n');
            }
        } else {
            reporter.stdout.push_str(&format!(
                "document_type={} native_text={} ocr={} empty={} fallback={}\n",
                document.document_type,
                document.routing_counts.native_text,
                document.routing_counts.ocr,
                document.routing_counts.empty,
                document.routing_counts.fallback
            ));
        }
        // BLK-7: the frozen per-document mapping — page-level failures are
        // PARTIAL (10, real_scan_fault_injection contract), not document
        // failures (3). Document-level failures keep their own code.
        let exit = document_exit_code(document, cancelled);
        doc_results.push((document.input.clone(), exit));
    }
    // Multi-input: scan-summary.json + aggregate exit (X1.3/X1.4).
    if multi {
        let documents: Vec<ScanSummaryDocument> = doc_results
            .iter()
            .map(|(input, exit)| ScanSummaryDocument {
                input: input.clone(),
                output_dir: output_for(input),
                status: match *exit {
                    0 => "success",
                    130 => "cancelled",
                    code if (3..10).contains(&code) => "failed",
                    _ => "partial",
                },
                exit_code: *exit,
                elapsed_ms: 0,
            })
            .collect();
        let summary = scan_summary_json(
            &resolved_inputs,
            output_root.as_deref().unwrap_or(Path::new(".")),
            &documents,
            started.elapsed().as_millis(),
        );
        if let Some(root) = &output_root {
            if let Err(error) =
                ocrx_output::atomic_write(root.join("scan-summary.json"), summary.as_bytes())
            {
                eprintln!("warning: scan-summary.json commit failed: {error}");
            }
            if report_format == ReportFormat::Json {
                reporter.stdout.push_str(&summary);
                reporter.stdout.push('\n');
            }
        }
    }
    emit_reporter(&mut reporter);
    let codes: Vec<i32> = doc_results.iter().map(|(_, exit)| *exit).collect();
    std::process::exit(final_scan_exit(cancelled, multi, &codes));
}

/// Final scan exit (RBLK-3): cancellation dominates everything — including
/// a cancel before the first document started, where empty codes must not
/// fold into 0. Otherwise multi-input follows the X1.4 aggregation ladder
/// (130 > document failure 3 > partial 10 > 0); single input keeps the
/// document's own structured code (usage 2 must not collapse into 3).
#[must_use]
fn final_scan_exit(cancelled: bool, multi: bool, codes: &[i32]) -> i32 {
    if cancelled {
        return 130;
    }
    if multi {
        return aggregate_scan_exit(codes);
    }
    codes.first().copied().unwrap_or(0)
}

/// Frozen per-document exit mapping (BLK-7): cancellation dominates
/// everything (RBLK-3 — a document failure racing a global cancel still
/// reports 130); page-level failures are partial (10); document-level
/// failures carry their structured code; a clean document is 0.
#[must_use]
fn document_exit_code(document: &ocrx_scan::DocumentOutcome, cancelled: bool) -> i32 {
    if cancelled || document.summary.cancelled {
        return 130;
    }
    if let Some(failure) = &document.failure {
        return i32::from(failure.code);
    }
    if document.summary.failed_pages > 0
        || document.summary.truncated_pages > 0
        || document.summary.degenerated_pages > 0
    {
        return 10;
    }
    0
}

/// Bridge the scan crate's counter type onto the legacy report shape.
/// Frozen routing counters for result.json (moved from the deleted legacy
/// helper block; the scan crate's `RoutingCounts` remains the source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoutingCounts {
    pub native_text: usize,
    pub ocr: usize,
    pub empty: usize,
    pub fallback: usize,
}

fn app_counts(counts: &ocrx_scan::app::RoutingCounts) -> RoutingCounts {
    RoutingCounts {
        native_text: usize::try_from(counts.native_text).unwrap_or(usize::MAX),
        ocr: usize::try_from(counts.ocr).unwrap_or(usize::MAX),
        empty: usize::try_from(counts.empty).unwrap_or(usize::MAX),
        fallback: usize::try_from(counts.fallback).unwrap_or(usize::MAX),
    }
}

struct ScanSummaryDocument {
    input: PathBuf,
    output_dir: PathBuf,
    status: &'static str,
    exit_code: i32,
    /// Wall time of this document's scan, from the orchestrator's clock
    /// (skipped documents report 0).
    elapsed_ms: u128,
}

/// X1.4 exit aggregation: any cancellation dominates (130); otherwise any
/// document-level failure (usage/runtime/pdf/output codes) dominates the
/// page-level partial code; all-success is 0 unless some document finished
/// with page-level incompleteness (10).
fn aggregate_scan_exit(codes: &[i32]) -> i32 {
    if codes.contains(&130) {
        130
    } else if codes.iter().any(|code| *code != 0 && *code != 10) {
        3
    } else if codes.contains(&10) {
        10
    } else {
        0
    }
}

/// X1.3 output layout: `<output-root>/<doc-stem>-<hash8>/`, where hash8 is
/// the first 8 hex of the canonical input path's SHA-256 - deterministic,
/// collision-proof for equal stems from different directories.
fn output_dir_for_doc(output_root: &Path, canonical_input: &Path) -> PathBuf {
    let stem = canonical_input
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_owned());
    let digest = ocrx_core::sha256(canonical_input.to_string_lossy().as_bytes());
    output_root.join(format!("{stem}-{}", &digest[..8]))
}

/// X1.3: the orchestrated invocation's aggregate summary file.
fn scan_summary_json(
    inputs: &[PathBuf],
    output_root: &Path,
    documents: &[ScanSummaryDocument],
    elapsed_ms: u128,
) -> String {
    let documents_json = documents
        .iter()
        .map(|document| {
            format!(
                "{{\"input\":\"{}\",\"output_dir\":\"{}\",\"status\":\"{}\",\"exit_code\":{},\"elapsed_ms\":{}}}",
                json_escape_path(&document.input),
                json_escape_path(&document.output_dir),
                document.status,
                document.exit_code,
                document.elapsed_ms,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let inputs_json = inputs
        .iter()
        .map(|input| json_quote(&input.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":1,\"mode\":\"multi\",\"inputs\":[{}],\"output_root\":\"{}\",\"documents\":[{}],\"elapsed_ms\":{}}}",
        inputs_json,
        json_escape_path(output_root),
        documents_json,
        elapsed_ms,
    )
}

/// Multi-input orchestrated scan (X1): one PDFium owner for the whole
/// invocation, one inference actor reused across documents, document-level
/// FIFO with windows never crossing documents, exit aggregation, and a
/// `scan-summary.json` next to the per-document directories.
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
    /// O3-T21/N1: the actual inference identity — the model object for
    /// non-default models derives from it; the default (ovisocr2) shape
    /// keeps the historical frozen string byte-identically.
    model_identity: Option<&'a ocrx_inference_api::InferenceIdentity>,
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
    degenerated_pages: u32,
    think_blocks_stripped_total: u64,
    warnings: Vec<String>,
    diagnostics: Vec<PageDiagnostic>,
    pdf_routing: Vec<String>,
    routing_counts: RoutingCounts,
    image_dir: &'a Path,
    image_count: usize,
    elapsed_ms: u128,
    /// R2.5/R2.6: precomputed reread object (identity + counters — ONE
    /// key; a second `reread` would shadow the identity in last-wins
    /// parsers).
    reread_json: String,
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
        match fields.model_identity {
            // N1 derivation for the non-default faces: every field comes
            // from the actual identity the platform reported.
            Some(identity) if identity.model.model.as_str() != "ovisocr2" => {
                let mmproj_revision = identity
                    .model
                    .assets
                    .iter()
                    .find(|asset| asset.role.as_str() == "mmproj")
                    .map(|asset| asset.sha256.clone())
                    .unwrap_or_default();
                format!(
                    "{{\"id\":\"{}\",\"name\":\"{}\",\"family\":\"{}\",\"profile\":\"{}\",\"revision\":\"{}\",\"mmproj_revision\":\"{}\",\"adapter\":\"{}\",\"adapter_version\":\"{}\",\"prompt_schema_version\":1,\"runtime\":\"llama.cpp@{}\",\"quantization\":\"{}\"}}",
                    identity.model.model.as_str(),
                    identity.display.display_name,
                    identity.display.family,
                    identity.model.profile.as_str(),
                    identity.model.source_revision,
                    mmproj_revision,
                    identity.adapter.adapter.as_str(),
                    identity.adapter.version,
                    identity.runtime.wrapper_version,
                    identity.display.quantization
                )
            }
            // The default-model shape is frozen byte-for-byte (SS83 item 5:
            // the historical `llama.cpp@0.1.152` runtime string stays; the
            // 0.1.152 -> current correction is a separate contract change).
            _ => format!(
                "{{\"id\":\"ovisocr2\",\"name\":\"OvisOCR2\",\"family\":\"Ovis\",\"profile\":\"{}\",\"revision\":\"{}\",\"mmproj_revision\":\"{}\",\"adapter\":\"ovisocr2\",\"adapter_version\":\"0.1.0\",\"prompt_schema_version\":1,\"runtime\":\"llama.cpp@0.1.152\",\"quantization\":\"{}\"}}",
                fields.profile,
                ocrx_inference::builtin::OVISOCR2_REVISION,
                ocrx_inference::builtin::OVISOCR2_REVISION,
                quantization
            ),
        }
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
        "{{\"schema_version\":3,\"job_id\":\"scan\",\"status\":\"{}\",\"input\":\"{}\",\"markdown_path\":{},\"output_dir\":\"{}\",\"required_quality_tier\":\"{}\",\"document_type\":\"{}\",\"ocr_model_loaded\":{},\"model\":{},\"backend\":\"{}\",\"reread\":{},\"pages\":{{\"total\":{},\"completed\":{},\"failed\":{},\"truncated\":{},\"degenerated\":{},\"think_blocks_stripped_total\":{}}},\"routing_counts\":{{\"native_text\":{},\"ocr\":{},\"empty\":{},\"fallback\":{}}},\"images\":{{\"directory\":\"{}\",\"count\":{}}},\"elapsed_ms\":{},\"warnings\":{},\"diagnostics\":[{}],\"pdf_routing\":[{}],\"routing_metadata\":[{}]}}",
        fields.status,
        json_escape_path(fields.input),
        markdown,
        json_escape_path(fields.output_dir),
        fields.required_quality_tier,
        fields.document_type,
        fields.ocr_model_loaded,
        model,
        fields.backend,
        fields.reread_json,
        fields.total_pages,
        fields.completed_pages,
        fields.failed_pages,
        fields.truncated_pages,
        fields.degenerated_pages,
        fields.think_blocks_stripped_total,
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

fn resolve_progress_mode(progress: &str, log_level: &str) -> ProgressMode {
    // An explicit --progress always wins (review m6): `--log-level error`
    // used to silently override an explicit bar/plain. The downgrade only
    // shapes the automatic choice.
    match progress {
        "jsonl" => ProgressMode::Jsonl,
        "bar" => ProgressMode::Bar,
        "plain" => ProgressMode::Plain,
        "off" => ProgressMode::Off,
        _ => {
            if matches!(log_level, "error") {
                ProgressMode::Off
            } else if std::io::stderr().is_terminal() {
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

/// Derives the doctor [`ocrx_inference_api::DeviceDiagnostics`] from an
/// already-intersected snapshot (same pure rule as the device manager's
/// diagnose: recommend ladder + auto priority + installed overview).
fn doctor_diagnostics_from_snapshot(
    snapshot: &ocrx_inference_api::ResourceSnapshot,
    cache_root: &Path,
    model: &str,
    resolved_profile: Option<&str>,
) -> ocrx_inference_api::DeviceDiagnostics {
    // O3-T21/D-11: the readiness face derives its ladder and installed
    // overview from the SELECTED model's frozen catalog entries — the
    // no-flag shape (default model) stays byte-identical.
    let catalog = ocrx_inference::ModelCatalogV2::builtin();
    let model_id =
        ocrx_inference_api::ModelId::new(model).unwrap_or_else(|_| unreachable!("validated id"));
    let profiles = catalog
        .profiles(&model_id)
        .unwrap_or_else(|_| unreachable!("doctor model gate validated by the CLI parser"));
    let profile_names: Vec<&str> = profiles
        .iter()
        .map(ocrx_inference_api::ProfileId::as_str)
        .collect();
    // The resolved profile (implicit default) wins when provided —
    // recomputing with the single-model ladder produced a profile that
    // contradicted the scan's resolved pair at 6-7 GiB (review BLOCK-1).
    let recommended_profile = resolved_profile.map(ToOwned::to_owned).unwrap_or_else(|| {
        ocrx_inference::recommend::recommend_profile(&profile_names, snapshot.memory_gb)
            .map(|profile| profile.to_string())
            .unwrap_or_else(|_| "unavailable".to_owned())
    });
    let recommended_device = if snapshot.metal_available {
        "metal"
    } else if snapshot.cuda_available {
        "cuda"
    } else if snapshot.vulkan_available {
        "vulkan"
    } else if snapshot.cpu_available {
        "cpu"
    } else {
        "unavailable"
    };
    let installed_profiles = profile_names
        .iter()
        .copied()
        .filter(|profile| {
            catalog
                .manifest(
                    &model_id,
                    &ocrx_inference_api::ProfileId::new(*profile).expect("checked id is valid"),
                )
                .ok()
                .and_then(|manifest| {
                    ocrx_inference::verify::is_installed(cache_root, &manifest).ok()
                })
                .unwrap_or(false)
        })
        .map(ToOwned::to_owned)
        .collect();
    ocrx_inference_api::DeviceDiagnostics {
        platform: platform_paths::detect().as_str().to_owned(),
        backend: ocrx_inference_api::BackendId::new(recommended_device)
            .unwrap_or_else(|_| ocrx_inference_api::BackendId::new("cpu").expect("cpu is valid")),
        image_token_limit: default_max_image_tokens(model),
        memory_gb: snapshot.memory_gb,
        metal_available: snapshot.metal_available,
        cpu_available: snapshot.cpu_available,
        recommended_profile,
        recommended_device: recommended_device.to_owned(),
        cache_root: cache_root.to_string_lossy().into_owned(),
        installed_profiles,
        profiles: profile_names
            .iter()
            .map(|profile| (*profile).to_owned())
            .collect(),
    }
}

/// Doctor backend display: the frozen title-case strings ("Cpu"/"Metal"/
/// "Cuda"/"Vulkan") derived from the device manager's backend id.
fn doctor_backend_display(diagnostics: &ocrx_inference_api::DeviceDiagnostics) -> String {
    let mut display = diagnostics.backend.as_str().to_owned();
    if let Some(first) = display.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    display
}

fn print_reporter(reporter: Reporter) {
    print!("{}", reporter.stdout);
    eprint!("{}", reporter.stderr);
}

fn print_help(topic: Option<&str>) {
    match topic {
        None => println!("ocrx <version|scan|doctor|model|skill|help>"),
        Some("scan") => {
            println!("ocrx scan <input.pdf> [more.pdf|directory]... [options]");
            println!("  Multiple inputs (or a directory of PDFs) run as one orchestrated");
            println!("  invocation; --output-dir is then the output root (required), with");
            println!("  one <stem>-<hash8> subdirectory per document plus scan-summary.json.");
            println!("  -o, --output-dir DIR");
            println!("  --pages SPEC                 page range, e.g. 1,3-5");
            println!("  --model ovisocr2|ovisocr3");
            println!("  --model-profile auto|compact|balanced|quality|reference");
            println!("  --pdf-mode auto|text|ocr");
            println!("  --quality-tier l1|l2 (default l2)");
            println!("  --device auto|cpu|metal|cuda|vulkan");
            println!("  --report text|json");
            println!("  --dpi DPI                    render resolution (1-600, default 300)");
            println!("  --max-image-tokens N         image token budget (>=128, default 1024)");
            println!(
                "  --max-output-tokens N        output token budget (128-16384, default 16384)"
            );
            println!("  --context-size N             context size (>=1024, default 20480)");
            println!("  --progress auto|bar|plain|jsonl|off");
            println!("  --log-level error|warn|info|debug|trace");
            println!("  --offline                    do not download");
            println!("  --non-interactive            disable interactive prompts");
            println!(
                "  --table-reread on|off        table region reread pipeline (default off)
  --layout-model ID            layout model id (frozen v1: pp-doclayout-v2)
  --layout-regions PATH        sidecar regions JSON for the document
  --keep-pages                 keep full-page PNGs after merge"
            );
            println!("  --password VALUE             PDF open password (never logged)");
            println!("  --resume | --overwrite       mutually exclusive");
        }
        Some("doctor") => println!("ocrx doctor [--report json] [--model ovisocr2|ovisocr3]"),
        Some("model") => {
            println!("ocrx model list [--model ovisocr2|ovisocr3]");
            println!("ocrx model info [--model ovisocr2|ovisocr3] [PROFILE]");
            println!("ocrx model recommend [--model ovisocr2|ovisocr3]");
            println!(
                "ocrx model install [--model ovisocr2|ovisocr3] PROFILE [--offline] [--non-interactive]"
            );
            println!("ocrx model remove [--model ovisocr2|ovisocr3] PROFILE");
            println!(
                "ocrx model verify [--model ovisocr2|ovisocr3] PROFILE [--offline] [--non-interactive]"
            );
        }
        Some("skill") => println!("ocrx skill <install|uninstall>"),
        Some(topic) => eprintln!("unknown help topic: {topic}"),
    }
}

/// Reports the current platform identifier for compatibility keys and the
/// doctor JSON report (single spelling source: `platform_paths::detect`).
fn current_platform() -> &'static str {
    platform_paths::detect().as_str()
}

fn doctor_json_report(
    snapshot: ocrx_inference_api::ResourceSnapshot,
    model: &str,
    resolved_profile: Option<&str>,
) -> String {
    let cache_root = std::env::var_os("OCRX_MODEL_CACHE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(ocrx_inference::default_cache_root);
    doctor_json_report_with_paths(snapshot, cache_root, None, model, resolved_profile)
}

fn doctor_json_report_with_paths(
    snapshot: ocrx_inference_api::ResourceSnapshot,
    cache_root: PathBuf,
    home: Option<&Path>,
    model: &str,
    resolved_profile: Option<&str>,
) -> String {
    // T073: doctor flows through the DeviceManager DTO shape. The injected
    // snapshot is ALREADY hardware∩compiled-intersected (production builds
    // it via resource_snapshot()), so the DTO derives from it without
    // re-probing — the formatter tests inject artificial snapshots and the
    // JSON must mirror them (golden behavior).
    let diagnostics =
        doctor_diagnostics_from_snapshot(&snapshot, &cache_root, model, resolved_profile);
    let profile = diagnostics.recommended_profile.clone();
    let device = diagnostics.recommended_device.clone();
    let platform = diagnostics.platform.clone();
    // Single source of the doctor spelling; the frozen golden strings
    // ("Metal"/"Cuda"/"Vulkan"/"Cpu") map from the backend id.
    let backend = doctor_backend_display(&diagnostics);
    let installed_profiles: Vec<String> = diagnostics.installed_profiles.clone();
    // `home_dir` covers the Windows USERPROFILE/HOMEDRIVE+HOMEPATH chain; a
    // bare $HOME read is always unset in cmd.exe, which used to force
    // skill_installed=false on Windows even after a successful install.
    let environment_home = home_dir().ok();
    let skill_installed = home
        .map(|home| home.join(".codex/skills/ocrx/SKILL.md").is_file())
        .or_else(|| {
            environment_home
                .as_deref()
                .map(|home| home.join(".codex/skills/ocrx/SKILL.md").is_file())
        })
        .unwrap_or(false);
    let profiles_json = diagnostics
        .profiles
        .iter()
        .map(|profile| format!("\"{profile}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":2,\"version\":\"{}\",\"platform\":\"{platform}\",\"model\":\"{model}\",\"backend\":\"{backend}\",\"image_token_limit\":{},\"memory_gb\":{},\"metal_available\":{},\"cpu_available\":{},\"cache_root\":\"{}\",\"installed_profiles\":[{}],\"recommended_profile\":\"{profile}\",\"recommended_device\":\"{device}\",\"skill_installed\":{},\"profiles\":[{profiles_json}]}}",
        env!("CARGO_PKG_VERSION"),
        diagnostics.image_token_limit,
        diagnostics.memory_gb,
        diagnostics.metal_available,
        diagnostics.cpu_available,
        json_escape_path(&cache_root),
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
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{
        CliCommand, ConfigSource, DEFAULT_MODEL, ResolvedConfig, RoutingCounts, ScanOptions,
        ScanResultFields, ScanSummaryDocument, StructureRereadMode, aggregate_scan_exit,
        classify_install_error, current_platform, doctor_json_report, document_exit_code,
        final_scan_exit, injected_failure, output_dir_for_doc, parse_command,
        parse_injected_failure_page, parse_page_selection, parse_scan_args, planned_ocr_pages,
        resolve_config, resolve_scan_inputs, scan_summary_json,
    };
    use ocrx_core::{
        PageClassification, PageType, PdfReasonCode, ProcessingMethod, RouteConfidence,
    };
    use ocrx_output::{PageDiagnostic, RenderAudit, ResultReport, ResultStatus};
    use ocrx_scan::app::initial_ocr_queue_for_mode;
    use ocrx_scan::native::{
        NativeFirstPhaseResult, NativeOnlyDecision, NativeValidationRoute, build_final_ocr_queue,
        native_only_decision, ocr_queue_after_native_phase, run_native_first_phase,
        service_pages_from_ocr_queue,
    };
    use ocrx_scan::plan::dynamic_fallback_requires_model_assets;
    use ocrx_scan::state::OcrQueuePage;

    // --- M4.2/D-64: the new-flag combination table --------------------------

    fn structure_reread_on_args(regions: &Path) -> Vec<&str> {
        let regions_str = regions.to_str().expect("temp path is utf-8");
        vec![
            "a.pdf",
            "--table-structure-reread",
            "on",
            "--layout-model",
            "pp-doclayout-v2",
            "--layout-regions",
            Box::leak(regions_str.to_owned().into_boxed_str()),
        ]
    }

    #[test]
    fn structure_reread_flag_parses_with_three_states() {
        // M4.3/D-71: ABSENT = best-effort ON (the default flip).
        let base = parse_scan_args(&["a.pdf"]).unwrap();
        assert_eq!(
            base.structure_reread,
            StructureRereadMode::DefaultBestEffort
        );
        // Explicit ON parses (regions auto-generate at run time — no
        // parse-time layout-input requirement anymore).
        let tmp = std::env::temp_dir().join("ocrx-m42-regions-exists.json");
        std::fs::write(&tmp, b"{}").unwrap();
        let on = parse_scan_args(&structure_reread_on_args(&tmp)).unwrap();
        assert_eq!(on.structure_reread, StructureRereadMode::On);
        assert_eq!(on.layout_model.as_deref(), Some("pp-doclayout-v2"));
        // Explicit ON even WITHOUT any layout inputs parses (auto-gen).
        let on_bare = parse_scan_args(&["a.pdf", "--table-structure-reread", "on"]).unwrap();
        assert_eq!(on_bare.structure_reread, StructureRereadMode::On);
        let off = parse_scan_args(&["a.pdf", "--table-structure-reread", "off"]).unwrap();
        assert_eq!(off.structure_reread, StructureRereadMode::Off);
        assert!(parse_scan_args(&["a.pdf", "--table-structure-reread", "maybe"]).is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn structure_reread_validation_matches_the_d71_contract() {
        // D-71: the structure arm auto-generates regions — an explicit ON
        // without layout inputs validates clean.
        let on_bare = parse_scan_args(&["a.pdf", "--table-structure-reread", "on"]).unwrap();
        assert!(super::validate_reread(&on_bare).is_ok());

        // Layout flags without an ARMED arm: still an error naming both
        // switches — the arm must be explicitly OFF for this combination.
        let mut off_base = parse_scan_args(&["a.pdf"]).unwrap();
        off_base.structure_reread = StructureRereadMode::Off;
        let err = super::validate_reread(&ScanOptions {
            layout_model: Some("pp-doclayout-v2".to_owned()),
            ..off_base
        })
        .unwrap_err();
        assert!(
            err.contains("--table-structure-reread on"),
            "message must name both switches: {err}"
        );
        // Under the DEFAULT (best-effort) the same flags are CONSUMED — ok.
        let default_base = parse_scan_args(&["a.pdf"]).unwrap();
        assert!(
            super::validate_reread(&ScanOptions {
                layout_model: Some("pp-doclayout-v2".to_owned()),
                ..default_base
            })
            .is_ok()
        );
    }

    #[test]
    fn structure_only_run_reports_its_own_section() {
        // M4.2 BLOCK-3: a structure-only run must NOT render as
        // enabled:false — its own section rides the blob with the verified
        // engine identity and the current rules generation.
        let identity = serde_json::json!({
            "engine": "slanet-plus", "models_revision": "bae6e5f8", "protocol_revision": "proto3",
            "device": "gpu",
            "paddleocr": "3.7.0", "paddlex": "3.7.2",
            "paddlepaddle": "3.3.1", "models_sha256": {}
        });
        let blob = super::reread_result_identity(None, &identity.to_string(), 4, 0);
        assert!(
            blob.contains("\"enabled\":false"),
            "old arm stays off: {blob}"
        );
        assert!(blob.contains("\"table_structure_reread\""), "{blob}");
        assert!(blob.contains("\"slanet-plus\""), "{blob}");
        assert!(blob.contains("\"rules_generation\":5"), "{blob}");
        assert!(blob.contains("\"tables_reread_total\":4"), "{blob}");
        // OFF runs keep the exact pre-M4.2 shape (no structure section).
        let off = super::reread_result_identity(None, "", 0, 0);
        assert!(!off.contains("table_structure_reread"), "{off}");
    }

    #[test]
    fn old_flag_only_run_keeps_the_vlm_era_generation_in_the_result_identity() {
        // D-63: old-flag-only ON reports the VLM-era generation (2) in the
        // result identity blob; the SLANet generation (3) lives in the
        // checkpoint compatibility identity.
        let on = super::reread_result_identity(
            Some(&ocrx_scan::compatibility::RereadOptions {
                layout_model_id: "pp-doclayout-v2".to_owned(),
                layout_assets_sha256: "hash".to_owned(),
                regions_path: PathBuf::from("r.json"),
            }),
            "",
            0,
            0,
        );
        assert!(on.contains("\"rules_generation\":2"), "{on}");
    }

    // --- X1.3: orchestrated output layout and exit aggregation -------------

    #[test]
    fn output_dir_for_doc_is_deterministic_and_collision_free_for_same_stems() {
        let root = Path::new("root");
        // Fixtures must use the host's native separator style: on unix a
        // backslash path is a single component, so file_stem would swallow
        // the directory part and break the name assertions below.
        #[cfg(target_os = "windows")]
        let (a, b) = (
            Path::new(r"D:\docs\guide.pdf"),
            Path::new(r"D:\other\guide.pdf"),
        );
        #[cfg(not(target_os = "windows"))]
        let (a, b) = (Path::new("/docs/guide.pdf"), Path::new("/other/guide.pdf"));
        let dir_a = output_dir_for_doc(root, a);
        let dir_b = output_dir_for_doc(root, b);
        assert_eq!(
            dir_a,
            output_dir_for_doc(root, a),
            "layout must be deterministic"
        );
        assert_ne!(
            dir_a, dir_b,
            "same stem from different directories must not collide"
        );
        let name_a = dir_a.file_name().unwrap().to_string_lossy().to_string();
        assert!(name_a.starts_with("guide-"), "{name_a}");
        assert_eq!(
            name_a.len(),
            "guide-".len() + 8,
            "hash suffix is exactly 8 hex: {name_a}"
        );
        let expected_hash = ocrx_core::sha256(a.to_string_lossy().as_bytes());
        assert!(name_a.ends_with(&expected_hash[..8]));
    }

    #[test]
    fn aggregate_scan_exit_follows_the_severity_ladder() {
        assert_eq!(aggregate_scan_exit(&[]), 0);
        assert_eq!(aggregate_scan_exit(&[0, 0]), 0);
        assert_eq!(aggregate_scan_exit(&[0, 10]), 10);
        assert_eq!(
            aggregate_scan_exit(&[0, 3, 10]),
            3,
            "document failure dominates partial"
        );
        assert_eq!(aggregate_scan_exit(&[9, 10, 0]), 3);
        assert_eq!(
            aggregate_scan_exit(&[10, 130]),
            130,
            "cancellation dominates everything"
        );
        assert_eq!(aggregate_scan_exit(&[0, 130, 5]), 130);
    }

    #[test]
    fn scan_summary_json_carries_inputs_output_and_per_document_status() {
        let documents = vec![
            ScanSummaryDocument {
                input: PathBuf::from(r"D:\docs\a.pdf"),
                output_dir: PathBuf::from(r"D:\out\a-12345678"),
                status: "success",
                exit_code: 0,
                elapsed_ms: 4_250,
            },
            ScanSummaryDocument {
                input: PathBuf::from(r"D:\docs\bad.pdf"),
                output_dir: PathBuf::from(r"D:\out\bad-abcdef01"),
                status: "skipped",
                exit_code: 3,
                elapsed_ms: 0,
            },
        ];
        let json = scan_summary_json(
            &[
                PathBuf::from(r"D:\docs\a.pdf"),
                PathBuf::from(r"D:\docs\bad.pdf"),
            ],
            Path::new(r"D:\out"),
            &documents,
            1_234,
        );
        assert!(
            json.starts_with("{\"schema_version\":1,\"mode\":\"multi\""),
            "{json}"
        );
        // json_escape_path keeps the raw separator spelling and escapes it.
        assert!(json.contains("\"output_root\":\"D:\\\\out\""), "{json}");
        assert!(json.contains("\"output_dir\":\"D:\\\\out"), "{json}");
        assert!(
            json.contains("\"status\":\"success\"") && json.contains("\"exit_code\":0"),
            "{json}"
        );
        assert!(
            json.contains("\"status\":\"skipped\"") && json.contains("\"exit_code\":3"),
            "{json}"
        );
        assert!(json.contains("\"elapsed_ms\":1234"), "{json}");
        assert!(json.contains("\"elapsed_ms\":4250"), "{json}");
        assert_eq!(
            json.matches("\"input\"").count(),
            2,
            "one input key per document entry: {json}"
        );
        assert!(json.contains("\"inputs\":[\"D:"), "{json}");
    }

    #[test]
    fn scan_parser_single_input_keeps_default_output_semantics() {
        let options = parse_scan_args(&["input.pdf"]).unwrap();
        assert_eq!(options.inputs, vec!["input.pdf".to_owned()]);
        assert!(options.output_dir.is_none());
    }

    // --- X1.1: multi-input scan surface ------------------------------------

    #[test]
    fn scan_parser_accepts_multiple_positional_inputs_with_output_root() {
        let options = parse_scan_args(&[
            "a.pdf",
            "--table-reread",
            "on",
            "--layout-model",
            "pp-doclayout-v2",
            "--layout-regions",
            &format!("missing-regions-{}.json", std::process::id()),
        ]);
        // A nonexistent regions file fails the completeness check with a
        // targeted message instead of a scan-time surprise.
        let error = options.err().unwrap_or_default();
        assert!(error.contains("--layout-regions"), "{error}");
        let dir = std::env::temp_dir().join(format!("ocrx-cli-reread-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let regions = dir.join("regions.json");
        std::fs::write(&regions, "{}").unwrap();
        let regions_path = regions.to_string_lossy().into_owned();
        let on = parse_scan_args(&[
            "a.pdf",
            "--table-reread",
            "on",
            "--layout-model",
            "pp-doclayout-v2",
            "--layout-regions",
            &regions_path,
        ])
        .unwrap();
        assert!(on.table_reread);
        assert_eq!(on.layout_model.as_deref(), Some("pp-doclayout-v2"));
        assert_eq!(on.layout_regions.as_deref(), Some(regions_path.as_str()));
        let off = parse_scan_args(&["a.pdf"]).unwrap();
        assert!(!off.table_reread);
        assert_eq!(off.layout_model, None);
        assert_eq!(off.layout_regions, None);
        // column-split retry arm (documented switch, parse arm restored).
        let split_off = parse_scan_args(&["a.pdf", "--column-split-retry", "off"]).unwrap();
        assert!(!split_off.column_split_retry);
        let split_on = parse_scan_args(&["a.pdf", "--column-split-retry", "on"]).unwrap();
        assert!(split_on.column_split_retry);
        let _ = std::fs::remove_dir_all(&dir);
        // Completeness/shape of the reread switch (no fs needed).
        let missing_model = parse_scan_args(&["a.pdf", "--table-reread", "on"])
            .err()
            .unwrap();
        assert!(missing_model.contains("--layout-model"), "{missing_model}");
        let unknown = parse_scan_args(&[
            "a.pdf",
            "--table-reread",
            "on",
            "--layout-model",
            "mineru-layout",
            "--layout-regions",
            "r.json",
        ])
        .err()
        .unwrap();
        assert!(unknown.contains("pp-doclayout-v2"), "{unknown}");
        let missing_regions = parse_scan_args(&[
            "a.pdf",
            "--table-reread",
            "on",
            "--layout-model",
            "pp-doclayout-v2",
        ])
        .err()
        .unwrap();
        assert!(
            missing_regions.contains("--layout-regions"),
            "{missing_regions}"
        );
        // M4.3/D-71: stray layout flags under the DEFAULT (best-effort)
        // are CONSUMED by the structure arm — no error; with the arm
        // explicitly OFF they still name both switches.
        assert!(parse_scan_args(&["a.pdf", "--layout-model", "pp-doclayout-v2"]).is_ok());
        for stray in [
            [
                "a.pdf",
                "--table-structure-reread",
                "off",
                "--layout-model",
                "pp-doclayout-v2",
            ]
            .as_slice(),
            [
                "a.pdf",
                "--table-structure-reread",
                "off",
                "--layout-regions",
                "r.json",
            ]
            .as_slice(),
        ] {
            let err = parse_scan_args(stray).err().unwrap();
            assert!(err.contains("--table-reread on"), "{err}");
        }
        assert!(parse_scan_args(&["a.pdf", "--table-reread", "maybe"]).is_err());
        // R2.5/BLK-5: the result identity reflects the effective reread
        // execution identity; the asset hash follows the frozen canonical
        // rule and is deterministic across resolutions.
        let off = super::reread_result_identity(None, "", 0, 0);
        assert!(off.contains("\"enabled\":false"), "{off}");
        assert!(off.contains("\"prompt_version\":\"\""));
        assert!(off.contains("\"rules_generation\":0"));
        assert!(off.contains("\"tables_reread_total\":0"));
        assert!(off.contains("\"replaced_total\":0"));
        let on = super::reread_result_identity(
            Some(&ocrx_scan::compatibility::RereadOptions {
                layout_model_id: "pp-doclayout-v2".to_owned(),
                layout_assets_sha256: "hash".to_owned(),
                regions_path: PathBuf::from("r.json"),
            }),
            "",
            3,
            1,
        );
        assert!(on.contains("\"enabled\":true"), "{on}");
        assert!(on.contains("\"layout_model\":\"pp-doclayout-v2\""));
        assert!(on.contains("\"prompt_version\":\"table-reread-v1\""));
        assert!(on.contains("\"rules_generation\":2"));
        assert!(on.contains("\"tables_reread_total\":3"));
        assert!(on.contains("\"replaced_total\":1"));
        let dir = std::env::temp_dir().join(format!("ocrx-cli-reread-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let regions = dir.join("regions.json");
        std::fs::write(&regions, "{}").unwrap();
        let mut base = parse_scan_args(&[
            "a.pdf",
            "--table-reread",
            "on",
            "--layout-model",
            "pp-doclayout-v2",
            "--layout-regions",
            regions.to_string_lossy().as_ref(),
        ])
        .unwrap();
        let first = super::reread_options(&base).unwrap().unwrap();
        let second = super::reread_options(&base).unwrap().unwrap();
        assert_eq!(first.layout_assets_sha256, second.layout_assets_sha256);
        // Frozen canonical rule: sha256 over manifest-order role:sha256 lines.
        let manifest = ocrx_models::layout::paddlex_layout_manifest(
            ocrx_models::layout::PADDLE_LAYOUT_MODEL_ID,
            None,
        )
        .unwrap();
        let mut canonical = String::new();
        for asset in &manifest.assets {
            canonical.push_str(&format!(
                "{}:{}
",
                asset.role, asset.sha256
            ));
        }
        assert_eq!(
            first.layout_assets_sha256,
            ocrx_output::sha256(canonical.as_bytes())
        );
        assert_eq!(first.layout_model_id, "pp-doclayout-v2");
        base.table_reread = false;
        base.layout_model = None;
        assert!(super::reread_options(&base).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let options = parse_scan_args(&["a.pdf", "b.pdf", "-o", "root", "--resume"]).unwrap();
        assert_eq!(options.inputs, vec!["a.pdf".to_owned(), "b.pdf".to_owned()]);
        assert_eq!(options.output_dir.as_deref(), Some("root"));
        assert!(options.resume);
    }

    #[test]
    fn scan_parser_collects_inputs_between_options() {
        let options = parse_scan_args(&[
            "a.pdf",
            "--resume",
            "b.pdf",
            "-o",
            "r",
            "--pdf-mode",
            "ocr",
            "c.pdf",
        ])
        .unwrap();
        assert_eq!(
            options.inputs,
            vec!["a.pdf".to_owned(), "b.pdf".to_owned(), "c.pdf".to_owned()]
        );
    }

    #[test]
    fn scan_parser_requires_output_root_for_multiple_inputs() {
        let error = parse_scan_args(&["a.pdf", "b.pdf"]).unwrap_err();
        assert!(
            error.contains("--output-dir"),
            "error must point at the missing output root: {error}"
        );
        // A single input keeps the implicit default; only the multi-input
        // mode demands the root.
        assert!(parse_scan_args(&["a.pdf"]).is_ok());
    }

    #[test]
    fn resolve_scan_inputs_expands_directories_dedupes_and_sorts() {
        let root = std::env::temp_dir().join(format!("ocrx-x11-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("docs");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.pdf", "a.pdf", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let direct = dir.join("z.pdf");
        std::fs::write(&direct, b"x").unwrap();
        let resolved = resolve_scan_inputs(&[
            dir.to_string_lossy().to_string(),
            direct.to_string_lossy().to_string(), // duplicates an expansion entry
            dir.join("a.pdf").to_string_lossy().to_string(), // explicit duplicate
        ])
        .unwrap();
        let names: Vec<String> = resolved
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        // Sorted by path, *.txt ignored, deduped against the expansion.
        assert_eq!(names, vec!["a.pdf", "b.pdf", "z.pdf"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_scan_inputs_rejects_missing_paths_and_pdfless_directories() {
        let root = std::env::temp_dir().join(format!("ocrx-x11-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let error = resolve_scan_inputs(&["definitely-missing.pdf".to_owned()]).unwrap_err();
        assert!(error.contains("input not found"), "{error}");
        let error = resolve_scan_inputs(&[root.to_string_lossy().to_string()]).unwrap_err();
        assert!(error.contains("no PDFs"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_parser_builds_typed_options_and_rejects_other_models() {
        assert_eq!(
            parse_scan_args(&["input.pdf", "-o", "out", "--pages", "3,1-2", "--resume"]),
            Ok(ScanOptions {
                inputs: vec!["input.pdf".to_owned()],
                output_dir: Some("out".to_owned()),
                pages: Some(vec![1, 2, 3]),
                model: "ovisocr3".to_owned(),
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
                password: None,
                column_split_retry: true,
                model_explicit: false,
                table_reread: false,
                structure_reread: StructureRereadMode::DefaultBestEffort,
                layout_model: None,
                layout_regions: None,
            })
        );
        assert!(parse_scan_args(&["input.pdf", "--model", "other"]).is_err());
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

    /// Resolves the compiled CLI binary (CARGO_BIN_EXE or a cargo run
    /// fallback), matching `run_cli`.
    /// A base ovisocr2 identity fixture (tests mutate the model face).
    fn test_support_identity() -> ocrx_inference_api::InferenceIdentity {
        ocrx_inference_api::InferenceIdentity {
            model: ocrx_inference_api::ModelIdentity::new(
                ocrx_inference_api::ModelId::new("ovisocr2").unwrap(),
                ocrx_inference_api::ProfileId::new("compact").unwrap(),
                "rev",
                Vec::new(),
            )
            .unwrap(),
            runtime: ocrx_inference::registry::runtime_identity(),
            adapter: ocrx_inference::registry::adapter_identity(),
            backend: ocrx_inference_api::BackendId::new("cpu").unwrap(),
            decode_generation: 3,
            batch_depth: 8,
            display: ocrx_inference_api::ModelDisplayMeta {
                display_name: "OvisOCR2".to_owned(),
                family: "Ovis".to_owned(),
                quantization: "Q4_K_M".to_owned(),
            },
        }
    }

    fn cli_binary() -> Command {
        if let Ok(binary) = std::env::var("CARGO_BIN_EXE_ocrx") {
            return Command::new(binary);
        }
        let mut command = Command::new("cargo");
        command
            .args(["run", "--quiet", "--bin", "ocrx", "--"])
            .current_dir(env!("CARGO_MANIFEST_DIR"));
        command
    }

    /// The vendored PDFium library directory: the repo root itself, or the
    /// main checkout two levels up from a nested worktree.
    fn pdfium_library_for_tests() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let mut candidates = vec![root.join("vendor/pdfium/bin")];
        if let Some(above) = root.parent().and_then(|dir| dir.parent()) {
            candidates.push(above.join("vendor/pdfium/bin"));
        }
        for candidate in candidates {
            if candidate.join("pdfium.dll").is_file() || candidate.join("libpdfium.dylib").is_file()
            {
                return std::fs::canonicalize(&candidate).unwrap_or(candidate);
            }
        }
        root.join("vendor/pdfium/bin")
    }

    /// The non-redistributable golden composite: the repo root, or the main
    /// checkout for a nested worktree (gitignored inputs).
    fn golden_source_pdf() -> Option<PathBuf> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let mut candidates =
            vec![root.join("assets/regression/ovisocr2-draft-v1/inputs/source.pdf")];
        if let Some(above) = root.parent().and_then(|dir| dir.parent()) {
            candidates.push(above.join("assets/regression/ovisocr2-draft-v1/inputs/source.pdf"));
        }
        candidates.into_iter().find(|path| path.is_file())
    }

    fn run_cli_with_cache(args: &[&str], cache: &Path) -> std::process::Output {
        if let Ok(binary) = std::env::var("CARGO_BIN_EXE_ocrx") {
            return Command::new(binary)
                .args(args)
                .env("OCRX_MODEL_CACHE", cache)
                .output()
                .expect("CLI test binary should run");
        }
        Command::new("cargo")
            .args(["run", "--quiet", "--bin", "ocrx", "--"])
            .args(args)
            .env("OCRX_MODEL_CACHE", cache)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo should launch the CLI test binary")
    }

    fn run_cli_with_env(args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
        if let Ok(binary) = std::env::var("CARGO_BIN_EXE_ocrx") {
            let mut command = Command::new(binary);
            command.args(args);
            for (key, value) in envs {
                command.env(key, value);
            }
            return command.output().expect("CLI test binary should run");
        }
        let mut command = Command::new("cargo");
        command
            .args(["run", "--quiet", "--bin", "ocrx", "--"])
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"));
        for (key, value) in envs {
            command.env(key, value);
        }
        command
            .output()
            .expect("cargo should launch the CLI test binary")
    }

    /// BLOCK-1 (review 2026-11-19): the three implicit-default faces —
    /// `model recommend`, `doctor`, and the scan's resolved pair — must
    /// agree at the D-50 memory tiers (real CLI subprocesses with
    /// OCRX_MEMORY_GB).
    #[test]
    fn d50_three_faces_agree_at_the_memory_tiers() {
        for (memory, expected_model, expected_profile) in [
            ("6", "ovisocr2", "balanced"),
            ("7", "ovisocr2", "balanced"),
            ("8", "ovisocr3", "quality"),
        ] {
            let envs = [("OCRX_MEMORY_GB", memory)];
            let recommend = run_cli_with_env(&["model", "recommend"], &envs);
            let recommend_text = String::from_utf8(recommend.stdout).unwrap();
            assert!(
                recommend_text.contains(&format!("model={expected_model}"))
                    && recommend_text.contains(&format!("profile={expected_profile}")),
                "{memory} GiB recommend: {recommend_text}"
            );
            let doctor = run_cli_with_env(&["doctor", "--report", "json"], &envs);
            let doctor_text = String::from_utf8(doctor.stdout).unwrap();
            assert!(
                doctor_text.contains(&format!("\"model\":\"{expected_model}\""))
                    && doctor_text
                        .contains(&format!("\"recommended_profile\":\"{expected_profile}\"")),
                "{memory} GiB doctor: {doctor_text}"
            );
        }
    }

    /// BLOCK-2 (review 2026-11-19): a legal FALLBACK configuration must
    /// pass the context gate after the resolution — the pre-parse gate
    /// rejected --context-size 18432 with the ovisocr3 budget.
    #[test]
    fn d50_fallback_context_size_passes_after_resolution() {
        let output = run_cli_with_env(
            &[
                "scan",
                "missing-review.pdf",
                "--context-size",
                "18432",
                "--offline",
                "--non-interactive",
            ],
            &[("OCRX_MEMORY_GB", "6")],
        );
        // The gate must NOT reject the config; the scan proceeds to the
        // input stage (exit 3 = missing input, the frozen page-count face).
        assert_eq!(
            output.status.code(),
            Some(3),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("context size"),
            "the context gate must not fire after fallback: {stderr}"
        );
    }

    #[test]
    fn cli_resume_of_truncated_page_keeps_partial_status_and_exit_10() {
        // BLK-2 regression (real CLI subprocess): a same-parameter --resume
        // that reuses a committed truncated OCR page must keep the document
        // marker, report status=partial with pages.truncated=1, and exit 10 —
        // never success/exit 0.
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let input = workspace.join("benchmarks/fixtures/pdf/normal-1-page.pdf");
        assert!(input.is_file(), "pdf fixture missing: {}", input.display());
        // pdfium is a vendored, gitignored binary: prefer the vendored copy,
        // then the ambient OCRX_PDFIUM_LIBRARY, and skip when neither exists.
        let pdfium = workspace.join("vendor/pdfium/bin/pdfium.dll");
        let pdfium = if pdfium.is_file() {
            pdfium
        } else if let Some(library) = std::env::var_os("OCRX_PDFIUM_LIBRARY") {
            std::path::PathBuf::from(library)
        } else {
            eprintln!("skipping: no vendored or OCRX_PDFIUM_LIBRARY pdfium available");
            return;
        };
        if !pdfium.is_file() {
            eprintln!("skipping: pdfium library not found: {}", pdfium.display());
            return;
        }
        let _ = 0;
        let root = std::env::temp_dir().join(format!(
            "ocrx-resume-truncated-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input_sha = ocrx_core::sha256_file(&input).unwrap();
        // T100-era seed: a V6 OCR-complete checkpoint carrying the true
        // reference identity (pinned hashes), truncated=true — so `--resume`
        // must REUSE it (pre-compare + integrity pass; zero decode since the
        // page is the only selected page) and keep the page partial.
        let assets = ocrx_inference::builtin::OVISOCR2_MODEL_ASSETS
            .iter()
            .find(|asset| asset.profile == "reference")
            .expect("pinned reference asset");
        let identity = ocrx_inference_api::InferenceIdentity {
            model: ocrx_inference_api::ModelIdentity::new(
                ocrx_inference_api::ModelId::new("ovisocr2").unwrap(),
                ocrx_inference_api::ProfileId::new("reference").unwrap(),
                ocrx_inference::builtin::OVISOCR2_REVISION,
                vec![
                    ocrx_inference_api::AssetIdentity {
                        role: ocrx_inference_api::AssetRole::new("mmproj").unwrap(),
                        sha256: ocrx_inference::builtin::OVISOCR2_MMPROJ_SHA256.to_owned(),
                        byte_len: ocrx_inference::builtin::OVISOCR2_MMPROJ_BYTES,
                    },
                    ocrx_inference_api::AssetIdentity {
                        role: ocrx_inference_api::AssetRole::new("model").unwrap(),
                        sha256: assets.sha256.to_owned(),
                        byte_len: assets.bytes,
                    },
                ],
            )
            .unwrap(),
            runtime: ocrx_inference_api::RuntimeIdentity {
                runtime: ocrx_inference_api::RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".to_owned(),
                sys_version: "0.1.161".to_owned(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".to_owned(),
            },
            adapter: ocrx_inference_api::AdapterIdentity {
                adapter: ocrx_inference_api::AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".to_owned(),
            },
            backend: ocrx_inference_api::BackendId::new("cpu").unwrap(),
            decode_generation: 3,
            batch_depth: 8,
            display: ocrx_inference_api::ModelDisplayMeta {
                display_name: "OvisOCR2".to_owned(),
                family: "Ovis".to_owned(),
                quantization: "BF16".to_owned(),
            },
        };
        let compatibility = ocrx_output::ScanCompatibility::ocr(
            &input_sha,
            ocrx_output::PdfIdentity {
                pdf_mode: "ocr".to_owned(),
                quality_tier: "l2".to_owned(),
                classifier_version: "pdf-route-v2".to_owned(),
                extractor_version: "pdf-text-v2".to_owned(),
                platform: current_platform().to_owned(),
            },
            ocrx_output::OcrIdentity {
                regions_sha256: String::new(),
                inference: identity,
                prompt_id: "ocr-markdown".to_owned(),
                prompt_version: "ocr-markdown-v1".to_owned(),
                prompt_schema_version: 1,
                dpi: 300,
                max_image_tokens: 1024,
                max_output_tokens: 16_384,
                context_size: 20_480,
                batch_depth: 8,
                decode_generation: 3,
                pixel_width_budget: 2880,
                pixel_height_budget: 2880,
                page_parser_version: "ovis-page-v1".to_owned(),
                table_reread: false,
                formula_reread: false,
                layout_model_id: String::new(),
                layout_model_assets_sha256: String::new(),
                reread_prompt_version: String::new(),
                reread_rules_generation: 0,
                table_structure_reread: None,
            },
        );
        // Artifacts must be intact for reuse: the md the checkpoint hashes.
        let markdown_body = "truncated page body";
        std::fs::write(root.join("page-0001.md"), markdown_body).unwrap();
        let page_json = r#"{"schema_version":1,"page_number":1,"blocks":[],"termination":"max_tokens","warnings":["truncated"]}"#;
        std::fs::write(root.join("page-0001.json"), page_json).unwrap();
        ocrx_output::write_v6(
            root.join("page-0001.checkpoint"),
            &ocrx_output::V6Checkpoint {
                page_number: 1,
                status: ocrx_output::V6Status::Complete,
                compatibility,
                output_sha256: ocrx_output::artifact_sha256(&root.join("page-0001.md"), &root)
                    .unwrap(),
                truncated: true,
                degenerated: false,
                attempts: 1,
                raw: None,
                warnings: vec!["truncated".to_owned()],
                think_blocks_stripped: 0,
                audit: None,
            },
        )
        .unwrap();
        let input = input.canonicalize().unwrap();
        let output_arg = root.canonicalize().unwrap();
        let mut command = match std::env::var("CARGO_BIN_EXE_ocrx") {
            Ok(binary) => Command::new(binary),
            Err(_) => {
                // Keep the subprocess contract even under cargo-run fallback.
                let mut command = Command::new("cargo");
                command
                    .args(["run", "--quiet", "--bin", "ocrx", "--"])
                    .current_dir(env!("CARGO_MANIFEST_DIR"));
                command
            }
        };
        let output = command
            .args([
                "scan",
                input.to_str().unwrap(),
                "--pages",
                "1",
                "--pdf-mode",
                "ocr",
                "--model-profile",
                "reference",
                "--device",
                "cpu",
                "--dpi",
                "300",
                "--output-dir",
                output_arg.to_str().unwrap(),
                "--resume",
                "--report",
                "json",
                "--offline",
                "--non-interactive",
            ])
            .env("OCRX_PDFIUM_LIBRARY", pdfium.canonicalize().unwrap())
            .output()
            .expect("CLI subprocess should run");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert_eq!(
            output.status.code(),
            Some(10),
            "exit code: {stdout}
--- stderr ---
{stderr}"
        );
        assert!(stdout.contains("\"status\":\"partial\""), "{stdout}");
        assert!(stdout.contains("\"truncated\":1"), "{stdout}");
        let result = root.join("result.json");
        if result.is_file() {
            let text = std::fs::read_to_string(&result).unwrap();
            assert!(text.contains("\"status\":\"partial\""), "{text}");
            assert!(text.contains("\"truncated\":1"), "{text}");
        }
        let document = std::fs::read_to_string(root.join("document.md")).unwrap();
        assert!(document.contains("输出被截断"), "{document}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // NOTE: the doctor stdout/stderr contract lives in tests/cli_subprocess.rs,
    // which spawns the binary directly. A `cargo run`-based twin here
    // intermittently captured cargo's own "Blocking waiting for file lock"
    // stderr chatter under parallel test load and false-failed.

    #[test]
    fn bundled_pdfium_is_resolved_from_installed_executable_layout() {
        // Upstream hardening (main 2a415a4): the install layout keeps pdfium
        // beside the executable (Windows) or under ../lib (macOS/Linux), and
        // the default resolution must find it without OCRX_PDFIUM_LIBRARY.
        let root = std::env::temp_dir().join(format!("ocrx-bundled-pdfium-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        let lib = root.join("lib");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        let executable = bin.join(if cfg!(windows) { "ocrx.exe" } else { "ocrx" });
        std::fs::write(&executable, b"binary").unwrap();
        #[cfg(target_os = "macos")]
        let library = lib.join("libpdfium.dylib");
        #[cfg(target_os = "windows")]
        let library = bin.join("pdfium.dll");
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        let library = lib.join("libpdfium.so");
        std::fs::write(&library, b"pdfium").unwrap();
        assert_eq!(
            super::bundled_pdfium_path(&executable)
                .canonicalize()
                .unwrap(),
            library.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
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
    fn model_command_parser_accepts_the_model_selector() {
        match parse_command(&["model", "list"]).unwrap() {
            CliCommand::Model { action, model, .. } => {
                assert_eq!(action, "list");
                assert_eq!(model, None);
            }
            other => panic!("{other:?}"),
        }
        match parse_command(&["model", "info", "--model", "ovisocr3", "quality"]).unwrap() {
            CliCommand::Model {
                action,
                model,
                profile,
                ..
            } => {
                assert_eq!(action, "info");
                assert_eq!(model.as_deref(), Some("ovisocr3"));
                assert_eq!(profile.as_deref(), Some("quality"));
            }
            other => panic!("{other:?}"),
        }
        // Unknown selector: scan-family error spelling (D-6).
        let error = parse_command(&["model", "list", "--model", "qwen3.8"]).unwrap_err();
        assert_eq!(error, "unsupported model: qwen3.8");
        // Profile-required actions still require a profile under a selector.
        let error = parse_command(&["model", "install", "--model", "ovisocr3"]).unwrap_err();
        assert_eq!(error, "model install requires a profile");
        // remove keeps rejecting the transaction flags under a selector.
        let error = parse_command(&[
            "model",
            "remove",
            "--model",
            "ovisocr3",
            "quality",
            "--offline",
        ])
        .unwrap_err();
        assert_eq!(error, "model remove does not support --offline");
    }

    #[test]
    fn install_error_classification_covers_the_kind_vocabulary() {
        // C2/D-5: exit codes frozen (offline/disk = 5, else 4); kinds are the
        // plugin contract (transport/hash/lock/offline/disk; license rides
        // the policy gate). The catch-all keeps code 4.
        use ocrx_inference_api::InferenceError as E;
        let table: [(E, u8, &str); 6] = [
            (E::Offline, 5, "offline"),
            (E::InsufficientDisk("no space".into()), 5, "disk"),
            (E::LicenseRequired, 4, "license"),
            (E::InstallLocked("locked".into()), 4, "lock"),
            (E::HashMismatch("bad hash".into()), 4, "hash"),
            (E::Transport("curl died".into()), 4, "transport"),
        ];
        for (error, code, kind) in table {
            let (got_code, got_kind, message) = classify_install_error(&error);
            assert_eq!(got_code, code, "code for {kind}");
            assert_eq!(got_kind, kind, "kind mapping");
            assert!(!message.is_empty(), "message present for {kind}");
        }
        // Unknown categories keep exit code 4 with the catch-all kind.
        let (code, kind, _) = classify_install_error(&E::MissingAsset("/gone".to_owned()));
        assert_eq!((code, kind), (4, "install"));
        // Offline message keeps the historical spelling (plain face).
        let (_, _, message) = classify_install_error(&E::Offline);
        assert_eq!(message, "model install blocked: offline mode");
    }

    #[test]
    fn model_command_parser_accepts_report_json_only_on_recommend_and_install() {
        // C2/D-4 (dsh-plugin-v0.1 stage 2): `--report json` is a machine-
        // readable face for EXACTLY recommend and install; every other model
        // subcommand keeps the historical unsupported-flag rejection.
        match parse_command(&["model", "recommend", "--report", "json"]).unwrap() {
            CliCommand::Model {
                action,
                report_json,
                ..
            } => {
                assert_eq!(action, "recommend");
                assert!(report_json);
            }
            other => panic!("{other:?}"),
        }
        match parse_command(&["model", "install", "--report", "json", "quality"]).unwrap() {
            CliCommand::Model {
                action,
                report_json,
                ..
            } => {
                assert_eq!(action, "install");
                assert!(report_json);
            }
            other => panic!("{other:?}"),
        }
        // Plain recommend/install stay report_json=false.
        match parse_command(&["model", "recommend"]).unwrap() {
            CliCommand::Model { report_json, .. } => assert!(!report_json),
            other => panic!("{other:?}"),
        }
        // Every other subcommand rejects --report.
        for action in ["list", "info", "verify", "remove"] {
            let arguments: Vec<&str> = match action {
                "verify" | "remove" => vec!["model", action, "quality", "--report", "json"],
                _ => vec!["model", action, "--report", "json"],
            };
            let error = parse_command(&arguments).unwrap_err();
            assert_eq!(error, "unsupported model flag: --report", "action {action}");
        }
        // The value is pinned to `json` (no text face on model commands).
        let error = parse_command(&["model", "recommend", "--report", "text"]).unwrap_err();
        assert_eq!(error, "unsupported model report format: text");
    }

    #[test]
    fn cli_model_list_default_lists_both_models_and_the_selector_narrows() {
        // C6: (a) the no-flag list gains the ovisocr3 line in declaration
        // order; (b) the first line keeps the historical bytes;
        // (c) the explicit `--model ovisocr2` shape is frozen on creation.
        let cache = std::env::temp_dir().join(format!("ocrx-model-list-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let output = run_cli_with_cache(&["model", "list"], &cache);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "model=ovisocr2
model=ovisocr3
"
        );
        for (args, expected) in [
            (
                &["model", "list", "--model", "ovisocr2"][..],
                "model=ovisocr2
",
            ),
            (
                &["model", "list", "--model", "ovisocr3"][..],
                "model=ovisocr3
",
            ),
        ] {
            let output = run_cli_with_cache(args, &cache);
            assert!(output.status.success(), "{args:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                expected,
                "{args:?}"
            );
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.is_empty(), "{args:?} stderr={stderr}");
        }
        // Unknown selector: no stdout, scan-family error on stderr, exit 2.
        let output = run_cli_with_cache(&["model", "list", "--model", "qwen3.8"], &cache);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("unsupported model: qwen3.8")
        );
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn cli_model_info_default_and_selector_golden() {
        let cache = std::env::temp_dir().join(format!("ocrx-model-info-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        // Default face (D-48): ovisocr3, single quality rung (empty cache →
        // nothing installed).
        let output = run_cli_with_cache(&["model", "info"], &cache);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "model=ovisocr3 profiles=quality installed=
"
        );
        // OvisOCR3 selector face: frozen on creation (single rung).
        for (args, expected) in [
            (
                &["model", "info", "--model", "ovisocr3"][..],
                "model=ovisocr3 profiles=quality installed=
",
            ),
            (
                &["model", "info", "--model", "ovisocr3", "quality"][..],
                "model=ovisocr3 profiles=quality installed=quality
",
            ),
        ] {
            let output = run_cli_with_cache(args, &cache);
            assert!(output.status.success(), "{args:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                expected,
                "{args:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn cli_model_recommend_selector_resolves_the_single_rung() {
        let cache =
            std::env::temp_dir().join(format!("ocrx-model-recommend-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let output = run_cli_with_cache(&["model", "recommend", "--model", "ovisocr3"], &cache);
        assert!(output.status.success());
        // The memory/device suffixes are host-dependent; the model/profile
        // prefix is the frozen shape (single-rung set resolves to quality).
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.starts_with("model=ovisocr3 profile=quality memory_gb="),
            "got: {stdout}"
        );
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    #[ignore = "env-gated: OCRX_E2E_MODEL=1 with the real ovisocr3 assets installed"]
    fn cli_model_verify_ovisocr3_success_path_with_real_assets() {
        // O3-T10 completion check: content + binding both pass once the
        // per-model binding mapping lands; the success line is frozen on
        // first light (plan O3-T09 item 4 handoff).
        if std::env::var_os("OCRX_E2E_MODEL").is_none() {
            eprintln!("skipped: set OCRX_E2E_MODEL=1");
            return;
        }
        let output = run_cli(&["model", "verify", "--model", "ovisocr3", "quality"]);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "model=ovisocr3 profile=quality revision=7be644a312005c3a55e2c826aee985a5c93d06b2 status=installed
"
        );
    }

    #[test]
    fn help_text_lists_the_model_selector_on_both_faces() {
        // SS83 item 4 / O3-T20: the registered help lines for scan and the
        // model subcommand family.
        let output = run_cli(&["help", "scan"]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("--model ovisocr2|ovisocr3"),
            "scan help: {stdout}"
        );
        let output = run_cli(&["help", "model"]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("ocrx model list [--model ovisocr2|ovisocr3]"),
            "model help: {stdout}"
        );
        assert!(
            stdout.contains("ocrx model verify [--model ovisocr2|ovisocr3] PROFILE"),
            "model help: {stdout}"
        );
    }

    #[test]
    fn scan_model_error_shares_the_frozen_error_spelling() {
        // D-6: scan and the model family share the same unsupported-model
        // error shape (no stdout, exit 2).
        let output = run_cli(&["scan", "input.pdf", "--model", "qwen3.8"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("unsupported model: qwen3.8"),
            "stderr: {stderr}"
        );
    }

    #[test]
    fn scan_parser_accepts_registered_models_and_rejects_unknown() {
        // O3-T16 item 2: the scan gate opens to the registered models.
        let parsed = parse_scan_args(&["input.pdf", "--model", "ovisocr3"]).unwrap();
        assert_eq!(parsed.model, "ovisocr3");
        let parsed = parse_scan_args(&["input.pdf", "--model", "ovisocr2"]).unwrap();
        assert_eq!(parsed.model, "ovisocr2");
        let parsed = parse_scan_args(&["input.pdf"]).unwrap();
        assert_eq!(parsed.model, DEFAULT_MODEL);
        let error = parse_scan_args(&["input.pdf", "--model", "qwen3.8"]).unwrap_err();
        assert_eq!(error, "unsupported model: qwen3.8");
    }

    #[test]
    fn scan_model_selector_errors_cleanly_without_assets() {
        // Always-on e2e shape: the accepted selector reaches the asset gate
        // and fails with the standard blocked spelling (no panic, no
        // partial output) when the cache has no ovisocr3 assets.
        let cache = std::env::temp_dir().join(format!("ocrx-scan-gate-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let output = run_cli_with_cache(
            &[
                "scan",
                "missing-input.pdf",
                "--model",
                "ovisocr3",
                "--offline",
            ],
            &cache,
        );
        assert_eq!(output.status.code(), Some(3));
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    #[ignore = "env-gated: OCRX_E2E_MODEL=1 with the real assets installed, the golden PDF, and PDFium"]
    fn scan_prompt_capture_asserts_the_frozen_pack_bytes_per_model() {
        // C7 (O3-T16): the assertion rides the real CLI subprocess path —
        // every encoded TaskRequest.prompt must byte-equal the selected
        // model's frozen pack (ovisocr3 explicit == ovisocr2 default ==
        // the historical bytes, per the spike). The v6 checkpoint
        // compatibility triplet must name the same pack.
        if std::env::var_os("OCRX_E2E_MODEL").is_none() {
            eprintln!("skipped: set OCRX_E2E_MODEL=1");
            return;
        }
        let source = golden_source_pdf();
        let Some(source) = source else {
            eprintln!("skipping: golden source.pdf unavailable");
            return;
        };
        for (selector, model) in [("", "ovisocr2"), ("--model ovisocr3", "ovisocr3")] {
            let work =
                std::env::temp_dir().join(format!("ocrx-capture-{model}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).unwrap();
            let capture = work.join("prompts.txt");
            let output_dir = work.join("out");
            let mut args: Vec<&str> = vec!["scan", source.to_str().unwrap()];
            args.extend(selector.split_whitespace());
            args.extend(["-o", output_dir.to_str().unwrap()]);
            let output = cli_binary()
                .args(&args)
                .env("OCRX_E2E_CAPTURE_OUT", &capture)
                .env("OCRX_PDFIUM_LIBRARY", pdfium_library_for_tests())
                .output()
                .expect("CLI binary should run");
            assert!(
                output.status.success(),
                "capture scan failed for {model}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let prompts = std::fs::read_to_string(&capture).unwrap();
            let prompts: Vec<&str> = prompts
                .split("---PROMPT---\n")
                .map(|prompt| prompt.trim_end_matches('\n'))
                .filter(|prompt| !prompt.is_empty())
                .collect();
            assert!(!prompts.is_empty(), "no prompts captured for {model}");
            let pack =
                ocrx_scan::ocr::prompt::pack_for(&ocrx_inference_api::ModelId::new(model).unwrap());
            for prompt in &prompts {
                assert_eq!(
                    *prompt, pack.prompt,
                    "captured prompt drifted from the {model} frozen pack"
                );
            }
            // N1: the result.json model object for the non-default face
            // derives from the actual identity (real-asset evidence; read
            // BEFORE the zero-load resume run rewrites the file with the
            // honest model:null zero-load shape).
            let result_json = std::fs::read_to_string(output_dir.join("result.json"))
                .expect("result.json written");
            assert!(ocrx_core::is_valid_json(&result_json));
            if model == "ovisocr3" {
                assert!(
                    result_json.contains("\"id\":\"ovisocr3\""),
                    "result: {result_json}"
                );
                assert!(result_json.contains("\"name\":\"OvisOCR3\""));
                assert!(result_json.contains("\"adapter\":\"ovisocr3\""));
                assert!(result_json.contains("\"runtime\":\"llama.cpp@0.1.161\""));
                assert!(
                    result_json
                        .contains("\"revision\":\"7be644a312005c3a55e2c826aee985a5c93d06b2\"")
                );
            }

            // C14 zero-load resume reuse: a second --resume run over the
            // intact checkpoints must pass the pre-compare with the SAME
            // model and never prepare/load (the capture backend only exists
            // once the runtime factory is asked for a device — an empty
            // capture file proves the zero-load path end to end).
            let capture2 = work.join("prompts-resume.txt");
            let mut resume_args: Vec<&str> = vec!["scan", source.to_str().unwrap()];
            resume_args.extend(selector.split_whitespace());
            resume_args.extend(["-o", output_dir.to_str().unwrap(), "--resume"]);
            let output = cli_binary()
                .args(&resume_args)
                .env("OCRX_E2E_CAPTURE_OUT", &capture2)
                .env("OCRX_PDFIUM_LIBRARY", pdfium_library_for_tests())
                .output()
                .expect("CLI binary should run");
            assert!(
                output.status.success(),
                "resume scan failed for {model}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                !capture2.exists(),
                "resume reuse for {model} must be zero-load (no session factory use)"
            );
            // The zero-load resume rewrote result.json with the honest
            // no-session shape (identity absent -> model null).
            let resumed_json =
                std::fs::read_to_string(output_dir.join("result.json")).expect("resume result");
            assert!(resumed_json.contains("\"ocr_model_loaded\":false"));
            assert!(resumed_json.contains("\"model\":null"));

            // D-8/X1 multi-document shared session: two inputs ride one
            // invocation (one prepare/load), each document produces its own
            // output tree, and every prompt still byte-equals the pack.
            // (Identical paths dedup, so the second input is a real copy.)
            let source_copy = work.join("source-copy.pdf");
            std::fs::copy(&source, &source_copy).expect("golden copy for the multi-doc leg");
            let multi_out = work.join("multi");
            let capture3 = work.join("prompts-multi.txt");
            let source_spelling = source.to_str().unwrap();
            let copy_spelling = source_copy.to_str().unwrap();
            let output = cli_binary()
                .args([
                    "scan",
                    source_spelling,
                    copy_spelling,
                    "-o",
                    multi_out.to_str().unwrap(),
                ])
                .args(selector.split_whitespace())
                .env("OCRX_E2E_CAPTURE_OUT", &capture3)
                .env("OCRX_PDFIUM_LIBRARY", pdfium_library_for_tests())
                .output()
                .expect("CLI binary should run");
            assert!(
                output.status.success(),
                "multi-doc scan failed for {model}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let doc_dirs = std::fs::read_dir(&multi_out)
                .expect("multi output root")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .count();
            assert_eq!(doc_dirs, 2, "one output directory per document");
            let multi_prompts = std::fs::read_to_string(&capture3).unwrap();
            let multi_prompts: Vec<&str> = multi_prompts
                .split("---PROMPT---\n")
                .map(|prompt| prompt.trim_end_matches('\n'))
                .filter(|prompt| !prompt.is_empty())
                .collect();
            assert!(!multi_prompts.is_empty());
            for prompt in &multi_prompts {
                assert_eq!(*prompt, pack.prompt);
            }

            // Checkpoint triplet agreement (C7 tail assertion).
            let checkpoint = output_dir.join("page-0001.checkpoint");
            let file =
                ocrx_output::read_any(&checkpoint).expect("first page checkpoint must exist");
            if let ocrx_output::CheckpointFile::V6(checkpoint) = file {
                let ocr = checkpoint.compatibility.ocr.as_ref().expect("ocr route");
                assert_eq!(ocr.prompt_id, pack.id);
                assert_eq!(ocr.prompt_version, pack.version);
                assert_eq!(ocr.prompt_schema_version, pack.schema_version);
                assert_eq!(ocr.inference.model.model.as_str(), model);
            } else {
                panic!("expected a v6 checkpoint");
            }
            let _ = std::fs::remove_dir_all(&work);
        }
    }

    #[test]
    fn cli_model_verify_error_path_for_ovisocr3_missing_assets() {
        // Phase-1 shape (plan O3-T09 item 4): the ovisocr3 verify face
        // exists and errors on the error path; its success path lights up
        // with O3-T10 (per-model binding).
        let cache = std::env::temp_dir().join(format!("ocrx-model-verify-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let output = run_cli_with_cache(
            &["model", "verify", "--model", "ovisocr3", "quality"],
            &cache,
        );
        assert_eq!(output.status.code(), Some(4));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("model verification failed"),
            "stderr: {stderr}"
        );
        let _ = std::fs::remove_dir_all(&cache);
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
    fn implicit_default_discrimination_matrix_5_6_7_8_gib() {
        // D-50 v1.2: the CLI-entry (model, profile) resolution. 5 GiB is
        // the frozen floor error (process exit 6 — the same face as the
        // profile-recommendation failure); 6/7 fall back to
        // (ovisocr2, balanced); >=8 keeps (ovisocr3, quality).
        let base = || ScanOptions {
            inputs: vec!["input.pdf".to_owned()],
            output_dir: None,
            pages: None,
            model: DEFAULT_MODEL.to_owned(),
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
            offline: true,
            non_interactive: true,
            keep_pages: false,
            password: None,
            column_split_retry: true,
            table_reread: false,
            structure_reread: StructureRereadMode::DefaultBestEffort,
            layout_model: None,
            layout_regions: None,
            model_explicit: false,
        };
        // 5 GiB: the floor error path exits the process — verify through
        // the same helper the fn uses (the exit face is frozen).
        let memory = 5_u64;
        match super::model_recommend::recommend_default_selection(memory) {
            Err(_) => {}
            other => panic!("5 GiB must error, got {other:?}"),
        }
        // 6/7 GiB: the full options rewrite lands on ovisocr2 balanced.
        // (The scan-path fn composes a real DeviceManager; the memory
        // comes from the machine — 63 GiB dev box keeps >=8. The MATRIX
        // itself is pinned at the recommend_default_selection layer, and
        // the explicit-freeze and identity threading below cover the
        // scan-path semantics.)
        let mut low = base();
        low.model = "ovisocr3".to_owned();
        low.profile = "auto".to_owned();
        // An explicit --model never resolves a cross-model pair; an
        // explicit profile freezes too (the resolver returns None).
        assert!(super::resolve_implicit_default_pair(true, "auto", 16).is_none());
        assert!(super::resolve_implicit_default_pair(false, "quality", 16).is_none());
        assert!(super::resolve_implicit_default_pair(false, "auto", 16).is_some());
    }

    #[test]
    fn image_token_budget_default_is_per_model() {
        // O3-T18/D-5 revision: the default scan-level image budget is
        // 2048 for ovisocr3 (projector needs the larger grid on dense
        // pages) and 1024 for ovisocr2 (unchanged).
        assert_eq!(super::default_max_image_tokens("ovisocr2"), 1024);
        assert_eq!(super::default_max_image_tokens("ovisocr3"), 2048);
        // The context-fit gate applies the resolved default: ovisocr3
        // (2048 + 16384 + 1024 = 19456) fits the 20480 default context but
        // not a deliberately smaller one.
        let ok = parse_scan_args(&[
            "input.pdf",
            "--model",
            "ovisocr3",
            "--context-size",
            "19456",
        ]);
        assert!(ok.is_ok(), "ovisocr3 resolved budget must fit 19456");
        // BLOCK-2: the parse-level gate is gone — the overflow face is
        // post-resolution (validate_context_gate; covered end-to-end by
        // d50_fallback_context_size_passes_after_resolution and the
        // explicit-model orchestration path).
        // BLOCK-2: the context gate is POST-resolution now — the parser
        // accepts the args and the gate runs against the RESOLVED model's
        // budget (ovisocr3 default face: 2048+16384+1024=19456; ovisocr2
        // fallback face: 1024+16384+1024=18432 — see
        // d50_fallback_context_size_passes_after_resolution).
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
        // Windows needs drive-qualified paths: "/work" and "/tmp" are not
        // absolute there, which would change what the policy validates.
        let (input, cache, outside) = if cfg!(target_os = "windows") {
            (
                Path::new("C:\\work\\input.pdf"),
                Path::new("C:\\work\\cache"),
                Path::new("C:\\tmp\\result"),
            )
        } else {
            (
                Path::new("/work/input.pdf"),
                Path::new("/work/cache"),
                Path::new("/tmp/result"),
            )
        };
        assert!(super::validate_output_policy(input, Path::new("relative"), &[cache]).is_err());
        assert!(
            super::validate_output_policy(input, input.join("out").as_path(), &[cache]).is_err()
        );
        assert!(
            super::validate_output_policy(input, cache.join("job").as_path(), &[cache]).is_err()
        );
        assert!(super::validate_output_policy(input, outside, &[cache]).is_ok());
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
            model_identity: None,
            degenerated_pages: 0,
            think_blocks_stripped_total: 0,
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
            reread_json: String::new(),
        });

        assert!(json.contains("\"status\":\"partial\""));
        assert!(json.contains("\"truncated\":1"));
        assert!(json.contains("\"think_blocks_stripped_total\":0"));
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
            model_identity: None,
            degenerated_pages: 0,
            think_blocks_stripped_total: 0,
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
            reread_json: String::new(),
        });
        assert!(json.contains("\"routing_metadata\":[{\"page\":3"));
        assert!(json.contains("native_validation_failed"));
    }

    #[test]
    fn scan_result_json_preserves_page_diagnostic_warnings_and_audit() {
        let json = super::serialize_scan_result(&ScanResultFields {
            model_identity: None,
            degenerated_pages: 0,
            think_blocks_stripped_total: 0,
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
            reread_json: String::new(),
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
    fn budget_contract_mirrors_adapter_cap_and_default_combo_fits_context() {
        // D5.1: over-cap budgets fail the preflight validation once, never as
        // per-page adapter errors.
        assert!(
            super::validate_numeric_options(Some(300), Some(1024), Some(20_000), Some(24_576))
                .is_err()
        );
        assert!(
            super::validate_numeric_options(
                Some(300),
                Some(1024),
                Some(ocrx_scan::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS),
                Some(20_480)
            )
            .is_ok()
        );
        // D5.2: the default combo (image 1024 + output 16384 + prompt 1024 =
        // 18432) must pass the capacity check against the 20480 context.
        assert!(super::validate_numeric_options(None, None, None, None).is_ok());
        // n_ctx_train=262144 for the frozen model (D5.0): no fallback needed,
        // defaults stay at the §3.7.1/§3.7.2 values.
        assert_eq!(ocrx_scan::ocr::prompt::DEFAULT_MAX_OUTPUT_TOKENS, 16_384);
    }

    #[test]
    fn doctor_json_report_has_stable_golden_shape() {
        let empty = std::env::temp_dir().join(format!("ocrx-doctor-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        let report = super::doctor_json_report_with_paths(
            ocrx_inference_api::ResourceSnapshot {
                memory_gb: 16,
                metal_available: true,
                cuda_available: false,
                vulkan_available: false,
                cpu_available: true,
            },
            empty.clone(),
            Some(&empty),
            DEFAULT_MODEL,
            None,
        );
        let _ = std::fs::remove_dir_all(&empty);
        // The platform field follows the running OS (macos/windows; other
        // targets report "unsupported"); keep the rest of the golden fixed so
        // the JSON shape stays stable (m28). C3 adds the workspace `version`
        // face for the DSH plugin (env!, so it moves with the workspace).
        let platform = super::current_platform();
        let escaped_cache_root = super::json_escape_path(&empty);
        assert_eq!(
            report,
            format!(
                "{{\"schema_version\":2,\"version\":\"{}\",\"platform\":\"{platform}\",\"model\":\"ovisocr3\",\"backend\":\"Metal\",\"image_token_limit\":2048,\"memory_gb\":16,\"metal_available\":true,\"cpu_available\":true,\"cache_root\":\"{escaped_cache_root}\",\"installed_profiles\":[],\"recommended_profile\":\"quality\",\"recommended_device\":\"metal\",\"skill_installed\":false,\"profiles\":[\"quality\"]}}",
                env!("CARGO_PKG_VERSION")
            )
        );
        // Golden text alone cannot catch an unescaped backslash (the fixture
        // dir would bake the malformed bytes into the baseline), so also
        // assert the report is a structurally valid single JSON object.
        assert!(ocrx_core::is_valid_json(&report));
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
            "--password",
            "open-sesame",
        ])
        .unwrap();
        assert_eq!(parsed.max_image_tokens, Some(1024));
        assert_eq!(parsed.max_output_tokens, Some(4096));
        assert_eq!(parsed.context_size, Some(8192));
        assert_eq!(parsed.progress, "jsonl");
        assert!(parsed.offline && parsed.non_interactive && parsed.keep_pages);
        assert_eq!(parsed.password.as_deref(), Some("open-sesame"));
    }

    #[test]
    fn scan_parser_accepts_log_level() {
        let parsed = parse_scan_args(&["input.pdf", "--log-level", "error"]).unwrap();
        assert_eq!(parsed.log_level, "error");
        assert!(parse_scan_args(&["input.pdf", "--log-level", "verbose"]).is_err());
    }

    #[test]
    fn doctor_selector_faces_are_frozen_on_creation() {
        // D-11/O3-T21 + D-48: the no-flag default face IS ovisocr3 now;
        // `--model ovisocr2` is its own readiness face (four-rung ladder,
        // own subtree install overview) — explicit selections are frozen
        // and never cross models (D-50).
        let empty = std::env::temp_dir().join(format!("ocrx-doctor-face-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        let snapshot = ocrx_inference_api::ResourceSnapshot {
            memory_gb: 16,
            metal_available: false,
            cuda_available: false,
            vulkan_available: false,
            cpu_available: true,
        };
        let default_face = super::doctor_json_report_with_paths(
            snapshot,
            empty.clone(),
            Some(&empty),
            DEFAULT_MODEL,
            None,
        );
        let explicit_ovisocr3 = super::doctor_json_report_with_paths(
            snapshot,
            empty.clone(),
            Some(&empty),
            "ovisocr3",
            None,
        );
        assert_eq!(default_face, explicit_ovisocr3);
        // The ovisocr2 explicit face stays its own frozen shape (it is the
        // D-50 fallback model, not the default).
        let explicit_ovisocr2 = super::doctor_json_report_with_paths(
            snapshot,
            empty.clone(),
            Some(&empty),
            "ovisocr2",
            None,
        );
        assert_ne!(default_face, explicit_ovisocr2);
        let ovisocr3 = super::doctor_json_report_with_paths(
            snapshot,
            empty.clone(),
            Some(&empty),
            "ovisocr3",
            None,
        );
        let platform = super::current_platform();
        let escaped_cache_root = super::json_escape_path(&empty);
        assert_eq!(
            ovisocr3,
            format!(
                "{{\"schema_version\":2,\"version\":\"{}\",\"platform\":\"{platform}\",\"model\":\"ovisocr3\",\"backend\":\"Cpu\",\"image_token_limit\":2048,\"memory_gb\":16,\"metal_available\":false,\"cpu_available\":true,\"cache_root\":\"{escaped_cache_root}\",\"installed_profiles\":[],\"recommended_profile\":\"quality\",\"recommended_device\":\"cpu\",\"skill_installed\":false,\"profiles\":[\"quality\"]}}",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert!(ocrx_core::is_valid_json(&ovisocr3));
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn cli_doctor_selector_face_reports_the_selected_model() {
        let output = run_cli(&["doctor", "--model", "ovisocr3", "--report", "json"]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1);
        assert!(stdout.contains("\"schema_version\":2"));
        assert!(stdout.contains("\"model\":\"ovisocr3\""));
        assert!(stdout.contains("\"profiles\":[\"quality\"]"));
        assert!(ocrx_core::is_valid_json(&stdout));
        // BLOCK-1 (review 2026-11-19): the no-flag doctor face must agree
        // with the scan's D-50 resolution — pinned per memory tier via
        // OCRX_MEMORY_GB (the runner's own RAM must not leak into the
        // assertion): 8 GiB -> ovisocr3/quality, 6 GiB -> ovisocr2/balanced
        // with the fallback token face.
        let output = run_cli_with_env(&["doctor", "--report", "json"], &[("OCRX_MEMORY_GB", "8")]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("\"model\":\"ovisocr3\"")
                && stdout.contains("\"recommended_profile\":\"quality\""),
            "stdout: {stdout}"
        );
        let output = run_cli_with_env(&["doctor", "--report", "json"], &[("OCRX_MEMORY_GB", "6")]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("\"model\":\"ovisocr2\"")
                && stdout.contains("\"recommended_profile\":\"balanced\""),
            "stdout: {stdout}"
        );
        // Unknown selector: frozen error spelling, exit 2.
        let output = run_cli(&["doctor", "--model", "qwen3.8"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("unsupported model: qwen3.8")
        );
    }

    #[test]
    fn doctor_parser_validates_the_model_selector() {
        match parse_command(&["doctor", "--model", "ovisocr3"]).unwrap() {
            CliCommand::Doctor { json, model, .. } => {
                assert!(!json);
                assert_eq!(model.as_deref(), Some("ovisocr3"));
            }
            other => panic!("{other:?}"),
        }
        match parse_command(&["doctor", "--report", "json", "--model", "ovisocr2"]).unwrap() {
            CliCommand::Doctor { json, model, .. } => {
                assert!(json);
                assert_eq!(model.as_deref(), Some("ovisocr2"));
            }
            other => panic!("{other:?}"),
        }
        let error = parse_command(&["doctor", "--model", "qwen3.8"]).unwrap_err();
        assert_eq!(error, "unsupported model: qwen3.8");
        let error = parse_command(&["doctor", "--report", "text"]).unwrap_err();
        assert!(error.contains("doctor"), "error: {error}");
    }

    #[test]
    fn scan_result_model_object_derives_from_the_identity_for_ovisocr3() {
        // N1: the non-default face derives every field from the actual
        // identity; the default face keeps the frozen historical string.
        let mut identity = test_support_identity();
        identity.model = ocrx_inference_api::ModelIdentity::new(
            ocrx_inference_api::ModelId::new("ovisocr3").unwrap(),
            ocrx_inference_api::ProfileId::new("quality").unwrap(),
            "7be644a312005c3a55e2c826aee985a5c93d06b2",
            vec![
                ocrx_inference_api::AssetIdentity {
                    role: ocrx_inference_api::AssetRole::new("model").unwrap(),
                    sha256: "11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6"
                        .to_owned(),
                    byte_len: 811_843_712,
                },
                ocrx_inference_api::AssetIdentity {
                    role: ocrx_inference_api::AssetRole::new("mmproj").unwrap(),
                    sha256: "698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd"
                        .to_owned(),
                    byte_len: 204_987_136,
                },
            ],
        )
        .unwrap();
        identity.adapter = ocrx_inference_api::AdapterIdentity {
            adapter: ocrx_inference_api::AdapterId::new("ovisocr3").unwrap(),
            version: "0.1.0".to_owned(),
        };
        identity.runtime = ocrx_inference::registry::runtime_identity();
        identity.display = ocrx_inference_api::ModelDisplayMeta {
            display_name: "OvisOCR3".to_owned(),
            family: "Ovis".to_owned(),
            quantization: "Q8_0".to_owned(),
        };
        let fields = ScanResultFields {
            model_identity: Some(&identity),
            reread_json: super::reread_result_identity(None, "", 0, 0),
            status: "success",
            input: Path::new("in.pdf"),
            markdown_path: None,
            output_dir: Path::new("out"),
            profile: "quality",
            required_quality_tier: "l2",
            document_type: "ocr",
            ocr_model_loaded: true,
            backend: "cpu",
            total_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            truncated_pages: 0,
            degenerated_pages: 0,
            think_blocks_stripped_total: 0,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            pdf_routing: Vec::new(),
            routing_counts: RoutingCounts {
                native_text: 0,
                ocr: 0,
                empty: 0,
                fallback: 0,
            },
            image_dir: Path::new("out/images"),
            image_count: 0,
            elapsed_ms: 5,
        };
        let json = super::serialize_scan_result(&fields);
        assert!(json.contains("\"id\":\"ovisocr3\""), "json: {json}");
        assert!(json.contains("\"name\":\"OvisOCR3\""));
        assert!(json.contains("\"profile\":\"quality\""));
        assert!(json.contains("\"revision\":\"7be644a312005c3a55e2c826aee985a5c93d06b2\""));
        assert!(json.contains(
            "\"mmproj_revision\":\"698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd\""
        ));
        assert!(json.contains("\"adapter\":\"ovisocr3\""));
        assert!(json.contains("\"runtime\":\"llama.cpp@0.1.161\""));
        assert!(json.contains("\"quantization\":\"Q8_0\""));
        assert!(ocrx_core::is_valid_json(&json));
    }

    #[test]
    fn scan_result_default_face_keeps_the_frozen_ovisocr2_string() {
        // SS83 item 5: even with an identity present, the default-model
        // shape keeps the historical bytes (including llama.cpp@0.1.152).
        let identity = test_support_identity();
        let fields = ScanResultFields {
            model_identity: Some(&identity),
            reread_json: super::reread_result_identity(None, "", 0, 0),
            status: "success",
            input: Path::new("in.pdf"),
            markdown_path: None,
            output_dir: Path::new("out"),
            profile: "compact",
            required_quality_tier: "l2",
            document_type: "ocr",
            ocr_model_loaded: true,
            backend: "cpu",
            total_pages: 1,
            completed_pages: 1,
            failed_pages: 0,
            truncated_pages: 0,
            degenerated_pages: 0,
            think_blocks_stripped_total: 0,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            pdf_routing: Vec::new(),
            routing_counts: RoutingCounts {
                native_text: 0,
                ocr: 0,
                empty: 0,
                fallback: 0,
            },
            image_dir: Path::new("out/images"),
            image_count: 0,
            elapsed_ms: 5,
        };
        let json = super::serialize_scan_result(&fields);
        assert!(json.contains("\"id\":\"ovisocr2\""));
        assert!(json.contains("\"runtime\":\"llama.cpp@0.1.152\""));
    }

    #[test]
    fn doctor_json_report_redacts_sensitive_cache_paths() {
        let report = doctor_json_report(
            ocrx_inference_api::ResourceSnapshot {
                memory_gb: 16,
                metal_available: true,
                cuda_available: false,
                vulkan_available: false,
                cpu_available: true,
            },
            DEFAULT_MODEL,
            None,
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
            Ok(CliCommand::Doctor {
                json: false,
                model: None
            })
        );
        assert_eq!(
            parse_command(&["doctor", "--report", "json"]),
            Ok(CliCommand::Doctor {
                json: true,
                model: None
            })
        );
        assert_eq!(
            parse_command(&["scan", "input.pdf"]),
            Ok(CliCommand::Scan(ScanOptions {
                inputs: vec!["input.pdf".to_owned()],
                output_dir: None,
                pages: None,
                model: "ovisocr3".to_owned(),
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
                password: None,
                column_split_retry: true,
                model_explicit: false,
                table_reread: false,
                structure_reread: StructureRereadMode::DefaultBestEffort,
                layout_model: None,
                layout_regions: None,
            }))
        );
        assert_eq!(
            parse_command(&["model", "info"]),
            Ok(CliCommand::Model {
                action: "info".to_owned(),
                model: None,
                profile: None,
                offline: false,
                non_interactive: false,
                report_json: false,
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

    #[test]
    fn injected_failure_page_parser_accepts_positive_and_rejects_invalid() {
        assert_eq!(parse_injected_failure_page(Some("3")), Some(3));
        assert_eq!(parse_injected_failure_page(Some("0")), None);
        assert_eq!(parse_injected_failure_page(Some("abc")), None);
        assert_eq!(parse_injected_failure_page(None), None);
    }

    #[test]
    fn injected_failure_errors_only_for_the_matching_page() {
        assert!(injected_failure(Some("2"), 1, "render").is_ok());
        assert!(injected_failure(Some("2"), 2, "render").is_err());
        assert!(injected_failure(None, 2, "render").is_ok());
        assert!(injected_failure(Some("0"), 0, "render").is_ok());
    }

    #[test]
    fn output_policy_rejects_case_variant_of_protected_path_on_windows() {
        // M3 regression guard: NTFS matches paths case-insensitively, so a
        // case-variant spelling of a protected root must be rejected.
        if !cfg!(target_os = "windows") {
            return;
        }
        let input = Path::new("C:\\work\\input.pdf");
        let cache = Path::new("C:\\Users\\u\\AppData\\Local");
        assert!(
            super::validate_output_policy(
                input,
                Path::new("c:\\users\\U\\appdata\\local"),
                &[cache]
            )
            .is_err()
        );
    }

    #[test]
    fn scan_output_policy_allows_subdirectory_but_not_ancestor_of_working_directory() {
        // Regression guard: the working directory used to sit in the symmetric
        // protected list, which rejected every output below it and made scans
        // run from the PDF's own directory (`--output-dir out`, the default
        // `<input dir>/<stem>-ocr`) fail with exit 9.
        let (input, cache, cwd) = if cfg!(target_os = "windows") {
            (
                Path::new("C:\\work\\input.pdf"),
                Path::new("C:\\work\\cache\\ocrx\\models"),
                Path::new("C:\\work\\project"),
            )
        } else {
            (
                Path::new("/work/input.pdf"),
                Path::new("/work/cache/ocrx/models"),
                Path::new("/work/project"),
            )
        };
        assert!(
            super::validate_scan_output_policy(input, cwd.join("out").as_path(), &[cache], cwd)
                .is_ok(),
            "a subdirectory of the working directory is the normal layout"
        );
        // The working directory itself and every ancestor of it stay refused:
        // --overwrite deletes the whole output tree.
        assert!(super::validate_scan_output_policy(input, cwd, &[cache], cwd).is_err());
        let ancestor = if cfg!(target_os = "windows") {
            Path::new("C:\\work")
        } else {
            Path::new("/work")
        };
        assert!(super::validate_scan_output_policy(input, ancestor, &[cache], cwd).is_err());
    }

    #[test]
    fn document_exit_code_maps_page_failures_to_partial_blk7() {
        let base = || ocrx_scan::DocumentOutcome {
            input: PathBuf::from("in.pdf"),
            output_dir: PathBuf::from("out"),
            summary: ocrx_scan::state::JobSummary {
                structure_reread_json: String::new(),
                selected_pages: 1,
                completed_pages: 1,
                failed_pages: 0,
                truncated_pages: 0,
                degenerated_pages: 0,
                think_blocks_stripped: 0,
                tables_reread_total: 0,
                replaced_total: 0,
                warnings: Vec::new(),
                diagnostics: Vec::new(),
                cancelled: false,
            },
            pages: vec![1],
            backend: None,
            identity: None,
            telemetry_delta: None,
            step_profile_delta: None,
            markdown_path: None,
            document_type: "text".to_owned(),
            routing_counts: ocrx_scan::app::RoutingCounts::default(),
            pdf_routing: Vec::new(),
            images_count: 0,
            elapsed_ms: 0,
            failure: None,
        };
        // Clean document → 0.
        assert_eq!(document_exit_code(&base(), false), 0);
        // Page-level failure → PARTIAL 10 (real_scan_fault_injection contract).
        let mut failed = base();
        failed.summary.failed_pages = 1;
        assert_eq!(document_exit_code(&failed, false), 10);
        // Truncated → 10.
        let mut truncated = base();
        truncated.summary.truncated_pages = 1;
        assert_eq!(document_exit_code(&truncated, false), 10);
        // Document-level failure → its structured code (3/4/5/9).
        let mut doc_failed = base();
        doc_failed.failure = Some(ocrx_scan::DocumentFailure {
            code: 3,
            kind: "input".to_owned(),
            message: "unreadable".to_owned(),
            cause: None,
        });
        assert_eq!(document_exit_code(&doc_failed, false), 3);
        // Cancel → 130.
        assert_eq!(document_exit_code(&base(), true), 130);
        // RBLK-3: a document failure racing a global cancel still reports the
        // cancel — 130 dominates the failure code in both orderings.
        assert_eq!(document_exit_code(&doc_failed, true), 130);
        let mut doc_cancelled = base();
        doc_cancelled.summary.cancelled = true;
        doc_cancelled.failure = Some(ocrx_scan::DocumentFailure {
            code: 9,
            kind: "runtime".to_owned(),
            message: "runtime died as cancel landed".to_owned(),
            cause: None,
        });
        assert_eq!(document_exit_code(&doc_cancelled, false), 130);
        // Final aggregation follows the frozen ladder.
        assert_eq!(aggregate_scan_exit(&[0, 0]), 0);
        assert_eq!(aggregate_scan_exit(&[0, 10]), 10);
        assert_eq!(aggregate_scan_exit(&[10, 3]), 3);
        assert_eq!(aggregate_scan_exit(&[3, 130]), 130);
    }

    #[test]
    fn final_scan_exit_gives_cancellation_absolute_priority_rblk3() {
        // Zero documents started (cancel before the first document) must not
        // fold into 0 — single and multi input alike.
        assert_eq!(final_scan_exit(true, false, &[]), 130);
        assert_eq!(final_scan_exit(true, true, &[]), 130);
        // Failure + cancel → 130 (cancellation dominates document failures).
        assert_eq!(final_scan_exit(true, false, &[3]), 130);
        assert_eq!(final_scan_exit(true, true, &[3, 10]), 130);
        // No cancel: multi keeps the X1.4 ladder; single keeps its own code.
        assert_eq!(final_scan_exit(false, true, &[0, 10]), 10);
        assert_eq!(final_scan_exit(false, true, &[0, 3]), 3);
        assert_eq!(final_scan_exit(false, false, &[2]), 2);
        assert_eq!(final_scan_exit(false, false, &[10]), 10);
        assert_eq!(final_scan_exit(false, false, &[]), 0);
    }

    #[test]
    fn cli_out_of_range_pages_is_usage_error_blk4() {
        // BLK-4: an out-of-range --pages selection is a usage error (exit 2),
        // never a silent empty scan (the app validates the RAW selection
        // before filtering; page_count runs first per §6.3 step 1).
        let pdfium_candidate =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/bin/pdfium.dll");
        if !pdfium_candidate.is_file() {
            eprintln!("skipping: no vendored pdfium available");
            return;
        }
        // Canonical spelling: the upstream hardened output policy refuses a
        // raw `..` before lexical normalization, so the historical
        // manifest-relative spelling would be refused before the pages
        // validation this test targets.
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../benchmarks/fixtures/pdf/normal-1-page.pdf")
            .canonicalize()
            .expect("fixture resolves");
        let mut command = match std::env::var("CARGO_BIN_EXE_ocrx") {
            Ok(binary) => std::process::Command::new(binary),
            Err(_) => {
                let mut command = std::process::Command::new("cargo");
                command
                    .args(["run", "--quiet", "--bin", "ocrx", "--"])
                    .current_dir(env!("CARGO_MANIFEST_DIR"));
                command
            }
        };
        let output = command
            .args([
                "scan",
                fixture.to_str().expect("fixture path is utf-8"),
                "--pages",
                "2",
                "--report",
                "json",
            ])
            .env("OCRX_PDFIUM_LIBRARY", &pdfium_candidate)
            .output()
            .expect("CLI subprocess should run");
        assert_eq!(output.status.code(), Some(2));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("\"schema_version\":1"));
        assert!(stdout.contains("\"code\":2"));
        assert!(stdout.contains("page selection is out of range"));
    }
}
