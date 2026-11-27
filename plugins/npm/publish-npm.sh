#!/usr/bin/env bash
# publish-npm (review BLOCK-1 + BLOCK-9): publishes the three packages from
# EXPLICIT tarball paths (never guessed scoped filenames) and, on re-runs,
# verifies the registry copy is byte-identical to this build before skipping.
# Any identity mismatch or registry error FAILS the run — a silent mix of
# differently-built versions can never ship.
#
# Usage:
#   bash publish-npm.sh <tag-version> <plugin.tgz> <win32.tgz> <darwin.tgz>
# Env:
#   NODE_AUTH_TOKEN   npm auth (mapped by the workflow's environment)
#   PATH              may carry a fake npm in tests (behavioral coverage)
set -euo pipefail

VERSION="${1:?tag version required}"
PLUGIN_TGZ="${2:?plugin tarball required}"
WIN_TGZ="${3:?win32 tarball required}"
MAC_TGZ="${4:?darwin tarball required}"

fail() { echo "publish-npm: $*" >&2; exit 1; }

for tarball in "$PLUGIN_TGZ" "$WIN_TGZ" "$MAC_TGZ"; do
  [[ -f "$tarball" ]] || fail "tarball missing: $tarball"
done

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# classify <pkg> <local-tgz>: echoes "publish" when the registry has no such
# version (E404 only); echoes "skip" when the registry version is
# byte-identical to this build; FAILS on any mismatch or registry error.
classify() {
  local pkg="$1" local_tgz="$2"
  local err_file
  err_file=$(mktemp)
  if npm view "$pkg@$VERSION" version > /dev/null 2>"$err_file"; then
    rm -f "$err_file"
    # Published already — the ONLY legal skip is byte-identical content.
    local probe_dir downloaded
    probe_dir=$(mktemp -d)
    if ! (cd "$probe_dir" && npm pack "$pkg@$VERSION" > /dev/null 2>&1); then
      rm -rf "$probe_dir"
      fail "cannot download the published $pkg@$VERSION for the identity check"
    fi
    downloaded=$(ls "$probe_dir"/*.tgz 2>/dev/null | head -1)
    [[ -n "$downloaded" ]] || { rm -rf "$probe_dir"; fail "registry pack of $pkg@$VERSION produced no tarball"; }
    local local_sum remote_sum
    local_sum=$(sha256_of "$local_tgz")
    remote_sum=$(sha256_of "$downloaded")
    if [[ "$local_sum" != "$remote_sum" ]]; then
      rm -rf "$probe_dir"
      fail "IDENTITY MISMATCH for $pkg@$VERSION (registry sha256 $remote_sum != local $local_sum) — refusing to mix differently-built versions; unpublish within the 72h window or bump the version"
    fi
    rm -rf "$probe_dir"
    echo "skip"
  else
    if grep -q 'E404' "$err_file"; then
      rm -f "$err_file"
      echo "publish"
    else
      local detail
      detail=$(tail -3 "$err_file" | tr '\n' ' ')
      rm -f "$err_file"
      fail "npm view $pkg@$VERSION failed (network/auth errors must not be treated as missing): $detail"
    fi
  fi
}

PLUGIN_STATE=$(classify "@ocrx/dsh" "$PLUGIN_TGZ")
WIN_STATE=$(classify "@ocrx/ocrx-win32-x64" "$WIN_TGZ")
MAC_STATE=$(classify "@ocrx/ocrx-darwin-arm64" "$MAC_TGZ")
echo "classify: plugin=$PLUGIN_STATE win=$WIN_STATE mac=$MAC_STATE"

if [[ "$PLUGIN_STATE" == "skip" && "$WIN_STATE" == "skip" && "$MAC_STATE" == "skip" ]]; then
  echo "all three versions already published and identical — idempotent re-run, nothing to do"
  exit 0
fi

publish() {
  local tarball="$1" pkg="$2"
  npm publish --access public "$tarball"
  echo "published $pkg@$VERSION from $tarball"
}

# Order is frozen (plan stage 8): platform packages FIRST — the plugin's
# optionalDependencies pin their exact versions, which must already exist.
if [[ "$WIN_STATE" == "publish" ]]; then publish "$WIN_TGZ" "@ocrx/ocrx-win32-x64"; fi
if [[ "$MAC_STATE" == "publish" ]]; then publish "$MAC_TGZ" "@ocrx/ocrx-darwin-arm64"; fi
if [[ "$PLUGIN_STATE" == "publish" ]]; then publish "$PLUGIN_TGZ" "@ocrx/dsh"; fi

echo "publish complete for $VERSION"
