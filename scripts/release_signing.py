"""Sign portable artifacts with the repository's Ed25519 key, without printing secrets."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives import serialization


def public_text(key):
    return base64.b64encode(key.public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)).decode()


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    generate = commands.add_parser("generate")
    generate.add_argument("--private", type=Path, required=True)
    generate.add_argument("--public", type=Path, required=True)
    sign = commands.add_parser("sign")
    sign.add_argument("files", nargs="+", type=Path)
    sign.add_argument("--version", required=True)
    sign.add_argument("--output", type=Path, required=True)
    sign.add_argument("--key-file", type=Path)
    verify = commands.add_parser("verify")
    verify.add_argument("--manifest", type=Path, required=True)
    verify.add_argument("--public", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "generate":
        key = Ed25519PrivateKey.generate()
        args.private.parent.mkdir(parents=True, exist_ok=True)
        args.public.parent.mkdir(parents=True, exist_ok=True)
        if args.private.exists() or args.public.exists():
            raise SystemExit("Refusing to replace an existing release signing key.")
        private = key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())
        with args.private.open("x", encoding="ascii") as file:
            file.write(base64.b64encode(private).decode() + "\n")
        args.public.write_text(public_text(key.public_key()) + "\n", encoding="ascii")
        print("Created Ed25519 release key. Private material was not printed.")
        return
    if args.command == "sign":
        encoded = args.key_file.read_text(encoding="ascii").strip() if args.key_file else os.environ["NEKOPAK_RELEASE_PRIVATE_KEY"].strip()
        key = Ed25519PrivateKey.from_private_bytes(base64.b64decode(encoded, validate=True))
        files = []
        for path in args.files:
            data = path.read_bytes()
            files.append({"name": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        payload = {"schema_version": 1, "product": "NekoPak", "version": args.version, "files": files}
        canonical = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
        envelope = {"payload": payload, "signature": base64.b64encode(key.sign(canonical)).decode()}
        args.output.write_text(json.dumps(envelope, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"Signed {len(files)} artifact(s); version is covered by the signature.")
        return
    envelope = json.loads(args.manifest.read_text(encoding="utf-8"))
    canonical = json.dumps(envelope["payload"], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    key = Ed25519PublicKey.from_public_bytes(base64.b64decode(args.public.read_text(encoding="ascii").strip(), validate=True))
    key.verify(base64.b64decode(envelope["signature"], validate=True), canonical)
    for artifact in envelope["payload"]["files"]:
        if Path(artifact["name"]).name != artifact["name"]:
            raise SystemExit("Invalid signed artifact name.")
        data = (args.manifest.parent / artifact["name"]).read_bytes()
        if len(data) != artifact["bytes"] or hashlib.sha256(data).hexdigest() != artifact["sha256"]:
            raise SystemExit("Artifact does not match the signed manifest.")
    print("Signature and every artifact digest verified.")


if __name__ == "__main__":
    main()
