# Asset & Style Studio: the contracts (WP0, 2026-10-30)

The owner's spec is [SPEC.md](SPEC.md) (mockup: `C:\dev\evani-dev\.vibedeck\plans\asset-style-studio\mockup.png`, kept out of the repo for its 2.3 MB). The design is `.vibedeck/plans/asset-style-studio/ARCHITECTURE.md` in the main checkout. This page is what every work package codes against; [WORK_PACKAGES.md](WORK_PACKAGES.md) says who owns what.

All of it is code with tests in `crates/evani-studio` (pure data and pure functions: serde and serde_json only, no engine, no GPU). `cargo xtask check` runs its catalogue cross-checks and the asset x style coverage check on every run.

**Changing a contract.** Only through the orchestrator: one small commit marked `CONTRACT:` that updates this page, the crate and its tests together; every package then merges it. The only lines a package may change on its own are listed under its package.

## 1. The record (`state::Selection`)

`Selection { asset, style, material, parts }`: WHAT (an asset id), HOW (a style id), OF WHAT (a material id), and `parts`: part id -> the material the user gave that part (section 2, "Parts"). The record is kept **canonical**: a part's material that equals what the part would wear anyway is never stored, and a pick for the part the record's material sits on changes the record's material (`Selection::with_part`, `canonical`), so click order still can't matter. `key()` is `asset|style|material` plus `|part=material,...` when parts have their own (memory, presets and the variant key all see them). The selected part (`StudioState::part`, `None` = "All parts") is outside the record and in no key. The drawn result is `resolve(catalogues, selection, style controls, remembered settings)`, a pure function, so **click order can't matter**: the test `click_order_never_matters` drives the three picks Voxel, Horse, Bay coat in all 6 orders from two different starting states and gets the same record, bindings and keys every time.

Outside the record, never in a cache key, never rebuilding the object: `View { light, quality }` (test `lighting_and_quality_never_rebuild`).

Remembered per record (`StudioState::memory`, keyed by `asset|style|material`): `Settings { edits (param name -> value), camera }`. Style controls are remembered per style (`StudioState::controls`). Returning to a record restores both (test `combinations_keep_their_settings`, acceptance test 8). The whole state round-trips through JSON (`to_json` / `from_json`) for the page's storage; ids the catalogues no longer hold are dropped on load.

## 2. The asset catalogue (`catalog/assets/assets.json`, `assets::AssetCatalog`)

One row per asset, one source model per asset. Fields: `id` (`family.name`), `name`, `category` (from `categories`), `subcategory`, `class` (from `classes`), `search_words`, `source` (`code` + `shape`, `blender-script` + `script` + `model` [+ `species`], or `model` + `model`), `regions`, `default_material`, `frame {yaw, pitch, fill}`, `batch` (0 = proof set B0), `status` (`planned`, `building`, `ready`). Optional until ready: `thumbnail` (`thumbs/<id>.png` under the viewer's `models/`, about 0.6 to 7 KB, drawn from the asset's own source by `cargo run -p evani-style-geo --example studio_assets -- thumb <id> <out>`) and `tiers` (the device tiers of `assets/tiers/*.json` whose download carries the source).

