import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("publish_release", ROOT / "scripts/publish_release.py")
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)


class UpdateManifestTests(unittest.TestCase):
    def test_only_clean_matching_source_reports_are_publishable(self):
        report = {"commit": "a" * 40, "git_worktree_dirty": False}
        publish.require_source_reports([report, report], "a" * 40)
        for bad in [{**report, "commit": "b" * 40}, {**report, "git_worktree_dirty": True}, {"commit": "a" * 40}]:
            with self.subTest(report=bad), self.assertRaises(ValueError):
                publish.require_source_reports([report, bad], "a" * 40)

    def test_publisher_rejects_unsigned_files_wrong_identity_and_versions(self):
        expected = {"package.apk", "licenses.zip"}
        payload = {"schema_version": 1, "product": "NekoPak", "version": "0.1.3",
                   "files": [{"name": name} for name in sorted(expected)]}
        publish.require_platform_manifest({"payload": payload}, "0.1.3", expected)
        for field, value in [("version", "0.1.2"), ("product", "other"), ("schema_version", 2),
                             ("files", [{"name": "package.apk"}, {"name": "../licenses.zip"}]),
                             ("files", [{"name": "package.apk"}, {"name": "package.apk"}])]:
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                publish.require_platform_manifest({"payload": {**payload, field: value}}, "0.1.3", expected)

    def test_exact_payload_signature_covers_both_packages_and_notes(self):
        with tempfile.TemporaryDirectory() as temporary:
            files = {}
            for platform, suffix in [("windows-x86_64", "windows-x64-setup.exe"), ("android-aarch64", "android-arm64-v8a.apk")]:
                path = Path(temporary) / f"NekoPak-0.1.3-{suffix}"
                path.write_bytes(b"synthetic update package")
                files[platform] = path
            key = Ed25519PrivateKey.generate()
            envelope = publish.sign_manifest("0.1.3", "更新说明", files, key, "2026-11-10T00:00:00Z")
            payload = base64.b64decode(envelope["payload_base64"])
            signature = base64.b64decode(envelope["signature"])
            key.public_key().verify(signature, payload)
            decoded = json.loads(payload)
            self.assertEqual(decoded["notes"], "更新说明")
            self.assertEqual(len(decoded["platforms"]), 2)
            with self.assertRaises(Exception):
                key.public_key().verify(signature, payload + b" ")
            with self.assertRaises(ValueError):
                publish.sign_manifest("1.0.0", "notes", files, key)
            files.pop("android-aarch64")
            with self.assertRaises(ValueError):
                publish.sign_manifest("0.1.3", "notes", files, key)

if __name__ == "__main__":
    unittest.main()
