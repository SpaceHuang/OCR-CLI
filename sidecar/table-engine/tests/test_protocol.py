"""Protocol fixture tests (PROTOCOL.md, frozen contract) — no paddle needed."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import engine_protocol as proto  # noqa: E402


class TestHello:
    def test_hello_frame_shape_is_frozen(self):
        hello = proto.Hello(
            engine=proto.ENGINE_ID,
            models_revision=proto.EXPECTED_MODELS_REVISION,
            protocol_revision=proto.EXPECTED_PROTOCOL_REVISION,
            paddleocr=proto.EXPECTED_PADDLEOCR,
            paddlex=proto.EXPECTED_PADDLEX,
            paddlepaddle=proto.EXPECTED_PADDLEPADDLE,
            models_sha256={"m": {"files": 3, "sha256": "abc"}},
        )
        obj = json.loads(hello.to_json())
        assert obj["type"] == "hello"
        assert obj["engine"] == "slanet-plus"
        assert obj["models_revision"] == "bae6e5f8"
        assert obj["protocol_revision"] == "proto2"
        assert obj["paddleocr"] == "3.7.0"
        assert obj["paddlex"] == "3.7.2"
        assert obj["paddlepaddle"] == "3.4.0"
        assert obj["models_sha256"] == {"m": {"files": 3, "sha256": "abc"}}

    def test_supply_chain_constants_do_not_drift(self):
        assert proto.ENGINE_ID == "slanet-plus"
        assert proto.EXPECTED_MODELS_REVISION == "bae6e5f8"
        assert proto.EXPECTED_PROTOCOL_REVISION == "proto2"
        assert proto.EXPECTED_PADDLEOCR == "3.7.0"
        assert proto.EXPECTED_PADDLEX == "3.7.2"
        assert proto.EXPECTED_PADDLEPADDLE == "3.4.0"


class TestRecognize:
    def test_roundtrip_decodes_png_b64(self):
        frame = json.dumps(
            {"type": "recognize", "id": 7, "png_b64": "aGk=", "page": 12, "ordinal": 3}
        )
        request = proto.Recognize.from_json(frame)
        assert request.id == 7
        assert request.png_bytes == b"hi"
        assert request.page == 12
        assert request.ordinal == 3

    def test_wrong_type_raises(self):
        try:
            proto.Recognize.from_json('{"type":"ping","id":1}')
            raise AssertionError("expected ProtocolError")
        except proto.ProtocolError:
            pass

    def test_missing_field_raises(self):
        try:
            proto.Recognize.from_json('{"type":"recognize","id":1,"png_b64":"aa"}')
            raise AssertionError("expected ProtocolError")
        except proto.ProtocolError:
            pass


class TestResults:
    def test_result_ok_shape(self):
        obj = json.loads(proto.result_ok(7, "<table>"))
        assert obj == {"type": "result", "id": 7, "ok": True, "html": "<table>"}

    def test_result_reject_shape_and_validation(self):
        obj = json.loads(proto.result_reject(8, proto.REJECT_NO_RULING_LINES))
        assert obj == {"type": "result", "id": 8, "ok": False, "reject": "no_ruling_lines"}
        obj = json.loads(proto.result_reject(8, proto.REJECT_EMPTY_OUTPUT))
        assert obj["reject"] == "empty_output"
        try:
            proto.result_reject(8, "something_else")
            raise AssertionError("expected ProtocolError")
        except proto.ProtocolError:
            pass

    def test_error_frame_shape(self):
        obj = json.loads(proto.error_frame(9, "boom"))
        assert obj == {"type": "error", "id": 9, "message": "boom"}

    def test_ping_pong_reuses_result_frame(self):
        ping = proto.Ping.from_json('{"type":"ping","id":3}')
        assert ping.id == 3
        assert json.loads(proto.result_ok(ping.id, "pong"))["html"] == "pong"


class TestFraming:
    def test_read_write_roundtrip(self):
        import io

        buf = io.BytesIO()
        proto.write_frame(buf, proto.result_ok(1, "x"))
        buf.seek(0)
        assert proto.read_frame(buf) == proto.result_ok(1, "x")

    def test_read_frame_eof_returns_none(self):
        import io

        assert proto.read_frame(io.BytesIO(b"")) is None

    def test_crlf_tolerance(self):
        import io

        assert proto.read_frame(io.BytesIO(b'{"a":1}\r\n')) == '{"a":1}'
