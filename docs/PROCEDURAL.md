# The procedural library

Evani.Dev's procedural library makes assets from code: a seed and a few
params give a mesh (or an engine module draws it on the GPU). It is built
in two crates, and every category and every asset is its own Cargo feature,
so a game downloads only what its `evani.toml` lists.

Status (2026-11-04): the foundation and batches A, B and C: no stand-in
is left. 24 categories and 357 assets are listed
(catalog/procedural/library.json, imported from Chris's list "Each asset
has a clickable source reference next to it"). 2 assets are the reference
built generators, 10 are existing engine modules wrapped as they are, and
345 are basic generators (status `basic`, Realism only, the "1 out of 10"
pass: the simplest real procedural version of each, one idea each): batch
A's 121 nature assets, batch B's 119 built-world assets and batch C's 105
vehicles and creatures (vehicles, aircraft, boats, humans, clothing,
animals, weapons). Speed over quality: the architecture first; each later
quality pass replaces a function, not the interface.

## Pieces

| Piece | Where | What |
|---|---|---|
| Manifest | `catalog/procedural/library.json` | every category and asset: id, feature, live param prefix, source link and type (Direct / Adapt / Direct reference), licence as stated, status, path, style coverage, MIT inspiration of built assets |
| Base | `crates/evani-procedural` | the asset trait, the registry, params, styles, seeded noise, mesh helpers, shared shapes, the function asset, the placeholder, the scene and the module. No assets. |
| Library | `crates/evani-procedural-lib` | the assets, by category; one feature per category and per asset |
| Generator | `tools/procedural/gen_library.py` | writes the library's Cargo features and its placeholder table (`src/generated.rs`) from the manifest |
| Importer | `tools/procedural/import_docx.py` | reads the owner's .docx list into the manifest (keeps recorded status and notes) |
| Viewer | material-view, Studio, **Procedural** tab | every category as a fold, every asset as a card with an on/off switch, its param panel and its evani.toml line |
| Proof | `examples/proc-field` | a small game that places what its evani.toml lists; the gating proof builds it three ways |

## The asset trait

```rust
pub trait ProcAsset: 'static {
    fn info(&self) -> &AssetInfo;            // category, id, name, Cpu or Gpu path, look material
    fn params(&self) -> &[ParamSpec];        // its params' schema (sliders, colours)
    fn styles(&self) -> &[&'static str] { &[REALISM] }  // art styles built natively
    fn generate(&self, values: &Values, seed: u64, style: &str) -> Output;
}
```

- `AssetInfo::cpu(category, id, name)` for a mesh made on the CPU;
  `AssetInfo::gpu(category, id, name, host, params)` for a wrapped engine
  module that draws itself (`host` names the module, `params` its own live
  params' prefix).
- `Output::Mesh(Mesh)`: the engine's CPU mesh (`[px, py, pz, nx, ny, nz]`
  vertices, u32 indices, base at y = 0), drawn in the asset's `color`
  param. `Output::Parts(Vec<Part>)`: several meshes, each with its own
  colour (alpha under 1 uses the renderer's screen-door fade, for mist and
  water) and look material (`glow` for fire, `glass` for water and ice;
  a name the look lacks falls back to its first material).
  `Output::Module` for GPU assets. `bounds()`, `triangles()` and
  `is_valid()` cover all three.
- Deterministic: the same values, seed and style give the same mesh on
  every run and device (no wall clock, no `rand`; `evani_procedural::noise`
  hashes integers).
- Art style is a first-class input, like the material library pairs a
  material with a style: the 24 world-kit styles `WK01`..`WK24`
  (kits/evani-kit-material-view/data/styles.json). Every asset is built for
  Cinematic Realism (`WK24`) first; `Registry::generate(.., style)` falls
  back to Realism for a style the asset does not list and says so
  (`Generated { style, fallback }`). Adding a style to an asset later is a
  new entry in its `styles()` and a branch in `generate`; no interface
  changes. The manifest's `styles` column records coverage per asset
  (`native`, `fallback`).

## Basic generators (batch A) and the shared shapes

A basic generator is an `evani_procedural::FnAsset`: its `AssetInfo`, its
params and one function `make(values, seed) -> Output`. The library
registers each with `basic!(r, "<feature>", "<category>", "<id>", "<name>",
"<material>", [params], |v: &Values, seed: u32| { .. })`, which puts it
behind its own `#[cfg(feature = "<feature>")]`; the function is a plain
closure, so its code is compiled only with its feature. A function asset
stands on the ground: anything it makes under y = 0 is pressed up to it.

The shared pieces live in the base, `evani_procedural::shapes` (and
`noise`): an icosphere; seeded `rock`s and lumpy `blob`s; `frustum`, `disc`
and `block`; `tube` along a `path` (frames carried along it); `blade` and
`leaf` cards (both sides); leafy `crown`s; `trunk` and `branches`;
`scatter_disc` / `scatter_rect`; `puffs` and `streaks` for weather and
effects; `ripples` for sand; a rocky `vault` (caves, arches); a
`water_sheet`; and `place`, `rotate`, `flat_shade`, `fix_winding`,
`double_sided`, `to_ground`. Noise: `fbm2`, `fbm3`, `ridged2`, `cells2`
(cell distances for stones, cracks, pebbling). A batch-A asset is a few
lines over these; a later quality pass replaces the function, not the
interface.

Every basic asset has its seed, two to four params (size, one or two shape
settings, its colours) and Realism as its only style (others fall back).
The manifest records it as `status: basic` with a one-line MIT inspiration
note (`inspired_by`: the MIT source whose idea it uses, ideas only, written
fresh; no code copied, no third-party crate). The library's tests check
that every basic asset is deterministic, stands on the ground, stays under
400,000 triangles and that every slider (and every colour of a multi-part
asset) changes what it makes through the live params.

## The built world (batch B) and the build helpers

Batch B's 119 basic generators (geometry 14, roads, buildings,
architecture, furniture, props, industrial and city 15 each) are composed
from a second set of shared pieces, `evani_procedural::build`:

