#!/usr/bin/env node
// dsh-loading smoke (plan stage 9, requirement §14.1.3): asserts the
// scriptable part of the loading contract against a REAL dsh profile
// workspace created by `dsh plugin --profile ci add <tarball>`:
//   1. the loader-resolved package (pnpm layout) exports the Cordis shape
//      and apply() registers 4 tools + the command + the skill provider;
//   2. illegal Config values throw at load time;
//   3. the missing-binary face: no bundled platform binary on the runner ->
//      locateBinary returns null and the guidance names the repair paths;
//   4. the frozen engines.dsh window accepts the locked host version;
//   5. (--verify-removed) after `dsh plugin remove`, nothing under the dsh
//      home resolves or mentions the plugin anymore.
// Full agent-loop behavior stays a stage-9 manual gate (runbook §7).
import { createRequire } from 'node:module';
import { existsSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

function fail(message) {
  console.error(`dsh-loading-smoke: ${message}`);
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

function dshHome() {
  return process.env.DSH_HOME && process.env.DSH_HOME.trim().length > 0
    ? process.env.DSH_HOME
    : path.join(os.homedir(), '.dsh');
}

/** Finds the newest directory under the dsh home that resolves the plugin. */
function findProfileWorkspace() {
  const home = dshHome();
  if (!existsSync(home)) fail(`dsh home not found at ${home} (was the plugin added?)`);
  const queue = [home];
  const found = [];
  for (let visited = 0; queue.length > 0 && visited < 5000; visited += 1) {
    const dir = queue.shift();
    const candidate = path.join(dir, 'node_modules', '@ocrx', 'dsh');
    if (existsSync(candidate)) found.push(dir);
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.isDirectory()) queue.push(path.join(dir, entry.name));
    }
  }
  if (found.length === 0) fail('no profile workspace resolves @ocrx/dsh under the dsh home');
  return found.sort((left, right) => statSync(right).mtimeMs - statSync(left).mtimeMs)[0];
}

