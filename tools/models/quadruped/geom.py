# Shape helpers for the quadruped generator (original work; generalised from
# tools/models/horse/anatomy.py, no outside libraries). Every part is a closed
# loft of cross-sections, an ellipsoid or a tube along a centre line; the
# generator unions them with a voxel remesh, so parts may overlap freely.
#
# Frame: f forward (metres, + toward the head), z up, x to the animal's left.
# Blender: the animal faces -Y (Rigify's metarig; glTF +Z), P(f, z, x).
import math
from mathutils import Vector, Matrix

pi = math.pi
X = Vector((1, 0, 0))


def P(f, z, x=0.0):
    return Vector((x, -f, z))


def fz(v):
    """(f, z) of a Blender point."""
    return (-v.y, v.z)


def _cr(p0, p1, p2, p3, t):
    return 0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3)


def catmull(keys, s):
    """Catmull-Rom through a list of equal-length number lists at s in
    [0, len-1]."""
    n = len(keys)
    i = min(int(math.floor(s)), n - 2)
    t = s - i
    k0, k1, k2, k3 = keys[max(i - 1, 0)], keys[i], keys[i + 1], keys[min(i + 2, n - 1)]
    return [_cr(a, b, c, d, t) for a, b, c, d in zip(k0, k1, k2, k3)]