- `extrude` / `extrude_x` / `prism` / `extrude_axes`: a polygon (any
  winding, concave allowed: its caps are cut by ear clipping,
  `triangulate`) extruded along an axis; `rect`, `circle`, `star` and
  `gear` make the outlines;
- `sweep`: a closed 2-D profile carried along a path (frames kept as near
  world-up as the path allows, a path whose ends meet is a loop): roads,
  rails, kerbs, I-beams, gutters, W-beams, pipes, arches;
- `revolve`: a profile turned round the y axis, the solid on its left as
  it is walked: barrels, bottles, pots, columns, tanks, bollards;
- both keep a hard edge where the profile turns sharply (over about 53
  degrees) and smooth it elsewhere, so a box section stays crisp and a
  circle round;
- `bar` (a square section between two points), `rod` (a round one),
  `post`, `cbox`, `rounded_box`;
- `gable`, `hip` (roofs), `windows` (panes round a block's four walls),
  `stairs`, `s_path` (the seeded roads' S-curve), `along` (a point, its
  heading and its right at a share of a path), `path_len`, `catenary`.

Each built-world asset is a few parts (`Output::Parts`): its walls, roof,
glass, trim or frame each in a look material (glass for windows and lenses,
glow for lamps, metal for bare steel, stone and ground; the look's fabric and leather keep their own colour, so cloth is ground) and
colour, the main colour and sometimes a second one live params. Doors,
gates and the robot arm take angles; traffic lights which lamp is lit; road
signs their plate shape. The front faces +z, the side the library camera
sees. A function asset drops a part a setting empties (no stools, no
pots), so every slider stays valid at both ends.

## Vehicles and creatures (batch C) and the craft helpers

Batch C's 105 basic generators (vehicles, aircraft, boats, humans,
clothing, animals and weapons, 15 each) use a third set of shared pieces,
`evani_procedural::craft`:

- `loft` / `loft_x` (sections, each its own axes, lofted into a skin that
  faces out from the line of their centres; open profiles leave a U),
  `spindle` and `streamline` (round bodies: fuselages, torpedoes, fish),
  `ellipse` and `squircle` (a superellipse: round to boxy sections);
- `hull`: a boat hull from stations whose width, keel lift and sheer follow
  the length, closed under a deck or an open skin with a transom;
- `airfoil`, `panel` and `panels`: wings, tails, fins, keels and flippers
  (a lens section lofted from root to tip, with sweep and taper);
- `wheel` (a tyre turned from its section, a disc or spoked rim), `limb` and
  `chain` (tapered capsules through joints), `ball`, `ball_along`, `cone`,
  `mirror_x` / `mirror_z`, `scale_parts` and `stand` (parts moved together
  so the lowest stands on the ground);
- the humanoid template, `Human` (height, bulk, leg share, arm and knee
  pose): its 21 named joints (`frame`, `HUMAN_JOINTS`), its 20-bone list
  (`HUMAN_BONES`, name, parent, head and tail joints), its pieces (`torso`,
  `arm`, `leg`, `hand`, `foot`, `head`, `body`) each grown by a thickness, so
  a garment is the body offset and cut to length; `bone_shapes` draws the
  bones as debug octahedra;
- `face_morphs` / `apply_morphs`: the face's morph targets (`smile`,
  `jaw_open`, `brow_raise`, `blink`) as per-vertex offsets, blended live;
