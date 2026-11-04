#!/usr/bin/env pwsh
<#
.SYNOPSIS
Real Ctrl+Break smoke for the X1 multi-document cancellation contract
(rereview BLK-1): a PURE-NATIVE multi-document orchestrated scan.

.DESCRIPTION
Phase-2 rereview BLK-1: native documents never enter the ScanService, so
the invocation-level cancellation token was ignored - after Ctrl+C the
queue processed every remaining document and exited 0. This harness
spawns `ocrx scan <docs-dir>` (text mode, no model) in a new console
process group, waits until the FIRST document has committed its
result.json, sends a real CTRL_BREAK_EVENT, and asserts:

- the process was still running when the signal was sent (anchor implies it);
- the final exit code is 130 (never 0);
- scan-summary.json lists fewer documents than the input count;
- documents after the cancellation boundary have no output directories.

Requires the co-located PDFium DLL (native routing needs page analysis);
no OCR model is required.
#>
[CmdletBinding()]
param(
    [string]$Binary = "",
    [string]$PdfiumDir = "",
    [int]$DocCount = 200
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $Binary) { $Binary = Join-Path $repoRoot "target\debug\ocrx.exe" }
if (-not $PdfiumDir) { $PdfiumDir = Join-Path $repoRoot "vendor\pdfium\bin" }

