# Rig, weights, markers, clips and export for the Evani.Dev test horse
# (called by build_horse.py; see the notes at the top of that file).
#
# The skeleton is Blender's own Rigify horse metarig (bone names and
# hierarchy unchanged), fitted to this horse's landmarks. It is used as the
# deform skeleton directly (no generated control rig): the glb keeps
# Rigify's names, which the converter's region chart reads (spine, neck,
# head, tail, ear, leg words), and the mane and forelock bones ride along,
# unweighted, as roots for the later hair-strand job.
#
# Clips (30 fps, both loop):
#   walk  32 frames (1.07 s): 4-beat walk, left hind, left fore, right hind,
#         right fore, a quarter cycle apart; 1.55 m per stride in place (the
#         proof moves the horse at 1.45 m/s). Hooves are planted (solved, not
#         keyed by hand): the foreleg is a straight strut in stance, the hind
#         leg's stifle and hock move together (the reciprocal apparatus:
#         femur and cannon turn by the same angle).
#   idle  120 frames (4 s): weight shift, one breath (belly), an ear flick
#         each side, a slow head bob and a tail swish; hooves planted.
import bpy, math, json, os
from mathutils import Vector, Matrix, Quaternion
H = None  # build_horse (set by it: the landmarks and helpers)

FPS = 30
WALK_FRAMES = 32
IDLE_FRAMES = 120
STRIDE = float(os.environ.get('HORSE_STRIDE', '1.55'))
DUTY = 0.60
SCAPULA_SHARE = float(os.environ.get("HORSE_SCAPULA", "0.4"))
# Swing phase: hoof lift (m), extra hoof fold and fetlock flex (deg). A
# walking horse clears the ground by a few centimetres; deeper folds bent
# the stifle and elbow past 60 degrees and tore the skin behind them.
SWING_LIFT = 0.075
SWING_FOLD = 26.0
SWING_FLEX = 32.0
FORE_SWING_REACH = float(os.environ.get('HORSE_FORE_REACH', '0.975'))   # elbow to fetlock in swing, of forearm + cannon


def P(f, z, x=0.0):
    return Vector((x, -f, z))


def headp(a, d, x=0.0):
    return P(*H.head_pt(a, d), x)


def LMP(name, x=0.0):
    return P(*H.LM[name], x)


# ------------------------------------------------------------- landmarks
def landmarks(mesh_ob):
    """Every bone from the anatomy's landmarks (tools/models/horse/anatomy.py),
    so the joints sit where the mesh bends."""
    A = H.anatomy
    L = {}
    sp = [(-0.640, 1.400), (-0.400, 1.420), (-0.180, 1.390), (0.020, 1.360), (0.220, 1.370), (0.420, 1.380), (0.650, 1.335)]
    for i in range(6):
        L["spine.%03d" % (i + 1)] = (P(*sp[i]), P(*sp[i + 1]))
    # neck: through the middle of the neck's sections, base to the atlas
    nk = [(0.650, 1.335), (0.800, 1.470), (0.925, 1.625), (1.045, 1.775), (1.170, 1.885)]
    for i in range(4):
        L["neck.%03d" % (i + 1)] = (P(*nk[i]), P(*nk[i + 1]))
    tl = [(f, z) for f, z, r in A.DOCK[:5]] + [(A.DOCK[-1][0], A.DOCK[-1][1])]
    for i in range(5):
        L["tail.%03d" % (i + 1)] = (P(*tl[i]), P(*tl[i + 1]))
    L["abdomen"] = (P(0.000, 1.250), P(0.020, 0.920))
    L["chest"] = (P(0.400, 1.250), P(0.420, 0.900))
    L["hip"] = (P(-0.640, 1.400), P(-0.480, 1.120))
    L["head"] = (P(*nk[-1]), headp(0.52, 0.10))
    L["skull"] = (headp(0.05, 0.04), headp(0.60, 0.10))
    L["jaw"] = (headp(0.12, 0.22), headp(0.40, 0.165))
    L["jaw.001"] = (headp(0.40, 0.165), headp(0.62, 0.14))
    for sx, s in ((1, "L"), (-1, "R")):
        X = lambda v: Vector((v.x * sx, v.y, v.z))
        L["pelvis." + s] = (P(-0.640, 1.400), X(P(-0.380, 1.480, 0.200)))
        L["thigh." + s] = (X(LMP("hip_joint", 0.195)), X(LMP("stifle", 0.200)))
        L["lower_leg." + s] = (X(LMP("stifle", 0.200)), X(LMP("hock", 0.150)))
        L["hind_foot." + s] = (X(LMP("hock", 0.150)), X(LMP("hind_fetlock", 0.145)))
        L["r_toe." + s] = (X(LMP("hind_fetlock", 0.145)), X(LMP("hind_coronet", 0.145)))
        L["r_hoof." + s] = (X(LMP("hind_coronet", 0.145)), X(P(H.LM["hind_toe"][0], 0.004, 0.145)))
        # the shoulder blade turns about a point a third of the way down it
        # (not its top at the withers): the point of the shoulder moves a
        # few centimetres in the walk, as in a real horse
        L["shoulder." + s] = (X(P(0.560, 1.400, 0.130)), X(LMP("shoulder_joint", 0.160)))
        L["upper_arm." + s] = (X(LMP("shoulder_joint", 0.160)), X(LMP("elbow", 0.172)))
        L["forearm." + s] = (X(LMP("elbow", 0.172)), X(LMP("knee", 0.164)))
        L["forefoot." + s] = (X(LMP("knee", 0.164)), X(LMP("fore_fetlock", 0.156)))
        L["f_toe." + s] = (X(LMP("fore_fetlock", 0.156)), X(LMP("fore_coronet", 0.155)))
        L["f_hoof." + s] = (X(LMP("fore_coronet", 0.155)), X(P(H.LM["fore_toe"][0], 0.004, 0.155)))
        L["breast." + s] = (X(P(0.750, 1.120, 0.070)), X(P(0.840, 1.020, 0.070)))
        base, up, fwd, side = H.ear_frame(sx)
        L["ear." + s] = (base - up * 0.01, base + up * 0.075)
        L["ear.%s.001" % s] = (base + up * 0.075, base + up * 0.165)
        L["eye." + s] = (headp(H.EYE_AB[0], H.EYE_AB[1], sx * 0.09), headp(H.EYE_AB[0], H.EYE_AB[1], sx * 0.17))
        L["nose." + s] = (headp(H.NOSTRIL_AB[0], H.NOSTRIL_AB[1], sx * 0.045), headp(H.NOSTRIL_AB[0], H.NOSTRIL_AB[1], sx * 0.09))
        L["skull." + s] = (headp(0.05, 0.04), headp(0.30, 0.20, sx * 0.10))
    # mane roots along the crest, poll (01) to withers (05); 06 the forelock
    crest = crest_line(mesh_ob)
    for i, f in enumerate(MANE_F):
        z = crest(f)
        b = P(f, z - 0.012)
        L["mane_base.%02d" % (i + 1)] = (b, b + Vector((0, 0.025, 0.05)))
        L["mane_top.%02d" % (i + 1)] = (b + Vector((0, 0.025, 0.05)), b + Vector((0, 0.05, 0.11)))
    fb = headp(0.05, 0.0)
    L["mane_base.06"] = (fb + Vector((0, 0, -0.01)), fb + Vector((0, -0.04, 0.04)))
    L["mane_top.06"] = (fb + Vector((0, -0.04, 0.04)), fb + Vector((0, -0.09, 0.06)))
    return L


