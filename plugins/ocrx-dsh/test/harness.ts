// Fake ctx harness (D-9): a minimal stand-in for the DSH/Cordis plugin
// context. Records registrations so tests can assert what the plugin
// contributes, and stubs the subprocess/workingDirectory/approval seams.
//
// The FakeSubprocess mirrors the REAL seam contract verified in the stage-0
// spike (dsh-subprocess types.ts): spawn(spec) with { argv, cwd, stdio:
// collect specs, graceMs, signal?, env? } returns a handle with collected
// offset readers, a done promise, terminate() and waitForExit().

import * as assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

export interface RegisteredTool {
  name: string;
  options: Record<string, unknown>;
}

export interface RegisteredCommand {
  name: string;
  options: Record<string, unknown>;
}

export interface SkillProviderRecord {
  provider: unknown;
}

/** Records the tool/command/skill registrations performed by apply(). */
export class FakeCtx {
  readonly tools: { register: (tool: unknown, options?: unknown) => void } = {
    register: (tool: unknown, options?: unknown) => {
      assert.ok(tool && typeof tool === 'object', 'tool must be an object');
      const name = (tool as { name?: unknown }).name;
      assert.equal(typeof name, 'string', 'tool.name must be a string');
      this.registeredTools.push({
        name: name as string,
        options: (options ?? {}) as Record<string, unknown>,
      });
    },
  };

  readonly commands: { register: (command: unknown, options?: unknown) => void } = {
    register: (command: unknown, options?: unknown) => {
      const name = (command as { name?: unknown }).name;
      assert.equal(typeof name, 'string', 'command.name must be a string');
      this.registeredCommands.push({
        name: name as string,
        options: (options ?? {}) as Record<string, unknown>,
      });
    },
  };

  readonly skills: { registerProvider: (provider: unknown) => void } = {
    registerProvider: (provider: unknown) => {
      this.registeredSkillProviders.push({ provider });
    },
  };

  readonly registeredTools: RegisteredTool[] = [];
  readonly registeredCommands: RegisteredCommand[] = [];
  readonly registeredSkillProviders: SkillProviderRecord[] = [];

  /** Services exposed through ctx.get('...'). Undefined = service absent. */
  approval: unknown;
  subprocess: unknown;
  workingDirectory: unknown;

  get(name: string): unknown {
    if (name === 'approval') return this.approval;
    if (name === 'subprocess') return this.subprocess;
    if (name === 'workingDirectory') return this.workingDirectory;
    return undefined;
  }
}

// --- Subprocess seam doubles ----------------------------------------------

export interface CollectMode {
  maxBytes: number;
  spill?: { maxBytes: number };
}

export interface SpawnSpec {
  argv: string[];
  cwd: string;
  env?: Record<string, string | undefined>;
  signal?: AbortSignal;
  stdio?: {
    stdin?: 'ignore' | 'pipe';
    stdout?: 'pipe' | 'inherit' | CollectMode;
    stderr?: 'pipe' | 'inherit' | CollectMode;
    control?: 'pipe';
  };
  graceMs?: number;
}

export interface CollectedText {
  text: string;
  offset: number;
  truncated: boolean;
}

export interface FakeHandle {
  collected: {
    stdout?: { readFrom(offset: number): CollectedText };
    stderr?: { readFrom(offset: number): CollectedText };
  };
  done: Promise<{ exitCode: number | null; signal: string | null }>;
  terminate(): void | Promise<void>;
  waitForExit(signal?: AbortSignal): Promise<void>;
  /** The spec this handle serves (argv/cwd/env assertions). */
  spec: SpawnSpec;
}

export interface FakeSpawnResult {
  stdout?: string;
  stderr?: string;
  code?: number | null;
  /** Simulates the process having been killed (SIGKILL face). */
  killed?: boolean;
  /** Delay before the process finishes (ms) — for timeout races. */
  delayMs?: number;
}

/** Deterministic subprocess stub: replays scripted results in order. */
export class FakeSubprocess {
  readonly requests: SpawnSpec[] = [];
  /** Terminated handles (cancel/timeout assertions). */
  readonly terminated: FakeHandle[] = [];
  private script: Array<(spec: SpawnSpec) => FakeSpawnResult> = [];

  /** Queue one scripted result for the next spawn call. */
  enqueue(result: FakeSpawnResult): void {
    this.script.push(() => result);
  }

  /** Queue a scripted result computed from the spec (argv assertions). */
  enqueueWith(fn: (spec: SpawnSpec) => FakeSpawnResult): void {
    this.script.push(fn);
  }

  /** Fail the spawn itself (service refused / ENOENT face). */
  spawnError: Error | null = null;

  readonly spawn = (spec: SpawnSpec): FakeHandle => {
    this.requests.push(spec);
    if (this.spawnError) throw this.spawnError;
    const next = this.script.shift();
    if (!next) {
      throw new Error(`FakeSubprocess has no scripted result for: ${spec.argv.join(' ')}`);
    }
    const result = next(spec);
    return makeFakeHandle(spec, result, this.terminated);
  };
}

