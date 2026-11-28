"""Wire protocol for the table-engine sidecar (PROTOCOL.md, M4.2 D-59).

Line-delimited JSON over stdin/stdout, strictly serial. This module is
importable WITHOUT paddle — the framing/encoding layer only depends on the
standard library so the protocol fixture tests run anywhere.
"""

from __future__ import annotations

import base64
import json
import sys
from dataclasses import dataclass, field

ENGINE_ID = "slanet-plus"
# Frozen supply chain (plan D-59; requirements.txt must match).
# M4.3 D-72: the single `revision` split into two axes — the model-dir freeze
# (manifest-keyed, changes only when model files change) and the worker
# behavior generation (changes on any worker-side behavior change, e.g. the
# P0 no-table-found handling).
EXPECTED_MODELS_REVISION = "bae6e5f8"
EXPECTED_PROTOCOL_REVISION = "proto3"
# D-79 ship configuration: the GPU wheel with device="gpu". The cpu channel
# remains a legal runtime fallback (the device rides the hello for audit).
EXPECTED_DEVICE = "gpu"
EXPECTED_PADDLEOCR = "3.7.0"
EXPECTED_PADDLEX = "3.7.2"
EXPECTED_PADDLEPADDLE = "3.3.1"

REJECT_NO_RULING_LINES = "no_ruling_lines"
REJECT_EMPTY_OUTPUT = "empty_output"


@dataclass
class Hello:
    engine: str
    models_revision: str
    protocol_revision: str
    device: str
    paddleocr: str
    paddlex: str
    paddlepaddle: str
    models_sha256: dict = field(default_factory=dict)

    def to_json(self) -> str:
        return json.dumps(
            {
                "type": "hello",
                "engine": self.engine,
                "models_revision": self.models_revision,
                "protocol_revision": self.protocol_revision,
                "device": self.device,
                "paddleocr": self.paddleocr,
                "paddlex": self.paddlex,
                "paddlepaddle": self.paddlepaddle,
                "models_sha256": self.models_sha256,
            },
            sort_keys=True,
        )


@dataclass
class Recognize:
    id: int
    png_b64: str
    page: int
    ordinal: int

    @property
    def png_bytes(self) -> bytes:
        return base64.b64decode(self.png_b64)

    @classmethod
    def from_json(cls, line: str) -> "Recognize":
        obj = json.loads(line)
        if obj.get("type") != "recognize":
            raise ProtocolError(f"expected recognize frame, got {obj.get('type')!r}")
        for key in ("id", "png_b64", "page", "ordinal"):
            if key not in obj:
                raise ProtocolError(f"recognize frame missing {key!r}")
        return cls(
            id=int(obj["id"]),
            png_b64=obj["png_b64"],
            page=int(obj["page"]),
            ordinal=int(obj["ordinal"]),
        )


@dataclass
class Ping:
    id: int

    @classmethod
    def from_json(cls, line: str) -> "Ping":
        obj = json.loads(line)
        if obj.get("type") != "ping" or "id" not in obj:
            raise ProtocolError(f"expected ping frame: {line[:120]!r}")
        return cls(id=int(obj["id"]))


@dataclass
class RegionsRequest:
    """M4.3 P2a (D-71): generate route-A regions for EVERY page of the pdf.
    The model dir rides the request (the CLI owns the installed layout
    assets); the payload binding (source_pdf_sha256) is computed worker-side."""

    id: int
    pdf_path: str
    model_dir: str

    @classmethod
    def from_json(cls, line: str) -> "RegionsRequest":
        obj = json.loads(line)
        if obj.get("type") != "regions":
            raise ProtocolError(f"expected regions frame, got {obj.get('type')!r}")
        for key in ("id", "pdf_path", "model_dir"):
            if key not in obj:
                raise ProtocolError(f"regions frame missing {key!r}")
        return cls(id=int(obj["id"]), pdf_path=obj["pdf_path"], model_dir=obj["model_dir"])


def result_ok(request_id: int, html: str) -> str:
    return json.dumps({"type": "result", "id": request_id, "ok": True, "html": html}, sort_keys=True)


def regions_ok(request_id: int, regions_json: str) -> str:
    """M4.3 P2a: the regions payload rides its own success key (a `regions`
    request never answers with `html`)."""
    return json.dumps(
        {"type": "result", "id": request_id, "ok": True, "regions_json": regions_json},
        sort_keys=True,
    )


def result_reject(request_id: int, reject: str) -> str:
    if reject not in (REJECT_NO_RULING_LINES, REJECT_EMPTY_OUTPUT):
        raise ProtocolError(f"unknown reject reason: {reject!r}")
    return json.dumps(
        {"type": "result", "id": request_id, "ok": False, "reject": reject}, sort_keys=True
    )


def error_frame(request_id: int, message: str) -> str:
    return json.dumps({"type": "error", "id": request_id, "message": message}, sort_keys=True)


class ProtocolError(ValueError):
    """A malformed frame — the connection is considered unusable."""


def read_frame(stream) -> str | None:
    """One protocol line without the trailing newline; None on clean EOF."""
    line = stream.readline()
    if not line:
        return None
    return line.decode("utf-8", errors="strict").rstrip("\r\n")


def write_frame(stream, frame: str) -> None:
    stream.write((frame + "\n").encode("utf-8"))
    stream.flush()


def stdout() -> "TextIO":  # pragma: no cover - trivial alias for tests to patch
    return sys.stdout
