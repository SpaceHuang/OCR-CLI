// ocrx_doctor tool (requirement §7.5.1): passes through `doctor --report
// json` (no --non-interactive — doctor has no such flag) and attaches the
// plugin-view fields (binary path/source, bundled version, plugin version,
// real ocrx version via C3, missing → D-11 degrade note). STATUS DISPLAY
// ONLY: installed_profiles covers exactly one resolved model and does not
// see custom cache overrides — never pre-gate ocrx_scan with it.
import { locateBinary } from '../binary.js';
import { LIMITS, runBinary } from '../run.js';
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import { resolveRunFace, runEnv } from '../exec-face.js';
import type { ToolEnv } from './scan-env.js';

const PLUGIN_VERSION = '0.1.0';

export function createDoctorTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_doctor',
    description:
      'Report the local ocrx installation status: hardware snapshot, selected model, cache ' +
      'location, installed profiles, and plugin/binary versions. STATUS DISPLAY ONLY: the ' +
      'installed_profiles field covers exactly one resolved model and does not see custom ' +
      'cache overrides — never use it to pre-gate ocrx_scan.',
    parameters: {},
    required: [],
    isConcurrencySafe: () => true,
    outputDescription: 'Doctor JSON report plus plugin-view fields.',
    renderText: (_args, value) => textResult(value),
    execute: async () => {
      const face = await resolveRunFace(env, {});
      if ('error' in face) {
        return {
          ok: false,
          error: face.error,
          text: `ocrx binary problem: ${face.error.message}`,
        };
      }
      const outcome = await runBinary(face.subprocess, {
        argv: [face.binaryPath, 'doctor', '--report', 'json'],
        cwd: face.cwd,
        env: runEnv(env),
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
          ok: false,
          exit_code: outcome.code,
          error: {
            reason: 'cli_error' as const,
            message: `ocrx doctor exited ${outcome.code} without parseable JSON.`,
            guidance: 'Retry once; if it persists, this is a CLI/plugin contract problem.',
            details_unavailable: true,
            cause: outcome.stderr.slice(-2000) || outcome.stdout.slice(-2000) || null,
          },
          text: 'ocrx doctor produced no parseable JSON.',
        };
      }
      const location = locateBinary(env.config.command).location;
      const cliVersion = typeof doctor.version === 'string' ? doctor.version : null;
      return {
        ok: true,
        doctor,
        plugin: {
          name: '@ocrx/dsh',
          version: PLUGIN_VERSION,
          binary_path: location?.path ?? null,
          binary_source: location?.source ?? null,
          bundled_version: locateBinary(env.config.command).bundledVersion,
          engines_dsh: '>=0.2.2-rc.5 <0.2.3-0',
        },
        cli_version: cliVersion,
        cli_version_note:
          cliVersion === null
            ? 'ocrx version unknown (pre-C3 binary): the doctor JSON carries no version field.'
            : null,
        limitations:
          'installed_profiles covers exactly one resolved model and does not reflect custom cache overrides; use it for display, never to pre-gate scans.',
        text: buildDoctorText(doctor, location?.path ?? null, location?.source ?? null, cliVersion),
      };
    },
  });
}

function buildDoctorText(
  doctor: Record<string, unknown>,
  binaryPath: string | null,
  source: string | null,
  cliVersion: string | null,
): string {
  const lines = [
    `ocrx status: model=${String(doctor.model ?? '?')} backend=${String(doctor.backend ?? '?')} memory=${String(doctor.memory_gb ?? '?')}GiB.`,
    `Profiles installed: ${JSON.stringify(doctor.installed_profiles ?? [])} (single-model view; custom cache dirs are invisible to doctor).`,
    `Cache: ${String(doctor.cache_root ?? '?')}`,
    `Binary: ${binaryPath ?? '?'} (${source ?? '?'}); cli version ${cliVersion ?? 'unknown (pre-C3 binary)'}.`,
  ];
  const installed = doctor.installed_profiles;
  if (Array.isArray(installed) && installed.length === 0) {
    lines.push('No model installed yet: run a scan; the bootstrap flow will offer a consented install.');
  }
  return lines.join('\n');
}
