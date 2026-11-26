// Shared tool-execution face: binary resolution, cwd (D-8) and the run env
// every tool needs (requirement §7.3.1/§6.5).
import { locateBinary } from './binary.js';
import { LIMITS } from './run.js';
import type { ToolEnv } from './tools/scan.js';
import type { BinaryLocation, NormalizedError, SubprocessSeam, WorkingDirectorySeam } from './types.js';

export interface ToolExec {
  agent?: unknown;
  signal?: AbortSignal;
  callId?: string;
}

export interface ResolvedRunFace {
  binaryPath: string;
  binarySource: BinaryLocation['source'];
  cwd: string;
  subprocess: SubprocessSeam;
}

export interface RunFaceError {
  error: NormalizedError;
}

/**
 * Locates the binary and resolves cwd (§7.3.1): cwd comes from
 * workingDirectory.ensure(agent, signal) with the bash-tool-shaped null
 * agent fallback to process.cwd(); a missing binary does NOT throw — it
 * returns the binary_not_found error for the structured result.
 */
export async function resolveRunFace(env: ToolEnv, exec: ToolExec): Promise<ResolvedRunFace | RunFaceError> {
  const subprocess = env.ctx.get('subprocess') as SubprocessSeam | undefined;
  if (!subprocess || typeof subprocess.spawn !== 'function') {
    return {
      error: {
        reason: 'binary_not_found',
        message: 'The host did not provide the subprocess service; ocrx cannot be started.',
        guidance: 'This deployment lacks the subprocess seam; the ocrx tools cannot run here.',
      },
    };
  }
  const { location } = locateBinary(env.config.command);
  if (!location) {
    return {
      error: {
        reason: 'binary_not_found',
        message: 'ocrx executable not found for this platform.',
        guidance: binaryNotFoundGuidanceText(),
      },
    };
  }
  let cwd = process.cwd();
  const workingDirectory = env.ctx.get('workingDirectory') as WorkingDirectorySeam | undefined;
  if (exec.agent !== undefined && workingDirectory && typeof workingDirectory.ensure === 'function') {
    try {
      cwd = await workingDirectory.ensure(exec.agent, exec.signal);
    } catch {
      cwd = process.cwd();
    }
  }
  return { binaryPath: location.path, binarySource: location.source, cwd, subprocess };
}

/** extraEnv = the cleaned host environment merged at the seam (D-8). */
export function runEnv(env: ToolEnv): Record<string, string> {
  return { ...env.config.extraEnv };
}

export const RUN_LIMITS = LIMITS;

function binaryNotFoundGuidanceText(): string {
  return (
    'The ocrx executable was not found for this platform. Fixes, in order of preference: ' +
    '(1) reinstall the plugin so the bundled platform package is restored; ' +
    '(2) download the GitHub Release archive and run install.ps1/install.sh; ' +
    '(3) point the plugin config at an existing binary: ocrx: { command: "<path>" }.'
  );
}
