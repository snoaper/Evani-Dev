"""The realism parity number (procedural realism round 8): how close the proof
game's picture of a library asset is to the Material library's own.

    python tools/procedural/parity_diff.py <library.png> <game.png> <out.json> [x0,y0,x1,y1]

Both pictures are the same size, from the same camera. The subject is
where either picture differs from its own backdrop (the colour at the top
corners) by more than 14 levels, outside an optional ignored box; the
report gives the subject masks'
overlap (1 = the same silhouette), the mean absolute difference over the
subject (0..255, all channels) and each picture's mean subject colour, and
writes a side-by-side with the difference beside the json.
"""
import json
import sys

from PIL import Image, ImageChops


def mask(im):
    w, h = im.size
    bg = [im.getpixel((8, 8)), im.getpixel((w - 9, 8))]
    px = im.load()
    out = Image.new('L', im.size, 0)
    po = out.load()
    for y in range(h):
        for x in range(w):
            p = px[x, y]
            if min(sum(abs(p[k] - b[k]) for k in range(3)) for b in bg) > 14 * 3:
                po[x, y] = 255
    return out


def main(a_path, b_path, out_path, ignore="0,0,0,0"):
    a = Image.open(a_path).convert('RGB')
    b = Image.open(b_path).convert('RGB')
    if a.size != b.size:
        b = b.resize(a.size)
    ma, mb = mask(a), mask(b)
    box = [int(v) for v in ignore.split(',')]
    for m in (ma, mb):
        m.paste(0, box)
    both = ImageChops.multiply(ma, mb)
    union = ImageChops.lighter(ma, mb)
    n_both = sum(1 for v in both.getdata() if v)
    n_union = max(1, sum(1 for v in union.getdata() if v))
    diff = ImageChops.difference(a, b)
    tot, n = 0, 0
    sa, sb = [0, 0, 0], [0, 0, 0]
    for d, m, pa, pb in zip(diff.getdata(), both.getdata(), a.getdata(), b.getdata()):
        if m:
            tot += sum(d)
            n += 1
            for k in range(3):
                sa[k] += pa[k]
                sb[k] += pb[k]
    n = max(n, 1)
    rep = {
        "library": a_path, "game": b_path, "size": list(a.size),
        "silhouette_overlap": round(n_both / n_union, 4),
        "subject_mean_abs_diff_0_255": round(tot / (3 * n), 2),
        "library_subject_mean_rgb": [round(v / n, 1) for v in sa],
        "game_subject_mean_rgb": [round(v / n, 1) for v in sb],
    }
    side = Image.new('RGB', (a.size[0] * 3 // 2, a.size[1] // 2))
    side.paste(a.resize((a.size[0] // 2, a.size[1] // 2)), (0, 0))
    side.paste(b.resize((a.size[0] // 2, a.size[1] // 2)), (a.size[0] // 2, 0))
    side.paste(diff.point(lambda v: min(255, v * 4)).resize((a.size[0] // 2, a.size[1] // 2)), (a.size[0], 0))
    side_path = out_path.rsplit('.', 1)[0] + '-side.png'
    side.save(side_path)
    rep["side_by_side"] = side_path
    with open(out_path, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(rep, f, indent=1)
        f.write('\n')
    print(json.dumps(rep))


if __name__ == '__main__':
    main(*sys.argv[1:5])
