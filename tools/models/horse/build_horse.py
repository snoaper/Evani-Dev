# Evani.Dev test horse (original work, built from numbers in this file only:
# no downloaded meshes, no outside libraries; Blender itself and its bundled
# Rigify horse metarig are the only tools).
#
# A bay riding horse (about 16 hands: 1.60 m at the withers, body length
# point of shoulder to point of buttock about 1.60 m, legs about half the
# withers height), for the fur and hair work (B5 fur, mane and tail strands
# later). Mane and tail hair are NOT on the mesh: markers for the strand job
# are (vertex groups, a crest curve and anchor nodes).
#
# Run (pinned Blender 5.2.2):
#   blender -b --factory-startup -t 2 --python tools/models/horse/build_horse.py -- \
#       --out <folder> [--faces 14000] [--voxel 0.006] [--views side,front,threeq,head]
#       [--stage surface|mesh|all] [--fillet 10] [--mean 2] [--seed 7]
#   (the defaults build the committed v4 horse; tests/fixtures/models/horse is
#   its conversion)
# Writes <folder>/horse.blend, <folder>/horse.glb (stage all) and renders
# <folder>/<view>.png for each view asked.
#
# How it is made (so it can be rebuilt and changed by numbers):
#  1. Parts as closed lofts of superellipse cross-sections between a pair of
#     silhouette points (top and bottom for the trunk, neck and head; front
#     and back for the legs), plus muscle ellipsoids, cupped ears and hooves.
#  2. Voxel union (one watertight surface), smoothing that blends the joins,
#     the face carved (eye sockets, nostrils, mouth line).
#  3. QuadriFlow: even all-quad topology, symmetric about the midline.
#  4. UV islands cut along hair partings (topline, belly, back of each leg,
#     under the jaw), scaled evenly and turned so +v runs the way hair flows.
#  5. Rigify horse metarig fitted to the landmarks below, automatic weights,
#     weights cleaned (four per vertex, normalised, hooves rigid).
#  6. Walk (4-beat, looping) and idle (weight shift, ear flick, breathing).
import bpy, bmesh, sys, os, math, json, argparse
from mathutils import Vector, Matrix, Quaternion, Euler

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ap = argparse.ArgumentParser()
ap.add_argument("--out", required=True)
ap.add_argument("--faces", type=int, default=14000)
ap.add_argument("--voxel", type=float, default=0.006)
ap.add_argument("--views", default="")
ap.add_argument("--stage", default="all")
ap.add_argument("--engine", default="WORKBENCH")
ap.add_argument("--blend", default="")   # --stage ao: the built horse to bake

# v3: v2 built with --fillet 30 (about 18 cm of crease rounding), which
# filled the flank fold and the groin with a sheet of skin from the belly to
# the gaskin: the rest-pose half of the walk's "bat wing". 10 rounds the
# muscle joins and keeps the folds.
ap.add_argument("--fillet", type=int, default=10)
ap.add_argument("--mean", type=int, default=2)
ap.add_argument("--smooth", type=int, default=8)
ap.add_argument("--seed", type=int, default=7)   # v3: the quad layout that untangles cleanly with the v3 chest (3 left a fold at the throat)
ARGS = ap.parse_args(argv)
OUT = os.path.abspath(ARGS.out)
os.makedirs(OUT, exist_ok=True)
pi = math.pi


sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from anatomy import (P, POLL, HEAD_DEG, _hd, _hn, head_pt, head_local, LM, EYE_X, EYE_R, HOOVES, DOCK, MUSCLES,  # noqa: F401
                     catmull, loft, prof_part, limb_part, ellipsoid, muscle, trunk_keys, neck_keys, head_keys, FORE, HIND)
import anatomy  # noqa: F401

# head features in the head frame (a down the face, b in toward the jaw)
EYE_AB = (0.200, 0.050)
NOSTRIL_AB = (0.600, 0.060)
NOSTRIL_X = 0.050
MOUTH_CORNER_AB = (0.505, 0.138)
MOUTH_FRONT_AB = (0.637, 0.118)
EAR_AB = (0.012, 0.004)
EAR_X = 0.054
EAR_DIR = (0.26, 1.0)     # (f, z): up and a little forward
EAR_OUT = 0.30            # and out
EAR_LEN = 0.165


def HV(a, b, x=0.0):
    return P(*head_pt(a, b), x)


def build_parts():
    bm = bmesh.new()
    prof_part(bm, trunk_keys(), 70, 64)
    prof_part(bm, neck_keys(), 36, 48)
    prof_part(bm, head_keys(), 56, 56)
    limb_part(bm, FORE, 80, 36)
    limb_part(bm, HIND, 80, 36)
    for f, z, x, r, ax in anatomy.MUSCLES:
        muscle(bm, f, z, x, r, ax)
    if anatomy.BREAST:
        (bf, bz), br, bax = anatomy.BREAST
        muscle(bm, bf, bz, 0.0, (br[1], br[0], br[2]), bax, mirror=False)
    for f, z, x, r, ax, tilt in getattr(anatomy, "PECTORALS", []):
        muscle(bm, f, z, x, r, ax, tilt_out=tilt)
    # head masses: the masseter disc on the cheek, brow over the eye, the
    # nostril flare, the upper and lower lip and the chin
    d = (P(*_hd) - P(0, 0)).normalized()
    n_ = (P(*_hn) - P(0, 0)).normalized()
    X = Vector((1, 0, 0))
    for sx in (1, -1):
        ellipsoid(bm, HV(0.170, 0.195, sx * 0.090), (d, n_, X), (0.088, 0.088, 0.034))   # masseter: the flat round cheek
        ellipsoid(bm, HV(EYE_AB[0] - 0.008, 0.026, sx * 0.097), (d, n_, X), (0.040, 0.016, 0.022))
        ellipsoid(bm, HV(NOSTRIL_AB[0] - 0.010, NOSTRIL_AB[1] - 0.006, sx * 0.047), (d, n_, X), (0.050, 0.034, 0.028))
        # facial crest: a long low ridge from under the eye along the face
        ax_ = (d + n_ * 0.28).normalized()
        ellipsoid(bm, HV(0.315, 0.112, sx * 0.087), (ax_, n_.cross(X).cross(ax_).normalized() if False else (n_ - ax_ * n_.dot(ax_)).normalized(), X), (0.100, 0.016, 0.016))
    ellipsoid(bm, HV(0.612, 0.098), (d, n_, X), (0.036, 0.034, 0.058))   # upper lip
    ellipsoid(bm, HV(0.598, 0.146), (d, n_, X), (0.034, 0.024, 0.044))   # lower lip
    ellipsoid(bm, HV(0.560, 0.172), (d, n_, X), (0.040, 0.028, 0.036))   # chin
    # dock
    rings = []
    for i in range(41):
        s = i / 40 * (len(DOCK) - 1)
        f, z, r = catmull(DOCK, s)
        f2, z2, _ = catmull(DOCK, min(s + 0.05, len(DOCK) - 1.0001))
        f1, z1, _ = catmull(DOCK, max(s - 0.05, 0))
        t = (P(f2, z2) - P(f1, z1)).normalized()
        u = Vector((1, 0, 0)).cross(t).normalized()
        rings.append([P(f, z) + Vector((1, 0, 0)) * (r * math.cos(2 * pi * k / 24)) + u * (r * math.sin(2 * pi * k / 24)) for k in range(24)])
    loft(bm, rings)
    for sx in (1, -1):
        ear(bm, sx)
    for fc, xc, hw, hl, htoe, hheel in HOOVES:
        for sx in (1, -1):
            hoof(bm, fc, sx * xc, hw, hl, htoe, hheel)
    return bm


