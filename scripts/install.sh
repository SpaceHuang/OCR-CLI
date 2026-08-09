#!/usr/bin/env bash
set -euo pipefail

PREFIX=${OCRX_INSTALL_PREFIX:-"$HOME/.local"}
ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
BIN="$ROOT_DIR/target/release/ocrx"
[[ "$(uname -s)" == "Darwin" ]] || { echo "OCR-CLI v0.1 supports macOS only" >&2; exit 2; }
[[ "$(uname -m)" == "arm64" ]] || { echo "OCR-CLI v0.1 supports macOS arm64 only" >&2; exit 2; }
if [[ -n "${OCRX_RELEASE_ARCHIVE:-}" ]]; then
  [[ -f "$OCRX_RELEASE_ARCHIVE" ]] || { echo "release archive missing" >&2; exit 4; }
  if [[ -n "${OCRX_RELEASE_SHA256:-}" ]]; then
    actual=$(shasum -a 256 "$OCRX_RELEASE_ARCHIVE" | awk '{print $1}')
    [[ "$actual" == "$OCRX_RELEASE_SHA256" ]] || { echo "release archive hash mismatch" >&2; exit 5; }
  fi
  TEMP_DIR=$(mktemp -d)
  trap 'rm -rf "$TEMP_DIR"' EXIT
  tar -xzf "$OCRX_RELEASE_ARCHIVE" -C "$TEMP_DIR"
  BIN=$(find "$TEMP_DIR" -type f -path '*/bin/ocrx' -print -quit)
  [[ -n "$BIN" && -x "$BIN" ]] || { echo "archive has no ocrx binary" >&2; exit 4; }
else
  [[ -f "$BIN" ]] || { echo "build target/release/ocrx first" >&2; exit 4; }
fi
mkdir -p "$PREFIX/bin"
install -m 755 "$BIN" "$PREFIX/bin/ocrx"
if [[ "${OCRX_INSTALL_SKILL:-0}" == "1" ]]; then
  HOME="$HOME" OCRX_SKILL_TARGET="${OCRX_SKILL_TARGET:-all}" "$PREFIX/bin/ocrx" skill install
fi
echo "installed $PREFIX/bin/ocrx"
case ":${PATH:-}:" in *":$PREFIX/bin:"*) ;; *) echo "add $PREFIX/bin to PATH" ;; esac
echo "model weights were not downloaded; run explicit model installation when configured"
