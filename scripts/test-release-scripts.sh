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
! grep -qE 'gguf|download.*model|model.*download' "$ROOT_DIR/scripts/package-macos-arm64.sh"
! grep -qE 'gguf|download.*model' "$ROOT_DIR/scripts/install.sh"
# Release stage must be a standard skill root (SKILL.md beside bin/), not assets/skills nesting.
grep -q 'STAGE="$OUT_DIR/ocrx"' "$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q 'cp "$ROOT_DIR/assets/skills/ocrx/SKILL.md" "$STAGE/SKILL.md"' "$ROOT_DIR/scripts/package-macos-arm64.sh"
python3 "$ROOT_DIR/scripts/validate-skill.py" "$ROOT_DIR/assets/skills/ocrx/SKILL.md"
echo "release scripts syntax/privacy checks passed"
