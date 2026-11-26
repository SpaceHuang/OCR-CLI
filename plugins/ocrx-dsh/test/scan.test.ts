// Stage 4 tests: ocrx_scan argv mapping, stdout/stderr shape discrimination,
// the frozen batch pairing algorithm, the §7.6 error table, the readiness
// matrix and the timeout/cancel faces (requirement §7.2-§7.6, §8.2, §14.1.1).
import * as assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildScanArgv, deviceGuardViolation, scanMutexViolations, type ScanArgs } from '../src/argv/scan.js';
import { mapScanFailure, reasonForCodeWithoutObject } from '../src/parse/errors.js';
import { normalizeScanRun } from '../src/parse/result.js';
import { parseStderr, parseStdout } from '../src/parse/streams.js';
import { resolveBootstrapTarget, readinessNote, type PlanFetcher } from '../src/readiness.js';
import { fetchRecommend, parseTextRecommend } from '../src/recommend.js';
import type { CliFailureObject, InstallPlan, ScanToolResult } from '../src/types.js';
import { FakeCtx, FakeSubprocess, type SpawnSpec } from './harness.js';

const failure = (code: number, kind: string, message = 'm', cause: string | null = null): CliFailureObject => ({
  schema_version: 1,
  code,
  kind,
  message,
  cause,
  suggestion: null,
});

const v3 = (overrides: Record<string, unknown> = {}) => ({
  schema_version: 3,
  status: 'success',
  input: 'C:/docs/a.pdf',
  markdown_path: 'C:/out/a.md',
  output_dir: 'C:/out/a',
  pages: { total: 3, completed: 3, failed: 0 },
  warnings: ['w1'],
  elapsed_ms: 1200,
  images: { directory: 'C:/out/a/images', count: 1 },
  ...overrides,
});

const aggregate = (documents: Array<Record<string, unknown>>, outputRoot = 'C:/out') => ({
  schema_version: 1,
  mode: 'multi',
  inputs: documents.map((document) => document.input),
  output_root: outputRoot,
  documents,
  elapsed_ms: 100,
});

const runOutcome = (overrides: Partial<Parameters<typeof normalizeScanRun>[0]> = {}) => ({
  stdout: '',
  stderr: '',
  code: 0,
  killed: false,
  spawnFailed: false,
  ...overrides,
});

// --- argv 映射 ---------------------------------------------------------------

test('scan argv carries the four hardwired flags and never an offline argument', () => {
  const args: ScanArgs = {
    input: ['C:/docs/a.pdf'],
    outputDir: 'C:/out',
    model: 'ovisocr2',
    modelProfile: 'balanced',
    pdfMode: 'auto',
    qualityTier: 'l1',
    device: 'cpu',
    resume: true,
    password: 'open-sesame',
  };
  const { argv } = buildScanArgv(args);
  assert.ok(argv.includes('--offline'), '恒 --offline（§11.3 封死静默下载）');
  assert.ok(argv.includes('--non-interactive'));
  assert.ok(argv.includes('--report') && argv.includes('json'));
  assert.ok(argv.includes('--progress') && argv.includes('off'));
  // 参数顺序：位置输入在最前，硬编码旗标在最后。
  assert.equal(argv[0], 'scan');
  assert.equal(argv[1], 'C:/docs/a.pdf');
  assert.equal(argv[argv.length - 1], 'off');
  assert.ok(!argv.includes('--model-profile', 0) || argv.includes('--model-profile'));
  assert.ok(argv.includes('--password') && argv.includes('open-sesame'));
});

test('scan argv omits unset optional flags and auto profile', () => {
  const { argv } = buildScanArgv({ input: ['a.pdf'], outputDir: 'out' });
  for (const flag of ['--pages', '--model', '--model-profile', '--pdf-mode', '--quality-tier', '--device', '--resume', '--overwrite', '--password']) {
    assert.ok(!argv.includes(flag), `${flag} must be absent when unset`);
  }
  const autoProfile = buildScanArgv({ input: ['a.pdf'], outputDir: 'out', modelProfile: 'auto' });
  assert.ok(!autoProfile.argv.includes('--model-profile'), 'auto 等同于未显式指定');
});

