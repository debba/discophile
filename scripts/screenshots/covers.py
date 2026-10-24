"""Original covers for the demo library, in the styles of the iOS promo covers. Each base is a soft mesh of
the album's palette instead of a blurred real cover. Usage: covers.py OUTPUT_DIR"""
import json, math, pathlib, random, shutil, sys
import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageEnhance, ImageFilter, ImageFont

HERE = pathlib.Path(__file__).resolve().parent
FONTS = HERE.parents[1] / ".screenshots/fonts"
S = 1200


def font(name, size, weight=None):
    f = ImageFont.truetype(str(FONTS / name), size)
    if weight:
        try:
            f.set_variation_by_axes([weight] if "Inter" not in name else [32, weight])
        except Exception:
            pass
    return f


def rgb(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))
def lum(c): return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
def ink_for(bg): return (16, 16, 18) if lum(bg) > 150 else (250, 248, 244)


def grain(im, amount, seed):
    n = np.random.default_rng(seed).normal(0, amount, (S, S, 1))
    return Image.fromarray(np.clip(np.asarray(im, dtype=float) + n, 0, 255).astype("uint8"))


def mesh(palette, seed, radius=0.09, sat=1.25):
    """Large soft blobs of the palette over its first colour, blurred like an out-of-focus photo."""
    rnd = random.Random(seed)
    colors = [rgb(c) for c in palette]
    im = Image.new("RGB", (S, S), colors[0]); d = ImageDraw.Draw(im)
    for i in range(9):
        c = colors[rnd.randrange(len(colors))]
        r = S * rnd.uniform(.18, .42); x, y = S * rnd.uniform(0, 1), S * rnd.uniform(0, 1)
        d.ellipse([x - r, y - r, x + r, y + r], fill=c)
    im = im.filter(ImageFilter.GaussianBlur(S * radius)).filter(ImageFilter.GaussianBlur(S * radius / 2))
    return ImageEnhance.Color(im).enhance(sat)


def text_center(d, xy, s, f, fill): d.text(xy, s, font=f, fill=fill, anchor="mm")


def fit(name, s, width, start, weight=None):
    size = start
    while size > 20:
        f = font(name, size, weight)
        if f.getbbox(s)[2] - f.getbbox(s)[0] <= width:
            return f
        size -= 6
    return font(name, 20, weight)


