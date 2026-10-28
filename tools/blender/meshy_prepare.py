# Headless Blender script: import one Meshy source model, measure it, export an
# untouched master .glb and render a 4-view preview strip.
# Run: blender -b --factory-startup -t 2 --python tools/blender/meshy_prepare.py -- <model-id> <art-source/meshy dir> <preview dir>
import bpy, sys, os, json, math, glob
import numpy as np
from mathutils import Vector

model_id, root, prev_dir = sys.argv[sys.argv.index("--") + 1:][:3]
src = os.path.join(root, model_id)
pick = lambda pat: sorted(glob.glob(os.path.join(src, pat)))
report = {"id": model_id}

# ---- import
bpy.ops.wm.read_factory_settings(use_empty=True)
fbx = pick("*.fbx")
if fbx:
    bpy.ops.import_scene.fbx(filepath=fbx[0])
    report["source"] = os.path.basename(fbx[0])
else:
    obj = pick("*.obj")[0]
    bpy.ops.wm.obj_import(filepath=obj)
    report["source"] = os.path.basename(obj)

meshes = [o for o in bpy.data.objects if o.type == "MESH"]

# ---- PBR hookup for the Marshal (separate maps next to the FBX)
def tex(path, nc):
    im = bpy.data.images.load(path)
    im.colorspace_settings.name = "Non-Color" if nc else "sRGB"
    return im

base = [p for p in pick("*_texture.png")]
if model_id == "frontier-marshal" and base:
    nrm = pick("*_normal.png")[0]
    met = pick("*_metallic.png")[0]
    rou = pick("*_roughness.png")[0]
    for o in meshes:
        for slot in o.material_slots:
            m = slot.material or bpy.data.materials.new("Marshal")
            slot.material = m
            m.use_nodes = True
            nt = m.node_tree
            nt.nodes.clear()
            out = nt.nodes.new("ShaderNodeOutputMaterial")
            bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
            nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
            def node(path, nc):
                n = nt.nodes.new("ShaderNodeTexImage")
                n.image = tex(path, nc)
                return n
            nt.links.new(node(base[0], False).outputs["Color"], bsdf.inputs["Base Color"])
            nt.links.new(node(met, True).outputs["Color"], bsdf.inputs["Metallic"])
            nt.links.new(node(rou, True).outputs["Color"], bsdf.inputs["Roughness"])
            nm = nt.nodes.new("ShaderNodeNormalMap")
            nt.links.new(node(nrm, True).outputs["Color"], nm.inputs["Color"])
            nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])

# ---- measure
tris = verts = polys = 0
mins = Vector((1e30,) * 3); maxs = Vector((-1e30,) * 3)
vol = 0.0; uv_out = 0; uv_total = 0; comps = 0; has_uv = True
for o in meshes:
    me = o.data
    verts += len(me.vertices); polys += len(me.polygons)
    lt = np.zeros(len(me.polygons), dtype=np.int32)
    me.polygons.foreach_get("loop_total", lt)
    tris += int((lt - 2).sum())
    mw = np.array(o.matrix_world)
    co = np.zeros(len(me.vertices) * 3, dtype=np.float32); me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3) @ mw[:3, :3].T + mw[:3, 3]
    mins = Vector(np.minimum(np.array(mins), co.min(0))); maxs = Vector(np.maximum(np.array(maxs), co.max(0)))
    me.calc_loop_triangles()
    lts = len(me.loop_triangles)
    ti = np.zeros(lts * 3, dtype=np.int32); me.loop_triangles.foreach_get("vertices", ti); ti = ti.reshape(-1, 3)
    a, b, c = co[ti[:, 0]].astype(np.float64), co[ti[:, 1]].astype(np.float64), co[ti[:, 2]].astype(np.float64)
    vol += float((np.einsum("ij,ij->i", a, np.cross(b, c)) / 6.0).sum())
    # connected components over edges
    ed = np.zeros(len(me.edges) * 2, dtype=np.int32); me.edges.foreach_get("vertices", ed); ed = ed.reshape(-1, 2)
    lab = np.arange(len(me.vertices))
    while True:
        la, lb = lab[ed[:, 0]], lab[ed[:, 1]]
        m = np.minimum(la, lb)
        new = lab.copy()
        np.minimum.at(new, la, m); np.minimum.at(new, lb, m)
        while True:
            nn = new[new]
            if (nn == new).all(): break
            new = nn
        if (new == lab).all(): break
        lab = new
    comps += len(np.unique(lab))
    if me.uv_layers:
        uv = np.zeros(len(me.uv_layers[0].uv) * 2, dtype=np.float32)
        me.uv_layers[0].uv.foreach_get("vector", uv)
        uv_total += len(uv) // 2
        uv_out += int(((uv < -0.001) | (uv > 1.001)).any(axis=0).sum()) if False else int((((uv.reshape(-1, 2) < -0.001) | (uv.reshape(-1, 2) > 1.001)).any(axis=1)).sum())
    else:
        has_uv = False
