// /ocrx slash command (requirement §8.3; implementation lands in plan stage 7).
// Commands execute outside turns: NO confirmation channel exists here, so
// this command intentionally offers no model-install action.
import type { ToolEnv } from './tools/scan-env.js';

export interface CommandDefinition {
  name: string;
  description: string;
  handler(invocation: {
    commandId?: string;
    agent?: unknown;
    rawInput?: string;
    signal?: AbortSignal;
    [key: string]: unknown;
  }): Promise<{ kind: 'success' | 'error'; text: string }> | { kind: 'success' | 'error'; text: string };
}

export function createOcrxCommand(env: ToolEnv): CommandDefinition {
  return {
    name: 'ocrx',
    description:
      'OvisOCR (ocrx) status overview; "/ocrx doctor" runs the full self-check. ' +
      'Model installation always happens in a conversation turn via the ocrx_model_install tool.',
    handler: async (invocation) => {
      const input = typeof invocation.rawInput === 'string' ? invocation.rawInput.trim() : '';
      if (input === 'doctor') {
        return {
          kind: 'success',
          text: 'ocrx doctor self-check is not wired in this build (plan stage 7 pending).',
        };
      }
      return {
        kind: 'success',
        text: 'ocrx status overview is not wired in this build (plan stage 7 pending).',
      };
    },
  };
}
