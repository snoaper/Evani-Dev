# Pictures of the quadruped generator's animals (review pictures; Workbench,
# headless, no sound).
#  1. From build.py: shots(ob, views, out, prefix, S), one animal coloured by
#     region with its coat chart (patches darkened, belly and face lighter).
#  2. A sheet, the animals side by side at true scale (orthographic):
#     blender -b --factory-startup --python tools/models/quadruped/render.py -- \
#         --compare <a.blend|a.glb>,<b...>,... --out <folder> [--views front,side,threeq] [--res 2400x1000]
#     .blend: the generator's file (its animal object); .glb: any model (the
#     horse's master), drawn in its own materials.
import bpy, sys, os, math, json, argparse
from mathutils import Vector, Matrix

VIEWS = {
    # direction from the animal to the camera (Blender: the animal faces -Y), and a tilt
    "side": Vector((1, 0, 0.08)),
    "front": Vector((0.0, -1, 0.12)),
    "threeq": Vector((0.78, -0.85, 0.42)),
    "rear": Vector((0.25, 1, 0.25)),
    "top": Vector((0.3, 0.1, 1)),
    "rearq": Vector((0.8, 0.85, 0.35)),
}


def region_ids(S):
    """The asset row's region ids, in order (catalog/assets/assets.json)."""
    root = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
    with open(os.path.join(root, "catalog", "assets", "assets.json"), encoding="utf-8") as f:
        row = next(a for a in json.load(f)["assets"] if a["id"] == S["asset"])
    return [r["id"] for r in row["regions"]]


def _scene(res):
    sc = bpy.context.scene
    w, h = (int(x) for x in res.split("x"))
    sc.render.resolution_x, sc.render.resolution_y = w, h
    sc.render.engine = "BLENDER_WORKBENCH"
    sh = sc.display.shading
    sh.light = "STUDIO"
    sh.show_cavity = True
    sh.cavity_type = "BOTH"
    sh.cavity_ridge_factor = 0.6
    sh.cavity_valley_factor = 1.0
    sh.show_shadows = True
    sh.shadow_intensity = 0.35
    sc.display.shadow_focus = 0.6
    sh.show_object_outline = False
    sc.render.film_transparent = False
    world = sc.world or bpy.data.worlds.new("World")
    sc.world = world
    world.color = (0.80, 0.82, 0.85)
    sc.view_settings.view_transform = "Standard"
    return sc


def _colour_by_region(ob, S):
    """A preview colour attribute from the regions and the coat chart."""
    me = ob.data
    part = me.attributes.get("_EVANI_PART")
    chart = me.color_attributes.get("_EVANI_COAT")
    if part is None:
        return False
    ids = region_ids(S)
    cols = S.get("render_colors", {})
    col = me.color_attributes.get("preview") or me.color_attributes.new("preview", "BYTE_COLOR", "POINT")
    patch_dark = S.get("id") == "cow"
    for v in me.vertices:
        rid = ids[part.data[v.index].value]
        c = Vector(cols.get(rid, (0.7, 0.7, 0.7)))
        if chart is not None and rid in ("coat", "leg", "ear", "tail"):
            face, belly, legs, patch = chart.data[v.index].color[:4]
            if patch_dark and patch > 0.55 and legs < 0.5 and face < 0.5:
                c = Vector((0.10, 0.09, 0.09))
            if not patch_dark:
                c = c.lerp(Vector((0.86, 0.78, 0.62)), 0.55 * max(belly, face * 0.6))
        col.data[v.index].color = (c.x, c.y, c.z, 1.0)
    me.color_attributes.active_color = col
    return True


def _camera(sc):
    cam = bpy.data.objects.get("cam") or bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    if cam.name not in sc.collection.objects:
        sc.collection.objects.link(cam)
    sc.camera = cam
    cam.data.type = "ORTHO"
    return cam


def _bounds(obs):
    pts = []
    for o in obs:
        if o.type != "MESH":
            continue
        dg = bpy.context.evaluated_depsgraph_get()
        oe = o.evaluated_get(dg)
        me = oe.to_mesh()
        pts += [oe.matrix_world @ v.co for v in me.vertices]
        oe.to_mesh_clear()
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return lo, hi, pts


def _aim(cam, target, d, scale):
    d = d.normalized()
    cam.location = target + d * 12.0
    cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
    cam.data.ortho_scale = scale
    cam.data.clip_end = 40


def _frame_scale(pts, cam_dir, aspect, margin=1.12):
    d = cam_dir.normalized()
    right = d.cross(Vector((0, 0, 1)))
    if right.length < 1e-4:
        right = Vector((1, 0, 0))
    right.normalize()
    up = right.cross(d).normalized()
    xs = [p.dot(right) for p in pts]
    ys = [p.dot(up) for p in pts]
    w, h = max(xs) - min(xs), max(ys) - min(ys)
    centre = right * ((max(xs) + min(xs)) / 2) + up * ((max(ys) + min(ys)) / 2)
    along = sum((p.dot(d) for p in pts)) / len(pts)
    return max(w, h * aspect) * margin, centre + d * along


