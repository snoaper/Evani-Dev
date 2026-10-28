# Headless Blender script: preview renders of the rigged Frontier Marshal in a
# stand-in kart built from the kart racer's own numbers (draw.rs `draw_kart`).
# Run: blender -b --factory-startup -t 2 --python tools/blender/marshal_preview.py -- \
#          <frontier-marshal.rig.blend> <preview dir>
# Writes sheets into the preview dir: seated (front, side, three-quarter),
# leans, look back, celebrate and idle strips, weights and deformation close-ups,
# sleeve-fringe close-ups (10).
import bpy, sys, os, math, colorsys
import numpy as np
from mathutils import Vector, Matrix, Quaternion, Euler

_args = sys.argv[sys.argv.index("--") + 1:]
BLEND, OUT = _args[:2]
# optional third argument: the sheet numbers to render, e.g. "3,9" (default all)
WANT = {int(x) for x in _args[2].split(",")} if len(_args) > 2 else set(range(1, 11))
bpy.ops.wm.open_mainfile(filepath=BLEND)
os.makedirs(OUT, exist_ok=True)
sc = bpy.context.scene
arm = bpy.data.objects["marshal_rig"]
mesh = bpy.data.objects["frontier_marshal"]
for o in bpy.data.objects:
    if o.type == "EMPTY": o.hide_render = True

# ------------------------------------------------------------ render setup
sc.render.engine = "BLENDER_WORKBENCH"
sh = sc.display.shading
sh.light = "STUDIO"; sh.color_type = "TEXTURE"; sh.show_cavity = False
sh.show_shadows = True
sc.view_settings.view_transform = "Standard"
w = bpy.data.worlds.new("w"); w.use_nodes = False; w.color = (0.55, 0.6, 0.66); sc.world = w
sc.render.film_transparent = False
cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam")); sc.collection.objects.link(cam); sc.camera = cam
cam.data.lens = 50; cam.data.clip_start = 0.01; cam.data.clip_end = 100

# -------------------------------------------------------- stand-in kart
SEAT_Z = -0.30
def k2b(x, y, z): return Vector((x, -z + SEAT_Z, y))
kart = []
def solid_mat(name, rgb):
    m = bpy.data.materials.new(name); m.use_nodes = True
    im = bpy.data.images.new(name, 4, 4); im.generated_color = (*rgb, 1.0)
    nt = m.node_tree; bsdf = nt.nodes["Principled BSDF"]
    tx = nt.nodes.new("ShaderNodeTexImage"); tx.image = im
    nt.links.new(tx.outputs["Color"], bsdf.inputs["Base Color"])
    m.diffuse_color = (*rgb, 1.0)
    return m
M_BODY = solid_mat("kart_body", (0.75, 0.16, 0.12))
M_DARK = solid_mat("kart_dark", (0.09, 0.09, 0.1))
M_WHITE = solid_mat("kart_white", (0.85, 0.85, 0.85))
M_SPOKE = solid_mat("kart_spoke", (0.95, 0.75, 0.1))
def part(kind, o, pitch, size, mat, name="part"):
    if kind == "box":
        bpy.ops.mesh.primitive_cube_add(size=1.0)
    else:
        bpy.ops.mesh.primitive_cylinder_add(vertices=32, radius=0.5, depth=1.0)
    ob = bpy.context.active_object; ob.name = name
    # kart size (x, y, z) -> Blender (x, z, y); cylinders: kart y axis -> Blender z
    ob.scale = (size[0], size[2], size[1])
    ob.rotation_euler = (pitch, 0, 0)
    ob.location = k2b(*o)
    ob.data.materials.append(mat)
    kart.append(ob)
    return ob
