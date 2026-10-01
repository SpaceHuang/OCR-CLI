# Install OCR-CLI on Windows (user directory, no administrator required).
# Mirrors scripts/install.sh; platform differences are only the package format
# (zip), paths, and shell. Decision 4: pdfium.dll is bundled with the release;
# model weights and GPU runtime are never downloaded by this installer.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ($env:OCRX_INSTALL_PREFIX) {
  $Prefix = $env:OCRX_INSTALL_PREFIX
} else {
  $Prefix = Join-Path $env:USERPROFILE '.local'
}

function Fail([string]$Message, [int]$Code) {
  [Console]::Error.WriteLine($Message)
  exit $Code
}

function Test-Sha256([string]$Path, [string]$Expected) {
  if (-not $Expected) { return $true }
  $actual = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLower()
  return $actual -eq $Expected.ToLower()
}

function Detect-GpuBackend {
  $sys = Join-Path $env:WINDIR 'System32'
  if (Test-Path (Join-Path $sys 'nvcuda.dll')) { return 'cuda' }
  if (Test-Path (Join-Path $sys 'vulkan-1.dll')) { return 'vulkan' }
  return 'cpu'
}

# Verify every file listed in the archive's SHA256SUMS (the archive carries its
# own integrity manifest; a mismatch or missing file aborts the install).
function Assert-ArchiveHashes([string]$StageRoot) {
  $Sums = Join-Path $StageRoot 'SHA256SUMS'
  if (-not (Test-Path $Sums)) { Fail 'archive SHA256SUMS missing' 5 }
  foreach ($Line in (Get-Content $Sums)) {
    if (-not $Line.Trim()) { continue }
    $Parts = $Line -split '  ', 2
    if ($Parts.Count -ne 2) { Fail 'archive SHA256SUMS malformed' 5 }
    $File = Join-Path $StageRoot $Parts[1]
    if (-not (Test-Path $File)) { Fail "archive file missing: $($Parts[1])" 5 }
    $Actual = (Get-FileHash -Algorithm SHA256 $File).Hash.ToLower()
    if ($Actual -ne $Parts[0].ToLower()) { Fail "archive hash mismatch: $($Parts[1])" 5 }
  }
}

if ($env:OS -ne 'Windows_NT') { Fail 'OCR-CLI v0.1 supports Windows only' 2 }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { Fail 'OCR-CLI v0.1 supports Windows x64 only' 2 }

$Offline = $env:OCRX_OFFLINE -eq '1'
$Gpu = Detect-GpuBackend
Write-Host "detected GPU backend: $Gpu (v0.1 ships a CPU-only build)"

$TempDir = Join-Path $env:TEMP ('ocrx-install-' + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
try {
  $InstallBinary = $null
  $InstallPdfium = $null
  $ArchiveBinDir = $null

  if ($env:OCRX_RELEASE_ARCHIVE) {
    if (-not (Test-Path $env:OCRX_RELEASE_ARCHIVE)) { Fail 'release archive missing' 4 }
    if (-not (Test-Sha256 $env:OCRX_RELEASE_ARCHIVE $env:OCRX_RELEASE_SHA256)) {
      Fail 'release archive hash mismatch' 5
    }
    $ExtractDir = Join-Path $TempDir 'extract'
    Expand-Archive -Path $env:OCRX_RELEASE_ARCHIVE -DestinationPath $ExtractDir -Force
    $InstallBinary = Get-ChildItem -Path $ExtractDir -Recurse -Filter 'ocrx.exe' | Select-Object -First 1 -ExpandProperty FullName
    if (-not $InstallBinary) { Fail 'archive has no ocrx binary' 4 }
    $InstallPdfium = Get-ChildItem -Path $ExtractDir -Recurse -Filter 'pdfium.dll' | Select-Object -First 1 -ExpandProperty FullName
    $ArchiveBinDir = Split-Path $InstallBinary -Parent
    Assert-ArchiveHashes (Split-Path $ArchiveBinDir -Parent)
  } elseif (Test-Path (Join-Path $RootDir 'target\release\ocrx.exe')) {
    $InstallBinary = Join-Path $RootDir 'target\release\ocrx.exe'
    $VendorPdfium = Join-Path $RootDir 'vendor\pdfium\bin\pdfium.dll'
    if (Test-Path $VendorPdfium) { $InstallPdfium = $VendorPdfium }
  } elseif ($env:OCRX_RELEASE_URL) {
    if ($Offline) { Fail 'OCRX_OFFLINE is set but no local release archive or build is available' 6 }
    if (-not $env:OCRX_RELEASE_SHA256) { Fail 'OCRX_RELEASE_SHA256 is required to verify the downloaded release' 5 }
    $DownloadedZip = Join-Path $TempDir 'release.zip'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocol]::Tls12
    Invoke-WebRequest -Uri $env:OCRX_RELEASE_URL -OutFile $DownloadedZip
    if (-not (Test-Sha256 $DownloadedZip $env:OCRX_RELEASE_SHA256)) {
      Fail 'downloaded release archive hash mismatch' 5
    }
    $ExtractDir = Join-Path $TempDir 'extract'
    Expand-Archive -Path $DownloadedZip -DestinationPath $ExtractDir -Force
    $InstallBinary = Get-ChildItem -Path $ExtractDir -Recurse -Filter 'ocrx.exe' | Select-Object -First 1 -ExpandProperty FullName
    if (-not $InstallBinary) { Fail 'archive has no ocrx binary' 4 }
    $InstallPdfium = Get-ChildItem -Path $ExtractDir -Recurse -Filter 'pdfium.dll' | Select-Object -First 1 -ExpandProperty FullName
    $ArchiveBinDir = Split-Path $InstallBinary -Parent
    Assert-ArchiveHashes (Split-Path $ArchiveBinDir -Parent)
  } else {
    if ($Offline) { Fail 'OCRX_OFFLINE is set and no local release archive or build is available' 6 }
    Fail 'set OCRX_RELEASE_ARCHIVE or build target\release\ocrx.exe first' 4
  }

  $BinDir = Join-Path $Prefix 'bin'
  New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
  if ($ArchiveBinDir) {
    # Copy the whole bin/ set (binary, pdfium.dll, VC++ runtime DLLs).
    Get-ChildItem -File -Path $ArchiveBinDir | Copy-Item -Destination $BinDir -Force
  } else {
    Copy-Item $InstallBinary (Join-Path $BinDir 'ocrx.exe') -Force
    if ($InstallPdfium) { Copy-Item $InstallPdfium (Join-Path $BinDir 'pdfium.dll') -Force }
  }

  if ($env:OCRX_INSTALL_SKILL -eq '1') {
    if (-not $env:OCRX_SKILL_TARGET) { $env:OCRX_SKILL_TARGET = 'all' }
    if ($InstallPdfium) { $env:OCRX_PDFIUM_LIBRARY = Join-Path $BinDir 'pdfium.dll' }
    & (Join-Path $BinDir 'ocrx.exe') skill install
    if ($LASTEXITCODE -ne 0) { Fail 'skill install failed' 4 }
  }

  Write-Host "installed $(Join-Path $BinDir 'ocrx.exe')"
  $pathDirs = $env:PATH -split ';'
  $inPath = $false
  foreach ($d in $pathDirs) { if ($d -and ($d -ieq $BinDir)) { $inPath = $true; break } }
  if (-not $inPath) { Write-Host "add $BinDir to PATH" }
  Write-Host 'model weights were not downloaded; install them explicitly with the CLI when needed'
} finally {
  Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}
