import copy
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release_context", ROOT / "scripts/release_context.py")
context = importlib.util.module_from_spec(spec)
spec.loader.exec_module(context)


class ReleaseContextTests(unittest.TestCase):
    def test_matching_tag_and_manual_versions(self):
        self.assertEqual(context.resolve_version("0.2.0", "push", "refs/tags/v0.2.0", {}, "NekoPak/NekoPak"), "v0.2.0")
        for requested in ["", "0.2.0", "v0.2.0"]:
            self.assertIn(context.resolve_version("0.2.0", "workflow_dispatch", "refs/heads/main", {}, "NekoPak/NekoPak", requested), {"0.2.0", "v0.2.0"})
        self.assertEqual(context.resolve_version("0.2.0", "push", "refs/tags/parity-0.2.0", {}, "NekoPak/NekoPak"), "parity-0.2.0")

    def test_mismatched_versions_branches_and_unsafe_labels_are_rejected(self):
        for event, ref, requested in [("push", "refs/tags/v0.1.4", ""), ("push", "refs/heads/main", ""),
                                       ("workflow_dispatch", "refs/heads/main", "0.2.0\nOTHER=value"),
                                       ("workflow_dispatch", "refs/heads/main", "0.1.4")]:
            with self.subTest(event=event, ref=ref, requested=requested), self.assertRaises(ValueError):
                context.resolve_version("0.2.0", event, ref, {}, "NekoPak/NekoPak", requested)

    def test_only_matching_merged_release_prs_from_this_repository(self):
        pr = {"merged": True, "base": {"ref": "main"},
              "head": {"ref": "codex/release-0.2.0", "repo": {"full_name": "NekoPak/NekoPak"}}}
        self.assertEqual(context.resolve_version("0.2.0", "pull_request", "refs/heads/main", {"pull_request": pr}, "NekoPak/NekoPak"), "0.2.0")
        for field, value in [("merged", False), ("base", {"ref": "other"}),
                             ("head", {"ref": "codex/release-0.2.0", "repo": {"full_name": "other/fork"}}),
                             ("head", {"ref": "codex/release-0.1.4", "repo": {"full_name": "NekoPak/NekoPak"}}),
                             ("head", {"ref": "feature/work", "repo": {"full_name": "NekoPak/NekoPak"}})]:
            changed = copy.deepcopy(pr)
            changed[field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                context.resolve_version("0.2.0", "pull_request", "refs/heads/main", {"pull_request": changed}, "NekoPak/NekoPak")


if __name__ == "__main__":
    unittest.main()
