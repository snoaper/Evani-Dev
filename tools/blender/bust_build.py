"""Builds the material viewer's bust (apps/material-view, model id `char-head`).

Run headless with the pinned Blender (docs/stage3/spikes.md):

    blender -b --factory-startup -t 2 --python tools/blender/bust_build.py -- <out.glb>
    cargo xtask convert <out.glb> --id char-head --tiers low,medium,high --out <dir>

Critic rounds 1-4 found the old bust (tools/models/char-bust.mjs) crude: a
bowl-cut cap with a jagged fringe of lock cards, small eyes, a pale nostril
fleck and a thin mouth line, built from loose ellipsoids. This one is our own
adult head (no real person's likeness), sculpted as signed distance fields in
numpy with the face's anatomy laid out by proportion (eyes half-way down the
head, a nose base one eye-width under them, lips with a cupid's bow, a
philtrum and a groove under the lower lip, almond lids that wrap the
eyeballs), meshed closed and manifold with OpenVDB (bundled with Blender),
and simplified with Blender's decimate. The hair (critic rounds 6-10 read the
old slicked-back bun as a combed helmet) is a soft layered crop: broad locks
grow from a whorl at the crown and fall over the forehead into a loose
fringe, over the temples and the tops of the ears and down to the nape, in
two layers whose tips end at staggered lengths, so there is no hairline edge
and no bare scalp at the sides; the lower face is one broad soft mass under
the cheeks, so the jaw rounds from ear to chin.

Metres, the head's centre at the origin, facing +z, +y up (glTF axes). Parts
and materials, in this order (the viewer's test checks the kinds):

* head (material `skin`, evani_kind skin): head, ears, neck and lids, one
  closed surface. UVs: equirect round the head's centre (u = longitude /
  2 pi, v = polar angle / 2 pi), so a UV unit is about as long both ways.
* eyes (material `eye`): two closed spheres behind the lids, looking along
  +z, azimuthal UVs (0.5, 0.5) on the pupil axis.
* hair (material `hair`, evani_kind hair): the layered hair, the brows and
  the upper lash lines, each closed. u: one whole step a lock round the
  whorl (its wrap seam in a groove at the back); v: 0.08 at the whorl, 0.94
  10 mm before a lock's tip and 1 at the tip (the shader fades only that
  last stretch, so each tip thins out soft over the skin).
  Single-sided: every hair part is closed, so no back face ever shows.
* hair_base (material `hair_base`, untagged): the hair volume shrunk by
  1.5 mm, closed and dark, so the seams between locks never show skin or sky;
  it sinks under the skin before the tips part.
* top (material `top`): shoulders and chest in a round-necked top.
* socle (material `stone`): a turned socle; its foot is the model's lowest
  point, SOCLE_FOOT under the head's centre.
* lips (material `lips`, evani_kind skin): a closed solid piece over the
  lips, 0.35 mm proud inside the vermilion and sinking under the skin just
  past it; the renderer reads no vertex colours, so the lips' rose is a part
  of its own (as the brows are), coloured by a small texture that is the
  skin's own colour at the piece's edge (planar UVs over LIP_BOX), so the
  lips blend into the face.

The output is the same on every run (no random numbers; OpenVDB's mesher
orders its output deterministically for one grid).
"""

import json
import math
import os
import struct
import sys
import time

import numpy as np
import openvdb

T0 = time.time()


def log(*a):
    print(f"[bust {time.time() - T0:6.1f}s]", *a, flush=True)


# ---- field primitives (numpy arrays in, arrays out) ---------------------------
def ell(x, y, z, c, r):
    """Ellipsoid, the usual bound (exact on its surface)."""
    px, py, pz = (x - c[0]) / r[0], (y - c[1]) / r[1], (z - c[2]) / r[2]
    k0 = np.sqrt(px * px + py * py + pz * pz)
    k1 = np.sqrt((px / r[0]) ** 2 + (py / r[1]) ** 2 + (pz / r[2]) ** 2)
    return k0 * (k0 - 1.0) / np.maximum(k1, 1e-9)


def cap(x, y, z, a, b, ra, rb):
    """A capsule from a (radius ra) to b (radius rb)."""
    px, py, pz = x - a[0], y - a[1], z - a[2]
    dx, dy, dz = b[0] - a[0], b[1] - a[1], b[2] - a[2]
    h = np.clip((px * dx + py * dy + pz * dz) / (dx * dx + dy * dy + dz * dz), 0.0, 1.0)
    qx, qy, qz = px - dx * h, py - dy * h, pz - dz * h
    return np.sqrt(qx * qx + qy * qy + qz * qz) - (ra + (rb - ra) * h)


def smin(a, b, k):
    h = np.maximum(k - np.abs(a - b), 0.0) / k
    return np.minimum(a, b) - h * h * k * 0.25


def smax(a, b, k):
    return -smin(-a, -b, k)


def ssub(a, b, k):
    """`a` with `b` carved out, the edge rounded over `k`."""
    return smax(a, -b, k)


def sstep(a, b, x):
    t = np.clip((x - a) / (b - a), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def rot_x(y, z, deg):
    c, s = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return c * y - s * z, s * y + c * z


# ---- proportions --------------------------------------------------------------
EYE_R = 0.0118
EYE_X, EYE_Y, EYE_Z = 0.0318, 0.010, 0.0750
#: The almond opening, in the eye's own (lateral, up) coordinates.
EYE_W = 0.0148          # half the opening's width
EYE_UP, EYE_DN = 0.0066, 0.0053
BEND = 21.0             # the mouth wraps round the teeth: z + BEND x^2
MOUTH_Y = -0.0545       # where the lips meet


def almond(ex, ey):
    """Signed distance-ish to the eye opening (negative inside), ex lateral."""
    ey = ey - 0.07 * ex  # the outer corner a little higher than the inner
    t = np.clip(ex / EYE_W, -1.2, 1.2)
    k = np.maximum(1.0 - t * t, 0.0)
    up = EYE_UP * np.sqrt(k) * (1.0 - 0.28 * t) * (1.0 + 0.10 * (1 - k))
    dn = -EYE_DN * np.power(k, 0.62) * (1.0 + 0.22 * t)
    a = np.maximum(ey - up, dn - ey)
    return np.maximum(a, (np.abs(ex) - EYE_W) * 0.6)


def _ear_rim():
    """The helix's path (x out, y up, z forward in the ear's frame, and a
    radius), a Catmull-Rom curve through a few points, finely sampled."""
    k = [(0.0712, 0.012, 0.011, 0.0022), (0.0735, 0.024, 0.004, 0.0026), (0.0770, 0.0255, -0.006, 0.0028),
         (0.0805, 0.016, -0.0145, 0.0028), (0.0815, 0.001, -0.0165, 0.0027), (0.0800, -0.013, -0.013, 0.0025),
         (0.0765, -0.022, -0.006, 0.0022)]
    k = [k[0]] + k + [k[-1]]
    out = []
    for i in range(1, len(k) - 2):
        p0, p1, p2, p3 = (np.array(k[j]) for j in (i - 1, i, i + 1, i + 2))
        for t in np.linspace(0, 1, 6, endpoint=False):
            out.append(tuple(0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3)))
    out.append(k[-2])
    return out


EAR_RIM = _ear_rim()


