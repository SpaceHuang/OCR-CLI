//! Cross-platform process-memory sampling for resource regression baselines (W0.3).
//!
//! The Windows resource baseline needs per-process RSS/footprint samples taken
//! while a scan renders a long document, mirroring how the macOS baseline
//! sampled `pdfium-check` with `ps -o rss=`. The [`ProcessMemorySampler`]
//! trait abstracts the collection so regression harnesses work unchanged on
//! both platforms; platform-specific collection lives only here, in the probe
//! layer (plan W0.3 Refactor).

/// One working-set sample of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessMemorySample {
    /// Resident set size in KiB. Windows reports the process working set;
    /// Unix reports RSS. Both are the "content-relevant" memory signal the
    /// non-linear-growth gate is measured against.
    pub rss_kb: u64,
}

/// Samples process-level memory for a PID.
///
/// Returns `None` when the platform refuses to report (the macOS sandbox
/// RSS failure mode) or the process is gone; callers must treat sampling
/// gaps as data, not as zero.
pub trait ProcessMemorySampler {
    fn sample_rss(&self, pid: u32) -> Option<ProcessMemorySample>;
}

/// Real sampler reading the platform's standard process listing.
///
/// Safe-only by design: `GetProcessMemoryInfo`/`GlobalMemoryStatusEx` would
/// need `unsafe` FFI, which the workspace forbids, so Windows reads the same
/// working-set counter through `tasklist` (and Unix through `ps`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RealProcessMemorySampler;

impl ProcessMemorySampler for RealProcessMemorySampler {
    fn sample_rss(&self, pid: u32) -> Option<ProcessMemorySample> {
        if cfg!(target_os = "windows") {
            let output = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            parse_tasklist_rss_kb(&String::from_utf8_lossy(&output.stdout))
                .map(|rss_kb| ProcessMemorySample { rss_kb })
        } else {
            let output = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &pid.to_string()])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            parse_ps_rss_kb(&String::from_utf8_lossy(&output.stdout))
                .map(|rss_kb| ProcessMemorySample { rss_kb })
        }
    }
}

/// Extracts the working set (KiB) from `tasklist /FO CSV /NH` output.
///
/// A data row is a quoted CSV line ending in a `"NN,NNN K"` memory field;
/// the localized no-match INFO line is unquoted and reads as `None`.
fn parse_tasklist_rss_kb(stdout: &str) -> Option<u64> {
    let line = stdout.lines().find(|line| line.starts_with('"'))?;
    let fields = csv_fields(line);
    let memory = fields.last()?;
    let digits: String = memory.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Extracts RSS (KiB) from `ps -o rss=` output (one bare number per line).
fn parse_ps_rss_kb(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .find(|line| !line.trim().is_empty())?
        .trim()
        .parse()
        .ok()
}

/// Splits one CSV line on commas that are outside quoted fields, stripping
/// the surrounding quotes (process names may contain commas).
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for character in line.chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            _ => current.push(character),
        }
    }
    fields.push(current);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasklist_parser_reads_quoted_mem_usage_with_thousands_separator() {
        let stdout = "\"pdfium-check.exe\",\"48252\",\"Console\",\"1\",\"75,940 K\"\r\n";
        assert_eq!(parse_tasklist_rss_kb(stdout), Some(75_940));
    }

    #[test]
    fn tasklist_parser_takes_first_data_row_and_ignores_trailing_rows() {
        let stdout = "\"ocrx.exe\",\"1\",\"Console\",\"1\",\"1,024 K\"\r\n\"other.exe\",\"2\",\"Console\",\"1\",\"2 K\"\r\n";
        assert_eq!(parse_tasklist_rss_kb(stdout), Some(1_024));
    }

    #[test]
    fn tasklist_parser_rejects_localized_info_lines_and_empty_output() {
        // When no PID matches, tasklist prints a localized INFO message that
        // is not a quoted CSV row; it must read as "no sample", never 0 KiB.
        assert_eq!(parse_tasklist_rss_kb("INFO: no tasks match.\r\n"), None);
        assert_eq!(parse_tasklist_rss_kb(""), None);
    }

    #[test]
    fn ps_parser_reads_bare_rss_kb_with_padding() {
        assert_eq!(parse_ps_rss_kb("  12345\n"), Some(12_345));
        assert_eq!(parse_ps_rss_kb(""), None);
        assert_eq!(parse_ps_rss_kb("RSS\n"), None);
    }

    /// W0.3 Red: the sampler must return process-level memory on Windows,
    /// unlike the macOS sandbox where RSS reads failed.
    #[cfg(windows)]
    #[test]
    fn real_sampler_returns_process_level_memory_on_windows() {
        let sample = RealProcessMemorySampler
            .sample_rss(std::process::id())
            .expect("Windows must report RSS for the current process");
        assert!(sample.rss_kb > 0);
    }

    /// An exited PID must yield `None`, not a zero-valued sample, so a
    /// harness can distinguish "process gone" from "measured 0".
    #[cfg(windows)]
    #[test]
    fn real_sampler_returns_none_for_exited_process() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit"])
            .spawn()
            .expect("cmd must spawn");
        let pid = child.id();
        child.wait().expect("child must exit");
        assert_eq!(RealProcessMemorySampler.sample_rss(pid), None);
    }
}
