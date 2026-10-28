# Evani.Model API: the .evmodel schema and the MCP tools (first draft, M0)

Status: M0 draft, 2026-10-15, for Chris's approval. Nothing is built yet. Design reasons: docs/model_research/ (notes 01, 06, 10, 12, 18, 19). Overview: docs/EVANI_MODEL.md.

## 1. The intent document (.evmodel)

### 1.1 Example (the brief's kart, verbatim)
```
make: kart
like: library/sugarpeak_kart
style: toon_chunky
parts:
  chassis: { stance: low, wide }
  rear_tyres: { size: oversized }
  engine_cover: { shape: bubble, exhausts: 2 }
  seat: { type: bucket }
  nose: { shape: blunt, rounded }
relief:
  tyres.outer: tread(chevron, depth 4mm, pitch 2cm)
  chassis.edges: wear(chips, depth 1mm, density 0.3)
must:
  fit_box: [1.3, 0.8, 0.7] m
  wheels_clear_chassis_at_steer: 30 deg
  seats: chibi_driver_v2
match: { side: ref/kart_side.png, front: ref/kart_front.png }
explore: auto
return: 3
```
(`seed` is omitted here, so it is derived from the document's content hash. The example's `fit_box` is smaller than the kart racer's karts, about 3.1 x 2.2 x 1.27 m; the benchmark karts use the game's scale.)

### 1.2 Fields
Every top-level field from the brief, in order. Only `make` is required.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `make` | class id | required | The class template to start from: `kart`, `character_body`, `building`, `tree`, `rock`, `fence`, `sign`, `crate`, `barrel`, `lamp_post`, `bench`, `door`, `chest`, `sword`, `shield`, `lollipop`, `gumdrop`, `candy_cane` (plus benchmark classes added as templates: `house`, `tower`, `mushroom`, `cloud`, `cactus`, `bush`; and template variants such as `kart/truck_cab`). |
| `like` | ref, or list of refs, or map ref → weight | none | Existing models it resembles: their intent and recipe become the starting point and prior. Refs: `library/<id>`, `bench/<id>`, or a relative path to another `.evmodel`. Weights 0 to 1. |
| `style` | style profile id | the class's default | Style profile (adjectives → ranges, steps, bevel and proportion rules), for example `toon_chunky` (a profile on the catalog's `toon` look). |
| `parts` | map part name → part spec | template parts | Adjectives, optional numbers and relationships per part (1.4). Unlisted template parts keep their defaults. |
| `relief` | map region → pattern call, or list of calls | none | Relief layers on regions chosen by meaning (1.5). |
| `must` | map must kind → argument | class defaults | Hard tests (1.6). Each becomes a constraint where possible and a verified check otherwise. |
| `match` | map view → reference | none | References scored by silhouette IoU per view and, for meshes, Chamfer and F1 (1.7). |
| `explore` | `auto` or a level (32, 64, 128, 256, 512, 1024) | `auto` | Search effort (1.8). |
| `return` | integer 1 to 8 | 3 | How many candidates (top K) `model.make` returns. |
| `seed` | integer 0 to 4294967295 | from content hash | Seed for sampling, noise and jitter. The same request at the same level gives the same model bytes on every machine (1.8). |

Optional header (proposal, not in the brief): `evmodel: 1`, the format version, assumed 1 when absent.

### 1.3 Text syntax
- UTF-8. `#` starts a comment. Indentation is 2 spaces per level (tabs are an error).
- `key: value` lines; a key with nothing after the colon opens a nested block.
- Inline maps `{ a: b, c }`: entries are `key: value` or a bare word (a flag adjective, `true` in JSON).
- Lists `[a, b, c]`. A list followed by a unit applies the unit to each item: `[1.3, 0.8, 0.7] m`.
- Quantities: a number with a unit, with or without a space: `4mm`, `2 cm`, `30 deg`, `0.5 rad`, `25%`. Units: `m`, `cm`, `mm`, `deg`, `rad`, `%`. A plain number is a count or a ratio; a count may end in `k` for thousands (`3k` = 3000, `1.5k` = 1500), as in `tri_budget`.
- Calls: `name(arg, arg, key value, key value)`: positional words first, then `key value` pairs (space-separated, quantities allowed): `tread(chevron, depth 4mm, pitch 2cm)`.
- References: words with `/` and `.` (`library/sugarpeak_kart`, `ref/kart_side.png`, `chassis.top`). Quoted strings `"..."` for anything else.
- Words are lowercase with `_`. Unknown words are errors listing the three nearest known words with their one-line meanings.