def head_field(x, y, z, ears=True):
    """The head and neck without eye sockets (negative inside)."""
    ax = np.abs(x)
    # Cranium, a rounded forehead and the face's mass.
    d = ell(x, y, z, (0, 0.026, -0.014), (0.0765, 0.088, 0.095))
    d = smin(d, ell(x, y, z, (0, 0.040, 0.026), (0.064, 0.058, 0.060)), 0.03)
    d = smin(d, ell(x, y, z, (0, -0.018, 0.030), (0.063, 0.062, 0.064)), 0.03)
    # Cheekbones high and soft, full cheeks.
    d = smin(d, ell(ax, y, z, (0.046, -0.002, 0.052), (0.022, 0.015, 0.024)), 0.022)
    d = smin(d, ell(ax, y, z, (0.036, -0.032, 0.056), (0.026, 0.026, 0.024)), 0.028)
    d = smin(d, ell(ax, y, z, (0.030, -0.018, 0.070), (0.017, 0.014, 0.016)), 0.018)
    # The lower face: one broad soft mass under the cheeks, so the jaw
    # rounds from ear to chin in a U, not a V of flat planes (critic rounds
    # 6-10: blocky jaw and cheek planes).
    d = smin(d, ell(x, y, z, (0, -0.050, 0.032), (0.050, 0.043, 0.051)), 0.030)
    # The jaw: from under the ear to the chin, and a rounded chin, blended
    # wide (rounds 7 and 8: blocky from the front at 0.028 and 0.02).
    d = smin(d, cap(ax, y, z, (0.050, -0.034, -0.006), (0.024, -0.080, 0.056), 0.023, 0.020), 0.046)
    d = smin(d, ell(x, y, z, (0, -0.083, 0.073), (0.020, 0.017, 0.020)), 0.028)
    # The muzzle over the teeth, so the cheeks run into the lips.
    d = smin(d, ell(x, y, z, (0, -0.050, 0.072), (0.030, 0.026, 0.028)), 0.02)
    # Brow: a soft ridge, lower and fuller over the inner eye.
    d = smin(d, ell(ax, y, z, (0.026, 0.030, 0.077), (0.026, 0.0095, 0.012)), 0.014)
    d = smin(d, ell(x, y, z, (0, 0.033, 0.082), (0.016, 0.012, 0.010)), 0.012)
    # The nose: bridge, tip, wings; nostrils open downward under the tip.
    nose = cap(x, y, z, (0, 0.022, 0.089), (0, -0.020, 0.1115), 0.0058, 0.0072)
    nose = smin(nose, ell(x, y, z, (0, -0.0255, 0.1110), (0.0088, 0.0080, 0.0088)), 0.006)
    nose = smin(nose, ell(ax, y, z, (0.0106, -0.0302, 0.0980), (0.0066, 0.0058, 0.0080)), 0.0075)
    # The columella: a rounded bridge slanting from the tip's underside back
    # to where it meets the lip (critic round 5: an ellipsoid hung 5 mm in
    # front of the lip there and meshed into a pale cone under the nose).
    nose = smin(nose, cap(x, y, z, (0, -0.0290, 0.1078), (0, -0.0368, 0.0990), 0.0030, 0.0034), 0.005)
    d = smin(d, nose, 0.0095)
    # Nostrils: shallow soft openings in the underside, between the
    # columella and each wing (deep pits left pale crumbs inside).
    d = ssub(d, ell(ax, y, z, (0.0060, -0.0336, 0.1008), (0.0030, 0.0012, 0.0042)), 0.0024)
    # The mouth, in coordinates that follow the curve of the teeth.
    mz = z + BEND * x * x
    # Philtrum: a groove between two soft ridges.
    d = ssub(d, cap(x, y, mz, (0, -0.0380, 0.0998), (0, -0.0465, 0.1014), 0.0016, 0.0020), 0.0030)
    # Lips: the upper with a cupid's bow and a small bead, the lower fuller.
    bow = 0.0011 * np.exp(-((ax - 0.0045) / 0.003) ** 2)
    # Lips: rolled forward and out of the muzzle, the border a crisp edge.
    # Set back into the muzzle and blended wide, so they swell out of the
    # face instead of sitting on it (critic round 5: pasted-on pads).
    up = ell(x, y - bow, mz, (0, -0.0502, 0.0980), (0.0232, 0.0060, 0.0088))
    up = smin(up, ell(x, y, mz, (0, -0.0527, 0.1010), (0.0058, 0.0028, 0.0042)), 0.003)
    lo = ell(x, y, mz, (0, -0.0603, 0.0970), (0.0198, 0.0076, 0.0088))
    lips = smin(up, lo, 0.002)
    d = smin(d, lips, 0.013)
    # Where the lips meet, the corners' soft hollows and the groove under
    # the lower lip: pushed into the surface as smooth dents. A carved
    # slit 0.7 mm tall was finer than the triangles and meshed into a torn
    # pale band, and a carved ellipsoid's smoothed edge rises into a rim
    # (nicks at the corners, a stray line under the lip, critic round 5).
    # Only near the surface, so nothing deeper moves.
    lift = 0.0008 * (x / 0.0232) ** 2
    near = sstep(0.006, 0.0, np.abs(d))
    line = 0.0011 * np.exp(-((y - lift - MOUTH_Y) / 0.0010) ** 2) * sstep(0.025, 0.017, ax)
    corner = 0.0007 * np.exp(-((ax - 0.0225) / 0.0035) ** 2 - ((y - MOUTH_Y) / 0.0035) ** 2)
    groove = 0.0009 * np.exp(-((y + 0.0695) / 0.0035) ** 2 - (x / 0.015) ** 2)
    d = d + (line + corner + groove) * near
    if ears:
        # Ears: a flap leaning back, a rolled rim, a bowl and a lobe.
        # Ears: a flap standing out from the head towards its back, a rolled
        # rim (helix) round its top and back, a bowl (concha) and a lobe.
        cy0, cz0 = -0.002, -0.012
        ey_, ez_ = rot_x(y - cy0, z - cz0, -14)
        ear = ell(ax, ey_, ez_, (0.0715, 0.0, 0.0), (0.0075, 0.0280, 0.0170))
        rim = None
        for a, b in zip(EAR_RIM[:-1], EAR_RIM[1:]):
            c = cap(ax, ey_, ez_, a, b, a[3], b[3])
            rim = c if rim is None else np.minimum(rim, c)
        ear = smin(ear, rim, 0.003)
        ear = smin(ear, ell(ax, ey_, ez_, (0.0735, -0.024, -0.002), (0.0050, 0.0075, 0.0065)), 0.004)
        ear = ssub(ear, ell(ax, ey_, ez_, (0.0815, -0.004, -0.0035), (0.0050, 0.0095, 0.0070)), 0.0025)
        ear = ssub(ear, ell(ax, ey_, ez_, (0.0825, 0.010, -0.007), (0.0030, 0.0110, 0.0060)), 0.0015)
        d = smin(d, ear, 0.005)
    # The neck, oval, leaning a little forward.
    nz = z + 0.020 + 0.07 * (y + 0.06)
    neck = (np.sqrt((x / 0.050) ** 2 + (nz / 0.047) ** 2) - 1.0) * 0.047
    d = smin(d, np.maximum(neck, y + 0.035), 0.022)
    # Closed at the bottom, inside the top, where nothing shows.
    return smax(d, -(y + 0.215), 0.006)


def eye_local(x, y):
    sx = np.where(x >= 0, 1.0, -1.0)
    return (np.abs(x) - EYE_X), (y - EYE_Y), sx


def skin_field(x, y, z):
    d = head_field(x, y, z)
    ex, ey, _ = eye_local(x, y)
    dz = z - EYE_Z
    r = np.sqrt(ex * ex + ey * ey + dz * dz)
    a = almond(ex, ey)
    # The socket: a bowl a little above the eye's centre, deeper under the
    # brow (the upper lid's fold) than over the cheek.
    # Round each eye the face becomes the lids' outer surface, a flattened
    # dome over the eyeball (2.9 mm in front of it, wider than it is deep),
    # blending back into brow and cheek further out (behind the eye's
    # centre the head stays solid, so nothing hollows).
    rho = np.sqrt(ex * ex + (1.15 * ey) ** 2)
    w = sstep(0.021, 0.0085, rho) * sstep(-0.012, -0.004, dz)
    dome = ell(ex, ey, dz, (0.0, 0.0005, 0.0), (0.0190, 0.0168, EYE_R + 0.0029))
    lidball = smin(dome, np.maximum(d, dz), 0.002)
    d = d + w * (lidball - d)
    # The opening in front of the eyeball.
    d = ssub(d, np.maximum(a, -dz), 0.0008)
    # Lids: a shell round the eyeball, outside the opening, rounded at the
    # margin; then the eyeball's own room.
    shell = np.abs(r - (EYE_R + 0.0016)) - 0.0011
    lids = smax(shell, -a, 0.0007)
    lids = np.maximum(lids, -dz - 0.006)
    d = smin(d, lids, 0.0016)
    # The fold above the upper lid, under the brow.
    t = np.clip(ex / (EYE_W + 0.005), -1.0, 1.0)
    cy = 0.07 * ex + (EYE_UP + 0.0040) * np.sqrt(np.maximum(1.0 - t * t, 0.0)) * (1.0 - 0.2 * t)
    # A shallow groove on the surface itself (never deeper than 0.6 mm).
    fold = 0.0006 * np.exp(-((ey - cy) / 0.0013) ** 2) * sstep(EYE_W + 0.004, EYE_W - 0.004, np.abs(ex))
    d = d + fold * sstep(0.004, 0.0, np.abs(d))
    d = ssub(d, r - (EYE_R + 0.0005), 0.0004)
    return d


