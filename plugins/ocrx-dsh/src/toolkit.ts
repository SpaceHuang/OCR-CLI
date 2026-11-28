// Tool definition builder (plan D-2 / spike conclusion): the DSH tool
// registry consumes plain `ToolDefinition` objects — `defineTool` is a pure
// DSL→JSON-Schema transform (dsh-tools schema.ts:555-603) — so this plugin
// compiles the same shape itself and keeps ZERO imports from host packages.
// The compiled shape mirrored here:
//   parameters: { type:'object', properties: {...}, required?: [...] }
//   output:     { schema, render(args, value) => ContentBlock[] }
//   execute(args, exec) → canonical JSON value; failures throw
//   timeoutMs / isConcurrencySafe are static/functional optional fields.
// The CI dsh-loading job (stage 9) verifies real-host acceptance.

export interface ToolParamSpec {
  type: 'string' | 'boolean' | 'array' | 'integer';
  description: string;
  enum?: string[];
  items?: { type: 'string' | 'integer' };
}

export interface ToolArgsError extends Error {
  readonly violations: string[];
}

export interface ToolRunContext {
  agent?: unknown;
  callId?: string;
  signal?: AbortSignal;
  [key: string]: unknown;
}

export interface ToolDefinition {
  name: string;
  description: string;
  parameters: {
    type: 'object';
    properties: Record<string, ToolParamSpec>;
    required?: string[];
  };
  output: {
    schema: { type: string; description?: string };
    render(args: unknown, value: unknown): Array<{ type: 'text'; text: string }>;
  };
  timeoutMs?: number;
  isConcurrencySafe?: (args: unknown) => boolean;
  execute(args: unknown, exec: ToolRunContext): Promise<unknown>;
}

export function toolArgsError(name: string, violations: string[]): ToolArgsError {
  const error = new Error(`invalid arguments for ${name}: ${violations.join('; ')}`) as ToolArgsError;
  error.name = 'ToolArgsError';
  Object.defineProperty(error, 'violations', { value: violations });
  return error;
}

interface RawToolInput {
  name: string;
  description: string;
  parameters: Record<string, ToolParamSpec>;
  required?: string[];
  outputDescription?: string;
  timeoutMs?: number;
  isConcurrencySafe?: (args: unknown) => boolean;
  renderText: (args: Record<string, unknown>, value: unknown) => string;
  execute: (args: Record<string, unknown>, exec: ToolRunContext) => Promise<unknown>;
  validate?: (args: Record<string, unknown>) => string[];
}

/** Validates incoming args against the declared parameter spec. */
export function validateArgs(
  name: string,
  parameters: Record<string, ToolParamSpec>,
  required: string[],
  args: unknown,
): string[] {
  const violations: string[] = [];
  if (args === null || typeof args !== 'object' || Array.isArray(args)) {
    return [`${name}: arguments must be an object`];
  }
  const record = args as Record<string, unknown>;
  for (const key of required) {
    if (!(key in record) || record[key] === undefined) violations.push(`${key} is required`);
  }
  for (const [key, spec] of Object.entries(parameters)) {
    const value = record[key];
    if (value === undefined) continue;
    if (spec.type === 'array') {
      if (!Array.isArray(value)) {
        violations.push(`${key} must be an array`);
        continue;
      }
      const itemType = spec.items?.type;
      if (itemType === 'string' && value.some((item) => typeof item !== 'string')) {
        violations.push(`${key} items must be strings`);
      }
      if (itemType === 'integer' && value.some((item) => !Number.isInteger(item))) {
        violations.push(`${key} items must be integers`);
      }
      continue;
    }
    if (typeof value !== spec.type) {
      violations.push(`${key} must be a ${spec.type}`);
      continue;
    }
    if (spec.type === 'string' && spec.enum !== undefined && !(spec.enum as string[]).includes(value as string)) {
      violations.push(`${key} must be one of ${spec.enum.join(' | ')}`);
    }
  }
  return violations;
}

/** Compiles a validated tool definition (the defineTool output shape). */
export function defineTool(input: RawToolInput): ToolDefinition {
  if (input.timeoutMs !== undefined && (!Number.isFinite(input.timeoutMs) || input.timeoutMs <= 0)) {
    throw new Error(`defineTool(${input.name}): timeoutMs must be a positive finite number`);
  }
  const parameters: ToolDefinition['parameters'] = {
    type: 'object',
    properties: input.parameters,
    ...(input.required && input.required.length > 0 ? { required: input.required } : {}),
  };
  return {
    name: input.name,
    description: input.description,
    parameters,
    output: {
      // The rc.5 host schema validator accepts ONLY
      // object/array/string/number/integer/boolean/null (found by the real
      // host loading face — `type: 'json'` was rejected at registration).
      // Tool results are canonical JSON objects, so `object` is the honest
      // permissive root.
      schema: { type: 'object', description: input.outputDescription ?? input.description },
      render(args, value) {
        return [{ type: 'text', text: input.renderText(args as Record<string, unknown>, value) }];
      },
    },
    ...(input.timeoutMs !== undefined ? { timeoutMs: input.timeoutMs } : {}),
    ...(input.isConcurrencySafe ? { isConcurrencySafe: input.isConcurrencySafe } : {}),
    async execute(args, exec) {
      const violations = [
        ...validateArgs(input.name, input.parameters, input.required ?? [], args),
        ...(input.validate?.(args as Record<string, unknown>) ?? []),
      ];
      if (violations.length > 0) throw toolArgsError(input.name, violations);
      return input.execute(args as Record<string, unknown>, exec);
    },
  };
}

/** Default text delivery: tools carry a ready text line, else pretty JSON. */
export function textResult(value: unknown): string {
  const text = (value as { text?: unknown }).text;
  return typeof text === 'string' ? text : JSON.stringify(value, null, 2);
}

/** JSON-serializes a value deterministically for tool results. */
export function toJson(value: unknown): string {
  return JSON.stringify(value, null, 2);
}
