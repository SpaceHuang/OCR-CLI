// ocrx_model_install tool (requirement §7.5.3/§11.3; implementation lands in
// plan stage 6). This is the ONLY download entry point and it carries its own
// consent gate implemented via ctx.get('approval').
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import type { ToolEnv } from './scan.js';

export function createModelInstallTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_model_install',
    description:
      'Download and install an ocrx OCR model (the only tool that ever downloads anything). ' +
      'Asks for explicit user consent first with the real size and target directory; refuses ' +
      'to download without approval. Downloads cannot resume once interrupted.',
    parameters: {
      model: {
        type: 'string',
        description: 'Model to install. Omit to use the CLI default resolution.',
        enum: ['ovisocr2', 'ovisocr3'],
      },
      modelProfile: {
        type: 'string',
        description:
          'Profile to install. Omit to inherit the readiness target resolved from the failed ' +
          'scan (the bootstrap flow).',
        enum: ['compact', 'balanced', 'quality', 'reference'],
      },
    },
    required: [],
    isConcurrencySafe: () => false,
    outputDescription: 'Install outcome with progress statistics and residual report.',
    renderText: (_args, value) => textResult(value),
    execute: async () => {
      // Stage 6 wires consent gate / argv / run / residual scan here.
      return {
        ok: false,
        error: {
          reason: 'cli_error',
          message: 'ocrx_model_install execute pipeline not wired in this build',
          guidance: 'This placeholder is replaced by plan stage 6.',
        },
        text: 'ocrx_model_install is not wired in this build (plan stage 6 pending).',
      };
    },
  });
}
