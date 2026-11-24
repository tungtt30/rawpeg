#!/usr/bin/env python3
"""Generate the rawpeg app icon assets.

Outputs (all committed, so CI never needs to run this):
  assets/icon-source.png   1024x1024 master
  assets/rawpeg-256.png    runtime window icon
  assets/rawpeg.ico        Windows executable icon (PNG-compressed entries)
  assets/rawpeg.icns       macOS bundle icon

macOS only: uses the built-in `sips` and `iconutil` tools.
No third-party Python packages required.
"""

import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(ROOT, "assets")
SIZE = 1024


def clamp01(value):
    return max(0.0, min(1.0, value))


def blend(buf, index, color, alpha):
    if alpha <= 0.0:
        return
    if alpha >= 1.0:
        buf[index] = color[0]
        buf[index + 1] = color[1]
        buf[index + 2] = color[2]
        buf[index + 3] = 255
        return
    inv = 1.0 - alpha
    buf[index] = int(buf[index] * inv + color[0] * alpha)
    buf[index + 1] = int(buf[index + 1] * inv + color[1] * alpha)
    buf[index + 2] = int(buf[index + 2] * inv + color[2] * alpha)
    buf[index + 3] = int(buf[index + 3] * inv + 255 * alpha)


def fill_rounded_rect(buf, size, radius, top, bottom):
    center = size / 2.0
    half = size / 2.0
    inner = half - radius
    for y in range(size):
        t = y / (size - 1)
        base_r = int(top[0] + (bottom[0] - top[0]) * t)
        base_g = int(top[1] + (bottom[1] - top[1]) * t)
        base_b = int(top[2] + (bottom[2] - top[2]) * t)
        dy = abs(y + 0.5 - center) - inner
        dy = dy if dy > 0.0 else 0.0
        row = y * size * 4
        for x in range(size):
            dx = abs(x + 0.5 - center) - inner
            dx = dx if dx > 0.0 else 0.0
            dist = (dx * dx + dy * dy) ** 0.5 - radius
            coverage = clamp01(0.5 - dist)
            if coverage > 0.0:
                blend(buf, row + x * 4, (base_r, base_g, base_b), coverage)


def fill_circle(buf, size, cx, cy, radius, color):
    x0 = max(0, int(cx - radius - 2))
    x1 = min(size - 1, int(cx + radius + 2))
    y0 = max(0, int(cy - radius - 2))
    y1 = min(size - 1, int(cy + radius + 2))
    for y in range(y0, y1 + 1):
        row = y * size * 4
        for x in range(x0, x1 + 1):
            dx = x + 0.5 - cx
            dy = y + 0.5 - cy
            dist = (dx * dx + dy * dy) ** 0.5
            coverage = clamp01(radius + 0.75 - dist)
            if coverage > 0.0:
                blend(buf, row + x * 4, color, coverage)


def write_png(path, size, rgba):
    def chunk(kind, data):
        crc = zlib.crc32(kind + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)

    stride = size * 4
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        raw.extend(rgba[y * stride : (y + 1) * stride])
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    payload = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as handle:
        handle.write(payload)


def write_ico(path, entries):
    # entries: list of (size, png_bytes); Windows Vista+ accepts PNG payloads.
    offset = 6 + 16 * len(entries)
    directory = bytearray(struct.pack("<HHH", 0, 1, len(entries)))
    payload = bytearray()
    for size, blob in entries:
        dimension = 0 if size >= 256 else size
        directory.extend(
            struct.pack(
                "<BBBBHHII",
                dimension,
                dimension,
                0,
                0,
                1,
                32,
                len(blob),
                offset,
            )
        )
        payload.extend(blob)
        offset += len(blob)
    with open(path, "wb") as handle:
        handle.write(directory + payload)


def sips_resize(source, destination, size):
    subprocess.run(
        ["sips", "-z", str(size), str(size), source, "--out", destination],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def main():
    if sys.platform != "darwin":
        sys.exit("make_icons.py requires macOS (sips + iconutil)")
    if not shutil.which("sips") or not shutil.which("iconutil"):
        sys.exit("make_icons.py requires sips and iconutil in PATH")

    os.makedirs(ASSETS, exist_ok=True)
    size = SIZE
    buf = bytearray(size * size * 4)

    fill_rounded_rect(buf, size, 210.0, (38, 42, 56), (14, 15, 21))
    fill_circle(buf, size, 512.0, 512.0, 356.0, (63, 68, 84))
    fill_circle(buf, size, 512.0, 512.0, 334.0, (23, 24, 30))
    fill_circle(buf, size, 512.0, 512.0, 292.0, (236, 230, 217))
    fill_circle(buf, size, 512.0, 512.0, 252.0, (23, 24, 30))
    fill_circle(buf, size, 512.0, 512.0, 202.0, (227, 154, 75))
    fill_circle(buf, size, 512.0, 512.0, 154.0, (17, 18, 22))
    fill_circle(buf, size, 398.0, 380.0, 38.0, (255, 255, 255))

    master = os.path.join(ASSETS, "icon-source.png")
    write_png(master, size, buf)
    print("wrote", os.path.relpath(master, ROOT))

    with tempfile.TemporaryDirectory(prefix="rawpeg-icons-") as tmp:
        sizes = [16, 32, 48, 64, 128, 256]
        pngs = {}
        for icon_size in sizes:
            target = os.path.join(tmp, "icon-%d.png" % icon_size)
            sips_resize(master, target, icon_size)
            with open(target, "rb") as handle:
                pngs[icon_size] = handle.read()

        runtime = os.path.join(ASSETS, "rawpeg-256.png")
        shutil.copyfile(os.path.join(tmp, "icon-256.png"), runtime)
        print("wrote", os.path.relpath(runtime, ROOT))

        ico = os.path.join(ASSETS, "rawpeg.ico")
        write_ico(ico, [(icon_size, pngs[icon_size]) for icon_size in sizes])
        print("wrote", os.path.relpath(ico, ROOT))

        iconset = os.path.join(tmp, "rawpeg.iconset")
        os.makedirs(iconset)
        iconset_map = {
            "icon_16x16.png": 16,
            "icon_16x16@2x.png": 32,
            "icon_32x32.png": 32,
            "icon_32x32@2x.png": 64,
            "icon_128x128.png": 128,
            "icon_128x128@2x.png": 256,
            "icon_256x256.png": 256,
            "icon_256x256@2x.png": 512,
            "icon_512x512.png": 512,
            "icon_512x512@2x.png": 1024,
        }
        for name, icon_size in iconset_map.items():
            source = master if icon_size == size else os.path.join(tmp, "icon-%d.png" % icon_size)
            if icon_size not in pngs and icon_size != size:
                sips_resize(master, source, icon_size)
            shutil.copyfile(source, os.path.join(iconset, name))

        icns = os.path.join(ASSETS, "rawpeg.icns")
        subprocess.run(
            ["iconutil", "-c", "icns", iconset, "-o", icns],
            check=True,
        )
        print("wrote", os.path.relpath(icns, ROOT))


if __name__ == "__main__":
    main()
