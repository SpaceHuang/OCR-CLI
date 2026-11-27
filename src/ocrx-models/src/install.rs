//! Revision-scoped install transactions (plan §5.6/§7.3/§8.5).
//!
//! One transaction covers every asset of a profile (private model + shared
//! projector): a single revision-level lock, residue sweep, `.part` staging,
//! backups, verify-before-commit, reverse-order rollback, and a deterministic
//! `install-receipt.json` written last. The receipt is an audit record, not
//! a trust source — `verify` always recomputes hashes from disk.
//!
//! C2 (dsh-plugin-v0.1 plan stage 2) adds the machine-readable run report
//! (per-asset download/verify statistics, D-5), the stage-event stream, the
//! download attempt cap (D-6), the stale-residue sweep over both scopes and
//! the best-effort signal purge (D-7).

use std::path::Path;

use ocrx_inference_api::{
    DownloadProposal, InferenceError, InstallPolicy, InstallReceipt, ModelManifest,
};
use ocrx_primitives::sha256_file;
use serde::Serialize;

use crate::cache::{asset_cache_path, manifest_asset_paths};
use crate::transport::Transport;
use crate::verify::verify_content;

/// Deterministic receipt file name inside the profile directory.
pub const RECEIPT_FILE_NAME: &str = "install-receipt.json";

/// Per-asset download attempt cap (plan D-6): curl keeps its own
/// `--retry 2`; this bounds the OUTER loop so a flaky mirror cannot spin.
pub const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;

/// Default download-progress ticker interval (plan D-5: ~500ms).
pub const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Progress/stage events emitted during one install transaction (plan D-5).
/// The CLI renders them as human-readable stderr lines; the machine-readable
/// statistics ride the final [`InstallRunReport`], not this stream.
#[derive(Debug, Clone)]
pub enum InstallEvent<'a> {
    LockAcquired {
        pid: u32,
    },
    DownloadStart {
        role: &'a str,
        url: &'a str,
        byte_len: u64,
        attempt: u32,
    },
    DownloadProgress {
        role: &'a str,
        bytes: u64,
        total_bytes: u64,
        rate_bps: f64,
    },
    Verify {
        role: &'a str,
        sha256_ok: bool,
        size_ok: bool,
    },
    Commit,
    Done,
}

impl InstallEvent<'_> {
    /// Stage name for the human-readable progress lines (plan D-5 stages:
    /// lock-acquired / download-start / download-progress / verify / commit /
    /// done).
    #[must_use]
    pub fn stage_name(&self) -> &'static str {
        match self {
            Self::LockAcquired { .. } => "lock-acquired",
            Self::DownloadStart { .. } => "download-start",
            Self::DownloadProgress { .. } => "download-progress",
            Self::Verify { .. } => "verify",
            Self::Commit => "commit",
            Self::Done => "done",
        }
    }
}

/// Per-asset download statistic (plan D-5 summary schema).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssetDownloadStat {
    pub role: String,
    pub bytes: u64,
    pub elapsed_ms: u128,
    pub avg_rate_bps: f64,
}

/// Per-asset verification statistic (plan D-5 summary schema).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssetVerifyStat {
    pub role: String,
    pub sha256_ok: bool,
    pub size_ok: bool,
}

/// Download statistics block of the run report (§9.2: actual transferred
/// bytes, elapsed/rate per asset; the idempotent path reports
/// `downloaded: false` with zeroed transfer stats).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct DownloadReport {
    pub downloaded: bool,
    pub per_asset: Vec<AssetDownloadStat>,
    pub total_bytes: u64,
    pub avg_rate_bps: f64,
}

/// Verification statistics block of the run report (every manifest asset is
/// verified exactly once per run, including pre-existing correct assets).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct VerifyReport {
    pub per_asset: Vec<AssetVerifyStat>,
}

/// Final install run report (plan D-5): the machine-readable summary that
/// carries the §9.2 progress statistics. The CLI serializes it as the last
/// stdout line of `model install --report json`.
#[derive(Debug, Clone, Serialize)]
pub struct InstallRunReport {
    pub model: String,
    pub profile: String,
    pub revision: String,
    /// Planned byte total (consent-copy comparison face).
    pub size_bytes: u64,
    pub elapsed_ms: u128,
    pub stage: &'static str,
    pub download: DownloadReport,
    pub verify: VerifyReport,
    /// The receipt (audit record) rides along for the CLI face.
    #[serde(skip)]
    pub receipt: InstallReceipt,
}

