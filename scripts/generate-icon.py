#!/usr/bin/env python3
"""
Generate multi-resolution Windows .ico from the project's qvod.png.

The source PNG (1254×1254 RGB) has a white background.  This script
strips the background via an edge-seeded flood-fill, then writes a
properly transparent ICO at 6 resolutions (16×16 … 256×256).

Usage:
    python3 scripts/generate-icon.py

Output:
    crates/qvs-gui/assets/qvod.ico  — 6 resolutions as PNG-in-ICO
    assets/qvod.ico                  — copy for packaging
"""
import struct, io, os, sys

try:
    from PIL import Image
except ImportError:
    sys.exit("Requires Pillow: pip install Pillow")

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(PROJECT_ROOT, "qvod.png")

DST_CRATE = os.path.join(PROJECT_ROOT, "crates", "qvs-gui", "assets", "qvod.ico")
DST_ROOT = os.path.join(PROJECT_ROOT, "assets", "qvod.ico")
DSTS = [DST_CRATE, DST_ROOT]

if not os.path.isfile(SRC):
    sys.exit(f"Source PNG not found: {SRC}")

# ---------------------------------------------------------------------------
# 1. Load source, ensure RGBA
# ---------------------------------------------------------------------------
img = Image.open(SRC)
print(f"Source: {img.size}, mode={img.mode}")

if img.mode != "RGBA":
    img = img.convert("RGBA")

# ---------------------------------------------------------------------------
# 2. Strip white background via edge-seeded flood-fill
# ---------------------------------------------------------------------------
# The icon is a red circle + white play triangle centred on a white canvas.
# We flood-fill from the four edges with a transparent colour.  The fill
# stops at non-white pixels (the red circle), so the white play triangle
# inside the circle is preserved.

def strip_white_bg(image: Image.Image, threshold: int = 40) -> Image.Image:
    """Make all white-ish background pixels transparent via flood-fill."""
    w, h = image.size
    rgba = image.load()

    # Collect all white-ish edge pixels as seeds
    seeds = []
    for x in range(w):
        for y in (0, h - 1):
            r, g, b, a = rgba[x, y]
            if r > 255 - threshold and g > 255 - threshold and b > 255 - threshold and a > 0:
                seeds.append((x, y))
    for y in range(1, h - 1):
        for x in (0, w - 1):
            r, g, b, a = rgba[x, y]
            if r > 255 - threshold and g > 255 - threshold and b > 255 - threshold and a > 0:
                seeds.append((x, y))

    if not seeds:
        print("  No white edge pixels found — skipping background removal")
        return image

    # BFS flood-fill
    visited = bytearray(w * h)
    stack = seeds
    for sx, sy in seeds:
        visited[sy * w + sx] = 1

    while stack:
        cx, cy = stack.pop()
        for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            nx, ny = cx + dx, cy + dy
            if 0 <= nx < w and 0 <= ny < h and not visited[ny * w + nx]:
                r, g, b, a = rgba[nx, ny]
                if r > 255 - threshold and g > 255 - threshold and b > 255 - threshold and a > 0:
                    visited[ny * w + nx] = 1
                    stack.append((nx, ny))

    # Set visited (background) pixels to fully transparent
    count = 0
    for y in range(h):
        for x in range(w):
            if visited[y * w + x]:
                rgba[x, y] = (0, 0, 0, 0)
                count += 1

    print(f"  Stripped {count} background pixels ({(count / (w * h)) * 100:.1f}%)")
    return image

img = strip_white_bg(img)
if img.mode != "RGBA":
    img = img.convert("RGBA")

# ---------------------------------------------------------------------------
# 3. Resize and collect PNG-encoded chunks
# ---------------------------------------------------------------------------
SIZES = [16, 32, 48, 64, 128, 256]
png_chunks = []
for s in SIZES:
    resized = img.resize((s, s), Image.LANCZOS)
    buf = io.BytesIO()
    resized.save(buf, format="PNG")
    png_chunks.append(buf.getvalue())
    print(f"  {s:3}×{s:<3}  {len(png_chunks[-1]):>6} bytes")

# ---------------------------------------------------------------------------
# 4. Build ICO
# ---------------------------------------------------------------------------
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