def ear_frame(sx):
    base = HV(EAR_AB[0], EAR_AB[1], sx * EAR_X)
    up = Vector((sx * EAR_OUT, -EAR_DIR[0], EAR_DIR[1])).normalized()
    fwd = Vector((sx * 0.45, -1.0, 0.0))
    fwd = (fwd - up * fwd.dot(up)).normalized()   # the cup opens forward and a little out
    side = up.cross(fwd).normalized()
    return base, up, fwd, side


def ear(bm, sx):
    """A leaf-shaped, cupped, thick-walled ear: C-shaped sections from a
    rolled base (nearly a tube) through the broad cup to a pointed tip that
    turns a little in toward the other ear."""
    base, up, fwd, side = ear_frame(sx)
    inward = Vector((-sx, 0, 0))
    n = 32
    rings = []
    K = 22
    for i in range(K + 1):
        t = i / K
        # leaf: a broad rolled base, widest at 35 %, then a long taper to a point
        if t < 0.35:
            f = 0.78 + 0.22 * math.sin(0.5 * pi * t / 0.35)
        else:
            f = math.cos(0.5 * pi * (t - 0.35) / 0.65) ** 1.15
        w = 0.041 * f + 0.0008
        dep = w * (0.80 - 0.30 * t)
        th = min(0.011 - 0.005 * t, 0.45 * w + 0.0008)   # the wall thins with the ear to a point
        A = math.radians(160 - 65 * t)
        c = base + up * (EAR_LEN * t) + inward * (0.016 * t ** 2.0) + fwd * (0.012 * math.sin(pi * t))
        pts = []
        for k in range(n // 2):
            al = -A + 2 * A * k / (n // 2 - 1)
            pts.append(c + side * (math.sin(al) * w) - fwd * (math.cos(al) * dep))
        for k in range(n // 2):
            al = A - 2 * A * k / (n // 2 - 1)
            pts.append(c + side * (math.sin(al) * max(w - th, 0.0008)) - fwd * (math.cos(al) * max(dep - th, 0.0004)))
        rings.append(pts)
    rings.insert(0, [p - up * 0.03 for p in rings[0]])   # sink the base into the head
    loft(bm, rings)


def hoof(bm, fc, xc, hw, hl, htoe, hheel):
    """The hoof wall from the ground to the coronet (higher at the toe than
    the heels, the toe wall at about the pastern's angle), flaring out toward
    the ground, the sole flat."""
    n, K = 40, 10
    rings = []
    for k in range(K + 1):
        s = k / K
        sc = 1.0 - 0.16 * s
        shift = -0.056 * s
        pts = []
        for i in range(n):
            th = 2 * pi * i / n
            fr = (1 + math.cos(th)) / 2
            ln = hl * (1.0 if math.cos(th) > 0 else 0.92)
            p = P(fc + shift + math.cos(th) * ln * sc, 0.0, xc + math.sin(th) * hw * sc * (1.0 - 0.06 * (1 - fr)))
            p.z = s * (hheel + (htoe - hheel) * fr ** 1.5) + 0.002
            pts.append(p)
        rings.append(pts)
    rings[0] = [Vector((p.x, p.y, 0.0)) for p in rings[0]]
    rings.append([p + Vector((0, 0, 0.014)) for p in rings[-1]])
    loft(bm, rings)


# ------------------------------------------------------------ surface work
def new_obj(name, bm):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    return ob


def activate(ob):
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)


def smooth_mod(ob, factor, iters, group=None):
    m = ob.modifiers.new("smooth", "SMOOTH")
    m.factor, m.iterations = factor, iters
    if group:
        m.vertex_group = group
    activate(ob)
    bpy.ops.object.modifier_apply(modifier=m.name)


def in_ear(co):
    for sx in (1, -1):
        base, up, fwd, side = ear_frame(sx)
        h = (co - base).dot(up)
        if -0.01 < h < EAR_LEN + 0.02 and ((co - base) - up * h).length < 0.07:
            return True
    return False


def protect_weights(ob, name="smoothable"):
    """Smooth everywhere but the ears, the face's fine parts and the lower
    legs and hooves."""
    g = ob.vertex_groups.get(name) or ob.vertex_groups.new(name=name)
    for v in ob.data.vertices:
        z = v.co.z
        w = 1.0
        if in_ear(v.co):
            w = 0.08
        a, b, x = head_local(v.co)
        if a > 0.45 and b < 0.26 and z > 1.2:
            w = min(w, 0.35)      # muzzle: keep the lips and nostrils
        if z < 0.62:
            w = min(w, 0.40)
        if z < 0.10:
            w = min(w, 0.2 + 2.0 * max(z - 0.02, 0))
        g.add([v.index], w, "REPLACE")
    return name


def _ss(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def carve(ob):
    """Sculpt by numbers on the dense surface: eye sockets with a lid rim,
    flared nostrils (a deep comma-shaped opening with a raised rim), the
    mouth line between the lips, the groove between the jaws, the jugular
    groove of the neck."""
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.normal_update()
    for v in bm.verts:
        a, b, x = head_local(v.co)
        ax = abs(x)
        push = 0.0
        if -0.05 < a < 0.70 and b < 0.34:
            # v3: the eye is shaped on the final quads (shape_eyes: an almond
            # with lids, the eye itself part of the skin), not carved here
            # nostril: a large comma on the front corner of the muzzle,
            # opening forward and out: the round part low and forward, the
            # tail curling up and back toward the face line; its rim raised
            # (the flare)
            na = (a - NOSTRIL_AB[0]) / 0.040
            nb = (b - NOSTRIL_AB[1]) / 0.024
            nx = (ax - NOSTRIL_X) / 0.030
            nb += 0.45 * max(-na, 0) ** 2        # the tail rises toward the face line
            rr = na * na + nb * nb + nx * nx
            if rr < 1.0:
                push += 0.036 * (1 - rr) ** 0.55
            elif rr < 2.4 and ax > 0.015:
                push -= 0.006 * math.sin(pi * (rr - 1.0) / 1.4)
            # mouth line: a groove from the corner to the front, the upper
            # lip over the lower
            if MOUTH_CORNER_AB[0] - 0.01 < a < 0.66:
                u = (a - MOUTH_CORNER_AB[0]) / (MOUTH_FRONT_AB[0] - MOUTH_CORNER_AB[0])
                line_b = MOUTH_CORNER_AB[1] + (MOUTH_FRONT_AB[1] - MOUTH_CORNER_AB[1]) * min(max(u, 0), 1)
                dd = b - line_b
                if abs(dd) < 0.012:
                    fade = _ss(-0.05, 0.15, u)
                    push += 0.009 * (1 - abs(dd) / 0.012) ** 1.5 * fade
            # the hollow above the eye
            da, db = (a - 0.150) / 0.040, (b - 0.012) / 0.030
            if ax > 0.06 and da * da + db * db < 1:
                push += 0.006 * (1 - da * da - db * db)
            # the channel between the jaws (under the head)
            if 0.10 < a < 0.46 and ax < 0.04 and b > 0.15:
                push += 0.008 * (1 - ax / 0.04) * _ss(0.10, 0.18, a) * (1 - _ss(0.38, 0.46, a))
        else:
            f, z = -v.co.y, v.co.z
            # the groove down the middle of the breast (the two pectoral
            # masses read as an inverted V from the front)
            if f > 0.70 and 0.90 < z < 1.24 and ax < 0.040:
                push += 0.0045 * (1 - (ax / 0.040) ** 2) * _ss(0.70, 0.78, f) * _ss(0.90, 0.98, z) * (1 - _ss(1.14, 1.24, z))
            # v3: the groove between the hamstrings (biceps femoris in
            # front, semitendinosus behind) down the back of the thigh
            if -0.80 < f < -0.62 and 0.86 < z < 1.20 and ax > 0.09:
                fg = -0.700 + 0.030 * (1.18 - z)
                dd = abs(f - fg) / 0.026
                if dd < 1:
                    push += (0.005 if LAYOUT else 0.003) * (1 - dd * dd) * _ss(0.86, 0.95, z) * (1 - _ss(1.12, 1.20, z))   # v4: 5 -> 3 mm (lumpy from behind)
            # v3: the hollow in front of the Achilles cord, above the point
            # of the hock, on both sides of the leg (the cord stands out)
            if -0.72 < f < -0.55 and 0.60 < z < 0.80 and ax > 0.09:
                fc_ = -0.665 + 0.12 * (z - 0.62)        # the cord's front edge
                dd = (fc_ - f) / 0.045                   # 0 at the cord, 1 at the tibia
                side = abs(ax - 0.155)
                if 0 < dd < 1 and side > 0.012:
                    push += 0.006 * math.sin(math.pi * dd) * _ss(0.60, 0.65, z) * (1 - _ss(0.74, 0.80, z)) * _ss(0.012, 0.03, side)
            # jugular groove: along the lower third of the neck's side
            zl = 1.32 + (f - 0.90) * 2.7
            if 0.88 < f < 1.08 and ax > 0.04 and abs(z - zl) < 0.035 and z > 1.25:
                push += 0.004 * (1 - abs(z - zl) / 0.035) * _ss(0.88, 0.94, f) * (1 - _ss(1.02, 1.08, f))
        if push:
            v.co -= v.normal * push
    bm.to_mesh(me)
    bm.free()


def build_surface(layout=False):
    """The dense surface. layout=True: v3's surface (anatomy.LAYOUT_V3, no
    shoulder blend, v3's hamstring groove), only for laying out the quads."""
    saved = {k: getattr(anatomy, k) for k in anatomy.LAYOUT_V3}
    global LAYOUT
    LAYOUT = layout
    if layout:
        for k, v in anatomy.LAYOUT_V3.items():
            setattr(anatomy, k, v)
    try:
        bm = build_parts()
    finally:
        for k, v in saved.items():
            setattr(anatomy, k, v)
    ob = new_obj("layout" if layout else "parts", bm)
    activate(ob)
    me = ob.data
    me.remesh_voxel_size = ARGS.voxel
    me.use_remesh_fix_poles = True
    bpy.ops.object.voxel_remesh()
    if ARGS.fillet > 0:
        sdf_fillet(ob, ARGS.voxel, ARGS.fillet, ARGS.mean)
    smooth_mod(ob, 0.6, ARGS.smooth, protect_weights(ob))
    if not layout:
        shoulder_blend(ob)
    carve(ob)
    smooth_mod(ob, 0.5, 2)
    LAYOUT = False
    return ob


LAYOUT = False   # True while the v3 layout surface is built


def shoulder_blend(ob, iters=60):
    """v4: the side of the breast met the arm in a crease and stood out in
    front of the shoulder as a round ball (the three-quarter view). Smooth
    that join more than the rest: a soft zone over the point of the shoulder
    and the side of the breast, fading out on every side."""
    g = ob.vertex_groups.new(name="shoulder_blend")
    for v in ob.data.vertices:
        f, z, ax = -v.co.y, v.co.z, abs(v.co.x)
        w = _ss(0.60, 0.70, f) * _ss(0.86, 0.94, z) * (1 - _ss(1.20, 1.32, z)) * _ss(0.05, 0.10, ax)
        if w > 0.01:
            g.add([v.index], w, "REPLACE")
    smooth_mod(ob, 0.7, iters, g.name)
    ob.vertex_groups.remove(ob.vertex_groups[g.name])


def sdf_fillet(ob, voxel, fillet, mean):
    """Round off the creases where parts meet: the surface as a signed
    distance grid, filleted (only concave places move), lightly averaged,
    then back to a mesh."""
    ng = bpy.data.node_groups.new("fillet", "GeometryNodeTree")
    ng.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
    ng.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
    N = ng.nodes
    gi, go = N.new("NodeGroupInput"), N.new("NodeGroupOutput")
    m2s = N.new("GeometryNodeMeshToSDFGrid")
    m2s.inputs["Voxel Size"].default_value = voxel
    m2s.inputs["Band Width"].default_value = 6
    fil = N.new("GeometryNodeSDFGridFillet")
    fil.inputs["Iterations"].default_value = fillet
    mn = N.new("GeometryNodeSDFGridMean")
    mn.inputs["Width"].default_value = 1
    mn.inputs["Iterations"].default_value = max(mean, 0)
    g2m = N.new("GeometryNodeGridToMesh")
    g2m.inputs["Threshold"].default_value = 0.0
    L = ng.links
    L.new(gi.outputs[0], m2s.inputs["Mesh"])
    L.new(m2s.outputs[0], fil.inputs["Grid"])
    if mean > 0:
        L.new(fil.outputs[0], mn.inputs["Grid"])
        L.new(mn.outputs[0], g2m.inputs["Grid"])
    else:
        L.new(fil.outputs[0], g2m.inputs["Grid"])
    L.new(g2m.outputs[0], go.inputs[0])
    md = ob.modifiers.new("fillet", "NODES")
    md.node_group = ng
    activate(ob)
    bpy.ops.object.modifier_apply(modifier=md.name)
    print("HORSE_FILLET", len(ob.data.vertices))


def quad_remesh(src, faces, lay=None):
    """QuadriFlow on `lay` (v4: v3's surface, so the quads keep v3's layout),
    then everything projected onto `src`, the shape."""
    bpy.ops.object.select_all(action="DESELECT")
    lay = lay or src
    ob = lay.copy()
    ob.data = lay.data.copy()
    ob.name = "horse"
    bpy.context.scene.collection.objects.link(ob)
    activate(ob)
    # QuadriFlow is slow on a dense input: thin it first (shape kept)
    # (a coarser voxel pass: decimating can leave it non-manifold, which
    # QuadriFlow refuses)
    keep = ob.data.copy()
    for vs in (0.010, 0.008, 0.007, 0.006):
        ob.data = keep.copy()
        ob.data.remesh_voxel_size = vs
        bpy.ops.object.voxel_remesh()
        bm = bmesh.new()
        bm.from_mesh(ob.data)
        bad = sum(1 for e in bm.edges if not e.is_manifold) + sum(1 for v in bm.verts if not v.is_manifold)
        bm.free()
        print("HORSE_THIN", vs, bad)
        if bad == 0:
            break
    # one light pass: voxel steps can leave points touching where thin parts
    # (ear walls) meet, which QuadriFlow refuses
    smooth_mod(ob, 0.5, 1)
    bpy.ops.object.quadriflow_remesh(target_faces=faces, use_mesh_symmetry=True, use_preserve_sharp=False, use_preserve_boundary=False, smooth_normals=False, mode="FACES", seed=ARGS.seed)
    ob = bpy.context.view_layer.objects.active
    print("HORSE_QUADRIFLOW", len(ob.data.polygons))
    if len(ob.data.polygons) > faces * 1.6:
        raise RuntimeError("QuadriFlow did not run (%d faces)" % len(ob.data.polygons))
    # relax the quads a little, then back onto the dense surface
    smooth_mod(ob, 0.5, 3)
    sw = ob.modifiers.new("sw", "SHRINKWRAP")
    sw.target = src
    sw.wrap_method = "NEAREST_SURFACEPOINT"
    bpy.ops.object.modifier_apply(modifier=sw.name)
    if lay is not src:
        hold_layout(ob, lay)
    soften_shoulder(ob, src)
    soften_buttock(ob)
    untangle(ob)
    refine_head(ob, src)
    untangle(ob)
    shape_eyes(ob)
    shape_hooves(ob, src)
    untangle(ob)
    return ob


# ------------------------------------------------------------ hooves (v4)
# v3's hoof colour followed the 2 cm quads, so the coronet (where the horn
# meets the hair) and the wall read as a jagged staircase. v4 subdivides each
# hoof and its coronet once (about 1 cm), puts the wall back on the hoof's
# own shape (the loft in anatomy's HOOVES numbers), snaps one row of points
# onto the coronet line, so the material edge runs along it, and raises a
# low coronet band just above it.
CORONET_BAND = (0.012, 0.0016)   # height above the coronet line and how far out


def hoof_frame(f, x):
    """The hoof (numbers) a point at (f, x) belongs to, or None."""
    for fc, xc, hw, hl, htoe, hheel in HOOVES:
        xs = xc if x >= 0 else -xc
        if abs(x - xs) < hw * 1.6 and abs(f - (fc - 0.028)) < hl * 1.7:
            return fc, xs, hw, hl, htoe, hheel
    return None


def hoof_coords(f, z, x, hf):
    """(s, angle, fr) of a point on hoof hf: s 0 at the ground, 1 at the
    coronet (hoof() in this file: rings shrink and lean back as s rises)."""
    fc, xs, hw, hl, htoe, hheel = hf
    s = max(0.0, min(1.3, (z - 0.002) / (0.5 * (htoe + hheel))))
    th, fr = 0.0, 1.0
    for _ in range(6):
        sc, c = 1.0 - 0.16 * min(s, 1.0), fc - 0.056 * min(s, 1.0)
        ln = hl * (1.0 if math.cos(th) > 0 else 0.92)
        th = math.atan2((x - xs) / (hw * sc), (f - c) / (ln * sc))
        fr = (1 + math.cos(th)) / 2
        s = (z - 0.002) / (hheel + (htoe - hheel) * fr ** 1.5)
    return s, th, fr


def hoof_point(hf, s, th):
    """(f, x) of the hoof wall at height fraction s and angle th."""
    fc, xs, hw, hl, htoe, hheel = hf
    s = min(s, 1.0)
    sc, c = 1.0 - 0.16 * s, fc - 0.056 * s
    fr = (1 + math.cos(th)) / 2
    ln = hl * (1.0 if math.cos(th) > 0 else 0.92)
    return c + math.cos(th) * ln * sc, xs + math.sin(th) * hw * sc * (1.0 - 0.06 * (1 - fr))


def shape_hooves(ob, src):
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    faces = [f for f in bm.faces if f.calc_center_median().z < 0.17 and hoof_frame(-f.calc_center_median().y, f.calc_center_median().x)]
    edges = list({e for f in faces for e in f.edges})
    bmesh.ops.subdivide_edges(bm, edges=edges, cuts=1, use_grid_fill=True, use_only_quads=False)
    bm.to_mesh(me)
    bm.free()
    g = ob.vertex_groups.new(name="hoof_refine")
    g.add([v.index for v in me.vertices if v.co.z < 0.18 and hoof_frame(-v.co.y, v.co.x)], 1.0, "REPLACE")
    activate(ob)
    sw = ob.modifiers.new("swf", "SHRINKWRAP")
    sw.target = src
    sw.wrap_method = "NEAREST_SURFACEPOINT"
    sw.vertex_group = g.name
    bpy.ops.object.modifier_apply(modifier=sw.name)
    ob.vertex_groups.remove(ob.vertex_groups[g.name])
    bm = bmesh.new()
    bm.from_mesh(me)
    snapped = set()
    cands = []
    for v in bm.verts:
        f, z, x = -v.co.y, v.co.z, v.co.x
        hf = hoof_frame(f, x)
        if hf is None or z > 0.16:
            continue
        s, th, fr = hoof_coords(f, z, x, hf)
        if 0.10 < s < 0.96:
            # the wall: back onto the hoof's own surface
            fh, xh = hoof_point(hf, s, th)
            v.co.y, v.co.x = -fh, xh
        zc = hf[5] + (hf[4] - hf[5]) * fr ** 1.5 + 0.002
        cands.append((abs(z - zc), v, hf, th, zc))
    # one row onto the coronet line: nearest first, never two neighbours
    for dz, v, hf, th, zc in sorted(cands, key=lambda t: t[0]):
        if dz > 0.0065 or any(e.other_vert(v) in snapped for e in v.link_edges):
            continue
        fh, xh = hoof_point(hf, 1.0, th)
        v.co.x, v.co.y, v.co.z = xh, -fh, zc
        snapped.add(v)
    # the coronet band: a low roll of skin just above the line
    for dz, v, hf, th, zc in cands:
        h = v.co.z - zc
        if v in snapped or not (0.0 < h < CORONET_BAND[0]):
            continue
        out_ = Vector((math.sin(th), -math.cos(th), 0.0))   # out from the hoof's axis (x, -f)
        v.co += out_ * (CORONET_BAND[1] * math.sin(math.pi * h / CORONET_BAND[0]))
    bm.to_mesh(me)
    bm.free()
    print("HORSE_HOOVES", len(faces), "faces refined,", len(snapped), "coronet points")


def hold_layout(ob, lay):
    """v4: the flank and stifle keep v3's surface (the walk's stifle fold was
    measured on it; the belly lift in front of it shortened its worst edge),
    blending into the v4 shape over 6 cm on every side."""
    n = 0
    for v in ob.data.vertices:
        f, z = -v.co.y, v.co.z
        w = _ss(-0.46, -0.40, f) * (1 - _ss(-0.14, -0.08, f)) * (1 - _ss(1.10, 1.16, z)) * _ss(0.70, 0.76, z)
        if w > 0.0:
            ok, hit, _, _ = lay.closest_point_on_mesh(v.co)
            if ok:
                v.co = v.co.lerp(hit, w)
                n += 1
    print("HORSE_HOLD_LAYOUT", n)


SOFTEN = 60   # v4: rounds of smoothing the breast-to-shoulder join on the quads


def soften_shoulder(ob, src):
    """v4: the point of the shoulder and the side of the breast read as a
    round ball in the three-quarter view (a convex mass ringed by creases at
    the neck and the arm). Smoothing the 6 mm surface reaches too little;
    on the 2 cm quads the same smoothing spreads over 10 cm and blends the
    mass into the neck, the shoulder and the arm. Kept clear of the elbow
    and armpit (f > 0.66) and the chest floor (z > 0.95), whose stretch the
    walk measures."""
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    wt = {}
    for v in bm.verts:
        f, z, ax = -v.co.y, v.co.z, abs(v.co.x)
        w = _ss(0.66, 0.74, f) * _ss(0.95, 1.02, z) * (1 - _ss(1.22, 1.34, z)) * _ss(0.03, 0.08, ax)
        if w > 0.01:
            wt[v] = w
    # Only the creases fill: a point moves toward its neighbours' middle
    # only when that is outward (a hollow), so the creases ringing the mass
    # at the neck and the arm round off while the point of the shoulder (the
    # canon's body length) keeps its place.
    for _ in range(SOFTEN):
        bm.normal_update()
        new = {}
        for v, w in wt.items():
            ns = [e.other_vert(v).co for e in v.link_edges]
            if ns:
                d = sum(ns, Vector()) / len(ns) - v.co
                if d.dot(v.normal) > 0.0:
                    new[v] = v.co + d * (0.5 * w)
        for v, c in new.items():
            v.co = c
    bm.to_mesh(ob.data)
    bm.free()
    print("HORSE_SOFTEN_SHOULDER", len(wt))


SOFTEN_BUTTOCK = 40   # v4: rounds of shrink-free smoothing behind the thigh


def soften_buttock(ob):
    """v4: the back of the thigh read lumpy from behind (the hamstring
    masses and their groove left small bumps on the 2 cm quads). Taubin
    smoothing (a shrinking step, then a slightly larger growing one) evens
    the bumps without shrinking the quarter, so the point of the buttock
    (the canon's body length) stays where it is."""
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    wt = {}
    for v in bm.verts:
        f, z, ax = -v.co.y, v.co.z, abs(v.co.x)
        w = (1 - _ss(-0.62, -0.54, f)) * _ss(0.84, 0.90, z) * (1 - _ss(1.00, 1.08, z)) * _ss(0.04, 0.08, ax)
        if w > 0.01:
            wt[v] = w
    for _ in range(SOFTEN_BUTTOCK):
        for k in (0.5, -0.53):
            new = {}
            for v, w in wt.items():
                ns = [e.other_vert(v).co for e in v.link_edges]
                if ns:
                    new[v] = v.co + (sum(ns, Vector()) / len(ns) - v.co) * (k * w)
            for v, c in new.items():
                v.co = c
    # higher, round the point of the buttock, only the hollows between the
    # bumps fill (a point never moves past its neighbours, so the point of
    # the buttock keeps its place)
    hi = {}
    for v in bm.verts:
        f, z, ax = -v.co.y, v.co.z, abs(v.co.x)
        w = (1 - _ss(-0.62, -0.54, f)) * _ss(0.96, 1.02, z) * (1 - _ss(1.28, 1.36, z)) * _ss(0.04, 0.08, ax)
        if w > 0.01:
            hi[v] = w
    for _ in range(SOFTEN_BUTTOCK):
        bm.normal_update()
        new = {}
        for v, w in hi.items():
            ns = [e.other_vert(v).co for e in v.link_edges]
            if ns:
                d = sum(ns, Vector()) / len(ns) - v.co
                if d.dot(v.normal) > 0.0:
                    new[v] = v.co + d * (0.5 * w)
        for v, c in new.items():
            v.co = c
    bm.to_mesh(ob.data)
    bm.free()
    print("HORSE_SOFTEN_BUTTOCK", len(wt), len(hi))


def in_head(c, amin):
    a, b, x = head_local(c)
    return c.z > 1.25 and a > amin and a > -0.03 + 0.325 * b and b < 0.42


def refine_head(ob, src):
    """The even quads (about 2 cm) lose the face: subdivide the head once
    and the muzzle (lips, nostrils, chin) twice, then project the new
    points back onto the detailed surface."""
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    for amin in (HEAD_REFINE[0], HEAD_REFINE[1], "eye", "eye2"):
        bm.faces.ensure_lookup_table()
        if amin in ("eye", "eye2"):
            # v3: the eyes once more (about 5 mm quads), for a clean almond
            # and lid margins; v4: and the eye and its lids once again (about
            # 2.5 mm), so a lid is a rounded roll, not one raised vertex
            cs = [eye_frame(sx)[0] for sx in (1, -1)]
            rr = 0.055 if amin == "eye" else EYE_FINE
            faces = [f for f in bm.faces if min((f.calc_center_median() - c).length for c in cs) < rr]
        else:
            faces = [f for f in bm.faces if in_head(f.calc_center_median(), amin) or (amin < 0.2 and in_ear(f.calc_center_median()))]
        edges = list({e for f in faces for e in f.edges})
        bmesh.ops.subdivide_edges(bm, edges=edges, cuts=1, use_grid_fill=True, use_only_quads=False)
    bm.to_mesh(me)
    bm.free()
    g = ob.vertex_groups.new(name="head_refine")
    g.add([v.index for v in me.vertices if in_head(v.co, HEAD_REFINE[0] - 0.05) or in_ear(v.co)], 1.0, "REPLACE")
    activate(ob)
    sw = ob.modifiers.new("swh", "SHRINKWRAP")
    sw.target = src
    sw.wrap_method = "NEAREST_SURFACEPOINT"
    sw.vertex_group = g.name
    bpy.ops.object.modifier_apply(modifier=sw.name)
    gg = ob.vertex_groups.get("head_refine")
    if gg is not None:
        ob.vertex_groups.remove(gg)
    print("HORSE_HEAD_REFINE", len(ob.data.polygons))


HEAD_REFINE = (-0.02, 0.50)   # head (a past the poll) once, the muzzle (past a = 0.50) twice


def untangle(ob, rounds=24):
    """Projection back onto the surface can fold a few quads through each
    other in a tight crease (groin, throat): relax just those, in place,
    until no two faces that share no vertex cross."""
    from mathutils.bvhtree import BVHTree
    for r in range(rounds):
        bm = bmesh.new()
        bm.from_mesh(ob.data)
        bm.faces.ensure_lookup_table()
        t = BVHTree.FromBMesh(bm)
        bad = set()
        for a, b in t.overlap(t):
            if a < b and not {v.index for v in bm.faces[a].verts} & {v.index for v in bm.faces[b].verts}:
                bad.update((a, b))
        print("HORSE_UNTANGLE", r, len(bad), [tuple(round(c, 3) for c in (-bm.faces[i].calc_center_median().y, bm.faces[i].calc_center_median().z, bm.faces[i].calc_center_median().x)) for i in list(bad)[:6]] if r == rounds - 1 else "")
        if not bad:
            bm.free()
            return
        vs = {v for i in bad for v in bm.faces[i].verts}
        for _ in range(1 + r // 4):     # widen the relaxed patch if it keeps failing
            vs |= {n for v in list(vs) for e in v.link_edges for n in e.verts}
        for _ in range(4):
            new = {v: sum((e.other_vert(v).co for e in v.link_edges), Vector()) / max(len(v.link_edges), 1) for v in vs}
            for v, c in new.items():
                v.co = v.co.lerp(c, 0.6)
        bm.to_mesh(ob.data)
        bm.free()
    raise RuntimeError("untangle: faces still cross after %d rounds" % rounds)


# ------------------------------------------------------------ regions, UVs
FORE_TOP, HIND_TOP = 0.88, 0.90


def seg_dist(p, a, b):
    ab = b - a
    t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.dot(ab), 1e-12)))
    return (p - (a + ab * t)).length


def on_dock(c):
    pts = [P(f, z) for f, z, r in DOCK]
    best = 9.0
    for (a, b), (_, _, r) in zip(zip(pts, pts[1:]), DOCK):
        best = min(best, seg_dist(c, a, b) - r)
    return best < 0.025


def on_hoof(c):
    """v4: horn below the coronet line of the hoof's own shape (hoof_coords:
    s under 1), the same line shape_hooves snaps a row of points onto."""
    f, z, x = -c.y, c.z, c.x
    hf = hoof_frame(f, x)
    if hf is None or z > 0.16:
        return None
    if hoof_coords(f, z, x, hf)[0] < 1.0:
        return "F" if hf[0] > 0 else "H"
    return None


def face_region(c):
    f, z, x = -c.y, c.z, c.x
    side = "L" if x >= 0 else "R"
    # head: past the plane from behind the poll to the throatlatch
    a, b, _ = head_local(c)
    if z > 1.5 and a > -0.03 + 0.325 * b:
        if in_ear(c) and b < 0.0:
            return "ear_" + side
        return "head"
    if f < -0.76 and on_dock(c):
        return "tail"
    h = on_hoof(c)
    if h:
        return "hoof_" + h + side
    if f > 0.3 and z < FORE_TOP:
        return "leg_F" + side
    if f < -0.2 and z < HIND_TOP and (f < -0.40 or z < 0.86):
        return "leg_H" + side
    return "body_" + side


FLOW = {
    "body": Vector((0, 1, -0.55)),
    "leg": Vector((0, 0, -1)),
    "hoof": Vector((0, 0, -1)),
    "tail": Vector((0, 0.3, -1)),
    "ear": Vector((0, 0, 1)),
    "head": None,  # along the face, poll to muzzle
}


def flow_of(reg):
    k = reg.split("_")[0]
    if k == "head":
        return (P(*_hd) - P(0, 0)).normalized()
    return FLOW[k].normalized()


def uv_and_materials(ob):
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    reg = {f.index: face_region(f.calc_center_median()) for f in bm.faces}
    # hooves get their own material; tiny islands of a region inside another are merged
    for _ in range(3):
        for f in bm.faces:
            ns = [reg[g.index] for e in f.edges for g in e.link_faces if g is not f]
            if ns and all(n != reg[f.index] for n in ns):
                reg[f.index] = max(set(ns), key=ns.count)
    for f in bm.faces:
        r = reg[f.index]
        f.material_index = 1 if r.startswith("hoof") else 0
    # seams: region borders, plus a parting inside each region
    for e in bm.edges:
        e.seam = False
        lf = e.link_faces
        if len(lf) != 2:
            continue
        r0, r1 = reg[lf[0].index], reg[lf[1].index]
        if r0 != r1:
            # body_L | body_R is the topline and belly parting
            e.seam = True
            continue
        c0, c1 = lf[0].calc_center_median(), lf[1].calc_center_median()
        m = (e.verts[0].co + e.verts[1].co) / 2
        if r0.startswith("leg") or r0.startswith("tail") or r0.startswith("ear"):
            # down the back of the limb: where the faces cross its centre plane
            cen = region_centre(bm, reg, r0, m.z)
            if r0.startswith("ear"):
                cen = None
            if cen is not None:
                behind = (-m.y) < cen[1] - 0.005
                if behind and (c0.x - cen[0]) * (c1.x - cen[0]) < 0:
                    e.seam = True
            else:
                a0 = (c0 - EAR_C[r0]).dot(EAR_F[r0])
                if a0 < 0 and (c0 - EAR_C[r0]).dot(EAR_S[r0]) * (c1 - EAR_C[r0]).dot(EAR_S[r0]) < 0:
                    e.seam = True
        elif r0 == "head":
            a, b, x = head_local(m)
            if b > 0.09 and c0.x * c1.x < 0:
                e.seam = True
    bm.to_mesh(me)
    bm.free()
    activate(ob)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.unwrap(method="CONFORMAL", margin=0.002)
    bpy.ops.uv.average_islands_scale()
    bpy.ops.object.mode_set(mode="OBJECT")
    orient_islands(ob, reg)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.select_all(action="SELECT")
    bpy.ops.uv.pack_islands(rotate=False, scale=True, margin=0.004)
    bpy.ops.object.mode_set(mode="OBJECT")
    return reg


_CEN = {}
EAR_C, EAR_F, EAR_S = {}, {}, {}


def region_centre(bm, reg, r, z):
    if r not in _CEN:
        bands = {}
        for f in bm.faces:
            if reg[f.index] != r:
                continue
            c = f.calc_center_median()
            k = int(c.z / 0.02)
            bands.setdefault(k, []).append(c)
        _CEN[r] = {k: (sum(p.x for p in v) / len(v), sum(-p.y for p in v) / len(v)) for k, v in bands.items()}
    k = int(z / 0.02)
    t = _CEN[r]
    for d in (0, 1, -1, 2, -2):
        if k + d in t:
            return t[k + d]
    return None


def setup_ears():
    for sx, side in ((1, "L"), (-1, "R")):
        base, up, fwd, side_ = ear_frame(sx)
        r = "ear_" + side
        EAR_C[r] = base + up * 0.08
        EAR_F[r] = fwd
        EAR_S[r] = up.cross(fwd).normalized()


def orient_islands(ob, reg):
    """Turn each UV island so +v runs the way the hair flows on it."""
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    uvl = bm.loops.layers.uv.active
    bm.faces.ensure_lookup_table()
    seen = set()
    for f0 in bm.faces:
        if f0.index in seen:
            continue
        island, stack = [], [f0]
        seen.add(f0.index)
        while stack:
            f = stack.pop()
            island.append(f)
            for e in f.edges:
                if e.seam:
                    continue
                for g in e.link_faces:
                    if g.index not in seen:
                        seen.add(g.index)
                        stack.append(g)
        acc = Vector((0.0, 0.0))
        for f in island:
            F = flow_of(reg[f.index])
            l = f.loops
            p0, p1, p2 = l[0].vert.co, l[1].vert.co, l[2].vert.co
            u0, u1, u2 = l[0][uvl].uv, l[1][uvl].uv, l[2][uvl].uv
            e1, e2 = p1 - p0, p2 - p0
            n = e1.cross(e2)
            if n.length < 1e-12:
                continue
            Fp = F - n.normalized() * F.dot(n.normalized())
            # solve Fp = s*e1 + t*e2 in the face plane, map to uv
            m11, m12, m22 = e1.dot(e1), e1.dot(e2), e2.dot(e2)
            r1, r2 = Fp.dot(e1), Fp.dot(e2)
            det = m11 * m22 - m12 * m12
            if abs(det) < 1e-14:
                continue
            s = (r1 * m22 - r2 * m12) / det
            t = (r2 * m11 - r1 * m12) / det
            acc += (u1 - u0) * s + (u2 - u0) * t
        if acc.length < 1e-9:
            continue
        ang = math.atan2(acc.x, acc.y)   # rotate acc onto +v
        c, s = math.cos(ang), math.sin(ang)
        cen = Vector((0.0, 0.0))
        k = 0
        for f in island:
            for l in f.loops:
                cen += l[uvl].uv
                k += 1
        cen /= max(k, 1)
        for f in island:
            for l in f.loops:
                d = l[uvl].uv - cen
                l[uvl].uv = cen + Vector((d.x * c - d.y * s, d.x * s + d.y * c))
    bm.to_mesh(me)
    bm.free()


# ------------------------------------------------------------ eyes (v3)
# v2's eye was a black ball in a round pit ("a dented black blob"). v3 shapes
# the eye into the skin on the final quads: an almond opening (5.8 by 3 cm,
# the back corner a little higher), the eye inside it a low glossy dome of
# its own material, upper and lower lids as raised margins round it (the
# upper fuller) and the fold above the upper lid. One watertight surface: no
# second shell to cross the skin.
EYE_A = 0.029            # half length of the almond
EYE_UP, EYE_LOW = 0.0175, 0.0155
EYE_TILT = 0.003         # the back corner this much higher than the front
EYE_PU, EYE_PL = 0.62, 0.70  # the lids' curves (0.5: an ellipse; higher: sharper corners)
EYE_DOME = 0.0120        # the eye's bulge at its middle (v4: 6.5 -> 12 mm, a rounder eyeball that catches the light)
EYE_SINK = 0.0030        # v4: how far the eyeball sits in under the lid margins
LID_IN = 0.0028           # v4: how far the lid's edge rolls in over the eyeball
EYE_FINE = 0.042         # v4: radius of the eye's second refinement (about 2.5 mm quads)
LID_W = 0.0085           # width of a lid margin (v4: 7.5 -> 8.5 mm)
LID_UP, LID_LOW = 0.0095, 0.0070
EYE_ZONE = 0.060         # v4: the patch reshaped round each eye (its last centimetre held)
EYE_RELAX = 8            # v4: rounds of relaxing in the eye's plane
EYE_SOFT = 6             # v4: rounds of smoothing the brow and cheek round the lids


def eye_frame(sx):
    """Centre (on the dense surface of the build), outward, long axis (to
    the back corner) and up of the eye on side sx."""
    c = EYE_CENTRES.get(sx)
    out = Vector((sx * 0.88, -0.40, 0.25)).normalized()
    u = Vector((0.0, 1.0, 0.15))
    u = (u - out * u.dot(out)).normalized()
    v = out.cross(u).normalized()
    if v.z < 0:
        v = -v
    if c is None:
        c = HV(EYE_AB[0], EYE_AB[1], sx * 0.106)
    return c, out, u, v


EYE_CENTRES = {}
EYE_VERTS = set()   # the eye's vertices (inside the almond and its outline)


def _almond(s, t):
    """Signed distance outside the almond (approximate, metres), and which
    lid (True: upper) the point faces."""
    c = max(-EYE_A, min(EYE_A, s))
    k = max(0.0, 1.0 - (c / EYE_A) ** 2)
    tc = EYE_TILT * c / EYE_A
    up = t >= tc
    edge = tc + (EYE_UP * k ** EYE_PU if up else -EYE_LOW * k ** EYE_PL)
    d = (t - edge) if up else (edge - t)
    if abs(s) > EYE_A:
        d = max(d, abs(s) - EYE_A)
    return d, up, edge, c


def shape_eyes(ob):
    """v4: the almond's boundary ring snapped onto its outline as in v3, then
    the eye and the lids relaxed in the plane of the eye (so the snap leaves
    no puckers), then lifted by one smooth profile: a dark dome set inside
    the lids, the lid margins standing over it, a soft swelling round them
    and a smooth brow (v3's fold above the upper lid is gone: with the
    snapped ring it creased the skin)."""
    me = ob.data
    for sx in (1, -1):
        c0 = HV(EYE_AB[0], EYE_AB[1], sx * 0.20)
        ok, hit, nrm, _ = ob.closest_point_on_mesh(c0)
        EYE_CENTRES[sx] = hit.copy()
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    for sx in (1, -1):
        c, out, u, v = eye_frame(sx)
        near = [p for p in bm.verts if (p.co - c).length < EYE_ZONE and (p.co - c).dot(out) > -0.03]
        loc = {p: [(p.co - c).dot(u), (p.co - c).dot(v), (p.co - c).dot(out)] for p in near}
        inside = {p for p in near if _almond(loc[p][0], loc[p][1])[0] < 0}
        # the boundary ring: outside vertices of faces that have an inside one
        ring = set()
        for p in inside:
            for f in p.link_faces:
                for q in f.verts:
                    if q not in inside and q in loc:
                        ring.add(q)
        # v4: each ring point goes onto the outline straight out from the
        # eye's centre (v3 took the nearest outline point, which bunched
        # neighbours onto one spot and wrinkled the lid margin)
        for p in ring:
            s0, t0 = loc[p][0], loc[p][1]
            lo, hi = 0.0, 1.0
            while _almond(s0 * hi, t0 * hi)[0] < 0:
                hi *= 1.5
            for _ in range(30):
                mid = 0.5 * (lo + hi)
                if _almond(s0 * mid, t0 * mid)[0] < 0:
                    lo = mid
                else:
                    hi = mid
            loc[p][0], loc[p][1] = s0 * hi, t0 * hi
        # relax in the eye's plane: the eye inside the fixed ring, and the lid
        # and brow zone between the ring and the zone's edge (fixed)
        held = {p for p in near if (p.co - c).length > EYE_ZONE - 0.010}
        free = [p for p in near if p not in ring and p not in held]
        for _ in range(EYE_RELAX):
            new = {}
            for p in free:
                ns = [e.other_vert(p) for e in p.link_edges if e.other_vert(p) in loc]
                if not ns:
                    continue
                ms = sum(loc[q][0] for q in ns) / len(ns)
                mt = sum(loc[q][1] for q in ns) / len(ns)
                mw = sum(loc[q][2] for q in ns) / len(ns)
                # keep each side of the ring on its own side
                if (p in inside) == (_almond(ms, mt)[0] < 0):
                    new[p] = (loc[p][0] * 0.4 + ms * 0.6, loc[p][1] * 0.4 + mt * 0.6, loc[p][2] * 0.6 + mw * 0.4)
            for p, q in new.items():
                loc[p][0], loc[p][1], loc[p][2] = q
        for p in near:
            s_, t_, w_ = loc[p]
            d, up, edge, cc = _almond(s_, t_)
            taper = max(0.0, 1.0 - (cc / EYE_A) ** 2) ** 0.6
            if p in ring:
                lift = (LID_UP if up else LID_LOW) * max(taper, 0.30)
                EYE_VERTS.add(p.index)
            elif p in inside:
                e = (s_ / EYE_A) ** 2 + ((t_ - EYE_TILT * s_ / EYE_A) / (EYE_UP if up else EYE_LOW)) ** 2
                lift = EYE_DOME * math.sqrt(max(0.0, 1.0 - e)) - EYE_SINK
                # the lid's inner edge rolls over the eyeball (the dark lid
                # margin), so the lid ends in a rounded edge, not a step
                hl = (LID_UP if up else LID_LOW) * max(taper, 0.30)
                lift = max(lift, hl * math.cos(0.5 * math.pi * min(-d / LID_IN, 1.0)) ** 2)
                EYE_VERTS.add(p.index)
            else:
                # the lid: a firm rim at the margin on a soft swelling that
                # fades smoothly into the brow and cheek
                h = (LID_UP if up else LID_LOW) * max(taper, 0.30 * max(0.0, 1.0 - d / LID_W))   # the ring's corners fade out
                # a rounded roll: full height at the margin, easing down
                # over the lid's width, then a soft swelling into the brow
                rim = math.cos(0.5 * math.pi * min(d / LID_W, 1.0)) ** 1.2
                swell = _ss(LID_W * 3.4, 0.0, d)
                lift = h * (0.62 * rim + 0.38 * swell)
            p.co = c + u * s_ + v * t_ + out * (w_ + lift)
        # the brow and cheek round the lids: smoothed in place, so the
        # subdivided patch shows no creases (the lid margins stay sharp)
        soft = [p for p in near if p not in ring and p not in inside and p not in held and _almond(loc[p][0], loc[p][1])[0] > LID_W * 0.9]
        for _ in range(EYE_SOFT):
            bmesh.ops.smooth_vert(bm, verts=soft, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    bm.to_mesh(me)
    bm.free()
    print("HORSE_EYES", {k: [round(x, 3) for x in v_] for k, v_ in EYE_CENTRES.items()})


def mark_eyes(ob):
    """The faces inside each almond take the eye material (glossy, dark)."""
    me = ob.data
    n = 0
    for sx in (1, -1):
        c, out, u, v = eye_frame(sx)
        for p in me.polygons:
            if (p.center - c).length < 0.045 and all(i in EYE_VERTS for i in p.vertices):
                p.material_index = 2
                n += 1
    print("HORSE_EYE_FACES", n)
    return ob


def materials(ob):
    def mat(name, rgb, rough, coat=0.0):
        m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        m.use_nodes = True
        b = m.node_tree.nodes.get("Principled BSDF")
        b.inputs["Base Color"].default_value = (*rgb, 1)
        b.inputs["Roughness"].default_value = rough
        if coat and "Coat Weight" in b.inputs:
            b.inputs["Coat Weight"].default_value = coat       # the wet cornea
            b.inputs["Coat Roughness"].default_value = 0.03
        m.diffuse_color = (*rgb, 1)
        # v4: one-sided (glTF doubleSided false). The horse is one closed
        # shell, so its inside is never meant to be seen; v3 exported
        # double-sided, so where the walk folds the skin through itself (the
        # flank, walk frames 0, 12 and 28) the engine could draw the fold's
        # inside faces. (Not the cause of the pale patch under the belly:
        # that is open-sky light on the hidden inside of the far thigh; see
        # DECISIONS, test horse v4.)
        m.use_backface_culling = True
        return m
    ob.data.materials.clear()
    ob.data.materials.append(mat("coat", (0.23, 0.10, 0.045), 0.55))
    ob.data.materials.append(mat("hoof", (0.035, 0.03, 0.028), 0.45))
    ob.data.materials.append(mat("eye", (0.022, 0.013, 0.008), 0.06, coat=1.0))


# ------------------------------------------------------------ renders
def render_views(views, prefix=""):
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_WORKBENCH" if ARGS.engine == "WORKBENCH" else "BLENDER_EEVEE"
    w = bpy.data.worlds.get("World") or bpy.data.worlds.new("World")
    sc.world = w
    w.color = (0.30, 0.33, 0.37)
    sc.render.resolution_x, sc.render.resolution_y = 1400, 1000
    sc.render.film_transparent = False
    if ARGS.engine == "WORKBENCH":
        sh = sc.display.shading
        sh.light = "STUDIO"
        sh.color_type = "MATERIAL"
        sh.show_cavity = True
        sh.cavity_type = "BOTH"
        sh.show_specular_highlight = True
    else:
        world = bpy.data.worlds.get("World") or bpy.data.worlds.new("World")
        sc.world = world
        world.use_nodes = True
        world.node_tree.nodes["Background"].inputs[0].default_value = (0.55, 0.62, 0.72, 1)
        world.node_tree.nodes["Background"].inputs[1].default_value = 0.8
        sun = bpy.data.objects.get("sun") or bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
        if sun.name not in sc.collection.objects:
            sc.collection.objects.link(sun)
        sun.data.energy = 3.5
        sun.rotation_euler = Euler((math.radians(50), 0, math.radians(-35)))
    cam = bpy.data.objects.get("cam") or bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    if cam.name not in sc.collection.objects:
        sc.collection.objects.link(cam)
    sc.camera = cam
    shots = {
        "side": (P(0.15, 1.05, 6.2), P(0.15, 1.05, 0), 32),
        "front": (P(5.5, 1.15, 0.0), P(0, 1.10, 0), 32),
        "threeq": (P(3.6, 1.6, 3.9), P(0.1, 1.0, 0), 34),
        "head": (P(2.15, 1.95, 1.05), P(1.30, 1.70, 0.0), 30),
        "headside": (P(1.30, 1.70, 2.0), P(1.30, 1.70, 0.0), 30),
        "headfront": (P(2.70, 1.80, 0.0), P(1.30, 1.70, 0.0), 22),
        "muzzle": (P(2.05, 1.62, 0.55), P(1.52, 1.50, 0.0), 26),
        "muzzleside": (P(1.50, 1.50, 1.10), P(1.50, 1.50, 0.0), 26),
        "ear": (P(1.75, 2.12, 0.75), P(1.26, 2.03, 0.0), 22),
        "under": (P(1.6, 0.9, 0.0), P(1.25, 1.62, 0.0), 30),
        "back": (P(-5.5, 1.2, 0.0), P(0, 1.1, 0), 32),
        "top": (P(0.1, 7.0, 0.01), P(0.1, 0.0, 0), 32),
        "legs": (P(0.0, 0.5, 3.4), P(0.0, 0.45, 0), 34),
    }
    for v in views:
        if v not in shots:
            continue
        eye, tgt, fov = shots[v]
        cam.location = eye
        d = (tgt - eye).normalized()
        cam.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
        cam.data.angle = math.radians(fov)
        sc.render.filepath = os.path.join(OUT, prefix + v + ".png")
        bpy.ops.render.render(write_still=True)


def report(ob):
    me = ob.data
    tris = sum(len(p.vertices) - 2 for p in me.polygons)
    quads = sum(1 for p in me.polygons if len(p.vertices) == 4)
    print("HORSE_MESH", json.dumps({"verts": len(me.vertices), "faces": len(me.polygons), "quads": quads, "tris": tris,
                                     "dims": [round(x, 3) for x in ob.dimensions]}))


def bake_only():
    """--stage ao --blend <horse.blend>: bake the occlusion onto an already
    built horse and export it again (the shape, rig and clips untouched)."""
    import rig_horse
    rig_horse.H = sys.modules[__name__]
    bpy.ops.wm.open_mainfile(filepath=os.path.abspath(ARGS.blend))
    rig = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    ob = next(o for o in bpy.data.objects if o.type == "MESH" and any(m.type == "ARMATURE" for m in o.modifiers))
    if rig.animation_data:
        rig.animation_data.action = None
    rig_horse.bake_ao(ob, rig)
    rig_horse.export(ob, rig, OUT)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, "horse.blend"))


def main():
    if ARGS.stage == "ao":
        return bake_only()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    setup_ears()
    src = build_surface()
    lay = None if ARGS.stage == "surface" else build_surface(layout=True)
    if ARGS.stage == "surface":
        # a quick look at the shape: the dense surface only
        src.name = "horse"
        materials(src)
        for p in src.data.polygons:
            p.use_smooth = True
        report(src)
        bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, "horse.blend"))
        if ARGS.views:
            render_views([v for v in ARGS.views.split(",") if v])
        return
    ob = quad_remesh(src, ARGS.faces, lay)
    lay.hide_render = True
    lay.hide_viewport = True
    for p in ob.data.polygons:
        p.use_smooth = True
    materials(ob)
    uv_and_materials(ob)
    mark_eyes(ob)     # v3: the eyes are skin with their own material (weighted like the lids round them)
    rig = None
    if ARGS.stage != "mesh":
        import rig_horse
        rig_horse.H = sys.modules[__name__]
        rig = rig_horse.run(ob, ARGS, OUT)
    src.hide_render = True
    src.hide_viewport = True
    report(ob)
    if rig is not None:
        rig_horse.finish(ob, rig, ARGS, OUT)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, "horse.blend"))
    if ARGS.views:
        render_views([v for v in ARGS.views.split(",") if v])


sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
if __name__ == "__main__":
    main()
