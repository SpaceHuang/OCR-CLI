// 归一化 (requirement §7.4; plan §6 阶段 4 冻结规则): stdout/stderr →
// documents[] + 顶层 error。批量失败对象与文档的关联为保守 FIFO——
// 禁止按 code/kind 猜测配对；130 取消路径不假定 stderr 必有对象。
import { readFileSync, existsSync } from 'node:fs';

import { mapScanFailure, reasonForCodeWithoutObject, launcherFailedError } from './errors.js';
import { parseStderr, parseStdout, type Aggregate, type StdoutShape, type V3Result } from './streams.js';
import { detectLauncherFailure } from '../binary.js';
import type { CliFailureObject, DocumentError, NormalizedDocument, ScanToolResult } from '../types.js';
import type { RunOutcome } from '../run.js';

export interface NormalizeContext {
  /** Plugin-side cancel/timeout state (§7.4.4): killed runs have no output. */
  timedOut?: boolean;
  waitedMs?: number;
  cancelledByCaller?: boolean;
  /** Multi-document invocation (aggregate expected). */
  multi: boolean;
}

export function normalizeScanRun(outcome: RunOutcome, context: NormalizeContext): ScanToolResult {
  // 1. Plugin-side timeout/cancel (§7.4.4): the CLI buffers output until
  // process end — a killed run has NOTHING to parse.
  if (context.timedOut) {
    return {
      ok: false,
      exit_code: null,
      partial: false,
      documents: [],
      error: {
        reason: 'timeout',
        message: `The scan exceeded its time budget and was terminated after ${Math.round((context.waitedMs ?? 0) / 100) / 10}s.`,
        guidance:
          'Re-run with a pages selection (e.g. "1-5") to sample the document first, or raise the defaultTimeoutMs config; use resume: true to continue from page checkpoints.',
      },
      text: buildTimeoutText(context.waitedMs ?? 0),
    };
  }
  if (context.cancelledByCaller || outcome.killed) {
    return {
      ok: false,
      exit_code: null,
      partial: false,
      documents: [],
      error: {
        reason: 'interrupted',
        message: 'The scan was cancelled.',
        guidance: 'Re-run with resume: true to continue from the page checkpoints.',
      },
      text: 'Scan cancelled. Re-run with resume: true to continue from checkpoints.',
    };
  }
  // 2. Launch faces (§6.5): spawn refusal → binary_not_found; loader
  // failure → launcher_failed.
  if (outcome.spawnFailed) {
    const error = launcherFailedError(
      `Failed to start ocrx: ${outcome.spawnError?.message ?? 'unknown spawn failure'}`,
      'binary_not_found',
    );
    error.message = `ocrx executable not found or not executable (${outcome.spawnError?.message ?? 'spawn failed'})`;
    return {
      ok: false,
      exit_code: null,
      partial: false,
      documents: [],
      error,
      text: `ocrx binary problem: ${error.message}`,
    };
  }
  if (detectLauncherFailure(outcome)) {
    const error = launcherFailedError(
      `ocrx exited immediately without any CLI output (exit code ${outcome.code}) — launcher-level failure.`,
    );
    return {
      ok: false,
      exit_code: outcome.code,
      partial: false,
      documents: [],
      error,
      text: `ocrx launcher failure (exit ${outcome.code}). ${error.guidance}`,
    };
  }
  // 3. Parse both streams. BLOCK-8: a truncated stderr means a lost PREFIX —
  // FIFO pairing is forbidden, so the flag rides into the batch normalizer.
  const stdout = parseStdout(outcome.stdout);
  const stderr = parseStderr(outcome.stderr);
  const exitCode = outcome.code ?? -1;
  const multi = context.multi;
  if (multi) {
    return normalizeAggregate(stdout, stderr, exitCode, outcome, outcome.stderrTruncated === true);
  }
  return normalizeSingle(stdout, stderr, exitCode, outcome);
}

// --- 单文档路径 ---------------------------------------------------------------

