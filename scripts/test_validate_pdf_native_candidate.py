import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import importlib.util

spec = importlib.util.spec_from_file_location("validator", Path(__file__).with_name("validate-pdf-native-candidate.py"))
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)

ROOT = Path(__file__).resolve().parents[1]
SET = ROOT / "assets/regression/pdf-native-text-candidate-v0.1"

class PdfNativeCandidateValidatorTest(unittest.TestCase):
    def test_current_candidate_passes_structural_validation(self):
        with patch.object(validator, "sha256", return_value=json.loads((SET / "manifest.json").read_text())["input_sha256"]):
            self.assertEqual(validator.validate(SET), 0)

    def test_rejects_non_normalized_bbox(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            import shutil
            shutil.copytree(SET, target, dirs_exist_ok=True)
            page = json.loads((SET / "truth/page-0001.json").read_text())
            page["blocks"][0]["bbox"]["right"] = 1001
            (target / "truth/page-0001.json").write_text(json.dumps(page))
            with patch.object(validator, "sha256", return_value=json.loads((SET / "manifest.json").read_text())["input_sha256"]):
                self.assertEqual(validator.validate(target), 3)

if __name__ == "__main__":
    unittest.main()
