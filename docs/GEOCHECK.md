# Geometry integrity checks (geocheck)

AI-built games often have overlapping, floating, gapped and z-fighting
geometry. Geocheck finds it automatically, reports every issue with where it
is, a highlighted capture and a suggested fix, and blocks a kit's hand-over
until each issue is fixed or tagged as intentional.

It is dev tooling only. Nothing of it reaches a game download: the checker is
a native tool (`tools/evani-geocheck`, `tools/evani-geocheck-2d`), and the
in-game debug view lives in `evani-live`, which only `live` builds contain
(`cargo xtask size-report` and the marker scan for `debug.geometry` /
`live.geometry` in the release wasm confirm it).

| Part | Where |
|---|---|
| Geometry library (shapes, scene, the checks, image checks, report, tags) | `tools/evani-geocheck/src/*.rs` |
| Runner and kit adapters | `tools/evani-geocheck/src/main.rs`, `src/cli/` |
| 2D checks (tile and sprite games) | `tools/evani-geocheck-2d` (its README) |
| Live debug view `debug.geometry` | `crates/evani-live/src/geometry.rs`, LIVE_API.md 6.27 |
| Prevention helpers | `evani_render::place`, `evani_render::spline`, `RenderWorld::draw_decal` |

## Running it

```
cargo xtask geocheck kart-racer            # same as: cargo run -p evani-geocheck -- kart-racer
cargo xtask geocheck kart-racer --no-render
```

