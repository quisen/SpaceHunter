#!/usr/bin/env python3
"""Generates the SpaceHunter icons (PNG, no dependencies): a small treemap tile."""
import struct, zlib, sys, os

def png(path, size, maskable=False):
    s = size
    pal = [(0xff,0x7f,0x7f),(0xff,0xbf,0x7f),(0xff,0xe0,0x3a),(0x7f,0xe8,0x7f),(0x7f,0xe0,0xf0),(0xaf,0xaf,0xff),(0xe8,0x7f,0xe8)]
    # treemap-like partition of the unit square: (x0,y0,x1,y1,colour)
    tiles = [(0,0,.58,.62,0),(.58,0,1,.38,5),(.58,.38,1,.62,4),(0,.62,.34,1,1),(.34,.62,.7,1,2),(.7,.62,1,.81,3),(.7,.81,1,1,6)]
    pad = .14 if maskable else .0
    bg = (0x12,0x16,0x20)
    rows = []
    for y in range(s):
        row = bytearray([0])
        for x in range(s):
            u, v = x / s, y / s
            px = None
            if maskable:
                u, v = (u - pad) / (1 - 2*pad), (v - pad) / (1 - 2*pad)
            if 0 <= u < 1 and 0 <= v < 1:
                for (a,b,c,d,ci) in tiles:
                    g = 0.012
                    if a+g <= u < c-g and b+g <= v < d-g:
                        base = pal[ci]
                        # bevel: light top-left, dark bottom-right
                        t = ((u-a)/(c-a) + (v-b)/(d-b)) / 2
                        k = 1.12 - 0.34 * t
                        px = tuple(min(255, int(ch * k)) for ch in base)
                        break
            if px is None:
                # rounded square background (transparent corners when not maskable)
                if not maskable:
                    r = .18
                    cx, cy = min(max(x/s, r), 1-r), min(max(y/s, r), 1-r)
                    inside = ((x/s-cx)**2 + (y/s-cy)**2) <= r*r
                    row += bytes(bg + (255 if inside else 0,))
                    continue
                px = bg
            row += bytes(px + (255,))
        rows.append(bytes(row))
    def ch(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d))
    data = b'\x89PNG\r\n\x1a\n' + ch(b'IHDR', struct.pack('>IIBBBBB', s, s, 8, 6, 0, 0, 0)) + ch(b'IDAT', zlib.compress(b''.join(rows), 9)) + ch(b'IEND', b'')
    open(path, 'wb').write(data)

out = sys.argv[1] if len(sys.argv) > 1 else 'web/icons'
os.makedirs(out, exist_ok=True)
png(f'{out}/icon-192.png', 192)
png(f'{out}/icon-512.png', 512)
png(f'{out}/maskable-512.png', 512, True)
png(f'{out}/favicon-32.png', 32)
png(f'{out}/app-256.png', 256)  # source for the Windows .ico
print('icons written to', out)
