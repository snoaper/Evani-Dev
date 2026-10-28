# Headless Blender script: rig the Meshy "Frontier Marshal", make its clips and
# export the rigged master .glb (Stage 3, S3-16 and decision 10).
# Run (pinned Blender 5.2.2 LTS, docs/stage3/spikes.md):
#   blender -b --factory-startup -t 2 --python tools/blender/marshal_rig.py -- \
#       <frontier-marshal.master.glb> <out .rigged.master.glb> <out .blend>
# The untouched master is only read. Everything here is deterministic: the same
# master gives the same rig, weights and clips.
#
# Frames (documented in docs/stage3/source-models.md, Marshal section):
#  - Blender: X = the Marshal's left, -Y = forward, Z = up. glTF (export +Y up):
#    +X left, +Y up, +Z forward, the kart racer's kart-local frame.
#  - Model origin: the ground under him when standing; for the seated clips it
#    is the point on the ground under the seat contact, with `anchor:seat` 0.64 m
#    over it (the kart racer's seat top, draw.rs `draw_kart`).
import bpy, sys, os, math, json
import numpy as np
from mathutils import Vector, Matrix, Quaternion
from mathutils.kdtree import KDTree

SRC, OUT_GLB, OUT_BLEND = sys.argv[sys.argv.index("--") + 1:][:3]

# ---------------------------------------------------------------- constants
S = 1.15            # scale from the Meshy master (1.90 m) to kart metres (2.185 m)
FPS = 30
SEAT_H = 0.64       # kart racer seat top over the kart's ground (0.56 + 0.16 / 2)
SRC_Z_OFFSET = 0.951  # master is centred on its origin; this puts the soles at 0

# Kart racer numbers (kits/evani-kit-kart-racer/src/draw.rs, kart-local x left,
# y up, z forward, metres). Converted to Blender with the seat point at y = 0:
# B = (x, -z + SEAT_Z, y), SEAT_Z = -0.30 (seat base centre z).
SEAT_Z = -0.30
def k2b(x, y, z):
    return Vector((x, -z + SEAT_Z, y))
WHEEL_C = k2b(0.0, 0.98, 0.34)
WHEEL_R = 0.23
_p = -1.0  # wheel pitch about kart x (draw.rs: pitch_q(-1.0))
# Wheel axis (kart (0, cos p, sin p)), pointing up and back at the driver, and
# "up" along the wheel plane (kart (0, -sin p, cos p)), both in Blender.
WHEEL_N = Vector((0.0, -math.sin(_p), math.cos(_p)))
WHEEL_U = Vector((0.0, -math.cos(_p), -math.sin(_p)))
WHEEL_X = Vector((1.0, 0.0, 0.0))
STEER_MAX = 0.9     # rad, the kit's full visual wheel turn (steer_vis * 0.9)
GRIP_CURL = 84.0    # deg, finger curl on the wheel: the fingers wrap the rim
GRIP_THUMB = 58.0   # deg, thumb curl: the thumb closes over the rim
COAT_FOLD = (-66.0, 160.0)  # deg, seated: coat_back tucked under, coat_tail laid back on the cushion (bone X, follows the hips)
SMOOTH_PASSES = 12  # weight smoothing over the seam bands (fringes, nape, coat flap)
FRINGE_SIGMA = 0.015  # m, width of the sleeve-fringe weight profile along the arm (40 mm tried: worse in every clip)
# sleeve fringe bones: strand roots stay on the sleeve, the strand bodies follow
# short bones under the arm that hang toward gravity in every pose
FRINGE_X = (0.30, 0.40, 0.52, 0.64, 0.76)   # src m along the arm, bone centres (the strands reach 0.77)
FRINGE_PARENT = ("upper_arm", "upper_arm", "forearm", "forearm", "forearm")
FRINGE_ROOT_Z = 1.385  # src m, the strand roots (sleeve underside)
FRINGE_ROOT = 0.05     # m, root band: blends from the sleeve into the fringe bones (0.03: worst stretch 12.8x before the post-smoothing blend; 0.07: 4.6x against 4.0x)
FRINGE_DROOP = 0.85    # share of the way from following the arm to hanging straight down

# ------------------------------------------------------------ skeleton (src m)
# Joint positions measured on the master (feet at 0, unscaled), Blender frame.
# name: (head, tail, parent, roll axis for Z)
UP, FWD, BACK, DOWN = (0, 0, 1), (0, -1, 0), (0, 1, 0), (0, 0, -1)
SIDED = {"shoulder", "upper_arm", "forearm", "hand", "fingers1", "fingers2", "thumb1", "thigh", "shin", "foot"}
def side(name, h, t, parent, zax):
    """Left (+X) and mirrored right copies of a bone; a sided parent gets the
    same suffix."""
    out = {}
    for s, suf in ((1, "_l"), (-1, "_r")):
        p = parent + suf if parent in SIDED else parent
        out[name + suf] = ((h[0] * s, h[1], h[2]), (t[0] * s, t[1], t[2]), p, zax)
    return out
