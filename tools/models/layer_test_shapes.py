"""Source models for the layered-finish test shapes, written from formulas:

    python -I tools/models/layer_test_shapes.py coat|bubble|brushed|pouf|coat-ball|bubble-ball <out.glb>
    cargo xtask convert <out.glb> --id layer-<kind> --tiers low,high --out <dir>

then copy <id>.high.glb to apps/<app>/assets/models/<id>.glb and
<id>.low.glb next to it, and run `cargo xtask manifest <app>`. The shader
balls (coat-ball, bubble-ball) convert with `--tiers low,ultra` and ship the
ultra file as <id>.glb: ultra keeps every triangle, and the high tier's
simplifier opened cracks at their seams.

* coat, bubble: a uv sphere of radius 0.42 standing on y = 0 (u once round
  from the back, v from the top pole down, tangents along u), with a clear
  coat or a soap film;
* brushed: a steel disc of radius 0.5 and 0.03 thick facing +z, radial
  tangents (anisotropy), with its rim and back;
* pouf: a rounded cushion of radius 0.45 and 0.46 tall (lathe), velvet
  sheen with the plain-weave cloth extras (scale 4.5 repeats per UV unit:
  the files once shipped their coordinates quantized a quarter size on
  High and a sixteenth on Low, which drew two different weaves; the
  converter now keeps them true, `-vtf`), u round it and v down its
  profile;
* coat-ball, bubble-ball: the material viewer's shader ball (lathe, 0.76
  tall): a ball of radius 0.36 with a recessed band round its middle (two
  crisp ledges, so a coat's sharp reflection and a softer base read on the
  edges), on a short neck and a round foot, with the coat or the film. The
  plain balls above stay for the sandbox proofs; critic round 2 of
  material-view asked for a real shader-ball shape.

Every normal points out of the shape and every triangle faces its corners'
normals, poles and disc centres included: the first files had those
normals flipped, which drew a dark dot on top of every proof ball (critic
round 1 of material-view). crates/evani-gltf/tests/normals.rs checks every
game model for it."""
import json, math, struct, sys
SEG, RING, R = 48, 24, 0.42
pos, nor, tan, uv, idx = [], [], [], [], []
def vert(p, n, t, w):
    pos.append(p); nor.append(n); tan.append(t); uv.append(w); return len(pos) - 1
def face(a, b, c):
    pa, pb, pc = pos[a], pos[b], pos[c]
    e1 = [pb[i] - pa[i] for i in range(3)]; e2 = [pc[i] - pa[i] for i in range(3)]
    f = (e1[1]*e2[2]-e1[2]*e2[1], e1[2]*e2[0]-e1[0]*e2[2], e1[0]*e2[1]-e1[1]*e2[0])
    if sum(x * x for x in f) < 1e-18: return
    out = [nor[a][i] + nor[b][i] + nor[c][i] for i in range(3)]
    if sum(f[i] * out[i] for i in range(3)) < 0: b, c = c, b
    idx.extend((a, b, c))
def ball():
    grid = {}
    for j in range(RING + 1):
        v = j / RING
        th = v * math.pi
        st, ct = math.sin(th), math.cos(th)
        for k in range(SEG + 1):
            # Pole rows: one vertex per segment, at the segment's middle, so
            # each pole triangle gets its own uv and tangent.
            u = (k + 0.5) / SEG if j in (0, RING) else k / SEG
            ph = u * 2 * math.pi
            s, c = math.sin(ph), math.cos(ph)
            n = (s * st, ct, -c * st) if j not in (0, RING) else (0.0, 1.0 if j == 0 else -1.0, 0.0)
            p = (n[0] * R, n[1] * R + R, n[2] * R) if j not in (0, RING) else (0.0, R + R * n[1], 0.0)
            t = (c, 0.0, s, 1.0)
            grid[(j, k)] = vert(p, n, t, (u, v))
    for j in range(RING):
        for k in range(SEG):
            a, b, c, d = grid[(j, k)], grid[(j + 1, k)], grid[(j + 1, k + 1)], grid[(j, k + 1)]
            if j > 0: face(a, b, d)
            if j + 1 < RING: face(d, b, c)