size = maxs - mins
imgs = {}
for im in bpy.data.images:
    if im.size[0] > 0 and im.type == "IMAGE":
        imgs[im.name] = [im.size[0], im.size[1], bool(im.packed_file) if hasattr(im, "packed_file") else False]
report.update({
    "triangles": tris, "vertices": verts, "polygons": polys,
    "objects": len(bpy.data.objects), "meshes": len(meshes), "materials": len(bpy.data.materials),
    "images": imgs,
    "bbox_min": [round(x, 4) for x in mins], "bbox_max": [round(x, 4) for x in maxs],
    "size_xyz": [round(x, 4) for x in size], "height_z": round(size.z, 4),
    "scene_unit_scale": bpy.context.scene.unit_settings.scale_length,
    "armatures": len(bpy.data.armatures), "bones": sum(len(a.bones) for a in bpy.data.armatures),
    "actions": len(bpy.data.actions), "shape_keys": sum(1 for m in bpy.data.meshes if m.shape_keys),
    "has_uv": has_uv, "uv_loops": uv_total, "uv_loops_outside_0_1": uv_out,
    "signed_volume": round(vol, 5), "volume_negative_means_inverted": vol < 0,
    "connected_components": comps,
    "pose_width_over_height": round(max(size.x, size.y) / size.z, 3) if size.z else None,
})

# ---- master glb
glb = os.path.join(src, model_id + ".master.glb")
bpy.ops.export_scene.gltf(filepath=glb, export_format="GLB", export_yup=True, export_extras=True,
                          export_image_format="AUTO", export_materials="EXPORT", export_apply=False)
report["glb_bytes"] = os.path.getsize(glb)

# ---- preview strip: front, side, back, three-quarter
sc = bpy.context.scene
sc.render.engine = "BLENDER_WORKBENCH"
sc.display.shading.light = "STUDIO"; sc.display.shading.color_type = "TEXTURE"
sc.display.shading.show_cavity = False
sc.render.resolution_x, sc.render.resolution_y = 400, 600
sc.render.film_transparent = False
w = bpy.data.worlds.new("w"); w.use_nodes = False; w.color = (0.45, 0.45, 0.45); sc.world = w
sc.view_settings.view_transform = "Standard"
cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam")); sc.collection.objects.link(cam); sc.camera = cam
cam.data.type = "ORTHO"; cam.data.clip_end = 1e5; cam.data.clip_start = 0.001
ctr = (mins + maxs) / 2
rad = max(size.x, size.y)
cam.data.ortho_scale = max(size.z * 1.12, rad * 1.1 * 600 / 400)
dist = max(size.x, size.y, size.z) * 3
os.makedirs(prev_dir, exist_ok=True)
strips = []
for i, az in enumerate((0, 90, 180, 45)):
    # az 0 = looking from -Y (front); 90 = from +X; 180 = from +Y; 45 = three-quarter front-right
    a = math.radians(az)
    cam.location = ctr + Vector((math.sin(a) * dist, -math.cos(a) * dist, 0))
    cam.rotation_euler = (math.pi / 2, 0, a)
    p = os.path.join(prev_dir, f"_{model_id}_{i}.png")
    sc.render.filepath = p
    bpy.ops.render.render(write_still=True)
    strips.append(p)
arrs = []
for p in strips:
    im = bpy.data.images.load(p); a = np.zeros(im.size[0] * im.size[1] * 4, dtype=np.float32); im.pixels.foreach_get(a)
    arrs.append(a.reshape(im.size[1], im.size[0], 4))
sheet = np.concatenate(arrs, axis=1)
out = bpy.data.images.new("sheet", sheet.shape[1], sheet.shape[0])
out.pixels.foreach_set(sheet.ravel()); out.filepath_raw = os.path.join(prev_dir, model_id + ".png"); out.file_format = "PNG"; out.save()
for p in strips: os.remove(p)

jp = os.path.join(src, model_id + ".report.json")
json.dump(report, open(jp, "w"), indent=1)
print("MESHY_REPORT", json.dumps({k: v for k, v in report.items() if k != "images"}))
print("MESHY_IMAGES", json.dumps(imgs))
