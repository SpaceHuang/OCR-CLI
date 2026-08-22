# P0 regression set contract

The release gate requires a versioned, permitted regression set with at least
30 pages and human truth. This repository intentionally does not fabricate
truth labels or model outputs. A complete set must provide:

- `manifest.json` with schema version, input SHA-256, page count, renderer DPI,
  pixel budget, prompt/adapter/runtime/model revisions and license status;
- `truth/page-NNNN.json` for every page, using `PageDocument` schema version 1;
- coverage for normal OCR: plain text, formula, table, visual region, mixed
  layout and blank pages; truncated and failure behavior belongs to separate
  synthetic negative fixtures, not to the accuracy-scored corpus;
- explicit distribution permission and provenance.

The scorer must reject missing pages, missing truth fields, fewer than 30 pages,
or fewer than two pages in each required normal category.
