// stdout/stderr 形态判别 (requirement §1.3.4/§7.3.2/§7.4): stdout 有三种
// 形态（单文档 v3 / 多文档聚合 / 失败对象 v1——单文档失败就在 stdout），
// 必须先判别形态；stderr 全流逐行解析 schema v1 失败对象（批量），人类
// 可读行保留为有界尾部兜底。
import type { CliFailureObject } from '../types.js';

export interface V3Result {
  schema_version: number;
  status: 'success' | 'partial' | string;
  input: string;
  markdown_path?: string | null;
  output_dir: string;
  pages?: { total?: number; completed?: number; failed?: number };
  warnings?: string[];
  elapsed_ms?: number;
  images?: { directory?: string; count?: number };
  [key: string]: unknown;
}

export interface AggregateDocument {
  input: string;
  output_dir: string;
  status: 'success' | 'partial' | 'failed' | 'cancelled' | string;
  exit_code: number;
  elapsed_ms?: number;
}

export interface Aggregate {
  schema_version: number;
  mode: 'multi' | string;
  inputs: string[];
  output_root: string;
  documents: AggregateDocument[];
  elapsed_ms?: number;
  [key: string]: unknown;
}

export type StdoutShape =
  | { kind: 'v3'; value: V3Result }
  | { kind: 'aggregate'; value: Aggregate }
  | { kind: 'failure'; value: CliFailureObject }
  | { kind: 'unparseable'; raw: string }
  | { kind: 'empty' };

/** True when the object is a schema v1 failure object. */
export function isFailureObject(value: unknown): value is CliFailureObject {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as CliFailureObject).schema_version === 1 &&
    typeof (value as CliFailureObject).code === 'number' &&
    typeof (value as CliFailureObject).kind === 'string'
  );
}

/** 判别 stdout 形态：先试 JSON（单行/整体），失败回落 unparseable。 */
export function parseStdout(text: string): StdoutShape {
  const trimmed = text.trim();
  if (trimmed.length === 0) return { kind: 'empty' };
  const candidates = [trimmed, ...trimmed.split(/\r?\n/).map((line) => line.trim())];
  for (const candidate of candidates) {
    if (candidate.length === 0) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(candidate);
    } catch {
      continue;
    }
    if (typeof parsed !== 'object' || parsed === null) continue;
    const record = parsed as Record<string, unknown>;
    if (record.schema_version === 3 && typeof record.output_dir === 'string') {
      return { kind: 'v3', value: parsed as V3Result };
    }
    if (
      record.schema_version === 1 &&
      record.mode === 'multi' &&
      Array.isArray(record.documents)
    ) {
      return { kind: 'aggregate', value: parsed as Aggregate };
    }
    if (isFailureObject(parsed)) {
      return { kind: 'failure', value: parsed as CliFailureObject };
    }
  }
  return { kind: 'unparseable', raw: trimmed };
}

export interface StderrParsed {
  /** 逐文档 schema v1 失败对象（按发出顺序）。 */
  objects: CliFailureObject[];
  /** 人类可读行（进度/边界行），保留用于兜底诊断。 */
  humanLines: string[];
  /** 有界尾部（原始 stderr 末尾，兜底保留）。 */
  tail: string;
}

const MAX_TAIL_CHARS = 8_000;

/** stderr 全流逐行解析：失败对象 JSON 行 vs 人类可读行混排。 */
export function parseStderr(text: string): StderrParsed {
  const objects: CliFailureObject[] = [];
  const humanLines: string[] = [];
  const lines = text.split(/\r?\n/);
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.length === 0) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch {
      humanLines.push(line);
      continue;
    }
    if (isFailureObject(parsed)) {
      objects.push(parsed);
    } else {
      humanLines.push(line);
    }
  }
  const tail = text.length > MAX_TAIL_CHARS ? text.slice(-MAX_TAIL_CHARS) : text;
  return { objects, humanLines, tail };
}
