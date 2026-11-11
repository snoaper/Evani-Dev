"""Preview sheet for the material viewer's bust (tools/blender/bust_build.py).

    blender -b --factory-startup -t 2 --python tools/blender/bust_preview.py -- <bust.glb> <out-prefix> [engine]

Imports the bust and renders a front, three-quarter, side and face close-up
view (Workbench studio light by default, `eevee` or `cycles` for a lit look)
to <out-prefix>-<view>.png. Review pictures only; nothing ships.
"""

import math
import sys

import bpy
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1:]
src, prefix = argv[0], argv[1]
engine = argv[2] if len(argv) > 2 else "workbench"

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
scene = bpy.context.scene

# Blender axes: the glTF +z (face) is Blender -y, +y up is +z.
if engine == "cycles":
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 24
    scene.cycles.use_denoising = True
    scene.cycles.device = "CPU"
elif engine == "eevee":
    scene.render.engine = "BLENDER_EEVEE"
else:
    scene.render.engine = "BLENDER_WORKBENCH"
    sh = scene.display.shading
    sh.light = "STUDIO"
    sh.color_type = "MATERIAL"
    sh.show_cavity = True
    sh.cavity_type = "WORLD"
    sh.show_specular_highlight = True
scene.render.resolution_x = 900
scene.render.resolution_y = 900
scene.render.film_transparent = False

world = bpy.data.worlds.new("w")
world.use_nodes = True
bg = world.node_tree.nodes.get("Background")
if bg:
    bg.inputs[0].default_value = (0.62, 0.66, 0.72, 1)
    bg.inputs[1].default_value = 0.8
scene.world = world
if engine != "workbench":
    sun = bpy.data.lights.new("key", "SUN")
    sun.energy = 3.0
    so = bpy.data.objects.new("key", sun)
    so.rotation_euler = (math.radians(55), 0, math.radians(-35))
    scene.collection.objects.link(so)
    # Material colours from the base colour factor already; make skin soft.
    for m in bpy.data.materials:
        if m.name == "skin" and m.node_tree:
            p = m.node_tree.nodes.get("Principled BSDF")
            if p:
                p.inputs["Subsurface Weight"].default_value = 0.25
                p.inputs["Subsurface Radius"].default_value = (0.004, 0.0016, 0.0009)
else:
    for m in bpy.data.materials:
        if m.node_tree:
            p = m.node_tree.nodes.get("Principled BSDF")
            if p:
                c = p.inputs["Base Color"].default_value
                m.diffuse_color = (c[0], c[1], c[2], 1)
        if m.name == "eye":
            m.diffuse_color = (0.9, 0.9, 0.9, 1)

cam_data = bpy.data.cameras.new("cam")
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam


def shoot(name, target, az, el, dist, lens=85):
    t = Vector(target)
    a, e = math.radians(az), math.radians(el)
    # az 0 = in front of the face (Blender -y).
    off = Vector((math.sin(a) * math.cos(e), -math.cos(a) * math.cos(e), math.sin(e))) * dist
    cam.location = t + off
    d = t - cam.location
    cam.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
    cam_data.lens = lens
    scene.render.filepath = f"{prefix}-{name}.png"
    bpy.ops.render.render(write_still=True)


# glTF (x, y, z) -> Blender (x, -z, y).
head = (0, 0, 0.0)
views = argv[3].split(",") if len(argv) > 3 else ["front", "three", "side", "face", "bust"]
if "front" in views:
    shoot("front", (0, 0, 0.0), 0, 2, 0.9)
if "three" in views:
    shoot("three", (0, 0, 0.0), 35, 6, 0.9)
if "side" in views:
    shoot("side", (0, 0, 0.0), 90, 2, 0.9)
if "face" in views:
    shoot("face", (0, -0.06, -0.015), 12, 4, 0.42)
if "nose" in views:
    # Under the nose and the mouth, a little from below (critic round 5's fleck).
    shoot("nose", (0, -0.10, -0.04), 8, -6, 0.2)
if "back" in views:
    shoot("back", (0, 0, 0.0), 180, 10, 0.9)
if "bust" in views:
    shoot("bust", (0, 0, -0.2), 18, 8, 1.6, lens=60)
