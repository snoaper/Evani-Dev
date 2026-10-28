# Evani.Model: design overview (first draft, M0)

Status: M0 draft, 2026-10-15, for Chris's approval. Nothing here is built yet; M1 starts after approval. The brief (docs/EVANI_MODEL_BRIEF.md) is binding; the component designs and their reasons are in docs/model_research/ (one note per component, plus the known / ours / why / result table). The schema and tools are in docs/MODEL_API.md.

## What it is
Evani.Model is an original 3D modeler with its own geometry engine, operated by AI agents through MCP tools. An agent states intent (what to make, what it resembles, its style, adjectives per part, relief, hard tests, references); the machine searches many candidates, checks them by maths, and compiles the chosen one into game-ready glTF per tier with LODs, collision, anchors and normal maps. It produces shape and surface relief only: no colour, materials, skinning or animation. It is development-only: only compiled models (and later, optionally, recipes) reach players.

Success is measured by quality (exact, clean, good-looking geometry) and usage efficiency (calls, tokens, seconds, GPU time and bytes per finished model). Target: a finished, tested model in 2 or 3 calls, each returning in seconds, on any creator's machine.

## Where it lives
| Path | What |
|---|---|
| `crates/evani-model/` | The library: intent, styles, templates, part graph, generators, SDF, solver, search, scoring, refinement, priors, edits, relief, meshing, LODs, collision, UVs and tangents, glTF writer, hardware tiers. Native and WebAssembly-clean on its CPU path. |
| `crates/evani-model/data/` | Data only: style profiles (`styles/*.json`), class templates (`classes/*.json`), pattern definitions (`patterns/*.json`), vocabulary and synonyms (`vocab/*.json`), must kinds (`musts.json`). |
| `crates/evani-model/src/wgsl/` | Our WGSL: the field interpreter, volume reduction (silhouettes and measures), ray-march for non-axis views, scoring reductions. |
| `tools/evani-model/` | Native binary: command line (`make`, `edit`, `compile`, `check`, `sheet`, `compare`, `bench`, `describe-system`) and the stdio MCP server (eleven `model_*` tools planned, seven built, including `model_compare`). `compare <intent | .evmodel | model-id | ours.glb> --target <master.glb> [--tier ultra] [--size 512] [--threads N] [--out sheet.png]` aligns both on the ground without rescaling, prints silhouette IoU (front, side, back, three-quarter; mean) and region measures as JSON, and writes a side-by-side sheet PNG (`--out` is a file path). |
| `tests/model_bench/` | The benchmark set: 30 intents, the manifest, reference plan and (from M1) generated reference silhouettes and meshes. |
| `.evani/model/` (gitignored, project-local) | Work state: models, versions, caches by content hash, scorecards, sheets. |
| `library/` (local library folder, decided in M1) | Approved models with provenance, priors per class and style. |

Rules: the library never depends on evani-core, evani-render or any other engine crate or tool (geocheck included, because geocheck depends on both) and never modifies them (engine needs go to CORE_REQUESTS.md). It has its own mesh checker (`meshcheck`, below; decision D17) and its own preview renderer (`preview`, note 21). Dev-only helper tools that read the kart kit (tools/model-bench-ref) may depend on engine crates, because nothing in evani-model links them. Dependencies: wgpu, glam, bytemuck and serde only, unless Chris approves more (plan-decisions.md lists the requests). No third-party source code or model weights in the repository.

