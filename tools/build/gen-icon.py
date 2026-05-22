#!/usr/bin/env python3
"""Regenerate launcher/assets/icon.png and icon.ico from a procedural drawing.

Pure-stdlib (no Pillow, no ImageMagick) so CI runners and dev boxes do not
need extra system packages installed. Draws the same shapes as icon.svg.

Usage:
    python3 tools/build/gen-icon.py

Edits to icon.svg are independent — re-run this script after changing the
geometry constants below if the SVG and the rasters drift.
"""
from __future__ import annotations

import io
import math
import struct
import zlib
from pathlib import Path

# Palette (RGB).
BG          = (0x1A, 0x22, 0x38)  # dark navy
RING_WHITE  = (0xFF, 0xFF, 0xFF)
BLAST_INNER = (0xFF, 0xE0, 0x82)  # warm cream
BLAST_MID   = (0xFF, 0xAA, 0x00)  # orange
BLAST_OUTER = (0xFF, 0x6F, 0x00)  # dark orange
HILITE      = (0xFF, 0xFF, 0xFF)
TEXT_FG     = BG                  # "BB" body matches background — outlined white

# Geometry expressed in 512-px reference frame; scaled per output size.
REF = 512
BG_CORNER_R     = 72
RING_R          = 180
RING_THICK      = 10
RING_ALPHA      = 0.85
BLAST_R         = 150
HILITE_CX       = 220
HILITE_CY       = 220
HILITE_R        = 38
HILITE_ALPHA    = 0.28


def lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    t = max(0.0, min(1.0, t))
    return (
        int(round(a[0] + (b[0] - a[0]) * t)),
        int(round(a[1] + (b[1] - a[1]) * t)),
        int(round(a[2] + (b[2] - a[2]) * t)),
    )


def blend(bg: tuple[int, int, int], fg: tuple[int, int, int], alpha: float) -> tuple[int, int, int]:
    alpha = max(0.0, min(1.0, alpha))
    return (
        int(round(bg[0] * (1 - alpha) + fg[0] * alpha)),
        int(round(bg[1] * (1 - alpha) + fg[1] * alpha)),
        int(round(bg[2] * (1 - alpha) + fg[2] * alpha)),
    )


# Hand-pixeled 8x10 "B" glyph. 1 = ink, 0 = bg. Drawn outlined.
GLYPH_B = [
    "11111110",
    "11111111",
    "11000011",
    "11000011",
    "11111110",
    "11111110",
    "11000011",
    "11000011",
    "11111111",
    "11111110",
]