### 1.4 Part specs
A part spec is an inline map or a nested block. Entry kinds:
| Entry | Example | Meaning |
|---|---|---|
| bare adjective | `wide`, `rounded` | Resolved by the style profile to a step on one of the part's measures. Modifiers: `slightly_wide` (half step), `very_wide` (two steps). |
| measure: adjective | `stance: low`, `size: oversized` | A named measure set to the adjective's range. |
| measure: quantity | `length: 0.6 m`, `ride_height: 9 cm` | Pins a measure: it becomes a constraint, not a search dimension. |
| sub-part: count | `exhausts: 2` | Number of repeated sub-parts (array or mirror pair from the template). |
| `shape:` / `type:` word | `shape: bubble`, `type: bucket` | Chooses a template variant of the part (generator and default ranges). |
| relationship | `rests_on: chassis.top`, `attached_at: chassis.rear`, `mirrored: x` | `rests_on`, `flush_with`, `attached_at`, `centred_on`, `aligned`, `mirrored`, `array_along` (argument: an anchor `part.anchor`, an axis, or for `array_along` a map `{ along: part.curve, count: 5, spacing: 20cm }`). |
| `kind:` generator | `kind: lathe` | Required for a new part (a name not in the template): `rounded_box`, `shell`, `lathe`, `sweep`, `tube`, `loft`, `panel`, `frame`, `planks`, `bricks`, `noise_rock`, `branching_tree`, `blended_body`, `cage`, `external`. |
| `remove` | `wing: remove` | Drops a template part (and its relationships). |
| `from:` ref | `cab: { from: library/truck_kart.cab }` | Copies a part from another model. |

Part groups from the template can be named where a part is expected (`tyres` = all four tyres; `rear_tyres`, `front_tyres`).

### 1.5 Relief
`<part or group>.<region>: <pattern>(<style word>, key value, ...)`, or a list of calls on one region.
- Regions by meaning: `edges`, `cavities`, `up`, `down`, `contact`, `along(<part.curve>)`, `inside`, `all`; and generator regions such as `outer`, `inner`, `rim`, `top`, `sides`, `faces`.
- Patterns (our own library): `tread(chevron|block|rib)`, `knurl(diamond|straight)`, `rivets(row|grid)`, `panel_lines(seams|along)`, `bricks(running|stack)`, `planks(straight|staggered)`, `scales(round|diamond)`, `weave(plain|twill)`, `hammered`, `cracks`, `dents`, `scratches`, `stitching(dash|cross)`, `wood_grain`, `stone(cobble|ashlar|rubble)`, `bark(ridged|smooth)`, and `wear(chips|scratches|dents|cracks)` which follows the plausibility rules.
- Common keys: `depth`, `pitch`, `width`, `size`, `density` (0 to 1, a cap), `angle`, `scale`. All lengths in real units; no UVs.

### 1.6 Must kinds (initial registry; each has passing and failing unit tests, test A)
| Kind | Argument | Constraint or check |
|---|---|---|
| `fit_box` | `[length, width, height] m` along forward, right, up | the model's bounding box must lie inside the box (each extent at most the box's, 1 mm tolerance); a constraint that keeps free measures inside, never scaling the model to fill or fit the box (scaling only when a later option asks for it), then a check that reports any excess |
| `min_size` / `max_size` | map `{ length, width, height }` | constraint, then check |
| `clear` | `{ parts: [a, b], at_least: 2cm }` | check (field distance) |
| `swing_clear` | `{ part, about: part.anchor, angle: 110 deg, at_least: 5mm }` | check over the swept arc |
| `wheels_clear_chassis_at_steer` | angle | kart alias of `swing_clear` for both front wheels at ± angle |
| `seats` | body ref (for example `chibi_driver_v2`) | constraint (place at `driver_seat`) and check (no intersection, clearance) |
| `stands` | `ground` or `none` | default `ground`: rests on the ground plane with a stable footprint; `none` for floating things (a cloud) |
| `connected` | `true` | default on: one solid unless a part is marked loose |
| `symmetric` | axis | constraint (mirror domain), then check |
| `min_thickness` | quantity | check |
| `tri_budget` | map tier → count, for example `{ low: 3k, ultra: 60k }` | compile check |
| `anchor` | `{ name: grip, at: part.anchor, diameter: 3cm }` (`at` and `diameter` optional) | constraint (export the named anchor at `at`) and check (size) |

