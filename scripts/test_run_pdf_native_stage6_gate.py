import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SET = ROOT / "assets/regression/pdf-native-text-candidate-v0.1"
SCRIPT = ROOT / "scripts/run-pdf-native-stage6-gate.py"

class Stage6GateTest(unittest.TestCase):
    def test_current_draft_is_structurally_valid_but_blocked_for_release(self):
        result = subprocess.run([sys.executable, str(SCRIPT), str(SET)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 4)
        report = json.loads(result.stdout)
        self.assertTrue(report["structural_valid"])
        self.assertFalse(report["release_ready"])
        self.assertIn("human_native_acceptance_incomplete", report["blocked_reasons"])

if __name__ == "__main__":
    unittest.main()
