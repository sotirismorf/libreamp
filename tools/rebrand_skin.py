#!/usr/bin/env python3
"""Turns Winamp's classic base skin into LibreAmp's default skin.

Replaces the "WINAMP" lettering in the title bars of titlebar.bmp (normal and shade mode),
eqmain.bmp and pledit.bmp, reusing the skin's own pixel font so the result matches the
original artwork, and redraws the badge in main.bmp's corner with an L in place of the
lightning bolt. Everything else in the archive is copied unchanged.

    tools/rebrand_skin.py base-2.91.wsz skins/libreamp-base.wsz [--icon ICON]

With --icon, the badge is also written as the application icon: ICON.svg (one square per
pixel, so it stays sharp at any size) and ICON.png (256 px).

Requires Pillow.
"""

import io
import sys
import zipfile

from PIL import Image

ROWS = 7  # Glyphs are 6 px tall; the Q in EQUALIZER has a tail one row lower.
GAP = 1  # Between letters.
SPACE = 2  # Extra width of a space, on top of the gaps either side of it.

# B does not appear anywhere in the skin; it is drawn after the P and R next to it.
B = [
    [0.47, 1.00, 1.00, 1.00, 0.86, 0.20],
    [0.47, 1.00, 0.28, 0.06, 1.00, 0.69],
    [0.47, 1.00, 0.81, 0.81, 1.00, 0.40],
    [0.47, 1.00, 0.28, 0.06, 1.00, 0.74],
    [0.47, 1.00, 0.28, 0.06, 1.00, 0.74],
    [0.47, 1.00, 1.00, 1.00, 0.93, 0.30],
    [0.00] * 6,
]


def lum(p):
    return (p[0] * 3 + p[1] * 6 + p[2]) / 10


class Title:
    """One title bar's lettering: where the text and the bars either side of it are.

    `ty` is the text's top row; `x0..=x1` the text's columns; `lcap`/`rcap` the bar caps (the
    bars' outermost lit column) left and right of the text, or None where there is no bar.
    """

    def __init__(self, img, ty, x0, x1, lcap, rcap):
        self.img, self.ty, self.x0, self.x1, self.lcap, self.rcap = img, ty, x0, x1, lcap, rcap
        self.orig = img.copy()
        # Background columns just outside the text.
        self.gl, self.gr = x0 - 2, x1 + 3
        px = [img.getpixel((x, y)) for x in range(x0, x1 + 1) for y in range(ty, ty + 6)]
        top = max(lum(p) for p in px)
        bright = [p for p in px if lum(p) >= top * 0.97]
        self.fg = max(set(bright), key=bright.count)

    def bg(self, x, y):
        """Background at (x, y), interpolated between the columns either side of the text."""
        t = min(1.0, max(0.0, (x - self.gl) / (self.gr - self.gl)))
        a, b = self.orig.getpixel((self.gl, y)), self.orig.getpixel((self.gr, y))
        return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))

    def mask(self, x0, x1):
        """Coverage (0..1) of the text over columns x0..=x1, as rows of floats."""
        out = []
        for y in range(self.ty, self.ty + ROWS):
            bg = lum(self.bg(x0, y))
            span = lum(self.fg) - bg
            row = [(lum(self.orig.getpixel((x, y))) - bg) / span for x in range(x0, x1 + 1)]
            out.append([0.0 if v < 0.05 else min(1.0, v) for v in row])
        return out

    def rewrite(self, glyphs, center):
        """Replaces the text with `glyphs` (masks), moving the bars to keep their margins."""
        img, ty = self.img, self.ty
        w = sum(len(g[0]) for g in glyphs) + GAP * (len(glyphs) - 1)
        start = round((self.x0 + self.x1 + 1) / 2 - w / 2) if center else self.x0
        end = start + w - 1
        # Bars sit in a dark groove one pixel wider all round, so work from the row above.
        rows = range(ty - 1, ty + ROWS)
        col = lambda x: [self.orig.getpixel((x, y)) for y in rows]

        def put(x, pixels):
            for y, p in zip(rows, pixels):
                img.putpixel((x, y), p)

        lcap = self.lcap and start - (self.x0 - self.lcap)
        rcap = self.rcap and end + (self.rcap - self.x1)
        lo = min(self.lcap, lcap) + 1 if self.lcap else self.x0
        hi = max(self.rcap, rcap) - 1 if self.rcap else self.x1 + 3
        for x in range(lo, hi + 1):
            put(x, [self.bg(x, y) for y in rows])
        # Bars: the cap and groove end at the new length, middle columns filling any extension.
        if self.lcap:
            for x in range(self.lcap, lcap):
                put(x, col(self.lcap - 2))
            put(lcap, col(self.lcap))
            put(lcap + 1, col(self.lcap + 1))
        if self.rcap:
            for x in range(rcap + 1, self.rcap + 1):
                put(x, col(self.rcap + 2))
            put(rcap, col(self.rcap))
            put(rcap - 1, col(self.rcap - 1))

        x = start
        for g in glyphs:
            for dy, row in enumerate(g):
                for dx, a in enumerate(row):
                    if a > 0:
                        b = img.getpixel((x + dx, ty + dy))
                        img.putpixel(
                            (x + dx, ty + dy),
                            tuple(round(b[i] + (self.fg[i] - b[i]) * a) for i in range(3)),
                        )
            x += len(g[0]) + GAP


