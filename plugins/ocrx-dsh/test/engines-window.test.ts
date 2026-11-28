// D-10 engines-window test (requirement §12.3/§14.1.4): replicate dsh's
// `semver.satisfies(range, { includePrerelease: true })` semantics for the
// frozen window and pin BOTH faces — accept 0.2.2-rc.5, reject the real
// upper edge 0.2.3-alpha.1 — without installing a second dsh host.
// Source semantics: dsh-app-boot plugin-compatibility.ts:51-53 (stage-0
// spike §3), validated against the locked @deepseek-ai/dsh@0.2.2-rc.5 in
// CI's dsh-loading job.
import * as assert from 'node:assert/strict';
import { test } from 'node:test';

import { satisfies } from '../src/engines-window.js';

const FROZEN_WINDOW = '>=0.2.2-rc.5 <0.2.3-0';

test('frozen window accepts the locked dsh release 0.2.2-rc.5', () => {
  assert.equal(satisfies('0.2.2-rc.5', FROZEN_WINDOW, { includePrerelease: true }), true);
});

test('frozen window rejects the real upper edge 0.2.3-alpha.1', () => {
  assert.equal(satisfies('0.2.3-alpha.1', FROZEN_WINDOW, { includePrerelease: true }), false);
});

test('frozen window rejects the stable upper line', () => {
  assert.equal(satisfies('0.2.3', FROZEN_WINDOW, { includePrerelease: true }), false);
  assert.equal(satisfies('0.3.0', FROZEN_WINDOW, { includePrerelease: true }), false);
});

test('frozen window rejects the pre-lock prereleases', () => {
  assert.equal(satisfies('0.2.2-rc.4', FROZEN_WINDOW, { includePrerelease: true }), false);
  assert.equal(satisfies('0.2.1', FROZEN_WINDOW, { includePrerelease: true }), false);
});

test('in-window stable releases are accepted', () => {
  assert.equal(satisfies('0.2.2', FROZEN_WINDOW, { includePrerelease: true }), true);
});

test('prerelease comparison is by prerelease identifiers, then numerics', () => {
  // 0.2.2-rc.10 > 0.2.2-rc.5 (numeric, not lexicographic); -0 strips the
  // upper bound's prerelease so 0.2.3-alpha.x compares BELOW the bound.
  assert.equal(satisfies('0.2.2-rc.10', FROZEN_WINDOW, { includePrerelease: true }), true);
  assert.equal(satisfies('0.2.2-rc.1', FROZEN_WINDOW, { includePrerelease: true }), false);
  // includePrerelease=false would NOT let any prerelease in (dsh always
  // passes true — the plugin's own contract pins the flag).
  assert.equal(satisfies('0.2.2-rc.5', FROZEN_WINDOW, { includePrerelease: false }), false);
});
