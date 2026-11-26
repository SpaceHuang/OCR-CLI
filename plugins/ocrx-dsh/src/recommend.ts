// recommend 计划面 (C2/D-4 + D-11): `model recommend --report json` 解析；
// 旧二进制（退出码 2 unsupported model flag）→ 同一 execute 内去旗标重试
// 一次（首次调用在 argv 解析期即失败、无副作用），降级为文本 face。
import { buildRecommendArgv } from './argv/model.js';
import { LIMITS, runBinary } from './run.js';
import type { InstallPlan, SubprocessSeam } from './types.js';

export interface RecommendOutcome {
  plan: InstallPlan | null;
  /** D-11: plain-text face (model/profile) when the binary predates C2. */
  text: { model: string; profile: string } | null;
  /** True when the binary rejected --report (old binary face). */
  oldBinary: boolean;
  /** True when the binary is reachable but produced nothing usable. */
  unusable?: string;
}

/**
 * Fetches the install plan. Returns the plan on success; on an old binary
 * retries ONCE without the flag and returns the text face (D-11: the first
 * call fails at argv parsing, so the retry has no side effects).
 */
export async function fetchRecommend(
  subprocess: SubprocessSeam,
  binaryPath: string,
  cwd: string,
  extraEnv: Record<string, string>,
  model?: string,
  signal?: AbortSignal,
): Promise<RecommendOutcome> {
  const withFlag = buildRecommendArgv({ model, reportJson: true });
  const first = await runBinary(subprocess, {
    argv: [binaryPath, ...withFlag],
    cwd,
    env: extraEnv,
    signal,
    stdoutMaxBytes: LIMITS.stdout,
    stderrMaxBytes: LIMITS.stderr,
  });
  if (first.spawnFailed) {
    return { plan: null, text: null, oldBinary: false, unusable: first.spawnError?.message };
  }
  if (first.code === 2 && /unsupported model flag/i.test(first.stderr)) {
    // D-11 retry: same execute context, flag dropped.
    const retry = await runBinary(subprocess, {
      argv: [binaryPath, ...buildRecommendArgv({ model, reportJson: false })],
      cwd,
      env: extraEnv,
      signal,
      stdoutMaxBytes: LIMITS.stdout,
      stderrMaxBytes: LIMITS.stderr,
    });
    if (retry.spawnFailed) {
      return { plan: null, text: null, oldBinary: true, unusable: retry.spawnError?.message };
    }
    return { plan: null, text: parseTextRecommend(retry.stdout), oldBinary: true };
  }
  if (first.code === 0) {
    try {
      const plan = JSON.parse(first.stdout.trim()) as InstallPlan;
      if (plan.schema_version === 1 && plan.action === 'recommend') {
        return { plan, text: null, oldBinary: false };
      }
    } catch {
      return { plan: null, text: parseTextRecommend(first.stdout), oldBinary: false, unusable: 'unparseable plan' };
    }
  }
  return {
    plan: null,
    text: parseTextRecommend(first.stdout),
    oldBinary: false,
    unusable: first.code === 0 ? undefined : `recommend exited ${first.code}`,
  };
}

/** Parses the historical text face: `model=X profile=Y memory_gb=… device=…`. */
export function parseTextRecommend(stdout: string): { model: string; profile: string } | null {
  const match = /model=(\S+)\s+profile=(\S+)/.exec(stdout.trim());
  const model = match?.[1];
  const profile = match?.[2];
  if (!model || !profile) return null;
  return { model, profile };
}
