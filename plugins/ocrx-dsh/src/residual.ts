// residual.ts (requirement §7.5.3): 安装计划 target_dir（revision 目录）与
// profile 目录两 scope 扫描 *.install-*.part-* 等暂存残留，合计体积；
// 结果如实报告 + 「下次安装开始时自动清理」提示。
import { existsSync, readdirSync, statSync } from 'node:fs';

export interface ResidualLocation {
  path: string;
  bytes: number;
}

export interface ResidualReport {
  locations: ResidualLocation[];
  total_bytes: number;
}

function isPartFile(name: string): boolean {
  return name.includes('.part-') || name.endsWith('.part');
}

/** Scans the given directories (best-effort; missing dirs are fine). */
export function scanResidual(dirs: Array<string | undefined | null>): ResidualReport {
  const locations: ResidualLocation[] = [];
  for (const dir of dirs) {
    if (!dir || !existsSync(dir)) continue;
    let entries;
    try {
      entries = readdirSync(dir);
    } catch {
      continue;
    }
    for (const name of entries) {
      if (!isPartFile(name)) continue;
      const separator = dir.endsWith('/') || dir.endsWith('\\') ? '' : '/';
      const full = `${dir}${separator}${name}`;
      try {
        locations.push({ path: full, bytes: statSync(full).size });
      } catch {
        // vanished mid-scan — skip
      }
    }
  }
  return {
    locations,
    total_bytes: locations.reduce((sum, location) => sum + location.bytes, 0),
  };
}

export function residualNote(report: ResidualReport): string {
  if (report.locations.length === 0) return '';
  return (
    `Leftover staging files occupy ${formatBytes(report.total_bytes)} across ` +
    `${report.locations.length} file(s) (${report.locations.map((location) => location.path).join(', ')}). ` +
    'They are cleaned automatically at the start of the next install; downloads cannot resume, so the next run starts from zero.'
  );
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${bytes} B`;
}
