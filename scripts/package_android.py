"""Stage Android licenses or verify and package a built APK. Never creates signing keys."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import xml.etree.ElementTree as ET
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}
UI_PACKAGES = ["preact", "@tauri-apps/api", "@material-symbols/svg-400", "@material/material-color-utilities"]


def run(*args: str, cwd: Path = ROOT) -> str:
    return subprocess.check_output(list(map(str, args)), cwd=cwd, text=True, encoding="utf-8", errors="replace").strip()


def digest(path: Path) -> str:
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def json_text(data: object) -> bytes:
    return (json.dumps(data, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def safe_archive_name(name: str) -> bool:
    return bool(name) and not name.startswith(("/", "\\")) and "\\" not in name and all(part not in ("", ".", "..") for part in name.split("/"))


def elf_alignment(data: bytes) -> dict:
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[5] not in (1, 2):
        raise ValueError("Invalid ELF library.")
    endian = "<" if data[5] == 1 else ">"
    kind = data[4]
    if kind == 2:
        offset = struct.unpack_from(endian + "Q", data, 32)[0]
        entry_size, count = struct.unpack_from(endian + "HH", data, 54)
        minimum, alignment_offset, width = 56, 48, "Q"
    elif kind == 1:
        offset = struct.unpack_from(endian + "I", data, 28)[0]
        entry_size, count = struct.unpack_from(endian + "HH", data, 42)
        minimum, alignment_offset, width = 32, 28, "I"
    else:
        raise ValueError("Unknown ELF class.")
    if entry_size < minimum or not count or offset + count * entry_size > len(data):
        raise ValueError("Invalid ELF program-header bounds.")
    alignments = []
    for index in range(count):
        start = offset + index * entry_size
        if struct.unpack_from(endian + "I", data, start)[0] == 1:
            alignment = struct.unpack_from(endian + width, data, start + alignment_offset)[0]
            if alignment < 16384 or alignment & (alignment - 1):
                raise ValueError("Native library is not aligned for 16 KiB Android pages.")
            alignments.append(alignment)
    if not alignments:
        raise ValueError("ELF library has no LOAD segments.")
    return {"elf_class": kind * 32, "load_alignments": alignments, "page_size_16k": True}


def _license_files(base: Path) -> list[Path]:
    return sorted({path for pattern in ["LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "UNLICENSE*", "license*", "licenses/**/*"]
                   for path in base.glob(pattern) if path.is_file()})


def _pom_licenses(record: dict) -> tuple[list[dict], list[Path]]:
    cache = Path(os.environ.get("GRADLE_USER_HOME", Path.home() / ".gradle")) / "caches/modules-2/files-2.1"
    pending = [(record["group"], record["name"], record["version"])]
    seen = set()
    poms = []
    licenses = []
    namespace = {"m": "http://maven.apache.org/POM/4.0.0"}
    while pending:
        coordinates = pending.pop()
        if coordinates in seen or len(seen) >= 12:
            raise ValueError("Cyclic or excessively deep Maven license inheritance.")
        seen.add(coordinates)
        for pom in sorted(cache.joinpath(*coordinates).glob("*/*.pom")):
            poms.append(pom)
            root = ET.parse(pom).getroot()
            declarations = root.findall("m:licenses/m:license", namespace) + root.findall("licenses/license")
            for item in declarations:
                name = item.findtext("m:name", namespaces=namespace) or item.findtext("name") or ""
                url = item.findtext("m:url", namespaces=namespace) or item.findtext("url") or ""
                if name or url:
                    licenses.append({"name": name, "url": url})
            if not declarations:
                parent = root.find("m:parent", namespace)
                if parent is None:
                    parent = root.find("parent")
                if parent is not None:
                    fields = tuple(parent.findtext(f"m:{name}", namespaces=namespace) or parent.findtext(name) or "" for name in ["groupId", "artifactId", "version"])
                    if all(re.fullmatch(r"[A-Za-z0-9_.+-]+", value) for value in fields):
                        pending.append(fields)
    return licenses, poms


def _artifact_notices(artifact: Path) -> dict[str, bytes]:
    files = {}
    if not artifact.is_file() or not zipfile.is_zipfile(artifact):
        return files
    with zipfile.ZipFile(artifact) as archive:
        for name in archive.namelist():
            if safe_archive_name(name) and re.search(r"(^|/)(LICENSE|LICENCE|NOTICE|COPYING)([._-]|$)", name, re.I):
                files[name] = archive.read(name)
            elif name == "classes.jar":
                with zipfile.ZipFile(io.BytesIO(archive.read(name))) as classes:
                    for entry in classes.namelist():
                        if safe_archive_name(entry) and re.search(r"(^|/)(LICENSE|LICENCE|NOTICE|COPYING)([._-]|$)", entry, re.I):
                            files[f"classes.jar/{entry}"] = classes.read(entry)
    return files


def licenses(gradle_inventory: Path, assets_dir: Path, root: Path = ROOT) -> dict:
    generated = (root / "src-tauri/gen/android").resolve()
    assets_dir = assets_dir.resolve()
    if not assets_dir.is_relative_to(generated) or assets_dir.name != "nekopak-licenses":
        raise ValueError("License assets must be inside the generated Android project.")
    files = {name: (root / name).read_bytes() for name in ["LICENSE", "THIRD_PARTY_NOTICES.md"]}
    supplemental = json.loads((root / "licenses/sources.json").read_text(encoding="utf-8"))
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--format-version", "1", cwd=root))
    tree = run("cargo", "tree", "--locked", "--offline", "-p", "nekopak-app", "--target", "aarch64-linux-android", "--edges", "normal", "--prefix", "none", cwd=root)
    android_packages = {(parts[0], parts[1][1:]) for line in tree.splitlines() if len(parts := line.split()) > 1 and parts[1].startswith("v")}
    rust = []
    for package in metadata["packages"]:
        if package["source"] is None or (package["name"], package["version"]) not in android_packages:
            continue
        base = Path(package["manifest_path"]).parent
        texts = _license_files(base)
        additions = [item for item in supplemental if item["name"] == package["name"] and item["version"] == package["version"] and "file" in item]
        if not texts and not additions:
            raise ValueError(f"Missing Android Rust license text: {package['name']}@{package['version']}")
        prefix = f"rust/{package['name']}-{package['version']}"
        for file in texts:
            files[f"{prefix}/{file.relative_to(base).as_posix()}"] = file.read_bytes()
        for item in additions:
            file = root / item["file"]
            if digest(file) != item["sha256"]:
                raise ValueError(f"Supplemental license checksum mismatch: {file.name}")
            files[f"{prefix}/upstream/{file.name}"] = file.read_bytes()
        rust.append({"name": package["name"], "version": package["version"], "license": package.get("license"),
                     "source": package["source"], "license_files": [str(file.relative_to(base)) for file in texts],
                     "supplemental_licenses": additions})
    frontend = []
    for name in UI_PACKAGES:
        directory = root / "node_modules" / name
        package = json.loads((directory / "package.json").read_text(encoding="utf-8"))
        texts = _license_files(directory)
        if not texts:
            raise ValueError(f"Missing frontend license text: {name}")
        for file in texts:
            files[f"frontend/{name}/{file.relative_to(directory).as_posix()}"] = file.read_bytes()
        frontend.append({"name": name, "version": package["version"], "license": package.get("license"),
                         "files": [file.relative_to(directory).as_posix() for file in texts]})
    android = []
    apache = root / "licenses/upstream/tauri-plugin-2.7.1/LICENSE-APACHE-2.0"
    for record in json.loads(gradle_inventory.read_text(encoding="utf-8")):
        if record.get("project"):
            continue  # Tauri and its plugins are covered by the Rust crate inventory.
        declaration, poms = _pom_licenses(record)
        artifact = Path(record["file"])
        notices = _artifact_notices(artifact)
        prefix = f"android/{record['group']}/{record['name']}-{record['version']}"
        for file in poms:
            files[f"{prefix}/{file.name}"] = file.read_bytes()
        for name, data in notices.items():
            files[f"{prefix}/{name}"] = data
        if any("apache" in (item["name"] + item["url"]).lower() and ("2.0" in item["name"] or "2.0" in item["url"]) for item in declaration):
            files[f"{prefix}/LICENSE-APACHE-2.0"] = apache.read_bytes()
        elif not notices:
            raise ValueError(f"Missing Android JVM license text: {record['group']}:{record['name']}:{record['version']}")
        android.append({key: record[key] for key in ["group", "name", "version"]} | {
            "licenses": declaration, "artifact_sha256": digest(artifact), "notice_files": sorted(notices)})
    files["DEPENDENCY_LICENSES.json"] = json_text(rust)
    files["FRONTEND_LICENSES.json"] = json_text(frontend)
    files["ANDROID_LICENSES.json"] = json_text(android)
    report = {"scope": "Resolved Android Rust normal dependencies, bundled frontend/assets and Android JVM runtime artifacts; not a legal certification.",
              "rust_packages": len(rust), "frontend_packages": len(frontend), "android_packages": len(android),
              "files": len(files), "license_text_gaps": []}
    files["LICENSE_INVENTORY.json"] = json_text(report)
    assets_dir.mkdir(parents=True, exist_ok=True)
    # A single ZIP avoids stale loose files when the generated host is refreshed.
    archive_path = assets_dir / "licenses.zip"
    with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in sorted(files.items()):
            if not safe_archive_name(name):
                raise ValueError("Unsafe license archive name.")
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    print(json.dumps(report, indent=2))
    return report


def sdk_tool(sdk: Path, name: str) -> Path:
    extension = ".bat" if os.name == "nt" and name == "apksigner" else ".exe" if os.name == "nt" else ""
    path = sdk / "build-tools/36.0.0" / (name + extension)
    if not path.is_file():
        raise ValueError(f"Android build-tools 36.0.0 is missing {name}.")
    return path


def png_pixels(data: bytes) -> tuple[int, int, bytes]:
    """Decode bounded non-interlaced launcher PNG pixels, ignoring AAPT encoding."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("Launcher asset is not a PNG.")
    offset, compressed, palette, transparency = 8, bytearray(), b"", b""
    width = height = depth = kind = 0
    while offset + 12 <= len(data):
        size = struct.unpack_from(">I", data, offset)[0]
        tag = data[offset + 4:offset + 8]
        end = offset + 12 + size
        if end > len(data):
            raise ValueError("Truncated launcher PNG.")
        payload = data[offset + 8:offset + 8 + size]
        if zlib.crc32(tag + payload) & 0xffffffff != struct.unpack_from(">I", data, offset + 8 + size)[0]:
            raise ValueError("Launcher PNG CRC mismatch.")
        if tag == b"IHDR":
            width, height, depth, kind, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
            if not 0 < width <= 2048 or not 0 < height <= 2048 or compression or filtering or interlace:
                raise ValueError("Unsupported launcher PNG dimensions or interlace.")
        elif tag == b"PLTE":
            palette = payload
        elif tag == b"tRNS":
            transparency = payload
        elif tag == b"IDAT":
            compressed.extend(payload)
        elif tag == b"IEND":
            break
        offset = end
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(kind)
    if channels is None or (depth != 8 and not (kind in (0, 3) and depth in (1, 2, 4))):
        raise ValueError("Unsupported launcher PNG pixel format.")
    stride = (width * channels * depth + 7) // 8
    expected = (stride + 1) * height
    decoder = zlib.decompressobj()
    raw = decoder.decompress(bytes(compressed), expected + 1)
    if len(raw) != expected or not decoder.eof:
        raise ValueError("Invalid launcher PNG scanlines.")
    step = max(1, (channels * depth + 7) // 8)
    previous = bytearray(stride)
    rgba = bytearray()
    for row in range(height):
        start = row * (stride + 1)
        mode, current = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        if mode > 4:
            raise ValueError("Invalid launcher PNG filter.")
        for index in range(stride):
            left = current[index - step] if index >= step else 0
            up = previous[index]
            corner = previous[index - step] if index >= step else 0
            estimate = left + up - corner
            distances = [abs(estimate - value) for value in (left, up, corner)]
            paeth = (left, up, corner)[distances.index(min(distances))]
            current[index] = (current[index] + (0, left, up, (left + up) // 2, paeth)[mode]) & 255
        for x in range(width):
            if depth < 8:
                value = (current[x * depth // 8] >> (8 - depth - x * depth % 8)) & ((1 << depth) - 1)
                values = [value]
            else:
                values = current[x * channels:(x + 1) * channels]
            if kind == 6:
                pixel = tuple(values)
            elif kind == 2:
                transparent = tuple(struct.unpack(">HHH", transparency)) if len(transparency) == 6 else None
                pixel = (*values, 0 if tuple(values) == transparent else 255)
            elif kind == 3:
                index = values[0]
                if len(palette) < (index + 1) * 3:
                    raise ValueError("Launcher PNG palette is incomplete.")
                pixel = (*palette[index * 3:index * 3 + 3], transparency[index] if index < len(transparency) else 255)
            elif kind == 4:
                pixel = (values[0], values[0], values[0], values[1])
            else:
                value = values[0]
                grey = value * 255 // ((1 << depth) - 1)
                transparent = struct.unpack(">H", transparency)[0] if len(transparency) == 2 else None
                pixel = (grey, grey, grey, 0 if value == transparent else 255)
            # Optimizers may discard RGB under alpha=0; visual content is equal.
            rgba.extend(pixel if pixel[3] else (0, 0, 0, 0))
        previous = current
    return width, height, bytes(rgba)


def resource_table(text: str) -> dict:
    resources, current = {}, None
    for line in text.splitlines():
        header = re.match(r"\s*resource\s+(0x[0-9a-fA-F]+)\s+([^\s]+)", line)
        if header:
            current = {"id": header[1].lower(), "name": header[2], "files": {}}
            resources[header[2]] = current
        elif current:
            entry = re.match(r"\s*\(([^)]*)\)\s+\(file\)\s+(\S+)\s+type=(PNG|XML)", line)
            if entry:
                current["files"][entry[1]] = {"path": entry[2], "type": entry[3]}
    return resources


def verify_launcher_icons(apk: Path, sdk: Path, root: Path = ROOT) -> dict:
    """Follow manifest IDs and compiled adaptive references to actual branded pixels."""
    aapt2 = str(sdk_tool(sdk, "aapt2"))
    table = resource_table(run(aapt2, "dump", "resources", str(apk)))
    by_id = {value["id"]: value["name"] for value in table.values()}
    source = root / "src-tauri/icons/android"
    pixels_checked, aggregate = 0, hashlib.sha256()
    with zipfile.ZipFile(apk) as archive:
        # Validate legacy pixels first: the formerly shipped APK has the same
        # resource names but Tauri's blue/yellow glyph, not this project's cat.
        for file in sorted(source.rglob("*.png")):
            resource_name = f"{file.parent.name.split('-')[0]}/{file.stem}"
            qualifier = file.parent.name.split("-", 1)[1]
            definition = table.get(resource_name, {})
            entry = definition.get("files", {}).get(qualifier)
            if not entry or entry["type"] != "PNG":
                raise ValueError(f"APK launcher resource is missing: {resource_name} ({qualifier}).")
            expected = png_pixels(file.read_bytes())
            actual = png_pixels(archive.read(entry["path"]))
            if expected != actual:
                raise ValueError(f"APK launcher pixels do not match NekoPak brand: {resource_name} ({qualifier}).")
            aggregate.update(file.relative_to(source).as_posix().encode())
            aggregate.update(actual[2])
            pixels_checked += 1
    manifest = run(aapt2, "dump", "xmltree", "--file", "AndroidManifest.xml", str(apk))
    for attribute, expected in [("icon", "mipmap/ic_launcher"), ("roundIcon", "mipmap/ic_launcher_round")]:
        match = re.search(r"android:" + attribute + r"\([^)]*\)=@(0x[0-9a-fA-F]+)", manifest)
        if not match or by_id.get(match[1].lower()) != expected:
            raise ValueError(f"Android manifest {attribute} does not resolve to NekoPak's launcher resource.")
    adaptive_checked = []
    for api in (26, 33):
        expected_layers = {"background", "foreground"} | ({"monochrome"} if api == 33 else set())
        for name in ("ic_launcher", "ic_launcher_round"):
            source_xml = source / f"mipmap-anydpi-v{api}/{name}.xml"
            xml = ET.parse(source_xml).getroot()
            if xml.tag != "adaptive-icon" or {child.tag for child in xml} != expected_layers:
                raise ValueError(f"Invalid committed API {api} adaptive icon descriptor.")
            entry = table.get(f"mipmap/{name}", {}).get("files", {}).get(f"anydpi-v{api}")
            if not entry or entry["type"] != "XML":
                raise ValueError(f"APK API {api} adaptive launcher is missing: {name}.")
            tree = run(aapt2, "dump", "xmltree", "--file", entry["path"], str(apk))
            layers, current = {}, None
            for line in tree.splitlines():
                element = re.search(r"E: ([\w-]+)", line)
                if element:
                    current = element[1]
                reference = re.search(r"android:drawable\([^)]*\)=@(0x[0-9a-fA-F]+)", line)
                if reference and current:
                    layers[current] = by_id.get(reference[1].lower())
            expected = {child.tag: child.attrib["{http://schemas.android.com/apk/res/android}drawable"].removeprefix("@") for child in xml}
            if layers != expected:
                raise ValueError(f"APK adaptive launcher layer graph differs from source: {name} API {api}.")
            adaptive_checked.append(f"{name}:api{api}")
    return {"brand": "NekoPak cat", "manifest_icon": "@mipmap/ic_launcher", "manifest_round_icon": "@mipmap/ic_launcher_round",
            "png_variants_verified": pixels_checked, "adaptive_variants_verified": adaptive_checked,
            "pixel_set_sha256": aggregate.hexdigest(), "verification": "Compiled APK resource IDs, adaptive layer graph and decoded RGBA pixels match committed brand resources."}


def apk_metadata(apk: Path, sdk: Path, abi: str, profile: str) -> dict:
    signature = run(str(sdk_tool(sdk, "apksigner")), "verify", "--verbose", "--print-certs", str(apk))
    certificate = re.findall(r"Signer #\d+ certificate SHA-256 digest:\s*([0-9a-fA-F:]+)", signature)
    if not certificate:
        raise ValueError("APK signature verification returned no signing certificate.")
    badging = run(str(sdk_tool(sdk, "aapt")), "dump", "badging", str(apk))
    is_debug = "application-debuggable" in badging
    if is_debug != (profile == "debug"):
        raise ValueError("APK debug/release profile does not match requested package label.")
    if profile == "release" and re.search(r"certificate DN:.*(?:CN=Android Debug|O=Android)", signature, re.I):
        raise ValueError("Refusing a debug signing identity for a release APK.")
    run(str(sdk_tool(sdk, "zipalign")), "-c", "-P", "16", "-v", "4", str(apk))
    package = re.search(r"package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'", badging)
    if not package or package[1] != "io.nekopak.desktop":
        raise ValueError("Unexpected Android application identifier.")
    minimum = re.search(r"sdkVersion:'(\d+)'", badging)
    target = re.search(r"targetSdkVersion:'(\d+)'", badging)
    if not minimum or int(minimum[1]) != 24 or not target or int(target[1]) != 36:
        raise ValueError("APK SDK requirements do not match the Android host baseline.")
    libraries = []
    with zipfile.ZipFile(apk) as archive:
        if archive.testzip() is not None:
            raise ValueError("APK ZIP CRC validation failed.")
        for item in archive.infolist():
            if item.filename.startswith("lib/") and item.filename.endswith(".so"):
                parts = item.filename.split("/")
                if len(parts) != 3 or parts[1] != abi:
                    raise ValueError("APK contains an unexpected or mixed native ABI.")
                data = archive.read(item)
                libraries.append({"path": item.filename, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), **elf_alignment(data)})
        if not libraries:
            raise ValueError("APK contains no native Rust library.")
        licenses_path = "assets/nekopak-licenses/licenses.zip"
        if licenses_path not in archive.namelist():
            raise ValueError("APK does not include its dependency license inventory.")
        license_bytes = archive.read(licenses_path)
    with zipfile.ZipFile(io.BytesIO(license_bytes)) as notices:
        for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "DEPENDENCY_LICENSES.json", "FRONTEND_LICENSES.json", "ANDROID_LICENSES.json", "LICENSE_INVENTORY.json"]:
            if name not in notices.namelist():
                raise ValueError(f"APK license inventory is missing {name}.")
        license_report = json.loads(notices.read("LICENSE_INVENTORY.json"))
        if license_report.get("license_text_gaps") != []:
            raise ValueError("APK license inventory has unresolved text gaps.")
    launcher = verify_launcher_icons(apk, sdk)
    return {"application_id": package[1], "version_code": package[2], "version": package[3], "launcher_icon": launcher,
            "min_sdk": int(minimum[1]), "target_sdk": int(target[1]), "debuggable": is_debug,
            "certificate_sha256": certificate, "signature_verified": True, "zip_alignment_16k": True,
            "native_libraries": libraries, "license_inventory": license_report,
            "license_archive_sha256": hashlib.sha256(license_bytes).hexdigest()}


def build_provenance(apk: Path, abi: str, profile: str, root: Path = ROOT) -> dict:
    metadata = json.loads(run("cargo", "metadata", "--no-deps", "--locked", "--format-version", "1", cwd=root))
    library = Path(metadata["target_directory"]) / TARGETS[abi] / profile / "libnekopak_app.so"
    if not library.is_file():
        raise ValueError("Matching Cargo native output is missing; build this APK in the current checkout first.")
    rust_inputs = [root / name for name in ["Cargo.toml", "Cargo.lock", "src-tauri/Cargo.toml", "src-tauri/build.rs", "src-tauri/tauri.conf.json", "src-tauri/tauri.android.conf.json"]]
    rust_inputs += [file for base in ["crates", "src-tauri/src", "src-tauri/icons", "ui-dist"] for file in (root / base).rglob("*") if file.is_file()]
    if any(file.stat().st_mtime_ns > library.stat().st_mtime_ns for file in rust_inputs):
        raise ValueError("Rust or embedded frontend inputs are newer than the native library; rebuild before packaging.")
    host_inputs = [file for file in (root / "src-tauri/android").rglob("*") if file.is_file()]
    if any(file.stat().st_mtime_ns > apk.stat().st_mtime_ns for file in host_inputs):
        raise ValueError("Android host sources are newer than the APK; rebuild before packaging.")
    if library.stat().st_mtime_ns > apk.stat().st_mtime_ns:
        raise ValueError("Native output is newer than the APK; rerun Android assembly before packaging.")
    source_hash = hashlib.sha256()
    for file in sorted(rust_inputs + host_inputs):
        source_hash.update(file.relative_to(root).as_posix().encode())
        source_hash.update(file.read_bytes())
    return {"source_sha256": source_hash.hexdigest(), "cargo_native_sha256": digest(library),
            "freshness_checked": True, "freshness_scope": "Native Rust/frontend build inputs and Android host inputs; no reproducible-build claim."}


def require_matching_native(result: dict, abi: str) -> None:
    expected = f"lib/{abi}/libnekopak_app.so"
    libraries = [library for library in result["native_libraries"] if library["path"] == expected]
    if len(libraries) != 1 or libraries[0]["sha256"] != result["cargo_native_sha256"]:
        raise ValueError("APK does not contain the freshly built NekoPak native library.")


def package(args: argparse.Namespace) -> dict:
    sdk = args.sdk or os.environ.get("ANDROID_HOME")
    if not sdk:
        raise ValueError("Set ANDROID_HOME or pass --sdk.")
    apk = args.apk.resolve() if args.apk else None
    if apk is None:
        candidates = sorted((ROOT / "src-tauri/gen/android/app/build/outputs/apk").glob(f"**/*-{args.profile}.apk"))
        candidates = [path for path in candidates if "androidTest" not in path.parts]
        if len(candidates) != 1:
            raise ValueError("Pass --apk: expected exactly one matching APK, refusing a stale or ambiguous selection.")
        apk = candidates[0]
    result = apk_metadata(apk, Path(sdk), args.abi, args.profile)
    result.update(build_provenance(apk, args.abi, args.profile))
    require_matching_native(result, args.abi)
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    if result["version"] != version:
        raise ValueError("APK version differs from package.json; rebuild before packaging.")
    output = ROOT / "dist/android"
    output.mkdir(parents=True, exist_ok=True)
    suffix = "-debug" if args.profile == "debug" else ""
    name = f"NekoPak-{version}-android-{args.abi}{suffix}"
    destination = output / f"{name}.apk"
    shutil.copyfile(apk, destination)
    with zipfile.ZipFile(apk) as archive:
        licenses_file = output / f"{name}-licenses.zip"
        licenses_file.write_bytes(archive.read("assets/nekopak-licenses/licenses.zip"))
    result.update({"schema_version": 1, "profile": args.profile, "abi": args.abi,
                   "artifact": destination.name, "bytes": destination.stat().st_size, "sha256": digest(destination),
                   "licenses_artifact": licenses_file.name, "commit": run("git", "rev-parse", "HEAD"),
                   "git_worktree_dirty": bool(run("git", "status", "--porcelain")),
                   "cargo_lock_sha256": digest(ROOT / "Cargo.lock"), "npm_lock_sha256": digest(ROOT / "package-lock.json"),
                   "rustc": run("rustc", "-V"), "source_apk": apk.relative_to(ROOT).as_posix() if apk.is_relative_to(ROOT) else apk.name,
                   "acceptance": "Build, package structure, licenses, signing and 16 KiB alignment only. Device, service and feature acceptance are recorded separately.",
                   "signing_scope": "Android APK certificate; distinct from Ed25519 release manifest and Windows Authenticode."})
    (output / f"{name}-report.json").write_bytes(json_text(result))
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    license_parser = commands.add_parser("licenses")
    license_parser.add_argument("--gradle-inventory", type=Path, required=True)
    license_parser.add_argument("--assets-dir", type=Path, required=True)
    bundle = commands.add_parser("package")
    bundle.add_argument("--apk", type=Path)
    bundle.add_argument("--sdk", type=Path)
    bundle.add_argument("--abi", choices=TARGETS, default="arm64-v8a")
    bundle.add_argument("--profile", choices=["debug", "release"], required=True)
    args = parser.parse_args()
    if args.command == "licenses":
        licenses(args.gradle_inventory, args.assets_dir)
    else:
        package(args)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1) from None
