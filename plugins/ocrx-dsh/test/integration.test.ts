// Real-CLI integration tests (plan D-12, requirement §14.1.2): gated on
// OCRX_TEST_BINARY; CI's plugin-test job builds the binary with
// `cargo build --release --locked -p ocrx` and exports the env. Scan faces
// additionally need a PDFium library (OCRX_PDFIUM_LIBRARY or the repo's
// gitignored vendor copy) — without one they skip, mirroring the Rust e2e
// gate. The full bootstrap-with-real-download is a stage-9 manual gate; the
// CI faces here use the offline gate honestly.
import * as assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { test } from 'node:test';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { resolveConfig } from '../src/config.js';
import { createDoctorTool } from '../src/tools/doctor.js';
import { createModelInstallTool } from '../src/tools/modelInstall.js';
import { createModelTool } from '../src/tools/model.js';
import { createScanTool } from '../src/tools/scan.js';
import { FakeCtx, FakeApproval } from './harness.js';
import type { PluginContext } from '../src/types.js';

const REPO_ROOT = path.resolve(__dirname, '..', '..', '..', '..');
const BINARY = process.env.OCRX_TEST_BINARY ?? '';
const PDFIUM =
  process.env.OCRX_PDFIUM_LIBRARY ??
  (process.platform === 'win32'
    ? path.join(REPO_ROOT, 'vendor', 'pdfium', 'bin', 'pdfium.dll')
    : '');
const FIXTURES = path.join(REPO_ROOT, 'benchmarks', 'fixtures', 'pdf');
const HAS_PDFIUM = PDFIUM.length > 0 && existsSync(PDFIUM);

function env(options: { approval?: FakeApproval; offline?: boolean } = {}) {
  const ctx = new FakeCtx();
  // Real seam: spawn through child_process with the handle contract.
  ctx.subprocess = realSubprocessSeam();
  ctx.approval = options.approval ?? null;
  return {
    config: resolveConfig({
      command: BINARY,
      ...(options.offline ? { offline: true } : {}),
    }),
    ctx: ctx as unknown as PluginContext,
  };
}

/** Minimal real seam over child_process matching the handle contract. */
function realSubprocessSeam(): unknown {
  const { spawn } = require('node:child_process') as typeof import('node:child_process');
  return {
    spawn(spec: {
      argv: string[];
      cwd: string;
      env?: Record<string, string>;
      signal?: AbortSignal;
      stdio?: { stdout?: { maxBytes: number }; stderr?: { maxBytes: number } };
    }) {
      const program = spec.argv[0] ?? '';
      const child = spawn(program, [...spec.argv.slice(1)], {
        cwd: spec.cwd,
        env: { ...process.env, ...spec.env } as NodeJS.ProcessEnv,
        signal: spec.signal,
        windowsHide: true,
      });
      let stdout = '';
      let stderr = '';
      const stdoutLimit = (spec.stdio?.stdout as { maxBytes: number } | undefined)?.maxBytes ?? 8_000_000;
      const stderrLimit = (spec.stdio?.stderr as { maxBytes: number } | undefined)?.maxBytes ?? 4_000_000;
      child.stdout?.setEncoding('utf8');
      child.stderr?.setEncoding('utf8');
      child.stdout?.on('data', (chunk: string) => {
        stdout += chunk;
      });
      child.stderr?.on('data', (chunk: string) => {
        stderr += chunk;
      });
      const done = new Promise<{ exitCode: number | null; signal: string | null }>((resolve) => {
        child.on('close', (code, signalName) => resolve({ exitCode: code, signal: signalName }));
        child.on('error', () => resolve({ exitCode: null, signal: null }));
      });
      return {
        collected: {
          stdout: { readFrom: () => ({ text: stdout.slice(-stdoutLimit), offset: 0, truncated: false }) },
          stderr: { readFrom: () => ({ text: stderr.slice(-stderrLimit), offset: 0, truncated: false }) },
        },
        done,
        terminate: () => child.kill(),
        waitForExit: async () => {
          await done;
        },
      };
    },
  };
}

function tool_of(env: { config: ReturnType<typeof resolveConfig>; ctx: PluginContext }) {
  return createModelTool(env);
}

