# Asset & Style Studio: work packages (after WP0, 2026-10-30)

Read first: [SPEC.md](SPEC.md) (the owner's spec and its 8 acceptance tests), [CONTRACTS.md](CONTRACTS.md) (what you code against), the mockup `C:\dev\evani-dev\.vibedeck\plans\asset-style-studio\mockup.png`, and in the main checkout `.vibedeck/plans/asset-style-studio/{AUDIT,ARCHITECTURE}.md` and `.vibedeck/research/2026-10-30-asset-and-style-studio-asset-x-style-x-material.md` (ideas only: the papers and articles are never copied).

## For every package

- **Branch**: your own worktree and branch from `studio` (e.g. `studio-wp3`), merged back into `studio` by the orchestrator. Never into m1-foundation, never pushed.
- **Owner's rules for the library**: every asset exists in every art style; "unavailable" is only allowed while a variant generates or an asset is being built, never as the end state. One source model per asset; every style is generated from it, hand-made touch-ups only where generation falls short (as `assets/styles/<asset>/<style>.override.*`, under the same checks).
- **Engine rules**: bytes first; build-time on/off strict (studio code only behind `material-studio` / the kit's `studio`); our own code only (no new mesh, voxel or UI library); all content original; WebGPU only; no `unwrap`/`expect` outside tests; a new dependency pinned `=x.y.z` with a reason in DECISIONS.md; every tunable a registered param `kit.<id>.<name>`; deterministic (no clock, no `rand`).
- **Errors become tools**: every bug you fix ships a test or check that would have caught it.
- **Contracts are frozen**: `crates/evani-studio`, `catalog/assets/assets.json`'s classes and categories, the styles' `studio` block format. A change goes through the orchestrator as one `CONTRACT:` commit. You may edit only the lines your package lists below.
- **Shared files**: `Cargo.toml` (workspace) and `DECISIONS.md` take additive edits only (a new member line, a new section at the end); `catalog/assets/assets.json`: only your own assets' rows.
- **PC safety**: `resource_check` first; `CARGO_TARGET_DIR=<scratch>\evani-target\<your branch>`, `TEMP=<scratch>\evani-tmp\<your branch>`, `CARGO_BUILD_JOBS=2` (1 under two free cores); builds through `node "<Evani.App tools folder>/run-low.mjs" <cmd>`; long commands wrapped in `node <scratch>/evani-stallfix/watch-run.mjs <cmd> 2>&1 | tee <log>`; headless muted browsers only (`--headless=new --mute-audio`, a full `--user-data-dir` you made, pointer lock / fullscreen / keyboard lock stubbed); stop only your own processes, by PID.
- **Done always includes**: `cargo xtask check --quick` while working, `cargo xtask check` clean before hand-over (the viewer's known review-card lines aside), your package's tests, a local commit per step, and a hand-over listing what still blocks the B0 gate.
- **Captures** (visual packages): `.evani/shots/<name>/`, at 700 px, 1280 px at 125 % and 150 %, and 2560 px; nothing overlapping, clipped or squeezed.

## Order and parallel groups

```
WP0 (done) ──┬── Wave A, in parallel now ──────────────────────────────┐
             │   WP1 kit integration      WP2 asset loader              │
             │   WP3 geometry generators  WP5 screen passes             │
             │   WP6 page (UI shell)      WP7 cow + dog models          │
             │   WP8 proof props                                        │
             └── Wave B, once WP2's source reader and WP1's bridge land:│
                 WP4 studio scene + region materials                    │
                 Wave C, once WP1-WP4 and WP6 land:                     │
                 WP9 style matrix + acceptance harness ─── B0 gate (owner) ── WP11 horse fur join ── WP10 library batches
```

The studio is proved on the plain horse first (orchestrator's call, 2026-10-30). The horse's fur and coat-chart drawing is fitted to the per-material shaders in WP11, after the B0 gate and **before** the library batches, so the realistic-style horse has its fur before B1 starts.

If the PC only takes about six builders at once, start WP3, WP7, WP2, WP6, WP1 and WP8 (the long poles first) and WP5 as soon as one finishes. WP4 can start early on the test shapes (Passthrough + WP8's code sources) and pick up the horse when WP2 lands.

| WP | Name | Size | Worker | Starts | Serves tests |
|---|---|---|---|---|---|
| 1 | Kit integration | M | opus-medium | now | 4, 5, 6, 7, 8 |
| 2 | Asset loader + `belongs_on` fill | M-L | opus-high | now | 1, 2, 3, 5 |
| 3 | Geometry generators | L | opus-xhigh | now | 1, 2, 3, 4 |
| 4 | Studio scene + region materials | M-L | opus-high | after WP2 source, WP1 bridge | 1-7 |
| 5 | Screen passes | M-L | opus-high | now | 4 (pixel, paper styles) |
| 6 | Page (UI shell) | L | opus-high | now | all (the user drives them here) |
| 7 | Quadruped generator: cow, dog | L | opus-high | now | 5 |
| 8 | Proof props: wall, roof, patch, shape sources | S | sonnet-medium | now | auto-match examples |
| 9 | Style matrix + acceptance harness | M | sonnet-high | after 1-4, 6 | all 8, proven |
| 11 | Horse fur and coat chart on the per-material shaders (the hair-v2 join) | L | opus-high | after the B0 gate, before WP10 | realistic and painterly styles |
| 10 | Library batches B1-B11 | XL | per batch | after WP11 | — |

---

## WP1: Kit integration

**Goal.** The studio record drives the existing viewer machinery: params, lights, the material catalogue's block, memory.

**Owns.** `kits/evani-kit-material-view/src/studio.rs` (and new `studio_*.rs` beside it), the kit's tests for it, `kits/evani-kit-material-view/data/lights.json` (adding Daylight).

**Uses.** `evani_studio::{StudioState, Pick, Resolved, styles::{CONTROLS, control_param}}`; the kit's `catalogue` params (`kit.material_view.cat.open`, the block edits) and `styles` params.

**Build.**
1. Register every (style, control) pair as a param `control_param(style, id)` with the control's range and step; a param change (panel or evani-live `param_set`) becomes `StudioCommand::Control`; the record's change writes the params back (the way `styles::sync_system` does).
2. Light presets: ids `studio`, `daylight`, `sunset`, `night` (add Daylight to lights.json; keep Overcast); `View.light` drives the viewer's light. Quality drives the tier only.
3. Bridge the record's material to the catalogue: the record's material opens that row (`cat.open`) so the recipe stage builds its block; block edits made while a record is open are stored in `Settings.edits` by param name and written back on return.
4. Studio `Settings.camera` from the orbit camera on leave, restored on return.
5. Kit tests through the ECS (headless `App`, feature `studio`): the 8 acceptance scenarios at state level, plus "a light change runs no rebuild" (a build counter on the scene, like `CatScene.builds`).

**Done when.** The tests above pass with `cargo test -p evani-kit-material-view --features studio`; every control is in `schema_get`; changing a control over evani-live moves the record's keys exactly as `resolve` says; `cargo xtask check` clean.

---

## WP2: Asset loader and the `belongs_on` fill

**Goal.** Every B0 asset's source reaches the geometry stage as a `SourceMesh`, and every material says where it belongs.

**Owns.** `crates/evani-style-geo/src/source.rs`, `apps/material-view/src/studio/loader.rs`, the horse's studio source tier in `apps/material-view/assets/models/` (and `stream.json` via `cargo xtask manifest material-view`), `tools/evani-convert` changes if a rest-pose tier needs one, `tools/materials/belongs_on.mjs` (new), the `belongs_on` values in `catalog/materials/materials.json`, the horse row's `status` in assets.json.

**Uses.** `evani_studio::geo::SourceMesh`, `assets::AssetRow`, `PART_ATTRIBUTE` / `CHART_ATTRIBUTE` in source.rs; the engine's `evani-gltf` reader; `Models::handle`. Entry points WP3 built ahead of it (in evani-style-geo, owned by WP3, reusable for every animal): `rest::rest_from_glb` (any tier at its bind pose, skinned meshes with their own bind matrices, with material, chart, anchors and joints per vertex) and `proxy::{mane, tail}` (a solid mane fin and hanging tail from the tier's `anchor:mane_01..05`, `forelock_root`, `tail_dock` and tail joints); `standin.rs` is the horse's source until `source_from_glb` replaces it.

**Build.**
1. `source_from_glb(bytes, asset)`: merge primitives in model space at bind pose, scale to radius 1 about the bounds' centre, region per vertex from `_EVANI_PART`, else from the horse's chart by rules held as data (along / around / distal / glTF material -> region id: coat, leg, ear, mane, tail, hoof, muzzle, eye), else region 0. Never guess past the row's regions.
2. The horse's studio tier: rest pose, unskinned, no clips (the viewer has `anim` off; check refuses a skinned model without it), converted with `cargo xtask convert`; low tier is enough for B0 (about 150 KB). The mane and tail need a solid proxy shell (hair v2 strands are for PBR styles): coordinate with WP7, who generalises the horse scripts.
3. `loader.rs`: one model handle per asset, framing from the source's bounds and the row's `frame`, the "being built" state for a non-ready asset (same id, a label, never another object), and the stand-in while a tier streams is **the same asset's** lowest tier, never the pale ball (`apps/material-view/src/scenes/styles.rs` keeps its ball only for the legacy scenes).
4. The `belongs_on` fill: a script proposes values from `subcategory`, id prefix and `params.shape` (horse_coats -> quadruped.horse, dogs_cats -> quadruped.dog / .cat, livestock by name, roofing -> building.roof, brick and stonework -> building.wall, grass ground -> land.terrain...); classes it needs that aren't in assets.json go to the orchestrator as one `CONTRACT:` request (new classes with their primary assets planned); the result is reviewed row by row in the diff.

**Done when.** A test reads `tests/fixtures/models/horse/horse.low.glb` into a valid `SourceMesh` with every region present and thin regions non-empty; the studio tier passes `check`; every ready material row has a reviewed `belongs_on` or is deliberately generic (a test counts both); the horse row is `ready` and `cargo xtask check`'s coverage stays clean.

---

## WP3: Geometry generators

**Goal.** Real voxel, low-poly, papercraft, clay and yarn shapes of any asset, from its one source.

**Owns.** `crates/evani-style-geo/src/{voxel,lowpoly,deform}.rs` and new private modules there, the crate's tests; the `generator_version` lines in `crates/evani-studio/src/geo.rs` (each turns 1 in the change that makes its generator work); `apps/material-view/src/studio/variants.rs`; `tools/xtask/src/style_bake.rs` (new, plus its one line in xtask's main.rs); the geometry numbers of the geometry-led styles' `studio` blocks.

**Uses.** `GeometryGenerator`, `SourceMesh`, `StyledMesh`, `Geometry`; WP2's `source_from_glb` for the horse (synthetic and code sources until then).

**Build.**
1. Voxel (the hard one, the research's top pick, our own code): conservative triangle-box overlap voxelisation; fill inside; cell size from the asset (radius 1) times `cell`; thin regions keep at least one cell per segment and the whole is one 26-connected piece (legs, ears, mane, tail); each cell takes the region at its centre; `samples` = cell centre; greedy meshing inside one region only, outer faces only; at most 16 parts.
2. Low-poly: quadric edge collapse to about `target_tris` (300-800 for an animal), region borders and thin parts locked, un-indexed flat facets. Papercraft: low-poly plus flat folded panels.
3. Lumpy and ridges: the deforms in `apps/material-view/src/treat.rs` moved here, seeded from the asset id.
4. `variants.rs`: run `Availability::Generate` jobs off the frame (4 ms slices or a worker), an in-memory LRU and an OPFS cache by `GeometryKey` (50 MB cap, regenerate if storage is refused), progress for the page; meanwhile the same asset is drawn plain.
5. `cargo xtask style-bake <asset|--all>`: the build-time twin for voxel grids and low-poly tiers, written as `apps/material-view/assets/styles/<asset>/<style>.bin`, listed in stream.json, keyed by the same hash.

**Done when.** The crate's contract test passes for every generator it turns on; on the real horse source: voxel keeps every thin region (one connected piece, at least one cell per leg segment), low-poly lands in 300-800 triangles, both deterministic byte for byte; a voxel horse generates in under 300 ms natively (release) and stays within 8 KB as a baked grid; `cargo xtask check`'s gap note lists only Proportion.

---

## WP4: Studio scene and region materials

**Goal.** The viewport shows `Resolved`: the right shape, each region in its material and tone, in the style's surface treatment.

**Owns.** `apps/material-view/src/studio/scene.rs` (and `scene_*.rs`), the coat-to-region colour mapping (a fur row's colours and `points` onto `tone: points` regions), the studio's use of `treat.rs` (refactor it to take a `StyledMesh` and a `Surface` if needed; the legacy scenes must keep working), adding `"studio"` to the app's `FAMILIES` with its scene.

**Uses.** `Studio::state.resolve(..)`, `Resolved.bindings`, WP3's `StyledMesh` (or Passthrough), WP2's loader, WP1's catalogue bridge for each binding's material block.

**Build.** One model slot per view replaced in place (`Models::replace_glb`; never one mesh per variant: the 56 custom-mesh cap); at most 16 material slots (one per part); a bay horse: body red-brown, legs, mane and tail black, hooves horn, eyes kept; brick on a horse works; a build counter so a light change provably builds nothing; comparison mode draws 2-4 records side by side.

**Done when.** Headless captures (muted) of horse x {Realism, Toon, Voxel} x {bay, brick} and cow x Voxel look right (critic pass); a test asserts the light change leaves the build counter still, a material change keeps the geometry key, and the drawn model's part count equals the bindings; the legacy scenes' tests still pass.

---

## WP5: Screen passes

**Goal.** Pixel, Pixel HD, paper (Watercolour, Ink, Gothic, Storybook), tilt-shift (Diorama) and silhouette as real screen passes.

**Owns.** New files in `crates/evani-render` behind a new feature `studio-passes` (its Cargo line and one wiring line per hook point), their shaders, `apps/material-view/src/studio/pass.rs`, the pass numbers of the pass-led styles' `studio` blocks.

**Uses.** `Pass`, `PassKey`; the pixel16 / pixelhd looks' pass code (our own engine, reuse is fine).

**Build.** A low-res target with palette cut, snapped camera and id outline for pixel; edge darkening, grain and bleed for paper; depth-based blur band for tilt-shift; fogged dark cut-outs for silhouette. Each a registered param set; switching a pass costs no rebuild.

**Landed (branch `studio-looks`).** `crates/evani-render/src/studio_passes.rs` and `shaders/studio_*.wgsl` (params `render.studio_pass.*`, one apply/back pass pair after the tonemap, nothing drawn in mode `none`); `apps/material-view/src/studio/pass.rs` (the hook and the pixel camera snap). `studio/pass_proof.rs` is a **temporary, dev-only** proof view (live builds): it draws the horse from WP2's loader (`StudioSource`, the streamed studio tier; nothing compiled in) in the record's style so the passes could be captured before WP4. WP4's scene and WP2's loader replace it: removed when WP4 merged (its surface rules, the pixel pass's line, the screen-space engraving ink and the thin ink shell, now live in `studio/scene.rs` `render_of`). It adds no model file, manifest entry or catalogue row.

**Done when.** Pixel and Pixel HD show a horse with no voxel stand-in; each pass within the shader budget (60 KB per module; docs/materials/SHADER_BUDGET.md); a test that toggling a pass never changes the variant key or the model build counter; games without `studio-passes` are byte for byte the same (`cargo xtask build <game> --ship` size unchanged).

---

## WP6: The page (UI shell)

**Goal.** The mockup's studio, working against the kit's commands.

**Owns.** `apps/material-view/src/studio/page.rs` (and `page_*.rs`), the studio part of `apps/material-view/index.html` and its CSS, the one mount line in `web.rs`, persistence (`StudioState::to_json` / `from_json` in IndexedDB, localStorage fallback), removing the legacy hard-coded shape routes from what the studio shows (`StyleView.shape`'s "sphere" default, `Shape::ALL`'s sphere-first menu, the catalogue's `cat.shape` auto ball): the studio path takes its object only from the record.

**Uses.** `Studio`, `StudioCommand`, `Notice`, `Resolved`, `AssetCatalog` (categories, counts, search words), the material catalogue's existing search and tree (`catalogue::Library`).

**Build.** Style bar (all 24, active one clear, "More" on narrow widths); materials bar with search and chips; asset browser (tabs, search, categories with counts, thumbnails); viewport tools (orbit, pan, zoom, reset, render view, material only, wireframe, fullscreen) and the combination label; inspector (material settings for the style, style controls, lighting Studio / Daylight / Sunset / Night, current combination, export, save, presets); variations strip (same asset and style, other materials; incompatible tiles greyed with the reason). Notices as one line (`ShownOn`, `NotApplied` with its "Show on a horse" button, `Substituted`, `NotBuilt`), "being built" and "generating" states. Sized by each panel's space (container queries, a ResizeObserver); long labels end in "…" with a tooltip.

**Done when.** Captures at 700, 1280 at 125 % and 150 %, and 2560 px are clean; every button does something real (a test lists the controls and the command each pushes); reload restores the record, its settings and the saved combinations; a guard test that the studio path names no default shape.

---

## WP7: Quadruped generator, cow and dog

**Goal.** A parametric quadruped generator (ours) that makes the cow and the dog, and can remake the horse.

**Owns.** `tools/models/quadruped/**` (generalised from `tools/models/horse/`: build, anatomy, rig, species JSON files), the cow's and dog's model files in `apps/material-view/assets/models/`, their `tools/blender/rig_check/*.json`, the cow and dog rows' `status` in assets.json.

**Uses.** Blender headless (Python, versioned scripts), `tools/blender/rig_check.py`, `cargo xtask convert`; the region ids in assets.json (the generator writes `_EVANI_PART` per vertex by those ids; legs, ears, tail, horns and mane as separate solid parts so they survive voxels).

**Build.** Species files for cow (horns, udder, tail tuft, heavier build) and dog (muzzle, ears, curled or straight tail); the studio tier (rest pose, unskinned, `_EVANI_PART`) and the rigged tiers; mane and tail as solid proxy shells (also for the horse, with WP2).

**Converter (done in WP7, 2026-10-30).** `cargo xtask convert` on `studio` dropped every custom vertex attribute from the tiers. The hair-v2 chain (16a067b7, 746cc51a, f3699e78) was tried as a cherry-pick and does not apply without the B5 material-recipe region code, so WP7 added the minimal carry `tools/evani-convert/src/carry.rs` instead (`_EVANI_PART` and `_EVANI_COAT` parked in COLOR_1 / COLOR_0 through gltfpack; in a tier `_EVANI_PART` is unorm8 x4, the index in the first channel). **WP11** replaces it with hair-v2's `attributes.rs` and keeps both tags in every tier; **WP2**'s `source_from_glb` reads `_EVANI_PART` as u8 scalar or unorm8 x4 (first channel x 255).

**Done when.** Cow and dog pass rig_check, convert (tier sizes under the horse's), geocheck; WP2's `source_from_glb` reads them with every region present; a critic pass on captures: recognisable cow and dog from 6 views; rows flipped to `ready` and coverage clean.

---

## WP8: Proof props and the code sources

**Goal.** The wall, the roof, the ground patch, and the code shapes, as `SourceMesh` with regions.

**Owns.** `crates/evani-style-geo/src/code.rs` (new: `code_source(shape) -> Option<SourceMesh>` for `wall`, `gable_roof`, `slope`, `sphere`, `cube`), its tests, the wall, roof and patch rows' `status` in assets.json.

**Uses.** `SourceMesh`, the asset rows' regions (wall: face, cap; roof: tiles, gable; patch: ground; shapes: surface).

**Build.** Closed, consistently wound meshes at radius 1 with a region per vertex; thin parts none; UVs not needed (the recipe stage samples by position).

**Done when.** Each code source passes `SourceMesh::validate` and Passthrough; brick previews on the wall, tiles on the roof, grass on the patch (the auto-match tests already expect those assets); rows `ready`; coverage clean.

---

## WP9: Style matrix and the acceptance harness

**Goal.** Proof, not promises: every ready asset in every style, and the owner's 8 scenarios in the real viewport.

**Owns.** `tools/xtask/src/style_matrix.rs` (`cargo xtask style-matrix`, and its line in `check --full` for material-view), `tools/live-check/studio-accept.mjs`, the report under `target/style-matrix/`.

**Build.** For every ready asset x style cell: resolves; the generator works (an ERROR now, not a note); bytes and triangles within budget; identity (silhouette IoU from 6 views against the source: at least 0.8 voxel, 0.85 low-poly); thin parts kept; at most 16 parts; geocheck clean. The harness drives the 8 tests in a headless, muted browser through the page and asserts records, keys and build counters, with a capture per step in `.vibedeck/review/`.

**Done when.** The matrix is green for B0, and the 8 scenarios pass with captures: the B0 gate the owner signs.

---

## WP11: Horse fur and coat chart on the per-material shaders (after the B0 gate, before WP10)

**Goal.** The realistic and painterly horse wears its real fur: the body-chart coat drawing (bay's black points from the chart), fur shells, baked occlusion and the hair v2 comb, working on round 7's one-shader-per-material path.

**Why it waits.** The 8 acceptance tests don't need it, and it touches the shader code other packages change during waves A to C. WP0's trial merge of horse-model (7bb0e6d5) onto matcat-r8-looks gave 11 conflicting files: evani-render `gpu/models.rs`, `gpu/models/vm.rs`, `gpu/pipelines.rs`, `Cargo.toml`; evani-material-recipes `Cargo.toml`, `lib.rs`, `catalog.rs`, `params.rs`, `shader.rs`; tools/evani-guide `materials.rs`; DECISIONS.md. The code was written for the all-in-one model shader; round 7 split it per material.

**Owns.** The join of hair-v2 (88cc4eae, which holds horse-model) into `studio` after the B0 gate, on its own scratch branch first; the engine files above; the horse's tiers re-baked with the comb.

**Build.** Join hair-v2; fit the region chart (`_EVANI_REGION` at its vertex slot), fur shells and baked occlusion into the split pipelines (`ModelSplit` keys, per-material modules); keep every existing material shader within its budget (docs/materials/SHADER_BUDGET.md); the studio's PBR styles (Painterly, Realism) draw the horse's coat with the chart and shells; the voxel and other generated styles keep using the source's regions (WP2), unchanged.

**Done when.** `cargo xtask check --full` clean on `studio`; the hair-v2 and horse-model tests pass on the split path; the B0 acceptance harness (WP9) still passes; captures of the bay horse in Realism and Painterly show the coat chart and fur, critic pass; byte proof per material shader unchanged or within budget.

---

## WP10: Library batches (after WP11)

Per ARCHITECTURE.md section 3.3: B1 farm and pets (quadruped generator), B2 wild mammals and birds, B3 building kit, B4 plants, B5 people, B6 furniture, B7-B8 everyday objects, B9 vehicles, B10 fantasy and sci-fi, B11 the rest. Each batch: planned, built, matrix-checked, reviewed; "keep going" between batches only if the owner says so.