- **Regions** (1 to 16, one material slot each): `id`, `accepts` (tokens `class`, `generic`, `recipe:<id>`, `sub:<category>/<subcategory>`), optional `follows` (takes another region's material; one level), `tone` (`base` or `points`: a bay's black legs, mane and tail), `thin` (the geometry stage must keep it), `default_material`, `keep` (the source model's own colour: eyes).
- **Parts** (1 to 16; every region in exactly one part; `assets::Part`): what the user gives its own material. `id`, `label` ("Gable ends"), `kind` (from the catalogue's `part_kinds`: `coat`, `mane`, `tail`, `hoof`, `horn`, `muzzle`, `nose`, `eye`, `roof_tiles`, `ridge`, `masonry`, `plaster`, `timber_trim`, `coping`, `plinth`, `ground`, `surface`) and `regions` (its lead first). A part's material lands on its lead and on every listed region that doesn't follow another region of the same part (a horse's legs keep following the coat, in their points tone); it breaks a follow that crosses parts (a mane given its own hair stops following the coat). Its default is its lead region's (`default_material`, the region it follows, or the asset's). A part of kept regions keeps the model's colour unless a region accepts the pick by a specific token (the dog's nose takes a skin). The kind is what carries a part's material to another asset (a horse's tail colour onto the cow's tail) and the page's grouping. Parts now:

  | Asset | Parts (default material) |
  |---|---|
  | Roof | Roof tiles (the record's: terracotta), Ridge cap (flat clay tiles), Gable ends (red brick), Fascia and barge boards (oak beam) |
  | Wall | Wall face (the record's: red brick), Coping (limestone), Plinth (follows the face) |
  | Horse | Coat (coat, legs, ears), Mane, Tail (both follow the coat's points), Hooves, Muzzle, Eyes (kept) |
  | Cow | Coat, Tail, Horns, Hooves, Muzzle, Eyes |
  | Dog | Coat, Tail, Nose (kept), Eyes (kept) |
  | Ground patch, sphere, cube | one part each |
- **Classes**: `id` (`family.kind`), `name`, `label` ("a horse"), `plural`, `exclusive` (its materials are foreign elsewhere), `swatch` (shows any material: test shapes), `primary_asset`.
- `default_asset` is the asset the studio opens on and must not be a swatch: **no demonstration sphere** (test `a_test_shape_can_never_be_the_opening_asset`). Sphere and cube are ordinary rows of the `shape.basic` class.
- `status` means: `ready` = the source model is in the app (on every tier, with its thumbnail) and every style must exist for it; `building` = shown as "being built" (the same asset, never another object). Each package flips its own assets' rows to `ready` when their source lands.

B0 holds 8 rows: horse, cow, dog, wall, roof, ground patch, sphere, cube.

## 3. `belongs_on` on materials (`materials::MaterialIndex`)

Optional on any row of `catalog/materials/materials.json`: one class id or a list (`"quadruped.horse"`, `["quadruped.*"]`). Absent = a generic surface. It decides two things: where a material previews when no asset was chosen on purpose (any class), and whether it is foreign on another asset (only `exclusive` classes). The recipe crate ignores it (it never reaches the 32-byte block); `cargo xtask check` errors on a class that doesn't exist. Every row is decided by the reviewed rules in `tools/materials/belongs_on.json` (applied by `node tools/materials/belongs_on.mjs --write`; the viewer kit's test `every_material_row_has_a_reviewed_belongs_on` holds the catalogue to them): 168 rows name a class (40 horse, 9 cattle and 12 dog coats, 29 wall, 10 roof and 68 ground materials), 504 are deliberately generic with the reason (cloth, metal, body parts the cow shares with the horse...), and 382 wait for an asset class that isn't in `assets.json` yet (`waits <class>`: farm animals and cats, wild mammals, birds, reptiles, fish, insects, plants, people, rock); rerun the script when a class is added.

## 4. The style split (`styles::StyleCatalog`)

Each style in `kits/evani-kit-material-view/data/styles.json` has a `studio` block: `geometry` (`none`, `voxel {cell, keep_thin}`, `lowpoly {cluster, target_tris, facets}`, `papercraft {cluster, target_tris}`, `lumpy {amount}`, `ridges {amount}`, `proportion {head, limbs}`), `pass` (`none`, `pixel {px, levels, outline}`, `paper {grain, edge, bleed}`, `tiltshift {focus, blur}`, `silhouette {fog}`) and `controls`. The **surface** stage is the style's existing `render` block (bands, outline, posterize, dither, mottle...). All 24 styles are split:

| Kind | Styles |
|---|---|
| Geometry-led | Voxel (voxel), Low-poly and Retro 3D (lowpoly), Papercraft, Yarn (ridges), Clay (lumpy), Rubber-hose and Storybook (proportion, later than B0) |
| Pass-led | Pixel and Pixel HD (pixel pass; the legacy voxel stand-in is dropped), Diorama (tilt-shift), Silhouette, Watercolour, Ink, Gothic, Storybook (paper) |
| Surface-led | Toon, Cosy, Cel, Anime, Comic, Dither, Neon |
| PBR | Painterly, Realism |

Where both blocks describe the shape, the numbers agree (test `every_style_is_split_and_agrees_with_its_render_block`).

**Controls** (`styles::CONTROLS`): `geometry.cell`, `geometry.detail`, `geometry.amount`, `geometry.head`, `surface.bands`, `surface.outline`, `pass.px`, `pass.blur`, `pass.grain`; each a registered param `kit.material_view.studio.<style id lowercase>.<control>` (`control_param`), clamped to its range, entering only its own stage's key.

## 5. Compatibility (`compat`)

`compat(asset, material)`: `Native` (belongs on the asset's class), `Generic` (belongs nowhere, on a non-exclusive class, or the asset is a swatch), `Foreign { made_for }` (an exclusive other class), `NoRegion` (no region accepts it). `place(asset, material)` is the region it lands on: among the regions a specific token (`class`, `recipe:`, `sub:`) takes it, the one the material's id names (a word of the id split on `.` and `_`: `horse.muzzle.pink` on the muzzle, not the coat that also takes hide; a hoof on the cow's hoof, not its horn), else the first in the asset's order; with no specific taker, the first that accepts `generic` (test `a_part_lands_on_the_region_it_names`). Every style goes with every asset.

## 6. Auto-match (`StudioState::apply`)

- Material picked, asset not chosen on purpose: if the material belongs on a class, the asset becomes its primary asset; notice `ShownOn` ("Showing Horse coat, bay on a horse").
- Material picked, asset chosen on purpose: applied if native or generic. Foreign: **the asset stays and the material is not applied**; notice `NotApplied { offer }` ("Made for horses · Show on a horse"); `Pick::ShowOn(asset)` is the one click.
- Asset picked: the style stays; the material stays if it goes on the new asset, else the one last used on that class, else the native one closest in colour, else the asset's default; notice `Substituted { because }`.
- Style picked: nothing else changes. A planned material changes nothing (`NotBuilt`). An unknown id is an error and changes nothing.
- **Parts.** `Pick::Part(Some(id))` selects a part, `Pick::Part(None)` is "All parts"; neither changes the record. With a part selected, `Pick::Material` (and a variation tile) goes on that part only (`Pick::PartMaterial { part, material }` names the part outright): never moves the asset; a material the part doesn't take changes nothing and says why (`NotOnPart`, or `NotApplied` with "Show on a horse" for a foreign one). With "All parts", the pick is the record's material as above, and it also goes on every other part *made for it* (`compat::part_made_for`: for a material made for a class, a part taking it by `class` or `sub:`; for a material made for nothing in particular, also by `recipe:` or `generic`), a part whose lead follows another follows again, and every other part keeps its own: a slate roof puts slate on the tiles and the ridge, never on the brick gable. A new asset takes the old asset's part materials by part kind where it has such a part that takes them.

## 7. Resolve and the cache keys (`resolve`, `hash`)

`Resolved { selection, availability, geometry, surface, pass, bindings, geometry_key, variant_key, pass_key }`.
- `availability`: `Ready` (geometry `none` on a ready asset), `Generate(GeometryKey)` (run or fetch the geometry stage; meanwhile draw the same asset plain), `NotBuilt` (the asset is still being built). There is **no unavailable end state** and resolve never names another asset (tests `resolve_draws_the_record_or_refuses`, `every_asset_resolves_in_every_style`).
- `bindings`: one per region, in the asset's order: `Selected`, `Part(part id)` (the record's material for that part, base tone), `Follows(region)`, `Default`, `Keep`, with the tone. `resolve::part_bindings` gives each part with its lead region's binding (the page's parts list and combination card).
- Keys: FNV-1a 64 (the engine's content hash) over a canonical stream, floats quantised to 1/10,000. `GeometryKey` = engine version, generator version, asset id, source model hash (from the stream manifest), geometry stage, geometry controls. `VariantKey` = geometry key, bindings, surface stage, surface controls, edits. `PassKey` = pass and pass controls. Light and quality enter none (test `cache_keys_cover_their_inputs`).

## 8. The geometry interface (`geo`, crate `evani-style-geo`)

`SourceMesh { positions, normals, indices, regions }` (rest pose, unskinned, radius 1, a region index per vertex) -> `GeometryGenerator::generate` -> `StyledMesh { parts }` (at most 16 parts, one region each; `samples` per vertex for the pattern colour, a voxel's cell centre). Deterministic. `generator_version(kind)` enters the geometry key; 0 = not built yet, and `cargo xtask check` notes those styles. `evani_style_geo::generator(kind)` is the one table; its test holds every working generator to the contract. Source attributes: `_EVANI_PART` (u8 region index per vertex, written by the studio's generators) or the horse's `_EVANI_REGION` chart.

**Code-built parts and their pattern layout.** `code.rs` builds each part as its own region (`code::regions`): the roof's tiles (8 lapped courses a side), ridge (a half-round capping of ridge tiles), gable (the two triangular ends) and fascia (a fascia board under each eave and a barge board along each verge, over the courses' stepped ends); the wall's face, cap and plinth. `code::code_uv(shape, region, p, n)` is a part's own pattern layout at the studio's scale (half a uv unit per unit of length, radius-1 space): roof tiles v down the slope along the surface and u along the ridge (each tile points downhill), ridge tiles v along the ridge, boards with the grain along the board; `None` (masonry, the gable, the wall, the ground) keeps the studio's level projection. The viewer asks for it with the layout `studio:<shape>` (`scene_build::asset_uv`); a voxel takes it at its cell centre. Ridge and fascia are `thin`, so voxel and low-poly keep them (test `styles_keep_the_parts_apart`).

## 9. In the viewer

- Kit (`studio` feature): `Studio { state, pending: Vec<StudioCommand>, error }` and `StudioData(Rc<Catalogs>)`; commands `Pick`, `Control`, `Edit`, `Light`, `Quality`, `Compare`, `Save`, applied in order once a frame. Re-exports the contracts as `evani_kit_material_view::studio::*`.
- App (`material-studio`, listed in evani.toml): `src/studio/` with one file per package. The scene draws `Studio::state.resolve(..)` and nothing else.
- The page's parts: the inspector opens with **Parts** (an "All parts" row, then each part with its own engine-drawn material ball and material name; `data-act="part"`); picking a part sends `Pick::Part`, then a chip, a "More" row or a variation tile goes on that part only. The chips mark the selected part's material and grey what can't go on it ("Can't go on the gable ends"). The variations strip varies the selected part (`variations::variations_for`). The combination card lists each part's material (your own picks marked). Material settings say which material they edit (the record's).

## 10. Checks that guard the contracts

| Breaks | Caught by |
|---|---|
| A bad asset row, class, region or source | `AssetCatalog::parse` (`cargo xtask check` ERROR) |
| `belongs_on` naming no class; a default material that is planned or doesn't go on its asset; a region token naming no recipe | `check::problems` (ERROR) |
| A style without a `studio` block, an unknown or useless control | `StyleCatalog::parse` (ERROR) |
| An asset that doesn't resolve in some style, resolves to another asset, or a ready asset resolving as not built | `check::coverage` (ERROR) |
| A ready asset without its thumbnail, missing a device tier, or whose source on some tier doesn't load, lacks a region, or has a thin region under 12 vertices | `evani_style_geo::source::asset_problems` (ERROR) |
| A style whose geometry generator isn't built | `check::gaps` (NOTE now; an ERROR for ready assets once WP9's style matrix lands) |
| Click order changing the result; light or quality rebuilding; a material change moving the geometry key | evani-studio tests |
| A generator claiming to work that breaks the mesh contract or isn't deterministic | evani-style-geo test |
| Brand or game names in the asset catalogue's text | the kit's `nothing_the_viewer_shows_names_an_existing_game_or_brand` |
| A game or kit depending on evani-studio or evani-style-geo without `optional = true` | `manifest::OPTIONAL_CRATES` (ERROR) |
