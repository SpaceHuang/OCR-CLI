#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
for script in package-macos-arm64.sh install.sh; do
  bash -n "$ROOT_DIR/scripts/$script"
done
grep -q "does not contain model weights" "$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q "model weights were not downloaded" "$ROOT_DIR/scripts/install.sh"
grep -q "OCRX_RELEASE_SHA256" "$ROOT_DIR/scripts/install.sh"
grep -q "OCRX_PDFIUM_LIBRARY" "$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q 'uname -s' "$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q 'arm64' "$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q 'macOS only' "$ROOT_DIR/scripts/install.sh"
grep -q 'arm64 only' "$ROOT_DIR/scripts/install.sh"
# Privacy gate: release scripts must never download or embed model weights.
# `! grep` is exempt from errexit in bash, so the negation form was a silent
# no-op; use an explicit failure branch instead.
if grep -qEi 'gguf|download.*model|model.*download' "$ROOT_DIR/scripts/package-macos-arm64.sh"; then
  echo "package-macos-arm64.sh mentions model downloads/gguf" >&2; exit 1
fi
if grep -qEi 'gguf|download.*model' "$ROOT_DIR/scripts/install.sh"; then
  echo "install.sh mentions model downloads/gguf" >&2; exit 1
fi
if grep -qEi 'gguf|download.*model' "$ROOT_DIR/scripts/install.ps1"; then
  echo "install.ps1 mentions model downloads/gguf" >&2; exit 1
fi
if grep -qEi 'gguf|download.*model|model.*download' "$ROOT_DIR/scripts/package-windows-x64.ps1"; then
  echo "package-windows-x64.ps1 mentions model downloads/gguf" >&2; exit 1
fi
# Release stage must be a standard skill root (SKILL.md beside bin/), not assets/skills nesting.
grep -q 'STAGE="$OUT_DIR/ocrx"' "$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q 'cp "$ROOT_DIR/assets/skills/ocrx/SKILL.md" "$STAGE/SKILL.md"' "$ROOT_DIR/scripts/package-macos-arm64.sh"
# --- W5.3: Windows release scripts mirror the macOS privacy/contract checks. ---
for script in package-windows-x64.ps1 install.ps1; do
  [[ -f "$ROOT_DIR/scripts/$script" ]] || { echo "missing $script" >&2; exit 1; }
done
grep -q "does not contain model weights" "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -q "Model weights are never included" "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -q "model weights were not downloaded" "$ROOT_DIR/scripts/install.ps1"
grep -q "OCRX_RELEASE_SHA256" "$ROOT_DIR/scripts/install.ps1"
grep -q "OCRX_PDFIUM_LIBRARY" "$ROOT_DIR/scripts/package-windows-x64.ps1"
# decision 4: the bundled pdfium.dll ships with its license set in the archive.
grep -qi "licenses" "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -q "requires Windows" "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -qE "requires x64|requires AMD64" "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -q "Windows only" "$ROOT_DIR/scripts/install.ps1"
grep -qE "x64 only|AMD64 only" "$ROOT_DIR/scripts/install.ps1"
# privacy: package never bundles/downloads models; install never bulk-downloads
# models — enforced by the explicit failure-branch gates near the top of this
# file, which cover these same two scripts (the former `! grep` lines here were
# a silent no-op under `set -e`, review R-4/m23).
# release stage is a standard skill root (SKILL.md beside bin/), not assets/skills nesting.
grep -Fq 'Join-Path $OutDir ' "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -q "SKILL.md" "$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -qi "SHA256SUMS" "$ROOT_DIR/scripts/package-windows-x64.ps1"
# offline rejection gate (decision 4: on-demand download with offline rejection).
grep -qi "OCRX_OFFLINE" "$ROOT_DIR/scripts/install.ps1"
grep -qi "OCRX_INSTALL_SKILL" "$ROOT_DIR/scripts/install.ps1"
# PowerShell syntax check when a PowerShell is available (mirrors `bash -n`).
PS=""
if command -v pwsh >/dev/null 2>&1; then PS=pwsh
elif command -v powershell >/dev/null 2>&1; then PS=powershell; fi
if [[ -n "$PS" ]]; then
  for script in package-windows-x64.ps1 install.ps1; do
    PSP=$(cygpath -w "$ROOT_DIR/scripts/$script" 2>/dev/null || echo "$ROOT_DIR/scripts/$script")
    "$PS" -NoProfile -Command "\$e=\$null; [void][System.Management.Automation.Language.Parser]::ParseFile(\"$PSP\", [ref]\$null, [ref]\$e); if (\$e.Count -gt 0) { exit 1 }" \
      || { echo "PowerShell syntax error in $script" >&2; exit 1; }
  done
