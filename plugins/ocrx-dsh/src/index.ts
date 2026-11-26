// Plugin entry (requirement §6.1/§6.3): name/inject/Config/apply.
// `approval` is intentionally NOT in inject (§6.3): a missing approval service
// must not prevent the whole plugin from loading — the install tool reaches it
// optionally through ctx.get('approval') and degrades to consent_denied.
import { resolveConfig, type OcrxConfig } from './config.js';
import { createOcrxCommand } from './command.js';
import { createSkillProvider } from './skill.js';
import { createDoctorTool } from './tools/doctor.js';
import { createModelTool } from './tools/model.js';
import { createModelInstallTool } from './tools/modelInstall.js';
import { createScanTool } from './tools/scan.js';
import type { ToolEnv } from './tools/scan-env.js';
import type { PluginContext } from './types.js';

export const name = 'ocrx-dsh';

export const inject = ['tools', 'commands', 'skills', 'subprocess', 'workingDirectory'];

/**
 * Optional standard-schema config declaration. Built only when the host's
 * schemastery peer is resolvable; cordis skips validation when it is absent
 * and resolveConfig() inside apply() stays the load-time validator.
 */
export const Config = buildConfigSchema();

function buildConfigSchema(): unknown {
  interface Schemastery {
    object(fields: Record<string, unknown>): Schemastery;
    boolean(): Schemastery;
    string(): Schemastery;
    natural(): Schemastery;
    default(value: unknown): Schemastery;
  }
  try {
    const z = require('@deepseek-ai/schemastery') as {
      object(fields: Record<string, unknown>): Schemastery;
      boolean(): Schemastery;
      string(): Schemastery;
      natural(): Schemastery;
    };
    return z.object({
      command: z.string(),
      extraEnv: z.object({}),
      offline: z.boolean().default(false),
      defaultTimeoutMs: z.natural().default(1_800_000),
      maxTimeoutMs: z.natural().default(14_400_000),
      installTimeoutMs: z.natural(),
      enableScan: z.boolean().default(true),
      enableDoctor: z.boolean().default(true),
      enableModel: z.boolean().default(true),
      enableModelInstall: z.boolean().default(true),
    });
  } catch {
    // Schemastery unavailable (plain-node unit tests): cordis is not present
    // either in that environment, so skipping the schema is safe.
    return undefined;
  }
}

export function apply(ctx: PluginContext, rawConfig: Record<string, unknown>): void {
  const config = resolveConfig(rawConfig);
  const env: ToolEnv = { config, ctx };
  if (config.enableScan) {
    ctx.tools.register(createScanTool(env), {});
  }
  if (config.enableDoctor) {
    ctx.tools.register(createDoctorTool(env), {});
  }
  if (config.enableModel) {
    ctx.tools.register(createModelTool(env), {});
  }
  if (config.enableModelInstall) {
    ctx.tools.register(createModelInstallTool(env), {});
  }
  ctx.commands.register(createOcrxCommand(env));
  ctx.skills.registerProvider(createSkillProvider(env));
}

export type { OcrxConfig };