/// Observes install events (stderr progress lines on the CLI face). Called
/// from the download ticker thread, hence Send + Sync.
pub type InstallObserver<'a> = &'a (dyn Fn(&InstallEvent<'_>) + Send + Sync);

/// Inputs for one offline-testable install transaction.
pub struct InstallContext<'a> {
    pub manifest: &'a ModelManifest,
    pub cache_root: &'a Path,
    pub policy: InstallPolicy,
    pub transport: &'a dyn Transport,
    pub interaction: &'a mut dyn ocrx_inference_api::InstallInteraction,
    /// Free bytes on the cache volume (probe injectable; `None` skips the
    /// preflight, e.g. in tests).
    pub available_bytes: Option<u64>,
    /// Optional progress observer (C2; `None` keeps the historical silent
    /// behavior, e.g. in tests).
    pub observer: Option<InstallObserver<'a>>,
    /// Ticker interval for download-progress events (`None` = ~500ms).
    /// Tests shrink it.
    pub progress_interval: Option<std::time::Duration>,
    /// Register a best-effort signal handler that purges THIS transaction's
    /// `.part` files before exiting (plan D-7; the CLI enables it).
    pub handle_signals: bool,
}

/// Emits an event when an observer is attached.
fn emit(observer: Option<InstallObserver<'_>>, event: InstallEvent<'_>) {
    if let Some(observer) = observer {
        observer(&event);
    }
}

/// Removes exactly the given transaction files (D-7 signal-handler leg;
/// best-effort: failures are ignored, the process is exiting anyway).
pub(crate) fn purge_paths(paths: &[std::path::PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

/// Verifies one staged (or pre-existing) asset and returns its verify
/// statistic plus whether the content is fully correct.
fn verify_asset(
    manifest: &ModelManifest,
    role: &ocrx_inference_api::AssetRole,
    path: &Path,
) -> (AssetVerifyStat, bool) {
    let Some(descriptor) = manifest.asset(role) else {
        return (
            AssetVerifyStat {
                role: role.as_str().to_owned(),
                sha256_ok: false,
                size_ok: false,
            },
            false,
        );
    };
    let size_ok = std::fs::metadata(path)
        .map(|meta| meta.len() == descriptor.byte_len)
        .unwrap_or(false);
    let sha256_ok = sha256_file(path)
        .map(|hash| hash == descriptor.sha256.to_ascii_lowercase())
        .unwrap_or(false);
    (
        AssetVerifyStat {
            role: role.as_str().to_owned(),
            sha256_ok,
            size_ok,
        },
        sha256_ok && size_ok,
    )
}

/// Builds verify statistics for every manifest asset against disk state
/// (used on the idempotent path; the download path collects in-loop).
fn verify_report_for(
    cache_root: &Path,
    manifest: &ModelManifest,
    observer: Option<InstallObserver<'_>>,
) -> Result<VerifyReport, InferenceError> {
    let mut per_asset = Vec::new();
    for (role, path) in manifest_asset_paths(cache_root, manifest)? {
        let (stat, _) = verify_asset(manifest, &role, &path);
        emit(
            observer,
            InstallEvent::Verify {
                role: stat.role.as_str(),
                sha256_ok: stat.sha256_ok,
                size_ok: stat.size_ok,
            },
        );
        per_asset.push(stat);
    }
    Ok(VerifyReport { per_asset })
}

/// The historical receipt-only face (manager trait): report statistics are
/// dropped. New production callers use [`install_with_report`].
///
/// # Errors
///
/// Offline/license/disk/transport/hash categories; a failed transaction
/// never exposes half a valid asset set (reverse-order rollback restores
/// the previous bytes, `.part` files are cleaned).
pub fn install(context: InstallContext<'_>) -> Result<InstallReceipt, InferenceError> {
    install_with_report(context).map(|report| report.receipt)
}

/// Downloads/verifies/commits one profile's asset group and returns the full
/// run report (plan D-5/D-6/D-7). Already-verified installs succeed offline
/// without any network activity (idempotency, legacy M-3 rule) and report
/// `downloaded: false` with zero transfer but full verify statistics.
///
/// # Errors
///
/// Offline/license/disk/transport/hash categories; a failed transaction
/// never exposes half a valid asset set (reverse-order rollback restores
/// the previous bytes, `.part` files are cleaned).
pub fn install_with_report(
    context: InstallContext<'_>,
) -> Result<InstallRunReport, InferenceError> {
    let started = std::time::Instant::now();
    let manifest = context.manifest;
    manifest.validate()?;
    let cache_root = context.cache_root;
    let observer = context.observer;
    let transport = context.transport;
    // 1. Idempotent short circuit: a fully verified install needs nothing.
    if verify_content(cache_root, manifest).is_ok() {
        let receipt_path = receipt_path(cache_root, manifest)?;
        if !receipt_path.is_file() {
            write_receipt(cache_root, manifest)?;
        }
        let verify = verify_report_for(cache_root, manifest, observer)?;
        emit(observer, InstallEvent::Done);
        return Ok(InstallRunReport {
            model: manifest.model.as_str().to_owned(),
            profile: manifest.profile.as_str().to_owned(),
            revision: manifest.source_revision.clone(),
            size_bytes: manifest.assets.iter().map(|asset| asset.byte_len).sum(),
            elapsed_ms: started.elapsed().as_millis(),
            stage: "done",
            download: DownloadReport::default(),
            verify,
            receipt: receipt(cache_root, manifest, false)?,
        });
    }
    // 2. Policy gates (frozen semantics, same as the legacy install paths).
    if context.policy.offline {
        return Err(InferenceError::Offline);
    }
    if context.policy.non_interactive && !context.policy.license_accepted {
        return Err(InferenceError::LicenseRequired);
    }
    // 3. Disk preflight against the bytes still missing (the legacy check
    // ran after the transfer; the pre-flight placement is the M-8 rule).
    let pending = pending_assets(cache_root, manifest)?;
    let pending_bytes: u64 = pending
        .iter()
        .filter_map(|role| manifest.asset(role))
        .map(|asset| asset.byte_len)
        .sum();
    if let Some(available) = context.available_bytes
        && available < pending_bytes
    {
        return Err(InferenceError::InsufficientDisk(format!(
            "need {pending_bytes} bytes, {available} available"
        )));
    }
    // 4. Interactive confirmation of the actual download set.
    if !context.policy.non_interactive {
        let proposals: Vec<DownloadProposal> = pending
            .iter()
            .filter_map(|role| {
                manifest.asset(role).map(|asset| DownloadProposal {
                    role: role.clone(),
                    url: asset.source_url.clone(),
                    byte_len: asset.byte_len,
                    license_id: manifest.license_id.clone(),
                })
            })
            .collect();
        if !proposals.is_empty() && !context.interaction.confirm_download(&proposals)? {
            // A declined proposal means no download may happen.
            return Err(InferenceError::Offline);
        }
    }
    // 5. Transaction: lock → sweep → stage → download (attempt cap + ticker)
    // → verify → backup → commit → receipt last.
    // Revision-level lock over the WHOLE transaction (upstream hardening):
    // the shared projector and its transaction files live at revision scope,
    // so concurrent installs of sibling profiles must serialize here — a
    // rejected contender must never clean the active transaction's files.
    let revision_dir = cache_root
        .join("ocrx")
        .join("models")
        .join(manifest.model.as_str())
        .join(&manifest.source_revision);
    std::fs::create_dir_all(&revision_dir)
        .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    let _lock = InstallLock::acquire(&revision_dir)?;
    emit(
        observer,
        InstallEvent::LockAcquired {
            pid: std::process::id(),
        },
    );
    // D-7: stale transaction residue sweep over BOTH scopes (this profile's
    // dir + the revision dir), pattern-matched to the ACTUAL staging names.
    // Only the lock holder reaches here, so nobody's LIVE transaction files
    // can be swept; a dead holder's leftovers are exactly the target.
    let profile_dir = revision_dir.join(manifest.profile.as_str());
    sweep_transaction_residue(&[&profile_dir, &revision_dir]);
    let targets: Vec<_> = manifest_asset_paths(cache_root, manifest)?;
    let mut directories = std::collections::BTreeSet::new();
    for (_, path) in &targets {
        if let Some(directory) = path.parent() {
            directories.insert(directory.to_path_buf());
        }
    }
    for directory in &directories {
        std::fs::create_dir_all(directory)
            .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    }
    let transaction = transaction_id();
    let staged: Vec<_> = targets
        .iter()
        .map(|(role, path)| {
            let part = path.with_extension(format!(
                "gguf{transaction}.part-{}",
                role.as_str().replace('/', "_")
            ));
            (role.clone(), path.clone(), part)
        })
        .collect();
    // D-7: best-effort signal handling — purge THIS transaction's .part
    // files, then exit(130). Scoped to the transaction window.
    let _signal_guard = if context.handle_signals {
        Some(SignalPurgeGuard::install(
            staged.iter().map(|(_, _, part)| part.clone()).collect(),
        ))
    } else {
        None
    };
    let interval = context.progress_interval.unwrap_or(PROGRESS_INTERVAL);
    let mut download_report = DownloadReport::default();
    let mut verify_report = VerifyReport::default();
    let mut transferred_any_asset = false;
    // The ticker observes the in-flight `.part` from its own thread, so the
    // whole download loop runs inside a scope that outlives every ticker.
    let result = std::thread::scope(|scope| {
        for (role, path, part) in &staged {
            let (stat, ok) = verify_asset(manifest, role, path);
            if ok {
                // Already correct on disk: verified, counted, no transfer.
                emit(
                    observer,
                    InstallEvent::Verify {
                        role: stat.role.as_str(),
                        sha256_ok: stat.sha256_ok,
                        size_ok: stat.size_ok,
                    },
                );
                verify_report.per_asset.push(stat);
                continue;
            }
            let descriptor = manifest
                .asset(role)
                .ok_or_else(|| InferenceError::InvalidManifest(format!("role {role} vanished")))?;
            let mut last_error: Option<InferenceError> = None;
            for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
                emit(
                    observer,
                    InstallEvent::DownloadStart {
                        role: role.as_str(),
                        url: &descriptor.source_url,
                        byte_len: descriptor.byte_len,
                        attempt,
                    },
                );
                let download_started = std::time::Instant::now();
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let ticker_stop = std::sync::Arc::clone(&stop);
                let ticker = scope.spawn(move || {
                    ticker_loop(
                        part,
                        descriptor.byte_len,
                        role.as_str(),
                        interval,
                        observer,
                        ticker_stop,
                    );
                });
                let outcome = transport.download(&descriptor.source_url, part);
                stop.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = ticker.join();
                match outcome {
                    Ok(()) => {
                        transferred_any_asset = true;
                        // A later successful attempt clears the stale error —
                        // the retry exists precisely to recover, so the run
                        // must NOT surface the earlier failure.
                        last_error = None;
                        let elapsed = download_started.elapsed();
                        let bytes = std::fs::metadata(part)
                            .map(|meta| meta.len())
                            .unwrap_or(descriptor.byte_len);
                        let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
                        download_report.total_bytes =
                            download_report.total_bytes.saturating_add(bytes);
                        download_report.per_asset.push(AssetDownloadStat {
                            role: role.as_str().to_owned(),
                            bytes,
                            elapsed_ms: elapsed.as_millis(),
                            avg_rate_bps: bytes as f64 / seconds,
                        });
                        break;
                    }
                    Err(error) => {
                        last_error = Some(error);
                    }
                }
            }
            if let Some(error) = last_error {
                return Err(error);
            }
            let (stat, ok) = verify_asset(manifest, role, part);
            emit(
                observer,
                InstallEvent::Verify {
                    role: stat.role.as_str(),
                    sha256_ok: stat.sha256_ok,
                    size_ok: stat.size_ok,
                },
            );
            verify_report.per_asset.push(stat);
            if !ok {
                // Hash and size gate (same categories as before the split).
                return Err(InferenceError::HashMismatch(format!("asset {role}")));
            }
            let _ = ok;
        }
        emit(observer, InstallEvent::Commit);
        commit_group(&staged, &transaction)
    });
    if let Err(error) = result {
        for (_, _, part) in &staged {
            let _ = std::fs::remove_file(part);
        }
        return Err(error);
    }
    write_receipt(cache_root, manifest)?;
    // Aggregate rate over the whole run (0 when nothing transferred).
    let elapsed_secs = started.elapsed().as_secs_f64();
    download_report.downloaded = transferred_any_asset;
    download_report.avg_rate_bps = if download_report.total_bytes > 0 && elapsed_secs > 0.0 {
        download_report.total_bytes as f64 / elapsed_secs
    } else {
        0.0
    };
    emit(observer, InstallEvent::Done);
    Ok(InstallRunReport {
        model: manifest.model.as_str().to_owned(),
        profile: manifest.profile.as_str().to_owned(),
        revision: manifest.source_revision.clone(),
        size_bytes: manifest.assets.iter().map(|asset| asset.byte_len).sum(),
        elapsed_ms: started.elapsed().as_millis(),
        stage: "done",
        download: download_report,
        verify: verify_report,
        receipt: receipt(cache_root, manifest, !pending.is_empty())?,
    })
}

/// Ticker body (plan D-5): stats the `.part` file at `interval`, emitting
/// download-progress with byte counts and the observed window rate. Stops
/// when `stop` flips.
#[allow(clippy::needless_pass_by_value)]
fn ticker_loop(
    part: &Path,
    total_bytes: u64,
    role: &str,
    interval: std::time::Duration,
    observer: Option<InstallObserver<'_>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut last_bytes = 0_u64;
    let mut last_tick = std::time::Instant::now();
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(interval);
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let Ok(metadata) = std::fs::metadata(part) else {
            continue;
        };
        let bytes = metadata.len();
        let now = std::time::Instant::now();
        let window = now
            .duration_since(last_tick)
            .as_secs_f64()
            .max(f64::EPSILON);
        let rate = if bytes > last_bytes {
            (bytes - last_bytes) as f64 / window
        } else {
            0.0
        };
        last_bytes = bytes;
        last_tick = now;
        emit(
            observer,
            InstallEvent::DownloadProgress {
                role,
                bytes,
                total_bytes,
                rate_bps: rate,
            },
        );
    }
}

/// Best-effort signal guard: installs a ctrlc-style handler that purges
/// THIS transaction's `.part` files, then exits(130) (plan D-7).
struct SignalPurgeGuard {
    #[allow(dead_code)]
    paths: std::sync::Arc<Vec<std::path::PathBuf>>,
}

impl SignalPurgeGuard {
    fn install(paths: Vec<std::path::PathBuf>) -> Self {
        let shared = std::sync::Arc::new(paths);
        let handler_paths = std::sync::Arc::clone(&shared);
        // Registration failure keeps the guard (and thus the normal failure-
        // path cleanup); only the extra signal purge is lost.
        let _ = ctrlc::set_handler(move || {
            purge_paths(&handler_paths);
            std::process::exit(130);
        });
        Self { paths: shared }
    }
}

/// Advisory revision-level install lock (upstream hardening): `create_new`
/// means a holder is detected atomically; a lock whose recorded PID is no
/// longer alive is reclaimed. The holder PID is written into the file so a
/// crashed process's lock is distinguishable from a live one.
struct InstallLock {
    path: std::path::PathBuf,
    owner: String,
    file: std::fs::File,
}

impl InstallLock {
    fn acquire(directory: &std::path::Path) -> Result<Self, InferenceError> {
        let path = directory.join(".install.lock");
        let create = || {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
        };
        let owner = std::process::id().to_string();
        let file = if let Ok(file) = create() {
            file
        } else {
            let holder = std::fs::read_to_string(&path)
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok());
            let stale = holder.is_some_and(|pid| process_liveness(pid) == ProcessLiveness::Dead);
            if stale {
                std::fs::remove_file(&path).map_err(|_| {
                    InferenceError::InstallLocked(path.to_string_lossy().into_owned())
                })?;
                create().map_err(|_| {
                    InferenceError::InstallLocked(path.to_string_lossy().into_owned())
                })?
            } else {
                // C2 stage-2 item 6: the rejection message exposes the holder
                // PID so the plugin guidance can name the competing process.
                return Err(InferenceError::InstallLocked(match holder {
                    Some(pid) => format!(
                        "another install holds {} (holder pid {pid})",
                        path.display()
                    ),
                    None => format!("another install holds {}", path.display()),
                }));
            }
        };
        use std::io::Write;
        let mut file = file;
        file.write_all(owner.as_bytes())
            .map_err(|_| InferenceError::InstallLocked(path.to_string_lossy().into_owned()))?;
        file.flush()
            .map_err(|_| InferenceError::InstallLocked(path.to_string_lossy().into_owned()))?;
        Ok(Self { path, owner, file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).is_ok_and(|contents| contents == self.owner) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn process_liveness(pid: u32) -> ProcessLiveness {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map_or(ProcessLiveness::Unknown, |status| {
            if status.success() {
                ProcessLiveness::Alive
            } else {
                ProcessLiveness::Dead
            }
        })
}

#[cfg(not(unix))]
fn process_liveness(pid: u32) -> ProcessLiveness {
    // /FO CSV keeps fields quoted and comma-separated; the default
    // whitespace split misparsed image names containing spaces and could
    // declare a live holder dead (review m10).
    let output = match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return ProcessLiveness::Unknown,
    };
    if String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        // CSV row: "name","pid",... — second quoted field is the PID.
        line.split("\",\"")
            .nth(1)
            .map(|field| field.trim_matches('"').parse::<u32>().ok())
            .unwrap_or(None)
            == Some(pid)
    }) {
        ProcessLiveness::Alive
    } else {
        ProcessLiveness::Dead
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessLiveness {
    Alive,
    Dead,
    Unknown,
}

/// Roles whose on-disk content is not already correct.
fn pending_assets(
    cache_root: &Path,
    manifest: &ModelManifest,
) -> Result<Vec<ocrx_inference_api::AssetRole>, InferenceError> {
    let mut pending = Vec::new();
    for (role, path) in manifest_asset_paths(cache_root, manifest)? {
        if !file_matches(manifest, &role, &path)? {
            pending.push(role);
        }
    }
    Ok(pending)
}

fn file_matches(
    manifest: &ModelManifest,
    role: &ocrx_inference_api::AssetRole,
    path: &Path,
) -> Result<bool, InferenceError> {
    let descriptor = manifest
        .asset(role)
        .ok_or_else(|| InferenceError::InvalidManifest(format!("role {role} vanished")))?;
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(false);
    };
    if metadata.len() != descriptor.byte_len {
        return Ok(false);
    }
    let hash = sha256_file(path)
        .map_err(|_| InferenceError::MissingAsset(path.to_string_lossy().into_owned()))?;
    Ok(hash == descriptor.sha256.to_ascii_lowercase())
}

/// Backs up existing targets, renames staged parts into place, and rolls
/// back in reverse order on any failure.
fn commit_group(
    staged: &[(
        ocrx_inference_api::AssetRole,
        std::path::PathBuf,
        std::path::PathBuf,
    )],
    transaction: &str,
) -> Result<(), InferenceError> {
    let backups: Vec<_> = staged
        .iter()
        .map(|(_, path, _)| {
            (
                path.clone(),
                path.with_extension(format!("gguf{transaction}.bak")),
                path.is_file(),
            )
        })
        .collect();
    for (path, backup, existed) in &backups {
        if *existed {
            std::fs::rename(path, backup)
                .map_err(|_| InferenceError::InsufficientDisk("backup rename failed".into()))?;
        }
    }
    let mut committed = 0_usize;
    for (_, path, part) in staged {
        match std::fs::rename(part, path) {
            Ok(()) => committed += 1,
            Err(_) => {
                // Roll back in reverse order: remove what we committed, then
                // restore every backup.
                let mut recovery_failed = false;
                for (_, target, _) in staged.iter().take(committed).rev() {
                    if std::fs::remove_file(target).is_err() && target.exists() {
                        recovery_failed = true;
                    }
                }
                for (target, backup, existed) in &backups {
                    if *existed && std::fs::rename(backup, target).is_err() {
                        recovery_failed = true;
                    }
                }
                if recovery_failed {
                    return Err(InferenceError::InsufficientDisk(
                        "commit failed and rollback was incomplete".into(),
                    ));
                }
                return Err(InferenceError::InsufficientDisk(
                    "commit rename failed".into(),
                ));
            }
        }
    }
    for (_, backup, existed) in &backups {
        if *existed {
            let _ = std::fs::remove_file(backup);
        }
    }
    Ok(())
}

/// True when the file name matches a leftover transaction artifact
/// (plan D-7: pattern-matched to the ACTUAL staging names, not a guess).
#[allow(clippy::case_sensitive_file_extension_comparisons)] // crate-generated lowercase names
fn is_stale_transaction_file(name: &str) -> bool {
    // Actual staging shapes (install.rs transaction_id + role suffix):
    //   model.gguf.install-<pid>-<hex>.part-model
    //   model.gguf.install-<pid>-<hex>.bak
    // Older/other shapes worth sweeping: *.part / *.bak / *.etag
    name.ends_with(".etag")
        || name.ends_with(".part")
        || name.ends_with(".bak")
        || (name.contains(".install-") && (name.contains(".part-")))
}

/// Sweeps stale transaction residue (`*.install-*.part-*`, `*.part`,
/// `*.bak`, `*.etag`) inside the given directories (plan D-7: the profile
/// dir AND the revision dir). Only the lock holder calls this, so a
/// competing install's live files can never be swept; non-transaction files
/// (receipts, assets) never match the pattern.
pub(crate) fn sweep_transaction_residue(scopes: &[&Path]) {
    for scope_dir in scopes {
        let Ok(entries) = std::fs::read_dir(scope_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_stale_transaction_file(&name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub(crate) fn transaction_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".install-{}-{nanos:x}", std::process::id())
}

fn receipt_path(
    cache_root: &Path,
    manifest: &ModelManifest,
) -> Result<std::path::PathBuf, InferenceError> {
    Ok(
        asset_cache_path(cache_root, manifest, &manifest.assets[0].role)?
            .parent()
            .ok_or_else(|| InferenceError::InvalidManifest("asset path has no parent".into()))?
            .join(RECEIPT_FILE_NAME),
    )
}

#[derive(Serialize)]
struct ReceiptAsset<'a> {
    role: &'a str,
    sha256: &'a str,
    byte_len: u64,
    scope: &'a str,
    source_url: &'a str,
}

#[derive(Serialize)]
struct ReceiptFile<'a> {
    schema_version: u32,
    model: &'a str,
    profile: &'a str,
    source_revision: &'a str,
    license_id: &'a str,
    assets: Vec<ReceiptAsset<'a>>,
    runtime_binding: &'a ocrx_inference_api::BindingRequirement,
    adapter_binding: &'a ocrx_inference_api::AdapterRequirement,
}

/// Writes the deterministic receipt JSON (stable field order via the
/// serde struct declaration; assets keep manifest order).
fn write_receipt(cache_root: &Path, manifest: &ModelManifest) -> Result<(), InferenceError> {
    let path = receipt_path(cache_root, manifest)?;
    let file = ReceiptFile {
        schema_version: 1,
        model: manifest.model.as_str(),
        profile: manifest.profile.as_str(),
        source_revision: &manifest.source_revision,
        license_id: &manifest.license_id,
        assets: manifest
            .assets
            .iter()
            .map(|asset| ReceiptAsset {
                role: asset.role.as_str(),
                sha256: &asset.sha256,
                byte_len: asset.byte_len,
                scope: match asset.scope {
                    ocrx_inference_api::AssetScope::Profile => "profile",
                    ocrx_inference_api::AssetScope::Revision => "revision",
                },
                source_url: &asset.source_url,
            })
            .collect(),
        runtime_binding: &manifest.runtime_binding,
        adapter_binding: &manifest.adapter_binding,
    };
    let json = serde_json::to_string(&file)
        .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?;
    let staged = path.with_extension(format!("json{}", transaction_id()));
    std::fs::write(&staged, json.as_bytes())
        .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    std::fs::rename(&staged, &path)
        .map_err(|error| InferenceError::InsufficientDisk(error.to_string()))?;
    Ok(())
}

/// Reads back the written receipt into the public DTO (audit record).
fn receipt(
    cache_root: &Path,
    manifest: &ModelManifest,
    downloaded: bool,
) -> Result<InstallReceipt, InferenceError> {
    let _ = receipt_path(cache_root, manifest)?;
    Ok(InstallReceipt {
        model: manifest.model.clone(),
        profile: manifest.profile.clone(),
        revision: manifest.source_revision.clone(),
        assets: manifest
            .assets
            .iter()
            .map(|asset| ocrx_inference_api::AssetIdentity {
                role: asset.role.clone(),
                sha256: asset.sha256.clone(),
                byte_len: asset.byte_len,
            })
            .collect(),
        downloaded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::FakeTransport;
    use ocrx_inference_api::AssetRole;
    use ocrx_inference_api::{
        AdapterId, AdapterRequirement, AssetDescriptor, AssetProvenance, AssetScope,
        BindingRequirement, InstallInteraction, ModelId, ProfileId, RuntimeId,
    };

    fn tiny_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: 2,
            model: ModelId::new("ovisocr2").unwrap(),
            profile: ProfileId::new("compact").unwrap(),
            display_name: "OvisOCR2".into(),
            family: "Ovis".into(),
            quantization: "Q4_K_M".into(),
            source_revision: "rev-test-1".into(),
            assets: vec![
                AssetDescriptor {
                    role: AssetRole::new("model").unwrap(),
                    required: true,
                    scope: AssetScope::Profile,
                    relative_path: "model.gguf".into(),
                    sha256: ocrx_primitives::sha256(b"model-bytes"),
                    byte_len: 11,
                    source_url: "https://example.invalid/model.gguf".into(),
                },
                AssetDescriptor {
                    role: AssetRole::new("mmproj").unwrap(),
                    required: true,
                    scope: AssetScope::Revision,
                    relative_path: "mmproj.gguf".into(),
                    sha256: ocrx_primitives::sha256(b"mmproj-bytes"),
                    byte_len: 12,
                    source_url: "https://example.invalid/mmproj.gguf".into(),
                },
            ],
            runtime_binding: BindingRequirement {
                runtime: RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".into(),
                sys_version: "0.1.161".into(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".into(),
            },
            adapter_binding: AdapterRequirement {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".into(),
            },
            license_id: "apache-2.0".into(),
            provenance: AssetProvenance {
                provider: "huggingface".into(),
                url: "https://example.invalid".into(),
            },
        }
    }

    /// Same shape as [`tiny_manifest`], but pinned to the ovisocr3 identity
    /// (T08: the install/remove/verify machinery is model-agnostic — it
    /// reads every path through the manifest).
    fn tiny_ovisocr3_manifest() -> ModelManifest {
        let mut manifest = tiny_manifest();
        manifest.model = ModelId::new("ovisocr3").unwrap();
        manifest.profile = ProfileId::new("quality").unwrap();
        manifest.display_name = "OvisOCR3".into();
        manifest.quantization = "Q8_0".into();
        manifest.source_revision = "rev-ovisocr3-1".into();
        manifest.adapter_binding.adapter = AdapterId::new("ovisocr3").unwrap();
        manifest
    }

    struct RecordingInteraction {
        confirmations: usize,
        answer: bool,
    }

    impl RecordingInteraction {
        fn accepting() -> Self {
            Self {
                confirmations: 0,
                answer: true,
            }
        }
    }

    impl InstallInteraction for RecordingInteraction {
        fn confirm_download(
            &mut self,
            proposals: &[DownloadProposal],
        ) -> Result<bool, InferenceError> {
            self.confirmations += 1;
            assert!(!proposals.is_empty());
            Ok(self.answer)
        }
    }

    fn ctx<'a>(
        manifest: &'a ModelManifest,
        root: &'a Path,
        transport: &'a dyn Transport,
        interaction: &'a mut dyn InstallInteraction,
        policy: InstallPolicy,
        available: Option<u64>,
    ) -> InstallContext<'a> {
        InstallContext {
            manifest,
            cache_root: root,
            policy,
            transport,
            interaction,
            available_bytes: available,
            observer: None,
            progress_interval: None,
            handle_signals: false,
        }
    }

    #[test]
    fn install_downloads_verifies_commits_and_writes_the_receipt_last() {
        let root = std::env::temp_dir().join(format!("ocrx-install-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"model-bytes");
        // mmproj bytes differ per URL: re-serve with the right prefix.
        transport.responses.lock().unwrap().push((
            "https://example.invalid/mmproj".into(),
            Ok(b"mmproj-bytes".to_vec()),
        ));
        let mut interaction = RecordingInteraction::accepting();
        let receipt = install(ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            InstallPolicy {
                offline: false,
                non_interactive: true,
                license_accepted: true,
            },
            None,
        ))
        .unwrap();
        assert!(receipt.downloaded);
        // Both assets verify on disk.
        assert_eq!(verify_content(&root, &manifest), Ok(()));
        // Receipt file exists and is deterministic JSON.
        let receipt_path =
            root.join("ocrx/models/ovisocr2/rev-test-1/compact/install-receipt.json");
        let json = std::fs::read_to_string(&receipt_path).unwrap();
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"role\":\"model\""));
        assert!(json.contains("d81235049384534c167caea52b85a694f6103d14"));
        // No transaction residue survives.
        let profile_dir = receipt_path.parent().unwrap();
        assert!(!profile_dir.read_dir().unwrap().flatten().any(|entry| {
            entry.file_name().to_string_lossy().contains(".part")
                || entry.file_name().to_string_lossy().contains(".bak")
        }));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_verify_remove_round_trip_is_model_agnostic_for_ovisocr3() {
        // T08/C5: the ovisocr3 subtree installs, verifies, and removes
        // through the same transaction machinery without touching the
        // ovisocr2 subtree.
        let root =
            std::env::temp_dir().join(format!("ocrx-install-ovisocr3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_ovisocr3_manifest();
        // Stage content directly (offline install path with staged content).
        for asset in &manifest.assets {
            let path = asset_cache_path(&root, &manifest, &asset.role).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                if asset.role.as_str() == "model" {
                    &b"model-bytes"[..]
                } else {
                    &b"mmproj-bytes"[..]
                },
            )
            .unwrap();
        }
        assert_eq!(verify_content(&root, &manifest), Ok(()));
        // The ovisocr2 subtree never appears.
        assert!(!root.join("ocrx/models/ovisocr2").exists());
        // Receipt lands under the ovisocr3 subtree.
        let receipt_path =
            root.join("ocrx/models/ovisocr3/rev-ovisocr3-1/quality/install-receipt.json");
        // A removal round trip clears only the ovisocr3 subtree.
        let transport = FakeTransport::new();
        let mut interaction = RecordingInteraction::accepting();
        install(ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            InstallPolicy {
                offline: true,
                non_interactive: true,
                license_accepted: true,
            },
            None,
        ))
        .unwrap();
        assert!(receipt_path.is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_installs_are_idempotent_and_never_touch_the_network() {
        let root = std::env::temp_dir().join(format!("ocrx-install-idem-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        // Stage the correct content directly (as a legacy install would have).
        for asset in &manifest.assets {
            let path = asset_cache_path(&root, &manifest, &asset.role).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                if asset.role.as_str() == "model" {
                    &b"model-bytes"[..]
                } else {
                    &b"mmproj-bytes"[..]
                },
            )
            .unwrap();
        }
        let transport = FakeTransport::new();
        let mut interaction = RecordingInteraction::accepting();
        let receipt = install(ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            InstallPolicy {
                offline: true, // offline must still succeed
                non_interactive: true,
                license_accepted: false,
            },
            None,
        ))
        .unwrap();
        assert!(!receipt.downloaded);
        assert!(transport.requested_urls().is_empty(), "no network");
        // A missing receipt is backfilled without downloading.
        let receipt_path =
            root.join("ocrx/models/ovisocr2/rev-test-1/compact/install-receipt.json");
        assert!(receipt_path.is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_failures_never_create_partial_state() {
        let root = std::env::temp_dir().join(format!("ocrx-install-policy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        let mut interaction = RecordingInteraction::accepting();
        // Offline.
        assert_eq!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy::offline(),
                None
            )),
            Err(InferenceError::Offline)
        );
        // Non-interactive without license.
        assert_eq!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: true,
                    license_accepted: false
                },
                None
            )),
            Err(InferenceError::LicenseRequired)
        );
        // Insufficient disk preflight.
        assert!(matches!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: true,
                    license_accepted: true
                },
                Some(10)
            )),
            Err(InferenceError::InsufficientDisk(_))
        ));
        assert!(!root.join("ocrx").exists(), "no partial state on disk");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hash_failures_roll_back_and_clean_parts() {
        let root = std::env::temp_dir().join(format!("ocrx-install-hash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"corrupted-bytes!!");
        let mut interaction = RecordingInteraction::accepting();
        assert!(matches!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: true,
                    license_accepted: true
                },
                None
            )),
            Err(InferenceError::HashMismatch(_))
        ));
        // Nothing usable was exposed: no committed asset exists.
        for asset in &manifest.assets {
            let path = asset_cache_path(&root, &manifest, &asset.role).unwrap();
            assert!(!path.exists(), "{} must not exist", asset.role);
        }
        // No .part residue.
        let profile_dir = root.join("ocrx/models/ovisocr2/rev-test-1/compact");
        if profile_dir.exists() {
            assert!(
                !profile_dir
                    .read_dir()
                    .unwrap()
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().contains(".part"))
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn declined_interactive_confirmation_aborts_without_downloads() {
        let root =
            std::env::temp_dir().join(format!("ocrx-install-declined-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"model-bytes");
        let mut interaction = RecordingInteraction {
            confirmations: 0,
            answer: false,
        };
        assert_eq!(
            install(ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                InstallPolicy {
                    offline: false,
                    non_interactive: false,
                    license_accepted: false
                },
                None
            )),
            Err(InferenceError::Offline)
        );
        assert_eq!(interaction.confirmations, 1);
        assert!(transport.requested_urls().is_empty(), "declined = no fetch");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn contended_revision_lock_rejects_and_spares_active_transaction_files() {
        // Upstream hardening (main 0149d65/5388e76 lineage): a second install
        // whose revision lock is held by a LIVE other process is rejected
        // with InstallLocked, and — because the rejected transaction never
        // enters its cleanup — the holder's active .part/.bak files survive
        // byte-for-byte. With an unknown/dead holder the lock is reclaimed.
        let root = std::env::temp_dir().join(format!("ocrx-install-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"model-bytes");
        transport.responses.lock().unwrap().push((
            "https://example.invalid/mmproj".into(),
            Ok(b"mmproj-bytes".to_vec()),
        ));
        let revision_dir = root
            .join("ocrx")
            .join("models")
            .join("ovisocr2")
            .join("rev-test-1");
        std::fs::create_dir_all(&revision_dir).unwrap();
        // A LIVE holder: use this test process's own PID. process_liveness
        // sees it as alive on every platform, so the lock must be rejected —
        // a nonexistent PID (e.g. u32::MAX-1) is DEAD on unix (kill -0 ->
        // ESRCH) and would be stale-reclaimed instead of rejected, which is
        // the other feature. CI caught exactly that divergence on macOS.
        let foreign_holder = std::process::id();
        std::fs::write(
            revision_dir.join(".install.lock"),
            foreign_holder.to_string(),
        )
        .unwrap();
        // Active transaction files belonging to the holder.
        let profile_dir = revision_dir.join("compact");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let active_part = profile_dir.join("model.active.part");
        let shared_part = revision_dir.join("mmproj.active.part");
        for path in [&active_part, &shared_part] {
            std::fs::write(path, b"active transaction").unwrap();
        }
        let mut interaction = RecordingInteraction::accepting();
        let error = install(ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            InstallPolicy {
                offline: false,
                non_interactive: true,
                license_accepted: true,
            },
            Some(u64::MAX),
        ))
        .expect_err("a held lock must reject the install");
        assert_eq!(error.kind(), "install_locked", "unexpected: {error}");
        // The rejected install touched nothing.
        for path in [&active_part, &shared_part] {
            assert_eq!(
                std::fs::read(path).unwrap(),
                b"active transaction",
                "active transaction file was disturbed: {path:?}"
            );
        }
        assert!(
            transport.requested_urls().is_empty(),
            "a locked install must not reach the network"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- C2/D-5: install run reports (progress statistics face) -------------

    fn reporting_ctx<'a>(
        manifest: &'a ModelManifest,
        root: &'a Path,
        transport: &'a dyn Transport,
        interaction: &'a mut dyn InstallInteraction,
        observer: InstallObserver<'a>,
    ) -> InstallContext<'a> {
        InstallContext {
            manifest,
            cache_root: root,
            policy: InstallPolicy {
                offline: false,
                non_interactive: true,
                license_accepted: true,
            },
            transport,
            interaction,
            available_bytes: None,
            observer: Some(observer),
            progress_interval: None,
            handle_signals: false,
        }
    }

    fn serve_both_assets(transport: &FakeTransport) {
        transport.serve("https://example.invalid/model", b"model-bytes");
        transport.serve("https://example.invalid/mmproj", b"mmproj-bytes");
    }

    #[test]
    fn install_report_carries_download_and_verify_statistics() {
        // §9.2 final-result statistics face (plan D-5): per-asset bytes/
        // elapsed/rate and per-asset verify results ride the RUN REPORT.
        let root = std::env::temp_dir().join(format!("ocrx-install-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        serve_both_assets(&transport);
        let mut interaction = RecordingInteraction::accepting();
        let report = install_with_report(reporting_ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            &|_| {},
        ))
        .unwrap();
        // Receipt face unchanged.
        assert!(report.receipt.downloaded);
        assert_eq!(report.model, "ovisocr2");
        assert_eq!(report.profile, "compact");
        assert_eq!(report.revision, "rev-test-1");
        assert_eq!(report.stage, "done");
        assert_eq!(report.size_bytes, 23, "11 + 12 planned bytes");
        let download = &report.download;
        assert!(download.downloaded);
        assert_eq!(download.total_bytes, 23);
        assert!(download.avg_rate_bps > 0.0, "fresh transfer has a rate");
        assert_eq!(download.per_asset.len(), 2);
        let model_stat = download
            .per_asset
            .iter()
            .find(|a| a.role == "model")
            .unwrap();
        assert_eq!(model_stat.bytes, 11);
        assert!(model_stat.avg_rate_bps > 0.0);
        let verify = &report.verify;
        assert_eq!(verify.per_asset.len(), 2);
        assert!(verify.per_asset.iter().all(|a| a.sha256_ok && a.size_ok));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn idempotent_install_reports_zero_transfer_but_full_verify() {
        let root = std::env::temp_dir().join(format!("ocrx-install-idem2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        for asset in &manifest.assets {
            let path = asset_cache_path(&root, &manifest, &asset.role).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                if asset.role.as_str() == "model" {
                    &b"model-bytes"[..]
                } else {
                    &b"mmproj-bytes"[..]
                },
            )
            .unwrap();
        }
        let transport = FakeTransport::new();
        let mut interaction = RecordingInteraction::accepting();
        let report = install_with_report(reporting_ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            &|_| {},
        ))
        .unwrap();
        assert!(!report.download.downloaded, "已就绪，未发生下载");
        assert_eq!(report.download.total_bytes, 0);
        assert!(report.download.avg_rate_bps.abs() < f64::EPSILON);
        assert!(report.download.per_asset.iter().all(|a| a.bytes == 0));
        assert_eq!(
            report.verify.per_asset.len(),
            2,
            "idempotent installs still verify every asset"
        );
        assert!(
            report
                .verify
                .per_asset
                .iter()
                .all(|a| a.sha256_ok && a.size_ok)
        );
        assert!(transport.requested_urls().is_empty(), "no network");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_emits_the_frozen_stage_event_sequence() {
        // Plan D-5 progress stages: lock-acquired / download-start /
        // download-progress / verify / commit / done.
        let root = std::env::temp_dir().join(format!("ocrx-install-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        serve_both_assets(&transport);
        let mut interaction = RecordingInteraction::accepting();
        let events: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let report = {
            let events_ref = &events;
            install_with_report(reporting_ctx(
                &manifest,
                &root,
                &transport,
                &mut interaction,
                &|event| {
                    events_ref
                        .lock()
                        .unwrap()
                        .push(event.stage_name().to_owned());
                },
            ))
            .unwrap()
        };
        assert_eq!(report.stage, "done");
        let stages = events.lock().unwrap().clone();
        for expected in [
            "lock-acquired",
            "download-start",
            "verify",
            "commit",
            "done",
        ] {
            assert!(
                stages.iter().any(|stage| stage == expected),
                "stage {expected} missing from {stages:?}"
            );
        }
        assert!(
            stages.iter().all(|stage| *stage != "download-progress"),
            "instant fake transfers emit no tick: {stages:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn download_progress_ticks_arrive_while_a_transfer_is_in_flight() {
        // The ~500ms ticker stats the .part file; a slow transfer must produce
        // at least one download-progress event with byte counts.
        let root = std::env::temp_dir().join(format!("ocrx-install-tick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = SlowTransport {
            delay: std::time::Duration::from_millis(120),
            responses: [
                ("https://example.invalid/model", b"model-bytes".to_vec()),
                ("https://example.invalid/mmproj", b"mmproj-bytes".to_vec()),
            ],
        };
        let mut interaction = RecordingInteraction::accepting();
        let progress_events: std::sync::Mutex<Vec<(String, u64, u64)>> =
            std::sync::Mutex::new(Vec::new());
        let observer = |event: &InstallEvent<'_>| {
            if let InstallEvent::DownloadProgress {
                role,
                bytes,
                total_bytes,
                ..
            } = event
            {
                progress_events
                    .lock()
                    .unwrap()
                    .push(((*role).to_owned(), *bytes, *total_bytes));
            }
        };
        let mut context = reporting_ctx(&manifest, &root, &transport, &mut interaction, &observer);
        context.progress_interval = Some(std::time::Duration::from_millis(20));
        install_with_report(context).unwrap();
        let ticks = progress_events.lock().unwrap().clone();
        assert!(!ticks.is_empty(), "a 120ms transfer must tick at 20ms");
        // Both slow transfers tick; each tick's total is that asset's pinned
        // byte length (11 model / 12 mmproj).
        assert!(
            ticks
                .iter()
                .all(|(role, _, total)| (*total == 11 && role == "model")
                    || (*total == 12 && role == "mmproj"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn transport_failures_respect_the_per_asset_attempt_cap() {
        // D-6: each asset gets at most 3 download attempts; the exceeded cap
        // surfaces as the transport error kind.
        let root = std::env::temp_dir().join(format!("ocrx-install-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = CountingFailTransport::default();
        let mut interaction = RecordingInteraction::accepting();
        let error = install_with_report(reporting_ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            &|_| {},
        ))
        .unwrap_err();
        assert!(
            matches!(error, InferenceError::Transport(_)),
            "got {error:?}"
        );
        assert_eq!(
            transport.attempts.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "exactly the attempt cap, no infinite retry"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_transaction_residue_is_swept_inside_both_scopes() {
        // D-7: after the lock is acquired, stale .part/.bak/.etag files in the
        // profile dir AND the revision dir are removed; the live transaction's
        // own files are naturally absent (we clean before staging).
        let root = std::env::temp_dir().join(format!("ocrx-install-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        serve_both_assets(&transport);
        let revision_dir = root
            .join("ocrx")
            .join("models")
            .join("ovisocr2")
            .join("rev-test-1");
        let profile_dir = revision_dir.join("compact");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let stale = [
            // Actual staging name shape (install.rs transaction_id + role suffix).
            profile_dir.join("model.gguf.install-1234-deadbeef.part-model"),
            revision_dir.join("mmproj.gguf.install-99-cafe.part-mmproj"),
            // Older/other shapes the sweep must also cover.
            profile_dir.join("model.gguf.part"),
            profile_dir.join("model.gguf.bak"),
            revision_dir.join("mmproj.gguf.etag"),
        ];
        for path in &stale {
            std::fs::write(path, b"stale").unwrap();
        }
        let survivor = profile_dir.join("install-receipt.json");
        std::fs::write(&survivor, b"{}").unwrap();
        let mut interaction = RecordingInteraction::accepting();
        install_with_report(reporting_ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            &|_| {},
        ))
        .unwrap();
        for path in &stale {
            assert!(
                !path.exists(),
                "stale residue must be swept: {}",
                path.display()
            );
        }
        assert!(survivor.exists(), "non-transaction files survive the sweep");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn purge_paths_removes_exactly_the_transaction_files() {
        // D-7 signal handler logic (cross-platform leg): the purge removes
        // exactly the staged .part paths of THIS transaction.
        let dir = std::env::temp_dir().join(format!("ocrx-install-purge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let own = dir.join("model.gguf.install-42-x.part-model");
        let other = dir.join("user-file.txt");
        std::fs::write(&own, b"part").unwrap();
        std::fs::write(&other, b"keep").unwrap();
        purge_paths(std::slice::from_ref(&own));
        assert!(!own.exists());
        assert!(other.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_contention_message_carries_the_holder_pid() {
        // Plan stage 2 item 6: the lock failure message exposes the holder
        // PID for the plugin's guidance text.
        let root =
            std::env::temp_dir().join(format!("ocrx-install-lockpid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FakeTransport::new();
        let revision_dir = root
            .join("ocrx")
            .join("models")
            .join("ovisocr2")
            .join("rev-test-1");
        std::fs::create_dir_all(&revision_dir).unwrap();
        let holder = std::process::id();
        std::fs::write(revision_dir.join(".install.lock"), holder.to_string()).unwrap();
        let mut interaction = RecordingInteraction::accepting();
        let error = install_with_report(reporting_ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            &|_| {},
        ))
        .unwrap_err();
        let rendered = error.to_string();
        assert!(
            rendered.contains(&holder.to_string()),
            "lock error must carry the holder pid: {rendered}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- C2/D-6: curl hardening flags ---------------------------------------

    #[test]
    fn curl_transport_flags_harden_connect_and_stall_detection() {
        let command = crate::transport::curl_download_command(
            "https://example.invalid/model.gguf",
            "/tmp/ocrx-test.part",
        );
        let args: Vec<String> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        for expected in [
            "--connect-timeout",
            "30",
            "--speed-time",
            "60",
            "--speed-limit",
            "10240",
        ] {
            assert!(
                args.iter().any(|argument| argument == expected),
                "curl flags must contain {expected}: {args:?}"
            );
        }
    }

    // --- Test doubles --------------------------------------------------------

    /// Transport that writes its (correct) bytes slowly so the ticker can
    /// observe the in-flight .part file.
    /// Transport that fails the first N requests per URL, then serves correct
    /// bytes (BLOCK-3 regression: the retry must actually recover).
    struct FlakyTransport {
        failures_before_success: usize,
        failures_by_url: std::sync::Mutex<std::collections::HashMap<String, usize>>,
        responses: [(&'static str, Vec<u8>); 2],
    }

    impl FlakyTransport {
        fn new(failures_before_success: usize) -> Self {
            Self {
                failures_before_success,
                failures_by_url: std::sync::Mutex::new(std::collections::HashMap::new()),
                responses: [
                    ("https://example.invalid/model", b"model-bytes".to_vec()),
                    ("https://example.invalid/mmproj", b"mmproj-bytes".to_vec()),
                ],
            }
        }
    }

    impl Transport for FlakyTransport {
        fn download(&self, url: &str, destination: &Path) -> Result<(), InferenceError> {
            let Some((_, payload)) = self
                .responses
                .iter()
                .find(|(prefix, _)| url.starts_with(prefix))
            else {
                return Err(InferenceError::Transport(format!(
                    "flaky transport has no route for {url}"
                )));
            };
            // Serve the failure budget PER URL so a recovered asset never
            // re-arms failures for its siblings.
            let key = url.to_owned();
            let mut state = self
                .failures_by_url
                .lock()
                .expect("flaky transport lock");
            let remaining = state.entry(key).or_insert(self.failures_before_success);
            if *remaining == 0 {
                std::fs::write(destination, payload)
                    .map_err(|error| InferenceError::Transport(error.to_string()))?;
                return Ok(());
            }
            *remaining -= 1;
            Err(InferenceError::Transport("transient outage".into()))
        }
    }

    #[test]
    fn a_retry_that_succeeds_clears_the_stale_error_and_commits() {
        // BLOCK-3: attempt 1 fails, attempt 2 serves correct bytes — the
        // install must COMMIT the good bytes, not return the stale error.
        let root = std::env::temp_dir().join(format!("ocrx-install-flaky-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = tiny_manifest();
        let transport = FlakyTransport::new(1);
        let mut interaction = RecordingInteraction::accepting();
        let report = install_with_report(reporting_ctx(
            &manifest,
            &root,
            &transport,
            &mut interaction,
            &|_| {},
        ))
        .expect("retry recovery must succeed");
        assert!(report.receipt.downloaded);
        assert_eq!(verify_content(&root, &manifest), Ok(()), "good bytes committed");
        assert_eq!(report.download.total_bytes, 23, "both assets transferred");
        assert_eq!(verify_content(&root, &manifest), Ok(()));
        let _ = std::fs::remove_dir_all(&root);
    }

    struct SlowTransport {
        delay: std::time::Duration,
        responses: [(&'static str, Vec<u8>); 2],
    }

    impl Transport for SlowTransport {
        fn download(&self, url: &str, destination: &Path) -> Result<(), InferenceError> {
            let Some((_, payload)) = self
                .responses
                .iter()
                .find(|(prefix, _)| url.starts_with(prefix))
            else {
                return Err(InferenceError::Transport(format!(
                    "slow transport has no route for {url}"
                )));
            };
            let chunk_size = (payload.len() / 4).max(1);
            let chunk_delay = self.delay / 4;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(destination)
                .map_err(|error| InferenceError::Transport(error.to_string()))?;
            for chunk in payload.chunks(chunk_size) {
                std::thread::sleep(chunk_delay);
                use std::io::Write;
                file.write_all(chunk)
                    .map_err(|error| InferenceError::Transport(error.to_string()))?;
            }
            Ok(())
        }
    }

    /// Transport that fails every request and counts the attempts.
    #[derive(Default)]
    struct CountingFailTransport {
        attempts: std::sync::atomic::AtomicUsize,
    }

    impl Transport for CountingFailTransport {
        fn download(&self, _url: &str, _destination: &Path) -> Result<(), InferenceError> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(InferenceError::Transport("simulated outage".into()))
        }
    }
}
