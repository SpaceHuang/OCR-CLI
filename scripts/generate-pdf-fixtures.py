#!/usr/bin/env python3
"""Generate deterministic, dependency-free PDF fixtures for renderer/error tests.

The encrypted fixture implements the PDF 1.4 Standard Security Handler,
revision 2 (40-bit RC4).  It deliberately uses an old, broadly readable PDF
encryption mode because this is a test input, not a recommendation for
protecting documents.
"""

import hashlib
from pathlib import Path
import struct
import sys


PASSWORD = "ocrx-test"
PASSWORD_PADDING = bytes.fromhex(
    "28bf4e5e4e758a4164004e56fffa01082e2e00b6d0683e802f0ca9fe6453697a"
)
FILE_ID = bytes.fromhex("2f6f6372782d6e656761746976652d666978747572652d76312f")


def rc4(key: bytes, value: bytes) -> bytes:
    """Return RC4(key, value), sufficient for the PDF R2 fixture format."""
    state = list(range(256))
    cursor = 0
    for index in range(256):
        cursor = (cursor + state[index] + key[index % len(key)]) & 0xFF
        state[index], state[cursor] = state[cursor], state[index]
    index = cursor = 0
    result = bytearray()
    for byte in value:
        index = (index + 1) & 0xFF
        cursor = (cursor + state[index]) & 0xFF
        state[index], state[cursor] = state[cursor], state[index]
        result.append(byte ^ state[(state[index] + state[cursor]) & 0xFF])
    return bytes(result)


def padded_password(password: str) -> bytes:
    encoded = password.encode("latin-1")[:32]
    return encoded + PASSWORD_PADDING[: 32 - len(encoded)]


def r2_encryption(user_password: str, owner_password: str, permissions: int) -> tuple[bytes, bytes]:
    """Return (owner_entry, file_key) for PDF Standard Security R2."""
    owner_key = hashlib.md5(padded_password(owner_password)).digest()[:5]
    owner_entry = rc4(owner_key, padded_password(user_password))
    digest = hashlib.md5(
        padded_password(user_password)
        + owner_entry
        + struct.pack("<i", permissions)
        + FILE_ID
    ).digest()
    return owner_entry, digest[:5]


def object_key(file_key: bytes, object_number: int) -> bytes:
    digest = hashlib.md5(
        file_key + object_number.to_bytes(3, "little") + b"\x00\x00"
    ).digest()
    return digest[: min(len(file_key) + 5, 16)]


def make_pdf(
    page_count: int,
    varied_sizes: bool = False,
    complex_page: bool = False,
    dense_page: bool = False,
    password: str | None = None,
) -> bytes:
    objects: list[bytes] = []

    def add(value: bytes | str) -> int:
        objects.append(value.encode("ascii") if isinstance(value, str) else value)
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
        if dense_page:
            # A real, readable page with enough words to deterministically exceed
            # a deliberately low --max-output-tokens setting (for example 32).
            words = "OCRX long output fixture verifies explicit truncation handling"
            lines = [
                f"BT /F1 6 Tf 30 {760 - row * 9} Td ({words} {row:03}) Tj ET"
                for row in range(80)
            ]
            text = "\n".join(lines)
        stream = text.encode("ascii")
        content_id = add(b"")
        # PDF stream encryption is object-specific.  Allocate object numbers
        # before deriving the key, then fill the object in-place.
        if password is not None:
            _, file_key = r2_encryption(password, "ocrx-fixture-owner", -4)
            stream = rc4(object_key(file_key, content_id), stream)
        objects[content_id - 1] = (
            f"<< /Length {len(stream)} >>\nstream\n".encode("ascii")
            + stream
            + b"\nendstream"
        )
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
    trailer = f"/Size {len(objects) + 1} /Root {catalog} 0 R"
    if password is not None:
        owner_entry, file_key = r2_encryption(password, "ocrx-fixture-owner", -4)
        user_entry = rc4(file_key, PASSWORD_PADDING)
        encryption_id = len(objects) + 1
        encryption = (
            b"<< /Filter /Standard /V 1 /R 2 /Length 40 /O <"
            + owner_entry.hex().encode("ascii")
            + b"> /U <"
            + user_entry.hex().encode("ascii")
            + b"> /P -4 >>"
        )
        # The encryption dictionary must itself be a numbered indirect object.
        # Add it before serialising the cross-reference table.
        objects.append(encryption)
        # Rebuild so offsets and /Size include this late-added object.
        encrypted_trailer = (
            f"/Size {len(objects) + 1} /Root {catalog} 0 R "
            f"/Encrypt {encryption_id} 0 R "
            f"/ID [<{FILE_ID.hex()}> <{FILE_ID.hex()}>]"
        )
        return serialize_pdf(objects, encrypted_trailer)
    output.extend(f"trailer\n<< {trailer} >>\nstartxref\n{xref}\n%%EOF\n".encode("ascii"))
    return bytes(output)


def serialize_pdf(objects: list[bytes], trailer: str) -> bytes:
    """Serialize object bytes and a conventional PDF cross-reference table."""
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
    output.extend(f"trailer\n<< {trailer} >>\nstartxref\n{xref}\n%%EOF\n".encode("ascii"))
    return bytes(output)


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "benchmarks/fixtures/pdf")
    root.mkdir(parents=True, exist_ok=True)
    (root / "normal-1-page.pdf").write_bytes(make_pdf(1))
    (root / "normal-100-pages.pdf").write_bytes(make_pdf(100))
    (root / "varied-page-sizes.pdf").write_bytes(make_pdf(4, varied_sizes=True))
    (root / "complex-20-pages.pdf").write_bytes(make_pdf(20, varied_sizes=True, complex_page=True))
    (root / "dense-text-for-max-tokens.pdf").write_bytes(make_pdf(1, dense_page=True))
    (root / "encrypted-password-required.pdf").write_bytes(make_pdf(1, password=PASSWORD))
    normal = (root / "normal-1-page.pdf").read_bytes()
    (root / "corrupt-truncated.pdf").write_bytes(normal[: len(normal) // 2])
    (root / "not-a-pdf.txt").write_text("this is intentionally not a PDF\n", encoding="ascii")
    print(f"generated fixtures in {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
