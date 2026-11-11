# Rig, weights, clips, occlusion and export for the quadruped generator
# (called by build.py; see the notes at the top of that file).
#
# The skeleton is Blender's own Rigify horse metarig (bone names and
# hierarchy unchanged) for every species, fitted to the species' joints
# (body.landmarks), used directly as the deform skeleton, plus four half-turn
# helper bones per side at the hips, shoulders, stifles and elbows (as the
# test horse, tools/models/horse/rig_horse.py). The mane bones are dropped
# for species without a mane.
#
# Clips (30 fps, both loop), solved, not keyed by hand:
#   walk  4-beat walk (left hind, left fore, right hind, right fore, a quarter
#         cycle apart) in place; feet planted in stance (each leg's cannon
#         swings with the line from its top joint to the foot, the two upper
#         bones solved to meet it), lifted and folded in swing.
#   idle  weight shift, one breath (belly), ear flicks, a slow look, a tail
#         swish or wag; feet planted.
import bpy, math, json, os
from mathutils import Vector, Matrix, Quaternion
B = None   # set by build.py: body.Body
S = None   # the species
ID = None

FPS = 30


def P(f, z, x=0.0):
    return Vector((x, -f, z))


def activate(ob):
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)


def ss(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


DEFORM = {"spine.001", "spine.002", "spine.003", "spine.004", "spine.005", "spine.006", "abdomen", "chest",
          "neck.001", "neck.002", "neck.003", "neck.004", "head", "jaw", "jaw.001",
          "tail.001", "tail.002", "tail.003", "tail.004", "tail.005"}
for _s in ("L", "R"):
    DEFORM |= {n + "." + _s for n in ("thigh", "lower_leg", "hind_foot", "r_toe", "r_hoof", "shoulder", "upper_arm",
                                       "forearm", "forefoot", "f_toe", "f_hoof", "breast", "ear")}
    DEFORM.add("ear.%s.001" % _s)
HELPERS = {}
for _s in ("L", "R"):
    HELPERS["thigh_half." + _s] = "thigh." + _s
    HELPERS["upper_arm_half." + _s] = "upper_arm." + _s
    HELPERS["stifle." + _s] = "lower_leg." + _s
    HELPERS["elbow." + _s] = "forearm." + _s
DEFORM |= set(HELPERS)
FORE = ("shoulder", "upper_arm", "forearm", "forefoot", "f_toe", "f_hoof")
HIND = ("thigh", "lower_leg", "hind_foot", "r_toe", "r_hoof")


def make_metarig():
    try:
        bpy.ops.preferences.addon_enable(module="rigify")
    except Exception as e:  # noqa
        print("QUAD_NOTE rigify enable:", e)
    import importlib
    horse = importlib.import_module("rigify.metarigs.Animals.horse")
    arm = bpy.data.armatures.new(ID + "_rig")
    ob = bpy.data.objects.new(ID + "_rig", arm)
    bpy.context.scene.collection.objects.link(ob)
    activate(ob)
    horse.create(ob)
    return ob


def fit(rig, L):
    activate(rig)
    bpy.ops.object.mode_set(mode="EDIT")
    eb = rig.data.edit_bones
    if not S.get("mane"):
        for b in [b for b in eb if b.name.startswith(("mane_base", "mane_top"))]:
            eb.remove(b)
    for b in eb:
        b.use_connect = False
    for n, (h, t) in L.items():
        if n in eb:
            eb[n].head, eb[n].tail = h, t
    for b in eb:
        d = (b.tail - b.head).normalized()
        b.align_roll(Vector((0, -1, 0)) if abs(d.z) > 0.7 else Vector((0, 0, 1)))
    for hn, dn in HELPERS.items():
        d = eb[dn]
        h = eb.new(hn)
        h.head = d.head.copy()
        h.tail = d.head + (d.tail - d.head).normalized() * (d.length * 0.4)
        h.roll = d.roll
        h.parent = d.parent
    for b in eb:
        if b.parent and (b.head - b.parent.tail).length < 1e-5 and b.name not in HELPERS:
            b.use_connect = True
        b.use_deform = b.name in DEFORM
    bpy.ops.object.mode_set(mode="OBJECT")
    rig.data.pose_position = "POSE"
    for pb in rig.pose.bones:
        pb.rotation_mode = "QUATERNION"


def seg_dist(p, a, b):
    ab = b - a
    t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.dot(ab), 1e-12)))
    return (p - (a + ab * t)).length


# ------------------------------------------------------------- weights
def limb_mask(name, co):
    """How much a limb bone may hold a vertex: none on the midline of the
    belly, chest and groin (a left and a right leg would pull one edge two
    ways), fading in toward the leg."""
    base = name.split(".")[0]
    ax = abs(co.x)
    if base in FORE[1:] + HIND:
        leg = "fore" if base in FORE else "hind"
        xl = B.leg_x(leg, max(co.z, 0.0))
        return ss(0.25 * xl, 0.65 * xl, ax)
    if base == "shoulder":
        return ss(0.15 * B.leg_x("fore", co.z), 0.5 * B.leg_x("fore", co.z), ax)
    return 1.0


TRUNK = ("spine.", "abdomen", "chest", "breast.")


