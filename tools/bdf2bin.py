#!/usr/bin/env python3
"""Converts a 6x12 cell BDF font (e.g. Terminus ter-u12n.bdf) to libreamp's glyph format:
repeated records of a little-endian u32 code point followed by 12 row bytes (MSB = leftmost)."""
import struct, sys

src, dst = sys.argv[1], sys.argv[2]
out = bytearray()
enc, rows, in_bitmap = None, [], False
for line in open(src, encoding="latin-1"):
    parts = line.split()
    if not parts:
        continue
    if parts[0] == "ENCODING":
        enc = int(parts[1])
    elif parts[0] == "BBX" and parts[1:] != ["6", "12", "0", "-2"]:
        sys.exit(f"unexpected glyph box {parts[1:]} for U+{enc:04X}")
    elif parts[0] == "BITMAP":
        in_bitmap, rows = True, []
    elif parts[0] == "ENDCHAR":
        in_bitmap = False
        if enc is not None and enc >= 0 and len(rows) == 12:
            out += struct.pack("<I", enc) + bytes(rows)
    elif in_bitmap:
        rows.append(int(parts[0], 16))
open(dst, "wb").write(out)
print(f"{len(out) // 16} glyphs")
