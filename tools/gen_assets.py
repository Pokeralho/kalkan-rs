"""Gera os fundos de exemplo dos temas (requer Pillow).

    python tools/gen_assets.py

* assets/themes/cyberpunk/bg.gif      – grade synthwave animada (loop perfeito)
* assets/themes/minimal_stats/bg.png  – degradê escuro suave com vinheta
"""
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
W, H = 320, 240


def cyberpunk_gif(path: Path, frames: int = 30) -> None:
    horizon = 96
    random.seed(7)
    stars = [(random.randrange(W), random.randrange(horizon - 10), random.random()) for _ in range(60)]
    out = []
    for f in range(frames):
        phase = f / frames
        img = Image.new("RGB", (W, H))
        d = ImageDraw.Draw(img)
        # Céu em degradê
        for y in range(horizon):
            t = y / horizon
            d.line([(0, y), (W, y)], fill=(int(10 + 50 * t), int(4 + 6 * t), int(30 + 60 * t)))
        # Estrelas piscando
        for x, y, s in stars:
            b = int(120 + 120 * (0.5 + 0.5 * math.sin((phase + s) * 2 * math.pi)))
            d.point((x, y), fill=(b, b, min(255, b + 30)))
        # Sol listrado
        cx, cy, r = W // 2, horizon - 6, 38
        for y in range(cy - r, horizon):
            dy = y - cy
            if abs(dy) > r:
                continue
            band = (y - (cy - r)) / (2 * r)
            if y > cy - 6 and int((y + f * 0.4) / 4) % 2 == 0:
                continue
            half = int(math.sqrt(r * r - dy * dy))
            col = (255, int(200 - 150 * band), int(60 + 100 * band))
            d.line([(cx - half, y), (cx + half, y)], fill=col)
        # Chão
        d.rectangle([0, horizon, W, H], fill=(8, 0, 20))
        # Linhas horizontais em perspectiva (rolando = loop perfeito)
        for i in range(14):
            z = (i + phase) / 14
            y = horizon + int((H - horizon) * z * z)
            glow = int(80 + 175 * z)
            d.line([(0, y), (W, y)], fill=(glow, 0, int(glow * 0.9)), width=1)
        # Linhas de fuga
        for i in range(-10, 11):
            x_far = W / 2 + i * 10
            x_near = W / 2 + i * 70
            d.line([(x_far, horizon), (x_near, H)], fill=(200, 0, 180), width=1)
        d.line([(0, horizon), (W, horizon)], fill=(255, 80, 220), width=2)
        glow = img.filter(ImageFilter.GaussianBlur(3))
        img = Image.blend(img, glow, 0.35)
        out.append(img.convert("P", palette=Image.ADAPTIVE, colors=128))
    out[0].save(path, save_all=True, append_images=out[1:], duration=50, loop=0, optimize=True)


def minimal_png(path: Path) -> None:
    img = Image.new("RGB", (W, H))
    px = img.load()
    random.seed(3)
    for y in range(H):
        for x in range(W):
            t = (x / W) * 0.6 + (y / H) * 0.4
            dx, dy = (x - W * 0.3) / W, (y - H * 0.2) / H
            v = max(0.0, 1.0 - math.sqrt(dx * dx + dy * dy) * 1.4)
            n = random.uniform(-2, 2)
            r = 14 + 10 * v + 6 * t + n
            g = 16 + 14 * v + 4 * t + n
            b = 22 + 26 * v + 10 * t + n
            px[x, y] = (int(r), int(g), int(b))
    img.save(path)


if __name__ == "__main__":
    cyberpunk_gif(ROOT / "assets/themes/cyberpunk/bg.gif")
    minimal_png(ROOT / "assets/themes/minimal_stats/bg.png")
    print("assets gerados")
