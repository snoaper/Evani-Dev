# evani-model-fur-locks

Build-time tool for Evani.Model (development only, never in a game
download), ported from ChimpDuty's gap module: procedural fur locks. Clumped, drooping ribbons
of hair grown over the fur regions of a meshed model, so a code-built ape's
coat breaks its outline and reads as shaggy fur instead of a smooth skin.
Written from scratch for Evani.Dev and Evani.Model. Lengyel and NVIDIA's
shells and fins, Silva et al.'s curling and clumping fur and Scheuermann's
hair-card talk were read for ideas only; no code was copied.

Dependencies: `evani-model` and `serde_json` (the toolkit's rule). No unwrap or
expect outside tests; fmt and clippy clean with `-D warnings`.

## Feature

| Feature | Adds |
|---|---|
| (none) | `params` only: the `kit.fur_locks.*` registry, `LockParams`, `RegionFur`, `Flow`, `LockError`, `MAX_TRIS`, `MAX_LOCKS`, `SOLID` |
| `locks` | The generator: `FurSurface`, `fur_locks`, `FurLocks`, `Lock`, `Stats`; the bake (`bake`) and the `fur-locks` command line |

A build that does not turn on `locks` compiles no generator code.

## API

```rust
use evani_model_fur_locks::{FurSurface, LockParams, RegionFur, Flow, fur_locks};

// 1. The surface: every instance of every meshed solid, in world space,
//    each triangle labelled with its solid, colour region and part.
let surface = FurSurface::from_solids(&model, &tier_mesh.solids);

// 2. Settings: defaults plus per-region lengths and flows.
let mut p = LockParams::default();              // fur 40 mm, face_fur 18 mm
p.set("kit.fur_locks.droop", 0.8)?;
p.set_region(RegionFur::new("head.fur", 30.0).flow(Flow::Away([0.0, 1.45, 0.05])));

// 3. Grow. One SolidMesh named "fur_locks" (world frame, one plain instance).
let locks = fur_locks(&surface, &p)?;
tier_mesh.solids.push(locks.solid);
```

- `FurSurface::from_solids(&Model, &[SolidMesh])` reads each triangle's
  colour region from `model.parts[tri_part].region`. Mirrored instances are
  flipped across x = 0 with their winding fixed, so both sides grow their own
  locks. `FurSurface::add_mesh(mesh, solid, t, mirror, region_of)` adds any
  other mesh by hand (for example a `.glb` labelled by vertex colour).
- `fur_locks(&FurSurface, &LockParams) -> Result<FurLocks, LockError>`.
  `FurLocks` has:
  - `solid`: `SolidMesh { solid: "fur_locks", mesh, instances, report }`.
    `mesh.tri_part` copies each lock's root-triangle part, so region colour,
    bone choice and the bake follow the fur the lock grows from. `report` is
    `meshcheck::check` with `expect_closed: false`. Locks are open sheets by
    design, so its open edges are expected.
  - `uv` per vertex: across the ribbon (0 or 1) and along it (0 at the root,
    1 at the tip), for a root-to-tip tint and alpha.
  - `strand` per vertex: the unit strand direction, for a Kajiya-Kay style
    highlight.
  - `locks`: root, normal, root triangle, region key, length, gear mask and
    clump leader of each lock.
  - `stats`: counts (locks, triangles, leaders, near gear, skipped under
    gear, clipped at gear, fur area).
- Errors: labels that do not match the triangle count, an index past the
  vertices or the name tables, a non-finite vertex, or a region entry with
  a zero or negative length or width. No fur and a count of 0 both give an
  empty result, not an error.

### What is fur, bare and gear

A triangle is **fur** when `LockParams::regions` has an entry for it.
`<solid>.<region>` (`head.fur`) wins over `<region>` (`fur`). A region named
in `LockParams::bare` (default: face, brow, socket, lip, ear, eye_white,
iris, dark, mouth, teeth, skin, nose) is **bare**: no locks and no boost.
**Every other region is gear**, including the empty "no region".

### Shape of a lock

A lock is a tapered ribbon of `segments` quads (2 x segments triangles: 6, 8
or 10; the default 4 gives 8):

1. Roots are scattered over the fur, weighted by area times region
   `density`, and kept at least `spacing` of the even spacing apart. A root
   with gear straight above it within `cover_mm` (fur under a sleeve) is
   skipped.
2. Length = region `length_mm` x (1 ± `length_jitter`) x
   (1 + (`gear_boost` - 1) x (1 - smoothstep(0, `gear_band_mm`, distance to gear))).
   That is twice as long where fur touches gear by default, fading to the
   plain length 30 mm away.
3. Direction: the region's `Flow` (`Dir`, `Away` from or `Toward` a point,
   or the default `flow_x/y/z`, down and slightly back), projected onto the
   skin and turned by seeded noise (`flow_noise`, `flow_scale_mm`). The lock
   leaves the skin at `lift`.
4. The centre line bends down under `droop` and sideways under `curl`. Each
   lock then pulls towards its clump leader, more towards the tip
   (`clump`). Leaders are picked `clump_mm` apart; a lock only follows a
   leader on the same side of the body. The line is kept above its root's
   skin plane.
5. A line that would pass into gear is shrunk towards its root so it ends
   2 mm short of the gear (at least a fifth of it stays), with the same
   triangle count.
6. The ribbon is rolled about its axis (`twist`) and tapered to
   `tip_width`. Its vertex normals lean 55% towards the skin normal, so locks
   light like the coat.

Draw the solid with a double-sided material (or no back-face culling): a
ribbon is one sheet.

## Budget

