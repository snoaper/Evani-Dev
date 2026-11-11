# The test horse's shape, by numbers (original work; no downloaded meshes, no
# outside libraries). Used by build_horse.py (the mesh) and rig_horse.py (the
# skeleton fit), so the bones always sit on the anatomy they move.
#
# Frame: f forward (metres, + toward the head), z up, x to the horse's left.
# Blender: the horse faces -Y (Rigify's metarig; glTF +Z).
#
# Proportions follow the classical head-length canon (HL = one head, poll to
# the front of the upper lip):
#   height at the withers 2.5 HL; body length (point of shoulder to point of
#   buttock) about the height or a little less; neck top line (poll to
#   withers) 1.5 HL, neck underline (throatlatch to the chest) 1 HL; elbow at
#   about half the withers height; knee and hock at about the same height;
#   point of buttock about level with the point of shoulder; croup (point of
#   hip to point of buttock) a third of the body length; pasterns 50-55 deg.
# LM below holds every landmark; canon_report() measures them on the mesh.
#
# How a part is made: closed lofts of cross-sections. A section lies in the
# plane through its two midline points A (top or front) and B (bottom or
# back) and the lateral axis; its right half is a list of control points
# (x metres out from the midline, t = fraction of the way from A to B), the
# left half its mirror. Sections are interpolated between keys (Catmull-Rom on
# every number), so each key is a measured slice of the horse.
import math
from mathutils import Vector, Matrix

pi = math.pi
HW = 1.60            # height at the withers
HL = HW / 2.5        # one head length (0.64 m)


def P(f, z, x=0.0):
    return Vector((x, -f, z))


# ------------------------------------------------------------- landmarks
HEAD_DEG = 48.0      # the face line below horizontal
POLL = (1.232, 1.978)
_hd = (math.cos(math.radians(HEAD_DEG)), -math.sin(math.radians(HEAD_DEG)))   # down the face
_hn = (-math.sin(math.radians(HEAD_DEG)), -math.cos(math.radians(HEAD_DEG)))  # toward the jaw


def head_pt(a, b=0.0):
    """(f, z) of the point `a` down the face line from the poll and `b` in
    toward the jaw."""
    return (POLL[0] + _hd[0] * a + _hn[0] * b, POLL[1] + _hd[1] * a + _hn[1] * b)


def head_local(p):
    """(a, b, x) of a Blender point in the head frame."""
    f, z = -p.y - POLL[0], p.z - POLL[1]
    return f * _hd[0] + z * _hd[1], f * _hn[0] + z * _hn[1], p.x


LM = {
    # trunk
    "withers": (0.42, 1.600),
    "back_low": (0.02, 1.515),
    "croup": (-0.40, 1.588),
    "point_hip": (-0.335, 1.445),      # lateral, x = 0.245
    "tail_head": (-0.705, 1.488),
    "point_buttock": (-0.758, 1.165),
    "point_shoulder": (0.800, 1.165),
    "girth": (0.40, 0.800),           # bottom of the chest behind the elbow
    # foreleg (joint centres)
    "shoulder_joint": (0.745, 1.165),
    "elbow": (0.578, 0.815),
    "point_elbow": (0.505, 0.865),
    "knee": (0.602, 0.470),
    "fore_fetlock": (0.598, 0.168),
    "fore_coronet": (0.664, 0.082),
    "fore_toe": (0.760, 0.0),
    # hind leg
    "hip_joint": (-0.555, 1.285),
    "stifle": (-0.285, 0.955),
    "hock": (-0.632, 0.505),
    "point_hock": (-0.716, 0.588),
    "hind_fetlock": (-0.636, 0.168),
    "hind_coronet": (-0.574, 0.082),
    "hind_toe": (-0.476, 0.0),
    # neck and head
    "poll": POLL,
    "throatlatch": (1.167, 1.806),
    "neck_chest": (0.880, 1.240),     # where the neck's underline meets the chest
    "muzzle": head_pt(0.640, 0.075),
    "eye": head_pt(0.200, 0.050),     # x = 0.106
}
EYE_X = 0.106
EYE_R = 0.026


# ------------------------------------------------------------- sections
def _cr(p0, p1, p2, p3, t):
    return 0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3)


