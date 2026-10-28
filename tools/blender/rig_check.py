# Headless Blender script: measure a rigged character's clips against limits
# from a small JSON config, so deformation errors found on one model become a
# check every rigged model runs (Stage 3; docs/stage3/source-models.md "Rig check").
#
# Run (pinned Blender 5.2.2 LTS):
#   blender -b --factory-startup -t 2 --python tools/blender/rig_check.py -- \
#       <model .glb or .blend> <config .json> [--out report.json] [--report-only]
#       [--clips a,b]   (only these clips, for a quick look while working)
# Exit code 1 when any limit is exceeded (0 with --report-only). One line per
# clip and check is printed ("RIG_CHECK ..."), then "RIG_CHECK_RESULT pass|fail".
#
# Config (all positions in the model's glTF frame: +Y up, metres; every section
# is optional; "clips" omitted = every clip in the file):
# {
#   "armature": "marshal_rig",            # optional; default: the only armature
#   "meshes": ["frontier_marshal"],       # optional; default: meshes skinned to it
#   "frame_step": 1,
#   "anchor_gaps": [                      # (a) anchor to target distance
#     {"name": "left hand on rim", "anchor": "anchor:hand_l",
#      "target": {"ring": {"center": [0, 0.98, 0.34], "normal": [0, 0.54, 0.84], "radius": 0.23}},
#      "clips": ["idle_seated"], "max_mm": 15, "mean_mm": 10}
#   ],                                    # target: {"anchor": name} | {"point": [x,y,z]} | {"ring": {...}}
#   "colliders": [                        # (b) vertices sunk inside a box
#     {"name": "seat", "box": {"center": [0, 0.56, -0.30], "size": [0.72, 0.16, 0.62], "rot_deg": [0, 0, 0]},
#      "clips": ["idle_seated"], "ignore_mm": 5, "max_count": 0, "max_depth_mm": 10,
#      "only_groups": [], "exclude_groups": []}
#   ],
#   "stretch": [                          # (c) edge stretch from the rest pose
#     {"name": "all", "clips": ["celebrate"], "max_ratio": 1.6, "min_ratio": 0.4,
#      "min_change_mm": 2, "max_count": 0, "only_groups": [], "exclude_groups": [],
#      "region": [{"center": [0.6, 1.46, 0], "size": [0.64, 0.28, 0.5]}]}
#   ],
#   "hang": [                             # (d) hanging strands: strand-edge angle from straight down
#     {"name": "fringes", "region": [...], "clips": ["celebrate"], "root_mm": 15,
#      "edge_mm": 3, "rest_deg": 20, "max_deg": 60, "max_count": 0, "max_mean_deg": 30,
#      "min_len_ratio": 0.5, "max_len_ratio": 2.0}
#   ]                                     # strand edge: below the top root_mm, edge_mm long,
#                                         # within rest_deg of down at rest; count = edges over
#                                         # max_deg or outside the length ratios
# }
# Vertex selection (colliders and stretch): "only_groups"/"exclude_groups" by
# the vertex's strongest vertex group (bone) name, a name ending in "*" is a
# prefix; "region"/"exclude_region": boxes in the rest (bind) pose, the vertex
# must be inside every "region" box and outside every "exclude_region"
# box. rot_deg is XYZ Euler degrees in the glTF frame. A vertex's stretch is the
# largest ratio of a connected edge's posed length to its rest length (and the
# smallest, for "min_ratio"); edges whose length changes by under min_change_mm
# (default 2) count as unchanged, so tiny edges do not flood the report.
# Flagged vertices are reported by strongest group with their rest-pose centre.
import bpy, sys, os, json, math
import numpy as np
from mathutils import Vector, Matrix, Euler

argv = sys.argv[sys.argv.index("--") + 1:]
MODEL, CONFIG = argv[0], argv[1]
OUT = argv[argv.index("--out") + 1] if "--out" in argv else None
REPORT_ONLY = "--report-only" in argv
with open(CONFIG, encoding="utf-8") as f:
    CFG = json.load(f)

