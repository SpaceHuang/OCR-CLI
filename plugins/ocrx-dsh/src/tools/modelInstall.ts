// ocrx_model_install tool (requirement §7.5.3/§8.2.4/§9.2/§11.3; plan stage
// 6). The ONLY download entry point with its own consent gate implemented
// via ctx.get('approval') inside execute. No pipeline timeoutMs is
// registered (downloads cannot resume; Config.installTimeoutMs is optional);
// isConcurrencySafe is constantly false (long task).
import { buildInstallArgv } from '../argv/install.js';
import { requestConsent } from '../consent.js';
import { fetchRecommend } from '../recommend.js';
import { formatBytes, residualNote, scanResidual } from '../residual.js';
import { LIMITS, runBinary, runBinaryWithTimeout } from '../run.js';
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import type { InstallPlan, InstallSummary } from '../types.js';
import { resolveRunFace, runEnv } from '../exec-face.js';
import type { ToolEnv } from './scan-env.js';

export function createModelInstallTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_model_install',
    description:
      'Download and install an ocrx OCR model (the only tool that ever downloads anything). ' +
      'It asks for explicit user consent first with the real size and target directory, and ' +
      'refuses to download without approval. Interrupted downloads cannot resume and leave ' +
      'staging files until the next install cleans them. Omit model/modelProfile to inherit ' +
      'the target resolved from a failed scan (the bootstrap flow).',
    parameters: {
      model: {
        type: 'string',
        description: 'Model to install. Omit to use the CLI default resolution.',
        enum: ['ovisocr2', 'ovisocr3'],
      },
      modelProfile: {
        type: 'string',
        description:
          'Profile to install. Omit to inherit the readiness target resolved from the failed ' +
          'scan (the bootstrap flow).',
        enum: ['compact', 'balanced', 'quality', 'reference'],
      },
    },
    required: [],
    isConcurrencySafe: () => false,
    outputDescription:
      'Install outcome with progress statistics (stats.download/verify from the CLI summary), ' +
      'residual report and readiness guidance.',
    renderText: (_args, value) => textResult(value),
    execute: async (rawArgs, exec) => {
      const args = rawArgs as { model?: string; modelProfile?: string };
      const face = await resolveRunFace(env, exec);
      if ('error' in face) {
        return installError(face.error.message, face.error);
      }
      const runRecommend = async (model?: string) =>
        fetchRecommend(face.subprocess, face.binaryPath, face.cwd, runEnv(env), model, exec.signal);
      // 1. 计划面（单次预取：目标解析与同意文案共用同一份计划）。
      const firstOutcome = await runRecommend(args.model);
      // 2. 目标解析（四组合矩阵复用 readiness 的解析器）。
      const argvResult = await buildInstallArgv(args, {
        offline: env.config.offline,
        fetchPlan: async () => firstOutcome.plan,
        fallbackText: async () => firstOutcome.text,
      });
      if ('error' in argvResult) {
        return installError(argvResult.error.message, argvResult.error);
      }
      const { argv, target } = argvResult;
      // 3. 同意文案：真实体积 + 目标目录 + 不可续传提示（C2 计划面）。
      const plan = firstOutcome.plan;
      const consentBody = buildConsentBody(target, plan, env.config.offline);
      // 3. 同意门（execute 内经 ctx.get('approval')）。
      const consent = await requestConsent(env.ctx, {
        title: `Install ocrx model ${target.model} / ${target.profile}`,
        body: consentBody,
        toolName: 'ocrx_model_install',
        agent: exec.agent,
        callId: exec.callId,
        signal: exec.signal,
      });
      if (consent.outcome === 'cancelled') {
        return {
          ok: false,
          error: {
            reason: 'consent_cancelled',
            message: 'The model download was cancelled before it started.',
            guidance:
              'Nothing was downloaded. You can ask again any time; the consent prompt will be shown again.',
          },
          text: 'Model install cancelled at the consent prompt.',
        };
      }
      if (consent.outcome !== 'approved') {
        return {
          ok: false,
          error: {
            reason: 'consent_denied',
            message: `The model was NOT downloaded: consent was not granted (${consent.detail}).`,
            guidance: OFFLINE_PLACEMENT_GUIDANCE,
          },
          text: `Model install blocked: no consent (${consent.detail}).`,
        };
      }
      // 4. 执行安装（缺省不设超时；Config.installTimeoutMs 可选上限）。
      const spec = {
        argv: [face.binaryPath, ...argv],
        cwd: face.cwd,
        env: runEnv(env),
        signal: exec.signal,
        stdoutMaxBytes: LIMITS.stdout,
        stderrMaxBytes: LIMITS.stderr,
      };
      const outcome =
        env.config.installTimeoutMs === undefined
          ? { ...(await runBinary(face.subprocess, spec)), timedOut: false as const }
          : await runBinaryWithTimeout(face.subprocess, spec, env.config.installTimeoutMs);
      return finishInstall(env, outcome, target, plan);
    },
  });
}

