# The quadruped's body, by numbers from a species file (original work; no
# downloaded meshes). build.py makes the mesh from parts(), rig.py fits the
# skeleton to landmarks(), and both use the same numbers, so the bones always
# sit on the anatomy they move. Every field is described in README.md.
#
# Frame: f forward (metres, + toward the head), z up, x to the animal's left.
import math
import bmesh
from mathutils import Vector

from geom import (P, X, pi, catmull, smoothstep, profile, section_part, loft, tube, spline, ellipsoid, muscle)


class Body:
    """A species file read once: the head frame and every part's numbers."""

    def __init__(self, S):
        self.S = S
        h = S["head"]
        self.poll = tuple(h["poll"])
        ang = math.radians(h["angle"])
        self.hd = (math.cos(ang), -math.sin(ang))    # down the face line
        self.hn = (-math.sin(ang), -math.cos(ang))   # in toward the jaw
        self.head_len = h["length"]

    # ---------------------------------------------------------- head frame
    def head_pt(self, a, b=0.0):
        """(f, z) of the point `a` metres down the face line from the poll
        and `b` in toward the jaw."""
        return (self.poll[0] + self.hd[0] * a + self.hn[0] * b, self.poll[1] + self.hd[1] * a + self.hn[1] * b)

    def HV(self, a, b, x=0.0):
        return P(*self.head_pt(a, b), x)

    def head_local(self, p):
        """(a, b, x) of a Blender point in the head frame."""
        f, z = -p.y - self.poll[0], p.z - self.poll[1]
        return f * self.hd[0] + z * self.hd[1], f * self.hn[0] + z * self.hn[1], p.x

    def head_dir(self, out, fwd, up, sx):
        """A direction given as (out, forward, up) on side sx."""
        return Vector((sx * out, -fwd, up)).normalized()

    # ---------------------------------------------------------- keys
    @staticmethod
    def _prof(k):
        """A section profile from a key's tail: [w, widest, p_top, p_bot,
        (flat)] or an explicit list of half widths at geom.TT."""
        if isinstance(k[0], list):
            from geom import TT
            return [(0.0, 0.0)] + [(w, t) for w, t in zip(k[0], TT)] + [(0.0, 1.0)]
        w, widest = k[0], k[1]
        p_top = k[2] if len(k) > 2 else 2.0
        p_bot = k[3] if len(k) > 3 else 2.0
        flat = k[4] if len(k) > 4 else 0.0
        return profile(w, widest, p_top, p_bot, flat)

    def trunk_keys(self):
        g = self.S.get("girth", 1.0)
        out = []
        for k in self.S["trunk"]:
            f, zt, zb = k[0], k[1], k[2]
            pts = [(x * g, t) for x, t in self._prof(k[3:])]
            out.append((P(f, zt), P(f, zb), pts))
        return out

    def neck_keys(self):
        g = self.S.get("girth", 1.0)
        return [(P(*k[0]), P(*k[1]), [(x * g, t) for x, t in self._prof(k[2:])]) for k in self.S["neck"]["keys"]]

    def head_keys(self):
        g = self.S.get("girth", 1.0)
        out = []
        for k in self.S["head"]["keys"]:
            a, off, D = k[0], k[1], k[2]
            out.append((self.HV(a, off), self.HV(a, off + D), [(x * g, t) for x, t in self._prof(k[3:])]))
        return out

    # ---------------------------------------------------------- legs
    def leg_rings(self, leg, side, n=36, steps=72):
        """A leg's skin: rings perpendicular to its centre line through the
        skin keys [f, z, half depth, half width, x (, squareness)]."""
        keys = [list(k) + ([2.0] if len(k) < 6 else []) for k in self.S["legs"][leg]["skin"]]
        rings = []
        for i in range(steps + 1):
            s = i / steps * (len(keys) - 1)
            f, z, d, w, xc, p = catmull(keys, s)
            f2, z2 = catmull(keys, min(s + 0.05, len(keys) - 1.0001))[:2]
            f1, z1 = catmull(keys, max(s - 0.05, 0.0))[:2]
            t = (P(f2, z2) - P(f1, z1)).normalized()
            fwd = Vector((0, -1, 0))
            nrm = (fwd - t * fwd.dot(t)).normalized()      # forward, across the leg
            c = P(f, z, side * xc)
            ring = []
            for k in range(n):
                th = 2 * pi * k / n
                cs, sn = math.cos(th), math.sin(th)
                e = 2.0 / max(p, 0.5)
                ring.append(c + X * (w * math.copysign(abs(cs) ** e, cs)) + nrm * (d * math.copysign(abs(sn) ** e, sn)))
            rings.append(ring)
        return rings

    def joints(self, leg):
        return {k: tuple(v) for k, v in self.S["legs"][leg]["joints"].items()}

    def leg_x(self, leg, z):
        """The leg's centre x at height z (the skin keys' x, by height)."""
        cache = self.__dict__.setdefault("_legx", {})
        if leg not in cache:
            cache[leg] = sorted((k[1], k[4]) for k in self.S["legs"][leg]["skin"])
        pairs = cache[leg]
        if z <= pairs[0][0]:
            return pairs[0][1]
        for (z0, x0), (z1, x1) in zip(pairs, pairs[1:]):
            if z <= z1:
                return x0 + (x1 - x0) * (z - z0) / max(z1 - z0, 1e-6)
        return pairs[-1][1]

    def leg_inside(self, leg, f, z, x):
        """How far (f, z, |x|) lies inside the leg's skin at height z, as
        the elliptic radius of the skin key there (below 1: in the leg).
        The keys are taken by height, linearly."""
        cache = self.__dict__.setdefault("_legkeys", {})
        if leg not in cache:
            cache[leg] = sorted((k[1], k[0], k[2], k[3], k[4]) for k in self.S["legs"][leg]["skin"])
        ks = cache[leg]
        if z <= ks[0][0]:
            k = ks[0]
        elif z >= ks[-1][0]:
            k = ks[-1]
        else:
            for k0, k1 in zip(ks, ks[1:]):
                if z <= k1[0]:
                    u = (z - k0[0]) / max(k1[0] - k0[0], 1e-6)
                    k = tuple(a + (b - a) * u for a, b in zip(k0, k1))
                    break
        _, fc, d, w, xc = k
        return math.hypot((f - fc) / d, (abs(x) - xc) / w)

    # ---------------------------------------------------------- parts
    def ear_frame(self, sx):
        e = self.S["ears"]
        base = self.HV(e["a"], e["b"], sx * e["x"])
        up = self.head_dir(*e["dir"], sx)
        op = self.head_dir(*e["open"], sx)
        op = (op - up * op.dot(up)).normalized()     # the cup opens this way
        side = up.cross(op).normalized()
        return base, up, op, side

    def ear_centre(self, sx, t):
        e = self.S["ears"]
        base, up, op, side = self.ear_frame(sx)
        droop = self.head_dir(*e.get("droop_dir", [0, 0, -1]), sx)
        return base + up * (e["length"] * t) + droop * (e.get("droop", 0.0) * t * t) + op * (e.get("bow", 0.0) * math.sin(pi * t))

    def ear(self, bm, sx):
        """A cupped, thick-walled ear: C-shaped sections from a rolled base
        to the tip. shape 'leaf' (widest a third of the way up, a long taper)
        or 'triangle' (widest at the base, straight sides to a point)."""
        e = self.S["ears"]
        base, up, op, side = self.ear_frame(sx)
        n, K = 32, 22
        W, th0 = e["width"], e["thick"]
        widest = e.get("widest", 0.35)
        tip = e.get("tip", 1.15)
        rings = []
        for i in range(K + 1):
            t = i / K
            if t < widest:
                f = 0.70 + 0.30 * math.sin(0.5 * pi * t / max(widest, 1e-3))
            else:
                f = math.cos(0.5 * pi * (t - widest) / (1 - widest)) ** tip
            w = W * f + 0.0008
            dep = w * e.get("cup", 0.75) * (1.0 - 0.35 * t)
            th = min(th0 * (1 - 0.45 * t), 0.45 * w + 0.0008)
            A = math.radians(160 - 65 * t)
            c = self.ear_centre(sx, t)
            pts = []
            for k in range(n // 2):
                al = -A + 2 * A * k / (n // 2 - 1)
                pts.append(c + side * (math.sin(al) * w) - op * (math.cos(al) * dep))
            for k in range(n // 2):
                al = A - 2 * A * k / (n // 2 - 1)
                pts.append(c + side * (math.sin(al) * max(w - th, 0.0008)) - op * (math.cos(al) * max(dep - th, 0.0004)))
            rings.append(pts)
        rings.insert(0, [p - up * e.get("sink", 0.025) for p in rings[0]])
        loft(bm, rings)

    def horn_line(self, sx, samples=24):
        h = self.S["horns"]
        base = self.HV(h["a"], h["b"], sx * h["x"])
        pts = [base + Vector((sx * o, -fw, u)) for o, fw, u in h["line"]]
        return spline(pts, samples)

    def horn_radius(self, t):
        rs = self.S["horns"]["r"]
        return catmull([[r] for r in rs], t * (len(rs) - 1))[0]

    def horn(self, bm, sx):
        line = self.horn_line(sx)
        m = len(line) - 1
        tube(bm, line, [max(self.horn_radius(i / m), 0.002) for i in range(m + 1)], n=16)

    def tail_line(self, samples=40):
        keys = self.S["tail"]["line"]
        out = []
        for i in range(samples + 1):
            f, z, r = catmull(keys, i / samples * (len(keys) - 1))
            out.append((P(f, z), max(r, 0.002)))
        return out

    def tail(self, bm, sink=0.0):
        """sink > 0: the root is carried that far on into the rump (a
        separate tail, `tail.separate`, whose base must stay buried)."""
        line = self.tail_line()
        cs, rs = [c for c, _ in line], [r for _, r in line]
        if sink > 0:
            d = (cs[0] - cs[1]).normalized()
            cs = [cs[0] + d * sink, cs[0] + d * (sink * 0.5)] + cs
            rs = [rs[0] * 0.9, rs[0]] + rs
        tube(bm, cs, rs, n=24)

    def eye_centre(self, sx):
        e = self.S["eyes"]
        return self.HV(e["a"], e["b"], sx * e["x"])

    def hoof(self, bm, fc, xc, hw, hl, htoe, hheel, slant=0.35, flare=0.16, cleft=0.0):
        """A hoof wall from the ground to the coronet (higher at the toe than
        the heels, the toe wall slanting back by `slant` of its height),
        flaring out toward the ground, the sole flat. cleft > 0: two claws
        (a cloven hoof) with that gap between them."""
        claws = [(xc, hw)] if cleft <= 0 else [(xc + s * (hw + cleft) / 2, (hw - cleft) / 2) for s in (1, -1)]
        for cx, cw in claws:
            n, K = 24, 6
            rings = []
            for k in range(K + 1):
                s = k / K
                sc = 1.0 - flare * s
                pts = []
                for i in range(n):
                    th = 2 * pi * i / n
                    fr = (1 + math.cos(th)) / 2
                    ln = hl * (1.0 if math.cos(th) > 0 else 0.92)
                    wid = cw * sc
                    if cleft > 0:
                        # a claw: rounder outside, flatter toward the cleft, pointed toe
                        inner = (math.sin(th) * (cx - xc)) < 0
                        wid *= (0.85 if inner else 1.0) * (1.0 - 0.18 * fr ** 3)
                    zz = s * (hheel + (htoe - hheel) * fr ** 1.5)
                    p = P(fc + math.cos(th) * ln * sc - slant * zz, 0.0, cx + math.sin(th) * wid)
                    p.z = zz + 0.002
                    pts.append(p)
                rings.append(pts)
            rings[0] = [Vector((p.x, p.y, 0.0)) for p in rings[0]]
            rings.append([p + Vector((0, 0, 0.018)) for p in rings[-1]])
            loft(bm, rings)

    def paw(self, bm, fc, xc, length, width, height, dewclaw=False, sx=1):
        """A dog's paw: a rounded foot pad mass and four toes in an arc at
        the front, each toe a soft ellipsoid sitting on the ground."""
        ellipsoid(bm, P(fc - 0.15 * length, height * 0.55, xc), (Vector((0, -1, 0)), Vector((0, 0, 1)), X), (length * 0.45, height * 0.55, width * 0.42))
        for i, (ang, sc) in enumerate(((-38, 0.86), (-13, 1.0), (13, 1.0), (38, 0.86))):
            a = math.radians(ang)
            tf = fc + length * 0.30 * math.cos(a) * sc
            tx = xc + width * 0.40 * math.sin(a)
            ellipsoid(bm, P(tf, height * 0.36, tx), (Vector((0, -1, 0)), Vector((0, 0, 1)), X), (length * 0.20, height * 0.36, width * 0.15))
        if dewclaw:
            ellipsoid(bm, P(fc - 0.25 * length, height * 1.6, xc - sx * width * 0.38), (Vector((0, -1, 0)), Vector((0, 0, 1)), X), (length * 0.10, height * 0.25, width * 0.10))

    def feet(self, bm, hard):
        """hard=False: the paws (part of the skin); hard=True: hoof walls and
        dew claws (keratin: separate closed parts, rigid on their bones)."""
        F = self.S["feet"]
        for leg in ("fore", "hind"):
            ft = F[leg]
            for sx in (1, -1):
                xc = sx * ft["x"]
                if F["type"] == "paw":
                    if not hard:
                        self.paw(bm, ft["f"], xc, ft["length"], ft["width"], ft["height"], dewclaw=ft.get("dewclaw", False), sx=sx)
                elif hard:
                    self.hoof(bm, ft["f"], xc, ft["width"], ft["length"], ft["toe"], ft["heel"], ft.get("slant", 0.35), ft.get("flare", 0.16), ft.get("cleft", 0.0) if F["type"] == "cloven" else 0.0)
                if hard:
                    for dc in ft.get("dew_claws", []):
                        for s2 in (1, -1):
                            ellipsoid(bm, P(dc[0], dc[1], xc + s2 * dc[2]), (Vector((0, -1, 0)), Vector((0, 0, 1)), X), tuple(dc[3]), segs=16, rings_=10)

    def blobs(self, bm):
        """Extra masses: [f, z, x, [long, across, lateral], [axis f, z], mirror?]"""
        for m in self.S.get("muscles", []):
            mirror = m[5] if len(m) > 5 else True
            tilt = m[6] if len(m) > 6 else 0.0
            muscle(bm, m[0], m[1], m[2], tuple(m[3]), tuple(m[4]), mirror=mirror, tilt_out=tilt)
        for m in self.S["head"].get("masses", []):
            # head masses in the head frame: [a, b, x, [along, in, lateral], mirror?]
            d = (P(*self.hd) - P(0, 0)).normalized()
            n_ = (P(*self.hn) - P(0, 0)).normalized()
            for sx in ([1, -1] if (len(m) < 5 or m[4]) and m[2] != 0 else [1]):
                ellipsoid(bm, self.HV(m[0], m[1], sx * m[2]), (d, n_, X), tuple(m[3]))

    def dewlap(self, bm):
        d = self.S["neck"].get("dewlap")
        if not d:
            return
        # a thin fold hanging under the neck: [f, z top, z bottom, half width]
        keys = [(P(k[0], k[1]), P(k[0], k[2]), profile(k[3], 0.35, 2.0, 2.2)) for k in d]
        section_part(bm, keys, 30, 24)

    def udder(self, bm):
        u = self.S.get("udder")
        if not u:
            return
        c = P(u["f"], u["z"])
        ellipsoid(bm, c, (Vector((0, -1, 0)), Vector((0, 0, 1)), X), tuple(u["r"]))
        for e in u.get("attach", []):
            # the attachments that tie the udder into the belly and up between the thighs
            ellipsoid(bm, P(e[0], e[1]), (Vector((0, -1, 0)), Vector((0, 0, 1)), X), tuple(e[2]))
        for tf, tx in u["teats"]:
            for sx in (1, -1):
                top = P(u["f"] + tf, u["z"] - u["r"][1] * 0.6, sx * tx)
                bot = P(u["f"] + tf, u["z"] - u["r"][1] - u["teat_len"], sx * tx)
                tube(bm, spline([top, (top + bot) / 2, bot], 6), [u["teat_r"]] * 6 + [u["teat_r"] * 0.7], n=12)

    def mane(self, bm):
        m = self.S.get("mane")
        if not m:
            return
        # a solid proxy shell along the crest: [f, z (crest), height, half width]
        keys = [(P(k[0], k[1] + k[2]), P(k[0], k[1] - 0.02), profile(k[3], 0.55, 2.0, 2.0)) for k in m["keys"]]
        section_part(bm, keys, 40, 24)


def parts(B):
    """The skin's solid parts, as one bmesh (unioned by the voxel remesh);
    eyes, horns and hooves are separate (separate())."""
    bm = bmesh.new()
    section_part(bm, B.trunk_keys(), 80, 64)
    section_part(bm, B.neck_keys(), 40, 48)
    section_part(bm, B.head_keys(), 60, 56)
    for leg in ("fore", "hind"):
        for sx in (1, -1):
            loft(bm, B.leg_rings(leg, sx))
    B.blobs(bm)
    if not B.S["tail"].get("separate"):
        B.tail(bm)
    B.feet(bm, hard=False)
    B.dewlap(bm)
    B.udder(bm)
    B.mane(bm)
    return bm


def separate(B):
    """The closed parts kept out of the skin's union, each its own solid at
    its own resolution (thinner than a quad of the skin, or hard and rigid
    on one bone): [(region rule, bmesh)] with rule `eyes`, `ears`, `horns`,
    `hooves`. Ears are thick-walled cups thinner than a skin quad: their base
    sinks into the head and the ear bones carry them."""
    out = []
    bm = bmesh.new()
    e = B.S["eyes"]
    for sx in (1, -1):
        c = B.eye_centre(sx)
        ellipsoid(bm, c, (Vector((0, -1, 0)), Vector((0, 0, 1)), X), (e["r"] * e.get("long", 1.0), e["r"] * e.get("tall", 0.9), e["r"] * 0.85), segs=16, rings_=10)
    out.append(("eyes", bm))
    bm = bmesh.new()
    for sx in (1, -1):
        B.ear(bm, sx)
    out.append(("ears", bm))
    if B.S["tail"].get("separate"):
        # a tail thinner than the skin's quads (a dog's) would come out of the
        # remesh as a flat ribbon: its own tube, the root sunk into the rump,
        # carried by the tail bones
        bm = bmesh.new()
        B.tail(bm, sink=2.0 * B.S["tail"]["line"][0][2])
        out.append(("tail", bm))
    if B.S.get("horns"):
        bm = bmesh.new()
        for sx in (1, -1):
            B.horn(bm, sx)
        out.append(("horns", bm))
    if B.S["feet"]["type"] != "paw" or any(B.S["feet"][k].get("dew_claws") for k in ("fore", "hind")):
        bm = bmesh.new()
        B.feet(bm, hard=True)
        if bm.verts:
            out.append(("hooves", bm))
        else:
            bm.free()
    return out


def landmarks(B):
    """Bone heads and tails (Blender points) for the Rigify horse metarig's
    names, from the species numbers."""
    S = B.S
    L = {}
    sp = [P(*p) for p in S["spine"]]
    for i in range(6):
        L["spine.%03d" % (i + 1)] = (sp[i], sp[i + 1])
    # neck: through the middle of the neck's sections, base to the poll
    nk = [(A + Bb) / 2 for A, Bb, _ in B.neck_keys()]
    nk[0] = sp[-1]
    nk[-1] = B.HV(0.0, B.S["head"].get("atlas_b", 0.08))
    nk = spline(nk, 4)
    for i in range(4):
        L["neck.%03d" % (i + 1)] = (nk[i], nk[i + 1])
    tl = [c for c, _ in B.tail_line(5)]
    for i in range(5):
        L["tail.%03d" % (i + 1)] = (tl[i], tl[i + 1])
    j = S["rig"]
    L["abdomen"] = (P(*j["abdomen"][0]), P(*j["abdomen"][1]))
    L["chest"] = (P(*j["chest"][0]), P(*j["chest"][1]))
    L["hip"] = (sp[0], P(*j["hip_tail"]))
    hl = B.head_len
    L["head"] = (nk[-1], B.HV(hl * 0.82, hl * 0.18))
    L["skull"] = (B.HV(hl * 0.08, hl * 0.06), B.HV(hl * 0.90, hl * 0.16))
    L["jaw"] = (B.HV(*j["jaw"][0]), B.HV(*j["jaw"][1]))
    L["jaw.001"] = (B.HV(*j["jaw"][1]), B.HV(*j["jaw"][2]))
    fo, hi = B.joints("fore"), B.joints("hind")
    for sx, s in ((1, "L"), (-1, "R")):
        def LP(pt, leg):
            return P(pt[0], pt[1], sx * B.leg_x(leg, pt[1]))
        L["pelvis." + s] = (sp[0], P(hi["hip"][0] + 0.12 * hl, hi["hip"][1] + 0.1 * hl, sx * B.leg_x("hind", hi["hip"][1])))
        L["thigh." + s] = (LP(hi["hip"], "hind"), LP(hi["stifle"], "hind"))
        L["lower_leg." + s] = (LP(hi["stifle"], "hind"), LP(hi["hock"], "hind"))
        L["hind_foot." + s] = (LP(hi["hock"], "hind"), LP(hi["fetlock"], "hind"))
        L["r_toe." + s] = (LP(hi["fetlock"], "hind"), LP(hi["coronet"], "hind"))
        L["r_hoof." + s] = (LP(hi["coronet"], "hind"), LP((hi["toe"][0], 0.004), "hind"))
        L["shoulder." + s] = (LP(fo["scapula"], "fore"), LP(fo["shoulder"], "fore"))
        L["upper_arm." + s] = (LP(fo["shoulder"], "fore"), LP(fo["elbow"], "fore"))
        L["forearm." + s] = (LP(fo["elbow"], "fore"), LP(fo["wrist"], "fore"))
        L["forefoot." + s] = (LP(fo["wrist"], "fore"), LP(fo["fetlock"], "fore"))
        L["f_toe." + s] = (LP(fo["fetlock"], "fore"), LP(fo["coronet"], "fore"))
        L["f_hoof." + s] = (LP(fo["coronet"], "fore"), LP((fo["toe"][0], 0.004), "fore"))
        br = j["breast"]
        L["breast." + s] = (P(br[0][0], br[0][1], sx * br[2]), P(br[1][0], br[1][1], sx * br[2]))
        e = S["ears"]
        L["ear." + s] = (B.ear_centre(sx, -0.1), B.ear_centre(sx, 0.45))
        L["ear.%s.001" % s] = (B.ear_centre(sx, 0.45), B.ear_centre(sx, 1.0))
        ec = B.eye_centre(sx)
        L["eye." + s] = (ec - X * sx * 0.02 * hl, ec + X * sx * 0.12 * hl)
        L["nose." + s] = (B.HV(hl * 0.92, hl * 0.12, sx * 0.05 * hl), B.HV(hl * 0.92, hl * 0.12, sx * 0.14 * hl))
        L["skull." + s] = (B.HV(hl * 0.08, hl * 0.06), B.HV(hl * 0.45, hl * 0.35, sx * 0.18 * hl))
        del e
    return L
