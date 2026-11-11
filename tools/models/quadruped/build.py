# Evani.Dev quadruped generator (original work, built from numbers only: no
# downloaded meshes, no outside libraries; Blender itself and its bundled
# Rigify horse metarig are the only tools). One script makes every
# four-legged animal from a species file (species/<id>.json): the cow and the
# dog now, horse-like, sheep, pig, goat, cat and deer later. README.md lists
# every parameter.
#
# Run (pinned Blender 5.2.2, headless):
#   blender -b --factory-startup -t 2 --python tools/models/quadruped/build.py -- \
#       --species cow --out <folder> [--stage surface|mesh|all] [--views side,front,threeq]
#       [--faces N] [--ao]
# Writes <folder>/<id>.blend, <folder>/<id>_studio.glb (stage mesh and all: the
# rest pose, unskinned, for the studio's viewer), <folder>/<id>.glb (stage all:
# rigged, walk and idle clips), both with `_EVANI_PART` and `_EVANI_COAT` per
# vertex, <folder>/<id>.report.json
# and <folder>/<view>.png for each view asked. Then:
#   cargo xtask convert <folder>/<id>_studio.glb --id <id> --out apps/material-view/assets/models
#   cargo xtask convert <folder>/<id>.glb --id <id> --out tests/fixtures/models/<id>
#
# How it is made:
#  1. Parts (body.py): closed lofts of cross-sections for the trunk, neck and
#     head; legs lofted along their centre lines; ears, horns, tail, feet,
#     udder, dewlap, mane and muscle masses.
#  2. Voxel union (one watertight surface), an SDF fillet at the joins,
#     smoothing (less on the ears, face and lower legs), the face carved
#     (nostrils, mouth line).
#  3. QuadriFlow: even all-quad topology, symmetric about the midline,
#     projected back on the dense surface, crossings relaxed.
#  4. Eyes as separate closed parts set into the head.
#  5. Regions: the asset row's region ids (catalog/assets/assets.json), one
#     per vertex in `_EVANI_PART`, by the species' rules; the coat chart
#     `_EVANI_COAT` (face, belly, lower legs, patches) for coats and voxel
#     colour per body part.
#  6. Rig (rig.py): the Rigify horse metarig fitted to the species' joints,
#     automatic weights cleaned, walk and idle clips, baked occlusion.
import bpy, bmesh, sys, os, math, json, argparse
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, HERE)
from geom import P, X, pi, smoothstep, poly_dist  # noqa: E402
import body  # noqa: E402
import glbfix  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ap = argparse.ArgumentParser()
ap.add_argument("--species", required=True)
ap.add_argument("--out", required=True)
ap.add_argument("--stage", default="all")
ap.add_argument("--views", default="")
ap.add_argument("--faces", type=int, default=0)
ap.add_argument("--voxel", type=float, default=0.0)   # a coarser voxel for a quick look at the shape
ap.add_argument("--ao", action="store_true")   # bake occlusion into `_EVANI_AO` (the studio converter does not carry it yet)
ap.add_argument("--ao-samples", type=int, default=64)
ARGS = ap.parse_args(argv)
OUT = os.path.abspath(ARGS.out)
os.makedirs(OUT, exist_ok=True)

with open(os.path.join(HERE, "species", ARGS.species + ".json"), encoding="utf-8") as f:
    S = json.load(f)
ID = S["id"]
if ARGS.voxel:
    S["voxel"] = ARGS.voxel
B = body.Body(S)
with open(os.path.join(ROOT, "catalog", "assets", "assets.json"), encoding="utf-8") as f:
    ROW = next(a for a in json.load(f)["assets"] if a["id"] == S["asset"])
REGION_IDS = [r["id"] for r in ROW["regions"]]
for rule in S["regions"]:
    if rule["id"] not in REGION_IDS:
        raise SystemExit("species %s: region %r is not in %s's regions %s" % (ID, rule["id"], S["asset"], REGION_IDS))


