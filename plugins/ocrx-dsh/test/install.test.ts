// Stage 5/6 tests: doctor/model tools, the consent gate, install argv
// filling, install failure mapping, stats delivery and residual reporting
// (requirement §7.5, §8.2.4, §9.2, §14.1.1).
import * as assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { test } from 'node:test';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { buildInstallArgv } from '../src/argv/install.js';
import { requestConsent } from '../src/consent.js';
import { formatBytes, residualNote, scanResidual } from '../src/residual.js';
import { resolveConfig } from '../src/config.js';
import { createDoctorTool } from '../src/tools/doctor.js';
import { createModelTool } from '../src/tools/model.js';
import { createModelInstallTool } from '../src/tools/modelInstall.js';
import { FakeApproval, FakeCtx, FakeSubprocess } from './harness.js';
import type { PluginContext } from '../src/types.js';

const doctorJson = JSON.stringify({
  schema_version: 2,
  version: '0.2.0',
  platform: 'windows',
  model: 'ovisocr3',
  backend: 'Cpu',
  memory_gb: 16,
  cpu_available: true,
  metal_available: false,
  cache_root: 'C:/cache',
  installed_profiles: [],
  recommended_profile: 'quality',
  recommended_device: 'cpu',
  profiles: ['quality'],
});

const installSummary = (downloaded = true) =>
  JSON.stringify({
    model: 'ovisocr3',
    profile: 'quality',
    revision: 'rev1',
    size_bytes: 1016830848,
    elapsed_ms: 654321,
    stage: 'done',
    download: {
      downloaded,
      per_asset: downloaded
        ? [{ role: 'model', bytes: 811843712, elapsed_ms: 540000, avg_rate_bps: 1503400.2 }]
        : [],
      total_bytes: downloaded ? 1016830848 : 0,
      avg_rate_bps: downloaded ? 1555483.7 : 0,
    },
    verify: {
      per_asset: [{ role: 'model', sha256_ok: true, size_ok: true }],
    },
  });

const planJson = JSON.stringify({
  schema_version: 1,
  action: 'recommend',
  model: 'ovisocr3',
  profile: 'quality',
  default_model: 'ovisocr3',
  revision: 'rev1',
  size_bytes: 1016830848,
  target_dir: 'C:/cache/ocrx/models/ovisocr3/rev1',
  assets: [],
  installed: false,
  cache_root: 'C:/cache',
});

interface Rig {
  subprocess: FakeSubprocess;
  approval: FakeApproval;
  env: { config: ReturnType<typeof resolveConfig>; ctx: PluginContext };
}

function rig(options: { command?: string; offline?: boolean; approval?: FakeApproval } = {}): Rig {
  const subprocess = new FakeSubprocess();
  const ctx = new FakeCtx();
  ctx.subprocess = { spawn: subprocess.spawn };
  ctx.approval = options.approval ?? null;
  return {
    subprocess,
    approval: options.approval ?? new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
    env: {
      config: resolveConfig({ ...(options.command ? { command: options.command } : {}), ...(options.offline ? { offline: true } : {}) }),
      ctx: ctx as unknown as PluginContext,
    },
  };
}

// --- ocrx_doctor -----------------------------------------------------------

test('doctor: parses the JSON report and attaches plugin-view fields', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  subprocess.enqueue({ stdout: `${doctorJson}\n`, code: 0 });
  const tool = createDoctorTool(env);
  const result = (await tool.execute({}, {})) as {
    ok: boolean;
    doctor: Record<string, unknown>;
    plugin: { binary_source: string | null; version: string };
    cli_version: string | null;
    text: string;
  };
  assert.equal(result.ok, true);
  assert.equal(result.doctor.schema_version, 2);
  assert.equal(result.plugin.version, '0.1.0');
  assert.equal(result.plugin.binary_source, 'config');
  assert.equal(result.cli_version, '0.2.0');
  assert.match(result.text, /C:\/cache/);
});

test('doctor: D-11 degrade on a pre-C3 binary (no version field)', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  const old = JSON.parse(doctorJson) as Record<string, unknown>;
  delete old.version;
  subprocess.enqueue({ stdout: `${JSON.stringify(old)}\n`, code: 0 });
  const tool = createDoctorTool(env);
  const result = (await tool.execute({}, {})) as { cli_version: string | null; cli_version_note: string | null };
  assert.equal(result.cli_version, null);
  assert.match(result.cli_version_note ?? '', /pre-C3|unknown/);
});