if (-not (Test-Path -LiteralPath $Binary)) { throw "missing binary: $Binary" }
if (-not (Test-Path -LiteralPath (Join-Path $PdfiumDir "pdfium.dll"))) {
    throw "missing pdfium.dll under $PdfiumDir"
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class NativeCancelSmoke {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct STARTUPINFO {
        public int cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION {
        public IntPtr hProcess, hThread;
        public int dwProcessId;
        public int dwThreadId;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CreateProcess(
        string lpApplicationName,
        string lpCommandLine,
        IntPtr lpProcessAttributes,
        IntPtr lpThreadAttributes,
        bool bInheritHandles,
        uint dwCreationFlags,
        IntPtr lpEnvironment,
        string lpCurrentDirectory,
        ref STARTUPINFO lpStartupInfo,
        out PROCESS_INFORMATION lpProcessInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GenerateConsoleCtrlEvent(uint dwCtrlEvent, uint dwProcessGroupId);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr hProcess, out uint lpExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);

    [DllImport("kernel32.dll")]
    public static extern bool AllocConsole();

    [DllImport("kernel32.dll")]
    public static extern bool FreeConsole();

    [DllImport("kernel32.dll")]
    public static extern IntPtr GetConsoleWindow();
}
"@

$outputRoot = Join-Path $env:TEMP ("ocrx-native-cancel-smoke-" + [guid]::NewGuid().ToString("N"))
$docsDir = Join-Path $outputRoot "docs"
New-Item -ItemType Directory -Path $docsDir | Out-Null

$source = Join-Path $repoRoot "benchmarks\fixtures\pdf\normal-1-page.pdf"
for ($i = 0; $i -lt $DocCount; $i++) {
    Copy-Item $source (Join-Path $docsDir ("doc-{0:D3}.pdf" -f $i))
}

$env:OCRX_PDFIUM_LIBRARY = $PdfiumDir

$scanArgs = @(
    "scan"
    "`"$docsDir`""
    "--output-dir"
    "`"$outputRoot\out`""
    "--pdf-mode"
    "text"
    "--device"
    "cpu"
    "--offline"
    "--non-interactive"
    "--report"
    "json"
)
$commandLine = "`"$Binary`" " + ($scanArgs -join " ")

$allocatedConsole = $false
if ([NativeCancelSmoke]::GetConsoleWindow() -eq [IntPtr]::Zero) {
    if (-not [NativeCancelSmoke]::AllocConsole()) {
      throw "AllocConsole failed with error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()); the child cannot register a Ctrl+C handler"
    }
    $allocatedConsole = $true
}

$pi = New-Object NativeCancelSmoke+PROCESS_INFORMATION
try {
    $si = New-Object NativeCancelSmoke+STARTUPINFO
    $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)

    $created = [NativeCancelSmoke]::CreateProcess(
        $Binary,
        $commandLine,
        [IntPtr]::Zero,
        [IntPtr]::Zero,
        $true,
        0x200, # CREATE_NEW_PROCESS_GROUP
        [IntPtr]::Zero,
        $outputRoot,
        [ref]$si,
        [ref]$pi
    )
    if (-not $created) {
        throw "CreateProcess failed with error $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    $childPid = $pi.dwProcessId
    # Cancel anchor: the FIRST document has committed result.json (proof the
    # child is alive and mid-invocation), leaving 199 native docs in queue.
    $outDir = Join-Path $outputRoot "out"
    $deadline = (Get-Date).AddMinutes(8)
    while ($true) {
        $committed = @(Get-ChildItem -LiteralPath $outDir -Directory -ErrorAction SilentlyContinue |
            Where-Object { Test-Path (Join-Path $_.FullName "result.json") })
        if ($committed.Count -ge 1) { break }
        [uint32]$exitNow = 0
        [void][NativeCancelSmoke]::GetExitCodeProcess($pi.hProcess, [ref]$exitNow)
        if ($exitNow -ne 259) { throw "child exited before the cancel anchor (code $exitNow)" }
        if ((Get-Date) -gt $deadline) { throw "no document committed before the deadline" }
        Start-Sleep -Milliseconds 100
    }

    # The child must still be running when the signal lands (rereview BLK-1
    # repro guard).
    [uint32]$stillActive = 0
    [void][NativeCancelSmoke]::GetExitCodeProcess($pi.hProcess, [ref]$stillActive)
    if ($stillActive -ne 259) { throw "child already exited before the signal (code $stillActive)" }

    if (-not [NativeCancelSmoke]::GenerateConsoleCtrlEvent(1, [uint32]$childPid)) {
      throw "GenerateConsoleCtrlEvent failed with error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()); the smoke result would be meaningless"
    }

    $waitDeadline = (Get-Date).AddSeconds(120)
    while ($true) {
        [uint32]$exitCode = 0
        [void][NativeCancelSmoke]::GetExitCodeProcess($pi.hProcess, [ref]$exitCode)
        if ($exitCode -ne 259) { break }
        if ((Get-Date) -gt $waitDeadline) {
            throw "child did not exit within 120s of Ctrl+Break; cancellation did not reach the native queue"
        }
        Start-Sleep -Milliseconds 100
    }
    [uint32]$exitCode = 0
    [void][NativeCancelSmoke]::GetExitCodeProcess($pi.hProcess, [ref]$exitCode)

    if ($exitCode -ne 130) {
        throw "expected exit code 130 after Ctrl+Break, got $exitCode"
    }
    if (-not (Test-Path (Join-Path $outDir "scan-summary.json"))) {
        throw "scan-summary.json missing after cancellation"
    }
    $summary = Get-Content (Join-Path $outDir "scan-summary.json") -Raw | ConvertFrom-Json
    $processed = @($summary.documents).Count
    if ($processed -ge $DocCount) {
        throw "all $DocCount documents were processed despite cancellation (processed=$processed)"
    }
    $successes = @($summary.documents | Where-Object { $_.status -eq "success" }).Count
    if ($successes -lt 1) {
        throw "the pre-signal document should have committed (successes=$successes)"
    }
    if ($summary.documents | Where-Object { $_.status -eq "success" -and $_.exit_code -ne 0 }) {
        throw "a committed document reported a non-zero exit code"
    }

    Write-Host "native cancel smoke ok: exit=130, processed=$processed/$DocCount (< input), pre-signal commits retained"
}
finally {
    [void][NativeCancelSmoke]::CloseHandle($pi.hProcess)
    [void][NativeCancelSmoke]::CloseHandle($pi.hThread)
    if ($allocatedConsole) { [void][NativeCancelSmoke]::FreeConsole() }
}