# The badge in main.bmp: a silver diamond centred between pixels, |dx| + |dy| <= 9.
BADGE_CX, BADGE_CY = 257.5, 97.5
BADGE_AREA = (247, 87, 268, 108)  # Inclusive; the old badge and bolt lie inside it.
SILVER, EDGE, RING = (0xAD, 0xAF, 0xB5), (0x48, 0x48, 0x48), (0x13, 0x14, 0x1D)
# The bolt's golds, lit from the top left, and its dark brown outline.
SHEEN, LIGHT, GOLD, ORANGE = (0xA3, 0x94, 0x6B), (0xA7, 0x8E, 0x56), (0xA6, 0x85, 0x31), (0xA7, 0x64, 0x1F)
LIT_EDGE, SHADE_EDGE = (0x59, 0x43, 0x1C), (0x36, 0x2D, 0x11)
# The L: an italic 3 px stroke leaning the way the bolt did, breaking out of the badge at the
# top and on the right. Spans of x (inclusive) per row.
L_TOP, L_FOOT, L_BOTTOM, L_RIGHT = 89, 100, 102, 264
L_SPANS = {
    y: (253 + (L_BOTTOM - y) // 2, L_RIGHT if y >= L_FOOT else 255 + (L_BOTTOM - y) // 2)
    for y in range(L_TOP, L_BOTTOM + 1)
}

def badge_distance(x, y):
    return abs(x - BADGE_CX) + abs(y - BADGE_CY)


def relogo(img):
    """Replaces the lightning-bolt badge with one bearing an L. Returns its pixels."""
    x0, y0, x1, y1 = BADGE_AREA
    orig = img.copy()
    logo = {}

    def draw(x, y, c):
        img.putpixel((x, y), c)
        logo[(x, y)] = c

    # Clear the old badge and bolt: anything inside the badge or not the panel's dark blue. The
    # gap is filled by relaxing towards the surrounding panel so its gradient carries on.
    def is_logo(x, y):
        r, _, b = orig.getpixel((x, y))
        return badge_distance(x, y) <= 9 or b < r + 8 or not 28 <= lum((r, _, b)) <= 46

    hole = {(x, y) for x in range(x0 + 1, x1) for y in range(y0 + 1, y1) if is_logo(x, y)}
    px = {(x, y): orig.getpixel((x, y)) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)}
    for _ in range(400):
        nxt = {}
        for x, y in hole:
            n = [px[(x + dx, y + dy)] for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))]
            nxt[(x, y)] = tuple(sum(c[i] for c in n) / 4 for i in range(3))
        px.update(nxt)
    for x, y in hole:
        img.putpixel((x, y), tuple(round(v) for v in px[(x, y)]))

    for x in range(x0, x1 + 1):
        for y in range(y0, y1 + 1):
            d = badge_distance(x, y)
            if d <= 9:
                draw(x, y, SILVER if d <= 7 else EDGE if d <= 8 else RING)

    def in_l(x, y):
        span = L_SPANS.get(y)
        return span is not None and span[0] <= x <= span[1]

    for x in range(x0, x1 + 1):
        for y in range(y0, y1 + 1):
            if in_l(x, y):
                # Lit from the top left: light edges there, orange on the far side.
                if not in_l(x + 1, y) or not in_l(x, y + 1):
                    c = ORANGE
                elif not in_l(x - 1, y):
                    c = SHEEN if L_TOP + 2 < y < L_FOOT - 1 else LIGHT
                elif not in_l(x, y - 1):
                    c = LIGHT
                else:
                    c = GOLD
                draw(x, y, c)
            elif any(in_l(x + dx, y + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1)):
                # Outline: lighter where light falls on it (above and left of the stroke).
                lit = in_l(x + 1, y) or in_l(x, y + 1) or in_l(x + 1, y + 1)
                draw(x, y, LIT_EDGE if lit and badge_distance(x, y) <= 7 else SHADE_EDGE)
    return logo


