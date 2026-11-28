#!/usr/bin/env node
// C4 platform-package builder (plan stage 8): packs ONE platform's ocrx
// distribution (the same staging directory the Release archive was built
// from) into an npm tarball. Single platform per invocation — the release
// matrix jobs each pack their own platform and upload with distinct
// artifact names; cross-platform convergence happens only in the
// publish-npm job.
//
// Usage:
//   node build-platform-packages.mjs --platform win32-x64|darwin-arm64 \
//     --stage <dist staging dir> --version <semver> --out-dir <dir>
//
// Assertions (requirement §5.1, plan stage 8):
//   - exact file set per platform (Windows: exe + pdfium + three CRT DLLs +
//     licenses incl. ms-vc-runtime; macOS: bin/ocrx + lib/libpdfium.dylib +
//     licenses)
//   - the packaged SHA256SUMS matches a fresh hash of every staged file
//     (same-source proof against the Release archive)
//   - packed tarball <= 64 MiB
//   - no postinstall/preinstall/prepare scripts anywhere
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const MAX_PACKAGE_BYTES = 64 * 1024 * 1024;

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (key.startsWith('--')) {
      args[key.slice(2)] = argv[index + 1];
      index += 1;
    }
  }
  return args;
}

/**
 * MSYS/Git-Bash argument conversion can hand out extended-length paths
 * (`\\?\C:\...`); npm mis-resolves that spelling, so strip the prefix and
 * normalize separators for every incoming path argument.
 */
function normalizePathArgument(value) {
  if (typeof value !== 'string') return value;
  const stripped = value.replace(/^\\\\\?\\/, '');
  return process.platform === 'win32' ? path.normalize(stripped) : stripped;
}

function fail(message) {
  console.error(`build-platform-packages: ${message}`);
  process.exit(1);
}

const FILE_SETS = {
  'win32-x64': {
    template: 'package-win32-x64.json',
    package: '@ocrx/ocrx-win32-x64',
    os: ['win32'],
    cpu: ['x64'],
    requiredFiles: [
      'bin/ocrx.exe',
      'bin/pdfium.dll',
      'bin/msvcp140.dll',
      'bin/vcruntime140.dll',
      'bin/vcruntime140_1.dll',
      'LICENSE',
      'THIRD_PARTY_NOTICES.md',
      'SHA256SUMS',
    ],
    requiredDirs: ['licenses/pdfium', 'licenses/llama.cpp', 'licenses/ms-vc-runtime'],
  },
  'darwin-arm64': {
    template: 'package-darwin-arm64.json',
    package: '@ocrx/ocrx-darwin-arm64',
    os: ['darwin'],
    cpu: ['arm64'],
    requiredFiles: [
      'bin/ocrx',
      'lib/libpdfium.dylib',
      'LICENSE',
      'THIRD_PARTY_NOTICES.md',
      'SHA256SUMS',
    ],
    requiredDirs: ['licenses/pdfium', 'licenses/llama.cpp'],
  },
};

function assertFileSet(platform, stage) {
  const spec = FILE_SETS[platform];
  for (const relative of spec.requiredFiles) {
    const full = path.join(stage, relative);
    if (!existsSync(full) || !statSync(full).isFile()) {
      fail(`staged file missing: ${relative}`);
    }
  }
  for (const relative of spec.requiredDirs) {
    const full = path.join(stage, relative);
    if (!existsSync(full) || !statSync(full).isDirectory()) {
      fail(`staged directory missing: ${relative}`);
    }
  }
}

/** Same-source proof: every staged file's hash must equal SHA256SUMS. */
function assertSha256Sums(stage) {
  const sumsPath = path.join(stage, 'SHA256SUMS');
  const lines = readFileSync(sumsPath, 'utf8')
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0);
  const recorded = new Map();
  for (const line of lines) {
    const match = /^([0-9a-f]{64}) [ *](.+)$/.exec(line);
    if (!match) fail(`SHA256SUMS line unparsable: ${line}`);
    // Both packagers generate sums via `find .`, so entries carry a `./`
    // prefix; normalize keys to the walker's relative spelling.
    const name = match[2].startsWith('./') ? match[2].slice(2) : match[2];
    recorded.set(name, match[1]);
  }
  let covered = 0;
  walk(stage).forEach((relative) => {
    if (relative === 'SHA256SUMS') return;
    const full = path.join(stage, relative);
    const actual = createHash('sha256').update(readFileSync(full)).digest('hex');
    const expected = recorded.get(relative);
    if (expected === undefined) {
      fail(`SHA256SUMS does not cover staged file: ${relative}`);
    }
    if (expected !== actual) {
      fail(`SHA256SUMS mismatch for ${relative}: sums=${expected} actual=${actual}`);
    }
    covered += 1;
  });
  const extra = [...recorded.keys()].filter((name) => !existsSync(path.join(stage, name)));
  if (extra.length > 0) {
    fail(`SHA256SUMS lists missing files: ${extra.join(', ')}`);
  }
  console.log(`sha256 ok: ${covered} staged files match SHA256SUMS`);
}