DR, DH, DSEG, DRING = 0.5, 0.015, 64, 15
def disc():
    for side in (1, -1):
        g = {}
        for j in range(DRING + 1):
            r = DR * j / DRING
            for k in range(DSEG + 1):
                ang = ((k + 0.5) if j == 0 else k) / DSEG * 2 * math.pi
                c, s_ = math.cos(ang), math.sin(ang)
                g[(j, k)] = vert((r * c, r * s_, side * DH), (0.0, 0.0, float(side)), (c, s_, 0.0, 1.0), (0.5 + 0.5 * r / DR * c, 0.5 - 0.5 * r / DR * s_))
        for j in range(DRING):
            for k in range(DSEG):
                a_, b_, c_, d_ = g[(j, k)], g[(j + 1, k)], g[(j + 1, k + 1)], g[(j, k + 1)]
                if j > 0: face(a_, b_, d_)
                face(d_, b_, c_)
    rim = {}
    for row, z in enumerate((DH, -DH)):
        for k in range(DSEG + 1):
            ang = k / DSEG * 2 * math.pi
            c, s_ = math.cos(ang), math.sin(ang)
            rim[(row, k)] = vert((DR * c, DR * s_, z), (c, s_, 0.0), (0.0, 0.0, 1.0, 1.0), (k / DSEG, float(row)))
    for k in range(DSEG):
        a_, b_, c_, d_ = rim[(0, k)], rim[(1, k)], rim[(1, k + 1)], rim[(0, k + 1)]
        face(a_, b_, d_); face(d_, b_, c_)
PR, PH, PC, PSEG = 0.45, 0.46, 0.11, 64
def pouf():
    # Profile from the top centre round the rim to the bottom centre: a
    # slightly domed top, a rounded corner, a gently bulging side, a flat
    # bottom with a small rounded foot.
    prof = []
    # Few rings on the nearly flat caps: the converter's simplifier would
    # collapse the two sides of the uv seam there differently (open edges).
    for i in range(4):
        r = (PR - PC) * i / 3
        prof.append((r, PH - 0.025 * (r / (PR - PC)) ** 2))
    top = prof[-1][1]
    for i in range(1, 9):
        a = i / 8 * math.pi / 2
        prof.append((PR - PC + PC * math.sin(a), top - PC + PC * math.cos(a)))
    side_top, side_bot = top - PC, 0.05
    # A straight side with no rows between the corners: rows on a flat run
    # are collapsed unevenly on the two sides of the uv seam by the
    # converter's simplifier, which opens cracks.
    side_bot = 0.05
    for i in range(0, 5):
        a = i / 4 * math.pi / 2
        prof.append((PR - 0.05 + 0.05 * math.cos(a), 0.05 - 0.05 * math.sin(a)))
    for i in range(1, 3):
        # The bottom is hollowed 6 mm inside the foot ring, so only the ring
        # meets the floor (no face lies on the floor plane).
        prof.append(((PR - 0.05) * (1 - i / 2), 0.006))
    # Lengths along the profile (v) and normals from the profile direction.
    L = [0.0]
    for i in range(1, len(prof)):
        L.append(L[-1] + math.dist(prof[i], prof[i - 1]))
    grid = {}
    n = len(prof)
    for j, (r, y) in enumerate(prof):
        a = prof[max(j - 1, 0)]; b = prof[min(j + 1, n - 1)]
        dr, dy = b[0] - a[0], b[1] - a[1]
        l = math.hypot(dr, dy) or 1.0
        nr, ny = -dy / l, dr / l
        if j == 0: nr, ny = 0.0, 1.0
        if j == n - 1: nr, ny = 0.0, -1.0
        for k in range(PSEG + 1):
            u = (k + 0.5) / PSEG if j in (0, n - 1) else k / PSEG
            ang = u * 2 * math.pi
            c, s_ = math.cos(ang), math.sin(ang)
            grid[(j, k)] = vert((r * c, y, r * s_), (nr * c, ny, nr * s_), (-s_, 0.0, c, 1.0), (u, L[j] / L[-1]))
    for j in range(n - 1):
        for k in range(PSEG):
            a_, b_, c_, d_ = grid[(j, k)], grid[(j + 1, k)], grid[(j + 1, k + 1)], grid[(j, k + 1)]
            if j > 0: face(a_, b_, d_)
            if j + 1 < n - 1: face(d_, b_, c_)

# The shader ball (coat-ball, bubble-ball): profile pieces from the top
# centre down to the bottom centre, each its own run of rings (so the band's
# ledges are crisp creases), meeting edge to edge.
BR, BI, BYC, BSEG = 0.36, 0.325, 0.395, 96
def arc(r, a0, a1, n):
    """Outer or inner sphere rows from polar angle a0 to a1 (degrees)."""
    out = []
    for i in range(n + 1):
        a = math.radians(a0 + (a1 - a0) * i / n)
        out.append((r * math.sin(a), BYC + r * math.cos(a), (math.sin(a), math.cos(a))))
    return out
