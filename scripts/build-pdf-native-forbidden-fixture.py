#!/usr/bin/env python3
"""Build a deterministic 20-page image-only PDF and its native-negative manifest."""
import hashlib
import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw
from reportlab.lib.utils import ImageReader
from reportlab.pdfgen import canvas

def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "assets/regression/pdf-native-forbidden-v0.1")
    root.mkdir(parents=True, exist_ok=True)
    inputs = root / "inputs"; inputs.mkdir(exist_ok=True)
    pdf = inputs / "image-only-20-pages.pdf"
    png = root / ".fixture-page.png"
    image = Image.new("RGB", (612, 792), "white")
    draw = ImageDraw.Draw(image)
    draw.rectangle((72, 100, 540, 690), outline=(30, 80, 150), width=6)
    draw.ellipse((160, 220, 450, 510), fill=(80, 150, 220), outline=(10, 40, 90), width=5)
    draw.text((160, 620), "OCRX IMAGE-ONLY FIXTURE", fill=(10, 40, 90))
    image.save(png)
    writer = canvas.Canvas(str(pdf), pagesize=(612, 792), pageCompression=1)
    for page in range(1, 21):
        writer.drawImage(ImageReader(str(png)), 0, 0, width=612, height=792, mask="auto")
        writer.showPage()
    writer.save()
    png.unlink()
    digest = hashlib.sha256(pdf.read_bytes()).hexdigest()
    pages = [{"candidate_page": i, "source_page": i, "expected_page_type": "image", "native_allowed": False, "truth_version": "pdf-native-negative-v1", "expected_native_text_chars": 0, "expected_native_blocks": 0} for i in range(1, 21)]
    manifest = {"schema_version": 1, "manifest_role": "pdf_native_quality_set", "quality_set_type": "native_forbidden", "set_id": "pdf-native-forbidden-v0.1", "status": "ready_for_route_gate", "page_count": 20, "source_pdf": str(pdf.resolve()), "input_sha256": digest, "truth_version": "pdf-native-negative-v1", "benchmark_version": "pdf-native-benchmark-v1", "classifier_revision": "pdf-route-v1", "extractor_revision": "pdf-text-v1", "native_allowed": False, "pages": pages, "selection": {"selection_basis": "deterministic image-only fixture with no PDF text objects"}, "license": {"redistributable": True, "provenance": "Generated deterministic local test fixture."}}
    (root / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    (root / "review").mkdir(exist_ok=True)
    (root / "review" / "README.md").write_text("# PDF native forbidden set\n\n20 deterministic image-only pages. Native extraction is forbidden; route must be image/OCR or explicit failure.\n")
    print(json.dumps({"set_id": manifest["set_id"], "pages": 20, "input_sha256": digest}, sort_keys=True))

if __name__ == "__main__":
    main()