## Module layout of crates/evani-model (planned)
| Module | Responsibility | Note |
|---|---|---|
| `intent` | Parse `.evmodel` text and canonical JSON, validate against the vocabulary, report errors with fix lines, canonical hashing | 01 |
| `style` | Load style profiles, resolve adjectives to measure ranges and steps, bevel and proportion rules | 02 |
| `class` | Load class templates, build the default part graph | 03 |
| `graph` | Part graph: parts, roles, anchors, relationships, relief layers, content hashes | 03, 06 |
| `gen` | Part generators behind one interface; external-service adapter (off by default) | 04 |
| `sdf` | SDF graph, Lipschitz bookkeeping, CPU evaluator with gradients, instruction list for the GPU, our own deterministic maths | 05 |
| `solve` | Analytic relationship solving, surface-aware contact, Levenberg-Marquardt for cycles and inequalities | 06 |
| `must` | Registry of must kinds: as constraint, as check; passing and failing unit cases | 06 |
| `search` | Quasi-random sampler, priors warping, the level ladder and its schedules, rungs, interval pruning with CPU settling, early stop on CPU scores | 07, 09, 18 |
| `score` | Volume pass (silhouettes, box, volume, thickness), IoU, proportions, scorecards | 07 |
| `refine` | CMA-ES; silhouette-gradient fit (experimental) | 08 |
| `prior` | Per class and style priors from approvals | 09 |
| `edit` | Phrase to intent diff, local re-search, verification, versions, undo | 10 |
| `retrieve` | Lexical index over docs, vocabulary and recipes | 10 |
| `critic` | Critic packets, verdict intake, two-round rethink | 11 |
| `relief` | Region selectors, pattern library, per-tier split, readability | 12 |
| `mesh` | Adaptive octree dual extraction, validator hook | 13 |
| `meshcheck` | Our own mesh checker: degenerate and duplicate triangles, open and non-manifold edges, flipped normals, self-intersections through a spatial grid; plain arrays in, a sorted issue list out; std only, no engine crates (D17) | 13, 20 |
| `lod` | Per-tier settings, part and layer dropping, silhouette protection | 14 |
| `collide` | Proxies from parts, grouping, fit check | 15 |
| `surface` | Normals, MikkTSpace-convention tangents, charts, packing, exact per-texel bakes | 16 |
| `gltf` | Deterministic `.glb` writer with KHR_mesh_quantization and `extras.evani`; reader for round-trip tests | 17 |
| `hw` | Detection, tiers, budgets, tiling | 18 |
| `gpu` | wgpu device, pipelines, buffers (feature `gpu`, native backends need decision D1) | 05, 18 |
| `hash` | SHA-256 (own, from FIPS 180-4), content hashes | 18 |
| `preview` | Headless preview and sign-off pictures: fast mesh, software rasteriser, exact per-pixel normals, toon bands, our PNG writer, the five sheet kinds | 21 |
| `pose` | Pose helpers. `curl_finger(chain, &Grip)` closes a finger joint chain (knuckle to tip) round a cylinder (axis point and direction, radius, finger half thickness `pad`, per-joint limit `max_bend`, default 100 degrees): each bone hinges about the axis by the smallest exact angle that brings its far joint onto the padded surface. Returns per-bone local rotations (in the parent's rest axes, `world[i] = world[i-1] * local[i]`), world rotations, signed angles, posed joints and a contact flag per bone. Closed form with `dmath`, so the same bits everywhere; a mirrored hand curls the mirrored way. Build time only (0 runtime bytes) | gap: finger curl |

## Data flow
```
.evmodel text / JSON
   │  intent: parse, validate vocabulary, fill seed            (errors: fix lines, ms)
   ▼
class template + style profile + priors + `like` items
   │  graph: default parts, adjectives → measure ranges, relationships, anchors, musts
   ▼
part graph with free parameters (typically 15 to 60)
   │  sdf: each part's generator builds its field nodes; relief geometry layers per tier later
   ▼
level: `explore` number, or `auto` → level from tier + time budget (chosen once, recorded)
   ▼
search (GPU or CPU, the level's fixed schedule; nothing reads the clock but the safety stop)
   │  sample candidates (quasi-random through priors) → solve constraints per candidate (CPU)
   │  rung 0 (32³ volume pass): cheap musts gate, score → keep the schedule's count
   │  rung 1 (64³) → rung 2 (128³ narrow band; chase view projected from the grid) → finalists
   │  GPU interval scores; close calls re-scored on the CPU; early stop on CPU scores
   ▼
deterministic CPU score of finalists, then refinement (CMA-ES) for the schedule's generations
   │  all must tests at full precision → top K scorecards                (model.make returns)
   ▼
pick (or edit → local re-search → verify → new version)
   ▼
deterministic CPU compile per tier                                        (model.compile)
   │  relief split by tier → field + geometry relief → adaptive dual meshing → validator
   │  → normals, tangents, charts, packing, exact bakes (normal, height) → collision proxies
   │  → quantised .glb per tier + metadata (part tree, anchors, recipe, sizes, provenance)
   ▼
checks: our reader, glTF validator (separate tool), our meshcheck, musts; preview sheets on request (note 21)
   ▼
library (local, with provenance) → evani-convert (Stage 3) → game; uploads only by Chris's click
```

## Determinism
**The promise: the same request at the same level gives the same model bytes on any machine.** A request is the canonical intent with its seed, the level, the tool version (which fixes the schedule table) and the content hashes of the data it reads (style profile, class template, vocabulary, prior file, `like` items, reference masks); all are recorded in the scorecard and recipe.

How it holds:
- **No clock in any decision.** The effort is a level from a fixed ladder (32 to 1024 candidates) with one published schedule each (MODEL_API.md 1.8): rung sizes and resolutions, finalists, refinement starts, population and generations, early-stop rule. Budgets are counted in candidates, rungs and generations, never in milliseconds (the only clock use is a safety stop past ten times the budget, which returns an error and no model; MODEL_API.md 1.8).
- **The GPU explores, the CPU decides.** Candidates and their constraints are made on the CPU. The GPU returns each score as an interval (cells within its error bound of the surface counted both ways); decisions that the intervals settle stand, and every close call is re-scored exactly on the CPU, so each keep or drop, early stop, refinement ranking and final order is the one the CPU-only run makes. The GPU error bound is checked with sentinel cells in every dispatch (note 07).
- **A bit-reproducible CPU path:** f32 with fixed operation order and no fused multiply-add, our own transcendental functions, fixed tiles merged in order, parallel results collected in index order, no randomly ordered hash maps on decision or compile paths, fixed solver iteration counts, index tie-breaks, byte-deterministic output, SHA-256 of the files as the model hash.

**What `explore: auto` means.** `auto` picks the level once, before the search, from the compute tier and the time budget only, using each tier's published reference speed, never this machine's measured speed. So two machines on the same tier with the same budget get the same level and the same model, however fast each is; machines on different tiers get different levels and may return different models (each passing its musts). The reply names the level, and passing that level as `explore` reproduces the model anywhere (slower on weaker machines). A machine's tier is stored after calibration, so it does not change between calls. Decision D5; details in notes 07, 08 and 18.

## Tiers and budgets
| Tier | Detection | make (3 s): `auto` level = candidates | edit | kart compile |
|---|---|---|---|---|
| Pro | at least 22 GiB of GPU memory reported (Apple Silicon: the GPU's usable unified memory), high measured throughput | 512 | under 1 s | (not set by brief) |
| Standard | discrete GPU or Apple Silicon (the dev PC's RTX 4060 Ti 8 GB) | 256 | under 1 s | under 10 s |
| Lite | integrated or older GPU, or slow measured | 64 | under 2 s | under 30 s |
| CPU-only | no usable adapter, or forced | 32 | under 2 s | under 60 s |
| Cloud | the same API remotely through Evani.app (after M5) | the remote machine's tier | | |

wgpu does not report GPU memory, so memory comes from the driver's tool where present, a device table, and a 50 ms throughput calibration (note 18). The result is stored per machine and only revisited when the adapter, driver or tool version changes, or on request. A machine slower than its tier's reference speed finishes the same schedule late and says so (`over_budget`); it never returns a result cut short by the clock.

## Limits (honest, from the brief and our design)
- **No colour, materials, skinning or animation.** The 8 karts differ in colour today; that stays in the game (decision D4 covers shape variants).
- **Very thin features** (thinner than the finest tier's cell, for example aerials and wires) use explicit meshes alongside the fields (note 04, 13).
- **Very large scenes** (whole levels) are not fields: Evani.Model makes props, vehicles, structures and stylized organic shapes; scenes are assembled by kits.
- **Detailed organic hero characters** come from optional plug-in generators through evani-convert (TRELLIS.2, OmniPart: licence limits in decision D8); the chibi body is the most organic native target.
- **Fields are bounds, not always exact distances** after blends and domain operations; all tracing and meshing use the recorded Lipschitz bounds.
- **GPU scores are not bit-identical across vendors**: they are used only as intervals; close calls go to the CPU, so they never decide a result on their own.
- **`auto` is repeatable per tier, not across tiers**: two machines on different tiers may return different models from the same intent on `auto`; pass the level from the reply to reproduce one.
- **evani-convert is Stage 3**: until it exists, models are checked by our reader, the Khronos validator and our own meshcheck; M5 waits for it (decision D11).
- **WebGPU has no tessellation**: large relief is baked into each tier's geometry.
- **The game cannot show medium and fine relief yet**: normal maps, parallax and shader micro-detail need engine and look support that is filed in CORE_REQUESTS.md (note 12). Until it exists, relief is judged in Evani.Model's own preview pictures (note 21), and M4's in-game proof waits for it (decision D19).

## Milestones (from the brief)
M0 research (this draft) → M1 compiler (intent, SDF, solver, generators, compiler; a kart intent compiles to valid glTF with LODs, anchors and collision, passing section A) → M2 search (8 karts at IoU ≥ 0.9 within speed targets on Standard and Lite; stop and report if missed) → M3 language → M4 relief → M5 integration and proof. The M1 task list is docs/model_research/plan-m1.md.