MANE_F = [1.16, 1.04, 0.91, 0.77, 0.62]


def crest_line(ob):
    pts = [(-v.co.y, v.co.z) for v in ob.data.vertices if abs(v.co.x) < 0.03]

    def f_(f):
        near = [z for ff, z in pts if abs(ff - f) < 0.025]
        return max(near) if near else 1.6
    return f_


DEFORM = {"spine.001", "spine.002", "spine.003", "spine.004", "spine.005", "spine.006", "abdomen", "chest",
          "neck.001", "neck.002", "neck.003", "neck.004", "head", "jaw", "jaw.001",
          "tail.001", "tail.002", "tail.003", "tail.004", "tail.005"}
for _s in ("L", "R"):
    DEFORM |= {n + "." + _s for n in ("thigh", "lower_leg", "hind_foot", "r_toe", "r_hoof", "shoulder", "upper_arm",
                                       "forearm", "forefoot", "f_toe", "f_hoof", "breast", "ear")}
    DEFORM.add("ear.%s.001" % _s)

# Helper bones: each turns half as far as the bone it follows (same head,
# same rest direction, same parent), so the skin across the hip, shoulder,
# stifle and elbow blends over three transforms instead of two and the bend
# spreads over a wide band. Keyed in the clips like every other bone. Names
# carry a leg word, so the converter's body chart reads them as leg.
HELPERS = {}
for _s in ("L", "R"):
    HELPERS["thigh_half." + _s] = "thigh." + _s
    HELPERS["upper_arm_half." + _s] = "upper_arm." + _s
    HELPERS["stifle." + _s] = "lower_leg." + _s
    HELPERS["elbow." + _s] = "forearm." + _s
DEFORM |= set(HELPERS)


def make_metarig():
    try:
        bpy.ops.preferences.addon_enable(module="rigify")
    except Exception as e:  # noqa
        print("HORSE_NOTE rigify enable:", e)
    import importlib
    horse = importlib.import_module("rigify.metarigs.Animals.horse")
    arm = bpy.data.armatures.new("horse_rig")
    ob = bpy.data.objects.new("horse_rig", arm)
    bpy.context.scene.collection.objects.link(ob)
    H.activate(ob)
    horse.create(ob)
    return ob


def fit(rig, L):
    H.activate(rig)
    bpy.ops.object.mode_set(mode="EDIT")
    eb = rig.data.edit_bones
    missing = [b.name for b in eb if b.name not in L]
    for b in eb:
        b.use_connect = False
    for n, (h, t) in L.items():
        if n in eb:
            eb[n].head, eb[n].tail = h, t
    # rolls: z axis up (or forward for vertical bones), the same on both sides
    for b in eb:
        d = (b.tail - b.head).normalized()
        b.align_roll(Vector((0, -1, 0)) if abs(d.z) > 0.7 else Vector((0, 0, 1)))
    for hn, dn in HELPERS.items():
        d = eb[dn]
        h = eb.new(hn)
        h.head = d.head.copy()
        h.tail = d.head + (d.tail - d.head).normalized() * 0.12
        h.roll = d.roll
        h.parent = d.parent
    for b in eb:
        if b.parent and (b.head - b.parent.tail).length < 1e-5 and b.name not in HELPERS:
            b.use_connect = True
        b.use_deform = b.name in DEFORM
    bpy.ops.object.mode_set(mode="OBJECT")
    if missing:
        print("HORSE_NOTE bones kept where the metarig had them:", missing)
    rig.data.pose_position = "POSE"
    for pb in rig.pose.bones:
        pb.rotation_mode = "QUATERNION"