# ---- hair ---------------------------------------------------------------------
# Critic rounds 6-10 read the old slicked-back bun as a combed helmet: a hard
# hairline, no volume, and a straight edge above the ear with bare scalp under
# it. This hair is a soft, layered crop. It grows from a whorl at the crown
# and falls in every direction: forward over the hairline into a loose fringe
# on the forehead, down over the temples (short sideburns in front of the
# ears), over the tops of the ears and down the back of the head to the nape.
# It stands well off the skull (fuller over the top and front), it is built
# from broad locks with deep grooves between them, and the hair never ends on
# one line: each lock tapers to its own pointed tip at its own length, lying
# on the skin, so the edge is a row of staggered tips. The real hairline sits
# under the hair and never shows.
#: The whorl: locks fan out round the axis from HAIR_O through the crown.
HAIR_O = np.array([0.0, 0.020, -0.020])
_HA = np.array([0.0, 1.0, -0.45])
_HA = _HA / np.linalg.norm(_HA)
_HE1 = np.array([0.0, 0.0, 1.0]) - _HA * _HA[2]
_HE1 = _HE1 / np.linalg.norm(_HE1)      # bearing 0: towards the face
_HE2 = np.cross(_HA, _HE1)
#: Locks round the whorl (one shader lock each, u steps of one).
NLOCK = 56
#: Arc radius used to turn the whorl's angles into lengths on the head.
HAIR_R = 0.092
#: The locks' sideways sweep (radians of bearing at the tip): the fringe
#: falls across the forehead, not straight down.
SWEEP = 0.12
#: The tips' taper (metres), the stretch over which the shader fades a tip
#: out (TIP_FADE: the last 6 % of v) and the rounded end.
TIP_LEN = 0.008
TIP_FADE = 0.010
END_R = 0.0040

#: Where the hair ends, by bearing round the whorl: points on the skin (x>=0
#: side; mirrored, the fringe made uneven by FRINGE_TILT): a fringe on the
#: forehead, the temples, a short sideburn in front of the ear, over the top
#: of the ear, behind it and down to the nape.
_EDGE = [(0.000, 0.060, 0.082), (0.022, 0.060, 0.078), (0.040, 0.052, 0.064), (0.056, 0.036, 0.050),
         (0.066, 0.022, 0.034), (0.071, 0.016, 0.016), (0.075, 0.018, -0.012), (0.073, 0.014, -0.032),
         (0.069, 0.000, -0.046), (0.062, -0.026, -0.060), (0.046, -0.050, -0.074), (0.000, -0.060, -0.082)]
FRINGE_TILT = 0.10


def _whorl_angles(x, y, z):
    px, py, pz = x - HAIR_O[0], y - HAIR_O[1], z - HAIR_O[2]
    a = px * _HA[0] + py * _HA[1] + pz * _HA[2]
    e1 = px * _HE1[0] + py * _HE1[1] + pz * _HE1[2]
    e2 = px * _HE2[0] + py * _HE2[1] + pz * _HE2[2]
    th = np.arctan2(np.sqrt(e1 * e1 + e2 * e2), a)
    return th, np.arctan2(e2, e1)


def _edge_table():
    pts = [(x, y, z) for x, y, z in _EDGE] + [(-x, y, z) for x, y, z in _EDGE if x > 0]
    P = np.array(pts)
    th, al = _whorl_angles(P[:, 0], P[:, 1], P[:, 2])
    o = np.argsort(al)
    al, th = al[o], th[o]
    # Periodic: wrap one point each way.
    al = np.concatenate([al[-1:] - 2 * math.pi, al, al[:1] + 2 * math.pi])
    th = np.concatenate([th[-1:], th, th[:1]])
    return al, th


_EDGE_AL, _EDGE_TH = _edge_table()


def hair_end(al):
    """The whorl angle at which the hair ends along bearing `al`."""
    t = np.interp(al, _EDGE_AL, _EDGE_TH)
    # The fringe a little longer on one side: it sweeps across the brow.
    return t + FRINGE_TILT * np.sin(al) * np.exp(-(al / 0.9) ** 2)


def _hash1(i, k=0.0):
    s = np.sin(np.mod(i, NLOCK) * 12.9898 + k * 78.233) * 43758.5453
    return s - np.floor(s)


def _lock_value(u, k):
    """A per-lock random value (0..1), the lock's own at its middle and
    blended smoothly across the grooves: a field that jumps between locks
    throws the surface projection off (crossing triangles)."""
    uc = u - 0.5
    i0 = np.floor(uc)
    f = uc - i0
    f = f * f * (3.0 - 2.0 * f)
    return _hash1(i0, k) * (1.0 - f) + _hash1(i0 + 1.0, k) * f


def _front(al):
    """1 over the forehead and temples (bearing up to about 60 degrees from
    the face), easing to 0 by the ears."""
    return sstep(0.0, 0.5, np.cos(al))


def hair_coords(x, y, z, shift=0.0, k=0.0):
    """(u, w, s): u round the whorl in locks (whole steps at the grooves,
    swept and unevenly spaced, `shift` locks along for the under-layer), w
    from the whorl (0) to the lock's tip (1) and s the distance past the tip
    in metres (negative before it). `k` picks the layer's lock lengths."""
    th, al = _whorl_angles(x, y, z)
    end = hair_end(al)
    w0 = th / end
    # Sweep the locks sideways towards their tips; uneven lock widths
    # (whole periods round the whorl, so u stays continuous). u runs from
    # the back of the head (its wrap seam in a groove there).
    a2 = np.mod(al + SWEEP * np.clip(w0, 0, 1.2) ** 2 + math.pi, 2 * math.pi)
    u = a2 * (NLOCK / (2 * math.pi)) + 0.28 * np.sin(a2 * 5.0) + 0.17 * np.sin(a2 * 11.0) + 0.09 * np.sin(a2 * 19.0) + shift
    lo, span = (0.95, 0.12) if k == 0.0 else (0.89, 0.08)
    # Over the forehead the lengths vary far less (a fringe cut nearly
    # level): on Low, one sample a pixel cuts the tips hard, and deep notches
    # between them read as a torn, see-through fringe (critic round 11).
    # (Round 12: the whole front of the head to the temples, not only the
    # middle of the forehead, where the deep teeth stayed on Low.)
    calm = 1.0 - 0.85 * _front(al)
    ln = lo + span * 0.5 + span * calm * (_lock_value(u, 4.0 + k) - 0.5)
    w = w0 / ln
    s = (th - end * ln) * HAIR_R
    return u, w, s


def _layer(x, y, z, th, al, shift, k, scale):
    u, w, s = hair_coords(x, y, z, shift, k)
    lock = np.floor(u)
    # Volume: full over the front and top, a little less at the sides and
    # back, flatter at the whorl, thinning towards the tips.
    front = np.maximum(np.cos(al), 0.0) ** 1.5
    body = 0.0125 + 0.0095 * front
    t = 0.0085 + (body - 0.0085) * sstep(0.02, 0.38, w)
    t = t * (1.0 - 0.70 * sstep(0.60, 1.0, w))
    # Each lock its own fullness (none at the whorl, where locks are tiny).
    t = scale * t * (1.0 + (0.16 * _lock_value(u, 2.0 + k) - 0.08) * sstep(0.08, 0.35, w))
    # The lock's outline: full width along its length, tapering to a point
    # over its last TIP_LEN, with a rounded end.
    lw = (2 * math.pi / NLOCK) * HAIR_R * np.maximum(np.sin(th), 0.25)
    c = (u - lock - 0.5) * lw
    # The fringe's tips are short: its notches stay shallow (round 11).
    tip_len = TIP_LEN * (1.0 - 0.7 * _front(al))
    taper = np.clip(-s / tip_len, 0.0, 1.0) ** 0.8
    # Never thinner than a fifth of a lock: the converter's simplified
    # levels folded paper-thin points into non-manifold edges (geocheck).
    hw = 0.5 * lw * np.maximum(taper, 0.2)
    outline = np.maximum(np.abs(c) - hw, s)
    # Across the lock: a rounded ridge. The grooves deepen to the skin
    # before the taper starts, so the tips part with no step at their sides.
    r = np.maximum(1.0 - (c / np.maximum(hw, 1e-6)) ** 2, 0.0)
    # (Ridges only where a lock is wide enough to mesh: near the whorl the
    # locks are narrower than the grid and their grooves speckled.)
    kk = 0.09 * sstep(0.06, 0.30, w) * sstep(0.0025, 0.0055, (2 * math.pi / NLOCK) * HAIR_R * np.sin(th))
    kk = kk + (1.0 - kk) * sstep(-2.4 * tip_len, -1.05 * tip_len, s)
    ridge = 1.0 - kk + kk * r
    cap_ = sstep(0.0, END_R, -s)
    # Outside the outline the surface dives under the skin steeply, so no
    # thin wedge of hair runs on under it. (A gentler 1.6:1 dive and a
    # bell-shaped lock put hair within a few millimetres of the skin over a
    # wider band, and the Lite/Low sun bake read the skin under it lit:
    # `baked_sun_draws_no_smear_on_any_model` failed on Medium by 0.2 points.)
    return np.where(outline < 0.0, t * ridge * cap_, -3.0 * outline), s