test('doctor argv carries --report json and never --non-interactive', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  subprocess.enqueue({ stdout: `${doctorJson}\n`, code: 0 });
  const tool = createDoctorTool(env);
  await tool.execute({}, {});
  const spec = subprocess.requests[0]!;
  assert.ok(spec.argv.includes('--report') && spec.argv.includes('json'));
  assert.ok(!spec.argv.includes('--non-interactive'), 'doctor 无此旗标');
  assert.ok(!spec.argv.includes('--offline'), 'doctor 不带 offline（仅 install/scan）');
});

// --- ocrx_model -------------------------------------------------------------

test('model: list text passthrough with --non-interactive', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  subprocess.enqueue({ stdout: 'model=ovisocr2\nmodel=ovisocr3\n', code: 0 });
  const tool = createModelTool(env);
  const result = (await tool.execute({ action: 'list' }, {})) as { ok: boolean; output: string };
  assert.equal(result.ok, true);
  assert.ok(result.output.includes('ovisocr2'));
  const spec = subprocess.requests[0]!;
  assert.ok(spec.argv.includes('--non-interactive'));
  assert.ok(!spec.argv.includes('--report'), 'list 不带 --report（文本透传）');
  assert.ok(!spec.argv.includes('--progress') && !spec.argv.includes('--model-profile'));
});

test('model: verify requires a profile (tool layer)', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  const tool = createModelTool(env);
  await assert.rejects(tool.execute({ action: 'verify' }, {}), /verify requires a profile/);
  assert.equal(subprocess.requests.length, 0);
});

test('model: recommend parses the plan and reports size/dir; profile forbidden on others', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
  const tool = createModelTool(env);
  const result = (await tool.execute({ action: 'recommend', model: 'ovisocr3' }, {})) as {
    ok: boolean;
    plan: { size_bytes: number; target_dir: string };
    text: string;
  };
  assert.equal(result.ok, true);
  assert.equal(result.plan.size_bytes, 1016830848);
  assert.match(result.text, /GB/);
  const spec = subprocess.requests[0]!;
  assert.ok(spec.argv.includes('--report') && spec.argv.includes('json'), 'recommend 携带计划面');
  await assert.rejects(
    (() => {
      const subprocess2 = new FakeSubprocess();
      const rig2 = rig({ command: 'ocrx' });
      rig2.subprocess = subprocess2;
      const tool2 = createModelTool(rig2.env);
      return tool2.execute({ action: 'list', profile: 'quality' }, {});
    })(),
    /only accepted with the verify action/,
  );
});

// --- 同意门（三分支）------------------------------------------------------------

test('consent: approved only on allowed-once', async () => {
  const ctx = new FakeCtx() as unknown as PluginContext;
  (ctx as unknown as { approval: unknown }).approval = new FakeApproval({ kind: 'resolve', value: 'allowed-once' });
  const result = await requestConsent(ctx, { title: 't', body: 'b', toolName: 'x', agent: {} });
  assert.equal(result.outcome, 'approved');
});

test('consent: rejected maps user rejection AND the never policy', async () => {
  const ctx = new FakeCtx() as unknown as PluginContext;
  (ctx as unknown as { approval: unknown }).approval = new FakeApproval({ kind: 'never' });
  const result = await requestConsent(ctx, { title: 't', body: 'b', toolName: 'x', agent: {} });
  assert.equal(result.outcome, 'rejected');
  assert.match(result.detail, /never/);
});

test('consent: unavailable on missing service, throw and undefined outcome (fail closed)', async () => {
  const absent = new FakeCtx() as unknown as PluginContext;
  const missing = await requestConsent(absent, { title: 't', body: 'b', toolName: 'x', agent: {} });
  assert.equal(missing.outcome, 'unavailable');
  // request throws (outside an open turn).
  const throwing = new FakeCtx() as unknown as PluginContext;
  (throwing as unknown as { approval: unknown }).approval = new FakeApproval({
    kind: 'reject',
    error: new Error('approval.request() outside an open turn'),
  });
  const thrown = await requestConsent(throwing, { title: 't', body: 'b', toolName: 'x', agent: {} });
  assert.equal(thrown.outcome, 'unavailable');
  // No agent context.
  const withService = new FakeCtx() as unknown as PluginContext;
  (withService as unknown as { approval: unknown }).approval = new FakeApproval({ kind: 'resolve', value: 'allowed-once' });
  const noAgent = await requestConsent(withService, { title: 't', body: 'b', toolName: 'x' });
  assert.equal(noAgent.outcome, 'unavailable', '无 agent → 前置不满足');
});

