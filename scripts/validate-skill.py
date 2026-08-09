#!/usr/bin/env python3
"""Validate the source-of-truth OCR-CLI Skill contract."""

from pathlib import Path
import re
import sys


REQUIRED = (
    "ocrx doctor --report json",
    "absolute paths",
    "--model ovisocr2",
    "--non-interactive",
    "--resume",
    "human verification",
)


def validate(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    errors = []
    if not text.startswith("---\n") or "name: ocrx" not in text:
        errors.append("missing stable front matter")
    for item in REQUIRED:
        if item not in text:
            errors.append(f"missing required instruction: {item}")
    if re.search(r"https?://|\.gguf\b|prompt\s*[:=]|token\s*[:=]", text, re.I):
        errors.append("skill must not embed URLs, model filenames, prompts, or tokens")
    for line in text.splitlines():
        if "](" in line and not re.search(r"\]\([^)]*\)", line):
            errors.append("malformed markdown link")
    return errors


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("assets/skills/ocrx/SKILL.md")
    errors = validate(path)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"valid skill: {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