BONES = {
    "root": ((0, 0, 0), (0, 0.25, 0), None, UP),
    "hips": ((0, 0.02, 0.93), (0, 0.02, 1.03), "root", FWD),
    "spine": ((0, 0.02, 1.03), (0, 0.02, 1.17), "hips", FWD),
    "spine1": ((0, 0.02, 1.17), (0, 0.02, 1.32), "spine", FWD),
    "spine2": ((0, 0.02, 1.32), (0, 0.01, 1.50), "spine1", FWD),
    "neck": ((0, 0.01, 1.50), (0, -0.005, 1.62), "spine2", FWD),
    "head": ((0, -0.005, 1.62), (0, -0.005, 1.79), "neck", FWD),
    "hat": ((0, 0.0, 1.79), (0, 0.0, 1.90), "head", FWD),
}
BONES.update(side("shoulder", (0.03, 0.01, 1.47), (0.17, 0.04, 1.47), "spine2", UP))
BONES.update(side("upper_arm", (0.17, 0.04, 1.47), (0.45, 0.06, 1.45), "shoulder", UP))
BONES.update(side("forearm", (0.45, 0.06, 1.45), (0.705, 0.065, 1.44), "upper_arm", UP))
BONES.update(side("hand", (0.705, 0.065, 1.44), (0.778, 0.07, 1.443), "forearm", UP))
BONES.update(side("fingers1", (0.778, 0.07, 1.443), (0.815, 0.073, 1.443), "hand", UP))
BONES.update(side("fingers2", (0.815, 0.073, 1.443), (0.85, 0.075, 1.442), "fingers1", UP))
BONES.update(side("fingers3", (0.85, 0.075, 1.442), (0.885, 0.077, 1.44), "fingers2", UP))
BONES.update(side("thumb1", (0.715, 0.035, 1.43), (0.748, 0.012, 1.42), "hand", UP))
BONES.update(side("thumb2", (0.748, 0.012, 1.42), (0.785, 0.0, 1.41), "thumb1", UP))
# the back of the coat skirt: a two-bone flap, so the seated clips can fold it
# back over the seat instead of letting it hang through the cushion
for _i, (_x, _p) in enumerate(zip(FRINGE_X, FRINGE_PARENT)):
    BONES.update(side("fringe%d" % (_i + 1), (_x, 0.075, FRINGE_ROOT_Z), (_x, 0.075, FRINGE_ROOT_Z - 0.12), _p, BACK))
BONES["coat_back"] = ((0, 0.17, 0.95), (0, 0.18, 0.86), "hips", BACK)
BONES["coat_tail"] = ((0, 0.18, 0.86), (0, 0.19, 0.76), "coat_back", BACK)
BONES.update(side("thigh", (0.09, 0.02, 0.92), (0.16, 0.02, 0.53), "hips", FWD))
BONES.update(side("shin", (0.16, 0.02, 0.53), (0.2, 0.06, 0.11), "thigh", FWD))
BONES.update(side("foot", (0.2, 0.06, 0.11), (0.205, -0.08, 0.03), "shin", UP))
BONES.update(side("toe", (0.205, -0.08, 0.03), (0.21, -0.17, 0.03), "foot", UP))
ORDER = []
def _visit(n):
    if n in ORDER: return
    p = BONES[n][2]
    if p: _visit(p)
    ORDER.append(n)
for n in BONES: _visit(n)

# ------------------------------------------------------------------ helpers
def ctx_obj(o):
    for x in bpy.context.selected_objects: x.select_set(False)
    o.select_set(True); bpy.context.view_layer.objects.active = o

# ------------------------------------------------------------------- import
bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.fps = FPS
bpy.ops.import_scene.gltf(filepath=SRC)
mesh = [o for o in bpy.data.objects if o.type == "MESH"][0]
mesh.name = "frontier_marshal"; mesh.data.name = "frontier_marshal"
mesh.data.transform(Matrix.Diagonal((S, S, S, 1.0)) @ Matrix.Translation((0, 0, SRC_Z_OFFSET)))
mesh.data.update()

# ----------------------------------------------------------------- armature
arm_data = bpy.data.armatures.new("marshal_rig")
arm = bpy.data.objects.new("marshal_rig", arm_data)
sc.collection.objects.link(arm)
ctx_obj(arm)
bpy.ops.object.mode_set(mode="EDIT")
eb = arm_data.edit_bones
for n in ORDER:
    h, t, p, z = BONES[n]
    b = eb.new(n)
    b.head = Vector(h) * S; b.tail = Vector(t) * S
    b.align_roll(Vector(z))
    if p:
        b.parent = eb[p]
        b.use_connect = False
    b.use_deform = n != "root"
bpy.ops.object.mode_set(mode="OBJECT")

# ------------------------------------------------------------------ weights
# Bone heat on a welded copy (the glTF import splits vertices on UV and normal
# seams; heat on split vertices would weight the two sides of a seam apart and
# crack it), then rule-based fixes, then copied back vertex by vertex.
weld = mesh.copy(); weld.data = mesh.data.copy(); weld.name = "weld"
sc.collection.objects.link(weld)
import bmesh
bm = bmesh.new(); bm.from_mesh(weld.data)
bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
bm.to_mesh(weld.data); bm.free()
ctx_obj(weld); arm.select_set(True); bpy.context.view_layer.objects.active = arm
bpy.ops.object.parent_set(type="ARMATURE_AUTO")
names = [n for n in ORDER if n != "root"]
bi = {n: i for i, n in enumerate(names)}
NV = len(weld.data.vertices)
W = np.zeros((NV, len(names)), np.float64)
gidx = {g.index: g.name for g in weld.vertex_groups}
for v in weld.data.vertices:
    for g in v.groups:
        n = gidx[g.group]
        if n in bi: W[v.index, bi[n]] = g.weight
P = np.array([v.co[:] for v in weld.data.vertices]) / S  # source metres, feet at 0
heat_empty = int((W.sum(1) <= 1e-6).sum())

