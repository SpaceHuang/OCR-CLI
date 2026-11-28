// Loading smoke (stage 0 / plan §6 阶段 3 验收): the compiled plugin exports
// the Cordis plugin shape (name, inject, apply) and loads against a fake ctx
// without throwing. The inject list is frozen by requirement §6.3: approval
// must NOT be in it (a missing approval service would prevent the whole
// plugin from loading).
import assert from 'node:assert/strict';
import { test } from 'node:test';

import * as plugin from '@ocrx/dsh';
import { FakeCtx } from './harness.js';

test('plugin exports the cordis shape with the frozen inject list', () => {
  assert.equal(plugin.name, 'ocrx-dsh');
  assert.ok(typeof plugin.apply === 'function', 'apply must be a function');
  assert.deepEqual(
    [...plugin.inject].sort(),
    ['commands', 'skills', 'subprocess', 'tools', 'workingDirectory'].sort(),
  );
  assert.ok(!plugin.inject.includes('approval'), 'approval must not be a required inject');
});

test('apply loads against a bare fake ctx without throwing', () => {
  const ctx = new FakeCtx();
  assert.doesNotThrow(() => plugin.apply(ctx as never, {}));
});

test('apply registers the four tools, the command and the skill provider', () => {
  const ctx = new FakeCtx();
  plugin.apply(ctx as never, {});
  const toolNames = ctx.registeredTools.map((tool) => tool.name).sort();
  assert.deepEqual(toolNames, ['ocrx_doctor', 'ocrx_model', 'ocrx_model_install', 'ocrx_scan']);
  assert.deepEqual(
    ctx.registeredCommands.map((command) => command.name),
    ['ocrx'],
  );
  assert.equal(ctx.registeredSkillProviders.length, 1, 'exactly one skill provider');
});

test('enableModelInstall=false skips the install tool registration', () => {
  const ctx = new FakeCtx();
  plugin.apply(ctx as never, { enableModelInstall: false });
  const toolNames = ctx.registeredTools.map((tool) => tool.name);
  assert.ok(!toolNames.includes('ocrx_model_install'));
  assert.ok(toolNames.includes('ocrx_scan'));
});

test('config validation rejects illegal values at load time', () => {
  assert.throws(() => plugin.apply(new FakeCtx() as never, { defaultTimeoutMs: 100, maxTimeoutMs: 50 }));
  assert.throws(() => plugin.apply(new FakeCtx() as never, { defaultTimeoutMs: -1 }));
  assert.throws(() => plugin.apply(new FakeCtx() as never, { offline: 'yes' }));
  assert.throws(() => plugin.apply(new FakeCtx() as never, { command: 42 }));
});

// Real-host loading face (found by booting @deepseek-ai/dsh@0.2.2-rc.5 web):
// the host schema validator accepts ONLY this type subset — `type: 'json'`
// made registration fail with JsonSchemaError. Pin every compiled schema.
const HOST_ALLOWED_TYPES = new Set(['object', 'array', 'string', 'number', 'integer', 'boolean', 'null']);

function assertSchemaWithinHostSubset(schema: unknown, label: string): void {
  const record = schema as { type?: string; properties?: Record<string, { type?: string; items?: unknown }> };
  assert.ok(
    HOST_ALLOWED_TYPES.has(record.type ?? ''),
    `${label}.type must be in the rc.5 host subset, got ${record.type}`,
  );
  for (const [key, property] of Object.entries(record.properties ?? {})) {
    assert.ok(
      HOST_ALLOWED_TYPES.has(property.type ?? ''),
      `${label}.properties.${key}.type must be in the rc.5 host subset, got ${property.type}`,
    );
    const items = property.items as { type?: string } | undefined;
    if (items?.type !== undefined) {
      assert.ok(
        HOST_ALLOWED_TYPES.has(items.type),
        `${label}.properties.${key}.items.type must be in the rc.5 host subset, got ${items.type}`,
      );
    }
  }
}

test('every compiled tool schema stays within the rc.5 host JSON-schema subset', () => {
  // Capture the full definitions (FakeCtx stores name+options only).
  const definitions: Array<{
    name: string;
    parameters: Record<string, unknown>;
    output: { schema: Record<string, unknown> };
  }> = [];
  const ctx = {
    tools: { register(definition: { name: string; parameters: Record<string, unknown>; output: { schema: Record<string, unknown> } }) { definitions.push(definition); } },
    commands: { register() {} },
    skills: { registerProvider() {} },
    get() { return undefined; },
  };
  plugin.apply(ctx as never, {});
  assert.equal(definitions.length, 4, 'all four tools captured');
  for (const definition of definitions) {
    assertSchemaWithinHostSubset(definition.parameters, `${definition.name}.parameters`);
    assertSchemaWithinHostSubset(definition.output.schema, `${definition.name}.output.schema`);
  }
});