test('resume/overwrite mutex is rejected at the tool layer', () => {
  assert.equal(scanMutexViolations({ input: ['a'], outputDir: 'o', resume: true, overwrite: true }).length, 1);
  assert.equal(scanMutexViolations({ input: ['a'], outputDir: 'o', resume: true }).length, 0);
  assert.equal(scanMutexViolations({ input: ['a'], outputDir: 'o' }).length, 0);
});

test('device guard: cuda/vulkan rejected only for the bundled Windows source', () => {
  assert.match(deviceGuardViolation('cuda', 'bundled', 'win32') ?? '', /GPU \(CUDA\) build/);
  assert.match(deviceGuardViolation('vulkan', 'bundled', 'win32') ?? '', /CPU build/);
  assert.equal(deviceGuardViolation('cuda', 'config', 'win32'), null, 'Config.command 来源放行');
  assert.equal(deviceGuardViolation('cuda', 'path', 'win32'), null, 'PATH 来源交由运行期映射');
  assert.equal(deviceGuardViolation('cuda', 'bundled', 'darwin'), null, 'macOS 无此静态拒绝');
  assert.equal(deviceGuardViolation('cpu', 'bundled', 'win32'), null);
});

// --- stdout 形态判别 ---------------------------------------------------------------

test('stdout shape discrimination covers all three faces', () => {
  const v3Shape = parseStdout(`${JSON.stringify(v3())}\n`);
  assert.equal(v3Shape.kind, 'v3');
  const aggregateShape = parseStdout(`${JSON.stringify(aggregate([{ input: 'a', output_dir: 'o', status: 'success', exit_code: 0 }]))}\n`);
  assert.equal(aggregateShape.kind, 'aggregate');
  const failureShape = parseStdout(`${JSON.stringify(failure(4, 'model'))}\n`);
  assert.equal(failureShape.kind, 'failure');
  assert.equal(parseStdout('').kind, 'empty');
  assert.equal(parseStdout('plain text output\n').kind, 'unparseable');
});

test('stderr parses JSON failure lines and keeps human lines as tail', () => {
  const parsed = parseStderr(
    [
      '[doc 1/2 C:/docs/a.pdf]',
      JSON.stringify(failure(4, 'model', 'missing', 'offline')),
      'progress bar noise',
      JSON.stringify(failure(3, 'input', 'bad input')),
    ].join('\n'),
  );
  assert.equal(parsed.objects.length, 2);
  assert.equal(parsed.objects[0]?.code, 4);
  assert.equal(parsed.objects[1]?.code, 3);
  assert.equal(parsed.humanLines.length, 2);
  assert.ok(parsed.tail.includes('progress bar noise'));
});

// --- 错误映射（§7.6 全表）---------------------------------------------------------------

test('error table maps every listed row (scan path, kind-first)', () => {
  assert.equal(mapScanFailure(failure(2, 'usage')).reason, 'usage');
  assert.equal(mapScanFailure(failure(3, 'input')).reason, 'input');
  assert.equal(mapScanFailure(failure(3, 'pdf')).reason, 'pdfium_missing');
  assert.equal(mapScanFailure(failure(3, 'pdf_native')).reason, 'pdf_native');
  assert.equal(mapScanFailure(failure(4, 'model', 'missing', 'offline')).reason, 'model_missing');
  assert.equal(mapScanFailure(failure(4, 'model', 'bad', 'unsupported_profile')).reason, 'model');
  assert.equal(mapScanFailure(failure(4, 'model', 'bad', 'runtime_initialization')).reason, 'model');
  assert.equal(mapScanFailure(failure(5, 'output')).reason, 'output');
  assert.equal(mapScanFailure(failure(9, 'output')).reason, 'output');
  const device = mapScanFailure(failure(6, 'runtime', 'The Requested Device is Unavailable here', 'device'));
  assert.equal(device.reason, 'device_unavailable', 'message match is case-insensitive');
  assert.equal(mapScanFailure(failure(130, 'x')).reason, 'interrupted');
});