# glTF (+Y up, +Z forward) <-> Blender (+Z up, -Y forward)
def g2b(p): return Vector((p[0], -p[2], p[1]))
G2B = Matrix(((1, 0, 0), (0, 0, -1), (0, 1, 0)))

# ------------------------------------------------------------------- load
if MODEL.lower().endswith(".blend"):
    bpy.ops.wm.open_mainfile(filepath=MODEL)
else:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=MODEL)
sc = bpy.context.scene
def fail_config(msg):
    print("RIG_CHECK_ERROR", msg); sys.exit(2)
arms = [o for o in bpy.data.objects if o.type == "ARMATURE"]
if CFG.get("armature"):
    named = [o for o in arms if o.name == CFG["armature"]]
    if not named:
        fail_config("armature %r not found; armatures: %s" % (CFG["armature"], [o.name for o in arms]))
    arms = named
if len(arms) != 1:
    print("RIG_CHECK_ERROR expected one armature, found", [o.name for o in arms]); sys.exit(2)
arm = arms[0]
def skinned(o):
    return o.type == "MESH" and any(m.type == "ARMATURE" and m.object == arm for m in o.modifiers)
meshes = [o for o in bpy.data.objects if skinned(o)]
if CFG.get("meshes"):
    missing = [n for n in CFG["meshes"] if n not in [o.name for o in meshes]]
    if missing:
        fail_config("meshes %s not found among the meshes skinned to %s: %s" % (missing, arm.name, [o.name for o in meshes]))
    meshes = [o for o in meshes if o.name in CFG["meshes"]]
if not meshes:
    fail_config("no mesh skinned to %s" % arm.name)
# every anchor the config names must exist (a typo must not crash or pass)
for e in CFG.get("anchor_gaps", []):
    for n in (e.get("anchor"), e.get("target", {}).get("anchor")):
        if n is not None and n not in bpy.data.objects:
            fail_config("anchor %r (entry %r) not found in the model" % (n, e.get("name", "")))
for o in bpy.data.objects:
    if o.animation_data:
        for tr in o.animation_data.nla_tracks: tr.mute = True
ad = arm.animation_data_create()
STEP = max(1, int(CFG.get("frame_step", 1)))

# clips: every action that animates this armature's bones
def action_fcurves(a):
    """F-curves of an action: legacy (a.fcurves) and Blender 4.4+ layered
    actions (layers > strips > channelbags)."""
    out = list(getattr(a, "fcurves", None) or [])
    for layer in getattr(a, "layers", []):
        for strip in layer.strips:
            for bag in getattr(strip, "channelbags", []):
                out.extend(bag.fcurves)
    return out
BONES = set(b.name for b in arm.data.bones)
def animates_bones(a):
    for fc in action_fcurves(a):
        p = fc.data_path
        if p.startswith('pose.bones["'):
            if p[len('pose.bones["'):].split('"]')[0] in BONES: return True
    return False
def clip_names():
    return sorted(a.name for a in bpy.data.actions if animates_bones(a))
ALL_CLIPS = clip_names()
def clip_action(name):
    if name in bpy.data.actions: return bpy.data.actions[name]
    for a in bpy.data.actions:   # glTF import may suffix the armature name
        if a.name.startswith(name + "_") or a.name.startswith(name + "."): return a
    return None
ONLY = argv[argv.index("--clips") + 1].split(",") if "--clips" in argv else None
def clips_for(entry):
    want = entry.get("clips") or ALL_CLIPS
    return [c for c in want if ONLY is None or c in ONLY]

