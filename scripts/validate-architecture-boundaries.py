#!/usr/bin/env python3
"""Architecture boundary validator (plan §8.14, T089).

Reads the workspace Cargo manifests and crate sources with only the Python
standard library and enforces the §3.2 dependency whitelist plus the
forbidden-type/name rules. Returns 0 when the tree is clean; any violation
prints a `VIOLATION:` line and exits 1.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent
SRC = WORKSPACE / "src"

# §3.2 whitelist: for each crate, the set of ocrx-* crates it may depend on.
ALLOWED_DEPS: dict[str, set[str]] = {
    "ocrx-primitives": set(),
    "ocrx-core": {"ocrx-primitives"},
    "ocrx-inference-api": {"ocrx-primitives"},
    "ocrx-pdf": {"ocrx-core"},
    "ocrx-models": {"ocrx-core", "ocrx-primitives", "ocrx-inference-api"},
    "ocrx-runtime": {"ocrx-core", "ocrx-primitives", "ocrx-inference-api"},
    "ocrx-ovisocr2": {"ocrx-primitives", "ocrx-inference-api"},
    "ocrx-output": {
        "ocrx-core",
        "ocrx-primitives",
        "ocrx-inference-api",
        "ocrx-runtime",
    },
    "ocrx-inference": {
        "ocrx-primitives",
        "ocrx-inference-api",
        "ocrx-models",
        "ocrx-runtime",
        "ocrx-ovisocr2",
    },
    "ocrx-scan": {
        "ocrx-core",
        "ocrx-primitives",
        "ocrx-inference-api",
        "ocrx-output",
        "ocrx-pdf",
    },
    "ocrx-eval": {
        "ocrx-core",
        "ocrx-primitives",
        "ocrx-inference-api",
        "ocrx-pdf",
        "ocrx-output",
        "ocrx-scan",
    },
    "ocrx-skill": set(),
    "ocrx": {
        "ocrx-core",
        "ocrx-inference",
        "ocrx-inference-api",
        "ocrx-scan",
        "ocrx-output",
        "ocrx-skill",
    },
}

# Dev-dependency whitelist (test-only, env-gated): production deps stay
# strict per ALLOWED_DEPS; these pairs are test fixtures only.
ALLOWED_DEV_DEPS: dict[str, set[str]] = {
    "ocrx-runtime": {"ocrx-models", "ocrx-pdf"},
}

# Forbidden identifiers per crate source (§3.2 + RBLK-3).
FORBIDDEN_SOURCES: dict[str, list[str]] = {
    # The scan app must reference StepProfile only through the public API.
    "ocrx-scan": ["ocrx_runtime::StepProfile"],
    # The inference platform must not carry PDF/scan/output/eval types.
    "ocrx-inference": [
        "ocrx_core::Page",
        "ocrx_core::Pdf",
        "ocrx_pdf::",
        "ocrx_output::",
        "ocrx_scan::",
        "ocrx_eval::",
    ],
    "ocrx-inference-api": [
        "ocrx_pdf::",
        "ocrx_output::",
        "ocrx_scan::",
        "ocrx_eval::",
        "pub page_number",
        "pub output_root",
    ],
    "ocrx-pdf": ["ocrx_inference", "ocrx_output", "ocrx_scan", "ocrx_eval"],
    "ocrx-output": ["ocrx_scan::", "ocrx_eval::"],
    "ocrx-models": ["ocrx_runtime::", "ocrx_pdf::", "ocrx_inference::"],
}

DEP_RE = re.compile(r"^(?:([\w-]+)\s*=\s*(?:\{[^}]*?path\s*=\s*\"|\")\.\./([\w-]+))", re.M)
# `path = "../ocrx-xxx"` inside a dependency block of Cargo.toml
PATH_RE = re.compile(r"path\s*=\s*\"\.\./([\w-]+)\"")
CRATE_NAME_RE = re.compile(r"^name\s*=\s*\"([\w-]+)\"", re.M)
SECTION_RE = re.compile(r"^\[(.+?)\]", re.M)


def crate_manifest_deps(manifest_path: Path) -> tuple[str, list[str], list[str]]:
    """Returns (crate_name, [prod ocrx deps], [dev ocrx deps])."""
    text = manifest_path.read_text(encoding="utf-8")
    name_match = CRATE_NAME_RE.search(text)
    if not name_match:
        raise SystemExit(f"VIOLATION: no crate name in {manifest_path}")
    name = name_match.group(1)
    deps: list[str] = []
    dev_deps: list[str] = []
    sections: list[tuple[str, int]] = [
        (m.group(1), m.start()) for m in SECTION_RE.finditer(text)
    ]
    sections.append(("<end>", len(text)))

    for (section, start), (_, next_start) in zip(sections, sections[1:]):
        is_dev = section.startswith("dev-dependencies")
        if not section.startswith("dependencies") and not is_dev:
            continue
        for m in PATH_RE.finditer(text[start:next_start]):
            dep = m.group(1)
            if dep.startswith("ocrx"):
                (dev_deps if is_dev else deps).append(dep)
    return name, deps, dev_deps


def main() -> int:
    violations: list[str] = []
    manifests = sorted(SRC.glob("*/Cargo.toml"))
    if not manifests:
        print("VIOLATION: no crate manifests found under src/")
        return 1

    for manifest in manifests:
        name, deps, dev_deps = crate_manifest_deps(manifest)
        allowed = ALLOWED_DEPS.get(name)
        if allowed is None:
            violations.append(f"unknown crate {name} (add it to the whitelist)")
            continue
        for dep in deps:
            if dep not in allowed:
                violations.append(
                    f"{name}: forbidden dependency on {dep} (whitelist: {sorted(allowed)})"
                )
        allowed_dev = ALLOWED_DEV_DEPS.get(name, set())
        for dep in dev_deps:
            if dep not in allowed_dev:
                violations.append(
                    f"{name}: forbidden dev-dependency on {dep} (whitelist: {sorted(allowed_dev)})"
                )

    # Source-level forbidden identifiers.
    for crate, forbidden in FORBIDDEN_SOURCES.items():
        crate_dir = SRC / crate / "src"
        if not crate_dir.is_dir():
            continue
        for rs_file in crate_dir.rglob("*.rs"):
            text = rs_file.read_text(encoding="utf-8", errors="replace")
            for needle in forbidden:
                if needle in text:
                    rel = rs_file.relative_to(WORKSPACE)
                    violations.append(
                        f"{crate}: forbidden identifier {needle!r} in {rel}"
                    )

    if violations:
        for violation in sorted(set(violations)):
            print(f"VIOLATION: {violation}")
        print(f"{len(violations)} violation(s)")
        return 1
    print("architecture boundaries OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
