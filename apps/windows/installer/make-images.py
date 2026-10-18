# 生成 Inno Setup 安装向导图片（品牌色：深色 + 紫色，图标取自 Qantrip/Assets/Qantrip.ico）。
#
#   uv run --with pillow python apps/windows/installer/make-images.py
#
# 输出到本目录 images/：wizard-<宽>.png（欢迎 / 完成页左侧，164:314 比例，对应 100% / 150% / 200% / 250% 缩放）
# 与 small-<边长>.png（其余页面右上角，透明背景）。尺寸取自 Inno Setup 6.6+ 文档中各缩放比例下的图片区域。
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

here = Path(__file__).resolve().parent
assets = here.parent / "Qantrip" / "Assets"
out = here / "images"
out.mkdir(exist_ok=True)

ico = Image.open(assets / "Qantrip.ico")
ico.size = max(ico.info["sizes"])
icon = ico.convert("RGBA")

TOP = (24, 16, 44)        # 近黑的深紫
BOTTOM = (59, 28, 120)    # 紫
GLOW = (124, 58, 237)     # 品牌紫（Theme.Accent 浅色）
TEXT = (242, 242, 247)
SUB = (196, 181, 253)


def gradient(w, h):
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        c = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3))
        for x in range(w):
            px[x, y] = c
    return img


def wizard(w, h):
    s = w / 202  # 以 100% 的 202x386 为基准
    img = gradient(w, h).convert("RGBA")
    # 图标后的柔光
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(glow)
    cx, cy, r = w / 2, h * 0.36, 70 * s
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=GLOW + (150,))
    glow = glow.filter(ImageFilter.GaussianBlur(34 * s))
    img = Image.alpha_composite(img, glow)
    # 细小的星点
    d = ImageDraw.Draw(img)
    for i, (fx, fy, fr) in enumerate([(0.18, 0.12, 1.4), (0.82, 0.2, 1.1), (0.7, 0.07, 0.9), (0.1, 0.7, 1.0), (0.9, 0.76, 1.3), (0.25, 0.86, 0.9), (0.78, 0.9, 1.1)]):
        rr = fr * s
        d.ellipse((fx * w - rr, fy * h - rr, fx * w + rr, fy * h + rr), fill=(255, 255, 255, 150 if i % 2 else 200))
    # 图标
    size = round(104 * s)
    ic = icon.resize((size, size), Image.LANCZOS)
    img.alpha_composite(ic, (round(cx - size / 2), round(cy - size / 2)))
    # 文字
    title = ImageFont.truetype(str(assets / "Fonts" / "FrauncesDisplay.ttf"), round(24 * s))
    mono = ImageFont.truetype(str(assets / "Fonts" / "MonaspaceNeonVar.ttf"), round(10.5 * s))
    d = ImageDraw.Draw(img)
    ty = cy + size / 2 + 26 * s
    d.text((cx, ty), "Nekro Qantrip", font=title, fill=TEXT, anchor="mt")
    d.text((cx, ty + 36 * s), "Press Q to cast a spell", font=mono, fill=SUB, anchor="mt")
    return img.convert("RGB")


for w, h in [(202, 386), (336, 643), (430, 824), (534, 1022)]:
    wizard(w, h).save(out / f"wizard-{w}.png", optimize=True)

for n in [58, 97, 124, 159]:
    canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    pad = round(n * 0.04)
    ic = icon.resize((n - 2 * pad, n - 2 * pad), Image.LANCZOS)
    canvas.alpha_composite(ic, (pad, pad))
    canvas.save(out / f"small-{n}.png", optimize=True)

print("written to", out)
