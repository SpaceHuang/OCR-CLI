import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/validate-regression-set.py"
NATIVE = ROOT / "assets/regression/pdf-native-text-candidate-v0.1"

class RegressionValidatorDispatchTest(unittest.TestCase):
    def test_legacy_entrypoint_dispatches_native_manifest(self):
        result = subprocess.run([sys.executable, str(SCRIPT), str(NATIVE)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["pages"], 30)
        self.assertTrue(report["valid"])

    def test_legacy_manifest_still_uses_legacy_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {"schema_version": 1, "page_count": 30, "input_sha256": "x", "prompt_revision": "p", "adapter_revision": "a"}
            (root / "manifest.json").write_text(json.dumps(manifest))
            (root / "truth").mkdir()
            result = subprocess.run([sys.executable, str(SCRIPT), str(root)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 3)
            self.assertIn("truth page count", result.stderr)

if __name__ == "__main__":
    unittest.main()