def hair_thickness(x, y, z):
    """How far the hair stands off the skin (negative: under the skin, by
    how far outside a lock's outline the point is), and s of the top layer.
    One layer of locks, each its own length, so the edge is staggered tips,
    not one row of teeth. (A shorter under-layer half a lock along, its tips
    in the top layer's notches, left narrow slots that the converter's
    simplified Medium level folded into non-manifold edges.) Each notch
    between two tips opens wide (TIP_LEN short against the lock's width)
    for the same reason."""
    th, al = _whorl_angles(x, y, z)
    ta, sa = _layer(x, y, z, th, al, 0.0, 0.0, 1.0)
    return ta - 0.0002, sa


def hair_field(x, y, z):
    """The hair volume, a shell over the scalp in layered locks."""
    base = head_field(x, y, z, ears=False)
    thick, _ = hair_thickness(x, y, z)
    h = base - thick
    # A shell over the scalp, not the whole head.
    return smax(h, -(base + 0.006), 0.002)


def scalp_field(x, y, z):
    """The hair's dark base: the hair volume shrunk by 1.5 mm, so the
    sub-pixel seams between locks show dark hair, not skin or sky. It sinks
    under the skin before the tips part (the tips fade to clear in the
    shader and must show skin, not a dark rim)."""
    base = head_field(x, y, z, ears=False)
    thick, s = hair_thickness(x, y, z)
    h = smax(base - thick, -(base + 0.006), 0.002)
    # (Round 12: sooner and deeper. On the simplified levels its big triangles
    # bridged the notches between the tips and drew black wedges on the skin.)
    return h + 0.0015 + 0.012 * sstep(-3.0 * TIP_LEN, -1.6 * TIP_LEN, s)


def hair_uv(P):
    """u: the lock coordinate (whole steps at the grooves); v:
    0.08 at the whorl to 0.94 TIP_FADE before the tip, then to 1 at the tip
    (the shader fades only that last stretch, so each tip thins out into a
    wisp; at 5 mm the fringe ended in hard spikes)."""
    x, y, z = P[:, 0], P[:, 1], P[:, 2]
    ua, w, s = hair_coords(x, y, z, 0.0, 0.0)
    v = 0.08 + 0.86 * np.clip(w, 0.0, 1.0)
    v = np.where(s > -TIP_FADE, 0.94 + 0.06 * np.clip((s + TIP_FADE) / TIP_FADE, 0.0, 1.0), np.minimum(v, 0.94))
    return np.stack([ua, v], 1)


def brow_field(x, y, z):
    """The brows: a thin layer lying on the brow skin."""
    ex, ey, _ = eye_local(x, y)
    s = skin_field(x, y, z)
    layer = np.maximum(s - 0.0008, -(s + 0.0010))
    # The brow's line: from the inner end up to a peak two-thirds out, then
    # down to a thin tail.
    t = np.clip((ex + 0.016) / 0.036, 0.0, 1.0)
    cy = 0.0118 + 0.0040 * np.sin(np.minimum(t / 0.68, 1.0) * math.pi / 2) - 0.0055 * np.maximum(t - 0.68, 0) / 0.32 * np.maximum(t - 0.68, 0) / 0.32 * 1.0
    # Never thinner than 2.4 mm across, so the simplifier keeps the tail whole.
    w = 0.0013 * (1 - t) + 0.0012
    along = np.maximum(-0.016 - ex, ex - 0.020)
    m = smax(np.abs(ey - cy) - w, along, 0.0015)
    return smax(layer, m, 0.0006)


#: The lip piece's box seen from the front (x and y): its planar UVs and its
#: colour texture both span it.
LIP_BOX = (-0.028, -0.072, 0.028, -0.040)
#: The skin's and the lips' base colours (sRGB); the lip texture blends from
#: the first at the piece's edge to the second inside the vermilion.
SKIN_SRGB = (226, 178, 148)
LIP_SRGB = (202, 138, 126)
LIP_LINE_SRGB = (150, 80, 76)


def lip_outline(x, y):
    """The vermilion's outline seen from the front (negative inside): the
    upper lip's and the lower lip's ovals, as in head_field."""
    bow = 0.0011 * np.exp(-((np.abs(x) - 0.0045) / 0.003) ** 2)
    zero = np.zeros_like(x)
    up = ell(x, y - bow, zero, (0, -0.0502, 0), (0.0232, 0.0062, 1.0))
    lo = ell(x, y, zero, (0, -0.0605, 0), (0.0198, 0.0076, 1.0))
    return smin(up, lo, 0.002)


def lip_field(x, y, z):
    """The lips' colour: one solid piece over both lips, 1.4 mm wider than
    the vermilion, following the skin's crease where the lips meet, and cut
    off by a plane inside the flesh. It stands 0.35 mm proud inside the
    vermilion and sinks under the skin before its own edge, and its texture
    is the skin's colour there, so the lips blend into the face with no
    step and no line (critic round 5: pasted-on lips). The renderer reads no
    vertex colours, so the lips are their own part with their own material;
    the skin's own lips, under the piece, keep the shape.
    """
    s = skin_field(x, y, z)
    mz = z + BEND * x * x
    o = lip_outline(x, y)
    # Proud of the skin inside, under it near the edge.
    proud = 0.00035 - 0.0007 * sstep(-0.0022, 0.0, o)
    m = o - 0.0014
    # The piece's back: a plane in the flesh behind the lips.
    m = smax(m, 0.094 - mz, 0.001)
    return smax(s - proud, m, 0.0004)


def lip_texture(w=128, h=64):
    """The lip piece's colour (sRGB rows, top row first, as glTF's v runs
    down): the skin's colour outside the vermilion, the lips' rose inside
    after a soft 5 mm blend, darker along the line where the lips meet."""
    x0, y0, x1, y1 = LIP_BOX
    u = (np.arange(w) + 0.5) / w
    v = (np.arange(h) + 0.5) / h
    X = x0 + (x1 - x0) * u[None, :]
    Y = y1 - (y1 - y0) * v[:, None]
    X, Y = np.broadcast_arrays(X, Y)
    o = lip_outline(X, Y)
    t = sstep(0.0006, -0.0050, o)
    lift = 0.0008 * (X / 0.0232) ** 2
    line = np.exp(-((Y - lift - MOUTH_Y) / 0.0012) ** 2) * sstep(0.0215, 0.012, np.abs(X))
    skin, lip, dark = (np.array(c, dtype=np.float64) for c in (SKIN_SRGB, LIP_SRGB, LIP_LINE_SRGB))
    col = skin + (lip - skin) * t[..., None]
    col = col + (dark - col) * (0.55 * line * t)[..., None]
    return np.clip(np.round(col), 0, 255).astype(np.uint8)


