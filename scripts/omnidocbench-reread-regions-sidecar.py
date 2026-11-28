"""Batch PP-DocLayoutV2 pre-detection over the B0b table-corpus PDFs, writing
per-stem regions JSON (route A files for --layout-regions).

M4.3 P2a: this is now a THIN CLI over sidecar/table-engine/layout_regions.py —
the frozen preprocessing/detection lives there (single source of truth; the
worker's `regions` frame mode consumes the same implementation, so benchmark
sidecars and product-generated regions are byte-equivalent by construction).

Launch:
    python -I scripts/omnidocbench-reread-regions-sidecar.py \
        --corpus <corpus.json> --pdfs <pdf dir> --output-dir <out> \
        --model-dir <pp-doclayout-v2 model dir>
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "sidecar" / "table-engine"))

from layout_regions import main  # noqa: E402

if __name__ == "__main__":
    raise SystemExit(main())
