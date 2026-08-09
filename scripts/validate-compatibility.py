#!/usr/bin/env python3
import json
import sys

def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate-compatibility.py MATRIX", file=sys.stderr)
        return 2
    data = json.load(open(sys.argv[1], encoding="utf-8"))
    required = ["schema_version", "model_id", "model_revision", "profiles", "mmproj", "runtime", "license", "source_url"]
    if any(key not in data for key in required) or data["model_id"] != "ovisocr2":
        print("invalid compatibility matrix", file=sys.stderr)
        return 3
    if data["profiles"] != ["compact", "balanced", "quality", "reference"]:
        print("profile matrix mismatch", file=sys.stderr)
        return 3
    if len(data["mmproj"].get("sha256", "")) != 64 or data["mmproj"].get("bytes", 0) <= 0:
        print("invalid projector lock", file=sys.stderr)
        return 3
    print(json.dumps({"model_id": data["model_id"], "revision": data["model_revision"], "profiles": data["profiles"]}, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