def png_bytes(img):
    """A PNG (RGB, no filtering) of an h x w x 3 uint8 image."""
    import zlib
    h, w, _ = img.shape
    raw = b"".join(b"\0" + img[r].tobytes() for r in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def lash_field(x, y, z):
    """Upper lash lines: a fine roll along each upper lid's margin."""
    ex, ey, _ = eye_local(x, y)
    dz = z - EYE_Z
    r = np.sqrt(ex * ex + ey * ey + dz * dz)
    a = almond(ex, ey)
    tube = np.sqrt((r - (EYE_R + 0.0030)) ** 2 + (a - 0.0004) ** 2) - 0.0007
    tube = np.maximum(tube, -(ey - 0.07 * ex - 0.0008))
    tube = np.maximum(tube, np.abs(ex) - EYE_W * 0.92)
    return np.maximum(tube, -dz)


# ---- the top and the socle ----------------------------------------------------
CHEST_CUT = -0.300


def rbox(x, y, z, c, h, r):
    qx, qy, qz = np.abs(x - c[0]) - h[0] + r, np.abs(y - c[1]) - h[1] + r, np.abs(z - c[2]) - h[2] + r
    return np.sqrt(np.maximum(qx, 0) ** 2 + np.maximum(qy, 0) ** 2 + np.maximum(qz, 0) ** 2) + np.minimum(np.maximum(qx, np.maximum(qy, qz)), 0) - r


def top_field(x, y, z):
    ax = np.abs(x)
    d = rbox(x, y, z, (0, -0.258, -0.012), (0.118, 0.054, 0.052), 0.036)
    # Shoulders sloping from the neck, the arms cut off round, as busts are.
    d = smin(d, cap(ax, y, z, (0.0, -0.164, -0.018), (0.138, -0.212, -0.016), 0.040, 0.044), 0.05)
    d = smin(d, ell(x, y, z, (0, -0.228, 0.014), (0.088, 0.046, 0.038)), 0.03)
    d = smax(d, ax - 0.158, 0.030)
    # The collar rises round the neck to the band (the neck passes inside).
    nz = z + 0.014
    nr = np.sqrt((x / 1.04) ** 2 + nz * nz)
    band_y = -0.144 - 0.012 * sstep(-0.02, 0.05, z)
    collar = smax(nr - 0.060, y - band_y, 0.004)
    d = smin(d, smax(collar, -0.25 - y, 0.004), 0.014)
    # The folded band lying on the neck.
    d = smin(d, np.sqrt((nr - 0.0525) ** 2 + (y - band_y) ** 2) - 0.0050, 0.004)
    return smax(d, CHEST_CUT - y, 0.006)


#: The socle's profile, top down: (radius, y offset under the chest cut).
SOCLE_PROFILE = [(0.060, 0.000), (0.064, -0.004), (0.064, -0.012), (0.052, -0.020), (0.042, -0.030),
                 (0.040, -0.058), (0.046, -0.064), (0.046, -0.072), (0.050, -0.078), (0.066, -0.090),
                 (0.080, -0.100), (0.084, -0.106), (0.084, -0.124), (0.080, -0.128)]
SOCLE_FOOT = CHEST_CUT - 0.128


def socle_field(x, y, z):
    rr = np.sqrt(x * x + z * z)
    prof = np.array(SOCLE_PROFILE)
    ys = CHEST_CUT + 0.002 + prof[:, 1]
    # Radius at this height (linear through the profile), as a revolved
    # height field; rounded by the mesher's own smoothing.
    rad = np.interp(-y, -ys, prof[:, 0])
    d = rr - rad
    # Rounded rims top and bottom (a sharp rim simplifies into slivers).
    d = smax(d * 0.85, y - (CHEST_CUT + 0.002), 0.003)
    return smax(d, SOCLE_FOOT - y, 0.003)


# ---- meshing ------------------------------------------------------------------
def mesh_field(f, lo, hi, h, slab=24):
    """The zero surface of `f` in the box lo..hi on an h-spaced grid: closed
    quads from OpenVDB, wound outward, as float64 points and int faces."""
    n = [int(math.ceil((hi[i] - lo[i]) / h)) + 1 for i in range(3)]
    band = 3.0 * h
    grid = openvdb.FloatGrid(background=band)
    grid.gridClass = openvdb.GridClass.LEVEL_SET
    xs = lo[0] + h * np.arange(n[0])
    zs = lo[2] + h * np.arange(n[2])
    for j0 in range(0, n[1], slab):
        j1 = min(n[1], j0 + slab)
        ys = lo[1] + h * np.arange(j0, j1)
        X, Y, Z = np.meshgrid(xs, ys, zs, indexing="ij")
        v = np.clip(f(X, Y, Z), -band, band).astype(np.float32)
        grid.copyFromArray(v, ijk=(0, j0, 0), tolerance=0.0)
    grid.prune()
    pts, tris, quads = grid.convertToPolygons(isovalue=0.0, adaptivity=0.0)
    pts = lo + h * pts.astype(np.float64)
    faces = [list(q) for q in quads] + [list(t) for t in tris]
    # Wind outward: compare with the field's gradient at each face.
    P = pts
    g = grad(f, P)
    flip = 0
    for fc in faces[:2000]:
        a, b, c = P[fc[0]], P[fc[1]], P[fc[2]]
        nrm = np.cross(b - a, c - a)
        flip += 1 if nrm.dot(g[fc[0]]) < 0 else -1
    if flip > 0:
        faces = [fc[::-1] for fc in faces]
    log(f"  grid {n}, {len(pts)} points, {len(faces)} faces")
    return pts, faces


def grad(f, P, e=1e-4):
    x, y, z = P[:, 0], P[:, 1], P[:, 2]
    g = np.stack([f(x + e, y, z) - f(x - e, y, z), f(x, y + e, z) - f(x, y - e, z), f(x, y, z + e) - f(x, y, z - e)], axis=1)
    return g / np.maximum(np.linalg.norm(g, axis=1, keepdims=True), 1e-12)


def project(f, P, iters=3):
    for _ in range(iters):
        x, y, z = P[:, 0], P[:, 1], P[:, 2]
        e = 1e-4
        g = np.stack([f(x + e, y, z) - f(x - e, y, z), f(x, y + e, z) - f(x, y - e, z), f(x, y, z + e) - f(x, y, z - e)], axis=1) / (2 * e)
        gg = np.maximum((g * g).sum(1), 1e-12)
        step = (f(x, y, z) / gg)[:, None] * g
        step = np.clip(step, -0.0004, 0.0004)
        P = P - step
    return P


def decimate(name, pts, faces, ratio):
    """Blender's collapse decimate (symmetric in x), triangulated."""
    import bpy
    import bmesh
    me = bpy.data.meshes.new(name)
    me.from_pydata(pts.tolist(), [], faces)
    me.validate()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    if ratio < 1.0:
        mod = ob.modifiers.new("dec", "DECIMATE")
        mod.decimate_type = "COLLAPSE"
        mod.ratio = ratio
        mod.use_symmetry = True
        mod.symmetry_axis = "X"
        mod.use_collapse_triangulate = True
    tri = ob.modifiers.new("tri", "TRIANGULATE")
    tri.quad_method = "SHORTEST_DIAGONAL"
    dg = bpy.context.evaluated_depsgraph_get()
    ev = ob.evaluated_get(dg)
    m2 = bpy.data.meshes.new_from_object(ev)
    bm = bmesh.new()
    bm.from_mesh(m2)
    bmesh.ops.dissolve_degenerate(bm, dist=1.2e-5, edges=bm.edges[:])
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    P = np.array([v.co[:] for v in bm.verts], dtype=np.float64)
    F = np.array([[v.index for v in f.verts] for f in bm.faces], dtype=np.int64)
    bm.free()
    log(f"  {name}: {len(P)} vertices, {len(F)} triangles")
    return P, F


def bad_faces(P, N, F):
    a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
    n = np.cross(b - a, c - a)
    area = np.linalg.norm(n, axis=1)
    bad = area <= 1e-12
    bad |= (n * (N[F[:, 0]] + N[F[:, 1]] + N[F[:, 2]])).sum(1) <= 0
    bad |= ((n * N[F[:, 0]]).sum(1) <= 0) | ((n * N[F[:, 1]]).sum(1) <= 0) | ((n * N[F[:, 2]]).sum(1) <= 0)
    return bad


def welded(P):
    K = np.round(P * 1e5).astype(np.int64)
    _, inv = np.unique(K, axis=0, return_inverse=True)
    return inv.ravel()


def crossing_faces(P, F, f=None):
    """Faces in crossing pairs that share no welded corner (geocheck's rule),
    zero-area and welded-corner faces, and (given the field) faces folded
    over against it."""
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree
    W = welded(P)
    T = W[F]
    a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
    area = np.linalg.norm(np.cross(b - a, c - a), axis=1)
    crushed = (area < 1e-11) | (T[:, 0] == T[:, 1]) | (T[:, 1] == T[:, 2]) | (T[:, 0] == T[:, 2])
    if f is not None:
        g = grad(f, (a + b + c) / 3.0)
        crushed |= (np.cross(b - a, c - a) * g).sum(1) <= 0
    bad = set(np.nonzero(crushed)[0].tolist())
    tree = BVHTree.FromPolygons([Vector(p) for p in P], F.tolist(), epsilon=0.0)
    for i, j in tree.overlap(tree):
        if i < j and not (set(T[i]) & set(T[j])):
            bad.add(i)
            bad.add(j)
    return np.array(sorted(bad), dtype=np.int64)


def relax(f, P, F, name, rounds=8):
    """Smooth the corners of crossing or crushed triangles (and their
    neighbours) until none is left."""
    nbr = [[] for _ in range(len(P))]
    for t in F:
        for k in range(3):
            nbr[t[k]].extend((t[(k + 1) % 3], t[(k + 2) % 3]))
    for r in range(rounds):
        bad = crossing_faces(P, F, f)
        if len(bad) == 0:
            break
        vs = set(F[bad].ravel().tolist())
        for v in list(vs):
            vs.update(nbr[v])
        vs = np.array(sorted(vs))
        for _ in range(3):
            P[vs] = np.array([P[nbr[v]].mean(0) for v in vs])
        if r < rounds // 2:
            P[vs] = project(f, P[vs], iters=1)
    left = len(crossing_faces(P, F, f))
    log(f"  {name}: relaxed, {left} crossing or crushed triangles left")
    return P


def drop_fins(F, P):
    """Triangles on the same three (welded) corners come in back-to-back
    pairs (a fin the simplifier left): drop every copy, which leaves each of
    their edges with its two proper neighbours."""
    T = np.sort(welded(P)[F], axis=1)
    _, inv, cnt = np.unique(T, axis=0, return_inverse=True, return_counts=True)
    keep = cnt[inv.ravel()] == 1
    return F[keep], int((~keep).sum())


def fix_normals(P, N, F, passes=4):
    """Where a triangle disagrees with the sum of its vertex normals (the
    geometry check's `inverted`), its corners take the face-weighted mean."""
    for _ in range(passes):
        a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
        fn = np.cross(b - a, c - a)
        vn = N[F[:, 0]] + N[F[:, 1]] + N[F[:, 2]]
        bad = (vn * fn).sum(1) <= 0
        if not bad.any():
            break
        acc = np.zeros_like(P)
        for k in range(3):
            np.add.at(acc, F[:, k], fn)
        vs = np.unique(F[bad].ravel())
        N[vs] = acc[vs] / np.maximum(np.linalg.norm(acc[vs], axis=1, keepdims=True), 1e-12)
    # Last slivers: their corners lean towards the sliver's own facing.
    for _ in range(8):
        a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
        fn = np.cross(b - a, c - a)
        vn = N[F[:, 0]] + N[F[:, 1]] + N[F[:, 2]]
        bad = np.nonzero((vn * fn).sum(1) <= 0)[0]
        if len(bad) == 0:
            break
        for i in bad:
            u = fn[i] / max(np.linalg.norm(fn[i]), 1e-15)
            for v in F[i]:
                n = N[v] + 0.6 * u
                N[v] = n / max(np.linalg.norm(n), 1e-12)
    return N


def unfold(P, F, name, rounds=24):
    """Triangles folded over by geocheck's own rule (the face against the sum
    of its corners' smooth normals, built from the triangles per vertex)
    have their corners moved half way to their neighbours' middle, a few
    rounds, the round with the fewest folds kept. The field-normal test in
    `finish` does not see these: tiny tip triangles behind an ear passed it,
    and the converter's levels inherited them (geocheck 'inverted' on the
    Medium file)."""
    W = welded(P)
    nbr = [set() for _ in range(W.max() + 1)]
    for t in W[F]:
        for k in range(3):
            nbr[t[k]].update((t[(k + 1) % 3], t[(k + 2) % 3]))
    nbr = [np.array(sorted(n)) for n in nbr]

    def folds(Q):
        a, b, c = Q[F[:, 0]], Q[F[:, 1]], Q[F[:, 2]]
        fn = np.cross(b - a, c - a)
        acc = np.zeros((len(nbr), 3))
        for k in range(3):
            np.add.at(acc, W[F[:, k]], fn)
        n = acc / np.maximum(np.linalg.norm(acc, axis=1, keepdims=True), 1e-12)
        s = n[W[F[:, 0]]] + n[W[F[:, 1]]] + n[W[F[:, 2]]]
        return np.nonzero(((fn * s).sum(1) < 0) & (np.linalg.norm(fn, axis=1) > 1e-12))[0]

    # One position per welded vertex (seam copies move together).
    pos = np.zeros((len(nbr), 3))
    pos[W] = P
    Q = P.copy()
    bad = folds(Q)
    first, best, best_pos = len(bad), len(bad), pos.copy()
    for _ in range(rounds):
        if len(bad) == 0:
            break
        for g in np.unique(W[F[bad]].ravel()):
            if len(nbr[g]):
                pos[g] = 0.5 * (pos[g] + pos[nbr[g]].mean(0))
        Q = pos[W]
        bad = folds(Q)
        if len(bad) < best:
            best, best_pos = len(bad), pos.copy()
    P = best_pos[W]
    # A sliver wound the wrong way round (a corner facing opposite its
    # neighbours) does not turn over by smoothing: collapse its shortest
    # edge, where the edge's two ends share exactly the two opposite corners
    # (the link condition, so the surface stays manifold) and neither is a
    # seam copy.
    F = F.copy()
    collapsed = 0
    for _ in range(16):
        bad = folds(P)
        if len(bad) == 0:
            break
        done = False
        cnt = np.bincount(W, minlength=len(nbr))
        for ti in bad:
            t = F[ti]
            edges = sorted(((np.linalg.norm(P[t[i]] - P[t[(i + 1) % 3]]), t[i], t[(i + 1) % 3]) for i in range(3)))
            for _, a, b in edges:
                if cnt[W[a]] != 1 or cnt[W[b]] != 1:
                    continue
                fa = set(np.nonzero((F == a).any(1))[0])
                fb = set(np.nonzero((F == b).any(1))[0])
                both = fa & fb
                ring_a = set(F[list(fa)].ravel()) - {a}
                ring_b = set(F[list(fb)].ravel()) - {b}
                if len(both) != 2 or len((ring_a & ring_b) - {a, b}) != 2:
                    continue
                P[a] = 0.5 * (P[a] + P[b])
                F[F == b] = a
                keep = np.ones(len(F), bool)
                keep[list(both)] = False
                F = F[keep]
                collapsed += 1
                done = True
                break
            if done:
                break
        if not done:
            break
    W = welded(P)
    left = len(folds(P)) if collapsed else best
    log(f"  {name}: {first} folded triangles by geocheck's rule, {left} left ({collapsed} slivers collapsed)")
    return P, F


def agree_normals(P, N, F, deg=20.0):
    """Where a vertex's field normal and its mesh's own (area-weighted) normal
    differ by more than `deg`, take the mesh's: where the hair turns down
    under the skin the two disagree, and the converter's simplified levels
    left triangles facing against the field normals there (geocheck's
    'inverted' on the Medium file)."""
    a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
    fn = np.cross(b - a, c - a)
    acc = np.zeros_like(P)
    for k in range(3):
        np.add.at(acc, F[:, k], fn)
    m = acc / np.maximum(np.linalg.norm(acc, axis=1, keepdims=True), 1e-12)
    off = (N * m).sum(1) < math.cos(math.radians(deg))
    N = N.copy()
    N[off] = m[off]
    log(f"  {off.sum()} of {len(N)} vertex normals taken from the mesh")
    return N


def finish(f, P, F, move=True, name="part"):
    """Vertices on the true surface, normals from the field. Where moving a
    vertex onto the surface would turn a triangle over (thin places: a lid's
    margin, the mouth's line), it stays where the decimate left it; a normal
    that still disagrees with its triangles is replaced by their average."""
    P0 = P.copy()
    if move:
        P = project(f, P)
    N = grad(f, P)
    for _ in range(4):
        bad = bad_faces(P, N, F)
        if not bad.any():
            break
        vs = np.unique(F[bad].ravel())
        P[vs] = P0[vs]
        N[vs] = grad(f, P[vs])
    bad = bad_faces(P, N, F)
    if bad.any():
        # Area-weighted face normals at the remaining vertices.
        a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
        fn = np.cross(b - a, c - a)
        acc = np.zeros_like(P)
        for k in range(3):
            np.add.at(acc, F[:, k], fn)
        vs = np.unique(F[bad].ravel())
        N[vs] = acc[vs] / np.maximum(np.linalg.norm(acc[vs], axis=1, keepdims=True), 1e-12)
    P = relax(f, P, F, name)
    # Corners closer than the geometry check's weld (0.01 mm) become one
    # vertex, and the triangles that collapse with them go.
    W = welded(P)
    first = {}
    for i, w in enumerate(W):
        first.setdefault(int(w), i)
    remap = np.array([first[int(w)] for w in W])
    F = remap[F]
    F = F[(F[:, 0] != F[:, 1]) & (F[:, 1] != F[:, 2]) & (F[:, 0] != F[:, 2])]
    a_, b_, c_ = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
    F = F[np.linalg.norm(np.cross(b_ - a_, c_ - a_), axis=1) >= 1e-12]
    F, fins = drop_fins(F, P)
    if fins:
        log(f"  {name}: dropped {fins} fin triangles")
    if move:
        N = grad(f, P)
    else:
        # Sharp mouldings: the mesh's own (area-weighted) normals.
        a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
        fn = np.cross(b - a, c - a)
        N = np.zeros_like(P)
        for k in range(3):
            np.add.at(N, F[:, k], fn)
        N /= np.maximum(np.linalg.norm(N, axis=1, keepdims=True), 1e-12)
    N = fix_normals(P, N, F)
    bad = bad_faces(P, N, F)
    a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
    vn = N[F[:, 0]] + N[F[:, 1]] + N[F[:, 2]]
    inv = ((vn * np.cross(b - a, c - a)).sum(1) <= 0).sum()
    log(f"  {name}: {inv} triangles against their normals")
    return P, N, F


# ---- UVs ----------------------------------------------------------------------
def split_seam(P, N, F, U, wrap_u=1.0):
    """Faces that straddle u's wrap get their own copies of the low side."""
    P, N, U = list(P), list(N), list(U)
    F = F.copy()
    extra = {}
    for i, fc in enumerate(F):
        us = [U[v][0] for v in fc]
        if max(us) - min(us) > 0.5 * wrap_u:
            for k, v in enumerate(fc):
                if U[v][0] < 0.5 * wrap_u:
                    if v not in extra:
                        extra[v] = len(P)
                        P.append(P[v])
                        N.append(N[v])
                        U.append((U[v][0] + wrap_u, U[v][1]))
                    F[i][k] = extra[v]
    return np.array(P), np.array(N), F, np.array(U)


def uv_equirect(P, c=(0.0, 0.0, 0.0)):
    q = P - np.array(c)
    lon = np.arctan2(q[:, 0], q[:, 2])  # 0 at the front, the seam at the back
    u = (lon + math.pi) / (2 * math.pi)
    r = np.linalg.norm(q, axis=1) + 1e-12
    v = np.arccos(np.clip(q[:, 1] / r, -1, 1)) / (2 * math.pi)
    return np.stack([u, v], 1)


# ---- glTF ---------------------------------------------------------------------
def lin(c):
    s = c / 255.0
    return s / 12.92 if s <= 0.04045 else ((s + 0.055) / 1.055) ** 2.4


def rgb(r, g, b):
    return [round(lin(r), 4), round(lin(g), 4), round(lin(b), 4), 1.0]


def write_glb(path, parts, materials, images=(), image_names=("lips",)):
    bin_parts = []
    views, accessors = [], []
    off = 0

    def push(buf, target):
        nonlocal off
        pad = (-off) % 4
        if pad:
            bin_parts.append(b"\0" * pad)
            off += pad
        view = {"buffer": 0, "byteOffset": off, "byteLength": len(buf)}
        if target is not None:
            view["target"] = target
        views.append(view)
        bin_parts.append(buf)
        off += len(buf)
        return len(views) - 1

    def acc(arr, typ, minmax=False):
        if typ == "SCALAR":
            big = int(arr.max()) > 65535
            a = arr.astype(np.uint32 if big else np.uint16).ravel()
            v = push(a.tobytes(), 34963)
            accessors.append({"bufferView": v, "componentType": 5125 if big else 5123, "count": int(a.size), "type": typ})
        else:
            a = np.ascontiguousarray(arr, dtype=np.float32)
            v = push(a.tobytes(), 34962)
            e = {"bufferView": v, "componentType": 5126, "count": int(a.shape[0]), "type": typ}
            if minmax:
                e["min"] = [float(m) for m in a.min(0)]
                e["max"] = [float(m) for m in a.max(0)]
            accessors.append(e)
        return len(accessors) - 1

    img_views = [push(png, None) for png in images]
    meshes = []
    for name, P, N, U, F, mat in parts:
        prim = {"attributes": {"POSITION": acc(P, "VEC3", True), "NORMAL": acc(N, "VEC3"), "TEXCOORD_0": acc(U, "VEC2")},
                "indices": acc(F, "SCALAR"), "material": mat}
        meshes.append({"name": name, "primitives": [prim]})
    gltf = {
        "asset": {"version": "2.0", "generator": "Evani.Dev tools/blender/bust_build.py (material viewer bust)"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "char_head", "children": list(range(1, len(parts) + 1))}] + [{"name": p[0], "mesh": i} for i, p in enumerate(parts)],
        "meshes": meshes,
        "materials": materials,
        "bufferViews": views,
        "accessors": accessors,
        "buffers": [{"byteLength": 0}],
    }
    if img_views:
        # Named for the model, so the converter's texture files can share a
        # game's textures/ folder with other models' (`char-head-lips.low.jpg`).
        gltf["images"] = [{"bufferView": v, "mimeType": "image/png", "name": f"char-head-{n}"} for v, n in zip(img_views, image_names)]
        gltf["samplers"] = [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}]
        gltf["textures"] = [{"source": i, "sampler": 0} for i in range(len(img_views))]
    binb = b"".join(bin_parts)
    binb += b"\0" * ((-len(binb)) % 4)
    gltf["buffers"][0]["byteLength"] = len(binb)
    js = json.dumps(gltf, separators=(",", ":")).encode()
    js += b" " * ((-len(js)) % 4)
    total = 12 + 8 + len(js) + 8 + len(binb)
    with open(path, "wb") as fh:
        fh.write(struct.pack("<III", 0x46546C67, 2, total))
        fh.write(struct.pack("<II", len(js), 0x4E4F534A) + js)
        fh.write(struct.pack("<II", len(binb), 0x004E4942) + binb)
    return total