def midline(mesh, rig):
    vg = mesh.vertex_groups
    names = {g.index: g.name for g in vg}
    trunk = [b for b in rig.data.bones if b.use_deform and b.name.startswith(TRUNK)]
    moved = 0
    for v in mesh.data.vertices:
        ws = [(names[g.group], g.weight) for g in v.groups if names[g.group] in DEFORM]
        if sum(w for _, w in ws) <= 0:
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
    print("QUAD_MIDLINE", moved)


def harmonic(ev, n, x0, free, iters=1500):
    """x0 (n,) values; the free vertices relax to the mean of their
    neighbours (Jacobi), the rest are held (as the test horse)."""
    import numpy as np
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
    import numpy as np
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def _share(W, cols, zone, ev, n):
    """The share of W among cols, normalised; where these bones hold less
    than 0.4 of a zone vertex, filled from its neighbours."""
    import numpy as np
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


def limb_share(mesh, rig, upto):
    """Each leg's share of the skin round the armpit and the groin, spread
    evenly (a harmonic blend) between plain anchors, from the species' own
    joints: the leg below the elbow or stifle and the outside of the upper
    leg are the leg's, the midline strip between the legs is the trunk's,
    and the skin between relaxes to an even blend, so the swing of the
    upper leg is shared over the widest band of skin. The test horse's
    limb_share does the same with its own measured anchors; this one scales
    with any species. Body skin only (vertices below `upto`)."""
    import numpy as np
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
    ev = ev[(ev[:, 0] < upto) & (ev[:, 1] < upto)]
    body = np.arange(n) < upto
    names = [g.name for g in vg]
    col = {nm: i for i, nm in enumerate(names)}
    W = np.zeros((n, len(names)))
    for v in me.vertices[:upto]:
        for g in v.groups:
            if names[g.group] in DEFORM:
                W[v.index, g.group] = g.weight
    rows = W.sum(axis=1)
    W = np.where(rows[:, None] > 1e-9, W / np.maximum(rows[:, None], 1e-9), W)
    bones = rig.data.bones
    touched = np.zeros(n, dtype=bool)
    report = {}
    for side, sg in (("L", 1.0), ("R", -1.0)):
        xs = X * sg
        on = body & (xs > -0.005)
        for limb, chain, top, mid in (("hind", HIND, "thigh", "lower_leg"), ("fore", FORE[1:], "upper_arm", "forearm")):
            lc = [col[c + "." + side] for c in chain if c + "." + side in col]
            j0 = bones[top + "." + side].head_local       # hip or shoulder joint
            j1 = bones[mid + "." + side].head_local       # stifle or elbow
            j2 = bones[mid + "." + side].tail_local       # hock or knee
            L1 = (j1 - j0).length
            a = np.array([-j0.y, j0.z])
            b = np.array([-j1.y, j1.z])
            ab = b - a
            t = np.clip(((F - a[0]) * ab[0] + (Z - a[1]) * ab[1]) / ab.dot(ab), 0.0, 1.0)
            dseg = np.hypot(F - (a[0] + ab[0] * t), Z - (a[1] + ab[1] * t))
            legx = np.array([B.leg_x(limb, max(float(z), 0.0)) for z in Z])
            zone = on & (dseg < 0.9 * L1) & (Z > j2.z)
            # the leg proper: below the elbow or stifle and inside the leg's
            # own skin outline (a deep belly or brisket hangs as low but is
            # the trunk's)
            inleg = np.array([B.leg_inside(limb, float(f), float(z), float(x)) < 1.35 if zone[i] else False
                              for i, (f, z, x) in enumerate(zip(F, Z, xs))])
            below = (Z < j1.z - 0.3 * (j1.z - j2.z)) & inleg
            one = on & (below | ((xs > legx) & (dseg < 0.35 * L1) & (t > 0.15)))
            zero = on & (xs < 0.25 * abs(j1.x))
            one &= ~zero
            free = zone & ~one & ~zero
            T0 = W[:, lc].sum(axis=1)
            T0 = np.where(one, 1.0, np.where(zero, 0.0, T0))
            T = harmonic(ev, n, T0, free, 2500)
            # round off the edges of the fixed patches: a few rounds over the
            # zone with only the leg below the joint held
            T = harmonic(ev, n, T, zone & ~(below & one) & ~zero, 10)
            T = np.where(on, np.clip(T, 0.0, 1.0), 0.0)
            T[T < 0.002] = 0.0
            sel = zone & ((T > 0) | (W[:, lc].sum(axis=1) > 1e-6))
            if not sel.any():
                continue
            other = [i for i, nm in enumerate(names) if nm in DEFORM and i not in lc and nm not in HELPERS
                     and not nm.endswith("." + ("R" if side == "L" else "L")) and W[sel, i].max() > 0.005]
            if not other:
                other = [col[nm] for nm in TRUNK_BONES if nm in col]
            Dt = _share(W, other, sel & (T < 0.999), ev, n)
            Dl = _share(W, lc, sel & (T > 0), ev, n)
            idx = np.nonzero(sel)[0]
            newW = W.copy()
            newW[idx] = 0.0
            newW[np.ix_(idx, other)] = (1 - T[idx, None]) * Dt[np.ix_(idx, range(len(other)))]
            newW[np.ix_(idx, lc)] = T[idx, None] * Dl[np.ix_(idx, range(len(lc)))]
            tie = S.get("rig", {}).get("breast_tie", 0.0)
            if limb == "fore" and tie > 0 and "breast." + side in col:
                # the skin of the chest floor round the elbow is tied to the
                # arm by the pectorals: its trunk share goes to the breast
                # bone (which follows the arm), most at the elbow, none
                # behind the girth or above the breast bone's root
                bh = bones["breast." + side].head_local
                q = (1 - _ss_np(0.0, 0.6 * L1, (-j1.y) - F)) * (1 - _ss_np(bh.z - 0.2 * L1, bh.z + 0.2 * L1, Z))
                q = tie * q * (xs > 0)
                bc = col["breast." + side]
                for nm in ("chest", "abdomen", "spine.004", "spine.005", "spine.006"):
                    if nm in col:
                        c = col[nm]
                        mv = newW[idx, c] * q[idx]
                        newW[idx, c] -= mv
                        newW[idx, bc] += mv
            W = newW
            touched |= sel
            report[limb + "." + side] = {"zone": int(zone.sum()), "blend": int(((T > 0.02) & (T < 0.98) & zone).sum())}
    idx = np.nonzero(touched)[0]
    for i in idx:
        w = W[i]
        order = np.argsort(-w)
        ww = np.zeros_like(w)
        ww[order[:4]] = w[order[:4]]
        ww[ww < 0.005] = 0.0
        s = ww.sum()
        W[i] = ww / s if s > 0 else w
    for gi, nm in enumerate(names):
        if nm in DEFORM:
            vg[gi].remove([int(i) for i in idx])
    for i in idx:
        for gi in np.nonzero(W[i] > 0)[0]:
            vg[int(gi)].add([int(i)], float(W[i, gi]), "REPLACE")
    print("QUAD_LIMB_SHARE", json.dumps(report))