def cols(*ns): return [bi[n] for n in ns]
def renorm(rows):
    s = W[rows].sum(1, keepdims=True); s[s <= 1e-9] = 1.0; W[rows] /= s
def only(rows, keep, fallback):
    mask = np.zeros(len(names), bool); mask[cols(*keep)] = True
    W[np.ix_(rows, ~mask)] = 0.0
    empty = rows[W[rows].sum(1) <= 1e-6]
    W[empty, bi[fallback]] = 1.0
    renorm(rows)

fixes = {}
FRINGE_BLEND = []  # (rows, root-band blend u, fringe-bone weights) per side
# the fringe bones drive only the strands (set below); bone heat spread them
# over the sleeve
W[:, [bi[n] for n in names if n.startswith("fringe")]] = 0.0
renorm(np.arange(NV))
for s, suf in ((1, "_l"), (-1, "_r")):
    sx = P[:, 0] * s
    armz = np.abs(P[:, 2] - 1.44) < 0.14
    # hand and fingers only beyond the cuff; the cuff only on the arm
    hand_rows = np.where((sx > 0.703) & armz)[0]
    only(hand_rows, ["hand" + suf, "fingers1" + suf, "fingers2" + suf, "fingers3" + suf, "thumb1" + suf, "thumb2" + suf], "hand" + suf)
    cuff_rows = np.where((sx > 0.40) & (sx <= 0.703) & armz)[0]
    only(cuff_rows, ["forearm" + suf, "upper_arm" + suf], "forearm" + suf)
    fixes["hand" + suf] = len(hand_rows); fixes["cuff" + suf] = len(cuff_rows)
    # Sleeve fringes hang 10-15 cm under the arm in a band y 0.025..0.125 (the
    # chest-yoke fringes beside the armpit are in front of and behind it). A
    # strand far from the bone tears when neighbouring vertices follow different
    # bone mixes, so every fringe vertex takes a smooth profile of the sleeve
    # underside's weights along the arm (Gaussian in x, FRINGE_SIGMA): a strand is
    # rigid and the next strand differs only a little. (rig_check: stretch.)
    fr = np.where((sx > 0.255) & (P[:, 2] < 1.385) & (P[:, 2] > 1.15)
                  & (P[:, 1] >= 0.025) & (P[:, 1] <= 0.125))[0]
    src = np.where((sx > 0.245) & (P[:, 2] >= 1.385) & (P[:, 2] <= 1.425)
                   & (P[:, 1] >= 0.0) & (P[:, 1] <= 0.15))[0]
    k = np.exp(-0.5 * ((sx[fr][:, None] - sx[src][None, :]) / FRINGE_SIGMA) ** 2)
    W[fr] = (k @ W[src]) / np.maximum(k.sum(1, keepdims=True), 1e-9)
    renorm(fr)
    # below the root band the strand follows the fringe bones (linear between
    # the two nearest along the arm, so neighbouring strands differ only a
    # little), which hang toward gravity in every pose (see droop()). Applied
    # after the seam smoothing below: smoothing the blend itself diffused
    # uneven forearm weight into the strand tips, which tore 2 mm edges there
    u = np.clip((FRINGE_ROOT_Z - P[fr, 2]) / FRINGE_ROOT, 0.0, 1.0)[:, None]
    Fb = np.zeros((len(fr), len(names)))
    for i in range(len(FRINGE_X)):
        Fb[:, bi["fringe%d%s" % (i + 1, suf)]] = np.interp(sx[fr], FRINGE_X, np.eye(len(FRINGE_X))[i])
    FRINGE_BLEND.append((fr, u, Fb))
    fixes["fringe" + suf] = len(fr)
    # the chest-yoke fringes beside the armpit hang from the chest: no arm bones
    yoke = np.where((sx > 0.10) & (sx < 0.36) & (P[:, 2] < 1.385) & (P[:, 2] > 1.15)
                    & ((P[:, 1] < 0.025) | (P[:, 1] > 0.125)))[0]
    only(yoke, ["spine2", "spine1", "shoulder" + suf], "spine2")
    fixes["yoke_fringe" + suf] = len(yoke)
# head, hair and hat: rigid head above the jaw line, the hat above the brim;
# blended into the bone-heat weights over 1.60..1.65 so the hair at the nape and
# the beard do not tear from the collar when the head turns (rig_check: stretch)
headz = (P[:, 2] > 1.60) & (np.abs(P[:, 0]) < 0.26)
hr = np.where(headz)[0]
hat_w = np.clip((P[hr, 2] - 1.76) / 0.04, 0.0, 1.0)
rig = np.clip((P[hr, 2] - 1.60) / 0.05, 0.0, 1.0)[:, None]
R = np.zeros((len(hr), len(names)))
R[:, bi["hat"]] = hat_w; R[:, bi["head"]] = 1.0 - hat_w
W[hr] = W[hr] * (1.0 - rig) + R * rig
renorm(hr)
fixes["head_hat"] = len(hr)
# coat skirt back: hips at the waist, then the coat_back / coat_tail flap (no
# thighs: bone heat gave the skirt to the thighs, which swung it down through
# the seat when he sits). Ramped in at the sides so the side seams stay put.
W[:, bi["coat_back"]] = 0.0; W[:, bi["coat_tail"]] = 0.0
renorm(np.arange(NV))
# only the back panel (y > 0.11): anything in front of the flap's hinge would
# swing down through the seat when the flap folds back
skirt = np.where((P[:, 1] > 0.11) & (P[:, 2] < 0.97) & (P[:, 2] > 0.66) & (np.abs(P[:, 0]) < 0.3))[0]
zz, yy = P[skirt, 2], P[skirt, 1]
flap = np.clip((0.97 - zz) / 0.05, 0.0, 1.0) * np.clip((yy - 0.11) / 0.04, 0.0, 1.0)
# under the hem (z < 0.80) the legs and crotch sit at y < 0.05: they keep their
# weights; only the hanging hem fringe behind them joins the flap
low = zz < 0.80
flap[low] *= np.clip((yy[low] - 0.07) / 0.04, 0.0, 1.0)
tail = np.clip((0.88 - zz) / 0.04, 0.0, 1.0)
F = np.zeros((len(skirt), len(names)))
F[:, bi["coat_back"]] = 1.0 - tail; F[:, bi["coat_tail"]] = tail
W[skirt] = W[skirt] * (1.0 - flap[:, None]) + F * flap[:, None]
renorm(skirt)
fixes["coat_flap"] = int((flap > 0.01).sum())

