// ocrx_scan tool (requirement §7.2/§7.3; implementation lands in plan stage 4).
import { defineTool, textResult, type ToolDefinition, type ToolRunContext } from '../toolkit.js';
import type { OcrxConfig } from '../config.js';
import type { PluginContext } from '../types.js';

export interface ToolEnv {
  config: OcrxConfig;
  ctx: PluginContext;
}

export function createScanTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_scan',
    description:
      'Convert PDF documents to Markdown with the local ocrx CLI. Always runs offline ' +
      '(never downloads anything). Directory input scans every PDF inside it in one batch. ' +
      'Returns a compact per-document summary plus artifact paths; it never inlines full ' +
      'Markdown bodies.',
    parameters: {
      input: {
        type: 'array',
        description: 'Absolute paths to PDF files, or directories (batch).',
        items: { type: 'string' },
      },
      outputDir: { type: 'string', description: 'Absolute output directory for scan artifacts.' },
      pages: {
        type: 'string',
        description:
          'Optional page selection, e.g. "1,3-5". Sample large PDFs with a few pages first.',
      },
      model: {
        type: 'string',
        description: 'Explicit model. Omit to let the CLI auto-select.',
        enum: ['ovisocr2', 'ovisocr3'],
      },
      modelProfile: {
        type: 'string',
        description: 'Explicit quality profile. Omit for the CLI default resolution.',
        enum: ['auto', 'compact', 'balanced', 'quality', 'reference'],
      },
      pdfMode: {
        type: 'string',
        description: 'PDF processing mode.',
        enum: ['auto', 'text', 'ocr'],
      },
      qualityTier: {
        type: 'string',
        description: 'Quality tier.',
        enum: ['l1', 'l2'],
      },
      device: {
        type: 'string',
        description:
          'Execution device. cuda/vulkan are unavailable in the bundled Windows CPU build ' +
          '(use the ocrx command config for a GPU build).',
        enum: ['auto', 'cpu', 'metal', 'cuda', 'vulkan'],
      },
      resume: {
        type: 'boolean',
        description:
          'Resume an interrupted scan (page-level checkpoints). Reuse after a model install ' +
          'to keep already-scanned documents.',
      },
      overwrite: {
        type: 'boolean',
        description:
          'Allow writing into an existing non-empty output directory. This does NOT delete ' +
          'previous artifacts.',
      },
      password: {
        type: 'string',
        description:
          'Open password for encrypted PDFs. WARNING: tool arguments are recorded in the ' +
          'session history and are visible to the model.',
      },
    },
    required: ['input', 'outputDir'],
    timeoutMs: env.config.maxTimeoutMs,
    isConcurrencySafe: () => false,
    outputDescription:
      'Normalized scan result: ok/partial flags, per-document status and error reasons, ' +
      'artifact paths, and model readiness guidance when the OCR model is missing.',
    renderText: (_args, value) => textResult(value),
    validate: (args) => {
      const violations: string[] = [];
      if (args.resume && args.overwrite) {
        violations.push('resume and overwrite are mutually exclusive');
      }
      return violations;
    },
    execute: async (_args: Record<string, unknown>, _exec: ToolRunContext) => {
      // Stage 4 wires argv/run/parse/readiness here.
      return {
        ok: false,
        exit_code: null,
        partial: false,
        documents: [],
        error: {
          reason: 'cli_error',
          message: 'ocrx_scan execute pipeline not wired in this build',
          guidance: 'This placeholder is replaced by plan stage 4.',
        },
        text: 'ocrx_scan is not wired in this build (plan stage 4 pending).',
      };
    },
  });
}
