# Build a Windows x64 Skill+CLI archive.
#
# Archive content is deterministic for a given source revision (same file
# set, same bytes, SHA256SUMS verifies it), but PS 5.1 `Compress-Archive`
# writes backslash entry separators and current timestamps, so the zip
# bytes themselves are NOT reproducible across runs; the sole consumer
# (install.ps1) unpacks it fine on both PowerShell editions.
# Layout matches the standard Skill directory contract:
#   ocrx/
#     SKILL.md
#     bin/ocrx.exe
#     bin/pdfium.dll
#     bin/ (VC++ runtime DLLs: msvcp140, vcruntime140, vcruntime140_1)
#     licenses/
#     LICENSE
#     THIRD_PARTY_NOTICES.md
#     INSTALL.txt
# Model weights are never included.
[CmdletBinding()]
param(
  [string]$OutDir,
  [string]$Version = $(if ($env:OCRX_VERSION) { $env:OCRX_VERSION } else { '0.1.0' })
)

$ErrorActionPreference = 'Stop'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $OutDir) { $OutDir = Join-Path $RootDir 'dist' }
$Archive = Join-Path $OutDir "ocrx-$Version-windows-x64.zip"
# Top-level directory name must be the skill id so agents can drop it into skills/.
$Stage = Join-Path $OutDir 'ocrx'
$LegacyStage = Join-Path $OutDir "ocrx-$Version-windows-x64"

function Fail([string]$Message, [int]$Code) {
  [Console]::Error.WriteLine($Message)
  exit $Code
}

# PE machine type check so a wrong-arch pdfium.dll is rejected at packaging time.
function Assert-PeMachineType([string]$Path, [string]$Label, [uint16]$Expected) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  if ($bytes.Length -lt 64) { Fail "$Label is not a valid PE image" 4 }
  $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
  if ($peOffset -lt 0 -or $peOffset -gt $bytes.Length - 4) { Fail "$Label is not a valid PE image" 4 }
  $machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
  if ($machine -ne $Expected) { Fail "$Label is not the expected architecture" 4 }
}

# Locate the VC++ redistributable CRT directory. The release binary links the
# /MD CRT through llama.cpp, so these three DLLs must ship app-local in bin/
# or a clean machine cannot start the binary. Override with OCRX_VC_RUNTIME_DIR.
function Find-VcCrtDir {
  $CrtDlls = @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')
  if ($env:OCRX_VC_RUNTIME_DIR) { return $env:OCRX_VC_RUNTIME_DIR }
  $Vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path $Vswhere)) { return $null }
  $VsRoot = & $Vswhere -latest -products * -property installationPath
  if (-not $VsRoot) { return $null }
  $RedistRoot = Join-Path $VsRoot 'VC\Redist\MSVC'
  if (-not (Test-Path $RedistRoot)) { return $null }
  # Sort by the Redist version directory parsed as [version] so 14.40 wins
  # over 14.9 (plain name order is lexicographic and picks 14.9 first).
  # Candidate layout is <root>\<version>\x64\Microsoft.VC*.CRT, so the
  # version name is the CRT dir's grandparent. Unparseable names sort last.
  $Candidates = Get-ChildItem -Directory -Path (Join-Path $RedistRoot '*\x64\Microsoft.VC*.CRT') -ErrorAction SilentlyContinue |
    Sort-Object -Property @{
      Expression = {
        try {
          [version](Split-Path -Leaf (Split-Path -Parent (Split-Path -Parent $_.FullName)))
        } catch { [version]'0.0.0.0' }
      }
      Descending = $true
    }
  foreach ($Candidate in $Candidates) {
    $Complete = $true
    foreach ($Dll in $CrtDlls) {
      if (-not (Test-Path (Join-Path $Candidate.FullName $Dll))) { $Complete = $false; break }
    }
    if ($Complete) { return $Candidate.FullName }
  }
  return $null
}

if ($env:OS -ne 'Windows_NT') { Fail 'requires Windows' 2 }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { Fail 'requires x64' 2 }
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
  Fail 'cmake not found on PATH; run from a VS developer prompt or add CMake to PATH' 4
}

if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
if (Test-Path $LegacyStage) { Remove-Item -Recurse -Force $LegacyStage }
New-Item -ItemType Directory -Force -Path (Join-Path $Stage 'bin') | Out-Null

Push-Location $RootDir
try {
  & cargo build --release --locked -p ocrx
  if ($LASTEXITCODE -ne 0) { Fail 'cargo build failed' 4 }
} finally {
  Pop-Location
}

$BuiltBinary = Join-Path $RootDir 'target\release\ocrx.exe'
if (-not (Test-Path $BuiltBinary)) { Fail 'release binary missing' 4 }
Copy-Item $BuiltBinary (Join-Path $Stage 'bin\ocrx.exe') -Force

if (-not $env:OCRX_PDFIUM_LIBRARY) {
  Fail 'set OCRX_PDFIUM_LIBRARY to package the required x64 PDFium dll' 4
}
if (-not (Test-Path $env:OCRX_PDFIUM_LIBRARY)) { Fail 'PDFium dll missing' 4 }
Assert-PeMachineType -Path $env:OCRX_PDFIUM_LIBRARY -Label 'PDFium dll' -Expected ([uint16]0x8664)
Copy-Item $env:OCRX_PDFIUM_LIBRARY (Join-Path $Stage 'bin\pdfium.dll') -Force