def smoothstep(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


# Fractions of the way from the top (or front) midline point to the bottom
# (back) one where a section's half widths are sampled.
TT = (0.02, 0.08, 0.18, 0.32, 0.48, 0.64, 0.78, 0.90, 0.97)


def profile(w, widest=0.5, p_top=2.0, p_bot=2.0, flat_top=0.0):
    """A section's right half [(x, t)] from four numbers: half width w,
    the widest point (0 top .. 1 bottom), superellipse exponents above and
    below it (2 round, 3+ boxy), flat_top (0..1) a flattened top (a broad
    forehead or back). Midline points at t 0 and 1."""
    pts = [(0.0, 0.0)]
    for t in TT:
        if t < widest:
            u = (widest - t) / max(widest, 1e-6)
            p = p_top
        else:
            u = (t - widest) / max(1 - widest, 1e-6)
            p = p_bot
        x = w * max(1 - u ** p, 0.0) ** (1.0 / p)
        if t < widest and flat_top > 0:
            x = max(x, w * flat_top * (1 - smoothstep(0.0, widest, widest - t) * 0.15))
        pts.append((x, t))
    pts.append((0.0, 1.0))
    return pts


def half_curve(pts):
    k = len(pts)

    def get(i):
        if i < 0:
            x, t = pts[-i]
            return (-x, t)
        if i >= k:
            x, t = pts[2 * (k - 1) - i]
            return (-x, t)
        return pts[i]
    dense = []
    for i in range(k - 1):
        p0, p1, p2, p3 = get(i - 1), get(i), get(i + 1), get(i + 2)
        for j in range(12):
            tt = j / 12
            dense.append((_cr(p0[0], p1[0], p2[0], p3[0], tt), _cr(p0[1], p1[1], p2[1], p3[1], tt)))
    dense.append(pts[-1])
    return dense


def section(A, B, pts, n, lat=X):
    """A closed ring of n points (n even) in the plane of the midline points
    A, B and the lateral axis; pts the right half profile [(x, t)]."""
    dense = half_curve(pts)
    m = n // 2
    AB = B - A
    L = AB.length
    acc = [0.0]
    for (x0, t0), (x1, t1) in zip(dense, dense[1:]):
        acc.append(acc[-1] + math.hypot(x1 - x0, (t1 - t0) * L))
    tot = acc[-1] or 1.0
    half = []
    j = 0
    for i in range(m + 1):
        s = tot * i / m
        while j < len(acc) - 2 and acc[j + 1] < s:
            j += 1
        u = (s - acc[j]) / max(acc[j + 1] - acc[j], 1e-12)
        x = dense[j][0] + (dense[j + 1][0] - dense[j][0]) * u
        t = dense[j][1] + (dense[j + 1][1] - dense[j][1]) * u
        half.append((max(x, 0.0), t))
    half[0] = (0.0, half[0][1])
    half[-1] = (0.0, half[-1][1])
    ring = [A + AB * t + lat * x for x, t in half]
    ring += [A + AB * t - lat * x for x, t in reversed(half[1:-1])]
    return ring


def loft(bm, rings, cap=True):
    """Quads between consecutive rings, fan caps at both ends."""
    vs = [[bm.verts.new(p) for p in r] for r in rings]
    n = len(rings[0])
    for a, b in zip(vs, vs[1:]):
        for i in range(n):
            j = (i + 1) % n
            bm.faces.new((a[i], a[j], b[j], b[i]))
    if cap:
        for ring, rev in ((vs[0], False), (vs[-1], True)):
            c = bm.verts.new(sum((v.co for v in ring), Vector()) / len(ring))
            for i in range(n):
                j = (i + 1) % n
                bm.faces.new((c, ring[j], ring[i]) if not rev else (c, ring[i], ring[j]))


def section_part(bm, keys, steps, n=48, xc=0.0):
    """keys: [(A Vector, B Vector, pts)] all with the same number of profile
    points; Catmull-Rom between them. xc moves the part sideways."""
    flat = []
    for A, B, pts in keys:
        flat.append([A.x, A.y, A.z, B.x, B.y, B.z] + [v for p in pts for v in p])
    rings = []
    for i in range(steps + 1):
        s = i / steps * (len(keys) - 1)
        v = catmull(flat, s)
        pts = [(max(v[6 + 2 * j], 0.0), v[7 + 2 * j]) for j in range((len(v) - 6) // 2)]
        pts[0] = (0.0, pts[0][1])
        pts[-1] = (0.0, pts[-1][1])
        ring = section(Vector(v[0:3]), Vector(v[3:6]), pts, n)
        if xc:
            ring = [p + X * xc for p in ring]
        rings.append(ring)
    loft(bm, rings)


def sgnpow(v, e):
    return math.copysign(abs(v) ** e, v)


def frame_from(t, up_hint):
    """Two unit vectors across the direction t: u (toward up_hint) and w."""
    t = t.normalized()
    u = (up_hint - t * up_hint.dot(t))
    if u.length < 1e-6:
        u = Vector((0, 0, 1)) - t * t.z
    u.normalize()
    w = t.cross(u).normalized()
    return u, w


def tube(bm, centre, radii, n=24, up_hint=Vector((1, 0, 0)), shape=None, cap=True):
    """A tube along a centre line (list of Vectors) with per-point radii
    (r or (r_u, r_w)); shape(i, k) may scale each ring point. Rings are
    perpendicular to the line; u points toward up_hint."""
    rings = []
    m = len(centre)
    for i, c in enumerate(centre):
        t = centre[min(i + 1, m - 1)] - centre[max(i - 1, 0)]
        u, w = frame_from(t, up_hint)
        r = radii[i]
        ru, rw = (r, r) if isinstance(r, (int, float)) else r
        ring = []
        for k in range(n):
            a = 2 * pi * k / n
            s = shape(i, k) if shape else 1.0
            ring.append(c + u * (math.cos(a) * ru * s) + w * (math.sin(a) * rw * s))
        rings.append(ring)
    loft(bm, rings, cap=cap)


def spline(points, samples):
    """Catmull-Rom through Vectors, `samples` + 1 evenly spaced (by
    parameter) points."""
    keys = [[p.x, p.y, p.z] for p in points]
    return [Vector(catmull(keys, i / samples * (len(points) - 1))) for i in range(samples + 1)]


def ellipsoid(bm, centre, axes, radii, segs=24, rings_=16):
    """An ellipsoid: centre (Vector), axes (3 unit Vectors), radii."""
    M = Matrix((axes[0] * radii[0], axes[1] * radii[1], axes[2] * radii[2])).transposed()
    rows = []
    for j in range(1, rings_):
        ph = pi * j / rings_ - pi / 2
        rows.append([centre + M @ Vector((math.cos(ph) * math.cos(2 * pi * i / segs), math.cos(ph) * math.sin(2 * pi * i / segs), math.sin(ph))) for i in range(segs)])
    vs = [[bm.verts.new(p) for p in r] for r in rows]
    for a, b in zip(vs, vs[1:]):
        for i in range(segs):
            k = (i + 1) % segs
            bm.faces.new((a[i], a[k], b[k], b[i]))
    bot = bm.verts.new(centre - M @ Vector((0, 0, 1)))
    top = bm.verts.new(centre + M @ Vector((0, 0, 1)))
    for i in range(segs):
        k = (i + 1) % segs
        bm.faces.new((bot, vs[0][k], vs[0][i]))
        bm.faces.new((top, vs[-1][i], vs[-1][k]))


def muscle(bm, f, z, x, radii, axis, mirror=True, tilt_out=0.0):
    """A muscle mass: an ellipsoid centred at (f, z, x), its long axis along
    `axis` (f, z) in the side view, radii (long, across, lateral); tilt_out
    turns the long axis out of the side plane (radians)."""
    a = Vector((0.0, -axis[0], axis[1])).normalized()
    for sx in ([1, -1] if mirror else [1]):
        aa = (a + X * (sx * math.tan(tilt_out))).normalized()
        l2 = (X * sx - aa * aa.dot(X * sx)).normalized() * sx
        ellipsoid(bm, P(f, z, sx * x), (aa, l2.cross(aa).normalized() * sx, l2), radii)


def seg_dist(p, a, b):
    ab = b - a
    t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-12)))
    return (p - (a + ab * t)).length, t


def poly_dist(p, pts):
    """Distance from p to a polyline and the fraction along it (0..1)."""
    best = (1e9, 0.0)
    total = sum((b - a).length for a, b in zip(pts, pts[1:])) or 1.0
    acc = 0.0
    for a, b in zip(pts, pts[1:]):
        d, t = seg_dist(p, a, b)
        if d < best[0]:
            best = (d, (acc + t * (b - a).length) / total)
        acc += (b - a).length
    return best