def write_icon(logo, stem):
    """Writes the badge pixels as a square SVG and a 256 px PNG, centred, background clear."""
    xs, ys = [x for x, _ in logo], [y for _, y in logo]
    side = max(max(xs) - min(xs), max(ys) - min(ys)) + 3  # One pixel of air all round.
    ox = min(xs) - (side - (max(xs) - min(xs) + 1)) // 2
    oy = min(ys) - (side - (max(ys) - min(ys) + 1)) // 2
    rects = "".join(
        f'<rect x="{x - ox}" y="{y - oy}" width="1" height="1" fill="#{r:02x}{g:02x}{b:02x}"/>'
        for (x, y), (r, g, b) in sorted(logo.items(), key=lambda kv: (kv[0][1], kv[0][0]))
    )
    with open(stem + ".svg", "w") as f:
        f.write(
            f'<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" '
            f'viewBox="0 0 {side} {side}" shape-rendering="crispEdges">{rects}</svg>\n'
        )
    small = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    for (x, y), c in logo.items():
        small.putpixel((x - ox, y - oy), c + (255,))
    scale = 256 // side
    big = small.resize((side * scale, side * scale), Image.NEAREST)
    icon = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
    icon.paste(big, ((256 - big.width) // 2, (256 - big.height) // 2))
    icon.save(stem + ".png")


def blank(width):
    return [[0.0] * width for _ in range(ROWS)]


def main(src, dst, icon=None):
    with zipfile.ZipFile(src) as z:
        entries = [(i, z.read(i)) for i in z.infolist()]
    find = lambda stem: next(i for i, (info, _) in enumerate(entries)
                             if info.filename.lower().rsplit("/", 1)[-1] == stem + ".bmp")
    images = {}
    for stem in ("titlebar", "eqmain", "pledit", "main"):
        idx = find(stem)
        images[stem] = (idx, Image.open(io.BytesIO(entries[idx][1])).convert("RGB"))
    tb, eq, pl = (images[s][1] for s in ("titlebar", "eqmain", "pledit"))

    # Title bars, active then inactive. Coordinates are the original artwork's.
    main_titles = [Title(tb, y, 145, 183, 139, 189) for y in (5, 20)]
    shade_titles = [Title(tb, y, 46, 84, None, 91) for y in (34, 47)]
    eq_titles = [Title(eq, y, 95, 187, 89, 193) for y in (139, 154)]
    pl_titles = [Title(pl, y, 33, 119, 28, 123) for y in (5, 26)]

    # Letters from the active title bars.
    src_tb, src_eq, src_pl = main_titles[0], eq_titles[0], pl_titles[0]
    g = {
        "L": src_eq.mask(163, 167),
        "I": src_tb.mask(154, 155),
        "B": B,
        "R": src_eq.mask(182, 187),
        "E": src_eq.mask(138, 142),
        "A": src_tb.mask(163, 168),
        "M": src_tb.mask(170, 176),
        "P": src_tb.mask(178, 183),
        "EQUALIZER": src_eq.mask(138, 187),
        "PLAYLIST": src_pl.mask(76, 119),
        " ": blank(SPACE),
    }
    libreamp = [g[c] for c in "LIBREAMP"]
    for t in main_titles:
        t.rewrite(libreamp, center=True)
    for t in shade_titles:
        t.rewrite(libreamp, center=False)
    for t in eq_titles:
        t.rewrite(libreamp + [g[" "], g["EQUALIZER"]], center=True)
    # "LIBREAMP PLAYLIST" is wider than the 100 px title tile, so the playlist is just that.
    for t in pl_titles:
        t.rewrite([g["PLAYLIST"]], center=True)
    logo = relogo(images["main"][1])
    if icon:
        write_icon(logo, icon)

    for idx, img in images.values():
        buf = io.BytesIO()
        img.save(buf, "BMP")
        entries[idx] = (entries[idx][0], buf.getvalue())
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as z:
        for info, data in entries:
            z.writestr(info, data, zipfile.ZIP_DEFLATED)


if __name__ == "__main__":
    args = sys.argv[1:]
    icon = None
    if len(args) == 4 and args[2] == "--icon":
        icon = args.pop()
        args.pop()
    if len(args) != 2:
        sys.exit(__doc__)
    main(*args, icon=icon)
