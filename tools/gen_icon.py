#!/usr/bin/env python3
"""Generate the yyproject icons using the common YY style (see NOTICE.md).
Requires Pillow. Run: python tools/gen_icon.py
"""
import io
import struct
from pathlib import Path
from PIL import Image, ImageDraw
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SCALE = 4
WHITE = (255, 255, 255, 255)


def draw_y(d, x, y, w, h, t, a):
    """左上 (x, y)、幅 w・高さ h、線の太さ t、腕の上端の幅 a の「Y」（上端が水平な多角形）。"""
    cx = x + w / 2
    mid = y + h * 0.5
    d.polygon([(x, y), (x + a, y), (cx + t / 2, mid), (cx - t / 2, mid)], fill=WHITE)
    d.polygon([(x + w - a, y), (x + w, y), (cx + t / 2, mid), (cx - t / 2, mid)], fill=WHITE)
    # 腕の合流点から下へ
    d.polygon(
        [(cx - t / 2, mid - t * 0.6), (cx + t / 2, mid - t * 0.6), (cx + t / 2, y + h), (cx - t / 2, y + h)],
        fill=WHITE,
    )


def draw_symbol(d, x, y, w, h, t, ot):
    for i, length in enumerate((0.80, 0.55, 0.68)):
        yy = y + h * i / 3
        d.rectangle([x, yy, x + ot * 2, yy + ot * 2], outline=WHITE, width=max(1, round(ot / 2)))
        d.rectangle([x + w * .22, yy, x + w * length, yy + ot * 2], fill=WHITE)


def render(size):
    top_c, bottom_c = ((0x64, 0x74, 0x8B), (0x1E, 0x29, 0x3B))
    n = size * SCALE
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    # 縦のグラデーション
    grad = Image.new("RGBA", (n, n))
    gd = ImageDraw.Draw(grad)
    for yy in range(n):
        f = yy / max(n - 1, 1)
        c = tuple(round(top_c[i] + (bottom_c[i] - top_c[i]) * f) for i in range(3))
        gd.line([(0, yy), (n, yy)], fill=c + (255,))
    mask = Image.new("L", (n, n), 0)
    margin = n * (0.03 if size >= 32 else 0.0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [margin, margin, n - 1 - margin, n - 1 - margin], radius=n * 0.2, fill=255
    )
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    # どのサイズでも「YY」の下にアプリの記号を描く（色だけの違いにしない）。小さいサイズは YY を
    # 小さめにして、記号を太く大きく描く（16 px でも形で見分けられるように）
    if size < 40:
        t, a = n * 0.11, n * 0.1
        top, h = n * 0.08, n * 0.4
        w, gap = n * 0.34, n * 0.05
        box = (n * 0.14, n * 0.56, n * 0.72, n * 0.36)
        st = n * 0.12
        ot = n * 0.075
    else:
        t, a = n * 0.1, n * 0.12
        top, h = n * 0.14, n * 0.44
        w, gap = n * 0.3, n * 0.06
        box = (n * 0.22, n * 0.65, n * 0.56, n * 0.22)
        st = n * 0.055
        ot = n * 0.03
    x0 = (n - (w * 2 + gap)) / 2
    draw_y(d, x0, top, w, h, t, a)
    draw_y(d, x0 + w + gap, top, w, h, t, a)
    draw_symbol(d, *box, st, ot)
    return img.resize((size, size), Image.LANCZOS)


def dib(img):
    """アイコン用の 32 ビット DIB（上下反転した BGRA と AND マスク）。"""
    w, h = img.size
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    px = img.load()
    rows = []
    for y in reversed(range(h)):
        rows.append(b"".join(bytes((b, g, r, a)) for r, g, b, a in (px[x, y] for x in range(w))))
    mask_row = ((w + 31) // 32) * 4
    return header + b"".join(rows) + bytes(mask_row * h)


def write_ico(path, images):
    """ICO を書く。256 は PNG、それ以外は BMP（古い読み込み処理でも表示できるように）。"""
    blobs = []
    for img in images:
        if img.size[0] >= 256:
            buf = io.BytesIO()
            img.save(buf, format="PNG", optimize=True)
            blobs.append(buf.getvalue())
        else:
            blobs.append(dib(img))
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for img, blob in zip(images, blobs):
        w = img.size[0]
        out += struct.pack("<BBBBHHII", w % 256, w % 256, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    path.write_bytes(out + b"".join(blobs))


def main():
    res = Path(__file__).resolve().parents[1] / "app/res"
    res.mkdir(parents=True, exist_ok=True)
    images = [render(size) for size in SIZES]
    write_ico(res / "yyproject.ico", images)
    images[-1].save(res / "yyproject-256.png")


if __name__ == "__main__":
    main()
