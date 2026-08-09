#!/bin/zsh
set -u

# Run this from a normal macOS Terminal, outside the Codex sandbox.
# Usage:
#   ./scripts/macos-ovisocr2-metal-smoke.zsh [ASSET_DIR] [IMAGE]

SCRIPT_DIR=${0:A:h}
REPO_DIR=${SCRIPT_DIR:h}
ASSET_DIR=${1:-${TMPDIR:-/tmp}/ocrx-ovisocr2-community}
IMAGE=${2:-}
MODEL="${ASSET_DIR}/OvisOCR2-Q4_K_M.gguf"
MMPROJ="${ASSET_DIR}/mmproj-F16.gguf"
LOG_DIR="${TMPDIR:-/tmp}/ocrx-runtime-smoke"
LOG_FILE="${LOG_DIR}/metal-$(date +%Y%m%d-%H%M%S).log"

mkdir -p "${LOG_DIR}"

die() { print -u2 "error: $*"; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "this script requires macOS"
[[ -f "${MODEL}" ]] || die "missing model: ${MODEL}"
[[ -f "${MMPROJ}" ]] || die "missing projector: ${MMPROJ}"

[[ -n "${IMAGE}" ]] || die "pass an image as the second argument"
[[ -f "${IMAGE}" ]] || die "missing image: ${IMAGE}"

print "==> host"
print "arch: $(uname -m)"
print "macOS: $(sw_vers -productVersion)"
system_profiler SPDisplaysDataType 2>/dev/null | sed -n '1,35p'

command -v cargo >/dev/null 2>&1 || die "cargo not found"
command -v python3 >/dev/null 2>&1 || print "warning: python3 not found; cargo may still run"

print "==> assets"
print "model: ${MODEL}"
print "mmproj: ${MMPROJ}"
print "image: ${IMAGE}"
print "log: ${LOG_FILE}"

print "==> runtime probe (GPU enabled)"
set +e
(
  cd "${REPO_DIR}" || exit 1
  export OCRX_RUNTIME_MODEL="${MODEL}"
  export OCRX_RUNTIME_MMPROJ="${MMPROJ}"
  export OCRX_RUNTIME_IMAGE="${IMAGE}"
  export OCRX_RUNTIME_MAX_TOKENS="${OCRX_RUNTIME_MAX_TOKENS:-16}"
  export OCRX_RUNTIME_USE_GPU=1
  cargo run -p ocrx-runtime --features ocrx-runtime/metal --bin mtmd-generate-probe --offline
) 2>&1 | tee "${LOG_FILE}"
RC=${pipestatus[1]}
set -u

print "==> exit code: ${RC}"
if (( RC == 0 )); then
  print "PASS: Metal model/projector load and generation completed"
else
  print "FAIL: see ${LOG_FILE}"
  print "Look for 'picking default device', 'command queue', and 'NullReturn'."
fi
exit ${RC}