### 1.7 Match references
Map of view → reference: views `front`, `side`, `back`, `top`, `three_quarter` and class views (kart: `chase`); `mesh: <path>` for a reference mesh (Chamfer distance and F1 reported); `weight: { side: 2 }` optional. Image references are binary masks (paths relative to the intent file). In M1 and M2 only masks written by our own tools are read: 8-bit greyscale PNG with stored (uncompressed) deflate blocks, 255 inside and 0 outside; any other PNG or image format is refused with a fix line naming the reference tool, until our own inflate decoder is planned (no new crate).

**Gate IoU.** The IoU that is reported and that the quality targets use (test C: 0.9 or better) is the CPU score of the candidate's silhouette rendered at 512 px in the item's fixed frame against the 512 px reference mask, per view. The 32 to 128-cell search rungs only rank candidates; their scores are never reported as the IoU.

### 1.8 Explore and the level schedule
`explore` takes `auto` or a level: 32, 64, 128, 256, 512 or 1024 (the number of candidates sampled). Any other integer is rounded up to the next level and the reply says so.

**The promise.** The same request at the same level gives the same model bytes on any machine. A request is the canonical intent with its seed, the level, the tool version (which fixes this schedule table) and the content hashes of the data it reads: style profile, class template, vocabulary, prior file, `like` items and reference masks. The reply's `provenance` lists all of them. Nothing in a search reads the clock: budgets are counted in candidates, rungs and generations. The one exception is the safety stop below, which returns an error and no model, so it never alters or cuts short a returned result. The GPU only ranks: close calls are settled on the CPU, so a GPU run equals the CPU-only run of the same level (docs/model_research/07 and 18).

**Schedules** (draft for M2, which tunes the numbers; any change bumps the schedule version, which is part of the tool version):

| Level | Rung 0 (32 cells) | Rung 1 (64 cells) | Rung 2 (128 cells, narrow band) | Finalists (CPU) | Refinement: starts × population × generations (after early stop) |
|---|---|---|---|---|---|
| 32 | 32 | 8 | 8 | 8 | 2 × 8 × 6 (9) |
| 64 | 64 | 16 | 8 | 8 | 2 × 8 × 10 (15) |
| 128 | 128 | 32 | 8 | 8 | 2 × 12 × 12 (18) |
| 256 | 256 | 64 | 16 | 8 | 3 × 12 × 15 (22) |
| 512 | 512 | 128 | 32 | 8 | 3 × 16 × 20 (30) |
| 1024 | 1024 | 256 | 64 | 8 | 3 × 16 × 30 (45) |

Early stop (after rungs 1 and 2, on CPU scores): the top `return` candidates are the same as at the previous rung and the best exact score rose by less than 0.005. Edits use their own small ladder (16, 32, 64 candidates near the current version) with the same rules.

**`auto`.** Resolved once, before the search, from two things only: the compute tier and the call's `budget_s`. The level is the largest one whose predicted time at the tier's published reference speed fits the budget; at the default 3 s that is Pro 512, Standard 256, Lite 64, CPU-only 32 (edits at 1 s: Pro and Standard 64, Lite 32, CPU-only 16). The reference speeds are data, measured on the hardware-matrix machines in M2. So:
- two machines on the same tier with the same budget return the same model, however fast each one is;
- machines on different tiers may return different models (each passing its musts); pass the level from the reply as `explore` to get the other's result exactly (slower on a weaker machine);
- a machine's tier is stored after its calibration and only re-measured when its adapter, driver or tool version changes, or on request, so one machine never flips between calls;
- a machine slower than its tier's reference finishes the schedule late and the reply says `"over_budget": true`; past ten times the budget the call stops with an error, a fix line naming a lower level, and no model.

