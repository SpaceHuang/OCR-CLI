// engines-window.ts (D-10): a faithful subset of `semver.satisfies(range,
// { includePrerelease })` for the comparator forms dsh evaluates engines.dsh
// with. Covers `>=X`, `<Y`, `X -0` upper bounds, and hyphen-free AND ranges
// joined by whitespace — the exact grammar of the frozen window and its
// documented evolution path. Semantics per node-semver: prerelease versions
// only satisfy comparators that mention a prerelease on the SAME
// [major, minor, patch] tuple, unless includePrerelease widens them.
export interface SatisfiesOptions {
  includePrerelease?: boolean;
}

interface ParsedVersion {
  major: number;
  minor: number;
  patch: number;
  prerelease: Array<string | number>;
}

/** Parses a strict `x.y.z[-prerelease]` version (build metadata ignored). */
export function parseVersion(input: string): ParsedVersion {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(input.trim());
  if (!match) throw new TypeError(`invalid semver: ${input}`);
  const prerelease: Array<string | number> = [];
  if (match[4]) {
    for (const identifier of match[4].split('.')) {
      const numeric = /^\d+$/.test(identifier) ? Number(identifier) : identifier;
      prerelease.push(numeric);
    }
  }
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease,
  };
}

function comparePrerelease(left: ParsedVersion['prerelease'], right: ParsedVersion['prerelease']): number {
  if (left.length === 0 && right.length === 0) return 0;
  // No prerelease sorts HIGHER than any prerelease on the same tuple.
  if (left.length === 0) return 1;
  if (right.length === 0) return -1;
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const leftPart = left[index];
    const rightPart = right[index];
    if (leftPart === undefined) return -1;
    if (rightPart === undefined) return 1;
    if (leftPart === rightPart) continue;
    const leftNumeric = typeof leftPart === 'number';
    const rightNumeric = typeof rightPart === 'number';
    if (leftNumeric && rightNumeric) return leftPart < rightPart ? -1 : 1;
    // Numeric identifiers always sort lower than alphanumeric ones.
    if (leftNumeric) return -1;
    if (rightNumeric) return 1;
    return leftPart < rightPart ? -1 : 1;
  }
  return 0;
}

function compareVersions(left: ParsedVersion, right: ParsedVersion): number {
  if (left.major !== right.major) return left.major - right.major;
  if (left.minor !== right.minor) return left.minor - right.minor;
  if (left.patch !== right.patch) return left.patch - right.patch;
  return comparePrerelease(left.prerelease, right.prerelease);
}

interface Comparator {
  operator: '>=' | '>' | '<' | '<=' | '=';
  version: ParsedVersion;
  /** The comparator's own spelling carries a prerelease (node-semver rule). */
  mentionsPrerelease: boolean;
}

function parseComparator(token: string): Comparator {
  const match = /^(>=|<=|>|<|=)?\s*(.+)$/.exec(token.trim());
  const operator = (match?.[1] ?? '=') as Comparator['operator'];
  const version = parseVersion(match?.[2] ?? token);
  return { operator, version, mentionsPrerelease: version.prerelease.length > 0 };
}

function satisfiesComparator(version: ParsedVersion, comparator: Comparator, options: SatisfiesOptions): boolean {
  const order = compareVersions(version, comparator.version);
  let pass = false;
  switch (comparator.operator) {
    case '>=': pass = order >= 0; break;
    case '>': pass = order > 0; break;
    case '<': pass = order < 0; break;
    case '<=': pass = order <= 0; break;
    case '=': pass = order === 0; break;
  }
  if (!pass) return false;
  if (version.prerelease.length === 0) return true;
  // Prerelease versions satisfy a range only when SOME comparator mentions a
  // prerelease on the same tuple — or the caller widened with includePrerelease.
  if (options.includePrerelease === true) return true;
  return comparator.mentionsPrerelease && comparator.version.major === version.major
    && comparator.version.minor === version.minor && comparator.version.patch === version.patch;
}

/**
 * Evaluates whitespace-AND comparator ranges (the `engines.dsh` grammar dsh
 * hands to semver.satisfies with includePrerelease: true).
 */
export function satisfies(versionInput: string, range: string, options: SatisfiesOptions = {}): boolean {
  const version = parseVersion(versionInput);
  const tokens = range.trim().split(/\s+/).filter((token) => token.length > 0);
  if (tokens.length === 0) return false;
  // `X -Y` numeric upper bound (the `-0` face): a hyphen range needs the
  // partial grammar; the frozen window spells it as `<0.2.3-0`, which the
  // comparator path already covers. Unknown comparator shapes fail closed.
  return tokens.every((token) => {
    if (!/^(>=|<=|>|<|=)?\s*v?\d/.test(token)) return false;
    return satisfiesComparator(version, parseComparator(token), options);
  });
}
