#!/usr/bin/env python3
"""Draws the app icon as a 1024x1024 PNG with the standard library only.

Design: a macOS-style rounded square with a blue-to-indigo gradient, and two white
speech bubbles that overlap (people and bots talk as equals).

Usage: make-icon.py <out.png> [size]
"""

import math
import struct
import sys
import zlib


def rounded_rect_sdf(px, py, cx, cy, hw, hh, r):
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    ox = max(qx, 0.0)
    oy = max(qy, 0.0)
    return math.hypot(ox, oy) + min(max(qx, qy), 0.0) - r


def circle_sdf(px, py, cx, cy, r):
    return math.hypot(px - cx, py - cy) - r


def triangle_sdf(px, py, a, b, c):
    # Signed distance to a triangle (negative inside).
    def edge(p0, p1):
        ex, ey = p1[0] - p0[0], p1[1] - p0[1]
        wx, wy = px - p0[0], py - p0[1]
        t = max(0.0, min(1.0, (wx * ex + wy * ey) / (ex * ex + ey * ey)))
        dx, dy = wx - ex * t, wy - ey * t
        return dx * dx + dy * dy, ex * wy - ey * wx

    d1, s1 = edge(a, b)
    d2, s2 = edge(b, c)
    d3, s3 = edge(c, a)
    d = math.sqrt(min(d1, d2, d3))
    inside = (s1 >= 0 and s2 >= 0 and s3 >= 0) or (s1 <= 0 and s2 <= 0 and s3 <= 0)
    return -d if inside else d


def coverage(d, scale):
    return max(0.0, min(1.0, 0.5 - d / scale))


def lerp(a, b, t):
    return a + (b - a) * t


def render(size):
    s = size / 1024.0
    top = (64, 156, 255)
    bottom = (88, 86, 214)
    rows = []
    for y in range(size):
        row = bytearray([0])  # PNG filter type: none
        fy = (y + 0.5) / s
        for x in range(size):
            fx = (x + 0.5) / s
            # Background tile on Apple's icon grid: 824 x 824 at (100, 100), radius 185.
            d_bg = rounded_rect_sdf(fx, fy, 512, 512, 412, 412, 185)
            a_bg = coverage(d_bg, 1 / s)
            if a_bg <= 0:
                row += b"\x00\x00\x00\x00"
                continue
            t = (fy - 100) / 824
            col = [lerp(top[i], bottom[i], max(0.0, min(1.0, t))) for i in range(3)]

            # Back bubble (lighter, upper right) with a tail on the right.
            d_b2 = min(
                rounded_rect_sdf(fx, fy, 590, 430, 210, 150, 110),
                triangle_sdf(fx, fy, (690, 520), (780, 650), (600, 560)),
            )
            a_b2 = coverage(d_b2, 1 / s) * 0.55
            col = [lerp(c, 255, a_b2) for c in col]

            # Front bubble (white, lower left) with a tail on the left, plus an outline gap.
            d_b1 = min(
                rounded_rect_sdf(fx, fy, 440, 590, 230, 160, 120),
                triangle_sdf(fx, fy, (300, 680), (230, 820), (400, 720)),
            )
            a_gap = coverage(d_b1 - 22, 1 / s) * a_b2 / 0.55
            col = [lerp(col[i], lerp(top[i], bottom[i], max(0.0, min(1.0, t))), a_gap) for i in range(3)]
            a_b1 = coverage(d_b1, 1 / s)
            col = [lerp(c, 255, a_b1) for c in col]

            # Three dots in the front bubble.
            d_dots = min(circle_sdf(fx, fy, cx, 590, 34) for cx in (340, 440, 540))
            a_dot = coverage(d_dots, 1 / s) * a_b1
            col = [lerp(col[i], lerp(top[i], bottom[i], 0.6), a_dot) for i in range(3)]

            row += bytes([round(col[0]), round(col[1]), round(col[2]), round(255 * a_bg)])
        rows.append(bytes(row))
    return b"".join(rows)


def write_png(path, size, raw):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: make-icon.py <out.png> [size]")
    size = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
    write_png(sys.argv[1], size, render(size))


if __name__ == "__main__":
    main()
