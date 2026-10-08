#!/usr/bin/env python3
"""Render the Space Hunter vector mark to PNG, using only the standard library."""
import os
import struct
import sys
import zlib


def png(path, size, maskable=False):
    def pixel(x, y):
        # The maskable icon keeps the entire mark inside the central safe area.
        if maskable:
            x, y = (x - 8) * 64 / 48, (y - 8) * 64 / 48
        cx, cy = min(max(x, 16), 48), min(max(y, 16), 48)
        if (x-cx)**2 + (y-cy)**2 > 16**2:
            return (111, 65, 199, 255 if maskable else 0)
        if (16 <= x < 39 and 16 <= y < 26) or (16 <= x < 26 and 26 <= y < 32) or (28 <= x < 48 and 29 <= y < 35) or (38 <= x < 48 and 35 <= y < 48) or (25 <= x < 38 and 38 <= y < 48):
            return (241, 238, 230, 255)
        if (43.5 <= x <= 53.5 and 10.5 <= y <= 13.5) or (50.5 <= x <= 53.5 and 10.5 <= y <= 20.5):
            return (217, 182, 111, 255)
        return (111, 65, 199, 255)

    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            samples = [pixel((x+(i+.5)/4)*64/size, (y+(j+.5)/4)*64/size) for j in range(4) for i in range(4)]
            row.extend(round(sum(p[c] for p in samples)/16) for c in range(4))
        rows.append(bytes(row))

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))

    data = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(b''.join(rows), 9)) + chunk(b'IEND', b'')
    with open(path, 'wb') as output:
        output.write(data)


out = sys.argv[1] if len(sys.argv) > 1 else 'web/icons'
os.makedirs(out, exist_ok=True)
for name, size in [('icon-192', 192), ('icon-512', 512), ('maskable-512', 512), ('favicon-32', 32), ('app-256', 256)]:
    png(f'{out}/{name}.png', size, name.startswith('maskable'))
print('icons written to', out)