part("box", (0, 0.36, 0), 0, (1.3, 0.3, 2.3), M_BODY)
part("box", (0, 0.52, 0.92), 0.22, (1.05, 0.34, 0.85), M_BODY)
for x in (0.74, -0.74): part("box", (x, 0.46, -0.02), 0, (0.34, 0.38, 0.76), M_BODY)
part("box", (0, 0.3, 1.42), 0, (1.6, 0.22, 0.28), M_WHITE)
part("box", (0, 0.6, -0.98), 0, (0.95, 0.44, 0.5), M_WHITE)
part("box", (0, 1.2, -1.18), -0.12, (1.6, 0.09, 0.42), M_BODY)
part("box", (0, 0.56, -0.3), 0, (0.72, 0.16, 0.62), M_DARK, "seat")
part("box", (0, 0.86, -0.62), -0.28, (0.74, 0.7, 0.16), M_DARK, "seat_back")
part("box", (0, 0.78, 0.5), 0.9, (0.08, 0.08, 0.5), M_DARK)
wheel = part("cyl", (0, 0.98, 0.34), -1.0, (0.46, 0.06, 0.46), M_DARK, "wheel")
# a yellow spoke from the hub to the top of the rim shows the wheel's turn
# (the wheel's local -Y is the top of the rim; its scale is 0.46, 0.46, 0.06)
bpy.ops.mesh.primitive_cube_add(size=1.0)
spoke = bpy.context.active_object
spoke.data.materials.append(M_SPOKE); spoke.parent = wheel
spoke.matrix_parent_inverse = Matrix.Identity(4)
spoke.scale = (0.05 / 0.46, 0.22 / 0.46, 0.075 / 0.06); spoke.location = (0, -0.11 / 0.46, 0.6)
kart.append(spoke)
for (x, z, r, wd) in ((0.8, 0.95, 0.33, 0.32), (-0.8, 0.95, 0.33, 0.32), (0.84, -0.86, 0.42, 0.44), (-0.84, -0.86, 0.42, 0.44)):
    bpy.ops.mesh.primitive_cylinder_add(vertices=32, radius=r, depth=wd)
    t = bpy.context.active_object; t.rotation_euler = (0, math.pi / 2, 0); t.location = k2b(x, r, z)
    t.data.materials.append(M_DARK); kart.append(t)
bpy.ops.mesh.primitive_plane_add(size=12)
ground = bpy.context.active_object; ground.data.materials.append(solid_mat("ground", (0.42, 0.5, 0.36)))
WHEEL_N = Vector((0.0, -math.sin(-1.0), math.cos(-1.0)))
def set_wheel(steer):
    wheel.rotation_mode = "QUATERNION"
    wheel.rotation_quaternion = Quaternion(WHEEL_N, steer) @ Euler((-1.0, 0, 0)).to_quaternion()
def show_kart(on):
    for o in kart: o.hide_render = not on

# --------------------------------------------------------------- helpers
def pose(clip, frame):
    ad = arm.animation_data
    act = bpy.data.actions[clip]
    ad.action = act
    if hasattr(ad, "action_slot") and act.slots: ad.action_slot = act.slots[0]
    sc.frame_set(frame)
    bpy.context.view_layer.update()

def look(target, az, el, dist, lens=50):
    a, e = math.radians(az), math.radians(el)
    d = Vector((math.sin(a) * math.cos(e), -math.cos(a) * math.cos(e), math.sin(e)))
    cam.location = Vector(target) + d * dist
    cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
    cam.data.lens = lens

def shot(name, res=(640, 720)):
    sc.render.resolution_x, sc.render.resolution_y = res
    p = os.path.join(OUT, "_" + name + ".png"); sc.render.filepath = p
    bpy.ops.render.render(write_still=True)
    im = bpy.data.images.load(p); a = np.zeros(res[0] * res[1] * 4, np.float32); im.pixels.foreach_get(a)
    bpy.data.images.remove(im); os.remove(p)
    return a.reshape(res[1], res[0], 4)

def sheet(name, arrs):
    s = np.concatenate(arrs, axis=1)
    out = bpy.data.images.new(name, s.shape[1], s.shape[0])
    out.pixels.foreach_set(s.ravel()); out.filepath_raw = os.path.join(OUT, name + ".png"); out.file_format = "PNG"; out.save()
    bpy.data.images.remove(out)
    print("SHEET", name)

SEATED_T = (0, -0.25, 1.15)
# ------------------------------------------------------------- 1 seated
if 1 in WANT:
    show_kart(True); set_wheel(0.0)
    pose("idle_seated", 0)
    rows = []
    for az, el in ((0, 8), (90, 5), (35, 18)):
        look(SEATED_T, az, el, 4.4); rows.append(shot("seat%d" % az))
    sheet("01_seated_front_side_3q", rows)