- the four-legged template, `Quad` (body length and girth, leg length and
  radius, neck length and angle, head, snout, tail, ears) and `quadruped`:
  the barrel, legs, neck, head, ears, tail, feet and eyes, and the points
  extras hang on (crown, withers, rump, neck line).

Vehicles, aircraft and boats face +x (wheels, gear, floats or keel on the
ground), people and clothing face +z on a grey mannequin, animals face +x,
weapons lie in side view (length along x, the side facing +z). The rigs
are data plus a debug view: the skeletal rig draws the shared bone list;
the expression morphs blend the shared targets by their four weights. The
library's tests check that no asset of the 357 is a stand-in (in the
manifest, in this build and in the generated table), and that every
batch-C asset switches off and on and changes with every slider at its far
end, other styles falling back to Realism
(`the_library_has_no_stand_ins_left`,
`every_batch_c_asset_switches_and_its_sliders_change_it`).

## Realism: catalogue surfaces and the shared realism base

The realism pass (2026-11-04, docs/procedural-realism-progress.md) makes the
library's assets look real under Cinematic Realism, two at a time. Its
shared base, in evani-procedural, is used by every realism asset:

- `surface`: a part in a catalogue material (`Part::surfaced(mesh,
  Surface)`): the row's 32-byte block (recipe, colours, numbers, seed,
  condition amounts) and PBR factors, its uv layout (`Uv::Box`, `Cylinder`,
  `Sphere`, in metres per uv unit) and `tint`, `variant`, `number`, `seed`,
  `condition`. `presets` holds the catalogue rows the library uses (each a
  `const`, so only those an asset names reach a download; the test
  `surface_presets_match_the_catalogue` keeps them equal to the catalogue).
  `FINISHES` / `finish` / `finish_tinted`: one slider over six everyday
  finishes. With the base's `surface` feature (the library's `realism`
  feature, which each realism asset turns on) the module writes an asset's
  surfaced parts into one in-memory `.glb` and draws it through the
  textured-model path, so the recipe stage paints the grain, pores or
  pebbles on the GPU: nothing is downloaded, the block is 32 bytes of code.
  Without it (or in an app with no models) the part draws flat in colour
  A, as before. A game that wants the painted surface lists the recipe in
  `[materials] recipes` (concrete and plaster: `plaster`; oak: `wood`;
  granite and limestone: `stone`; soil: `ground`; grass: `foliage`);
  without it the model draws its plain PBR colour.
- `detail`: `subdivide`, `displace`, `perturb`, `weld`, `split_by`,
  `slope`, `mask_edge`, `chip_box_edges` and `made_block` (a rounded,
  welded, edge-worn block).
- `land`: `Layer` and `cover` (ground cover by slope, height and noisy
  patches, each layer a catalogue surface), `rolling` (warped,
  weathered ground), `turf` and `patchy` (a turf whose dry-grass and
  worn-soil patches the turf shader draws smoothly: foliage number 9
  `patches`), `soil_sides` (the soil walls of a cut plot; terrains' `edge`
  param) and `landform` (rock, grass and snow by slope and height: the
  landforms' live `rock` and `snow` params, with the `MIX_PRESETS` Rocky,
  Grassy and Snow-capped in the library panel).
- `forms`: everyday objects a finish can be previewed on (a ball, a block,
  a door, a table, a fence, steps). The core shapes with a `finish` take a
  `preview` param (0: their own shape), and the library panel offers "On a
  door", "On a table" and so on as presets. Only the library's `preview`
  feature (the Material library view) builds them, so no game downloads
  them.
