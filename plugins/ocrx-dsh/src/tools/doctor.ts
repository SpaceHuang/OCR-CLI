// ocrx_doctor tool (requirement §7.5.1; implementation lands in plan stage 5).
import { defineTool, textResult, type ToolDefinition } from '../toolkit.js';
import type { ToolEnv } from './scan-env.js';

export function createDoctorTool(env: ToolEnv): ToolDefinition {
  return defineTool({
    name: 'ocrx_doctor',
    description:
      'Report the local ocrx installation status: hardware snapshot, selected model, cache ' +
      'location, installed profiles, and plugin/binary versions. STATUS DISPLAY ONLY: the ' +
      'installed_profiles field covers exactly one resolved model and does not see custom ' +
      'cache overrides — never use it to pre-gate ocrx_scan.',
    parameters: {},
    required: [],
    isConcurrencySafe: () => true,
    outputDescription: 'Doctor JSON report plus plugin-view fields.',
    renderText: (_args, value) => textResult(value),
    execute: async () => {
      // Stage 5 wires argv/run/parse here.
      return {
        ok: false,
        error: {
          reason: 'cli_error',
          message: 'ocrx_doctor execute pipeline not wired in this build',
          guidance: 'This placeholder is replaced by plan stage 5.',
        },
        text: 'ocrx_doctor is not wired in this build (plan stage 5 pending).',
      };
    },
  });
}
