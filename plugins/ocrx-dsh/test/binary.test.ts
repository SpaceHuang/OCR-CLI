// Binary location + launcher failure detection tests (requirement §6.5,
// plan stage 3; fake-binary fixtures per D-9).
import * as assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { test } from 'node:test';
import { tmpdir } from 'node:os';
import path from 'node:path';

import {
  binaryNotFoundGuidance,
  detectLauncherFailure,
  locateBinary,
  WINDOWS_LOADER_STATUS_DLL_NOT_FOUND,
} from '../src/binary.js';

function stubExecutable(dir: string, name: string): string {
  const executable =
    process.platform === 'win32' && !name.includes('.') ? `${name}.exe` : name;
  mkdirSync(dir, { recursive: true });
  const full = path.join(dir, executable);
  writeFileSync(full, '');
  return full;
}

test('explicit config.command wins over every other source', () => {
  const result = locateBinary('C:/gpu-build/ocrx.exe');
  assert.deepEqual(result.location, { path: 'C:/gpu-build/ocrx.exe', source: 'config' });
});

test('bundled shadows PATH when the platform package is present', () => {
  const stubDir = mkdtempSync(path.join(tmpdir(), 'ocrx-bundled-'));
  try {
    const bundledPath = stubExecutable(stubDir, 'ocrx');
    const onPathDir = mkdtempSync(path.join(tmpdir(), 'ocrx-onpath-'));
    stubExecutable(onPathDir, 'ocrx');
    const previousPath = process.env.PATH;
    process.env.PATH = `${onPathDir}${path.delimiter}${previousPath ?? ''}`;
    try {
      const result = locateBinary(undefined, { resolveBundled: () => ({ path: bundledPath, source: 'bundled' }) });
      assert.equal(result.location?.source, 'bundled', 'bundled must shadow PATH (§6.5.6)');
      assert.equal(result.location?.path, bundledPath);
    } finally {
      if (previousPath === undefined) delete process.env.PATH;
      else process.env.PATH = previousPath;
      rmSync(onPathDir, { recursive: true, force: true });
    }
  } finally {
    rmSync(stubDir, { recursive: true, force: true });
  }
});

test('PATH is used when the bundled package is missing (skip face)', () => {
  const onPathDir = mkdtempSync(path.join(tmpdir(), 'ocrx-onpath-'));
  try {
    stubExecutable(onPathDir, 'ocrx');
    const previousPath = process.env.PATH;
    process.env.PATH = `${onPathDir}${path.delimiter}${previousPath ?? ''}`;
    try {
      const result = locateBinary(undefined, { resolveBundled: () => null });
      assert.equal(result.location?.source, 'path');
    } finally {
      if (previousPath === undefined) delete process.env.PATH;
      else process.env.PATH = previousPath;
    }
  } finally {
    rmSync(onPathDir, { recursive: true, force: true });
  }
});

test('known install location is the last resort', () => {
  const home = mkdtempSync(path.join(tmpdir(), 'ocrx-home-'));
  const previous = process.platform === 'win32' ? process.env.USERPROFILE : process.env.HOME;
  const previousPath = process.env.PATH;
  try {
    stubExecutable(path.join(home, '.local', 'bin'), 'ocrx');
    // Isolate PATH too: the dev/CI machine may carry a real ocrx on PATH,
    // which would win before the known-location fallback.
    process.env.PATH = String(path.delimiter);
    if (process.platform === 'win32') process.env.USERPROFILE = home;
    else process.env.HOME = home;
    const result = locateBinary(undefined, { resolveBundled: () => null });
    assert.equal(result.location?.source, 'known');
    assert.ok(result.location?.path.includes(path.join('.local', 'bin')));
  } finally {
    if (previousPath === undefined) delete process.env.PATH;
    else process.env.PATH = previousPath;
    if (previous === undefined) {
      if (process.platform === 'win32') delete process.env.USERPROFILE;
      else delete process.env.HOME;
    } else if (process.platform === 'win32') {
      process.env.USERPROFILE = previous;
    } else {
      process.env.HOME = previous;
    }
    rmSync(home, { recursive: true, force: true });
  }
});

test('all sources missing: null location, tools still registrable', () => {
  const previousPath = process.env.PATH;
  const previousHome = process.platform === 'win32' ? process.env.USERPROFILE : process.env.HOME;
  try {
    process.env.PATH = String(path.delimiter);
    if (process.platform === 'win32') process.env.USERPROFILE = mkdtempSync(path.join(tmpdir(), 'ocrx-empty-'));
    else process.env.HOME = mkdtempSync(path.join(tmpdir(), 'ocrx-empty-'));
    const result = locateBinary(undefined, { resolveBundled: () => null });
    assert.equal(result.location, null);
  } finally {
    if (previousPath === undefined) delete process.env.PATH;
    else process.env.PATH = previousPath;
    if (process.platform === 'win32' && previousHome !== undefined) process.env.USERPROFILE = previousHome;
    if (process.platform !== 'win32' && previousHome !== undefined) process.env.HOME = previousHome;
  }
});

test('launcher failure faces are detected, spawn failures are not', () => {
  // Windows loader failure with no output.
  assert.equal(
    detectLauncherFailure({ code: WINDOWS_LOADER_STATUS_DLL_NOT_FOUND, stdout: '', stderr: '', spawnFailed: false }),
    true,
    '0xC0000135 face',
  );
  // Immediate silent crash (nonzero exit, zero output on both streams).
  assert.equal(
    detectLauncherFailure({ code: 1, stdout: '', stderr: '', spawnFailed: false }),
    true,
    'silent crash face',
  );
  // Spawn itself failed (ENOENT) → binary_not_found, NOT launcher_failed.
  assert.equal(
    detectLauncherFailure({ code: null, stdout: '', stderr: '', spawnFailed: true }),
    false,
  );
  // Real CLI failure paths always print something.
  assert.equal(
    detectLauncherFailure({ code: 4, stdout: '{"schema_version":1}', stderr: '', spawnFailed: false }),
    false,
  );
  assert.equal(
    detectLauncherFailure({ code: 6, stdout: '', stderr: 'error: runtime\n', spawnFailed: false }),
    false,
  );
  // Success.
  assert.equal(
    detectLauncherFailure({ code: 0, stdout: 'ok', stderr: '', spawnFailed: false }),
    false,
  );
  // Console Ctrl+C (130) is not a launcher failure.
  assert.equal(
    detectLauncherFailure({ code: 130, stdout: '', stderr: '', spawnFailed: false }),
    false,
  );
});

test('guidance texts cover the repair paths', () => {
  const notFound = binaryNotFoundGuidance();
  assert.ok(notFound.includes('reinstall'), notFound);
  assert.ok(notFound.includes('command'), 'mentions the config escape hatch');
  assert.ok(binaryNotFoundGuidance('win32').includes('install.ps1'));
  assert.ok(binaryNotFoundGuidance('darwin').includes('install.sh'));
});
