# Destruction: the `evani-destruct` module

Evani.Dev's opt-in destruction module. It rebuilds the mechanics of the destruction in Chris's Hexagon
FPS (`..\Hexagon`, packaged there as `evani/hexagon-concrete-engine`) in Evani's own style:
Rust, deterministic, pooled, drawn through `RenderWorld`. No Hexagon code is copied; Hexagon is plain
JavaScript ES modules on its own hand-written WebGL2 engine (TypeScript sources in `src/`), so only its
behaviour and its tuning numbers carry over.

Chris asked for it himself (2026-10-21: "we have a pretty good destructive engine in Hexagon, follow the
same mechanics and rebuild it for Evani.dev"). By the stage plan it belongs to **Stage 5** (Hexagon is kit
006 there; docs/HEXAGON_PLAN.md); his request brings it forward. See DECISIONS.md.

## 1. What Hexagon does

Hexagon's breakable material ("concrete") is one continuous volume. Metal never breaks.

### 1.1 Damage model: hole spheres

- Every hit cuts a **hole sphere** `[layer, x, y, z, r, seed]` into the concrete. Bullets cut small holes
  (pistol 0.12 m, sniper 0.3 m) centred `holeDepth` (0.04 m) past the surface; rockets (1.5 m) and grenades
  (1.25 m) cut craters with `blastHoles`, one hole on every layer whose height band the sphere reaches.
- A hole removes **all** material inside it, across walls, blocks and floor slabs. The hole list is the
  single source of truth: collision, line of sight, bots, hit tests and drawing all ask the same
  `holed(layer, x, y, z)`.
- Holes are indexed in a square grid of 2 m buckets (46 per side); `holed` tests only its bucket's list.
- Positions are quantised (centimetres, radius in millimetres) so server and clients test the same numbers.
- **Damage memory is capped but never drops a hit.** Per bucket `holeCap` (24), in total `holeMax`
  (1800). A hit inside an existing hole adds nothing. In a crowded bucket the hit grows the nearby hole
  whose enclosing sphere is smallest. When the list is full the field recycles (`makeRoom`): first holes
  lying inside another are dropped, then neighbouring pairs are joined into one enclosing hole (least new
  volume first, preferring joins that eat no standing concrete), and only when no pair can join do the
  oldest go.
- The `seed` (0..63) picks the look of the break.
- Metal near a blast gets visual-only **dents** (`DentList`, 64 max).

### 1.2 Fracture: how a hole looks

Hexagon does not pre-fracture meshes. Its concrete shader discards fragments inside hole spheres (a
float texture lists the holes per bucket) and dresses each rim with Voronoi jags, spalled chips, fine and
long radial cracks (out to 2.7 radii), soot and a bump; crater walls are faceted meshes built per hole.

### 1.3 Support and structural collapse ("concrete never floats")

Re-checked whenever the hole list changes (a version stamp), after the tick's shots:

- **Cover cells.** Cover (walls, pillars, bunkers) is built of 1 m cubes on integer grids. A cell's
  **mass** is the share of its 27 sample points (a 3 x 3 x 3 lattice at 1/6, 1/2, 5/6) not inside a hole.
  Cells with mass at least `cellMinSolid` (0.35) carry load and join 6-connected **groups**; cells with a
  little left go with the first group that reaches them; isolated scraps form groups of their own.
- **Group support.** Under each load-carrying bottom cell, 4 x 4 points are tested: held where the cell's
  foot is not holed and floor (concrete not holed, or metal) is under it. The group stands while the held
  share of its footprint is at least `supportMin` (0.3) **and** its centre of mass (mass-weighted) lies
  within `tipMargin` (0.05 m) of the convex hull of the held points. A small hole under the middle does
  not move it.
- **Tipping.** If the centre of mass hangs past the hull, the group tips over the nearest hull edge, toward
  the open side, pivoting on that edge. If too little holds it, it goes the way its mass leans from what
  support is left. It turns under its own weight (torque `g (ms cos t + mh sin t) / I`), slides off at
  `tipRelease` (0.6 rad), keeps turning up to `tipMax` (1.45 rad) at `tipSpin` (0.8 rad/s, half on an
  edge), falls under `fallGravity` (20 m/s²) and lands when a point under it meets something solid.
