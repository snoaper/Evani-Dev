# Stage 3 build plan: Real content

Status: building started 2026-10-16 on Chris's word ("so get moving on finishing stage 3 and 4"), before Gate 2's sign-off; Gate 2 still needs his play-through and signature. Planned 2026-10-15; item S3-0 (spikes) ran first on a scratch branch. S3-1 is built (see its line); the other items are not.

Sources: STAGES.md (Stage 3 and the Evani.Model track), CORE_REQUESTS.md, DECISIONS.md, README.md, docs/LIVE_API.md, docs/EVANI_MODEL.md and docs/model_research/17-gltf-output.md and 20-integration.md, and the research reports in `.vibedeck/research/` (2026-10-15): model loading (`...-ktx2-texture-2.md`, the updated card), animation, converter, text/UI and streaming.

## 1. In plain words (for Chris)

Stage 3 lets Evani.Dev games use real 3D models instead of shapes built from boxes and cylinders.

- **A converter** takes a detailed model (from Blender, or made by an AI) and makes a much smaller copy for each quality level (Low, Medium, High, Ultra). It checks its own work: it photographs the original and the small copy from 8 angles on every level, and keeps squeezing only while nobody could tell them apart. Each model gets a report of its size and quality.
- **The engine** learns to read those models, show their textures and surface detail, animate characters (blending moves, hands that stay on the steering wheel), and pick the right detail level by distance.
- **Downloads stay small.** A phone on Low downloads only the Low files. Detailed files arrive in the background after the title screen. Games that don't use models download none of this code.
- **Text and menus drawn in the game** get sharp, scalable fonts and a tiny layout helper.
- **In the Evani.dev tab** you drop a model onto the game, it converts, appears in the running game, and a slider shows before and after.
- **The proof:** a detailed Blender kart and an AI-made character, converted and driving in the kart racer.

Most approaches are already chosen by you on the research cards (section 3). You answered the 11 choices in section 7 "all as recommended". Decision 12 (Hexagon, section 8b) is decided too: option (b), a Hexagon soldier and weapon as extra converter tests.

## 2. Where Stage 3 starts from (what Stage 2 built)

| Today | What Stage 3 adds |
|---|---|
| 3D forward path draws built-in meshes (cube, sphere, bevelled cube, cylinder, cone, dome...) plus up to 56 custom meshes (`meshes` feature, `MeshLibrary`), vertices are position + normal only, no UVs, no textures; materials are flat look colours (`MaterialDesc`) with the toon ramp | Imported, quantized, textured meshes with normal maps, per-tier files and LODs (S3-2 to S3-4) |
| The kart racer builds its karts and drivers from primitives (`draw_kart`, kits/evani-kit-kart-racer/src/draw.rs) | A converted hero kart and an animated driver (S3-16) |
| `sprite-art` and `text` features: named sprite sheets with mips and a 5 x 9 bitmap font, `text::draw` / `text::measure` (CORE_REQUESTS items A and B) | MSDF fonts, word wrap, kerning and a box layout grow from the same API (S3-13) |
| `evani_core::assets`: a manifest keyed by content hash with an async loader | Tier, priority and group per entry; background fetch and Cache Storage (S3-4) |
| Live tooling: focus mode and the asset view (`focus.*`, `asset.*`, LIVE_API 6.28), `capture.frame`, `compare.ab`, the look guard, headless Chrome runner (`tools/bench-runner/run.mjs`, Node built-ins only) | Model commands, drop-to-convert and the before/after slider (S3-14); the quality gate reuses the capture path and the runner (S3-6) |
| geocheck with its own `meshcheck` | Converted models are checked too; the kart racer's adapter reads model meshes (S3-16) |
| Tier presets carry no texture-memory number (the catalog has `max_texture_memory`) | Carried into `assets/tiers/*.json` (S3-1) |

## 3. Approaches already chosen by Chris (research cards, 2026-10-15)

These are decided; the items below follow them. Licence rule for every one: we borrow the approach; code is copied only with attribution (keep the copyright notice and say where it came from, in the file header and a new `THIRD_PARTY_NOTICES.md`).

