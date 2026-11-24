"""Render the branded NSIS installer bitmaps (monochrome + single red accent).

    python scripts/installer-art.py

Outputs into apps/desktop/src-tauri/installer/:
  sidebar.bmp  164x314  welcome / finish pages (left panel)
  header.bmp   150x57   top-right of the inner pages
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "apps" / "desktop" / "src-tauri" / "installer"
OUT.mkdir(parents=True, exist_ok=True)

BG = (10, 10, 10)
WHITE = (242, 242, 242)
GREY = (120, 120, 120)
LINE = (34, 34, 34)
RED = (255, 59, 48)
S = 4  # supersampling


def font(size, bold=False):
    for name in (["segoeuib.ttf", "arialbd.ttf"] if bold else ["segoeui.ttf", "arial.ttf"]):
        try:
            return ImageFont.truetype(name, size * S)
        except OSError:
            continue
    return ImageFont.load_default()


def mark(d, x, y, size, fg):
    """The Velox V with its red drop, drawn on a 24-unit grid."""
    k = size * S / 24
    pts = [(3, 3), (7.1, 3), (12, 12.4), (16.9, 3), (21, 3), (12, 18.6)]
    d.polygon([(x * S + px * k, y * S + py * k) for px, py in pts], fill=fg)
    d.rectangle([x * S + 10.6 * k, y * S + 19.6 * k, x * S + 13.4 * k, y * S + 22.4 * k], fill=RED)


def sidebar():
    w, h = 164, 314
    im = Image.new("RGB", (w * S, h * S), BG)
    d = ImageDraw.Draw(im)
    # faint speed lines
    for i, yy in enumerate((206, 214, 222)):
        d.line([(24 * S, yy * S), ((24 + 70 - i * 18) * S, yy * S)], fill=LINE, width=S)
    mark(d, 22, 40, 44, WHITE)
    d.text((24 * S, 104 * S), "Velox", font=font(26, True), fill=WHITE)
    d.text((25 * S, 140 * S), "Fast downloads.", font=font(10), fill=GREY)
    d.text((25 * S, 155 * S), "Built in Rust.", font=font(10), fill=GREY)
    d.rectangle([24 * S, 282 * S, 30 * S, 288 * S], fill=RED)
    d.text((38 * S, 278 * S), "open source", font=font(9), fill=GREY)
    im.resize((w, h), Image.LANCZOS).save(OUT / "sidebar.bmp")


def header():
    w, h = 150, 57
    im = Image.new("RGB", (w * S, h * S), (255, 255, 255))
    d = ImageDraw.Draw(im)
    mark(d, 82, 16, 24, (12, 12, 12))
    d.text((106 * S, 18 * S), "Velox", font=font(14, True), fill=(12, 12, 12))
    im.resize((w, h), Image.LANCZOS).save(OUT / "header.bmp")


sidebar()
header()
print("installer art written to", OUT)
