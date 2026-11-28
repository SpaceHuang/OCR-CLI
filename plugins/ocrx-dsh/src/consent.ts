// consent.ts (requirement §7.5.3/§8.2.4): ctx.get('approval') → 三分支。
// 服务缺席/request 抛错/前置不满足（无 agent）→ unavailable；用户拒绝或
// never 策略 → rejected；用户取消 → cancelled。任何分支都不降级为静默下载。
import type { PluginContext } from './types.js';

export type ConsentOutcome = 'approved' | 'rejected' | 'unavailable' | 'cancelled';

export interface ConsentRequest {
  title: string;
  body: string;
  toolName: string;
  agent?: unknown;
  callId?: string;
  signal?: AbortSignal;
}

export interface ConsentResult {
  outcome: ConsentOutcome;
  /** Detail for the structured result (e.g. why the service was absent). */
  detail: string;
}

const APPROVED_VALUE = 'allowed-once';

export async function requestConsent(ctx: PluginContext, request: ConsentRequest): Promise<ConsentResult> {
  // 前置不满足：审批 request 要求会话处于打开 turn 内（有 agent 上下文）。
  if (request.agent === undefined) {
    return {
      outcome: 'unavailable',
      detail: 'No conversation agent context is available, so the consent prompt cannot be shown.',
    };
  }
  const approval = ctx.get('approval');
  if (
    !approval ||
    typeof (approval as { request?: unknown }).request !== 'function'
  ) {
    return {
      outcome: 'unavailable',
      detail: 'The approval service is not available in this deployment.',
    };
  }
  try {
    const outcome = await (approval as {
      request(input: Record<string, unknown>): Promise<unknown>;
    }).request({
      agent: request.agent,
      toolName: request.toolName,
      callId: request.callId,
      reason: request.title,
      displayReason: request.body,
      signal: request.signal,
    });
    if (outcome === APPROVED_VALUE) return { outcome: 'approved', detail: 'approved by the user' };
    if (outcome === 'rejected') {
      return { outcome: 'rejected', detail: 'the user rejected the download (or the policy is "never")' };
    }
    if (outcome === 'cancelled') {
      return { outcome: 'cancelled', detail: 'the user cancelled the prompt' };
    }
    // 'unavailable' 或未知值：fail closed。
    return {
      outcome: 'unavailable',
      detail: `the approval service returned "${String(outcome)}"`,
    };
  } catch (error) {
    // request 抛错（含 turn 外调用）→ unavailable，与缺席同路径。
    return {
      outcome: 'unavailable',
      detail: `the approval request failed: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
}
