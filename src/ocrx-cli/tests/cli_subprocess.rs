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
fn cli_doctor_json_report_parses_with_backslash_cache_root() {
    // BLK-1 regression guard: a Windows cache root contains raw backslashes;
    // the report used to emit them unescaped, producing invalid JSON that
    // broke every jq/Agent consumer. Pin OCRX_MODEL_CACHE to a backslash
    // path and require the output to be a parseable JSON object.
    let cache = std::env::temp_dir().join(format!("ocrx-doctor-parse-{}", std::process::id()));
    std::fs::create_dir_all(&cache).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .args(["doctor", "--report", "json"])
        .env("OCRX_MODEL_CACHE", &cache)
        .output()
        .expect("CLI test binary should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains('\\'),
        "cache root with backslashes should appear escaped in: {stdout}"
    );
    assert!(
        ocrx_core::is_valid_json(&stdout),
        "doctor --report json must be parseable JSON, got: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&cache);
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
fn cli_skill_project_scope_installs_and_uninstalls_under_project_dir() {
    let project =
        std::env::temp_dir().join(format!("ocrx-skill-cli-project-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);

    let install = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .args(["skill", "install"])
        .env("OCRX_SKILL_SCOPE", "project")
        .env("OCRX_SKILL_PROJECT", &project)
        .output()
        .expect("CLI test binary should run");
    assert!(
        install.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let stdout = String::from_utf8_lossy(&install.stdout);
    assert!(stdout.contains("scope=project") && stdout.contains("status=installed"));
    assert!(project.join(".codex/skills/ocrx/SKILL.md").is_file());
    assert!(project.join(".claude/skills/ocrx/SKILL.md").is_file());

    let uninstall = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .args(["skill", "uninstall"])
        .env("OCRX_SKILL_SCOPE", "project")
        .env("OCRX_SKILL_PROJECT", &project)
        .output()
        .expect("CLI test binary should run");
    assert!(
        uninstall.status.success(),
        "uninstall failed: {}",
        String::from_utf8_lossy(&uninstall.stderr)
    );
    assert!(!project.join(".codex/skills/ocrx/SKILL.md").exists());
    assert!(!project.join(".claude/skills/ocrx/SKILL.md").exists());

    let _ = std::fs::remove_dir_all(&project);
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

#[cfg(all(target_os = "windows", not(feature = "vulkan")))]
#[test]
fn cli_scan_rejects_vulkan_on_cpu_only_build() {
    // W4.1 Red: a Windows CPU-only binary has `vulkan` compiled out, so an
    // explicit `--device vulkan` request must be rejected with exit code 6 and
    // exactly one JSON error object, never silently fall back to CPU.
    //
    // The scan flow validates the input PDF (which requires PDFium) before it
    // resolves the device request, so this contract needs a real pdfium.dll
    // and a real PDF fixture. Skip when the local vendor layout is absent
    // (clean CI), mirroring the PDFium probe test in ocrx-pdf.
    let pdfium_dll =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/bin/pdfium.dll");
    if !pdfium_dll.is_file() {
        eprintln!(
            "skipping vulkan rejection check: vendor pdfium.dll not present at {}",
            pdfium_dll.display()
        );
        return;
    }
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/fixtures/pdf/normal-1-page.pdf");
    let output = Command::new(env!("CARGO_BIN_EXE_ocrx"))
        .args([
            "scan",
            fixture.to_str().expect("fixture path is utf-8"),
            "--device",
            "vulkan",
            "--report",
            "json",
        ])
        .env("OCRX_PDFIUM_LIBRARY", &pdfium_dll)
        .output()
        .expect("CLI test binary should run");
    assert_eq!(output.status.code(), Some(6));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"code\":6"));
    assert!(stdout.contains("\"kind\":\"runtime\""));
}
