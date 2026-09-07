import json, subprocess, sys, unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/validate-pdf-native-forbidden.py"
SET = ROOT / "assets/regression/pdf-native-forbidden-v0.1"
class ForbiddenValidatorTest(unittest.TestCase):
    def test_generated_fixture_is_valid(self):
        result = subprocess.run([sys.executable, str(SCRIPT), str(SET)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["pages"], 20)
        self.assertFalse(report["native_allowed"])
if __name__ == "__main__": unittest.main()
