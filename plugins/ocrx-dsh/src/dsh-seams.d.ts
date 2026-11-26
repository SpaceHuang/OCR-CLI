// Hand-written minimal type surface for the DSH/Cordis seams this plugin
// consumes (plan D-2: the published @deepseek-ai/dsh CLI package does not
// ship plugin-facing declarations — stage-0 spike conclusion — so we pin the
// exact subset we call). Structural only: the host provides the real
// implementations; unknown-tolerant where the frozen window allows variation.

/** Approval service face reached optionally via ctx.get('approval'). */
export interface ApprovalService {
  request(input: {
    agent?: unknown;
    toolName?: string;
    callId?: string;
    reason?: string;
    displayReason?: string;
    signal?: AbortSignal;
    [key: string]: unknown;
  }): Promise<ApprovalRequestOutcome>;
}

/** Outcome vocabulary of approval.request (dsh-user-approval types.ts). */
export type ApprovalRequestOutcome =
  | 'allowed-once'
  | 'rejected'
  | 'cancelled'
  | 'unavailable'
  | string;

export interface SpawnOptions {
  argv: string[];
  cwd: string;
  env?: Record<string, string | undefined>;
  signal?: AbortSignal;
  stdio?: {
    stdin?: 'ignore' | 'pipe';
    stdout?: 'pipe' | 'inherit' | { maxBytes: number; spill?: { maxBytes: number } };
    stderr?: 'pipe' | 'inherit' | { maxBytes: number; spill?: { maxBytes: number } };
    control?: 'pipe';
  };
  graceMs?: number;
}

export interface SpawnHandle {
  collected: {
    stdout?: CollectedOutput;
    stderr?: CollectedOutput;
  };
  done: Promise<{ exitCode: number | null; signal: string | null }>;
  terminate(): void | Promise<void>;
  waitForExit(signal?: AbortSignal): Promise<void>;
}

export interface CollectedOutput {
  readFrom(offset: number): { text: string; offset: number; truncated?: boolean };
  byteLength?: number;
}

export interface SubprocessSeam {
  spawn(spec: SpawnOptions): SpawnHandle;
}

export interface WorkingDirectorySeam {
  ensure(agent?: unknown, signal?: AbortSignal): Promise<string>;
}

/** Minimal context face: `get` is the optional-service accessor. */
export interface PluginContext {
  get<T = unknown>(name: string): T | undefined;
  tools: { register(definition: unknown, options?: Record<string, unknown>): unknown };
  commands: { register(definition: unknown): unknown };
  skills: { registerProvider(create: unknown): unknown };
  [key: string]: unknown;
}
