#!/usr/bin/env pwsh
<#
.SYNOPSIS
Windows real Ctrl+C smoke test for the W3.3 cancel/resume contract.

.DESCRIPTION
Spawns the compiled `ocrx.exe` in a new console process group, waits until the
first page is committed, sends a real CTRL_BREAK_EVENT (which `ctrlc` maps to the
same handler as Ctrl+C), and verifies exit code 130, retained checkpoint, no
leftover `.part` files, and a successful `--resume` continuation.

This requires an installed compact OvisOCR2 profile and the co-located PDFium
DLL. It should be run from a console-capable host; the script allocates a console
for itself when none is present so the child can register a Ctrl+C handler.
#>
[CmdletBinding()]
param(
    [string]$Binary = "",
    [string]$PdfiumDir = "",
    [string]$ModelCache = "",
    [string]$Fixture = ""
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $Binary) { $Binary = Join-Path $repoRoot "target\debug\ocrx.exe" }
if (-not $PdfiumDir) { $PdfiumDir = Join-Path $repoRoot "vendor\pdfium\bin" }
if (-not $ModelCache) { $ModelCache = $env:LOCALAPPDATA }
if (-not $Fixture) { $Fixture = Join-Path $repoRoot "benchmarks\fixtures\pdf\varied-page-sizes.pdf" }

if (-not (Test-Path -LiteralPath $Binary)) { throw "missing binary: $Binary" }
if (-not (Test-Path -LiteralPath (Join-Path $PdfiumDir "pdfium.dll"))) {
    throw "missing pdfium.dll under $PdfiumDir"
}
if (-not (Test-Path -LiteralPath $Fixture)) { throw "missing fixture: $Fixture" }

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class CtrlCNative {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct STARTUPINFO {
        public int cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public int dwX, dwY, dwXSize, dwYSize;
        public int dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION {
        public IntPtr hProcess, hThread;
        public int dwProcessId, dwThreadId;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
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
    public static extern uint WaitForSingleObject(IntPtr hHandle, uint dwMilliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr hProcess, out uint lpExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);

    [DllImport("kernel32.dll")]
    public static extern IntPtr GetConsoleWindow();

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AllocConsole();

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool FreeConsole();
}
"@

$outputDir = Join-Path $env:TEMP ("ocrx-ctrl-c-smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $outputDir | Out-Null
$scanLog = Join-Path $outputDir "scan.log"

$env:OCRX_PDFIUM_LIBRARY = $PdfiumDir
$env:OCRX_MODEL_CACHE = $ModelCache

$scanArgs = @(
    "scan"
    "`"$Fixture`""
    "--output-dir"
    "`"$outputDir`""
    "--pages"
    "1,2"
    "--model-profile"
    "compact"
    "--device"
    "cpu"
    "--offline"
    "--non-interactive"
    "--report"
    "json"
    "--max-image-tokens"
    "128"
    "--max-output-tokens"
    "128"
)
$commandLine = "`"$Binary`" " + ($scanArgs -join " ")

$allocatedConsole = $false
if ([CtrlCNative]::GetConsoleWindow() -eq [IntPtr]::Zero) {
    [void][CtrlCNative]::AllocConsole()
    $allocatedConsole = $true
}

$pi = New-Object CtrlCNative+PROCESS_INFORMATION
try {
    $si = New-Object CtrlCNative+STARTUPINFO
    $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
    $logStream = [System.IO.File]::Open($scanLog, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    $logHandle = $logStream.SafeFileHandle.DangerousGetHandle()
    $si.dwFlags = 0x100 # STARTF_USESTDHANDLES
    $si.hStdOutput = $logHandle
    $si.hStdError = $logHandle
    $created = [CtrlCNative]::CreateProcess(
        $Binary,
        $commandLine,
        [IntPtr]::Zero,
        [IntPtr]::Zero,
        $true,
        0x200, # CREATE_NEW_PROCESS_GROUP
        [IntPtr]::Zero,
        $repoRoot,
        [ref]$si,
        [ref]$pi
    )
    if (-not $created) {
        $logStream.Dispose()
        throw "CreateProcess failed with error $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $logStream.Dispose()

    $childPid = $pi.dwProcessId
    $firstCheckpoint = Join-Path $outputDir "page-0001.checkpoint"
    $deadline = (Get-Date).AddMinutes(12)
    while (-not (Test-Path -LiteralPath $firstCheckpoint)) {
        if ((Get-Date) -gt $deadline) {
            throw "page 1 did not commit before deadline; output was not produced"
        }
        Start-Sleep -Seconds 2
    }

    # CTRL_BREAK_EVENT (1) targets the new process group reliably on Windows.
    [void][CtrlCNative]::GenerateConsoleCtrlEvent(1, [uint32]$childPid)

    [void][CtrlCNative]::WaitForSingleObject($pi.hProcess, 120000)
    [uint32]$exitCode = 0
    [void][CtrlCNative]::GetExitCodeProcess($pi.hProcess, [ref]$exitCode)

    if ($exitCode -ne 130) {
        throw "expected exit code 130 after Ctrl+C, got $exitCode"
    }
    if (-not (Test-Path -LiteralPath $firstCheckpoint)) {
        throw "completed page checkpoint was not retained after cancellation"
    }
    if (Get-ChildItem -LiteralPath $outputDir -Recurse -Filter "*.part*" -ErrorAction SilentlyContinue) {
        throw "temporary .part files were left behind after cancellation"
    }

    # Resume must reuse page 1 and finish page 2.
    & $Binary scan $Fixture --output-dir $outputDir --pages 1,2 --model-profile compact `
        --device cpu --offline --non-interactive --report json --resume `
        --max-image-tokens 128 --max-output-tokens 128 2>&1 | Out-Null
    $resumeCode = $LASTEXITCODE
    if ($resumeCode -notin @(0, 10)) {
        throw "resume scan failed with exit code $resumeCode"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $outputDir "page-0002.checkpoint"))) {
        throw "resume scan did not produce page 2 checkpoint"
    }

    Write-Host "ctrl-c smoke ok: cancelled=130, checkpoint retained, resume completed (code $resumeCode)"
}
finally {
    [void][CtrlCNative]::CloseHandle($pi.hProcess)
    [void][CtrlCNative]::CloseHandle($pi.hThread)
    if ($allocatedConsole) { [void][CtrlCNative]::FreeConsole() }
    Remove-Item -LiteralPath $outputDir -Recurse -Force -ErrorAction SilentlyContinue
}
