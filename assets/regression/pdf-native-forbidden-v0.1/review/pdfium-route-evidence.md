# PDFium route evidence

The 20 pages were probed individually with the rebuilt `pdfium-check` binary.

| signal | pages |
|---|---:|
| native text characters > 0 | 0/20 |
| native blocks > 0 | 0/20 |
| image objects > 0 | 20/20 |

This is evidence for the image-only negative fixture. It does not by itself
replace the CLI route confusion report; the release gate must still assert
that none of these pages has `actual_method=native_text`.
