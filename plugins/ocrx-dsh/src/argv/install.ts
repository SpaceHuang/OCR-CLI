// argv/install.ts (requirement §7.5.3): `model install [--model m] [--offline]
// --non-interactive --report json <profile>`。profile 缺省填充按四组合矩阵
// （复用 readiness 的解析器）；无来源可得 → 结构化错误，不得发出缺 profile
// 的 argv。--offline 与 Config.offline 的关系：argv 只按 Config.offline 携带。
import { resolveBootstrapTarget, type BootstrapTarget, type PlanFetcher, type TextRecommendFetcher } from '../readiness.js';

export interface InstallArgs {
  model?: string;
  modelProfile?: string;
}

export interface InstallArgvResult {
  argv: string[];
  target: BootstrapTarget;
}

export interface InstallArgvError {
  error: {
    reason: 'usage';
    message: string;
    guidance: string;
  };
}

/**
 * Resolves the (model, profile) target through the §7.5.3 matrix, then
 * builds the argv. The profile is ALWAYS present in the returned argv —
 * the CLI rejects a profile-less install and we never emit one.
 * `reportJson: false` builds the D-11 retry face (no machine-readable flag).
 */
export async function buildInstallArgv(
  args: InstallArgs,
  options: {
    offline: boolean;
    reportJson: boolean;
    fetchPlan: PlanFetcher;
    fallbackText?: TextRecommendFetcher;
  },
): Promise<InstallArgvResult | InstallArgvError> {
  const target = await resolveBootstrapTarget(args, options.fetchPlan, options.fallbackText);
  if (target.profile === 'unknown' || target.model === 'unknown') {
    return {
      error: {
        reason: 'usage',
        message: 'Could not resolve an install target (model/profile).',
        guidance:
          'Provide model and/or modelProfile explicitly, or run a scan first: the bootstrap ' +
          'flow resolves the target from the failed scan (ocrx_model action "recommend" can ' +
          'also tell you the machine-appropriate pair).',
      },
    };
  }
  const argv: string[] = ['model', 'install'];
  if (target.model !== 'unknown') argv.push('--model', target.model);
  if (options.offline) argv.push('--offline');
  argv.push('--non-interactive');
  if (options.reportJson) argv.push('--report', 'json');
  argv.push(target.profile);
  return { argv, target };
}