test('error table: unlisted kind on a table code is cli_error, not launcher_failed', () => {
  const error = mapScanFailure(failure(4, 'weird_kind'));
  assert.equal(error.reason, 'cli_error');
  assert.equal(error.kind, 'weird_kind');
  const error6 = mapScanFailure(failure(6, 'runtime', 'memory below floor'));
  assert.equal(error6.reason, 'cli_error', 'code 6 runtime without the device message');
});

test('error table: code without a failure object falls back to code default + details_unavailable', () => {
  const input = reasonForCodeWithoutObject(3, 'error: no such file');
  assert.equal(input.reason, 'input');
  assert.equal(input.details_unavailable, true);
  assert.ok(input.cause?.includes('no such file'));
  const runtime = reasonForCodeWithoutObject(6, '');
  assert.equal(runtime.reason, 'runtime');
  assert.equal(runtime.details_unavailable, true);
  assert.equal(reasonForCodeWithoutObject(130, '').reason, 'interrupted');
  assert.equal(reasonForCodeWithoutObject(99, '').reason, 'launcher_failed', '表外 code 兜底');
});

// --- 单文档归一化 ---------------------------------------------------------------

test('single-doc v3 success normalizes without inlining bodies', () => {
  const result = normalizeScanRun(
    runOutcome({ stdout: `${JSON.stringify(v3())}\n`, code: 0 }),
    { multi: false },
  );
  assert.equal(result.ok, true);
  assert.equal(result.partial, false);
  assert.equal(result.documents.length, 1);
  const document = result.documents[0];
  assert.equal(document?.status, 'success');
  assert.equal(document?.pages, 3);
  assert.equal(document?.warnings_count, 1);
  assert.equal(document?.markdown_path, 'C:/out/a.md');
  assert.equal(document?.markdown, undefined, '正文不内联（§7.4.1）');
  assert.equal(document?.result_json, undefined, 'result.json 全文不内联');
  assert.match(result.text ?? '', /Markdown: C:\/out\/a\.md/);
});

test('single-doc v3 partial (exit 10) is ok:true + partial:true', () => {
  const result = normalizeScanRun(
    runOutcome({ stdout: `${JSON.stringify(v3({ status: 'partial', pages: { total: 5, completed: 4, failed: 1 } }))}\n`, code: 10 }),
    { multi: false },
  );
  assert.equal(result.ok, true);
  assert.equal(result.partial, true);
  assert.equal(result.documents[0]?.status, 'partial');
});

test('single-doc failure object on stdout maps through the table', () => {
  const result = normalizeScanRun(
    runOutcome({ stdout: `${JSON.stringify(failure(4, 'model', 'missing', 'offline'))}\n`, code: 4 }),
    { multi: false },
  );
  assert.equal(result.ok, false);
  assert.equal(result.error?.reason, 'model_missing');
  assert.match(result.error?.guidance ?? '', /ocrx_model_install/);
  assert.match(result.error?.guidance ?? '', /resume: true/);
});

test('exit code without a failure object carries details_unavailable', () => {
  const result = normalizeScanRun(
    runOutcome({ stdout: '', stderr: 'error: memory below floor\n', code: 6 }),
    { multi: false },
  );
  assert.equal(result.ok, false);
  assert.equal(result.error?.reason, 'runtime');
  assert.equal(result.error?.details_unavailable, true);
});

