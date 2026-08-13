//! Subprocess CLI contract tests.
//!
//! These exercise the compiled `ocrx` binary end-to-end via `CARGO_BIN_EXE_ocrx`,
//! which cargo sets (and builds the bin for) for integration tests. This is more
//! robust than locating the binary from a unit test and works cross-platform.

use std::process::Command;

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ocrx"))
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

#[cfg(target_os = "macos")]
#[test]
fn cli_initialization_error_is_one_json_error_object() {
    let output = run_cli(&["scan", "input.pdf", "--report", "json"]);
    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"schema_version\":1"));
    assert!(!stdout.contains("input.pdf"));
}