# Smooth the seams the rules above leave (sleeve fringe / yoke / sleeve, the
# nape blend band, the coat flap's edges): a few Laplacian passes over the
# welded mesh in those bands only. Equal neighbours (a rigid strand) are left
# as they are; a hard step between neighbours, which tears a surface far from
# its bone, is spread over a few edges. (rig_check: stretch.)
EW = np.empty(len(weld.data.edges) * 2, np.int64); weld.data.edges.foreach_get("vertices", EW)
EW = EW.reshape(-1, 2)
deg = np.zeros(NV); np.add.at(deg, EW[:, 0], 1.0); np.add.at(deg, EW[:, 1], 1.0)
band = (((np.abs(P[:, 0]) > 0.10) & (np.abs(P[:, 0]) < 0.69) & (P[:, 2] > 1.15) & (P[:, 2] < 1.43))
        | ((P[:, 2] > 1.52) & (P[:, 2] < 1.70) & (np.abs(P[:, 0]) < 0.26))
        | ((P[:, 1] > 0.0) & (P[:, 2] > 0.62) & (P[:, 2] < 1.02) & (np.abs(P[:, 0]) < 0.32)))
band_rows = np.where(band)[0]
for _ in range(SMOOTH_PASSES):
    acc = np.zeros_like(W)
    np.add.at(acc, EW[:, 0], W[EW[:, 1]]); np.add.at(acc, EW[:, 1], W[EW[:, 0]])
    avg = acc / np.maximum(deg, 1.0)[:, None]
    W[band_rows] = 0.5 * W[band_rows] + 0.5 * avg[band_rows]
renorm(band_rows)
fixes["smoothed"] = len(band_rows)
# the sleeve fringe strands hand over from the sleeve to their fringe bones
for fr, u, Fb in FRINGE_BLEND:
    W[fr] = W[fr] * (1.0 - u) + Fb * u
    renorm(fr)

# unweighted leftovers take their nearest weighted neighbour
emp = np.where(W.sum(1) <= 1e-6)[0]
if len(emp):
    ok = np.where(W.sum(1) > 1e-6)[0]
    kd = KDTree(len(ok))
    for i, r in enumerate(ok): kd.insert(P[r], i)
    kd.balance()
    for r in emp: W[r] = W[ok[kd.find(P[r])[1]]]
# at most 4 influences, drop slivers, normalise
order = np.argsort(-W, axis=1)
for r in range(NV):
    W[r, order[r, 4:]] = 0.0
W[W < 0.01] = 0.0
renorm(np.arange(NV))

# copy back to the split mesh
kd = KDTree(NV)
for v in weld.data.vertices: kd.insert(v.co, v.index)
kd.balance()
for n in names: mesh.vertex_groups.new(name=n)
far = 0.0
for v in mesh.data.vertices:
    co, i, d = kd.find(v.co); far = max(far, d)
    for j in np.nonzero(W[i])[0]:
        mesh.vertex_groups[names[j]].add([v.index], float(W[i, j]), "REPLACE")
bpy.data.objects.remove(weld)
mesh.parent = arm
mod = mesh.modifiers.new("Armature", "ARMATURE"); mod.object = arm

# ------------------------------------------------------------------ anchors
def anchor(name, bone, pos):
    e = bpy.data.objects.new(name, None)
    e.empty_display_type = "ARROWS"; e.empty_display_size = 0.08
    sc.collection.objects.link(e)
    e.parent = arm
    if bone:
        e.parent_type = "BONE"; e.parent_bone = bone
    bpy.context.view_layer.update()
    e.matrix_world = Matrix.Translation(Vector(pos))
    return e
ANCH_HAND = Vector((0.776, 0.066, 1.418))   # palm centre, just under the palm (rest, src m)
anchor("anchor:hand_l", "hand_l", ANCH_HAND * S)
anchor("anchor:hand_r", "hand_r", Vector((-ANCH_HAND.x, ANCH_HAND.y, ANCH_HAND.z)) * S)
anchor("anchor:head_look", "head", Vector((0.0, -0.095, 1.74)) * S)
anchor("anchor:seat", "root", Vector((0.0, 0.0, SEAT_H)))
bpy.context.view_layer.update()

# -------------------------------------------------------------------- poser
bones = arm.data.bones
REST = {b.name: b.matrix_local.copy() for b in bones}
PAR = {b.name: (b.parent.name if b.parent else None) for b in bones}
LEN = {b.name: b.length for b in bones}
REL = {n: (REST[PAR[n]].inverted() @ REST[n]) if PAR[n] else REST[n] for n in REST}
ANCH_LOCAL = {}
for sfx in ("_l", "_r"):
    e = bpy.data.objects["anchor:hand" + sfx]
    ANCH_LOCAL[sfx] = REST["hand" + sfx].inverted() @ (e.matrix_world.translation)

