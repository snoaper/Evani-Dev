# Model conversion: cow

evani-convert 0.1.0; tools: gltfpack 1.3, ktx 4.4.2, gltf-transform 4.5.1.

## Inputs

| Role | File | Bytes | SHA-256 |
|---|---|---:|---|
| input | <project>/.evani/tmp/cow_v1/cow.glb | 1746184 | `cf8ba5e1b22781b77c36a78ea667ff85c374cca617d32151773153aa6274a404` |

Master kept untouched: `cow.master.glb` (1.67 MB), never downloaded by a game.

## Tiers

| Tier | File | Triangles per level | Draw calls | Texture size | Texture memory (ASTC / BC7 / budget) | Phone download | Desktop download | Tier files raw / gzip / brotli |
|---|---|---|---:|---:|---|---:|---:|---|
| low | `cow.low.glb` | 10065 | 4 | no textures | 0.0 KB / 0.0 KB / 96.00 MB | 110.1 KB | 110.1 KB | 110.1 KB / 83.5 KB / 77.7 KB |
| medium | `cow.medium.glb` | 20135 | 4 | no textures | 0.0 KB / 0.0 KB / 192.00 MB | 169.9 KB | 169.9 KB | 169.9 KB / 131.5 KB / 122.6 KB |
| high | `cow.high.glb` | 30202 | 4 | no textures | 0.0 KB / 0.0 KB / 512.00 MB | 263.7 KB | 263.7 KB | 263.7 KB / 209.9 KB / 196.5 KB |
| ultra | `cow.ultra.glb` | 40270 | 4 | no textures | 0.0 KB / 0.0 KB / 1536.00 MB | 330.5 KB | 330.5 KB | 330.5 KB / 250.4 KB / 234.8 KB |

**Package** (every tier file, texture and the manifest; not the master or reports): 5 files, 875.1 KB raw, 675.5 KB gzip, 631.8 KB brotli, against the 10 MB playground cap: **within**.

## Animation clips

Each tier file carries the compact clip block (docs/MODEL_FORMAT.md "Clips") instead of the glTF animations, which only the master keeps. Error is the worst distance between the baked clip and the master's animation resampled at 30 Hz, at shell points around every joint, over every frame.

| Tier | Clip | Frames | Tracks (constant) | At rest, dropped | Keys | Rotation bits | Bytes | Worst shell error | Worst node | Bound |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| low | `idle` | 151 | 44 (0) | 124 | 1617 | 48 | 11639 | 0.253 mm | `f_hoof.L` | within 0.3 mm |
| low | `walk` | 46 | 47 (0) | 121 | 1939 | 48 | 13887 | 0.289 mm | `f_hoof.L` | within 0.3 mm |
| low | (block) | | | | | | 25543 | | | |
| medium | `idle` | 151 | 44 (0) | 124 | 1617 | 48 | 11639 | 0.253 mm | `f_hoof.L` | within 0.3 mm |
| medium | `walk` | 46 | 47 (0) | 121 | 1939 | 48 | 13887 | 0.289 mm | `f_hoof.L` | within 0.3 mm |
| medium | (block) | | | | | | 25543 | | | |
| high | `idle` | 151 | 44 (0) | 124 | 1617 | 48 | 11639 | 0.253 mm | `f_hoof.L` | within 0.3 mm |
| high | `walk` | 46 | 47 (0) | 121 | 1939 | 48 | 13887 | 0.289 mm | `f_hoof.L` | within 0.3 mm |
| high | (block) | | | | | | 25543 | | | |
| ultra | `idle` | 151 | 44 (0) | 124 | 1617 | 48 | 11639 | 0.253 mm | `f_hoof.L` | within 0.3 mm |
| ultra | `walk` | 46 | 47 (0) | 121 | 1939 | 48 | 13887 | 0.289 mm | `f_hoof.L` | within 0.3 mm |
| ultra | (block) | | | | | | 25543 | | | |

## Files

| File | Raw | Gzip | Brotli |
|---|---:|---:|---:|
| `cow.low.glb` | 112716 | 85506 | 79564 |
| `cow.medium.glb` | 173984 | 134638 | 125508 |
| `cow.high.glb` | 270052 | 214923 | 201257 |
| `cow.ultra.glb` | 338456 | 256395 | 240398 |

## Checks

Mesh findings come from evani-model's meshcheck on level 0 in world space. They are findings, not errors: `self_intersection` counts crossing triangle pairs, also between separate parts that sit inside each other (wheels in a body), and simplified levels can add `duplicate` or `non_manifold_edge`.

| What | Triangles | Components | Mesh findings |
|---|---:|---:|---|
| master | 40270 | 23 | self_intersection 2262 |
| low level 0 | 10065 | 23 | degenerate 2, self_intersection 775 |
| medium level 0 | 20135 | 23 | degenerate 8, non_manifold_edge 1, non_manifold_vertex 1, self_intersection 1093 |
| high level 0 | 30202 | 23 | degenerate 4, duplicate 1, non_manifold_edge 2, non_manifold_vertex 1, self_intersection 1276 |
| ultra level 0 | 40270 | 23 | degenerate 4, duplicate 2, non_manifold_edge 4, self_intersection 1808 |

Khronos validator (glTF-Transform `validate`): `cow.low.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `cow.medium.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `cow.high.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings); `cow.ultra.glb` 0 errors, 2 warnings (geometry, on the uncompressed twin: 0 errors, 2 warnings). The validator cannot look inside EXT_meshopt_compression, so each file's geometry is also validated on a twin made with the same settings without `-cc`.

Read back with the engine's reader (evani-gltf): every file opens; anchors, named nodes, levels and texture files are all there.

## Notes

- studio tags: `_EVANI_PART` and `_EVANI_COAT` kept on 4 primitive(s) in every tier (parked in COLOR_0/COLOR_1 through gltfpack; in the tier files `_EVANI_PART` is unorm8 x4, the region index in its first channel)
- one level of detail per tier: the model is animated (skins, animations, cameras and extension data such as lights point at indices across the file, which the merge does not remap, and baked clips name each tier file's nodes once; LOD chains for skinned and animated models are not built yet)