function walk(dir, base = '') {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const relative = base === '' ? entry.name : `${base}/${entry.name}`;
    if (entry.isDirectory()) out.push(...walk(path.join(dir, entry.name), relative));
    else out.push(relative);
  }
  return out;
}

function assertNoBuildScripts(packageJson) {
  for (const key of ['preinstall', 'install', 'postinstall', 'prepack', 'postpack', 'prepare']) {
    if (packageJson.scripts && packageJson.scripts[key] !== undefined) {
      fail(`package.json must not declare a "${key}" script (pnpm allowBuilds policy)`);
    }
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const platform = args.platform;
  const stage = normalizePathArgument(args.stage);
  const version = args.version;
  const outDir = normalizePathArgument(args['out-dir']);
  if (!platform || !FILE_SETS[platform]) {
    fail(`--platform must be one of ${Object.keys(FILE_SETS).join(', ')}`);
  }
  if (!stage || !existsSync(stage)) fail('--stage must be the dist staging directory');
  if (!version || !/^[0-9]+\.[0-9]+\.[0-9]+/.test(version)) fail('--version must be a semver');
  if (!outDir) fail('--out-dir must be the tgz destination');
  const spec = FILE_SETS[platform];

  assertFileSet(platform, stage);
  assertSha256Sums(stage);

  const template = JSON.parse(readFileSync(path.join(HERE, spec.template), 'utf8'));
  if (template.name !== spec.package) fail(`template name mismatch: ${template.name}`);
  if (JSON.stringify(template.os) !== JSON.stringify(spec.os)) fail('template os mismatch');
  if (JSON.stringify(template.cpu) !== JSON.stringify(spec.cpu)) fail('template cpu mismatch');
  assertNoBuildScripts(template);
  const manifest = { ...template, version };

  mkdirSync(outDir, { recursive: true });
  // Build the work paths by JOIN (mkdirSync's recursive return can carry a
  // \\?\-normalized spelling that npm then mis-resolves).
  const workRoot = path.join(outDir, `.pack-${platform}-${process.pid}`);
  mkdirSync(workRoot, { recursive: true });
  const packageDir = path.join(workRoot, 'package');
  mkdirSync(packageDir, { recursive: true });
  for (const entry of readdirSync(stage, { withFileTypes: true })) {
    cpSync(path.join(stage, entry.name), path.join(packageDir, entry.name), {
      recursive: entry.isDirectory(),
    });
  }
  const manifestPath = path.join(packageDir, 'package.json');
  // Rendered write keeps key order stable and deterministic.
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));

  // Run npm through ITS OWN node entry (bin/npm-cli.js) with plain argv:
  // shell:true on Windows routes through cmd.exe, which mangles backslash
  // paths, and spawning npm.cmd directly needs shell for the same reason.
  // `npm pack <folder>` avoids setting a working directory (cmd.exe rejects
  // extended-length cwd paths).
  const npmCli = path.join(path.dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js');
  const packArgv = [process.execPath, npmCli, 'pack', packageDir, '--pack-destination', outDir, '--json'];
  const pack = existsSync(npmCli)
    ? spawnSync(packArgv[0], packArgv.slice(1), { encoding: 'utf8', windowsHide: true })
    : spawnSync('npm', ['pack', packageDir, '--pack-destination', outDir, '--json'], {
        encoding: 'utf8',
        shell: process.platform === 'win32',
      });
  if (pack.status !== 0) {
    console.error('DEBUG argv:', JSON.stringify(packArgv));
    console.error('DEBUG packageDir exists:', existsSync(packageDir));
    fail(`npm pack failed: ${pack.stderr}`);
  }
  rmSync(workRoot, { recursive: true, force: true });
  let tgzPath = null;
  try {
    const parsed = JSON.parse(pack.stdout);
    tgzPath = parsed[0]?.filename ? path.join(outDir, parsed[0].filename) : null;
  } catch {
    const tgz = readdirSync(outDir).filter((name) => name.endsWith(`-${version}.tgz`));
    tgzPath = tgz.length > 0 ? path.join(outDir, tgz[tgz.length - 1]) : null;
  }
  if (!tgzPath || !existsSync(tgzPath)) fail('npm pack produced no tarball');
  const size = statSync(tgzPath).size;
  if (size > MAX_PACKAGE_BYTES) {
    fail(`package exceeds the 64 MiB limit: ${size} bytes`);
  }
  console.log(`${platform}: ${tgzPath} (${size} bytes)`);
  return tgzPath;
}

const produced = main();
console.log(produced);
