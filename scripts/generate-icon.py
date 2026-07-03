#!/usr/bin/env python3
"""
Generate multi-resolution Windows .ico from the project's qvod.png.

Usage:
    python3 scripts/generate-icon.py

Output:
    assets/qvod.ico  — 6 resolutions (16×16 … 256×256) as PNG-in-ICO
"""
import struct, io, os, sys

try:
    from PIL import Image
except ImportError:
    sys.exit("Requires Pillow: pip install Pillow")

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(PROJECT_ROOT, "qvod.png")

# Primary location: qvs-gui crate (used by build.rs → windres)
DST_CRATE = os.path.join(PROJECT_ROOT, "crates", "qvs-gui", "assets", "qvod.ico")
# Secondary location: project root assets (used by packaging scripts)
DST_ROOT = os.path.join(PROJECT_ROOT, "assets", "qvod.ico")
DSTS = [DST_CRATE, DST_ROOT]

if not os.path.isfile(SRC):
    sys.exit(f"Source PNG not found: {SRC}")

img = Image.open(SRC)
print(f"Source: {img.size}, mode={img.mode}")

if img.mode != "RGBA":
    img = img.convert("RGBA")

SIZES = [16, 32, 48, 64, 128, 256]

# Resize and collect PNG-encoded image data
png_chunks = []
for s in SIZES:
    resized = img.resize((s, s), Image.LANCZOS)
    buf = io.BytesIO()
    resized.save(buf, format="PNG")
    png_chunks.append(buf.getvalue())
    print(f"  {s:3}×{s:<3}  {len(png_chunks[-1]):>6} bytes")

# Build ICO
entries = b""
data = b""
offset = 6 + len(SIZES) * 16  # header + directory

for i, s in enumerate(SIZES):
    w = 0 if s == 256 else s
    h = 0 if s == 256 else s
    entry = struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(png_chunks[i]), offset)
    entries += entry
    data += png_chunks[i]
    offset += len(png_chunks[i])

for dst in DSTS:
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    with open(dst, "wb") as f:
        f.write(struct.pack("<HHH", 0, 1, len(SIZES)))
        f.write(entries)
        f.write(data)
    print(f"Written: {dst}  ({os.path.getsize(dst):,} bytes, {len(SIZES)} entries)")
