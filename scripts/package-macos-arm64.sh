#!/usr/bin/env bash
set -euo pipefail

# Build a reproducible macOS arm64 Skill+CLI archive.
# Layout matches the standard Skill directory contract:
#   ocrx/
#     SKILL.md
#     bin/ocrx
#     lib/libpdfium.dylib
#     INSTALL.txt
# Model weights are never included.
ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
OUT_DIR=${1:-"$ROOT_DIR/dist"}
VERSION=${OCRX_VERSION:-0.1.0}
ARCHIVE="$OUT_DIR/ocrx-${VERSION}-macos-arm64.tar.gz"
# Top-level directory name must be the skill id so agents can drop it into skills/.
STAGE="$OUT_DIR/ocrx"
LEGACY_STAGE="$OUT_DIR/ocrx-${VERSION}-macos-arm64"

[[ "$(uname -s)" == "Darwin" ]] || { echo "requires macOS" >&2; exit 2; }
[[ "$(uname -m)" == "arm64" ]] || { echo "requires arm64" >&2; exit 2; }
rm -rf "$STAGE" "$LEGACY_STAGE"
mkdir -p "$STAGE/bin"
(cd "$ROOT_DIR" && cargo build --release -p ocrx)
cp "$ROOT_DIR/target/release/ocrx" "$STAGE/bin/ocrx"
[[ -x "$STAGE/bin/ocrx" ]] || { echo "release binary missing" >&2; exit 4; }
if [[ -n "${OCRX_PDFIUM_LIBRARY:-}" ]]; then
  [[ -f "$OCRX_PDFIUM_LIBRARY" ]] || { echo "PDFium dylib missing" >&2; exit 4; }
  file "$OCRX_PDFIUM_LIBRARY" | grep -q 'arm64' || { echo "PDFium dylib is not arm64" >&2; exit 4; }
  mkdir -p "$STAGE/lib"
  cp "$OCRX_PDFIUM_LIBRARY" "$STAGE/lib/libpdfium.dylib"
else
  echo "set OCRX_PDFIUM_LIBRARY to package the required arm64 PDFium dylib" >&2
  exit 4
fi
cp "$ROOT_DIR/assets/skills/ocrx/SKILL.md" "$STAGE/SKILL.md"
[[ -f "$STAGE/SKILL.md" ]] || { echo "SKILL.md missing at skill root" >&2; exit 4; }
# Ship the full license set with the archive (C4 / requirement 5.1: the
# macOS archive previously shipped WITHOUT licenses - the npm platform
# package must be same-source with the Release archive, so both carry the
# identical set). Mirrors the Windows packager: the project Apache-2.0 text,
# the third-party notices, the pinned llama.cpp MIT license, and the PDFium
# license set from the fetched baseline (licenses/ sits next to the dylib).
for RepoFile in LICENSE THIRD_PARTY_NOTICES.md; do
  [[ -f "$ROOT_DIR/$RepoFile" ]] || { echo "license file missing in repo: $RepoFile" >&2; exit 4; }
  cp "$ROOT_DIR/$RepoFile" "$STAGE/$RepoFile"
done
[[ -f "$ROOT_DIR/assets/licenses/llama.cpp/LICENSE" ]] || { echo "llama.cpp license missing under assets/licenses" >&2; exit 4; }
mkdir -p "$STAGE/licenses/llama.cpp"
cp "$ROOT_DIR/assets/licenses/llama.cpp/LICENSE" "$STAGE/licenses/llama.cpp/LICENSE"
if [[ -n "${OCRX_PDFIUM_LIBRARY:-}" ]]; then
  PdfiumRoot=$(dirname "$(dirname "$OCRX_PDFIUM_LIBRARY")")
  if [[ -d "$PdfiumRoot/licenses" ]]; then
    mkdir -p "$STAGE/licenses/pdfium"
    cp -R "$PdfiumRoot/licenses/." "$STAGE/licenses/pdfium/"
  fi
  [[ -d "$STAGE/licenses/pdfium" ]] || { echo "PDFium license set missing next to the dylib" >&2; exit 4; }
fi
cat > "$STAGE/INSTALL.txt" <<'EOF'
This directory is a standard Agent Skill root (SKILL.md) plus the OCR-CLI binary.

Layout:
  ocrx/
    SKILL.md
    bin/ocrx
    lib/libpdfium.dylib
    INSTALL.txt
    LICENSE, THIRD_PARTY_NOTICES.md, licenses/

It does not contain model weights. Model installation is explicit.

Agent / drop-in Skill install:
  Copy or symlink this `ocrx/` directory into the host skills folder, e.g.
    ~/.codex/skills/ocrx
    ~/.claude/skills/ocrx
    ~/.cursor/skills/ocrx

CLI install (PATH):
  Put bin/ocrx on PATH, or run: OCRX_RELEASE_ARCHIVE=... bash scripts/install.sh
  Optional: OCRX_INSTALL_SKILL=1 to also write host Skill files via `ocrx skill install`.

When invoking the bundled binary from this Skill directory, set:
  OCRX_PDFIUM_LIBRARY=<absolute-path-to-this-dir>/lib/libpdfium.dylib
EOF
(cd "$STAGE" && otool -L bin/ocrx >/dev/null)
# Hash manifest mirrors the Windows packager's `sha256sum -c` layout and
# excludes SHA256SUMS itself. The sums are written to a temp file outside
# the stage first: redirecting straight into "$STAGE/SHA256SUMS" creates
# the file before `find` runs, which books a self-referential entry
# holding the hash of an empty file.
SUMS_TMP="$OUT_DIR/.SHA256SUMS.tmp"
(cd "$STAGE" && find . -type f -print0 | sort -z | xargs -0 shasum -a 256 > "$SUMS_TMP")
mv "$SUMS_TMP" "$STAGE/SHA256SUMS"
rm -f "$ARCHIVE"
tar -czf "$ARCHIVE" -C "$OUT_DIR" ocrx
echo "$ARCHIVE"