- **Floor.** Each level's floor is a grid of 0.5 m samples (nothing / concrete / metal). Floor cut off from
  all metal drops as a slab with any cover on it (cover "rides" the slab). Floor also fails when hanging by
  slivers thinner than `floorSliverMin` (1 m), when less than `floorSupportMin` (0.4) of its original
  connection remains, or when its centre of mass lies more than `floorTipMargin` (0.75 m) past what holds
  it (it hinges down on that edge). Failed floor is cut out with holes and drops in pieces of at most
  `floorPieceMax` (100 m²), at most `floorDropMax` (600 m²) per step; crumbs up to `floorCrumbMax` (1 m²)
  are cut away and vanish.
- **Limits.** At most `fallMax` (12) blocks fall at once; more crumble in place. Below `fallDeathY`
  (-30 m) a block has left the world.
- **Lamps** hanging under concrete tear loose when 2 of their 10 mount points are cut, swing, then drop.

### 1.4 Damage propagation

Damage spreads through the structure, not through a damage-points field:

1. A hit removes material (a hole); nothing has hit points.
2. Removed material lowers cell masses and cuts floor samples, which changes groups and support.
3. Unsupported pieces fall. A landing blasts an **impact crater** of radius
   `min(collapseHoleMax 1.6, collapseHoleR 0.8 * cbrt(blocks))` centred `collapseUp` (0.5 m) above the
   touch-down point, into whatever it lands on, which can fail the floor below in turn: a chain.

### 1.5 Debris physics, lifetime and pooling

Visual only (never gameplay; never sent over the network):

- A **fixed pool** of `debrisMax` (512) chunks in typed arrays, used as a ring buffer: when full the
  oldest slot is reused. No allocation per frame.
- A new hole throws `debrisPerM` (24) chunks per metre of radius (at least `debrisMin` 3, at most 64):
  40 % big tumbling lumps from the middle (bigger the nearer the centre), the rest fast gravel from the
  rim, sizes `debrisSize` 0.025 to 0.42 m. A landed block throws `collapseDebris` (10) per cell upward
  (at most 80) plus a big dust puff; spawn points inside solid are skipped.
- Each step: gravity `debrisGravity` (20 m/s²), short sub-steps (at most 0.12 m each, up to 8) so nothing
  tunnels, per axis x, z, y: a blocked move bounces (`debrisBounce` 0.35 of the speed kept) and friction
  (`debrisFriction` 0.6) slows the other axes and the spin. Slow on a floor (below `debrisRestSpeed`
  0.9 m/s) it **rests**; a resting chunk whose floor is blown away falls again.
- **Lifetime.** `debrisSettleMs` (2500) after coming to rest, or near the end of its life `debrisMaxMs`
  (9000), it sinks and shrinks over `debrisFadeMs` (900) and its slot is freed.
- **Dust**: short puffs (drag, a little gravity, grow and thin out), at most 900.

### 1.6 Frame costs in Hexagon

`holed` is one bucket scan (at most 24 sphere tests). The support check runs only when holes change and
only near holes (`anyHoleNear`); a cell's mass is 27 `holed` calls. Debris is O(pool) per frame with up to
8 sub-steps of 3 `solid` probes per live chunk. Drawing is one instanced draw for all chunks plus the
concrete shader's per-fragment hole loop (at most 48 holes).

## 2. What `evani-destruct` ports