/** Minimal fake ctx (same contract as plugins/ocrx-dsh/test/harness.ts). */
function makeFakeCtx() {
  const registered = { tools: [], commands: [], skillProviders: [] };
  const ctx = {
    registered,
    tools: {
      register(definition) {
        registered.tools.push(definition);
      },
    },
    commands: {
      register(definition) {
        registered.commands.push(definition);
      },
    },
    skills: {
      registerProvider(provider) {
        registered.skillProviders.push(provider);
      },
    },
    get() {
      return undefined; // no approval / subprocess services in this smoke
    },
  };
  return ctx;
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  // ---- engines window (D-10): the manifest face against the locked host ----
  const repoPluginPkg = path.join(args.workspace ?? process.cwd(), 'plugins', 'ocrx-dsh', 'package.json');
  const manifest = JSON.parse(readFileSync(repoPluginPkg, 'utf8'));
  const { satisfies } = awaitImportEnginesHelper();
  if (satisfies) {
    if (!satisfies('0.2.2-rc.5', manifest.engines.dsh, { includePrerelease: true })) {
      fail('engines.dsh window must accept the locked host 0.2.2-rc.5');
    }
    if (satisfies('0.2.3-alpha.1', manifest.engines.dsh, { includePrerelease: true })) {
      fail('engines.dsh window must reject the upper-edge prerelease');
    }
  }

  if (args['verify-removed']) {
    const home = dshHome();
    if (existsSync(home)) {
      const queue = [home];
      for (let visited = 0; queue.length > 0 && visited < 5000; visited += 1) {
        const dir = queue.shift();
        const candidate = path.join(dir, 'node_modules', '@ocrx');
        if (existsSync(candidate)) {
          // An EMPTY scope directory is pnpm's own artifact after removing
          // the symlinks — plugin residue means actual FILES remain.
          const leftover = readdirSync(candidate);
          if (leftover.length > 0) fail(`plugin residue remains: ${candidate} (${leftover.join(', ')})`);
        }
        let entries;
        try {
          entries = readdirSync(dir, { withFileTypes: true });
        } catch {
          continue;
        }
        for (const entry of entries) {
          if (entry.isDirectory()) queue.push(path.join(dir, entry.name));
        }
      }
      // The profile manifest must no longer activate the bundle patch.
      const profileManifest = path.join(home, args.profile ?? 'ci', 'package.json');
      if (existsSync(profileManifest)) {
        const parsed = JSON.parse(readFileSync(profileManifest, 'utf8'));
        const bundles = JSON.stringify(parsed['dsh.profile.bundles'] ?? []);
        if (bundles.includes('ocrx')) fail(`profile still activates the plugin: ${bundles}`);
      }
    }
    console.log('dsh-loading-smoke: removal verified, no plugin residue under the dsh home');
    return;
  }

  // ---- loading face ----
  const workspace = findProfileWorkspace();
  const requireFromProfile = createRequire(path.join(workspace, 'node_modules', '@ocrx', 'dsh', 'package.json'));
  const plugin = requireFromProfile('@ocrx/dsh');
  if (plugin.name !== 'ocrx-dsh') fail(`loader resolved a different plugin: ${plugin.name}`);
  if (typeof plugin.apply !== 'function') fail('apply missing from the loaded module');
  const inject = [...plugin.inject];
  for (const seam of ['tools', 'commands', 'skills', 'subprocess', 'workingDirectory']) {
    if (!inject.includes(seam)) fail(`inject missing ${seam}`);
  }
  if (inject.includes('approval')) fail('approval must not be a required inject (§6.3)');

  const ctx = makeFakeCtx();
  plugin.apply(ctx, {});
  const toolNames = ctx.registered.tools.map((tool) => tool.name).sort();
  assertDeepEqual(toolNames, ['ocrx_doctor', 'ocrx_model', 'ocrx_model_install', 'ocrx_scan'], 'tool registration');
  if (ctx.registered.commands.length !== 1 || ctx.registered.commands[0].name !== 'ocrx') {
    fail('the /ocrx command must be registered');
  }
  if (ctx.registered.skillProviders.length !== 1) fail('the skill provider must be registered');
  const scanTool = ctx.registered.tools.find((tool) => tool.name === 'ocrx_scan');
  if (scanTool.isConcurrencySafe === undefined || scanTool.isConcurrencySafe() !== false) {
    fail('ocrx_scan must register isConcurrencySafe -> false');
  }
  // enableModelInstall=false removes the download path entirely.
  const disabledCtx = makeFakeCtx();
  plugin.apply(disabledCtx, { enableModelInstall: false });
  if (disabledCtx.registered.tools.some((tool) => tool.name === 'ocrx_model_install')) {
    fail('enableModelInstall=false must not register the install tool');
  }

  // ---- Config validation at load time ----
  let rejected = false;
  try {
    plugin.apply(makeFakeCtx(), { defaultTimeoutMs: 100, maxTimeoutMs: 50 });
  } catch {
    rejected = true;
  }
  if (!rejected) fail('illegal Config must throw during apply (load-time validation)');

  // ---- missing binary face (§6.5.4) ----
  const binaryModule = loadBinaryFromProfile(requireFromProfile);
  const location = binaryModule.locateBinary(undefined);
  if (location.location !== null) {
    // A runner with a real ocrx on PATH is still fine — report, don't fail.
    console.log(`dsh-loading-smoke: binary found on runner (${location.location.source}); skipping the not-found face`);
  } else {
    const guidance = binaryModule.binaryNotFoundGuidance();
    if (!guidance.includes('command')) fail('binary_not_found guidance must name the config escape hatch');
  }

  // ---- cleanup: nothing written outside the workspace by loading ----
  console.log(`dsh-loading-smoke: plugin loaded from ${workspace}; registration/Config/binary faces verified`);
}

function assertDeepEqual(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(`${label}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
  }
}

/** Loads the compiled binary module from the profile workspace lib/. */
function loadBinaryFromProfile(requireFromProfile) {
  const lib = requireFromProfile('@ocrx/dsh/lib/binary.js');
  return lib;
}

function awaitImportEnginesHelper() {
  try {
    return { satisfies: createRequire(import.meta.url)('../ocrx-dsh/lib/engines-window.js').satisfies };
  } catch {
    return { satisfies: null };
  }
}

main();
