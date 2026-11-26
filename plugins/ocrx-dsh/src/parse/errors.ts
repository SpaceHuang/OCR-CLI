// 错误映射 (requirement §7.6): (代码路径, code, kind) 三元组判定，kind
// 优先于 code；install 路径为 install_failed 例外；表内 code 无失败对象时
// 回落 code 默认 + details_unavailable；表外/spawn 失败 → launcher_failed。
// guidance 由插件生成（按 cause 类别分支，不依赖 CLI suggestion）。
import type { CliFailureObject, FailureReason, NormalizedError } from '../types.js';

export type CliPath = 'scan' | 'install';

const DEVICE_UNAVAILABLE_MESSAGE = 'requested device is unavailable';

/** §7.6 表内 code（scan 路径）。install 路径的 4/5 语义不同，单独处理。 */
const KNOWN_SCAN_CODES = new Set([2, 3, 4, 5, 6, 9, 10, 130]);

function scanTableEntry(failure: CliFailureObject): NormalizedError {
  const { code, kind } = failure;
  const cause = failure.cause ?? '';
  const message = failure.message ?? '';
  const base = {
    message: message || kind,
    code,
    kind,
    cause: failure.cause,
    suggestion: failure.suggestion,
  };
  // kind 已知 → 按表映射
  if (code === 2) return { ...base, reason: 'usage', guidance: 'Fix the tool arguments: ' + usageGuidance(failure) };
  if (code === 3 && kind === 'input') {
    return { ...base, reason: 'input', guidance: 'Verify the input paths are absolute and the files exist. Directories are scanned as batches.' };
  }
  if (code === 3 && kind === 'pdf') {
    return {
      ...base,
      reason: 'pdfium_missing',
      guidance:
        'The PDFium library is missing or damaged. Reinstall the plugin (restores the bundled PDFium), or point the environment at a valid library via the extraEnv config key OCRX_PDFIUM_LIBRARY.',
    };
  }
  if (code === 3 && kind === 'pdf_native') {
    return {
      ...base,
      reason: 'pdf_native',
      guidance: 'This PDF has an unreliable text layer, so text-mode extraction refused it. Re-run with pdfMode "auto" or "ocr".',
    };
  }
  if (code === 4 && kind === 'model' && cause === 'offline') {
    return { ...base, reason: 'model_missing', guidance: modelMissingGuidance() };
  }
  if (code === 4 && kind === 'model') {
    return { ...base, reason: 'model', guidance: modelCauseGuidance(cause) };
  }
  if (code === 5 && kind === 'output') {
    return { ...base, reason: 'output', guidance: outputGuidance(failure) };
  }
  if (code === 6 && kind === 'runtime' && message.toLowerCase().includes(DEVICE_UNAVAILABLE_MESSAGE)) {
    return {
      ...base,
      reason: 'device_unavailable',
      guidance:
        'The requested device does not exist in this build: the bundled Windows/macOS CPU package has no cuda/vulkan support. Remove the device argument, or point the plugin config command at your own GPU (CUDA) build of ocrx.',
    };
  }
  if (code === 9 && kind === 'output') {
    return { ...base, reason: 'output', guidance: outputGuidance(failure) };
  }
  if (code === 10) {
    return { ...base, reason: 'partial', guidance: 'Some pages failed or were degraded; see the per-document details. Re-run with resume: true to retry only the failed pages.' };
  }
  if (code === 130) {
    return { ...base, reason: 'interrupted', guidance: 'The scan was interrupted from the console. Re-run with resume: true to continue from the page checkpoints.' };
  }
  // 表内 code 但 kind 未列出 → cli_error（透传原始字段）
  if (KNOWN_SCAN_CODES.has(code)) {
    return {
      ...base,
      reason: 'cli_error',
      guidance: `The CLI reported an unexpected failure shape (code ${code}, kind ${kind ?? 'unknown'}). Raw fields are preserved above.`,
    };
  }
  // 表外 → launcher_failed（不应该到达：spawn 层已识别；兜底）
  return { ...base, reason: 'launcher_failed', guidance: launcherFailedGuidanceText() };
}

