"""Ruling-gate fixture tests — T0 frozen calibration crops (27/27 set)."""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

pytest.importorskip("cv2", reason="opencv required (paddleocr dependency tree)")

from ruling_gate import has_ruling_lines  # noqa: E402

FIXTURES = Path(__file__).resolve().parent / "fixtures"


def test_pseudo_table_without_lines_is_rejected():
    """The T0 EXTRA region (score list, no ruling lines) must fail the gate."""
    png = (FIXTURES / "pseudo_table_no_lines.png").read_bytes()
    assert has_ruling_lines(png) is False


def test_ruled_real_table_passes():
    """A real bordered financial table (T0 p00_r0, conf 0.99) passes."""
    png = (FIXTURES / "ruled_real_table.png").read_bytes()
    assert has_ruling_lines(png) is True


def test_undecodable_bytes_raise_instead_of_passing():
    with pytest.raises(ValueError, match="undecodable"):
        has_ruling_lines(b"not a png")
