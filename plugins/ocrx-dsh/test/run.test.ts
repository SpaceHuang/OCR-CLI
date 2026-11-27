// run.ts tests: seam contract shapes, bounded collect, execute-side timeout
// and cancel propagation (requirement §7.3; plan stage 3).
import * as assert from 'node:assert/strict';
import { test } from 'node:test';

import { FakeSubprocess, FakeHandle, sleep } from './harness.js';
import { LIMITS, runBinary, runBinaryWithTimeout } from '../src/run.js';

const base = {
  argv: ['ocrx', 'doctor', '--report', 'json'],
  cwd: 'C:/work',
  stdoutMaxBytes: LIMITS.stdout,
  stderrMaxBytes: LIMITS.stderr,
};

test('runBinary spawns the full explicit argv through the seam spec', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ stdout: '{"ok":1}', code: 0 });
  const outcome = await runBinary(subprocess, base);
  assert.equal(outcome.code, 0);
  assert.equal(outcome.stdout, '{"ok":1}');
  assert.equal(outcome.spawnFailed, false);
  const spec = subprocess.requests[0];
  assert.ok(spec, 'one spawn');
  assert.deepEqual(spec.argv, ['ocrx', 'doctor', '--report', 'json'], 'argv[0] is the program');
  assert.equal(spec.cwd, 'C:/work');
  const stdoutMode = spec.stdio?.stdout as { maxBytes: number };
  assert.equal(stdoutMode.maxBytes, LIMITS.stdout, 'stdout is a bounded collect');
});

test('seam spawn failures surface as spawnFailed (ENOENT face)', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.spawnError = new Error('ENOENT: no such file');
  const outcome = await runBinary(subprocess, base);
  assert.equal(outcome.spawnFailed, true);
  assert.ok(outcome.spawnError);
  assert.equal(outcome.code, null);
});

test('cancel signal terminates the process (plugin-side cancellation face)', async () => {
  const subprocess = new FakeSubprocess();
  const controller = new AbortController();
  subprocess.enqueue({ stdout: 'partial', code: 0, delayMs: 5_000 });
  const run = runBinary(subprocess, { ...base, signal: controller.signal });
  await sleep(30);
  controller.abort();
  const outcome = await run;
  assert.equal(outcome.killed, true, 'terminated by the seam');
  assert.equal(subprocess.terminated.length, 1, 'terminate reached the handle');
});

test('execute-side timeout terminates at the deadline and reports waitedMs', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ code: 0, delayMs: 5_000 });
  const outcome = await runBinaryWithTimeout(subprocess, base, 80);
  assert.equal(outcome.timedOut, true);
  assert.ok((outcome.waitedMs ?? 0) >= 60, `waitedMs sane: ${outcome.waitedMs}`);
  assert.equal(outcome.timedOut === true && outcome.stdout.length, 0, 'killed run has no output to parse');
  assert.equal(subprocess.terminated.length, 1);
});

test('fast success inside the timeout is not flagged as timedOut', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ stdout: 'ok', code: 0 });
  const outcome = await runBinaryWithTimeout(subprocess, base, 10_000);
  assert.notEqual(outcome.timedOut, true);
  assert.equal(outcome.stdout, 'ok');
});

test('caller cancellation is distinguished from the timeout', async () => {
  const subprocess = new FakeSubprocess();
  const controller = new AbortController();
  subprocess.enqueue({ code: 0, delayMs: 5_000 });
  const run = runBinaryWithTimeout(subprocess, { ...base, signal: controller.signal }, 60_000);
  await sleep(30);
  controller.abort();
  const outcome = await run;
  assert.equal(outcome.cancelledByCaller, true);
  assert.notEqual(outcome.timedOut, true);
});

test('env is forwarded to the spec for extraEnv injection', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ code: 0 });
  await runBinary(subprocess, { ...base, env: { OCRX_PDFIUM_LIBRARY: 'C:/x/pdfium.dll' } });
  assert.equal(subprocess.requests[0]?.env?.OCRX_PDFIUM_LIBRARY, 'C:/x/pdfium.dll');
});

// Silence unused-import lint face for the handle type (contract reference).
export type { FakeHandle };

test('BLOCK-8: stderr collect spec carries a spill file (full-stream face)', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ code: 0 });
  await runBinary(subprocess, base);
  const stderrMode = subprocess.requests[0]?.stdio?.stderr as { maxBytes: number; spill?: { maxBytes: number } };
  assert.ok(stderrMode, 'stderr is a collect spec');
  assert.ok(stderrMode.spill, '§7.3.2: stderr must enable spill (failure objects are full-fidelity)');
  assert.ok(stderrMode.spill.maxBytes >= stderrMode.maxBytes);
});

