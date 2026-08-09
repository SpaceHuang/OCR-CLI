#!/bin/zsh
set -euo pipefail

# Download and verify the pinned community OvisOCR2 GGUF assets.
# Usage: ./scripts/download-community-ovisocr2.zsh [OUTPUT_DIR]

ROOT_DIR=${1:-"${TMPDIR:-/tmp}/ocrx-ovisocr2-community"}
REVISION="8a22290a0c42dbcf84739d6d4c2763f877494ae0"
BASE_URL="https://huggingface.co/Abiray/OvisOCR2-GGUF/resolve/${REVISION}"

MODEL_FILE="OvisOCR2-Q4_K_M.gguf"
MODEL_SIZE=529297280
MODEL_SHA256="4ae54c66bfe1efb2882854ff642c3d137aa082dbd0699bc9234ff39874a492a1"

MMPROJ_FILE="mmproj-F16.gguf"
MMPROJ_SIZE=204987079
MMPROJ_SHA256="7e371704e8f3c88638830e52866dfa88b2fd383155e3a0f9484e4ce037fdd42f"

mkdir -p "${ROOT_DIR}"

download() {
  local file="$1"
  local url="$2"
  echo "==> Downloading ${file} into ${ROOT_DIR}"
  curl --http1.1 -fL --retry 20 --retry-delay 10 --continue-at - \
    --output "${ROOT_DIR}/${file}" "${url}"
}

verify() {
  local file="$1"
  local expected_size="$2"
  local expected_sha256="$3"
  local path="${ROOT_DIR}/${file}"

  [[ -f "${path}" ]] || { echo "missing: ${path}" >&2; exit 1; }
  local actual_size
  if command -v stat >/dev/null 2>&1; then
    actual_size=$(stat -f '%z' "${path}" 2>/dev/null || stat -c '%s' "${path}")
  elif command -v wc >/dev/null 2>&1; then
    actual_size=$(wc -c < "${path}")
    actual_size=${actual_size//[[:space:]]/}
  else
    actual_size="${expected_size}"
    echo "warning: stat/wc unavailable; size check skipped for ${file}" >&2
  fi
  [[ "${actual_size}" == "${expected_size}" ]] || {
    echo "size mismatch: ${file}: ${actual_size} != ${expected_size}" >&2
    exit 1
  }
  local actual_sha256
  local python_bin=""
  for candidate in python3 /usr/bin/python3 /opt/homebrew/bin/python3 /usr/local/bin/python3; do
    if [[ -x "${candidate}" ]] || command -v "${candidate}" >/dev/null 2>&1; then python_bin="${candidate}"; break; fi
  done
  local openssl_bin=""
  for candidate in openssl /usr/bin/openssl /opt/homebrew/bin/openssl /usr/local/bin/openssl; do
    if [[ -x "${candidate}" ]] || command -v "${candidate}" >/dev/null 2>&1; then openssl_bin="${candidate}"; break; fi
  done
  if [[ -n "${python_bin}" ]]; then
    actual_sha256=$("${python_bin}" -c 'import hashlib,sys; h=hashlib.sha256(); f=open(sys.argv[1],"rb"); [h.update(b) for b in iter(lambda:f.read(1024*1024),b"")]; f.close(); print(h.hexdigest())' "${path}")
  elif [[ -n "${openssl_bin}" ]]; then
    actual_sha256=$("${openssl_bin}" dgst -sha256 "${path}")
    actual_sha256=${actual_sha256##*= }
  else
    echo "cannot verify ${file}: require python3 or openssl" >&2
    exit 1
  fi
  [[ "${actual_sha256}" == "${expected_sha256}" ]] || {
    echo "sha256 mismatch: ${file}: ${actual_sha256} != ${expected_sha256}" >&2
    exit 1
  }
  echo "verified ${file}: ${actual_sha256}"
}

download "${MODEL_FILE}" "${BASE_URL}/${MODEL_FILE}"
verify "${MODEL_FILE}" "${MODEL_SIZE}" "${MODEL_SHA256}"

download "${MMPROJ_FILE}" "${BASE_URL}/${MMPROJ_FILE}"
verify "${MMPROJ_FILE}" "${MMPROJ_SIZE}" "${MMPROJ_SHA256}"

echo "All community OvisOCR2 assets verified in: ${ROOT_DIR}"
echo "revision: ${REVISION}"
