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
if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
  # Install a tiny archive through the real installer and assert the bundled
  # PDFium survives extraction after the temporary directory is removed.
  INSTALL_SMOKE=$(mktemp -d)
  trap 'rm -rf "$INSTALL_SMOKE"' EXIT
  mkdir -p "$INSTALL_SMOKE/archive/ocrx/bin" "$INSTALL_SMOKE/archive/ocrx/lib"
  printf '#!/bin/sh\nexit 0\n' > "$INSTALL_SMOKE/archive/ocrx/bin/ocrx"
  chmod +x "$INSTALL_SMOKE/archive/ocrx/bin/ocrx"
  printf 'pdfium sentinel\n' > "$INSTALL_SMOKE/archive/ocrx/lib/libpdfium.dylib"
  tar -czf "$INSTALL_SMOKE/ocrx.tar.gz" -C "$INSTALL_SMOKE/archive" ocrx
  OCRX_INSTALL_PREFIX="$INSTALL_SMOKE/prefix" \
    OCRX_RELEASE_ARCHIVE="$INSTALL_SMOKE/ocrx.tar.gz" \
    HOME="$INSTALL_SMOKE/home" bash "$ROOT_DIR/scripts/install.sh" >/dev/null
  cmp "$INSTALL_SMOKE/archive/ocrx/lib/libpdfium.dylib" \
    "$INSTALL_SMOKE/prefix/lib/libpdfium.dylib"
  test -x "$INSTALL_SMOKE/prefix/bin/ocrx"
fi
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
# --- dsh-plugin-v0.1 stage 8: C4 npm pipeline + workflow structure --------
node --version >/dev/null 2>&1 || { echo "node is required for the npm pipeline checks" >&2; exit 1; }

# macOS archive must carry the full license set (C4, requirement §5.1: the
# npm platform package is same-source with the Release archive).
MACOS_PKG="$ROOT_DIR/scripts/package-macos-arm64.sh"
grep -q 'THIRD_PARTY_NOTICES.md' "$MACOS_PKG" || { echo "macos packager: third-party notices missing" >&2; exit 1; }
grep -q 'licenses/llama.cpp' "$MACOS_PKG" || { echo "macos packager: llama.cpp license missing" >&2; exit 1; }
grep -q 'licenses/pdfium' "$MACOS_PKG" || { echo "macos packager: pdfium license set missing" >&2; exit 1; }

# Platform-package builder: syntax + fixture self-test (exact file set,
# SHA256SUMS same-source proof, size cap, real npm pack, no build scripts).
NPM_BUILDER="$ROOT_DIR/plugins/npm/build-platform-packages.mjs"
[[ -f "$NPM_BUILDER" ]] || { echo "npm platform builder missing" >&2; exit 1; }
node --check "$NPM_BUILDER"
# Node children cannot resolve bash's POSIX temp paths on Windows; create
# the fixture root through node so every handed-out path is a real one.
PACK_FIXTURE=$(node -e "console.log(require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(),'ocrx-pack-')))")
cleanup_trap() { rm -rf "$PACK_FIXTURE"; }
trap cleanup_trap EXIT
mkdir -p "$PACK_FIXTURE/stage/bin" "$PACK_FIXTURE/stage/licenses/pdfium" \
  "$PACK_FIXTURE/stage/licenses/llama.cpp" "$PACK_FIXTURE/stage/licenses/ms-vc-runtime"