const scanEnv = () => {
  const e = env();
  // scan walks compose(None) and does NOT read OCRX_MODEL_CACHE (C5 is P1),
  // so the cache face is isolated through the default_cache_root inputs
  // (LOCALAPPDATA on Windows, HOME on macOS) instead.
  const isolatedHome = mkdtempSync(path.join(tmpdir(), 'ocrx-it-home-'));
  e.config = {
    ...e.config,
    extraEnv: {
      ...(HAS_PDFIUM ? { OCRX_PDFIUM_LIBRARY: PDFIUM } : {}),
      OCRX_TABLE_ENGINE_CMD: '',
      ...(process.platform === 'win32' ? { LOCALAPPDATA: isolatedHome, USERPROFILE: isolatedHome } : { HOME: isolatedHome }),
    },
  };
  return e;
};

test('integration: zero-model native-text PDF succeeds end to end', { skip: !BINARY || !HAS_PDFIUM }, async () => {
  const e = scanEnv();
  const outputDir = mkdtempSync(path.join(tmpdir(), 'ocrx-it-out-'));
  try {
    const tool = createScanTool(e);
    const result = (await tool.execute(
      {
        input: [path.join(FIXTURES, 'native-plus-empty.pdf')],
        outputDir,
        pdfMode: 'text',
      },
      { signal: undefined },
    )) as {
      ok: boolean;
      documents: Array<{ status: string; markdown_path?: string; output_dir: string | null }>;
      text: string;
    };
    assert.equal(result.ok, true, result.text);
    const document = result.documents[0];
    assert.equal(document?.status, 'success');
    assert.ok(document?.markdown_path, 'markdown path returned');
    // 产物 diff 面（§14.2）：目录里真有 markdown + result.json。
    const files = readdirSync(document.output_dir ?? outputDir);
    assert.ok(files.includes('result.json'), 'result.json written');
    assert.ok(
      existsSync(document.markdown_path ?? ''),
      'markdown file written',
    );
    const markdown = readFileSync(document.markdown_path ?? '', 'utf8');
    assert.ok(markdown.length > 0, 'non-empty markdown');
  } finally {
    rmSync(outputDir, { recursive: true, force: true });
  }
});

test('integration: single-doc model_missing derives from the stdout failure object', { skip: !BINARY || !HAS_PDFIUM }, async () => {
  const e = scanEnv();
  const outputDir = mkdtempSync(path.join(tmpdir(), 'ocrx-it-mm-'));
  try {
    const tool = createScanTool(e);
    // OCR-routed single document with an EMPTY cache: stdout carries the
    // schema v1 model_missing object (single-doc contract).
    const result = (await tool.execute(
      { input: [path.join(FIXTURES, 'normal-1-page.pdf')], outputDir, pdfMode: 'ocr' },
      { signal: undefined },
    )) as { error: { reason: string; guidance: string } | null; readiness?: { recommend: { model: string } } };
    assert.equal(result.error?.reason, 'model_missing', `reason: ${result.error?.reason}`);
    assert.match(result.error?.guidance ?? '', /ocrx_model_install/);
    // 无参数 recommend 面（默认模型，不硬编码）：readiness 的 model 必须等于
    // 同机 `ocrx model recommend` 文本面返回的模型——与 runner 内存无关
    // （7 GiB CI runner 回退 ovisocr2，32 GiB 本机选 ovisocr3，均为正确行为）。
    const textFace = (await tool_of(e).execute({ action: 'recommend' }, {})) as {
      ok: boolean;
      plan: { model: string } | null;
    };
    const expectedModel = textFace.plan?.model ?? 'ovisocr3';
    assert.equal(result.readiness?.recommend.model, expectedModel, `无参数 recommend 面（同机一致）`);
  } finally {
    rmSync(outputDir, { recursive: true, force: true });
  }
});

test('integration: batch model_missing derives per-document from stderr', { skip: !BINARY || !HAS_PDFIUM }, async () => {
  const e = scanEnv();
  const outputDir = mkdtempSync(path.join(tmpdir(), 'ocrx-it-batch-'));
  try {
    const tool = createScanTool(e);
    const result = (await tool.execute(
      {
        input: [path.join(FIXTURES, 'normal-1-page.pdf'), path.join(FIXTURES, 'empty-1-page.pdf')],
        outputDir,
        pdfMode: 'ocr',
      },
      { signal: undefined },
    )) as {
      documents: Array<{ error: { reason: string } | null; status: string }>;
      scan_summary?: string;
    };
    // empty-1-page.pdf needs no model (zero-model path succeeds); the OCR
    // page fails model_missing via the stderr JSON line.
    const missing = result.documents.find((document) => document.error?.reason === 'model_missing');
    assert.ok(missing, 'model_missing derived in batch');
  } finally {
    rmSync(outputDir, { recursive: true, force: true });
  }
});