// --- Re-review BLOCK-8: real rc.5 read faces (lossy / spillPath) -----------

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

test('lossy read with a spill file: run.ts recovers the FULL stream (not truncated)', async () => {
  const subprocess = new FakeSubprocess();
  const spillDir = mkdtempSync(path.join(tmpdir(), 'ocrx-spill-'));
  const spillPath = path.join(spillDir, 'stderr-spill.log');
  // Full stream in the spill file: two failure objects (the FULL history).
  const fullStream = ['{"schema_version":1,"code":4,"kind":"model","message":"first","cause":"offline","suggestion":null}', '{"schema_version":1,"code":3,"kind":"input","message":"second","cause":null,"suggestion":null}'].join('\n');
  writeFileSync(spillPath, fullStream);
  // In-memory tail kept only the SECOND object (prefix slid out).
  subprocess.enqueue({
    stdout: '',
    stderr: '{"schema_version":1,"code":3,"kind":"input","message":"second","cause":null,"suggestion":null}\n',
    stderrLossy: true,
    stderrSpillPath: spillPath,
    code: 3,
  });
  const outcome = await runBinary(subprocess, base);
  assert.equal(outcome.stderrTruncated, false, 'readable spill = full stream, NOT truncated');
  assert.equal(outcome.stderr, fullStream, 'stderr text recovered from spillPath');
  assert.ok(outcome.stderr.includes('"first"'), 'the lost prefix is recovered');
  rmSync(spillDir, { recursive: true, force: true });
});

test('lossy read WITHOUT a spill file: stderrTruncated=true, tail kept', async () => {
  const subprocess = new FakeSubprocess();
  const tail = '{"schema_version":1,"code":3,"kind":"input","message":"second","cause":null,"suggestion":null}\n';
  subprocess.enqueue({ stderrTail: tail, stderrLossy: true, code: 3 });
  const outcome = await runBinary(subprocess, base);
  assert.equal(outcome.stderrTruncated, true, 'lossy without spill = prefix lost');
  assert.equal(outcome.stderr, tail, 'tail text preserved');
});

test('stdout lossy with a spill file: aggregate JSON recovered from spill', async () => {
  const subprocess = new FakeSubprocess();
  const spillDir = mkdtempSync(path.join(tmpdir(), 'ocrx-spill-out-'));
  const spillPath = path.join(spillDir, 'stdout-spill.log');
  const aggregate = JSON.stringify({
    schema_version: 1, mode: 'multi', inputs: ['a.pdf'],
    output_root: 'C:/out',
    documents: [{ input: 'a.pdf', output_dir: 'C:/out/a', status: 'success', exit_code: 0 }],
    elapsed_ms: 1,
  });
  writeFileSync(spillPath, `${aggregate}\n`);
  subprocess.enqueue({ stdoutLossy: true, stdoutSpillPath: spillPath, stderr: '', code: 0 });
  const outcome = await runBinary(subprocess, base);
  assert.ok(outcome.stdout.includes('"mode":"multi"'), 'stdout recovered from spill');
  rmSync(spillDir, { recursive: true, force: true });
});

// Silence unused imports kept for the harness contract reference.
void readFileSync;
void path;

// --- Re-review-2 BLOCK-10: stdout collect must REQUEST spill ----------------

test('BLOCK-10: stdout collect spec requests a spill (full-fidelity aggregate face)', async () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ code: 0 });
  await runBinary(subprocess, base);
  const stdoutMode = subprocess.requests[0]?.stdio?.stdout as {
    maxBytes: number;
    spill?: { maxBytes: number };
  };
  assert.ok(stdoutMode, 'stdout is a collect spec');
  assert.ok(stdoutMode.spill, '§7.3.2: stdout must request spill — the host only creates spillPath for specs that ask for it');
  assert.ok(stdoutMode.spill.maxBytes >= stdoutMode.maxBytes);
});

test('BLOCK-10: fake harness refuses a scripted spill without the spec request', () => {
  const subprocess = new FakeSubprocess();
  subprocess.enqueue({ stdout: 'tail', stdoutLossy: true, stdoutSpillPath: 'C:/x/spill.log', code: 0 });
  // The harness guard throws inside spawn — runBinary reports it as a
  // spawnFailed face; assert the guard fired by calling the fake directly.
  assert.throws(
    () => subprocess.spawn({ ...base, stdio: { stdout: { maxBytes: base.stdoutMaxBytes } } }),
    /stdoutSpillPath scripted but the stdout collect spec requests no spill/,
    'no test may inject a capability the real host never produces',
  );
});