function normalizeSingle(
  stdout: StdoutShape,
  stderr: { objects: CliFailureObject[]; tail: string },
  exitCode: number,
  outcome: RunOutcome,
): ScanToolResult {
  if (stdout.kind === 'v3') {
    const partial = exitCode === 10 || stdout.value.status === 'partial';
    const document = fromV3(stdout.value, exitCode, partial);
    return {
      ok: true,
      exit_code: exitCode,
      partial,
      documents: [document],
      error: null,
      text: buildSingleDocText(document, stdout.value),
    };
  }
  if (stdout.kind === 'failure') {
    const error = mapScanFailure(stdout.value);
    return {
      ok: false,
      exit_code: exitCode,
      partial: false,
      documents: [],
      error,
      text: `Scan failed: ${error.message}`,
    };
  }
  // 无失败对象：表内 code 走 code 默认 + details_unavailable；表外兜底。
  if (exitCode === 0) {
    // stdout 形态异常但退出码 0——unparseable 兜底（保守：cli_error）。
    return {
      ok: false,
      exit_code: exitCode,
      partial: false,
      documents: [],
      error: {
        reason: 'cli_error',
        message: 'ocrx exited 0 but produced no parseable result payload.',
        guidance: 'Retry once; if it persists this is a CLI/plugin contract bug — report it with the output below.',
        code: 0,
        details_unavailable: true,
        cause: outcome.stdout.slice(-2000) || outcome.stderr.slice(-2000) || null,
      },
      text: 'ocrx returned no parseable payload (exit 0).',
    };
  }
  const error = reasonForCodeWithoutObject(exitCode, outcome.stderr || stderr.tail);
  return {
    ok: false,
    exit_code: exitCode,
    partial: false,
    documents: [],
    error,
    text: `Scan failed (exit ${exitCode}${error.details_unavailable ? ', details unavailable' : ''}).`,
  };
}

function fromV3(value: V3Result, exitCode: number, partial: boolean): NormalizedDocument {
  return {
    input: value.input,
    output_dir: value.output_dir,
    status: partial ? 'partial' : 'success',
    exit_code: exitCode,
    pages: value.pages?.completed,
    warnings_count: value.warnings?.length ?? 0,
    duration_ms: value.elapsed_ms,
    markdown: undefined,
    result_json: undefined,
    markdown_path: value.markdown_path ?? undefined,
    images_dir: value.images?.directory,
    error: null,
  };
}

// --- 批量路径（冻结配对算法）------------------------------------------------------

