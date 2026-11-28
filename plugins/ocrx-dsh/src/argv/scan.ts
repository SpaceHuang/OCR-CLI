// argv/scan.ts (requirement §7.2): 参数→argv 映射。无条件追加
// `--offline --non-interactive --report json --progress off`；`offline`
// 不作为工具参数暴露（构造性封死静默下载，§11.3）。
import { LIMITS } from '../run.js';

export interface ScanArgs {
  input: string[];
  outputDir: string;
  pages?: string;
  model?: 'ovisocr2' | 'ovisocr3';
  modelProfile?: 'auto' | 'compact' | 'balanced' | 'quality' | 'reference';
  pdfMode?: 'auto' | 'text' | 'ocr';
  qualityTier?: 'l1' | 'l2';
  device?: 'auto' | 'cpu' | 'metal' | 'cuda' | 'vulkan';
  resume?: boolean;
  overwrite?: boolean;
  password?: string;
}

export interface ArgvBuild {
  argv: string[];
}

/**
 * Builds the full explicit argv. The four hardwired flags are appended
 * LAST and cannot be suppressed by any argument value — the silent-download
 * path is closed by construction (§11.3), pinned by unit tests.
 */
export function buildScanArgv(args: ScanArgs): ArgvBuild {
  const argv: string[] = ['scan', ...args.input, '--output-dir', args.outputDir];
  if (args.pages !== undefined) argv.push('--pages', args.pages);
  if (args.model !== undefined) argv.push('--model', args.model);
  if (args.modelProfile !== undefined && args.modelProfile !== 'auto') {
    argv.push('--model-profile', args.modelProfile);
  }
  if (args.pdfMode !== undefined) argv.push('--pdf-mode', args.pdfMode);
  if (args.qualityTier !== undefined) argv.push('--quality-tier', args.qualityTier);
  if (args.device !== undefined) argv.push('--device', args.device);
  if (args.resume === true) argv.push('--resume');
  if (args.overwrite === true) argv.push('--overwrite');
  if (args.password !== undefined) argv.push('--password', args.password);
  argv.push('--offline', '--non-interactive', '--report', 'json', '--progress', 'off');
  return { argv };
}

/** 工具层互斥校验（§7.2：resume/overwrite 互斥，不发 argv）。 */
export function scanMutexViolations(args: ScanArgs): string[] {
  const violations: string[] = [];
  if (args.resume === true && args.overwrite === true) {
    violations.push('resume and overwrite are mutually exclusive; pick one.');
  }
  return violations;
}

/**
 * device 预判（§7.2）：cuda/vulkan 在「Windows 随包平台包」来源上由工具层
 * 直接拒绝（静态能力事实，不启动进程）。Config.command 来源放行；PATH/
 * known 来源不做静态预判，交由运行期退出码 6 行映射。
 */
export function deviceGuardViolation(device: string | undefined, binarySource: string | undefined, platform: string): string | null {
  if (device !== 'cuda' && device !== 'vulkan') return null;
  if (platform !== 'win32') return null;
  if (binarySource === 'bundled') {
    return (
      'This device is not available: the bundled Windows package is a CPU build without ' +
      'cuda/vulkan support (static capability, no process started). ' +
      'Remove the device argument, or point the plugin config command at your own GPU (CUDA) build of ocrx.'
    );
  }
  return null;
}

export const SCAN_RUN_LIMITS = LIMITS;
