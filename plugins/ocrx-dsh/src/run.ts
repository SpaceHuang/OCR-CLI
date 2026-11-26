// spawn 封装 (requirement §7.3; plan D-8): runs the located ocrx binary
// through the host subprocess seam. All argv fully explicit (no shell), cwd
// mandatory (D-8 fallback to process.cwd()), env = cleaned host environment
// + extraEnv, stdout/stderr bounded collect, and an execute-side timeout
// that actively terminates at the deadline.
import type { SubprocessSeam } from './types.js';

export interface RunSpec {
  argv: string[];
  cwd: string;
  env?: Record<string, string>;
  signal?: AbortSignal;
  /** stdout collect limit (tail retention on overflow). */
  stdoutMaxBytes: number;
  /** stderr collect limit. */
  stderrMaxBytes: number;
}

export interface RunOutcome {
  stdout: string;
  stderr: string;
  code: number | null;
  killed: boolean;
  /** True when the seam refused the spawn outright (service missing). */
  spawnFailed: boolean;
  spawnError?: Error;
  /** Execute-side timeout fired (process terminated at the deadline). */
  timedOut?: boolean;
  /** How long the timed-out run waited before termination. */
  waitedMs?: number;
}

const DEFAULT_GRACE_MS = 5_000;

const STDOUT_LIMIT = 8 * 1_000_000;
const STDERR_LIMIT = 4 * 1_000_000;

/** Runs one fully-specified command through the seam. */
export async function runBinary(
  subprocess: SubprocessSeam,
  spec: RunSpec,
): Promise<RunOutcome> {
  const abort = spec.signal;
  try {
    const handle = subprocess.spawn({
      argv: spec.argv,
      cwd: spec.cwd,
      env: spec.env,
      signal: abort,
      stdio: {
        stdin: 'ignore',
        stdout: { maxBytes: spec.stdoutMaxBytes },
        stderr: { maxBytes: spec.stderrMaxBytes },
      },
      graceMs: DEFAULT_GRACE_MS,
    });
    const done = await handle.done;
    const stdout = handle.collected.stdout?.readFrom(0)?.text ?? '';
    const stderr = handle.collected.stderr?.readFrom(0)?.text ?? '';
    return {
      stdout,
      stderr,
      code: done.exitCode,
      killed: done.signal !== null && done.signal !== undefined,
      spawnFailed: false,
    };
  } catch (error) {
    return {
      stdout: '',
      stderr: '',
      code: null,
      killed: false,
      spawnFailed: true,
      spawnError: error instanceof Error ? error : new Error(String(error)),
    };
  }
}

/**
 * Runs with an execute-side timeout (§7.3.6): at the deadline the process is
 * terminated via the abort signal (the seam owns process-tree termination)
 * and the outcome reports `timedOut` with the waited duration. A killed run
 * has no output to parse (the CLI buffers until process end, §7.4.4). The
 * caller's own signal (tool cancellation) also aborts and surfaces as
 * `killed` + `timedOut: false` via `cancelledByCaller`.
 */
export async function runBinaryWithTimeout(
  subprocess: SubprocessSeam,
  spec: RunSpec,
  timeoutMs: number,
): Promise<RunOutcome & { cancelledByCaller?: boolean }> {
  const startedAt = Date.now();
  const controller = new AbortController();
  let callerAborted = false;
  const callerListener = () => {
    callerAborted = true;
    controller.abort();
  };
  spec.signal?.addEventListener('abort', callerListener, { once: true });
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const outcome = await runBinary(subprocess, { ...spec, signal: controller.signal });
    if (outcome.killed && !callerAborted && Date.now() - startedAt >= timeoutMs - 50) {
      return { ...outcome, timedOut: true, waitedMs: Date.now() - startedAt };
    }
    if (callerAborted) {
      return { ...outcome, cancelledByCaller: true };
    }
    return outcome;
  } finally {
    clearTimeout(timer);
    spec.signal?.removeEventListener('abort', callerListener);
  }
}

export const LIMITS = { stdout: STDOUT_LIMIT, stderr: STDERR_LIMIT };