def qaxis(axis, deg): return Quaternion(Vector(axis), math.radians(deg))

class Pose:
    def __init__(self): self.P = {}
    def default(self, n):
        p = PAR[n]
        if p is None: return REST[n].copy()
        return self.get(p) @ REL[n]
    def get(self, n):
        if n not in self.P: self.P[n] = self.default(n)
        return self.P[n]
    def head(self, n): return self.get(n).translation.copy()
    def tail(self, n): return self.get(n) @ Vector((0, LEN[n], 0))
    def rot(self, n, q, loc=None):  # world-axis rotation about the bone head
        d = self.default(n)
        R = q.to_matrix() @ d.to_3x3()
        self.P[n] = Matrix.Translation(loc if loc is not None else d.translation) @ R.to_4x4()
    def local(self, n, q):
        self.P[n] = self.default(n) @ q.to_matrix().to_4x4()
    def aim(self, n, direction, twist=0.0):
        d = self.default(n)
        y = d.col[1].xyz.normalized(); t = Vector(direction).normalized()
        R = y.rotation_difference(t).to_matrix() @ d.to_3x3()
        if twist: R = Quaternion(t, twist).to_matrix() @ R
        self.P[n] = Matrix.Translation(d.translation) @ R.to_4x4()
    def frame(self, n, Y, Z, loc=None):
        d = self.default(n)
        Y = Vector(Y).normalized(); X = Y.cross(Vector(Z)).normalized(); Z = X.cross(Y)
        R = Matrix((X, Y, Z)).transposed()
        self.P[n] = Matrix.Translation(loc if loc is not None else d.translation) @ R.to_4x4()
    def basis(self):
        out = {}
        for n in ORDER:
            out[n] = self.default(n).inverted() @ self.get(n)
        return out

def two_bone(a, t, l1, l2, pole):
    d = t - a; dist = min(max(d.length, 1e-4), (l1 + l2) * 0.9995)
    dn = d.normalized()
    ca = max(-1.0, min(1.0, (l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist)))
    p = pole - dn * pole.dot(dn); p.normalize()
    elbow = a + dn * (l1 * ca) + p * (l1 * math.sqrt(1 - ca * ca))
    return elbow, a + dn * dist, d.length - dist

def arm_ik(ps, suf, grip, hand_Y, hand_Z, pole, curl, thumb=20.0):
    """Hand frame (Y along the fingers, Z out of the back of the hand) placed so
    its anchor sits on `grip`; the arm reaches it by two-bone IK."""
    s = 1 if suf == "_l" else -1
    Y = Vector(hand_Y).normalized(); X = Y.cross(Vector(hand_Z)).normalized(); Z = X.cross(Y)
    R = Matrix((X, Y, Z)).transposed()
    wrist = grip - R @ ANCH_LOCAL[suf]
    ua, fa = "upper_arm" + suf, "forearm" + suf
    # out of reach: the shoulder (clavicle) swings toward the grip first, the
    # least that closes the gap, up to 30 degrees (call before the arm is posed)
    sh = "shoulder" + suf
    base = ps.get(sh).copy()
    H = base.translation.copy(); T = base @ Vector((0, LEN[sh], 0))
    ax = (T - H).cross(wrist - H)
    if ax.length > 1e-6:
        ax.normalize()
        for k in range(0, 61):
            q = Quaternion(ax, math.radians(0.5 * k))
            M = Matrix.Translation(H) @ q.to_matrix().to_4x4() @ Matrix.Translation(-H) @ base
            ps.P[sh] = M
            if (wrist - (M @ REL[ua]).translation).length <= (LEN[ua] + LEN[fa]) * 0.9995:
                break
    a = ps.head(ua)
    elbow, w2, miss = two_bone(a, wrist, LEN[ua], LEN[fa], Vector(pole))
    ps.aim(ua, elbow - a)
    ps.aim(fa, w2 - elbow)
    # give the forearm half of the wrist's twist so the cuff turns with the hand
    f = ps.get(fa); fy = f.col[1].xyz.normalized()
    fz = f.col[2].xyz; hz = Z - fy * Z.dot(fy)
    if hz.length > 1e-6:
        ang = fz.normalized().angle(hz.normalized())
        sgn = 1.0 if fz.cross(hz).dot(fy) > 0 else -1.0
        ps.aim(fa, w2 - elbow, twist=0.5 * ang * sgn)
    ps.frame("hand" + suf, Y, Z, loc=ps.tail(fa))
    fingers(ps, suf, curl, thumb)
    return miss

def fingers(ps, suf, curl, thumb):
    c = math.radians(curl)
    for k, f in enumerate(("fingers1", "fingers2", "fingers3")):
        ps.local(f + suf, Quaternion((1, 0, 0), -c * (0.9, 1.1, 0.8)[k]))
    t = math.radians(thumb)
    ps.local("thumb1" + suf, Quaternion((0, 0, 1), t * 0.5 * (1 if suf == "_l" else -1)) @ Quaternion((1, 0, 0), -t * 0.6))
    ps.local("thumb2" + suf, Quaternion((1, 0, 0), -t))