def line(p, q, n):
    # The profile runs from the top down, so the outside is to the right of
    # the direction of travel: (-dy, dr) (the cap's lip faces down, the
    # bowl's rim up).
    dr, dy = q[0] - p[0], q[1] - p[1]
    l = math.hypot(dr, dy)
    nr, ny = -dy / l, dr / l
    return [(p[0] + dr * i / n, p[1] + dy * i / n, (nr, ny)) for i in range(n + 1)]
def shaderball():
    top = arc(BR, 0, 76, 26)
    # The upper ledge: the outer surface steps in to the recessed band.
    ledge_r0 = BR * math.sin(math.radians(76))
    y0 = BYC + BR * math.cos(math.radians(76))
    band_a0 = math.degrees(math.acos((y0 - BYC) / BI))
    ledge1 = line((ledge_r0, y0), (BI * math.sin(math.radians(band_a0)), y0), 2)
    band_a1 = 180 - band_a0
    band = arc(BI, band_a0, band_a1, 10)
    y1 = BYC + BI * math.cos(math.radians(band_a1))
    lower_a = math.degrees(math.acos((y1 - BYC) / BR))
    ledge2 = line((BI * math.sin(math.radians(band_a1)), y1), (BR * math.sin(math.radians(lower_a)), y1), 2)
    low = arc(BR, lower_a, 150, 16)
    # The neck under the ball, the foot's rounded rim and its hollow bottom
    # (only the rim meets the floor, as on the pouf).
    nr = low[-1][0]
    neck = line((nr, low[-1][1]), (0.13, 0.085), 4)
    flare = [(0.13 + 0.15 * (1 - math.cos(a)), 0.085 - 0.045 * math.sin(a), (math.sin(a) * 0.45 + 0.0, math.cos(a))) for a in [math.pi / 2 * i / 6 for i in range(7)]]
    # The foot's rounded rim (a half round: up, out, down) and its hollow
    # bottom back to the centre, one run with one normal where they meet:
    # a seam there is what a simplifier cracks open.
    rim = [(0.28 + 0.02 * math.sin(math.pi * i / 8), 0.02 + 0.02 * math.cos(math.pi * i / 8), (math.sin(math.pi * i / 8), math.cos(math.pi * i / 8))) for i in range(9)]
    bottom = line((0.28, 0.0), (0.0, 0.006), 4)
    rim = rim + [(r, y, (0.0, -1.0)) for (r, y, _) in bottom[1:]]
    pieces = [top, ledge1, band, ledge2, low, neck, flare, rim]
    total = 0.0
    lengths = []
    for pc in pieces:
        lengths.append(total)
        total += sum(math.dist(pc[i][:2], pc[i - 1][:2]) for i in range(1, len(pc)))
    for pc, L0 in zip(pieces, lengths):
        grid = {}
        acc = L0
        rows = []
        for j, (r, y, (nr_, ny_)) in enumerate(pc):
            if j > 0:
                acc += math.dist(pc[j][:2], pc[j - 1][:2])
            rows.append(acc / total)
        n = len(pc)
        for j, (r, y, (nr_, ny_)) in enumerate(pc):
            pole = r < 1e-6
            for k in range(BSEG + 1):
                u = (k + 0.5) / BSEG if pole else k / BSEG
                ang = u * 2 * math.pi
                c, s_ = math.cos(ang), math.sin(ang)
                nl = math.hypot(nr_, ny_) or 1.0
                grid[(j, k)] = vert((r * c, y, r * s_), (nr_ / nl * c, ny_ / nl, nr_ / nl * s_), (-s_, 0.0, c, 1.0), (u, rows[j]))
        for j in range(n - 1):
            for k in range(BSEG):
                a_, b_, c_, d_ = grid[(j, k)], grid[(j + 1, k)], grid[(j + 1, k + 1)], grid[(j, k + 1)]
                if pc[j][0] > 1e-6: face(a_, b_, d_)
                if pc[j + 1][0] > 1e-6: face(d_, b_, c_)

kind = sys.argv[1]
{"brushed": disc, "pouf": pouf, "coat-ball": shaderball, "bubble-ball": shaderball}.get(kind, ball)()
# Check: every triangle faces the way its corners' normals do.
for t in range(0, len(idx), 3):
    a, b, c = idx[t:t + 3]
    e1 = [pos[b][i] - pos[a][i] for i in range(3)]; e2 = [pos[c][i] - pos[a][i] for i in range(3)]
    f = (e1[1]*e2[2]-e1[2]*e2[1], e1[2]*e2[0]-e1[0]*e2[2], e1[0]*e2[1]-e1[1]*e2[0])
    for v in (a, b, c):
        assert sum(f[i] * nor[v][i] for i in range(3)) > 0, (t, v)
def pack(fmt, rows):
    return b"".join(struct.pack("<" + fmt, *r) for r in rows)
