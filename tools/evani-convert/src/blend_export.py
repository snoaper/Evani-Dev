# Evani.Dev model converter (tools/evani-convert): export the open .blend
# with Blender's own glTF exporter, headless (STAGE3_PLAN decision 5).
# Run by the converter as:
#   blender -b --factory-startup -t 2 <file.blend> --python blend_export.py -- <out.glb>
# Custom properties go out as glTF extras, so `extras.evani` set in Blender
# reaches the converter.
import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]
bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLB",
    export_extras=True,
    export_apply=True,
    export_yup=True,
    use_selection=False,
)
