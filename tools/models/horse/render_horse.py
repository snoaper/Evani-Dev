# Renders of the test horse from its .blend (review pictures).
#   blender -b <horse.blend> --python tools/models/horse/render_horse.py -- --out <folder>
#       [--shots side,front,threeq,head] [--clip walk --frames 0,8,16,24] [--engine EEVEE|WORKBENCH]
#       [--prefix name_]
import bpy, sys, os, math, argparse
from mathutils import Vector, Euler

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ap = argparse.ArgumentParser()
ap.add_argument("--out", required=True)
ap.add_argument("--shots", default="side")
ap.add_argument("--clip", default="")
ap.add_argument("--frames", default="0")
ap.add_argument("--engine", default="WORKBENCH")
ap.add_argument("--prefix", default="")
ap.add_argument("--res", default="1400x1000")
ap.add_argument("--uvcheck", action="store_true")
A = ap.parse_args(argv)
A.out = os.path.abspath(A.out)
os.makedirs(A.out, exist_ok=True)


def P(f, z, x=0.0):
    return Vector((x, -f, z))


SHOTS = {
    "side": (P(0.15, 1.05, 6.2), P(0.15, 1.05, 0), 32),
    "front": (P(5.5, 1.15, 0.0), P(0, 1.10, 0), 32),
    "threeq": (P(3.6, 1.7, 3.9), P(0.1, 1.05, 0), 34),
    "head": (P(2.10, 2.00, 1.25), P(1.32, 1.70, 0.0), 30),
    "headside": (P(1.32, 1.70, 2.0), P(1.32, 1.70, 0.0), 30),
    "headfront": (P(2.60, 1.95, 0.55), P(1.35, 1.70, 0.0), 24),
    "muzzle": (P(2.05, 1.62, 0.55), P(1.52, 1.50, 0.0), 26),
    "ear": (P(1.60, 2.15, 0.65), P(1.26, 2.05, 0.0), 22),
    "rear": (P(-4.2, 1.45, 1.6), P(-0.4, 1.0, 0.0), 30),
    "rearq": (P(-3.6, 1.55, 3.6), P(-0.05, 0.95, 0.0), 34),
    "hindleg": (P(-0.48, 0.85, 2.7), P(-0.48, 0.82, 0.0), 34),
    "foreleg": (P(0.55, 0.85, 2.7), P(0.55, 0.82, 0.0), 34),
    "eye": (P(1.62, 1.92, 0.75), P(1.37, 1.84, 0.10), 18),
    "eyeside": (P(1.42, 1.86, 0.62), P(1.36, 1.83, 0.10), 16),
    "chest": (P(3.4, 1.05, 1.2), P(0.6, 1.0, 0.0), 26),
    "back": (P(-5.5, 1.2, 0.0), P(0, 1.1, 0), 32),
    "legs": (P(0.0, 0.5, 3.4), P(0.0, 0.45, 0), 34),
    "flank": (P(-0.1, 1.1, 2.6), P(-0.15, 1.0, 0), 45),
    "under": (P(1.2, 0.35, 1.9), P(0.0, 0.95, 0), 45),
    "underback": (P(-1.4, 0.35, 1.8), P(-0.2, 0.95, 0), 45),
}

sc = bpy.context.scene
rig = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
for o in bpy.data.objects:
    if o.name.startswith("anchor:") or o.name in ("mane_line", "parts"):
        o.hide_render = True
if A.uvcheck:
    # the coat's uv layout: a grid whose arrows should run with the hair
    img = bpy.data.images.new("uvgrid", 1024, 1024)
    img.generated_type = "COLOR_GRID"
    m = bpy.data.materials.get("coat")
    m.use_nodes = True
    nt = m.node_tree
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    mp = nt.nodes.new("ShaderNodeMapping")
    mp.inputs["Scale"].default_value = (6, 6, 1)
    tc = nt.nodes.new("ShaderNodeTexCoord")
    nt.links.new(tc.outputs["UV"], mp.inputs["Vector"])
    nt.links.new(mp.outputs["Vector"], tex.inputs["Vector"])
    nt.links.new(tex.outputs["Color"], nt.nodes["Principled BSDF"].inputs["Base Color"])
w, h = (int(x) for x in A.res.split("x"))
sc.render.resolution_x, sc.render.resolution_y = w, h
if A.engine == "WORKBENCH":
    sc.render.engine = "BLENDER_WORKBENCH"
    sh = sc.display.shading
    sh.light = "STUDIO"
    sh.color_type = "TEXTURE" if A.uvcheck else "MATERIAL"
    sh.show_cavity = True
    sh.cavity_type = "BOTH"
else:
    sc.render.engine = "BLENDER_EEVEE"
    world = bpy.data.worlds.get("World") or bpy.data.worlds.new("World")
    sc.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.62, 0.70, 0.80, 1)
    bg.inputs[1].default_value = 0.9
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sc.collection.objects.link(sun)
    sun.data.energy = 4.0
    sun.data.angle = math.radians(3)
    sun.rotation_euler = Euler((math.radians(48), 0, math.radians(-40)))
    fill = bpy.data.objects.new("fill", bpy.data.lights.new("fill", "SUN"))
    sc.collection.objects.link(fill)
    fill.data.energy = 1.2
    fill.rotation_euler = Euler((math.radians(60), 0, math.radians(140)))
    # ground
    bpy.ops.mesh.primitive_plane_add(size=30, location=(0, 0, 0))
    g = bpy.context.active_object
    m = bpy.data.materials.new("ground")
    m.use_nodes = True
    m.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.32, 0.36, 0.25, 1)
    m.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.95
    g.data.materials.append(m)
    try:
        sc.eevee.use_shadows = True
    except Exception:
        pass
cam = bpy.data.objects.get("cam") or bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
if cam.name not in sc.collection.objects:
    sc.collection.objects.link(cam)
sc.camera = cam
frames = [int(x) for x in A.frames.split(",") if x != ""]
if A.clip and rig is not None:
    act = bpy.data.actions[A.clip]
    rig.animation_data_create()
    rig.animation_data.action = act
    if hasattr(rig.animation_data, "action_slot") and act.slots:
        rig.animation_data.action_slot = act.slots[0]
else:
    frames = [0]
    if rig is not None and rig.animation_data:
        rig.animation_data.action = None
for fr in frames:
    sc.frame_set(fr)
    for s in A.shots.split(","):
        eye, tgt, fov = SHOTS[s]
        cam.location = eye
        cam.rotation_euler = (tgt - eye).normalized().to_track_quat("-Z", "Y").to_euler()
        cam.data.angle = math.radians(fov)
        tag = ("%s%s_f%02d" % (A.clip + "_", s, fr)) if A.clip else s
        sc.render.filepath = os.path.join(A.out, A.prefix + tag + ".png")
        bpy.ops.render.render(write_still=True)
print("RENDERED", len(frames))