# ---------------------------------------------------------- mesh sampling
dg = bpy.context.evaluated_depsgraph_get()
def deformed():
    """World-space vertex positions of every checked mesh, one array."""
    dg.update()
    out = []
    for o in meshes:
        ev = o.evaluated_get(dg)
        me = ev.to_mesh()
        a = np.empty(len(me.vertices) * 3, np.float64); me.vertices.foreach_get("co", a)
        a = a.reshape(-1, 3)
        M = np.array(ev.matrix_world)
        out.append(a @ M[:3, :3].T + M[:3, 3])
        ev.to_mesh_clear()
    return np.concatenate(out)

# topology and strongest group per vertex (static)
EDGES, GROUP = [], []
off = 0
for o in meshes:
    me = o.data
    e = np.empty(len(me.edges) * 2, np.int64); me.edges.foreach_get("vertices", e)
    EDGES.append(e.reshape(-1, 2) + off)
    names = {g.index: g.name for g in o.vertex_groups}
    for v in me.vertices:
        best, bw = "", -1.0
        for g in v.groups:
            if g.weight > bw: best, bw = names.get(g.group, ""), g.weight
        GROUP.append(best)
    off += len(me.vertices)
EDGES = np.unique(np.sort(np.concatenate(EDGES), axis=1), axis=0)   # each edge once
GROUP = np.array(GROUP)
NV = len(GROUP)

def select(entry):
    m = np.ones(NV, bool)
    def match(pats):
        r = np.zeros(NV, bool)
        for p in pats:
            r |= np.char.startswith(GROUP, p[:-1]) if p.endswith("*") else (GROUP == p)
        return r
    if entry.get("only_groups"): m &= match(entry["only_groups"])
    if entry.get("exclude_groups"): m &= ~match(entry["exclude_groups"])
    for key, want in (("region", True), ("exclude_region", False)):
        for b in entry.get(key, []):
            c, R, half = box_frame(b)
            L = (REST - c) @ R
            inside = np.all(np.abs(L) <= half, axis=1)
            m &= inside if want else ~inside
    return m

def top_groups(mask, n=4):
    """The strongest-group names of the flagged vertices, with a count and the
    rest-pose (bind) centre of each, in the glTF frame, so a report says where."""
    u, c = np.unique(GROUP[mask], return_counts=True)
    o = np.argsort(-c)[:n]
    out = {}
    for i in o:
        sel = mask & (GROUP == u[i])
        p = REST[sel].mean(axis=0)
        out[str(u[i])] = {"count": int(c[i]), "rest_at": [round(float(p[0]), 3), round(float(p[2]), 3), round(float(-p[1]), 3)]}
    return out

# rest pose
arm.data.pose_position = "REST"
REST = deformed()
arm.data.pose_position = "POSE"
REST_LEN = np.linalg.norm(REST[EDGES[:, 0]] - REST[EDGES[:, 1]], axis=1)
EOK = REST_LEN > 1e-5

# ------------------------------------------------------------ targets
def anchor_pos(name):
    """An anchor's evaluated world position (after constraints and drivers)."""
    return bpy.data.objects[name].evaluated_get(dg).matrix_world.translation.copy()

def target_dist(tgt, p):
    if "anchor" in tgt:
        return (anchor_pos(tgt["anchor"]) - p).length
    if "point" in tgt:
        return (g2b(tgt["point"]) - p).length
    r = tgt["ring"]
    c = g2b(r["center"]); n = g2b(r["normal"]).normalized()
    d = p - c; h = d.dot(n); radial = d - n * h
    rl = radial.length
    return math.sqrt(h * h + (rl - r["radius"]) ** 2)

def box_frame(b):
    R = Euler([math.radians(x) for x in b.get("rot_deg", [0, 0, 0])], "XYZ").to_matrix()
    Rb = G2B @ R @ G2B.transposed()            # the box's axes in Blender
    c = np.array(g2b(b["center"])); half = np.array(b["size"], float) / 2.0
    half = np.array([half[0], half[2], half[1]])  # glTF (x, y, z) sizes -> Blender (x, z, y) axes
    return c, np.array(Rb), half

