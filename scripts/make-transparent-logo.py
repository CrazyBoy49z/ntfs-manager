#!/usr/bin/env python3
import struct
import sys
import zlib
from collections import deque

PNG = b"\x89PNG\r\n\x1a\n"

def read_chunks(data):
    if not data.startswith(PNG):
        raise SystemExit("not a PNG")
    pos = len(PNG)
    chunks = []
    while pos < len(data):
        length = struct.unpack(">I", data[pos:pos+4])[0]
        kind = data[pos+4:pos+8]
        payload = data[pos+8:pos+8+length]
        chunks.append((kind, payload))
        pos += 12 + length
        if kind == b"IEND":
            break
    return chunks

def paeth(a, b, c):
    p = a + b - c
    pa = abs(p - a)
    pb = abs(p - b)
    pc = abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c

def unfilter(raw, width, height, bpp):
    stride = width * bpp
    rows = []
    offset = 0
    previous = bytearray(stride)
    for _ in range(height):
        kind = raw[offset]
        offset += 1
        current = bytearray(raw[offset:offset+stride])
        offset += stride
        for i in range(stride):
            left = current[i-bpp] if i >= bpp else 0
            up = previous[i]
            up_left = previous[i-bpp] if i >= bpp else 0
            if kind == 1:
                current[i] = (current[i] + left) & 0xff
            elif kind == 2:
                current[i] = (current[i] + up) & 0xff
            elif kind == 3:
                current[i] = (current[i] + ((left + up) >> 1)) & 0xff
            elif kind == 4:
                current[i] = (current[i] + paeth(left, up, up_left)) & 0xff
            elif kind != 0:
                raise SystemExit(f"unsupported PNG filter {kind}")
        rows.append(current)
        previous = current
    return rows

def write_png(path, width, height, rows):
    def chunk(kind, payload):
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xffffffff)
        )
    scan = bytearray()
    for row in rows:
        scan.append(0)
        scan.extend(row)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    data = PNG + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(scan), 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as handle:
        handle.write(data)

def main(source, destination):
    chunks = read_chunks(open(source, "rb").read())
    ihdr = next(payload for kind, payload in chunks if kind == b"IHDR")
    width, height, depth, color_type, compression, filtering, interlace = struct.unpack(">IIBBBBB", ihdr)
    if depth != 8 or color_type not in (2, 3, 6) or compression or filtering or interlace:
        raise SystemExit("expected non-interlaced 8-bit RGB, RGBA, or indexed PNG")

    bpp = {2: 3, 3: 1, 6: 4}[color_type]
    compressed = b"".join(payload for kind, payload in chunks if kind == b"IDAT")
    rows = unfilter(zlib.decompress(compressed), width, height, bpp)

    palette = next((payload for kind, payload in chunks if kind == b"PLTE"), None)
    transparency = next((payload for kind, payload in chunks if kind == b"tRNS"), b"")

    rgba = [bytearray(width * 4) for _ in range(height)]
    for y, row in enumerate(rows):
        for x in range(width):
            src = x * bpp
            dst = x * 4

            if color_type == 3:
                if palette is None:
                    raise SystemExit("indexed PNG is missing PLTE")
                index = row[src]
                p = index * 3
                rgba[y][dst:dst+3] = palette[p:p+3]
                rgba[y][dst+3] = transparency[index] if index < len(transparency) else 255
            else:
                rgba[y][dst:dst+3] = row[src:src+3]
                rgba[y][dst+3] = row[src+3] if bpp == 4 else 255

    def white(x, y):
        i = x * 4
        r, g, b, a = rgba[y][i:i+4]
        return a > 0 and r >= 238 and g >= 238 and b >= 238

    queue = deque()
    seen = set()
    for x in range(width):
        for y in (0, height - 1):
            if white(x, y):
                queue.append((x, y))
                seen.add((x, y))
    for y in range(height):
        for x in (0, width - 1):
            if white(x, y) and (x, y) not in seen:
                queue.append((x, y))
                seen.add((x, y))

    while queue:
        x, y = queue.popleft()
        i = x * 4
        rgba[y][i+3] = 0
        for nx, ny in ((x-1,y),(x+1,y),(x,y-1),(x,y+1)):
            if 0 <= nx < width and 0 <= ny < height and (nx, ny) not in seen and white(nx, ny):
                seen.add((nx, ny))
                queue.append((nx, ny))

    write_png(destination, width, height, rgba)

if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: make-transparent-logo.py input.png output.png")
    main(sys.argv[1], sys.argv[2])