test('consent: cancelled stays cancelled', async () => {
  const ctx = new FakeCtx() as unknown as PluginContext;
  (ctx as unknown as { approval: unknown }).approval = {
    request: async () => 'cancelled',
  };
  const result = await requestConsent(ctx, { title: 't', body: 'b', toolName: 'x', agent: {} });
  assert.equal(result.outcome, 'cancelled');
});

// --- install argv（profile 缺省填充/无来源结构化错误）--------------------------------------------

test('install argv: explicit model+profile verbatim, offline per config', async () => {
  const subprocess = new FakeSubprocess();
  const result = await buildInstallArgv({ model: 'ovisocr2', modelProfile: 'balanced' }, {
    offline: true,
    reportJson: true,
    fetchPlan: async () => null,
  });
  if (!('argv' in result)) throw new Error('expected argv');
  assert.deepEqual(result.argv, ['model', 'install', '--model', 'ovisocr2', '--offline', '--non-interactive', '--report', 'json', 'balanced']);
  void subprocess;
});

test('install argv: only model explicit → recommend --model face (no crossing)', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('--model') && spec.argv.includes('ovisocr2'));
    return { stdout: 'model=ovisocr2 profile=balanced memory_gb=16 device=cpu\n', code: 0 };
  });
  const result = await buildInstallArgv({ model: 'ovisocr2' }, {
    offline: false,
    reportJson: true,
    fetchPlan: async () => null,
    fallbackText: async (model) => {
      // Simulate the D-11 text face for the SAME model.
      void subprocess;
      return model === 'ovisocr2' ? { model: 'ovisocr2', profile: 'balanced' } : null;
    },
  });
  if (!('argv' in result)) throw new Error('expected argv');
  assert.ok(result.argv.includes('balanced'));
  assert.ok(result.argv.includes('--model') && result.argv.includes('ovisocr2'));
  assert.ok(!subprocess.requests.some((spec) => spec.argv.includes('--report')), 'D-11 无旗标重试面');
});

test('install argv: only profile explicit → CLI default model + that profile', async () => {
  const result = await buildInstallArgv({ modelProfile: 'balanced' }, {
    offline: false,
    reportJson: true,
    fetchPlan: async () => ({
      schema_version: 1,
      action: 'recommend',
      model: 'ovisocr2',
      profile: 'balanced',
      default_model: 'ovisocr3',
      revision: 'rev',
      size_bytes: 1,
      target_dir: 'C:/x',
      assets: [],
      installed: false,
      cache_root: 'C:/cache',
    }),
  });
  if (!('argv' in result)) throw new Error('expected argv');
  assert.ok(result.argv.includes('--model') && result.argv.includes('ovisocr3'), 'default_model 来自 plan（低内存 recommend 跨模型也不偏离）');
  assert.ok(result.argv.includes('balanced'));
});

test('install argv: no source at all → structured usage error, NO argv', async () => {
  const result = await buildInstallArgv({}, {
    offline: false,
    reportJson: true,
    fetchPlan: async () => null,
    fallbackText: async () => null,
  });
  if ('argv' in result) throw new Error('must not build argv without a target');
  assert.equal(result.error.reason, 'usage');
});

// --- install 工具：同意门接线 + stats 交付 + 失败映射 -----------------------------------------------

test('install tool: consent denied → consent_denied + offline placement guidance, no download', async () => {
  const { subprocess, env } = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'never' }),
  });
  const tool = createModelInstallTool(env);
  const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    ok: boolean;
    error: { reason: string; guidance: string };
  };
  assert.equal(result.ok, false);
  assert.equal(result.error.reason, 'consent_denied');
  assert.match(result.error.guidance, /Offline placement/);
  assert.equal(subprocess.requests.length, 1, '仅 recommend 计划面（单次预取），无 install 调用');
  assert.ok(!subprocess.requests.some((spec) => spec.argv.includes('install')));
});

test('install tool: unavailable (no approval service) → consent_denied, plugin still loaded', async () => {
  const { subprocess, env } = rig({ command: 'ocrx' });
  const tool = createModelInstallTool(env);
  const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    error: { reason: string };
  };
  assert.equal(result.error.reason, 'consent_denied');
  void subprocess;
});