test('spawn failure maps binary_not_found and the loader face maps launcher_failed', () => {
  const spawnFailed = normalizeScanRun(
    runOutcome({ spawnFailed: true, spawnError: new Error('ENOENT'), code: null }),
    { multi: false },
  );
  assert.equal(spawnFailed.error?.reason, 'binary_not_found');
  const loader = normalizeScanRun(
    runOutcome({ code: 3221225781 }),
    { multi: false },
  );
  assert.equal(loader.error?.reason, 'launcher_failed');
});

// --- 批量归一化（冻结配对规则回归钉）---------------------------------------------------------------

test('batch mixed failures keep per-document reasons and top error is the first failure', () => {
  const stderr = [failure(4, 'model', 'missing', 'offline'), failure(3, 'input', 'bad')].map((object) => JSON.stringify(object)).join('\n');
  const stdout = aggregate([
    { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'failed', exit_code: 4 },
    { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 3 },
  ]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 3 }), { multi: true });
  assert.equal(result.ok, false);
  assert.equal(result.documents[0]?.error?.reason, 'model_missing', '不误报 input');
  assert.equal(result.documents[1]?.error?.reason, 'input');
  assert.equal(result.error?.reason, 'model_missing', '顶层取首个失败文档');
});

test('exit code 10 is ok:true + partial:true with per-document reasons', () => {
  const stderr = JSON.stringify(failure(3, 'input', 'bad'));
  const stdout = aggregate([
    { input: 'a.pdf', output_dir: 'C:/out/a', status: 'success', exit_code: 0 },
    { input: 'b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 3 },
  ]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 10 }), { multi: true });
  assert.equal(result.ok, true);
  assert.equal(result.partial, true);
  assert.equal(result.documents[1]?.error?.reason, 'input');
});

test('FIFO pairing survives two documents with the same code and different causes', () => {
  const stderr = [failure(4, 'model', 'missing', 'offline'), failure(4, 'model', 'init failed', 'runtime_initialization')]
    .map((object) => JSON.stringify(object))
    .join('\n');
  const stdout = aggregate([
    { input: 'a.pdf', output_dir: 'o/a', status: 'failed', exit_code: 4 },
    { input: 'b.pdf', output_dir: 'o/b', status: 'failed', exit_code: 4 },
  ]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 3 }), { multi: true });
  assert.equal(result.documents[0]?.error?.reason, 'model_missing', '按序配对，不按 kind 猜');
  assert.equal(result.documents[1]?.error?.reason, 'model');
});

test('fewer objects than failed documents: the tail falls back to details_unavailable', () => {
  const stderr = JSON.stringify(failure(4, 'model', 'missing', 'offline'));
  const stdout = aggregate([
    { input: 'a.pdf', output_dir: 'o/a', status: 'failed', exit_code: 4 },
    { input: 'b.pdf', output_dir: 'o/b', status: 'failed', exit_code: 3 },
  ]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 3 }), { multi: true });
  assert.equal(result.documents[0]?.error?.reason, 'model_missing', '已配对部分保持');
  assert.equal(result.documents[1]?.error?.details_unavailable, true);
  assert.equal(result.documents[1]?.exit_code, 3, '退出码不被吞掉');
});

test('more objects than failed documents: nothing is attributed (degraded)', () => {
  const stderr = [failure(3, 'input', 'bad'), failure(4, 'model', 'missing', 'offline')]
    .map((object) => JSON.stringify(object))
    .join('\n');
  const stdout = aggregate([
    { input: 'a.pdf', output_dir: 'o/a', status: 'failed', exit_code: 3 },
    { input: 'b.pdf', output_dir: 'o/b', status: 'cancelled', exit_code: 130 },
  ]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 3 }), { multi: true });
  assert.equal(result.error?.attribution_degraded, true, 'attribution_degraded 标注');
  assert.equal(result.documents[0]?.error?.details_unavailable, true, '非 130 失败文档回落 code 默认');
  assert.ok((result.stderr_tail ?? '').includes('offline'), '对象原文保留在 stderr 尾部');
});

