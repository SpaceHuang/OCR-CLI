# Functional smoke of install.ps1's GPU variant priority chain (plan §5 T1.3).
#
# Builds minimal fake release archives (a staged skill root with bin/ and a
# SHA256SUMS manifest) and runs install.ps1 in a child PowerShell for the
# matrix {cuda+vulkan+cpu, vulkan+cpu, cpu-only} x {nvcuda, vulkan-only,
# none}. GPU detection is driven through the OCRX_TEST_GPU_OVERRIDE test seam
# (mirrors the OCRX_E2E_INJECT_* precedent: production runs never set it).
#
# Fake binaries are real executables so launch verification behaves:
#   - base ocrx.exe   = hostname.exe (small; ignores args, exits 0)
#   - variants        = cmd.exe      (bigger; ignores args, exits 0)
#   - broken variant  = a text file with an .exe extension (launch throws)
# The chain must promote the highest-priority launchable variant, leave the
# remaining candidates in place, keep the CPU binary as ocrx-cpu.exe exactly
# when a promotion happened, and fall through a broken cuda variant to
# vulkan.

$ErrorActionPreference = 'Stop'
$RootDir = Split-Path -Parent $PSScriptRoot
$InstallScript = Join-Path $PSScriptRoot 'install.ps1'
$Work = Join-Path ([System.IO.Path]::GetTempPath()) ("ocrx-chain-smoke-" + [System.Guid]::NewGuid().ToString('N'))
$CpuSize = (Get-Item "$env:WINDIR\System32\hostname.exe").Length
$GpuSize = (Get-Item "$env:WINDIR\System32\cmd.exe").Length

function Fail([string]$Message) {
  [Console]::Error.WriteLine($Message)
  Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
  exit 1
}

function New-FakeBinDir([string]$Bin, [string[]]$Variants, [string]$BrokenVariant) {
  New-Item -ItemType Directory -Force -Path $Bin | Out-Null
  Copy-Item "$env:WINDIR\System32\hostname.exe" (Join-Path $Bin 'ocrx.exe')
  foreach ($Variant in $Variants) {
    Copy-Item "$env:WINDIR\System32\cmd.exe" (Join-Path $Bin $Variant)
  }
  if ($BrokenVariant) {
    Set-Content -Path (Join-Path $Bin $BrokenVariant) -Value 'not an executable'
  }
}

