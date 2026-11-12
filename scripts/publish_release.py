"""Assemble a verified, public preview release and a separately signed update manifest."""
import argparse
import base64
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

ROOT = Path(__file__).resolve().parents[1]


def require_platform_manifest(envelope, version, expected_names):
    payload = envelope["payload"]
    if (payload.get("schema_version") != 1 or payload.get("product") != "NekoPak"
            or payload.get("version") not in {version, f"v{version}"}):
        raise ValueError("Platform manifest identity or version does not match this release.")
    names = [item["name"] for item in payload["files"]]
    if len(names) != len(expected_names) or set(names) != set(expected_names):
        raise ValueError("Every published platform file must belong to its signed manifest.")


def require_source_reports(reports, commit):
    if any(report.get("commit") != commit or report.get("git_worktree_dirty") is not False for report in reports):
        raise ValueError("Release packages must be built from the clean checked out source commit.")


def sign_manifest(version, notes, files, key, published_at=None):
    if not re.fullmatch(r"0\.\d+\.\d+", version):
        raise ValueError("This preview publisher accepts only pre-1.0 versions.")
    if len(notes.encode()) > 16000 or not notes.strip():
        raise ValueError("Release notes must be present and bounded.")
    suffixes = {"windows-x86_64": "windows-x64-setup.exe", "android-aarch64": "android-arm64-v8a.apk"}
    platforms = {}
    for platform, path in files.items():
        if platform not in suffixes or path.name != f"NekoPak-{version}-{suffixes[platform]}":
            raise ValueError("Unexpected update artifact name or platform.")
        if not 0 < path.stat().st_size <= 256 * 1024 * 1024:
            raise ValueError("Update artifact size exceeds the supported limit.")
        platforms[platform] = {"name": path.name, "bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    if set(platforms) != set(suffixes):
        raise ValueError("Both signed platform packages are required.")
    payload = {"schema_version": 1, "product": "NekoPak", "version": version, "channel": "preview",
               "published_at": published_at or datetime.now(timezone.utc).isoformat(), "notes": notes, "platforms": platforms}
    raw = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return {"payload_base64": base64.b64encode(raw).decode(), "signature": base64.b64encode(key.sign(raw)).decode()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--windows", required=True, type=Path)
    parser.add_argument("--android", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    notes = (ROOT / "release/notes" / f"{version}.md").read_text(encoding="utf-8")
    archive_name = f"NekoPak-{version}-windows-x64-portable.zip"
    installer_name = f"NekoPak-{version}-windows-x64-setup.exe"
    android_stem = f"NekoPak-{version}-android-arm64-v8a"
    for directory, names in [(args.windows, {archive_name, installer_name, "size-report.json"}),
                             (args.android, {android_stem + ".apk", android_stem + "-licenses.zip", android_stem + "-report.json"})]:
        subprocess.run(["python", str(ROOT / "scripts/release_signing.py"), "verify", "--manifest", str(directory / "release-manifest.json"), "--public", str(ROOT / "release/release-signing.pub")], check=True)
        require_platform_manifest(json.loads((directory / "release-manifest.json").read_text(encoding="utf-8")), version, names)
    windows = json.loads((args.windows / "size-report.json").read_text(encoding="utf-8"))
    android_file = args.android / (android_stem + ".apk")
    android = json.loads(android_file.with_name(android_file.stem + "-report.json").read_text(encoding="utf-8"))
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    require_source_reports([windows, android], commit)
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
        raise ValueError("Publication requires a clean source checkout; use ignored staging directories.")
    key = Ed25519PrivateKey.from_private_bytes(base64.b64decode(os.environ["NEKOPAK_RELEASE_PRIVATE_KEY"].strip(), validate=True))
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    if public != base64.b64decode((ROOT / "release/release-signing.pub").read_text().strip(), validate=True):
        raise ValueError("Release key does not match the embedded updater trust key.")
    manifest = sign_manifest(version, notes, {"windows-x86_64": args.windows / installer_name, "android-aarch64": android_file}, key)
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        raise ValueError("Use an empty release staging directory.")
    output.mkdir(parents=True, exist_ok=True)
    for path in [args.windows / archive_name, args.windows / installer_name, args.windows / "size-report.json", android_file,
                 android_file.with_name(android_file.stem + "-licenses.zip"), android_file.with_name(android_file.stem + "-report.json"), ROOT / "release/release-signing.pub"]:
        shutil.copyfile(path, output / path.name)
    shutil.copyfile(args.windows / "release-manifest.json", output / "windows-release-manifest.json")
    shutil.copyfile(args.android / "release-manifest.json", output / "android-release-manifest.json")
    (output / "update-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (output / "release-notes.md").write_text(notes, encoding="utf-8")
    if args.publish:
        existing = json.loads(subprocess.check_output(["gh", "api", "repos/NekoPak/NekoPak/releases?per_page=100"], text=True, encoding="utf-8"))
        if any(release["tag_name"] == f"v{version}" for release in existing):
            raise ValueError("Release version already exists; never replace published updater assets.")
        tags = subprocess.check_output(["git", "ls-remote", "--tags", "https://github.com/NekoPak/NekoPak.git",
                                        f"refs/tags/v{version}", f"refs/tags/v{version}^{{}}"], text=True)
        revisions = dict(line.split()[::-1] for line in tags.splitlines() if line.strip())
        tagged_commit = revisions.get(f"refs/tags/v{version}^{{}}", revisions.get(f"refs/tags/v{version}"))
        if tagged_commit is not None and tagged_commit != commit:
            raise ValueError("An existing version tag points to different source; refusing publication.")
        # Create a draft with complete assets, publish last so clients never see
        # an incomplete package/manifest pair. No token or key goes in arguments.
        subprocess.run(["gh", "release", "create", f"v{version}", "--repo", "NekoPak/NekoPak", "--target", commit,
                        "--title", f"NekoPak {version} Preview", "--notes-file", str(output / "release-notes.md"), "--draft", "--prerelease", *[str(p) for p in output.iterdir() if p.name != "release-notes.md"]], check=True)
        subprocess.run(["gh", "release", "edit", f"v{version}", "--repo", "NekoPak/NekoPak", "--draft=false", "--prerelease", "--latest=false"], check=True)
    print(f"Prepared {'and published ' if args.publish else ''}signed preview {version} with both platforms.")


if __name__ == "__main__":
    main()
