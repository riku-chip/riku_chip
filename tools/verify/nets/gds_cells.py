#!/usr/bin/env python3
"""Nombres de todas las celdas de un GDS (registros STRNAME), en una línea."""
import struct
import sys

data = open(sys.argv[1], "rb").read()
i, names = 0, []
while i + 4 <= len(data):
    n, t = struct.unpack(">HB", data[i : i + 3])
    if n < 4:
        break
    if t == 0x06:
        names.append(data[i + 4 : i + n].rstrip(b"\0").decode("latin-1"))
    i += n
print(" ".join(names))