def shots(ob, views, out, prefix, S, res="1400x1000"):
    sc = _scene(res)
    sh = sc.display.shading
    if _colour_by_region(ob, S):
        sh.color_type = "VERTEX"
    else:
        sh.color_type = "MATERIAL"
    for o in bpy.data.objects:
        if o.type == "MESH" and o is not ob:
            o.hide_render = True
    cam = _camera(sc)
    _, _, pts = _bounds([ob])
    w, h = (int(x) for x in res.split("x"))
    for v in views:
        d = VIEWS[v]
        scale, centre = _frame_scale(pts, d, w / h)
        _aim(cam, centre, d, scale)
        sc.render.filepath = os.path.join(out, prefix + v + ".png")
        bpy.ops.render.render(write_still=True)
    print("QUAD_RENDERED", len(views))


# ------------------------------------------------------------ the sheet
def _load(path):
    before = set(bpy.data.objects)
    if path.lower().endswith(".blend"):
        with bpy.data.libraries.load(path, link=False) as (src, dst):
            dst.objects = [n for n in src.objects]
        new = [o for o in dst.objects if o is not None]
        for o in new:
            bpy.context.scene.collection.objects.link(o)
    else:
        bpy.ops.import_scene.gltf(filepath=path)
        new = [o for o in bpy.data.objects if o not in before]
    keep = []
    for o in new:
        if o.type == "ARMATURE":
            o.data.pose_position = "REST"
            if o.animation_data:
                o.animation_data.action = None
        if o.type == "MESH" and (o.name in ("parts",) or o.name.startswith("anchor") or o.hide_render):
            o.hide_render = True
            continue
        if o.type == "MESH":
            keep.append(o)
    return new, keep


def _materials_to_viewport(obs):
    for o in obs:
        for m in o.data.materials:
            if m is None:
                continue
            if m.use_nodes:
                b = m.node_tree.nodes.get("Principled BSDF")
                if b is not None:
                    c = b.inputs["Base Color"].default_value
                    m.diffuse_color = (c[0], c[1], c[2], 1.0)


def sheet(paths, out, views, res, species_dir):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = _scene(res)
    groups = []
    for p in paths:
        new, meshes = _load(p)
        sid = os.path.splitext(os.path.basename(p))[0].split(".")[0]
        sp = os.path.join(species_dir, sid + ".json")
        if p.lower().endswith(".blend") and os.path.isfile(sp):
            with open(sp, encoding="utf-8") as f:
                S = json.load(f)
            for m in meshes:
                _colour_by_region(m, S)
        else:
            _materials_to_viewport(meshes)
            for m in meshes:
                # the horse's master: its own coat colour as vertex colour, so
                # one colour mode draws the whole sheet
                me = m.data
                col = me.color_attributes.new("preview", "BYTE_COLOR", "CORNER")
                for poly in me.polygons:
                    mat = me.materials[poly.material_index] if me.materials else None
                    c = mat.diffuse_color if mat else (0.6, 0.6, 0.6, 1)
                    for li in poly.loop_indices:
                        col.data[li].color = (c[0], c[1], c[2], 1.0)
                me.color_attributes.active_color = col
        roots = [o for o in new if o.parent is None]
        groups.append((roots, meshes))
    sc.display.shading.color_type = "VERTEX"
    w, h = (int(x) for x in res.split("x"))
    cam = _camera(sc)
    gap = 0.25
    for v in views:
        d = VIEWS[v].normalized()
        right = d.cross(Vector((0, 0, 1))).normalized()
        # place the animals in a row across the picture, each by its own width
        # in this view, ground on z = 0
        cursor = 0.0
        for roots, meshes in groups:
            for r in roots:
                r.location = Vector((0, 0, 0))
            bpy.context.view_layer.update()
            _, _, pts = _bounds(meshes)
            xs = [p.dot(right) for p in pts]
            shift = cursor - min(xs)
            for r in roots:
                r.location = right * shift
            cursor += (max(xs) - min(xs)) + gap
        bpy.context.view_layer.update()
        allpts = []
        for _, meshes in groups:
            allpts += _bounds(meshes)[2]
        scale, centre = _frame_scale(allpts, d, w / h, margin=1.06)
        _aim(cam, centre, d, scale)
        sc.render.filepath = os.path.join(out, "sheet_" + v + ".png")
        bpy.ops.render.render(write_still=True)
    print("QUAD_SHEET", len(views))


if __name__ == "__main__" and "--compare" in sys.argv:
    argv = sys.argv[sys.argv.index("--") + 1:]
    ap = argparse.ArgumentParser()
    ap.add_argument("--compare", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--views", default="side,front,threeq")
    ap.add_argument("--res", default="2400x1000")
    A = ap.parse_args(argv)
    os.makedirs(A.out, exist_ok=True)
    here = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, here)
    sheet([os.path.abspath(p) for p in A.compare.split(",")], os.path.abspath(A.out), A.views.split(","), A.res, os.path.join(here, "species"))
