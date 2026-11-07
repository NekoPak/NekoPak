import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('parity', ROOT / 'scripts/check_parity.py')
PARITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARITY)

class ParityGate(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((ROOT / 'docs/parity-manifest.json').read_text(encoding='utf-8'))

    def test_current_preview_is_valid_but_cannot_claim_parity(self):
        self.assertEqual(PARITY.check(self.manifest), [])
        self.assertTrue(PARITY.check(self.manifest, release=True))

    def test_required_features_cannot_be_deleted_or_demoted(self):
        for name in PARITY.REQUIRED_WINDOWS:
            deleted = copy.deepcopy(self.manifest)
            deleted['features'] = [f for f in deleted['features'] if f['id'] != name]
            self.assertTrue(PARITY.check(deleted))
            demoted = copy.deepcopy(self.manifest)
            next(f for f in demoted['features'] if f['id'] == name)['mandatory_windows'] = False
            self.assertTrue(PARITY.check(demoted))

    def test_verified_requires_existing_evidence(self):
        feature = self.manifest['features'][0]
        feature['status'] = 'verified'
        feature['evidence'] = []
        self.assertTrue(PARITY.check(self.manifest))
        feature['evidence'] = ['docs/NOT_REAL.md']
        self.assertTrue(PARITY.check(self.manifest))

    def test_feature_flags_do_not_replace_current_upstream_release_review(self):
        for feature in self.manifest['features']:
            if feature['mandatory_windows']:
                feature['status'] = 'verified'
                feature['evidence'] = ['docs/UPSTREAM_PARITY_RESEARCH.md']
        self.assertTrue(PARITY.check(self.manifest, release=True))
        self.manifest['baseline'].pop('reviewed_at')
        self.manifest['baseline']['latest_stable_review_required_before_release'] = False
        self.assertTrue(PARITY.check(self.manifest))

if __name__ == '__main__':
    unittest.main()
