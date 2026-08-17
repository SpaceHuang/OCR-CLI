#!/usr/bin/env pwsh
<#
.SYNOPSIS
Download and verify the pinned official OvisOCR2 BF16 checkpoint used by the W0.4
reference generator.

.DESCRIPTION
Reads benchmarks/official-bf16/manifest.json, downloads every listed file from
hf-mirror.com, and verifies its SHA-256 before declaring success. The weights are
kept out of Git by benchmarks/official-bf16/.gitignore.
#>
[CmdletBinding()]
param(
    [string]$Mirror = "https://hf-mirror.com"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$manifestPath = Join-Path $repoRoot "benchmarks\official-bf16\manifest.json"
if (-not (Test-Path -LiteralPath $manifestPath)) {
    throw "missing manifest: $manifestPath"
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$repository = $manifest.source.repository
$revision = $manifest.source.revision

foreach ($file in $manifest.files) {
    $destination = Join-Path $repoRoot ("benchmarks\official-bf16\" + $file.path)
    $directory = Split-Path -Parent $destination
    New-Item -ItemType Directory -Force -Path $directory | Out-Null

    $name = Split-Path -Leaf $destination
    $url = "$Mirror/$repository/resolve/$revision/$name"
    if (Test-Path -LiteralPath $destination) {
        $existingHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
        if ($existingHash -eq $file.sha256) {
            Write-Host "ok   $($file.path)"
            continue
        }
        Write-Host "hash mismatch, re-downloading $($file.path)"
        Remove-Item -LiteralPath $destination -Force
    }

    Write-Host "get  $($file.path)"
    curl.exe -sS -L --fail --retry 5 --retry-delay 3 -C - --max-time 1500 $url -o $destination
    if ($LASTEXITCODE -ne 0) {
        throw "download failed: $url"
    }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
    if ($hash -ne $file.sha256) {
        throw "SHA-256 mismatch for $($file.path): expected $($file.sha256), got $hash"
    }
}

Write-Host "verified all $(@($manifest.files).Count) official BF16 files"
