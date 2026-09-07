import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("native_scorer", Path(__file__).with_name("score-pdf-native.py"))
scorer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scorer)
ROOT = Path(__file__).resolve().parents[1]
SET = ROOT / "assets/regression/pdf-native-text-candidate-v0.1"

class NativeScorerTest(unittest.TestCase):
    def test_current_candidate_scores_perfect_against_itself(self):
        report = scorer.score(SET / "truth", SET / "truth")
        self.assertEqual(report["page_count"], 30)
        self.assertEqual(report["metrics"]["text_ned"], 0.0)
        self.assertEqual(report["metrics"]["block_f1"], 1.0)
        self.assertEqual(report["metrics"]["bbox_recall"], 1.0)

    def test_text_and_block_regression_is_measured(self):
        with tempfile.TemporaryDirectory() as directory:
            prediction = Path(directory)
            for source in (SET / "truth").glob("page-*.json"):
                page = json.loads(source.read_text())
                page["native_text"] = "wrong"
                page["blocks"] = []
                (prediction / source.name).write_text(json.dumps(page))
            report = scorer.score(SET / "truth", prediction)
            self.assertGreater(report["metrics"]["text_ned"], 0.0)
            self.assertEqual(report["metrics"]["block_recall"], 0.0)

    def test_direct_cli_page_document_is_not_treated_as_blank(self):
        page = {"page_number": 1, "blocks": [{"type": "markdown", "content": "hello"}]}
        normalized = scorer.normalize_prediction(page)
        self.assertEqual(normalized["native_text"], "hello")
        self.assertEqual(normalized["candidate_status"], "complete")

if __name__ == "__main__":
    unittest.main()
