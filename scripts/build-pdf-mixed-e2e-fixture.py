#!/usr/bin/env python3
"""Build a small reproducible native/image/mixed PDF for real CLI routing E2E."""
import hashlib
import json
import subprocess
import sys
from pathlib import Path



def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "assets/regression/pdf-mixed-e2e-v0.1")
    inputs = root / "inputs"
    review = root / "review"
    inputs.mkdir(parents=True, exist_ok=True)
    review.mkdir(parents=True, exist_ok=True)

    source = Path(sys.argv[2])
    image_pdf = Path(sys.argv[3])
    overlay = inputs / "mixed-overlay.pdf"
    _write_text_overlay(overlay)
    pdf = inputs / "native-image-mixed-3-pages.pdf"
    native = inputs / "native-page.pdf"
    image = inputs / "image-page.pdf"
    mixed_base = inputs / "mixed-base.pdf"
    mixed = inputs / "mixed-page.pdf"
    subprocess.run(["qpdf", str(source), "--pages", str(source), "5", "--", str(native)], check=True)
    subprocess.run(["qpdf", str(image_pdf), "--pages", str(image_pdf), "1", "--", str(image)], check=True)
    subprocess.run(["qpdf", str(native), "--pages", str(native), "1", "--", str(mixed_base)], check=True)
    subprocess.run(["qpdf", str(mixed_base), "--overlay", str(native), "--from=1", "--to=1", "--", str(mixed)], check=True)
    subprocess.run(["qpdf", "--empty", "--pages", str(native), str(image), str(mixed), "--", str(pdf)], check=True)

    digest = hashlib.sha256(pdf.read_bytes()).hexdigest()
    manifest = {
        "schema_version": 1,
        "manifest_role": "pdf_content_aware_e2e",
        "set_id": "pdf-mixed-e2e-v0.1",
        "status": "ready_for_cli_e2e",
        "page_count": 3,
        "source_pdf": str(pdf.resolve()),
        "input_sha256": digest,
        "source_material": {
            "native_source": str(source.resolve()),
            "image_source": str(image_pdf.resolve()),
        },
        "pages": [
            {"page": 1, "expected_page_type": "text", "expected_route": "native_text"},
            {"page": 2, "expected_page_type": "image", "expected_route": "ocr"},
            {"page": 3, "expected_page_type": "uncertain", "expected_route": "ocr"},
        ],
    }
    (root / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    (review / "README.md").write_text(
        "# Mixed PDF E2E fixture\n\n"
        "Page 1 is copied from a native-text source, page 2 from the image-only fixture, "
        "and page 3 duplicates the native text layer to create an uncertain route.\n"
    )
    print(json.dumps({"pdf": str(pdf), "sha256": digest, "pages": 3}, ensure_ascii=False))


def _write_text_overlay(path: Path) -> None:
    """Write a dependency-free one-page PDF containing only a small text object."""
    stream = b"BT /F1 12 Tf 72 730 Td (OCRX MIXED TEXT OBJECT) Tj ET\n"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"endstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(out))
        out.extend(f"{index} 0 obj\n".encode())
        out.extend(obj)
        out.extend(b"\nendobj\n")
    xref = len(out)
    out.extend(f"xref\n0 {len(objects) + 1}\n".encode())
    out.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        out.extend(f"{offset:010d} 00000 n \n".encode())
    out.extend(f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    path.write_bytes(out)


if __name__ == "__main__":
    main()