# ---- the parts ----------------------------------------------------------------
def read_parts(path):
    """The parts of a glb this script wrote, by name: (P, N, U, F, material)."""
    with open(path, "rb") as fh:
        b = fh.read()
    n = struct.unpack("<I", b[12:16])[0]
    g = json.loads(b[20:20 + n])
    binb = b[20 + n + 8:]

    def arr(i):
        a = g["accessors"][i]
        v = g["bufferViews"][a["bufferView"]]
        dt = {5126: np.float32, 5123: np.uint16, 5125: np.uint32}[a["componentType"]]
        k = {"SCALAR": 1, "VEC2": 2, "VEC3": 3}[a["type"]]
        x = np.frombuffer(binb, dtype=dt, count=a["count"] * k, offset=v.get("byteOffset", 0))
        return x.reshape(-1, k) if k > 1 else x
    out = {}
    for m in g["meshes"]:
        pr = m["primitives"][0]
        at = pr["attributes"]
        out[m["name"]] = (arr(at["POSITION"]).astype(np.float64), arr(at["NORMAL"]).astype(np.float64),
                          arr(at["TEXCOORD_0"]).astype(np.float64), arr(pr["indices"]).astype(np.int64).reshape(-1, 3), pr["material"])
    return out


def build(out, fine=0.0006, only=None):
    """`only`: a set of part names for a quick look (the file is then not
    the full model)."""
    parts = []
    # While tuning one part: BUST_REUSE=<an earlier full build> and
    # BUST_REUSE_PARTS=head,eyes,top,socle,lips take those parts as they
    # were, so only the rest is rebuilt (the skin alone takes minutes).
    reuse = {}
    if os.environ.get("BUST_REUSE"):
        old = read_parts(os.environ["BUST_REUSE"])
        for k in os.environ.get("BUST_REUSE_PARTS", "").split(","):
            if k in old:
                reuse[k] = old[k]
    part_of = {"skin": "head", "eyes": "eyes", "hair": "hair", "scalp": "hair_base", "top": "top", "socle": "socle", "lips": "lips"}

    def want(name):
        if part_of[name] in reuse:
            P, N, U, F, mat = reuse[part_of[name]]
            parts.append((part_of[name], P, N, U, F, mat))
            log(f"{name}: reused")
            return False
        return only is None or name in only

    if want("skin"):
        log("skin")
        P, F = mesh_field(skin_field, (-0.092, -0.222, -0.118), (0.092, 0.122, 0.132), fine)
        P, F = decimate("skin", P, F, 46000 / max(len(F) * 2, 1))
        P, N, F = finish(skin_field, P, F, name="skin")
        P, F = unfold(P, F, "skin")
        U = uv_equirect(P)
        P, N, F, U = split_seam(P, N, F, U)
        parts.append(("head", P, N, U, F, 0))

    if want("eyes"):
        log("eyes")
        eyes = sphere_eyes()
        parts.append(("eyes",) + eyes + (1,))

    if want("hair"):
        log("hair")
        hp, hn, hu, hf = [], [], [], []

        def add(Pa, Na, Ua, Fa):
            base = sum(len(p) for p in hp)
            hp.append(Pa)
            hn.append(Na)
            hu.append(Ua)
            hf.append(Fa + base)

        P, F = mesh_field(hair_field, (-0.100, -0.075, -0.152), (0.100, 0.148, 0.110), fine * 1.25)
        P, F = decimate("hair", P, F, 40000 / max(len(F) * 2, 1))
        P, N, F = finish(hair_field, P, F, name="hair")
        P, F = unfold(P, F, "hair")
        # The hair's normals are its mesh's own: with the field's, the
        # converter's Medium level kept triangles facing against them where
        # the hair turns down under the skin (6 even with only the
        # disagreeing fifth swapped).
        N = agree_normals(P, N, F, deg=0.0)
        U = hair_uv(P)
        if os.environ.get("BUST_DEBUG"):
            bad = crossing_faces(P, F, hair_field)
            if len(bad):
                cc = P[F[bad]].mean(1)
                _, wv, sv = hair_coords(cc[:, 0], cc[:, 1], cc[:, 2])
                b0 = head_field(cc[:, 0], cc[:, 1], cc[:, 2], ears=False)
                log("  bad hair faces: w", np.histogram(np.clip(wv, 0, 1.4), bins=7, range=(0, 1.4))[0].tolist(),
                    "s mm", np.histogram(np.clip(sv * 1000, -40, 10), bins=5, range=(-40, 10))[0].tolist(),
                    "skin depth mm", np.histogram(np.clip(b0 * 1000, -8, 20), bins=7, range=(-8, 20))[0].tolist())
        P, N, F, U = split_seam(P, N, F, U, wrap_u=NLOCK)
        add(P, N, U, F)
        log("brows")
        P, F = mesh_field(brow_field, (-0.060, 0.012, 0.055), (0.060, 0.040, 0.105), fine * 0.6)
        P, F = decimate("brows", P, F, 3000 / max(len(F) * 2, 1))
        P, N, F = finish(brow_field, P, F, name="brows")
        ex = np.abs(P[:, 0]) - EYE_X
        U = np.stack([(ex + 0.02) / 0.0016, np.clip((P[:, 1] - 0.016) / 0.012, 0.05, 0.9)], 1)
        add(P, N, U, F)
        log("lashes")
        P, F = mesh_field(lash_field, (-0.052, 0.008, 0.070), (0.052, 0.024, 0.090), fine * 0.4)
        if len(F):
            P, F = decimate("lashes", P, F, 2400 / max(len(F) * 2, 1))
            P, N, F = finish(lash_field, P, F, name="lashes")
            U = np.stack([(np.abs(P[:, 0]) - EYE_X + 0.02) / 0.0012, np.full(len(P), 0.5)], 1)
            add(P, N, U, F)
        parts.append(("hair", np.concatenate(hp), np.concatenate(hn), np.concatenate(hu), np.concatenate(hf), 2))

    if want("scalp"):
        log("scalp")
        P, F = mesh_field(scalp_field, (-0.100, -0.075, -0.152), (0.100, 0.148, 0.110), fine * 1.25)
        P, F = decimate("scalp", P, F, 12000 / max(len(F) * 2, 1))
        P, N, F = finish(scalp_field, P, F, name="scalp")
        P, F = unfold(P, F, "scalp")
        U = uv_equirect(P)
        P, N, F, U = split_seam(P, N, F, U)
        parts.append(("hair_base", P, N, U, F, 3))

    if want("top"):
        log("top")
        P, F = mesh_field(top_field, (-0.185, -0.31, -0.10), (0.185, -0.11, 0.09), fine * 1.6)
        P, F = decimate("top", P, F, 9000 / max(len(F) * 2, 1))
        P, N, F = finish(top_field, P, F, name="top")
        U = P[:, [0, 1]] * 4
        parts.append(("top", P, N, U, F, 4))

    if want("socle"):
        log("socle")
        P, F = mesh_field(socle_field, (-0.09, SOCLE_FOOT - 0.004, -0.09), (0.09, CHEST_CUT + 0.006, 0.09), fine * 1.6)
        # 12,000 (was 5,000): the low tier's far level keeps 8%, and at 398
        # triangles the bead ring and the cap's collar collapsed into notches
        # (the baked sun then disagreed with the shadow map by 6.5 points).
        P, F = decimate("socle", P, F, 12000 / max(len(F) * 2, 1))
        P, N, F = finish(socle_field, P, F, move=False, name="socle")
        U = np.stack([np.arctan2(P[:, 0], P[:, 2]) / (2 * math.pi) + 0.5, P[:, 1] * 4], 1)
        P, N, F, U = split_seam(P, N, F, U)
        parts.append(("socle", P, N, U, F, 5))

    if want("lips"):
        log("lips")
        x0, y0, x1, y1 = LIP_BOX
        P, F = mesh_field(lip_field, (x0, y0, 0.076), (x1, y1, 0.118), fine * 0.5)
        P, F = decimate("lips", P, F, 9000 / max(len(F) * 2, 1))
        P, N, F = finish(lip_field, P, F, name="lips")
        # Planar from the front over the lip box (the texture's frame).
        U = np.stack([(P[:, 0] - x0) / (x1 - x0), (y1 - P[:, 1]) / (y1 - y0)], 1)
        parts.append(("lips", P, N, U, F, 6))

    materials = [
        {"name": "skin", "pbrMetallicRoughness": {"baseColorFactor": rgb(226, 178, 148), "metallicFactor": 0, "roughnessFactor": 0.55},
         "extras": {"evani_kind": "skin", "evani_skin": {"pore_scale": 500}}},
        {"name": "eye", "pbrMetallicRoughness": {"baseColorFactor": rgb(235, 228, 220), "metallicFactor": 0, "roughnessFactor": 0.05},
         "extras": {"evani_kind": "eye", "evani_eye": {"iris_color": [0.10, 0.20, 0.27], "iris_size": 0.5}}},
        {"name": "hair", "pbrMetallicRoughness": {"baseColorFactor": rgb(70, 44, 28), "metallicFactor": 0, "roughnessFactor": 0.5},
         "extras": {"evani_kind": "hair", "evani_hair": {"tip_width": 1.0, "edge_softness": 0.0, "highlight_color": [0.52, 0.40, 0.28]}}},
        {"name": "hair_base", "pbrMetallicRoughness": {"baseColorFactor": rgb(46, 29, 19), "metallicFactor": 0, "roughnessFactor": 0.7}},
        {"name": "top", "pbrMetallicRoughness": {"baseColorFactor": rgb(78, 96, 116), "metallicFactor": 0, "roughnessFactor": 0.85}},
        {"name": "stone", "pbrMetallicRoughness": {"baseColorFactor": rgb(212, 204, 190), "metallicFactor": 0, "roughnessFactor": 0.45}},
                # Its colour is the texture: the skin's own at the piece's edge, so the
        # lips blend into the face (no factor tint).
        {"name": "lips", "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 1.0], "baseColorTexture": {"index": 0},
                                                  "metallicFactor": 0, "roughnessFactor": 0.5},
         "extras": {"evani_kind": "skin", "evani_skin": {"pore_scale": 500}}},
    ]
    size = write_glb(out, parts, materials, images=[png_bytes(lip_texture())] if any(q[0] == "lips" for q in parts) else [])
    tris = sum(len(p[4]) for p in parts)
    verts = sum(len(p[1]) for p in parts)
    log(f"{out}: {size} bytes, {tris} triangles, {verts} vertices; socle foot {SOCLE_FOOT:.3f}")