# ------------------------------------------------------------- weights
def skin(mesh, rig):
    H.activate(mesh)
    rig.select_set(True)
    bpy.context.view_layer.objects.active = rig
    for hn in HELPERS:
        rig.data.bones[hn].use_deform = False   # weighted by joint_blend, not by heat
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    for hn in HELPERS:
        rig.data.bones[hn].use_deform = True
    mesh.parent = rig
    vg = mesh.vertex_groups
    me = mesh.data
    # hooves are horn: rigid on their hoof bone
    for p in me.polygons:
        if p.material_index != 1:
            continue
        for vi in p.vertices:
            v = me.vertices[vi]
            f = -v.co.y
            side = "L" if v.co.x >= 0 else "R"
            bone = ("f_hoof." if f > 0 else "r_hoof.") + side
            for g in v.groups:
                vg[g.group].remove([vi])
            vg[bone].add([vi], 1.0, "REPLACE")
    # a left limb never pulls the right side (and back)
    for v in me.vertices:
        for g in list(v.groups):
            n = vg[g.group].name
            if (n.endswith(".L") and v.co.x < -0.03) or (n.endswith(".R") and v.co.x > 0.03):
                if not n.startswith("breast") and not n.startswith("pelvis"):
                    vg[g.group].remove([v.index])
    H.activate(mesh)
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_smooth(group_select_mode="BONE_DEFORM", factor=0.5, repeat=int(os.environ.get("HORSE_SMOOTH", "8")), expand=0.0)
    bpy.ops.object.mode_set(mode="OBJECT")
    midline(mesh, rig)
    bands(mesh)
    H.activate(mesh)
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_smooth(group_select_mode="BONE_DEFORM", factor=0.35, repeat=1, expand=0.0)
    bpy.ops.object.mode_set(mode="OBJECT")
    midline(mesh, rig)
    H.activate(mesh)
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_smooth(group_select_mode="BONE_DEFORM", factor=0.5, repeat=int(os.environ.get("HORSE_SMOOTH2", "4")), expand=0.0)
    bpy.ops.object.mode_set(mode="OBJECT")
    # v3: the limbs' share of the skin is rebuilt from anatomy (limb_share),
    # replacing v2's local smoothing zones and thigh mask, then the helpers
    limb_share(mesh, rig)
    joint_blend(mesh)
    H.activate(mesh)
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_clean(group_select_mode="BONE_DEFORM", limit=0.01)
    bpy.ops.object.vertex_group_limit_total(group_select_mode="BONE_DEFORM", limit=4)
    bpy.ops.object.vertex_group_normalize_all(group_select_mode="BONE_DEFORM", lock_active=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    # any vertex left without a weight: its nearest bone
    deform = [b for b in rig.data.bones if b.use_deform]
    lost = 0
    for v in me.vertices:
        if sum(g.weight for g in v.groups if vg[g.group].name in DEFORM) < 1e-4:
            best = min(deform, key=lambda b: seg_dist(v.co, b.head_local, b.tail_local))
            vg[best.name].add([v.index], 1.0, "REPLACE")
            lost += 1
    print("HORSE_WEIGHTS", json.dumps({"groups": len(vg), "unweighted_fixed": lost}))


# ------------------------------------------------------------- limb share (v3)
# v2's walk stretched a sheet of skin from the flank to the hock ("bat
# wing"): the thigh mask handed the stifle to the belly (the femur held 6-8
# percent of the skin over the stifle, the abdomen 35, the gaskin 55), so the
# stifle stayed with the belly while the gaskin swung back. v3 decides, per
# vertex, how much a limb owns the skin (T, 0 trunk to 1 limb) by anatomy:
# anchors where the answer is plain (the leg below the stifle or elbow, the
# outside of the thigh behind the flank fold, the arm over the triceps: limb;
# the midline, the belly in front of the flank fold, the croup, the girth,
# the withers: trunk), and an even (harmonic) blend over the surface in
# between, so the flank fold, groin and armpit stretch evenly over 15-20 cm
# instead of over two or three edges. Inside the hind limb the stifle is a
# plane split (femur above, gaskin below, 7 cm each way), so the skin above
# the stifle follows the femur. Then joint_blend adds the half-turn helpers.
HIND_LIMB = ("thigh", "lower_leg", "hind_foot", "r_toe", "r_hoof")
FORE_LIMB = ("shoulder", "upper_arm", "forearm", "forefoot", "f_toe", "f_hoof")
HIND_CREASE = ((-0.300, 1.420), (-0.170, 0.990))   # the front of the thigh: point of the hip to the stifle fold
HIND_OUT = float(os.environ.get("HORSE_HIND_OUT", "0.17"))       # the outside of the thigh owned by the limb: |x| past this
HIND_FRONT = float(os.environ.get("HORSE_HIND_FRONT", "0.18"))   # the trunk owns the flank this far in front of the crease (low down)
HIND_FRONT_TOP = float(os.environ.get("HORSE_HIND_FRONT_TOP", "0.06"))   # and this far near the point of the hip
FORE_ARM = float(os.environ.get("HORSE_FORE_ARM", "0.55"))       # the arm's outside owned by the limb forward of this f
FORE_GIRTH = float(os.environ.get("HORSE_FORE_GIRTH", "0.30"))   # the trunk owns the girth behind this f
T_ROUND = int(os.environ.get("HORSE_T_ROUND", "10"))   # relaxation rounds over the limb share after the solve
PATELLA = (-0.225, 0.955)   # the front of the stifle: always the leg's (else it lags with the belly)
STIFLE_BAND = 0.07                                       # the blend's half width at the joint (m)
STIFLE_TILT = math.radians(float(os.environ.get("HORSE_STIFLE_TILT", "8.5")))
STIFLE_K = float(os.environ.get("HORSE_STIFLE_K", "0.42"))  # and its half angle far from it (sine, about 25 deg)


def _np():
    import numpy as np
    return np


def harmonic(ev, n, x0, free, iters=1500):
    """x0 (n,) values; the free vertices relax to the mean of their
    neighbours (Jacobi), the rest are held."""
    np = _np()
    x = x0.astype(np.float64).copy()
    a, b = ev[:, 0], ev[:, 1]
    deg = np.maximum(np.bincount(a, minlength=n) + np.bincount(b, minlength=n), 1)
    fi = np.nonzero(free)[0]
    if len(fi) == 0:
        return x
    for _ in range(iters):
        s = np.bincount(a, weights=x[b], minlength=n) + np.bincount(b, weights=x[a], minlength=n)
        x[fi] = s[fi] / deg[fi]
    return x


def _ss_np(e0, e1, x):
    np = _np()
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def _seg2(pf, pz, a, b):
    np = _np()
    af, az = a
    bf, bz = b
    df, dz = bf - af, bz - az
    t = np.clip(((pf - af) * df + (pz - az) * dz) / (df * df + dz * dz), 0.0, 1.0)
    return np.hypot(pf - (af + df * t), pz - (az + dz * t))


def _dist(W, cols, zone, ev, n):
    """The share of W among cols, normalised; where these bones hold less
    than 0.4 of a zone vertex, filled from its neighbours (harmonic)."""
    np = _np()
    sub = W[:, cols]
    tot = sub.sum(axis=1)
    D = np.where(tot[:, None] > 1e-6, sub / np.maximum(tot[:, None], 1e-9), 0.0)
    free = zone & (tot < 0.4)
    if free.any():
        for k in range(len(cols)):
            D[:, k] = harmonic(ev, n, D[:, k], free, 600)
        s = D.sum(axis=1)
        D = np.where(s[:, None] > 1e-6, D / np.maximum(s[:, None], 1e-9), 0.0)
    return D


def limb_share(mesh, rig):
    np = _np()
    me = mesh.data
    vg = mesh.vertex_groups
    n = len(me.vertices)
    co = np.empty(n * 3)
    me.vertices.foreach_get("co", co)
    co = co.reshape(n, 3)
    X, F, Z = co[:, 0], -co[:, 1], co[:, 2]
    ev = np.empty(len(me.edges) * 2, dtype=np.int64)
    me.edges.foreach_get("vertices", ev)
    ev = ev.reshape(-1, 2)
    names = [g.name for g in vg]
    col = {nm: i for i, nm in enumerate(names)}
    W = np.zeros((n, len(names)))
    for v in me.vertices:
        for g in v.groups:
            if names[g.group] in DEFORM:
                W[v.index, g.group] = g.weight
    rows = W.sum(axis=1)
    W = np.where(rows[:, None] > 1e-9, W / np.maximum(rows[:, None], 1e-9), W)
    LM = H.LM
    touched = np.zeros(n, dtype=bool)
    report = {}
    for side, sg in (("L", 1.0), ("R", -1.0)):
        xs = X * sg
        on = xs > -0.005
        for limb, chain in (("hind", HIND_LIMB), ("fore", FORE_LIMB)):
            lc = [col[c + "." + side] for c in chain if c + "." + side in col]
            if limb == "hind":
                (p1f, p1z), (p2f, p2z) = HIND_CREASE
                L_ = math.hypot(p2f - p1f, p2z - p1z)
                uf, uz = (p2f - p1f) / L_, (p2z - p1z) / L_
                d_front = (F - p1f) * (-uz) + (Z - p1z) * uf     # + in front of the crease
                hf, hz = LM["hip_joint"]
                sf, sz = LM["stifle"]
                lf = math.hypot(sf - hf, sz - hz)
                t_ax = (F - hf) * (sf - hf) / lf + (Z - hz) * (sz - hz) / lf   # down the femur from the hip
                patella = np.hypot(F - PATELLA[0], Z - PATELLA[1])
                one = on & (xs > 0.05) & (((Z < 0.90) & (F < -0.20)) | ((d_front < -0.08) & (t_ax > 0.14) & (xs > HIND_OUT)) | ((patella < 0.06) & (xs > 0.15)))
                # the flank blend is wide low down (the stifle fold) and narrow near
                # the point of the hip: skin far in front of the hip joint would
                # bob up and down with the femur and fold
                front_lim = HIND_FRONT_TOP + (HIND_FRONT - HIND_FRONT_TOP) * (1 - _ss_np(1.02, 1.22, Z))
                zero = (xs < 0.02) | (d_front > front_lim) | (t_ax < -0.06) | (F > 0.05) | (Z > 1.50)
                box = on & (F > -0.88) & (F < 0.05) & (Z > 0.80) & (Z < 1.50)
            else:
                scap = _seg2(F, Z, (0.560, 1.400), LM["shoulder_joint"])
                one = on & (((Z < 0.76) & (xs > 0.07) & (F > 0.45)) |
                            ((xs > 0.17) & (Z > 0.86) & (Z < 1.22) & (F > FORE_ARM) & (F < 0.82)) |
                            ((xs > 0.15) & (Z > 1.10) & (Z < 1.40) & (scap < 0.07)))
                zero = (xs < 0.02) | (F < FORE_GIRTH) | (Z > 1.47) | ((F > 0.80) & (Z > 1.30)) | (F > 0.90)
                box = on & (F > FORE_GIRTH) & (F < 0.90) & (Z > 0.70) & (Z < 1.47)
            one &= ~zero
            free = box & ~one & ~zero
            T0 = np.where(one, 1.0, 0.0)
            # start from the limb's present share, so few rounds are needed
            T0 = np.where(free, np.clip(W[:, lc].sum(axis=1), 0, 1), T0)
            T = harmonic(ev, n, T0, free, 2500)
            T = np.where(on, T, 0.0)
            # round off the edges of the fixed patches (a kink where an
            # anchor meets the free blend concentrates the stretch): a few
            # relaxation rounds over the whole box, the leg proper held
            deep = on & (((Z < 0.86) & (F < -0.25)) if limb == "hind" else ((Z < 0.72) & (F > 0.45)))
            T = harmonic(ev, n, T, box & ~deep & (xs >= 0.02), T_ROUND)
            T = np.where(deep, 1.0, T)
            T[T < 0.002] = 0.0
            has = W[:, lc].sum(axis=1) > 1e-6
            sel = on & ((T > 0) | has)
            if not sel.any():
                continue
            other = [i for i, nm in enumerate(names) if nm in DEFORM and i not in lc and nm not in HELPERS and W[sel, i].max() > 0.005]
            zone = sel & (T > 0)
            Dt = _dist(W, other, zone & (T < 0.999), ev, n)
            if limb == "hind":
                # the femur above the stifle, the gaskin and below beneath it
                hf, hz = LM["hip_joint"]
                sf, sz = LM["stifle"]
                kf, kz = LM["hock"]
                a1 = np.array([hf - sf, hz - sz]); a1 /= np.linalg.norm(a1)
                a2 = np.array([kf - sf, kz - sz]); a2 /= np.linalg.norm(a2)
                nn = a2 - a1
                nn /= np.linalg.norm(nn)
                # tilted so the split runs lower at the back of the leg,
                # where the hamstrings end (about 7 cm below the stifle)
                ct, st = math.cos(STIFLE_TILT), math.sin(STIFLE_TILT)
                nn = np.array([nn[0] * ct - nn[1] * st, nn[0] * st + nn[1] * ct])
                # by angle round the stifle, not a flat slab: far behind
                # the joint (the back of the thigh is 45 cm from it) the
                # blend widens with the distance, so the hamstrings bend
                # over a wide band instead of creasing along a line
                s_ = (F - sf) * nn[0] + (Z - sz) * nn[1]
                r_ = np.maximum(np.hypot(F - sf, Z - sz), STIFLE_BAND / STIFLE_K)
                u = _ss_np(-STIFLE_K, STIFLE_K, s_ / r_)
                low = [col[c + "." + side] for c in HIND_LIMB[1:]]
                Dl = _dist(W, low, zone, ev, n)
                Dlimb = np.zeros((n, len(lc)))
                Dlimb[:, 0] = 1 - u
                for k in range(len(low)):
                    Dlimb[:, 1 + k] = u * Dl[:, k]
            else:
                Dlimb = _dist(W, lc, zone, ev, n)
            idx = np.nonzero(sel)[0]
            newW = W.copy()
            newW[np.ix_(idx, other)] = (1 - T[idx, None]) * Dt[np.ix_(idx, range(len(other)))]
            newW[np.ix_(idx, lc)] = T[idx, None] * Dlimb[idx]
            # vertices the limb does not own: the limb's old weights go to the rest
            W = newW
            touched |= sel
            report[limb + "." + side] = {"limb": int((T > 0.5).sum()), "blend": int(((T > 0.02) & (T < 0.98)).sum())}
    # keep the four largest weights, then write back
    idx = np.nonzero(touched)[0]
    for i in idx:
        w = W[i]
        order = np.argsort(-w)
        keep = order[:4]
        ww = np.zeros_like(w)
        ww[keep] = w[keep]
        ww[ww < 0.005] = 0.0
        s = ww.sum()
        W[i] = ww / s if s > 0 else w
    for gi, nm in enumerate(names):
        if nm not in DEFORM:
            continue
        vg[gi].remove([int(i) for i in idx])
    for i in idx:
        for gi in np.nonzero(W[i] > 0)[0]:
            vg[int(gi)].add([int(i)], float(W[i, gi]), "REPLACE")
    print("HORSE_LIMB_SHARE", json.dumps(report))


def joint_blend(mesh):
    """Across each helper's joint, a vertex held by the parent side (weight
    p) and the child bone (weight c) is re-blended as a quadratic Bezier of
    the three transforms: with s = c / (p + c), the parent side keeps
    (1 - s)^2, the helper (half the turn) 2 s (1 - s), the child s^2. Hip
    and shoulder first (parent side: every trunk bone), then stifle and
    elbow (parent side: the thigh or upper arm and its helper)."""
    vg = mesh.vertex_groups
    for hn in HELPERS:
        if hn not in vg:
            vg.new(name=hn)
    names = {g.index: g.name for g in vg}
    order = []
    for s in ("L", "R"):
        order += [("thigh_half." + s, "thigh." + s, None), ("upper_arm_half." + s, "upper_arm." + s, None),
                  ("stifle." + s, "lower_leg." + s, ("thigh." + s, "thigh_half." + s)),
                  ("elbow." + s, "forearm." + s, ("upper_arm." + s, "upper_arm_half." + s))]
    for hn in HELPERS:
        if hn not in vg:
            vg.new(name=hn)
    n = 0
    for hn, cn, par in order:
        for v in mesh.data.vertices:
            ws = {names[g.group]: g.weight for g in v.groups if names[g.group] in DEFORM}
            c = ws.get(cn, 0.0)
            if c <= 1e-4:
                continue
            if par is None:
                side = cn[-1]
                pk = [k for k in ws if (k != cn and not k.endswith("." + side)) or k.startswith(("breast.", "pelvis.", "shoulder."))]
                pk = [k for k in pk if k not in HELPERS and k != cn]
            else:
                pk = [k for k in par if k in ws]
            p = sum(ws[k] for k in pk)
            if p <= 1e-4:
                continue
            t = p + c
            sc = c / t
            for k in pk:
                vg[k].add([v.index], ws[k] * (1 - sc), "REPLACE")
            vg[cn].add([v.index], t * sc * sc, "REPLACE")
            vg[hn].add([v.index], ws.get(hn, 0.0) + t * 2 * sc * (1 - sc), "REPLACE")
            n += 1
    print("HORSE_JOINT_BLEND", n)


def _ss(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def limb_mask(name, co):
    """How much a limb bone may hold a vertex: none on the midline of the
    belly, chest and groin (where a left and a right leg would pull one
    edge two ways), fading in toward the leg."""
    ax, z, f = abs(co.x), co.z, -co.y
    base = name.split(".")[0]
    if base in ("lower_leg", "hind_foot", "r_toe", "r_hoof"):
        return _ss(0.035, 0.085, ax) * (1 - _ss(1.0, 1.08, z))
    if base == "thigh":
        return _ss(0.03, 0.15, ax) * (1 - _ss(-0.32, -0.20, f))
    if base in ("forearm", "forefoot", "f_toe", "f_hoof"):
        return _ss(0.0, 0.06, ax) * (1 - _ss(0.86, 0.96, z))
    if base == "upper_arm":
        return _ss(0.03, 0.15, ax)
    if base == "shoulder":
        return _ss(0.03, 0.08, ax)
    return 1.0


TRUNK = ("spine.", "abdomen", "chest", "breast.")
UPPER = ("spine.", "abdomen", "chest", "breast.", "thigh.", "upper_arm.", "shoulder.")


def bands(mesh):
    """Wide, even blends where a limb leaves the body: the gaskin takes
    over from the thigh over the 22 cm above it, the forearm from the
    upper arm and chest over the 18 cm above the elbow's underside, so the
    stifle and elbow bends spread over many edges instead of a few."""
    vg = mesh.vertex_groups
    names = {g.index: g.name for g in vg}
    n = 0
    for v in mesh.data.vertices:
        f, z, ax = -v.co.y, v.co.z, abs(v.co.x)
        side = "L" if v.co.x >= 0 else "R"
        if f < -0.25 and ax > 0.05 and 0.76 < z < 1.02:
            low, w = "lower_leg." + side, 1 - _ss(0.78, 1.00, z)
        elif 0.47 < f < 0.82 and ax > 0.11 and 0.70 < z < 0.94:
            low, w = "forearm." + side, 1 - _ss(0.72, 0.92, z)
        else:
            continue
        ws = {names[g.group]: g.weight for g in v.groups if names[g.group] in DEFORM}
        lower_parts = sum(x for k, x in ws.items() if not k.startswith(UPPER) and k != low)
        if lower_parts > 0.2:
            continue   # below the knee or hock: leave it
        up = {k: x for k, x in ws.items() if k.startswith(UPPER)}
        us = sum(up.values())
        if us < 1e-4:
            up = {("thigh." if f < 0 else "upper_arm.") + side: 1.0}
            us = 1.0
        for k in ws:
            vg[k].add([v.index], 0.0, "REPLACE")
        vg[low].add([v.index], w, "REPLACE")
        for k, x in up.items():
            vg[k].add([v.index], (1 - w) * x / us, "REPLACE")
        n += 1
    print("HORSE_BANDS", n)


def midline(mesh, rig):
    vg = mesh.vertex_groups
    names = {g.index: g.name for g in vg}
    trunk = [b for b in rig.data.bones if b.use_deform and b.name.startswith(TRUNK)]
    moved = 0
    for v in mesh.data.vertices:
        ws = [(names[g.group], g.weight) for g in v.groups if names[g.group] in DEFORM]
        total = sum(w for _, w in ws)
        if total <= 0:
            continue
        cut = 0.0
        for n, w in ws:
            m = limb_mask(n, v.co)
            if m < 1.0:
                vg[n].add([v.index], w * m, "REPLACE")
                cut += w * (1 - m)
        if cut < 1e-4:
            continue
        moved += 1
        tw = [(n, w) for n, w in ws if n.startswith(TRUNK)]
        ts = sum(w for _, w in tw)
        if ts > 1e-4:
            for n, w in tw:
                vg[n].add([v.index], w + cut * w / ts, "REPLACE")
        else:
            best = min(trunk, key=lambda b: seg_dist(v.co, b.head_local, b.tail_local))
            vg[best.name].add([v.index], cut, "ADD")
    print("HORSE_MIDLINE", moved)


def seg_dist(p, a, b):
    ab = b - a
    t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.dot(ab), 1e-12)))
    return (p - (a + ab * t)).length


