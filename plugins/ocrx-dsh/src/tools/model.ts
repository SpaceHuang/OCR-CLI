// ocrx_model tool (requirement §7.5.2; plan D-4): action 四值映射；无条件
// 追加 --non-interactive；禁止 --progress/--model-profile；--report 仅
// recommend（且仅 json，D-4）——list/info/verify 一律文本透传；recommend
// 解析计划 JSON 并给出体积/revision/目录（C2），同时保留人类可读摘要；
// 旧二进制（退出码 2 unsupported flag）按 D-11 去旗标重试并降级。
import { buildModelArgv, type ModelAction } from '../argv/model.js';
import { LIMITS, runBinary } from '../run.js';
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import type { InstallPlan } from '../types.js';
import { resolveRunFace, runEnv } from '../exec-face.js';
import type { ToolEnv } from './scan-env.js';

export function createModelTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_model',
    description:
      'Query the local ocrx model catalog: list registered models, show model info, recommend ' +
      'a profile for this machine, or verify install integrity. recommend also reports the ' +
      'exact download size, revision and target directory (machine-readable plan face); on ' +
      'older ocrx binaries it degrades to the text face and marks the size unconfirmed.',
    parameters: {
      action: {
        type: 'string',
        description: 'Catalog query to run.',
        enum: ['list', 'info', 'recommend', 'verify'],
      },
      model: {
        type: 'string',
        description: 'Model selector (info/verify/recommend). Omit for the CLI default face.',
        enum: ['ovisocr2', 'ovisocr3'],
      },
      profile: {
        type: 'string',
        description: 'Profile to verify (verify only).',
        enum: ['compact', 'balanced', 'quality', 'reference'],
      },
    },
    required: ['action'],
    isConcurrencySafe: () => true,
    outputDescription: 'Model catalog query result (text passthrough or machine-readable plan).',
    renderText: (_args, value) => textResult(value),
    validate: (args) => {
      if (args.action === 'verify' && typeof args.profile !== 'string') {
        return ['verify requires a profile'];
      }
      if (args.action !== 'verify' && args.profile !== undefined) {
        return ['profile is only accepted with the verify action'];
      }
      return [];
    },
    execute: async (rawArgs, exec) => {
      const args = rawArgs as { action: ModelAction; model?: string; profile?: string };
      const face = await resolveRunFace(env, exec);
      if ('error' in face) {
        return { ok: false, error: face.error, text: face.error.message };
      }
      const argv = buildModelArgv({
        action: args.action,
        model: args.model as 'ovisocr2' | 'ovisocr3' | undefined,
        profile: args.profile as 'compact' | 'balanced' | 'quality' | 'reference' | undefined,
        // D-4: the machine-readable plan face rides ONLY recommend.
        reportJson: args.action === 'recommend',
      });
      const outcome = await runBinary(face.subprocess, {
        argv: [face.binaryPath, ...argv],
        cwd: face.cwd,
        env: runEnv(env),
        signal: exec.signal,
        stdoutMaxBytes: LIMITS.stdout,
        stderrMaxBytes: LIMITS.stderr,
      });
      if (outcome.spawnFailed) {
        return {
          ok: false,
          error: {
            reason: 'binary_not_found' as const,
            message: `ocrx could not be started (${outcome.spawnError?.message ?? 'spawn failed'})`,
            guidance: 'Reinstall the plugin or point the ocrx command config at a working binary.',
          },
          text: 'ocrx binary could not be started.',
        };
      }
      if (args.action === 'recommend') {
        return recommendResult(args, outcome);
      }
      // list / info / verify：文本透传（退出码非 0 → 结构化错误 + 原文）。
      if (outcome.code !== 0) {
        return {
          ok: false,
          exit_code: outcome.code,
          error: {
            reason: 'cli_error' as const,
            message: `ocrx model ${args.action} exited ${outcome.code}.`,
            guidance: 'Check the model/profile spelling with action "list", then retry.',
            details_unavailable: true,
            cause: outcome.stderr.slice(-2000) || outcome.stdout.slice(-2000) || null,
          },
          text: outcome.stderr.trim() || outcome.stdout.trim(),
        };
      }
      return {
        ok: true,
        exit_code: 0,
        action: args.action,
        output: outcome.stdout.trim(),
        text: outcome.stdout.trim(),
      };
    },
  });
}

interface RunOutcomeLike {
  code: number | null;
  stdout: string;
  stderr: string;
  spawnFailed: boolean;
}

function recommendResult(args: { model?: string }, outcome: RunOutcomeLike): unknown {
  if (outcome.code === 0) {
    try {
      const plan = JSON.parse(outcome.stdout.trim()) as InstallPlan;
      if (plan.schema_version === 1 && plan.action === 'recommend') {
        const size = plan.installed
          ? 'already installed'
          : `${(plan.size_bytes / 1_000_000_000).toFixed(2)} GB`;
        return {
          ok: true,
          action: 'recommend',
          plan,
          text:
            `Recommendation: model=${plan.model} profile=${plan.profile} — ${size}` +
            (plan.installed ? '' : ` → ${plan.target_dir}`) +
            `. Default model on this CLI: ${plan.default_model}.`,
        };
      }
    } catch {
      // fall through to the text face below
    }
    const text = outcome.stdout.trim();
    return {
      ok: true,
      action: 'recommend',
      plan: null,
      plan_note: 'plan JSON unparseable; raw output preserved',
      output: text,
      text: text || 'recommend produced no output.',
    };
  }
  if (outcome.code === 2 && /unsupported model flag/i.test(outcome.stderr)) {
    return {
      ok: false,
      exit_code: outcome.code,
      error: {
        reason: 'cli_error' as const,
        message: 'The configured ocrx binary predates the machine-readable recommend face (C2).',
        guidance:
          'Install size: ~1.0 GB for the default OvisOCR3 quality profile (see the official docs). ' +
          'Upgrade ocrx to get exact sizes; the install tool still works.',
      },
      text: 'ocrx recommend --report json unsupported by this binary (pre-C2).',
    };
  }
  return {
    ok: false,
    exit_code: outcome.code,
    error: {
      reason: 'cli_error' as const,
      message: `ocrx model recommend exited ${outcome.code}.`,
      guidance: 'Check memory constraints (the profile floor is 6 GiB), then retry.',
      details_unavailable: true,
      cause: outcome.stderr.slice(-2000) || outcome.stdout.slice(-2000) || null,
    },
    text: outcome.stderr.trim() || 'model recommend failed.',
  };
}