printf 'fake ocrx binary' > "$PACK_FIXTURE/stage/bin/ocrx.exe"
printf 'pdfium' > "$PACK_FIXTURE/stage/bin/pdfium.dll"
printf 'crt1' > "$PACK_FIXTURE/stage/bin/msvcp140.dll"
printf 'crt2' > "$PACK_FIXTURE/stage/bin/vcruntime140.dll"
printf 'crt3' > "$PACK_FIXTURE/stage/bin/vcruntime140_1.dll"
printf 'license' > "$PACK_FIXTURE/stage/LICENSE"
printf 'notices' > "$PACK_FIXTURE/stage/THIRD_PARTY_NOTICES.md"
printf 'pdfium license' > "$PACK_FIXTURE/stage/licenses/pdfium/pdfium.txt"
printf 'llama license' > "$PACK_FIXTURE/stage/licenses/llama.cpp/LICENSE"
printf 'vc notice' > "$PACK_FIXTURE/stage/licenses/ms-vc-runtime/README.txt"
( cd "$PACK_FIXTURE/stage" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS )
TGZ=$(node "$NPM_BUILDER" --platform win32-x64 --stage "$PACK_FIXTURE/stage" \
  --version 0.0.0-test --out-dir "$PACK_FIXTURE/out" | tail -1)
[[ -f "$TGZ" ]] || { echo "npm pack self-test produced no tarball" >&2; exit 1; }
# GNU tar reads `C:` as a remote host; hand it a POSIX spelling when one is
# available (Git Bash / MSYS), otherwise the native path is already fine.
TGZ_POSIX=$(cygpath -u "$TGZ" 2>/dev/null || echo "$TGZ")
[[ -f "$TGZ_POSIX" ]] || TGZ_POSIX="$TGZ"
PLATFORM_ENTRIES=$(tar -tzf "$TGZ_POSIX")
echo "$PLATFORM_ENTRIES" | grep -q 'package/bin/ocrx.exe' || { echo "tarball misses the binary" >&2; exit 1; }
echo "$PLATFORM_ENTRIES" | grep -q 'package/bin/msvcp140.dll' || { echo "tarball misses a CRT dll" >&2; exit 1; }
echo "$PLATFORM_ENTRIES" | grep -q 'package/licenses/ms-vc-runtime/README.txt' || { echo "tarball misses the vc license" >&2; exit 1; }
tar -xOf "$TGZ_POSIX" package/package.json > "$PACK_FIXTURE/pkgmanifest.json"
grep -q '"access": "public"' "$PACK_FIXTURE/pkgmanifest.json" || { echo "platform package must publish public" >&2; exit 1; }
if grep -qE 'postinstall|preinstall|"prepare"' "$PACK_FIXTURE/pkgmanifest.json"; then
  echo "platform package must not declare build scripts" >&2; exit 1
fi
# Negative face: a tampered stage file must break the same-source proof.
printf 'tampered' >> "$PACK_FIXTURE/stage/LICENSE"
if node "$NPM_BUILDER" --platform win32-x64 --stage "$PACK_FIXTURE/stage" \
     --version 0.0.0-test --out-dir "$PACK_FIXTURE/out2" >/dev/null 2>&1; then
  echo "builder accepted a tampered stage (SHA256SUMS proof broken)" >&2; exit 1
fi
trap - EXIT
cleanup_trap || true

# Templates: exact package names (D-1) with public access.
grep -q '"@ocrx/ocrx-win32-x64"' "$ROOT_DIR/plugins/npm/package-win32-x64.json"
grep -q '"@ocrx/ocrx-darwin-arm64"' "$ROOT_DIR/plugins/npm/package-darwin-arm64.json"
grep -q '"access": "public"' "$ROOT_DIR/plugins/npm/package-win32-x64.json"
grep -q '"access": "public"' "$ROOT_DIR/plugins/npm/package-darwin-arm64.json"

