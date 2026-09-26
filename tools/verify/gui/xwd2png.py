"""xwd -> PNG: python3 xwd2png.py captura.xwd salida.png (requiere Pillow)."""
import struct
import sys

from PIL import Image

d = open(sys.argv[1], "rb").read()
h = struct.unpack(">25I", d[:100])
hsize, w, hgt, byte_order, bpp, bpl, ncolors = h[0], h[4], h[5], h[7], h[11], h[12], h[19]
off = hsize + ncolors * 12
raw = d[off:off + bpl * hgt]
mode = "BGRX" if byte_order == 0 else "XRGB"
img = Image.frombuffer("RGB", (w, hgt), raw, "raw", mode, bpl, 1)
img.save(sys.argv[2])
print(w, hgt, bpp)