- Recipe variants added for realism (catalog/materials/recipes.json):
  wood `solid` and `solid (end grain)` (round 8, in place of round 6's
  `figured`: the wood as a log's volume, rings round a tilted pith axis
  along x outside the piece, so the faces, bevels and ends show one piece
  with no seam), stone `crystalline` (granite as 3D crystals of four
  minerals, the dark mica in clusters, faded by the pixel footprint's area
  so the crystals reach the silhouette) and `bedded` (sandstone beds of
  uneven thickness and tone, dipping a little, iron lines, pits), plaster
  `cast` (pour blotches, fine pinholes, clustered 4 to 11 mm bugholes);
  `rc_foot` (rc_shared.wgsl) is the shared weathering at the foot (cast
  concrete, bedded sandstone). Older blocks never use these variants, so
  every catalogue material draws as before.
- The recipe stage's specular anti-aliasing (`mat_saa`, round 8, every
  recipe): the roughness widened by how fast the normal turns inside the
  pixel, so a rounded edge never shows a pinpoint glint.
- `noise`: gradient noise (`perlin2`, `perlin3`), rotated fBm (`fbm`,
  `fbm3p`), domain `warp`, `eroded` (derivative-damped fBm), `cells2_id`.

The library view is a photo studio (round 8, `LIBRARY_VIEW` in
apps/material-view/src/studio/proc_view.rs): a warm low sun as the key
(bearing 130, 32 degrees up) with soft shadows of a physical size (the
viewer now builds evani-render's `pcss-stable` and `shadow-cascades`);
ambient and reflections from the engine's sky light turned into studio
panels (evani-render `studio-sky`, `render.sky.studio`; the look's
`studio` param hands the hemisphere to it), the ambient occlusion on the
ambient and reflections only; Khronos PBR Neutral as the tone curve (the
look's `tonemap` param) at a fixed exposure and saturation 1; no rim, no
shadow tint; no sky dome, a warm pale backdrop the floor fades into (the
look's `haze_near` and `haze_far`, set from each item's camera) and a
gentle depth of field with a 1-step dither (the studio's miniature pass at
`grade` 0). Albedos are real ones again (concrete about 0.27, oak a warm
honey, sandstone buff), not the darkened ones rounds 4 to 7 used. The
realism assets have their own framing (`REALISM_FRAMING` in proc_data.rs: a
longer lens, about 70 mm, and the camera height of each reference
picture). The flat field wears evani-grass lawn blades over its turf (the
viewer hands the grass module a patch the plot's size; the hills wait for
CORE_REQUESTS `grass-heightfield`).

Each realism asset's reference picture, score (out of 5), status and note
live in its manifest entry (`realism`); `gen_library.py` writes
`catalog/procedural/references.json`, and the Procedural tab shows the
picture as "Reference target" above the asset's settings. The pictures are
viewer data (`apps/material-view/assets/proc-refs/`), never in a game.

### Real relief (round 9)

Chris (binding): models match their reference's shape, and relief is real
geometry that shows in silhouette, never painted. The shared helpers, in
`evani_procedural::detail` unless named:

- `spall(mesh, at, radius, depth, fine, seed)`: a broken chip, its floor a
  few flat fracture facets, notching the arris and the silhouette;
  `made_block` breaks real chips along a block's arrises by `wear` (the
  cube, the oak block, the game's wall) and keeps the rounded box's exact
  normals on the faces (smooth normals over its edge-bunched grid made a
  cushion).
- `carve_crack_moved`, `smooth_normals_where`: carving changes the normals
  only where it moved the surface.
- `refine_near`: conforming longest-edge bisection where a carving needs
  vertices (no T-junctions; a thin arris triangle is cut only along its
  length).
- `kis`: every facet a shallow pyramid (the faceted ball's `relief`).
- `onto_rounded_box`, `resample_profile`: paths laid on a rounded box (the
  oak's `checks` run over its arrises), lathe profiles with rows for relief.
- `land::outcrops`: weathered rock slabs on the steepest ground, tilted with
  the slope and sunk into it (the hills' `outcrops`).
- `noise::gpu`: the recipe stage's own hash and value noise on the CPU, so
  geometry lines up with the material (the column's soft beds weather back
  where the shader paints them).
- Tiered mesh density: the module passes the active tier's
  `Density::for_tier` as `Values::density`; carving refines less on Lite to
  Medium (the relief keeps its size and place; a tier change remakes the
  asset). `realism_assets_stay_light` prints both counts.

Live relief params: `procedural.geometry.cube_cuboid.{wear,crack,crack_length,crack_width,crack_depth,crack_branches}`,
`procedural.geometry.bevelled_box.checks`, `procedural.geometry.icosphere.relief`,
`procedural.geometry.cylinder.chips`, `procedural.terrain.rolling_hills.outcrops`.
The rolling hills wear evani-grass blades on their own ground
(`wrapped::hills_grass`, `Grass::add_patch_on`; CORE_REQUESTS
`grass-heightfield`).

### Concrete (round 9, Chris's concrete spec)

One composable concrete surface for any shape (plaster variant 5,
`crates/evani-material-recipes/shaders/rc_plaster_concrete.wgsl`): macro
tone (warm and cool drifts, lift lines), aggregate, clustered pores (tiny,
small, rare lobed blowholes, recessed by real normals, darker with depth),
grain and varied roughness, in object space (it stays on a moving object),
its normal from its height (`rc_h2n`). Presets: `surface::concrete_preset`
(0 fresh poured, 1 standard structural slightly aged, 2 aged exposed,
3 rough industrial, 4 polished; the cube's live `concrete` param) and
`surface::concrete_view` (the diagnostic views; the cube's `view`). Any
asset gets it from `surface::cast_concrete` or `concrete_preset`; a made
block's edges wear by `detail::edge_wear` (chips on a few arrises, most
intact); the cube's crack is optional (`crack` is its chance). Tiers: Lite
and Low draw the dominant plane's colour, Medium adds the normals, High and
Ultra blend the planes and add aggregate and grain.

### Applying a realism material in a game

How an AI (or anyone) gets exactly the library's look on a surface in a
game, by the route the engine already has (round 8; Chris: "an AI building
a game says 'I want this look on that wall' and must get exactly what the
library shows"):

1. Find it: `catalog/procedural/library.json` lists every asset (its
   `realism` entry: the finish and the reference picture); the finishes are
   catalogue materials (`catalog/materials`, `evani_procedural::surface::
   FINISHES`: cast concrete, polished concrete, plaster, granite, oak,
   brushed steel, sandstone).
2. List it in `evani.toml`: `[modules] procedural = ["geometry-cube-cuboid"]`
   (the asset; a wall is the cube with its width, height and depth set),
   `models = true`, and the finish's recipe in `[materials] recipes`
   (concrete and plaster: `plaster`; oak: `wood`; granite and sandstone:
   `stone`; soil: `ground`; grass: `foliage`).
3. Light it as the library does: the stylizedpbr look with features `film`,
   `pcss` and `gtao`; evani-render `ibl`, `gtao`, `pcss-stable`,
   `shadow-cascades` and `studio-sky`; evani-fx `shadows`. Then call
   `evani_procedural::studio::apply(&mut params)` once (it sets the
   studio's light, ambient, reflections, soft shadows and tone curve and
   returns the names of any param the build lacks) and point the sun with
   `render.light.direction` (`studio::SUN` is the library's).
4. Place and tune it live: `ProcScene::place("geometry-cube-cuboid", at)`,
   then `procedural.geometry.cube_cuboid.<param>` through `param_set` (the
   live API, docs/LIVE_API.md): `width`, `height`, `depth`, `finish`,
   `color`, `bevel`, `wear` (chipped, worn edges), `crack` (a carved crack:
   "a more cracked wall" is a higher `crack`).

The proof is the proof game's parity scene (examples/proc-field, feature
`wall`; `tools/procedural/parity.mjs`, then `parity_diff.py` against the
library's shot): the same cube under the same camera in a game and in the
library, compared pixel by pixel (docs/procedural-realism-progress.md,
round 8). What the library adds round it is only a photograph's: its
camera and framing, the backdrop and haze, and the depth of field (the
studio's miniature pass, evani-render `studio-passes`, at `grade` 0).

## The registry

`Registry` holds the categories and assets compiled into this build:
`evani_procedural_lib::registry()` adds each under its `#[cfg(feature)]`,
so it never names an asset that is not in the download. It also:

- registers every asset's live params (`register_params`);
- reads an asset's live values and seed (`values`), and generates it
  (`generate`);
- lists an asset's param names (`param_names`) and its switch (`is_on`).

## Live params

Every asset gets, in the engine's one param registry (the same one the Look
Lab, the library viewer and AI agents drive with `param_set`, docs/LIVE_API.md):

- `procedural.<category>.<asset>.on`: on (drawn) or off (removed);
- `procedural.<category>.<asset>.seed`: CPU assets;
- `procedural.<category>.<asset>.<param>`: one per `ParamSpec`.

Ids use `_` for `-` (`procedural.terrain.rolling_hills.feature_size`).
A wrapped module keeps its own params as they were
(`material.nature.water.*`, `material.grass.*`, `sky.*`); the library adds
only its switch.

## The module and the scene

`evani_procedural_lib::module()` is the engine module
(`evani.module.procedural`); add it after the render module. It:

- registers the params, and puts the `Registry` and a `ProcScene` in the world;
- draws every placement in `ProcScene` whose asset is on, generating its
  meshes (one per part) into the engine's mesh library and regenerating
  them only when its params, seed or the scene's style change, and frees
  the meshes of an asset no longer placed (the library has 248 custom
  slots);
- hosts the wrapped modules this build needs (the nature pass, the grass);
  an app that adds one itself says `.hosted_elsewhere(&["nature"])`;
- hosts the drawers of CPU assets whose look is a GPU pass
  (`.host_drawing(key, &[features], module)`): their placements stay in
  `ProcScene` but the module draws them, not the mesh. The volumetric
  clouds (`sky-volumetric-looking-clouds`) are one: their generator lays
  out the cumulus's dense heaps, its thinner vapour and its wisps, and
  `evani_procedural_lib::clouds::CloudVolume`
  ray-marches them as a volume (four game passes, live params
  `procedural.cloud_march.*`), so a game that lists the asset and places it
  gets the volume with nothing else to add.

A game places assets:

```rust
world.res_mut::<evani_procedural::ProcScene>().place("terrain-rolling-hills", [0.0, 0.0, 0.0]);
```

## Composition from typed links

The building-block graph (`catalog/procedural/blocks.json`, version 2;
docs/ASSET_STUDIO_GUIDE.md "Building blocks") says what each landscape and
block is made of and how each child feeds its parent: **layer** (a material
blended into the parent's surface by a mask), **scatter** (instanced copies)
or **base** (the parent's shape: `merge`, `part` or `surface`).
`tools/procedural/gen_blocks.mjs` writes it as engine data
(`crates/evani-procedural-lib/src/blocks_table.rs`, feature `compose`) and
`evani_procedural::compose` assembles an asset from it:

- **layer** links become the catalogue materials' condition layers (snow,
  growth for moss, mud...; `Surface::condition`) on the parent's own
  surfaced parts, at the link's `blend`: a layer never adds a mesh or a
  draw. Its mask is evaluated on the CPU (`LayerUse::coverage`) and carried
  in the assembly. A planned block brought in by a layer (the alpine meadow)
  applies its own links inside that layer's mask.
- **scatter** links become seeded placements of the child asset over the
  parent's ground (density per m², scale range, slope and height bands,
  water). Only scatter links make instances.
- **base** links: `merge` joins the child's meshes into the parent's parts
  (into the part with the same surface, else the first: never a new draw);
  `part` is its own placement at the parent's spot (the sky dome, the sea,
  the scree apron); `surface` dresses the parent's bare parts in the
  child's surface.

A game turns it on with the `compose` feature of evani-procedural-lib:

```rust
ctx.resource(evani_procedural_lib::composition()); // the module draws layers and base joins
let mut scene = world.res_mut::<evani_procedural::ProcScene>();
evani_procedural::compose::place(&evani_procedural_lib::blocks_table::GRAPH, &registry, &params,
    "terrain-mountains", [0.0, 0.0, 0.0], &Default::default(), &mut scene); // the asset, its parts and its scattered copies
```

What has no engine path yet is listed in `Assembly::missing`, never
guessed: a layer whose material has no condition layer (blending a second
catalogue material by a mask; e.g. turf's loam, the marsh grass), a
catalogue material named as a surface (its 32-byte block isn't compiled in,
so bare parts stay flat), per-link height, curvature and noise masks in the
shader (the condition shader masks snow by slope only, through the global
`material.cond.snow.angle`), and the scatter's LOD and impostor distances
(the module draws every placement at full detail). Children's param changes
don't remake a composed parent until its own params change. The test
`compose_tests` proves that Mountains, with its snow and its alpine meadow's
moss, assembles as one mesh with two material layers and that only scatter
links make instances.

## Features and evani.toml

```toml
[modules]
procedural = ["terrain", "geometry-cube-cuboid"]
```

turns on the app features `procedural` and `procedural-<part>`; each
forwards to evani-procedural-lib's feature `<part>`:

```toml
[features]
procedural = ["dep:evani-procedural-lib"]
procedural-terrain = ["procedural", "evani-procedural-lib/terrain"]
procedural-geometry-cube-cuboid = ["procedural", "evani-procedural-lib/geometry-cube-cuboid"]
```

- `<category>` turns on all its assets; `<category>-<asset>` one asset.
- Internal: `cat-<category>` (the category's entry, on with any of its
  assets) and `host-nature` / `host-grass` / `host-sky` (the wrapped crates,
  on only with an asset that wraps them).
- `cargo xtask check` reads `procedural` like the material modules
  (`nature`, `proc`): a listed part the library has no feature for is an
  ERROR, and so is a listed module the app does not depend on.

## Proof of gating

`python -I tools/procedural/gate_proof.py <target-dir> <out.json>` builds
`examples/proc-field` three ways (release wasm, the engine's ship profile)
and scans each wasm for every asset's name (compiled in only with its code):

| Build | wasm bytes | added | gzip added | assets inside |
|---|---|---|---|---|
| none | 1,515,729 | 0 | 0 | none |
| one asset (terrain-rolling-hills) | 1,542,197 | +26,468 | +11,144 | that one only |
| one category (geometry, 15: now 14 basic + the cuboid) | 1,592,148 | +76,419 | +31,309 | geometry's only |
| one batch-A asset (rocks-medium-rocks) | 1,548,455 | +32,726 | +13,744 | that one only |
| one batch-A category (trees, 15 basic) | 1,585,073 | +69,344 | +28,645 | trees' only |
| one batch-B asset (buildings-castles) | 1,550,868 | +35,139 | +15,246 | that one only |
| one batch-B category (city, 15 basic) | 1,600,951 | +85,222 | +34,701 | city's only |
| one batch-C asset (vehicles-sedans) | 1,554,080 | +38,351 | +16,810 | that one only |
| one batch-C category (animals, 15 basic) | 1,604,307 | +88,578 | +37,695 | animals' only |

(2026-11-04, after batch B. "none" is the same 1,515,729 bytes as before
batch B, and the same bytes: built in the same folder at m1-foundation
956909a4 and at proclib, the two wasm files differ only in the engine's
embedded 7-character commit stamp. The base grew by the build helpers, only
in builds that use them; a one-asset build carries only that asset's code.
The name scan matches a second, unrelated name inside a longer one: "Fruit"
in "Fruit trees" for trees, "Stone" in the castle's own strings; neither
asset is in those builds.)

(After batch C, 2026-11-04: "none" is again 1,515,729 bytes; compared byte
by byte with the m1-foundation build it differs in 7 bytes, the embedded
commit stamp, and nothing else. The name scan finds 10 of the 15 animals in
the animals build: the other five names (Dogs, Cats, Pigs, Deer, Fish) are
too short for the scan to count, and no other category's name is there.)

A game without the `procedural` line has no dependency on the library at
all, so it adds zero bytes; with one asset, no other asset's name or code is
in the wasm (2026-11-03, rustc 1.99.1; DECISIONS.md "Procedural library").

## The viewer

The material-view's Studio has a **Procedural** tab (next to Materials). With
the library built in (its evani.toml lists every category), the tab lists
the library's 24 categories as folds ("Terrain and landforms · 15 · 1
built"), each asset a card with an **On / Off** switch (the asset's `.on`
param), and opening a card shows its panel: the switch, the style note, its
sliders (seed and params, all live params), presets and the "Use in a game"
fold with its evani.toml line. A wrapped entry opens the existing staging
(the storm sea is the Ocean on its storm preset; lakes, ponds and rivers the
nature water; grass a patch of evani-grass). Switching an asset off removes
it from the view; on puts it back.

## Adding an asset (category workers)

First look for a similar existing asset, material or module to copy and adapt (see docs/ASSET_STUDIO_GUIDE.md section 1.0).

1. Find the asset in `catalog/procedural/library.json` (its `feature`, ids
   and source). Read the source for maths and structure only: no code is
   copied or vendored, and no new third-party procedural crate is added.
2. Copy `crates/evani-procedural-lib/src/template.rs`'s `Template` into
   `src/<category>.rs` behind `#[cfg(feature = "<category>-<asset>")]` (add
   `pub mod <category>;` to lib.rs if new), with the manifest's ids.
3. Use `evani_procedural::geo` (grids, boxes, lathes, transforms, merging,
   normals) and `noise` (hash, value noise, fBm); add shared helpers to the
   base crate, not to one category.
4. Register it in `registry()` under the same `#[cfg]`.
5. In the manifest set `status` to `done` (or `basic` for a first, simplest version), `inspired_by` to the MIT source
   and what was taken, `styles.native` to the styles it builds, and `cargo`
   if it needs another crate's feature. Run
   `python -I tools/procedural/gen_library.py` (drops its placeholder).
6. `cargo test -p evani-procedural-lib --features <category>`, then
   `cargo xtask check`.

A category's tests: `crates/evani-procedural-lib/src/tests.rs` checks that
the manifest, the Cargo features and the registry agree, that every asset
registers valid param names and generates a valid mesh, and the built
assets' own behaviour. Add a test per built asset there.

## Built and wrapped assets, and their inspiration

| Asset | Status | Inspired by (MIT, read for structure only) / wraps |
|---|---|---|
| geometry-cube-cuboid | built (CPU) | three.js BoxGeometry (MIT): per-face segment grids |
| terrain-rolling-hills | built (CPU, seeded) | THREE.Terrain (MIT): a height grid filled by layered value noise (fBm) |
| water-oceans | wrapped | evani-material-nature part `spectral-sea` (the Ocean card) |
| water-ocean-waves | wrapped | the storm sea: the spectral sea on its storm preset |
| water-lakes, water-ponds | wrapped | evani-material-nature's water (`nature = ["ocean"]`) |
| water-rivers | built (CPU, seeded; Asset Studio column 20) | a river valley in its surround: a height grid (finer by the river, crack-free between levels) carved along a meandering line of uneven bends and widths (point bars with cobbles, cut banks), fields, scrub, wildflowers, oaks, low hills and layered ridges; its water is evani-material-nature's (`river::water`, flowing along `water.flow_direction`), its bank grass evani-grass's (`river::bank_grass`) |
| textures-glass | wrapped | evani-material-nature's glass |
| sky-dynamic-sky-dome | wrapped | evani-material-proc part `sky` (the kit installs it) |
| vegetation-short-grass, vegetation-long-grass, agriculture-wheat-plants | wrapped | evani-grass (lawn, meadow, wheat) |

Batch A (status `basic`, 121 assets): terrain (14), rocks (15), vegetation
(10), trees (15), water (10), sky (14), agriculture (14), textures (14) and
effects (15); each one's idea and MIT inspiration is its `inspired_by` line
in the manifest. The textures are swatch tiles whose pattern is relief in
the mesh (planks, bricks, cells, weave), so the light reads it; the effects
and weather are one still moment as a mesh (puffs, streaks, glowing blades).

Batch B (status `basic`, 119 assets): geometry (14, with the built
cuboid), roads, buildings, architecture, furniture, props, industrial and
city (15 each); again each one's idea and MIT inspiration is its
`inspired_by` line. The library's tests check that none of the 8
categories' 120 assets is a stand-in, and that each switches off and on
through its `.on` param and changes with every slider at its far end
(`the_built_world_has_no_stand_ins_left`,
`every_built_world_asset_switches_and_its_sliders_change_it`).

The library camera (material-view, `library_cam`) fits each asset's box
to 80% of a 16:10 view from the usual heading, so small, flat and long
assets fill the frame; a narrower view pulls back. A picture of the scene
alone (`proc-shots --bare`) hides the panels and the view refits to the
whole canvas.

Batch C (status `basic`, 105 assets): vehicles, aircraft, boats, humans,
clothing, animals and weapons (15 each); each one's idea and MIT
inspiration is its `inspired_by` line.

The placeholder (a simple shape picked from a name, in its category's
colour, with a size, a colour and a seed) stays in the base for a new
listed asset before its generator exists; none is in use.

Asset Studio rounds (building, capturing, submitting and scoring a column's asset, with the capture script `tools/studio/asset-shoot.mjs`): `docs/ASSET_STUDIO_GUIDE.md`.