const OFFLINE_PLACEMENT_GUIDANCE =
  'Offline placement: download the two GGUF assets manually (see the ocrx docs for the exact ' +
  'URLs and the pinned SHA256 list) and place them under the cache root: ' +
  '<cache>/ocrx/models/<model>/<revision>/<profile>/model.gguf and ' +
  '<cache>/ocrx/models/<model>/<revision>/mmproj.gguf. The default cache root is ' +
  'Windows %LOCALAPPDATA%, macOS ~/Library/Caches. After placing the files, run "ocrx model ' +
  'verify <profile>" to confirm integrity. No retry happens automatically and nothing is ' +
  'downloaded without consent.';

function buildConsentBody(
  target: { model: string; profile: string; size_bytes?: number; target_dir?: string },
  plan: InstallPlan | null,
  offlineConfig: boolean,
): string {
  const size =
    target.size_bytes !== undefined
      ? formatBytes(target.size_bytes)
      : plan && plan.profile === target.profile
        ? formatBytes(plan.size_bytes)
        : 'about 1.0 GB (size unconfirmed for this profile; see the official docs)';
  const dir = target.target_dir ?? plan?.target_dir ?? '(the CLI default cache location)';
  return (
    `Download and install the OCR model ${target.model} (${target.profile}), about ${size}, ` +
    `into ${dir}. The download cannot resume: if it is interrupted it restarts from zero, ` +
    'and staging files stay on disk until the next install cleans them. ' +
    (offlineConfig
      ? 'NOTE: the plugin config sets offline=true, so the CLI will refuse the download; the prompt is shown so the intent is explicit. '
      : '') +
    'Do you allow this download?'
  );
}

interface InstallOutcomeLike {
  stdout: string;
  stderr: string;
  code: number | null;
  killed: boolean;
  spawnFailed: boolean;
  spawnError?: Error;
  timedOut?: boolean;
  waitedMs?: number;
  cancelledByCaller?: boolean;
}

function finishInstall(
  env: ToolEnv,
  outcome: InstallOutcomeLike,
  target: { model: string; profile: string; target_dir?: string },
  plan: InstallPlan | null,
): unknown {
  // 取消/超时（§9.2）：如实报告残留 + 提示。
  if (outcome.timedOut === true || outcome.cancelledByCaller === true || outcome.killed) {
    const report = scanResidual([target.target_dir, plan?.target_dir]);
    return {
      ok: false,
      error: {
        reason: outcome.timedOut ? ('timeout' as const) : ('interrupted' as const),
        message: outcome.timedOut
          ? `The install exceeded ${String(env.config.installTimeoutMs)} ms and was terminated.`
          : 'The install was cancelled.',
        guidance:
          'Downloads cannot resume; the next install restarts from zero. ' +
          (residualNote(report) || 'No staging files were left behind.'),
      },
      residual: report,
      text:
        (outcome.timedOut ? 'Install timed out.' : 'Install cancelled.') +
        (report.locations.length > 0 ? ` ${residualNote(report)}` : ''),
    };
  }
  if (outcome.spawnFailed) {
    return installError(`ocrx could not be started (${outcome.spawnError?.message ?? 'spawn failed'})`, {
      reason: 'binary_not_found',
      message: 'ocrx executable problem during install.',
      guidance: 'Reinstall the plugin or point the ocrx command config at a working binary.',
    });
  }
  // 成功：stdout 末行为安装摘要 JSON（含 §9.2 进度统计）。
  const summary = extractSummary(outcome.stdout);
  if (outcome.code === 0 && summary) {
    const note =
      summary.download.downloaded === false
        ? 'The model was already in place: no download happened this run, but verification ran.'
        : 'The model is installed and verified; a retry of the failed scan (same arguments plus resume: true) can proceed.';
    return {
      ok: true,
      installed: { model: summary.model ?? target.model, profile: summary.profile ?? target.profile },
      revision: summary.revision,
      stats: {
        download: summary.download,
        verify: summary.verify,
        stage: summary.stage,
        elapsed_ms: summary.elapsed_ms,
      },
      note,
      text: buildInstallText(summary, note),
    };
  }
  // 失败：schema v1 失败对象在 stdout（C2 后）；纯文本为 C2 前过渡。
  const mapped = mapInstallFailure(outcome);
  const report = scanResidual([target.target_dir, plan?.target_dir]);
  const error = {
    ...mapped,
    ...(report.locations.length > 0
      ? { guidance: `${mapped.guidance} ${residualNote(report)}` }
      : {}),
  };
  return {
    ok: false,
    exit_code: outcome.code ?? undefined,
    error,
    residual: report.locations.length > 0 ? report : undefined,
    text: `Install failed: ${error.message}`,
  };
}

function extractSummary(stdout: string): InstallSummary | null {
  const lines = stdout.trim().split(/\r?\n/);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index]?.trim();
    if (!line) continue;
    try {
      const parsed = JSON.parse(line) as Partial<InstallSummary>;
      if (parsed && typeof parsed === 'object' && parsed.download && parsed.verify && parsed.stage) {
        return parsed as InstallSummary;
      }
    } catch {
      // not the summary line
    }
  }
  return null;
}