test('install tool: approved → install runs, stats ride the final result', async () => {
  const { subprocess, env, approval } = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
  });
  // 1: plan face (consent body), 2: install.
  subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('install') && spec.argv.includes('quality'));
    assert.ok(spec.argv.includes('--report') && spec.argv.includes('json'), 'C2 机器可读旗标');
    assert.ok(!spec.argv.includes('--offline'), 'Config.offline=false → 不携带');
    return {
      stdout: `model=ovisocr3 profile=quality revision=rev1 status=installed\n${installSummary(true)}\n`,
      code: 0,
    };
  });
  const tool = createModelInstallTool(env);
  const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    ok: boolean;
    installed: { model: string; profile: string };
    stats: { download: { downloaded: boolean; total_bytes: number; avg_rate_bps: number }; verify: { per_asset: unknown[] } };
    note: string;
    text: string;
  };
  assert.equal(result.ok, true);
  assert.deepEqual(result.installed, { model: 'ovisocr3', profile: 'quality' });
  assert.equal(result.stats.download.downloaded, true);
  assert.equal(result.stats.download.total_bytes, 1016830848);
  assert.equal(result.stats.verify.per_asset.length, 1);
  assert.equal(approval.requests.length, 1);
  assert.match(approval.requests[0]?.body ?? '', /1\.02 GB|1016830848|GB/);
  assert.match(approval.requests[0]?.body ?? '', /cannot resume/);
  assert.match(result.note, /resume: true/);
});

test('install tool: idempotent summary keeps full stats with downloaded=false', async () => {
  const { subprocess, env } = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
  });
  subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
  subprocess.enqueue({ stdout: `model=ovisocr3 profile=quality status=installed\n${installSummary(false)}\n`, code: 0 });
  const tool = createModelInstallTool(env);
  const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    stats: { download: { downloaded: boolean } };
    note: string;
  };
  assert.equal(result.stats.download.downloaded, false);
  assert.match(result.note, /already in place|No download/);
});

test('install tool: failure objects map per the install exception rules', async () => {
  const cases: Array<{ stdout: string; code: number; reason: string; label: string }> = [
    { stdout: `${JSON.stringify({ schema_version: 1, code: 5, kind: 'offline', message: 'offline mode', cause: null })}\n`, code: 5, reason: 'install_blocked', label: 'offline object' },
    { stdout: `${JSON.stringify({ schema_version: 1, code: 5, kind: 'disk', message: 'need more space', cause: null })}\n`, code: 5, reason: 'install_blocked', label: 'disk object' },
    { stdout: `${JSON.stringify({ schema_version: 1, code: 4, kind: 'transport', message: 'connection reset', cause: null })}\n`, code: 4, reason: 'install_failed', label: 'transport object' },
    { stdout: `${JSON.stringify({ schema_version: 1, code: 4, kind: 'hash', message: 'hash mismatch', cause: null })}\n`, code: 4, reason: 'install_failed', label: 'hash object' },
    { stdout: `${JSON.stringify({ schema_version: 1, code: 4, kind: 'lock', message: 'another install holds the lock (holder pid 4242)', cause: null })}\n`, code: 4, reason: 'install_failed', label: 'lock object' },
    { stdout: `${JSON.stringify({ schema_version: 1, code: 4, kind: 'mystery', message: '??', cause: null })}\n`, code: 4, reason: 'install_failed', label: 'unlisted kind' },
  ];
  for (const testCase of cases) {
    const { subprocess, env } = rig({
      command: 'ocrx',
      approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
    });
    subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
    subprocess.enqueue({ stdout: testCase.stdout, code: testCase.code });
    const tool = createModelInstallTool(env);
    const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
      error: { reason: string };
    };
    assert.equal(result.error.reason, testCase.reason, testCase.label);
  }
});

