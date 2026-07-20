import sys
import unittest
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import check_docs
from check_docs import check_parsed_document


class ParsedRequirementChecks(unittest.TestCase):
    def test_archived_change_uses_retained_artifact(self):
        with patch.object(check_docs.subprocess, "run", side_effect=AssertionError("openspec show is not needed after archive")):
            self.assertEqual(check_docs.check_openspec(), [])

    def test_truncated_requirement_and_scenario_fail(self):
        errors = check_parsed_document(
            {
                "deltas": [
                    {
                        "requirement": {
                            "text": "truncated prefix",
                            "scenarios": [{"rawText": "WHEN only"}],
                        }
                    }
                ]
            }
        )
        self.assertTrue(any("incomplete requirement body" in error for error in errors))
        self.assertTrue(any("WHEN and THEN" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
