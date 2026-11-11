# Measure the test horse against the classical head-length canon and draw it:
# an orthographic side view over a grid of head lengths, the landmarks
# marked, and a table of target against measured.
#   blender -b <horse.blend> --python tools/models/horse/canon_overlay.py -- --out <folder> [--name canon_side]
# Writes <folder>/<name>.png and <folder>/canon.json. Measured on the mesh in
# its rest pose (silhouettes, ray hits along the neck); joint heights come
# from the landmarks the skeleton is fitted to (anatomy.LM).
import bpy, bmesh, sys, os, math, json, argparse
from mathutils import Vector, Matrix, Euler

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import anatomy as A  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ap = argparse.ArgumentParser()
ap.add_argument("--out", required=True)
ap.add_argument("--name", default="canon_side")
ARGS = ap.parse_args(argv)
OUT = os.path.abspath(ARGS.out)
os.makedirs(OUT, exist_ok=True)
P = A.P
HL = A.HL

ob = bpy.data.objects["horse"]
rig = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
if rig is not None:
    rig.data.pose_position = "REST"
for o in bpy.data.objects:
    if o.name.startswith("anchor:") or o.name in ("mane_line", "parts"):
        o.hide_render = True
bpy.context.view_layer.update()
dg = bpy.context.evaluated_depsgraph_get()
ev = ob.evaluated_get(dg)
me = ev.to_mesh()
V = [(ev.matrix_world @ v.co) for v in me.vertices]
mats = {}
for p in me.polygons:
    for i in p.vertices:
        mats[i] = p.material_index
body = [v for i, v in enumerate(V) if mats.get(i, 0) != 2]   # not the eyes


def F(v):
    return -v.y


# --- silhouettes
withers = max((v for v in body if abs(v.x) < 0.04 and 0.30 < F(v) < 0.56), key=lambda v: v.z)
girth = min((v for v in body if abs(v.x) < 0.06 and 0.30 < F(v) < 0.52), key=lambda v: v.z)
ps = max((v for v in body if abs(v.x) > 0.17 and 1.02 < v.z < 1.32), key=F)
pb = min((v for v in body if abs(v.x) > 0.03 and 1.00 < v.z < 1.35 and F(v) > -0.82), key=F)
muzzle_a = max(A.head_local(v)[0] for v in body if v.z > 1.2 and F(v) > 1.0)
head_depth = max(A.head_local(v)[1] for v in body if v.z > 1.3 and 0.10 < A.head_local(v)[0] < 0.30 and A.head_local(v)[1] < 0.45)

# --- the neck's top line and underline: rays from inside the neck toward
# the crest and the throat at each key, joined into polylines
bvh_src = bmesh.new()
bvh_src.from_mesh(me)
bvh_src.transform(ev.matrix_world)
from mathutils.bvhtree import BVHTree  # noqa: E402
tree = BVHTree.FromBMesh(bvh_src)


def hit(origin, target):
    d = (target - origin)
    loc, n, i, dist = tree.ray_cast(origin, d.normalized(), d.length * 2.0)
    return loc


keys = A.NECK
crest, throat = [], []
for i in range(41):
    s = i / 40 * (len(keys) - 1)
    a0, a1, b0, b1 = A.catmull([(k[0][0], k[0][1], k[1][0], k[1][1]) for k in keys], s)
    ca, tb = P(a0, a1), P(b0, b1)
    mid = (ca + tb) / 2
    crest.append(hit(mid, ca + (ca - mid) * 0.5))
    throat.append(hit(mid, tb + (tb - mid) * 0.5))
crest = [c for c in crest if c is not None]
throat = [c for c in throat if c is not None]


def length(pts):
    return sum((b - a).length for a, b in zip(pts, pts[1:]))


# top line from the withers to the poll; underline from the chest junction
# (where the throat leaves the breast, at the neck_chest landmark) to the
# throatlatch
poll = P(*A.POLL)
top = [c for c in crest if F(c) >= F(withers)]
top_len = length([withers] + top + [poll])
nc = P(*A.LM["neck_chest"])
tl = P(*A.LM["throatlatch"])
und = [c for c in throat if (c - nc).dot(tl - nc) >= 0]
under_len = length([nc] + und + [tl]) if und else 0.0

LM = A.LM


def ang(a, b):
    return math.degrees(math.atan2(LM[a][1] - LM[b][1], LM[b][0] - LM[a][0]))


