// Config schema and load-time validation (requirement §6.4).
//
// Validation failures throw during apply() so an illegal config blocks
// plugin load (requirement §14.2). Field semantics:
// - `offline` only affects ocrx_model / ocrx_model_install; ocrx_scan always
//   runs with --offline regardless (§11.3, hardcoded in argv/scan.ts).
// - `defaultTimeoutMs` is enforced inside scan execute; `maxTimeoutMs` is the
//   registration-time pipeline timeoutMs (§7.3.6).
// - `installTimeoutMs` is optional and never defaults: the install tool does
//   not register a pipeline timeout unless configured (§6.4/§7.5.3).
// - `enableModelInstall: false` means the tool is NOT registered (§6.4).

export interface OcrxConfig {
  command?: string;
  extraEnv: Record<string, string>;
  offline: boolean;
  defaultTimeoutMs: number;
  maxTimeoutMs: number;
  installTimeoutMs?: number;
  enableScan: boolean;
  enableDoctor: boolean;
  enableModel: boolean;
  enableModelInstall: boolean;
}

export const DEFAULTS: OcrxConfig = {
  extraEnv: {},
  offline: false,
  defaultTimeoutMs: 1_800_000,
  maxTimeoutMs: 14_400_000,
  enableScan: true,
  enableDoctor: true,
  enableModel: true,
  enableModelInstall: true,
};

/** Normalizes and validates raw config; throws on illegal values. */
export function resolveConfig(raw: Record<string, unknown> | undefined | null): OcrxConfig {
  const input = raw ?? {};
  const command = optionalString(input.command, 'command');
  const extraEnv = stringRecord(input.extraEnv, 'extraEnv');
  const offline = optionalBoolean(input.offline, 'offline') ?? DEFAULTS.offline;
  const defaultTimeoutMs = optionalPositiveInt(input.defaultTimeoutMs, 'defaultTimeoutMs');
  const maxTimeoutMs = optionalPositiveInt(input.maxTimeoutMs, 'maxTimeoutMs');
  const installTimeoutMs = optionalPositiveInt(input.installTimeoutMs, 'installTimeoutMs');
  const resolved: OcrxConfig = {
    ...(command === undefined ? {} : { command }),
    extraEnv,
    offline,
    defaultTimeoutMs: defaultTimeoutMs ?? DEFAULTS.defaultTimeoutMs,
    maxTimeoutMs: maxTimeoutMs ?? DEFAULTS.maxTimeoutMs,
    ...(installTimeoutMs === undefined ? {} : { installTimeoutMs }),
    enableScan: optionalBoolean(input.enableScan, 'enableScan') ?? DEFAULTS.enableScan,
    enableDoctor: optionalBoolean(input.enableDoctor, 'enableDoctor') ?? DEFAULTS.enableDoctor,
    enableModel: optionalBoolean(input.enableModel, 'enableModel') ?? DEFAULTS.enableModel,
    enableModelInstall:
      optionalBoolean(input.enableModelInstall, 'enableModelInstall') ?? DEFAULTS.enableModelInstall,
  };
  if (resolved.defaultTimeoutMs > resolved.maxTimeoutMs) {
    throw new TypeError(
      `ocrx-dsh: defaultTimeoutMs (${resolved.defaultTimeoutMs}) must be <= maxTimeoutMs (${resolved.maxTimeoutMs})`,
    );
  }
  return resolved;
}

function fail(field: string, expectation: string, value: unknown): never {
  throw new TypeError(`ocrx-dsh: config.${field} must be ${expectation}, got ${display(value)}`);
}

function display(value: unknown): string {
  return typeof value === 'string' ? JSON.stringify(value) : String(value);
}

function optionalString(value: unknown, field: string): string | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'string' || value.length === 0) fail(field, 'a non-empty string', value);
  return value;
}

function optionalBoolean(value: unknown, field: string): boolean | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'boolean') fail(field, 'a boolean', value);
  return value;
}

function optionalPositiveInt(value: unknown, field: string): number | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'number' || !Number.isInteger(value) || value <= 0) {
    fail(field, 'a positive integer', value);
  }
  return value;
}

function stringRecord(value: unknown, field: string): Record<string, string> {
  if (value === undefined || value === null) return {};
  if (typeof value !== 'object' || Array.isArray(value)) {
    fail(field, 'an object of string values', value);
  }
  const out: Record<string, string> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    if (typeof entry !== 'string') fail(`${field}.${key}`, 'a string', entry);
    out[key] = entry;
  }
  return out;
}
