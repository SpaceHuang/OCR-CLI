# Native L2 acceptance checklist

For every page, compare the rendered source page with its raw PDFium facts.

- [ ] Source page and SHA-256 match the manifest.
- [ ] Page is body text, without important image/chart/table content.
- [ ] All visible text is present and in reading order.
- [ ] Unicode and whitespace are correct.
- [ ] Text is split into reliable L2 blocks with roles and boundaries.
- [ ] Every block has a valid normalized bbox and positive unique source order.
- [ ] Header/footer/page number handling is correct.
- [ ] PDFium text coverage and bbox coverage meet the frozen thresholds.
- [ ] No duplicate text geometry or geometry anomaly exists.
- [ ] `validate_native_page(l2, ...)` passes.
- [ ] Human reviewer sets `review_status=accepted` and `allow_native=true` only after all checks pass.

The current draft is expected to fail the block/bbox checks until the extractor
is improved; this checklist is not a claim of acceptance.