Options: `--title-s 40` (seconds of the title screen's attract race),
`--race-s 30` (seconds of an autopilot race), `--every 6` (sample every n-th
frame), `--no-render` (skip the live captures: no gap, flicker or highlight
shots), `--port 8095` (live bridge port), `--highlights 40` (how many issues
get a highlighted capture), `--keep-raw` (keep the raw captures), `--zfight`
(the native run and the z-fighting geometry test only, see below; with it,
`--report-only` and `--sample N`, see "Fast repeats").

`cargo xtask geocheck kart-racer --zfight` writes
`.vibedeck/geocheck/<app>/zfight/`: `zfight.json` (the depth setup, the
formula, every patch with its objects, kind, world position, overlap area,
separation, the depth epsilon and the separable gap at its view distance,
the cameras that see it and its outline polygon; `groups`: patches with the
same kind and objects, which share a cause and a fix) and `zfight.md` (the
same as readable tables: "By cause", one row per group, then every patch).
A fix's number (how thick, how far apart) clears the largest epsilon over
every camera that sees the patch (`fix_epsilon_m`), and a group's fix is that
of its most demanding patch. Unless `--no-render`, it also captures the live
game (still world, the game's own cameras): `overview.png` from the title
camera that sees the most patches, each group's patches boxed in its own
colour (a key top left names the first eight groups), and `NN-<kind>.png` per patch (up to `--highlights`), each patch
outlined magenta and its box cyan.

#### Fast repeats: the capture cache, `--report-only`, `--sample N`

The scan (the replay plus the depth maths over about 700 cameras) takes
minutes; the report from it takes seconds. So `--zfight` saves the scan once
and reuses it:

```
cargo xtask geocheck kart-racer --zfight                 # scans, or reuses the cached scan
cargo xtask geocheck kart-racer --zfight --report-only   # re-renders the report from the cached scan
cargo xtask geocheck kart-racer --zfight --sample 50     # quick check: 50 fixed cameras, partial report
```

- The cache is `<target>/evani-geocheck-cache/<app>/<key>/` (the cargo
  target dir: `CARGO_TARGET_DIR`, else `target/`; never committed). It holds
  the scan (the static world's instances and custom meshes, the cameras,
  the untagged issues, the "what ran" text and the notes), the debug images'
  raw live captures (`shots/`, one per camera pose) and `manifest.json`.
- The key changes with any of: the build (every file of the crates the scan
  compiles in or captures with: evani-geocheck and its workspace
  dependencies, the app's crate and its dependencies, the live bridge,
  `tools/live-check`, files they `include_str!` from elsewhere, `Cargo.toml`,
  `Cargo.lock`; the toolchain and profile), the scene (replay length and
  sampling, the thresholds), the camera set (all, or the sample) and the
  capture settings (depth setup, viewport, still switch, tool version).
  Not in it: Markdown, the tags in `geocheck.toml` (they apply when the
  report is written) and the report's own code (`cli/zfight.rs`,
  `cli/png.rs`), so a report-formatting change is a `--report-only` away.
- A repeat `--zfight` with the same key prints `used the cached capture
  <key>` and skips the replay and the scan; debug images whose raw capture
  is cached are made without the live game.
- `--report-only` never scans or captures: with no matching cache it fails,
  says what changed since the newest saved scan (files, thresholds, camera
  set) and tells you to run without `--report-only`. Its output is
  byte-identical to the full run's (`zfight_tests.rs`).
- `--sample N` scans a fixed, seeded subset of N recorded cameras (the same
  subset every time; its own key). Its report goes to
  `.vibedeck/geocheck/<app>/zfight-sample/` (never `zfight/`), says
  `"partial": true` with `cameras` N of `cameras_total` in the JSON and
  PARTIAL in the Markdown heading, and is never proof: `cargo xtask check`
  refuses a partial `report.json` or `zfight/zfight.json`.

It writes `.vibedeck/geocheck/<app>/`:

- `report.md`: counts per check (untagged and tagged), how each check ran,
  notes, the worst issues, then every issue grouped by type with its
  location, measure, detail, crop and fix (long groups list the worst 12 and
  point to the JSON).
- `report.json`: every issue: `check`, `kind`, `objects`, `items` (draw
  slots), `at` (world position), `aabb`, `measure` and `unit`, `detail`,
  `fix`, `severity`, `crop` (PNG path or null), `pixels`, `time` (moving
  issues), `tagged` (null, or the tag's reason).
- `crops/`: a highlighted capture per issue that got one (the issue's box
  outlined in its check's colour, in the debug view).
- `views/`: four debug-view captures of the whole world for the critic.

Exit code: 0 when every issue is tagged, 1 when any is untagged, 2 when the
check could not run. `cargo xtask review` reads `report.json` (its
`geo_issues` check).

### Kit adapters

The library is generic: it reads what a game drew (`RenderWorld` mesh
instances, in draw order) and its cameras. The runner needs a small adapter
per game that runs it natively and records frames. Today only `kart-racer`
has one (`src/cli/kart.rs`): it runs the title screen's attract race, then a
one-player race on the autopilot, snapshots the draw list and camera every
`--every` frames, and supplies the kit's collision limit and a wall-hugging
bot through the kit's own `kart::step`. Any other app gives a clear error.
New 3D kits add an adapter alongside (the same shape: record frames, name the
roles of moving parts, give the collision limit); 2D kits write a scene dump
for `evani-geocheck-2d` instead.

### Games outside the engine

A game kept in its own workspace (`cargo xtask build --app-dir`) can't add an
adapter inside this runner, so its kit brings its own, and

```
cargo xtask geocheck --app-dir ../my-game/app [options]
```

runs it in the game's workspace, writing to
`<game workspace>/.vibedeck/geocheck/<id>/` (where `cargo xtask check
--app-dir` reads it):

- `examples/geocheck_dump.rs` in the kit: the 2D route below, unchanged (the
  dump runs in the game's workspace, `evani-geocheck-2d` in the engine; tags
  from `<app>/geocheck.toml`, else the kit's).
- `examples/geocheck.rs` in the kit: a 3D adapter, run as `cargo run --release
  -p <kit> --example geocheck -- --out <report folder> --app <id> [the other
  options]`. It runs the game natively, builds what it drew into the
  library's `scene::Scene` and runs the library's checks (`checks::*`,
  `zfight::zfight`, `tags::apply` with its tags), then writes `report.json`
  and `report.md` (`report::Report::to_json` / `to_markdown`) into `--out`.
  Exit 0 when every issue is tagged, 1 when any is untagged, 2 when it could
  not run, as the runner. It depends on `evani-geocheck` by path with
  `default-features = false` (the library only; same engine checkout).

With neither, the command is an ERROR naming both. With both, the 2D dump is
used, as for a game in `apps/`.

### 2D games

```
cargo xtask geocheck 2d-platformer
```

When the game's kit has `examples/geocheck_dump.rs`, the same command takes
the 2D route: the example plays the level with the kit's bot and draw system
and writes `.vibedeck/geocheck/<app>/dump/scene.json` (plus the sprite sheet),
then `evani-geocheck-2d` writes `report.md`, `report.json` and `crops/` next
to it (old crops are cleared first). Tags come from `apps/<app>/geocheck.toml`,
else `kits/evani-kit-<kit>/geocheck.toml`, plus the scene's own `geocheck`
array. Same exit rule: it fails while any issue is untagged. The 3D options
don't apply. The platformer's dump (`kits/evani-kit-2d-platformer/examples/geocheck_dump.rs`)
is the worked example; the scene format and the eight 2D checks are in
`tools/evani-geocheck-2d/README.md`.

#### Draw joins (2D check 8)

A thin line running across a 2D screen (often only at some camera
positions, so it seems to flicker or run along) is usually a broken join:
two sprites that should meet edge to edge, such as the copies of a repeated
parallax strip, land one art pixel apart and the sky shows through. Check 8
(`join_gap`) predicts it from the dumped sprites, not from pictures: per
frame and per world layer it rasterises every sprite rect (coverage) and its
atlas pixels (opacity) on the art-pixel grid and flags gaps of at most
`join_gap_px` (2) with opaque art of the same layer on both sides, at least
`join_line_px` (4) long, counting only the pixels the player sees (not
under opaque art of a nearer layer). Gaps inside a sprite's own art
(outlines), HUD layers and smooth-art scenes are not flagged.

Because a rounding join breaks only at some camera positions, the dump adds a
`sweep`: frames drawn at every whole-art-pixel camera step over a stretch of
the level (the platformer sweeps 64 steps, enough for every parallax factor's
repeat). Fix: snap a strip's offset once and step its copies by whole art
pixels, and snap with `floor(v + 0.5)` rather than Rust's `round` (half away
from zero). Tag an intended gap with `check = "joins"` and
`object = "<layer name>*"`. Found by Fennel's Leap's far and near hills.
The live `debug_geometry` view is 3D-only (it paints mesh instances), so
this check runs through `cargo xtask geocheck <id>` only.

## How the scene is read

- **Static or moving**: an instance unchanged (mesh, transform, material) in
  at least 80% (`static_frac`) of the sampled frames is static scenery.
- **Kinds**: at least 10 m across (`structure`) is a structure (ground, road
  slab, embankment, mountain); thin and flat (5 cm or less, `decal`) is a
  decal (paint lines, patches); the rest are parts.
- **Objects**: kits draw an object's parts close together in draw order. A
  part joins a touching part among the last 12 draws (`window`, most recent
  first) while the group stays within 12 m (`max_object`). Two pieces of one
  run (same mesh and cross-section, long, parallel: kerbs, rails, fence
  rails) never join each other directly, nor do two copies of one part (two
  crates side by side); two finished groups merge only through a run piece
  (a fence rail reaching back to the previous post). Labels are the look's
  material name and the mesh (`kerb_white (cube)`); in the kart racer the
  material names are the kit's role names, so paint lines and rails show as
  `kerb_white` or `kerb_red`.

## Custom meshes

Meshes in the app's `MeshLibrary` (swept roads, kerbs, walls, imported or
generated models) are registered with the checker's `Shapes`
(`Shapes::add_library`): the CLI adapter snapshots the library at the end of
the run, the live view whenever its revision changes. An instance whose mesh
is unknown is never dropped silently: it is counted and named in the report's
notes (`Scene.skipped`; live: `notes`). If the library changed during a
recording, the report says custom meshes are checked as they were at the end.

- **Surface checks** (rays for floating and camera, coplanar faces, mesh
  validation) use the mesh's triangles as they are.
- **Overlap volume** needs convex solids. A mesh made by `spline::sweep` is
  recognised by its ring layout and split into one convex piece per
  ring-to-ring segment (a twisted quad is split along its convex diagonal);
  the volume of a pair is summed over the pieces, and the pieces of one mesh
  are never compared with each other, so touching segments are not
  overlaps. A segment that is still not convex (a very tight bend) is left
  out and counted: the overlap volume is then a lower bound and the report
  says how many segments were skipped. Any other closed concave mesh is a
  surface only (no overlap volume) and the report names it. Open concave
  surfaces (ribbons) need no note.
- **Mesh validation** runs on every custom mesh in use. A closed sweep must
  be manifold. Open edges are reported unless the adapter declares them
  intended with `Shapes::allow_open(id)` (the plane is always allowed). The kart
  adapter declares the item box's decor (`Recording::open_meshes`): a flat sheet
  of paint (the "?" and rivets) that stands off each face, so it is open on
  purpose; its pieces still must not overlap, which a test checks.

## Decals

Everything drawn with `RenderWorld::draw_decal` is its own kind (`Decal`),
whatever its thickness (thin items of 5 cm or less in the ordinary draw list
are decals too). The CLI snapshot records where the decals start in the draw
list (`Snapshot::decal_from`); the live view knows it from `rw.decals`.

- In the **z-fight** test a decal is drawn `DECAL_DEPTH_OFFSET` closer in
  screen depth (about `4e-5 * z^2` m at view distance `z`), and the test
  counts that offset. A decal on or above its surface never fights it. A
  decal below its surface (a strip following a smoother curve than the
  road's flat panels) fights where its offset just cancels its gap:
  `decal behind surface`. Two decals on one plane get the same offset and
  fight each other: `decals on one plane`. A decal drawn as a thin slab
  (a flat cube) has an underside that gets the same offset as its top, so
  from far off the dark underside shows through: `thin slab` (see "The
  depth maths"). A decal needs no underside.
- A decal is skipped in the **overlap** test.
- A decal must lie on a surface: **decal off surface** (under check 2, yellow).
  From several points of its mid-plane, rays go along its thin axis both ways;
  the nearest non-decal surface must be within 3 cm (`decal_gap`) of the
  decal's faces. A decal sunk into the surface, or a little proud of it, is
  fine. One that floats higher, lies over nothing, or is buried deeper than
  its own thickness plus 3 cm (so it can't be seen) is an issue.
- In the live view decals are tinted like any other instance, and `items` in
  an issue index the meshes first and then the decals.

## The eight checks

| # | Check | How it works | Limits |
|---|---|---|---|
| 1 | **Static overlaps** (red) | Exact convex intersection volume of every pair of solid static items of different objects (broad phase on an 8 m grid), above 0.002 m3 (`overlap_volume`). A part sunk into a structure up to 0.1 m (`sink`) is contact. Two run pieces overlapping where they meet are a **seam overlap**, checked even inside one object. Overlaps buried inside a third solid (rails meeting inside their post) are skipped. Kinds: `prop overlap`, `seam overlap`, `sunk into structure`, `structure overlap`. | Built-in meshes are convex; a custom swept mesh is split into convex pieces per segment and measured exactly; any other closed concave custom mesh is a surface only, and the report says so (see "Custom meshes"). Decals are skipped. Parts of one object may overlap by design and are not checked against each other (except run pieces). |
| 2 | **Floating objects** (yellow) | Rays straight down from the base of every object that isn't a structure (five per base part); a gap above 3 cm (`float_gap`) is `floating`, nothing below at all is `over nothing`. Decals are judged by their own test: a decal more than 3 cm (`decal_gap`) from any surface, or buried in a solid, is `decal off surface` (see "Decals"). | Objects meant to fly need a tag. Leaning or hanging objects are judged by their lowest parts only. |
| 3 | **Gaps and cracks** (magenta) | In the live game with the sky magenta: a calibration shot straight up reads the real sky colour, then 16 views (8 bench-orbit poses, 8 low track stations) at 4096 px wide. Sky seen where a view ray points down onto the ground footprint (the box of the structures at ground height) within the drawn distance (95% of the 1000 m far plane), and still does 2 pixels higher in the image, is a gap; the report box and crop cover only those pixels. Each view gets one more shot with a green sky: a pixel must follow the sky in both, so a bright magenta object is not a hole. A gap that is a small enclosed island of sky (up to 600 px) is reported as a crack. Open mesh edges that aren't intended borders come from check 5. | Only sees what those 16 views see. Sky past the ground's edge is the world edge (review item 14), not a gap; sky past the far plane and at the horizon (a pixel of pose error there moves the ground hit by hundreds of metres) is ignored, so a hole seen only at a grazing angle far away is missed. Sky between objects at or above the horizon is never a crack. |
| 4 | **Z-fighting** (cyan) | Geometry, from the renderer's depth maths (see "The depth maths"): faces of different items parallel within 10 degrees (`zfight_angle`) whose overlap, seen from each recorded game camera (title, race; the live camera in the debug view) out to 150 m (`view_distance`), is closer in depth than the depth buffer can separate there. The epsilon comes from the engine's projection (near 0.1 m, far 1000 m, reversed-Z, see "The depth maths"), its depth format (Depth32Float) and the decal depth offset, with no tuned gap. A patch must be at least 4 px (`zfight_px`) and a pixel wide on screen, face the camera (back faces of a closed mesh are hidden by its own front faces, convex or not: two touching kerb runs, each a closed mesh of separate pieces, never fight at their hidden end caps; an open mesh's back faces still count), and be in sight (no third item in front). Faces with the same material and colour facing the same way are skipped. An item's own faces that face opposite ways (a thin slab's top and underside) are checked too, because the engine draws back faces and lights them dark. Kinds: `coplanar faces`, `near-coplanar faces`, `decal behind surface`, `decals on one plane`, `thin slab`. Movers and runtime decals are checked per frame from that frame's camera. Render: each view is shot at jitter (0,0), (1,0), (0.5,0) and (0,0) again; pixels that change where plain geometry would only shift are flicker, except on an object's outline (see "Flicker and outlines"). Pixels that differ between the two (0,0) shots (things moving between captures) are masked out first. | Only the recorded cameras (and the live one): a replay or photo camera needs its own poses. The render test catches fights at the 16 views' distances only; the geometry test is the main one. Shading that flickers without two surfaces (an outline or shadow artefact) is not geometry: the render test or a capture shows it. |
| 5 | **Mesh validation** | Every mesh the game draws, validated as a reusable library (`meshcheck::validate`, for imported, converted and generated meshes too): degenerate and duplicate triangles, open edges, non-manifold edges, flipped and inside-out normals, self-intersections. The plane is open on purpose. | Self-intersection test is O(n^2) on the triangle count with a box prefilter; fine for game meshes, slow for scans. |
| 6 | **Moving clipping** | Over the sampled frames: movers deeper than 0.2 m (`penetration`) and 0.02 m3 into static solids, a mover crossing a solid between two samples (tunnelling), movers leaving the world, and a character inside its vehicle (kart: the helmet against the tub, nose, wing, bumper, seat and tyres). Repeats of one mover against one solid join into one issue (worst frame). | Sampling every 6th frame (10 per second) can miss a short clip; `--every 1` is thorough. Effects (boost flames, goo) are movers too: tag them if they are meant to sink in. |
| 7 | **Collision mismatch** | The kit's collision limit (kart: a kart's side stops at `half_width + verge - kart_radius` plus its drawn half-width) is compared with the first drawn wall: probes every 2 m on both sides (tolerance 0.6 m, `collision_tol`): `mover enters the wall` (a wall drawn inside the limit: karts drive into it), `invisible wall` (stopped short of, or with nothing, drawn), `missing wall` (a wall drawn with no collision). Neighbouring probes of one kind join into one issue. Then a bot drives a lap pressed against each wall through the kit's own movement code: `bot inside a wall`, or blocked with nothing drawn near it. | Needs the adapter to state the collision rule; kits with physics colliders would compare collider shapes with the drawn meshes instead. |
| 8 | **Camera clipping** | The near plane (eye, corners, edge midpoints, centre) tested against static solids along the bench orbit every 0.5 s, and every recorded game camera against the statics and that frame's movers. | Only the recorded cameras; a replay or photo camera needs its own poses. |

### The depth maths

Two surfaces fight where the depth values they write at one pixel differ by
less than the error in those values. Everything comes from the renderer
(`DepthSetup::engine`, checked against `pipelines.rs`, `frame.rs` and
`forward.wgsl` by a unit test, so a change there fails the test):

- Depth of a point at view distance `z`: `d = a + b / z` (`a`, `b` from the
  projection: near 0.1 m, far 1000 m, reversed: near is depth 1, far 0, set
  by `evani_render::depth`). A decal is drawn `off` = `DECAL_DEPTH_OFFSET`
  closer, which is `t * off` in depth with `t` = `depth::TOWARD_CAMERA` (+1
  reversed, -1 not). The hardware depth bias is only on
  the shadow pipeline.
- Separation at a point of face A: the eye ray meets B's plane;
  `delta = (d_B - off_B) - (d_A - off_A)`. It is affine in screen space, so
  the patch where it is small is cut exactly.
- Error: `eps = step(d) + xf_A + xf_B + snap_A + snap_B`. `step` is the
  depth format's spacing at `d` (Depth32Float: one f32 ulp). `xf` is f32
  rounding in the vertex transform (grows with distance from the world
  origin). `snap` is the rasteriser's 1/256-pixel vertex snapping times the
  plane's depth slope (larger at grazing angles).
- It fights where `|delta| < eps`. In metres along the normal the smallest
  separable gap is `eps / k`, `k = |b| (r.f) / (z^2 |n.r|)`; the report gives
  that and the decal offset in metres at the patch.

Measured on the engine's setup: about 2 mm at 20 m seen head-on (vertex
rounding dominates), growing with distance squared and at grazing angles.
The decal offset is about 67 depth ulps, so a flush decal is safe.

**Thin slabs.** The camera pipelines cull no faces and the forward shader
lights a face by its own normal, so a back face is drawn, and dark. An
item's top and its underside are a pair like any two items: where the item
is thinner than `eps / k` the underside shows through the top as a dark
speckle that moves with the camera (`thin slab`). It hits decals drawn as
flat cubes hardest: the offset pulls the underside forward as much as the
top. Faces of one item that face the same way never pair. The unit test
that guards the depth setup also checks that no pipeline sets `cull_mode`
and that `forward.wgsl` has no `front_facing` (`DepthSetup::back_faces`).

The epsilon is a bound: every rounding term at its worst, added. A pair
inside it can fight, and at some camera or sub-pixel offset it may; the
patch a capture shows is often smaller. Sugarpeak Rally's lollipop rings
(6 cm apart, flagged at 70 to 150 m against an epsilon of 7 to 23 cm) did
not visibly fight in the title camera captures, while its 1 mm road patches
(flagged as `thin slab`) did, and so, more faintly, did its grass blobs (up
to 12 mm thick, also `thin slab`: the underside's fan shows through the top
as dark spokes).

Each patch is clipped from the two faces' overlap in A's plane, per viewer,
to the near plane, the screen and `|delta| < eps`; the report gives its area,
smallest and largest separation, view distance and the cameras that see it.
The old test (a 5 cm gap within `depth_k * z^2`, `zfight_area`, 1.8 degrees)
is gone: it had a tuned constant and missed slope and decal cases.

### Flicker and outlines

Under sub-pixel jitter two different things change a pixel: z-fighting
(inside the patch two surfaces share, the depth test picks the other
surface) and aliasing (on an object's outline, the coverage changes: a thin
stick, a stripe edge, a kerb edge, a rim). The flat-area test only drops
edges visible in the (0,0) shot; a stick thinner than a pixel can be
invisible there and appear in the half-pixel shot, so it looked like a
fight.

Each changed pixel is therefore also tested against the scene itself: rays
on a quarter-pixel grid up to 1.5 px around the pixel centre (the jitter
moves up to a pixel; multisampling looks inside the pixel) each give their
front set: the first item hit, plus the items hit within 5 cm
(`zfight_gap`, along the ray) behind it on a face parallel to it (within
the geometry test's 1.8 degrees), decals included. If the set is not the
same at every sample, or the first-hit face turns by more than 30 degrees
(a fold the renderer's edge lines draw; gentler than a smooth-shaded
sphere's facets), the pixel is on an outline and is set aside (the report
notes how many). Inside a fight patch every ray meets the same two
parallel surfaces, so the set is the same pair all round and the pixel
counts. Surfaces that meet at an angle (a start light set into its panel,
a kerb on the road) never merge into one set, so the item seen first flips
across their meeting line and that line is an outline. Clusters are formed and sized
(`min_pixels` 4, unchanged) after that.

Why ray casting rather than an ID or depth capture: the live capture has no
object-ID buffer (the debug view tints by issue, not by item), and adding
one means a renderer change for a dev tool. The scene the checker already
holds gives the same answer and needs nothing from the game. Its limits: it
trusts the recorded scene to match the render (static objects only, which
the still-world switch guarantees), and an outline thinner than a quarter
pixel between grid samples is missed (its coverage change is then at most a
quarter of the contrast). Rays are cached, so neighbouring pixels share
them; only changed pixels are tested.

Highlights: the worst issue of each untagged group first, then the next
worst, up to `--highlights`; each is framed from above and to the side
(`Camera::framing`) in the debug view and cropped with its box outlined.
Moving issues show the static context only (a capture pauses the game).

The debug view analyses the whole draw list in the page before it answers,
which takes tens of seconds in a big world (about 25 s for the kart racer's
2000 instances on the wasm build). The render phase therefore switches it on
once (`debug.geometry` with `on`, `every_s` 30, so it is not repeated),
takes every debug and highlight shot with it on, then switches it off. The
analysis is the same from any camera and the world is held still. The bridge
gives `debug.geometry` with `on` and `capture.frame` with `geometry: true`
91 s instead of 11 s for the same reason (earlier runs lost alternate debug
shots to the 11 s limit).

Overlap and flicker are about static objects only. When a game has a
still-world switch (a bool param; the kart racer's is
`kit.kart_racer.static_world`, named by its adapter), the render phase turns
it on before the gap and flicker views and off at the end: the game holds its
sim and draws no movers (karts, items, their effects, the minimap), and every
shot holds the clock at the same time, so nothing changes between the four
jitter shots of a view. Without one, the game runs on between captures and
whatever moved between the two (0,0) shots is masked out.

## Intentional cases (tags)

A rock sunk into the ground, a balloon that flies: tag it with a reason and
the checks skip it (the report still lists it, as tagged). Tags live in the
level or kit data:

- `apps/<app>/geocheck.toml` and `kits/evani-kit-<app>/geocheck.toml`:

  ```toml
  [thresholds]
  sink = 0.3

  [[tag]]
  check = "overlap"            # a check name, or "*" for every check
  object = "kart_body (sphere)*" # glob on an object label (optional)
  at = [12.0, 0.0, -40.0]      # with radius (optional)
  radius = 3.0
  reason = "gumdrops are half-buried sweets on purpose"
  ```

- a `"geocheck"` array in the kit's `data/*.json` (same fields per entry).

Check names: `overlap`, `floating`, `gap`, `zfight`, `mesh`, `moving`,
`collision`, `camera`. A tag without a reason, or with an unknown check, is
an error.

### Thresholds

`[thresholds]` overrides any of: `structure` 10, `max_object` 12, `touch`
0.15, `window` 12, `decal` 0.05, `overlap_volume` 0.002, `sink` 0.1,
`float_gap` 0.03, `decal_gap` 0.03, `zfight_gap` 0.05 (render test only),
`zfight_angle` 10 (degrees), `zfight_px` 4 (pixels), `view_distance` 150,
`penetration` 0.2, `penetration_volume` 0.02,
`collision_tol` 0.6, `static_frac` 0.8 (metres, m2, m3).

## The debug view (live)

`debug.geometry` (LIVE_API.md 6.27, MCP tool `debug_geometry`) switches a
colour-coded view on and off in the running game: overlaps red, floating
yellow, z-fighting cyan, everything else grey, the sky magenta (gaps show).
It runs checks 1, 2 and 4 on the frame's draw list at most every 0.5 s,
lists the issues with their fixes, and `focus` frames one. `capture.frame`
takes `geometry: true` to capture in the view. The AI uses it while fixing:
switch it on, focus an issue, fix, capture again.

## Prevention in the engine

- **Snap to ground**: `evani_render::place::snap_to_ground` and `snap_parts`
  (raycast placement: `raycast`, `ground_height`), with a small `sink`.
- **Footprints**: `place::Footprints` reserves each prop's XZ footprint (and
  paths with `reserve_path`) so props can't overlap; `try_place` places or
  refuses.
- **Continuous spline meshes**: `evani_render::spline::{Path, Profile::slab,
  Profile::wall, sweep}` builds a track, kerb or wall as one mesh along a
  path, with no seams to overlap or gap.
- **Decals**: `RenderWorld::draw_decal` draws road lines and patches with a
  depth offset after the surface, so they never fight it.

## In the kit process

Merged into `docs/KIT_PROCESS.md`: step 7 (run `cargo xtask geocheck <app>`;
any untagged issue blocks hand-over; fix or tag with a reason), step 8 (the
critic reviews the debug views and crops for R9 and checks each tag's
reason), "Kit cooperation" (the report and where tags live) and Done-when
item 23, which `cargo xtask review` checks from `report.json`.