def catmull(keys, s):
    """Catmull-Rom through a list of equal-length number tuples at s in
    [0, len-1]."""
    n = len(keys)
    i = min(int(math.floor(s)), n - 2)
    t = s - i
    k0, k1, k2, k3 = keys[max(i - 1, 0)], keys[i], keys[i + 1], keys[min(i + 2, n - 1)]
    return [_cr(a, b, c, d, t) for a, b, c, d in zip(k0, k1, k2, k3)]


def half_curve(pts, m):
    """The right half of a symmetric closed section through control points
    pts [(x, t)] (first and last on the midline), with the mirror image as
    the Catmull-Rom neighbours (so it crosses the midline square), resampled
    evenly by length into m + 1 points."""
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
        for j in range(16):
            tt = j / 16
            dense.append((_cr(p0[0], p1[0], p2[0], p3[0], tt), _cr(p0[1], p1[1], p2[1], p3[1], tt)))
    dense.append(pts[-1])
    return dense, m


def section(A, B, pts, n):
    """Ring of n points (n even): A, B Vectors on the midline, pts the half
    profile."""
    dense, m = half_curve(pts, n // 2)
    AB = B - A
    L = AB.length
    # resample by length measured in metres (t scaled by |AB|)
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
    X = Vector((1, 0, 0))
    ring = [A + AB * t + X * x for x, t in half]
    ring += [A + AB * t - X * x for x, t in reversed(half[1:-1])]
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


def prof_part(bm, keys, steps, n=48, xc=0.0, mirror=False):
    """keys: (fa, za, fb, zb, [(x, t), ...]); all keys with the same number
    of profile points. xc moves the part sideways (and mirror makes two)."""
    flat = [[k[0], k[1], k[2], k[3]] + [v for p in k[4] for v in p] for k in keys]
    for side in ([1, -1] if mirror else [1]):
        rings = []
        for i in range(steps + 1):
            s = i / steps * (len(keys) - 1)
            v = catmull(flat, s)
            pts = [(max(v[4 + 2 * j], 0.0), v[5 + 2 * j]) for j in range((len(v) - 4) // 2)]
            pts[0] = (0.0, pts[0][1])
            pts[-1] = (0.0, pts[-1][1])
            ring = section(P(v[0], v[1]), P(v[2], v[3]), pts, n)
            if xc:
                ring = [p + Vector((side * xc, 0, 0)) for p in ring]
            rings.append(ring)
        loft(bm, rings)


def sgnpow(v, e):
    return math.copysign(abs(v) ** e, v)


def ell_ring(A, B, xc, w, eq, pa, pb, n):
    """A limb section between silhouette points A and B: widest at eq (0 at
    B, 1 at A), half width w, superellipse exponents pa (A half), pb (B half)."""
    E = B + (A - B) * eq
    ha, hb = (A - E).length, (E - B).length
    u = (A - B).normalized()
    lat = Vector((1, 0, 0))
    pts = []
    for i in range(n):
        th = 2 * pi * i / n
        c, s = math.cos(th), math.sin(th)
        p = pa if s >= 0 else pb
        pts.append(E + lat * (xc + w * sgnpow(c, 2.0 / p)) + u * ((ha if s >= 0 else hb) * sgnpow(s, 2.0 / p)))
    return pts


def limb_part(bm, keys, steps, n=36):
    """keys: (f front, z front, f back, z back, x centre, half width, eq,
    p front, p back); both sides."""
    for side in (1, -1):
        rings = []
        for i in range(steps + 1):
            s = i / steps * (len(keys) - 1)
            fa, za, fb, zb, xc, w, eq, pa, pb = catmull(keys, s)
            rings.append(ell_ring(P(fa, za), P(fb, zb), side * xc, max(w, 0.004), min(max(eq, 0.05), 0.95), pa, pb, n))
        loft(bm, rings)


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
    lat = Vector((1, 0, 0))
    for sx in ([1, -1] if mirror else [1]):
        aa = (a + lat * (sx * math.tan(tilt_out))).normalized()
        l2 = (lat * sx - aa * aa.dot(lat * sx)).normalized() * sx
        ellipsoid(bm, P(f, z, sx * x), (aa, l2.cross(aa).normalized() * sx, l2), radii)


# ------------------------------------------------------------- trunk
# Vertical slices from the rump to the breast: f, top z, bottom z (midline),
# then half widths at the fixed fractions TT from the top (the barrel is
# widest low, the croup high and broad, the withers a narrow ridge).
TT = (0.03, 0.12, 0.30, 0.50, 0.70, 0.86, 0.96)
# v3: the underline tucks up into the flank behind the barrel (the belly
# meets the hind leg above the stifle, not beside the gaskin), and the belly
# narrows into the groin between the thighs, so the stifle and gaskin hang
# free of the belly and only the thigh joins the trunk, through the flank
# fold and the groin.
# v4: the underline rises a little more behind the girth (the belly hung
# low: 0.822 m at f 0, now 0.840; 0.800 at f 0.15, now 0.812); the girth, f
# 0.28 to 0.50, which sets the canon's chest depth, and the flank at f -0.14,
# where the stifle folds in the walk, are unchanged.
TRUNK = [
    (-0.660, 1.515, 1.175, (0.065, 0.130, 0.165, 0.160, 0.130, 0.085, 0.045)),
    (-0.540, 1.562, 1.100, (0.100, 0.195, 0.228, 0.212, 0.180, 0.128, 0.064)),
    (-0.400, 1.588, 1.050, (0.115, 0.225, 0.253, 0.244, 0.214, 0.158, 0.084)),
    (-0.270, 1.574, 1.016, (0.115, 0.228, 0.266, 0.270, 0.250, 0.186, 0.100)),
    (-0.140, 1.546, 0.890, (0.112, 0.226, 0.276, 0.290, 0.282, 0.232, 0.136)),
    (0.000, 1.516, 0.840, (0.104, 0.218, 0.282, 0.302, 0.291, 0.246, 0.146)),
    (0.150, 1.522, 0.812, (0.086, 0.200, 0.276, 0.302, 0.292, 0.248, 0.148)),
    (0.280, 1.566, 0.795, (0.058, 0.168, 0.256, 0.286, 0.280, 0.240, 0.140)),
    (0.400, 1.600, 0.800, (0.034, 0.118, 0.214, 0.256, 0.256, 0.220, 0.130)),
    (0.500, 1.586, 0.812, (0.036, 0.110, 0.196, 0.232, 0.236, 0.205, 0.120)),
    (0.600, 1.540, 0.845, (0.050, 0.110, 0.182, 0.216, 0.220, 0.190, 0.110)),
    (0.700, 1.440, 0.890, (0.070, 0.120, 0.180, 0.212, 0.214, 0.192, 0.125)),
    (0.780, 1.330, 0.950, (0.070, 0.114, 0.170, 0.200, 0.202, 0.184, 0.120)),
    (0.815, 1.280, 0.975, (0.066, 0.108, 0.156, 0.178, 0.178, 0.160, 0.102)),
    (0.836, 1.235, 1.000, (0.048, 0.082, 0.114, 0.126, 0.124, 0.110, 0.068)),
    (0.848, 1.200, 1.040, (0.020, 0.036, 0.050, 0.054, 0.052, 0.044, 0.026)),
]


def trunk_keys():
    out = []
    for f, zt, zb, ws in TRUNK:
        pts = [(0.0, 0.0)] + [(w, t) for w, t in zip(ws, TT)] + [(0.0, 1.0)]
        out.append((f, zt, f, zb, pts))
    return out


# ------------------------------------------------------------- neck
# Crest point, throat point; then a half width and the shared profile
# (fractions of the half width; the crest a narrow ridge, the widest a
# little below the middle, the windpipe rounded underneath).
NECK = [
    ((0.480, 1.585), (0.820, 1.120), 0.150),
    ((0.600, 1.578), (0.880, 1.240), 0.158),
    ((0.720, 1.628), (0.955, 1.360), 0.136),
    ((0.840, 1.718), (0.990, 1.480), 0.113),
    ((0.948, 1.828), (1.018, 1.588), 0.097),
    ((1.050, 1.905), (1.048, 1.672), 0.089),
    ((1.148, 1.960), (1.090, 1.752), 0.084),
    ((1.222, 1.975), (1.152, 1.800), 0.080),
]
NECK_PROF = ((0, 0), (0.30, 0.012), (0.64, 0.08), (0.90, 0.25), (1.0, 0.48), (0.96, 0.68), (0.80, 0.84), (0.50, 0.95), (0, 1.0))


def neck_keys():
    return [(a[0], a[1], b[0], b[1], [(x * w, t) for x, t in NECK_PROF]) for a, b, w in NECK]


# ------------------------------------------------------------- head
# Slices across the face line: a (down the face from the poll), off (the
# top in from the face line: the muzzle rounds off), D (depth to the lower
# edge of the jaw), the half profile (x metres, t fraction of D). The bottom
# midline sits a little above the jaw's edges: the channel between the
# jaws. Forehead broad and flat, eyes on the sides at its widest, the round
# jowl deepest under the eye, the face tapering to the muzzle.
HEAD = [
    (-0.035, 0.030, 0.100, ((0, 0), (0.035, 0.02), (0.056, 0.14), (0.064, 0.34), (0.064, 0.55), (0.058, 0.75), (0.044, 0.92), (0.020, 0.99), (0, 1.0))),
    (0.020, 0.000, 0.150, ((0, 0), (0.050, 0.010), (0.078, 0.090), (0.088, 0.27), (0.090, 0.47), (0.086, 0.67), (0.072, 0.86), (0.040, 0.985), (0, 1.0))),
    (0.065, 0.000, 0.178, ((0, 0), (0.060, 0.007), (0.093, 0.070), (0.104, 0.22), (0.106, 0.44), (0.102, 0.65), (0.088, 0.85), (0.052, 0.985), (0, 0.99))),
    (0.100, 0.000, 0.250, ((0, 0), (0.066, 0.005), (0.101, 0.050), (0.110, 0.17), (0.112, 0.40), (0.109, 0.62), (0.095, 0.83), (0.058, 0.985), (0, 0.90))),
    (0.135, 0.000, 0.296, ((0, 0), (0.070, 0.004), (0.106, 0.045), (0.114, 0.15), (0.114, 0.38), (0.110, 0.60), (0.096, 0.82), (0.058, 0.985), (0, 0.90))),
    (0.180, 0.000, 0.312, ((0, 0), (0.071, 0.003), (0.107, 0.040), (0.115, 0.13), (0.110, 0.32), (0.107, 0.55), (0.093, 0.80), (0.055, 0.985), (0, 0.90))),
    (0.235, 0.000, 0.298, ((0, 0), (0.066, 0.003), (0.100, 0.042), (0.108, 0.14), (0.104, 0.34), (0.101, 0.57), (0.087, 0.81), (0.050, 0.985), (0, 0.90))),
    (0.290, 0.000, 0.262, ((0, 0), (0.058, 0.004), (0.086, 0.050), (0.093, 0.16), (0.095, 0.38), (0.092, 0.60), (0.078, 0.82), (0.045, 0.980), (0, 0.90))),
    (0.350, 0.000, 0.226, ((0, 0), (0.050, 0.006), (0.074, 0.060), (0.081, 0.20), (0.083, 0.42), (0.079, 0.64), (0.066, 0.84), (0.038, 0.980), (0, 0.90))),
    (0.420, 0.000, 0.198, ((0, 0), (0.042, 0.008), (0.061, 0.070), (0.067, 0.22), (0.068, 0.45), (0.065, 0.66), (0.055, 0.85), (0.031, 0.980), (0, 0.97))),
    (0.500, 0.000, 0.184, ((0, 0), (0.041, 0.010), (0.059, 0.080), (0.064, 0.24), (0.065, 0.46), (0.062, 0.67), (0.053, 0.86), (0.029, 0.985), (0, 0.98))),
    (0.560, 0.002, 0.180, ((0, 0), (0.048, 0.012), (0.070, 0.090), (0.073, 0.25), (0.068, 0.46), (0.062, 0.66), (0.050, 0.85), (0.028, 0.980), (0, 0.99))),
    (0.600, 0.007, 0.172, ((0, 0), (0.046, 0.015), (0.066, 0.100), (0.068, 0.27), (0.063, 0.48), (0.056, 0.68), (0.045, 0.86), (0.025, 0.980), (0, 0.99))),
    (0.625, 0.017, 0.150, ((0, 0), (0.040, 0.020), (0.056, 0.120), (0.058, 0.30), (0.054, 0.50), (0.048, 0.70), (0.038, 0.87), (0.020, 0.980), (0, 0.99))),
    (0.640, 0.035, 0.100, ((0, 0), (0.025, 0.050), (0.036, 0.200), (0.038, 0.40), (0.035, 0.58), (0.030, 0.75), (0.022, 0.90), (0.012, 0.980), (0, 1.0))),
]


def head_keys():
    out = []
    for a, off, D, pts in HEAD:
        fa, za = head_pt(a, off)
        fb, zb = head_pt(a, off + D)
        out.append((fa, za, fb, zb, list(pts)))
    return out


# ------------------------------------------------------------- legs
# (f front, z front, f back, z back, x centre, half width, eq, p front, p back)
FORE = [
    (0.760, 1.300, 0.620, 1.300, 0.125, 0.040, 0.5, 2.0, 2.0),    # inside the shoulder (arm)
    (0.815, 1.165, 0.580, 1.150, 0.148, 0.072, 0.5, 2.0, 2.0),    # point of the shoulder
    (0.800, 1.060, 0.555, 1.040, 0.152, 0.082, 0.5, 2.0, 2.0),    # arm: biceps front, triceps back
    (0.745, 0.950, 0.505, 0.920, 0.170, 0.086, 0.5, 2.0, 2.0),    # point of the elbow behind
    (0.692, 0.862, 0.506, 0.850, 0.176, 0.080, 0.55, 2.0, 2.0),   # where the breast meets the forearm
    (0.668, 0.780, 0.524, 0.780, 0.176, 0.070, 0.55, 2.0, 2.0),   # forearm
    (0.656, 0.680, 0.544, 0.680, 0.173, 0.058, 0.55, 2.1, 2.0),
    (0.646, 0.580, 0.558, 0.580, 0.169, 0.048, 0.55, 2.3, 2.0),
    (0.642, 0.520, 0.562, 0.520, 0.166, 0.046, 0.5, 2.5, 2.0),
    (0.650, 0.470, 0.552, 0.470, 0.164, 0.052, 0.5, 2.8, 2.0),    # knee: flat front, the accessory carpal behind
    (0.640, 0.420, 0.566, 0.420, 0.162, 0.042, 0.5, 2.6, 2.0),
    (0.631, 0.360, 0.570, 0.360, 0.160, 0.032, 0.5, 2.4, 1.8),    # cannon: bone in front, tendons behind
    (0.628, 0.260, 0.568, 0.260, 0.158, 0.032, 0.5, 2.4, 1.8),
    (0.632, 0.200, 0.562, 0.200, 0.157, 0.037, 0.5, 2.3, 2.0),
    (0.642, 0.168, 0.546, 0.148, 0.156, 0.045, 0.5, 2.2, 2.0),    # fetlock, the ergot behind
    (0.645, 0.160, 0.593, 0.120, 0.155, 0.037, 0.5, 2.0, 2.0),    # pastern (52.5 deg)
    (0.670, 0.129, 0.618, 0.089, 0.155, 0.039, 0.5, 2.0, 2.0),
    (0.693, 0.102, 0.637, 0.060, 0.155, 0.047, 0.5, 2.0, 2.0),    # coronet
]
# v3: the back line is no longer one straight edge from the buttock to the
# hock: the hamstrings bulge behind the thigh, sweep in under it into the
# gaskin, run down the Achilles cord (concave, a pointed back) and stand out
# again at the point of the hock; in front, the stifle is a rounded knob
# below the flank and the gaskin narrows to the hock (widest toward the
# front, eq > 0.5).
HIND = [
    (-0.300, 1.280, -0.600, 1.340, 0.155, 0.105, 0.5, 2.0, 2.0),  # inside the hip
    (-0.255, 1.120, -0.760, 1.165, 0.168, 0.138, 0.48, 2.0, 2.2),  # thigh: flank front, point of the buttock back
    (-0.234, 1.040, -0.780, 1.085, 0.178, 0.132, 0.46, 2.0, 2.2),  # the hamstrings bulge behind
    (-0.214, 0.960, -0.773, 0.990, 0.186, 0.120, 0.50, 2.3, 2.1),  # stifle: a rounded knob in front, below the flank
    (-0.260, 0.900, -0.748, 0.905, 0.182, 0.102, 0.54, 2.1, 2.0),  # the leg narrows under the stifle
    (-0.318, 0.850, -0.713, 0.832, 0.173, 0.086, 0.56, 2.0, 1.9),  # gaskin: the hamstrings sweep in
    (-0.386, 0.777, -0.692, 0.765, 0.164, 0.068, 0.60, 2.0, 1.7),
    (-0.453, 0.706, -0.681, 0.702, 0.158, 0.054, 0.63, 2.0, 1.5),  # the Achilles cord behind
    (-0.515, 0.642, -0.690, 0.645, 0.154, 0.047, 0.62, 2.0, 1.5),
    (-0.563, 0.585, -0.725, 0.590, 0.151, 0.048, 0.58, 2.2, 1.7),  # hock: point of the hock behind
    (-0.588, 0.525, -0.700, 0.535, 0.149, 0.051, 0.5, 2.5, 2.0),
    (-0.597, 0.450, -0.676, 0.460, 0.147, 0.042, 0.5, 2.5, 2.0),
    (-0.604, 0.380, -0.672, 0.380, 0.146, 0.033, 0.5, 2.4, 1.8),  # cannon (vertical)
    (-0.604, 0.270, -0.670, 0.270, 0.145, 0.033, 0.5, 2.4, 1.8),
    (-0.602, 0.200, -0.670, 0.200, 0.145, 0.037, 0.5, 2.3, 2.0),
    (-0.596, 0.168, -0.686, 0.148, 0.145, 0.045, 0.5, 2.2, 2.0),  # fetlock
    (-0.589, 0.159, -0.643, 0.121, 0.145, 0.036, 0.5, 2.0, 2.0),  # pastern (54 deg)
    (-0.565, 0.126, -0.619, 0.088, 0.145, 0.038, 0.5, 2.0, 2.0),
    (-0.544, 0.099, -0.600, 0.059, 0.145, 0.045, 0.5, 2.0, 2.0),  # coronet
]
# Hooves: ground centre f, x, half width, half length, toe and heel heights.
HOOVES = [(0.694, 0.155, 0.062, 0.066, 0.090, 0.042), (-0.540, 0.145, 0.057, 0.064, 0.090, 0.045)]
# Tail dock: centre line (f, z) and radius.
DOCK = [(-0.690, 1.490, 0.050), (-0.770, 1.468, 0.047), (-0.826, 1.415, 0.043), (-0.853, 1.335, 0.038),
        (-0.864, 1.250, 0.033), (-0.868, 1.172, 0.027), (-0.869, 1.130, 0.018)]

# Muscle masses: (f, z, x, radii (long, across, lateral), long axis (f, z))
# The breast: one broad pectoral mass across the front (a slight groove down
# its middle comes from the carve), between and above the forearms.
# v3: one broad mass across the breast (the two pectorals, a shallow groove
# down the middle from the carve) fills the front between the arms, so from
# the front the chest reads broad and smooth instead of two round arm tops.
# ((f, z), (vertical, front-back, lateral) radii, long axis (f, z))
BREAST = ((0.722, 0.985), (0.092, 0.185, 0.140), (0.42, 0.91))   # v4: lateral 0.160 -> 0.140 (its round side was the ball)
# v4: over the (narrower) breast, two pectoral masses, each running from the
# breastbone up and out to the point of its shoulder (f, z, x, radii (long, front-back,
# lateral), long axis (f, z), tilt out of the side plane): their upper ends
# run into the arms, where v3's broad breast showed its round side in front
# of the shoulder as a ball in the three-quarter view.
PECTORALS = [(0.752, 1.025, 0.068, (0.170, 0.090, 0.085), (0.30, 0.95), 0.55)]
MUSCLES = [
    (0.635, 1.290, 0.135, (0.215, 0.150, 0.052), (0.70, -0.72)),   # shoulder: scapula and its muscles
    (0.555, 1.060, 0.160, (0.130, 0.100, 0.055), (0.15, -1.0)),    # triceps, above the elbow
    (-0.470, 1.440, 0.120, (0.230, 0.100, 0.075), (1.0, -0.20)),   # gluteals over the croup
    (-0.575, 1.215, 0.165, (0.230, 0.178, 0.100), (0.30, -1.0)),   # hindquarter: biceps femoris (v4: lateral 0.110 -> 0.100)
    (-0.700, 1.040, 0.112, (0.200, 0.066, 0.064), (0.12, -1.0)),   # semitendinosus: the bulge behind the thigh (v4: softer, it read lumpy from behind)
    (-0.282, 1.045, 0.194, (0.125, 0.078, 0.060), (0.40, -0.92)),  # quadriceps, in front of the femur, down to the stifle
    (-0.650, 0.845, 0.162, (0.085, 0.048, 0.062), (0.25, -1.0)),   # gaskin: the gastrocnemius, behind
    (-0.345, 1.410, 0.205, (0.110, 0.080, 0.032), (1.0, -0.1)),    # point of the hip
]

# v4: the quad layout. QuadriFlow lays its quads out from the whole surface,
# so any change to the shape reshuffles every quad, and the walk's worst
# stretched edges (rig_check) move by up to 1x with them. v4 keeps v3's
# layout: build_horse remeshes the v3 surface below (seed 7, the layout the
# rig check limits were measured on) and then projects those quads onto the
# v4 shape above.
LAYOUT_V3 = dict(
    TRUNK=[
    (-0.660, 1.515, 1.175, (0.065, 0.130, 0.165, 0.160, 0.130, 0.085, 0.045)),
    (-0.540, 1.562, 1.100, (0.100, 0.195, 0.228, 0.212, 0.180, 0.128, 0.064)),
    (-0.400, 1.588, 1.050, (0.115, 0.225, 0.253, 0.244, 0.214, 0.158, 0.084)),
    (-0.270, 1.574, 1.016, (0.115, 0.228, 0.266, 0.270, 0.250, 0.186, 0.100)),
    (-0.140, 1.546, 0.890, (0.112, 0.226, 0.276, 0.290, 0.282, 0.232, 0.136)),
    (0.000, 1.516, 0.822, (0.104, 0.218, 0.282, 0.302, 0.292, 0.250, 0.150)),
    (0.150, 1.522, 0.800, (0.086, 0.200, 0.276, 0.302, 0.292, 0.250, 0.150)),
    (0.280, 1.566, 0.795, (0.058, 0.168, 0.256, 0.286, 0.280, 0.240, 0.140)),
    (0.400, 1.600, 0.800, (0.034, 0.118, 0.214, 0.256, 0.256, 0.220, 0.130)),
    (0.500, 1.586, 0.812, (0.036, 0.110, 0.196, 0.232, 0.236, 0.205, 0.120)),
    (0.600, 1.540, 0.845, (0.050, 0.110, 0.182, 0.216, 0.220, 0.190, 0.110)),
    (0.700, 1.440, 0.890, (0.070, 0.120, 0.180, 0.212, 0.214, 0.192, 0.125)),
    (0.780, 1.330, 0.950, (0.070, 0.114, 0.170, 0.200, 0.202, 0.184, 0.120)),
    (0.815, 1.280, 0.975, (0.066, 0.108, 0.156, 0.178, 0.178, 0.160, 0.102)),
    (0.836, 1.235, 1.000, (0.048, 0.082, 0.114, 0.126, 0.124, 0.110, 0.068)),
    (0.848, 1.200, 1.040, (0.020, 0.036, 0.050, 0.054, 0.052, 0.044, 0.026)),
],
    MUSCLES=[
    (0.635, 1.290, 0.135, (0.215, 0.150, 0.052), (0.70, -0.72)),   # shoulder: scapula and its muscles
    (0.555, 1.060, 0.160, (0.130, 0.100, 0.055), (0.15, -1.0)),    # triceps, above the elbow
    (-0.470, 1.440, 0.120, (0.230, 0.100, 0.075), (1.0, -0.20)),   # gluteals over the croup
    (-0.575, 1.215, 0.170, (0.230, 0.178, 0.110), (0.30, -1.0)),   # hindquarter: biceps femoris
    (-0.700, 1.040, 0.120, (0.190, 0.072, 0.072), (0.12, -1.0)),   # semitendinosus: the bulge behind the thigh
    (-0.282, 1.045, 0.194, (0.125, 0.078, 0.060), (0.40, -0.92)),  # quadriceps, in front of the femur, down to the stifle
    (-0.650, 0.845, 0.162, (0.085, 0.048, 0.062), (0.25, -1.0)),   # gaskin: the gastrocnemius, behind
    (-0.345, 1.410, 0.205, (0.110, 0.080, 0.032), (1.0, -0.1)),    # point of the hip
],
    BREAST=((0.722, 0.985), (0.092, 0.185, 0.160), (0.42, 0.91)),
    PECTORALS=[],
)