test('install tool: pre-C2 text faces map offline/disk to install_blocked, transport to install_failed', async () => {
  const textCases: Array<{ stderr: string; code: number; reason: string; label: string }> = [
    { stderr: 'model install blocked: offline mode\n', code: 5, reason: 'install_blocked', label: 'offline text' },
    { stderr: 'model install blocked: insufficient disk\n', code: 5, reason: 'install_blocked', label: 'disk text' },
    { stderr: 'model install failed: Transport("connection reset")\n', code: 4, reason: 'install_failed', label: 'transport text' },
  ];
  for (const testCase of textCases) {
    const { subprocess, env } = rig({
      command: 'ocrx',
      approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
    });
    subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
    subprocess.enqueue({ stdout: '', stderr: testCase.stderr, code: testCase.code });
    const tool = createModelInstallTool(env);
    const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
      error: { reason: string; details_unavailable?: boolean };
    };
    assert.equal(result.error.reason, testCase.reason, testCase.label);
    assert.equal(result.error.details_unavailable, true, `${testCase.label} 过渡规则标注`);
  }
});

test('install tool: cancel reports residual .part files honestly', async () => {
  const { subprocess, env } = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'resolve', value: 'cancelled' }),
  });
  subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
  const tool = createModelInstallTool(env);
  const cancelled = (await tool.execute(
    { model: 'ovisocr3', modelProfile: 'quality' },
    { agent: {} },
  )) as { error: { reason: string } };
  assert.equal(cancelled.error.reason, 'consent_cancelled');
  // Install-cancel face: the caller aborts while the process runs.
  const rig2 = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
  });
  rig2.subprocess.enqueue({ stdout: `${planJson}\n`, code: 0 });
  rig2.subprocess.enqueue({ code: 0, delayMs: 5_000 });
  const tool2 = createModelInstallTool(rig2.env);
  const controller = new AbortController();
  const run = tool2.execute(
    { model: 'ovisocr3', modelProfile: 'quality' },
    { agent: {}, signal: controller.signal },
  );
  await new Promise((resolve) => {
    setTimeout(resolve, 30);
  });
  controller.abort();
  const result2 = (await run) as { error: { reason: string; guidance: string } };
  assert.equal(result2.error.reason, 'interrupted');
  assert.match(result2.error.guidance, /cannot resume/);
});

// --- residual ----------------------------------------------------------------

