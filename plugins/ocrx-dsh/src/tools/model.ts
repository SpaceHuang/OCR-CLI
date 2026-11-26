// ocrx_model tool (requirement §7.5.2; implementation lands in plan stage 5).
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import type { ToolEnv } from './scan.js';

export function createModelTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_model',
    description:
      'Query the local ocrx model catalog: list registered models, show model info, recommend ' +
      'a profile for this machine, or verify install integrity. With the machine-readable ' +
      'plan output, recommend also reports exact download size and target directory.',
    parameters: {
      action: {
        type: 'string',
        description: 'Catalog query to run.',
        enum: ['list', 'info', 'recommend', 'verify'],
      },
      model: {
        type: 'string',
        description: 'Model selector (info/verify/recommend).',
        enum: ['ovisocr2', 'ovisocr3'],
      },
      profile: {
        type: 'string',
        description: 'Profile to verify (verify only).',
        enum: ['compact', 'balanced', 'quality', 'reference'],
      },
    },
    required: ['action'],
    isConcurrencySafe: () => true,
    outputDescription: 'Model catalog query result (text passthrough or machine-readable plan).',
    renderText: (_args, value) => textResult(value),
    validate: (args) => {
      const violations: string[] = [];
      if (args.action === 'verify' && typeof args.profile !== 'string') {
        violations.push('verify requires a profile');
      }
      return violations;
    },
    execute: async () => {
      // Stage 5 wires argv/run/parse here.
      return {
        ok: false,
        error: {
          reason: 'cli_error',
          message: 'ocrx_model execute pipeline not wired in this build',
          guidance: 'This placeholder is replaced by plan stage 5.',
        },
        text: 'ocrx_model is not wired in this build (plan stage 5 pending).',
      };
    },
  });
}
