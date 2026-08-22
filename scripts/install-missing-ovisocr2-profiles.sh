#!/bin/sh
# Download the remaining pinned OvisOCR2 GGUF profiles with visible progress.
# Safe to rerun: incomplete *.download files are resumed, while completed files
# are verified before being reused.
set -eu

revision='8a22290a0c42dbcf84739d6d4c2763f877494ae0'
cache_root="${OCRX_MODEL_CACHE:-.ocrx-model-cache}"
model_root="$cache_root/ovisocr2"
projector_source="$model_root/compact/mmproj.gguf"
projector_sha256='7e371704e8f3c88638830e52866dfa88b2fd383155e3a0f9484e4ce037fdd42f'

sha256() {
    shasum -a 256 "$1" | awk '{print $1}'
}

require_hash() {
    actual="$(sha256 "$1")"
    if [ "$actual" != "$2" ]; then
        echo "SHA-256 mismatch: $1" >&2
        echo "expected: $2" >&2
        echo "actual:   $actual" >&2
        exit 1
    fi
}

download() {
    url="$1"
    destination="$2"
    if [ -s "$destination" ]; then
        curl --fail --location --continue-at - --retry 8 --retry-all-errors \
            --progress-bar --output "$destination" "$url"
    else
        curl --fail --location --retry 8 --retry-all-errors \
            --progress-bar --output "$destination" "$url"
    fi
}

install_profile() {
    profile="$1"
    filename="$2"
    expected_model_sha256="$3"
    directory="$model_root/$profile"
    model="$directory/model.gguf"
    partial="$directory/model.gguf.download"
    url="https://huggingface.co/Abiray/OvisOCR2-GGUF/resolve/$revision/$filename"

    mkdir -p "$directory"
    if [ -f "$model" ]; then
        require_hash "$model" "$expected_model_sha256"
        echo "$profile model already verified; skipping download."
    else
        echo "Downloading $profile ($filename)..."
        download "$url" "$partial"
        require_hash "$partial" "$expected_model_sha256"
        mv "$partial" "$model"
        echo "$profile model download and SHA-256 verification passed."
    fi

    if [ -f "$directory/mmproj.gguf" ]; then
        require_hash "$directory/mmproj.gguf" "$projector_sha256"
    else
        cp "$projector_source" "$directory/mmproj.gguf.download"
        require_hash "$directory/mmproj.gguf.download" "$projector_sha256"
        mv "$directory/mmproj.gguf.download" "$directory/mmproj.gguf"
    fi
    echo "$profile profile installed and verified."
}

if [ ! -f "$projector_source" ]; then
    echo "Missing compact projector: $projector_source" >&2
    echo "Install or repair compact first: target/release/ocrx model install compact --non-interactive" >&2
    exit 1
fi
require_hash "$projector_source" "$projector_sha256"

install_profile balanced OvisOCR2-Q5_K_M.gguf \
    e17744e367adb891cc9c82f4255408cf46a00efa1b20a83f8187d5bd662ce89a
install_profile quality OvisOCR2-Q8_0.gguf \
    3fba6d94312e550575a92d55cffb8d75997bf68a9f133c92ab6ad7f0bf2bc93e
install_profile reference OvisOCR2-F16.gguf \
    bd8f2c62c930d74590abcb5f65f79e898672e458d6044e8b2e885daea948592e

printf '%s\n' 'ovisocr2-missing-profiles-install=PASS'
