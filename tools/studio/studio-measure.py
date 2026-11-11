"""Measure a studio-object picture (a reference, or a render of view 0) the way the per-object studio is derived
and checked (docs/EVANI_COMMONS.md "Per-object studio"; references.json `camera` and `studio`).

  python tools/studio/studio-measure.py ref <picture>                       one picture's measures (JSON)
  python tools/studio/studio-measure.py compare <reference> <render> <out>   both, the gaps and the tolerances
                                                                            (<out>.json, and <out>.png: reference |
                                                                            render | difference, edge profiles)

Measures (8-bit sRGB, the picture scaled to 768 wide):
  backdrop_lum  the backdrop's grey: the median luminance of the top 12% of the frame
  floor_lum     the floor's grey: the median luminance of the bottom 10% in the outer 8% columns each side
  cast          max minus min of the backdrop's mean RGB (0: neutral grey)
  warmth        the backdrop's red minus blue (+ warm, - cool)
  key_side      the subject's mean luminance left of its centre minus right of it (+: lit from the left)
  fill_w, fill_h  the subject's width and height as a share of the frame (1st to 99th percentile of its pixels)
  centre        the subject's centre (x, y) as a share of the frame
The subject is every pixel far (summed RGB difference over 45) from its row's backdrop (a ramp between the outer
columns each side), so its
contact shadow counts as part of it, in the reference and the render alike.
Tolerances (the owner's brief, 2026-11-11): backdrop and floor +-4, cast <= 3 apart, key side +-4, fill and centre
+-0.02."""
import json
import sys

from PIL import Image, ImageChops, ImageDraw

W = 768
TOL = {"backdrop_lum": 4, "floor_lum": 4, "cast": 3, "key_side": 4, "fill_w": 0.02, "fill_h": 0.02, "centre_x": 0.02, "centre_y": 0.02}


def load(p):
    im = Image.open(p).convert("RGB")
    return im.resize((W, round(im.height * W / im.width)), Image.LANCZOS)


def lum(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def median(v):
    s = sorted(v)
    return s[len(s) // 2] if s else 0.0


def measure(img):
    px = img.load()
    w, h = img.size
    edge = int(w * 0.08)
    cols = list(range(0, edge)) + list(range(w - edge, w))
    def side(xr, y):
        cs = sorted((px[x, y] for x in xr), key=lum)
        mid = cs[len(cs) // 4: len(cs) - len(cs) // 4]  # the middle half: a stray subject pixel does not move it
        return [sum(c[i] for c in mid) / len(mid) for i in range(3)]

    # Each row's backdrop: a ramp from its left edge to its right edge (a key light pool or a shadow reaching
    # one edge is backdrop there, not subject); `prof` is the two edges' mean, for the profile plot.
    ramp, prof = [], []
    for y in range(h):
        l, r = side(range(0, edge), y), side(range(w - edge, w), y)
        ramp.append((l, r))
        prof.append([(l[i] + r[i]) / 2 for i in range(3)])
    top = [px[x, y] for y in range(0, int(h * 0.12), 2) for x in range(0, w, 2)]
    top_l = median([lum(c) for c in top])
    top_c = [c for c in top if abs(lum(c) - top_l) < 12]
    rgb = [sum(c[i] for c in top_c) / max(1, len(top_c)) for i in range(3)]
    flo = [px[x, y] for y in range(int(h * 0.9), h, 2) for x in cols[::2]]
    xs, ys, sub = [], [], []
    for y in range(0, h, 2):
        l, r = ramp[y]
        for x in range(0, w, 2):
            t = (x - edge / 2) / (w - edge)
            b = [l[i] + (r[i] - l[i]) * t for i in range(3)]
            c = px[x, y]
            if abs(c[0] - b[0]) + abs(c[1] - b[1]) + abs(c[2] - b[2]) > 45:
                xs.append(x)
                ys.append(y)
                sub.append(lum(c))
    out = {
        "backdrop_lum": round(top_l, 1),
        "backdrop_rgb": [round(v, 1) for v in rgb],
        "floor_lum": round(median([lum(c) for c in flo]), 1),
        "cast": round(max(rgb) - min(rgb), 1),
        "warmth": round(rgb[0] - rgb[2], 1),
        "gradient": [round(lum(prof[int(h * (i + 0.5) / 10)]), 1) for i in range(10)],
        "_prof": [lum(p) for p in prof],
    }
    if xs:
        q = lambda v, f: sorted(v)[min(len(v) - 1, int(len(v) * f))]
        x0, x1, y0, y1 = q(xs, 0.01), q(xs, 0.99), q(ys, 0.01), q(ys, 0.99)
        cx = sum(xs) / len(xs)
        left = [l for x, l in zip(xs, sub) if x < cx]
        right = [l for x, l in zip(xs, sub) if x >= cx]
        out.update({
            "key_side": round(sum(left) / max(1, len(left)) - sum(right) / max(1, len(right)), 1),
            "fill_w": round((x1 - x0) / w, 3),
            "fill_h": round((y1 - y0) / h, 3),
            "centre_x": round((x0 + x1) / 2 / w, 3),
            "centre_y": round((y0 + y1) / 2 / h, 3),
            "subject_lum": round(sum(sub) / len(sub), 1),
        })
    return out


def public(m):
    return {k: v for k, v in m.items() if not k.startswith("_")}


def main():
    if len(sys.argv) >= 3 and sys.argv[1] == "ref":
        print(json.dumps(public(measure(load(sys.argv[2])))))
        return
    if len(sys.argv) >= 5 and sys.argv[1] == "compare":
        ref_p, ren_p, out = sys.argv[2], sys.argv[3], sys.argv[4]
        ref, ren = load(ref_p), load(ren_p)
        if ren.size != ref.size:
            ren = ren.resize(ref.size, Image.LANCZOS)
        a, b = measure(ref), measure(ren)
        rows = {}
        for k, tol in TOL.items():
            if k in a and k in b:
                gap = round(b[k] - a[k], 3)
                rows[k] = {"reference": a[k], "render": b[k], "gap": gap, "tolerance": tol, "ok": abs(gap) <= tol}
        res = {"reference": ref_p, "render": ren_p, "rows": rows, "all_ok": all(r["ok"] for r in rows.values()), "reference_all": public(a), "render_all": public(b)}
        with open(out + ".json", "w", encoding="utf-8") as f:
            json.dump(res, f, indent=1)
        w, h = ref.size
        diff = ImageChops.difference(ref, ren).point(lambda v: min(255, v * 2))
        sheet = Image.new("RGB", (w * 3, h + 200), (24, 24, 24))
        sheet.paste(ref, (0, 0))
        sheet.paste(ren, (w, 0))
        sheet.paste(diff, (2 * w, 0))
        d = ImageDraw.Draw(sheet)
        for i, t in enumerate(["reference", "render (view 0)", "difference x2"]):
            d.text((w * i + 8, 8), t, fill=(255, 255, 0))
        ox, oy, gh = 8, h + 10, 180
        for prof, col in ((a["_prof"], (255, 220, 0)), (b["_prof"], (0, 220, 255))):
            d.line([(ox + y * (3 * w - 16) / h, oy + gh - prof[y] / 255 * gh) for y in range(h)], fill=col, width=2)
        d.text((ox, oy), "edge brightness top->bottom: reference yellow, render cyan", fill=(220, 220, 220))
        sheet.save(out + ".png")
        print(json.dumps({k: [r["reference"], r["render"], "ok" if r["ok"] else "OFF"] for k, r in rows.items()}))
        return
    print(__doc__)
    sys.exit(2)


if __name__ == "__main__":
    main()
