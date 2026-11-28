// spawn 封装 (requirement §7.3; plan D-8): runs the located ocrx binary
// through the host subprocess seam. All argv fully explicit (no shell), cwd
// mandatory (D-8 fallback to process.cwd()), env = cleaned host environment
// + extraEnv, stdout/stderr bounded collect, and an execute-side timeout
// that actively terminates at the deadline.
import { readFileSync } from 'node:fs';

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
  /** stdout spill cap (§7.3.2: the aggregate JSON is a full-stream face). */
  stdoutSpillMaxBytes?: number;
  /** stderr spill cap (§7.3.2: the failure-object stream is full-fidelity). */
  stderrSpillMaxBytes?: number;
}

export interface RunOutcome {
  stdout: string;
  stderr: string;
  /** True when stderr overflowed even with spill (prefix lost). */
  stderrTruncated: boolean;
  /** True when stdout overflowed even with spill (prefix lost). */
  stdoutTruncated: boolean;
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
        // §7.3.2: BOTH streams are full-fidelity faces — stdout carries the
        // batch aggregate JSON, stderr the per-document failure objects. The
        // collect specs request spill files so a lost prefix is recoverable
        // (BLOCK-10: recovery is unreachable unless the spec ASKS for spill —
        // the host only creates spillPath for collect specs that request it).
        stdout: {
          maxBytes: spec.stdoutMaxBytes,
          spill: { maxBytes: spec.stdoutSpillMaxBytes ?? spec.stdoutMaxBytes * 8 },
        },
        stderr: {
          maxBytes: spec.stderrMaxBytes,
          spill: { maxBytes: spec.stderrSpillMaxBytes ?? spec.stderrMaxBytes * 8 },
        },
      },
      graceMs: DEFAULT_GRACE_MS,
    });
    const done = await handle.done;
    // Locked host rc.5 read faces: `readFrom(0)` returns the whole retained
    // tail with `lossy: true` when the in-memory window slid past the head,
    // and `spillPath` pointing at the full-stream file when the collect spec
    // requested a spill. Recovery: a readable spill IS the full stream (not
    // truncated); lossy without a usable spill means the prefix is gone.
    const stdoutRead = handle.collected.stdout?.readFrom(0);
    let stdout = stdoutRead?.text ?? '';
    let stdoutTruncated = false;
    if (stdoutRead?.lossy === true) {
      if (typeof stdoutRead.spillPath === 'string') {
        try {
          stdout = readFileSync(stdoutRead.spillPath, 'utf8');
        } catch {
          stdoutTruncated = true;
        }
      } else {
        stdoutTruncated = true;
      }
    }
    const stderrRead = handle.collected.stderr?.readFrom(0);
    let stderr = stderrRead?.text ?? '';
    let stderrTruncated = false;
    if (stderrRead?.lossy === true) {
      if (typeof stderrRead.spillPath === 'string') {
        try {
          stderr = readFileSync(stderrRead.spillPath, 'utf8');
        } catch {
          stderrTruncated = true;
        }
      } else {
        stderrTruncated = true;
      }
    }
    return {
      stdout,
      stderr,
      stderrTruncated,
      stdoutTruncated,
      code: done.exitCode,
      killed: done.signal !== null && done.signal !== undefined,
      spawnFailed: false,
    };
  } catch (error) {
    return {
      stdout: '',
      stderr: '',
      stderrTruncated: false,
      stdoutTruncated: false,
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
