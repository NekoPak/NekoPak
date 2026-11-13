"""Manage a separate Android signing identity outside the source checkout.

Passwords are sent through environment variables/stdin, never shell arguments.
The backup directory must already exist with owner-only access.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["init", "configure-actions"])
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--keytool", type=Path)
    parser.add_argument("--repository", default="NekoPak/NekoPak")
    args = parser.parse_args()
    directory = args.directory.resolve(strict=True)
    root = Path(__file__).resolve().parents[1]
    if directory.is_relative_to(root) or not directory.is_dir():
        raise SystemExit("Use a protected backup directory outside the repository.")
    keystore = directory / "android-release.p12"
    metadata = directory / "android-signing.json"
    if args.command == "init":
        if keystore.exists() or metadata.exists():
            raise SystemExit("Android identity already exists; it was not overwritten.")
        if args.keytool is None:
            raise SystemExit("Pass a JDK21 keytool path.")
        password = secrets.token_urlsafe(36)
        values = {"keystore": str(keystore), "alias": "nekopak-android", "store_password": password,
                  "key_password": password, "format": "PKCS12", "application_id": "io.nekopak.desktop"}
        # Retain recoverable input before generating the permanent signing key.
        with metadata.open("x", encoding="utf-8") as output:
            os.chmod(metadata, 0o600)
            json.dump(values, output)
            output.flush()
            os.fsync(output.fileno())
        env = {**os.environ, "NEKOPAK_ANDROID_KEY_PASSWORD": password}
        result = subprocess.run([str(args.keytool), "-genkeypair", "-keystore", str(keystore),
            "-storetype", "PKCS12", "-storepass:env", "NEKOPAK_ANDROID_KEY_PASSWORD",
            "-keypass:env", "NEKOPAK_ANDROID_KEY_PASSWORD", "-alias", values["alias"],
            "-keyalg", "RSA", "-keysize", "3072", "-validity", "10000",
            "-dname", "CN=NekoPak Android, O=NekoPak Community", "-noprompt"],
            env=env, capture_output=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        if result.returncode:
            raise SystemExit("Key generation failed; protected recovery metadata was retained.")
        os.chmod(keystore, 0o600)
        print("Created a distinct Android signing identity in the protected backup directory.")
    else:
        values = json.loads(metadata.read_text(encoding="utf-8"))
        data = {"ANDROID_KEYSTORE_BASE64": base64.b64encode(keystore.read_bytes()).decode("ascii"),
                "ANDROID_KEYSTORE_PASSWORD": values["store_password"],
                "ANDROID_KEY_ALIAS": values["alias"], "ANDROID_KEY_PASSWORD": values["key_password"]}
        for name, value in data.items():
            result = subprocess.run(["gh", "secret", "set", name, "--repo", args.repository],
                input=value, text=True, capture_output=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            if result.returncode:
                raise SystemExit(f"Could not store {name}; existing local signing identity was retained.")
        print("Configured the four Android Actions secrets; no secret values were printed.")


if __name__ == "__main__":
    main()
