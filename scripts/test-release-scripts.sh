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
# privacy: package never bundles/downloads models; install never bulk-downloads models.
! grep -qEi "gguf|download.*model|model.*download" "$ROOT_DIR/scripts/package-windows-x64.ps1"
! grep -qEi "gguf|download.*model" "$ROOT_DIR/scripts/install.ps1"
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
# validate-skill.py needs a real Python; on Windows `python3` may be a Store stub.
PY=""
for candidate in python3 python; do
  if "$candidate" --version >/dev/null 2>&1; then PY="$candidate"; break; fi
done
[[ -n "$PY" ]] || { echo "python is required to validate SKILL.md" >&2; exit 1; }
"$PY" "$ROOT_DIR/scripts/validate-skill.py" "$ROOT_DIR/assets/skills/ocrx/SKILL.md"
echo "release scripts syntax/privacy checks passed"