test('[130, failure] with M == N: cancel stays interrupted, no reason shifting', () => {
  const stderr = JSON.stringify(failure(3, 'input', 'bad'));
  const stdout = aggregate([
    { input: 'a.pdf', output_dir: 'o/a', status: 'cancelled', exit_code: 130 },
    { input: 'b.pdf', output_dir: 'o/b', status: 'failed', exit_code: 3 },
  ]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 3 }), { multi: true });
  assert.equal(result.documents[0]?.error?.reason, 'interrupted', '取消文档不前移失败原因');
  assert.equal(result.documents[1]?.error?.reason, 'input');
  assert.equal(result.error?.attribution_degraded, undefined);
});

test('per-pair code mismatch degrades conservatively instead of guessing', () => {
  const stderr = JSON.stringify(failure(9, 'output', 'write failed'));
  const stdout = aggregate([{ input: 'a.pdf', output_dir: 'o/a', status: 'failed', exit_code: 3 }]);
  const result = normalizeScanRun(runOutcome({ stdout: `${JSON.stringify(stdout)}\n`, stderr, code: 3 }), { multi: true });
  assert.equal(result.documents[0]?.error?.details_unavailable, true, '失配 → 不挂错误对象');
  assert.equal(result.error?.attribution_degraded, true);
});

test('timeout and plugin-cancel faces return interrupted/timeout with empty documents', () => {
  const timeout = normalizeScanRun(runOutcome({ stdout: '', code: null, killed: true }), {
    multi: false,
    timedOut: true,
    waitedMs: 1800_000,
  });
  assert.equal(timeout.error?.reason, 'timeout');
  assert.equal(timeout.documents.length, 0);
  assert.match(timeout.error?.guidance ?? '', /pages|resume/);
  const cancelled = normalizeScanRun(runOutcome({ stdout: '', code: null, killed: true }), {
    multi: false,
    cancelledByCaller: true,
  });
  assert.equal(cancelled.error?.reason, 'interrupted');
  assert.match(cancelled.error?.guidance ?? '', /resume: true/);
});

// --- readiness 四组合矩阵 ---------------------------------------------------------------

const planFixture: InstallPlan = {
  schema_version: 1,
  action: 'recommend',
  model: 'ovisocr3',
  profile: 'quality',
  default_model: 'ovisocr3',
  revision: 'rev',
  size_bytes: 1016830848,
  target_dir: 'C:/cache/ocrx/models/ovisocr3/rev',
  assets: [],
  installed: false,
  cache_root: 'C:/cache',
};

test('readiness matrix: both explicit keep the pair verbatim', async () => {
  const calls: Array<string | undefined> = [];
  const fetchPlan: PlanFetcher = async (model) => {
    calls.push(model);
    return { ...planFixture, model: model ?? planFixture.model, profile: 'quality' };
  };
  const target = await resolveBootstrapTarget({ model: 'ovisocr2', modelProfile: 'balanced' }, fetchPlan);
  assert.deepEqual([target.model, target.profile], ['ovisocr2', 'balanced']);
  assert.equal(target.source, 'explicit');
  assert.equal(target.size_unconfirmed, true, 'plan ladder profile (quality) ≠ target → 体积待确认');
  assert.equal(target.target_dir, planFixture.target_dir, 'revision 目录仍可给');
});

test('readiness matrix: only model explicit uses recommend --model (never no-arg)', async () => {
  const calls: Array<string | undefined> = [];
  const fetchPlan: PlanFetcher = async (model) => {
    calls.push(model);
    return { ...planFixture, model: model ?? 'CROSSED', profile: 'quality' };
  };
  const target = await resolveBootstrapTarget({ model: 'ovisocr2' }, fetchPlan);
  assert.deepEqual(calls, ['ovisocr2'], '仅显式 model 查询');
  assert.equal(target.model, 'ovisocr2');
  assert.equal(target.profile, 'quality', 'plan 的 ladder profile');
  assert.equal(target.size_bytes, planFixture.size_bytes, 'profile 相符 → 体积可信');
});