### 1.9 Canonical JSON
Every `.evmodel` has one canonical JSON form; MCP tools accept either. Rules: lengths in metres, angles in degrees, percentages as fractions, bare adjectives as `true`, single `like` as a one-item list, calls as objects. The kart example:
```json
{
  "make": "kart",
  "like": ["library/sugarpeak_kart"],
  "style": "toon_chunky",
  "parts": {
    "chassis": { "stance": "low", "wide": true },
    "rear_tyres": { "size": "oversized" },
    "engine_cover": { "shape": "bubble", "exhausts": 2 },
    "seat": { "type": "bucket" },
    "nose": { "shape": "blunt", "rounded": true }
  },
  "relief": {
    "tyres.outer": [{ "pattern": "tread", "style": "chevron", "depth": 0.004, "pitch": 0.02 }],
    "chassis.edges": [{ "pattern": "wear", "style": "chips", "depth": 0.001, "density": 0.3 }]
  },
  "must": {
    "fit_box": [1.3, 0.8, 0.7],
    "wheels_clear_chassis_at_steer": 30,
    "seats": "chibi_driver_v2"
  },
  "match": { "side": "ref/kart_side.png", "front": "ref/kart_front.png" },
  "explore": "auto",
  "return": 3
}
```

## 2. MCP tools
Two kinds of tier, never mixed: `compute` is the machine class (`pro`, `standard`, `lite`, `cpu`, `cloud`), and `tier` / `tiers` are the game's render tiers from catalog/tiers.json (`low`, `medium`, `high`, `ultra`) that LODs and budgets are made for. Registered with underscores (`model_make`); the dotted names (`model.make`) are used in docs and on the command line (decision D10). Replies are compact JSON: names, measures, scores, hashes and file paths; never vertex data. Every error reply is `{ "error": "<code>", "msg": "<one line>", "fix": "<one line>" }`. Lengths in metres (mm precision), scores to 3 decimals.

### model_describe_system
Request `{}`, `{ "compute": "lite" }` (override the compute tier for this session) or `{ "recalibrate": true }` (measure again and store the result).
```json
{ "compute": "standard", "why": "discrete GPU, 8 GB (driver), 1.9 G evals/s",
  "gpu": { "name": "NVIDIA GeForce RTX 4060 Ti", "vendor": "nvidia", "type": "discrete", "backend": "dx12", "mem_gb": 8, "limits": { "max_buffer_mb": 2048, "max_storage_binding_mb": 2048, "max_workgroup": 256 } },
  "cpu": { "threads": 16, "mem_gb": 64 }, "platform": "windows",
  "budgets": { "make_s": 3, "edit_s": 1, "auto_level": 256, "auto_edit_level": 64 },
  "calibration": { "stored": "2026-10-15", "gpu_error_bound": 2.4e-6 } }
```

### model_make
Request `{ "intent": "<.evmodel text or canonical JSON or path>", "budget_s": 3, "compute": null, "sheet": false }`. `budget_s` only chooses the `auto` level before the search (1.8); it never cuts a search short.
```json
{ "model": "kart-7c1e", "compute": "standard", "level": 256, "schedule": 1, "explored": [256, 64, 16, 8], "settled_on_cpu": 11, "early_stop": false,
  "ms": 2870, "gpu_ms": 1910, "over_budget": false, "seed": 2207113,
  "provenance": { "intent": "sha256:…", "style": "toon_chunky@1", "class": "kart@1", "prior": "sha256:…", "like": ["library/sugarpeak_kart@sha256:…"], "tool": "0.1.0" },
  "top": [
    { "id": "k3f2", "score": 0.941, "iou": { "side": 0.93, "front": 0.95 }, "must": { "pass": 6, "fail": [] },
      "off_target": [{ "m": "rear_tyres.radius_ratio", "steps": 0.4 }], "note": "best" },
    { "id": "k1a9", "score": 0.928, "iou": { "side": 0.92, "front": 0.94 }, "must": { "pass": 6, "fail": [] },
      "note": "nose blunter (+0.6 step), lower stance (-0.3 step)" },
    { "id": "k8d0", "score": 0.902, "iou": { "side": 0.90, "front": 0.92 },
      "must": { "pass": 5, "fail": [{ "kind": "wheels_clear_chassis_at_steer", "got": "4 mm at 30 deg", "need": ">= 10 mm" }] } } ] }
```
Must tests are hard gates: a candidate that fails one is only returned when fewer than `return` candidates pass, and then listed last with the measured number and the limit (as `k8d0` above). With `"sheet": true` the reply adds `"sheet": "<path to one combined image>"`.

