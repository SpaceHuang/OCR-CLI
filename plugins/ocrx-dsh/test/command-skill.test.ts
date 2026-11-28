// Stage 7 tests: /ocrx command content faces, skill provider registration
// and content, and the uninstall-residue self-check (requirement §8.3,
// §10.1/§10.2, §14.2 卸载无残留).
import * as assert from 'node:assert/strict';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { test } from 'node:test';
import path from 'node:path';

import { resolveConfig } from '../src/config.js';
import { createOcrxCommand } from '../src/command.js';
import { createSkillProvider } from '../src/skill.js';
import { createModelInstallTool } from '../src/tools/modelInstall.js';
import { FakeCtx, FakeSubprocess } from './harness.js';
import type { PluginContext } from '../src/types.js';

function env(command = 'ocrx'): { config: ReturnType<typeof resolveConfig>; ctx: PluginContext } {
  const ctx = new FakeCtx();
  ctx.subprocess = { spawn: new FakeSubprocess().spawn };
  return { config: resolveConfig({ command }), ctx: ctx as unknown as PluginContext };
}

test('/ocrx prints the status overview with source, switches and trust model', async () => {
  const command = createOcrxCommand(env());
  const result = await command.handler({ rawInput: '' });
  assert.equal(result.kind, 'success');
  assert.match(result.text, /@ocrx\/dsh v0\.1\.0/);
  assert.match(result.text, /source: config/);
  assert.match(result.text, /scan, doctor, model, model_install/);
  assert.match(result.text, /full user file permissions/, '信任模型声明（§11.1.2）');
  assert.match(result.text, /after your explicit consent/);
});

test('/ocrx notes the offline=true block on downloads', async () => {
  const ctx = new FakeCtx();
  ctx.subprocess = { spawn: new FakeSubprocess().spawn };
  const command = createOcrxCommand({
    config: resolveConfig({ command: 'ocrx', offline: true }),
    ctx: ctx as unknown as PluginContext,
  });
  const result = await command.handler({ rawInput: '' });
  assert.match(result.text, /BLOCKED because the plugin config sets offline=true/);
});

test('/ocrx doctor runs the self-check and lists repair actions', async () => {
  const subprocess = new FakeSubprocess();
  const ctx = new FakeCtx();
  ctx.subprocess = { spawn: subprocess.spawn };
  subprocess.enqueue({
    stdout: `${JSON.stringify({
      schema_version: 2,
      version: '0.2.0',
      model: 'ovisocr3',
      backend: 'Cpu',
      memory_gb: 16,
      cpu_available: true,
      metal_available: false,
      cache_root: 'C:/cache',
      installed_profiles: [],
      profiles: ['quality'],
    })}\n`,
    code: 0,
  });
  const command = createOcrxCommand({
    config: resolveConfig({ command: 'ocrx' }),
    ctx: ctx as unknown as PluginContext,
  });
  const result = await command.handler({ rawInput: 'doctor' });
  assert.equal(result.kind, 'success');
  assert.match(result.text, /cli_version=0\.2\.0/);
  assert.match(result.text, /No OCR model installed yet/);
  assert.match(result.text, /single-model view/);
});

test('skill provider serves the SKILL.md content (bundled, not written to disk)', async () => {
  const provider = createSkillProvider(env())(undefined);
  const candidates = await provider.list();
  assert.equal(candidates.length, 1);
  assert.equal(candidates[0]?.name, 'ocrx');
  assert.ok(candidates[0]?.invocation.modelInvocable);
  const definition = await provider.get({ name: 'ocrx' });
  assert.ok(definition?.content);
  assert.match(definition.content, /resume: true/, '批量 resume 重试规则（§10.2）');
  assert.match(definition.content, /ocrx_model_install/);
  assert.match(definition.content, /session history/, 'password 暴露面如实声明');
  assert.match(definition.content, /OvisOCR3/, '默认模型表述跟随当前产品');
  assert.match(definition.content, /ocrx scan .* --offline/, '裸 CLI 降级说明');
  assert.equal(await provider.get({ name: 'other' }), undefined);
});

test('uninstall residue self-check: the plugin only reads its own package dir', () => {
  // The provider locator points INSIDE the package (skills/); loading a
  // provider must not create files. Assert the skills dir has exactly the
  // SKILL.md and no provider-created artifacts beside it.
  const skillsDir = path.join(__dirname, '..', '..', 'skills', 'ocrx');
  assert.ok(existsSync(skillsDir));
  const entries = readdirSync(skillsDir).filter((name) => name !== 'SKILL.md');
  assert.deepEqual(entries, [], 'provider 不落盘（§10.1）');
  const stat = statSync(path.join(skillsDir, 'SKILL.md'));
  assert.ok(stat.size > 500, 'SKILL.md carries real content');
});

test('enableModelInstall=false means the download path disappears entirely', () => {
  const ctx = new FakeCtx();
  ctx.subprocess = { spawn: new FakeSubprocess().spawn };
  const config = resolveConfig({ command: 'ocrx', enableModelInstall: false });
  // Direct check on the tool factory gate in apply(): reuse the load test's
  // face here with the real plugin.
  const plugin = require('@ocrx/dsh');
  plugin.apply(ctx as unknown as PluginContext, { enableModelInstall: false, command: 'ocrx' });
  assert.ok(!ctx.registeredTools.some((tool) => tool.name === 'ocrx_model_install'));
  assert.ok(ctx.registeredTools.some((tool) => tool.name === 'ocrx_scan'));
  void createModelInstallTool;
});
