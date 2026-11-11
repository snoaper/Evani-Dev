# Evani.Materials: batch 1 build plan (design and work packages)

Status: design only (2026-10-24). Nothing here is built. This plan turns `CATALOGUE_PLAN.md` and `catalogue.csv` (branch `material-catalogue`, up to 2ac44e28) into an engine design and ordered work packages. Builders take one package at a time.

Scope: **batch 1 (106 materials, all 29 recipes) plus the 60 optional materials (vehicles 30, fantasy / sci-fi 30)**, 166 in all. Chris approved it on a card ("Yes, start batch 1") and added "also do the 2 optional" (2026-10-24), so batch 11 is built alongside batch 1.

Inputs read for this plan:

- `docs/materials/CATALOGUE_PLAN.md` and `catalogue.csv` (this branch).
- The research report: `C:\dev\evani-dev\.vibedeck\research\2026-10-24-material-catalogue-fur-hide-bark-ground-ice-gem-co.md` (saved 2026-10-23T22:00Z; found, so no fallback was needed). Its verdict: build fresh on the engine's own recipes and borrow only ideas.
- The engine on `m1-foundation` at 65e84978 (read through `git show`, never the working tree): `crates/evani-material-{layers,character,nature,proc}`, evani-render's `look-surface`, `material-layers`, `char-materials`, `kit-material` and `kit-surface` features, `gpu/char_material.rs`, `gpu/models_layers.rs`, `kit_surface.rs`, `model/surface.rs`, `shaders/model_lit.wgsl`, `catalog/*.json`, `assets/tiers/*.json`, `STAGES.md`, `DECISIONS.md` (the four family sections and "Join 6").
- The viewer on `material-viewer-styles` at d1273d68 (worktree `Evani.Dev-matstyles`): `apps/material-view` (panel.rs, web.rs, treat.rs, scenes, evani.toml) and `kits/evani-kit-material-view` (library.rs, data/*.json).

## 1. Stage check

- **Current stage: 3 (Real content).** Stage 3 already holds the start of the engine's shared materials library: the ChimpDuty intake line (`evani-material-proc`, "the start of the engine's shared materials library"), and the four material families were built as Chris's requests and joined to m1-foundation with his approval (DECISIONS "Join 6", 2026-10-23).
- **The catalogue belongs to Stage 3** as an opt-in extension of that library: new opt-in material code, data and a dev viewer, and nothing for a game that doesn't list it (Stage 3's gate line "A game that doesn't use these features compiles without them"). It adds no gate box.
- **It is not yet filed in `STAGES.md`.** Package P0 adds one line to Stage 3's Includes, citing Chris's decisions of 2026-10-23 (about 1,000 materials) and 2026-10-24 (start batch 1, with the 2 optional groups). No box is ticked.
- **Two parts belong to later stages and are not built here:**
  - **Fur shells.** `fur-shells` (instanced shells and fins) is filed under Stage 8 as a render path: "Proposed, not approved; Chris may move it earlier." Batch 1 builds fur as the painted coat with a sheen rim on every tier. That is the plan's light version and it's Stage 3 material work. Package PF (shells) waits for Chris (section 8).
  - **Style treatments in the engine.** The viewer's 24 styles (treat.rs) are viewer-only geometry and flat colours. Moving them into the engine so every material is drawn in every style is look-transfer work (Stage 8, "Look transfer"). In batch 1 the viewer keeps its own treatments; section 2.7 says what that means for judging.
- **Publishing the viewer** as a playground is never done by a worker. Only the owner's click on the publish card does it.

## 2. Architecture

### 2.1 One new crate, existing families untouched

There is one new opt-in crate, **`crates/evani-material-recipes`**. It holds:

- the material block format (2.2);
- the recipe stage: the WGSL for every recipe that no existing family serves;
- the 8 condition layers;
- the `material.quality.*` groups;
- behind dev-only features, the catalogue reader and the row-to-glTF writer.

Every recipe has exactly one **backend**, written in `catalog/materials/recipes.json`:

| Backend | Recipes | What a catalogue row becomes |
|---|---|---|
| `recipes` (new crate, model recipe stage) | metal, plastic, wood, bark, stone, masonry, plaster, ground, leather, fur, feather, scales, hide, keratin, organic, ceramic, gem, ice, fire, glow; plus `paint`'s flake, twill and colour-shift extras | a 32-byte block in the glTF material's extras (`extras.evani_recipe`), plus plain PBR factors as a fallback |
| `layers` (evani-material-layers) | paint (clear coat, thin film), cloth; metal's brushed anisotropy | `KHR_materials_clearcoat`, `sheen`, `anisotropy` and `iridescence` values, which the family already reads |
| `character` (evani-material-character) | skin, hair, eye | `extras.evani_kind` plus the character record extras the family already reads |
| `nature` (evani-material-nature) | water, glass, foliage, thin | the glTF extensions or `evani_kind` tag the nature pass already reads |

The decisions behind this split:

- **No existing family crate is edited for batch 1.** Their recipes are served by writing the glTF data they already read. A game that lists only `cloth` turns on `material-layers` exactly as today, so it costs 0 new bytes.
- **Partial recipes are completed in the new crate.** metal, wood, stone and ceramic exist only in part (proc's box surfaces and relief's planks and tiles draw kit meshes, not models). masonry and plaster exist only on kit meshes. All six get a model-stage version in the new crate. They are written fresh, and may follow our own relief and proc pattern maths (same project and licence). relief.wgsl and material.wgsl stay untouched.
- **Kit-mesh (box-world) use of catalogue rows is not in batch 1.** It would map rows onto proc and relief tuning (section 7, later).
- **Composition rule, like Join 6's:** one path per material. A character kind never carries a recipe block. A recipe block composes with layers (the recipe edits the inputs, then the layered stage adds its coat and sheen lobes): flake paint under a clear coat is exactly that.

### 2.2 The material block (32 bytes) and how it ships

Every row encodes to one fixed **32-byte block**: 24 bytes of material, plus 8 bytes of default condition amounts. On the GPU it is 2 × `vec4<u32>`, unpacked with `unpack4x8unorm`.

| Bytes | Field |
|---|---|
| 0 | recipe code (1-29 in the order of section 3; 0 = no recipe) |
| 1 | variant (the recipe's pattern kind, an enum per recipe) |
| 2 | extras mask (the bits of the recipe's extras this material uses) |
| 3 | condition mask (bit 0 wet, 1 mud, 2 dust, 3 snow, 4 wear, 5 oxide, 6 growth, 7 blood: the CSV `conditions` column) |
| 4-6, 7-9, 10-12 | colours A, B and C (sRGB8) |
| 13-22 | ten recipe numbers n0-n9 (u8 unorm over each number's range in `recipes.json`) |
| 23 | seed (low byte of FNV-1a 32 of the id: a fixed pattern offset per material) |
| 24-31 | the default amount of each of the 8 condition layers (u8 unorm) |

How a game gets only what it lists:

- **Material numbers travel inside the models that use them.** `cargo xtask convert --materials <map.json>` (or a glTF material that already has `extras.evani_material = "<id>"`) writes:
  - `extras.evani_material` (the id, so the Evani.Materials room can read out a world's materials, as Chris's 2026-10-22 direction asks);
  - `extras.evani_recipe` (the block as 64 hex characters; the decoder is a few lines, with no base64 code in the wasm);
  - the plain PBR factors (the average colour, roughness and metalness: what draws if the recipe stage is ever unpatched);
  - for family-backed recipes, the family's own glTF fields.
  
  So a game downloads the numbers of exactly the materials its models use, about 32 bytes each, and never the names, search words or notes.
- **Shader code only for listed recipes.** Each recipe is a cargo feature of the new crate (`recipe-<name>`), and each condition layer is one too (`cond-<layer>`). The crate assembles the recipe-stage WGSL and its dispatch `switch` from the features that are on (cfg'd `include_str!`), so a recipe nobody lists isn't in the download.
- **`evani.toml`:** `[materials] recipes = ["wood", "metal", "fur", "cloth"]` and `conditions = ["wet", "mud", "snow"]`. The game author thinks in recipes, and families follow:
  - `cloth` turns on `material-layers`;
  - `skin` turns on `char-materials`;
  - `water` turns on the nature module;
  - a recipe-backed name turns on `evani-render/material-recipes` plus `evani-material-recipes/recipe-<name>`.
  
  The viewer branch's own `[modules] materials = [families]` key stays as it is; the two don't clash.
- **Check errors** (P10b):
  - a model whose block's recipe isn't listed;
  - an unknown recipe;
  - a listed recipe whose family feature is missing;
  - a block whose recipe or conditions disagree with its catalogue row (stale: rerun convert);
  - a stale `evani_material` id (a warning).
- **Code-built meshes** (kits that build their glb in code, like the viewer) call the same row-to-glTF writer. A packed per-game table (`[materials] list`) is not needed for batch 1 (section 7).
- **Dev data:** `catalog/materials/recipes.json` (29 recipes: code, backend, colours, numbers with ranges and defaults, variants, extras with bits, quality group, light version, viewer scene) and `catalog/materials/materials.json` (all 1,054 rows: the CSV columns plus `params`, `cond_defaults` and `status` = planned, ready or reviewed). They sit in a sub-folder, so tools that read `catalog/*.json` see nothing new.
  - P1 imports the CSV once. From then on the JSON is the master and the CSV is regenerated from it for spreadsheet review.
  - The catalog had no material data before this. That is a gap, now filled; it contradicts nothing.

### 2.3 The recipe stage: how the code plugs in

The model pipelines compile `crate::shaders::FORWARD.source`. The character family already rewrites lines of its model stage by exact text at pipeline build (`gpu/char_material.rs`, falling back to the unpatched stage when a line is missing). The recipe stage uses **the same mechanism**: no WGSL file that games use is edited.

**New evani-render feature: `material-recipes = ["look-surface", "dep:evani-material-recipes"]`** (off with `relief`, like the lit pass). It adds:

- `gpu/models_recipes.rs` (new file, same pattern as `models_layers.rs`), with two material-group bindings:
  - **binding 12:** the material's 32-byte block (a uniform; zero = no recipe);
  - **binding 13:** the shared per-frame recipe uniform (64 bytes): the 8 global condition amounts, the 8 quality levels, sim time and the extras allow bits.
  
  Live params move only binding 13, so a slider never rebuilds a pipeline. `set_block(material, block)` rewrites one binding 12 for the viewer's live sliders.
- `model/recipes.rs` (new): reads `extras.evani_recipe` (hex) into the material record.
- `gpu/recipe_material.rs` (new): the patch, made once per sample count and kept for the session, like `char_material.rs`.
- Same-line cfg'd additions only in `gpu/models.rs`, `model.rs` and `lib.rs`.

**The patch** goes into the stage the model pipelines draw: `model_lit_fs`, or `model_layered_fs` when `material-layers` is on. It is applied after the character patch when `char-materials` is on, and its anchor lines are chosen to exist in all four texts (plain, layered, char-patched, both). It makes three edits:

1. **Before the discard** (uniform control flow: every screen derivative is taken here, as `char_frame` is): `let rf = mat_frame(in.world, uv, normalize(in.normal));`.
2. **After `occ` is set, before any lighting:** `let ro = mat_surface(rf, MatIn(tex, in.color.rgb, n, g, in.world, uv, rough, metal, occ));` then base, rough, metal, n and occ take `ro`'s values. These lines become `var` where needed.
   - The recipe builds its colour from `tex` and the vertex colour, never from `model_mat.base`, so the fallback PBR colour is never applied twice.
   - `mat_surface` runs the recipe, then the condition layers (Bevy's ExtendedMaterial order, ideas only).
3. **Before fog:** `c = c + mat_extra(ro, n, v, l, sun_rgb, vis);`. This is the post-light term for:
   - fur's Kajiya-Kay-style sheen rim;
   - flake sparkle;
   - gem and ice sparkle and the reflection ramp;
   - mica glints;
   - fire, glow and hologram emission.

**Fallbacks and checks for the patch:**

- If any anchor is missing, the patch returns `None`. The model draws its plain PBR fallback, and that is logged once. Never a broken pipeline.
- Recipe 0 (no block) returns its inputs unchanged.
- A test holds the anchors under every combination (section 5, P2a and P2b).

**WGSL contract** (P2a writes it; recipe packages only add functions):

- `fn rc_<name>(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut`, one per recipe, where `q` is the level of its quality group.
- `fn cond_<layer>(amount: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut`, one per layer. All layers share one noise sample, `nz`.
- A shared `core.wgsl`:
  - an integer hash (PCG-style) and value noise;
  - Voronoi F1, F2 and F2−F1 with a 3×3 cell search, written fresh from the well-known algorithm (Quilez's MIT write-up as the idea; never Blender's GPL copy);
  - a hex-offset cell helper;
  - an anti-alias fade that fades a pattern to its average once its cell is under 1.5 pixels (as relief's micro-detail does);
  - `unpack` helpers for the block.
- **Pattern space:** `uv × scale` by default. `ground`, `stone` and `masonry` use world space, by dominant-axis projection on Lite to Medium and a 2-axis blend on High and Ultra. They are static surfaces, so world space doesn't swim.
- **Deterministic:** noise comes only from integer hashes, seeded by the block's seed byte. Animation uses sim time from the fixed step, in binding 13, never wall-clock time.

### 2.4 Condition layers

- They are one shared pass inside `mat_surface`, after the recipe and before lighting. Each layer edits colour, roughness, normal and AO.
- They work on every recipe-stage material. Family-backed materials don't get them in batch 1: the families keep their own shading (section 7).
- The masks:

| Layer | What it does |
|---|---|
| wet | darkens the colour and lowers roughness; puddles only on flat, up-facing pixels, from the shared noise |
| mud | rises from the ground up: world y above `material.cond.mud.ground` (the model has no object origin in the fragment stage), with noise break-up; drier and lighter at the top |
| dust | settles in cavities (low AO) and on top faces (world normal y) |
| snow | covers up-facing pixels above an angle; frost lightens and adds sparkle at High and up |
| wear | follows convex edges (normal curvature from the derivatives taken in `mat_frame`) plus scratch lines; on paint it shows colour B (the layer under the paint) |
| oxide | sits in cavities and on edges, in colour C of a metal block (rust, verdigris, tarnish) |
| growth | grows on shaded, damp faces: low AO, facing away from the sun, not on top faces |
| blood | Voronoi splats with random radius that darken to brown with `material.cond.blood.age` |

- **Amounts:** the final amount of a layer is `max(the material's default, the global material.cond.<layer>.amount)`, and only where the material's condition mask has that layer. So rain (global wet 0.8) wets everything that lists wet, and a muddy boot keeps its own mud.
- **Per-object amounts** come from material variants (a copy of the block with other amounts, about 32 bytes). No per-instance engine change is needed in batch 1.
- **The quality limit** (`material.quality.conditions`: 1, 2, 4 or 8 layers) is applied on the CPU. When params change, the crate rewrites each material's effective condition mask to its strongest N layers, so the shader never sorts.
- **Light version:** wet, dust and snow become a colour tint only, and the rest are off.

### 2.5 Tiers and fallbacks

- **No tier file changes.** The plan's planned `material_quality` field in `assets/tiers/*.json` is dropped: tiers are compiled into every game (DECISIONS "Lean data"), so a new field would change all 8 games' bytes. Instead the crate holds the tier table in code, as `material.layers.quality` and `material.character.quality` already do. This is a recorded change from CATALOGUE_PLAN §5.2.
- **The eight groups:** `material.quality.<group>` for detail, fur, water, glass, skin, foliage, conditions and reflections. Each takes `auto`, `lite`, `low`, `medium`, `high` or `ultra`.
  - `auto` follows the tier: lite → lite, low → low, medium → medium, high → high, ultra → ultra, and vr → medium (stereo frame cost).
- **What the levels mean for recipe-stage materials:**

| Level | What it draws |
|---|---|
| lite | the same pattern as low (changed after critic round 1, Chris's rule that every device keeps the look; the light versions in section 3's last column and `recipes.json`'s `light` are history) |
| low | the pattern with anti-aliasing, no normal edits |
| medium | adds procedural normals within 25 m |
| high | adds glints, sparkle and 2-step parallax near the camera |
| ultra | parallax everywhere in view |

- **Groups that belong to a family** (skin, water, glass, foliage, and detail's share of layers) drive that family's own param by name through the param registry, so neither crate depends on the other. For example, `material.quality.skin` sets `material.character.quality`. The mirror table lives in the new crate and skips any name that isn't registered.
- **Rule:** turning a group down only swaps in the light version. It never removes the material or draws it wrong.

### 2.6 Every switch as a live parameter

| Level | Names (registered only for what's compiled in) |
|---|---|
| Engine, global | `material.recipe.enabled`; `material.quality.<group>` (8); `material.recipe.<recipe>.<extra>` (bool, the global allow for each compiled extra, default on, e.g. `material.recipe.fur.pattern`, `material.recipe.paint.flake`) |
| Engine, conditions | `material.cond.enabled`; `material.cond.<layer>.amount` (8 global amounts); the layer's look numbers `material.cond.<layer>.<name>` (e.g. `wet.puddles`, `wet.puddle_scale`, `mud.ground`, `mud.height`, `mud.color`, `dust.color`, `dust.cavity`, `snow.angle`, `snow.frost`, `wear.edge`, `wear.scratches`, `oxide.strength`, `growth.color`, `growth.scale`, `blood.age`) |
| Viewer, per material | `kit.material_view.cond.<layer>.on` and `.amount` (write the open material's block), `kit.material_view.extra.<recipe>.<extra>` (the open material's extras bit), `kit.material_view.mat.<number>` (one slider per number of the open recipe, through `set_block`) |
| Build time | `evani.toml` `[materials] recipes` and `conditions` → cargo features `recipe-<name>` and `cond-<layer>` (2.2) |

- The `material.recipe.*` prefix avoids proc's per-surface names (`material.plaster.*`, `material.ao.*`).
- Every name follows the existing `material.<family>.<name>` and `kit.<id>.<name>` rules.

### 2.7 The viewer: catalogue, left tree, search, styles

- **Data:** the viewer includes `catalog/materials/materials.json` and `recipes.json` (through the new crate's dev feature `catalog`) and parses them at start-up. That is 1,054 rows, about 260 KB raw: dev data, inside a playground that stays under 10 MB, never in a game.
- **Index** (built in Rust, natively tested):
  - lower-case, accent-folded tokens from the name, the id parts, the search words and the look note;
  - ranking: exact name, then name start, then search word, then look note; ties go by id, so the order is the same every run;
  - typo tolerance: edit distance 1 for words of 4 or more letters, 2 for 8 or more, and a prefix match on the word being typed.
- **Layout:**
  - **Top:** the 24 style chips stay as they are.
  - **Left panel:** 280 px wide on screens 900 px and wider. On phones it's a full-height drawer opened by a "Browse" button.
  - **In the left panel, from the top down:**
    - the search box;
    - filter chips: recipe (grouped hard, soft, living, liquid and light), condition layers, exists today, batch, and ready or planned;
    - the category tree: 9 categories plus the 2 optional ones, each opening to subcategories with "ready / total" counts;
    - the result list.
  - All 1,054 rows show. Only ready rows open; planned rows are greyed with "planned".
- **Swatches** in the list are CSS chips, a two-colour gradient of colours A and B with the name: cheap. The real render is only on the test shape. Rendered thumbnails per style (166 × 24) are later work. This is a change from CATALOGUE_PLAN §8's "swatches drawn in the current style", chosen for cost.
- **Opening a row:**
  - The test shape's glb material is built by the row-to-glTF writer, so the viewer uses exactly the path a game's model uses.
  - The recipe's `viewer_scene` picks the shape: the shader ball for most recipes, a flat tile for ground, masonry and plaster, a cylinder for bark, a card for fire, and the family's own scene for skin, water and the other family-backed recipes.
  - Below that: the switches of 2.6 (only the conditions the row lists, and the recipe's extras), the number sliders, the quality-group chips, and "Copy id".
- **Styles across the top:**
  - The default style draws the recipe shader.
  - The other 23 styles draw the viewer's treat.rs treatment, fed with the material's colours A and B, its roughness and metalness.
  - Recipe detail (grain, fur pattern, sparkle) doesn't show inside those styles until style treatments move into the engine (Stage 8). The review card for batch 1 must say so.

### 2.8 Engine rules, how each is kept

- **Strict build-time on/off:** one feature per recipe and per layer. evani-render's feature is cfg'd and same-line. Each engine package proves that the 8 games are byte-identical (section 4).
- **No edits to WGSL that games use:** only the runtime exact-text patch, plus new WGSL files in the new crate.
- **Deterministic:** integer hashes, a seed byte and sim time.
- **WebGPU only:** no fallback path.
- **No `unwrap` or `expect`:** the hex decode and JSON parse return errors; outside tests, clippy denies them.
- **No new dependencies planned:** `serde_json` and naga `=30.0.1` (dev) are already in the workspace. If one is ever needed, it is pinned exactly with a DECISIONS line.
- **All content original:** names and numbers are ours. The prior art is used for ideas only (2.9).

### 2.9 Prior art and licences

| Source | Licence (per the report) | Use |
|---|---|---|
| ShellFurGodot (Arnklit) | MIT | Ideas only: the painted-coat and mobile split, and the shell count per device for PF. Chris chose "Build fresh" on 2026-10-22 and the report's verdict agrees. The P6 worker reads its real shader files in a read-only temp clone (`.evani-tmp-matcat/prior/`, nothing run) before writing ours, and credits it in DECISIONS as the idea source. Copying code would need THIRD_PARTY_NOTICES; not planned. |
| Lengyel's shells and fins; NVIDIA's FurShellsAndFins | papers | ideas only (PF) |
| Bevy `ExtendedMaterial` | MIT or Apache-2.0 (unchecked) | the layering order only (2.3) |
| Inigo Quilez's Voronoi | MIT | the algorithm, written fresh. **Never open Blender's `gpu_shader_material_tex_voronoi.glsl` (GPL-3.0).** |
| GDC 2004 diamond deck; Heckel's dispersion post | unknown | ideas only (gem, ice) |
| Poly Haven and ambientCG | assets CC0, Poly Haven API code AGPL-3.0 | data-layout ideas only, no code |
| Maxon's Flakes write-up | unknown | the idea of a random tilt per cell |
| Shadertoy | often non-commercial by default | never copied |
| Unreal "Surface Evolution" | commercial | a feature checklist only |

## 3. Recipe table (the spec for `recipes.json` and the block)

Notes for reading the table:

- Codes run 1-29 in the plan's order.
- n0-n9 are the u8 numbers. Ranges and defaults are set in P1 and checked by tests.
- The extras column gives each extra's bit in the extras byte.
- "Class" is the byte budget class of section 6.
- Recipes with a family backend list what their numbers map to (P10a).

| # | Recipe | Backend | Colours A / B / C | Numbers | Variants | Extras (bit) | Group | Light version | Class |
|---|---|---|---|---|---|---|---|---|---|
| 1 | metal | recipes (+layers for brushed) | base / edge polish / oxide | rough, metal, aniso, grain scale, grain strength, edge polish, engrave depth, engrave scale | plain, brushed, hammered, cast | grain 1, engraving 2, relief 4 | detail, reflections | plain metal-rough | M |
| 2 | paint | layers (coat, film) + recipes (extras) | base / under-layer / flake | rough, coat, coat rough, flake amount, flake size, film, film shift, twill scale | solid, metallic, pearl, colour-shift, matte, carbon | clearcoat 1 (layers), flake 2, film 4, twill 8 | detail, reflections | one gloss lobe, no flakes | extras 1.5 KB |
| 3 | plastic | recipes | colour / speck | rough, translucency, speck amount, speck scale, tread scale, tread depth | plain, rubber, foam, tread | translucency 1, specks 2, tread 4 | detail | plain lit colour | S |
| 4 | wood | recipes | early grain / late grain / knot | rough, grain scale, ring warp, ring contrast, knot amount, plank width, gap depth, fleck | flat-sawn, quarter-sawn, end grain, planks | grain relief 1, plank gaps 2 | detail | flat two-tone grain | M |
| 5 | bark | recipes | bark / furrow / patch | rough, furrow depth, furrow scale, plate size, stretch, peel | furrowed, plated, smooth, peeling | parallax 1 | detail | flat furrow stripes | M |
| 6 | stone | recipes | colour 1 / colour 2 / vein | rough, vein amount, vein scale, grain scale, layering, glints, edge translucency | granular, veined, layered, ore | parallax 1, glints 2, translucency 4 | detail | flat noise | M |
| 7 | masonry | recipes (model); proc/relief on kit meshes | unit / mortar / unit tint | unit width, unit height, joint width, unevenness, rough, variation, joint depth | running bond, stack, cobble, setts, ashlar, flagstone | parallax 1, unit variation 2 | detail | flat pattern | M |
| 8 | plaster | recipes (model); proc on kit meshes | colour / crack and stain | rough, trowel amount, trowel scale, crack amount, crack scale, aggregate | smooth, stucco, concrete, limewash | cracks 1, trowel relief 2 | detail | flat colour with noise | M |
| 9 | ground | recipes | soil / stone / sand tint | rough, stone size, stone amount, ripple amount, ripple scale, sparkle, clump | soil, sand, gravel, mud, road | parallax pebbles 1, sparkle 2 | detail | flat noise | M |
| 10 | cloth | layers | colour / sheen | weave scale, sheen, sheen rough, fuzz, rough | plain, twill, knit, pile, satin | sheen 1, weave relief 2, fuzz rim 4 | detail | colour with a sheen rim (layers' cheap path) | 0 new |
| 11 | leather | recipes | colour / crease | rough, grain scale, grain depth, crease amount, nap, wear lighten, tooling | smooth, suede, rawhide, tooled | crease relief 1, nap 2 | detail | flat with soft sheen | M |
| 12 | fur | recipes | base / tip / pattern | length, density, sheen, sheen shift, pattern scale, pattern contrast, flow angle, clump, points (darker toward one end of uv v) | solid, stripes, spots, patches, rosettes, dapple | shells 1 (PF, parked), pattern 2 | fur | painted coat + sheen rim (batch 1 draws this on every tier) | L |
| 13 | feather | recipes | colour / barring / iridescent hue | feather size, barring, barb scale, iridescence, rough, edge relief | plain, barred, speckled, eyed | iridescence 1, edge relief 2 | detail | flat barred colour | M |
| 14 | scales | recipes | colour / pattern / shimmer hue | scale size, shape (round to hex), relief depth, pattern scale, pattern contrast, iridescence, rough | overlapping, hex, plates, bumps | relief 1, shimmer 2 | detail | flat pattern | M |
| 15 | hide | recipes | colour / wrinkle / scatter | wrinkle depth, wrinkle scale, wetness, scatter, rough, spots | plain, wrinkled, warty, slick | wrinkle relief 1, scatter 2 | skin | lit colour, wrinkles painted | S |
| 16 | skin | character | tone / undertone / trait | tone, undertone, pore scale, wrinkles, freckles, redness | tone, trait | the family's | skin | wrap light | 0 new |
| 17 | hair | character | colour / root | root dark, shift, highlight 1, highlight 2 | straight, curly, coarse | the family's | fur | one highlight | 0 new |
| 18 | eye | character | iris 1 / iris 2 | pupil, iris depth, wetness | human, animal | the family's | skin | painted iris + catch-light | 0 new |
| 19 | keratin | recipes | colour / ring and edge | translucency, rings, ring scale, rough, nacre | horn, bone, antler, tooth, shell, beetle | edge translucency 1, nacre 2 | detail | flat glossy colour | S |
| 20 | foliage | nature | colour 1 / colour 2 / back-light | variation, thickness | leaf, needle, grass, straw | the family's | foliage | flat, no back-light | 0 new |
| 21 | thin | nature | colour / light-through | thickness, pattern | paper, wax, petal, membrane, lampshade | the family's | foliage | flat colour | 0 new |
| 22 | organic | recipes | colour 1 / colour 2 | wetness, scatter, crumb amount, crumb scale, rough, marbling | flesh, bread, fruit, slime, mushroom | scatter 1 | detail | flat colour | S |
| 23 | ceramic | recipes | body / glaze / speck | glaze gloss, glaze depth, crackle amount, crackle scale, speck amount, drip | earthenware, stoneware, porcelain, terracotta, enamel, tile | crackle 1, glaze depth 2 | detail | one gloss lobe | M |
| 24 | glass | nature | tint | thickness, frost, waviness | window, bottle, crystal, stained | the family's | glass | flat tinted see-through | 0 new |
| 25 | gem | recipes | colour / inclusion / core glow | dispersion, sparkle, facet scale, inclusions, clarity, core glow | brilliant, step, cabochon, raw crystal, amber | sparkle 1, inner glow 2 | glass | tinted gloss with one sparkle | L |
| 26 | water | nature | colour / deep | depth fade, wave size, foam, murk | pool, river, sea, liquid | the family's | water | flat tinted with a Fresnel edge | 0 new |
| 27 | ice | recipes | colour / deep / rim | clarity, crack amount, crack scale, sparkle, scatter, frost | clear, blue, frost, icicle | inner cracks 1 (2-step parallax), sparkle 2, scatter 4 | glass | white with a blue rim | M |
| 28 | fire | recipes | core / edge / smoke | intensity, flicker speed, flicker amount, softness, scroll speed, embers | flame, embers, lava, smoke, steam | flicker 1 | detail | static glowing card | M |
| 29 | glow | recipes (model emissive) | colour / rim and scanline | intensity, pulse speed, pulse amount, scanline density, scanline speed, flicker, rim | lamp, neon, screen, rune, hologram, energy | pulse 1, hologram 2 | none | plain emissive | S |

Notes:

- **Fire** is drawn on cards (any quad model), cut out by the base texture's alpha. A light it casts is the kit's point light, not the recipe's.
- **Hologram** in batch 11 is opaque emission with a Fresnel rim, scrolling scanlines (screen y plus sim time) and flicker. Its gaps are cut out with a dither. True additive blending needs a blended model pipeline, which is not in batch 1 (section 8).
- **Carbon fibre** is `paint` with the twill extra: a 2×2 check whose highlight direction flips per cell, drawn in `mat_extra`, under the layers' clear coat when it's present.
- **Flake paint:** each hashed cell tilts the normal a random amount, with a sparkle on the half vector. One hash per pixel.
- **Tyres** are `plastic` with the tread extra (stripes plus noise). Sidewall lettering is out of scope.

## 4. Branches and the byte proof

- **Engine branch: `matcat-engine`**, from `m1-foundation` at 65e84978, in a new worktree `..\evani-dev-matcat-engine`.
  - P0 merges `material-catalogue` (docs only) into it, so the plan travels with the code.
  - Packages P0 to P12 and PF land here in order, one commit or more per package step.
  - It never touches the main folder `C:\dev\evani-dev`, which holds others' uncommitted work.
- **Viewer branch: `matcat-viewer`**, from the tip of `material-viewer-styles`, in a new worktree `..\evani-dev-matcat-viewer`.
  - Today's tip, d1273d68, is marked "(wip)". Before P13 starts, the orchestrator confirms the styles work is finished or frozen, and the branch starts from that tip.
  - It merges `matcat-engine` at the start of each viewer package (a merge, never a rebase). Expect add-beside conflicts in `crates/evani-render/Cargo.toml` (the viewer branch's own `model-replace` feature) and the workspace members. Keep both sides, as Join 6 did.
  - When the viewer branches' blocked review clears and they join m1-foundation, `matcat-viewer` follows them.
- **Byte proof (the 8 games byte-identical):** 2d-platformer, base-building-rts, kart-racer, match-3, model-view, sandbox, tactical-fps, top-down-action-adventure.
  - **Baseline:**
    - Built once, in P0, at 65e84978, with today's compiler.
    - Copy `Evani.Dev-matstyles\.evani-tmp-matstyles\ship-base.sh` to `Evani.Dev-matcat-engine\.evani-tmp-matcat\`. Set B to a temp worktree at 65e84978 and `EVANI_GIT_COMMIT` to the same value the branch builds use, keep `CARGO_BUILD_JOBS=2` and the low-priority runner, and run it in the background.
    - It writes `sha-base.txt`.
  - **Comparing:**
    - The branch build writes `sha-matcat.txt` the same way.
    - The `cmp-base.sh` copy, with its paths changed, must print `same` for every game, both wasm and js.
  - **Compiler drift:** before each proof, compare `~/.rustup/update-hashes` timestamps with `sha-base.txt`. If the toolchain changed, rebuild the baseline first (the drift from 2026-10-22).
  - **When a proof is needed:**
    - A package that touches evani-render, evani-core, any look, kit or fx crate, the workspace `Cargo.toml` dependency versions, or xtask's build path proves all 8 games.
    - A package that touches only `apps/sandbox` proves the sandbox.
    - Packages that touch only the new crate, `catalog/materials/` or docs show it with `git diff --stat` and a `Cargo.lock` diff that only adds the new package. No ship build is needed.

## 5. Work packages

Every package ends with these common checks:

- `cargo xtask check` prints `evani check: clean` (`--quick` while working, the plain check at the end);
- clippy denies all warnings, natively and on `wasm32-unknown-unknown`, for the crates it touched, with the feature sets named;
- its named tests pass;
- a DECISIONS.md entry for what it decided, plus any measured bytes;
- a commit ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`;
- no push.

Screenshots go to `.evani-shot-matcat/` and temp files to `.evani-tmp-matcat/`, each holding a `.gitignore` of `*`. Captures are headless with audio muted, through the same sandbox-proof capture route the Join 6 family proofs used.

Each package is a Next step, so STAGES.md's before and after reviews apply. Keep each package under about 60 tool steps; if one runs long, commit and hand over the rest.

### P0: Branch, filing and baseline (sonnet-high)

- **Goal:** set up the branches, file the work in STAGES.md and build the byte-proof baseline.
- **Steps and files:**
  - Make worktree `Evani.Dev-matcat-engine` (branch `matcat-engine` from 65e84978) and merge `material-catalogue`.
  - `STAGES.md`: one Stage 3 Includes line, no box.
  - `DECISIONS.md`: "Material catalogue batch 1: design", a summary of sections 2 to 4 of this plan.
  - `CATALOGUE_PLAN.md` §7: note that batch 11 is built alongside batch 1.
  - Build the A8 baseline in the background.
- **Acceptance:**
  - `cargo xtask check` is clean.
  - `sha-base.txt` lists 8 games × (wasm, js), with no `BUILD-FAILED`.
  - `rustc -V` is recorded in DECISIONS.
- **Depends on:** nothing.

### P1: Catalogue data and block format (sonnet-high)

- **Goal:** the catalogue data files and the block encoder, with the data's own checks.
- **Files:**
  - `crates/evani-material-recipes/{Cargo.toml, src/lib.rs, src/block.rs, src/catalog.rs}` (features `catalog` and dev-only `data`, no shader yet);
  - `catalog/materials/recipes.json` (section 3, with ranges and defaults);
  - `catalog/materials/materials.json` (1,054 rows from the CSV, with `params` empty and `status` = planned);
  - `tools/materials/import_csv.mjs` (import, plus an export mode back to CSV);
  - workspace members;
  - `tools/evani-guide/src/manifest.rs` (`OPTIONAL_CRATES`).
- **Acceptance:** `cargo test -p evani-material-recipes --features catalog` passes these tests:
  - the counts equal CATALOGUE_PLAN §12's tables (1,054 in all, per category, per batch, per recipe);
  - ids are unique;
  - every row's conditions are allowed by its recipe;
  - every row encodes to 32 bytes with defaults;
  - encode then decode round-trips;
  - the seed is FNV-1a;
  - a missing recipe is an error, not a panic.
  
  Clippy passes with no features and with all features, native and wasm. The `Cargo.lock` diff only adds the new package.
- **Depends on:** P0.
- **Notes (P1, done):**
  - Block, recipe table and rows are in `crates/evani-material-recipes` (`src/block.rs`, `src/catalog.rs`, `tests/catalog.rs`), `catalog/materials/{recipes,materials}.json` and `tools/materials/import_csv.mjs`. 1,054 rows and 29 recipes; the CSV round-trips byte for byte.
  - Features: `catalog` (reader, adds `serde_json` only, already in the lock file) and `data` (embeds the two files, dev only). The block module needs no feature and no dependency. The `recipe-*` and `cond-*` features arrive in P2a and P3.
  - Numbers are keyed by name in `recipes.json` (`numbers[i]` is n`i`). A row overrides the defaults in `params` (`variant`, `colours`, `numbers`, `extras`) and `cond_defaults`; an amount above 0 needs the condition in the row's list.
  - Choices the plan left open: each recipe's allowed conditions are the union of its rows' conditions today (the plan gave no other source); extras default on except `fur.shells` (parked PF); a recipe with fewer than 3 colours leaves the rest zero; recipe `scene` is empty until P13/P14; number ranges and defaults are first estimates for P4 to P12 to tune (ranges may widen, never move a code or slot).
  - The block hex form is `Block::to_hex` (lower case, 64 characters); the GPU form is `Block::to_words` (8 little-endian u32).

### P2a: Recipe-stage WGSL core and patch (opus-high)

- **Goal:** the shared WGSL and the patch functions, as text only, inside the new crate.
- **Files:**
  - `crates/evani-material-recipes/shaders/core.wgsl` (hash, value noise, Voronoi F1, F2 and F2−F1, hex cells, the AA fade, block unpack, `MatIn`, `MatFrame`, `MatOut`, `mat_frame`, `mat_surface` with an empty dispatch, `mat_extra`);
  - `src/shader.rs`:
    - `patch_lit` and `patch_layered`, each also working on char-patched text;
    - the dispatch `switch`, generated from the enabled features;
    - a comment strip like `evani_material_character::shader::strip`.
- **Acceptance:** naga (`=30.0.1`, already pinned) validates the patched forward source for every combination (sketched in the test-loop example after this list):
  - plain, layers, char, and char plus layers;
  - 1x and 4x;
  - each shadow tail the char test covers.
  
  A test checks that every anchor is found in all four texts. Recipe 0 returns its inputs unchanged (a text test that the dispatch defaults to the identity). A missing anchor returns `None`. Clippy passes native and wasm.
- **Depends on:** P1.
- **Notes (P2a, done):**
  - Files: `shaders/core.wgsl`, `src/shader.rs` (unit tests inside), `tests/wgsl.rs` (naga: plain, layers, char, char+layers × both shadow tails × skinned or not × 1x/4x × no recipe and a stand-in recipe plus layer; the shipped forward module with and without the char patch; every anchor in all four texts; the `STAGE` table equal to `recipes.json` under `--features catalog`).
  - The tests live in the new crate, with evani-render (`look-surface`, `material-layers`, `char-materials`), evani-material-character, evani-material-layers, evani-core and naga as **dev-dependencies**. Once P2b gives evani-render an optional dependency on this crate, that is a dev-dependency cycle, which cargo allows; P2b may instead move `tests/wgsl.rs` to `crates/evani-render/tests/recipe_wgsl.rs` and drop those dev-dependencies.
  - Five edits, not three: the plan's three, plus `let metal` → `var metal` and `let base` → `var base` (one edit with two alternatives, since the layered and character-patched texts already have `var base`). `let occ` becomes `var occ` inside the plan's edit 2 (the `mat_surface` line). Edits only search inside the patched function's body.
  - `MatIn` is `MatIn(base, tex, in.color.rgb, n, g, in.world, uv, rough, metal, occ)`: the extra leading `base` is what recipe 0 returns unchanged.
  - `MatOut` = base, rough, metal, n, occ, plus the post-light inputs `emit`, `sheen` (rgb, w = rim power) and `glint` (rgb, pre-masked); `mat_extra` adds emission, the rim and sun-lit glints. A recipe that needs another post-light term extends `MatOut` and `mat_extra` in core.wgsl.
  - Binding 13 layout (64 bytes): `cond_a`, `cond_b` (global amounts), `misc` (x: 8 quality levels × 4 bits in `GROUPS` order, 0 lite .. 4 ultra; y: sim time f32 bits; z: off bits, 1 recipes, 2 conditions; w: reserved), `deny` (128 extras deny bits). All bits are "off/deny" so an all-zero uniform is safe. P3's condition look numbers (`wet.puddles` and the rest) don't fit: P3 grows the uniform or adds a binding.
  - Extras: global bit = `extra_base(code) + k`, consecutive in `STAGE` order (21 rows: the 20 `recipes` backends plus paint; 45 bits). Each compiled recipe gets `const RC_X_<NAME>`; a recipe tests `rc_extra(b, RC_X_<NAME>, k)`.
  - Quality group: the recipe's first `groups` entry; glow lists none and uses `detail`.
  - How a recipe package plugs in: add the `recipe-<name>` feature, `shaders/rc_<name>.wgsl` with `fn rc_<name>(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut`, and one cfg'd row in `shader::COMPILED`. P3 does the same with `shader::LAYERS` and `cond_<layer>`; the condition pass (`rc_amount`, one shared `nz`) is already generated.
  - The patch takes `msaa` and appends `const REC_MSAA`, matching the character patch's signature, so P2b's per-sample-count cache has the same shape.

A sketch of that combination loop, assuming the P2a test reads `FORWARD.source` the way the character tests do and has one helper per combination. `naga_validate`, `char_patched` and the other helper names are placeholders the worker defines.

```rust
// Sketch only: each source text goes through the recipe patch, at 1x and 4x.
// The shadow tails are covered the way char_layers_wgsl.rs already does it.
for (name, text) in [
    ("plain", plain()),
    ("layers", layered()),
    ("char", char_patched()),
    ("char+layers", char_layered()),
] {
    for msaa in [1u32, 4] {
        let Some(src) = patch_for(&text, msaa) else {
            panic!("{name}: an anchor is missing at {msaa}x");
        };
        naga_validate(&src).unwrap_or_else(|e| panic!("{name} {msaa}x: {e}"));
    }
}
```

### P2b: Engine wiring and sandbox proof scene (opus-high)

- **Goal:** turn the recipe stage on in evani-render and give the sandbox a proof scene.
- **Files:**
  - `crates/evani-render/Cargo.toml` (feature `material-recipes`);
  - new `src/gpu/models_recipes.rs` (bindings 12 and 13, `set_block`), new `src/gpu/recipe_material.rs` (the patch per sample count) and new `src/model/recipes.rs` (the hex extras reader);
  - same-line cfg additions in `gpu/models.rs`, `model.rs` and `lib.rs`;
  - `crates/evani-render/tests/recipe_wgsl.rs`;
  - `crates/evani-material-recipes/src/params.rs` (`material.recipe.enabled` and the 8 `material.quality.*` groups, with the tier table and the family mirror);
  - `apps/sandbox`: feature `recipes-proof`, off in ship, with a row of shapes whose materials carry blocks.
- **Acceptance:**
  - `recipe_wgsl.rs` passes for every feature combination, skinned or not.
  - The sandbox proof test checks that it loads, draws, logs no errors and registers its params.
  - A material with no block draws through the unchanged lighting (a text test).
  - Clippy passes on evani-render with `material-recipes` alone, with `material-layers,char-materials,material-recipes`, and without, native and wasm.
  - **The 8-game byte proof prints `same`.**
  - The core cost is measured: two sandbox release builds, with and without `recipes-proof` (wasm gzip -9). The core is at most 7 KB gzip.
- **Depends on:** P2a.
- **Notes (P2b, done):**
  - Files: evani-render `src/model/recipes.rs` (hex reader, `patched_forward`, `stage_state`, `BlockEdits`), `src/gpu/models_recipes.rs`, `src/gpu/recipe_material.rs`, `tests/recipe_wgsl.rs` (the P2a combinations plus this build's text and the no-block text test); evani-material-recipes `src/params.rs` (feature `engine`); sandbox `src/proof_recipes.rs` (feature `recipes-proof`).
  - `set_block` is `model::recipes::BlockEdits::set_block(model, material, &Block)`, a world resource the GPU side drains each frame (the viewer, P14, uses it).
  - Byte proof: all 16 files `same`. Core cost +7,522 B gzip (7.3 KiB), about 350 B over the 7 KB line; recorded in DECISIONS, not cut further.
  - For P3: binding 13 is still 64 bytes and full (`cond_a`, `cond_b`, `misc`, `deny`). The condition look numbers need room: grow `FRAME_BYTES` (and `frame_words`, the `RecFrame` struct and the uniform buffer size together; tests check the size against `FRAME_BYTES`) or add binding 14. `frame_words` already writes zeros for the global amounts; fill words 0..8 from `material.cond.<layer>.amount`.
  - For P4 onwards: strip each `rc_<name>.wgsl` at compile time with `shader::wgsl_strip`/`wgsl_stripped`/`utf8`, as `CORE_STRIPPED` is, so comments never ship. The proof scene's shapes are the layer fixtures; their UVs and sizes suit a first look, not a review. The catalogue rows used have empty `params`, so their colours are the recipe defaults (brass is steel-grey until P11 fills its numbers).

### P3: Condition layers (opus-high)

- **Goal:** the 8 condition layers of section 2.4.
- **Files:**
  - `crates/evani-material-recipes/shaders/conditions.wgsl` (8 `cond_*` functions, features `cond-<layer>`);
  - `src/conditions.rs` (the `material.cond.*` params, max-amount maths, the top-N effective mask);
  - the sandbox proof row gains wet, mud, dust, snow, wear, oxide, growth and blood on a neutral test material.
- **Acceptance:**
  - Unit tests: the amount rule, the top-N mask at 1, 2, 4 and 8, the light version as tint only, and a layer not in the mask stays off.
  - Naga passes with all layers on and with each alone.
  - Screenshots at Low and High (one per layer, plus all 8).
  - Sandbox proof `same`.
  - All 8 layers together cost at most 5 KB gzip.
- **Depends on:** P2b.
- **Notes (P3, done):**
  - Files: `shaders/cond_<layer>.wgsl` (8, not one conditions.wgsl: each is cfg'd on its own feature), `src/conditions.rs`, `shader::LAYERS` and `ALL_LAYERS`; evani-render `gpu/models_recipes.rs` (mask rewrite), `tests/recipe_wgsl.rs` (each layer alone and all eight, frame sizes), `tests/gpu_param_borrow.rs`; sandbox `proof_recipes.rs`.
  - Binding 13 is now 128 bytes (`ka`..`kd` hold the look numbers; `kd.w` and `misc.w` are still free). Contract: `cond_<layer>(b, a, o, i, f, nz)`.
  - The GPU side now really receives the live switches (P2b's quality and extras switches never did; see DECISIONS P3).
  - For P4 onwards: the proof's front row of plain test balls is the place to show a layer on a new recipe; materials need a non-zero recipe code for layers to run. Condition masks are rewritten per material on the CPU, so a recipe never reads `b.cond_mask` for layer decisions. Sizes: core +7,635 B (about 470 B over 7 KB, decision pending on shorter WGSL names), layers +4,541 B.

### P4: Partial recipes: metal, wood, stone, ceramic (opus-high)

- **Goal:** model-stage versions of the four partial recipes.
- **Files:** `shaders/rc_{metal,wood,stone,ceramic}.wgsl`, `src/recipes/{metal,wood,stone,ceramic}.rs` (the schema rows and the `recipes.json` equality test), and proof rows.
- **Notes:**
  - metal rows with anisotropy also write `KHR_materials_anisotropy` for layers (via P10a). Without layers, the recipe draws its own cheap streak.
  - This package sets the template for P5, P7 and P9: the file layout, the AA fade use, and the light version per level.
- **Acceptance:**
  - Each recipe draws its light version at lite and its full version at high (screenshots at Low and High).
  - Naga passes each recipe alone and all four together.
  - A schema test checks the Rust rows equal `recipes.json`.
  - Sandbox proof `same`.
  - Each recipe is at most 2 KB gzip, measured once for the four together.
- **Depends on:** P3.
- **Notes (P4, done):**
  - Files: `shaders/rc_{metal,wood,stone,ceramic}.wgsl`, `shaders/rc_shared.wgsl` (helpers `rc_out`, `rc_near`, `rc_tuv`, `rc_tw`, `rc_bump`, appended only when a recipe is compiled), `src/recipes/{mod,metal,wood,stone,ceramic}.rs` (schema rows), `shader::COMPILED` and `ALL_RECIPES`, `tests/recipes.rs`; evani-render `tests/recipe_wgsl.rs` (each recipe alone, all four, all four with all eight layers); sandbox `proof_recipes.rs` (`?set=p4` grid of 23 rows, `?row=<n>` close-ups, `?q=` also sets `material.quality.detail`).
  - **How to add a recipe (P5 to P9): the step list is the doc comment of `crates/evani-material-recipes/src/recipes/mod.rs`.** In short: a `rc_<name>.wgsl` with the contract signature and a lite branch, a `recipe-<name>` feature, a cfg'd row in `COMPILED` plus a row in `ALL_RECIPES`, a schema file in `src/recipes/` added to `BUILT`, numbers for the rows the proof shows, captures at Lite, Low and High.
  - Normals without tangents: a finite difference of the recipe's own height function, tilted with the cotangent frame `rc_tuv` (uv patterns) or `rc_tw` (world patterns); every derivative stays in `mat_frame`.
  - Live switches: the engine-level tunables of a recipe are its extras (`material.recipe.<recipe>.<extra>`, registered by themselves for compiled recipes) and its quality group; the per-material numbers live in the block (`BlockEdits::set_block`; the viewer's sliders are P14).
  - Sizes: standalone stripped WGSL gzip -9: metal 820, wood 1,119, stone 999, ceramic 864, shared helpers 414. Sandbox build pair (`--no-default-features`, all layers on, 305,072 B): all four +7,093 B (1,773 B each, within 2 KB); metal alone +4,941 B, of which about 2.5 KB is one-off recipe plumbing (the extras switches, their names and the dispatch, which the optimiser drops while no recipe is listed). That plumbing belongs with the core for P15's re-weigh.
  - The proof balls (layer fixtures) carry no texture coordinates of their own, so uv-scaled patterns (planks, tiles, engraving) look coarse on them; judge those on a model with real UVs (P14's shapes).
  - Catalogue gaps for P11: the plank deck's nail heads and the chequered marble floor have no recipe feature (rows left `planned`); corrugation (tin roof) and basalt's columns are geometry.

### P5: Ground and walls: masonry, plaster, ground, bark (sonnet-high)

- **Goal:** the four recipes, following the P4 template.
- **Notes:**
  - masonry and plaster on models; their patterns may follow our own relief and proc maths, in new files.
  - ground is Voronoi stones over noise soil, with sand ripple and sparkle.
  - bark is F2−F1 furrows plus plates and a vertical stretch.
  - World-space projection per 2.3.
- **Acceptance:** as P4. The checks also include no visible seams on a cube at High.
- **Depends on:** P4.
- **Notes (P5, done):**
  - Files: `shaders/rc_{bark,masonry,plaster,ground}.wgsl`, `src/recipes/{bark,masonry,plaster,ground}.rs`, `shader::COMPILED` and `ALL_RECIPES`, `tests/recipes.rs`; sandbox `proof_recipes.rs` (`?set=p5`, `?set=p5&shape=cube`, `?row=<n>` draws one row alone). Details and the rows left planned: DECISIONS P5.
  - Masonry, plaster and ground are world-space; bark is uv-space (u round, v along, furrows along v, u mirrored so a trunk seam shows a mirror line, not a cut). Bark on a real tree wants periodic noise in the core, filed for later.
  - `Schema.colours` is now a slice (plaster has two colours); P6 to P9 recipes with fewer than three colours need nothing more.
  - Sizes: standalone gzip bark 1,137, masonry 1,238, plaster 1,072, ground 1,331 B; the four add +2,980 B to the sandbox build (one stream). Byte proof: 16 of 16 `same`.
  - **Update (P6):** bark is now periodic in u (a recipe-only fix: circle-sampled 3D value noise and wrapping cells), so a trunk seam shows no mirror line and the core needs no periodic noise.
  - For P6: the proof's `?row=` camera and `grid_pos`/`grid_row_camera` take any rows list, so a P6 grid is a constant array plus a row list; `Mesh` in `proof_recipes.rs` builds shapes (the trunk suits fur, leather and keratin close-ups too).

### P6: Soft and living: fur, hide, leather, keratin (opus-high)

- **Goal:** the four recipes, with fur drawn painted.
- **Notes:**
  - **Fur:** the painted coat from base to tip colour along the flow angle, the pattern (stripes, spots, patches, rosettes, dapple, points) from noise and Voronoi, and a Kajiya-Kay-style sheen rim in `mat_extra`. No shells.
  - Read ShellFurGodot's shader files first (2.9). Ideas only.
  - **Hide:** wrinkle normals only, a wet sheen and a wrap scatter.
  - **Leather:** F2−F1 creases, grain, and suede nap.
  - **Keratin:** growth rings, edge translucency wrap and a nacre hue shift.
- **Notes (P6, done):**
  - Files: `shaders/rc_{leather,fur,hide,keratin}.wgsl`, `src/recipes/{leather,fur,hide,keratin}.rs`, `shader::COMPILED` and `ALL_RECIPES`, `tests/recipes.rs` (adds the fur one-pass text test), `recipe_wgsl.rs` (adds the P6 sets); sandbox `?set=p6`, `?yaw=`. Details, the 30 rows with numbers, the one batch-1 row left planned and the two fixes (pottery glazes, bark seam): DECISIONS P6.
  - Sizes: standalone gzip fur 1,040, leather 905, hide 760, keratin 858 B; the four add +2,021 B to the sandbox build (fur alone +712). Byte proof: 16 of 16 `same`.
  - Hair strands, locks and patterns are uv space and not periodic; a hidden seam is the model's uv layout job. For P7: scales and feather can take the same routes (pattern space from a direction, `o.sheen` for a rim; `rc_tuv` gives a barb or hair direction).
- **Acceptance:**
  - As P4.
  - Fur alone is at most 2.5 KB gzip.
  - At Lite the fur costs one pass, with no extra draws: a text test that no new pipeline or draw is added.
  - Screenshots of the 12 batch-1 fur rows' recipes on the shader ball.
- **Depends on:** P5.

### P7: Scales and feather (sonnet-high)

- **Goal:** the two recipes, following the P4 template.
- **Notes:**
  - **Scales:** F1 cells with alternate rows offset for hex, round-to-hex shape, relief, and fish shimmer (a hue by view angle).
  - **Feather:** barb stripes, barring, and an iridescent hue by view angle.
- **Acceptance:** as P4.
- **Depends on:** P6.
- **Notes (P7, done):**
  - Files: `shaders/rc_{feather,scales}.wgsl`, `src/recipes/{feather,scales}.rs`, `shader::COMPILED` and `ALL_RECIPES`, `tests/recipes.rs`, `recipe_wgsl.rs` (adds the P7 sets); sandbox `?set=p7` (snakes on a trunk, the rest on a uv sphere; `&shape=cube`, `?row=<n>`). Details, the 33 rows with numbers and the rows left planned: DECISIONS P7.
  - Both recipes are uv space on the core's `rc_hex` lattice with shingling (the row above lies over this one), so a recipe needs no core change; the pattern kind of scales follows `pattern_scale` (diamonds, blotches, bands, speckles, cross-faded) because the schema has one pattern colour and no pattern slot. Cell size is rounded to a whole number round u, so a ball or trunk shows no seam.
  - Sizes: standalone gzip feather 1,313, scales 1,373 B; the two add +1,879 B to the sandbox build (feather alone +1,053, scales alone +1,088). Byte proof: 16 of 16 `same`.
  - For P8: gem and ice are view-angle and Voronoi recipes; `o.sheen` (rgb, w = rim power) and `o.glint` (rgb, a view-angle, one-scale-at-a-time sparkle; see `rc_scales`) are the patterns to copy. The proof's `?set=`, `?row=` and shape routing take a new grid as one constant array and a rows list (add the set to `rows_of`, `set_from_url` and the shape choice in `draw`). The proof camera frames the balls small: capture a row with `--scale 2 --clip` in `shoot.mjs` to judge detail, one capture per command (several `--scale 3` captures in one command hang headless Chrome).

### P8: Gem and ice (opus-high)

- **Goal:** the two cheap see-through fakes.
- **Notes:**
  - **Gem:**
    - faceted normals from a Voronoi split of the view-space normal (`facet scale`);
    - one environment read;
    - a rainbow ramp by view angle (dispersion);
    - a Fresnel blend, a sparkle and an emissive core (crystals).
    - No real refraction (the nature pass's glass is the real see-through).
  - **Ice:**
    - a blue-white ramp by view angle;
    - inner Voronoi cracks with 2-step parallax at High and up;
    - sparkle noise, a rim and a wrap scatter.
- **Acceptance:**
  - As P4.
  - The gem is at most 2.5 KB gzip and the ice at most 2 KB.
  - Screenshots in the Studio and Night-lamp lights (the viewer's presets are not on this branch, so the proof scene's sun at two angles).
- **Depends on:** P7.
- **Notes (P8, done):**
  - Files: `shaders/rc_{gem,ice}.wgsl`, `src/recipes/{gem,ice}.rs`, `shader::COMPILED` and `ALL_RECIPES`, `tests/recipes.rs`, `recipe_wgsl.rs` (adds the P8 sets); sandbox `?set=p8` (pointed crystals and the icicle on a cone, the ice cube on a cube, the rest on a uv sphere; `&shape=cube`, `?row=<n>`; `?q=` now also sets `material.quality.glass`). Details, the 21 rows with numbers and the five rows left planned: DECISIONS P8.
  - Gem has no uv need: facets are a cellular noise over the normal. Ice is world space (`rc_proj`, two planes from high) with a second crack layer at 2-step parallax. Neither refracts. Both read `q` of the glass group (lite: tinted gloss with one sparkle, white with a blue rim).
  - Sizes: standalone gzip gem 1,042 B (budget 2.5 KB), ice 1,102 B (budget 2 KB); the two add +1,293 B to the sandbox build (gem alone +576, ice alone +690). Byte proof: 16 of 16 `same`.
  - For P9: a recipe follows the level `q` of its own quality group (gem and ice: glass; most others: detail). The proof grids take a constant array and a rows list; `?q=` sets the groups named in `draw` (add a recipe's group if it is not one of conditions, detail, fur, skin or glass).

### P9: Small recipes and the vehicle and fantasy extras (sonnet-high)

- **Goal:** the remaining small recipes, plus paint's three extras.
- **Notes:**
  - **plastic:** translucency wrap, specks and tyre tread.
  - **organic:** wetness, scatter, crumb and marbling.
  - **fire:** a colour ramp from scrolling noise, flicker from sim time, a soft edge and embers. The light version is a static card.
  - **glow:** emission from colour A, pulse, and the hologram variant (rim, scanlines, flicker, dither gaps).
  - **paint extras** (`recipe-paint`): metal flake, colour-shift ramp and carbon twill.
- **Acceptance:**
  - As P4.
  - Two runs of the same 120 sim frames give the same hash of the fire and hologram output (no wall clock), tested on the CPU mirror of the time inputs.
  - Paint extras are at most 1.5 KB gzip; plastic, organic and glow at most 1.2 KB each; fire at most 2 KB.
- **Depends on:** P8.
- **Notes (P9, done):**
  - Files: `shaders/rc_{paint,plastic,organic,fire,glow}.wgsl`, `src/recipes/{paint,plastic,organic,fire,glow}.rs`, `shader::COMPILED` and `ALL_RECIPES`, `tests/recipes.rs` (paint's backend is `layers` with `also: recipes`), `tests/time.rs` (the no-wall-clock hash test), `recipe_wgsl.rs` (adds the P9 sets); sandbox `?set=p9` (51 shapes in six rows, a new flat card, `&shape=cube`, `?row=0` to `5`). Details, the 52 rows with numbers and the rows left planned: DECISIONS P9.
  - **Snow fix first:** ice rows with frost above 0.3 now get close-up grain, soft clumps and light-following glints from medium and high (lite and low unchanged); before and after in `.evani/shots/p9/snow-before-*` and `snow-after-*`.
  - Sizes: standalone gzip paint 988 B (budget 1.5 KB), plastic 867, organic 715, glow 762 (1.2 KB each), fire 979 (2 KB); ice 1,377 (2 KB). All 21 recipes plus the shared helpers: 13,656 B gzip in one stream. The five add +2,733 B to the sandbox build. Byte proof: 16 of 16 `same`.
  - **Blend gap:** the model pass has no blended or additive mode and the alpha-mask discard runs before the recipe, so fire's soft edge and the hologram's alpha reach `o.base.a` and are unused. Flames are opaque cards. See P15.
  - **Batch-1 tally (106 rows, after P9):** 64 are drawn (`ready`, real numbers), 42 are `planned`. Of the 42, 40 are not recipe-stage work: they belong to the other families, whose numbers and glTF adapters come in P10a and P11 (character: 6 skin, 4 hair, 1 eye = 11; layers: 9 cloth; nature: 5 thin, 8 foliage and grass, 3 glass, 4 water = 20). The last 2 need a recipe feature: deer.coat.brown (a white belly and tail: the body-region mask) and wood.plank.deck (nail heads). Across the whole catalogue 175 of 1,054 rows have numbers. The rows with a mask or decal look (stripes, regions, two materials) are listed in DECISIONS P9 and under P15.

### P10a: Row to glTF, and family adapters (sonnet-high)

- **Goal:** turn any catalogue row into the glTF material that games and the viewer both use.
- **Files:** `crates/evani-material-recipes/src/gltf.rs` (feature `gltf`).
- **What it does:** `gltf_material(row) -> serde_json::Value` writes:
  - the PBR factors;
  - `extras.evani_material`;
  - `extras.evani_recipe` for recipe-backed rows;
  - for family rows: the KHR fields for layers (paint, cloth, metal anisotropy), `evani_kind` and character extras (skin, hair, eye), and the nature extensions and tag (water, glass, foliage, thin).
  
  The field mappings come from each family's own parser: read `evani-material-layers/src/parse.rs`, `evani-material-character/src/record.rs` and `evani-material-nature/src/gltf.rs`.
- **Acceptance:**
  - Each family's own parser, run in a test, reads the written JSON back to the intended record: layers `parse`, character `record::apply` and nature `gltf`. (They are dev-dependencies of this crate's tests only.)
  - A recipe row's block round-trips through hex.
  - Clippy passes.
- **Depends on:** P1 and P2a. It can run beside P3 to P9 on a sub-branch merged after P9.

- **Notes (P10a, done):**
  - Files: evani-material-recipes `src/gltf.rs` (feature `gltf` = `catalog`; dev and tools only), `tests/gltf.rs`; `shaders/core.wgsl` (`mat_extra` takes the lit colour and the geometric normal, then calls the generated `rc_post`), `src/shader.rs` (`post_wgsl`; patch edit 5 is now `c = mat_extra(c, ro, n, g, v, l, sun_rgb, vis);`), `shaders/rc_paint.wgsl` (`rc_paint_coat`, the lite fold, the colour-shift sweep), `src/params.rs` (`material.recipe.paint.coat_strength`); evani-render `tests/recipe_wgsl.rs` (the coat on every text); sandbox `src/proof_p10a.rs`, feature `recipes-family-proof`, `?set=p10a`.
  - **How a model reaches its block:** by name (`extras.evani_material` = a catalogue id; `gltf::apply_catalog` fills the material, P10b's convert calls it) or by value (`gltf_material(cat, row)` then `gltf::apply`, for code-built meshes). Family numbers are the row's own block values decoded over their ranges, so block and family data cannot disagree.
  - **Change from section 2.1: paint's clear coat is the recipe stage's, not the layers family's.** The paint recipe draws the coat from the block (`coat`, `coat_rough`) after lighting: a GGX sun lobe and a sky reflection on the geometric normal, taking their Fresnel share from what lies under them. Live: `material.recipe.paint.clearcoat` (on/off, existing) and `material.recipe.paint.coat_strength` (0-2, frame word 11). Tiers: lite folds the coat into the base roughness (the recipe's "one gloss lobe"), the sky reflection follows `material.quality.reflections` (none at lite). So a game listing paint needs no layers family (about 7.2 KB less), and a live block edit moves the coat. The adapter writes no `KHR_materials_clearcoat` for paint. **For P10b:** `paint` maps to `recipe-paint` only (not `material-layers`), and convert must not turn an authored `KHR_materials_clearcoat` on a paint material into `extras.evani_layers` (that would draw a second coat in a layered game).
  - **P10b also needs:** `[materials] recipes` naming a family recipe turns on the family (cloth → `material-layers`, skin/hair/eye → `char-materials`, water/glass/foliage/thin → the nature module); metal with anisotropy writes layers data, which draws only when `material-layers` is on (else the recipe's own streak).
  - Batch-1 tally after P10a and sizes: DECISIONS P10a.

### P10b: `evani.toml [materials]`, check and convert (opus-high)

- **Goal:** the build-tool side of 2.2.
- **Files:** xtask's evani.toml reader (`[materials] recipes`, `conditions` → features), the check rules of 2.2, `cargo xtask convert --materials <map.json>`, and the AGENTS.md engine block via `cargo xtask init-ai` (one bullet on `[materials]`).
- **Acceptance:**
  - xtask tests cover:
    - each new check error with its `fix:` line;
    - a model converted with a map carries `evani_material` and `evani_recipe`;
    - a game listing `cloth` gets `material-layers` and nothing from the new crate.
  - `cargo xtask check` is clean on the whole workspace.
  - **The 8-game byte proof prints `same`.**
- **Depends on:** P10a.
- **Notes (P10b, done):** `[materials]` reader and check rules (`tools/evani-guide/src/materials.rs`, `check.rs`), `cargo xtask convert --materials <map.json>` (`tools/evani-convert/src/materials.rs`), README and engine block, proof `?set=p10b`. Batch-1 tally unchanged (78 drawn, 28 planned). Details, sizes and the 16-file byte proof: DECISIONS P10b.

### P11a and P11b: Batch-1 numbers (sonnet-high, two sessions)

- **Goal:** set the numbers for the 106 batch-1 rows, 53 per session (P11a: animals, people, clothing, weapons and tools; P11b: the rest).
- **What to write:** `params`, `cond_defaults` and the variant in `materials.json`, then set `status: ready`. The numbers are original, chosen from each row's look note, and checked on the sandbox proof row (a few per recipe).
- **Acceptance:**
  - A test checks that every ready row encodes, every number is inside its range, and every recipe in batch 1 has at least one ready row.
  - A contact sheet of the session's rows on the proof shapes at High.
  - The CSV is re-exported.
- **Depends on:** P9 and P10a.
- **Notes (P11a and P11b, done together in one session):** 95 of 106 drawn, 11 planned with their reasons; the horse coat gets a dapple; character spots and an eye ring, the nature quality link (`material.nature.quality`) and `gltf::nature_params` were added. Everything, the proofs (`?set=p11a`, `?set=p11b&row=<id>`) and the pictures: DECISIONS "P11a and P11b".

### P12: The optional 60 (sonnet-high)

- **Goal:** the same work as P11 for the 30 vehicle and 30 fantasy / sci-fi rows: flake, pearl, colour-shift and matte paints, carbon, chrome, tyres, lenses, crystals, runes, hologram, force field, hull, chitin and the rest.
- **Acceptance:** as P11.
- **Depends on:** P11b.
- **Notes (P12, done on branch `matcat-p12`):**
  - **Tally of the optional 60 (batch 11): 34 drawn (`ready`, real numbers), 26 `planned`.** 20 were drawn already in P5 to P10a (7 car paints, tread, carbon, 8 gems, rune, hologram, energy core); P12 added 14: car.alloy.wheel, car.steel.wheel, car.engine.block, car.iron.wheel_rim, car.dashboard, car.wood.wagon, car.canvas.cover (the layers family's cloth), car.leather.seat, gem.turquoise, gem.pearl, magic.cursed (opaque smoke), magic.holy, magic.portal (the energy variant), sf.chitin. By group: vehicles 17 of 30 drawn, fantasy 14 of 20 (gems 10 of 12, magic 4 of 8), sci-fi 3 of 10.
  - **Left `planned`, with the reason (26):**
    - *Stripes, lining, bands and lettering (a region mask or a decal; P15 decides):* car.paint.racing_stripe, car.paint.carriage, car.paint.train, car.exhaust (blue and gold heat bands: tried as blued steel with gold edges, drawn as plain blue steel, so the bands are missing), car.boiler (black iron with brass bands), car.rail (rusty sides, polished top), car.tyre.sidewall (raised lettering), car.seat.vinyl and car.sail (seams), sf.hull.panel and sf.rust_bucket (panels and seams), sf.biomech (flesh fused with metal: two materials), sf.circuit (copper traces).
    - *A mirror with nothing to reflect:* car.chrome.bumper and sf.liquid_metal. Tried at roughness 0.02 with the base colour from bright to mid grey: the sandbox scene has only a flat sky and ground to mirror, so both read as a smooth grey satin ball with one highlight, not chrome. They need an environment reflection (a probe or sky map: a pass feature, not numbers).
    - *Needs a surface feature the recipes lack:* gem.opal (flashing colour patches) and gem.magic_crystal (a pulse on a gem; P8's list), magic.ethereal (see-through: the blend pass, P15), magic.forcefield (a flash where it is hit: an impact input), sf.alien_skin (pulsing patterns: hide has no time term).
    - *Other families' rows, not recipe-stage work (numbers wait for the nature proof, like batch 1's 20 nature rows in P11b):* glass: car.windscreen, car.headlight, car.tail_light, sf.solar; water: magic.potion; thin: magic.slime.
  - **Judged on the proof (headless, muted, `.evani/shots/p12/`, High, Low and Lite):** car paints read as car paint (a tight sun highlight and sky reflection under the coat; the pearl and colour-shift flip hue across the ball; metallic silver's flakes are faint on a ball at this size); the steel wheel is satin black, the carbon a woven cube, the alloy wheel polished aluminium, the engine block grimy grey cast metal with dust, the iron rim dark with worn edges, the wagon red painted planks with gaps, the canvas cover a cream pillow of weave, the tread a zig-zag column, the leather seat brown with a coarse crease net (tufting is geometry); the dashboard is flat dark plastic (the grain does not show at this size). Gems: ruby, sapphire, emerald and diamond show facet patches, amethyst and quartz pointed crystals, amber honey with bubbles, jade a waxy green ball, turquoise sky blue with dark veins, pearl a soft white with a pink-green sheen. Magic: the rune is a glowing blue cell net, the portal a swirling purple (a flat card, not a disc), the cursed smoke black with violet swirls (after tuning the edge colour up), holy a warm gold orb that blooms. Sci-fi: the hologram has scanlines and dither gaps, the energy core is a bright cyan orb, the chitin dark with a rainbow rim (more bubble than armour plate, as plates are geometry). At Low the same without sparkle; at Lite flat colours with one highlight.
  - **Proof:** `?set=p12` (feature `recipes-family-proof`, `proof_p12.rs`): the 34 drawn rows in five rows (vehicle paints 9, vehicle parts 8, gems and stones 10, magic 4, sci-fi 3), all through the row-to-glTF adapter; a test holds the set equal to the ready batch-11 rows. `tests/catalog.rs` gained `the_optional_sixty_are_drawn_or_planned_for_a_reason` (60 rows, 30 vehicles, the 26 planned ids by name, the 34 ready ones encode) and its encode test now reads each row's `cond_defaults` (engine block: dust 0.35, oxide 0.1; iron rim: wear 0.4; wagon: wear 0.3).

- **Drawn-versus-planned tally after the join of `matcat-p12` and `matcat-viewer` into `matcat-engine` (2026-10-24, counted from `catalog/materials/materials.json`, 1,054 rows, each id once):**

  | Group | Drawn (`ready`) | `planned` | Rows |
  |---|---|---|---|
  | Batch 1 (after P11a and P11b: was 78 / 28) | 95 | 11 | 106 |
  | Optional 60 (batch 11; P11b drew two more: was 34 / 26) | 36 | 24 | 60 |
  | **Batch 1 plus the optional 60** | **131** | **35** | **166** |
  | Whole catalogue (batches 1 to 11) | 222 | 832 | 1,054 |

  The other 888 rows (batches 2 to 10) hold the remaining 91 drawn rows (drawn early, as proofs of a recipe in P2 to P10a) and 797 `planned` ones that wait for their own batches.

- **Tally after batch 2 joined batch 1's line (`b2-main` into `matcat-engine`, 2026-10-25, counted from `catalog/materials/materials.json`; "draws" is `ready` plus `alias`, an alias opens its base row with its condition on):**

  | Group | Draws | `planned` | Rows |
  |---|---|---|---|
  | Batch 1 (P15 end: 95 / 11; roof.thatch now draws on batch 2's foliage `straw`) | 96 | 10 | 106 |
  | Batch 2 (97 `ready` + 1 `alias`, black ice) | 98 | 0 | 98 |
  | **Batch 1 plus batch 2** | **194** | **10** | **204** |
  | Optional 60 (batch 11; unchanged) | 36 | 24 | 60 |
  | Whole catalogue (batches 1 to 11) | 313 (312 + 1 alias) | 741 | 1,054 |

  Batch 1's ten planned rows are the P15 list less roof.thatch; each keeps the later batch P15 gave it. Batch 2's foliage numbers replaced batch 1's two-number nature values on the six rows both lines had set (roof.thatch, ground.grass.lawn, leaf.oak, leaf.maple.autumn, needle.pine, crop.wheat.ripe); ground.grass.meadow and crop.hay keep batch 1's values (DECISIONS "Join: batch 2 into matcat-engine").

### P13: Viewer: catalogue, left tree and search (opus-high, viewer branch)

- **Goal:** load the catalogue into the viewer, with the left category tree, search and filters.
- **Files:**
  - `kits/evani-kit-material-view/src/catalogue.rs` (load, tree, index, filters);
  - `apps/material-view/src/panel.rs` (tree, search and filter markup);
  - `web.rs` (events and the phone drawer);
  - `index.html` (the left column and the drawer CSS);
  - the viewer's `Cargo.toml` (the `evani-material-recipes/catalog` feature).
- **Acceptance:**
  - Native tests:
    - "penny" finds copper first; "gator" finds the alligator row; "grnite" finds granite;
    - the tree counts equal CATALOGUE_PLAN §2;
    - the ranking order is stable;
    - filters combine with search;
    - planned rows can't be opened.
  - Screenshots at desktop 1280×800 and phone 412×915: the tree, a search and the drawer.
  - Only viewer files change (`git diff --stat`).
  - Clippy passes native and wasm.
  - The existing viewer tests (44) still pass.
- **Depends on:** P1 only. It can start as soon as P1 is done and the styles tip is frozen.

### P14: Viewer: open a material, switches, styles (opus-high, viewer branch)

- **Goal:** opening a ready row draws it on the right test shape with every switch of 2.6.
- **What it does:**
  - Opening a ready row builds the shape's glb material with `gltf_material` (merge `matcat-engine` first) and picks the scene by `viewer_scene`. New shapes: a flat tile, a cylinder and a card in `shapes.rs`.
  - The switches of 2.6:
    - the condition switches the row lists;
    - the extras;
    - the number sliders through `set_block`;
    - the `material.quality.*` chips;
    - Copy id.
  - The style chips feed treat.rs with colours A and B, roughness and metalness.
  - The viewer's `evani.toml` lists every recipe and condition.
- **Acceptance:**
  - Native tests: a fur row opens on the shader ball with only its listed conditions; a slider changes that material's block without a pipeline rebuild (counted); a family row (velvet) opens in its family scene.
  - Screenshots: 5 recipes × (default style plus 3 other styles), and the switches before and after.
  - Only viewer files change.
- **Depends on:** P13, P11b and P12.

### P15: Final check, screenshots and review (sonnet-high)

- **Goal:** prove the whole batch and prepare it for Chris's review.
- **Steps:**
  - Both branches: `cargo xtask check --full`.
  - Clippy native and wasm for the new crate (no features, all features, each recipe alone), evani-render's combinations, the sandbox and the viewer.
  - The 8-game byte proof on `matcat-engine`.
  - The viewer branch compared with its own styles-tip baseline.
  - Contact sheets of all 166 materials at High and Low in the default style, plus 20 materials in 4 styles.
  - The per-recipe byte table measured against section 6.
  - `cargo xtask review material-view`, then `cargo xtask critic material-view`, run once.
  - The DECISIONS entry with every measured number.
  - Re-weigh the recipe core's overrun (P3: about 470 B over its 7 KB): the readable WGSL names (`rc_*` helpers, `MatIn` fields) were kept on purpose (decided in P4), so trim elsewhere or accept it here. Weigh with it the ~2.5 KB of one-off recipe plumbing the first listed recipe switches on (P4 notes).
  - **Body-region mask (a known gap, decide here):** no recipe has a mask that restricts a pattern or colour to a body region (belly, face, head, chest, throat, wing, fins, rump, legs, bands round the body), so every row whose look is a region stays `planned`. From a scan of the planned look notes, these wait on it: horse.coat.blood_bay, buckskin, dun, red_dun, grullo, mealy_bay, splash, spotted_blanket; cattle.coat.red_whiteface; sheep.face.black; donkey.coat.brown; dog.coat.black_tan, sable, tricolour; cat.coat.tuxedo, pointed; deer.coat.brown (white belly and tail), pronghorn.coat, bighorn.coat, cougar.coat, squirrel.fur.grey, chipmunk.fur, otter.fur, raccoon.fur, skunk.fur, badger.fur, opossum.fur, sea_otter.fur; bee.fuzz; pig.skin.belted, dolphin.skin, orca.skin, shark.skin; cardinal.feather, goldfinch.feather, eagle.feather.whitehead, vulture.feather, duck.feather.drake, flamingo.feather, pelican.feather, parrot.feather.green, macaw.feather.red, gull.feather; snake.garter, snake.banded, salmon.spawning, bass.skin, perch.skin, tuna.skin. The scan keys on words in the look note, so it can miss or over-include a row; the final review decides whether one shared mask (a body-region number plus a second colour, drawn from uv v or a vertex colour) is worth building in batch 2 or later.
  - **Fur frame cost (P6, measure on a real GPU):** the `?set=p6` page ran at about 8 fps in a headless software test, which is probably the test setup (no GPU adapter), but the final review must confirm it on a real GPU: capture the frame time of `?set=p6&q=high` and `q=ultra` with `telemetry_sample` or the overlay and record it against the P6 fur budget (one pass, no shells).
  - **P8's planned rows, with the body-region mask list (decide together):** gem.opal (flashing colour patches: a hue pattern at patch scale; a cabochon is one cell), gem.magic_crystal (a pulse on a gem: the gem recipe has no time term; P9's glow has one, so a pulse extra on gem, or a glow row, would draw it), ground.snow.crust (wind ridges: a relief the ice recipe lacks), ground.ice.sheet_road (a thin invisible glossy layer: needs alpha, not a surface), ice.frozen_waterfall (ribs run down the column while the icicle ripple rings round it). The final review decides these with the mask list above: each is either one small recipe extension or a decal.
  - **Open decision: blend pass for fire, smoke and holograms (P9; deferred from P10a to here):** the model pass has no blended or additive mode (alpha mask only, discard before the recipe), so fire, smoke, steam and the hologram draw as opaque cards. Decide here, with the other open gaps, whether to add a blended model pass (a pipeline change, so the 8-game byte proof must stay `same`), or leave these to the fx or particle path. Not built in P10a on purpose: a pipeline change mid-batch would put the 16-file byte proof at risk.
- **Acceptance:**
  - Every check is clean.
  - Every recipe is within budget, or the overrun is listed with a trim plan.
  - The review card says what Chris should judge, and the limitation in 2.7.
- **Depends on:** everything above.
- **Verdicts on the open list (P15, 2026-10-24; Chris's standing rule: the recommended option is taken and recorded, DECISIONS "P15"):** nothing on the list was built in P15. Each item is either accepted or needs an engine change that needs its own byte proof, so building it in the final check would reopen the proofs this package exists to close. Each one is placed where its first real users are:
  - **Core ~470 B over its 7 KB, plus ~2.5 KB of one-off recipe plumbing: accepted, no trim now.** The whole crate (core, all 8 layers, all 21 recipes) measures about 31 KB gzip against the plan's 50 KB estimate (section 6), so the total is well inside the budget. The readable names are what every later batch's recipes call. A game that lists nothing pays 0. Trim plan, used only if a game misses its tier download target: rename at strip time (`wgsl_strip` maps `rc_*` and `MatIn` names to short ones in every stripped file, so the source stays readable), worth an estimated 300 to 500 B.
  - **Blend (see-through) pass for fire, smoke, steam, holograms and glass: yes, built in batch 3 as B3-P3.** It is an opt-in sub-feature `material-recipes-blend` (at most 2.5 KB, `[materials] blend = true`) with its own 16-file byte proof, as the batch 3 plan recommends. It is not built in batch 1 because it adds a second model pipeline (sorting, depth write off), and its first real users are batch 3's 8 glass rows. Until then batch 1's flames, smoke and steam are opaque cards and the hologram is opaque with dither gaps; magic.ethereal stays planned.
  - **Mirror reflections (car.chrome.bumper, sf.liquid_metal, 2 optional rows): placed with batch 3's mirrors** (glass.mirror and glass.mirror_old need the same thing). Batch 3 first tries them with the engine's `ibl` sky cube on. If a mirror still reads as grey satin, a scene reflection probe (one cube rendered from the scene) is filed as a render request. This is a pass feature, not numbers, so it is not in batch 1.
  - **Body-region mask (deer.coat.brown in batch 1; about 50 rows in all): start of batch 5** (horses and livestock, where most of the waiting rows are), as the batch 2 and batch 3 plans already say. It is one helper in `rc_shared.wgsl`: a region number plus a second colour, taken from uv v or the vertex colour. The deer is batch 1's only row that needs it, so it stays planned. The region convention should be designed against the horse batch's models, not a ball.
  - **Region mask and decals for stripes, bands, seams and lettering (13 optional vehicle and fantasy rows): batch 11** (the optional 60's own batch), after batch 5's region helper and batch 3's layout helper (`rc_lay_stripe`, `rc_lay_rivets`). Bands and stripes go through the region helper; tyre lettering and seams through the engine's decals or a normal-map seam; sf.biomech as two materials on one model; sf.circuit as a trace layout.
  - **Fur frame cost: measured on the real GPU in P15** (DECISIONS "P15"; RTX 4060 Ti, headless Chrome, `?set=p6`, the whole frame of 30 shapes, not fur alone): settled GPU time 1.6 to 1.95 ms at High 720p, 1.5 to 2.0 ms at High 1080p, 2.0 to 2.2 ms at Ultra 720p, 1.9 to 2.8 ms at Ultra 1080p, 60 fps throughout. The whole fur scene at High is about 2 ms, so fur's own share is well inside the plan's 2 ms rule; no trim is filed.
  - **Wood 160 B over budget: not a batch 1 overrun.** Batch 1's wood is 1,122 B gzip, inside its 2 KB. The overrun is on batch 3's branch (the vertical turn and eight layouts, up to 2,159 B), and it is batch 3's own B3-P13 item with its own trim plan (move those behind a sub-feature). Recorded here only; that branch is not touched.
  - **The horse coat look: accepted for batch 1 as the painted coat.** Since P11 the bay is a dapple with countershading; the mottle is still a little grain-like up close. The depth of close-up fur comes with PF (fur shells), pencilled in after P15 and before batch 5. PF starts only when Chris moves `fur-shells` to Stage 3. The review card asks Chris to judge it.
  - **P8's planned rows:** gem.opal (a colour-patch extra on gem) and gem.magic_crystal (a pulse extra on gem) go to batch 11, with the other optional rows; magic.forcefield (an impact input) and sf.alien_skin (a time term on hide) go to batch 11 too. ground.snow.crust (a wind-ridge relief on ice) and ground.ice.sheet_road (needs the blend pass, or a wet layer on road) go to batch 2, their own batch. ice.frozen_waterfall (ribs down the column) goes to batch 10.
  - **Batch 1's other planned rows:** wood.plank.deck goes to batch 3 B3-P6 (the layout helper; already drawn on `b3-patterns`); cloth.denim.indigo to batch 3 B3-P10b (twill). wall.paper.damask and paper.newsprint go to batch 3's thin recipe stage (B3-P10b, woven and printed motifs through the layout). glass.crystal.cut goes to batch 3's glass recipe stage (B3-P10a, with the blend pass). roof.thatch goes to batch 2's foliage recipe. cloth.wool.blanket (stripes) and cloth.lace.white (openwork: batch 3's cut-out B3-P2) go to batch 4 (clothing). tool.rope.hemp is geometry, a model and not a material, and stays planned. water.ocean.open (whitecaps) goes to batch 10 (water, 14 rows).
  - **The viewer and nature rows (P11b's open item): placed with batch 2 (foliage and grass) and batch 10 (water).** The viewer still opens a nature row (water, glass, foliage, thin) on a ball with only its surface kind; it does not yet build a nature scene for the row or apply `gltf::nature_params`, so two ponds look alike there. The sandbox's `?set=p11b&row=<id>` is where batch 1's 20 nature rows are judged until then. Porting P11b's small scenes (basin, frame, leaf spray, grass patch) into the viewer is viewer work, not engine work, and its first real users are those two batches.
  - **Styles (section 8, risk 8): superseded.** Chris approved keeping each material's detail in every art style (branch `matcat-styledetail`, joined in P15). The review card now says that detail shows in every style, and names its weak spots: thin grain and cracks become speckle at Voxel and Pixel cube sizes, and Ink brush loses most of rust's colour.

### PF: Fur shells (opus-high, parked)

- **Stage gate:** only after Chris moves `fur-shells` from Stage 8 (section 8).
- **Order:** provisionally after P15 and before batch 5 (the away-decision of 2026-10-24).
- **What it builds:**
  - an evani-render feature `fur-shells`: one instanced draw of N shells offset along the normal, 4 to 6 shells on phones and 8 to 16 on desktop, with the count faded out by distance;
  - optional fins;
  - the strand mask from the fur block's density and clump numbers;
  - `material.quality.fur` (high: shells; ultra: plus fins near).
- **Acceptance:**
  - Byte proof (8 games);
  - frame-time measured on the device bench at Low and High;
  - the painted coat unchanged at Lite and Low.

**Order and parallelism:** P0 → P1 → P2a → P2b → P3 → P4 → P5 → P6 → P7 → P8 → P9 → P10a → P10b → P11a → P11b → P12 → P14 → P15.

- P13 runs on the viewer branch from P1 onwards.
- P10a may run beside P3 to P9.

## 6. Byte budget

Sizes are wasm gzip -9, measured as two sandbox release builds that differ only in the proof feature (the method the families used). Each package records its number in DECISIONS.

| Part | Budget | Measured for comparison |
|---|---|---|
| Core: block reader, bindings 12 and 13, patch code, core.wgsl, params | ≤ 7 KB | layers family +7.2 KB, character +13.6 KB |
| Condition layers, all 8 (about 0.6 KB each) | ≤ 5 KB | relief +14 KB |
| Class S recipe: plastic, hide, keratin, organic, glow (each) | ≤ 1.2 KB | |
| Class M recipe: metal, wood, bark, stone, masonry, plaster, ground, leather, feather, scales, ceramic, ice, fire (each) | ≤ 2 KB | |
| Class L recipe: fur (painted), gem (each) | ≤ 2.5 KB | |
| Paint extras: flake, twill, colour shift | ≤ 1.5 KB | |
| Family-backed recipes: cloth, paint coat, skin, hair, eye, water, glass, foliage, thin | 0 new | the existing family's cost, if it isn't already listed |
| Material numbers | 32 B raw each, inside the models that use them (about 20 B gzip) | |

**What a game downloads:**

- **A game that turns on everything** (every recipe on models plus all 8 layers): the new crate costs at most 7 + 5 + 5 × 1.2 + 13 × 2 + 2 × 2.5 + 1.5 = **about 50 KB gzip**. With the three model families it then also needs (layers 7.2, character 13.6, nature 29.3, measured), that is **about 100 KB gzip of wasm**, plus about 20 bytes per material its models use.
- **For scale:** the shipped sandbox wasm is about 313 KB gzip.
- **A typical world:** a western town with wood, metal, stone, masonry, plaster, ground, leather and fur, plus wet, mud, dust and snow, comes to 7 + 2.4 + 7 × 2 + 2.5 ≈ **26 KB gzip** for the new crate, plus layers and character if it has cloth and people.
- **The viewer** (everything plus the dev JSON, about 60 KB gzip) is a dev playground, under its 10 MB limit.
- **A game that lists nothing:** 0 bytes, proved by the byte proof.

## 7. Not in batch 1 (filed for later)

- Catalogue rows on kit meshes (box worlds): mapping rows onto proc and relief tuning, and conditions on the `kit_surface` path. This is for batches 2 and 3 (land, buildings).
- Condition layers on family-backed materials (wet cloth, snowy foliage). These would need small opt-in patches of the family stages. Filed for the batch that needs them (batch 4, clothing).
- A packed per-game material table (`[materials] list`) for kits that draw catalogue materials on code-built meshes without glb.
- A Blender add-on field for `evani_material` (the converter already reads the extra).
- Rendered swatch thumbnails per style.
- Re-pointing `styles.json`'s starter materials at catalogue ids.
- Per-instance condition amounts (needs an instance-data change, so a CORE_REQUESTS item if variants prove too coarse).
- Additive-blended holograms.
- Style treatments moved into the engine (Stage 8, look transfer).

## 8. Risks and open questions (each with the recommended answer)

1. **Fur shells are a Stage 8 item.**
   - Recommended: build batch 1's fur as the painted coat with a sheen rim on every tier (it's the light version anyway). Ask Chris to move `fur-shells` to Stage 3 and build PF after batch 1, before the horse batch (batch 5, 95 fur-heavy materials), where close-up fur matters most.
   - **This is the one decision for Chris** (it moves a stage item).
   - **Decided while Chris was away (2026-10-24, provisional):** Evani.App picked the recommended option, "Painted now, 3D fur before the horse batch".
     - Batch 1 goes ahead with painted fur, which needs no stage change.
     - PF is pencilled in after P15 and before batch 5. It still starts only after Chris himself moves `fur-shells` from Stage 8 to Stage 3 in STAGES.md, because only he moves stage items.
     - Easy to change: nothing is built for PF until then.
2. **"Build fresh" versus porting ShellFurGodot.** Chris chose "Build fresh, copy no code" for the library (2026-10-22). The brief suggested porting with attribution.
   - Recommended: ideas only, after reading its files; credit in DECISIONS. This is consistent with both the research verdict and Chris's choice. No question needed.
3. **Exact-text patches stacking up** (character, then recipes, over plain or layered).
   - Recommended: the anchors listed in P2a, every combination tested with naga, and a fallback to the plain PBR material if a line is missing. A change to `model_lit.wgsl` fails the test, never a game.
4. **Uniform control flow.** Derivatives after the discard are invalid WGSL.
   - Recommended: every derivative goes in `mat_frame`, before the discard (as the character family does). The recipes take no derivatives of their own: a test greps the recipe WGSL for `dpdx`, `dpdy` and `fwidth` outside `mat_frame`.
5. **The viewer branches are unmerged, and the styles tip is "(wip)".**
   - Recommended: P13 waits only for the orchestrator to confirm the styles work is frozen, then branches from that tip and merges `matcat-engine` by merge. This is a question for the orchestrator, not Chris.
6. **The tier-file field from CATALOGUE_PLAN §5.2.**
   - Recommended: drop it and keep the tier tables in the crate's code (byte-identical rule). Recorded in DECISIONS by P0.
7. **The mud reference height.** The fragment stage has no object origin.
   - Recommended: world height above `material.cond.mud.ground` for batch 1. The vertex colour alpha (unused by the lit stage) can carry a painted condition mask later.
8. **Styles show only colours, not recipe detail,** outside the default style.
   - Recommended: say so on the batch-1 review card. Judging in 24 styles in batch 1 means judging colour and value in the style treatment. Detail in styles comes with Stage 8's look transfer.
9. **Byte budgets are estimates.**
   - Recommended: each package measures once at its end. An overrun is trimmed before hand-over or listed in P15 with a plan, following lightness first.
10. **Compiler drift breaking the byte proof.**
    - Recommended: the timestamp check before each proof, and a rebuilt baseline when the compiler changed (section 4).
11. **Slow proofs repeated.**
    - Recommended: one baseline per compiler, sandbox-only proofs for sandbox-only packages, and builds in the background. Never the same slow build three times in one package.
