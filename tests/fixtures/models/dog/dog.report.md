# Model conversion: dog

evani-convert 0.1.0; tools: gltfpack 1.3, ktx 4.4.2, gltf-transform 4.5.1.

## Inputs

| Role | File | Bytes | SHA-256 |
|---|---|---:|---|
| input | <project>/.evani/tmp/dog_v2/dog.glb | 1258636 | `1c9093da630e2f24a9473ee347f7fd9353f4c447dfeaf2a2c779cc8a43487011` |

Master kept untouched: `dog.master.glb` (1.20 MB), never downloaded by a game.

## Tiers

| Tier | File | Triangles per level | Draw calls | Texture size | Texture memory (ASTC / BC7 / budget) | Phone download | Desktop download | Tier files raw / gzip / brotli |
|---|---|---|---:|---:|---|---:|---:|---|
| low | `dog.low.glb` | 6947 | 3 | no textures | 0.0 KB / 0.0 KB / 96.00 MB | 78.0 KB | 78.0 KB | 78.0 KB / 59.9 KB / 56.1 KB |
| medium | `dog.medium.glb` | 13896 | 3 | no textures | 0.0 KB / 0.0 KB / 192.00 MB | 124.8 KB | 124.8 KB | 124.8 KB / 98.0 KB / 90.5 KB |
| high | `dog.high.glb` | 20843 | 3 | no textures | 0.0 KB / 0.0 KB / 512.00 MB | 194.4 KB | 194.4 KB | 194.4 KB / 159.6 KB / 148.5 KB |
| ultra | `dog.ultra.glb` | 27792 | 3 | no textures | 0.0 KB / 0.0 KB / 1536.00 MB | 240.6 KB | 240.6 KB | 240.6 KB / 194.8 KB / 182.2 KB |

**Package** (every tier file, texture and the manifest; not the master or reports): 5 files, 638.7 KB raw, 512.6 KB gzip, 477.6 KB brotli, against the 10 MB playground cap: **within**.

## Animation clips

Each tier file carries the compact clip block (docs/MODEL_FORMAT.md "Clips") instead of the glTF animations, which only the master keeps. Error is the worst distance between the baked clip and the master's animation resampled at 30 Hz, at shell points around every joint, over every frame.

| Tier | Clip | Frames | Tracks (constant) | At rest, dropped | Keys | Rotation bits | Bytes | Worst shell error | Worst node | Bound |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| low | `idle` | 151 | 40 (0) | 128 | 581 | 48 | 4363 | 0.622 mm | `f_hoof.R` | within 1 mm |
| low | `walk` | 36 | 47 (0) | 121 | 1126 | 48 | 8196 | 0.999 mm | `ear.L.001` | within 1 mm |
| low | (block) | | | | | | 12576 | | | |
| medium | `idle` | 151 | 40 (0) | 128 | 787 | 48 | 5805 | 0.357 mm | `f_hoof.L` | within 0.5 mm |
| medium | `walk` | 36 | 47 (0) | 121 | 1477 | 48 | 10653 | 0.202 mm | `f_hoof.L` | within 0.5 mm |
| medium | (block) | | | | | | 16477 | | | |
| high | `idle` | 151 | 42 (0) | 126 | 1003 | 48 | 7329 | 0.227 mm | `f_hoof.R` | within 0.3 mm |
| high | `walk` | 36 | 47 (0) | 121 | 1440 | 48 | 10394 | 0.284 mm | `f_hoof.R` | within 0.3 mm |
| high | (block) | | | | | | 17742 | | | |
| ultra | `idle` | 151 | 44 (0) | 124 | 1237 | 48 | 8979 | 0.141 mm | `f_hoof.L` | within 0.2 mm |
| ultra | `walk` | 36 | 47 (0) | 121 | 1522 | 48 | 10968 | 0.162 mm | `f_hoof.L` | within 0.2 mm |
| ultra | (block) | | | | | | 19964 | | | |

## Files

| File | Raw | Gzip | Brotli |
|---|---:|---:|---:|
| `dog.low.glb` | 79852 | 61361 | 57478 |
| `dog.medium.glb` | 127828 | 100345 | 92719 |
| `dog.high.glb` | 199092 | 163401 | 152038 |
| `dog.ultra.glb` | 246424 | 199486 | 186550 |

## Checks

Mesh findings come from evani-model's meshcheck on level 0 in world space. They are findings, not errors: `self_intersection` counts crossing triangle pairs, also between separate parts that sit inside each other (wheels in a body), and simplified levels can add `duplicate` or `non_manifold_edge`.

| What | Triangles | Components | Mesh findings |
|---|---:|---:|---|
| master | 27792 | 6 | degenerate 8, duplicate 4, non_manifold_edge 8, self_intersection 1210 |
| low level 0 | 6947 | 6 | degenerate 6, non_manifold_edge 2, non_manifold_vertex 2, self_intersection 232 |
| medium level 0 | 13896 | 6 | degenerate 6, self_intersection 363 |
| high level 0 | 20843 | 6 | self_intersection 433 |
| ultra level 0 | 27792 | 6 | degenerate 4, self_intersection 777 |

Khronos validator (glTF-Transform `validate`): `dog.low.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `dog.medium.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `dog.high.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `dog.ultra.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings). The validator cannot look inside EXT_meshopt_compression, so each file's geometry is also validated on a twin made with the same settings without `-cc`.

Read back with the engine's reader (evani-gltf): every file opens; anchors, named nodes, levels and texture files are all there.

## Notes

- studio tags: `_EVANI_PART` and `_EVANI_COAT` kept on 3 primitive(s) in every tier (parked in COLOR_0/COLOR_1 through gltfpack; in the tier files `_EVANI_PART` is unorm8 x4, the region index in its first channel)
- one level of detail per tier: the model is animated (skins, animations, cameras and extension data such as lights point at indices across the file, which the merge does not remap, and baked clips name each tier file's nodes once; LOD chains for skinned and animated models are not built yet)
