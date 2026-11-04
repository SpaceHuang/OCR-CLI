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
  # Test-only seam for the chain smoke matrix (mirrors the OCRX_E2E_INJECT_*
  # precedent): drives detection without touching System32; production runs
  # never set it.
  if ($env:OCRX_TEST_GPU_OVERRIDE -in @('cuda', 'vulkan', 'cpu')) { return $env:OCRX_TEST_GPU_OVERRIDE }
  $sys = Join-Path $env:WINDIR 'System32'
  if (Test-Path (Join-Path $sys 'nvcuda.dll')) { return 'cuda' }
  if (Test-Path (Join-Path $sys 'vulkan-1.dll')) { return 'vulkan' }
  return 'cpu'
}

# The installer copies the VC++ runtime DLLs app-local into <prefix>\bin.
# An OCRX_INSTALL_PREFIX pointing into Windows or Program Files must be
# refused so the install can never overwrite operating-system files
# (review MINOR 13 / M-5). Both sides are trimmed + separator-terminated and
# compared case-insensitively, so exact spellings ('C:\Windows'), child
# paths, and GetFullPath-expanded short names are all caught.
function Test-SafeInstallPrefix([string]$Path) {
  $Resolved = [System.IO.Path]::GetFullPath($Path).TrimEnd('\').ToLower() + '\'
  foreach ($Root in @($env:WINDIR, ${env:ProgramFiles}, ${env:ProgramFiles(x86)})) {
    if (-not $Root) { continue }
    $RootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd('\').ToLower() + '\'
    if ($Resolved.StartsWith($RootFull)) { return $false }
  }
  return $true
}

# Verify every file listed in the archive's SHA256SUMS (the archive carries its
# own integrity manifest; a mismatch or missing file aborts the install), then
# verify no file under the staged root escapes the manifest (M-5): a staged
# file that SHA256SUMS does not name must never be installed.
function Assert-ArchiveHashes([string]$StageRoot) {
  $Sums = Join-Path $StageRoot 'SHA256SUMS'
  if (-not (Test-Path $Sums)) { Fail 'archive SHA256SUMS missing' 5 }
  $Listed = @{}
  foreach ($Line in (Get-Content $Sums)) {
    if (-not $Line.Trim()) { continue }
    $Parts = $Line -split '  ', 2
    if ($Parts.Count -ne 2) { Fail 'archive SHA256SUMS malformed' 5 }
    $File = Join-Path $StageRoot $Parts[1]
    if (-not (Test-Path $File)) { Fail "archive file missing: $($Parts[1])" 5 }
    $Actual = (Get-FileHash -Algorithm SHA256 $File).Hash.ToLower()
    if ($Actual -ne $Parts[0].ToLower()) { Fail "archive hash mismatch: $($Parts[1])" 5 }
    # The manifest writes paths with a leading ./ (found by the install
    # smoke); normalize it so the enumeration below compares like with like.
    $Relative = $Parts[1] -replace '^\./', ''
    $Listed[$Relative.ToLower()] = $true
  }
  foreach ($Staged in (Get-ChildItem -File -Recurse -Path $StageRoot)) {
    # Normalize separators too: the manifest uses forward slashes while the
    # extracted tree enumerates with backslashes.
    $Relative = $Staged.FullName.Substring($StageRoot.Length).TrimStart('\').Replace('\', '/').ToLower()
    # The manifest cannot list itself.
    if ($Relative -eq 'sha256sums') { continue }
    if (-not $Listed.ContainsKey($Relative)) {
      Fail "archive file not covered by SHA256SUMS: $($Staged.Name)" 5
    }
  }
}

if ($env:OS -ne 'Windows_NT') { Fail 'OCR-CLI v0.1 supports Windows only' 2 }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { Fail 'OCR-CLI v0.1 supports Windows x64 only' 2 }
if (-not (Test-SafeInstallPrefix $Prefix)) {
  Fail "refusing to install into a system directory: $Prefix" 3
}

$Offline = $env:OCRX_OFFLINE -eq '1'
$Gpu = Detect-GpuBackend
Write-Host "detected GPU backend: $Gpu"

$TempDir = Join-Path $env:TEMP ('ocrx-install-' + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
try {
  $InstallBinary = $null
  $InstallPdfium = $null
  $ArchiveBinDir = $null

  if ($env:OCRX_RELEASE_ARCHIVE) {
    if (-not (Test-Path $env:OCRX_RELEASE_ARCHIVE)) { Fail 'release archive missing' 4 }
    # The outer archive hash is mandatory even for a local file (M-5): a
    # local zip without a pinned hash is an unverified artifact.
    if (-not $env:OCRX_RELEASE_SHA256) { Fail 'OCRX_RELEASE_SHA256 is required to verify the release archive' 5 }
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
    # Frozen trust decision: release downloads are HTTPS-only (M-5). The
    # archive hash below guards integrity; this guards the transport.
    if (-not $env:OCRX_RELEASE_URL.StartsWith('https://')) {
      Fail 'OCRX_RELEASE_URL must use https' 5
    }
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
  # Stage the complete bin set first, then swap directories: a multi-file
  # copy straight into bin\ used to leave a half-old/half-new installation
  # when it failed midway (review MINOR 13).
  $StagingName = 'bin.staging-' + [System.Guid]::NewGuid().ToString('N')
  $OldName = 'bin.old-' + [System.Guid]::NewGuid().ToString('N')
  $StagingBin = Join-Path $Prefix $StagingName
  New-Item -ItemType Directory -Force -Path $StagingBin | Out-Null
  if ($ArchiveBinDir) {
    # Copy the whole bin/ set (binary, pdfium.dll, VC++ runtime DLLs).
    Get-ChildItem -File -Path $ArchiveBinDir | Copy-Item -Destination $StagingBin -Force
    # GPU variant promotion (W4.2; P2 T1.3 generalizes it to a priority
    # chain): the detected backend decides the candidate order — a cuda
    # machine tries ocrx-cuda.exe first, then ocrx-vulkan.exe; a vulkan
    # machine tries ocrx-vulkan.exe. Every level is launch-verified
    # (`version` exit 0); a failed level degrades to the next with an
    # explicit log. The CPU build stays authoritative when nothing launches
    # (it is kept as ocrx-cpu.exe only when a promotion happened).
    $VariantOrder = @()
    if ($Gpu -eq 'cuda') { $VariantOrder = @('ocrx-cuda.exe', 'ocrx-vulkan.exe') }
    elseif ($Gpu -eq 'vulkan') { $VariantOrder = @('ocrx-vulkan.exe') }
    $PromotedVariant = $null
    foreach ($VariantName in $VariantOrder) {
      $VariantPath = Join-Path $StagingBin $VariantName
      if (-not (Test-Path $VariantPath -PathType Leaf)) { continue }
      Copy-Item (Join-Path $StagingBin 'ocrx.exe') (Join-Path $StagingBin 'ocrx-cpu.exe') -Force
      try {
        & $VariantPath version | Out-Null
        if ($LASTEXITCODE -eq 0) {
          Remove-Item (Join-Path $StagingBin 'ocrx.exe') -Force
          Move-Item $VariantPath (Join-Path $StagingBin 'ocrx.exe') -Force
          $PromotedVariant = $VariantName
          break
        }
        Remove-Item $VariantPath -Force
        Write-Host "$VariantName failed to launch (version exit $LASTEXITCODE); trying next variant"
      } catch {
        Remove-Item $VariantPath -Force -ErrorAction SilentlyContinue
        Write-Host "$VariantName could not start on this machine; trying next variant"
      }
    }
    if ($PromotedVariant) {
      Write-Host "selected GPU ($PromotedVariant) build as default ocrx.exe; CPU build kept as ocrx-cpu.exe"
    }
  } else {
    Copy-Item $InstallBinary (Join-Path $StagingBin 'ocrx.exe') -Force
    if ($InstallPdfium) { Copy-Item $InstallPdfium (Join-Path $StagingBin 'pdfium.dll') -Force }
  }
  if (-not (Test-Path (Join-Path $StagingBin 'ocrx.exe') -PathType Leaf)) {
    Remove-Item -Recurse -Force $StagingBin -ErrorAction SilentlyContinue
    Fail 'no binary staged for install' 4
  }
  # Shared-bin safety (2026-11-05): an install prefix's bin/ is often a
  # SHARED directory (uv, hf, and friends live next to ocrx on the user's
  # PATH). The wholesale swap used to orphan every non-ocrx file into
  # bin.old-*, silently breaking those tools. Foreign files - anything the
  # archive does not ship and the installer does not manage - are copied
  # into the staging dir BEFORE the swap, so the atomic swap carries them
  # and the old dir can be removed safely afterwards. A failed swap rolls
  # the old bin back with its foreign files untouched (nothing moved out).
  $ManagedNames = @('ocrx.exe', 'ocrx-cpu.exe', 'ocrx-vulkan.exe', 'ocrx-cuda.exe',
                    'pdfium.dll', 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')
  $HadExisting = Test-Path $BinDir
  $RestoredForeign = 0
  if ($HadExisting) {
    $StagedNames = @(Get-ChildItem -File -Path $StagingBin | ForEach-Object { $_.Name })
    $ForeignFiles = @(Get-ChildItem -File -Path $BinDir | Where-Object {
      ($StagedNames -notcontains $_.Name) -and ($ManagedNames -notcontains $_.Name)
    })
    foreach ($ForeignFile in $ForeignFiles) {
      Copy-Item $ForeignFile.FullName (Join-Path $StagingBin $ForeignFile.Name) -Force
      $RestoredForeign += 1
    }
    if ($RestoredForeign -gt 0) {
      Write-Host "shared bin: preserving $RestoredForeign non-ocrx file(s) across the upgrade"
    }
  }
  if ($HadExisting) { Rename-Item $BinDir -NewName $OldName }
  try {
    Rename-Item $StagingBin -NewName 'bin'
  } catch {
    if ($HadExisting) { Rename-Item (Join-Path $Prefix $OldName) -NewName 'bin' }
    Fail "bin swap failed: $_" 4
  }
  if ($HadExisting) {
    # Remove-Item has no -PassThru on Windows PowerShell 5.1; detect success
    # by checking whether the old directory survived the removal. Safe now:
    # the old dir's content either moved into the new bin (foreign files)
    # or is stale ocrx-managed state superseded by this install.
    $OldPath = Join-Path $Prefix $OldName
    Remove-Item -Recurse -Force $OldPath -ErrorAction SilentlyContinue
    if (Test-Path $OldPath) {
      Write-Host "warning: could not remove $OldPath; remove it manually"
    }
  }

  Write-Host "installed $(Join-Path $BinDir 'ocrx.exe')"

  if ($env:OCRX_INSTALL_SKILL -eq '1') {
    if (-not $env:OCRX_SKILL_TARGET) { $env:OCRX_SKILL_TARGET = 'all' }
    if ($InstallPdfium) { $env:OCRX_PDFIUM_LIBRARY = Join-Path $BinDir 'pdfium.dll' }
    & (Join-Path $BinDir 'ocrx.exe') skill install
    if ($LASTEXITCODE -ne 0) { Fail 'skill install failed (binaries are installed)' 4 }
  }
  $pathDirs = $env:PATH -split ';'
  $inPath = $false
  foreach ($d in $pathDirs) { if ($d -and ($d -ieq $BinDir)) { $inPath = $true; break } }
  if (-not $inPath) { Write-Host "add $BinDir to PATH" }
  Write-Host 'model weights were not downloaded; install them explicitly with the CLI when needed'
} finally {
  Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}
