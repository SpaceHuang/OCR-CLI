# Selection rationale

The selected source pages listed in `manifest.json` were chosen because initial PDFium
probes show native Unicode text, full text coverage, no duplicate text
geometry, no geometry anomaly, and very low image coverage. The range is body
text rather than the previous mixed OCR regression pages.

The selection is deliberately a candidate, not a completed qualified set:
the current extractor emits coarse blocks and low bbox coverage. Those facts
must be resolved before any page is accepted as L2 native text.