body_len = F(ps) - F(pb)
hw = withers.z
m = {
    "head_length_m": round(muzzle_a, 3),
    "withers_height_m": round(hw, 3),
    "withers_in_heads": round(hw / muzzle_a, 2),
    "body_length_m": round(body_len, 3),
    "body_length_over_height": round(body_len / hw, 3),
    "neck_topline_in_heads": round(top_len / muzzle_a, 2),
    "neck_underline_in_heads": round(under_len / muzzle_a, 2),
    "elbow_over_height": round(LM["elbow"][1] / hw, 3),
    "knee_height_m": LM["knee"][1],
    "hock_height_m": LM["hock"][1],
    "point_of_buttock_z": round(pb.z, 3),
    "point_of_shoulder_z": round(ps.z, 3),
    "croup_over_body_length": round(math.hypot(LM["point_hip"][0] - F(pb), LM["point_hip"][1] - pb.z) / body_len, 3),
    "fore_pastern_deg": round(abs(ang("fore_fetlock", "fore_coronet")), 1),
    "hind_pastern_deg": round(abs(ang("hind_fetlock", "hind_coronet")), 1),
    "chest_depth_over_height": round((hw - girth.z) / hw, 3),
    "head_depth_at_jowl_over_head": round(head_depth / muzzle_a, 3),
    "neck_angle_deg": None,
}
# the neck's angle where it leaves the chest: the axis through the middles of
# the lower half of the neck
mids = [((c + t) / 2) for c, t in zip(crest[:21], throat[:21])]
if len(mids) > 4:
    d = mids[-1] - mids[2]
    m["neck_angle_deg"] = round(math.degrees(math.atan2(d.z, -d.y)), 1)
TARGET = {
    "withers_in_heads": "2.5",
    "body_length_over_height": "about 1.0 or a little less",
    "neck_topline_in_heads": "1.5",
    "neck_underline_in_heads": "1.0",
    "elbow_over_height": "0.5",
    "knee_height_m": "about the hock height",
    "point_of_buttock_z": "about the point of shoulder height",
    "croup_over_body_length": "0.33",
    "fore_pastern_deg": "50-55",
    "hind_pastern_deg": "50-55",
    "neck_angle_deg": "about 45",
}
with open(os.path.join(OUT, "canon.json"), "w") as f:
    json.dump({"measured": m, "target": TARGET}, f, indent=1)
print("CANON", json.dumps(m))

# ------------------------------------------------------------- drawing
sc = bpy.context.scene


def mat(name, rgb):
    mm = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    mm.diffuse_color = (*rgb, 1)
    return mm


red, yel, blu, wht = mat("ov_red", (0.85, 0.12, 0.10)), mat("ov_yel", (1.0, 0.85, 0.1)), mat("ov_blue", (0.15, 0.4, 0.95)), mat("ov_white", (1, 1, 1))
XN = 0.45   # drawn on a plane between the horse and the camera


def bar(a, b, w, mt):
    d = b - a
    bpy.ops.mesh.primitive_cube_add(size=1, location=(a + b) / 2)
    o = bpy.context.active_object
    o.scale = (0.002, max(abs(d.y), w), max(abs(d.z), w))
    o.data.materials.append(mt)
    return o


def line_fz(f0, z0, f1, z1, w, mt):
    a, b = P(f0, z0, XN), P(f1, z1, XN)
    d = b - a
    bpy.ops.mesh.primitive_cube_add(size=1, location=(a + b) / 2)
    o = bpy.context.active_object
    o.scale = (0.002, d.length, w)
    o.rotation_euler = Euler((math.atan2(d.z, d.y) if d.length > 0 else 0, 0, 0))
    o.data.materials.append(mt)


def dot(fz, r, mt, x=XN):
    bpy.ops.mesh.primitive_uv_sphere_add(radius=r, location=P(fz[0], fz[1], x), segments=12, ring_count=8)
    bpy.context.active_object.data.materials.append(mt)


def text(s, f, z, size, mt):
    bpy.ops.object.text_add(location=P(f, z, XN + 0.01))
    t = bpy.context.active_object
    t.data.body = s
    t.data.size = size
    t.rotation_euler = Euler((math.radians(90), 0, math.radians(90)))
    t.data.materials.append(mt)
    return t


