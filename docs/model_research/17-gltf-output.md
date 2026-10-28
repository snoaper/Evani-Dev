# 17 glTF 2.0 output with quantization and Evani metadata

M0 clean-room note, 2026-10-15. Own words from the Khronos specifications. No third-party code opened or copied.

## What it must do (brief, design instruction 11, originality 4)
Output glTF 2.0 with quantization, plus Evani metadata (part tree, anchors, height map, micro-detail recipe, size per tier). Compression is handled by evani-convert. The standard is followed but implemented ourselves.

## Known approaches and the standard
- **glTF 2.0** (Khronos): JSON scene description plus binary buffers (the single-file `.glb` form packs both); meshes are primitives with attribute accessors (POSITION, NORMAL, TANGENT, TEXCOORD_n) and an index accessor; nodes carry transforms; `extras` is free application data on any object; extensions are declared in `extensionsUsed`, and those a reader must understand also in `extensionsRequired`.
- **KHR_mesh_quantization** (in our words): allows smaller integer types for attributes: positions as signed or unsigned 8- or 16-bit integers (normalised or not), normals and tangents as normalised signed 8- or 16-bit, texture coordinates as signed or unsigned 8- or 16-bit (normalised or not). Accessor bounds are stored in the quantised values. Positions are dequantised by the node's transform (keep the scale uniform so normals stay correct); texture coordinates outside the integer type's natural range need KHR_texture_transform. Because files using it cannot be read without it, it must be listed in `extensionsRequired`.
- **Compression extensions** (EXT_meshopt_compression, KHR_draco_mesh_compression, KHR_texture_basisu) are applied later; in Evani.Dev that is evani-convert's job (STAGES.md, Stage 3).
- **LOD in glTF**: there is no core LOD mechanism; a vendor extension exists (MSFT_lod), or separate files per level.
- **Validation**: the Khronos glTF-Validator (Apache-2.0) checks files against the specification; we run it separately as a test tool.

## Our approach
- **Our own writer**, producing `.glb` files with serde_json for the JSON chunk and our own binary layout (4-byte alignment, accessor byte strides per the specification).
- **One file per tier** (`<model>.low.glb`, `.medium.glb`, `.high.glb`, `.ultra.glb`, and `.vr.glb` when asked): a tier downloads only its own LOD (matches the Stage 3 gate "Low downloads only the LODs it uses"), and evani-convert can compress each independently. A shared `<model>.evmodel.json` metadata file and per-tier textures (normal and height maps) sit next to them.
- **Quantisation.**
  - POSITION: unsigned 16-bit normalised over the model's bounding box, with the dequantisation as a uniform scale plus translation on the mesh's node; error at most half a step (for a 3.1 m kart about 0.024 mm, far below any tier's pixel).
  - NORMAL: signed 8-bit normalised on Low and Medium, 16-bit on High and Ultra (chosen by the measured shading error).
  - TANGENT: signed 8-bit normalised, fourth component exactly ±1.
  - TEXCOORD_0: unsigned 16-bit normalised; our packer keeps every chart inside 0 to 1, so no KHR_texture_transform is needed.
  - Indices: 16-bit when a tier's mesh has under 65,536 vertices, otherwise 32-bit.
  - KHR_mesh_quantization listed in both `extensionsUsed` and `extensionsRequired`.
- **Nodes.** The node tree mirrors the part tree (one node per part or merged group, named after the part), so parts can be hidden or animated by name; gameplay anchors are empty nodes named `anchor:<name>`; collision proxies sit under a `collision` node (note 15).
- **Evani metadata** in the root `extras.evani` (not a custom extension, so every glTF reader still loads the file): format version, model hash, intent hash and seed (provenance), part tree with roles, anchors with frames, per-tier triangle counts and byte sizes, relief layers with their tier deliveries, the micro-detail recipe for the look's shader, the collision proxies' parameters, and the scorecard summary. Large data (height maps) are referenced as files.
- **Deterministic bytes.** JSON keys in a fixed order, numbers printed by our own fixed formatter, buffers laid out in part order, so the same model gives the same bytes and hash on every machine.

## Why it should be better
- One file per tier gives smaller downloads than a single file with all LODs, and lets each tier be compressed and streamed separately.
- Metadata from the part graph (anchors, parts, collision) arrives with the model, so the kit reads its numbers from the model rather than constants.
- Byte-deterministic output makes model hashes meaningful across machines.

## Baseline and measure
- Khronos glTF-Validator: zero errors and zero warnings on every benchmark output.
- Bytes per tier (raw and after evani-convert once it exists); quantisation error (maximum position error, normal angle error).
- Load test: the files load in the engine's Stage 3 importer when it exists; until then, in Blender's importer run separately (count of loaded primitives and nodes).

## Risks
- **Readers that ignore `extensionsRequired`** may misread quantised data; mitigation: the validator and our own reader test.
- **`extras` size** if scorecards grow; mitigation: keep only the summary, full scorecards stay in the dev-side library.

## Sources (ideas only)
Khronos glTF 2.0 specification; KHR_mesh_quantization extension specification; KHR_texture_transform; EXT_meshopt_compression; MSFT_lod; Khronos glTF-Validator README (as a tool description).