import time
T0 = time.time()


def log(tag, data):
    print("QUAD_%s %s t=%.0fs" % (tag, json.dumps(data), time.time() - T0))


# ------------------------------------------------------------ objects
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


# ------------------------------------------------------------ where a point is
class Lines:
    """The ears', horns' and tail's centre lines, worked out once, each with
    a bounding sphere for a quick reject."""

    def __init__(self):
        e = S["ears"]
        self.ears = []
        for sx in (1, -1):
            line = [B.ear_centre(sx, i / 16) for i in range(17)]
            self.ears.append((line,) + self._sphere(line, e["width"] * 1.2 + 0.03))
        self.horns = []
        if S.get("horns"):
            for sx in (1, -1):
                line = B.horn_line(sx)
                length = sum((q - p).length for p, q in zip(line, line[1:]))
                rads = [B.horn_radius(i / (len(line) - 1)) for i in range(len(line))]
                self.horns.append((line, length, rads) + self._sphere(line, max(rads) * 1.3 + 0.03))
        tl = B.tail_line()
        self.tail = [c for c, _ in tl]
        self.tail_r = [r for _, r in tl]
        self.tail_len = sum((q - p).length for p, q in zip(self.tail, self.tail[1:]))
        self.tail_c, self.tail_R = self._sphere(self.tail, max(self.tail_r) * 1.4 + 0.03)

    @staticmethod
    def _sphere(pts, pad):
        c = sum(pts, Vector()) / len(pts)
        return c, max((p - c).length for p in pts) + pad


LN = None


def lines():
    global LN
    if LN is None:
        LN = Lines()
    return LN


def ear_t(co, pad=0.01):
    """(t along the ear 0..1, inside?) for the nearer ear."""
    e = S["ears"]
    best = (-1.0, False)
    for line, c, R in lines().ears:
        if (co - c).length > R:
            continue
        d, t = poly_dist(co, line)
        w = e["width"] * (0.75 + 0.3 * (1 - t)) + pad
        if d < w and t > best[0]:
            best = (t, True)
    return best


def horn_along(co, pad=0.008):
    best = -1.0
    for line, length, rads, c, R in lines().horns:
        if (co - c).length > R:
            continue
        d, t = poly_dist(co, line)
        r = rads[min(int(round(t * (len(rads) - 1))), len(rads) - 1)]
        if d < r * 1.25 + pad:
            best = max(best, t * length)
    return best


def tail_along(co, pad=0.012):
    L = lines()
    if (co - L.tail_c).length > L.tail_R:
        return -1.0
    d, t = poly_dist(co, L.tail)
    i = min(int(t * (len(L.tail_r) - 1)), len(L.tail_r) - 1)
    if d < L.tail_r[i] * 1.3 + pad:
        return t * L.tail_len
    return -1.0


def in_hoof(co):
    F = S["feet"]
    if F["type"] == "paw":
        return False
    for leg in ("fore", "hind"):
        ft = F[leg]
        for sx in (1, -1):
            f, x = -co.y, co.x
            zz = co.z
            if zz > ft["toe"] + 0.006:
                continue
            fc = ft["f"] - ft.get("slant", 0.35) * zz
            u = (f - fc) / ft["length"]
            w = (x - sx * ft["x"]) / ft["width"]
            if u * u + w * w > 1.45 ** 2:
                continue
            fr = min(max((u + 1) / 2, 0.0), 1.0)
            top = ft["heel"] + (ft["toe"] - ft["heel"]) * fr ** 1.5
            if zz < top + 0.004:
                return True
    return False


def in_head_box(co, r):
    a, b, x = B.head_local(co)
    return r["a"][0] <= a <= r["a"][1] and r["b"][0] <= b <= r["b"][1] and abs(x) <= r.get("x", 1.0)


