// Skill provider (requirement §10.1; content finalized in plan stage 7).
// The provider serves skills/ocrx/SKILL.md from the installed package —
// nothing is written to user directories.
import { readFileSync } from 'node:fs';
import path from 'node:path';

import type { ToolEnv } from './tools/scan-env.js';

export interface SkillDefinition {
  name: string;
  description: string;
  invocation: { modelInvocable: boolean; userInvocable: boolean };
  provider: string;
  source: string;
  rank: number;
  locator: string;
  content?: string;
}

const SKILL_DIR = path.join(__dirname, '..', 'skills', 'ocrx');

function skillFile(): string {
  return path.join(SKILL_DIR, 'SKILL.md');
}

function readSkill(): { name: string; description: string; body: string } {
  const raw = readFileSync(skillFile(), 'utf8');
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(raw);
  if (!match) return { name: 'ocrx', description: 'OvisOCR (ocrx) usage.', body: raw };
  const meta = match[1] ?? '';
  const body = raw.slice(match[0].length);
  const name = /^name:\s*(.+)$/m.exec(meta)?.[1]?.trim() ?? 'ocrx';
  const description = /^description:\s*(.+)$/m.exec(meta)?.[1]?.trim() ?? 'OvisOCR (ocrx) usage.';
  return { name, description, body };
}

export function createSkillProvider(_env: ToolEnv) {
  return (control: { signal?: AbortSignal } | undefined) => ({
    name: 'ocrx',
    async list(): Promise<SkillDefinition[]> {
      if (control?.signal?.aborted) return [];
      const skill = readSkill();
      return [
        {
          name: skill.name,
          description: skill.description,
          invocation: { modelInvocable: true, userInvocable: true },
          provider: 'ocrx',
          source: 'bundled',
          rank: 50,
          locator: skillFile(),
        },
      ];
    },
    async get(candidate: { name?: string }): Promise<SkillDefinition | undefined> {
      const skill = readSkill();
      if (candidate.name && candidate.name !== skill.name) return undefined;
      return {
        name: skill.name,
        description: skill.description,
        invocation: { modelInvocable: true, userInvocable: true },
        provider: 'ocrx',
        source: 'bundled',
        rank: 50,
        locator: skillFile(),
        content: skill.body,
      };
    },
  });
}
