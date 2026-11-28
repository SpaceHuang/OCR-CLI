// 二进制定位 (requirement §6.5; plan stage 3): config → bundled platform
// package → PATH → known install locations. A miss NEVER blocks load — the
// tools stay registered and report binary_not_found with repair guidance at
// call time. launcher_failed detection covers the Windows loader failure
// face (0xC0000135) and the silent-crash shape (§6.5.5).
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

import type { BinaryLocation } from './types.js';

export const WINDOWS_LOADER_STATUS_DLL_NOT_FOUND = 3_221_225_781; // 0xC0000135
export const NTSTATUS_FAILURE_FLOOR = 3_221_225_472; // 0xC0000000

/** The bundled platform package this build carries (D-1 naming). */
const PLATFORM_PACKAGES: Record<string, string> = {
  win32: '@ocrx/ocrx-win32-x64',
  darwin: '@ocrx/ocrx-darwin-arm64',
};

export interface LocateResult {
  location: BinaryLocation | null;
  /** The bundled platform package version when resolvable (doctor face). */
  bundledVersion: string | null;
}

function requireFromHere(): NodeRequire {
  // lib/binary.js → the package root is one level up; createRequire anchors
  // the resolution there so require.resolve finds sibling platform packages
  // installed in the profile workspace.
  return createRequire(path.join(__dirname, '..', 'package.json'));
}

function bundledBinary(): BinaryLocation | null {
  const platform = process.platform;
  const packageName = PLATFORM_PACKAGES[platform];
  if (!packageName) return null;
  let packageJsonPath: string;
  try {
    packageJsonPath = requireFromHere().resolve(`${packageName}/package.json`);
  } catch {
    return null; // optionalDependencies skipped/failed (§6.5.4 face)
  }
  let version: string | null = null;
  let packageDir: string;
  try {
    const manifest = requireFromHere()(packageJsonPath) as { version?: string };
    version = manifest.version ?? null;
    packageDir = path.dirname(packageJsonPath);
  } catch {
    return null;
  }
  const executable =
    platform === 'win32' ? path.join(packageDir, 'bin', 'ocrx.exe') : path.join(packageDir, 'bin', 'ocrx');
  if (!existsSync(executable)) return null;
  return { path: executable, source: 'bundled' };
}

function fromPath(): BinaryLocation | null {
  const pathVariable = process.env.PATH ?? '';
  const executableName = process.platform === 'win32' ? 'ocrx.exe' : 'ocrx';
  for (const directory of pathVariable.split(path.delimiter)) {
    if (!directory) continue;
    const candidate = path.join(directory, executableName);
    if (existsSync(candidate)) return { path: candidate, source: 'path' };
  }
  return null;
}

function fromKnownLocation(): BinaryLocation | null {
  const home = process.env.USERPROFILE ?? process.env.HOME;
  if (!home) return null;
  const executable =
    process.platform === 'win32'
      ? path.join(home, '.local', 'bin', 'ocrx.exe')
      : path.join(home, '.local', 'bin', 'ocrx');
  if (existsSync(executable)) return { path: executable, source: 'known' };
  return null;
}

export interface LocateOptions {
  /** Test seam: override the bundled-package resolution. */
  resolveBundled?: () => BinaryLocation | null;
}

/**
 * Locates the ocrx binary (§6.5.1-§6.5.4): an explicit `Config.command`
 * wins and is returned verbatim (spawn ENOENT surfaces at call time);
 * bundled shadows PATH (§6.5.6 — GPU users must set `command`).
 */
export function locateBinary(command?: string, options: LocateOptions = {}): LocateResult {
  const resolveBundled = options.resolveBundled ?? bundledBinary;
  const bundledVersion = bundledVersionOnly();
  if (command) {
    return { location: { path: command, source: 'config' }, bundledVersion };
  }
  const bundled = resolveBundled();
  if (bundled) {
    return { location: bundled, bundledVersion };
  }
  const onPath = fromPath();
  if (onPath) return { location: onPath, bundledVersion };
  return { location: fromKnownLocation(), bundledVersion };
}

function bundledVersionOnly(): string | null {
  const platform = process.platform;
  const packageName = PLATFORM_PACKAGES[platform];
  if (!packageName) return null;
  try {
    const require = requireFromHere();
    const manifest = require(`${packageName}/package.json`) as { version?: string };
    return manifest.version ?? null;
  } catch {
    return null;
  }
}

/**
 * launcher_failed detection (§6.5.5): the binary exists but dies before any
 * CLI output — the Windows loader face (missing CRT: 0xC0000135 and other
 * NTSTATUS failure codes) or an immediate silent crash.
 */
export function detectLauncherFailure(outcome: {
  code: number | null;
  stdout: string;
  stderr: string;
  spawnFailed: boolean;
  spawnError?: Error;
}): boolean {
  if (outcome.spawnFailed) return false; // ENOENT face → binary_not_found, not launcher
  if (outcome.code !== null && outcome.code >= NTSTATUS_FAILURE_FLOOR) return true;
  // Immediate silent crash: nonzero exit with zero output on both streams.
  // Every real CLI failure path prints something (stdout objects or stderr
  // lines), so an entirely silent nonzero exit is a launcher-level failure.
  return (
    outcome.code !== null &&
    outcome.code !== 0 &&
    outcome.code !== 130 &&
    outcome.stdout.length === 0 &&
    outcome.stderr.length === 0
  );
}

/** binary_not_found guidance (§6.5.4): repair paths, in order. */
export function binaryNotFoundGuidance(platform = process.platform): string {
  const reinstall =
    platform === 'win32'
      ? 'Reinstall the plugin ("dsh plugin update <profile> @ocrx/dsh") so the bundled Windows binary is restored, or download the GitHub Release archive and run install.ps1.'
      : 'Reinstall the plugin ("dsh plugin update <profile> @ocrx/dsh") so the bundled macOS binary is restored, or download the GitHub Release archive and run install.sh.';
  return (
    'The ocrx executable was not found for this platform. ' +
    'Fixes, in order of preference: (1) reinstall the plugin so the bundled platform package is restored; ' +
    `(2) ${reinstall} ` +
    '(3) point the plugin config at an existing binary: ocrx: { command: "C:/path/to/ocrx.exe" }. ' +
    'If you installed a GPU (CUDA) build yourself, set the command config to its path.'
  );
}

/** launcher_failed guidance (§6.5.5). */
export function launcherFailedGuidance(): string {
  return (
    'The ocrx executable was found but failed to start (missing runtime library or corrupted ' +
    'binary). Install the Microsoft Visual C++ Redistributable (Windows) and reinstall the ' +
    'plugin; if a custom command is configured, check that it points at a working ocrx build.'
  );
}