TRUNK_BONES = ("spine.001", "spine.002", "spine.003", "spine.004", "spine.005", "spine.006", "abdomen", "chest")


def joint_blend(mesh):
    """Across each helper's joint, a vertex held by the parent side (p) and
    the child bone (c) is re-blended as a quadratic Bezier of the three
    transforms (s = c / (p + c): the parent side keeps (1 - s)^2, the helper
    2 s (1 - s), the child s^2), as the test horse."""
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
    n = 0
    for hn, cn, par in order:
        for v in mesh.data.vertices:
            ws = {names[g.group]: g.weight for g in v.groups if names[g.group] in DEFORM}
            c = ws.get(cn, 0.0)
            if c <= 1e-4:
                continue
            if par is None:
                side = cn[-1]
                pk = [k for k in ws if (k != cn and not k.endswith("." + side)) or k.startswith(("breast.", "shoulder."))]
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
    print("QUAD_JOINT_BLEND", n)


def smooth_weights(mesh, repeat, factor=0.5):
    activate(mesh)
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_smooth(group_select_mode="BONE_DEFORM", factor=factor, repeat=repeat, expand=0.0)
    bpy.ops.object.mode_set(mode="OBJECT")


def rigid(mesh, vids, bone):
    vg = mesh.vertex_groups
    me = mesh.data
    for vi in vids:
        for g in list(me.vertices[vi].groups):
            vg[g.group].remove([vi])
        vg[bone].add([vi], 1.0, "REPLACE")


def body_heat(mesh, rig, eye_from):
    """Bone-heat weights for the body skin alone (vertices below eye_from).
    The separate solids (eyes, ears, horns, hooves) cut into the skin, and heat
    run over the whole mesh fails there: whole patches came back unweighted
    and were then snapped to one bone each. The skin is weighted on a copy
    without them; the copy's vertices keep the body's indices, as the solids
    are appended after it."""
    import bmesh
    cp = mesh.copy()
    cp.data = mesh.data.copy()
    for g in list(cp.vertex_groups):
        cp.vertex_groups.remove(g)
    cp.modifiers.clear()
    bpy.context.scene.collection.objects.link(cp)
    bm = bmesh.new()
    bm.from_mesh(cp.data)
    bm.verts.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if v.index >= eye_from], context="VERTS")
    bm.to_mesh(cp.data)
    bm.free()
    activate(cp)
    rig.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    names = {g.index: g.name for g in cp.vertex_groups}
    out = [{names[g.group]: g.weight for g in v.groups if g.weight > 1e-4} for v in cp.data.vertices]
    data = cp.data
    bpy.data.objects.remove(cp, do_unlink=True)
    bpy.data.meshes.remove(data)
    return out


def fill_gaps(mesh, upto):
    """Body vertices heat left unweighted take the average of their weighted
    neighbours, front by front across the skin (never a snap to one bone)."""
    me = mesh.data
    vg = mesh.vertex_groups
    names = {g.index: g.name for g in vg}
    adj = [[] for _ in range(upto)]
    for e in me.edges:
        a, b = e.vertices
        if a < upto and b < upto:
            adj[a].append(b)
            adj[b].append(a)
    W = [{names[g.group]: g.weight for g in me.vertices[i].groups if names[g.group] in DEFORM and g.weight > 1e-4}
         for i in range(upto)]
    todo = {i for i in range(upto) if not W[i]}
    filled = 0
    while todo:
        front = {}
        for i in todo:
            nb = [W[j] for j in adj[i] if W[j]]
            if not nb:
                continue
            acc = {}
            for w in nb:
                s = sum(w.values())
                for k, x in w.items():
                    acc[k] = acc.get(k, 0.0) + x / s / len(nb)
            front[i] = acc
        if not front:
            break
        for i, acc in front.items():
            W[i] = acc
            for k, x in acc.items():
                vg[k].add([i], x, "REPLACE")
        todo -= set(front)
        filled += len(front)
    print("QUAD_FILL", json.dumps({"filled": filled, "left": len(todo)}))