def render(size: int) -> bytes:
    """Return raw RGBA bytes (size*size*4) for an icon of the requested size."""
    scale = size / REF

    cx = cy = size / 2
    bg_r = BG_CORNER_R * scale
    ring_r = RING_R * scale
    ring_thick = RING_THICK * scale
    blast_r = BLAST_R * scale
    hilite_cx = HILITE_CX * scale
    hilite_cy = HILITE_CY * scale
    hilite_r = HILITE_R * scale

    # "BB" geometry — two glyphs centered horizontally.
    glyph_w_px = 8
    glyph_h_px = 10
    glyph_scale = (size * 0.22) / glyph_h_px  # ~22% of icon height
    bb_gap = glyph_w_px * glyph_scale * 0.25
    bb_total_w = 2 * glyph_w_px * glyph_scale + bb_gap
    bb_origin_x = (size - bb_total_w) / 2
    bb_origin_y = size * 0.52   # baseline-ish
    outline_px = max(1.0, scale * 4)

    px = bytearray(size * size * 4)
    pixel_at = lambda x, y: ((y * size + x) * 4)

    for y in range(size):
        for x in range(size):
            # Rounded-square background mask.
            inside_x = (bg_r <= x <= size - bg_r) or (bg_r <= y <= size - bg_r)
            if not inside_x:
                # corner — distance to corner center
                corner_cx = bg_r if x < bg_r else size - bg_r
                corner_cy = bg_r if y < bg_r else size - bg_r
                d = math.hypot(x - corner_cx, y - corner_cy)
                if d > bg_r:
                    # transparent corner
                    off = pixel_at(x, y)
                    px[off:off+4] = bytes((0, 0, 0, 0))
                    continue
            color = BG

            # Radial distance from icon center.
            dr = math.hypot(x - cx, y - cy)

            # Blast circle with radial gradient.
            if dr < blast_r:
                t = dr / blast_r
                # 0 -> inner, 0.55 -> mid, 1.0 -> outer
                if t < 0.55:
                    color = lerp(BLAST_INNER, BLAST_MID, t / 0.55)
                else:
                    color = lerp(BLAST_MID, BLAST_OUTER, (t - 0.55) / 0.45)

            # White accent ring.
            ring_outer = ring_r + ring_thick / 2
            ring_inner = ring_r - ring_thick / 2
            if ring_inner <= dr <= ring_outer:
                color = blend(color, RING_WHITE, RING_ALPHA)

            # Highlight blob.
            dh = math.hypot(x - hilite_cx, y - hilite_cy)
            if dh <= hilite_r:
                # smooth falloff
                a = HILITE_ALPHA * (1 - dh / hilite_r) ** 0.6
                color = blend(color, HILITE, a)

            # "BB" overlay — outlined white-stroke, navy-filled glyphs.
            for gi in range(2):
                gx0 = bb_origin_x + gi * (glyph_w_px * glyph_scale + bb_gap)
                gy0 = bb_origin_y
                if gx0 <= x < gx0 + glyph_w_px * glyph_scale and gy0 <= y < gy0 + glyph_h_px * glyph_scale:
                    gx_idx = int((x - gx0) / glyph_scale)
                    gy_idx = int((y - gy0) / glyph_scale)
                    if 0 <= gx_idx < glyph_w_px and 0 <= gy_idx < glyph_h_px:
                        if GLYPH_B[gy_idx][gx_idx] == "1":
                            color = TEXT_FG
                        else:
                            # outline detection: any neighboring ink pixel within outline_px?
                            ox_min = max(0, gx_idx - 1)
                            ox_max = min(glyph_w_px - 1, gx_idx + 1)
                            oy_min = max(0, gy_idx - 1)
                            oy_max = min(glyph_h_px - 1, gy_idx + 1)
                            near_ink = any(
                                GLYPH_B[oy][ox] == "1"
                                for oy in range(oy_min, oy_max + 1)
                                for ox in range(ox_min, ox_max + 1)
                            )
                            if near_ink:
                                color = RING_WHITE

            off = pixel_at(x, y)
            px[off:off+4] = bytes((color[0], color[1], color[2], 255))

    return bytes(px)


def make_png(rgba: bytes, size: int) -> bytes:
    """Encode an RGBA pixel buffer as a PNG."""
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit, RGBA
    # Each scanline prefixed with filter byte (0 = none).
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        raw.extend(rgba[y * size * 4:(y + 1) * size * 4])
    idat = zlib.compress(bytes(raw), 9)
    return sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def make_ico_with_pngs(pngs: list[tuple[int, bytes]]) -> bytes:
    """Pack PNG-encoded entries into an .ico container.
    PNG-in-ICO is supported on Vista+ which covers every supported Windows."""
    out = bytearray()
    # ICONDIR: reserved=0, type=1 (ico), count=N
    out += struct.pack("<HHH", 0, 1, len(pngs))
    # Reserve directory entries first; write image data after.
    dir_offset = len(out) + 16 * len(pngs)
    image_blob = bytearray()
    for (size, data) in pngs:
        # ICONDIRENTRY
        w = 0 if size >= 256 else size
        h = 0 if size >= 256 else size
        out += struct.pack(
            "<BBBBHHII",
            w, h,
            0,                     # color count (0 = ≥ 256)
            0,                     # reserved
            1,                     # planes
            32,                    # bit count
            len(data),             # bytes in resource
            dir_offset + len(image_blob),
        )
        image_blob += data
    out += image_blob
    return bytes(out)


def main() -> None:
    repo_root = Path(__file__).resolve().parents[2]
    out_dir = repo_root / "launcher" / "assets"
    out_dir.mkdir(parents=True, exist_ok=True)

    # PNG: single 512×512 for AppImage / .deb hicolor.
    png512 = make_png(render(512), 512)
    (out_dir / "icon.png").write_bytes(png512)
    print(f"wrote {out_dir / 'icon.png'} ({len(png512):,} bytes)")

    # ICO: 32 + 48 + 256 PNG-in-ICO entries for NSIS.
    entries = [
        (32,  make_png(render(32),  32)),
        (48,  make_png(render(48),  48)),
        (256, make_png(render(256), 256)),
    ]
    ico_bytes = make_ico_with_pngs(entries)
    (out_dir / "icon.ico").write_bytes(ico_bytes)
    print(f"wrote {out_dir / 'icon.ico'} ({len(ico_bytes):,} bytes, {len(entries)} sizes)")


if __name__ == "__main__":
    main()
