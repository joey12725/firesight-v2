#!/usr/bin/env python3
"""Shrinks the README screenshots in docs/images to under about 600 KB each.

Each PNG is reduced to an adaptive palette (the app draws flat colour and one smooth
ramp, which 256 colours with dithering hold well); if that is still too large it is
scaled down in steps. Needs Pillow. Run after `node e2e/docs-shots.mjs`.
"""
import os
import sys
from PIL import Image

LIMIT = 600_000
over = False
folder = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'docs', 'images')
for name in sorted(os.listdir(folder)):
    if not name.endswith('.png'):
        continue
    path = os.path.join(folder, name)
    original = Image.open(path).convert('RGB')
    before = os.path.getsize(path)
    scale = 1.0
    while True:
        image = original if scale == 1.0 else original.resize((round(original.width * scale), round(original.height * scale)), Image.LANCZOS)
        image.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG).save(path, optimize=True)
        if os.path.getsize(path) <= LIMIT or scale <= 0.5:
            break
        scale -= 0.1
    print(f'{name}: {before // 1024} KB -> {os.path.getsize(path) // 1024} KB at {image.width}x{image.height}' + ('' if scale == 1.0 else f' (scaled to {scale:.0%})'))
    over = over or os.path.getsize(path) > LIMIT
sys.exit(1 if over else 0)
