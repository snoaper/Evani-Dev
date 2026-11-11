"""Geocheck's mesh rules on a converted tier file (compressed and quantized),

    blender -b --factory-startup -t 2 --python tools/blender/bust_tier_check.py -- <model.tier.glb>

Blender reads the meshopt-compressed tier; each mesh (every level of detail)
is checked in world space with tools/blender/bust_check.py's rules and the
geometry check's own normals (area-weighted faces summed per index, as
tools/evani-geocheck material_view.rs `mesh_of` builds them).
"""
import bpy, sys, numpy as np
sys.path.insert(0, __import__("os").path.dirname(__file__))
argv = sys.argv[sys.argv.index("--") + 1:]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=argv[0], merge_vertices=False)
import bust_check as bc
for ob in bpy.context.scene.objects:
    if ob.type != 'MESH': continue
    me = ob.data
    me.calc_loop_triangles()
    M = ob.matrix_world; P = np.array([(M @ v.co)[:] for v in me.vertices]); P = P[:, [0, 2, 1]] * [1, 1, -1]
    # Per-loop normals into per-vertex (imported split normals).
    N = np.zeros_like(P)
    R = M.to_3x3().inverted().transposed()
    for l in me.loops: N[l.vertex_index] = (R @ l.normal).normalized()[:]
    N = N[:, [0, 2, 1]] * [1, 1, -1]
    F = np.array([t.vertices[:] for t in me.loop_triangles])
    # geocheck's own normals: area-weighted faces summed per index (mesh_of).
    fn = np.cross(P[F[:,1]]-P[F[:,0]], P[F[:,2]]-P[F[:,0]])
    N = np.zeros_like(P)
    for k in range(3): np.add.at(N, F[:,k], fn)
    N /= np.maximum(np.linalg.norm(N,axis=1,keepdims=True),1e-12)
    bc.check(ob.name, P, N, F)