# --------------------------------------------------------------- run
results = []
def frames_of(act):
    a, b = act.frame_range
    return list(range(int(round(a)), int(round(b)) + 1, STEP))

def pose_frame(act, f):
    ad.action = act
    if hasattr(ad, "action_slot") and act.slots: ad.action_slot = act.slots[0]
    sc.frame_set(f)

work = {}   # clip -> list of (kind, index)
for k, kind in (("anchor_gaps", "gap"), ("colliders", "sink"), ("stretch", "stretch"), ("hang", "hang")):
    for i, e in enumerate(CFG.get(k, [])):
        for c in clips_for(e): work.setdefault(c, []).append((kind, i))
# vertex selections depend only on the rest pose: work them out once
MASK = {("sink", i): select(e) for i, e in enumerate(CFG.get("colliders", []))}
MASK.update({("stretch", i): select(e) for i, e in enumerate(CFG.get("stretch", []))})
# hanging strands, measured edge by edge (no strand has to be found: fused
# strands, joined at their roots or by thin webs, fall into a few connected
# pieces, and tips paired with roots by distance pick the wrong strand). A
# strand edge is an edge of the selection, below its top root_mm (the surface
# the strands hang from, where they bend with it), at least edge_mm long and
# within rest_deg of straight down in the rest pose. Each frame, every strand
# edge's angle from straight down is taken, upper end to lower end as at
# rest. Webs between strands run across, not down, so they are not measured.
HANG = {}
for i, e in enumerate(CFG.get("hang", [])):
    m = select(e)
    if not m.any():
        fail_config("hang entry %r selects no vertices" % e.get("name", ""))
    top = REST[m, 2].max()
    m &= REST[:, 2] < top - e.get("root_mm", 15) / 1000.0
    es = EDGES[m[EDGES[:, 0]] & m[EDGES[:, 1]]]
    # upper end first
    up = REST[es[:, 0], 2] >= REST[es[:, 1], 2]
    es = np.where(up[:, None], es, es[:, ::-1])
    d = REST[es[:, 1]] - REST[es[:, 0]]
    ln = np.linalg.norm(d, axis=1)
    down = -d[:, 2] / np.maximum(ln, 1e-9) >= math.cos(math.radians(e.get("rest_deg", 20.0)))
    es = es[down & (ln >= e.get("edge_mm", 3.0) / 1000.0)]
    # seam-split copies of one edge share positions: measure each once
    _, first = np.unique(np.round(np.concatenate([REST[es[:, 0]], REST[es[:, 1]]], axis=1) * 1e4).astype(np.int64),
                         axis=0, return_index=True)
    es = es[np.sort(first)]
    if not len(es):
        fail_config("hang entry %r has no strand edge (edge_mm long, within rest_deg of down)" % e.get("name", ""))
    HANG[i] = es
HANG_LEN = {i: np.linalg.norm(REST[es[:, 1]] - REST[es[:, 0]], axis=1) for i, es in HANG.items()}