# ------------------------------------------------------------- 2 leans
if 2 in WANT:
    show_kart(True)
    rows = []
    for clip, steer in (("lean_left", 0.9), ("lean_center", 0.0), ("lean_right", -0.9)):
        set_wheel(steer); pose(clip, 0)
        look(SEATED_T, 0, 10, 4.4); rows.append(shot(clip + "f"))
    set_wheel(0.0)
    for clip, steer in (("lean_left", 0.9), ("lean_right", -0.9)):
        set_wheel(steer); pose(clip, 0)
        look(SEATED_T, 180, 25, 4.8); rows.append(shot(clip + "b"))
    set_wheel(0.0)
    sheet("02_lean_left_center_right_front_then_chase", rows)
# ------------------------------------------------------------- 3 look back
if 3 in WANT:
    show_kart(True)
    rows = []
    # frame 20 is the middle of the full-turn hold (frames 9-30)
    for f, az, dist in ((0, 200, 4.4), (20, 200, 4.4), (20, 160, 2.6), (20, 240, 2.6), (20, 340, 4.4), (20, 90, 4.4)):
        pose("look_back", f); look(SEATED_T, az, 15, dist); rows.append(shot("lb%d_%d" % (f, az)))
    sheet("03_look_back_f0_f20", rows)
# ------------------------------------------------------------- 4 celebrate
if 4 in WANT:
    show_kart(False)
    rows = []
    for f in (0, 10, 19, 26, 34, 49, 62, 75):
        pose("celebrate", f); look((0, 0, 1.25), 20, 8, 6.6); rows.append(shot("c%d" % f, (420, 720)))
    sheet("04_celebrate_frames_0_10_19_26_34_49_62_75", rows)
if 5 in WANT:
    show_kart(False)
    rows = []
    for f in (0, 18, 36, 54):
        pose("stand_idle", f); look((0, 0, 1.15), 25, 6, 6.4); rows.append(shot("s%d" % f, (420, 720)))
    sheet("05_stand_idle_frames_0_18_36_54", rows)
# ------------------------------------------------------------- 5 idle strip
if 6 in WANT:
    show_kart(True)
    rows = []
    for f in (0, 12, 24, 36, 48, 60):
        pose("idle_seated", f); look((0, -0.15, 1.35), 30, 12, 2.6); rows.append(shot("i%d" % f, (420, 520)))
    sheet("06_idle_seated_frames_0_12_24_36_48_60", rows)
# ---------------------------------------------- 6 deformation close-ups
if 7 in WANT:
    rows = []
    pose("idle_seated", 0)
    show_kart(False)
    look((0.0, -0.1, 1.45), 35, 20, 1.7); rows.append(shot("d_sh", (640, 640)))
    look((0.2, -0.3, 1.3), 120, 5, 1.7); rows.append(shot("d_fr", (640, 640)))
    look((0.0, -0.1, 0.85), 70, 10, 2.0); rows.append(shot("d_hip", (640, 640)))
    look((0.0, 0.1, 0.9), 200, 10, 2.0); rows.append(shot("d_back", (640, 640)))
    # the raised arm's sleeve fringes at the top of a fist pump
    pose("celebrate", 19)
    look((-0.45, 0.0, 1.95), 200, 5, 1.6); rows.append(shot("d_fr_up", (640, 640)))
    sheet("07_deform_closeups_seated_shoulders_fringes_hips_back", rows)
# --------------------------------------- 9 grip and seat close-ups
if 9 in WANT:
    rows = []
    show_kart(True)
    for clip, steer, f in (("idle_seated", 0.0, 0), ("lean_left", 0.9, 0), ("look_back", 0.0, 15)):
        set_wheel(steer); pose(clip, f)
        look((0.0, -0.62, 1.12), 0, 35, 1.1); rows.append(shot("g_%s_top" % clip, (640, 640)))
    set_wheel(0.0); pose("idle_seated", 0)
    look((0.24, -0.62, 1.05), 70, 10, 0.7); rows.append(shot("g_left_side", (640, 640)))
    # the coat tail against the seat: kart hidden, a thin plate at the seat top
    # (kart y 0.64) shows what would sink into the cushion
    show_kart(False)
    bpy.ops.mesh.primitive_cube_add(size=1.0)
    plate = bpy.context.active_object; plate.scale = (0.72, 0.62, 0.004); plate.location = (0, 0, 0.64)
    plate.data.materials.append(M_SPOKE)
    look((0.0, 0.1, 0.66), 135, 4, 1.5); rows.append(shot("g_coat_bq", (640, 640)))
    look((0.0, 0.1, 0.66), 90, 2, 1.5); rows.append(shot("g_coat_side", (640, 640)))
    bpy.data.objects.remove(plate)
    show_kart(True)
    sheet("09_grip_top_idle_leanleft_lookback_side_coat", rows)
