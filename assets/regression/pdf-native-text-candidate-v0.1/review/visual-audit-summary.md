# Visual audit status

30/30 pages were checked page by page with model vision against rendered source pages. The visible body text and reading order are mostly consistent with `native_text`; no page is marked final because human L2 acceptance is still required.

## Data clarifications

- `bbox` uses normalized 0-1000 coordinates (`bbox_coordinate_space`), while page size is recorded in PDF points.
- All pages show a bottom-right grey watermark. It is recorded as visual metadata and excluded from native text body truth pending human confirmation.
- Candidate pages 6, 11, and 29 contain explicit block/spacing findings in `truth/*.json`.
- The complete machine-readable audit is in `review/visual-audit.json`.