export function makeFakeHandle(
  spec: SpawnSpec,
  result: FakeSpawnResult,
  terminatedRegistry: FakeHandle[] = [],
): FakeHandle {
  const stdout = result.stdout ?? '';
  const stderr = result.stderr ?? '';
  const handle = {
    spec,
    collected: {
      stdout: {
        readFrom(offset: number): CollectedText {
          const text = stdout.slice(offset);
          return { text, offset, truncated: false };
        },
      },
      stderr: {
        readFrom(offset: number): CollectedText {
          const text = stderr.slice(offset);
          return { text, offset, truncated: false };
        },
      },
    },
    terminated: false,
    async terminate(): Promise<void> {
      if (!handle.terminated) {
        handle.terminated = true;
        terminatedRegistry.push(handle as unknown as FakeHandle);
      }
    },
    async waitForExit(): Promise<void> {
      await done;
    },
    done: undefined as unknown as Promise<{ exitCode: number | null; signal: string | null }>,
  };
  // The real seam escalates an aborted signal into termination; mirror that.
  spec.signal?.addEventListener(
    'abort',
    () => {
      void handle.terminate();
    },
    { once: true },
  );
  let resolveDone!: (value: { exitCode: number | null; signal: string | null }) => void;
  const done = new Promise<{ exitCode: number | null; signal: string | null }>((resolve) => {
    resolveDone = resolve;
  });
  handle.done = done;
  const finish = (): void => {
    // A terminated handle reports a kill signal, not its script code.
    if (handle.terminated) {
      resolveDone({ exitCode: null, signal: 'SIGKILL' });
    } else {
      resolveDone({ exitCode: result.code ?? 0, signal: null });
    }
  };
  if (result.delayMs) {
    // terminate() resolves early; a later timer resolution is a no-op
    // (Promise resolve is idempotent). NOT unref'd: nothing else keeps the
    // loop alive while tests await `done`.
    setTimeout(finish, result.delayMs);
  } else {
    finish();
  }
  return handle as unknown as FakeHandle;
}

/** Sequenced working directory stub (each call pops the next value). */
export class FakeWorkingDirectory {
  private readonly dirs: string[];
  calls = 0;
  constructor(dirs: string[]) {
    this.dirs = dirs;
  }
  readonly ensure = async (): Promise<string> => {
    const dir = this.dirs[Math.min(this.calls, this.dirs.length - 1)] ?? process.cwd();
    this.calls += 1;
    return dir;
  };
}

/** A scripted approval service for the consent-gate tests. */
export class FakeApproval {
  private readonly behavior:
    | { kind: 'resolve'; value: string }
    | { kind: 'reject'; error: Error }
    | { kind: 'never' };
  /** Mirrors the real request contract: reason (title) + displayReason (body). */
  requests: Array<{ title: string; body: string; toolName?: string }> = [];
  constructor(behavior: FakeApproval['behavior']) {
    this.behavior = behavior;
  }
  readonly request = async (input: {
    reason?: string;
    displayReason?: string;
    title?: string;
    body?: string;
    toolName?: string;
  }): Promise<unknown> => {
    this.requests.push({
      title: input.title ?? input.reason ?? '',
      body: input.body ?? input.displayReason ?? '',
      toolName: input.toolName,
    });
    if (this.behavior.kind === 'resolve') return this.behavior.value;
    if (this.behavior.kind === 'reject') throw this.behavior.error;
    return 'rejected';
  };
}

export function makeTempDir(prefix: string): string {
  return mkdtempSync(path.join(tmpdir(), prefix));
}

export function removeTempDir(dir: string): void {
  rmSync(dir, { recursive: true, force: true });
}

// --- Real process helpers (used by run.ts tests and integration tests) -----

export interface RealSpawnOptions {
  file: string;
  args: string[];
  cwd: string;
  env?: Record<string, string | undefined>;
  signal?: AbortSignal;
  timeoutMs?: number;
}

export interface RealSpawnOutcome {
  stdout: string;
  stderr: string;
  code: number | null;
  killed: boolean;
}

/** Spawns a real child process (used where the seam contract matters). */
export function spawnReal(options: RealSpawnOptions): Promise<RealSpawnOutcome> {
  return new Promise((resolve, reject) => {
    const child = spawn(options.file, options.args, {
      cwd: options.cwd,
      env: options.env as NodeJS.ProcessEnv | undefined,
      signal: options.signal,
      windowsHide: true,
    });
    let stdout = '';
    let stderr = '';
    child.stdout?.setEncoding('utf8');
    child.stderr?.setEncoding('utf8');
    child.stdout?.on('data', (chunk: string) => {
      stdout += chunk;
    });
    child.stderr?.on('data', (chunk: string) => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('close', (code, signal) => {
      resolve({ stdout, stderr, code, killed: signal !== null });
    });
    if (options.timeoutMs) {
      const timer = setTimeout(() => child.kill(), options.timeoutMs);
      timer.unref();
    }
  });
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    timer.unref();
  });
}

export async function waitFor<T>(
  predicate: () => T | undefined,
  timeoutMs = 5000,
): Promise<T> {
  const start = Date.now();
  for (;;) {
    const value = predicate();
    if (value !== undefined) return value;
    if (Date.now() - start > timeoutMs) throw new Error('waitFor timed out');
    await sleep(10);
  }
}

/** The fake binary wrapper path for the current platform. */
export function fakeOcrxPath(): string {
  const wrapper = process.platform === 'win32' ? 'ocrx.cmd' : 'ocrx.sh';
  return path.join(__dirname, 'fake-wrappers', wrapper);
}