test('integration: batch retry with resume is not blocked by the output policy', { skip: !BINARY || !HAS_PDFIUM }, async () => {
  const e = scanEnv();
  const outputDir = mkdtempSync(path.join(tmpdir(), 'ocrx-it-resume-'));
  try {
    const tool = createScanTool(e);
    const args = {
      input: [path.join(FIXTURES, 'normal-1-page.pdf')],
      outputDir,
      pdfMode: 'ocr' as const,
    };
    const first = (await tool.execute(args, { signal: undefined })) as { error: { reason: string } | null };
    assert.equal(first.error?.reason, 'model_missing');
    // Second run WITH resume: must not hit the exit-9 output-policy face
    // (scan-summary.json exists), it fails with model_missing again —
    // proving the bootstrap retry rule keeps the re-entry legal.
    const second = (await tool.execute({ ...args, resume: true }, { signal: undefined })) as {
      error: { reason: string } | null;
    };
    assert.equal(second.error?.reason, 'model_missing', 'resume 重试不被退出码 9 拦截');
  } finally {
    rmSync(outputDir, { recursive: true, force: true });
  }
});

test('integration: doctor JSON parses and carries the version face', { skip: !BINARY }, async () => {
  const e = env();
  e.config = { ...e.config, extraEnv: { OCRX_MODEL_CACHE: mkdtempSync(path.join(tmpdir(), 'ocrx-it-doc-')) } };
  const tool = createDoctorTool(e);
  const result = (await tool.execute({}, {})) as { ok: boolean; cli_version: string | null };
  assert.equal(result.ok, true);
  assert.ok(result.cli_version && /^[0-9]+\.[0-9]+\.[0-9]+/.test(result.cli_version), 'C3 version face');
});

test('integration: model list/info/recommend text faces pass through', { skip: !BINARY }, async () => {
  const e = env();
  e.config = { ...e.config, extraEnv: { OCRX_MODEL_CACHE: mkdtempSync(path.join(tmpdir(), 'ocrx-it-m-')) } };
  const tool = createModelTool(e);
  const list = (await tool.execute({ action: 'list' }, {})) as { ok: boolean; output: string };
  assert.equal(list.ok, true);
  assert.match(list.output, /model=ovisocr[23]/);
  const info = (await tool.execute({ action: 'info', model: 'ovisocr3' }, {})) as { ok: boolean; output: string };
  assert.match(info.output, /profiles=quality/);
  const recommend = (await tool.execute({ action: 'recommend' }, {})) as { ok: boolean; plan: { profile: string } | null; text: string };
  assert.equal(recommend.ok, true);
  assert.ok(recommend.plan, 'C2 plan face on the real binary');
});

test('integration: install without consent stays blocked; offline gate maps install_blocked', { skip: !BINARY }, async () => {
  const cache = mkdtempSync(path.join(tmpdir(), 'ocrx-it-inst-'));
  const e = env({ offline: false, approval: new FakeApproval({ kind: 'never' }) });
  e.config = { ...e.config, extraEnv: { OCRX_MODEL_CACHE: cache } };
  const tool = createModelInstallTool(e);
  const denied = (await tool.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    error: { reason: string };
  };
  assert.equal(denied.error.reason, 'consent_denied');
  // Approved + offline config: the real CLI refuses with the offline gate
  // (exit 5, kind offline) → install_blocked, no download attempt.
  const approved = env({ offline: true, approval: new FakeApproval({ kind: 'resolve', value: 'allowed-once' }) });
  approved.config = { ...approved.config, extraEnv: { OCRX_MODEL_CACHE: cache } };
  const tool2 = createModelInstallTool(approved);
  const blocked = (await tool2.execute({ model: 'ovisocr3', modelProfile: 'quality' }, { agent: {} })) as {
    error: { reason: string; guidance: string };
  };
  assert.equal(blocked.error.reason, 'install_blocked');
  assert.match(blocked.error.guidance, /offline/i);
  rmSync(cache, { recursive: true, force: true });
});