interface FailureObjectLike {
  schema_version: number;
  code: number;
  kind: string;
  message: string;
  cause: string | null;
}

/** install 路径错误映射（§7.6 install 例外 + D-11 过渡规则）。 */
function mapInstallFailure(outcome: InstallOutcomeLike): {
  reason: 'install_blocked' | 'install_failed';
  message: string;
  guidance: string;
  cause?: string | null;
  details_unavailable?: boolean;
} {
  const object = parseFailureObject(outcome.stdout);
  const stderrSample = outcome.stderr.slice(-1000).trim();
  if (object && object.kind === 'offline') {
    return {
      reason: 'install_blocked',
      message: `The CLI refused the download: ${object.message}`,
      guidance:
        'This is the offline gate: either the plugin config sets offline=true (turn it off to ' +
        'allow consented downloads) or the CLI ran with --offline. ' +
        (stderrSample ? `CLI stderr: ${stderrSample}` : ''),
      cause: object.cause,
    };
  }
  if (object && object.kind === 'disk') {
    return {
      reason: 'install_blocked',
      message: `Not enough disk space: ${object.message}`,
      guidance:
        'Free up space (stale staging files under the cache count) and retry. ' +
        (stderrSample ? `CLI stderr: ${stderrSample}` : ''),
      cause: object.cause,
    };
  }
  if (object && (object.kind === 'transport' || object.kind === 'hash' || object.kind === 'lock')) {
    const detail =
      object.kind === 'lock'
        ? 'Another ocrx install holds the revision lock; retry after it finishes.'
        : object.kind === 'hash'
          ? 'The downloaded bytes failed verification; the source or transfer is unreliable.'
          : 'The transfer failed (network).';
    return {
      reason: 'install_failed',
      message: `${object.message} (${object.kind})`,
      guidance: `${detail} No automatic retry happens and no new consent is requested — check the network/disk, then ask again.`,
      cause: object.cause,
    };
  }
  if (object) {
    // 未列 kind → install_failed（§7.6 install 例外），原文透传。
    return {
      reason: 'install_failed',
      message: object.message || `install failed (code ${object.code}, kind ${object.kind})`,
      guidance:
        'Unrecognized install failure kind — raw fields preserved. No automatic retry, no re-consent.',
      cause: object.cause,
    };
  }
  // C2 前过渡规则：仅 stderr 文本（code 5 + offline/磁盘文本可区分）。
  if (outcome.code === 5 && /offline mode/i.test(stderrSample)) {
    return {
      reason: 'install_blocked',
      message: 'The CLI refused the download: offline mode.',
      guidance:
        'Either the plugin config sets offline=true or the CLI ran with --offline. ' +
        `CLI stderr: ${stderrSample}`,
      details_unavailable: true,
    };
  }
  if (outcome.code === 5 && /insufficient disk/i.test(stderrSample)) {
    return {
      reason: 'install_blocked',
      message: 'The CLI refused the download: insufficient disk.',
      guidance: `Free up space (stale staging files count) and retry. CLI stderr: ${stderrSample}`,
      details_unavailable: true,
    };
  }
  return {
    reason: 'install_failed',
    message: `ocrx model install exited ${outcome.code} without a structured failure object.`,
    guidance:
      `No automatic retry, no re-consent. Check the network and disk, then decide. ` +
      `CLI stderr: ${stderrSample || '(empty)'}`,
    details_unavailable: true,
  };
}

function parseFailureObject(stdout: string): FailureObjectLike | null {
  for (const line of stdout.trim().split(/\r?\n/)) {
    try {
      const parsed = JSON.parse(line.trim()) as Partial<FailureObjectLike>;
      if (parsed.schema_version === 1 && typeof parsed.code === 'number' && typeof parsed.kind === 'string') {
        return {
          schema_version: 1,
          code: parsed.code,
          kind: parsed.kind,
          message: parsed.message ?? '',
          cause: parsed.cause ?? null,
        };
      }
    } catch {
      // not JSON — the success line or human output
    }
  }
  return null;
}

function installError(
  text: string,
  error: { reason: string; message: string; guidance: string },
): unknown {
  return { ok: false, error, text };
}

function buildInstallText(summary: InstallSummary, note: string): string {
  const download = summary.download;
  return [
    `Model ${summary.model}/${summary.profile} installed (revision ${summary.revision}).`,
    download.downloaded
      ? `Downloaded ${formatBytes(download.total_bytes)} at ${formatBytes(Math.round(download.avg_rate_bps))}/s in ${(summary.elapsed_ms / 1000).toFixed(1)}s.`
      : 'No download this run (already installed); verification passed.',
    `Assets verified: ${summary.verify.per_asset.map((asset) => `${asset.role}(sha256=${asset.sha256_ok},size=${asset.size_ok})`).join(', ')}.`,
    note,
    'You can now retry the scan with the same arguments plus resume: true.',
  ].join('\n');
}