def leg_ik(ps, suf, ankle, foot_Y, foot_Z, pole):
    th, sh = "thigh" + suf, "shin" + suf
    a = ps.head(th)
    knee, an2, miss = two_bone(a, ankle, LEN[th], LEN[sh], Vector(pole))
    ps.aim(th, knee - a)
    ps.aim(sh, an2 - knee)
    ps.frame("foot" + suf, foot_Y, foot_Z, loc=ps.tail(sh))
    ps.get("toe" + suf)
    return miss

def spine_chain(ps, pitch, roll, yaw, weights=(0.2, 0.3, 0.3, 0.2)):
    """Spread a world-axis pitch (+ forward), roll (+ to his left) and yaw
    (+ to his left) over spine, spine1, spine2 and neck."""
    for n, w in zip(("spine", "spine1", "spine2", "neck"), weights):
        q = qaxis((0, 0, 1), yaw * w) @ qaxis((0, 1, 0), roll * w) @ qaxis((1, 0, 0), pitch * w)
        ps.rot(n, q)

# ------------------------------------------------------------- seated poses
REST_HIP = Vector(BONES["hips"][0]) * S
def wheel_grip(suf, steer, clock=75.0):
    """Grip point and hand frame on the kart's wheel. `steer` > 0 turns the
    wheel to his left (top of the rim moves to his left). `clock`: degrees
    from the top of the rim (90 = 9 and 3 o'clock)."""
    s = 1 if suf == "_l" else -1
    rot = Quaternion(WHEEL_N, steer)
    th = math.radians(clock)
    radial = (WHEEL_X * (s * math.sin(th)) + WHEEL_U * math.cos(th))
    p = WHEEL_C + rot @ (radial * WHEEL_R)
    Y = -WHEEL_N.copy()                      # fingers through the wheel plane
    Z = rot @ radial                          # back of the hand faces out
    Y = (Y * 0.85 + rot @ WHEEL_U * 0.15).normalized()
    return p, Y, Z

def seated(t, lean=0.0, look=0.0, breathe=True, idle=False):
    """t: seconds; lean -1..1 (+ = turning left, body leans left, wheel turned
    left); look 0..1 (over the right shoulder)."""
    ps = Pose()
    ps.get("root")
    w = 2 * math.pi * t / 2.4            # one breath per 2.4 s loop
    br = math.sin(w) if breathe else 0.0
    # hips: contact at anchor:seat; reclined 12 deg like the raked seat back
    hip_pitch = -8.0
    hip_roll = 6.0 * lean
    hip_yaw = -14.0 * look
    hip_loc = Vector((0.02 * lean, -0.04, SEAT_H + 0.105))
    ps.rot("hips", qaxis((0, 0, 1), hip_yaw) @ qaxis((0, 1, 0), hip_roll) @ qaxis((1, 0, 0), hip_pitch), loc=hip_loc)
    # coat skirt back: he sits on it. The flap tucks under the seat of his
    # trousers and the tail folds back, lying flat on the cushion up to the seat
    # back (it would otherwise hang through the cushion). Bone-local X, so the
    # coat follows the hips' lean and twist. Angles from a rig_check sink grid.
    ps.local("coat_back", qaxis((1, 0, 0), COAT_FOLD[0]))
    ps.local("coat_tail", qaxis((1, 0, 0), COAT_FOLD[1]))
    # spine leans forward to the wheel, into the turn, twists to look back:
    # rolls onto the left hip and turns the chest well round, so the look back
    # reads from the chase camera (it stays leaning forward: sitting up pulls
    # the right shoulder off the wheel)
    pitch = 34.0 + 1.2 * br - 4.0 * look
    roll = 22.0 * lean + 8.0 * look
    yaw = -44.0 * look
    if idle: yaw += 3.0 * math.sin(w * 0.5 + 0.6)
    spine_chain(ps, pitch, roll, yaw)
    # head: level gaze, counter-tilts a little in a lean, turns over the shoulder
    hy = -90.0 * look + 9.0 * lean + (7.0 * math.sin(w * 0.5) if idle else 0.0)
    hp = -24.0 + 12.0 * look + (2.5 * math.sin(w + 1.0) if idle else 0.8 * br)
    hr = -4.0 * lean - 10.0 * look
    ps.rot("head", qaxis((0, 0, 1), hy) @ qaxis((0, 1, 0), hr) @ qaxis((1, 0, 0), hp))
    if idle:
        ps.local("hat", Quaternion((1, 0, 0), math.radians(1.2 * math.sin(w * 0.5 - 0.4))))
    else:
        ps.get("hat")
    # shoulders rise with the breath and sit a little back; looking back, the
    # right shoulder comes forward instead, toward the wheel
    for suf, s in (("_l", 1), ("_r", -1)):
        fwd = s * 6.0 if suf == "_l" else mix(-6.0, 26.0, look)
        ps.rot("shoulder" + suf, qaxis((0, 1, 0), -s * 1.5 * br) @ qaxis((0, 0, 1), fwd))
    steer = STEER_MAX * lean
    miss = {}
    for suf, s in (("_l", 1), ("_r", -1)):
        pole = (s * 0.55, 0.15, -0.8)
        clock = 75.0
        if look > 0.0:
            # looking back over the right shoulder, the hands slide round the
            # rim to where the turned chest can still reach: the nearest place
            # to 75 degrees (10 and 2 o'clock) that the arm reaches, else the
            # place it misses least (1 degree steps, deterministic)
            best = None
            for c in range(30, 201):
                tmp = Pose(); tmp.P = dict(ps.P)
                p, Y, Z = wheel_grip(suf, steer, float(c))
                m = arm_ik(tmp, suf, p, Y, Z, pole, curl=GRIP_CURL, thumb=GRIP_THUMB)
                score = max(0.0, m - 0.002) * 1000.0 + abs(c - 75.0) * 0.3
                if best is None or score < best[0]: best = (score, float(c))
            clock = best[1]
        p, Y, Z = wheel_grip(suf, steer, clock)
        miss[suf] = arm_ik(ps, suf, p, Y, Z, pole, curl=GRIP_CURL, thumb=GRIP_THUMB)
        miss["clock" + suf] = clock
    # legs: feet forward on the pedals, knees up and a little apart
    for suf, s in (("_l", 1), ("_r", -1)):
        ankle = Vector((s * 0.15, -0.80, 0.60))
        fy = Vector((s * 0.12, -0.55, 0.83)); fz = Vector((0, 0.83, 0.55))
        miss["leg" + suf] = leg_ik(ps, suf, ankle, fy, fz, (s * 0.55, -0.5, 0.65))
        ps.get("toe" + suf)
    return ps, miss