| Mechanic | Ported | How in Evani |
| --- | --- | --- |
| Hole spheres, one `holed()` for everything | Yes | `HoleField::holed`, 2 m bucket grid, quantised like Hexagon |
| Bullet holes and blast craters | Yes | `HoleField::add_hole`, `Structure::blast` |
| Capped damage memory that never drops a hit | Yes | grow-nearest in a crowded bucket; recycle: drop contained, join least-volume pair, then oldest |
| Cell mass from 27 samples, `cellMinSolid` | Yes | `Structure::cell_mass` |
| 6-connected groups, weak cells join a group | Yes | `Structure::groups` |
| Footprint support, `supportMin`, `tipMargin`, convex hull | Yes | `support.rs` (hull + distance) |
| Tipping on an edge, slide off, turn, fall, land | Yes | `Falling::step`, same torque law and constants |
| Floor islands cut off from metal drop with cover riding | Yes | `Structure::floor_islands` + riding cover |
| Floor slivers thinner than `floorSliverMin` carry nothing | Yes | thin-run test on the floor grid |
| Crumbs up to `floorCrumbMax` vanish | Yes | cut away, no falling block |
| `fallMax`, crumble in place, `fallDeathY` | Yes | |
| Impact crater on landing, chain collapse | Yes | radius `min(max, r * cbrt(blocks))`, `collapseUp` |
| Debris pool, ring buffer, sub-steps, bounce, friction, rest, settle, fade | Yes | `Debris`, fixed capacity, never allocates after creation |
| Dust puffs | Yes | fixed pool inside `Debris` |
| Seeded randomness | Yes | `evani_core::rng::Pcg32` (Hexagon: injected mulberry32; its debris used `Math.random`) |
| Floor region split by neck width, `floorSupportMin`, `floorTipMargin` hinge | **No** | Hexagon's territory/region solver (`partDrops`) is about 150 lines of grid flood fills tuned to its sector map. The island + sliver rules cover the common case; added when a game needs hinged floor. |
| Splitting big slabs into `floorPieceMax` pieces, `floorDropMax` per step | **No** | Demo-sized floors never reach 100 m²; a slab falls whole. |
| Several layers, rotating layer frames (the ring) | **No** | Hexagon-arena specific. A game keeps one `HoleField` and `Structure` per moving part and transforms points itself. |
| Shader cut-outs, Voronoi rims, cracks, soot, crater wall meshes | **No** | Evani rule: never edit existing WGSL, and a kit material pass with a hole buffer is planned work for Stage 5 (docs/HEXAGON_PLAN.md). The module draws the 27-sample lattice as voxels instead (below), so what you see is exactly what the rules count. |
| Metal dents | **No** | Visual only, needs the same shader pass. |
| Lamps tearing loose | **No** | Hexagon arena content, not a general mechanic. |
| Network codecs (hole, gone-cell and falling-row strings) | **No** | Evani.Dev's net module carries its own state; the hole list is plain data (`HoleField::holes`) a game can send. |

### 2.1 Where the port differs on purpose

- **Dropped floor is marked gone.** Hexagon keeps a fallen floor tile as "holed everywhere" in its
  damage memory. Evani keeps a `floor_gone` flag per sample instead, so a dropped slab costs no hole
  slots and `holed()` stays as cheap as the hole count.
- **Frames are axis-aligned.** A structure is one grid at an origin with no rotation (Hexagon's ring
  layers turn). A game with a moving or rotated structure transforms points into its frame first.
- **`Debris::react` is new.** Hexagon's arena code reacts to each collapse event by hand; the module
  does it in one call (section 3).

## 3. Module design

Crate `crates/evani-destruct`, an optional dependency of a game behind a same-named feature `destruct`.
Depends on `evani-core` (params, `Pcg32`, `EngineModule`) and `evani-render` (`MeshInstance` for the draw
helpers). No third-party dependencies.

- `HoleField`: the hole list and bucket index; `holed`, `add_hole`, `holes()`, `clear()`.
- `Structure`: one destructible structure: a floor grid (`FloorKind::{Empty, Concrete, Metal}`, 0.5 m
  samples, slab thickness) and cover cells (1 m cubes on an integer grid at an origin). Owns its
  `HoleField`. `blast(p, r)` and `shoot(p, r)` cut holes; `step(dt, tuning, rng, events)` runs support,
  falling blocks and landings at the fixed 60 Hz tick; `solid(p)` is the collision query (floor, standing
  cells, the ground plane) for debris and for falling blocks.
- `Debris`: the fixed pool; `hole(p, r)`, `landed(p, blocks)`, `chunks(...)`, `dust(...)`, `step(dt, solid)`;
  `live()`, `resting()`. `react(events, tuning, rng, solid)` turns one tick's `DestructEvent`s into rubble
  and dust (a landed or crumbled block breaks up into chunks and a big puff, cut-away floor crumbs spill
  a few chunks), so a game does not have to match on the events itself.
- `Tuning`: every number in section 1 with Hexagon's value; `DestructParams::read(&ParamRegistry)` fills it
  from the live params each tick.
