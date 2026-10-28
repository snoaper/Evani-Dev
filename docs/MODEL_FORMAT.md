# Evani model format (format 1)

The contract between whatever makes a model (Evani.Model, the converter `evani-convert`, a hand export from Blender) and the engine's reader (`crates/evani-gltf`, the `models` module). Neither side depends on the other's code: both follow this document and test against the shared files in `tests/fixtures/models/`. Taken from docs/model_research/17-gltf-output.md (Evani.Model's glTF output) and the M1 writer in `crates/evani-model/src/gltf.rs`.

A model is standard glTF 2.0. Everything Evani-specific lives in node names and in `extras`, never in a custom extension, so any glTF program (Blender, glTF-Transform, the Khronos validator) still opens the files.

## Versioning

- The root `extras.evani.format` is a whole number. This document is **format 1**.
- The reader refuses a file whose format is 0, missing, or higher than the newest it knows (`evani_gltf::EVANI_FORMAT`, now 1), with an error, never a panic.
- Within one format, writers may only **add** keys. The reader keeps every key it doesn't use (`Evani::raw`), so new keys never break an older engine.
- Renaming or removing a key, or changing what a key means, needs format 2, a new section here, and a reader that still reads format 1.

## Files per model

| File | What it is |
|---|---|
| `<model>.<tier>.glb` | One file per tier: `low`, `medium`, `high`, `ultra`, and `vr` when asked. A tier downloads only its own file. |
| `<model>.evmodel.json` | The shared report beside them: provenance, the tiers made, per-tier triangles and sizes, checks. Dev side; the engine doesn't need it to draw. |
| textures | Per-tier images next to the files (normal and height maps from Evani.Model; the converter's texture sets, below). |
| `<model>.master.glb`, `<model>.manifest.json`, `<model>.report.json`, `<model>.report.md` | Written by `cargo xtask convert` into `assets/models/<id>/` (or `--out`): the untouched input (never downloaded), the asset manifest entries, and the conversion report (bytes per tier and file raw / gzip / brotli, triangles, draw calls, texture memory against the tier budget, the package against the 10 MB playground cap, tool versions, input hashes, validator and meshcheck findings). |

A `.gltf` with external buffers is also read (the caller fetches each URI); `.glb` is what ships.

## Extensions

| Extension | Read | Notes |
|---|---|---|
| KHR_mesh_quantization | yes | Integer and normalised attributes, dequantised by the spec's rules. Position dequantisation is an ordinary node scale and offset. Listed in `extensionsRequired` by Evani.Model. |
| EXT_meshopt_compression | yes | ATTRIBUTES (v0), TRIANGLES and INDICES modes; OCTAHEDRAL, QUATERNION and EXPONENTIAL filters. Decoded once at load. Fallback buffers with no data are never fetched. |
| KHR_texture_transform | yes (passed on) | gltfpack adds it when it quantises UVs. |
| anything else in `extensionsRequired` | no | The file is refused: it could not be drawn correctly. |

## Root `extras.evani`

Written by Evani.Model M1 (every key below), later extended by the converter (`lods`, `textures`).

| Key | Type | Written by | Read by the engine | Meaning |
|---|---|---|---|---|
| `format` | integer | Evani.Model | yes (required) | `1`. See Versioning. |
| `class` | string | Evani.Model | yes | The model's class (`kart`, `crate`). Also the root node's name. |
| `style` | string | Evani.Model | kept | The style it was made in. |
| `tier` | string | Evani.Model | yes | Which tier this file is (`low` ... `vr`). |
| `seed` | integer | Evani.Model | kept | Provenance: the generator seed. |
| `intent` | string | Evani.Model | kept | Provenance: content hash of the intent. |
| `parts` | array | Evani.Model | yes | The part tree, below. |
| `dropped` | array of `{part, why}` | Evani.Model | kept | Parts left out of this tier and why (too small to see). |
| `triangles` | integer | Evani.Model | kept | Triangles in this tier. |
| `budget` | integer | Evani.Model | kept | The tier's triangle budget. |
| `cell_m` | number | Evani.Model | kept | The tier's grid cell in metres (surface positions, anchors included, may move by up to one cell between tiers). |
| `tool`, `notes` | string, any | Evani.Model | kept | The writer's version; free notes. |
| `musts` | object | Evani.Model | kept | The intent's "must" checks and their results for this tier. |
| `normal_maps` | object | Evani.Model | kept | The tier's normal-map files and their convention (tangent space, MikkTSpace, +y up the chart). Not read: the engine uses each material's `normalTexture` (S3-3), and surface relief is per material (Surface relief). |
| `lods` | array | the converter | yes | LOD thresholds, below. |
| `textures` | array | the converter | yes | Per-device texture sets, below. |
| `sockets` | object | Evani.Model, or any writer | no (the converter turns it into anchor nodes) | Named attachment frames with a rotation, see Sockets. Absent when no anchor has one. |
| `clips` | object | the converter | the block, by `evani-anim` (`anim`) | The baked animation clips, see Clips. The game hands `evani_anim::Clips::decode` the view's bytes; the model loader does not find the view for it yet. |

"Kept" means the reader doesn't interpret it but hands it on in `Evani::raw`.

### `parts`

One entry per part, in the writer's order:

| Key | Type | Read | Meaning |
|---|---|---|---|
| `name` | string | yes (required) | The part's name. |
| `role` | string | yes | Its gameplay role (`body`, `wheel`, ...). |
| `parent` | string | yes, optional | The parent part's name; missing for a root part. Evani.Model M1 writes a flat list; nested writers add `parent`. |
| `solid` | string | yes | The name of the node that carries the part's mesh. Several parts may share one solid (merged groups). |
| `group` | string or null | yes | Mirror or repeat group. |
| `generator` | string | kept | Which generator made it. |
| `dropped` | bool | yes | The part is not in this tier. |

### `lods`

Several levels of detail inside one tier file, most detailed first:

```json
"lods": [ { "node": "kart_lod0", "min_screen": 0.25 }, { "node": "kart_lod1", "min_screen": 0.0 } ]
```

`node` names the root node of that level's subtree; `min_screen` is the smallest projected height, as a fraction of the screen height, at which that level is still used. A file without `lods` has one level.

The converter (`cargo xtask convert`, S3-5) names the level nodes `<id>_lod0`, `<id>_lod1`, ... (most detailed first). Each level is a whole copy of the model's drawn parts, so the input's part node names repeat once per level subtree: look a part up inside the level being drawn. Anchors (`anchor:` nodes with their `extras`) and the collision proxies (`collision`, `col:`) are gameplay data, not detail: only level 0 keeps them, so their names stay unique in the file and gameplay reads the same place whichever level is drawn. A model with skins, animations, cameras or extension data (lights, instancing) gets one level per tier file for now, with no `lods` list (those point at indices across the file, which the merge does not remap, and baked clips name each tier file's nodes once; LOD chains for skinned and animated models are not built yet). The converter report flags a tier whose level 0 alone is over that tier's whole-frame triangle or draw-call limit, like texture memory over its budget, and lists it as a problem. Evani.Model files keep their own single level (never re-simplified).

### `textures`

The per-device copies of a glTF texture. The texture's core `source` stays a plain PNG or JPEG (any program opens it, and it is the engine's RGBA fallback); the device copies are listed beside it:

```json
"textures": [ { "texture": 0, "name": "base", "sets": { "astc": "kart.low.base.astc.ktx2", "bc7": { "uri": "kart.low.base.bc7.ktx2" } } } ]
```

`texture` is an index into the file's `textures` (it must exist). Each set is a format (`astc`, `bc7`, `etc2`, `rgba`) mapped to a URI, either as a string or as `{ "uri": ... }`.

What the converter writes, per tier: the core image `textures/<name>.<tier>.jpg` (opaque colour and data) or `.png` (normal maps, alpha), and the sets `textures/<name>.<tier>.astc.ktx2` (ASTC 4x4) and `textures/<name>.<tier>.bc7.ktx2` (BC7), each written as a plain string. `name` is the material slot that first uses the image (`base`, `metal_rough`, `normal`, `occlusion`, `emissive`). The KTX2 files are plain: no supercompression, a full mip chain, sRGB for `base` and `emissive`, linear for the rest; that is what the engine's KTX2 reader accepts, and both the converter and `cargo xtask check` refuse anything else.

### `normal_maps` (Evani.Model, with the converter's sets)

Evani.Model lists each tier's normal-map PNGs in `normal_maps.maps[]` (`file`, `size`, ...). The converter keeps every key as written and adds, per map, the device copies:

```json
"normal_maps": { "maps": [ { "file": "character.low.body.normal.png", "size": 512,
  "sets": { "astc": "textures/character.low.body.normal.astc.ktx2", "bc7": "textures/character.low.body.normal.bc7.ktx2" } } ] }
```

`sets` is additive: a reader that doesn't know it uses `file`. The PNG stays beside the tier file.

## Surface relief

A material's own `extras.evani.relief` (on the glTF material, not the root) adds relief the normal map alone doesn't carry. Read only in games that turn on evani-render's `relief` feature with a look that supplies micro-detail patterns (the toon look's `relief`); every other game ignores it and downloads none of the code. Fixture: `tests/fixtures/models/relief-tile.glb`.

```json
"materials": [ { "name": "plate", "normalTexture": { "index": 1 }, "extras": { "evani": { "relief": {
  "height": { "index": 2, "depth": 0.03 },
  "micro": { "pattern": "knurl", "size": 0.025, "depth": 0.6, "angle": 0, "mask": { "texture": "base", "channel": "a" } } } } } } ]
```

| Key | Type | Meaning |
|---|---|---|
| `height.index` | integer | A glTF texture (index into `textures`), linear, red channel: 1 = top, 0 = deepest. |
| `height.depth` | number | How deep 0 is below 1, in UV units, above 0 and at most 0.25. |
| `micro.pattern` | string | `knurl` (crossed diagonal ridges), `ribs` (parallel ridges across u), `dots` (round domes on a grid) or `grain` (wavy lines along u). The look draws them. |
| `micro.size` | number | One period of the pattern, in UV units, above 0. |
| `micro.depth` | number, optional | 0 to 1, default 0.5: how far the pattern tips the normal and darkens its flanks. |
| `micro.angle` | number, optional | Degrees, default 0: the pattern turned in UV space. |
| `micro.mask` | `"none"` or object, optional | Where the pattern shows: `{ "texture": "base" or "normal", "channel": "r", "g", "b" or "a" }`, 1 = all of it. The height map can't be a mask. |

What the engine does with it, by tier: the micro pattern tips the mapped normal and darkens its flanks a little (so a banded toon ramp can't swallow it) on **every** tier (the same look on a phone as on a PC), fading out where one period is under 1.5 px on screen so it never shimmers; the height map drives parallax occlusion on every tier, **8 steps on High and 16 on Ultra, 2 on Lite, Low, Medium and VR** (the cheap march keeps raised panels reading raised on phones). Bake the height into the normal map as well (the fixture does) so the shading matches the shape everywhere. Relief needs tangents (`TANGENT` on the material's primitives). A part of a recipe the engine can't read (an unknown pattern, a depth out of range, a missing texture) is turned off on its own and the rest still draws; `cargo xtask check` names every such part and relief on primitives without tangents. Evani.Model's `normal_maps` list is kept but not read: the engine uses the material's `normalTexture`.

## Character materials

A material's own `extras.evani_kind` (on the glTF material, not the root) says what kind of character surface it is: `skin`, `eye`, `hair` (`fur` reads as `hair`) or `plain`. Read only in games that turn on evani-render's `char-materials` feature (it brings `look-surface`, so the game also needs a look with the lit surface pass, such as stylizedpbr's film rig); every other game ignores the tag and downloads none of the code (crate `evani-material-character`). The older `extras.evani_surface.kind` tag (`skin`, `eye`) is read too. Fixture: `tests/fixtures/models/char-head.glb` (written by `char-head.mjs`), drawn by the sandbox's `char-proof` build.

```json
"materials": [
  { "name": "eye",  "extras": { "evani_kind": "eye",  "evani_eye":  { "iris_color": [0.12, 0.25, 0.33], "iris_size": 0.48, "pupil_size": 0.36, "sclera_color": [0.8, 0.76, 0.72] } } },
  { "name": "skin", "extras": { "evani_kind": "skin", "evani_skin": { "pore_scale": 500, "pore_strength": 0.35, "wrinkle_strength": 0.25, "scatter_color": [1.0, 0.42, 0.26] } } },
  { "name": "hair", "doubleSided": true, "extras": { "evani_kind": "hair", "evani_hair": { "highlight_color": [1.0, 0.97, 0.92], "shift": 0.1, "edge_softness": 0.45, "tip_width": 0.25 } } } ]
```

Every value in `evani_eye`, `evani_skin` and `evani_hair` is optional: a missing one uses the live param of the same name (`material.eye.iris_color`, `material.skin.pore_scale`, `material.hair.tip_width` and so on; `shift` is `material.hair.primary_shift`, `highlight_color` is `material.hair.primary_color`). Colours are linear RGB.

What each kind needs from the mesh:

| Kind | UVs (`TEXCOORD_0`) | Notes |
|---|---|---|
| `eye` | Azimuthal round the eyeball's axis: uv (0.5, 0.5) on the pupil axis, the distance from it = the angle from the axis / 2 pi (so the iris edge, at angle asin(`iris_size`), sits at about 0.08 from the centre). | One sphere per eye, looking out along its axis. The cornea's bulge and refraction, the iris fibres, the limbal ring, the wet highlight and a catchlight are all drawn in the shader; the base colour is not used. |
| `skin` | Any even mapping; `pore_scale` counts pores per UV unit. | Light softens across the shadow edge by surface curvature (a fitted pre-integrated lookup) on Medium and up, pores from Medium, fine wrinkles across v on High and Ultra; Lite and Low use a wrapped, warm-edged diffuse. |
| `hair` | u: each whole step is one lock (lock number = floor(u)); v: 0 at the root, 1 at the tip. | Lay hair as cards or shells, double-sided. Strands run along v (the highlight follows dv on screen), each lock tapers to `tip_width` with a soft edge, and the edges are drawn with a coverage mask (smooth with 4x MSAA, dithered without), so no sorting is needed. Keep stacked shells (and a base cap under them) at least 1/32 of the model's longest side apart (10 mm on a 30 cm head): Lite and Low's baked sun starts its rays that far off a surface, so a layer packed closer under another reads sunlit there while the shadow map shades it. |

Quality follows the tier (`material.character.quality` -1), or a fixed 0 (Lite) to 3 (High). `material.character.enabled` 0 turns the character shading off for a before-and-after look. Without the lit surface pass (a game whose `relief` stage wins) the tags are read but nothing changes.

## Clips

An animated model's tier files carry their animations as one compact **clip block** in the GLB's binary chunk, not as glTF `animations` (the converter, S3-7; the in-game decoder is `evani-anim`, S3-8). The master keeps the standard glTF animations untouched. The reference reader and writer are `tools/evani-guide/src/clipblock.rs`; the converter, the shared model check and `evani-anim`'s decoder all follow it, and `tools/evani-convert/tests/anim_crosscheck.rs` holds the two readers to the same sample bits and the same refusals. Two rules both readers keep: every f32 in the block is finite (a block with a NaN or an infinity is refused), and a constant rotation (keys = 1) is normalised as it is read (a zero-length one reads as the identity).

```json
"clips": { "format": 1, "view": 12, "rate": 30, "list": [ { "name": "idle", "frames": 61 }, { "name": "wave", "frames": 31 } ] }
```

`view` is the buffer view (on buffer 0, the binary chunk) holding the block; `list` names the clips in block order, with their frame counts. `rate` is always 30.

**Block** (little-endian throughout; fixed layouts, no general-purpose compression, so the decoder stays about 1 to 2 KB of wasm):

| Part | Layout |
|---|---|
| Header | `EVC1`, u16 clip count, u16 reserved (0), then one u32 offset per clip from the block's start, each a multiple of 4 (clips are zero-padded to 4 bytes). |
| Clip header | u16 frames (samples at 30 Hz; duration (frames − 1) / 30 s), u16 tracks, u8 rot_bytes (4 or 6), u8 index_bytes (1 when frames ≤ 256, else 2), u16 reserved (0). |
| Track headers | 6 bytes each, sorted by (node, channel), no repeats: u16 node (index in this tier file), u16 keys (1 ≤ keys ≤ frames), u8 channel (0 translation, 1 rotation, 2 scale), u8 reserved (0). |
| Track data | In header order. **keys = 1**: a constant, f32 × 3 (f32 × 4, x y z w, for a rotation). **keys ≥ 2**: translation and scale first carry f32 × 3 min and f32 × 3 extent; then the key frame numbers (index_bytes each, first 0, last frames − 1, strictly increasing); then one value per key: a rotation in rot_bytes, a translation or scale as u16 × 3 with v = min + extent × q / 65535. |

**Rotations** are smallest-three in an unsigned integer of rot_bytes: bits 0–1 name the dropped (largest, made positive) component; the other three follow in x y z w order, 10 bits each in 4 bytes (10-10-10-2) or 15 bits each in 6 bytes (48 bits, one spare). Each stored component is `q / max × 2 − 1` times 1/√2; the dropped one is √(1 − the sum of the others' squares).

**Sampling** at time t: x = t × 30, clamped to [0, frames − 1]. Between the two keys around x, translation and scale lerp; rotation nlerps (the second key negated when the dot product is below 0, then normalised). A node with no track in the clip stays at its rest transform in the file. Clips are not looped or blended by the block; that is the player's (`evani-anim`: integer 60 Hz ticks, two per frame; a looped clip wraps at its last frame, another holds it).

**What the converter bakes** (`tools/evani-convert/src/clips.rs`): every channel resampled at 30 Hz (STEP held, LINEAR with slerp for rotations, CUBICSPLINE as Hermite curves with tangents × key spacing); the clip runs from 0 s to its last key time. Error is measured at virtual shell points: six points around each joint at its shell distance (the farthest direct child, the mesh radius of a rigid mesh node, never under 3 cm), in world space, against the resampled master over every frame and every node at or below an animated one. A track within tolerance of the rest pose is dropped, one within tolerance of its first frame is stored once, any other keeps only the keys a greedy forward pass needs on the quantised values. The converter tries 32-bit then 48-bit rotations at tolerances bound / 2^k and keeps the smallest clip whose worst shell error is within the tier's bound (`clip_error_m` in the converter's ladder.json, or `--clip-error <m>`): lite 2 mm, low 1 mm, medium 0.5 mm, high 0.3 mm, ultra 0.2 mm, vr 0.3 mm. The report lists each clip's bytes, keys, rotation bits and worst shell error. Morph-target `weights` channels are not baked (noted in the report); an animated node with a `matrix` is refused. Animated nodes and skin joints are given unique names before packing (gltfpack keeps named nodes), and the converter checks that each tier file's animated nodes kept the master's rest pose. A clip over its bound even with every key kept is a converter problem, with a note naming the cause: a translation or scale track whose range makes its 16-bit steps alone too coarse (long root motion: loosen the bound or split the clip), or rotations finer than 48 bits hold at that distance from the joint.

**Determinism.** The same input gives the same bytes on one platform and toolchain: tracks are sorted by node and channel, never in input order, and nothing reads a clock or a hash map. The resampler's slerp uses `sin` and `acos` from the platform's maths library, whose last bit can differ between platforms, so another platform may, rarely, keep a different key; its clip is still within the bound.

### Sockets

A socket is an anchor with a rotation: where and how an attached model sits (a weapon in a hand, a lamp on a post). The root `extras.evani.sockets` lists them by name, in the model frame (the scene root):

```json
"sockets": { "right_hand": { "p": [0.31, 1.02, 0.08], "q": [0.0, 0.7071, 0.0, 0.7071], "part": "hand.r", "joint": "hand_r" } }
```

| Key | Type | Meaning |
|---|---|---|
| `p` | [x, y, z] | Position in metres, model frame (for a joint socket, the rest pose). Required. |
| `q` | [x, y, z, w] | Unit quaternion: the attached model's frame in the model frame. Required. A point `l` of the attached model lands at `q·l + p`. |
| `part`, `joint` | string | Optional: the part it belongs to, the joint it rides on. |

- **Evani.Model** writes a socket for every template anchor with a `q` (`"anchors": [{ "name": "right_hand", "at": "hand.r.palm", "q": [0, 0.7071, 0, 0.7071] }]`, numbers or formulas, normalised as read): the `anchor:<name>` node gets a glTF `rotation` and `q` in its `extras.evani`, and the root gets the `sockets` entry. An anchor without `q` is written exactly as before.
- **`cargo xtask convert`** turns every socket into an `anchor:<name>` node with that rotation: it adds one at the scene root when the file has none, or sets the rotation of the one there (it must stand at `p` within 0.1 mm, an existing rotation must agree, and its parents may move, turn and scale it evenly; a `matrix` or an uneven scale above it is refused, each with a `fix:` line). The `sockets` entry stays in the metadata.
- **The engine** reads no `sockets` key: the anchor node's world matrix already carries the rotation, so `Models::anchor_frame` returns it (docs/CONTENT.md "Anchors"). A game that attaches nothing pays nothing, and a file without sockets converts byte for byte as before.

## Nodes

| Node | Name | `extras.evani` | Meaning |
|---|---|---|---|
| Root | the class (`kart`) | none | The scene's single root; every other node hangs below it. |
| Solid | the solid's name | `kind: "solid"`, `origin` [x,y,z], `mirrored` bool | Carries a mesh. Its scale and translation undo position quantisation (uniform scale, negative x for a mirrored instance). |
| Anchor | `anchor:<name>` | `kind: "anchor"`, `normal` [x,y,z], `part`; a socket adds `q` [x,y,z,w] (and the node a glTF `rotation`) | An empty node marking a gameplay point (`anchor:driver_seat`, `anchor:item_slot`). Its world matrix is the anchor (position, and the rotation for a socket); the engine lists every one (`Gltf::anchors`). |
| Collision | `collision` | `kind: "footprint"`, `shape: "capsule"`, `centre_xz`, `radius`, `half_segment` (when the model has a footprint) | Groups the collision proxies; never drawn. |
| Proxy | `col:<name>` (children of `collision`) | `kind: "collision"`, `solid`, `proxy` (shape and size), `missed`, `excess` | One collision shape per solid, placed by the node's translation and rotation. `missed` and `excess` are the fit errors. |

Names are the identifiers. Mesh names are not: tools drop them (below).

## Geometry

- Positions: unsigned 16-bit normalised over the model's box, undone by the solid node's transform. Normals: signed 8-bit normalised (Low, Medium) or 16-bit (High, Ultra). Tangents: signed 8-bit normalised, w exactly ±1. UV 0: unsigned 16-bit normalised in 0..1. Indices: 16-bit under 65,536 vertices, otherwise 32-bit.
- Materials (when present) use every core metallic-roughness field: base colour, metallic-roughness, normal, occlusion and emissive textures and factors, so a realistic look later needs no re-conversion. Evani.Model M1 writes no materials; the look reads its maps.

## After gltfpack

The converter (S3-5) runs gltfpack 1.3 with `-kn -ke` (keep node names and extras). Seen in the shared fixtures:

- **The root `extras` is dropped**, even with `-ke`. The converter must copy the root `extras.evani` from its input into each output file (adding `lods` and `textures`). A packed file without it reads fine but has no part tree. The converter copies the whole root `extras` back (an author's own keys beside `evani` too) and sets `evani.tier`.
- **`-vn` other than 8 or 16 is not unit length.** gltfpack stores 12-bit normals in int16 without scaling them, so they decode to length 1/16 and light dark. The Khronos validator can't see inside EXT_meshopt_compression; the converter validates an uncompressed twin of each tier as well, and only allows 8 or 16 normal bits.
- Node names and node `extras` survive, so anchors, the `collision` node and solid kinds survive.
- Each mesh moves onto a new unnamed child of its named node; that child carries the dequantisation scale and offset. Look meshes up by the named ancestor, not by the mesh node itself.
- Mesh names are dropped.
- Identical textures are merged (the corpus's five references to one picture become one texture), so `textures[].texture` indices must be written after packing.
- Quantised UVs get a `KHR_texture_transform` scale.
- Animations are resampled to 30 keys a second.
- Vertex order differs with and without `-cc`; compare meshes by content, never by vertex index.
- Surface anchors may move by up to `cell_m` between tiers (Evani.Model re-seats them on each tier's surface); anchors off the surface (wheel hubs) stay put.

## Skins

A skinned model (S3-9; drawn only in games with `anim`, which turns on `evani-render/skinning`) is a standard glTF skin: one skin per model, `JOINTS_0` and `WEIGHTS_0` on every skinned primitive, one inverse bind matrix per joint (none means the identity), at most 256 joints. Only the first four influences are read, as the converter keeps four.

- **On the GPU.** Each skinned vertex gets a second stream of 8 bytes: four joint indices (u8x4) and four weights (unorm8x4, renormalised as they are read to sum to exactly 255). The vertex shader blends four 4x3 matrices from one read-only storage buffer that holds every character's joints this frame; each instance carries its offset into it. glTF ignores a skinned mesh node's own transform, and so does the engine.
- **Bind pose.** A draw without a pose shows the bind pose, `world(joint) x inverse bind` from the file's rest transforms, so a skinned model drawn with `Models::draw` looks like its static mesh. A kit poses one with `Models::draw_posed(instance, rig.skinning(&skin))` (evani-anim's matrices, joint order); a wrong matrix count draws the bind pose.
- **Joints.** Over 64 joints a rig still draws, but the converter's report and `cargo xtask check` warn: the soft cap. Over 256 is refused.
- **Shadows and the sun cone.** Skinned models cast skinned shadows on tiers with a shadow map. On Lite and Low the self-shadow sun cone is baked once in bind pose and turns with each vertex's limb (DECISIONS.md, "GPU skinning").

Skinned meshes add their own codes, under the same short-code rule (`crates/evani-render/tests/skinning.rs` keeps this table in step with the code):

| Skin code | Meaning |
| --- | --- |
| `K1` | model: more than one skin (one per model) |
| `K2` | model: a skin with more than 256 joints |
| `K3` | model: not one inverse bind matrix per joint |
| `K4` | model: a skinned mesh without JOINTS_0 and WEIGHTS_0 |
| `K5` | model: a joint index past the skin's joints |

## Look matching (`--look`, S3-12)

`cargo xtask convert <model> --look toon [--snap-palette]` cuts a general model (`.glb`, `.gltf`, `.blend`) down to what that look shows, before the clean, the tier files and the quality gate. Without `--look` every map is kept, as before. The one table is `SLOTS` and `LOOKS` in `tools/evani-convert/src/look.rs`; a unit test holds it equal to what `model.wgsl`, `model_relief.wgsl` and `fn material()` in evani-render really read, so it cannot drift. A look without a row keeps everything (the report says so).

- **Drops** every material input the look never reads. Toon reads base colour (factor and map), normal map, emissive (factor and map), alpha mode and cutoff, double-sided; it never reads the metal-rough map, the occlusion map, or the metallic and roughness factors. The images only those slots used are not encoded or shipped.
- **Merges** materials that are equal in everything left (their names aside), rewriting each primitive's `material`; gltfpack then joins primitives of one node that share a material, which cuts draw calls (the engine draws one call per primitive and material). Nodes, scenes, extras, anchors and part names are never touched, so `Variant` recipes (S3-11) work on a matched file. Skipped when the file uses `KHR_materials_variants` or `KHR_animation_pointer` (they point at materials by index).
- **`--snap-palette`** (opt-in) moves an untextured base colour to the look's nearest palette swatch when it is within a CIELAB distance of 15; the report lists every colour moved and every one left alone. It changes how the model looks (a little), so the gate scores it.
- **The master stays the original**, so the gate scores the matched tier files against the full model under the look. The report's `look_match` section (and "Look matching" in the markdown) shows, per tier, draw calls without and with matching, the maps not shipped and the download and texture memory saved, measured by packing and encoding the original the same way.
- **Relief:** a material's `extras.evani.relief` is never dropped, and the maps the toon relief shader samples (base, normal, height) are kept. The height map is named only by `height.index` in the recipe, which glTF-Transform's `prune` cannot see (the converter used to drop it, with or without `--look`: found by S3-12). So the converter parks it in the material's free `occlusionTexture` (else `metallicRoughnessTexture`) slot, marked `height.parked`, through the clean and the packing, then sets `height.index` to wherever the packed file put it and removes the slot and the marker. The image ships as `textures/height.<tier>.png` (linear). If both slots are in use the map is lost and a note says so. Materials with different recipes are never merged.
- Stage 5's `smooth-lighting` (CORE_REQUESTS) may make toon read metal-rough and occlusion: that is one change to the toon row in `look.rs`.
- **The gate's reference** (with or without `--look`): the engine draws a normal map only on a primitive with tangents, and the clean gives the tier files MikkTSpace tangents when a normal-mapped primitive has none. For such a master the gate renders a reference made from the master by the same steps (the engine's normals where a mesh has none, weld, glTF-Transform `tangents`), written to the gate folder; the `.master.glb` itself is unchanged. The report's gate table names the reference used. Usage is unchanged.

## Checks

`crates/evani-gltf/tests/reader.rs` holds both sides to this document: it reads the gltfpack corpus (with and without compression, with normalised positions, with a fallback buffer) and Evani.Model's own files, and checks that anchors, parts and metadata come through. Any change to the reader, the decoder or Evani.Model's writer runs those tests as part of `cargo xtask check`. `crates/evani-gltf/tests/error_codes.rs` keeps the table below in step with the reader.

`cargo xtask check` opens every shipped model (in a game, its kit, or the shared `assets/models/`; not `*.master.glb` or `.work/`) with this reader and ERRORs on what it would refuse or draw wrong: normals that are not unit length (decoded, so inside meshopt too), a `lods` node that isn't in the file, a referenced image, texture set or normal map that is missing, a texture set that isn't a plain KTX2 of its format, an `extras.evani.clips` whose buffer view is missing or whose clip block does not parse or match its list, and a material `extras.evani.relief` the engine would turn off (see Surface relief). The same rules (`evani_guide::modelcheck`) check every file the converter writes.

## Error codes

A file the reader can't use is an error, never a crash. In a game's download (wasm) the error carries only a short code, which keeps the reader small; native builds (the converter, tests, tools) carry the full text. Look a code up here. Each code has one meaning, and a code is never reused for something else.

| Code | Meaning |
| --- | --- |
| `A1` | anim: sampler accessor out of range |
| `A2` | anim: channel sampler out of range |
| `A3` | anim: channel without target |
| `A4` | anim: channel node out of range |
| `A5` | anim: channel without path |
| `B1` | glb: bad magic (not a glTF binary) |
| `B2` | glb: not glTF version 2 |
| `B3` | glb: truncated header |
| `B4` | glb: length field does not match the file |
| `B5` | glb: truncated chunk header |
| `B6` | glb: bad chunk length |
| `B7` | glb: chunk runs past the end |
| `B8` | glb: no JSON chunk |
| `B9` | glb: first chunk is not JSON |
| `E1` | evani: extras.evani is not an object |
| `E2` | evani: extras.evani has no format number |
| `E3` | evani: extras.evani format is newer than this engine reads |
| `E4` | evani: part without a name |
| `E5` | evani: LOD without a node name |
| `E6` | evani: texture set for a missing texture |
| `E7` | evani: texture set without a file |
| `G1` | gltf: the JSON does not parse |
| `G2` | gltf: the JSON is not an object |
| `G3` | gltf: asset.version is not 2.x |
| `G4` | gltf: the file requires an extension this reader does not support |
| `G5` | gltf: buffer data is shorter than its byteLength |
| `G6` | gltf: buffer index out of range |
| `G7` | gltf: a view reads a buffer with no data (missing file or fallback buffer) |
| `G8` | gltf: bad byte range |
| `G9` | gltf: byte range past the end of its buffer |
| `G10` | gltf: buffer view index out of range |
| `G11` | gltf: accessor index out of range |
| `G12` | gltf: byteStride smaller than the element |
| `G13` | gltf: accessor too large |
| `G14` | gltf: accessor runs past the end of its buffer view |
| `G15` | gltf: integer accessor expected |
| `G16` | gltf: image index out of range |
| `G17` | gltf: a number array has the wrong length |
| `G18` | gltf: a number array holds a non-number |
| `G19` | gltf: a property that must be an array is not |
| `G20` | gltf: an offset or stride is not a whole number |
| `G21` | gltf: missing or out-of-range index |
| `G22` | gltf: index list entry out of range |
| `G23` | gltf: buffer without byteLength |
| `G24` | gltf: buffer view without byteLength |
| `G25` | gltf: a buffer view runs past its buffer's byteLength |
| `G26` | gltf: sparse accessors are not supported |
| `G27` | gltf: bad componentType |
| `G28` | gltf: unsupported accessor type |
| `G29` | gltf: accessor without count |
| `G30` | gltf: attribute accessor out of range |
| `G31` | gltf: a node has two parents or is its own child |
| `G32` | gltf: the node hierarchy has a cycle |
| `M1` | meshopt: view length or stride does not match the extension |
| `M2` | meshopt: count too large for the compressed data |
| `M3` | meshopt: view too large |
| `M4` | meshopt: unknown mode |
| `M5` | meshopt: unknown filter |
| `M6` | meshopt: compressed data in a fallback buffer |
| `M7` | meshopt: no byteLength |
| `M8` | meshopt: no byteStride |
| `M9` | meshopt: no count |
| `M10` | meshopt: a plain view reads a fallback buffer |
| `M11` | meshopt: corrupt or truncated stream |
| `M12` | meshopt: byteLength is not count * byteStride |
| `M13` | meshopt: bad ATTRIBUTES byteStride |
| `M14` | meshopt: bad index byteStride or filter |
| `M15` | meshopt: vertex codec version other than 0 (EXT allows only v0) |
| `M16` | meshopt: bad ATTRIBUTES header |
| `M17` | meshopt: TRIANGLES count is not a multiple of 3 |
| `M18` | meshopt: index codec version other than 1 |
| `M19` | meshopt: bad TRIANGLES header |
| `M20` | meshopt: index sequence codec version other than 1 |
| `M21` | meshopt: bad INDICES header |
| `M22` | meshopt: OCTAHEDRAL needs byteStride 4 or 8 |
| `M23` | meshopt: QUATERNION needs byteStride 8 |
