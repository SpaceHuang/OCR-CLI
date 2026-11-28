#!/usr/bin/env node
// Plugin packer (review BLOCK-2): builds AND packs @ocrx/dsh in one step so
// a CLEAN CHECKOUT produces a complete tarball. The workflow's publish-npm
// job is a fresh runner: `lib/` is gitignored and `LICENSE` lives at the
// repo root, so this script stages everything the package needs, compiles,
// packs, and ASSERTS the tarball content (entry point, licenses, patch,
// skill) before anything can reach the registry.
//
// Usage:
//   node pack-plugin.mjs --plugin-dir <plugins/ocrx-dsh> \
//     --repo-root <repo root> --version <semver> --out-dir <dir>
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';

function fail(message) {
  console.error(`pack-plugin: ${message}`);
  process.exit(1);
}

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

function normalizePathArgument(value) {
  if (typeof value !== 'string') return value;
  const stripped = value.replace(/^\\\\\?\\/, '');
  return process.platform === 'win32' ? path.normalize(stripped) : stripped;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const pluginDir = normalizePathArgument(args['plugin-dir'] ?? '.');
  const repoRoot = normalizePathArgument(args['repo-root'] ?? path.join(pluginDir, '..', '..'));
  const version = args.version;
  const outDir = normalizePathArgument(args['out-dir']);
  if (!/^[0-9]+\.[0-9]+\.[0-9]+/.test(version ?? '')) fail('--version must be a semver');
  if (!outDir) fail('--out-dir is required');
  const packageJsonPath = path.join(pluginDir, 'package.json');
  if (!existsSync(packageJsonPath)) fail(`plugin package.json not found: ${pluginDir}`);
  const manifest = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
  if (manifest.name !== '@ocrx/dsh') fail(`unexpected package name: ${manifest.name}`);
  for (const key of ['preinstall', 'install', 'postinstall', 'prepack', 'prepare']) {
    if (manifest.scripts?.[key] !== undefined) fail(`manifest must not declare ${key}`);
  }
  const licenseSource = path.join(repoRoot, 'LICENSE');
  if (!existsSync(licenseSource)) fail(`repo LICENSE not found at ${licenseSource}`);

  // 1. Compile with the plugin's OWN toolchain in ITS OWN directory (frozen
  //    lockfile install runs before this script in the workflow). tsc runs
  //    through node on its JS entry — .bin shims need cmd.exe on Windows —
  //    and types/ resolution needs the plugin's node_modules, so compiling
  //    in a detached stage would break both. lib/ is gitignored, so this
  //    ALSO works from a clean checkout: without the build there IS no
  //    package (review BLOCK-2).
  const tscJs = path.join(pluginDir, 'node_modules', 'typescript', 'bin', 'tsc');
  const libDir = path.join(pluginDir, 'lib');
  if (existsSync(tscJs)) {
    const compile = spawnSync(
      process.execPath,
      [tscJs, '-p', pluginDir],
      { encoding: 'utf8', windowsHide: true },
    );
    if (compile.status !== 0) {
      fail(`tsc failed: ${compile.stderr || compile.stdout}`);
    }
  } else if (!existsSync(path.join(libDir, 'index.js'))) {
    // No toolchain AND no previously built lib: a clean checkout without the
    // frozen-lockfile install cannot produce a package (review BLOCK-2).
    fail(
      `typescript not installed under the plugin (${tscJs}) and no prior lib/index.js — ` +
        'run the frozen-lockfile install (pnpm install --frozen-lockfile) before packing',
    );
  }
  if (!existsSync(path.join(libDir, 'index.js'))) {
    fail('no lib/index.js — the entry point would be missing from the tarball');
  }

  // 2. Stage: manifest (with the pinned version), the FRESH lib/, patch,
  //    skills, README, and the repo LICENSE (BLOCK-2: the package declares
  //    LICENSE in files — a clean checkout has no plugin-local copy).
  const stage = path.join(outDir, `.stage-plugin-${process.pid}`);
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(stage, { recursive: true });
  const manifestOut = { ...manifest, version };
  delete manifestOut.devDependencies;
  writeFileSync(path.join(stage, 'package.json'), JSON.stringify(manifestOut, null, 2));
  for (const entry of ['cordis.patch.yml', 'README.md']) {
    if (!existsSync(path.join(pluginDir, entry))) fail(`plugin file missing: ${entry}`);
    cpSync(path.join(pluginDir, entry), path.join(stage, entry));
  }
  cpSync(libDir, path.join(stage, 'lib'), { recursive: true });
  cpSync(path.join(pluginDir, 'skills'), path.join(stage, 'skills'), { recursive: true });
  cpSync(licenseSource, path.join(stage, 'LICENSE'));

  // 3. Pack through npm's own node entry (no cmd.exe path mangling).
  const npmCli = path.join(
    path.dirname(process.execPath),
    'node_modules',
    'npm',
    'bin',
    'npm-cli.js',
  );
  mkdirSync(outDir, { recursive: true });
  const pack = spawnSync(
    process.execPath,
    [npmCli, 'pack', stage, '--pack-destination', outDir, '--json'],
    { encoding: 'utf8', windowsHide: true },
  );
  if (pack.status !== 0) fail(`npm pack failed: ${pack.stderr}`);
  rmSync(stage, { recursive: true, force: true });
  let tgzPath = null;
  try {
    const parsed = JSON.parse(pack.stdout);
    tgzPath = parsed[0]?.filename ? path.join(outDir, parsed[0].filename) : null;
  } catch {
    const candidates = readdirSync(outDir).filter((name) => name.endsWith(`-${version}.tgz`));
    tgzPath = candidates.length > 0 ? path.join(outDir, candidates[candidates.length - 1]) : null;
  }
  if (!tgzPath || !existsSync(tgzPath)) fail('npm pack produced no tarball');

  // 4. Tarball content assertions (run tar from the tgz directory so drive
  //    letters never leak into GNU tar's host parsing).
  const tgzName = path.basename(tgzPath);
  const readEntry = (entry) => {
    const read = spawnSync('tar', ['-xOf', tgzName, entry], {
      cwd: path.dirname(tgzPath),
      encoding: 'utf8',
      windowsHide: true,
    });
    return read.status === 0 ? read.stdout : null;
  };
  const list = spawnSync('tar', ['-tzf', tgzName], {
    cwd: path.dirname(tgzPath),
    encoding: 'utf8',
    windowsHide: true,
  });
  if (list.status !== 0) fail(`cannot list the tarball: ${list.stderr}`);
  const entries = list.stdout.split(/\r?\n/).filter((line) => line.length > 0);
  for (const required of [
    'package/lib/index.js',
    'package/lib/tools/scan.js',
    'package/lib/config.js',
    'package/LICENSE',
    'package/cordis.patch.yml',
    'package/skills/ocrx/SKILL.md',
    'package/README.md',
    'package/package.json',
  ]) {
    if (!entries.includes(required)) {
      fail(`tarball is missing ${required} (entryCount=${entries.length})`);
    }
  }
  const packedManifest = JSON.parse(readEntry('package/package.json') ?? 'null');
  if (packedManifest?.main !== 'lib/index.js') fail('packed manifest main must be lib/index.js');
  if (packedManifest?.version !== version) fail('packed manifest version mismatch');
  if (!readEntry('package/lib/index.js')?.includes('ocrx-dsh')) {
    fail('packed lib/index.js does not look like the compiled entry');
  }
  if (!readEntry('package/LICENSE')?.includes('Apache')) {
    fail('packed LICENSE is not the repo Apache-2.0 text');
  }
  const size = statSync(tgzPath).size;
  console.log(`plugin tarball: ${tgzPath} (${size} bytes, ${entries.length} entries)`);
  console.log(tgzPath);
}

main();