# release.yml structure: per-platform npm artifacts (no runner claims the
# other platform's package), publish-npm shape, auth config, preflights
# before any publish, and BOTH first-run and re-run branches.
WF="$ROOT_DIR/.github/workflows/release.yml"
grep -q 'name: npm-ocrx-win32-x64' "$WF" || { echo "release.yml: windows npm artifact name missing" >&2; exit 1; }
grep -q 'name: npm-ocrx-darwin-arm64' "$WF" || { echo "release.yml: macos npm artifact name missing" >&2; exit 1; }
grep -q 'publish-npm:' "$WF" || { echo "release.yml: publish-npm job missing" >&2; exit 1; }
grep -q 'needs: \[package, release\]' "$WF" || { echo "release.yml: publish-npm must need both legs" >&2; exit 1; }
grep -q 'environment: npm' "$WF" || { echo "release.yml: npm environment gate missing" >&2; exit 1; }
grep -q 'registry-url: https://registry.npmjs.org' "$WF" || { echo "release.yml: registry-url auth config missing" >&2; exit 1; }
grep -q 'NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}' "$WF" || { echo "release.yml: NODE_AUTH_TOKEN mapping missing" >&2; exit 1; }
grep -q 'npm whoami' "$WF" || { echo "release.yml: authentication preflight missing" >&2; exit 1; }
grep -q 'does not point at the built commit' "$WF" || { echo "release.yml: tag presence/commit assert missing" >&2; exit 1; }
grep -q 'optionalDependencies do not pin' "$WF" || { echo "release.yml: plugin exact-pin assert missing" >&2; exit 1; }
# BLOCK-1: scoped tarball names carry NO '@' — pin the contract with a real
# npm pack probe and require the workflow globs to spell it exactly.
NAME_PROBE=$(mktemp -d)
mkdir -p "$NAME_PROBE/pkg"
printf '{"name":"@ocrx/dsh","version":"9.9.9-probe"}' > "$NAME_PROBE/pkg/package.json"
NPM_CLI="$(dirname "$(command -v node)")/node_modules/npm/bin/npm-cli.js"
PACKED_NAME=$(cd "$NAME_PROBE/pkg" && node "$NPM_CLI" pack --dry-run --json |
  grep -m1 -o '"filename": *"[^"]*"' | sed 's/.*: *"//; s/"$//')
[[ "$PACKED_NAME" == "ocrx-dsh-9.9.9-probe.tgz" ]] || { echo "npm pack name contract changed: got $PACKED_NAME" >&2; exit 1; }
rm -rf "$NAME_PROBE"
grep -q 'path: dist/npm/ocrx-ocrx-win32-x64-\*.tgz' "$WF" || { echo "release.yml: windows upload glob must drop the '@' (scoped tarball spelling)" >&2; exit 1; }
grep -q 'path: dist/npm/ocrx-ocrx-darwin-arm64-\*.tgz' "$WF" || { echo "release.yml: macos upload glob must drop the '@'" >&2; exit 1; }
if grep -q 'dist/npm/@ocrx-' "$WF"; then
  echo "release.yml still uses the wrong scoped glob spelling (@ocrx-)" >&2; exit 1
fi
grep -q 'ocrx-ocrx-win32-x64-"$TAG_VERSION".tgz' "$WF" || { echo "release.yml: preflight tarball path spelling wrong" >&2; exit 1; }
# BLOCK-2: publish-npm packs via pack-plugin.mjs after a frozen-lockfile install.
grep -q 'pnpm install --frozen-lockfile' "$WF" || { echo "release.yml: publish-npm must install the toolchain before packing" >&2; exit 1; }
grep -q 'pack-plugin.mjs' "$WF" || { echo "release.yml: publish-npm must pack via pack-plugin.mjs (content assertions)" >&2; exit 1; }
# BLOCK-9: publishing goes through the behavioral script (identity checks).
grep -q 'publish-npm.sh' "$WF" || { echo "release.yml: publish must run via plugins/npm/publish-npm.sh" >&2; exit 1; }
PUB="$ROOT_DIR/plugins/npm/publish-npm.sh"
bash -n "$PUB" || { echo "publish-npm.sh syntax error" >&2; exit 1; }
grep -q 'IDENTITY MISMATCH' "$PUB" || { echo "publish-npm.sh: registry identity check missing" >&2; exit 1; }
grep -q 'must not be treated as missing' "$PUB" || { echo "publish-npm.sh: registry-error classification missing" >&2; exit 1; }
grep -q 'all three versions already published and identical' "$PUB" || { echo "publish-npm.sh: idempotent re-run branch missing" >&2; exit 1; }
# Order: the workflow's auth preflight precedes the pack/publish steps.
WHOAMI_LINE=$(grep -n 'npm whoami' "$WF" | head -1 | cut -d: -f1)
PACK_LINE=$(grep -n 'pack-plugin.mjs' "$WF" | head -1 | cut -d: -f1)
[[ -n "$WHOAMI_LINE" && -n "$PACK_LINE" ]] || { echo "release.yml: preflight/publish markers missing" >&2; exit 1; }
[[ "$WHOAMI_LINE" -lt "$PACK_LINE" ]] || { echo "release.yml: auth preflight must precede packing" >&2; exit 1; }