# ------------------------------------------------------------- markers
def markers(mesh, rig):
    """For the hair-strand job: vertex groups and a crest curve on the
    mesh (in the .blend), anchor nodes on the bones (in the glb)."""
    me = mesh.data
    crest = crest_line(mesh)
    poll_f = H.POLL[0]
    g = mesh.vertex_groups.new(name="mane_line")
    for v in me.vertices:
        f = -v.co.y
        if 0.46 <= f <= poll_f - 0.02 and abs(v.co.x) < 0.028 and v.co.z > crest(f) - 0.02:
            g.add([v.index], 1.0, "REPLACE")
    fl = headp(0.05, 0.0)
    g = mesh.vertex_groups.new(name="forelock_root")
    for v in me.vertices:
        if (v.co - fl).length < 0.035:
            g.add([v.index], 1.0, "REPLACE")
    tr = P(H.DOCK[0][0], H.DOCK[0][1])
    g = mesh.vertex_groups.new(name="tail_root")
    for v in me.vertices:
        if (v.co - tr).length < 0.06:
            g.add([v.index], 1.0, "REPLACE")
    g = mesh.vertex_groups.new(name="tail_dock")
    for v in me.vertices:
        if H.on_dock(v.co) and -v.co.y < -0.74:
            g.add([v.index], 1.0, "REPLACE")
    # crest curve, poll to withers
    cu = bpy.data.curves.new("mane_line", "CURVE")
    cu.dimensions = "3D"
    sp = cu.splines.new("POLY")
    fs = [poll_f - 0.03 - i * (poll_f - 0.49) / 11 for i in range(12)]
    sp.points.add(len(fs) - 1)
    for p, f in zip(sp.points, fs):
        c = P(f, crest(f))
        p.co = (c.x, c.y, c.z, 1.0)
    co = bpy.data.objects.new("mane_line", cu)
    bpy.context.scene.collection.objects.link(co)
    co.hide_render = True
    # anchors (glb nodes): parented to their bones, at rest positions
    anchors = {"forelock_root": ("head", fl), "tail_root": ("tail.001", tr), "tail_dock": ("tail.005", P(H.DOCK[-1][0], H.DOCK[-1][1]))}
    for i, f in enumerate(MANE_F):
        anchors["mane_%02d" % (i + 1)] = ("neck.%03d" % min(4, 4 - i if i < 4 else 1), P(f, crest(f)))
    anchors["mane_05"] = ("spine.006", anchors["mane_05"][1])
    for name, (bone, pos) in anchors.items():
        e = bpy.data.objects.new("anchor:" + name, None)
        bpy.context.scene.collection.objects.link(e)
        e.empty_display_size = 0.03
        e.parent = rig
        e.parent_type = "BONE"
        e.parent_bone = bone
        b = rig.data.bones[bone]
        e.matrix_world = rig.matrix_world @ Matrix.Translation(pos)
    print("HORSE_MARKERS", json.dumps({n: len([v for v in me.vertices if any(gg.group == mesh.vertex_groups[n].index for gg in v.groups)])
                                       for n in ("mane_line", "forelock_root", "tail_root", "tail_dock")}))