- `DestructModule`: an `EngineModule` (label `destruct`) that registers the `destruct.*` params and inserts
  the `DestructParams` handles.
- `draw`: `draw_structure`, `draw_debris` push `MeshInstance`s: each standing cell as its 3 x 3 x 3 lattice
  of 1/3 m cubes (a sub-cube is drawn while its sample point is not holed), each floor sample as a 0.5 m
  tile per slab half, falling blocks with their pivot transform, chunks as tumbling cubes, dust as fading
  low spheres.
- Events (`DestructEvent::{Fall, Land, Out, Crumble, Crumbs}`) go into a reusable `Vec` the caller owns.

Determinism: all stepping is at the fixed tick with a fixed dt; all randomness is a `Pcg32` the caller
passes (a game forks its world `Pcg32`); no clocks.

## 4. Live parameters

All registered by `DestructModule`, owner `destruct`, readable and settable through the live API
(`param.set`). Ranges are in `crates/evani-destruct/src/params.rs` and in the report of `schema.get`;
defaults are Hexagon's values (`Tuning::default`). A change takes effect on the next tick; the sandbox
proof replays its loop from the start when any of them changes, so the result shows at once.

| Param | Unit | What it does |
| --- | --- | --- |
| `destruct.cell_min_solid` | share | Share of a cell's 27 samples left for it to carry load |
| `destruct.support_min` | share | Share of a group's footprint the floor must hold for it to stand |
| `destruct.tip_margin` | m | How far the centre of mass may sit past the held hull before the group tips |
| `destruct.tip_release` | rad | Tilt at which a block turning on its edge slides off and falls free |
| `destruct.tip_max` | rad | Largest tilt a falling block reaches |
| `destruct.tip_spin` | rad/s | Starting turn speed of falling concrete |
| `destruct.floor_sliver_min` | m | Floor joins thinner than this carry nothing |
| `destruct.floor_crumb_max` | m² | Floor pieces up to this area are cut away instead of falling |
| `destruct.fall_gravity` | m/s² | Gravity for falling concrete |
| `destruct.fall_max` | blocks | Most blocks falling at once; more crumble where they stand |
| `destruct.collapse_hole_r` | m | Impact crater radius per landed block (times the cube root of its cells) |
| `destruct.collapse_hole_max` | m | Cap on the impact crater radius |
| `destruct.collapse_up` | m | Height above the touch-down point of the impact crater's centre |
| `destruct.collapse_debris` | chunks/cell | Debris chunks per cell of a landed block |
| `destruct.debris_per_m` | chunks/m | Debris chunks per metre of hole radius |
| `destruct.debris_min` | chunks | Fewest chunks a hole throws |
| `destruct.debris_size_min` | m | Smallest gravel |
| `destruct.debris_size_max` | m | Largest lump (hole debris is also capped at 0.45 x the hole radius) |
| `destruct.debris_gravity` | m/s² | Debris gravity |
| `destruct.debris_bounce` | share | Share of speed a chunk keeps on a bounce |
| `destruct.debris_friction` | share | Share of sliding speed and spin kept on every bounce |
| `destruct.debris_rest_speed` | m/s | Below this speed on a floor a chunk lies still |
| `destruct.debris_settle` | s | Time at rest before a chunk fades |
| `destruct.debris_fade` | s | Sink-and-shrink time of a fading chunk |
| `destruct.debris_life` | s | Longest life of a chunk |

## 5. Using it in a game

```toml
# apps/<game>/Cargo.toml
[features]
destruct = ["dep:evani-destruct"]
[dependencies]
evani-destruct = { workspace = true, optional = true }
```

```rust
#[cfg(feature = "destruct")] { app = app.with_module(evani_destruct::DestructModule::new()); }
```

The game builds its `Structure`s from kit data, calls `shoot` / `blast` from its weapons, `step` from a
`Stage::Update` system and the draw helpers from its `Stage::Frame` system. A game that does not turn the
feature on compiles none of it (zero bytes; see DECISIONS.md for the byte check).

The sandbox proof (`cargo xtask`-free: `trunk build --features destruct-proof`) is `apps/sandbox/src/proof_destruct.rs`.