def skin(mesh, rig, reg, eye_from):
    for hn in HELPERS:
        rig.data.bones[hn].use_deform = False   # weighted by joint_blend, not by heat
    heat = body_heat(mesh, rig, eye_from)
    for hn in HELPERS:
        rig.data.bones[hn].use_deform = True
    activate(mesh)
    rig.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.parent_set(type="ARMATURE_NAME")
    mesh.parent = rig
    vg = mesh.vertex_groups
    me = mesh.data
    for n in DEFORM:
        if n not in vg:
            vg.new(name=n)
    for i, ws in enumerate(heat):
        for n, w in ws.items():
            # the ears are separate solids: their bones never hold the skin
            if n.startswith("ear."):
                n = "head"
            if n in DEFORM:
                vg[n].add([i], w, "ADD")
    fill_gaps(mesh, eye_from)
    # a left limb never pulls the right side (and back)
    for v in me.vertices:
        for g in list(v.groups):
            n = vg[g.group].name
            if (n.endswith(".L") and v.co.x < -0.02) or (n.endswith(".R") and v.co.x > 0.02):
                if not n.startswith(("breast", "pelvis")):
                    vg[g.group].remove([v.index])
    smooth_weights(mesh, 10)
    midline(mesh, rig)
    smooth_weights(mesh, 4, 0.4)
    midline(mesh, rig)
    limb_share(mesh, rig, eye_from)
    joint_blend(mesh)
    # the separate solids (from eye_from on): hooves and dew claws rigid on
    # their leg's hoof bone, horns and eyes on the head, ears on their bones
    feet = {}
    from geom import poly_dist
    ear_lines = {s: [B.ear_centre(sx, i / 16) for i in range(17)] for sx, s in ((1, "L"), (-1, "R"))}
    ear_len = sum((ear_lines["L"][i + 1] - ear_lines["L"][i]).length for i in range(16))
    from mathutils.bvhtree import BVHTree
    body_polys = [list(p.vertices) for p in me.polygons if max(p.vertices) < eye_from]
    skin_tree = BVHTree.FromPolygons([v.co.copy() for v in me.vertices[:eye_from]], body_polys)
    band = 0.10 * ear_len
    tail_chain = [(rig.data.bones["tail.%03d" % k].head_local.copy(), rig.data.bones["tail.%03d" % k].tail_local.copy())
                  for k in range(1, 6) if "tail.%03d" % k in rig.data.bones]
    tail_band = 0.5 * S["tail"]["line"][0][2]
    for v in me.vertices:
        r = reg[v.index]
        if v.index >= eye_from and r == "ear":
            # an ear: the part buried in the head stays on the head, fading
            # to the base bone just outside the skin, the tip bone taking
            # over up the ear
            s = "L" if v.co.x >= 0 else "R"
            t = poly_dist(v.co, ear_lines[s])[1]
            w1 = ss(0.30, 0.75, t)
            hit = skin_tree.find_nearest(v.co)
            sd = (v.co - hit[0]).dot(hit[1]) if hit[0] is not None else band
            wh = 1.0 - ss(-0.5 * band, 2.0 * band, sd)
            for g in list(v.groups):
                vg[g.group].remove([v.index])
            if wh > 0.0:
                vg["head"].add([v.index], wh, "REPLACE")
            if (1.0 - w1) * (1.0 - wh) > 0.0:
                vg["ear." + s].add([v.index], (1.0 - w1) * (1.0 - wh), "REPLACE")
            if w1 * (1.0 - wh) > 0.0:
                vg["ear.%s.001" % s].add([v.index], w1 * (1.0 - wh), "REPLACE")
        elif v.index >= eye_from and r == "tail":
            # a separate tail: hat blends between the tail bones' middles
            # along the chain, the part buried in the rump on spine.001
            u = None
            best = 1e9
            for k, (h, t) in enumerate(tail_chain):
                ab = t - h
                tt = max(0.0, min(1.0, (v.co - h).dot(ab) / max(ab.dot(ab), 1e-12)))
                d = (v.co - (h + ab * tt)).length
                if d < best:
                    best, u = d, k + tt
            for g in list(v.groups):
                vg[g.group].remove([v.index])
            hit = skin_tree.find_nearest(v.co)
            sd = (v.co - hit[0]).dot(hit[1]) if hit[0] is not None else 1.0
            wr = 1.0 - ss(-tail_band, tail_band, sd)
            nb = len(tail_chain)
            ws = {}
            for k in range(nb):
                w = max(0.0, 1.0 - abs(u - (k + 0.5)))
                if (k == 0 and u < 0.5) or (k == nb - 1 and u > nb - 0.5):
                    w = 1.0
                if w > 0.0:
                    ws["tail.%03d" % (k + 1)] = w
            s = sum(ws.values())
            if wr > 0.0:
                vg["spine.001"].add([v.index], wr, "REPLACE")
            for nm, w in ws.items():
                if w * (1 - wr) > 0.0:
                    vg[nm].add([v.index], w / s * (1 - wr), "REPLACE")
        elif v.index >= eye_from and r not in ("hoof",):
            feet.setdefault("head", []).append(v.index)
        elif r == "hoof":
            side = "L" if v.co.x >= 0 else "R"
            feet.setdefault(("f_hoof." if -v.co.y > 0 else "r_hoof.") + side, []).append(v.index)
    for bone, vids in feet.items():
        rigid(mesh, vids, bone)
    activate(mesh)
    bpy.ops.object.mode_set(mode="WEIGHT_PAINT")
    bpy.ops.object.vertex_group_clean(group_select_mode="BONE_DEFORM", limit=0.01)
    bpy.ops.object.vertex_group_limit_total(group_select_mode="BONE_DEFORM", limit=4)
    bpy.ops.object.vertex_group_normalize_all(group_select_mode="BONE_DEFORM", lock_active=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    deform = [b for b in rig.data.bones if b.use_deform]
    lost = 0
    for v in me.vertices:
        if sum(g.weight for g in v.groups if vg[g.group].name in DEFORM) < 1e-4:
            best = min(deform, key=lambda b: seg_dist(v.co, b.head_local, b.tail_local))
            vg[best.name].add([v.index], 1.0, "REPLACE")
            lost += 1
    # drop vertex groups that are not deform bones (heat can make none, but
    # the smoothing group from the build must not reach the glb)
    for g in list(vg):
        if g.name not in DEFORM:
            vg.remove(g)
    print("QUAD_WEIGHTS", json.dumps({"groups": len(vg), "unweighted_fixed": lost}))


# ------------------------------------------------------------- posing
class Poser:
    """Armature-space pose matrices from local rotations (FK) and wanted bone
    directions (the solved legs); writes quaternion keys."""

    def __init__(self, rig):
        self.rig = rig
        self.bones = rig.data.bones
        self.order = []

        def walk(b):
            self.order.append(b.name)
            for c in b.children:
                walk(c)
        for b in self.bones:
            if b.parent is None:
                walk(b)
        self.rest = {b.name: b.matrix_local.copy() for b in self.bones}
        self.len = {b.name: b.length for b in self.bones}
        self.abd_scale = Vector((1, 1, 1))

    def reset(self):
        self.basis = {n: Matrix.Identity(4) for n in self.order}
        self.pose = {}
        self.abd_scale = Vector((1, 1, 1))

    def local(self, n, q, loc=None):
        if n not in self.basis:
            return
        m = q.to_matrix().to_4x4()
        if loc is not None:
            m = Matrix.Translation(loc) @ m
        self.basis[n] = m

    def solve(self):
        for n in self.order:
            b = self.bones[n]
            if b.parent:
                rel = self.rest[b.parent.name].inverted() @ self.rest[n]
                self.pose[n] = self.pose[b.parent.name] @ rel @ self.basis[n]
            else:
                self.pose[n] = self.rest[n] @ self.basis[n]

    def _par(self, n):
        b = self.bones[n]
        return self.pose[b.parent.name] @ (self.rest[b.parent.name].inverted() @ self.rest[n]) if b.parent else self.rest[n]

    def set_local(self, n, q):
        self.basis[n] = q.to_matrix().to_4x4()
        self.pose[n] = self._par(n) @ self.basis[n]
        self._children(n)

    def _children(self, n):
        for c in self.bones[n].children:
            self.pose[c.name] = self._par(c.name) @ self.basis[c.name]
            self._children(c.name)

    def head(self, n):
        return self.pose[n].translation.copy()

    def tail(self, n):
        return self.pose[n] @ Vector((0, self.len[n], 0))

    def aim(self, n, direction):
        """Point bone n along `direction` (armature space), no twist, its head
        where its parent puts it; its children follow."""
        par = self._par(n)
        cur = (par.to_3x3() @ Vector((0, 1, 0))).normalized()
        q = cur.rotation_difference(direction.normalized())
        want = q.to_matrix() @ par.to_3x3()
        self.basis[n] = (par.to_3x3().inverted() @ want).to_4x4()
        self.pose[n] = par @ self.basis[n]
        self._children(n)

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
    x is the animal's left-right for every limb, spine, neck and tail bone)."""
    return Quaternion((1, 0, 0), math.radians(deg))


def qz(deg):
    return Quaternion((0, 0, 1), math.radians(deg))


def two_bone(a, target, l1, l2, bend_hint):
    d = target - a
    dist = min(d.length, l1 + l2 - 1e-5)
    dist = max(dist, abs(l1 - l2) + 1e-5)
    dn = d.normalized()
    x = (l1 * l1 - l2 * l2 + dist * dist) / (2 * dist)
    h = math.sqrt(max(l1 * l1 - x * x, 0.0))
    side = (bend_hint - dn * bend_hint.dot(dn))
    side = side.normalized() if side.length > 1e-6 else Vector((0, 1, 0))
    return a + dn * x + side * h


def sstep(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def foot_path(ph):
    """Foot target at cycle phase ph (0..1, 0 = touch-down): forward offset
    from the rest contact, lift, toe-down pitch (deg) and pastern flex (deg),
    and the swing fraction (0 in stance)."""
    W = S["walk"]
    duty, stride, lift_m = W["duty"], W["stride"], W["lift"]
    if ph < duty:
        s = ph / duty
        df = stride * duty * (0.5 - s)
        brk = sstep((s - 0.86) / 0.14)
        return df, 0.0, W.get("breakover", 26.0) * brk, -math.sin(math.pi * s) * W.get("sink", 6.0), 0.0
    s = (ph - duty) / (1 - duty)
    e = sstep(s)
    df = stride * duty * (-0.5 + e)
    lift = lift_m * math.sin(math.pi * min(s * 1.15, 1.0)) ** 1.2
    pitch = (W.get("breakover", 26.0) + W.get("swing_fold", 24.0) * math.sin(math.pi * min(s * 1.3, 1.0))) * (1 - sstep((s - 0.75) / 0.25))
    flex = W.get("swing_flex", 30.0) * math.sin(math.pi * min(s * 1.25, 1.0)) * (1 - sstep((s - 0.7) / 0.3))
    return df, lift, pitch, flex, s


class Legs:
    def __init__(self, ps):
        self.ps = ps
        ps.reset()
        ps.solve()
        self.rest = {n: (ps.head(n), ps.tail(n)) for n in ps.order}

    def place(self, side, fore, df, lift, pitch, flex, swing):
        ps, s = self.ps, side
        X = Vector((1, 0, 0))
        if fore:
            top, up, mid, can, toe, hoof = ["%s.%s" % (n, s) for n in FORE]
            bend_back = True
        else:
            up, mid, can, toe, hoof = ["%s.%s" % (n, s) for n in HIND]
            top = None
            bend_back = False
        rh, rt = self.rest[hoof]
        R = Matrix.Rotation(math.radians(pitch), 3, X)      # + lifts the heel, toe down
        off = Vector((0, -df, lift))
        coronet = rt + R @ (rh - rt) + off
        hoof_dir = R @ (rt - rh)
        th, tt = self.rest[toe]
        Rp = Matrix.Rotation(math.radians(pitch * 0.55 + flex), 3, X)
        fetlock = coronet + Rp @ (th - tt)
        # the leg line (top joint to fetlock) turns by ang from rest; the
        # cannon swings with it, folded in swing (fore: the knee forward,
        # hind: the hock closing)
        top_joint = self.rest[up][0]
        r_line = self.rest[can][1] - top_joint
        n_line = fetlock - top_joint
        ang = math.degrees(math.atan2(-(n_line.y * r_line.z - n_line.z * r_line.y), r_line.y * n_line.y + r_line.z * n_line.z))
        W = S["walk"]
        if fore and top:
            ps.set_local(top, qx(-ang * W.get("scapula_share", 0.3)))
        fold = math.sin(math.pi * swing) * (W.get("fore_fold", 55.0) if fore else -W.get("hind_fold", 30.0))
        ch, ct = self.rest[can]
        cdir = Matrix.Rotation(math.radians(ang * W.get("cannon_share", 0.8) + fold), 3, X) @ (ct - ch).normalized()
        knee = fetlock - cdir * ps.len[can]
        a = ps.head(up)
        rm = self.rest[mid][0]
        ra, rk = self.rest[up][0], self.rest[can][0]
        hint = rm - (ra + (rk - ra) * ((rm - ra).dot(rk - ra) / max((rk - ra).length_squared, 1e-9)))
        if hint.length < 1e-4:
            hint = Vector((0, 1 if bend_back else -1, 0))
        j = two_bone(a, knee, ps.len[up], ps.len[mid], hint)
        ps.aim(up, j - a)
        ps.aim(mid, knee - ps.head(mid))
        ps.aim(can, fetlock - ps.head(can))
        ps.aim(toe, coronet - ps.head(toe))
        ps.aim(hoof, hoof_dir)


def idle_bump(t, at, width):
    x = (t - at) / (width / 2)
    return 0.0 if abs(x) >= 1 else 0.5 + 0.5 * math.cos(math.pi * x)


def body_fk(ps, t_walk=None, t_idle=None):
    W = S["walk"]
    I = S.get("idle", {})
    sc = B.head_len / 0.6
    if t_walk is not None:
        ph = 2 * math.pi * t_walk
        bob = -W.get("bob", 0.015) * math.cos(2 * ph)
        sway = W.get("bob", 0.015) * 0.7 * math.sin(ph)
        ps.local("spine.001", qz(1.5 * math.sin(ph)) @ Quaternion((0, 1, 0), math.radians(2.0 * math.sin(ph))), loc=Vector((sway, 0.0, bob)))
        ps.local("spine.003", Quaternion((0, 1, 0), math.radians(-1.2 * math.sin(ph))))
        ps.local("spine.005", Quaternion((0, 1, 0), math.radians(-1.0 * math.sin(ph))))
        nod = math.sin(2 * ph + 0.9)
        ps.local("neck.001", qx(-W.get("nod", 3.0) * nod))
        ps.local("neck.002", qx(-1.2 * nod) @ qz(1.2 * math.sin(ph)))
        ps.local("neck.004", qx(2.0 * nod))
        ps.local("head", qx(1.5 * nod))
        ps.local("tail.001", qz(W.get("tail_swing", 5.0) * math.sin(ph - 0.5)))
        for i in range(2, 6):
            ps.local("tail.%03d" % i, qz(W.get("tail_swing", 5.0) * 0.8 * math.sin(ph - 0.5 - 0.5 * i)))
        for s, sg in (("L", 1), ("R", -1)):
            ps.local("ear." + s, qx(2.0 * math.sin(2 * ph + sg)) @ qz(sg * 2.0))
    else:
        ph = 2 * math.pi * t_idle
        shift = 0.014 * sc * math.sin(ph)
        ps.local("spine.001", Quaternion((0, 1, 0), math.radians(1.4 * math.sin(ph))), loc=Vector((shift, 0.0, -0.003 * sc * (1 - math.cos(2 * ph)))))
        breath = 0.5 - 0.5 * math.cos(ph)
        ps.abd_scale = Vector((1 + 0.03 * breath, 1.0, 1 + 0.025 * breath))
        ps.local("spine.004", qx(-0.5 * breath))
        ps.local("neck.001", qx(-2.0 * math.sin(ph + 0.7)) @ qz(-2.0 * math.sin(ph)))
        ps.local("neck.003", qz(-1.5 * math.sin(ph)))
        ps.local("head", qx(2.0 * math.sin(ph + 1.4)) @ qz(3.0 * math.sin(ph + 0.3)))
        fl = idle_bump(t_idle, 0.30, 0.10)
        fr = idle_bump(t_idle, 0.68, 0.12)
        ear = I.get("ear_flick", 25.0)
        ps.local("ear.L", qx(-ear * fl) @ qz(8.0 * fl))
        ps.local("ear.R", qx(-ear * 0.8 * fr) @ qz(-10.0 * fr))
        sw = idle_bump(t_idle, 0.52, 0.34)
        wag = I.get("tail_wag", 12.0)
        rate = I.get("tail_rate", 2.0)
        ps.local("tail.001", qz(wag * sw * math.sin(2 * rate * math.pi * (t_idle - 0.37))))
        for i in range(2, 6):
            ps.local("tail.%03d" % i, qz(wag * 0.7 * sw * math.sin(2 * rate * math.pi * (t_idle - 0.37) - 0.6 * i)))


def breast_follow(ps, legs):
    """The pectorals tie the arm to the breastbone, so the breast skin goes
    part of the way with the arm: each breast bone turns by a share of its
    upper arm's swing measured against the trunk (as the test horse)."""
    share = S["walk"].get("breast_follow", 0.6)
    for s in ("L", "R"):
        up, br = "upper_arm." + s, "breast." + s
        par = ps.bones[br].parent.name
        Pm = ps.pose[par].to_3x3() @ ps.rest[par].to_3x3().inverted()
        rh, rt = legs.rest[up]
        d_rel = Pm.inverted() @ (ps.tail(up) - ps.head(up))
        R = (rt - rh).rotation_difference(d_rel)
        Rk = Quaternion().slerp(R, share)
        bh, bt = legs.rest[br]
        ps.aim(br, Pm @ (Rk @ (bt - bh)))


def make_clips(rig):
    ps = Poser(rig)
    legs = Legs(ps)
    rig.animation_data_create()
    order = [("L", False, 0.0), ("L", True, 0.25), ("R", False, 0.5), ("R", True, 0.75)]
    clips = {}
    for name, frames in (("walk", S["walk"]["frames"]), ("idle", S.get("idle", {}).get("frames", 120))):
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
                    legs.place(side, fore, *foot_path((t - off) % 1.0))
                else:
                    legs.place(side, fore, 0.0, 0.0, 0.0, 0.0, 0.0)
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


# ------------------------------------------------------------- occlusion, export
AO_ATTR = "_EVANI_AO"


def bake_ao(mesh, rig, samples):
    """Ambient occlusion per vertex in the rest pose (Cycles, the animal
    alone), as the test horse v5 (rig_horse.py bake_ao)."""
    sc = bpy.context.scene
    old = (sc.render.engine, rig.data.pose_position)
    rig.data.pose_position = "REST"
    ca = mesh.data.color_attributes.new(AO_ATTR, "BYTE_COLOR", "POINT")
    keep = mesh.data.color_attributes.active_color
    mesh.data.color_attributes.active_color = ca
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = samples
    sc.render.threads_mode = "FIXED"
    sc.render.threads = 2
    w = sc.world or bpy.data.worlds.new("World")
    sc.world = w
    w.light_settings.distance = 0.5 * max(B.head_len / 0.6, 0.4)
    activate(mesh)
    mesh.hide_render = False
    bpy.context.view_layer.update()
    bpy.ops.object.bake(type="AO", target="VERTEX_COLORS")
    vals = []
    for d in ca.data:
        a = max(0.12, d.color[0])
        d.color = (a, a, a, 1.0)
        vals.append(a)
    vals.sort()
    q = lambda f: round(vals[min(len(vals) - 1, int(f * len(vals)))], 3)
    print("QUAD_AO", json.dumps({"vertices": len(vals), "p05": q(0.05), "median": q(0.5)}))
    sc.render.engine, rig.data.pose_position = old
    if keep is not None:
        mesh.data.color_attributes.active_color = keep


def export(mesh, rig, out):
    activate(rig)
    mesh.select_set(True)
    path = os.path.join(out, ID + ".glb")
    want = dict(filepath=path, export_format="GLB", use_selection=True, export_yup=True,
                export_apply=False, export_skins=True, export_animations=True,
                export_animation_mode="ACTIONS", export_force_sampling=True,
                export_frame_range=False, export_anim_single_armature=True,
                export_def_bones=True, export_tangents=False, export_materials="EXPORT",
                export_extras=False, export_cameras=False, export_lights=False,
                export_attributes=True, export_vertex_color="NONE")
    have = set(bpy.ops.export_scene.gltf.get_rna_type().properties.keys())
    bpy.ops.export_scene.gltf(**{k: v for k, v in want.items() if k in have})
    print("QUAD_EXPORT", path, os.path.getsize(path))
    return path


def run(mesh, reg, eye_from):
    from body import landmarks
    rig = make_metarig()
    fit(rig, landmarks(B))
    skin(mesh, rig, reg, eye_from)
    return rig


def _posed(mesh, n):
    import numpy as np
    dg = bpy.context.evaluated_depsgraph_get()
    em = mesh.evaluated_get(dg).data
    co = np.empty(len(em.vertices) * 3)
    em.vertices.foreach_get("co", co)
    return co.reshape(-1, 3)[:n]


def relax_stretch(mesh, rig, clips, upto, rounds=14, tmax=1.5, tmin=0.5, min_change=0.002, step=2):
    """Plays the clips, finds the body-skin edges stretched past tmax or
    squeezed under tmin (and changing by more than min_change m), and evens
    the weights of their vertices and their neighbours with the neighbours'
    (a local smoothing, keeping the four largest), round after round. A
    tool for every species: the weights come from heat, the midline masks
    and the limb blend, and the clips decide where the skin is asked to
    stretch; this spreads the change over more skin exactly there."""
    import numpy as np
    me = mesh.data
    vg = mesh.vertex_groups
    names = [g.name for g in vg]
    dcols = [i for i, nm in enumerate(names) if nm in DEFORM]
    n = len(me.vertices)
    ev = np.empty(len(me.edges) * 2, dtype=np.int64)
    me.edges.foreach_get("vertices", ev)
    ev = ev.reshape(-1, 2)
    ev = ev[(ev[:, 0] < upto) & (ev[:, 1] < upto)]
    rest = np.empty(n * 3)
    me.vertices.foreach_get("co", rest)
    rest = rest.reshape(-1, 3)[:upto]
    L0 = np.linalg.norm(rest[ev[:, 0]] - rest[ev[:, 1]], axis=1)
    a, b = ev[:, 0], ev[:, 1]
    deg = np.maximum(np.bincount(a, minlength=upto) + np.bincount(b, minlength=upto), 1).astype(np.float64)
    frames = []
    for act in clips.values():
        f0, f1 = int(act.frame_range[0]), int(act.frame_range[1])
        frames += [(act, f) for f in range(f0, f1 + 1, step)]
    scene = bpy.context.scene
    hist = []
    best = None
    for rnd in range(rounds + 1):
        W = np.zeros((upto, len(names)))
        for v in me.vertices[:upto]:
            for g in v.groups:
                W[v.index, g.group] = g.weight
        hi = np.ones(len(ev))
        lo = np.ones(len(ev))
        bad = np.zeros(len(ev), dtype=bool)
        for act, f in frames:
            rig.animation_data.action = act
            scene.frame_set(f)
            P = _posed(mesh, upto)
            L = np.linalg.norm(P[a] - P[b], axis=1)
            r = L / np.maximum(L0, 1e-9)
            hi = np.maximum(hi, r)
            lo = np.minimum(lo, r)
            bad |= ((r > tmax) | (r < tmin)) & (np.abs(L - L0) > min_change)
        # how far past the goal, summed (stretch and squeeze), the worst edge counted twice
        exc = np.maximum(hi - tmax, 0) + np.maximum(tmin / np.maximum(lo, 1e-3) - 1, 0)
        score = float(exc.sum() + 50 * exc.max())
        hist.append({"bad": int(bad.sum()), "max": round(float(hi.max()), 2), "min": round(float(lo.min()), 3), "score": round(score, 1)})
        if best is None or score < best[0]:
            best = (score, W.copy(), rnd)
        if rnd == rounds or not bad.any():
            break
        sel = np.zeros(upto, dtype=bool)
        sel[a[bad]] = True
        sel[b[bad]] = True
        for _ in range(2):      # grow by two rings
            sel[a[sel[b]]] = True
            sel[b[sel[a]]] = True
        for _ in range(3):
            S = np.zeros_like(W)
            np.add.at(S, a, W[b])
            np.add.at(S, b, W[a])
            M = S / deg[:, None]
            W[sel] = 0.5 * W[sel] + 0.5 * M[sel]
        idx = np.nonzero(sel)[0]
        for i in idx:
            w = W[i, dcols]
            order = np.argsort(-w)
            ww = np.zeros_like(w)
            ww[order[:4]] = w[order[:4]]
            ww[ww < 0.005] = 0.0
            s = ww.sum()
            ww = ww / s if s > 0 else w
            for k, gi in enumerate(dcols):
                if ww[k] > 0:
                    vg[gi].add([int(i)], float(ww[k]), "REPLACE")
                else:
                    vg[gi].remove([int(i)])
    if best[2] != len(hist) - 1:
        Wb = best[1]
        for i in np.nonzero(np.abs(Wb - W).max(axis=1) > 1e-6)[0]:
            for gi in dcols:
                if Wb[i, gi] > 0:
                    vg[gi].add([int(i)], float(Wb[i, gi]), "REPLACE")
                else:
                    vg[gi].remove([int(i)])
    print("QUAD_RELAX", json.dumps({"kept_round": best[2], "rounds": hist}))


def finish(mesh, rig, out, ao=True, ao_samples=64, eye_from=None):
    clips = make_clips(rig)
    if eye_from:
        relax_stretch(mesh, rig, clips, eye_from)
    rig.animation_data.action = None
    if ao:
        bake_ao(mesh, rig, ao_samples)
    export(mesh, rig, out)
    return {"clips": {k: int(v.frame_range[1] - v.frame_range[0]) + 1 for k, v in clips.items()}, "bones": len([b for b in rig.data.bones if b.use_deform])}
