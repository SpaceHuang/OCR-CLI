// /ocrx slash command (requirement §8.3; plan stage 7). Commands execute
// outside turns: NO confirmation channel exists here, so this command
// intentionally offers NO model-install action — installation always
// happens in a conversation turn via ocrx_model_install (consent gate).
import { locateBinary } from './binary.js';
import { LIMITS, runBinary } from './run.js';
import { resolveRunFace, runEnv } from './exec-face.js';
import type { ToolEnv } from './tools/scan-env.js';

export interface CommandDefinition {
  name: string;
  description: string;
  handler(invocation: {
    commandId?: string;
    agent?: unknown;
    rawInput?: string;
    signal?: AbortSignal;
    [key: string]: unknown;
  }): Promise<{ kind: 'success' | 'error'; text: string }> | { kind: 'success' | 'error'; text: string };
}

const PLUGIN_VERSION = '0.1.0';
const TRUST_MODEL =
  'Trust model: ocrx tools run with your full user file permissions (no harness sandbox); ' +
  'they can write any output directory and read any input path. Trusting these tools equals ' +
  'trusting a locally installed ocrx. Downloads happen ONLY via ocrx_model_install after ' +
  'your explicit consent; ocrx_scan always runs offline.';

export function createOcrxCommand(env: ToolEnv): CommandDefinition {
  return {
    name: 'ocrx',
    description:
      'OvisOCR (ocrx) status overview; "/ocrx doctor" runs the full self-check. ' +
      'Model installation always happens in a conversation turn via the ocrx_model_install tool.',
    handler: async (invocation) => {
      const input = typeof invocation.rawInput === 'string' ? invocation.rawInput.trim() : '';
      if (input === 'doctor') {
        return doctorCommand(env, invocation.signal);
      }
      const { location, bundledVersion } = locateBinary(env.config.command);
      const lines = [
        'OvisOCR (ocrx) — offline PDF → Markdown for this harness.',
        `Plugin: @ocrx/dsh v${PLUGIN_VERSION} (tools: ${toolSwitchSummary(env)}).`,
        `Binary: ${location ? `${location.path} (source: ${location.source})` : 'NOT FOUND — reinstall the plugin or set the ocrx command config'}`,
        bundledVersion ? `Bundled platform package: v${bundledVersion}.` : 'Bundled platform package: not resolvable (PATH/config binary in use).',
        'ocrx CLI version: run "/ocrx doctor" (requires the C3 version face).',
        'Model install status: run "/ocrx doctor" — NOTE installed_profiles covers exactly one resolved model and misses custom cache overrides.',
        `Downloads: only ocrx_model_install, always after your explicit consent${env.config.offline ? ' — currently BLOCKED because the plugin config sets offline=true' : ''}.`,
        `Scans: always offline, max ${Math.round(env.config.maxTimeoutMs / 60000)} minutes per run.`,
        TRUST_MODEL,
        'Next steps: ask the agent to convert a PDF (e.g. "convert C:/docs/report.pdf to markdown"); on first use it will ask consent to download the OCR model.',
      ];
      return { kind: 'success', text: lines.join('\n') };
    },
  };
}

function toolSwitchSummary(env: ToolEnv): string {
  const switches: string[] = [];
  if (env.config.enableScan) switches.push('scan');
  if (env.config.enableDoctor) switches.push('doctor');
  if (env.config.enableModel) switches.push('model');
  if (env.config.enableModelInstall) switches.push('model_install');
  return switches.join(', ') || 'none enabled';
}

async function doctorCommand(
  env: ToolEnv,
  signal: AbortSignal | undefined,
): Promise<{ kind: 'success' | 'error'; text: string }> {
  const face = await resolveRunFace(env, { signal });
  if ('error' in face) {
    return {
      kind: 'success',
      text: `ocrx binary problem: ${face.error.message}\n${face.error.guidance}`,
    };
  }
  const outcome = await runBinary(face.subprocess, {
    argv: [face.binaryPath, 'doctor', '--report', 'json'],
    cwd: face.cwd,
    env: runEnv(env),
    signal,
    stdoutMaxBytes: LIMITS.stdout,
    stderrMaxBytes: LIMITS.stderr,
  });
  if (outcome.spawnFailed) {
    return {
      kind: 'success',
      text: `ocrx could not be started (${outcome.spawnError?.message ?? 'spawn failed'}).\nReinstall the plugin or set the ocrx command config.`,
    };
  }
  let doctor: Record<string, unknown> | null = null;
  if (outcome.code === 0) {
    try {
      doctor = JSON.parse(outcome.stdout.trim()) as Record<string, unknown>;
    } catch {
      doctor = null;
    }
  }
  if (!doctor) {
    return {
      kind: 'success',
      text: `ocrx doctor exited ${outcome.code} without parseable JSON. Retry once; if it persists this is a CLI/plugin contract problem.\n${outcome.stderr.slice(-800)}`,
    };
  }
  const installed = Array.isArray(doctor.installed_profiles) ? doctor.installed_profiles : [];
  const actions: string[] = [];
  if (installed.length === 0) {
    actions.push(
      'No OCR model installed yet — ask the agent to convert a PDF; it will offer a consented model install (ocrx_model_install).',
    );
  }
  actions.push('Large PDFs: sample with pages "1-3" first, then run the full document.');
  const version = typeof doctor.version === 'string' ? doctor.version : 'unknown (pre-C3 binary)';
  return {
    kind: 'success',
    text: [
      `ocrx doctor: model=${String(doctor.model ?? '?')} backend=${String(doctor.backend ?? '?')} memory=${String(doctor.memory_gb ?? '?')}GiB cli_version=${version}.`,
      `Cache: ${String(doctor.cache_root ?? '?')} (installed profiles: ${JSON.stringify(installed)} — single-model view, custom cache overrides are invisible).`,
      'Repair/next actions:',
      ...actions.map((action) => `- ${action}`),
    ].join('\n'),
  };
}