# BLOCK-2 behavioral: pack-plugin.mjs on THIS checkout (a dirty one, but the
# script stages and compiles itself — the tarball must carry lib + LICENSE).
# Node cannot resolve MSYS `/d/...` spellings; hand it Windows mixed paths.
ROOT_WIN=$(cygpath -m "$ROOT_DIR" 2>/dev/null || echo "$ROOT_DIR")
PLUGIN_TGZ_TEST=$(node "$ROOT_DIR/plugins/npm/pack-plugin.mjs"   --plugin-dir "$ROOT_WIN/plugins/ocrx-dsh"   --repo-root "$ROOT_WIN"   --version 0.0.0-test   --out-dir "$ROOT_WIN/plugins/npm/pack-probe" | tail -1)
[[ -f "$PLUGIN_TGZ_TEST" ]] || { echo "pack-plugin self-test produced no tarball" >&2; exit 1; }
# GNU tar reads `C:` as a remote host — same POSIX spelling as above.
PLUGIN_TGZ_POSIX=$(cygpath -u "$PLUGIN_TGZ_TEST" 2>/dev/null || echo "$PLUGIN_TGZ_TEST")
[[ -f "$PLUGIN_TGZ_POSIX" ]] || PLUGIN_TGZ_POSIX="$PLUGIN_TGZ_TEST"
# tar | grep -q under pipefail kills tar via SIGPIPE (exit 141 = silent
# set -e failure even when the entry EXISTS); capture the listing instead.
PLUGIN_ENTRIES=$(tar -tzf "$PLUGIN_TGZ_POSIX")
echo "$PLUGIN_ENTRIES" | grep -q 'package/lib/index.js' || { echo "plugin tarball misses the entry point; entries:" >&2; echo "$PLUGIN_ENTRIES" | head -20 >&2; exit 1; }
echo "$PLUGIN_ENTRIES" | grep -q 'package/lib/tools/scan.js' || { echo "plugin tarball misses compiled tool modules" >&2; exit 1; }
echo "$PLUGIN_ENTRIES" | grep -q 'package/LICENSE' || { echo "plugin tarball misses the repo LICENSE" >&2; exit 1; }
echo "$PLUGIN_ENTRIES" | grep -q 'package/skills/ocrx/SKILL.md' || { echo "plugin tarball misses the skill" >&2; exit 1; }
if echo "$PLUGIN_ENTRIES" | grep -q 'package/lib/index.d.ts'; then
  : # declarations are fine to ship
fi
PLUGIN_MANIFEST=$(tar -xOf "$PLUGIN_TGZ_POSIX" package/package.json)
if echo "$PLUGIN_MANIFEST" | grep -qE 'postinstall|preinstall|"prepare"'; then
  echo "plugin tarball declares build scripts" >&2; exit 1
fi
PROBE_DIR=$(dirname "$PLUGIN_TGZ_TEST")
rm -rf "$PROBE_DIR"
rm -rf "$ROOT_DIR/plugins/npm/pack-probe" 2>/dev/null || true

# Plugin manifest: engines window, bundle patch structure (nested dsh key),
# platforms, exact optionalDependencies pins, public access, no build scripts.
node "$ROOT_DIR/plugins/npm/check-plugin-manifest.mjs" "$ROOT_DIR/plugins/ocrx-dsh/package.json"

