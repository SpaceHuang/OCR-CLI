// argv/model.ts (requirement §7.5.2; plan D-4): ocrx_model / recommend argv。
// --progress/--model-profile 禁用；--report 仅 recommend（且仅 json，D-4）；
// list/info/verify 一律文本透传。
export type ModelAction = 'list' | 'info' | 'recommend' | 'verify';

export interface ModelArgs {
  action: ModelAction;
  model?: 'ovisocr2' | 'ovisocr3';
  profile?: 'compact' | 'balanced' | 'quality' | 'reference';
  reportJson?: boolean;
}

export function buildModelArgv(args: ModelArgs): string[] {
  const argv: string[] = ['model', args.action];
  if (args.model !== undefined) argv.push('--model', args.model);
  if (args.action === 'verify' && args.profile !== undefined) argv.push(args.profile);
  if (args.action === 'recommend') {
    // BLOCK-7: a positional profile on the recommend PLAN face pins the
    // planned target (that manifest's exact bytes), beyond the ladder pick.
    if (args.reportJson === true && args.profile !== undefined) argv.push(args.profile);
    if (args.reportJson === true) argv.push('--report', 'json');
  }
  // list/info/recommend/verify 全部无条件带 --non-interactive。
  argv.push('--non-interactive');
  return argv;
}

/** recommend 专用（fetchRecommend / D-11 重试用）。 */
export function buildRecommendArgv(options: {
  model?: string;
  profile?: string;
  reportJson: boolean;
}): string[] {
  return buildModelArgv({
    action: 'recommend',
    model: options.model as ModelArgs['model'],
    profile: options.profile as ModelArgs['profile'],
    reportJson: options.reportJson,
  });
}