def ease(x):
    x = max(0.0, min(1.0, x)); return x * x * (3 - 2 * x)

def mix(a, b, k): return a * (1.0 - k) + b * k

def standing(t, env=0.0, pump=0.0, both=0.0):
    """Standing on the podium. env 0 = the relaxed stand (stand_idle); env 1 =
    the celebration: right fist raised (pump 1 = punched straight up, between
    pumps the fist drops to the hat), left fist on the hip, chest up, a deep
    knee dip between pumps. both 0..1: the left fist leaves the hip and goes
    up too (both arms up in a V for the last pump)."""
    ps = Pose()
    ps.get("root")
    w = 2 * math.pi * t / 2.4
    br = math.sin(w)
    sway = math.sin(w * 0.5)
    dip = env * (1.0 - pump)                 # knees bend between pumps
    hip_loc = REST_HIP + Vector((0.012 * sway * (1 - env), 0, -0.012 + 0.003 * br * (1 - env) - 0.085 * dip - 0.004 * env))
    ps.rot("hips", qaxis((0, 0, 1), -8.0 * env * pump) @ qaxis((0, 1, 0), 1.5 * sway * (1 - env)) @ qaxis((1, 0, 0), 4.0 * env + 6.0 * dip), loc=hip_loc)
    # chest: hunched a little in the dip, thrown back and up with each pump
    spine_chain(ps, mix(1.5 + 0.8 * br, 10.0 * (1.0 - pump) - 14.0 * pump, env),
                mix(-1.5 * sway, -7.0 * (1.0 - both) - 7.0 * pump * (1.0 - both), env), 10.0 * env * pump * (1.0 - both))
    ps.rot("head", qaxis((0, 0, 1), mix(4.0 * math.sin(w * 0.5 + 1.2), -6.0 * (1.0 - both), env))
           @ qaxis((1, 0, 0), mix(-1.0 + 0.6 * br, -6.0 - 22.0 * pump, env)))
    ps.get("hat")
    # Lowering an arm from the T-pose is +s about Y; raising it is -s.
    for suf, s in (("_l", 1), ("_r", -1)):
        r = env if suf == "_r" else env * both       # raised fist
        hp = env * (1.0 - both) if suf == "_l" else 0.0  # left fist on the hip
        ps.rot("shoulder" + suf, qaxis((0, 1, 0), mix(s * (3.0 - 1.0 * br), -s * (10.0 + 14.0 * pump), r)))
        # raised: 62 degrees over the horizontal between pumps, 86 at the top
        ua_y = mix(s * 72.0, -s * (62.0 + 24.0 * pump), r)
        ua_y = mix(ua_y, s * 45.0, hp)
        ua_z = -s * (10.0 + 8.0 * pump) * r * (1.0 - 0.5 * both)
        ps.rot("upper_arm" + suf, qaxis((0, 0, 1), ua_z) @ qaxis((1, 0, 0), mix(-6.0, 12.0, hp) * (1 - r)) @ qaxis((0, 1, 0), ua_y))
        fa_y = -s * 70.0 * (1.0 - pump) * r + s * 90.0 * hp
        ps.rot("forearm" + suf, qaxis((0, 1, 0), fa_y) @ qaxis((1, 0, 0), -14.0 * (1 - r) * (1 - hp)))
        ps.get("hand" + suf)
        fingers(ps, suf, 22.0 + 75.0 * max(r, hp), 10.0 + 50.0 * max(r, hp))
    for suf, s in (("_l", 1), ("_r", -1)):
        ankle = Vector(BONES["shin" + suf][1]) * S
        rest_foot = REST["foot" + suf]
        leg_ik(ps, suf, ankle, rest_foot.col[1].xyz, rest_foot.col[2].xyz, (s * 0.1, -1.0, 0.0))
    return ps

def celebrate(t):
    """2.5 s: starts on the stand pose (stand_idle frame 0), three fist pumps
    with a deep knee dip between them, both fists up in a V for the last one
    (held a moment), then eases back to standing."""
    env = ease(t / 0.3) * (1.0 - ease((t - 2.1) / 0.4))   # 0 at both ends
    pump = 0.0
    for c in (0.55, 1.0, 1.5):
        pump = max(pump, math.exp(-((t - c) / 0.12) ** 2))
    if t > 1.5:   # the last pump holds at the top
        pump = max(pump, 1.0 - ease((t - 1.75) / 0.35))
    both = ease((t - 1.2) / 0.3)
    return standing(t, env, pump, both)

