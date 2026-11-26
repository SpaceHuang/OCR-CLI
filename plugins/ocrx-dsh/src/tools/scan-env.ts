// Shared tool environment (ToolEnv): config + plugin context, imported by
// every tool module and the exec face.
import type { OcrxConfig } from '../config.js';
import type { PluginContext } from '../types.js';

export interface ToolEnv {
  config: OcrxConfig;
  ctx: PluginContext;
}