function normalizeAggregate(
  stdout: StdoutShape,
  stderr: { objects: CliFailureObject[]; tail: string },
  exitCode: number,
  outcome: RunOutcome,
  stderrTruncated: boolean,
): ScanToolResult {
  if (stdout.kind !== 'aggregate') {
    // 批量 stdout 聚合缺失：整次失败按 code 兜底（不可逐文档归因）。
    if (stdout.kind === 'failure') {
      const error = mapScanFailure(stdout.value);
      return {
        ok: false,
        exit_code: exitCode,
        partial: false,
        documents: [],
        error,
        text: `Scan failed before batch execution: ${error.message}`,
      };
    }
    const error = reasonForCodeWithoutObject(exitCode, outcome.stderr || stderr.tail);
    return {
      ok: false,
      exit_code: exitCode,
      partial: false,
      documents: [],
      error,
      text: `Scan failed (exit ${exitCode}); no aggregate payload to attribute.`,
    };
  }
  const aggregate = stdout.value;
  const documents = aggregate.documents.map((document) => baseDocument(document));
  const candidates = documents.filter((document) =>
    ![0, 10, 130].includes(document.exit_code),
  );
  const cancelled = documents.filter((document) => document.exit_code === 130);
  const objects = stderr.objects;
  const N = candidates.length;
  const M = objects.length;
  const attributionDegraded: string[] = [];

  // BLOCK-8: a truncated stderr lost its PREFIX — the surviving objects may
  // belong to EARLIER documents than their FIFO positions suggest, and the
  // per-pair code check cannot detect that when codes repeat. Never pair a
  // stream whose head is gone; fall back to code defaults everywhere.
  if (stderrTruncated && objects.length > 0) {
    attributionDegraded.push(
      `stderr was truncated (prefix lost): ${M} failure objects cannot be positioned — FIFO pairing forbidden.`,
    );
    for (const document of candidates) {
      attachCodeDefault(document);
    }
    const ok = exitCode === 0 || exitCode === 10;
    const partial = exitCode === 10;
    const topError = candidates[0]?.error ?? null;
    const scanSummary = existsScanSummary(aggregate.output_root)
      ? joinPath(aggregate.output_root, 'scan-summary.json')
      : undefined;
    const degradedResult: ScanToolResult = {
      ok,
      exit_code: exitCode,
      partial,
      documents,
      scan_summary: scanSummary,
      error: topError
        ? { ...topError, attribution_degraded: true }
        : {
            reason: 'cli_error',
            message: 'Batch failure attribution degraded: the failure-object stream was truncated.',
            guidance:
              'See per-document exit-code defaults and the stderr tail; re-run smaller batches if you need exact reasons.',
            attribution_degraded: true,
          },
      stderr_tail: stderr.tail.length > 0 ? stderr.tail : undefined,
    };
    degradedResult.text = buildBatchText(degradedResult, attributionDegraded, stderr);
    return degradedResult;
  }

  if (cancelled.length === 0) {
    // 无 130 候选：FIFO 一一配对 + 逐对 code 相容校验。
    const pairs = Math.min(N, M);
    for (let index = 0; index < pairs; index += 1) {
      const document = candidates[index];
      const failure = objects[index];
      if (!document || !failure) continue;
      attachPairedError(document, failure, attributionDegraded);
    }
    // M > N（无 130 时多余对象无法归属——保守不挂）或 M < N（缺对象回落）。
    if (M > N) {
      attributionDegraded.push(
        `${M} failure objects for ${N} failed documents: extra objects are preserved on stderr, not attributed.`,
      );
      for (const document of candidates.slice(M)) {
        attachCodeDefault(document);
      }
    } else {
      for (const document of candidates.slice(M)) {
        attachCodeDefault(document);
      }
    }
  } else {
    // 有 130 候选：三分支。
    if (M === N) {
      // 对象全属非 130 文档（非 130 必发对象且总数恰等）。
      const pairs = Math.min(N, M);
      for (let index = 0; index < pairs; index += 1) {
        const document = candidates[index];
        const failure = objects[index];
        if (!document || !failure) continue;
        attachPairedError(document, failure, attributionDegraded);
      }
    } else if (M > N) {
      // 多出对象属某个 130 文档但归属不唯一 → 全部不配对。
      attributionDegraded.push(
        `${M} failure objects for ${N} non-cancelled failed documents (plus ${cancelled.length} cancelled): attribution is degraded, objects preserved on stderr.`,
      );
      for (const document of candidates) {
        attachCodeDefault(document);
      }
    } else {
      // M < N：耗尽点起回落。
      const pairs = Math.min(N, M);
      for (let index = 0; index < pairs; index += 1) {
        const document = candidates[index];
        const failure = objects[index];
        if (!document || !failure) continue;
        attachPairedError(document, failure, attributionDegraded);
      }
      for (const document of candidates.slice(M)) {
        attachCodeDefault(document);
      }
      attributionDegraded.push(
        `${N} failed documents but only ${M} failure objects (stream truncated); the rest fall back to their exit-code defaults.`,
      );
    }
  }
  // 130 文档一律按表映射 interrupted（不消费对象）。
  for (const document of cancelled) {
    document.status = 'cancelled';
    document.error = {
      reason: 'interrupted',
      message: 'This document was cancelled.',
      guidance: 'Re-run with resume: true to finish the remaining documents.',
    };
  }
  // exit 10 文档（页级部分失败）：状态 partial，无失败对象可挂。
  for (const document of documents) {
    if (document.exit_code === 10) {
      document.status = 'partial';
      enrichFromResultJson(document);
    } else if (document.exit_code === 0) {
      document.status = (document.status === 'partial' ? 'partial' : 'success') as NormalizedDocument['status'];
      enrichFromResultJson(document);
    }
  }
  const ok = exitCode === 0 || exitCode === 10;
  const partial = exitCode === 10 || documents.some((document) => document.status === 'partial');
  const firstFailed = documents.find(
    (document) => document.error !== null && document.exit_code !== 130 && document.exit_code !== 10,
  );
  const firstCancelled = documents.find((document) => document.exit_code === 130);
  const topError = firstFailed?.error ?? firstCancelled?.error ?? null;
  const result: ScanToolResult = {
    ok,
    exit_code: exitCode,
    partial,
    documents,
    scan_summary: existsScanSummary(aggregate.output_root)
      ? joinPath(aggregate.output_root, 'scan-summary.json')
      : undefined,
    error: topError
      ? { ...topError, ...(attributionDegraded.length > 0 ? { attribution_degraded: true } : {}) }
      : attributionDegraded.length > 0
        ? {
            reason: 'cli_error',
            message: 'Batch failure attribution was degraded.',
            guidance: 'See the per-document reasons and the stderr tail for the unattributed objects.',
            attribution_degraded: true,
          }
        : null,
    stderr_tail: stderr.tail.length > 0 ? stderr.tail : undefined,
  };
  result.text = buildBatchText(result, attributionDegraded, stderr);
  return result;
}

function baseDocument(document: Aggregate['documents'][number]): NormalizedDocument {
  return {
    input: document.input,
    output_dir: document.output_dir,
    status: document.status as NormalizedDocument['status'],
    exit_code: document.exit_code,
    error: null,
  };
}

