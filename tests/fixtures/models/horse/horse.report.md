# Model conversion: horse

evani-convert 0.1.0; tools: gltfpack 1.3, ktx 4.4.2, gltf-transform 4.5.1.

## Inputs

| Role | File | Bytes | SHA-256 |
|---|---|---:|---|
| input | <project>/.evani/tmp/horse5/final/horse.glb | 1919976 | `87410f2c1ffd9a5fb35b160cae477513c29bf6f5347093991b3d23374bd9eaa3` |

Master kept untouched: `horse.master.glb` (1.83 MB), never downloaded by a game.

## Tiers

| Tier | File | Triangles per level | Draw calls | Texture size | Texture memory (ASTC / BC7 / budget) | Phone download | Desktop download | Tier files raw / gzip / brotli |
|---|---|---|---:|---:|---|---:|---:|---|
| low | `horse.low.glb` | 11239 | 3 | no textures | 0.0 KB / 0.0 KB / 96.00 MB | 151.1 KB | 151.1 KB | 151.1 KB / 118.4 KB / 110.2 KB |
| high | `horse.high.glb` | 33722 | 3 | no textures | 0.0 KB / 0.0 KB / 512.00 MB | 365.4 KB | 365.4 KB | 365.4 KB / 304.2 KB / 288.8 KB |
| ultra | `horse.ultra.glb` | 44966 | 3 | no textures | 0.0 KB / 0.0 KB / 1536.00 MB | 454.1 KB | 454.1 KB | 454.1 KB / 380.9 KB / 360.0 KB |

**Package** (every tier file, texture and the manifest; not the master or reports): 4 files, 971.2 KB raw, 803.8 KB gzip, 759.2 KB brotli, against the 10 MB playground cap: **within**.

## Animation clips

Each tier file carries the compact clip block (docs/MODEL_FORMAT.md "Clips") instead of the glTF animations, which only the master keeps. Error is the worst distance between the baked clip and the master's animation resampled at 30 Hz, at shell points around every joint, over every frame.

| Tier | Clip | Frames | Tracks (constant) | At rest, dropped | Keys | Rotation bits | Bytes | Worst shell error | Worst node | Bound |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| low | `idle` | 151 | 46 (2) | 122 | 2405 | 48 | 17185 | 0.374 mm | `f_hoof.R` | within 0.4 mm |
| low | `walk` | 41 | 47 (0) | 121 | 1588 | 48 | 11430 | 0.328 mm | `jaw.001` | within 0.4 mm |
| low | (block) | | | | | | 28634 | | | |
| high | `idle` | 151 | 46 (2) | 122 | 2405 | 48 | 17185 | 0.374 mm | `f_hoof.R` | within 0.4 mm |
| high | `walk` | 41 | 47 (0) | 121 | 1588 | 48 | 11430 | 0.328 mm | `jaw.001` | within 0.4 mm |
| high | (block) | | | | | | 28634 | | | |
| ultra | `idle` | 151 | 46 (2) | 122 | 2405 | 48 | 17185 | 0.374 mm | `f_hoof.R` | within 0.4 mm |
| ultra | `walk` | 41 | 47 (0) | 121 | 1588 | 48 | 11430 | 0.328 mm | `jaw.001` | within 0.4 mm |
| ultra | (block) | | | | | | 28634 | | | |

## Files

| File | Raw | Gzip | Brotli |
|---|---:|---:|---:|
| `horse.low.glb` | 154712 | 121264 | 112821 |
| `horse.high.glb` | 374180 | 311525 | 295698 |
| `horse.ultra.glb` | 464968 | 390013 | 368655 |

## Checks

Mesh findings come from evani-model's meshcheck on level 0 in world space. They are findings, not errors: `self_intersection` counts crossing triangle pairs, also between separate parts that sit inside each other (wheels in a body), and simplified levels can add `duplicate` or `non_manifold_edge`.

| What | Triangles | Components | Mesh findings |
|---|---:|---:|---|
| master | 44966 | 1 | degenerate 1, self_intersection 1 |
| low level 0 | 11239 | 3 | non_manifold_vertex 23, self_intersection 23 |
| high level 0 | 33722 | 2 | non_manifold_vertex 17, self_intersection 18 |
| ultra level 0 | 44966 | 1 | self_intersection 4 |

Khronos validator (glTF-Transform `validate`): `horse.low.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `horse.high.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `horse.ultra.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings). The validator cannot look inside EXT_meshopt_compression, so each file's geometry is also validated on a twin made with the same settings without `-cc`.

Read back with the engine's reader (evani-gltf): every file opens; anchors, named nodes, levels and texture files are all there.

## Notes

- layers: 1 material(s) carry sheen, clear coat, anisotropy or iridescence as extras.evani_layers (drawn with the engine's `material-layers` render feature)
- regions: mesh 0 (`layout.003`): charted from 56 bones (vertices: ear 765, head 9560, leg 6167, neck 1303, spine 4010, tail 279)
- regions: mesh 0 (`layout.003`): charted from 56 bones (vertices: leg 1408)
- regions: mesh 0 (`layout.003`): charted from 56 bones (vertices: head 510)
- occlusion: the baked _EVANI_AO kept on 3 primitive(s), 24002 vertices (mean 0.92, 608 under 0.5); drawn by games that turn on `[materials] baked_ao`
- one level of detail per tier: the model is animated (skins, animations, cameras and extension data such as lights point at indices across the file, which the merge does not remap, and baked clips name each tier file's nodes once; LOD chains for skinned and animated models are not built yet)
- regions / occlusion: levels simplified without gltfpack's attribute optimisation (-sv), which would zero the chart and the occlusion
