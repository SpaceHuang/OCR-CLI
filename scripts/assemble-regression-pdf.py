#!/usr/bin/env python3
"""Assemble a selected multi-source regression set into one private PDF."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("set_dir", type=Path)
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--qpdf", default="qpdf")
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()

    set_dir = args.set_dir.resolve()
    output = (args.output or set_dir / "inputs" / "source.pdf").resolve()
    page_map = json.loads((set_dir / "page-map.json").read_text(encoding="utf-8"))
    pages = page_map.get("pages") or []
    if page_map.get("schema_version") != 1 or not pages:
        print("invalid page-map.json", file=sys.stderr)
        return 2
    if output.exists() and not args.overwrite:
        print(f"output already exists: {output} (pass --overwrite)", file=sys.stderr)
        return 2
    qpdf = shutil.which(args.qpdf)
    if not qpdf:
        print(f"qpdf not found: {args.qpdf}", file=sys.stderr)
        return 2

    command = [qpdf, "--empty", "--pages"]
    for page in pages:
        source = (REPO_ROOT / page["source_pdf"]).resolve()
        source_page = int(page["source_page"])
        if not source.is_file() or source_page < 1:
            print(f"invalid source mapping: {page}", file=sys.stderr)
            return 2
        command.extend([str(source), str(source_page)])
    output.parent.mkdir(parents=True, exist_ok=True)
    command.extend(["--", str(output)])
    subprocess.run(command, check=True)
    print(
        json.dumps(
            {
                "output": str(output),
                "page_count": len(pages),
                "sha256": sha256(output),
            },
            ensure_ascii=False,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
