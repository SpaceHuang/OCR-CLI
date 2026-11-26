// readiness.ts (requirement §7.5.3/§8.2): model_missing → 就绪度推导。
// 自举目标继承四组合矩阵——双显式沿用 / 仅 model 经 recommend --model /
// 仅 profile = CLI 默认模型 + 该 profile（不经无参数 recommend）/ 均缺省
// 经无参数 recommend。C2 计划可得时带 size_bytes/target_dir；不可得时按
// D-11 降级标注（体积待确认）。插件不得硬编码默认模型。
import type { InstallPlan } from './types.js';

export interface ScanArgsLike {
  model?: string;
  modelProfile?: string;
}

/** Fetches the C2 install plan (machine-readable recommend face). */
export type PlanFetcher = (model?: string) => Promise<InstallPlan | null>;

/** D-11 fallback: parse the plain-text recommend face of an old binary. */
export type TextRecommendFetcher = (model?: string) => Promise<{ model: string; profile: string } | null>;

export interface BootstrapTarget {
  model: string;
  profile: string;
  /** From the C2 plan when the plan's own profile matches the target. */
  size_bytes?: number;
  target_dir?: string;
  /** D-11 degrade: size unknown (old binary or profile/ladder mismatch). */
  size_unconfirmed?: boolean;
  /** How this target was resolved (matrix face, for tests + guidance). */
  source: 'explicit' | 'model-recommend' | 'profile-explicit' | 'implicit-recommend';
}

/**
 * 四组合解析矩阵（§7.5.3）。
 * `recommendModelExplicitly` is used for the "only model explicit" face so
 * the model NEVER crosses (无参数 recommend 可能跨模型——禁止代替)。
 */
export async function resolveBootstrapTarget(
  args: ScanArgsLike,
  fetchPlan: PlanFetcher,
  fallbackText?: TextRecommendFetcher,
): Promise<BootstrapTarget> {
  const modelExplicit = typeof args.model === 'string' && args.model.length > 0;
  const profileExplicit =
    typeof args.modelProfile === 'string' &&
    args.modelProfile.length > 0 &&
    args.modelProfile !== 'auto';
  // 双显式：直接沿用（体积按该 model 的 plan，仅当 plan profile 相符）。
  if (typeof args.model === 'string' && typeof args.modelProfile === 'string') {
    const plan = await fetchPlan(args.model);
    return finalizeTarget({ model: args.model, profile: args.modelProfile, source: 'explicit' }, plan);
  }
  // 仅 model 显式：recommend --model（保留该 model，不用无参数 recommend）。
  if (typeof args.model === 'string') {
    const plan = await fetchPlan(args.model);
    if (plan) {
      return finalizeTarget(
        { model: args.model, profile: plan.profile, source: 'model-recommend' },
        plan,
      );
    }
    const text = (await fallbackText?.(args.model)) ?? null;
    if (text && text.model === args.model) {
      const plan2 = await fetchPlan(args.model);
      return finalizeTarget({ model: args.model, profile: text.profile, source: 'model-recommend' }, plan2, true);
    }
    // 无来源：保留 model，profile 未知（不得猜）。
    return { model: args.model, profile: 'unknown', size_unconfirmed: true, source: 'model-recommend' };
  }
  // 仅 profile 显式：CLI 默认模型（来自 plan 的 default_model，不硬编码）
  // + 该 profile；不经无参数 recommend。
  if (profileExplicit && typeof args.modelProfile === 'string') {
    const plan = await fetchPlan(undefined);
    const defaultModel = plan?.default_model;
    if (defaultModel) {
      return finalizeTarget(
        { model: defaultModel, profile: args.modelProfile, source: 'profile-explicit' },
        plan,
      );
    }
    return { model: 'unknown', profile: args.modelProfile, size_unconfirmed: true, source: 'profile-explicit' };
  }
  // 均缺省：无参数 recommend 的实际返回（与 scan 同源解析）。
  const plan = await fetchPlan(undefined);
  if (plan) {
    return finalizeTarget({ model: plan.model, profile: plan.profile, source: 'implicit-recommend' }, plan);
  }
  const text = (await fallbackText?.()) ?? null;
  if (text) {
    return { model: text.model, profile: text.profile, size_unconfirmed: true, source: 'implicit-recommend' };
  }
  return { model: 'unknown', profile: 'unknown', size_unconfirmed: true, source: 'implicit-recommend' };
}

/**
 * 体积/目录只在「plan 的 ladder profile == 目标 profile」时可信；
 * target_dir 是 revision 目录（与 profile 无关），始终可给。
 */
function finalizeTarget(
  target: Omit<BootstrapTarget, 'size_bytes' | 'target_dir' | 'size_unconfirmed'>,
  plan: InstallPlan | null | undefined,
  unconfirmed = false,
): BootstrapTarget {
  if (!plan) {
    return { ...target, size_unconfirmed: true };
  }
  if (unconfirmed || plan.profile !== target.profile) {
    return { ...target, target_dir: plan.target_dir, size_unconfirmed: true };
  }
  return {
    ...target,
    size_bytes: plan.size_bytes,
    target_dir: plan.target_dir,
  };
}

/** 人类可读的 readiness 附注（§8.2.2 示例输出形态）。 */
export function readinessNote(target: BootstrapTarget): string {
  const size = target.size_bytes
    ? `${(target.size_bytes / 1_000_000_000).toFixed(2)} GB`
    : 'size unconfirmed (~1.0 GB per the official docs)';
  return (
    `Install target: model=${target.model} profile=${target.profile} (${size})` +
    (target.target_dir ? ` into ${target.target_dir}` : '') +
    `. Resolved via ${target.source}.`
  );
}