# Bundle the PDFium license set next to the dll (redistribution notice requirement);
# every bundled file is covered by SHA256SUMS below.
$PdfiumRoot = Split-Path (Split-Path $env:OCRX_PDFIUM_LIBRARY -Parent) -Parent
$LicenseSrc = Join-Path $PdfiumRoot 'licenses'
if (-not (Test-Path $LicenseSrc)) { Fail 'PDFium license set missing next to the dll' 4 }
$LicenseDst = Join-Path $Stage 'licenses\pdfium'
New-Item -ItemType Directory -Force -Path $LicenseDst | Out-Null
Copy-Item -Path (Join-Path $LicenseSrc '*') -Destination $LicenseDst -Recurse -Force
if (Test-Path (Join-Path $PdfiumRoot 'LICENSE')) {
  Copy-Item (Join-Path $PdfiumRoot 'LICENSE') $LicenseDst -Force
}

# App-local VC++ runtime: ocrx.exe imports the /MD CRT DLLs, so they ship in
# bin/ alongside the binary (find them, verify arch, hash them via SHA256SUMS).
$VcCrtDir = Find-VcCrtDir
if (-not $VcCrtDir) {
  Fail 'VC++ runtime DLLs not found; set OCRX_VC_RUNTIME_DIR to a Microsoft.VC*.CRT directory' 4
}
foreach ($Dll in @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
  $CrtSource = Join-Path $VcCrtDir $Dll
  if (-not (Test-Path $CrtSource)) { Fail "VC runtime dll missing: $Dll" 4 }
  Assert-PeMachineType -Path $CrtSource -Label $Dll -Expected ([uint16]0x8664)
  Copy-Item $CrtSource (Join-Path (Join-Path $Stage 'bin') $Dll) -Force
}

# Ship the license set with the archive: the project Apache-2.0 text, the
# third-party notices, the pinned llama.cpp MIT license, and the VC++ runtime
# redistribution notice. Every bundled file is covered by SHA256SUMS below.
foreach ($RepoFile in @('LICENSE', 'THIRD_PARTY_NOTICES.md')) {
  $NoticeSource = Join-Path $RootDir $RepoFile
  if (-not (Test-Path $NoticeSource)) { Fail "license file missing in repo: $RepoFile" 4 }
  Copy-Item $NoticeSource (Join-Path $Stage $RepoFile) -Force
}
$LlamaLicense = Join-Path $RootDir 'assets\licenses\llama.cpp\LICENSE'
if (-not (Test-Path $LlamaLicense)) { Fail 'llama.cpp license missing under assets\licenses' 4 }
$LlamaDst = Join-Path $Stage 'licenses\llama.cpp'
New-Item -ItemType Directory -Force -Path $LlamaDst | Out-Null
Copy-Item $LlamaLicense (Join-Path $LlamaDst 'LICENSE') -Force
$VcNotice = Join-Path $RootDir 'assets\licenses\ms-vc-runtime\README.txt'
if (-not (Test-Path $VcNotice)) { Fail 'VC runtime notice missing under assets\licenses' 4 }
$VcDst = Join-Path $Stage 'licenses\ms-vc-runtime'
New-Item -ItemType Directory -Force -Path $VcDst | Out-Null
Copy-Item $VcNotice (Join-Path $VcDst 'README.txt') -Force

Copy-Item (Join-Path $RootDir 'assets\skills\ocrx\SKILL.md') (Join-Path $Stage 'SKILL.md') -Force
if (-not (Test-Path (Join-Path $Stage 'SKILL.md'))) { Fail 'SKILL.md missing at skill root' 4 }

$installText = @"
This directory is a standard Agent Skill root (SKILL.md) plus the OCR-CLI binary.

Layout:
  ocrx/
    SKILL.md
    bin/ocrx.exe
    bin/pdfium.dll
    bin/msvcp140.dll
    bin/vcruntime140.dll
    bin/vcruntime140_1.dll
    licenses/
    LICENSE
    THIRD_PARTY_NOTICES.md
    INSTALL.txt

It does not contain model weights. Model installation is explicit.

Agent / drop-in Skill install:
  Copy or symlink this `ocrx/` directory into the host skills folder, e.g.
    %USERPROFILE%\.codex\skills\ocrx
    %USERPROFILE%\.claude\skills\ocrx
    %USERPROFILE%\.cursor\skills\ocrx

CLI install (PATH):
  Put bin\ocrx.exe on PATH, or run: powershell -ExecutionPolicy Bypass -File scripts\install.ps1
  Optional: `$env:OCRX_INSTALL_SKILL=1 to also write host Skill files via `ocrx skill install`.

When invoking the bundled binary from this Skill directory, set:
  OCRX_PDFIUM_LIBRARY=<absolute-path-to-this-dir>\bin\pdfium.dll
"@
Set-Content -Path (Join-Path $Stage 'INSTALL.txt') -Value $installText -Encoding ascii

# Hash manifest mirrors `find . | shasum -a 256 > SHA256SUMS` (SHA256SUMS itself excluded).
# LF line endings: Set-Content would emit CRLF and break `sha256sum -c`.
$sums = Get-ChildItem -File -Recurse $Stage |
  Sort-Object FullName |
  ForEach-Object {
    $hash = (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower()
    $rel = '.' + (($_.FullName.Substring($Stage.Length)) -replace '\\', '/')
    "$hash  $rel"
  }
[System.IO.File]::WriteAllText((Join-Path $Stage 'SHA256SUMS'), (($sums -join "`n") + "`n"))

if (Test-Path $Archive) { Remove-Item -Force $Archive }
Push-Location $OutDir
try {
  Compress-Archive -Path 'ocrx' -DestinationPath $Archive -Force
} finally {
  Pop-Location
}
Write-Output $Archive