def sphere_eyes(rings=24, segs=40):
    Ps, Ns, Us, Fs = [], [], [], []
    for sx in (1.0, -1.0):
        c = np.array([sx * EYE_X, EYE_Y, EYE_Z])
        base = sum(len(p) for p in Ps)
        P, N, U = [], [], []
        # Pole on the pupil axis (+z); azimuthal UVs.
        P.append(c + [0, 0, EYE_R]); N.append([0, 0, 1]); U.append([0.5, 0.5])
        for i in range(1, rings):
            a = math.pi * i / rings
            for j in range(segs):
                b = 2 * math.pi * j / segs
                n = np.array([math.sin(a) * math.cos(b), math.sin(a) * math.sin(b), math.cos(a)])
                P.append(c + EYE_R * n); N.append(n)
                rr = a / (2 * math.pi)
                U.append([0.5 + rr * math.cos(b), 0.5 + rr * math.sin(b)])
        P.append(c + [0, 0, -EYE_R]); N.append([0, 0, -1]); U.append([0.5, 0.5])
        F = []
        last = len(P) - 1
        for j in range(segs):
            F.append([0, 1 + j, 1 + (j + 1) % segs])
        for i in range(rings - 2):
            r0, r1 = 1 + i * segs, 1 + (i + 1) * segs
            for j in range(segs):
                a, b = r0 + j, r0 + (j + 1) % segs
                c2, d = r1 + j, r1 + (j + 1) % segs
                F.append([a, c2, d]); F.append([a, d, b])
        r0 = 1 + (rings - 2) * segs
        for j in range(segs):
            F.append([last, r0 + (j + 1) % segs, r0 + j])
        F = np.array(F)
        # Wind outward.
        Pa = np.array(P)
        n0 = np.cross(Pa[F[:, 1]] - Pa[F[:, 0]], Pa[F[:, 2]] - Pa[F[:, 0]])
        if (n0 * (Pa[F[:, 0]] - c)).sum(1).mean() < 0:
            F = F[:, ::-1]
        Ps.append(Pa); Ns.append(np.array(N)); Us.append(np.array(U)); Fs.append(F + base)
    return np.concatenate(Ps), np.concatenate(Ns), np.concatenate(Us), np.concatenate(Fs)


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    out = argv[0] if argv else "char-head.glb"
    fine = float(argv[1]) if len(argv) > 1 else 0.0006
    only = set(argv[2].split(",")) if len(argv) > 2 else None
    build(out, fine, only)