# ------------------------------------------------------------- posing
class Poser:
    """Armature-space pose matrices from local rotations (FK) and from
    wanted bone directions (the solved legs); writes quaternion keys."""

    def __init__(self, rig):
        self.rig = rig
        self.bones = rig.data.bones
        self.order = []
        seen = set()

        def walk(b):
            self.order.append(b.name)
            for c in b.children:
                walk(c)
        for b in self.bones:
            if b.parent is None:
                walk(b)
        self.rest = {b.name: b.matrix_local.copy() for b in self.bones}
        self.len = {b.name: b.length for b in self.bones}

    def reset(self):
        self.basis = {n: Matrix.Identity(4) for n in self.order}
        self.pose = {}

    def local(self, n, q, loc=None):
        m = q.to_matrix().to_4x4()
        if loc is not None:
            m = Matrix.Translation(loc) @ m
        self.basis[n] = m

    def solve(self, upto=None):
        for n in self.order:
            b = self.bones[n]
            if b.parent:
                rel = self.rest[b.parent.name].inverted() @ self.rest[n]
                self.pose[n] = self.pose[b.parent.name] @ rel @ self.basis[n]
            else:
                self.pose[n] = self.rest[n] @ self.basis[n]
            if upto and n == upto:
                return

    def tail_with(self, n, basis):
        """Bone n's tail if its basis were `basis` (parents as posed)."""
        b = self.bones[n]
        par = self.pose[b.parent.name] @ (self.rest[b.parent.name].inverted() @ self.rest[n])
        return par @ basis @ Vector((0, self.len[n], 0))

    def set_local(self, n, q):
        b = self.bones[n]
        self.basis[n] = q.to_matrix().to_4x4()
        par = self.pose[b.parent.name] @ (self.rest[b.parent.name].inverted() @ self.rest[n])
        self.pose[n] = par @ self.basis[n]

    def head(self, n):
        return self.pose[n].translation.copy()

    def tail(self, n):
        return self.pose[n] @ Vector((0, self.len[n], 0))

    def aim(self, n, direction):
        """Point bone n along `direction` (armature space), no twist, its
        head where its parent puts it. Updates its pose and its children's."""
        b = self.bones[n]
        par = self.pose[b.parent.name] @ (self.rest[b.parent.name].inverted() @ self.rest[n]) if b.parent else self.rest[n]
        cur = (par.to_3x3() @ Vector((0, 1, 0))).normalized()
        q = cur.rotation_difference(direction.normalized())
        want = q.to_matrix() @ par.to_3x3()
        basis3 = par.to_3x3().inverted() @ want
        self.basis[n] = basis3.to_4x4()
        self.pose[n] = par @ self.basis[n]

    def key(self, frame):
        for n in self.order:
            pb = self.rig.pose.bones[n]
            m = self.basis[n]
            pb.rotation_quaternion = m.to_quaternion()
            pb.keyframe_insert("rotation_quaternion", frame=frame)
            if self.bones[n].parent is None:
                pb.location = m.translation
                pb.keyframe_insert("location", frame=frame)
            if n == "abdomen":
                pb.scale = self.abd_scale
                pb.keyframe_insert("scale", frame=frame)


