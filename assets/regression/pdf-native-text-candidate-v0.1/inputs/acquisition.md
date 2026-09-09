# Native text candidate source

Source PDF:

`/Users/space/Documents/Develop/OCR-CLI/test-data/books/商业金融/《商业的本质》杰克•韦尔奇【文字版_PDF电子书_下载】.pdf`

The candidate selects the 30 source pages listed in `manifest.json` (pages 5,
6, 7, 10, 11, 13-16, 18-33, and 35-39). The source file is
local and non-redistributable. Its SHA-256 and page mapping are recorded in
`manifest.json` and each truth page.

The selection was made from a text-heavy body-text range after PDFium native
probe screening. It intentionally excludes cover, scan-heavy, chart-heavy,
and image-heavy pages as far as the automated screening can determine.