def album_cover(artist, title, style, palette, seed):
    rnd = random.Random(seed); dom, acc = rgb(palette[0]), rgb(palette[1])
    if style == "mesh":
        im = mesh(palette, seed); d = ImageDraw.Draw(im)
        avg = tuple(int(v) for v in np.asarray(im.crop((0, int(S * .8), S, S))).reshape(-1, 3).mean(0)); ink = ink_for(avg)
        d.text((72, S - 184), title, font=fit("InstrumentSerif-Italic.ttf", title, S - 144, 152), fill=ink, anchor="ls")
        d.text((77, S - 96), artist.upper(), font=font("Inter.ttf", 35, 600), fill=ink, anchor="ls")
        return grain(im, 10, seed)
    if style == "block":
        im = Image.new("RGB", (S, S), acc); d = ImageDraw.Draw(im); ink = ink_for(acc)
        layer = Image.new("RGBA", (S, S)); ld = ImageDraw.Draw(layer)
        text_center(ld, (S / 2, S / 2), title, fit("ArchivoBlack-Regular.ttf", title, S * .62, 240), ink)
        soft = layer.filter(ImageFilter.GaussianBlur(3)); im.paste(soft, (0, 0), soft)
        text_center(d, (S / 2, S - 88), artist.lower(), font("Inter.ttf", 32, 500), ink)
        return grain(im, 6, seed)
    if style == "orb":
        base = tuple(int(c * .35) for c in dom) if lum(dom) > 60 else dom
        im = Image.new("RGB", (S, S), base)
        glow = Image.new("RGB", (S, S), (0, 0, 0)); g = ImageDraw.Draw(glow)
        cx, cy, r = S * (.5 + rnd.uniform(-.08, .08)), S * .42, S * .27
        g.ellipse([cx - r, cy - r, cx + r, cy + r], fill=acc)
        im = ImageChops.add(im, glow.filter(ImageFilter.GaussianBlur(S * .08)))
        im = ImageChops.add(im, glow.filter(ImageFilter.GaussianBlur(S * .012)).point(lambda v: v * .55))
        d = ImageDraw.Draw(im)
        text_center(d, (S / 2, S * .8), title, fit("DMSerifDisplay-Regular.ttf", title, S * .8, 136), (250, 248, 244))
        text_center(d, (S / 2, S * .9), artist.upper(), font("Inter.ttf", 30, 500), (250, 248, 244))
        return grain(im, 12, seed)
    if style == "horizon":
        top, low = rgb(palette[2]), dom
        y = np.linspace(0, 1, S)[:, None, None]; line = .62
        arr = np.where(y < line, np.array(top) * (1 - y * .25) + np.array(acc) * y * .25, np.array(low) * (.85 + (1 - y) * .3))
        im = Image.fromarray(np.clip(np.broadcast_to(arr, (S, S, 3)), 0, 255).astype("uint8")); d = ImageDraw.Draw(im)
        sun = Image.new("L", (S, S)); ImageDraw.Draw(sun).ellipse([S * .39, S * .4, S * .61, S * .62], fill=255)
        sun = ImageChops.multiply(sun, Image.fromarray(((np.arange(S)[:, None] < S * line) * 255).astype("uint8")).resize((S, S)))
        im.paste(Image.new("RGB", (S, S), tuple(min(255, int(c * 1.15 + 30)) for c in acc)), (0, 0), sun.filter(ImageFilter.GaussianBlur(2)))
        d.line([(0, S * line), (S, S * line)], fill=(255, 255, 255), width=3)
        if title:
            ink = ink_for(top)
            d.text((72, 120), title, font=fit("Unbounded.ttf", title, S - 144, 96, 700), fill=ink, anchor="ls")
            d.text((74, 172), artist, font=font("Inter.ttf", 35, 500), fill=ink, anchor="ls")
        return grain(im, 9, seed)
    if style == "frame":
        bg = dom; im = Image.new("RGB", (S, S), bg); d = ImageDraw.Draw(im)
        win = mesh(palette[1:] + palette[:1], seed, .05, 1.4).crop((S * .2, S * .1, S * .8, S * .7)).resize((int(S * .46), int(S * .56)))
        x0, y0 = int(S * .27), int(S * .14)
        d.rectangle([x0 - 21, y0 - 21, x0 + win.width + 21, y0 + win.height + 21], fill=tuple(max(0, c - 35) for c in bg))
        im.paste(win, (x0, y0)); d.line([(x0 + win.width / 2, y0), (x0 + win.width / 2, y0 + win.height)], fill=bg, width=11)
        ink = ink_for(bg)
        text_center(d, (S / 2, S * .84), title, fit("InstrumentSerif-Regular.ttf", title, S * .8, 120), ink)
        text_center(d, (S / 2, S * .92), artist.upper(), font("Inter.ttf", 27, 500), ink)
        return grain(im, 8, seed)
    if style == "mono":
        im = ImageEnhance.Contrast(mesh(["#9a9a9a", "#1c1c1c", "#e6e6e6"], seed, .07, 0).convert("L")).enhance(1.6).convert("RGB")
        d = ImageDraw.Draw(im); ink = ink_for(im.getpixel((S // 2, S // 2)))
        text_center(d, (S / 2, S / 2), title, fit("BebasNeue-Regular.ttf", title, S * .86, 368), ink)
        label = ink_for(tuple(int(v) for v in np.asarray(im.crop((72, S - 160, 560, S - 104))).reshape(-1, 3).mean(0)))
        d.rectangle([72, S - 136, 72 + 18, S - 118], fill=acc)
        d.text((104, S - 118), artist.upper(), font=font("Inter.ttf", 30, 600), fill=label, anchor="ls")
        return grain(im, 18, seed)
    raise ValueError(style)


def book_cover(author, title, narrator, style, palette, seed):
    dom, acc = rgb(palette[0]), rgb(palette[1]); words = title.split()
    half = math.ceil(len(words) / 2)
    lines = [title] if len(title) < 12 else [" ".join(words[:half]), " ".join(words[half:])]
    if style == "emblem":
        bg = tuple(int(c * .45) for c in dom) if lum(dom) > 70 else dom
        im = Image.new("RGB", (S, S), bg); d = ImageDraw.Draw(im); cx, cy = S / 2, S * .46
        for i, r in enumerate((264, 216, 160)):
            d.ellipse([cx - r, cy - r, cx + r, cy + r], outline=acc, width=8 if i == 0 else 3)
        kind = seed % 5
        if kind == 0:
            d.polygon([(cx, cy - 136), (cx + 120, cy + 88), (cx - 120, cy + 88)], outline=acc, width=6)
            d.ellipse([cx - 27, cy + 5, cx + 27, cy + 59], fill=acc)
        elif kind == 1:
            pts = [(cx + math.cos(math.pi / 2 + i * math.pi / 5) * (128 if i % 2 == 0 else 51),
                    cy - math.sin(math.pi / 2 + i * math.pi / 5) * (128 if i % 2 == 0 else 51)) for i in range(10)]
            d.polygon(pts, fill=acc)
        elif kind == 2:
            for i in range(3):
                d.line([(cx - 120, cy - 48 + i * 56), (cx, cy + 32 + i * 56), (cx + 120, cy - 48 + i * 56)], fill=acc, width=10, joint="curve")
        elif kind == 3:
            d.pieslice([cx - 96, cy - 96, cx + 96, cy + 96], 200, 340, fill=acc); d.rectangle([cx - 6, cy - 32, cx + 6, cy + 120], fill=acc)
        else:
            for i in range(3):
                for j in range(3):
                    d.rounded_rectangle([cx - 108 + i * 76, cy - 108 + j * 76, cx - 52 + i * 76, cy - 52 + j * 76], 11,
                                        fill=acc if (i + j) % 2 == 0 else None, outline=acc, width=4)
        text_center(d, (cx, S * .12), author.upper(), font("Inter.ttf", 37, 700), acc)
        f = fit("DMSerifDisplay-Regular.ttf", max(lines, key=len), S * .84, 136)
        for i, l in enumerate(lines):
            text_center(d, (cx, S * .8 + i * 120 - (len(lines) - 1) * 40), l.upper(), f, (250, 246, 236))
        text_center(d, (cx, S * .955), f"READ BY {narrator.upper()}", font("Inter.ttf", 22, 500), acc)
        return grain(im, 10, seed)
    if style == "horizon":
        im = album_cover("", "", "horizon", [palette[0], palette[1], palette[1]], seed); d = ImageDraw.Draw(im)
        ink = ink_for(im.getpixel((S // 2, 48)))
        f = fit("DMSerifDisplay-Italic.ttf", max(lines, key=len), S * .84, 152)
        for i, l in enumerate(lines):
            text_center(d, (S / 2, S * .13 + i * 132), l, f, ink)
        text_center(d, (S / 2, S * .9), author.upper(), font("Inter.ttf", 40, 700), (250, 248, 244))
        return im
    if style == "type":
        bg = dom if lum(dom) > 150 else (246, 242, 234)
        im = Image.new("RGB", (S, S), bg); d = ImageDraw.Draw(im)
        d.rectangle([0, 0, S, S * .06], fill=acc); d.rectangle([0, S * .94, S, S], fill=acc)
        ink = (24, 22, 20); f = fit("InstrumentSerif-Regular.ttf", max(lines, key=len), S * .82, 240)
        for i, l in enumerate(lines):
            text_center(d, (S / 2, S * .42 + i * 208 - (len(lines) - 1) * 104), l, f, ink)
        d.ellipse([S / 2 - 18, S * .66 - 18, S / 2 + 18, S * .66 + 18], fill=acc)
        text_center(d, (S / 2, S * .77), author, font("InstrumentSerif-Italic.ttf", 74), ink)
        text_center(d, (S / 2, S * .86), f"Read by {narrator}", font("Inter.ttf", 26, 500), (90, 86, 80))
        return grain(im, 5, seed)
    raise ValueError(style)


if __name__ == "__main__":
    out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
    catalog = json.load(open(HERE / "catalog.json"))
    for n, a in enumerate(catalog["albums"]):
        if a["style"] == "photo":
            shutil.copy(HERE / f"{a['key']}.jpg", out / f"{a['key']}.jpg")
        else:
            album_cover(a["artist"], a["title"], a["style"], a["palette"], n).save(out / f"{a['key']}.jpg", quality=92)
    for n, b in enumerate(catalog["books"]):
        book_cover(b["author"], b["title"], b["narrator"], b["style"], b["palette"], 100 + n).save(out / f"{b['key']}.jpg", quality=92)
    keys = [a["key"] for a in catalog["albums"]] + [b["key"] for b in catalog["books"]]
    cols, T = 8, 240
    sheet = Image.new("RGB", (cols * T, -(-len(keys) // cols) * T), "white")
    for i, k in enumerate(keys):
        sheet.paste(Image.open(out / f"{k}.jpg").resize((T - 8, T - 8)), (i % cols * T + 4, i // cols * T + 4))
    sheet.save(out.parent / "cover-sheet.jpg", quality=85)
    print(len(keys), "covers")