hl = muzzle_a
fpb = F(pb)
# grid: horizontal every half head from the ground, vertical every head from the point of buttock
for k in range(0, 8):
    z = k * hl / 2
    line_fz(fpb - 0.4, z, fpb + 3.0 * hl + 0.4, z, 0.0018 if k % 2 else 0.0035, blu)
    text("%.1f HL" % (k / 2), fpb - 0.55, z + 0.01, 0.035, blu)
for k in range(0, 5):
    f = fpb + k * hl
    line_fz(f, -0.02, f, 2.15, 0.0018, blu)
    text("%d HL" % k, f + 0.01, 2.17, 0.035, blu)
# measured: withers height, body length, point of shoulder / buttock level, elbow at half
line_fz(F(withers) - 0.12, hw, F(withers) + 0.12, hw, 0.004, red)
line_fz(fpb, pb.z, F(ps), pb.z, 0.003, red)
line_fz(fpb, ps.z, F(ps), ps.z, 0.002, yel)
line_fz(LM["elbow"][0] - 0.25, hw / 2, LM["elbow"][0] + 0.25, hw / 2, 0.003, red)
line_fz(LM["knee"][0] - 0.12, LM["knee"][1], LM["hock"][0] - 0.12, LM["hock"][1], 0.002, yel)
for c, t in zip(crest, crest[1:]):
    line_fz(F(c), c.z, F(t), t.z, 0.003, yel)
for c, t in zip(throat, throat[1:]):
    line_fz(F(c), c.z, F(t), t.z, 0.003, yel)
for name in ("withers", "point_shoulder", "point_buttock", "elbow", "knee", "hock", "point_hock", "stifle", "point_hip",
             "croup", "poll", "throatlatch", "neck_chest", "fore_fetlock", "fore_coronet", "hind_fetlock", "hind_coronet",
             "hip_joint", "shoulder_joint", "eye"):
    dot(LM[name], 0.014, red)
for v in (withers, ps, pb, girth):
    dot((F(v), v.z), 0.016, yel)
rows = [
    ("height at withers", "2.5 HL", "%.2f HL (%.2f m)" % (hw / hl, hw)),
    ("body length / height", "1.0 or less", "%.2f (%.2f m)" % (body_len / hw, body_len)),
    ("neck top line", "1.5 HL", "%.2f HL" % (top_len / hl)),
    ("neck underline", "1.0 HL", "%.2f HL" % (under_len / hl)),
    ("neck angle", "about 45 deg", "%s deg" % m["neck_angle_deg"]),
    ("elbow / height", "0.50", "%.2f" % (LM["elbow"][1] / hw)),
    ("knee / hock height", "equal", "%.2f / %.2f m" % (LM["knee"][1], LM["hock"][1])),
    ("pt buttock / pt shoulder", "level", "%.2f / %.2f m" % (pb.z, ps.z)),
    ("croup / body length", "0.33", "%.2f" % m["croup_over_body_length"]),
    ("pasterns fore / hind", "50-55 deg", "%.0f / %.0f deg" % (m["fore_pastern_deg"], m["hind_pastern_deg"])),
]
y = 2.10
text("canon, HL = head length (%.2f m)" % hl, 0.30, y + 0.05, 0.034, wht)
text("target", -0.18, y + 0.05, 0.034, wht)
text("measured", -0.45, y + 0.05, 0.034, wht)
for a_, b_, c_ in rows:
    text(a_, 0.30, y, 0.030, wht)
    text(b_, -0.18, y, 0.030, wht)
    text(c_, -0.45, y, 0.030, wht)
    y -= 0.045

sc.render.engine = "BLENDER_WORKBENCH"
sh = sc.display.shading
sh.light = "STUDIO"
sh.color_type = "MATERIAL"
sh.show_cavity = True
sc.render.resolution_x, sc.render.resolution_y = 2000, 1500
world = bpy.data.worlds.get("World") or bpy.data.worlds.new("World")
sc.world = world
world.color = (0.08, 0.08, 0.09)
cam = bpy.data.objects.new("ov_cam", bpy.data.cameras.new("ov_cam"))
sc.collection.objects.link(cam)
cam.data.type = "ORTHO"
cam.data.ortho_scale = 3.15
cam.location = P(0.55, 1.07, 8.0)
cam.rotation_euler = (P(0.55, 1.07, 0) - cam.location).normalized().to_track_quat("-Z", "Y").to_euler()
sc.camera = cam
sc.render.filepath = os.path.join(OUT, ARGS.name + ".png")
bpy.ops.render.render(write_still=True)
print("CANON_RENDERED", sc.render.filepath)