/** 逐对 code 相容校验：失配 → 保守降级（宁缺勿错）。 */
function attachPairedError(
  document: NormalizedDocument,
  failure: CliFailureObject,
  degraded: string[],
): void {
  if (failure.code !== document.exit_code) {
    degraded.push(
      `Failure object for "${document.input}" carries code ${failure.code} while the aggregate says ${document.exit_code}: attribution degraded (no guess).`,
    );
    attachCodeDefault(document);
    return;
  }
  const error = mapScanFailure(failure);
  document.error = {
    reason: error.reason,
    message: error.message,
    guidance: error.guidance,
    ...(error.details_unavailable ? { details_unavailable: true } : {}),
  };
  document.status = 'failed';
}

function attachCodeDefault(document: NormalizedDocument): void {
  const error = reasonForCodeWithoutObject(document.exit_code, '');
  document.error = {
    reason: error.reason,
    message: error.message,
    guidance: error.guidance,
    details_unavailable: true,
  };
  document.status = 'failed';
}

/** 读取各 <output_dir>/result.json 补齐页数/警告（§7.4 归一化来源）。 */
function enrichFromResultJson(document: NormalizedDocument): void {
  if (!document.output_dir) return;
  const resultPath = joinPath(document.output_dir, 'result.json');
  if (!existsSync(resultPath)) return;
  try {
    const raw = readFileSync(resultPath, 'utf8');
    const parsed = JSON.parse(raw) as V3Result;
    document.pages = parsed.pages?.completed ?? document.pages;
    document.warnings_count = parsed.warnings?.length ?? 0;
    document.duration_ms = parsed.elapsed_ms;
    document.markdown_path = parsed.markdown_path ?? document.markdown_path;
    document.images_dir = parsed.images?.directory;
  } catch {
    // tolerate partial/corrupt files — the aggregate face stays authoritative
  }
}

function existsScanSummary(outputRoot: string): boolean {
  return outputRoot.length > 0 && existsSync(joinPath(outputRoot, 'scan-summary.json'));
}

function joinPath(left: string, right: string): string {
  return left.endsWith('/') || left.endsWith('\\') ? `${left}${right}` : `${left}/${right}`;
}

// --- 文本交付（§7.7.1）------------------------------------------------------------

function buildSingleDocText(document: NormalizedDocument, value: V3Result): string {
  const pages = value.pages ?? {};
  const lines = [
    `Scan complete: status=${document.status} pages=${pages.completed ?? '?'}/${pages.total ?? '?'} warnings=${document.warnings_count ?? 0} in ${Math.round((document.duration_ms ?? 0) / 100) / 10}s.`,
    `Markdown: ${document.markdown_path ?? '(none)'}`,
    `Result: ${joinPath(document.output_dir ?? '', 'result.json')}`,
    `Images: ${document.images_dir ?? '(none)'}`,
    'Body content is not inlined; read the files above when needed.',
  ];
  return lines.join('\n');
}

function buildBatchText(
  result: ScanToolResult,
  degraded: string[],
  stderr: { tail: string },
): string {
  const lines: string[] = [];
  const succeeded = result.documents.filter((document) => document.status === 'success').length;
  const partialCount = result.documents.filter((document) => document.status === 'partial').length;
  const failedCount = result.documents.filter((document) => document.status === 'failed').length;
  const cancelledCount = result.documents.filter((document) => document.status === 'cancelled').length;
  lines.push(
    `Batch scan: ${result.documents.length} document(s) — success=${succeeded} partial=${partialCount} failed=${failedCount} cancelled=${cancelledCount}; exit=${result.exit_code}.`,
  );
  for (const document of result.documents) {
    const detail = document.error ? ` — ${document.error.reason}${document.error.details_unavailable ? ' (details unavailable)' : ''}` : '';
    lines.push(`- ${document.input}: ${document.status}${detail}`);
  }
  if (result.scan_summary) lines.push(`Summary: ${result.scan_summary}`);
  if (degraded.length > 0) {
    lines.push('Attribution degraded:');
    for (const entry of degraded) lines.push(`  * ${entry}`);
    lines.push(`stderr tail (unattributed objects preserved):\n${stderr.tail.slice(-2000)}`);
  }
  lines.push('Bodies are not inlined; read each output_dir/result.json for details.');
  return lines.join('\n');
}

function buildTimeoutText(waitedMs: number): string {
  return (
    `Scan timed out after ${Math.round(waitedMs / 100) / 10}s and was terminated. ` +
    'No progress tail exists (the CLI writes progress at process end only). ' +
    'Re-run with pages selection or resume: true.'
  );
}

export type { DocumentError };
