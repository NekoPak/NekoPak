#!/usr/bin/env python3
"""Headless smoke test for the native desktop client.

Runs the automation build against the offline mock backend under Xvfb with
both renderers and checks that every scripted screenshot was produced and
actually contains a rendered UI (not a blank or single-colour frame).

    cargo build -p nekopak-desktop --features automation
    python3 tests/native/run.py [--keep DIR]
"""
import argparse
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).with_name("smoke.txt")


def distinct_colours(path: Path) -> int:
    """Decodes an 8-bit RGBA PNG (as written by the app) and counts sampled colours."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    position, width, height, chunks = 8, 0, 0, []
    while position < len(data):
        length, kind = struct.unpack(">I4s", data[position:position + 8])
        body = data[position + 8:position + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", body[:10])
            if (depth, colour) != (8, 6):
                raise ValueError("unexpected pixel format")
        elif kind == b"IDAT":
            chunks.append(body)
        position += 12 + length
    raw = zlib.decompress(b"".join(chunks))
    stride = width * 4 + 1
    colours = set()
    previous = bytearray(width * 4)
    for row in range(height):
        line = bytearray(raw[row * stride + 1:(row + 1) * stride])
        mode = raw[row * stride]
        for i in range(len(line)):
            left = line[i - 4] if i >= 4 else 0
            up = previous[i]
            upper_left = previous[i - 4] if i >= 4 else 0
            if mode == 1:
                line[i] = (line[i] + left) & 255
            elif mode == 2:
                line[i] = (line[i] + up) & 255
            elif mode == 3:
                line[i] = (line[i] + (left + up) // 2) & 255
            elif mode == 4:
                p = left + up - upper_left
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - upper_left)
                line[i] = (line[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else upper_left)) & 255
        previous = line
        if row % 8 == 0:
            for x in range(0, width * 4, 32):
                colours.add(bytes(line[x:x + 3]))
    return len(colours)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep", help="copy screenshots into this directory")
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "nekopak"))
    options = parser.parse_args()
    expected = [line.split()[1] for line in SCRIPT.read_text(encoding="utf-8").splitlines() if line.startswith("shot ")]
    failures = []
    for renderer in ("winit-software", "winit-femtovg"):
        with tempfile.TemporaryDirectory(prefix="nekopak-ui-") as work:
            work = Path(work)
            shutil.copy(SCRIPT, work / "smoke.txt")
            environment = dict(
                os.environ,
                NEKOPAK_BACKEND="mock",
                NEKOPAK_TEST_DATA_DIR=str(work / "data"),
                NEKOPAK_UI_SCRIPT=str(work / "smoke.txt"),
                SLINT_BACKEND=renderer,
                LANG="zh_CN.UTF-8",
            )
            (work / "data").mkdir()
            command = ["xvfb-run", "-a", "-s", "-screen 0 1600x1000x24", options.binary]
            try:
                result = subprocess.run(command, env=environment, timeout=120, capture_output=True, text=True)
            except subprocess.TimeoutExpired:
                failures.append(f"{renderer}: did not finish within 120 s")
                continue
            if result.returncode != 0:
                failures.append(f"{renderer}: exit code {result.returncode}: {result.stderr.strip()[-400:]}")
            for name in expected:
                shot = work / name
                if not shot.is_file():
                    failures.append(f"{renderer}: missing {name}")
                    continue
                colours = distinct_colours(shot)
                if colours < 12:
                    failures.append(f"{renderer}: {name} looks blank ({colours} colours)")
                if options.keep:
                    target = Path(options.keep) / renderer
                    target.mkdir(parents=True, exist_ok=True)
                    shutil.copy(shot, target / name)
            print(f"{renderer}: {len(expected)} screenshots checked")
    for failure in failures:
        print("FAIL", failure, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