function New-FakeArchive([string]$Dir, [string[]]$Variants, [string]$BrokenVariant) {
  $Stage = Join-Path $Dir 'ocrx'
  New-FakeBinDir (Join-Path $Stage 'bin') $Variants $BrokenVariant
  # SHA256SUMS covers every staged file (install.ps1 enforces full coverage).
  $Sums = Join-Path $Stage 'SHA256SUMS'
  $Lines = @()
  Get-ChildItem -File -Recurse -Path $Stage | ForEach-Object {
    $Hash = (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower()
    $Relative = './' + $_.FullName.Substring($Stage.Length).TrimStart('\').Replace('\', '/')
    $Lines += "$Hash  $Relative"
  }
  Set-Content -Path $Sums -Value $Lines
  $Zip = Join-Path $Dir 'release.zip'
  # Endpoint scanners transiently lock freshly written files (LNK1105-style
  # race); retry the pack a few times before giving up.
  for ($Attempt = 1; $Attempt -le 5; $Attempt++) {
    try {
      Compress-Archive -Path $Stage -DestinationPath $Zip -Force
      break
    } catch {
      if ($Attempt -eq 5) { throw }
      Start-Sleep -Seconds 1
    }
  }
  return $Zip
}

function Run-Install([string]$Zip, [string]$GpuOverride, [string]$Prefix) {
  $env:OCRX_RELEASE_ARCHIVE = $Zip
  $env:OCRX_RELEASE_SHA256 = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLower()
  $env:OCRX_INSTALL_PREFIX = $Prefix
  $env:OCRX_TEST_GPU_OVERRIDE = $GpuOverride
  $env:OCRX_INSTALL_SKILL = $null
  & powershell -NoProfile -ExecutionPolicy Bypass -File $InstallScript *> (Join-Path $Work 'last-install.log')
  return $LASTEXITCODE
}

# Asserts the final bin state for one matrix cell.
#   Expect 'ocrx-cuda.exe' / 'ocrx-vulkan.exe': that variant became ocrx.exe,
#     ocrx-cpu.exe exists, and the not-selected candidates remain.
#   Expect 'cpu': no promotion — ocrx.exe is the CPU binary and no
#     ocrx-cpu.exe copy exists.
function Assert-Cell([string]$Prefix, [string]$Expect, [string[]]$ArchiveVariants, [string]$Label) {
  $Bin = Join-Path $Prefix 'bin'
  $DefaultSize = (Get-Item (Join-Path $Bin 'ocrx.exe')).Length
  $HasCpuCopy = Test-Path (Join-Path $Bin 'ocrx-cpu.exe') -PathType Leaf
  if ($Expect -eq 'cpu') {
    if ($HasCpuCopy) { Fail "$Label expected CPU default but a variant was promoted (ocrx-cpu.exe exists)" }
    if ($DefaultSize -ne $CpuSize) { Fail "$Label expected CPU default but ocrx.exe is not the CPU binary" }
    return
  }
  if (-not $HasCpuCopy) { Fail "$Label expected $Expect promoted but no ocrx-cpu.exe copy exists" }
  if ($DefaultSize -ne $GpuSize) { Fail "$Label expected $Expect promoted but ocrx.exe is not GPU-sized" }
  foreach ($Variant in $ArchiveVariants) {
    if ($Variant -eq $Expect) { continue }  # the promoted one was moved onto ocrx.exe
    if (-not (Test-Path (Join-Path $Bin $Variant) -PathType Leaf)) {
      Fail "$Label expected leftover candidate $Variant to stay in bin"
    }
  }
}

New-Item -ItemType Directory -Force -Path $Work | Out-Null
$ArchiveCudaVulkan = New-FakeArchive (Join-Path $Work 'a1') @('ocrx-cuda.exe', 'ocrx-vulkan.exe') $null
$ArchiveVulkanOnly = New-FakeArchive (Join-Path $Work 'a2') @('ocrx-vulkan.exe') $null
$ArchiveCpuOnly = New-FakeArchive (Join-Path $Work 'a3') @() $null

$Matrix = @(
  @{ Archive = 'cuda+vulkan'; Variants = @('ocrx-cuda.exe', 'ocrx-vulkan.exe'); Detect = 'cuda';   Expect = 'ocrx-cuda.exe' },
  @{ Archive = 'cuda+vulkan'; Variants = @('ocrx-cuda.exe', 'ocrx-vulkan.exe'); Detect = 'vulkan'; Expect = 'ocrx-vulkan.exe' },
  @{ Archive = 'cuda+vulkan'; Variants = @('ocrx-cuda.exe', 'ocrx-vulkan.exe'); Detect = 'cpu';    Expect = 'cpu' },
  @{ Archive = 'vulkan';      Variants = @('ocrx-vulkan.exe');                  Detect = 'cuda';   Expect = 'ocrx-vulkan.exe' },
  @{ Archive = 'vulkan';      Variants = @('ocrx-vulkan.exe');                  Detect = 'vulkan'; Expect = 'ocrx-vulkan.exe' },
  @{ Archive = 'vulkan';      Variants = @('ocrx-vulkan.exe');                  Detect = 'cpu';    Expect = 'cpu' },
  @{ Archive = 'cpu';         Variants = @();                                   Detect = 'cuda';   Expect = 'cpu' },
  @{ Archive = 'cpu';         Variants = @();                                   Detect = 'vulkan'; Expect = 'cpu' },
  @{ Archive = 'cpu';         Variants = @();                                   Detect = 'cpu';    Expect = 'cpu' }
)
$ArchiveFor = @{
  'cuda+vulkan' = $ArchiveCudaVulkan
  'vulkan'      = $ArchiveVulkanOnly
  'cpu'         = $ArchiveCpuOnly
}

$Failed = 0
foreach ($Cell in $Matrix) {
  $Prefix = Join-Path $Work ("prefix-" + $Cell.Archive.Replace('+', '_') + "-" + $Cell.Detect)
  $Exit = Run-Install $ArchiveFor[$Cell.Archive] $Cell.Detect $Prefix
  if ($Exit -ne 0) { Fail "install failed (exit $Exit) for $($Cell.Archive) x $($Cell.Detect); see last-install.log" }
  try {
    Assert-Cell $Prefix $Cell.Expect $Cell.Variants "$($Cell.Archive) x $($Cell.Detect)"
    Write-Host "ok   $($Cell.Archive) x $($Cell.Detect) -> $($Cell.Expect)"
  } catch {
    Write-Host "FAIL $($Cell.Archive) x $($Cell.Detect): $($_.Exception.Message)"
    $Failed += 1
  }
}

# Degrade path: the cuda variant exists but cannot launch; on a cuda machine
# the chain must fall through to the launchable vulkan variant.
$BrokenCudaZip = New-FakeArchive (Join-Path $Work 'a4') @('ocrx-vulkan.exe') 'ocrx-cuda.exe'
$Prefix4 = Join-Path $Work 'prefix-degrade'
$Exit4 = Run-Install $BrokenCudaZip 'cuda' $Prefix4
if ($Exit4 -ne 0) { Fail "degrade install failed (exit $Exit4)" }
try {
  # The broken cuda variant must be REMOVED by the degrade path (no dead
  # .exe left in bin), vulkan promoted, and the CPU copy kept.
  Assert-Cell $Prefix4 'ocrx-vulkan.exe' @() 'broken-cuda degrade'
  if (Test-Path (Join-Path $Prefix4 'bin\ocrx-cuda.exe') -PathType Leaf) {
    Fail 'broken-cuda degrade left the unlaunchable variant in bin'
  }
  Write-Host "ok   broken-cuda x cuda-detect -> falls through to ocrx-vulkan.exe"
} catch {
  Write-Host "FAIL broken-cuda degrade: $($_.Exception.Message)"
  $Failed += 1
}

# Shared-bin regression (2026-11-05): a prefix bin that hosts foreign
# tools (uv/hf style) must keep them across an upgrade - the wholesale
# swap used to orphan them into bin.old-*, breaking their PATH entries.
$SharedZip = $ArchiveFor['cpu']
$PrefixShared = Join-Path $Work 'prefix-shared'
$SharedBin = Join-Path $PrefixShared 'bin'
New-Item -ItemType Directory -Force -Path $SharedBin | Out-Null
Copy-Item "$env:WINDIR\System32\hostname.exe" (Join-Path $SharedBin 'ocrx.exe')  # previous install
Copy-Item "$env:WINDIR\System32\hostname.exe" (Join-Path $SharedBin 'uv.exe')     # foreign tool
Set-Content -Path (Join-Path $SharedBin 'tool-data.txt') -Value 'foreign data file'
$ExitShared = Run-Install $SharedZip 'none' $PrefixShared
if ($ExitShared -ne 0) { Fail "shared-bin install failed (exit $ExitShared)" }
try {
  if (-not (Test-Path (Join-Path $SharedBin 'uv.exe') -PathType Leaf)) {
    Fail 'shared-bin upgrade lost the foreign tool (uv.exe)'
  }
  if (-not (Test-Path (Join-Path $SharedBin 'tool-data.txt') -PathType Leaf)) {
    Fail 'shared-bin upgrade lost the foreign data file'
  }
  if (-not (Test-Path (Join-Path $SharedBin 'ocrx.exe') -PathType Leaf)) {
    Fail 'shared-bin upgrade did not install ocrx.exe'
  }
  $StaleOld = @(Get-ChildItem -Path $PrefixShared -Directory -Filter 'bin.old-*' -ErrorAction SilentlyContinue)
  if ($StaleOld.Count -gt 0) {
    Fail "shared-bin upgrade left bin.old-* dirs behind: $($StaleOld.Count)"
  }
  Write-Host 'ok   shared-bin upgrade preserves foreign files, installs ocrx, leaves no bin.old-*'
} catch {
  Write-Host "FAIL shared-bin: $($_.Exception.Message)"
  $Failed += 1
}

Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
if ($Failed -gt 0) {
  [Console]::Error.WriteLine("$Failed chain smoke cell(s) failed")
  exit 1
}
Write-Host 'install chain smoke matrix passed'
exit 0