def in_udder(co):
    u = S.get("udder")
    if not u:
        return False
    f, z, x = -co.y, co.z, co.x
    return abs(x) < u["r"][2] * 1.05 and abs(f - u["f"]) < u["r"][0] * 1.15 and z < u["z"] + u["r"][1] * 0.75


def leg_side(co):
    """('fore'|'hind', distal 0..1) if the point is on a free leg."""
    f, z = -co.y, co.z
    jf, jh = B.joints("fore"), B.joints("hind")
    mid = (jf["elbow"][0] + jh["stifle"][0]) / 2
    leg = "fore" if f > mid else "hind"
    j = jf if leg == "fore" else jh
    xl = B.leg_x(leg, z)
    if abs(abs(co.x) - xl) > 0.6 * (abs(jf["shoulder"][0] - jf["elbow"][0]) + 0.1):
        return None
    top = j["wrist"][1] if leg == "fore" else j["hock"][1]
    return leg, 1.0 - smoothstep(j["fetlock"][1], top, z)


SEPARATE_RULES = ("eyes", "ears", "horns", "hooves")


def rule_region(kind):
    return next((r["id"] for r in S["regions"] if r["rule"] == kind), REGION_IDS[0])


def region_of(co, part=None):
    """The region of a skin vertex by the species' rules (first match), or
    of a separate part's vertex by its part."""
    if part is not None:
        return rule_region(part)
    for r in S["regions"]:
        k = r["rule"]
        if k in SEPARATE_RULES:
            continue
        if k == "horns" and horn_along(co) > r.get("from", 0.0):
            return r["id"]
        if k == "ears":
            t, inside = ear_t(co, r.get("pad", 0.01))
            if inside and t > r.get("from", 0.1):
                return r["id"]
        if k == "tail" and tail_along(co, r.get("pad", 0.012)) > r.get("from", 0.0):
            return r["id"]
        if k == "head_box" and in_head_box(co, r):
            return r["id"]
        if k == "udder" and in_udder(co):
            return r["id"]
        if k == "legs":
            f = -co.y
            mid = (B.joints("fore")["elbow"][0] + B.joints("hind")["stifle"][0]) / 2
            top = r["fore_top"] if f > mid else r["hind_top"]
            if co.z < top and abs(co.x) > 0.25 * B.leg_x("fore" if f > mid else "hind", co.z):
                return r["id"]
        if k == "rest":
            return r["id"]
    return REGION_IDS[0]


# ------------------------------------------------------------ surface
def _poly_dist_np(pts, line):
    """Distances from every point (N x 3) to a polyline, and the fraction
    along it (numpy, for the dense surface)."""
    import numpy as np
    L = np.array([tuple(p) for p in line])
    A, Bq = L[:-1], L[1:]
    AB = Bq - A
    seg = np.linalg.norm(AB, axis=1)
    cum = np.concatenate([[0.0], np.cumsum(seg)])
    best = np.full(len(pts), 1e9)
    frac = np.zeros(len(pts))
    for i in range(len(A)):
        t = np.clip(((pts - A[i]) @ AB[i]) / max(AB[i] @ AB[i], 1e-12), 0.0, 1.0)
        d = np.linalg.norm(pts - (A[i] + t[:, None] * AB[i]), axis=1)
        m = d < best
        best[m] = d[m]
        frac[m] = (cum[i] + t[m] * seg[i]) / max(cum[-1], 1e-9)
    return best, frac


