"""Launcher regression checks compile a resource-only APK with the real AAPT2."""
import importlib.util
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


packaging = module("package_android", ROOT / "scripts/package_android.py")
project = module("android_project", ROOT / "scripts/android_project.py")


def chunk(name, data):
    return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data) & 0xffffffff)


def pixel_png(pixel):
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"\0" + bytes(pixel))) + chunk(b"IEND", b"")


class LauncherSourceTests(unittest.TestCase):
    def test_png_decoder_rejects_crc_and_ignores_fully_transparent_rgb(self):
        self.assertEqual(packaging.png_pixels(pixel_png((255, 100, 20, 0))), (1, 1, b"\0\0\0\0"))
        corrupt = bytearray(pixel_png((4, 5, 6, 255)))
        corrupt[-5] ^= 1
        with self.assertRaisesRegex(ValueError, "CRC"):
            packaging.png_pixels(bytes(corrupt))

    def test_sync_removes_only_old_launcher_templates_and_preserves_other_resources(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copytree(ROOT / "src-tauri/icons/android", root / "src-tauri/icons/android")
            resources = root / "src-tauri/gen/android/app/src/main/res"
            (resources / "drawable-v24").mkdir(parents=True)
            obsolete = resources / "drawable-v24/ic_launcher_foreground.xml"
            unrelated = resources / "drawable-v24/user_feature.xml"
            obsolete.write_text("template")
            unrelated.write_text("keep")
            report = project.sync_android_icons(root, resources)
            self.assertFalse(obsolete.exists())
            self.assertEqual(unrelated.read_text(), "keep")
            self.assertEqual(report["removed_template_resources"], ["drawable-v24/ic_launcher_foreground.xml"])
            for source in (root / "src-tauri/icons/android").rglob("*"):
                if source.is_file():
                    self.assertEqual(source.read_bytes(), (resources / source.relative_to(root / "src-tauri/icons/android")).read_bytes())
            self.assertEqual(project.sync_android_icons(root, resources)["removed_template_resources"], [])
            with self.assertRaisesRegex(ValueError, "outside"):
                project.sync_android_icons(root, root / "unrelated")


@unittest.skipUnless(os.environ.get("ANDROID_HOME"), "ANDROID_HOME required for actual AAPT2 packaged-resource tests")
class CompiledLauncherTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.sdk = Path(os.environ["ANDROID_HOME"])
        cls.aapt2 = packaging.sdk_tool(cls.sdk, "aapt2")
        cls.android = cls.sdk / "platforms/android-36/android.jar"
        if not cls.android.is_file():
            raise unittest.SkipTest("Android platform36 required")

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        self.res = self.work / "res"
        shutil.copytree(ROOT / "src-tauri/icons/android", self.res)

    def compile(self, icon="@mipmap/ic_launcher", round_icon="@mipmap/ic_launcher_round"):
        manifest = self.work / "AndroidManifest.xml"
        manifest.write_text(f'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="io.nekopak.desktop"><uses-sdk android:minSdkVersion="24" android:targetSdkVersion="36"/><application android:icon="{icon}" android:roundIcon="{round_icon}" /></manifest>', encoding="utf8")
        flattened, apk = self.work / "compiled.zip", self.work / "icons.apk"
        subprocess.run([str(self.aapt2), "compile", "--dir", str(self.res), "-o", str(flattened)], check=True, capture_output=True)
        subprocess.run([str(self.aapt2), "link", "-I", str(self.android), "--manifest", str(manifest), "--auto-add-overlay", "-o", str(apk), str(flattened)], check=True, capture_output=True)
        return apk

    def test_actual_compiled_brand_resources_pass_including_themed_and_round(self):
        result = packaging.verify_launcher_icons(self.compile(), self.sdk)
        self.assertEqual(result["png_variants_verified"], 25)
        self.assertEqual(len(result["adaptive_variants_verified"]), 4)

    def test_same_resource_name_with_wrong_pixels_is_rejected(self):
        # The former Tauri-template bug kept the correct resource name while its
        # pixels were unrelated. Verify contents, not merely filenames/presence.
        (self.res / "mipmap-hdpi/ic_launcher.png").write_bytes(pixel_png((255, 220, 0, 255)))
        with self.assertRaisesRegex(ValueError, "pixels do not match NekoPak"):
            packaging.verify_launcher_icons(self.compile(), self.sdk)

    def test_wrong_manifest_round_icon_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "roundIcon"):
            packaging.verify_launcher_icons(self.compile(round_icon="@mipmap/ic_launcher_foreground"), self.sdk)

    def test_wrong_adaptive_foreground_graph_is_rejected(self):
        source = self.res / "mipmap-anydpi-v33/ic_launcher.xml"
        source.write_text(source.read_text().replace("@mipmap/ic_launcher_foreground", "@mipmap/ic_launcher_background"))
        with self.assertRaisesRegex(ValueError, "layer graph"):
            packaging.verify_launcher_icons(self.compile(), self.sdk)


if __name__ == "__main__":
    unittest.main()
