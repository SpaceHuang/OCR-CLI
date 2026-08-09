#!/usr/bin/env python3
"""Generate deterministic, dependency-free PDF fixtures for macOS renderer tests."""

from pathlib import Path
import sys


def make_pdf(page_count: int, varied_sizes: bool = False, complex_page: bool = False) -> bytes:
    objects: list[bytes] = []

    def add(value: str) -> int:
        objects.append(value.encode("ascii"))
        return len(objects)

    catalog = add("<< /Type /Catalog /Pages 2 0 R >>")
    pages_id = add("")
    font_id = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    page_ids: list[int] = []
    for number in range(1, page_count + 1):
        text = f"BT /F1 18 Tf 72 700 Td (ocrx fixture page {number}) Tj ET"
        if complex_page:
            lines = [f"BT /F1 10 Tf 48 {y} Td (complex fixture line {number}-{y}) Tj ET" for y in range(700, 100, -24)]
            text = "\n".join([text, *lines])
        stream = text.encode("ascii")
        content_id = add(f"<< /Length {len(stream)} >>\nstream\n{text}\nendstream")
        width, height = (336, 432)
        if varied_sizes:
            width, height = ((612, 792) if number % 2 else (1000, 500))
        page_ids.append(add(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"))

    kids = " ".join(f"{page_id} 0 R" for page_id in page_ids)
    objects[pages_id - 1] = f"<< /Type /Pages /Kids [{kids}] /Count {page_count} >>".encode("ascii")

    output = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for index, value in enumerate(objects, start=1):
        offsets.append(len(output))
        output.extend(f"{index} 0 obj\n".encode("ascii"))
        output.extend(value)
        output.extend(b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode("ascii"))
    for offset in offsets[1:]:
        output.extend(f"{offset:010d} 00000 n \n".encode("ascii"))
    output.extend(f"trailer\n<< /Size {len(objects) + 1} /Root {catalog} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode("ascii"))
    return bytes(output)


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "benchmarks/fixtures/pdf")
    root.mkdir(parents=True, exist_ok=True)
    (root / "normal-1-page.pdf").write_bytes(make_pdf(1))
    (root / "normal-100-pages.pdf").write_bytes(make_pdf(100))
    (root / "varied-page-sizes.pdf").write_bytes(make_pdf(4, varied_sizes=True))
    (root / "complex-20-pages.pdf").write_bytes(make_pdf(20, varied_sizes=True, complex_page=True))
    normal = (root / "normal-1-page.pdf").read_bytes()
    (root / "corrupt-truncated.pdf").write_bytes(normal[: len(normal) // 2])
    (root / "not-a-pdf.txt").write_text("this is intentionally not a PDF\n", encoding="ascii")
    print(f"generated fixtures in {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