# Runbook exists and documents the first-run/re-run promotion split.
RUNBOOK="$ROOT_DIR/docs/develop/dsh-plugin-release-runbook.md"
[[ -f "$RUNBOOK" ]] || { echo "release runbook missing" >&2; exit 1; }
grep -q '首跑/重跑' "$RUNBOOK" || { echo "runbook: first-run/re-run split missing" >&2; exit 1; }
grep -q 'platform 包先于插件包\|平台包先于插件包' "$RUNBOOK" || { echo "runbook: publish order missing" >&2; exit 1; }
echo "release scripts syntax/privacy checks passed"

# BLOCK-9 behavioral: publish-npm.sh against a FAKE npm — covers first-run
# (all E404 → publish all, platforms before plugin), identical re-run (skip
# everything), identity mismatch (FAIL), and registry error (FAIL).
PUB_PROBE=$(mktemp -d)
PUB_BIN="$PUB_PROBE/bin"
mkdir -p "$PUB_BIN"
# The fake npm: scripted via $FAKE_STATE file (a JSON-ish list of versions).
cat > "$PUB_BIN/npm" << 'FAKE_NPM'
#!/usr/bin/env bash
STATE_FILE="${FAKE_STATE:?fake state file required}"
LOG_FILE="${FAKE_LOG:?fake log file required}"
echo "npm $*" >> "$LOG_FILE"
case "$1" in
  view)
    pkg_spec="$2"
    version="${pkg_spec##*@}"
    name="${pkg_spec%@*}"
    if grep -q "\"$name@$version\"" "$STATE_FILE"; then
      echo "$version"
      exit 0
    fi
    echo "npm error code E404" >&2
    exit 1
    ;;
  pack)
    # Download face: copy the LOCAL tarball of the same package name from
    # $FAKE_LOCAL_DIR (the identity check compares bytes). Scoped names map
    # to tarball names by dropping `@` and joining scope/name with `-`
    # (exactly npm's own spelling).
    pkg_spec="$2"
    name="${pkg_spec%@*}"
    local_name="${name#@}"
    local_name="${local_name//\//-}"
    local_match=$(ls "$FAKE_LOCAL_DIR" | grep "^${local_name}-" | head -1)
    cp "$FAKE_LOCAL_DIR/$local_match" "$PWD/${local_match}"
    exit 0
    ;;
  publish)
    # `npm publish --access public <tarball>`: the tarball is the LAST arg.
    tarball="${*: -1}"
    base=$(basename "$tarball")
    version="${base##*-}"; version="${version%.tgz}"
    name_part="${base%-$version.tgz}"
    # @ocrx/dsh packs as ocrx-dsh; store the scoped spelling for grep.
    case "$name_part" in
      ocrx-dsh) scoped="@ocrx/dsh" ;;
      ocrx-ocrx-win32-x64) scoped="@ocrx/ocrx-win32-x64" ;;
      ocrx-ocrx-darwin-arm64) scoped="@ocrx/ocrx-darwin-arm64" ;;
    esac
    echo "\"$scoped@$version\"" >> "$STATE_FILE"
    cp "$tarball" "$FAKE_PUBLISHED_DIR/$base"
    exit 0
    ;;
esac
echo "fake npm: unsupported call $*" >&2
exit 1
FAKE_NPM
chmod +x "$PUB_BIN/npm"
make_tarballs() {
  local dir="$1"
  mkdir -p "$dir"
  for pkg in ocrx-dsh ocrx-ocrx-win32-x64 ocrx-ocrx-darwin-arm64; do
    printf 'tarball %s v1' "$pkg" > "$dir/$pkg-1.2.3.tgz"
  done
}
run_publish() {
  local state="$1" local_dir="$2" published_dir="$3" local_tarballs="$4"
  mkdir -p "$published_dir"
  FAKE_STATE="$state" FAKE_LOG="$PUB_PROBE/calls.log" \
    FAKE_LOCAL_DIR="$local_tarballs" FAKE_PUBLISHED_DIR="$published_dir" \
    PATH="$PUB_BIN:$PATH" bash "$PUB" "1.2.3" \
    "$local_tarballs/ocrx-dsh-1.2.3.tgz" \
    "$local_tarballs/ocrx-ocrx-win32-x64-1.2.3.tgz" \
    "$local_tarballs/ocrx-ocrx-darwin-arm64-1.2.3.tgz"
}