blobs = [pack("3f", pos), pack("3f", nor), pack("4f", tan), pack("2f", uv), struct.pack("<%dI" % len(idx), *idx)]
views, off, binary = [], 0, b""
for i, b in enumerate(blobs):
    views.append({"buffer": 0, "byteOffset": off, "byteLength": len(b), **({"target": 34963} if i == 4 else {"target": 34962})})
    binary += b; off += len(b)
mn = [min(p[k] for p in pos) for k in range(3)]; mx = [max(p[k] for p in pos) for k in range(3)]
acc = [
    {"bufferView": 0, "componentType": 5126, "count": len(pos), "type": "VEC3", "min": mn, "max": mx},
    {"bufferView": 1, "componentType": 5126, "count": len(pos), "type": "VEC3"},
    {"bufferView": 2, "componentType": 5126, "count": len(pos), "type": "VEC4"},
    {"bufferView": 3, "componentType": 5126, "count": len(pos), "type": "VEC2"},
    {"bufferView": 4, "componentType": 5125, "count": len(idx), "type": "SCALAR"},
]
if kind == "coat":
    mat = {"name": "candy-paint", "pbrMetallicRoughness": {"baseColorFactor": [0.7, 0.04, 0.03, 1], "metallicFactor": 0.2, "roughnessFactor": 0.55},
           "extensions": {"KHR_materials_clearcoat": {"clearcoatFactor": 1.0, "clearcoatRoughnessFactor": 0.03}}}
    ext = "KHR_materials_clearcoat"; name = "coat"
elif kind == "coat-ball":
    # A deeper red with a soft metal flake under the coat, so the coat's
    # crisp sky reflection and sun glint read over a softer base highlight.
    mat = {"name": "candy-paint", "pbrMetallicRoughness": {"baseColorFactor": [0.42, 0.018, 0.02, 1], "metallicFactor": 0.25, "roughnessFactor": 0.5},
           "extensions": {"KHR_materials_clearcoat": {"clearcoatFactor": 1.0, "clearcoatRoughnessFactor": 0.02}}}
    ext = "KHR_materials_clearcoat"; name = "coat"
elif kind == "pouf":
    mat = {"name": "velvet", "doubleSided": True, "pbrMetallicRoughness": {"baseColorFactor": [0.22, 0.02, 0.06, 1], "metallicFactor": 0.0},
           "extras": {"evani_cloth": {"scale": 4.5, "subsurface": [0.35, 0.04, 0.08], "weave": "plain"}},
           "extensions": {"KHR_materials_sheen": {"sheenColorFactor": [0.95, 0.45, 0.55], "sheenRoughnessFactor": 0.45}}}
    ext = "KHR_materials_sheen"; name = "velvet"
elif kind == "brushed":
    mat = {"name": "brushed-steel", "pbrMetallicRoughness": {"baseColorFactor": [0.9, 0.9, 0.92, 1], "metallicFactor": 1.0, "roughnessFactor": 0.35},
           "extensions": {"KHR_materials_anisotropy": {"anisotropyStrength": 0.85, "anisotropyRotation": 0.0}}}
    ext = "KHR_materials_anisotropy"; name = "brushed"
else:
    mat = {"name": "soap-film", "pbrMetallicRoughness": {"baseColorFactor": [0.02, 0.02, 0.025, 1], "metallicFactor": 0.0, "roughnessFactor": 0.08},
           "extensions": {"KHR_materials_iridescence": {"iridescenceFactor": 1.0, "iridescenceIor": 1.33, "iridescenceThicknessMinimum": 400, "iridescenceThicknessMaximum": 400}}}
    ext = "KHR_materials_iridescence"; name = "bubble"
doc = {"asset": {"version": "2.0", "generator": "evani ball_src.py"}, "extensionsUsed": [ext], "scene": 0, "scenes": [{"nodes": [0]}],
       "nodes": [{"mesh": 0, "name": name}], "meshes": [{"name": name, "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TANGENT": 2, "TEXCOORD_0": 3}, "indices": 4, "material": 0}]}],
       "materials": [mat], "accessors": acc, "bufferViews": views, "buffers": [{"byteLength": len(binary)}]}
js = json.dumps(doc, separators=(",", ":")).encode()
js += b" " * ((4 - len(js) % 4) % 4); binary += b"\0" * ((4 - len(binary) % 4) % 4)
out = struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(binary)) + struct.pack("<II", len(js), 0x4E4F534A) + js + struct.pack("<II", len(binary), 0x004E4942) + binary
open(sys.argv[2], "wb").write(out)
print(sys.argv[2], len(pos), "verts", len(idx) // 3, "tris")
