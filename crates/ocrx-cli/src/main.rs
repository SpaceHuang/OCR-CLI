use image::ImageFormat;
use ocrx_core::ModelId;
use ocrx_models::{
    ModelCatalog, ResourceSnapshot, UrlInstallRequest, community_asset_sizes, community_asset_urls,
    community_manifest, recommend_device, recommend_profile,
};
use ocrx_output::{
    CancellationToken, CompatibilityKey, ErrorReport, ModelLoader, PageArtifactRecognizer,
    PageArtifactResult, PageDiagnostic, PageRenderer, ProgressEvent, ProgressMode, RenderAudit,
    RenderedPage, ReportFormat, Reporter, ResultReport, ScanService, format_report,
    remove_for_overwrite,
};
use ocrx_pdf::{
    PdfErrorKind, PdfWorkerScheduler, PdfiumRenderer, RenderOptions, classify_pdf_error,
    fit_pixel_budget,
};
use ocrx_runtime::{Backend, LlamaMtmdRuntime, RuntimeInfo};
use std::io::{Cursor, IsTerminal};
use std::path::{Path, PathBuf};
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
        ["model", action]
            if matches!(
                *action,
                "list" | "info" | "recommend" | "install" | "remove" | "verify"
            ) =>
        {
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
            let (model_path, _mmproj_path) = ocrx_models::cache_paths(&cache_root, &profile)
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
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let content = include_str!("../../../assets/skills/ocrx/SKILL.md");
            let result = match action.as_str() {
                "install" => ocrx_skill::install(&target, &home, content).map(|paths| {
                    format!(
                        "skill=ocrx status=installed targets={} files={}",
                        target,
                        paths.len()
                    )
                }),
                "uninstall" => ocrx_skill::uninstall(&target, &home).map(|paths| {
                    format!(
                        "skill=ocrx status=uninstalled targets={} files={}",
                        target,
                        paths.len()
                    )
                }),
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

struct OvisRecognizer {
    output: PathBuf,
    max_tokens: u32,
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
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::from_raw(image.width, image.height, image.rgb8.clone())
            .ok_or_else(|| std::io::Error::other("invalid RGB8"))?
            .write_to(&mut png, ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        Ok(PageArtifactResult {
            png: png.into_inner(),
            markdown: artifact.markdown,
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
    let cache = std::env::var_os("OCRX_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".ocrx-model-cache"));
    let snapshot = resource_snapshot();
    let profile = match resolve_scan_profile(&options.profile, snapshot.memory_gb) {
        Ok(profile) => profile,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 4,
                    kind: "model".to_owned(),
                    message: "model profile resolution failed".to_owned(),
                    cause: Some(error),
                    suggestion: Some("choose an explicit installed profile".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(4);
        }
    };
    let device = match recommend_device(snapshot, &options.device) {
        Ok(device) => device,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 6,
                    kind: "runtime".to_owned(),
                    message: "requested device is unavailable".to_owned(),
                    cause: Some(format!("{error:?}")),
                    suggestion: Some("use --device auto|cpu|metal".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(6);
        }
    };
    let (model, mmproj) = match ocrx_models::cache_paths(&cache, &profile) {
        Ok(paths) => paths,
        Err(error) => {
            reporter.error(
                &ErrorReport {
                    code: 4,
                    kind: "model".to_owned(),
                    message: "model path resolution failed".to_owned(),
                    cause: Some(format!("{error:?}")),
                    suggestion: Some("verify the selected profile".to_owned()),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(4);
        }
    };
    let manifest = community_manifest(&profile).expect("validated profile");
    let (model_bytes, mmproj_bytes) = community_asset_sizes(&profile).expect("validated profile");
    let assets_valid = model.is_file()
        && mmproj.is_file()
        && ocrx_models::verify_manifest_files_with_sizes(
            &manifest,
            &cache,
            model_bytes,
            mmproj_bytes,
        )
        .and_then(|_| {
            ocrx_models::validate_runtime_adapter_compatibility("0.1.152", "0.1.153", "ovisocr2")
        })
        .is_ok();
    if !assets_valid && options.offline {
        reporter.error(
            &ErrorReport {
                code: 4,
                kind: "model".to_owned(),
                message: "offline scan requires verified local model assets".to_owned(),
                cause: None,
                suggestion: Some("install or repair the selected profile first".to_owned()),
            },
            report_format,
        );
        print_reporter(reporter);
        std::process::exit(4);
    }
    if !assets_valid && !options.offline {
        if std::io::stdin().is_terminal() && !options.non_interactive {
            eprintln!(
                "Model profile '{profile}' is missing or invalid; download {} MB of model assets? [y/N]",
                (model_bytes.saturating_add(mmproj_bytes)) / 1_000_000
            );
            let mut answer = String::new();
            let _ = std::io::stdin().read_line(&mut answer);
            if !answer.trim().eq_ignore_ascii_case("y") {
                reporter.error(
                    &ErrorReport {
                        code: 4,
                        kind: "model".to_owned(),
                        message: "model installation not confirmed".to_owned(),
                        cause: None,
                        suggestion: Some("rerun with confirmation or --non-interactive".to_owned()),
                    },
                    report_format,
                );
                print_reporter(reporter);
                std::process::exit(4);
            }
        }
        let (model_url, mmproj_url) = community_asset_urls(&profile).expect("validated profile");
        let request = UrlInstallRequest {
            manifest,
            model_url,
            mmproj_url,
            cache_root: cache.clone(),
            offline: false,
            non_interactive: options.non_interactive,
            license_accepted: true,
            available_bytes: None,
        };
        if let Err(error) = ocrx_models::install_from_urls(&request) {
            reporter.error(
                &ErrorReport {
                    code: 4,
                    kind: "model".to_owned(),
                    message: "model installation failed".to_owned(),
                    cause: Some(format!("{error:?}")),
                    suggestion: Some(
                        "install the selected profile or use --offline with a complete cache"
                            .to_owned(),
                    ),
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(4);
        }
        if ocrx_models::verify_manifest_files_with_sizes(
            &request.manifest,
            &cache,
            model_bytes,
            mmproj_bytes,
        )
        .is_err()
        {
            reporter.error(
                &ErrorReport {
                    code: 4,
                    kind: "model".to_owned(),
                    message: "installed model failed verification".to_owned(),
                    cause: None,
                    suggestion: None,
                },
                report_format,
            );
            print_reporter(reporter);
            std::process::exit(4);
        }
    }
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
    let page_count = match PdfiumRenderer::new(library.clone()).page_count(&input) {
        Ok(count) => count,
        Err(error) => {
            let error_kind = match classify_pdf_error(&error.to_string()) {
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
    reporter.progress(
        &ProgressEvent {
            event: "scan_started".to_owned(),
            page: None,
            completed: 0,
            total: pages.len() as u32,
        },
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
    let loader = RuntimeLoader {
        model,
        mmproj,
        gpu: device != "cpu",
        max_image_tokens: options.max_image_tokens.unwrap_or(1024),
        context_size: options.context_size.unwrap_or(16_384),
    };
    let recognizer = OvisRecognizer {
        output: output.clone(),
        max_tokens: options.max_output_tokens.unwrap_or(4096),
    };
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
        profile: profile.clone(),
        parameters: format!(
            "dpi={dpi};tokens={:?};max_image_tokens={};context_size={};device={device}",
            options.max_output_tokens,
            options.max_image_tokens.unwrap_or(1024),
            options.context_size.unwrap_or(16_384)
        ),
    };
    let cancellation = CancellationToken::new();
    install_sigint_handler(cancellation.clone());
    let mut service = ScanService::new(renderer, loader, recognizer);
    let summary = service.run_with_checkpoints_cancelled(
        &pages.iter().map(|p| *p as u32).collect::<Vec<_>>(),
        &output,
        &compatibility,
        options.resume,
        options.keep_pages,
        &cancellation,
    );
    match summary {
        Ok(summary) => {
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
            let report = ResultReport {
                job_id: "scan".to_owned(),
                status: if summary.failed_pages == 0 && summary.truncated_pages == 0 {
                    ocrx_output::ResultStatus::Success
                } else {
                    ocrx_output::ResultStatus::Partial
                },
                total_pages: summary.selected_pages,
                completed_pages: summary.completed_pages,
                failed_pages: summary.failed_pages,
                output_dir: output.display().to_string(),
            };
            let markdown_output = (summary.failed_pages == 0).then(|| output.join("document.md"));
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
                backend: &device,
                total_pages: summary.selected_pages,
                completed_pages: summary.completed_pages,
                failed_pages: summary.failed_pages,
                truncated_pages: summary.truncated_pages,
                warnings: summary.warnings.clone(),
                diagnostics: summary.diagnostics.clone(),
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
            }
            reporter.progress(
                &ProgressEvent {
                    event: "scan_finished".to_owned(),
                    page: None,
                    completed: summary.completed_pages,
                    total: summary.selected_pages,
                },
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
    backend: &'a str,
    total_pages: u32,
    completed_pages: u32,
    failed_pages: u32,
    truncated_pages: u32,
    warnings: Vec<String>,
    diagnostics: Vec<PageDiagnostic>,
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
    format!(
        "{{\"schema_version\":3,\"job_id\":\"scan\",\"status\":\"{}\",\"input\":\"{}\",\"markdown_path\":{},\"output_dir\":\"{}\",\"model\":{{\"id\":\"ovisocr2\",\"name\":\"OvisOCR2\",\"family\":\"Ovis\",\"profile\":\"{}\",\"revision\":\"{}\",\"mmproj_revision\":\"{}\",\"adapter\":\"ovisocr2\",\"adapter_version\":\"0.1.0\",\"prompt_schema_version\":1,\"runtime\":\"llama.cpp@0.1.152\",\"quantization\":\"{}\"}},\"backend\":\"{}\",\"pages\":{{\"total\":{},\"completed\":{},\"failed\":{},\"truncated\":{}}},\"images\":{{\"directory\":\"{}\",\"count\":{}}},\"elapsed_ms\":{},\"warnings\":{},\"diagnostics\":[{}]}}",
        fields.status,
        json_escape_path(fields.input),
        markdown,
        json_escape_path(fields.output_dir),
        fields.profile,
        ocrx_models::COMMUNITY_REVISION,
        ocrx_models::COMMUNITY_REVISION,
        quantization,
        fields.backend,
        fields.total_pages,
        fields.completed_pages,
        fields.failed_pages,
        fields.truncated_pages,
        json_escape_path(fields.image_dir),
        fields.image_count,
        fields.elapsed_ms,
        warnings,
        diagnostics
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
        Some("scan") => println!(
            "ocrx scan <input.pdf> [--output-dir DIR] [--pages SPEC] [--model ovisocr2] [--model-profile PROFILE] [--device DEVICE] [--report text|json] [--resume|--overwrite]"
        ),
        Some("doctor") => println!("ocrx doctor [--report json]"),
        Some("model") => {
            println!("ocrx model <list|info|recommend|install|remove|verify> [PROFILE]")
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
    } else {
        "unsupported"
    };
    let backend = match info.backend {
        Backend::Metal => "Metal",
        Backend::Cpu => "Cpu",
    };
    let installed_profiles = ["compact", "balanced", "quality", "reference"]
        .into_iter()
        .filter(|profile| {
            ocrx_models::cache_paths(&cache_root, profile)
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
        CliCommand, ConfigSource, ResolvedConfig, ResourceSnapshot, ScanOptions, ScanResultFields,
        doctor_json_report, parse_command, parse_page_selection, parse_scan_args, resolve_config,
    };
    use ocrx_output::{PageDiagnostic, RenderAudit, ResultReport, ResultStatus};

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
    fn scan_parser_rejects_resume_overwrite_and_missing_values() {
        assert!(parse_scan_args(&["input.pdf", "--resume", "--overwrite"]).is_err());
        assert!(parse_scan_args(&["input.pdf", "--pages"]).is_err());
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
        let binary = std::env::var("CARGO_BIN_EXE_ocrx").unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug/ocrx")
                .to_string_lossy()
                .into_owned()
        });
        Command::new(binary)
            .args(args)
            .output()
            .expect("CLI test binary should run")
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
    fn cli_initialization_error_is_one_json_error_object() {
        let output = run_cli(&["scan", "input.pdf", "--report", "json"]);
        assert_eq!(output.status.code(), Some(4));
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
            backend: "cpu",
            total_pages: 2,
            completed_pages: 2,
            failed_pages: 0,
            truncated_pages: 1,
            warnings: vec!["truncated".to_owned()],
            diagnostics: Vec::new(),
            image_dir: Path::new("/tmp/output/images"),
            image_count: 3,
            elapsed_ms: 42,
        });

        assert!(json.contains("\"status\":\"partial\""));
        assert!(json.contains("\"truncated\":1"));
        assert!(json.contains("\"warnings\":[\"truncated\"]"));
        assert!(json.contains("/tmp/input\\tname\\n.pdf"));
    }

    #[test]
    fn scan_result_json_preserves_page_diagnostic_warnings_and_audit() {
        let json = super::serialize_scan_result(&ScanResultFields {
            status: "partial",
            input: Path::new("/tmp/input.pdf"),
            markdown_path: None,
            output_dir: Path::new("/tmp/output"),
            profile: "balanced",
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
            image_dir: Path::new("/tmp/output/images"),
            image_count: 0,
            elapsed_ms: 1,
        });

        assert!(json.contains("\"warnings\":[\"truncated\",\"render_scaled\"]"));
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
            parse_command(&["model", "verify"]),
            Ok(CliCommand::Model {
                action: "verify".to_owned(),
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