### model_pick
Request `{ "model": "kart-7c1e", "id": "k3f2" }` → `{ "model": "kart-7c1e", "version": 1, "id": "k3f2", "hash": "sha256:9b1c…" }`.

### model_edit
Request `{ "model": "kart-7c1e", "edit": "wider stance, rear tyres slightly more oversized", "budget_s": 1 }` (or `"edit": { "parts": { "chassis": { "very_wide": true } } }` as an intent diff).
```json
{ "model": "kart-7c1e", "version": 2, "parent": 1, "understood": ["chassis.track_width +1 step", "rear_tyres.radius_ratio +0.5 step"],
  "verified": true, "moved": [{ "m": "chassis.track_width", "from": 1.82, "to": 1.95 }, { "m": "rear_tyres.radius_ratio", "from": 1.31, "to": 1.37 }],
  "coupled": [{ "m": "wheel_hub.rl.x", "why": "attached_at chassis.rear_axle" }], "unchanged_parts": 9, "ms": 640 }
```
A failed verification returns `"verified": false` with the misbehaving measure and the nearest working alternative; the version is not created.

### model_fit
Request `{ "model": "kart-7c1e", "match": { "side": "ref/side.png" }, "budget_s": 3 }` → like `model_make` but refining the current version towards the references; returns IoU per view before and after.

### model_check
Request `{ "model": "kart-7c1e", "tier": "low" }` → `{ "must": { "pass": 6, "fail": [] }, "mesh": { "watertight": true, "manifold": true, "self_intersections": 0, "tris": 2980, "budget": 3000 }, "gltf_valid": true, "relief_readable": { "tyres.outer": "normal map", "chassis.edges": "shader" } }`. With a job id, reports compile progress.

### model_describe
Request `{ "model": "kart-7c1e" }` → part tree with roles, measures (by name, in metres and steps from target) and anchors; or `{ "query": "how do I make the cab boxy" }` → up to 3 retrieval snippets `[{ "src": "MODEL_API.md#1.4", "text": "…" }]`.

### model_sheet
Request `{ "model": "kart-7c1e", "kind": "blueprint" }` (`blueprint`, `raking`, `variants`, `curvature`, `thickness`) → `{ "sheet": "<path>.png", "caption": "front, side, top, chase; problem regions outlined" }`. One combined image per call, drawn headless by Evani.Model itself (docs/model_research/21-sheets-and-previews.md); the same model and kind give the same picture bytes on every machine.

### model_compile
Request `{ "model": "kart-7c1e", "id": "k3f2", "tiers": ["low", "medium", "high", "ultra"], "wait": true }`.
```json
{ "hash": "sha256:4e07…", "ms": 7400,
  "tiers": { "low": { "file": ".evani/model/kart-7c1e/v1/kart.low.glb", "tris": 2980, "kb": 61 },
             "ultra": { "file": ".evani/model/kart-7c1e/v1/kart.ultra.glb", "tris": 41200, "kb": 690, "maps": ["normal", "height"] } },
  "anchors": 9, "collision": 6, "checks": { "must": "6/6", "mesh": "ok", "gltf": "ok" } }
```

### model_calibrate_style
Request `{ "style": "toon_chunky", "adjective": "chunky", "classes": ["kart", "crate"] }` → `{ "sheet": "<path>", "proposal": { "chassis.bulk": { "range": [0.62, 0.8], "step": 0.06 } }, "status": "pending Chris's approval" }`. Approval is Chris's, recorded in the profile's provenance.

### model_save_to_library
Request `{ "model": "kart-7c1e", "version": 2, "name": "sugarpeak_kart_pip" }` → `{ "saved": "library/sugarpeak_kart_pip", "provenance": { "intent": "sha256:…", "seed": 2207113, "level": 256, "schedule": 1, "style": "toon_chunky@1", "prior": "sha256:…" }, "prior_updated": "kart/toon_chunky" }`. It never uploads: publishing to the Evani library on evani.app happens only through Evani.App's publish card and Chris's click.

## 3. Example session (target: 2 calls)
1. `model_make` with the kart intent → three scorecards, all musts passing on the first.
2. `model_compile` with `id: k3f2` → four tier files, sizes, hash, checks.
An edit, a sheet or a describe is a third call only when needed.