Never more than `MAX_TRIS` = 14,000 triangles: at most `MAX_LOCKS` = 1,750
locks of 8 triangles. With 10-triangle locks the cap allows 1,400; with 6,
the count cap of 1,750 gives 10,500 triangles. The cap is applied before
growing and checked again after.

## Parameters (`kit.fur_locks.*`)

| Name | Default | Range | What |
|---|---|---|---|
| `count` | 1750 | 0..1750 | Locks wanted (the triangle cap may lower it) |
| `segments` | 4 | 3..5 | Segments a lock (2 x segments triangles) |
| `seed` | 1 | 0..1e9 | Random seed |
| `length_mm` | 40 | 2..300 | Length for regions named only in JSON |
| `length_jitter` | 0.25 | 0..0.8 | Random length change (share, ±) |
| `width_mm` | 12 | 1..80 | Root width |
| `tip_width` | 0.18 | 0.02..1 | Tip width as a share of the root width |
| `lift` | 0.3 | 0..1 | Rise off the skin at the root |
| `droop` | 0.6 | 0..2 | Gravity bend |
| `curl` | 0.25 | 0..1 | Random sideways bend |
| `twist` | 0.5 | 0..1.5 | Random roll (radians, ±) |
| `clump` | 0.55 | 0..0.95 | Pull of tips towards the clump leader |
| `clump_mm` | 30 | 3..300 | Clump leader spacing |
| `flow_noise` | 0.45 | 0..1.5 | Flow wobble (radians, ±) |
| `flow_scale_mm` | 90 | 5..1000 | Size of the wobble's swirls |
| `gear_boost` | 2 | 1..4 | Length factor where fur meets gear |
| `gear_band_mm` | 30 | 0..300 | Distance over which the boost fades |
| `cover_mm` | 25 | 0..300 | Skip roots with gear above them within this distance |
| `spacing` | 0.55 | 0..1 | Smallest root spacing (share of the even spacing) |
| `flow_x`, `flow_y`, `flow_z` | 0, -1, -0.3 | -1..1 | Default flow direction |

Per region: `kit.fur_locks.region.<key>.length_mm` (2..300), `.density`
(0..10) and `.width` (0.1..4, a factor on `width_mm`). The defaults are
`fur` (40 mm) and `face_fur` (18 mm, density 1.6, width 0.7). Flows are set
in code (`RegionFur::flow`). `LockParams::from_json` and `to_json` round-trip
everything else; other modules' keys are ignored.

## Same bytes

One seeded `Pcg32` for the roots and one for the per-lock choices, a fixed
visiting order, Evani.Model's own `dmath` for every sine and cosine,
integer-hash value noise and no threads. Grid lookups use a hash map, but
candidates are sorted before use, so results never depend on its order.
The same surface and parameters give the same bytes on every platform. A
test compares the bits of two runs, and `fur-locks --check` does the same.

## Bake into a model (build time)

```
cargo run --release -j 2 -p evani-model-fur-locks --features locks --bin fur-locks -- tests/model_bench/characters/frontier_marshal.evmodel --tier medium --set kit.fur_locks.region.hair.length_mm=30 --set kit.fur_locks.count=600 --out marshal.medium.glb --check
```

`fur-locks` compiles one tier of an `.evmodel` with Evani.Model, grows the
locks over its fur regions and writes the `.glb` with the locks as one more
solid, `fur_locks` (each lock keeps the part it grew from, so region colours
and, on a skinned model, skin weights follow it; the root's
`extras.evani.fur_locks` records the count, triangles and seed). `--params`
reads a JSON file of `kit.fur_locks.*` values; `--set` adds or overrides one
(region fields too). `--check` bakes twice and fails unless the bytes match.
It prints one JSON line. In code: `bake::bake_tier` (a compiled `TierMesh`
in, the same tier plus the solid out) and `bake::bake_glb`.

The locks are triangles in the model file: no engine code, so a game's
download only grows by the file's own bytes (the marshal's medium tier:
7,974 body triangles plus 600 locks = 4,800 triangles, 231 KB to 495 KB).
The lock cap sits on top of the tier's budget: turn `kit.fur_locks.count`
down for light tiers. The bake writes the plain mesh (no normal maps).

Measured in ChimpDuty, where this module came from: a code-built bust with
1,750 locks and 14,000 triangles in 44 ms; the chimp v1 glb (132,034
triangles) in 68 ms; both byte-identical on a second run.

## Tests

`cargo test -j 2 -p evani-model-fur-locks --features locks` (10 tests): the
parameter names, ranges and JSON round trip; same seed gives the same bits
and a new seed gives new locks; the triangle cap at 6, 8 and 10 triangles a
lock (and exactly 14,000 at the defaults); the solid's name, instance,
components and normals, and every tip above its root; the gear mask (length
= L x (1 + mask), none far away, over 1.5x at the cuff), no roots under the
sleeve and no lock through it; length per region and per solid, bare skin
giving no boost and density 0 growing nothing; droop lowering tips and
clumping gathering them; bad input as errors; the frontier marshal compiled
by Evani.Model and read with `from_solids`, keeping its regions and parts
(and a mirrored instance landing on the other side); and the bake: the solid
in the `.glb`, skin weights on it, a second bake refused, and the same bytes
from two runs.

## Known limits

- A lock is kept above its own root's skin plane only. In a deep concave
  fold (an armpit) a long lock can still dip into the body.
- The gear boost uses the distance to the nearest gear triangle, so gear
  floating far off the fur (more than `gear_band_mm`) gives no boost.
- Clipping shrinks the whole lock towards its root, so a clipped lock keeps
  its bend but gets shorter. It does not bend around the gear.