# a) First run: registry empty → publish all three, platforms before plugin.
LOCAL_A="$PUB_PROBE/local-a"; make_tarballs "$LOCAL_A"
PUBLISHED_A="$PUB_PROBE/published-a"
: > "$PUB_PROBE/state-a"
run_publish "$PUB_PROBE/state-a" "$LOCAL_A" "$PUBLISHED_A" "$LOCAL_A" > /dev/null
test -f "$PUBLISHED_A/ocrx-ocrx-win32-x64-1.2.3.tgz" || { echo "fake publish: win tarball missing" >&2; exit 1; }
ORDER=$(grep -n 'publish' "$PUB_PROBE/calls.log" | grep -v view | cut -d: -f1 | tr '\n' ' ')
WIN_POS=$(grep -n 'publish --access public .*win32' "$PUB_PROBE/calls.log" | cut -d: -f1)
PLUGIN_POS=$(grep -n 'publish --access public .*ocrx-dsh-1.2.3' "$PUB_PROBE/calls.log" | cut -d: -f1)
[[ -n "$WIN_POS" && -n "$PLUGIN_POS" && "$WIN_POS" -lt "$PLUGIN_POS" ]] || { echo "publish order violated (platforms must precede the plugin)" >&2; exit 1; }

# b) Identical re-run → skip everything, no publish calls.
: > "$PUB_PROBE/calls.log"
RE_RUN_OUTPUT=$(run_publish "$PUB_PROBE/state-a" "$LOCAL_A" "$PUB_PROBE/published-b" "$LOCAL_A")
echo "$RE_RUN_OUTPUT" | grep -q 'already published and identical' || { echo "identical re-run must be idempotent; got:" >&2; echo "$RE_RUN_OUTPUT" >&2; exit 1; }
if grep -q 'publish --access' "$PUB_PROBE/calls.log"; then
  echo "identical re-run must not publish" >&2; exit 1
fi

# c) Identity mismatch → FAIL: the registry serves the ORIGINAL bytes
# (PUBLISHED_A) while the local tarball was tampered with.
LOCAL_C="$PUB_PROBE/local-c"; make_tarballs "$LOCAL_C"
printf 'TAMPERED' > "$LOCAL_C/ocrx-dsh-1.2.3.tgz"
if PATH="$PUB_BIN:$PATH" FAKE_STATE="$PUB_PROBE/state-a" FAKE_LOG="$PUB_PROBE/calls-c.log"     FAKE_LOCAL_DIR="$PUBLISHED_A" FAKE_PUBLISHED_DIR="$PUB_PROBE/published-c"     bash "$PUB" "1.2.3"     "$LOCAL_C/ocrx-dsh-1.2.3.tgz"     "$LOCAL_C/ocrx-ocrx-win32-x64-1.2.3.tgz"     "$LOCAL_C/ocrx-ocrx-darwin-arm64-1.2.3.tgz" > /dev/null 2>&1; then
  echo "identity mismatch must fail the run" >&2; exit 1
fi

# d) Registry error (non-E404) → FAIL, never "missing".
cat > "$PUB_BIN/npm" << 'FAKE_NPM_ERR'
#!/usr/bin/env bash
echo "npm error code ECONNRESET" >&2
exit 1
FAKE_NPM_ERR
chmod +x "$PUB_BIN/npm"
LOCAL_D="$PUB_PROBE/local-d"; make_tarballs "$LOCAL_D"
if PATH="$PUB_BIN:$PATH" bash "$PUB" "1.2.3" \
    "$LOCAL_D/ocrx-dsh-1.2.3.tgz" \
    "$LOCAL_D/ocrx-ocrx-win32-x64-1.2.3.tgz" \
    "$LOCAL_D/ocrx-ocrx-darwin-arm64-1.2.3.tgz" > /dev/null 2>&1; then
  echo "registry errors must fail, never count as missing" >&2; exit 1
fi
rm -rf "$PUB_PROBE"
echo "dsh-plugin pipeline checks passed"