DOWN = Vector((0.0, 0.0, -1.0))
def droop(ps):
    """Sleeve fringes hang: every fringe bone turns from where the arm carries
    it most of the way (FRINGE_DROOP) toward straight down, about the strand
    roots. With the arm raised the strands lie down along the sleeve instead
    of sticking out sideways."""
    for suf in ("_l", "_r"):
        for i in range(len(FRINGE_X)):
            n = "fringe%d%s" % (i + 1, suf)
            y = ps.default(n).col[1].xyz.normalized()
            q = Quaternion().slerp(y.rotation_difference(DOWN), FRINGE_DROOP)
            ps.aim(n, q @ y)
    return ps

# -------------------------------------------------------------------- clips
CLIPS = [
    # name, frames (last frame == first frame for loops), loop, fn(t)
    ("idle_seated", 72, True, lambda t: seated(t, idle=True)[0]),
    ("lean_left", 72, True, lambda t: seated(t, lean=1.0)[0]),
    ("lean_center", 72, True, lambda t: seated(t, lean=0.0)[0]),
    ("lean_right", 72, True, lambda t: seated(t, lean=-1.0)[0]),
    ("look_back", 39, False, None),
    ("stand_idle", 72, True, lambda t: standing(t)),
    ("celebrate", 75, False, lambda t: celebrate(t)),
]
def look_back_pose(t):
    # 1.3 s: turns in 0.3 s, holds the look back 0.7 s, turns back in 0.3 s
    k = ease(t / 0.3) * (1.0 - ease((t - 1.0) / 0.3))
    return seated(t, look=k)[0]

pref = bpy.context.preferences.edit
pref.keyframe_new_interpolation_type = "LINEAR"
ad = arm.animation_data_create()
ctx_obj(arm)
bpy.ops.object.mode_set(mode="POSE")
for pb in arm.pose.bones: pb.rotation_mode = "QUATERNION"
report = {"scale": S, "clips": {}, "ik_miss_mm": {}}
for name, frames, loop, fn in CLIPS:
    if name == "look_back": fn = look_back_pose
    act = bpy.data.actions.new(name); act.use_fake_user = True
    ad.action = act
    prev = {}
    for f in range(frames + 1):
        t = (f % frames if loop else f) / FPS
        ps = droop(fn(t))
        B = ps.basis()
        for n in ORDER:
            pb = arm.pose.bones[n]
            loc, q, _ = B[n].decompose()
            if n in prev and prev[n].dot(q) < 0: q = -q
            prev[n] = q
            pb.location = loc; pb.rotation_quaternion = q; pb.scale = (1, 1, 1)
            pb.keyframe_insert("location", frame=f, group=n)
            pb.keyframe_insert("rotation_quaternion", frame=f, group=n)
    act.use_frame_range = True; act.frame_start = 0; act.frame_end = frames
    act["evani_loop"] = loop
    report["clips"][name] = {"frames": frames + 1, "seconds": round(frames / FPS, 3), "loop": loop}
    tr = ad.nla_tracks.new(); tr.name = name
    tr.strips.new(name, 0, act); tr.mute = True
    ad.action = None
for suf in ("_l", "_r"):
    report["ik_miss_mm"]["seated_hand" + suf] = round(1000 * seated(0.0)[1][suf], 1)
    report["ik_miss_mm"]["lean_left_hand" + suf] = round(1000 * seated(0.0, lean=1.0)[1][suf], 1)
    report["ik_miss_mm"]["lean_right_hand" + suf] = round(1000 * seated(0.0, lean=-1.0)[1][suf], 1)
    lb = seated(0.65, look=1.0)[1]
    report["ik_miss_mm"]["look_back_hand" + suf] = round(1000 * lb[suf], 1)
    report["ik_miss_mm"]["look_back_clock" + suf] = lb["clock" + suf]
    report["ik_miss_mm"]["seated_leg" + suf] = round(1000 * seated(0.0)[1]["leg" + suf], 1)
bpy.ops.object.mode_set(mode="OBJECT")
for pb in arm.pose.bones:
    pb.location = (0, 0, 0); pb.rotation_quaternion = (1, 0, 0, 0)
sc.frame_start = 0; sc.frame_end = 74

# ------------------------------------------------------------------- extras
joints = [n for n in ORDER]
sc["evani"] = {
    "source": "art-source/meshy/frontier-marshal/frontier-marshal.master.glb",
    "scale_from_source": S, "units": "m", "frame": "+X left, +Y up, +Z forward",
    "joints": len(joints),
    "anchors": ["anchor:hand_l", "anchor:hand_r", "anchor:head_look", "anchor:seat"],
    "clips": {k: dict(v) for k, v in report["clips"].items()},
    "fps": FPS,
}
report.update({"joints": len(joints), "joint_names": joints, "heat_unweighted": heat_empty,
               "weight_fixes": fixes, "weld_copy_max_dist": far})

# ------------------------------------------------------------------- export
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
bpy.ops.export_scene.gltf(
    filepath=OUT_GLB, export_format="GLB", export_yup=True, export_extras=True,
    export_image_format="AUTO", export_materials="EXPORT", export_apply=False,
    export_skins=True, export_def_bones=False, export_animations=True,
    export_animation_mode="ACTIONS", export_force_sampling=True, export_frame_step=1,
    export_reset_pose_bones=True, export_anim_single_armature=True,
    export_optimize_animation_size=False)
report["glb_bytes"] = os.path.getsize(OUT_GLB)
print("MARSHAL_REPORT", json.dumps(report))