def qx(deg):
    """A turn about the bone's local x axis (its bend axis: rolls are set so
    x is the horse's left-right for every limb, spine, neck and tail bone)."""
    return Quaternion((1, 0, 0), math.radians(deg))


def qz(deg):
    return Quaternion((0, 0, 1), math.radians(deg))


def two_bone(a, target, l1, l2, bend_hint):
    """The middle joint of a two-bone chain from a to target, bending toward
    bend_hint (a direction), in the plane of the hint."""
    d = target - a
    dist = min(d.length, l1 + l2 - 1e-5)
    dist = max(dist, abs(l1 - l2) + 1e-5)
    dn = d.normalized()
    x = (l1 * l1 - l2 * l2 + dist * dist) / (2 * dist)
    h = math.sqrt(max(l1 * l1 - x * x, 0.0))
    side = (bend_hint - dn * bend_hint.dot(dn)).normalized()
    return a + dn * x + side * h


def smoothstep(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def foot_path(ph, rest_f):
    """Coronet target (f offset from rest, lift) and hoof pitch (deg, toe
    down +) and pastern flex at cycle phase ph (0..1, 0 = touch-down)."""
    if ph < DUTY:
        s = ph / DUTY
        df = STRIDE * DUTY * (0.5 - s)        # moves back at body speed
        lift = 0.0
        # breakover in the last 12 % of stance: heel rises about the toe
        brk = smoothstep((s - 0.88) / 0.12)
        pitch = 28.0 * brk
        sink = math.sin(math.pi * s) * 7.0     # fetlock drops under load (deg)
        flex = -sink
    else:
        s = (ph - DUTY) / (1 - DUTY)
        e = smoothstep(s)
        df = STRIDE * DUTY * (-0.5 + e)
        lift = SWING_LIFT * math.sin(math.pi * min(s * 1.15, 1.0)) ** 1.2
        pitch = 28.0 + SWING_FOLD * math.sin(math.pi * min(s * 1.3, 1.0))  # the hoof folds back
        pitch *= (1 - smoothstep((s - 0.75) / 0.25))
        flex = SWING_FLEX * math.sin(math.pi * min(s * 1.25, 1.0))
        flex *= (1 - smoothstep((s - 0.7) / 0.3))
    return df, lift, pitch, flex


class Legs:
    """Each leg's lengths and rest hoof contact."""

    def __init__(self, ps):
        self.ps = ps
        self.rest_pose = {}
        ps.reset()
        ps.solve()
        self.rest_pose = {n: (ps.head(n), ps.tail(n)) for n in ps.order}

    def place(self, side, fore, df, lift, pitch, flex, plant_x=0.0):
        """Pose one leg so its hoof is at the rest contact moved by df
        (forward), lifted by `lift`, pitched by `pitch` and with its
        pastern flexed by `flex` degrees."""
        ps = self.ps
        s = side
        if fore:
            toe_n, hoof_n, can_n, arm_n, up_n = "f_toe." + s, "f_hoof." + s, "forefoot." + s, "forearm." + s, "upper_arm." + s
        else:
            toe_n, hoof_n, can_n, arm_n, up_n = "r_toe." + s, "r_hoof." + s, "hind_foot." + s, "lower_leg." + s, "thigh." + s
        rh, rt = self.rest_pose[hoof_n]
        # the hoof: rest, turned by pitch about the toe (stance) or its own
        # coronet (swing), then moved
        X = Vector((1, 0, 0))
        R = Matrix.Rotation(math.radians(pitch), 3, X)   # +pitch lifts the heel, toe down
        off = Vector((0, -df, lift))
        toe_tip = rt
        coronet = toe_tip + R @ (rh - rt) + off
        hoof_dir = R @ (rt - rh)
        # pastern: rest direction (coronet to fetlock), turned by pitch + flex
        th, tt = self.rest_pose[toe_n]
        Rp = Matrix.Rotation(math.radians(pitch * 0.55 + flex), 3, X)
        fetlock = coronet + Rp @ (th - tt)
        if fore:
            # upper arm: hold the foreleg a straight strut in stance (solve its
            # angle so the elbow is forearm + cannon from the fetlock), bend at
            # the knee in swing
            l2, l3 = ps.len[arm_n], ps.len[can_n]
            want = (l2 + l3) * (0.995 if lift <= 1e-6 else FORE_SWING_REACH - 0.05 * math.sin(math.pi * min(max(flex, 0.0) / 55.0, 1.0)))
            # the shoulder blade turns with the arm (about its top, near the
            # withers): it takes SCAPULA_SHARE of the swing
            sh_n = "shoulder." + s
            best = None
            for k in range(-80, 81):
                ang = k * 0.5
                ps.set_local(sh_n, qx(ang * SCAPULA_SHARE))
                e = ps.tail_with(up_n, qx(ang * (1 - SCAPULA_SHARE)).to_matrix().to_4x4())
                err = abs((fetlock - e).length - want) + 0.0004 * abs(ang)
                if best is None or err < best[0]:
                    best = (err, ang)
            ps.set_local(sh_n, qx(best[1] * SCAPULA_SHARE))
            ps.set_local(up_n, qx(best[1] * (1 - SCAPULA_SHARE)))
            e = ps.tail(up_n)
            knee = two_bone(e, fetlock, l2, l3, Vector((0, -1, 0)))   # the knee bends forward
            ps.aim(arm_n, knee - e)
            ps.aim(can_n, fetlock - knee)
        else:
            # femur and cannon turn by the same angle (reciprocal apparatus):
            # find the thigh angle where the tibia fits between them
            l2, l3 = ps.len[arm_n], ps.len[can_n]
            rc_h, rc_t = self.rest_pose[can_n]
            cdir = (rc_t - rc_h).normalized()
            best = None
            for k in range(-70, 71):
                ang = k * 0.5
                stifle = ps.tail_with(up_n, qx(ang).to_matrix().to_4x4())
                hock = fetlock - Matrix.Rotation(math.radians(-ang), 3, X) @ cdir * l3
                err = abs((hock - stifle).length - l2) + 0.0002 * abs(ang)
                if best is None or err < best[0]:
                    best = (err, ang)
            ps.set_local(up_n, qx(best[1]))
            stifle = ps.tail(up_n)
            hock = two_bone(stifle, fetlock, l2, l3, Vector((0, 1, 0)))   # the hock points back
            ps.aim(arm_n, hock - stifle)
            ps.aim(can_n, fetlock - hock)
        ps.aim(toe_n, coronet - ps.tail(can_n))
        ps.aim(hoof_n, hoof_dir)


def body_fk(ps, t_walk=None, t_idle=None):
    """Spine, neck, head, tail, ears and belly: local turns."""
    ps.abd_scale = Vector((1, 1, 1))
    if t_walk is not None:
        ph = 2 * math.pi * t_walk
        bob = -0.018 * math.cos(2 * ph)       # lowest as each hind hoof lands
        sway = 0.012 * math.sin(ph)
        ps.local("spine.001", qz(1.5 * math.sin(ph)) @ Quaternion((0, 1, 0), math.radians(2.0 * math.sin(ph))),
                 loc=Vector((sway, 0.0, bob)))
        ps.local("spine.003", Quaternion((0, 1, 0), math.radians(-1.2 * math.sin(ph))))
        ps.local("spine.005", Quaternion((0, 1, 0), math.radians(-1.0 * math.sin(ph))))
        nod = math.sin(2 * ph + 0.9)
        ps.local("neck.001", qx(-3.5 * nod))
        ps.local("neck.002", qx(-1.5 * nod) @ qz(1.2 * math.sin(ph)))
        ps.local("neck.004", qx(2.5 * nod))
        ps.local("head", qx(1.5 * nod))
        ps.local("tail.001", qx(6.0) @ qz(5.0 * math.sin(ph - 0.5)))
        for i in range(2, 6):
            ps.local("tail.%03d" % i, qz(4.0 * math.sin(ph - 0.5 - 0.5 * i)))
        for s, sg in (("L", 1), ("R", -1)):
            ps.local("ear." + s, qx(-6.0 + 3.0 * math.sin(2 * ph + sg)) @ qz(sg * 3.0))
    else:
        ph = 2 * math.pi * t_idle
        shift = 0.022 * math.sin(ph)
        ps.local("spine.001", Quaternion((0, 1, 0), math.radians(1.6 * math.sin(ph))), loc=Vector((shift, 0.0, -0.004 * (1 - math.cos(2 * ph)))))
        breath = 0.5 - 0.5 * math.cos(ph)       # one breath in 4 s
        ps.abd_scale = Vector((1 + 0.035 * breath, 1.0, 1 + 0.03 * breath))
        ps.local("spine.004", qx(-0.6 * breath))
        ps.local("neck.001", qx(-2.5 * math.sin(ph + 0.7)) @ qz(-2.0 * math.sin(ph)))
        ps.local("neck.003", qz(-1.5 * math.sin(ph)))
        ps.local("head", qx(2.0 * math.sin(ph + 1.4)) @ qz(2.5 * math.sin(ph + 0.3)))
        fl = idle_bump(t_idle, 0.30, 0.10)
        fr = idle_bump(t_idle, 0.68, 0.12)
        ps.local("ear.L", qx(-4.0 - 38.0 * fl) @ qz(10.0 * fl))
        ps.local("ear.R", qx(-4.0 - 30.0 * fr) @ qz(-14.0 * fr))
        sw = idle_bump(t_idle, 0.52, 0.30)
        ps.local("tail.001", qx(5.0) @ qz(14.0 * sw * math.sin(4 * math.pi * (t_idle - 0.37))))
        for i in range(2, 6):
            ps.local("tail.%03d" % i, qz(9.0 * sw * math.sin(4 * math.pi * (t_idle - 0.37) - 0.6 * i)))


def idle_bump(t, at, width):
    """0 to 1 and back over `width` of the cycle, centred at `at`."""
    x = (t - at) / (width / 2)
    return 0.0 if abs(x) >= 1 else 0.5 + 0.5 * math.cos(math.pi * x)


FORE_CHAIN = ("shoulder", "upper_arm", "forearm", "forefoot", "f_toe", "f_hoof")
# The foreleg's swing fold at mid-swing (deg, about each bone's bend axis):
# the forearm reaches forward, the knee folds, the pastern and hoof tuck.
FORE_FOLD = {"forearm": 38.0, "forefoot": -68.0, "f_toe": -22.0, "f_hoof": -12.0}


def fore_swing(ps, legs, side, s):
    """The foreleg in swing, by turns rather than a hoof target (a target
    path made the solver snap the forearm through 90 degrees in two frames):
    a smooth blend from the pose at lift-off to the pose at touch-down, both
    solved with this frame's body, plus a fold that peaks mid-swing."""
    names = [n + "." + side for n in FORE_CHAIN]
    legs.place(side, True, *foot_path(DUTY - 1e-4, 0.0))
    q0 = [ps.basis[n].to_quaternion() for n in names]
    legs.place(side, True, *foot_path(0.0, 0.0))
    q1 = [ps.basis[n].to_quaternion() for n in names]
    e = smoothstep(s)
    bump = math.sin(math.pi * s)
    for n, a, b in zip(names, q0, q1):
        q = a.slerp(b, e)
        fold = FORE_FOLD.get(n.split(".")[0], 0.0)
        if fold:
            q = q @ qx(fold * bump)
        ps.set_local(n, q)


# v3: the pectorals tie the arm to the breastbone, so the breast skin goes
# part of the way with the arm: each breast bone turns by this share of its
# upper arm's swing (measured against the trunk), which spreads the stretch
# between the inside of the arm and the chest floor.
BREAST_FOLLOW = float(os.environ.get("HORSE_BREAST", "0.6"))


def breast_follow(ps, legs):
    for s in ("L", "R"):
        up, br = "upper_arm." + s, "breast." + s
        par = ps.bones[br].parent.name
        P = ps.pose[par].to_3x3() @ ps.rest[par].to_3x3().inverted()   # the trunk's turn since rest
        rh, rt = legs.rest_pose[up]
        d_rel = P.inverted() @ (ps.tail(up) - ps.head(up))
        R = (rt - rh).rotation_difference(d_rel)
        Rk = Quaternion().slerp(R, BREAST_FOLLOW)
        bh, bt = legs.rest_pose[br]
        ps.aim(br, P @ (Rk @ (bt - bh)))


def make_clips(rig):
    ps = Poser(rig)
    legs = Legs(ps)
    rig.animation_data_create()
    order = [("L", False, 0.0), ("L", True, 0.25), ("R", False, 0.5), ("R", True, 0.75)]
    clips = {}
    for name, frames in (("walk", WALK_FRAMES), ("idle", IDLE_FRAMES)):
        act = bpy.data.actions.new(name)
        act.use_fake_user = True
        rig.animation_data.action = act
        if hasattr(rig.animation_data, "action_slot") and act.slots:
            rig.animation_data.action_slot = act.slots[0]
        for fr in range(frames + 1):
            t = fr / frames
            ps.reset()
            if name == "walk":
                body_fk(ps, t_walk=t)
            else:
                body_fk(ps, t_idle=t)
            ps.solve()
            for side, fore, off in order:
                if name == "walk":
                    ph = (t - off) % 1.0
                    if fore and ph >= DUTY:
                        fore_swing(ps, legs, side, (ph - DUTY) / (1 - DUTY))
                        continue
                    df, lift, pitch, flex = foot_path(ph, 0.0)
                    # the hoof's rest contact is the middle of its stance
                else:
                    df, lift, pitch, flex = 0.0, 0.0, 0.0, 0.0
                legs.place(side, fore, df, lift, pitch, flex)
            breast_follow(ps, legs)
            for hn, dn in HELPERS.items():
                q = ps.basis[dn].to_quaternion()
                ps.basis[hn] = Quaternion().slerp(q, 0.5).to_matrix().to_4x4()
            ps.key(fr)
        for fc in _fcurves(act):
            for k in fc.keyframe_points:
                k.interpolation = "LINEAR"
        clips[name] = act
        rig.animation_data.action = None
    return clips


def _fcurves(a):
    out = list(getattr(a, "fcurves", None) or [])
    for layer in getattr(a, "layers", []):
        for strip in layer.strips:
            for bag in getattr(strip, "channelbags", []):
                out.extend(bag.fcurves)
    return out


# ------------------------------------------------------------- export
def export(mesh, rig, out):
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    keep = [mesh, rig] + [o for o in bpy.data.objects if o.name.startswith("anchor:")]
    for o in keep:
        o.select_set(True)
    bpy.context.view_layer.objects.active = rig
    path = os.path.join(out, "horse.glb")
    want = dict(filepath=path, export_format="GLB", use_selection=True, export_yup=True,
                export_apply=False, export_skins=True, export_animations=True,
                export_animation_mode="ACTIONS", export_force_sampling=True,
                export_frame_range=False, export_anim_single_armature=True,
                export_def_bones=True, export_tangents=False, export_materials="EXPORT",
                export_extras=False, export_cameras=False, export_lights=False,
                export_attributes=True, export_vertex_color="NONE")
    have = set(bpy.ops.export_scene.gltf.get_rna_type().properties.keys())
    dropped = [k for k in want if k not in have]
    if dropped:
        print("HORSE_NOTE exporter options not in this Blender:", dropped)
    bpy.ops.export_scene.gltf(**{k: v for k, v in want.items() if k in have})
    print("HORSE_EXPORT", path, os.path.getsize(path))


# v5: baked ambient occlusion, one value a vertex (the colour attribute
# `_EVANI_AO`, point domain, exported with the glTF; the converter carries it
# into every tier file and evani-render's `baked-ao` closes the light that is
# not the sun with it: sky and ground light, reflections, the look's fill,
# kick and rim). Baked in the rest pose with Cycles from the horse alone (no
# ground plane: tried, it darkened the hooves to 0.58 and the cannons to 0.65,
# muddy legs, and the look's ground light already stands for the ground).
# Why: the far hind thigh's inside is in shadow while the horse walks but took
# the full open-sky, rim and reflection light, a pale grey-lavender patch under
# the belly (DECISIONS, test horse v4 and v5).
AO_DISTANCE = 0.8       # metres a ray looks for the body: across the gap between the hind legs and up to
                        # the belly (0.45 gave nearly the same values: most rays escape)
AO_SAMPLES = 256        # rays a vertex (Cycles bake samples)
AO_FLOOR = 0.12         # never darker than this (a deep crease still takes a little sky)
AO_ATTR = "_EVANI_AO"


def bake_ao(mesh, rig):
    sc = bpy.context.scene
    old = (sc.render.engine, rig.data.pose_position)
    rig.data.pose_position = "REST"
    if mesh.data.color_attributes.get(AO_ATTR):
        mesh.data.color_attributes.remove(mesh.data.color_attributes[AO_ATTR])
    ca = mesh.data.color_attributes.new(AO_ATTR, "BYTE_COLOR", "POINT")
    mesh.data.color_attributes.active_color = ca
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = AO_SAMPLES
    w = sc.world or bpy.data.worlds.new("World")
    sc.world = w
    w.light_settings.distance = AO_DISTANCE
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    mesh.hide_render = False
    mesh.select_set(True)
    bpy.context.view_layer.objects.active = mesh
    bpy.context.view_layer.update()
    bpy.ops.object.bake(type="AO", target="VERTEX_COLORS")
    # the floor, and a summary for the log
    vals = []
    for d in ca.data:
        a = max(AO_FLOOR, d.color[0])
        d.color = (a, a, a, 1.0)
        vals.append(a)
    vals.sort()
    q = lambda f: round(vals[min(len(vals) - 1, int(f * len(vals)))], 3)
    print("HORSE_AO", json.dumps({"vertices": len(vals), "min": q(0.0), "p05": q(0.05), "p25": q(0.25), "median": q(0.5), "p90": q(0.9)}))
    sc.render.engine, rig.data.pose_position = old
    return ca


def run(mesh, args, out):
    rig = make_metarig()
    L = landmarks(mesh)
    fit(rig, L)
    skin(mesh, rig)
    return rig


def finish(mesh, rig, args, out):
    markers(mesh, rig)
    make_clips(rig)
    rig.animation_data.action = None
    bake_ao(mesh, rig)
    export(mesh, rig, out)