test('readiness matrix: only profile explicit uses the CLI default model (not no-arg recommend)', async () => {
  const calls: Array<string | undefined> = [];
  const fetchPlan: PlanFetcher = async (model) => {
    calls.push(model);
    // 无参数 recommend 在低内存下跨到另一模型档位（受控场景）。
    return model === undefined
      ? { ...planFixture, model: 'ovisocr2', profile: 'balanced', default_model: 'ovisocr3' }
      : planFixture;
  };
  const target = await resolveBootstrapTarget({ modelProfile: 'quality' }, fetchPlan);
  assert.deepEqual(calls, [undefined], '只查默认模型面');
  assert.deepEqual([target.model, target.profile], ['ovisocr3', 'quality'], 'CLI 默认模型 + 该 profile，不随 recommend 跨模型');
});

test('readiness matrix: neither explicit rides the no-arg recommend return', async () => {
  const fetchPlan: PlanFetcher = async () => ({ ...planFixture, model: 'ovisocr2', profile: 'balanced' });
  const target = await resolveBootstrapTarget({}, fetchPlan);
  assert.deepEqual([target.model, target.profile], ['ovisocr2', 'balanced']);
  assert.equal(target.size_bytes, planFixture.size_bytes);
});

test('readiness matrix: D-11 degrade marks size unconfirmed with the text face', async () => {
  const target = await resolveBootstrapTarget(
    {},
    async () => null,
    async () => ({ model: 'ovisocr3', profile: 'quality' }),
  );
  assert.deepEqual([target.model, target.profile], ['ovisocr3', 'quality']);
  assert.equal(target.size_unconfirmed, true);
  assert.match(readinessNote(target), /size unconfirmed/);
  assert.match(readinessNote({ ...target, size_bytes: 1016830848, target_dir: 'C:/x' }), /1\.02 GB/);
});

// --- D-11 recommend 降级（同一 execute 内去旗标重试一次）-------------------------------------

test('fetchRecommend retries once without the flag on an old binary', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueueWith((spec: SpawnSpec) => {
    assert.ok(spec.argv.includes('--report'), 'first call carries the flag');
    return { code: 2, stderr: 'error: unsupported model flag: --report\n' };
  });
  subprocess.enqueueWith((spec: SpawnSpec) => {
    assert.ok(!spec.argv.includes('--report'), 'retry drops the flag');
    assert.ok(!spec.argv.includes('--model'), 'no-arg face');
    return { stdout: 'model=ovisocr3 profile=quality memory_gb=16 device=cpu\n', code: 0 };
  });
  const outcome = await fetchRecommend(subprocess, 'ocrx', 'C:/w', {}, undefined);
  assert.equal(outcome.oldBinary, true);
  assert.deepEqual(outcome.text, { model: 'ovisocr3', profile: 'quality' });
  assert.equal(outcome.plan, null);
});

test('fetchRecommend parses the plan JSON on the modern face', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ stdout: `${JSON.stringify(planFixture)}\n`, code: 0 });
  const outcome = await fetchRecommend(subprocess, 'ocrx', 'C:/w', {}, 'ovisocr3');
  assert.equal(outcome.oldBinary, false);
  assert.deepEqual(outcome.plan, planFixture);
});

test('parseTextRecommend extracts model/profile from the historical line', () => {
  assert.deepEqual(parseTextRecommend('model=ovisocr2 profile=balanced memory_gb=8 device=cpu\n'), {
    model: 'ovisocr2',
    profile: 'balanced',
  });
  assert.equal(parseTextRecommend('no match here'), null);
});

// FakeCtx referenced to keep the import honest for tool-level tests later.
void FakeCtx;
export type { ScanToolResult };
