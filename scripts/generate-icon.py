#!/usr/bin/env python3
import math
import struct
import sys
import zlib

SIZE = 1024

FONT = {
    "N": ["10001","11001","10101","10011","10001","10001","10001"],
    "T": ["11111","00100","00100","00100","00100","00100","00100"],
    "F": ["11111","10000","10000","11110","10000","10000","10000"],
    "S": ["01111","10000","10000","01110","00001","00001","11110"],
}

def clamp(value, low=0.0, high=1.0):
    return max(low, min(high, value))

def smoothstep(edge0, edge1, value):
    if edge0 == edge1:
        return 1.0 if value >= edge1 else 0.0
    t = clamp((value - edge0) / (edge1 - edge0))
    return t * t * (3.0 - 2.0 * t)

def rounded_rect_coverage(x, y, x0, y0, x1, y1, radius, softness=2.0):
    cx = (x0 + x1) * 0.5
    cy = (y0 + y1) * 0.5
    hx = (x1 - x0) * 0.5 - radius
    hy = (y1 - y0) * 0.5 - radius
    qx = abs(x - cx) - hx
    qy = abs(y - cy) - hy
    ox = max(qx, 0.0)
    oy = max(qy, 0.0)
    distance = math.hypot(ox, oy) + min(max(qx, qy), 0.0) - radius
    return 1.0 - smoothstep(-softness, softness, distance)

def circle_coverage(x, y, cx, cy, radius, softness=2.0):
    distance = math.hypot(x - cx, y - cy) - radius
    return 1.0 - smoothstep(-softness, softness, distance)

def trapezoid_coverage(x, y, top_y, bottom_y, top_left, top_right, bottom_left, bottom_right):
    if y < top_y - 3 or y > bottom_y + 3:
        return 0.0
    t = clamp((y - top_y) / (bottom_y - top_y))
    left = top_left + (bottom_left - top_left) * t
    right = top_right + (bottom_right - top_right) * t
    edge = min(x - left, right - x, y - top_y, bottom_y - y)
    return smoothstep(-2.0, 2.0, edge)

def blend(dst, src, alpha):
    sa = clamp(alpha * (src[3] / 255.0))
    da = dst[3] / 255.0
    out_a = sa + da * (1.0 - sa)
    if out_a <= 0.0:
        return (0, 0, 0, 0)
    out = []
    for index in range(3):
        value = (src[index] * sa + dst[index] * da * (1.0 - sa)) / out_a
        out.append(int(clamp(value / 255.0) * 255.0 + 0.5))
    out.append(int(out_a * 255.0 + 0.5))
    return tuple(out)

def set_pixel(pixels, x, y, color, coverage=1.0):
    if x < 0 or y < 0 or x >= SIZE or y >= SIZE or coverage <= 0.0:
        return
    offset = (y * SIZE + x) * 4
    dst = tuple(pixels[offset:offset + 4])
    pixels[offset:offset + 4] = bytes(blend(dst, color, coverage))

def write_png(path, pixels):
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)
        start = y * SIZE * 4
        raw.extend(pixels[start:start + SIZE * 4])

    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    header = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    payload = (
        header
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as handle:
        handle.write(payload)

def draw_text(pixels, text, x0, y0, scale, color):
    cursor = x0
    for char in text:
        glyph = FONT[char]
        for gy, row in enumerate(glyph):
            for gx, bit in enumerate(row):
                if bit != "1":
                    continue
                x_start = cursor + gx * scale
                y_start = y0 + gy * scale
                for y in range(y_start, y_start + scale):
                    for x in range(x_start, x_start + scale):
                        set_pixel(pixels, x, y, color)
        cursor += 6 * scale

def render():
    pixels = bytearray(SIZE * SIZE * 4)

    # Rounded macOS icon background: blue highlight into graphite.
    for y in range(SIZE):
        for x in range(SIZE):
            coverage = rounded_rect_coverage(x, y, 68, 68, 956, 956, 205, 2.5)
            if coverage <= 0.0:
                continue
            nx = x / SIZE
            ny = y / SIZE
            glow = math.exp(-((nx - 0.28) ** 2 + (ny - 0.18) ** 2) / 0.10)
            edge = clamp((ny - 0.05) / 0.95)
            color = (
                int(10 + 18 * glow + 2 * edge),
                int(24 + 70 * glow + 5 * edge),
                int(38 + 125 * glow + 8 * edge),
                255,
            )
            set_pixel(pixels, x, y, color, coverage)

    # Inner highlight.
    for y in range(90, 940):
        for x in range(90, 940):
            outer = rounded_rect_coverage(x, y, 88, 88, 936, 936, 184, 2.0)
            inner = rounded_rect_coverage(x, y, 94, 94, 930, 930, 178, 2.0)
            ring = clamp(outer - inner)
            set_pixel(pixels, x, y, (135, 205, 255, 120), ring * 0.55)

    # External-drive body.
    for y in range(205, 760):
        for x in range(150, 875):
            coverage = trapezoid_coverage(x, y, 220, 728, 320, 704, 218, 806)
            if coverage <= 0.0:
                continue
            t = clamp((y - 220) / 508)
            highlight = clamp(1.0 - abs((x - 512) / 360.0))
            base = 218 - int(t * 45)
            color = (
                min(255, base + int(highlight * 32)),
                min(255, base + int(highlight * 35)),
                min(255, base + int(highlight * 38)),
                255,
            )
            set_pixel(pixels, x, y, color, coverage)

    # Top bevel.
    for y in range(220, 254):
        for x in range(305, 720):
            coverage = rounded_rect_coverage(x, y, 304, 218, 720, 256, 17, 1.5)
            set_pixel(pixels, x, y, (246, 249, 252, 210), coverage)

    # NTFS badge.
    for y in range(420, 575):
        for x in range(278, 748):
            coverage = rounded_rect_coverage(x, y, 280, 424, 744, 570, 37, 2.0)
            if coverage <= 0.0:
                continue
            gradient = clamp((y - 424) / 146)
            shade = int(55 - gradient * 17)
            set_pixel(pixels, x, y, (shade, shade + 6, shade + 12, 245), coverage)

    scale = 24
    text_width = 4 * 6 * scale - scale
    draw_text(
        pixels,
        "NTFS",
        int((SIZE - text_width) / 2),
        435,
        scale,
        (245, 248, 252, 255),
    )

    # Bottom panel.
    for y in range(615, 730):
        for x in range(225, 800):
            coverage = rounded_rect_coverage(x, y, 228, 620, 796, 724, 28, 2.0)
            set_pixel(pixels, x, y, (83, 91, 99, 238), coverage)

    # Slot.
    for y in range(651, 688):
        for x in range(355, 670):
            coverage = rounded_rect_coverage(x, y, 354, 650, 670, 690, 18, 1.5)
            set_pixel(pixels, x, y, (12, 20, 27, 255), coverage)

    # Blue + green activity LEDs.
    for cx, color in [
        (292, (44, 158, 255, 255)),
        (730, (70, 222, 112, 255)),
    ]:
        for y in range(640, 704):
            for x in range(cx - 34, cx + 34):
                glow = circle_coverage(x, y, cx, 672, 30, 3.0)
                core = circle_coverage(x, y, cx, 672, 15, 1.5)
                set_pixel(pixels, x, y, color, glow * 0.20 + core * 0.80)

    return pixels

if __name__ == "__main__":
    output = sys.argv[1] if len(sys.argv) > 1 else "NTFSManager.png"
    write_png(output, render())