def protect_weights(ob, name="smoothable"):
    """Smooth everywhere but the ears, horns, tail, the face's front and
    the lower legs and feet (numpy: the dense surface has 10^5 points)."""
    import numpy as np
    g = ob.vertex_groups.get(name) or ob.vertex_groups.new(name=name)
    me = ob.data
    n = len(me.vertices)
    co = np.empty(n * 3)
    me.vertices.foreach_get("co", co)
    co = co.reshape(n, 3)
    w = np.ones(n)
    e = S["ears"]
    Ls = lines()
    for line, c, R in Ls.ears:
        d, t = _poly_dist_np(co, line)
        w[(d < e["width"] * (0.75 + 0.3 * (1 - t)) + 0.004) & (t > 0.05)] = 0.08
    for line, length, rads, c, R in Ls.horns:
        d, t = _poly_dist_np(co, line)
        w[d < max(rads) * 1.25 + 0.004] = np.minimum(w[d < max(rads) * 1.25 + 0.004], 0.15)
    d, t = _poly_dist_np(co, Ls.tail)
    m = (d < max(Ls.tail_r) * 1.3 + 0.004) & (t * Ls.tail_len > 0.02)
    w[m] = np.minimum(w[m], 0.3)
    # the face's front, in the head frame
    f = -co[:, 1] - B.poll[0]
    z = co[:, 2] - B.poll[1]
    a = f * B.hd[0] + z * B.hd[1]
    b = f * B.hn[0] + z * B.hn[1]
    m = (a > 0.7 * B.head_len) & (b < 0.6 * B.head_len)
    w[m] = np.minimum(w[m], 0.35)
    knee = B.joints("fore")["wrist"][1]
    w[co[:, 2] < knee * 1.3] = np.minimum(w[co[:, 2] < knee * 1.3], 0.40)
    w[co[:, 2] < knee * 0.3] = np.minimum(w[co[:, 2] < knee * 0.3], 0.2)
    for val in np.unique(np.round(w, 3)):
        idx = np.nonzero(np.abs(np.round(w, 3) - val) < 1e-6)[0].tolist()
        g.add(idx, float(val), "REPLACE")
    return name


def carve(ob):
    """Sculpt by numbers on the dense surface: nostrils (an opening with a
    raised rim) and the mouth line between the lips."""
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.normal_update()
    for v in bm.verts:
        a, b, x = B.head_local(v.co)
        if a < 0.3 * B.head_len:
            continue
        ax = abs(x)
        push = 0.0
        for c in S.get("carve", []):
            if c["kind"] == "nostril":
                na = (a - c["a"]) / c["ra"]
                nb = (b - c["b"]) / c["rb"]
                nx = (ax - c["x"]) / c["rx"]
                rr = na * na + nb * nb + nx * nx
                if rr < 1.0:
                    push += c["depth"] * (1 - rr) ** 0.55
                elif rr < 2.4 and ax > 0.3 * c["x"]:
                    push -= c["depth"] * 0.18 * math.sin(pi * (rr - 1.0) / 1.4)
            elif c["kind"] == "mouth":
                (ca, cb), (fa, fb) = c["corner"], c["front"]
                if ca - 0.01 < a < fa + 0.02:
                    u = (a - ca) / (fa - ca)
                    line_b = cb + (fb - cb) * min(max(u, 0), 1)
                    dd = b - line_b
                    if abs(dd) < c["width"]:
                        push += c["depth"] * (1 - abs(dd) / c["width"]) ** 1.5 * smoothstep(-0.05, 0.15, u)
        if push:
            v.co -= v.normal * push
    bm.to_mesh(me)
    bm.free()


def sdf_fillet(ob, voxel, fillet, mean=2):
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
    mn.inputs["Iterations"].default_value = mean
    g2m = N.new("GeometryNodeGridToMesh")
    g2m.inputs["Threshold"].default_value = 0.0
    L = ng.links
    L.new(gi.outputs[0], m2s.inputs["Mesh"])
    L.new(m2s.outputs[0], fil.inputs["Grid"])
    L.new(fil.outputs[0], mn.inputs["Grid"])
    L.new(mn.outputs[0], g2m.inputs["Grid"])
    L.new(g2m.outputs[0], go.inputs[0])
    md = ob.modifiers.new("fillet", "NODES")
    md.node_group = ng
    activate(ob)
    bpy.ops.object.modifier_apply(modifier=md.name)