# ------------------------------------------- 10 sleeve fringe close-ups
def sleeve_mid(side=None):
    """World point at the elbow of the given arm (or the higher one)."""
    sides = [side] if side else ["_l", "_r"]
    pts = [arm.matrix_world @ arm.pose.bones["forearm" + s].head for s in sides]
    return max(pts, key=lambda p: p.z)
if 10 in WANT:
    rows = []
    show_kart(False)
    # the raised arm in celebrate at frames 24 and 54, from the front and the side
    for f in (24, 54):
        pose("celebrate", f); t = sleeve_mid()
        look(t, 0, 5, 1.0); rows.append(shot("s_cel%d_front" % f, (640, 640)))
        look(t, 90 if t.x > 0 else -90, 5, 1.0); rows.append(shot("s_cel%d_side" % f, (640, 640)))
    pose("stand_idle", 54); look(sleeve_mid("_l"), 60, 5, 1.0); rows.append(shot("s_stand", (640, 640)))
    show_kart(True); set_wheel(0.0)
    pose("look_back", 20); look(sleeve_mid("_l"), 60, 10, 1.0); rows.append(shot("s_lookback", (640, 640)))
    sheet("10_sleeve_fringes_celebrate_f24_f54_front_side_stand_lookback", rows)
# ------------------------------------------------- 7 weights colour map
if 8 not in WANT:
    print("PREVIEW_DONE"); sys.stdout.flush(); os._exit(0)
names = [vg.name for vg in mesh.vertex_groups]
# one clear colour per bone family; the right side is a darker shade
FAMILY = {"hips": (0.9, 0.1, 0.1), "spine": (1.0, 0.55, 0.0), "spine1": (1.0, 0.9, 0.0),
          "spine2": (0.55, 0.9, 0.1), "neck": (0.0, 0.7, 0.6), "head": (0.95, 0.95, 0.95),
          "hat": (0.3, 0.3, 0.3), "shoulder": (1.0, 0.4, 0.7), "upper_arm": (0.15, 0.35, 1.0),
          "forearm": (0.0, 0.85, 1.0), "hand": (0.1, 0.8, 0.2), "fingers1": (0.6, 0.2, 0.9),
          "fingers2": (0.8, 0.5, 1.0), "fingers3": (0.4, 0.1, 0.6), "thumb1": (0.6, 0.4, 0.2),
          "thumb2": (0.8, 0.6, 0.4), "thigh": (0.9, 0.1, 0.8), "shin": (0.2, 0.2, 0.7),
          "foot": (0.6, 0.6, 0.1), "toe": (0.1, 0.4, 0.1)}
pal = {}
for n in names:
    base, _, suf = n.rpartition("_") if n.endswith(("_l", "_r")) else (n, "", "")
    c = FAMILY.get(base, (0.5, 0.5, 0.5))
    pal[n] = tuple(x * (0.6 if suf == "r" else 1.0) for x in c)
me = mesh.data
ca = me.color_attributes.new("weights", "FLOAT_COLOR", "POINT")
cols = np.zeros((len(me.vertices), 4), np.float32); cols[:, 3] = 1
for v in me.vertices:
    c = np.zeros(3)
    for g in v.groups: c += np.array(pal[mesh.vertex_groups[g.group].name]) * g.weight
    cols[v.index, :3] = c
ca.data.foreach_set("color", cols.ravel())
me.color_attributes.active_color = ca
sh.color_type = "VERTEX"
rows = []
arm.animation_data.action = None
for pb in arm.pose.bones: pb.location = (0, 0, 0); pb.rotation_quaternion = (1, 0, 0, 0)
bpy.context.view_layer.update()
look((0, 0, 1.25), 0, 0, 5.5); rows.append(shot("w_rest_front", (640, 720)))
look((0, 0, 1.25), 180, 0, 5.5); rows.append(shot("w_rest_back", (640, 720)))
pose("idle_seated", 0)
look((0, -0.25, 1.15), 35, 15, 3.6); rows.append(shot("w_seat_q", (640, 720)))
look((0, -0.25, 1.15), 90, 5, 3.6); rows.append(shot("w_seat_s", (640, 720)))
sheet("08_weights_rest_front_back_seated", rows)
print("PREVIEW_DONE")
