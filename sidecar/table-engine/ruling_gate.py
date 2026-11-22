"""Ruling-line pre-check gate (M4.2 D-62; thresholds frozen from the T0
27-crop calibration: 27/27 correct — the pseudo table has zero ruling lines,
every real table that produces output has >= 2 long lines).

cv2 is provided by the paddleocr dependency tree; this module must not
import paddle (the gate runs before the pipeline loads anything).
"""

from __future__ import annotations

import cv2
import numpy as np

# Frozen constants (PROTOCOL.md): the horizontal kernel is 45% of the crop
# width, the vertical kernel 45% of the crop height; a region is "ruled"
# when it has >= 2 long horizontal OR >= 2 long vertical lines.
H_KERNEL_RATIO = 0.45
V_KERNEL_RATIO = 0.45
MIN_LINES = 2
HOUGH_THRESHOLD = 40
HOUGH_GAP = 3
ADAPTIVE_BLOCK = 15
ADAPTIVE_C = -2


def has_ruling_lines(png_bytes: bytes) -> bool:
    """True when the crop looks like a ruled table (>= MIN_LINES long lines)."""
    buf = np.frombuffer(png_bytes, dtype=np.uint8)
    gray = cv2.imdecode(buf, cv2.IMREAD_GRAYSCALE)
    if gray is None:
        # Undecodable bytes are not a ruling-gate verdict — let the caller
        # treat them as a pipeline error instead of silently passing.
        raise ValueError("ruling gate: undecodable png")
    height, width = gray.shape
    binary = cv2.adaptiveThreshold(
        255 - gray,
        255,
        cv2.ADAPTIVE_THRESH_MEAN_C,
        cv2.THRESH_BINARY,
        ADAPTIVE_BLOCK,
        ADAPTIVE_C,
    )
    h_kernel = max(10, int(width * H_KERNEL_RATIO))
    v_kernel = max(10, int(height * V_KERNEL_RATIO))
    horizontal = cv2.morphologyEx(
        binary, cv2.MORPH_OPEN, cv2.getStructuringElement(cv2.MORPH_RECT, (h_kernel, 1))
    )
    vertical = cv2.morphologyEx(
        binary, cv2.MORPH_OPEN, cv2.getStructuringElement(cv2.MORPH_RECT, (1, v_kernel))
    )
    h_lines = cv2.HoughLinesP(
        horizontal, 1, np.pi / 180, HOUGH_THRESHOLD, minLineLength=h_kernel, maxLineGap=HOUGH_GAP
    )
    v_lines = cv2.HoughLinesP(
        vertical, 1, np.pi / 180, HOUGH_THRESHOLD, minLineLength=v_kernel, maxLineGap=HOUGH_GAP
    )
    n_h = 0 if h_lines is None else len(h_lines)
    n_v = 0 if v_lines is None else len(v_lines)
    return n_h >= MIN_LINES or n_v >= MIN_LINES
