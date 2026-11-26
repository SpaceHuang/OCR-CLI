// Shared types: normalized result schema (requirement §7.4), reason
// enumeration (§7.6) and the DSH seam value faces.

import type { PluginContext } from './dsh-seams.js';

export type { PluginContext };

export interface SubprocessSeam {
  spawn(options: {
    command: string;
    args: string[];
    cwd: string;
    env?: Record<string, string>;
    signal?: AbortSignal;
  }): SpawnOutcome | Promise<SpawnOutcome>;
}

export interface SpawnOutcome {
  stdout?: string;
  stderr?: string;
  code?: number | null;
  killed?: boolean;
  error?: unknown;
}

export interface WorkingDirectorySeam {
  ensure(path?: string, options?: { agent?: unknown }): string;
}

export interface ApprovalOutcome {
  kind: 'approved' | 'rejected' | 'unavailable' | 'cancelled';
}

/** CLI failure object, schema v1 (ocrx-output ErrorReport JSON). */
export interface CliFailureObject {
  schema_version: number;
  code: number;
  kind: string;
  message: string;
  cause: string | null;
  suggestion: string | null;
}

/** Reason enumeration (requirement §7.6). */
export type FailureReason =
  | 'usage'
  | 'input'
  | 'pdfium_missing'
  | 'pdf_native'
  | 'model_missing'
  | 'model'
  | 'output'
  | 'runtime'
  | 'device_unavailable'
  | 'partial'
  | 'interrupted'
  | 'timeout'
  | 'cli_error'
  | 'launcher_failed'
  | 'binary_not_found'
  | 'install_blocked'
  | 'install_failed'
  | 'consent_denied'
  | 'consent_cancelled';

/** Normalized error block carried by results (§7.4). */
export interface NormalizedError {
  reason: FailureReason;
  message: string;
  guidance: string;
  /** Raw CLI fields preserved for cli_error/install_failed transparency. */
  code?: number;
  kind?: string;
  cause?: string | null;
  suggestion?: string | null;
  /** Set when a table code arrived without a failure object (§7.6 rule 4). */
  details_unavailable?: boolean;
  /** Set when batch failure attribution degraded (plan §6 阶段 4). */
  attribution_degraded?: boolean;
}

export interface DocumentError {
  reason: FailureReason;
  message: string;
  guidance: string;
  details_unavailable?: boolean;
}

export interface NormalizedDocument {
  input: string;
  output_dir: string | null;
  status: 'success' | 'partial' | 'failed' | 'cancelled';
  exit_code: number;
  pages?: number;
  markdown?: string;
  result_json?: string;
  warnings_count?: number;
  duration_ms?: number;
  error: DocumentError | null;
}

export interface ReadinessInfo {
  model_missing: true;
  recommend: {
    model: string;
    profile: string;
    size_bytes?: number;
    target_dir?: string;
    /** D-11 degrade marker when the C2 plan output is unavailable. */
    size_unconfirmed?: boolean;
  };
}

export interface ScanToolResult {
  ok: boolean;
  exit_code: number | null;
  partial: boolean;
  documents: NormalizedDocument[];
  scan_summary?: string;
  error: NormalizedError | null;
  readiness?: ReadinessInfo;
  /** Human-facing compact summary + artifact paths (§7.7.1). */
  text?: string;
  [key: string]: unknown;
}

/** C2 install-plan schema (plan D-4, schema_version 1). */
export interface InstallPlan {
  schema_version: number;
  action: 'recommend';
  model: string;
  profile: string;
  default_model?: string;
  revision: string;
  size_bytes: number;
  target_dir: string;
  assets: Array<{ role: string; path: string; size_bytes: number }>;
  installed: boolean;
  cache_root: string;
}

/** C2 install success summary (plan D-5). */
export interface InstallSummary {
  model: string;
  profile: string;
  revision: string;
  size_bytes: number;
  elapsed_ms: number;
  stage: string;
  download: {
    downloaded: boolean;
    per_asset: Array<{ role: string; bytes: number; elapsed_ms: number; avg_rate_bps: number }>;
    total_bytes: number;
    avg_rate_bps: number;
  };
  verify: {
    per_asset: Array<{ role: string; sha256_ok: boolean; size_ok: boolean }>;
  };
}

/** Binary location result (requirement §6.5). */
export interface BinaryLocation {
  path: string;
  source: 'config' | 'bundled' | 'path' | 'known';
}

export interface ResolvedBinary {
  location: BinaryLocation | null;
  launcherFailed: boolean;
}
