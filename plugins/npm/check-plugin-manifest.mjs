#!/usr/bin/env node
// Manifest checker for @ocrx/dsh (invoked by scripts/test-release-scripts.sh).
// Keeps the frozen manifest contract in one executable place: engines window,
// nested dsh.bundle shape (stage-0 spike finding), platforms, exact
// optionalDependencies pins (D-3), public publishConfig, and the no-build-
// scripts policy (requirement §5.3.2 / pnpm allowBuilds).
import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const manifest = JSON.parse(readFileSync(process.argv[2] ?? '', 'utf8'));
assert.equal(manifest.name, '@ocrx/dsh');
assert.equal(manifest.main, 'lib/index.js');
assert.equal(manifest.exports['.'].default, './lib/index.js');
assert.equal(manifest.engines.dsh, '>=0.2.2-rc.5 <0.2.3-0');
assert.equal(manifest.dsh.bundle.patch, './cordis.patch.yml');
assert.deepEqual(manifest.dsh.bundle.platforms, ['win32', 'darwin']);
assert.equal(manifest.publishConfig.access, 'public');
assert.equal(manifest.peerDependencies['@deepseek-ai/dsh'], '*');
for (const pin of Object.values(manifest.optionalDependencies)) {
  assert.match(pin, /^[0-9]+\.[0-9]+\.[0-9]+$/, `optionalDependencies pins must be exact, got ${pin}`);
}
const scripts = Object.keys(manifest.scripts ?? {});
for (const banned of ['preinstall', 'install', 'postinstall', 'prepack', 'prepare']) {
  assert.ok(!scripts.includes(banned), `no ${banned} script allowed`);
}
assert.ok((manifest.files ?? []).includes('cordis.patch.yml'), 'cordis.patch.yml must ship');
console.log(`manifest ok: ${manifest.name} ${manifest.version}`);