acc = {}
for clip in sorted(work):
    act = clip_action(clip)
    if act is None:
        results.append({"clip": clip, "check": "-", "ok": False, "error": "clip not found"}); continue
    for f in frames_of(act):
        pose_frame(act, f)
        bpy.context.view_layer.update(); dg.update()
        need_mesh = any(k != "gap" for k, _ in work[clip])
        V = deformed() if need_mesh else None
        for kind, i in work[clip]:
            a = acc.setdefault((clip, kind, i), {"vals": [], "frames": 0, "sunk": set(), "worst": 0.0, "wf": 0, "over": set(), "lo": 1e9, "hi": 0.0})
            a["frames"] += 1
            if kind == "gap":
                e = CFG["anchor_gaps"][i]
                p = anchor_pos(e["anchor"])
                a["vals"].append(1000.0 * target_dist(e["target"], p))
                a.setdefault("at", []).append(f)
            elif kind == "sink":
                e = CFG["colliders"][i]
                c, R, half = box_frame(e["box"])
                L = (V - c) @ R            # local coordinates along the box axes
                depth = np.min(half - np.abs(L), axis=1)
                ign = e.get("ignore_mm", 0.0) / 1000.0
                inside = (depth > ign) & MASK[("sink", i)]
                idx = np.nonzero(inside)[0]
                a["sunk"].update(idx.tolist())
                if len(idx):
                    d = float(depth[idx].max())
                    if d > a["worst"]: a["worst"], a["wf"] = d, f
            elif kind == "hang":
                e = CFG["hang"][i]
                es = HANG[i]
                v = V[es[:, 1]] - V[es[:, 0]]
                ang = np.degrees(np.arccos(np.clip(-v[:, 2] / np.maximum(np.linalg.norm(v, axis=1), 1e-9), -1.0, 1.0)))
                # a strand keeps its length while it hangs (webs between strands
                # may fold; strand edges may not)
                lr = np.linalg.norm(v, axis=1) / np.maximum(HANG_LEN[i], 1e-9)
                a["lo"] = min(a["lo"], float(lr.min())); a["hi_len"] = max(a.get("hi_len", 0.0), float(lr.max()))
                a["over"].update(np.nonzero((ang > e.get("max_deg", 180.0)) | (lr < e.get("min_len_ratio", 0.0))
                                            | (lr > e.get("max_len_ratio", 1e9)))[0].tolist())
                a["vals"].append(float(ang.mean()))
                hi = float(ang.max())
                if hi > a["hi"]: a["hi"], a["wf"] = hi, f
            else:
                e = CFG["stretch"][i]
                L = np.linalg.norm(V[EDGES[:, 0]] - V[EDGES[:, 1]], axis=1)
                ratio = np.where(EOK, L / np.where(EOK, REST_LEN, 1.0), 1.0)
                # tiny edges change ratio wildly without a visible change: a
                # change under min_change_mm counts as no stretch
                ratio[np.abs(L - REST_LEN) < e.get("min_change_mm", 2.0) / 1000.0] = 1.0
                vmax = np.ones(NV); vmin = np.ones(NV)
                np.maximum.at(vmax, EDGES[:, 0], ratio); np.maximum.at(vmax, EDGES[:, 1], ratio)
                np.minimum.at(vmin, EDGES[:, 0], ratio); np.minimum.at(vmin, EDGES[:, 1], ratio)
                sel = MASK[("stretch", i)]
                bad = sel & ((vmax > e.get("max_ratio", 1e9)) | (vmin < e.get("min_ratio", 0.0)))
                a["over"].update(np.nonzero(bad)[0].tolist())
                hi = float(vmax[sel].max()) if sel.any() else 1.0
                lo = float(vmin[sel].min()) if sel.any() else 1.0
                if hi > a["hi"]:
                    a["hi"], a["wf"] = hi, f
                    # the five worst edges in the selection at this frame
                    es = sel[EDGES[:, 0]] | sel[EDGES[:, 1]]
                    cand = np.nonzero(es)[0]
                    # seam-split copies of one edge share positions: list each once
                    top, seen = [], set()
                    for k in cand[np.argsort(-ratio[cand])]:
                        key = tuple(sorted((tuple(np.round(REST[EDGES[k, 0]], 4)), tuple(np.round(REST[EDGES[k, 1]], 4)))))
                        if key in seen: continue
                        seen.add(key); top.append(k)
                        if len(top) == 5: break
                    a["edges"] = [{"ratio": round(float(ratio[k]), 2), "rest_mm": round(1000 * float(REST_LEN[k]), 1),
                                   "groups": [str(GROUP[EDGES[k, 0]]), str(GROUP[EDGES[k, 1]])],
                                   "rest_at": [round(float(REST[EDGES[k, 0], 0]), 3), round(float(REST[EDGES[k, 0], 2]), 3),
                                               round(float(-REST[EDGES[k, 0], 1]), 3)]} for k in top]
                if lo < a["lo"]: a["lo"], a["lf"] = lo, f

