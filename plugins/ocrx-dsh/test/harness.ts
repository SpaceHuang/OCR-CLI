// Fake ctx harness (D-9): a minimal stand-in for the DSH/Cordis plugin
// context. Records registrations so tests can assert what the plugin
// contributes, and stubs the subprocess/workingDirectory/approval seams.
//
// The real host types live in src/dsh-seams.d.ts (hand-written minimal
// interfaces; the published @deepseek-ai/dsh CLI package does not ship
// plugin-facing type declarations — stage-0 spike conclusion).

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

export interface SpawnRequest {
  file: string;
  args: string[];
  options: {
    cwd: string;
    env?: Record<string, string>;
    signal?: AbortSignal;
    stdio?: unknown;
  } & Record<string, unknown>;
}

/** Deterministic subprocess stub: replays scripted results in order. */
export class FakeSubprocess {
  readonly requests: SpawnRequest[] = [];
  private script: Array<(request: SpawnRequest) => FakeSpawnResult> = [];

  /** Queue one scripted result for the next spawn call. */
  enqueue(result: FakeSpawnResult): void {
    this.script.push(() => result);
  }

  /** Queue a scripted result computed from the request (argv assertions). */
  enqueueWith(fn: (request: SpawnRequest) => FakeSpawnResult): void {
    this.script.push(fn);
  }

  readonly spawn = (file: string, args: string[], options: SpawnRequest['options']) => {
    const request: SpawnRequest = { file, args: [...args], options };
    this.requests.push(request);
    const next = this.script.shift();
    if (!next) {
      throw new Error(`FakeSubprocess has no scripted result for: ${file} ${args.join(' ')}`);
    }
    return next(request);
  };
}

export interface FakeSpawnResult {
  stdout?: string;
  stderr?: string;
  code?: number;
  killed?: boolean;
}

/** Sequenced working directory stub (each call pops the next value). */
export class FakeWorkingDirectory {
  constructor(private readonly dirs: string[]) {}
  calls = 0;
  readonly ensure = (): string => {
    const dir = this.dirs[Math.min(this.calls, this.dirs.length - 1)] ?? process.cwd();
    this.calls += 1;
    return dir;
  };
}

/** A scripted approval service for the consent-gate tests. */
export class FakeApproval {
  constructor(
    private readonly behavior:
      | { kind: 'resolve'; value: string }
      | { kind: 'reject'; error: Error }
      | { kind: 'never' },
  ) {}
  requests: Array<{ title: string; body: string }> = [];
  readonly request = async (input: { title: string; body: string }): Promise<unknown> => {
    this.requests.push({ title: input.title, body: input.body });
    if (this.behavior.kind === 'resolve') return this.behavior.value;
    if (this.behavior.kind === 'reject') throw this.behavior.error;
    return 'never';
  };
}

export function makeTempDir(prefix: string): string {
  return mkdtempSync(path.join(tmpdir(), prefix));
}

export function removeTempDir(dir: string): void {
  rmSync(dir, { recursive: true, force: true });
}

export const utf8 = 'utf8';

// --- Real process helpers (used by run.ts tests and timeout tests) ---------

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
