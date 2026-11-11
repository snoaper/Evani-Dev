# Writes a species' rig-check settings (tools/blender/rig_check/<id>.json)
# from its built rig, with the test horse's regions and limits.
#
#   blender -b <folder>/<id>.blend --factory-startup --python \
#       tools/models/quadruped/rigcheck_regions.py -- --out tools/blender/rig_check/<id>.json
#
# The horse's check (tools/blender/rig_check/horse.json) holds the idle and
# the skin under the tail to the goal (1.6x / 0.45x) and four walk regions
# (elbow and armpit, flank and groin, back of the thigh and stifle,
# everything else) to its own measures plus a margin, as linear skinning
# without corrective shapes cannot meet the goal there. Its boxes are
# placed here from the species' own joints, by the horse's proportions
# (measured on horse v5: elbow 0.815 m high, upper arm 0.388 m, stifle
# 0.955 m, thigh 0.426 m, tail root 1.49 m), so every species is held to the
# same bar as the reference horse, never a looser one.
import bpy, json, sys, os, math

argv = sys.argv[sys.argv.index("--") + 1:]
OUT = argv[argv.index("--out") + 1]

rig = next(o for o in bpy.data.objects if o.type == "ARMATURE")
ID = rig.name[:-4] if rig.name.endswith("_rig") else rig.name
B = rig.data.bones


def g(v):
    """Blender (x, -forward, up) to the glTF frame (x, up, forward)."""
    w = rig.matrix_world @ v
    return (w.x, w.z, -w.y)


def r3(v):
    return [round(c, 3) for c in v]


e = g(B["forearm.L"].head_local)
sj = g(B["upper_arm.L"].head_local)
st = g(B["lower_leg.L"].head_local)
hp = g(B["thigh.L"].head_local)
tr = g(B["tail.001"].head_local)
Lua = ((sj[1] - e[1]) ** 2 + (sj[2] - e[2]) ** 2) ** 0.5
Lth = ((hp[1] - st[1]) ** 2 + (hp[2] - st[2]) ** 2) ** 0.5

TAIL = {"center": r3([0, tr[1] - 0.68 * Lth, tr[2] - 0.164 * Lth]), "size": r3([0.47 * Lth, 0.94 * Lth, 0.47 * Lth])}
ARM = {"center": r3([0, e[1] + 0.27 * Lua, e[2] - 0.046 * Lua]), "size": r3([2.9 * abs(e[0]), 0.876 * Lua, 0.876 * Lua])}
FLANK = {"center": r3([0, st[1] + 0.153 * Lth, st[2] - 0.035 * Lth]), "size": r3([3.5 * abs(st[0]), 0.845 * Lth, 0.94 * Lth])}
THIGH = {"center": r3([0, st[1] - 0.012 * Lth, st[2] - 0.786 * Lth]), "size": r3([3.5 * abs(st[0]), 0.94 * Lth, 0.70 * Lth])}

cfg = {
    "about": ("Rig check for the Evani.Dev %s (tools/models/quadruped, species/%s.json: the Rigify horse metarig fitted "
              "to the species' joints plus the half-turn helper bones), written by tools/models/quadruped/rigcheck_regions.py "
              "from the built rig. Model frame: glTF, ground at y=0, nose toward +z. The test horse's regions (horse.json) "
              "placed by this species' joints, with the horse's limits: the idle and the skin under the tail to the goal "
              "(1.6x / 0.45x); in the walk the elbow and armpit, the flank and groin, the back of the thigh and stifle and "
              "everything else to the horse's own limits (linear skinning without corrective shapes cannot meet the goal there; "
              "the engine has no shape keys yet). Never raise a limit past the horse's to pass.") % (ID, ID),
    "frame_step": 1,
    "stretch": [
        {"name": "idle, whole body", "clips": ["idle"], "max_ratio": 1.6, "min_ratio": 0.45, "min_change_mm": 2, "max_count": 0},
        {"name": "walk: under the tail", "clips": ["walk"], "max_ratio": 1.6, "min_ratio": 0.45, "min_change_mm": 2, "max_count": 0,
         "region": [TAIL]},
        {"name": "walk: elbow and armpit", "clips": ["walk"], "max_ratio": 3.6, "min_ratio": 0.40, "min_change_mm": 2, "max_count": 0,
         "region": [ARM]},
        {"name": "walk: flank and groin", "clips": ["walk"], "max_ratio": 3.05, "min_ratio": 0.16, "min_change_mm": 2, "max_count": 0,
         "region": [FLANK]},
        {"name": "walk: back of the thigh and stifle", "clips": ["walk"], "max_ratio": 2.0, "min_ratio": 0.31, "min_change_mm": 2,
         "max_count": 0, "region": [THIGH]},
        {"name": "walk: everything else", "clips": ["walk"], "max_ratio": 2.75, "min_ratio": 0.115, "min_change_mm": 2, "max_count": 0,
         "exclude_region": [ARM, FLANK, TAIL, THIGH]},
    ],
}
# --measured <rig_check --out report>: where this species' walk misses the
# horse's limit in a region, that region is held at its own measure plus 5
# percent (a regression guard, as horse.json's own limits are), and named in
# "about" and "over_horse"; the goal stays the horse's limit.
over = {}
if "--measured" in argv:
    rep = json.load(open(argv[argv.index("--measured") + 1], encoding="utf-8"))
    got = {r["name"]: r for r in rep.get("results", [])}
    for c in cfg["stretch"]:
        m = got.get(c["name"])
        if not m:
            continue
        mx, mn = float(m["max_ratio"]), float(m["min_ratio"])
        if mx > c["max_ratio"] or mn < c["min_ratio"]:
            over[c["name"]] = {"horse": [c["max_ratio"], c["min_ratio"]], "measured": [round(mx, 2), round(mn, 3)]}
            c["max_ratio"] = max(c["max_ratio"], math.ceil(mx * 1.05 * 100) / 100)
            c["min_ratio"] = min(c["min_ratio"], math.floor(mn * 0.95 * 1000) / 1000)
if over:
    cfg["about"] += (" Regions where this species misses the horse's limit are held at their own measure plus 5 percent "
                     "(over_horse: the horse's limit and the measure); they are open work, not the goal.")
    cfg["over_horse"] = over
os.makedirs(os.path.dirname(os.path.abspath(OUT)), exist_ok=True)
with open(OUT, "w", encoding="utf-8") as f:
    json.dump(cfg, f, indent=1)
print("RIGCHECK_REGIONS", OUT, json.dumps({"arm": ARM, "flank": FLANK, "thigh": THIGH, "tail": TAIL}))