def build_surface():
    ob = new_obj("parts", body.parts(B))
    activate(ob)
    me = ob.data
    me.remesh_voxel_size = S["voxel"]
    me.use_remesh_fix_poles = True
    bpy.ops.object.voxel_remesh()
    if S.get("fillet", 0) > 0:
        sdf_fillet(ob, S["voxel"], S["fillet"])
    log("UNION", {"vertices": len(ob.data.vertices)})
    pw = protect_weights(ob)
    log("PROTECT", {})
    smooth_mod(ob, 0.6, S.get("smooth", 8), pw)
    carve(ob)
    smooth_mod(ob, 0.5, 2)
    log("SURFACE", {"vertices": len(ob.data.vertices)})
    return ob


def untangle(ob, rounds=24):
    """Projection back onto the surface can fold a few quads through each
    other in a tight crease: relax just those until no two faces that share
    no vertex cross."""
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
        if not bad:
            bm.free()
            log("UNTANGLE", {"rounds": r})
            return
        vs = {v for i in bad for v in bm.faces[i].verts}
        for _ in range(1 + r // 4):
            vs |= {n for v in list(vs) for e in v.link_edges for n in e.verts}
        for _ in range(4):
            new = {v: sum((e.other_vert(v).co for e in v.link_edges), Vector()) / max(len(v.link_edges), 1) for v in vs}
            for v, c in new.items():
                v.co = v.co.lerp(c, 0.6)
        bm.to_mesh(ob.data)
        bm.free()
    raise RuntimeError("untangle: faces still cross after %d rounds" % rounds)


def quad_remesh(src, faces):
    """QuadriFlow on a thinned copy of the dense surface (it is slow and
    fussy on dense input and refuses non-manifold points: a coarser voxel
    pass, then other seeds and coarser passes if it fails), relaxed and
    projected back onto the surface."""
    v0 = S["voxel"]
    seed = S.get("seed", 7)
    tries = [(v0 * 1.7, seed), (v0 * 1.7, seed + 1), (v0 * 1.35, seed), (v0 * 2.2, seed), (v0 * 2.2, seed + 2), (v0 * 2.8, seed + 3)]
    ob = src.copy()
    ob.data = src.data.copy()
    ob.name = ID
    bpy.context.scene.collection.objects.link(ob)
    keep = src.data.copy()
    for vs, sd in tries:
        ob.data = keep.copy()
        activate(ob)
        ob.data.remesh_voxel_size = vs
        bpy.ops.object.voxel_remesh()
        smooth_mod(ob, 0.5, 1)
        try:
            bpy.ops.object.quadriflow_remesh(target_faces=faces, use_mesh_symmetry=True, use_preserve_sharp=False, use_preserve_boundary=False,
                                             smooth_normals=False, mode="FACES", seed=sd)
        except RuntimeError as e:
            log("QUADRIFLOW_RETRY", {"voxel": round(vs, 4), "seed": sd, "error": str(e)[:120]})
            continue
        log("QUADRIFLOW", {"faces": len(ob.data.polygons), "voxel": round(vs, 4), "seed": sd})
        if len(ob.data.polygons) <= faces * 1.6:
            break
    else:
        raise RuntimeError("QuadriFlow did not run on any try")
    smooth_mod(ob, 0.5, 3)
    sw = ob.modifiers.new("sw", "SHRINKWRAP")
    sw.target = src
    sw.wrap_method = "NEAREST_SURFACEPOINT"
    bpy.ops.object.modifier_apply(modifier=sw.name)
    untangle(ob)
    collapse_slivers(ob, 0.4 * v0)
    return ob


def collapse_slivers(ob, d):
    """Projection onto the dense surface can pull quad corners almost
    together in creases (the groin, the armpit): edges far shorter than the
    voxel, which any skinning tears. Each run of such edges is collapsed to
    its centre."""
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    short = [e for e in bm.edges if e.calc_length() < d]
    if short:
        bmesh.ops.collapse(bm, edges=short, uvs=False)
    # folded quads: two opposite corners almost together (the glb's
    # triangles then carry a sliver diagonal) are merged
    pairs = []
    for f in bm.faces:
        if len(f.verts) == 4:
            v = f.verts
            for a, b in ((v[0], v[2]), (v[1], v[3])):
                if (a.co - b.co).length < d:
                    pairs.append((a, b))
                    break
    folded = 0
    for a, b in pairs:
        if a.is_valid and b.is_valid and a is not b:
            bmesh.ops.pointmerge(bm, verts=[a, b], merge_co=(a.co + b.co) * 0.5)
            folded += 1
    if short or folded:
        bmesh.ops.dissolve_degenerate(bm, dist=d * 0.05, edges=bm.edges[:])
    bm.to_mesh(ob.data)
    bm.free()
    log("SLIVERS", {"collapsed": len(short), "folded": folded, "under_m": round(d, 4)})


def add_separate(ob):
    """Join the separate parts (eyes, horns, hooves); returns the vertex
    ranges [(start, end, rule)] and where they begin."""
    ranges = []
    first = len(ob.data.vertices)
    for rule, bm in body.separate(B):
        n0 = len(ob.data.vertices)
        part = new_obj("part_" + rule, bm)
        for p in part.data.polygons:
            p.use_smooth = True
        activate(ob)
        part.select_set(True)
        bpy.ops.object.join()
        ranges.append((n0, len(ob.data.vertices), rule))
    return ranges, first


# ------------------------------------------------------------ regions, coat chart
PATCHES = None


def patch_blobs():
    """Patch centres and sizes, scattered over the trunk, neck and head from
    the species' seed (a plain LCG: deterministic, no random library)."""
    c = S.get("coat", {})
    st = c.get("patch_seed", 1) * 2654435761 % 4294967296

    def rnd():
        nonlocal st
        st = (st * 1664525 + 1013904223) % 4294967296
        return st / 4294967296.0
    tk = S["trunk"]
    f0, f1 = tk[0][0], tk[-1][0]
    r0, r1 = c.get("patch_size", [0.1, 0.3])
    blobs = []
    for i in range(c.get("patch_count", 12)):
        k = tk[min(int(rnd() * len(tk)), len(tk) - 1)]
        cf = f0 + (f1 - f0) * rnd()
        cz = k[2] + (k[1] - k[2]) * (0.1 + 0.9 * rnd())
        cx = (1 if rnd() > 0.5 else -1) * k[3] * (0.6 + 0.5 * rnd())
        r = r0 + (r1 - r0) * rnd()
        blobs.append((P(cf, cz, cx), r))
    # one over the head and one round each eye: a patched face
    for sx in (1, -1):
        blobs.append((B.eye_centre(sx), r0 * 0.9 * (0.8 + 0.4 * rnd())))
    return blobs


def patch_field(co):
    """Patches: a smooth 0..1 field of soft blobs scattered over the body
    (deterministic from the species' seed), for piebald and spotted coats
    (a material thresholds it, e.g. > 0.55)."""
    global PATCHES
    if PATCHES is None:
        PATCHES = patch_blobs()
    total = 0.0
    for c, r in PATCHES:
        d2 = (co - c).length_squared
        if d2 < 9 * r * r:
            total += math.exp(-d2 / (r * r))
    return min(total, 1.0)


def coat_chart(co, nrm, reg):
    """`_EVANI_COAT`: R face (1 down the middle of the face, 0 on the cheeks
    and behind the eyes), G belly (the underside), B lower legs (1 at the
    hoof or paw, 0 above the knee or hock), A patches (patch_field)."""
    a, b, x = B.head_local(co)
    hl = B.head_len
    fw = S.get("coat", {}).get("face_width", 0.05)
    face = smoothstep(-0.05 * hl, 0.15 * hl, a) * (1 - smoothstep(fw * 0.5, fw * 1.6, abs(x))) * (1 - smoothstep(0.35 * hl, 0.75 * hl, b))
    tk = S["trunk"]
    zmid = sum(k[1] + k[2] for k in tk) / (2 * len(tk))
    belly = smoothstep(-0.15, -0.65, nrm.z) * (1 - smoothstep(zmid * 0.85, zmid * 1.05, co.z))
    legs = 0.0
    if reg in ("leg", "hoof") or co.z < B.joints("fore")["wrist"][1] * 1.2:
        ls = leg_side(co)
        if ls:
            legs = ls[1]
        if reg == "hoof":
            legs = 1.0
    return (face, belly, legs, patch_field(co))


def regions_and_coat(ob, ranges):
    me = ob.data
    part = me.attributes.new("_EVANI_PART", "INT8", "POINT")
    chart = me.color_attributes.new("_EVANI_COAT", "BYTE_COLOR", "POINT")
    counts = {}
    reg_of = []
    me.calc_loop_triangles() if hasattr(me, "calc_loop_triangles") else None
    for v in me.vertices:
        rid = region_of(v.co, next((r for a, b, r in ranges if a <= v.index < b), None))
        reg_of.append(rid)
        part.data[v.index].value = REGION_IDS.index(rid)
        counts[rid] = counts.get(rid, 0) + 1
        chart.data[v.index].color = coat_chart(v.co, v.normal, rid)
    missing = [r for r in REGION_IDS if r not in counts]
    log("REGIONS", {"counts": counts, "missing": missing})
    if missing:
        raise RuntimeError("regions with no vertex: %s" % missing)
    return reg_of


def materials(ob, reg_of):
    me = ob.data
    me.materials.clear()
    slot = {}
    for i, (name, m) in enumerate(S["materials"].items()):
        mat = bpy.data.materials.new(name)
        mat.diffuse_color = tuple(m["color"]) + (1.0,)
        mat.roughness = m.get("roughness", 0.8)
        mat.use_nodes = True
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        if bsdf:
            bsdf.inputs["Base Color"].default_value = tuple(m["color"]) + (1.0,)
            bsdf.inputs["Roughness"].default_value = m.get("roughness", 0.8)
        me.materials.append(mat)
        for r in m["regions"]:
            slot[r] = i
    for p in me.polygons:
        votes = {}
        for vi in p.vertices:
            votes[reg_of[vi]] = votes.get(reg_of[vi], 0) + 1
        p.material_index = slot.get(max(votes, key=votes.get), 0)
        p.use_smooth = True


# ------------------------------------------------------------ the studio tier
def export_studio(ob):
    """<id>_studio.glb: the rest pose, unskinned, no clips, with the tags:
    what the studio's viewer loads (it has `anim` off). Convert it with
    --id <id> into apps/material-view/assets/models."""
    # The studio draws one shared tier on every device (the convert's low
    # tier, a quarter of the input's triangles): reduced first so that tier
    # lands near the horse's (2,900 triangles, 28 KB); the tags are taken
    # from the nearest full-detail vertex, never interpolated.
    target = S.get("studio_tris", 11600)
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    st = ob
    if tris > target:
        st = ob.copy()
        st.data = ob.data.copy()
        st.name = ID + "_studio"
        bpy.context.scene.collection.objects.link(st)
        dec = st.modifiers.new("dec", "DECIMATE")
        dec.decimate_type = "COLLAPSE"
        dec.ratio = target / tris
        dec.use_collapse_triangulate = True
        activate(st)
        bpy.ops.object.modifier_apply(modifier=dec.name)
        from mathutils.kdtree import KDTree
        kd = KDTree(len(ob.data.vertices))
        for v in ob.data.vertices:
            kd.insert(v.co, v.index)
        kd.balance()
        near = [kd.find(v.co)[1] for v in st.data.vertices]
        for a in ob.data.attributes:
            if a.domain != "POINT" or not a.name.startswith("_EVANI") or a.name not in st.data.attributes:
                continue
            src, dst = a.data, st.data.attributes[a.name].data
            key = "color" if hasattr(src[0], "color") else "value"
            for i, j in enumerate(near):
                setattr(dst[i], key, getattr(src[j], key))
        log("STUDIO_REDUCED", {"from": tris, "to": sum(len(p.vertices) - 2 for p in st.data.polygons)})
    ob = st
    activate(ob)
    path = os.path.join(OUT, ID + "_studio.glb")
    want = dict(filepath=path, export_format="GLB", use_selection=True, export_yup=True, export_apply=True,
                export_skins=False, export_animations=False, export_materials="EXPORT", export_extras=False,
                export_cameras=False, export_lights=False, export_attributes=True, export_vertex_color="NONE", export_tangents=False)
    have = set(bpy.ops.export_scene.gltf.get_rna_type().properties.keys())
    bpy.ops.export_scene.gltf(**{k: v for k, v in want.items() if k in have})
    glbfix.fix_glb_attributes(path)
    log("STUDIO_EXPORT", {"path": path, "bytes": os.path.getsize(path)})
    if ob.name == ID + "_studio":
        data = ob.data
        bpy.data.objects.remove(ob, do_unlink=True)
        bpy.data.meshes.remove(data)


# ------------------------------------------------------------ renders and report
def bounds(ob):
    vs = [ob.matrix_world @ v.co for v in ob.data.vertices]
    lo = Vector((min(v.x for v in vs), min(v.y for v in vs), min(v.z for v in vs)))
    hi = Vector((max(v.x for v in vs), max(v.y for v in vs), max(v.z for v in vs)))
    return lo, hi


def render_views(ob, views, prefix=""):
    import render
    render.shots(ob, views, OUT, prefix, S)


def report(ob, extra=None):
    me = ob.data
    lo, hi = bounds(ob)
    tris = sum(len(p.vertices) - 2 for p in me.polygons)
    rep = {"species": ID, "vertices": len(me.vertices), "faces": len(me.polygons), "triangles": tris,
           "size_m": {"length": round(hi.y - lo.y, 3), "height": round(hi.z - lo.z, 3), "width": round(hi.x - lo.x, 3)}}
    if extra:
        rep.update(extra)
    with open(os.path.join(OUT, ID + ".report.json"), "w", encoding="utf-8") as f:
        json.dump(rep, f, indent=1)
    log("REPORT", rep)


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    src = build_surface()
    if ARGS.stage == "surface":
        src.name = ID
        for p in src.data.polygons:
            p.use_smooth = True
        ranges, n0 = add_separate(src)
        reg = regions_and_coat(src, ranges)
        materials(src, reg)
        report(src)
        bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, ID + ".blend"))
        if ARGS.views:
            render_views(src, [v for v in ARGS.views.split(",") if v])
        return
    ob = quad_remesh(src, ARGS.faces or S["faces"])
    src.hide_render = True
    src.hide_viewport = True
    ranges, n0 = add_separate(ob)
    reg = regions_and_coat(ob, ranges)
    materials(ob, reg)
    export_studio(ob)
    extra = {}
    if ARGS.stage == "all":
        import rig
        rig.B, rig.S, rig.ID = B, S, ID
        r = rig.run(ob, reg, n0)
        extra = rig.finish(ob, r, OUT, ao=ARGS.ao, ao_samples=ARGS.ao_samples, eye_from=n0)
        glbfix.fix_glb_attributes(os.path.join(OUT, ID + ".glb"))
    report(ob, extra)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, ID + ".blend"))
    if ARGS.views:
        render_views(ob, [v for v in ARGS.views.split(",") if v])


if __name__ == "__main__":
    main()