test('residual scan totals .part files across both scopes', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'ocrx-resid-'));
  try {
    mkdirSync(path.join(dir, 'profile'), { recursive: true });
    writeFileSync(path.join(dir, 'profile', 'model.gguf.install-1-abc.part-model'), 'x'.repeat(100));
    mkdirSync(path.join(dir, 'rev'), { recursive: true });
    writeFileSync(path.join(dir, 'rev', 'mmproj.gguf.install-2-def.part-mmproj'), 'y'.repeat(50));
    const report = scanResidual([path.join(dir, 'profile'), path.join(dir, 'rev')]);
    assert.equal(report.locations.length, 2);
    assert.equal(report.total_bytes, 150);
    assert.match(residualNote(report), /cleaned automatically/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('formatBytes keeps human units', () => {
  assert.equal(formatBytes(1016830848), '1.02 GB');
  assert.equal(formatBytes(2048), '2048 B');
});

// --- BLOCK-5: residual scopes derive from the plan (profile + revision) ----

test('install cancel reports BOTH scopes once each with exact totals', async () => {
  const cacheDir = mkdtempSync(path.join(tmpdir(), 'ocrx-resid-scopes-'));
  try {
    const revisionDir = path.join(cacheDir, 'rev');
    const profileDir = path.join(revisionDir, 'quality');
    mkdirSync(profileDir, { recursive: true });
    // model.gguf lives in <rev>/<profile>/, mmproj in <rev>/ — both must be
    // counted exactly once (BLOCK-5: no double count, no missing profile file).
    writeFileSync(path.join(profileDir, 'model.gguf.install-1-x.part-model'), 'x'.repeat(100));
    writeFileSync(path.join(revisionDir, 'mmproj.gguf.install-2-y.part-mmproj'), 'y'.repeat(50));
    const { subprocess, env } = rig({
      command: 'ocrx',
      approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
    });
    subprocess.enqueue({
      stdout: `${JSON.stringify({
        schema_version: 1,
        action: 'recommend',
        model: 'ovisocr3',
        profile: 'quality',
        default_model: 'ovisocr3',
        revision: 'rev1',
        size_bytes: 1016830848,
        target_dir: revisionDir,
        assets: [],
        installed: false,
        cache_root: cacheDir,
      })}\n`,
      code: 0,
    });
    subprocess.enqueue({ code: 0, delayMs: 5000 });
    const tool = createModelInstallTool(env);
    const controller = new AbortController();
    const run = tool.execute(
      { model: 'ovisocr3', modelProfile: 'quality' },
      { agent: {}, signal: controller.signal },
    );
    await new Promise((resolve) => {
      setTimeout(resolve, 30);
    });
    controller.abort();
    const result = (await run) as {
      error: { reason: string; guidance: string };
      residual?: { locations: Array<{ path: string }>; total_bytes: number };
    };
    assert.equal(result.error.reason, 'interrupted');
    assert.ok(result.residual, 'residual report present');
    assert.equal(result.residual.locations.length, 2, 'model + mmproj, each exactly once');
    assert.equal(result.residual.total_bytes, 150, 'exact byte total (100 + 50)');
    assert.ok(
      result.residual.locations.some((location) => location.path.includes('model.gguf')),
      'the profile-scoped model part is NOT missed',
    );
    assert.match(result.error.guidance, /150 B/);
  } finally {
    rmSync(cacheDir, { recursive: true, force: true });
  }
});

// --- BLOCK-6: old-CLI three-sequence degrade (recommend + install) ---------

test('old CLI: recommend and install each degrade with one flag-drop retry, consent requested once', async () => {
  const { subprocess, env, approval } = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
  });
  // 1: recommend + flag → unsupported model flag (exit 2).
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('recommend') && spec.argv.includes('--report'));
    return { code: 2, stderr: 'error: unsupported model flag: --report\n' };
  });
  // 2: recommend without flag → text success.
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('recommend') && !spec.argv.includes('--report'));
    return { stdout: 'model=ovisocr3 profile=quality memory_gb=16 device=cpu\n', code: 0 };
  });
  // 3: install + flag → unsupported model flag (exit 2, parse-time, no effects).
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('install') && spec.argv.includes('--report'));
    return { code: 2, stderr: 'error: unsupported model flag: --report\n' };
  });
  // 4: install without flag → legacy text success (no summary JSON).
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('install') && !spec.argv.includes('--report'));
    assert.ok(!spec.argv.includes('--offline'));
    return { stdout: 'model=ovisocr3 profile=quality revision=rev1 status=installed\n', code: 0 };
  });
  const tool = createModelInstallTool(env);
  const result = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    ok: boolean;
    installed?: { model: string; profile: string };
    stats?: unknown;
    note?: string;
  };
  assert.equal(result.ok, true, 'legacy install success must surface as success');
  assert.deepEqual(result.installed, { model: 'ovisocr3', profile: 'quality' });
  assert.equal(result.stats, undefined, '旧 CLI 无统计——stats 如实缺省');
  assert.match(result.note ?? '', /predate|legacy|no.*statistic/i);
  assert.equal(subprocess.requests.length, 4, '三进程序列完整');
  assert.equal(approval.requests.length, 1, '重试不再次发起同意');
});

// --- BLOCK-7: consent size from the TARGET manifest -------------------------

test('explicit non-ladder profile gets real planned size in the consent body', async () => {
  const { subprocess, env, approval } = rig({
    command: 'ocrx',
    approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }),
  });
  // The plan is fetched FOR THE REQUESTED profile (ovisocr2/compact on this
  // fake = 540 MB), NOT the ladder pick — the consent body must show it.
  subprocess.enqueueWith((spec) => {
    assert.ok(spec.argv.includes('recommend'));
    assert.ok(spec.argv.includes('compact'), '按目标 profile 取计划');
    return {
      stdout: `${JSON.stringify({
        schema_version: 1,
        action: 'recommend',
        model: 'ovisocr2',
        profile: 'compact',
        default_model: 'ovisocr3',
        revision: 'rev1',
        size_bytes: 540000000,
        target_dir: 'C:/cache/ocrx/models/ovisocr2/rev1',
        assets: [],
        installed: false,
        cache_root: 'C:/cache',
      })}\n`,
      code: 0,
    };
  });
  subprocess.enqueue({ stdout: 'model=ovisocr2 profile=compact revision=rev1 status=installed\n', code: 0 });
  const tool = createModelInstallTool(env);
  const result = (await tool.execute({ model: 'ovisocr2', modelProfile: 'compact' }, { agent: {} })) as {
    ok: boolean;
  };
  assert.equal(result.ok, true);
  assert.equal(approval.requests.length, 1);
  assert.match(approval.requests[0]?.body ?? '', /540\.0 MB/);
  assert.match(approval.requests[0]?.body ?? '', /\(compact\)/);
  assert.doesNotMatch(approval.requests[0]?.body ?? '', /unconfirmed/);
});
