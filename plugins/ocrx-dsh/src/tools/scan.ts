// ocrx_scan tool (requirement §7.2/§7.3/§7.4/§7.6/§8.2; plan stage 4).
// Registration carries the pipeline timeoutMs = maxTimeoutMs (static);
// execute enforces defaultTimeoutMs itself (§7.3.6). The tool ALWAYS runs
// --offline (no download path exists in its argv, §11.3).
import { statSync } from 'node:fs';

import { buildScanArgv, deviceGuardViolation, scanMutexViolations, type ScanArgs } from '../argv/scan.js';
import { normalizeScanRun } from '../parse/result.js';
import { fetchRecommend } from '../recommend.js';
import { resolveBootstrapTarget, readinessNote, type BootstrapTarget } from '../readiness.js';
import { LIMITS, runBinaryWithTimeout } from '../run.js';
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import type { ReadinessInfo, ScanToolResult } from '../types.js';
import { resolveRunFace, runEnv } from '../exec-face.js';
import type { ToolEnv } from './scan-env.js';

export type { ToolEnv } from './scan-env.js';

export function createScanTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_scan',
    description:
      'Convert PDF documents to Markdown with the local ocrx CLI. Always runs offline ' +
      '(never downloads anything). Directory input scans every PDF inside it in one batch. ' +
      'Returns a compact per-document summary plus artifact paths; it never inlines full ' +
      'Markdown bodies. overwrite allows writing into a non-empty output directory but does ' +
      'NOT delete previous artifacts. password (encrypted PDFs) is recorded in the session ' +
      'history and is visible to the model.',
    parameters: {
      input: {
        type: 'array',
        description: 'Absolute paths to PDF files, or directories (batch).',
        items: { type: 'string' },
      },
      outputDir: { type: 'string', description: 'Absolute output directory for scan artifacts.' },
      pages: {
        type: 'string',
        description:
          'Optional page selection, e.g. "1,3-5". Sample large PDFs with a few pages first.',
      },
      model: {
        type: 'string',
        description: 'Explicit model. Omit to let the CLI auto-select.',
        enum: ['ovisocr2', 'ovisocr3'],
      },
      modelProfile: {
        type: 'string',
        description: 'Explicit quality profile. Omit for the CLI default resolution.',
        enum: ['auto', 'compact', 'balanced', 'quality', 'reference'],
      },
      pdfMode: {
        type: 'string',
        description: 'PDF processing mode.',
        enum: ['auto', 'text', 'ocr'],
      },
      qualityTier: {
        type: 'string',
        description: 'Quality tier.',
        enum: ['l1', 'l2'],
      },
      device: {
        type: 'string',
        description:
          'Execution device. cuda/vulkan are unavailable in the bundled Windows CPU build ' +
          '(rejected before any process starts; use the ocrx command config for a GPU build).',
        enum: ['auto', 'cpu', 'metal', 'cuda', 'vulkan'],
      },
      resume: {
        type: 'boolean',
        description:
          'Resume an interrupted scan (page-level checkpoints). Reuse after a model install ' +
          'to keep already-scanned documents.',
      },
      overwrite: {
        type: 'boolean',
        description:
          'Allow writing into an existing non-empty output directory. This does NOT delete ' +
          'previous artifacts.',
      },
      password: {
        type: 'string',
        description:
          'Open password for encrypted PDFs. WARNING: tool arguments are recorded in the ' +
          'session history and are visible to the model.',
      },
    },
    required: ['input', 'outputDir'],
    timeoutMs: env.config.maxTimeoutMs,
    isConcurrencySafe: () => false,
    outputDescription:
      'Normalized scan result: ok/partial flags, per-document status and error reasons, ' +
      'artifact paths, and model readiness guidance when the OCR model is missing.',
    renderText: (_args, value) => textResult(value),
    validate: (args) => scanMutexViolations(args as unknown as ScanArgs),
    execute: async (rawArgs, exec) => {
      const args = rawArgs as unknown as ScanArgs;
      // 1. 工具层拒绝（不发 argv）：device 预判（静态能力事实）。
      const face = await resolveRunFace(env, exec);
      if ('error' in face) {
        return scanErrorResult(face.error.message, face.error);
      }
      const deviceViolation = deviceGuardViolation(
        args.device,
        face.binarySource,
        process.platform,
      );
      if (deviceViolation) {
        return scanErrorResult('device not available in this build', {
          reason: 'device_unavailable',
          message: deviceViolation,
          guidance: deviceViolation,
        });
      }
      // 2. 批量判定（§7.3.2：批量输入或目录输入 → stderr 逐文档失败对象）。
      const multi = args.input.length > 1 || args.input.some((input) => isDirectory(input));
      // 3. 执行（execute 内 defaultTimeoutMs 计时 → 主动终止）。
      const { argv } = buildScanArgv(args);
      const outcome = await runBinaryWithTimeout(
        face.subprocess,
        {
          argv: [face.binaryPath, ...argv],
          cwd: face.cwd,
          env: runEnv(env),
          signal: exec.signal,
          stdoutMaxBytes: LIMITS.stdout,
          stderrMaxBytes: LIMITS.stderr,
        },
        env.config.defaultTimeoutMs,
      );
      // 4. 归一化（三种 stdout 形态 + 流分流 + 冻结配对）。
      const result = normalizeScanRun(outcome, {
        timedOut: outcome.timedOut === true,
        waitedMs: outcome.waitedMs,
        cancelledByCaller: outcome.cancelledByCaller === true,
        multi,
      });
      // 5. model_missing → readiness（§8.2；四组合目标继承矩阵）。
      if (topReason(result) === 'model_missing') {
        result.readiness = await buildReadiness(env, args, exec.signal);
        if (result.readiness) {
          const error = result.error as NonNullable<ScanToolResult['error']>;
          error.guidance = `${error.guidance} ${readinessNote(result.readiness.recommend as BootstrapTarget)}`;
        }
      }
      return result;
    },
  });
}

export function scanErrorResult(text: string, error: ScanToolResult['error']): ScanToolResult {
  return {
    ok: false,
    exit_code: null,
    partial: false,
    documents: [],
    error,
    text,
  };
}

function topReason(result: ScanToolResult): string | null {
  const fromDocs = result.documents.find((document) => document.error?.reason !== undefined)?.error
    ?.reason;
  return fromDocs ?? result.error?.reason ?? null;
}

async function buildReadiness(
  env: ToolEnv,
  args: ScanArgs,
  signal: AbortSignal | undefined,
): Promise<ReadinessInfo | undefined> {
  const face = await resolveRunFace(env, { signal });
  if ('error' in face) return undefined;
  const runRecommend = async (model?: string) =>
    fetchRecommend(face.subprocess, face.binaryPath, face.cwd, runEnv(env), model, signal);
  try {
    const target = await resolveBootstrapTarget(
      args,
      async (model) => (await runRecommend(model)).plan,
      async (model) => (await runRecommend(model)).text,
    );
    return {
      model_missing: true,
      recommend: {
        model: target.model,
        profile: target.profile,
        ...(target.size_bytes !== undefined ? { size_bytes: target.size_bytes } : {}),
        ...(target.target_dir !== undefined ? { target_dir: target.target_dir } : {}),
        ...(target.size_unconfirmed === true ? { size_unconfirmed: true } : {}),
      },
    };
  } catch {
    return undefined;
  }
}

function isDirectory(input: string): boolean {
  try {
    return statSync(input).isDirectory();
  } catch {
    return false;
  }
}