**Model loading** (updated card):
1. Our own GLB/JSON reader plus a small Rust port of the scalar meshopt decoder ([meshoptimizer](https://github.com/zeux/meshoptimizer), MIT). Test files made by gltfpack; the pure-Rust meshopt-rs (MIT) read only as a cross-check. No `gltf`, goth-gltf or meshopt crate.
2. Per-device texture sets, chosen by feature check ([basis_universal](https://github.com/BinomialLLC/basis_universal), Apache-2.0, approach only): ready-made ASTC (phones), BC7 (desktop) and an RGBA fallback; three.js KTX2Loader's order of preference; ASTC preferred over BC when both are reported (Safari bug [295566](https://bugs.webkit.org/show_bug.cgi?id=295566)). No transcoder in the game and no new dependency.
3. LODs as per-tier files made by gltfpack ([gltfpack README](https://github.com/zeux/meshoptimizer/blob/master/gltf/README.md), MIT): the lowest tier ships with the game or the title download, higher tiers arrive after the title screen and swap in; same node names across tiers. (Ticked on the first card; required by the gate line "Low downloads only the LODs it uses".)

**Animation** (all five together):
1. [ozz-animation](https://github.com/guillaumeblanc/ozz-animation) (MIT) as the blueprint for a small in-house Rust runtime: bind pose, sample, blend, model space, skinning matrices; nlerp; two-bone and aim IK.
2. Build-time baking and compression in our own tool (ozz offline tools and [ACL](https://github.com/nfrechette/acl) as references, both MIT): resample, key reduction within an error bound, quantised rotations, a compact binary clip format; no glTF animation parsing in the shipped wasm.
3. GPU skinning in the pattern of three.js SkinnedMesh and bevy ([three.js](https://github.com/mrdoob/three.js) MIT, bevy MIT or Apache-2.0): linear blend skinning in WGSL, one storage buffer of 4x3 joint matrices with per-instance offsets, u8x4 joints and unorm8x4 weights.
4. [bevy_animation](https://github.com/bevyengine/bevy/tree/main/crates/bevy_animation) and Godot AnimationTree (MIT or Apache-2.0, MIT) as design references: only clip, blend-of-two and masked additive nodes; tick-based crossfades; 1D blend spaces.
5. ACL (MIT): only the idea of an error measured at a virtual "shell" distance from each joint, and per-clip bit-rate selection; none of its code.
Not used: ozz-animation-rs (MPL-2.0, nightly compiler).

**Converter and quality gate:**
1. gltfpack / meshoptimizer (MIT) as the geometry and packing engine, run as an external program per tier.
2. [glTF-Transform](https://gltf-transform.dev/cli) (MIT, docs CC-BY-3.0) as the reference pipeline and fine steps (dedup, prune, weld, simplify, resize, then compress; inspect and validate for reports), run as an external Node program.
3. Our own "invisible difference" gate on image metrics: [NVIDIA FLIP](https://github.com/NVlabs/flip) (BSD-3-Clause, the worst-pixel idea) and SSIMULACRA2 (Rust crate, licence to verify before pinning): a score threshold plus a worst-pixel ceiling, binary-search back-off over 8 views per tier.
Not picked: KTX-Software as its own choice, Blender normal-map baking, and the Blender add-on/auto-LOD ideas (the consequences are open decisions 2, 5 and 6 in section 7).

**Text, UI and streaming:**
1. MSDF font atlases made at build time with [msdf-atlas-gen](https://github.com/Chlumsky/msdf-atlas-gen) (MIT), drawn by one small shader; only the atlas and a compact table ship; only the character sets a game lists; the bitmap font stays; complex-script shaping stays in the DOM.
2. [Clay](https://github.com/nicbarker/clay)'s layout model as an idea only (Zlib): a tiny in-house row/column box layout, fixed arrays, no allocation, no dependency. Not Taffy.
3. Content-hashed immutable files with Cache Storage and HTTP range requests ([HTTP cache partitioning](https://developer.chrome.com/blog/http-cache-partitioning); web platform, our own code): per-tier manifests, a "boot" group for the title, the rest fetched by priority in the JS shim.
Not used: a microui-style widget set, egui.

**One reconciliation.** Chris picked both "our own glTF reader" (meshes) and "no glTF animation parsing in the shipped wasm" (clips). Both hold: the runtime reader parses only the JSON fields for meshes, skins, materials, nodes and `extras.evani` (JSON parsing itself costs nothing new, because `serde_json` is already in every game through evani-core), and the clips travel in our compact binary format inside the same GLB, referenced from `extras.evani`. Standard glTF readers still open the file; they just don't see the compact clips. The master file keeps the standard glTF animations.

## 4. Modules and feature flags

Build-time on/off stays strict: every new piece is an optional Cargo feature, and a game that doesn't list it downloads none of it (proven by byte-identical ship builds, section 6, line 4).

| `evani.toml` module | Turns on | Contains |
|---|---|---|
| `models` | `evani-gltf` (optional dep), `evani-render/models`, the `stream` JS shim | GLB reader, meshopt decoder, textured and normal-mapped 3D path, compressed textures, LOD selection, tiered streaming |
| `anim` (implies `models`) | `evani-anim` (optional dep), `evani-render/skinning` | Clip decoder, sampler, blend nodes, IK, GPU skinning |
| `ui` | `evani-ui` (optional dep), `evani-render/msdf` (implies `text`) | MSDF text, layout, wrapping |
| (look feature) `relief` in evani-look-toon and `evani-render/relief` | turned on by a game whose models carry relief | Height parallax on High and Ultra, the micro-detail slot |

New crates: `crates/evani-gltf` (reader, no GPU, native and wasm), `crates/evani-anim` (pure CPU), `crates/evani-ui` (layout). Not `evani-model`: that name belongs to the Evani.Model track. New tools: `tools/evani-convert` (native, dev-only), `tools/convert-runner` (Node built-ins, like bench-runner), `apps/model-view` (dev-only app for the quality gate and the tab), `tools/blender-addon/` (only if decision 5 keeps it). Every module and shader carries the usual `evani.module.<name>` / `evani.shader.<name>` markers.

## 5. Work items, in order

Each item ends with `cargo xtask check` clean and `cargo xtask check --full` before hand-over, keeps all four games building and passing, records each new choice in DECISIONS.md, updates the AI guidance block (`cargo xtask init-ai`) for anything an agent must know, and commits. "Non-users unchanged" below always means: the 2d-platformer and match-3 ship builds (wasm and js) have the same SHA-1 before and after, and their marker scans show none of the new markers.

Tracks after S3-2 (each item's "Depends on" line is the rule; the tracks only group them):
- A runtime: S3-3, then S3-10 and S3-4, then S3-11.
- B converter: S3-5 (needs only S3-2), then S3-6 (also needs S3-3), then S3-12 and S3-14 (S3-14 also needs S3-4), then S3-15.
- C animation: S3-7 (needs S3-5), then S3-8, then S3-9 (also needs S3-3).
- D text and UI: S3-13a and S3-13b need only S3-1, so they can start first.

Items that change `crates/evani-render/src/gpu/frame.rs` or `pipelines.rs` (S3-3, S3-9, S3-10, S3-13a) merge one at a time, each rebased on whatever merged before it, in order of readiness. S3-10 goes straight after S3-3, because Evani.Model's M4 waits for it; it never waits behind the animation chain.

### S3-0. Spikes and tool check (scratch branch; may run before Gate 2): S, sonnet-medium
- What: throwaway checks, no code kept: (a) the current gltfpack release on Windows: `-si`, `-cc`, `-kn`, `-ke` (keep names and extras), `-tc` (does it have a built-in Basis encoder, or does it need KTX-Software?); (b) how to make ASTC 4x4, BC7 and RGBA KTX2 sets on Windows (KTX-Software `ktx create` / `ktx transcode`, or another route); (c) which compressed-texture features WebGPU reports on the dev PC, Chris's phone (SM-G991B) and tablet (TB305FU) (`texture-compression-astc`, `-bc`, `-etc2`), through a tiny test page; (d) whether a file dropped onto the game page reaches the page inside the Evani.dev tab's view; (e) the SSIMULACRA2 crate's licence, dependency tree and build time; (f) a Windows build of msdf-atlas-gen; (g) whether `navigator.storage.persist()` prompts in Chrome, Edge and Firefox.
- Tool: scratch branch `stage3-spikes`, notes in `docs/stage3/spikes.md` (kept), nothing else merged.
- Feature: none (nothing is built into main).
- Closes with: each question answered with the command and its output; open decisions 1 and 2 updated with the facts. **Done 2026-10-15** (docs/stage3/spikes.md, merge 44d2bc5): all seven answered except the tablet (its child-lock app blocks Chrome) and the drop test inside Evani.App's Evani.dev tab, which Chris tried (nothing happened: the view does not open dropped files; whether a page handler receives them is checked first in S3-14); Firefox's `persist()` behaviour is from its documentation only (Firefox is not installed; low priority).
- Depends on: nothing. Size budget: none (nothing ships).

### S3-1. Module wiring and tier numbers: S, sonnet-medium
- What: the `models`, `anim` and `ui` modules in `evani.toml` (xtask maps them to app features; `anim` implies `models`); markers and `cargo xtask sizes` columns for them; evani-guide check ERRORs with `fix:` lines: a game that has `.glb` assets but no `models`, a skinned model without `anim`, a font listed without `ui`; `max_texture_memory` added to `assets/tiers/*.json` as plain megabytes from catalog/tiers.json (96 / 192 / 512 / 1536, VR 256) and read into `TierPreset`; `new-game` templates and the AGENTS block mention the modules.
- Tool: tools/xtask, tools/evani-guide, evani-core tiers.
- Feature: the modules themselves; nothing new is compiled for games that don't list them.
- Tests: `maps_models_anim_ui`, `glb_without_models_is_an_error`, `texture_memory_follows_the_catalog`.
- Budget: 0 bytes for every current game (SHA-1 identical; the tier number is read only by `models` code).
- Depends on: S3-0.
- **Done 2026-10-16 (merge 638b4a4).** The app crates declare `models = ["evani-core/texture-budget"]`, `anim = ["models"]` and `ui = []` now; S3-2, the anim crate and the ui crate each add their `dep:` to their own feature line and their `evani.module.<name>` marker (the on/off check expects it exactly when listed). `max_texture_memory` exists only with evani-core's `texture-budget` feature (a game's `models` turns it on): ship wasm and js SHA-1 are identical before and after for all four games. Lite is 64 MB (the catalog's value; the item text listed the other five).

### S3-2. `evani-gltf`: our own GLB reader and meshopt decoder: M, opus-high
- What: a new crate. GLB container (header, JSON chunk, BIN chunk) and `.gltf` with external buffers; only the fields used (buffers, bufferViews, accessors, meshes and primitives, nodes, skins, materials with every core metallic-roughness field: base colour, metallic-roughness, normal, occlusion and emissive textures and factors, so a later realistic look needs no re-conversion; textures and images; `extras.evani`), parsed through `serde_json::Value` (already in every game). KHR_mesh_quantization (normalised and integer attributes; node scale and offset). EXT_meshopt_compression: a scalar port of `decodeVertexBuffer` / `decodeIndexBuffer` (ATTRIBUTES, TRIANGLES and INDICES modes, v0 bitstream, OCTAHEDRAL, QUATERNION and EXPONENTIAL filters); fallback buffers with no data are ignored. Anchors from empty nodes named `anchor:<name>`, the part tree, LOD thresholds and texture-set names from `extras.evani`. A native-only feature `anim-source` reads glTF animation channels and samplers for the converter (S3-7); no game turns it on. Writes `docs/MODEL_FORMAT.md`: the `extras.evani` schema we read, versioned, taken from docs/model_research/17-gltf-output.md so Evani.Model and the engine follow one document.
- Feature: the crate is an optional dependency, pulled in only by `models`.
- Tests: `decodes_gltfpack_corpus` (the same model with and without `-cc` decodes to identical attributes), `dequantizes_like_the_spec`, `fallback_buffers_are_ignored`, `reads_evani_extras_and_anchors`, `reads_evani_model_fixtures` (fixture files written by Evani.Model's M1 writer, copied into `tests/fixtures/models/`), `bad_input_is_an_error_not_a_panic` (truncated files, bad magic, wrong offsets, random bytes), `anim_source_is_off_in_game_builds`.
- Budget: at most 8 KB gzip wasm for games with `models`; 0 for others.
- Attribution: if any decoder code is ported rather than written from the extension spec, the MIT notice goes in the file header and `THIRD_PARTY_NOTICES.md`.
- Depends on: S3-1.
- **Done 2026-10-16 (merge c8b712c).** All seven tests plus an error-code test pass; the corpus test caught a real meshopt decoder bug (fixed) and runs in `cargo xtask check --full`. docs/MODEL_FORMAT.md holds the `extras.evani` schema and the error codes. Size: +15,945 B gz in the sandbox ship build, above the 8 KB estimate after shrinking (Chris accepted it; DECISIONS lists what was tried). Non-users unchanged: the four games' ship wasm and js SHA-1 identical before and after (commit id pinned). The reader declares `evani.module.models`.

### S3-3. Textured, normal-mapped 3D path: L, opus-high
- What, in `evani-render` behind feature `models`:
  - A model pipeline beside the built-in mesh pipeline: quantized vertices stay quantized on the GPU (u16 positions with per-mesh scale and offset, i8 normals and tangents, u16 UVs), so memory matches the download.
  - Materials: base colour (texture times factor), tangent-space normal map (MikkTSpace convention), emissive, alpha opaque or mask, double-sided. The toon look's existing `toon_ramp` slot shades with the mapped normal (no slot signature change). Models cast and receive the tier's shadows and get the outline effect. Metallic-roughness and occlusion are kept in the material data (S3-2) but the toon look does not sample them, and the loader never fetches a map the active look does not sample, so they cost a toon game nothing even before look matching (S3-12) removes them at build time.
  - Textures from per-device sets: at device creation, request `texture-compression-astc`, `-bc` and `-etc2` only if the adapter has them and only with `models`; pick per KTX2Loader's order (ASTC 4x4, then BC7, then BC1/BC3, then ETC2, then RGBA), preferring ASTC whenever it is reported; the sets are listed per texture in `extras.evani.textures` (S3-5); parse the KTX2 container (levels, format; no supercompression, the server compresses the bytes); the RGBA fallback is the file's own core image (PNG or JPEG) decoded by the browser (`createImageBitmap`), so no decoder ships.
  - Texture memory held to the tier's `max_texture_memory`: over budget, the largest textures drop their top mip first, with an overlay note.
  - Draw calls counted against the tier's `max_draw_calls`.
- Feature: `models`. Non-users unchanged.
- Tests: `wgsl_validates_with_and_without_models` (naga), `format_choice_prefers_astc_over_bc`, `unsampled_maps_are_never_fetched`, `ktx2_header_reads_levels_and_format`, `dequant_matches_the_reader`, `texture_budget_drops_top_mips_first`; captures of a fixture model on Low, Medium, High and Ultra; bench passes on every tier with a model phase.
- Budget: at most 12 KB gzip wasm (pipeline, WGSL, KTX2 parsing, upload) for games with `models`.
- Closes part of the CORE_REQUESTS Evani.Model relief row (normal-mapped toon shading; the rest is S3-10).
- Depends on: S3-2.
- **Done 2026-10-16 (merge 8cc7504).** All six planned tests pass, plus `vertex_formats_are_what_the_test_reads` (the speck bug: scales divided by 65535 twice), `texture_uris_resolve_against_the_model_folder`, `models_wgsl_ships_without_comments_but_with_markers_and_lines` and `models_shade_the_same_with_and_without_a_shadow_map` (Chris's same-look rule: a face turned away from the sun is in its own shadow on every tier; run by `cargo xtask check --full` for every project). The sandbox draws four Frontier Marshal copies (ASTC and BC7 sets); captures on all five tiers, Lite against Ultra side by side; bench passes on every tier with the model phase. Size: about 16,367 B gz against the 12 KB budget after three shrinks (whole `models` feature +32,330 with the reader); Chris accepted it. Non-users unchanged: the three other games' ship wasm and js SHA-1 identical (commit id pinned). DECISIONS "Textured models: `models` render path". Review fix (2026-10-17, Q1): Lite and Low no longer look brighter on textured models; a baked open-sky cone per vertex casts the model's own shadow on itself where there is no shadow map, baked lazily over about a second of frames only on those tiers (`models_cast_their_own_shadow_without_a_shadow_map`, `sun_cones_bake_only_where_they_are_read`, both in `LOOK_PARITY_TESTS`). Merge a3b7e8a.

- S3-3 review fix, LOD levels (2026-10-17, S3-6 quality gate): the baked sun cone used every part of every LOD level as an occluder, so a hidden coarser level shaded the drawn one (a kart scored SSIMULACRA2 -3 on Low, dark blotches). Now a part is shaded only by the parts drawn with it: the shared parts plus its own level's; a shared part is baked against the shared parts plus the lowest level present (LOD0). Test: `hidden_lod_levels_do_not_shade_the_drawn_one` (in `LOOK_PARITY_TESTS`, since the whole `model_wgsl` file runs). Merge: see DECISIONS.

### S3-4. Model handles, LOD choice and tiered streaming: M, opus-medium
- What:
  - `ModelHandle`: returned at once; the kit draws its own placeholder (for example today's primitive kart) until the model is ready, then the model; per-tier file choice (`<id>.<tier>.glb`); LOD inside a tier file chosen by projected screen size with hysteresis.
  - Determinism: models, LODs and streaming are render-only and never change the simulation. Anything gameplay reads (anchors, sizes, collision) comes from the boot group or from kit data written at build time, so the simulation never waits on the network.
  - Asset manifest v2 (v1 still reads): per entry `tier`, `priority` and `group` (`boot` or later). The title screen waits for `boot` only.
  - The `stream` JS shim (part of `models`): background `fetch` by priority after the title screen; Cache Storage keyed by content hash for large files; `navigator.storage.persist()` only where S3-0 showed it does not prompt; re-fetch on eviction; Range requests for packed files.
- Feature: `models` (wasm side) and the shim file, added to the page only for games with `models`.
- Tests: `low_requests_only_low_files` (native, with a fake fetcher), `lod_hysteresis_does_not_flicker`, `sim_is_identical_with_and_without_streamed_models`, `evicted_cache_refetches`, `manifest_v1_still_reads`; a headless Chrome network log on Low and on Ultra (bench-runner records CDP `Network.requestWillBeSent`).
- Budget: at most 3 KB gzip wasm and 2 KB gzip JS for games with `models`.
- Depends on: S3-3.
- **Done 2026-10-17 (merge 913ac32).** All five planned tests pass, plus the netlog judge tests and `snippet_the_page_never_imports_is_an_orphan`. `cargo xtask netlog sandbox --tier low,ultra` (headless Chrome, 6000 kbit/s): Low fetched only `frontier-marshal.low.glb`, `.low.base.jpg`, `.low.normal.png` (model ready at 3.3 s); Ultra the shared glb and the BC7 set (4.3 s); the grey placeholder shows at 1.5 s, the model by 4 s. Size: wasm +8,628 B gz (budget 3 KB), js +824 B gz (budget 2 KB); decided 2026-10-17, Chris: "Accept it as it is" (about 8.4 KB gz wasm against the 3 KB plan); this replaces the earlier acceptance by EVANI AI, and no streaming trim is planned. Non-users unchanged: the three other games' ship wasm and js SHA-1 identical. DECISIONS "Model handles, LOD and tiered streaming".

### S3-5. `evani-convert`: pipeline and reports: L, opus-high
- What: a native Rust command line, dev-only (`cargo xtask convert <file> [--id <id>] [--look <look>]`), that drives pinned external tools (decision 1) and never links them:
  - Steps in glTF-Transform's `optimize` order: dedup, prune, weld, then per tier simplify (gltfpack `-si`, attribute-aware: normals, UVs and colours weighed), resize textures, encode texture sets, then quantize and meshopt-compress (gltfpack `-cc`), keeping node names and extras (`-kn`, `-ke`). Tangents by glTF-Transform `tangents` (MikkTSpace).
  - Duplicates become one mesh used by several nodes, which the engine already draws as instances; no EXT_mesh_gpu_instancing, so the reader stays small.
  - Texture size per tier from the largest on-screen size: tier reference resolution times render scale times DPR cap, at the model's closest camera distance (from `extras.evani` or `--min-distance`), rounded up to a power of two, with a floor and cap per tier.
  - Texture sets per tier: ASTC 4x4 and BC7 KTX2 (tool per decision 2). How a tier file points to them: each texture's core glTF `source` is a plain PNG (alpha or normal maps) or JPEG (opaque colour) image, which any glTF reader opens and which is also the engine's RGBA fallback; the phone and desktop KTX2 files are listed beside it in `extras.evani.textures`. So the files stay standard glTF without KHR_texture_basisu (decision 7).
  - `ktx create` reads PNG, not JPEG (S3-0): JPEG sources are decoded to PNG before encoding the texture sets. gltfpack `-kn` keeps node names and extras but drops mesh names and can move a mesh onto a new unnamed node, so tier and anchor identifiers live in node names and `extras`, never in mesh names.
  - All material maps (metallic-roughness and occlusion included) are kept by default; only look matching (S3-12) removes them.
  - The master is kept untouched (`<id>.master.glb`, never downloaded by a game). Ultra gets the master's geometry, only compressed; lower tiers get simplified LODs. Each tier file holds that tier's LOD chain.
  - Inputs: `.glb`, `.gltf`, `.blend` through headless Blender's own glTF exporter (decision 5), and Evani.Model per-tier files (already simplified, with their own normal and height maps: compressed and gated, never re-simplified).
  - Output `assets/models/<id>/`: `<id>.<tier>.glb`, `textures/<name>.<tier>.<astc|bc7>.ktx2` and `.png` or `.jpg`, manifest entries, `<id>.report.json` and `<id>.report.md`.
  - Report: bytes per tier and per file (raw, gzip, brotli), triangles, draw calls, texture memory against the tier budget, the whole package against the 10 MB playground cap, tool versions and input hashes. `meshcheck` findings listed.
- Feature: none (a native dev tool; games never link it).
- Tests: `report_lists_every_tier_and_file`, `extras_and_anchor_nodes_survive`, `converted_files_pass_the_khronos_validator` (glTF-Transform `validate`, no errors), `realistic_maps_survive_without_look_matching`, `same_input_same_bytes` (fixed tool versions), `missing_tool_names_the_fix` (points to `cargo xtask doctor --fix`), `texture_size_follows_screen_size`, `evani_model_files_are_not_resimplified`.
- Budget: nothing ships (dev tool). The 10 MB package cap is reported per game.
- Depends on: S3-2, decision 1 (and 2 for textures).
- **Done 2026-10-17 (merge 6a15487).** All eight tests pass on the pinned tools (`EVANI_REQUIRE_TOOLS=1`, no skips) and run in `cargo xtask check --full`. The four non-user games' ship builds are byte-identical before and after (wasm and JS SHA-1). Found and fixed on the way: gltfpack's `-vn 12` wrote normals 1/16 long, which the Khronos validator can't see inside meshopt, so each tier is also validated as an uncompressed twin, normals are 8 or 16 bits only, and the shared model check (`evani_guide::modelcheck`, also in `cargo xtask check`) reads every shipped model with the engine's reader. Real runs: kart_pip 29 KB package; Evani.Model character 8.3 MB (2.5 MB brotli), within the cap; Frontier Marshal 165 MB, over the cap, because plain KTX2 (no supercompression) at 2048 and 4096 costs about 17 to 34 MB per tier, and its attribute-heavy mesh simplifies little (Low 11,449 of 19,545 triangles). DECISIONS "The model converter" lists the gaps for Chris.

### S3-6. The quality gate: L, opus-high
- What:
  - `apps/model-view`: a dev-only app (engine renderer with `models`, `anim`, `capture`) that shows one model under the game's look, on a fixed clock.
  - `tools/convert-runner/run.mjs` (Node built-ins, CDP, reusing bench-runner's Chrome finder; one Chrome at a time, below-normal priority, muted): renders master and candidate from 8 fixed views per tier at that tier's reference resolution and render scale. Views: 4 around at 20 degrees up, 2 high three-quarter, 1 low front, 1 close-up on the densest detail. VR is rendered with its mono preset, as the bench does.
  - Metrics: SSIMULACRA2 per view (pinned crate in the converter only, decision 3) and our own worst-pixel ceiling after FLIP's idea (a perceptual colour and edge difference, 99.9th percentile).
  - Back-off: binary search over each tier's quality ladder (simplify ratio, texture size, position, normal and UV bits, animation error) on the worst of the 8 views; the smallest setting that passes wins.
  - Report: per tier and view the scores, the setting chosen, and a sheet image (original, converted, difference map).
  - Calibration page: 10 to 20 pairs Chris judges once, blind, to set the thresholds (decision 4).
- Feature: none in games; `apps/model-view` is a dev-only app that turns on `models`, `anim` and `capture` for itself.
- Tests: `identical_input_scores_perfect`, `damaged_model_fails` (10% of triangles kept), `search_is_monotonic_and_repeatable`, `views_are_deterministic` (two runs, same pixels).
- Budget: nothing ships.
- Depends on: S3-3, S3-5, decisions 3 and 4.
- **Done 2026-10-17 (merge 8ee0e9b).** Evidence: `cargo xtask check --full` clean (EVANI_REQUIRE_TOOLS=1); the four named tests and four more pass rendering for real; kart_pip passes every tier at ratio 1 (worst view 79.7 to 81.8 against bars 75.8 to 76.8), kart_fixture Medium to Ultra pass, Lite and Low fail on the baked sun (DECISIONS "The quality gate (S3-6)"); non-user ship builds byte-identical (merge commit); thresholds provisional until Chris's blind test; QQloop passed round 1.
- **Open finding (2026-10-20, from S3-12; handled by S7-5 on 2026-10-21, DECISIONS "The quality gate scores what each device samples (S7-5)"):** textured trials don't see texture compression. The search scores each setting with resized PNG maps and picks the cheapest passing one, often by a fraction of a point; the finished file carries ASTC/BC7 sets, and `drifted` deliberately ignores textured tiers, so a finished file can fail the bar its trial passed. Measured on the S3-12 look fixture: Medium trial 81.1, finished file 76.7 (bar 80.4, fail); Low 80.7 to 77.0 (bar 80.5, fail) with a checker reused as the normal map, 80.9 to 79.1 (bar 77.7, pass) with a real bump map. Fix idea: encode the trial's maps as the tier does, or score the trial with the device set. DECISIONS "Look matching in the converter (S3-12)".

### S3-7. Animation clip baking: M, opus-high (animation pick 2, ACL idea from pick 5)
- What, in evani-convert using `evani-gltf/anim-source`: read skins and clips; resample at a fixed 30 Hz; drop keys whose result stays within an error measured at a virtual shell distance from each joint (ACL's idea); constant tracks stored once; rotations smallest-three in 48 bits (or 10-10-10-2 in 32 when the error allows); translations and scales 16-bit normalised to the clip's range; bit rate chosen per clip by the error. Writes our compact clip block into the GLB's binary chunk, referenced from `extras.evani.clips` (format in docs/MODEL_FORMAT.md). The master keeps the standard animations. The quality gate's ladder includes the clip error.
- Feature: `evani-gltf/anim-source`, native only, used by the converter.
- Tests: `round_trip_error_within_bound`, `constant_tracks_cost_one_value`, `same_clip_same_bytes`, report lists bytes and worst shell error per clip.
- Budget: nothing ships here; the decoder is in S3-8.
- Depends on: S3-2, S3-5.
- **Done 2026-10-17 (merge ca78c99).** The `EVC1` clip block (docs/MODEL_FORMAT.md "Clips", reference reader `evani_guide::clipblock`) is baked into every tier file of an animated model; all nine clip tests pass on the pinned tools (`EVANI_REQUIRE_TOOLS=1`, no skips), the rig fixture's three clips take 550 B on Low (worst shell error 0.84 mm, bound 1 mm, 32-bit rotations) and 694 B on Ultra (0.026 mm, bound 0.2 mm, 48-bit), and the three non-user games' ship builds are byte-identical (SHA-1 table in the merge commit).

### S3-8. Animation runtime `evani-anim`: M, opus-high (animation picks 1 and 4)
- What, pure CPU, no dependency: clip decoder (about 1 to 2 KB); sampler with a cursor; time kept as integer ticks advanced in the fixed 60 Hz step (never wall-clock); nlerp; bind pose, local to model space, skinning matrices; three node types only (clip, blend of two, masked additive); tick-based crossfades; 1D blend spaces with phase sync; two-bone IK (pole vector, soften) and aim IK; attachment joints. Gameplay reads only sim-side data (state, clip ticks, events at key ticks); pose sampling, crossfade weights and IK are render-only and never write back into the simulation.
- Feature: optional dependency through `anim`.
- Tests: `same_ticks_same_pose_bits`, `crossfade_weights_sum_to_one`, `two_bone_ik_reaches_or_stretches_safely`, `aim_ik_points_at_target`, `blend_space_phase_sync`, `masked_additive_leaves_masked_joints`.
- Budget: at most 6 KB gzip wasm for games with `anim`.
- Depends on: S3-7 (format).
- **Done 2026-10-17 (merge 239819a); size over budget, accepted by Chris.** `crates/evani-anim` (evani-core only): the six planned tests pass, plus a pinned pose-bits hash, NaN-weight guards, bad-block refusals, the module parameter `anim.crossfade_ticks` and a size guard (`no_clamp_with_computed_bounds`); evani-convert's `anim_crosscheck` holds the decoder to the reference reader's sample bits and refusals at five bounds and both rotation widths. **Size: +9,300 B gz wasm** in the sandbox ship build with a probe running the whole pipeline (+19,790 before the shrink pass; the float formatter `f32::clamp` pulled in was 10.1 KB of it), against the 6 KB budget; a further cut would cost IK as its own opt-in (about 1.5 to 2 KB) or a less careful decoder (about 1 KB). Decided 2026-10-17, Chris: "Accept about 9 KB now, shrink later" (+9,300 B gz against the 6 KB plan); shrinking later is tracked in STAGES.md's Parking lot ("Models and anim size trim"), with those two cuts: optional IK (about 1.5 to 2 KB) and lighter clip checks (about 1 KB). DECISIONS.md "Animation runtime". `anim` on with nothing calling it costs 0 B raw. The four games' ship builds (none lists `anim`) are byte-identical (SHA-1 table in the merge commit).

### S3-9. GPU skinning: M, opus-medium (animation pick 3)
- What, `evani-render` feature `skinning` (turned on by `anim`): linear blend skinning in the vertex shader (about 30 lines of WGSL), one read-only storage buffer of 4x3 joint matrices for every character with a per-instance offset, joints u8x4 and weights unorm8x4, skinned shadows; a soft cap of 64 joints per rig that the converter report and `cargo xtask check` warn above.
- Feature: `evani-render/skinning`, turned on only through `anim`.
- Tests: `skinning_wgsl_validates`, `joint_offsets_per_instance`, `bind_pose_draws_like_the_static_mesh` (capture comparison), bench character phase passes on every tier.
- Budget: at most 2 KB gzip for games with `anim`.
- Depends on: S3-3, S3-8.
- **Done 2026-10-20 (branch s3-9-skinning, on m1-foundation ff4f189; merged into m1-foundation through `s3-integration` on 2026-10-20); size over budget, for Chris.** `evani-render/skinning` (turned on only by `anim`): linear blend skinning in the vertex stage from one joint storage buffer with a per-instance offset, skinned shadows, bind-pose sun cone, soft cap 64 joints with a converter and `cargo xtask check` warning. Tests pass: `skinning_wgsl_validates` (also with `relief`), `joint_offsets_per_instance`, `bind_pose_matches_the_static_mesh` (planned as `bind_pose_draws_like_the_static_mesh`; the capture half is below). Capture: bind pose against the static copy on Lite, Low, Medium, High and Ultra, mean 0, max 0, no pixel over 8 levels (`.evani-shot-s3-9/captures/`). Bench character phase (256 posed rigs): 6 of 6 tiers pass, avg 0.55 ms (Lite) to 1.89 ms (Ultra), p99 1.3 to 7.3 ms (one earlier full run failed Lite's 2D phase while other workers loaded the CPU). **Size: +6,684 B gz wasm** in the sandbox ship build (anim 290,178 to anim with skinning and one posed draw 296,862) against the 2 KB budget, about 3.3 times over, not trimmed; decided (Chris, 2026-10-20): accepted for now, and the trim is a priority next step (the open "Size trim" task in STAGES.md Stage 3; lightness first). Games without `anim` are byte-identical (SHA-1 table for 2d-platformer, match-3, kart-racer, default sandbox and default model-view in DECISIONS "GPU skinning" and the merge commit). Needs core WebGPU (vertex-stage storage buffers). Public API narrowed to `Models::draw_posed`, `model::skin`, `Part::skin`, `Prepared::bind` and `Pipelines::skin`. Follow-up: model-view's `gate` still has no `skinning`, so the quality gate still skips skinned models ("not gated: skinned model"); turning it on, drawing the bind pose and adding the clip error to the ladder is a small S3-6 change. DECISIONS.md "GPU skinning".

### S3-10. Surface relief in the toon look (the Evani.Model core request): M, opus-high
- What: finishes the CORE_REQUESTS row from the Evani.Model track: height parallax on High and Ultra (`evani-render/relief`); a micro-detail slot in the toon look that evaluates a small pattern recipe (pattern id, parameters, mask) from `extras.evani`, faded out by screen-space derivatives below about 1.5 px; the toon ramp with the mapped normal is already in S3-3. The look guard runs (`look_status`, `look_check`); baselines are proposed, never approved by the agent.
- Feature: `relief` on the look and the renderer, turned on only by games whose models carry relief.
- Tests: `relief_wgsl_validates`, `micro_detail_fades_below_threshold`, `parallax_off_on_low_and_medium`; captures against Evani.Model's own preview pictures of the same model.
- Budget: at most 3 KB gzip for games with `relief`.
- Closes: the CORE_REQUESTS Evani.Model relief row (status done, with evidence), which unblocks Evani.Model M4's in-game proof.
- Note (2026-10-17, from ChimpDuty, sent with Chris's OK; not built, not part of this item's done-when): `model.wgsl` already reads the metal-rough and occlusion maps (S3-3, kept by S3-5) but does not use them. Using them (roughness-driven specular, occlusion on the ambient term) is the first small piece of CORE_REQUESTS `smooth-lighting`, which is filed for Stage 5; Chris decides at this item's review whether to pull that piece in here, behind `relief` or a flag of its own, with the toon look unchanged. Rule (Chris, via the ChimpDuty orchestrator, 2026-10-17): this is built from scratch for Evani.Dev; outside projects (XeGTAO, Bevy, three.js) are for ideas only, no copied code, no new dependency (docs/model_research/target-chimp-duty.md 6b). Report: ..\ChimpDuty\.vibedeck\research\2026-10-17-high-end-look-for-code-built-characters.md.
- Depends on: S3-3.
- **Done 2026-10-17 (merge 0578699).** evani-render `relief` (model_relief.wgsl: height parallax 16 steps on Ultra, 8 on High, and, as a deliberate change to the plan line for Chris's same-look rule, a cheap 2-step parallax on Lite, Low and Medium; four arithmetic micro patterns knurl, ribs, dots, grain with slope ink, faded below about 1.5 px) and evani-look-toon `relief` (the `micro_detail` slot); recipe fields in docs/MODEL_FORMAT.md "Surface relief"; original 12 KB fixture `tests/fixtures/models/relief-tile.glb` in the sandbox `relief` proof. Tests `relief_wgsl_validates`, `micro_detail_fades_below_threshold` and `parallax_cheap_on_low_and_medium` (renamed from `parallax_off_on_low_and_medium`) pass; `cargo xtask check` and `--full` clean. Size +2,395 B gz wasm on 33e97ea; 2d-platformer, match-3, kart-racer and the default sandbox byte-identical (SHA-1 table in the merge commit). Five-tier captures in .vibedeck/screenshots/s3-10-relief/ (r4-final, r3-postmerge); Evani.Model has no preview renders yet, so tiers are compared against Ultra. Look guard: basic look verified against Chris's approval (seq 1) in Evani.App's session; its small drifts predate this work (same on the base); toon unguarded; no baseline proposed. Not built: the metal-rough and occlusion maps (`smooth-lighting`, Chris to decide). DECISIONS "Surface relief (S3-10)". **Follow-up (2026-10-20, branch `sun-smear-fix`):** the purple patch the five-tier sheets showed on Lite and Low was the baked sun cone, not relief; fixed in `evani-render` sun.rs (tests `big_faces_get_no_diagonal_sun_smear`, `baked_sun_draws_no_smear_on_any_model`, `bake_is_stable_under_position_quantisation`), about +2.1 KB gz wasm in games with `models` (Chris, 2026-10-20: trimming it is a priority next step); DECISIONS "The purple patch on Lite and Low".

### S3-11. Detail rules and part variants: S, sonnet-medium
- What: `docs/CONTENT.md` with the four Stage 3 detail rules (hero assets come from detailed models; kits or recipes assemble detailed parts into variants; looks provide shader micro-detail; detailed assets stream after the title by tier). API: per-instance show, hide and recolour of named nodes; `anchor(name)` frames; a variant recipe in kit JSON (parts by name, which anchor each attaches to, colours). evani-guide warnings: a hero model without per-tier files; a boot group over a set size (default 1 MB; a parameter, not a rule).
- Feature: part of `models` (API) and evani-guide (warnings); nothing for other games.
- Tests: `variant_from_parts_uses_anchors`, `hidden_part_draws_nothing`, `boot_group_size_warning`.
- Budget: inside the `models` budget.
- Depends on: S3-4.
- **Done 2026-10-17 (merge 6c95af4).** Evidence: `cargo xtask check --full` clean; the three named tests pass; the 2d-platformer, match-3 and kart-racer ship builds have unchanged SHA-1s; sandbox gzip wasm +8.7 KB (engine +5.1, variant use +3.6); Low and Ultra captures in `.vibedeck/screenshots/s3-11-variants/`; QQloop passed round 2.

### S3-12. Look matching (optional converter step): S, sonnet-medium
- What: `evani-convert --look <look>`: drops texture maps the look never samples (for toon: metallic-roughness and occlusion), merges materials that map to the same look role (fewer draw calls), and can snap base colours to the look's palette. The quality gate still decides.
- Tool and feature: tools/evani-convert, a converter option; nothing in games.
- Tests: `toon_drops_unused_maps`, `merged_materials_cut_draw_calls`, `gate_still_runs_after_matching`.
- Budget: nothing ships; it makes toon games smaller (bytes and draw calls saved are in the report).
- Depends on: S3-5, S3-6.
- **Done 2026-10-20 (branch `s3-12-look-match`; merged into m1-foundation through `s3-integration` on 2026-10-20).** `cargo xtask convert <model> --look toon [--snap-palette]` (tools/evani-convert/src/look.rs, docs/MODEL_FORMAT.md "Look matching"). With `EVANI_REQUIRE_TOOLS=1`, no skips: `toon_drops_unused_maps`, `merged_materials_cut_draw_calls` and `gate_still_runs_after_matching` pass, the last rendering for real (matched Low tier file SSIMULACRA2 79.1, worst pixel 0.169, bar 77.7 from a format floor of 82.7), with six more look tests (names, anchors and extras kept, palette snap only when asked, no table changes nothing, relief maps survive with and without a look, fixture is the converter's output). Measured savings on the fixture's Low tier (report `look_match`): draw calls 4 to 2, metal-rough (two) and occlusion maps not shipped, 67.7 KB phone and 67.5 KB desktop download and 64.1 KB texture memory (ASTC and BC7) saved. Along the way: the gate's reference now carries the tier files' tangents (it scored a normal-mapped file against a master drawn without its normal map), relief height maps survive conversion with or without a look, and the gate test converts at 1 m with a real normal map (DECISIONS "Look matching in the converter (S3-12)"; the open S3-6 finding above). `cargo xtask check --full` clean. Nothing ships.

### S3-13. Text and UI layer (text/UI picks 1 and 2)
**S3-13a. MSDF text: M, opus-medium**
- What: an xtask step runs the pinned msdf-atlas-gen (decision 1) on the fonts a game lists in `evani.toml` (`[ui] fonts = [{ file, charsets = ["latin"] }]`), MTSDF with a small pixel range, and writes the atlas (PNG, decoded by the browser) plus a compact binary table (advances, kerning pairs, metrics). Runtime in `evani-render` feature `msdf`: glyph lookup, layout (advance, kerning, word wrap, left, centre, right), quads through the existing sprite batcher with an MSDF fragment variant (median of three channels, screen-space smoothstep; outline and glow from the MTSDF alpha). The `text::draw` API grows a font field; the bitmap font stays for debug and pixel looks. Complex scripts and typed input stay in the DOM.
- Tests: `msdf_layout_wraps_and_aligns`, `kerning_is_applied`, `msdf_wgsl_validates`, `missing_glyph_draws_question_mark`, `only_listed_charsets_are_baked`.
- Budget: at most 4 KB gzip wasm for games with `ui`, plus the font files (about 25 to 60 KB per font and character set, a streamed asset).
- Depends on: S3-1, decision 9 (fonts).
- **Done 2026-10-17 (merge b1ee618).** `cargo xtask fonts` bakes the `[ui] fonts` list with the pinned msdf-atlas-gen (MTSDF, 24 px/em, range 6) into a PNG atlas and an `.evf` table; Atkinson Hyperlegible (OFL) is the engine font. All five planned tests pass, plus shader stripping and outline pinhole closing. Size: `ui` +17,066 B gz wasm over no text, +6,095 over bitmap text, accepted by Chris with the split parked; games without `ui` unchanged (SHA-1 in the merge). Captures: .vibedeck/screenshots/s3-13a-msdf/.

**S3-13b. Box layout `evani-ui`: S, sonnet-high**
- What: a row/column box tree built each frame (fit, grow and fixed sizes, padding, gap, alignment, text wrap through a measure callback, scroll offset), fixed arrays, no allocation, no dependency, output a list of rectangles and text runs that the renderer draws. Proof in the sandbox: a canvas HUD panel built with it (captures).
- Feature: the `evani-ui` crate is an optional dependency through `ui`.
- Tests: `layout_matches_golden_boxes`, `layout_does_not_allocate` (counting allocator in the test), `same_tree_same_boxes`.
- Budget: at most 3 KB gzip wasm for games with `ui`.
- Depends on: S3-13a for text runs (the boxes can be built first).
- **Done 2026-10-17 (merge 80610f9).** `crates/evani-ui`: flat 128-box arena, per-axis fit/grow/fixed passes, padding, gap, alignment, scroll with clip, wrap through the MSDF measure; the three required tests plus six rule tests pass. Proof: the sandbox `ui` build draws a Mission HUD panel (header and badge, progress bar, wrapping paragraph, scrolling list, two buttons); captures on High, Low, phone 844x390@3 and a short 700x360 view in .vibedeck/screenshots/s3-13b-ui/. Box fills use a new hard-edged `Mode::Solid` in the MSDF sprite variant. Size: +2,668 B gz wasm over the S3-13a `ui` sandbox (budget 3 KB). Games without `ui` unchanged (SHA-1 in the merge). HUD text under bloom and the tone map is noted in DECISIONS; no UI pass built.
- **UI pass after post: merged f00859a (2026-10-17).** `ui` games draw sprites on `msdf::HUD_LAYER` and above in a `ui` pass after the tone map (rule in DECISIONS.md and docs/UI_LAYOUT.md). Evidence: `hud_draws_after_the_tone_map`, `hud_layers_split_after_the_scene`; `hud_white_stays_white` (tools/hud-check/run.mjs) before 0/10, after 10/10 on Lite to Ultra at 1280x720 and 844x390@3 (fill exact everywhere, ink 255 on Medium to Ultra, 220 to 243 on Lite and Low, where render scale softens strokes: follow-up in CORE_REQUESTS.md). +212 B gz wasm on the sandbox `ui` build; 2d-platformer, match-3, kart-racer and the default sandbox byte-identical (SHA-1 table in the merge); `cargo xtask check` and `--full` clean.

**S3-13c. Match-3 plug-ins carried in from Stage 2 (Chris, 2026-10-16: "Move them to Stage 3")**
- Sprite art by name: match-3 asks the `cartoon2d` look for smooth heart and wrapped-sweet pieces by name, with a capture at 128 px cells. Engine side is done; the plug-in and its review capture move here. CORE_REQUESTS.md, custom-art row and plan item A. **Done (first half), branch s3-13c-m3art, merge hash 1c77dcb.** The `cartoon2d` look supplies the six piece bodies and the prism as smooth 128 px frames by colour name; the kit asks by name and falls back to its built-in shapes. This kit has no heart or wrapped-sweet pieces, so silhouettes are unchanged (a heart is a look change for Chris to decide). Captures in .vibedeck/screenshots/m3-art/; DECISIONS.md "Match-3 piece art by name".
- Canvas text: match-3's floating scores and combo callouts move from DOM text into the canvas with the `text` feature, with a review capture showing them. Engine side is done; the plug-in and its review capture move here. CORE_REQUESTS.md, text row and plan item B. Not built; best done with or after S3-13a.

### S3-14. Drop-to-convert in the Evani.dev tab: L, opus-high
- What:
  - Bridge (tools/evani-live-bridge): `POST /live/v1/convert` (dev token, size cap, `.glb`, zipped `.gltf`, `.blend`); runs evani-convert one at a time at below-normal priority; progress notes (`convert_progress`, `convert_done`) on `/ctl` and `/evani-dev`; results under `.evani/convert/<hash>/`, served from the bridge.
  - Page (evani-live, dev builds only): a drop zone and a "Convert a model" button (the fallback; S3-0: a file dropped on the game in the Evani.dev tab did nothing, so first check that a page drop handler receives drops there, and ask Evani.App to forward drops if it does not); `model.load` hot-loads the result into focus mode without a reload; the before/after slider is a live split view (the frame drawn twice with a scissor at the slider: original left, converted right), with a tier switch and the report panel (sizes, scores, worst view).
  - Commands `model.convert`, `model.load`, `model.compare`, `model.report`, MCP tools `model_convert`, `model_load`, `model_compare`, `model_report`, LIVE_API section 16 (additive to `evani-live/1`).
  - Evani.App: no change if drops reach the page; otherwise a request filed with the Evani.App project to forward a dropped file to the bridge.
- Feature: inside `live` (evani-live and the bridge), which `--ship` builds always drop.
- Tests: `convert_upload_needs_the_token`, `oversize_upload_is_refused`, `one_conversion_at_a_time`, `hot_load_keeps_the_frame_counter_running`; a machine run in a hidden test copy of Evani.App with a synthetic drag-and-drop (the Gate 1 method).
- Budget: nothing ships (dev only, dropped by `--ship`).
- Depends on: S3-4, S3-5, S3-6.
- **Done 2026-10-20 (branch s3-14-drop-convert; merged into m1-foundation through `s3-integration` on 2026-10-20).** Bridge: `POST /live/v1/convert` (dev token, 401 without it, 403 for a foreign Origin or a `Sec-Fetch-Site` other than same-origin/none, size cap, `.glb`, zipped `.gltf`, `.blend` opened with `--disable-autoexec`), one job at a time at below-normal priority, a 30-minute run limit that ends the converter's whole process tree, old conversions pruned (newest 8, 7 days). Page (evani-live `models`, wired into sandbox, 2d-platformer, kart-racer and match-3; model-view deliberately not, DECISIONS): drop zone, "Convert a model" button, slider, split switch, tier buttons, report panel; the split is evani-render's dev-only `model::Split` (`hooks`). Commands and MCP tools `model.convert` (a change, with a note), `model.load`, `model.compare` (`at`, `split`, `tier`), `model.report`; LIVE_API section 16. Tests: `convert_upload_needs_the_token`, `oversize_upload_is_refused`, `one_conversion_at_a_time`, `hot_load_keeps_the_frame_counter_running`, plus `run_limit_ends_the_whole_process_tree`, `convert_routes_refuse_other_sites`, `old_conversions_are_pruned`. Machine run: Chrome's own drag events in hidden, muted headless Chrome against the sandbox's bridge (not a hidden Evani.App copy): a text file refused with a sentence, a 45 KB `.glb` converted with all four tiers through the quality gate (79.7 to 81.8), split view at 0.5, 0.2 and 0.8, split off, low tier switched; captures in the worktree's `.evani-shot-s3-14/`. Whether a drop reaches the page inside the Evani.dev tab itself is still unproven (risk below); the button is the fallback. Nothing ships: `--ship` drops `live`.

### S3-15. Blender add-on "Export to Evani.dev" (only if decision 5 keeps it): S, sonnet-medium
- What: `tools/blender-addon/evani_export/`, GPL-3.0-or-later with its own LICENSE and `blender_manifest.toml`, kept apart from engine code. One button: export with Blender's own glTF exporter (extras on) to a temporary file, then run evani-convert, or post it to the running bridge so it hot-loads in the Evani.dev tab. Built with `blender --command extension build` through xtask.
- Feature: none; a separate GPL folder, never linked by the engine or games.
- Tests: a headless Blender run exports a test model and the converted files appear.
- Budget: nothing ships.
- Depends on: S3-5 (S3-14 for hot-load).
- **Done 2026-10-17 (merge 0ec21c5).** Evidence: `cargo xtask blender-addon --test` PASS in headless Blender 5.2.2 (4 tier files and the report, package 36 KB of 10240 KB), `cargo xtask check` clean. The hot-load post was inert until S3-14; since S3-14 it posts to the bridge's `/live/v1/convert` with the session's dev token (DECISIONS, Blender add-on).

### S3-16. The gate models in the kart racer: L, opus-high (plus a Blender worker for the models)
- What:
  - Models: a detailed kart built in Blender the Precious way (live Blender, part by part) and the AI-made character (decision 10), rigged, with clips: idle, a steering lean as a 1D blend space, look back, celebrate. Both converted, passing the gate on every tier.
  - Kart racer, through `fix 001` (status stays `review`): kit feature `hero-models`, on in `apps/kart-racer/evani.toml` (decision 11). The player's kart and driver come from the models, using anchors (wheels, seat, steering wheel); the driver's hands hold the wheel by two-bone IK, lean follows steering, the celebrate clip plays on the podium; primitives are the placeholder until the files stream in; rivals stay primitives (Stage 4). Free roam unchanged.
  - The geocheck adapter reads the model meshes through evani-gltf.
- Feature: `hero-models` in evani-kit-kart-racer (turns on `models` and `anim`); 2d-platformer and match-3 untouched.
- Budget: the kart racer's first download grows by the engine code only (about 30 KB gzip for `models` and `anim`); the model files stream after the title. Per-tier model bytes are set by the gate and shown in the reports; the whole package stays under 10 MB.
- Tests: kit tests (determinism with and without models), geocheck 0 untagged, bench passes Low to Ultra and reports VR, review card 0 FAIL and the critic, ship size per tier and the package under 10 MB.
- Depends on: S3-3, S3-4, S3-5, S3-6, S3-7, S3-8, S3-9 (S3-10 and S3-11 if the models carry relief or variants).

### S3-17. Gate 3 run: M, sonnet-high
- What: run every check in section 6, write `.vibedeck/screenshots/gate3/gate3-status.md`, then hand to Chris for the human checks.
- Tool and feature: xtask, bench-runner, convert-runner, a hidden test copy of Evani.App; no new feature.
- Tests: every closing test above re-run on the merged branch, plus `cargo xtask check --full`, `cargo xtask bench` on every tier and `cargo xtask sizes` for all games.
- Budget: nothing ships.
- Depends on: all of the above.

### Deferred and open Stage 3 items (2026-10-20)

Recorded when S3-9, the sun-smear fix, S3-12 and S3-14 were merged into m1-foundation (branch `s3-integration`). None of these is built; each says why.

- **Size trims done (2026-10-20, branch s3-size-trim).** Loader sorts and extras (6ac61ac), wasm-opt `--low-memory-unused` in every game (d976694): every game smaller, gzip -1.0 to -2.4 KB without models, -6 to -7 KB with models. Table, findings and three open proposals (simpler sun bake, builder paths, nightly panic strip) are in DECISIONS.md, "Stage 3 size trims, lightness first".

- **S3-13c canvas text (the second half):** needs match-3 kit game changes (scores and callouts moved from DOM text into the canvas), and Chris's decision on whether that change is worth breaking Gate 3's proof that games which don't use the new features stay unchanged (match-3 is one of the two byte-identical games). The first half (piece art by name) is done.
- **S3-16 gate models in the kart racer:** kit game changes (`fix 001`, `hero-models`), plus the two models themselves; not started. Gate 3's first two lines wait on it.
- **Model-view textures:** a change in the model-view kit, not engine work; left for a kit session.
- **Evani.Model tab items (docs/EVANI_MODEL_TAB.md):** need the Evani.App side built first; nothing more to do in the engine until then.
- **Size trim, open, a priority next step (Chris, 2026-10-20, lightness first):** model streaming (S3-4, +8,628 B gz wasm against 3 KB), Lite/Low shading, GPU skinning (S3-9, +6,684 B gz against 2 KB) and the Lite/Low sun-cone smear fix (+2,149 B gz), with the anim cuts (S3-8, +9,300 B gz: optional IK, lighter clip checks). Un-parked as an open Stage 3 tooling task (STAGES.md Stage 3, "Size trim"); each cut is its own step with the SHA-1 proof for non-users.
- **Text-module split:** done (branch `s3-text-split`, DECISIONS "Text split"): `msdf` (sharp text, the `ui` module) and `text` (pixel text) are independent render features; a `ui` game saves about 9 to 10 KB gz (STAGES.md Stage 3).
- **Texture supercompression (KTX2 Zstandard):** decided (Chris, 2026-10-20): keep postponed (DECISIONS, the texture supercompression entry, with its reopening condition).
- **Quality gate, found in the S3-12 fixture review (2026-10-20; not built):** the gate's eight views always frame the model to fill the picture (plus a closer close-up), but the converter sizes each tier's textures from the model's closest camera distance (`extras.evani.min_distance_m` or `--min-distance`). A model that honestly says it is never seen nearer than, say, 40 m gets small maps that are right for the game, and the gate then judges them from far nearer than the game ever shows: the best rung falls under FLOOR_MIN, the bar stays 90 and every rung fails (the S3-12 fixture at 40 m: best rung -26.1, tier file -57.9). `gate_still_runs_after_matching` now converts at 1 m, which is fair for what that test checks (that look matching leaves the gate working) but no longer covers this path. Fix idea: place the views no nearer than `min_distance_m` (scaled to the normalised model), or score each tier at the distance its textures were sized for; then a test with a far distance and fine textures that must pass. Filed with S3-6's open trial-vs-shipped finding; for Chris to schedule.
- **Merge blocker, found by the s3-integration full check (2026-10-20): fixed, a test-side cause.** `baked_sun_draws_no_smear_on_any_model` failed on S3-12's fixture `look-match.low.glb` under the toon look's sun (lit 92.2% with a shadow map, 99.2% baked; limit 6 points). Cause: the fixture has two LOD levels (the fine sphere and trim, and the converter's coarse copy at the same place), and the test's exact shadow cast hit every level at once, so the coarse level read as shadowed by the fine level lying over it (all of the missed shadow was at a hit distance under a twentieth of the model, level 1 blocked by level 0). No pass draws a hidden level (the instances are staged per drawn level, the shadow pass included), and the bake already keeps levels apart (`hidden_lod_levels_do_not_shade_the_drawn_one`). Fix: the test now scores each level as drawn (shared parts plus that level); the 6-point limit and the fixture are unchanged and each level must pass on its own. Now: level 0 within 3.1 points, level 1 within 1.5. No engine code changed (0 B wasm).
- **`gate_still_runs_after_matching` failing on m1-foundation (2026-10-20): fixed in the sun-cone bake; bar, skips and fixture untouched.** With `EVANI_REQUIRE_TOOLS=1` the best rung scored SSIMULACRA2 30.3 (tier file -18.0, bar stuck at 90): the gate's reference (the master re-welded, 3 parts each packing its own vertices) and the 1-part tier file baked different sun shade. Cause 1: big-triangle votes grouped by vertex index, so a point shared by two parts heard only its own part; fix: copies of a point (matched within a 64th of a grid cell) share the first copy's votes (30.3 to 61.6). Cause 2: on the symmetric sphere equal faces tie half open, half blocked, and float rounding of the area sums (which differs with each file's packing) decided; fix: votes within 0.1% of even count as open, as an exact tie always did (an own-ray tie rule passed the gate but broke `big_faces_get_no_diagonal_sun_smear`). Now floor 82.2, bar 77.2, tier file 78.7, worst pixel 0.169; all 9 `model_wgsl` tests pass; guard test `bake_does_not_depend_on_how_a_model_is_split`. Byte effect: 0 B (sandbox ship wasm 217,005 B gzip before and after). DECISIONS "Look matching in the converter (S3-12)".
- **Seen once, not reproduced (2026-10-20):** in the first `cargo xtask check --full` after that fix, `gate_still_runs_after_matching` failed with "render .work/gate/reference.glb (low): timed out after 180 s (no progress)": the gate page never started the job. The converter suite rerun the same way (`--test-threads=2`) and a second full check both passed. Suspects, unproven: GPU or CPU contention from other workers' headless Chrome, or the gate page being rebuilt (`runner::gate_dist`, which writes its stamp only after `trunk build`) while another test thread loads it. If it recurs, take a lock around `gate_dist` and log the page's console on a no-progress timeout.
- **S3-17 Gate 3 run:** stays open, and last.

### Sizes at a glance

| Item | Size | Worker | Ships (gzip, only for games that use it) |
|---|---|---|---|
| S3-0 Spikes | S | sonnet-medium | nothing |
| S3-1 Module wiring | S | sonnet-medium | 0 |
| S3-2 Reader and meshopt | M | opus-high | 8 KB or less |
| S3-3 Textured 3D path | L | opus-high | 12 KB or less |
| S3-4 Handles, LOD, streaming | M | opus-medium | 3 KB wasm, 2 KB JS |
| S3-5 Converter pipeline | L | opus-high | nothing (tool) |
| S3-6 Quality gate | L | opus-high | nothing (tool) |
| S3-7 Clip baking | M | opus-high | nothing (tool) |
| S3-8 Animation runtime | M | opus-high | 6 KB or less |
| S3-9 GPU skinning | M | opus-medium | 2 KB or less |
| S3-10 Relief in the toon look | M | opus-high | 3 KB or less |
| S3-11 Detail rules, variants | S | sonnet-medium | inside `models` |
| S3-12 Look matching | S | sonnet-medium | nothing (tool) |
| S3-13a MSDF text | M | opus-medium | 4 KB plus fonts |
| S3-13b Box layout | S | sonnet-high | 3 KB or less |
| S3-14 Drop-to-convert | L | opus-high | nothing (dev only) |
| S3-15 Blender add-on | S | sonnet-medium | nothing (separate) |
| S3-16 Gate models in the kart racer | L | opus-high | model files, streamed |
| S3-17 Gate run | M | sonnet-high | nothing |

Engine code for a game using everything: about 40 KB gzip on top of today's game, plus its model and font files.

## 6. How each Gate 3 line will be proven

| Gate line | Evidence |
|---|---|
| A detailed Blender kart and an AI-generated character convert, pass the quality gate on every tier, and load into the kart racer. | Both `<id>.report.md` / `.json` show PASS on Low, Medium, High, Ultra and VR (mono), each view's SSIMULACRA2 and worst-pixel numbers against the thresholds, with the sheet images. Kart racer review captures on every tier show the hero kart and the animated driver (hands on the wheel, lean, podium celebration). Bench passes Low to Ultra with the models; geocheck 0 untagged; `cargo xtask check --full` clean. Human check: Chris drives it. |
| Size and quality reports exist for both, and Low downloads only the LODs it uses. | The two reports (bytes per tier and per file, raw and compressed, master against converted, triangles, draw calls, texture memory, package against 10 MB). Headless Chrome network logs on Low and on Ultra: Low fetches only `.low.` model files and Low textures in its own texture format; test `low_requests_only_low_files`. |
| Dropping a model onto the Evani.dev tab converts and hot-loads it, with a before/after slider. | Machine run in a hidden test copy of Evani.App against the real bridge: a synthetic drop, the converted model on the turntable without a reload (frame counter keeps rising), slider screenshots at 0, 50 and 100%, the report panel filled. Human check: Chris drops a model in the installed app. |
| A game that doesn't use these features compiles without them. | Ship builds of 2d-platformer and match-3 with the same wasm and js SHA-1 before Stage 3 and after each item; their marker scans show no `evani.module.models`, `anim` or `ui` and none of the new shader markers; `cargo xtask sizes` columns at 0; `cargo xtask check --full` builds every game with only its listed modules; evani-guide test `glb_without_models_is_an_error`. |

## 7. Decisions for Chris

Already decided (section 3): our own reader, per-device texture sets, per-tier LODs, the five animation picks, the three converter picks, the three text/UI/streaming picks.

Decided 2026-10-15: Chris answered "Yes, all as recommended" to decisions 1 to 11 below. Each is now taken as recommended, with two conditions: decision 2 still depends on what the S3-0 spike finds, and for decision 10 the generator is named to Chris and he approves it before any money is spent. Decision 12 (Hexagon, section 8b) was answered the same day: option (b).

Decided as recommended:

1. **Install the converter's helper programs?** gltfpack and glTF-Transform (Node is already on this PC), and msdf-atlas-gen for fonts. All are free (MIT), run as separate programs, are pinned to exact versions, and are installed by `cargo xtask doctor --fix`, so any creator's AI gets them too. *Recommended: yes*, because the converter and the sharp fonts cannot work without them.
2. **Add Khronos's texture tool (KTX-Software) if the spike shows we need it?** Making the phone and desktop texture sets needs one encoder; gltfpack may not do it alone (checked in S3-0). Free (Apache-2.0), a separate program. *Recommended: yes, if S3-0 confirms it is needed*, because it is the official tool and does exactly this job. **S3-0 result (2026-10-15, docs/stage3/spikes.md): needed.** gltfpack `-tc` only writes Basis-supercompressed KTX2, which needs a transcoder in the game; KTX-Software `ktx create` writes ASTC 4x4 and RGBA directly and BC7 through `--encode uastc` then `ktx transcode --target bc7`. So decision 2 applies as approved.
3. **Add one picture-comparison library to the converter only?** SSIMULACRA2 as a pinned Rust crate, after its licence is checked; it never reaches a game. *Recommended: yes*, because a tested measure of "looks the same" is safer than writing our own.
4. **How strict is "invisible difference"?** Start at SSIMULACRA2 90 or more on the worst of the 8 views plus a worst-pixel ceiling, then you judge 10 to 20 before/after pairs once, without labels, and we set the numbers from that. *Recommended: this start and the one blind test*, because there is no published standard for game models.
5. **Keep Blender in Stage 3?** Two parts: (a) convert `.blend` files by running Blender in the background; (b) the "Export to Evani.dev" button inside Blender, licensed GPL-3.0-or-later and kept separate from the engine. You didn't tick the Blender option on the card, but Stage 3's text and gate name both. *Recommended: keep both, the button as the last small item*, because the gate's kart comes from Blender and the button is a one-day job; Blender then needs installing on this PC (it is not set up for command-line use today).
6. **Stage 3's text promises "baked normal maps"; you left baking out.** Either drop that phrase from Stage 3 (Evani.Model models bring their own normal maps; imported models keep detail through gentler simplification, which the quality gate enforces) and park baking, or add our own baker later. *Recommended: drop it and park it*, because the quality gate already stops any visible loss.
7. **Stage 3’s text promises texture files in the Basis format (KHR_texture_basisu); your per-device choice ships phone and desktop texture sets instead.** Each model file then points to a plain picture any program can open, with the phone and desktop versions listed in Evani’s own notes inside the file. *Recommended: change Stage 3’s wording to say so*, because the files stay standard and open in any program, and players download no converter.
8. **Serve every Evani game from evani.app?** Browsers share cached files only within one site, so library files are downloaded once across games only if all games live on evani.app. *Recommended: yes*, because it is what makes the second game load faster (Stage 7's gate needs it too).
9. **Fonts: allow open-licence fonts (SIL Open Font Licence) as engine assets?** The rule "all content is original" would otherwise mean drawing our own fonts. *Recommended: allow them for UI text only, with their licence file shipped*, because a good readable font is infrastructure, not game art.
10. **Where does the AI-made test character come from?** *Recommended: a generator whose terms let you own and publish the result, then a worker rigs it in Blender and makes its clips*, because rigging and clips are not something generators reliably give, and auto-rigging is Stage 8.
11. **Turn the hero kart and driver on in the published kart racer now?** *Recommended: yes for the player's kart and driver, streamed after the title with today's kart as the placeholder; rivals stay as they are until Stage 4's art pass*, because the gate needs them in the game, and the first download grows only by about 30 KB of engine code while the model files arrive after the title screen.

## 8. Fit with the Evani.Model track

- **Contract by document and fixtures, not code.** Evani.Model may not depend on engine crates, and the engine does not depend on it. `docs/MODEL_FORMAT.md` (written in S3-2 from Evani.Model's note 17) defines `extras.evani`, `anchor:<name>` nodes, the `collision` node and per-tier file names; both sides test against shared fixture files in `tests/fixtures/models/`.
- **Evani.Model's files skip simplification.** They arrive as `<model>.<tier>.glb` with their own LODs, normal and height maps; evani-convert only compresses, encodes textures, runs the gate and reports (`evani_model_files_are_not_resimplified`).
- **The relief request closes in Stage 3** (S3-3 and S3-10), which unblocks M4's in-game proof.
- **M5 and test J** need S3-2 to S3-6 (and S3-16's anchor use in the kart racer). Evani.Model's part splitter for imported meshes (its note 20) plugs into evani-convert as an optional step during M5; Stage 3 leaves room for it and does not build it.
- **No waiting either way.** M1 to M3 run in parallel with Stage 3; nothing in Stage 3 waits for Evani.Model. The gate's kart is a Blender kart; Evani.Model's own kart comparison is its gate, not Stage 3's.

## 8b. Hexagon and the cinematic goal

Chris wants Hexagon rebuilt in Evani.Dev as a cinematic first-person shooter (a Battlefield-like look), planned now and built after Stage 3. docs/HEXAGON_PLAN.md has the plan: Hexagon becomes the demo game of roadmap kit 006 (the first-person shooter kit) in Stage 5, and Ultra's top effects come in Stage 8. Stage 3 is the content base it stands on:

- **What Stage 3 will give Hexagon (once built):** the model reader with the full realistic material data kept (metallic-roughness, occlusion, normal maps; S3-2, S3-3), the converter and quality gate with per-tier LODs and phone and desktop texture sets (S3-5, S3-6), streaming after the title screen (S3-4, S3-11), skinned soldiers with blending and two-bone and aim IK for hands and weapons (S3-7 to S3-9), and sharp HUD text and layout (S3-13).
- **Stays for later:** the realistic lit look with image-based lighting, point lights, clustered forward and two shadow cascades, and bots through evani-net (Stage 5, kit 006); GPU-driven culling, four cascades, volumetric fog and temporal upscale (Stage 8). The Hexagon plan's own six questions were answered on 2026-10-15 ("Yes, all as recommended"; DECISIONS.md, "Hexagon rebuild plan").
- **The tightest point it shares with Stage 3:** the 10 MB playground cap counts every device family's copy of every texture (HEXAGON_PLAN.md section 4), so the converter report's package total against 10 MB (S3-5) matters most for Hexagon.

12. **Should Hexagon become a Stage 3 proof next to the kart racer?** Options: (a) yes, build Hexagon in Stage 3 as a second gate proof: not possible as a cinematic game yet, because its lighting, culling and bots are Stage 5 work, and it would delay Gate 3; (b) no, the kart racer stays Stage 3's proof, and one Hexagon soldier (rigged, with aim IK on a weapon) and one weapon go through the converter during Stage 3 as extra test cases, checking the realistic material data and the 10 MB texture budget early; (c) nothing Hexagon until Stage 5. *Recommended: (b)*, because it tests what Hexagon needs from Stage 3 early without slowing Stage 3, and the full build fits the Stage 5 plan. **Decided 2026-10-15: (b).** Chris: "Yes, add the soldier and weapon test".

## 9. Risks

- **Tool drift on Windows.** Confirmed by S3-0: gltfpack 1.3 is used for geometry only (its `-tc` writes Basis KTX2, which needs a transcoder), KTX-Software 4.4.2 makes the texture sets, and BC7 goes through UASTC then `ktx transcode`. Mitigation: exact pins, checksums, `doctor --fix`, the converter names the missing tool and the fix.
- **"Looks the same" is not a solved measure.** SSIMULACRA2 was tuned on photos; toon images have flat areas and outlines. Mitigation: two measures (score plus worst pixel), the blind calibration, sheet images in every report.
- **Per-device textures are bigger on disk** than Basis files (ASTC 4x4 and BC7 are 8 bits per texel before web compression). Mitigation: the converter sizes textures to on-screen need and the look-matching step drops unused maps; reports show the bytes, so Stage 7's byte budget can revisit with data.
- **Weak devices.** Chris's tablet already fails the bench at Low (Stage 2's open device line). Models add draw calls and texture memory. Mitigation: per-tier budgets enforced by the converter report and the renderer; the kart racer keeps primitives as the fallback.
- **The 10 MB playground cap counts every tier's files.** A big master could break it. Mitigation: the report shows the package total; the Ultra file is compressed and its textures capped.
- **Quality gate run time and machine load** (8 views, 5 tiers, a search per tier). Mitigation: one Chrome at a time at below-normal priority, master renders cached by hash, resource check before a batch.
- **Determinism.** Streaming finishes at random times. Mitigation: models are render-only; gameplay data comes from the boot group or kit data; a test runs the sim with and without models.
- **File drops in the Evani.dev tab.** Tried 2026-10-15 (S3-0): a dropped file did nothing in the tab. Mitigation: S3-14 first checks whether a page drop handler receives drops there, keeps the page's "Convert a model" button, and asks Evani.App to forward drops if needed.
- **Licences.** The Blender add-on is GPL and stays in its own folder with no engine code; any ported MIT, Apache or BSD code keeps its notice in `THIRD_PARTY_NOTICES.md`; fonts per decision 9.
- **Several items touch the same render files.** Mitigation: the merge order in section 5.

## 10. Out of scope (later stages)

- Kart racer art pass: rivals built from detailed parts, close-up garage or podium camera, one-click publish (Stage 4).
- Per-tier download targets and a report of where every byte went; the shared cache across games proven on evani.app (Stage 7). Note: decision 8 and the per-device texture sets lay the ground, but Stage 7's gate is measured there.
- Cluster streaming geometry, look transfer between looks, prompt-to-playable assets, auto-rigging and retargeting (Stage 8).
- Point lights, image-based lighting, cascades beyond what tiers have now (Stage 5 and 8 render paths).
- Dual-quaternion skinning, morph targets, 2D blend spaces and FABRIK: each behind its own feature when a game needs it.
- In-canvas widgets (buttons, sliders), typed text input and complex scripts: the DOM handles them.
- Draco, MSFT_lod, EXT_mesh_gpu_instancing and a lazy-loaded Basis transcoder (not chosen).
- Normal-map baking for imported models (Parking lot, if decision 6 is taken as recommended).
- Gaussian splats and photogrammetry (Stage 9).
