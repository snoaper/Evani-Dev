# Review pictures of a built animal in a clip: blender -b <id>.blend --python tools/models/quadruped/pose_shots.py --
#     --out <folder> --clip walk --frames 0,9,18,27 [--views side,front]
import bpy, sys, os, argparse
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import render  # noqa: E402
argv = sys.argv[sys.argv.index("--") + 1:]
ap = argparse.ArgumentParser()
ap.add_argument("--out", required=True)
ap.add_argument("--clip", default="walk")
ap.add_argument("--frames", default="0")
ap.add_argument("--views", default="side")
A = ap.parse_args(argv)
os.makedirs(A.out, exist_ok=True)
rig = next(o for o in bpy.data.objects if o.type == "ARMATURE")
mesh = next(o for o in bpy.data.objects if o.type == "MESH" and o.parent is rig)
act = bpy.data.actions[A.clip]
rig.animation_data_create()
rig.animation_data.action = act
if hasattr(rig.animation_data, "action_slot") and act.slots:
    rig.animation_data.action_slot = act.slots[0]
sc = render._scene("1000x700")
sc.display.shading.color_type = "MATERIAL"
for o in bpy.data.objects:
    if o.type == "MESH" and o is not mesh:
        o.hide_render = True
cam = render._camera(sc)
sc.frame_set(0)
_, _, pts = render._bounds([mesh])
for fr in [int(x) for x in A.frames.split(",")]:
    sc.frame_set(fr)
    for v in A.views.split(","):
        d = render.VIEWS[v]
        scale, centre = render._frame_scale(pts, d, 1000 / 700, margin=1.3)
        render._aim(cam, centre, d, scale)
        sc.render.filepath = os.path.join(A.out, "%s_%s_f%02d.png" % (A.clip, v, fr))
        bpy.ops.render.render(write_still=True)
