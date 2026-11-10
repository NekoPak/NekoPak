import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_android", ROOT / "scripts/package_android.py")
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


def elf(alignment=16384):
    data = bytearray(120)
    data[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<Q", data, 32, 64)
    struct.pack_into("<HH", data, 54, 56, 1)
    struct.pack_into("<I", data, 64, 1)
    struct.pack_into("<Q", data, 112, alignment)
    return bytes(data)


def create_apk(path, alignment=16384, include_licenses=True, abi="arm64-v8a"):
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(f"lib/{abi}/libnekopak_lib.so", elf(alignment))
        if include_licenses:
            contents = io.BytesIO()
            with zipfile.ZipFile(contents, "w") as notices:
                for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "DEPENDENCY_LICENSES.json", "FRONTEND_LICENSES.json", "ANDROID_LICENSES.json"]:
                    notices.writestr(name, "synthetic fixture")
                notices.writestr("LICENSE_INVENTORY.json", json.dumps({"license_text_gaps": []}))
            archive.writestr("assets/nekopak-licenses/licenses.zip", contents.getvalue())


class AndroidPackageValidation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.apk = Path(self.temp.name) / "app.apk"
        create_apk(self.apk)
        self.debuggable = False
        self.debug_key = False
        self.signature_fails = False

    def tool_output(self, *args, **kwargs):
        if "apksigner" in args[0]:
            if self.signature_fails:
                raise subprocess.CalledProcessError(1, args[0])
            return "Signer #1 certificate SHA-256 digest: " + "a" * 64 + "\nSigner #1 certificate DN: " + (
                "CN=Android Debug, O=Android, C=US" if self.debug_key else "CN=NekoPak synthetic test"
            )
        if "aapt" in args[0]:
            return "package: name='io.nekopak.desktop' versionCode='100' versionName='0.1.0'\nsdkVersion:'24'\ntargetSdkVersion:'36'\n" + (
                "application-debuggable" if self.debuggable else ""
            )
        if "zipalign" in args[0]:
            return "Verification successful"
        self.fail("Unexpected external command")

    def inspect(self, profile="release", abi="arm64-v8a"):
        with patch.object(packaging, "sdk_tool", side_effect=lambda sdk, name: Path(name)), patch.object(packaging, "run", side_effect=self.tool_output), patch.object(packaging, "verify_launcher_icons", return_value={"brand": "NekoPak cat"}):
            return packaging.apk_metadata(self.apk, Path(self.temp.name), abi, profile)

    def test_verified_release_reports_certificate_and_all_native_alignment(self):
        result = self.inspect()
        self.assertTrue(result["signature_verified"])
        self.assertTrue(result["zip_alignment_16k"])
        self.assertEqual(result["native_libraries"][0]["load_alignments"], [16384])
        self.assertEqual(result["certificate_sha256"], ["a" * 64])

    def test_rejects_4k_native_library_even_if_zip_is_aligned(self):
        create_apk(self.apk, alignment=4096)
        with self.assertRaisesRegex(ValueError, "16 KiB"):
            self.inspect()

    def test_rejects_debug_app_or_debug_signing_identity_as_release(self):
        self.debuggable = True
        with self.assertRaisesRegex(ValueError, "profile"):
            self.inspect()
        self.debuggable = False
        self.debug_key = True
        with self.assertRaisesRegex(ValueError, "debug signing"):
            self.inspect()

    def test_failed_apksigner_is_not_accepted(self):
        self.signature_fails = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.inspect()

    def test_rejects_missing_license_payload(self):
        create_apk(self.apk, include_licenses=False)
        with self.assertRaisesRegex(ValueError, "license inventory"):
            self.inspect()

    def test_rejects_mislabeled_architecture(self):
        create_apk(self.apk, abi="x86_64")
        with self.assertRaisesRegex(ValueError, "ABI"):
            self.inspect()

    def test_debug_is_explicitly_labeled(self):
        self.debuggable = self.debug_key = True
        self.assertTrue(self.inspect("debug")["debuggable"])

    def test_truncated_or_hostile_elf_headers_are_rejected(self):
        for data in [b"", b"not an ELF" * 16, elf()[:70]]:
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                packaging.elf_alignment(data)
        data = bytearray(elf())
        struct.pack_into("<Q", data, 32, 2**63)
        with self.assertRaises(ValueError):
            packaging.elf_alignment(data)

    def test_license_archive_paths_cannot_escape(self):
        for path in ["../key", "/key", "C:\\key", "a/../../key", "a//key"]:
            with self.subTest(path=path):
                self.assertFalse(packaging.safe_archive_name(path))

    def test_maven_parent_license_is_preserved_and_cycles_fail(self):
        cache = Path(self.temp.name) / "caches/modules-2/files-2.1"
        child = cache / "example/child/1/hash/child.pom"
        parent = cache / "example/parent/1/hash/parent.pom"
        child.parent.mkdir(parents=True)
        parent.parent.mkdir(parents=True)
        child.write_text("<project><parent><groupId>example</groupId><artifactId>parent</artifactId><version>1</version></parent></project>")
        parent.write_text("<project><licenses><license><name>Apache License, Version 2.0</name><url>https://www.apache.org/licenses/LICENSE-2.0</url></license></licenses></project>")
        with patch.dict(os.environ, {"GRADLE_USER_HOME": self.temp.name}):
            licenses, poms = packaging._pom_licenses({"group": "example", "name": "child", "version": "1"})
            self.assertEqual(len(poms), 2)
            self.assertEqual(licenses[0]["name"], "Apache License, Version 2.0")
            parent.write_text("<project><parent><groupId>example</groupId><artifactId>child</artifactId><version>1</version></parent></project>")
            with self.assertRaisesRegex(ValueError, "Cyclic"):
                packaging._pom_licenses({"group": "example", "name": "child", "version": "1"})

    def test_native_freshness_rejects_newer_rust_input(self):
        root = Path(self.temp.name)
        files = ["Cargo.toml", "Cargo.lock", "src-tauri/Cargo.toml", "src-tauri/build.rs",
                 "src-tauri/tauri.conf.json", "src-tauri/tauri.android.conf.json"]
        for name in files:
            file = root / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text("synthetic input")
            os.utime(file, (100, 100))
        library = root / "target/x86_64-linux-android/debug/libnekopak_app.so"
        library.parent.mkdir(parents=True)
        library.write_bytes(elf())
        os.utime(library, (200, 200))
        os.utime(self.apk, (300, 300))
        with patch.object(packaging, "run", return_value=json.dumps({"target_directory": str(root / "target")})):
            self.assertTrue(packaging.build_provenance(self.apk, "x86_64", "debug", root)["freshness_checked"])
            os.utime(root / "src-tauri/build.rs", (400, 400))
            with self.assertRaisesRegex(ValueError, "newer than the native"):
                packaging.build_provenance(self.apk, "x86_64", "debug", root)

    def test_release_cannot_attribute_a_stale_library_to_fresh_cargo_output(self):
        current = "a" * 64
        result = {"profile": "release", "cargo_native_sha256": current,
                  "native_libraries": [{"path": "lib/arm64-v8a/libnekopak_app.so", "sha256": "b" * 64},
                                       {"path": "lib/arm64-v8a/libunrelated.so", "sha256": current}]}
        with self.assertRaisesRegex(ValueError, "freshly built NekoPak"):
            packaging.require_matching_native(result, "arm64-v8a")
        result["native_libraries"][0]["sha256"] = current
        packaging.require_matching_native(result, "arm64-v8a")


if __name__ == "__main__":
    unittest.main()