fi
# --- P2 T1.2: cuda packaging branch (plan §5; v3.10 MAJOR-1 build-env steps). ---
# The cuda branch must mirror the vulkan gate shape (env check → build →
# ocrx-cuda.exe) and embed the T1.1-verified build environment as executable
# steps: Ninja generator, MSVC x64 init (vcvars64) or fail-loud, short
# CARGO_TARGET_DIR, and cache-clean-before-retry. The unknown-enum hard
# rejection stays.
PKG="$ROOT_DIR/scripts/package-windows-x64.ps1"
grep -q "unsupported GpuPackages entry" "$PKG"
grep -q "'cuda'" "$PKG" || { echo "package: no cuda GpuPackages branch" >&2; exit 1; }
grep -q 'CUDA_PATH' "$PKG" || { echo "package: cuda branch must gate on CUDA_PATH" >&2; exit 1; }
grep -q 'ocrx-cuda.exe' "$PKG" || { echo "package: cuda artifact must be bin/ocrx-cuda.exe" >&2; exit 1; }
grep -q 'CMAKE_GENERATOR' "$PKG" || { echo "package: cuda branch must pin CMAKE_GENERATOR=Ninja (T1.1: VS generator fails with 'No CUDA toolset found')" >&2; exit 1; }
grep -q 'Ninja' "$PKG" || { echo "package: cuda branch must reference the Ninja generator" >&2; exit 1; }
grep -q 'vcvars64.bat' "$PKG" || { echo "package: cuda branch must initialize or demand the MSVC x64 env (vcvars64.bat)" >&2; exit 1; }
grep -q 'CARGO_TARGET_DIR' "$PKG" || { echo "package: cuda branch must set/validate a short CARGO_TARGET_DIR (250-char nested-path limit)" >&2; exit 1; }
grep -qEi 'clean|Remove-Item' "$PKG" || { echo "package: cuda branch must clean the dirty CMake cache before retry" >&2; exit 1; }
# --- P2 T1.3: installer GPU variant priority chain (plan §5 T1.3). ---
# Detection stays nvcuda → vulkan-1 → cpu; the promotion becomes a chain
# cuda → vulkan → cpu with per-level launch verification and explicit
# degrade logs.
INST="$ROOT_DIR/scripts/install.ps1"
grep -q 'ocrx-cuda.exe' "$INST" || { echo "install: no cuda chain candidate" >&2; exit 1; }
grep -q 'ocrx-vulkan.exe' "$INST" || { echo "install: no vulkan chain candidate" >&2; exit 1; }
grep -q 'nvcuda.dll' "$INST" || { echo "install: nvcuda detection missing" >&2; exit 1; }
grep -q 'vulkan-1.dll' "$INST" || { echo "install: vulkan detection missing" >&2; exit 1; }
grep -q "@('ocrx-cuda.exe', 'ocrx-vulkan.exe')" "$INST" \
  || { echo "install: cuda must precede vulkan in the promotion chain" >&2; exit 1; }
grep -qEi 'failed to launch|could not start|failed launch' "$INST" \
  || { echo "install: chain levels must log explicit degrade reasons" >&2; exit 1; }
# Functional smoke of the installer chain when a PowerShell is available:
# {cuda+vulkan+cpu, vulkan+cpu, cpu-only} × {nvcuda, vulkan-only, none}.
if [[ -n "$PS" ]]; then
  "$PS" -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$ROOT_DIR/scripts/test-install-chain.ps1" 2>/dev/null || echo "$ROOT_DIR/scripts/test-install-chain.ps1")" \
    || { echo "install chain smoke matrix failed" >&2; exit 1; }
fi
# validate-skill.py needs a real Python; on Windows `python3` may be a Store stub.
PY=""
for candidate in python3 python; do
  if "$candidate" --version >/dev/null 2>&1; then PY="$candidate"; break; fi
done
[[ -n "$PY" ]] || { echo "python is required to validate SKILL.md" >&2; exit 1; }
"$PY" "$ROOT_DIR/scripts/validate-skill.py" "$ROOT_DIR/assets/skills/ocrx/SKILL.md"
echo "release scripts syntax/privacy checks passed"