export function usageGuidance(failure: CliFailureObject): string {
  return [
    failure.cause ? `cause: ${failure.cause}.` : null,
    'Check enums (model, modelProfile, pdfMode, qualityTier, device), page expressions, and that resume/overwrite are not both set.',
  ]
    .filter(Boolean)
    .join(' ');
}

export function modelMissingGuidance(): string {
  return (
    'The OCR model is not installed. Bootstrap flow: confirm the target with the user, run ' +
    'ocrx_model_install (it asks for consent with the real size and destination), then retry ' +
    'ocrx_scan with the SAME arguments plus resume: true (drop overwrite if it was set — they ' +
    'are mutually exclusive). The install tool is the only path that downloads anything.'
  );
}

export function modelCauseGuidance(cause: string): string {
  if (cause.startsWith('unsupported_profile')) {
    return 'This model/profile combination is not available. Check the model/modelProfile values against ocrx_model action "info".';
  }
  if (cause.startsWith('runtime_initialization')) {
    return 'The OCR runtime failed to initialize. Retry once; if it persists switch modelProfile to "compact" or change device.';
  }
  return `The model failed outside the missing-assets path (cause: ${cause || 'unknown'}). Check ocrx_model action "info" for the installed profiles, then retry.`;
}

export function outputGuidance(failure: CliFailureObject): string {
  const causes: string[] = [];
  if (failure.cause) causes.push(`CLI detail: ${failure.cause}`);
  return (
    'Writing the scan output failed. Common causes: the output directory is inside the working ' +
    'directory or an input directory, overlaps an input, contains "..", or already holds a ' +
    'scan-summary.json (use resume: true or overwrite: true); otherwise check permissions and ' +
    'free disk space.' +
    (causes.length > 0 ? ` ${causes.join(' ')}` : '')
  );
}

export function launcherFailedGuidanceText(): string {
  return (
    'The ocrx executable was found but failed to start (missing runtime library or corrupted ' +
    'binary). Install the Microsoft Visual C++ Redistributable (Windows) and reinstall the ' +
    'plugin; if a custom command is configured, check that it points at a working ocrx build.'
  );
}

/** 判定规则第 4 条：表内 code 但完全没有失败对象（仅一行文本）。 */
export function reasonForCodeWithoutObject(code: number, stderrSample: string): NormalizedError {
  const defaults: Record<number, { reason: FailureReason; guidance: string }> = {
    2: { reason: 'usage', guidance: 'Fix the tool arguments (enums, page expressions, resume/overwrite exclusivity).' },
    3: { reason: 'input', guidance: 'Verify the input paths are absolute and the files exist.' },
    4: { reason: 'model', guidance: modelCauseGuidance('') },
    5: { reason: 'output', guidance: outputGuidance({} as CliFailureObject) },
    6: { reason: 'runtime', guidance: 'The runtime refused to start (memory/device constraints). Try modelProfile "compact" or a different device.' },
    9: { reason: 'output', guidance: outputGuidance({} as CliFailureObject) },
    10: { reason: 'partial', guidance: 'Some pages failed or were degraded; re-run with resume: true.' },
    130: { reason: 'interrupted', guidance: 'The scan was interrupted from the console; re-run with resume: true.' },
  };
  const fallback = defaults[code] ?? {
    reason: 'launcher_failed' as FailureReason,
    guidance: launcherFailedGuidanceText(),
  };
  return {
    reason: fallback.reason,
    message: `CLI exited with code ${code} without a structured failure object`,
    guidance: fallback.guidance,
    code,
    details_unavailable: true,
    cause: stderrSample.trim() || null,
  };
}

/** 表外 code / spawn 层失败。 */
export function launcherFailedError(detail: string, reason: FailureReason = 'launcher_failed'): NormalizedError {
  return {
    reason,
    message: detail,
    guidance: launcherFailedGuidanceText(),
  };
}

/** 主入口：scan 路径的 (code, kind) 判定。 */
export function mapScanFailure(failure: CliFailureObject): NormalizedError {
  return scanTableEntry(failure);
}