ok_all = True
for (clip, kind, i), a in sorted(acc.items()):
    if kind == "gap":
        e = CFG["anchor_gaps"][i]
        mx, mean = max(a["vals"]), sum(a["vals"]) / len(a["vals"])
        ok = mx <= e.get("max_mm", 1e9) and mean <= e.get("mean_mm", 1e9)
        r = {"clip": clip, "check": "gap", "name": e.get("name", e["anchor"]), "max_mm": round(mx, 1),
             "mean_mm": round(mean, 1), "worst_frame": int(a["at"][int(np.argmax(a["vals"]))]),
             "limit_max_mm": e.get("max_mm"), "limit_mean_mm": e.get("mean_mm"), "ok": ok}
    elif kind == "sink":
        e = CFG["colliders"][i]
        n = len(a["sunk"]); d = 1000.0 * a["worst"]
        ok = n <= e.get("max_count", 1e9) and d <= e.get("max_depth_mm", 1e9)
        m = np.zeros(NV, bool); m[list(a["sunk"])] = True
        r = {"clip": clip, "check": "sink", "name": e.get("name", "box"), "count": n, "max_depth_mm": round(d, 1),
             "worst_frame": a["wf"], "groups": top_groups(m), "limit_count": e.get("max_count"),
             "limit_depth_mm": e.get("max_depth_mm"), "ok": ok}
    elif kind == "hang":
        e = CFG["hang"][i]
        n = len(a["over"]); mean = max(a["vals"])
        ok = n <= e.get("max_count", 0) and mean <= e.get("max_mean_deg", 180.0)
        m = np.zeros(NV, bool); m[HANG[i][sorted(a["over"]), 0]] = True
        r = {"clip": clip, "check": "hang", "name": e.get("name", "hang"), "edges": int(len(HANG[i])),
             "max_deg": round(a["hi"], 1), "worst_mean_deg": round(mean, 1), "worst_frame": a["wf"],
             "min_len_ratio": round(a["lo"], 3), "max_len_ratio": round(a.get("hi_len", 1.0), 3),
             "limit_min_len_ratio": e.get("min_len_ratio"), "limit_max_len_ratio": e.get("max_len_ratio"),
             "count": n, "groups": top_groups(m), "limit_max_deg": e.get("max_deg"),
             "limit_mean_deg": e.get("max_mean_deg"), "limit_count": e.get("max_count", 0), "ok": ok}
    else:
        e = CFG["stretch"][i]
        n = len(a["over"])
        ok = n <= e.get("max_count", 0)
        m = np.zeros(NV, bool); m[list(a["over"])] = True
        r = {"clip": clip, "check": "stretch", "name": e.get("name", "stretch"), "count": n,
             "max_ratio": round(a["hi"], 3), "min_ratio": round(a["lo"], 3), "worst_frame": a["wf"],
             "min_frame": a.get("lf", 0),
             "groups": top_groups(m), "worst_edges": a.get("edges", []),
             "limit_max_ratio": e.get("max_ratio"), "limit_min_ratio": e.get("min_ratio"),
             "limit_count": e.get("max_count", 0), "ok": ok}
    results.append(r)
for r in results:
    ok_all &= r["ok"]
    print("RIG_CHECK", json.dumps(r))
report = {"model": os.path.basename(MODEL), "config": os.path.basename(CONFIG), "vertices": NV,
          "clips": ALL_CLIPS, "results": results, "pass": ok_all}
if OUT:
    with open(OUT, "w", encoding="utf-8") as f: json.dump(report, f, indent=1)
print("RIG_CHECK_RESULT", "pass" if ok_all else "fail")
sys.stdout.flush()
if not ok_all and not REPORT_ONLY:
    os._exit(1)
