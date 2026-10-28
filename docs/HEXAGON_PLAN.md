# Hexagon on Evani.Dev: rebuild plan

Status: plan only (2026-10-15). Questions in section 5 decided the same day (Chris: "Yes, all as recommended"). Nothing here is built. Chris: "rebuild Hexagon in Evani.dev and the graphic level is to be cinematic FPS on same level as latest battle field", "best look on strong PCs, lighter on phones", and "Plan now, build after Stage 3". STAGES.md rule 1 allows planning a later stage, never building it into main, so the build starts only after Gate 3 is signed off and in the stage this plan proposes (section 6).

Every number marked *(est.)* is a planning estimate, to be measured with `cargo xtask bench`, `bench_run` and `cargo xtask build hexagon --ship` before it becomes a target. Nothing in this plan changes the catalog: where the catalog lacks something Hexagon needs, section 7 says so and proposes the change for Chris.

Sources read: the Hexagon repo (`..\Hexagon`: README.md, `src/rules.ts`, `src/runtime.ts`, `src/bot.ts`, `src/client/*.ts`, `game/tuning.json`, `game/manifest.json`, the `evani/` code packs); the prior-art report `.vibedeck/research/2026-10-15-cinematic-first-person-shooter-on-webgpu-what-is-a.md`; STAGES.md, ROADMAP.md, CORE_REQUESTS.md, DECISIONS.md, docs/PLAYGROUNDS.md, docs/KIT_PROCESS.md, `catalog/*.json`, `assets/tiers/*.json` and the crates. `docs/STAGE3_PLAN.md` landed while this was written (merge af49f82); section 3 and the download budget follow its items (S3-1 to S3-17) and its chosen approaches.

## 1. What "Battlefield level" means here

The research report's finding, which this plan adopts: nothing in a browser looks like Battlefield today, and the 10 MB playground package rules out Battlefield's asset volume (photogrammetry, huge streamed worlds). What does carry over, and what this plan builds, is the *lighting and camera* half of that look:

- **Copy (cheaply):** physically based materials with one consistent light model; an analytic sky and atmosphere; distance and height fog, scaled-down froxel volumetric fog with shadowed light shafts on Ultra; cascaded sun shadows; filmic tonemapping and a photographic grade; temporal anti-aliasing with upscaling on Ultra; strong muzzle flash, explosion light, smoke and dust; weapon view models drawn in their own pass.
- **Skip:** Battlefield-scale destruction and 64-player maps, Nanite-class geometry, ray tracing, dynamic probe GI. Hexagon's own destruction (holes, craters, tipping walls, falling floor) is small-scale and already runs in 168 KB, so it stays.
- **Ideas only, no code:** Babylon.js 9 (Apache-2.0) for the pass design (frame-graph passes with declared resources so a tier drops a pass cleanly, clustered light binning, compute volumetrics); Frostbite talks (EA-copyrighted slides) for what to copy. **Maths, code only with attribution:** three.js r183+ (MIT) TRAA, GTAO, SSR effect maths, rewritten in WGSL in `evani-fx`. Splat backdrops (PlayCanvas, MIT) only as a later Ultra option (section 5, question 4).

The quality bar for the High tier is concrete: **High must look at least as good as today's WebGL2 Hexagon** (GGX PBR, 2048² PCF sun shadow, sector point lights, procedural materials with bump detail, 4x MSAA HDR, bloom, ACES, grade, vignette, grain; `src/client/shaders.ts` header). Ultra is where it goes beyond today. Against today, High drops one thing on purpose: film grain, for clarity in a competitive shooter (section 4.2); the fixed grade and a light vignette stay, built into the look's tonemap step.

What a strong PC sees at launch, plainly: on High, real-time shadows from two cascades, many lamp and muzzle lights, ambient occlusion, fog, scorch marks and a filmic grade, but no light shafts, no reflections and no flying concrete chunks (a dust puff instead), because today's effect budget for High runs out; Ultra adds the chunks and reflections; Stage 8 then swaps the reflections for volumetric light shafts unless the costs are revised. Section 5, question 5 is the way to close that gap on High.

## 2. Hexagon's scope: today, and what the Evani.Dev version keeps, changes or drops

### Today (Hexagon repo, MIT, by @snoaper)

A team first-person shooter for ChimpVibe on a hand-written WebGL2 engine, 168 KB total (client bundle `game/client/main.js` 120 KB).

- **Arena** (`src/rules.ts`): built from a seed. A hollow hexagon (inner apothem `A_IN` 14 m, outer `A_OUT` 38 m, so about 76 m across flats) in 6 trapezoid sectors around a bottomless chasm; 3 levels 8 m apart (`FY = [0, 8, 16]`). Top level: 6 sealed sectors, the two bases opposite each other with core zones. Middle level: a ring of 6 sealed cars that turns 60° every ring interval (3 min default, 5 s turn). Bottom level: two halves split by metal walls. Lifts link the levels; the ring is the only route into the enemy base.
- **Materials:** metal never breaks (dents and scorch only). Concrete (cover, pillars, bunkers, every walking floor) takes holes and craters; cover tips over and falls when its floor is shot away; floor islands cut off from the metal frame drop as slabs; debris chunks and dust. The server keeps one hole list (up to 1800) used by collision, line of sight, bots and drawing.
- **Weapons:** pistol (hitscan, 22), sniper (hitscan, 95, zoom), rockets (projectile, splash, 3 m crater), grenades (lobbed, bouncing, 2.2 s fuse, 2.5 m crater).
- **Rules:** first to 25 kills, or hold the enemy core zone for 8 s; 15 min limit. Respawn 3 s, spawn shield 2 s, half self-damage, no friendly fire.
- **Multiplayer:** lobby (Blue and Orange, 4 a side, ready share 75 %, 20 s countdown), bots fill empty slots, deterministic server at 20 Hz with 3 movement sub-steps (so movement is already a 60 Hz step), client prediction, 100 ms interpolation, lag-free ring carry.
- **Client:** GGX PBR with procedural materials, sun shadow map, sector point lights, muzzle and explosion lights, aerial fog, bloom, ACES, grade, vignette, grain; soldiers with a walk cycle and team colours; weapon view models; synthesized audio; HUD with minimap and ring status; mouse with pointer lock and a free-look fallback where the host refuses the lock; touch stick, look drag and buttons; settings (ring interval, sensitivity, invert Y, volume).
- **Library:** parts are already published in the Evani library (the Hexagon Fortress playground and the Hexagon code packs in `evani/`). They are the reference, not code to copy: the rebuild is Rust and WGSL.

### The Evani.Dev version

| Part | Keeps | Changes | Drops |
|---|---|---|---|
| Arena | Same seeded hexagon, same dimensions (14 / 38 m, 3 levels 8 m apart, chasm), rotating middle ring, lifts, sealed levels and the ring as the infiltration route | Rebuilt in Rust in the kit from the same constants; the arena stays procedural (no download for it); level and tuning numbers become JSON data and `kit.tactical_fps.*` params | Nothing |
| Destruction | Holes, craters, tipping cover, falling floor islands, debris, metal dents; the one deterministic hole list | Hole cut-outs drawn by a kit material shader reading a storage buffer instead of a float texture; debris through the catalog `destruction` effect where the tier budget allows, a dust puff otherwise | Nothing in gameplay; visual debris count varies by tier, gameplay holes never do |
| Weapons | The four weapons and their numbers | Hitscan gets recoil patterns and movement spread (the tactical-fps genre's first system); sniper zoom gets a scope view | Nothing |
| Rules | Kills to win, core hold, time limit, respawn, shield | Played against bots in the playground's Play mode; Free roam adds the mode the library requires | Nothing |
| Bots | 4 a side, fill empty slots | Rewritten in Rust; deterministic on the seeded `Pcg32` | Nothing |
| Netcode | Deterministic fixed-step sim, input sub-steps, prediction and interpolation as the design | Runs through `evani-net` in loopback (bots, offline) first, which Stage 5's gate asks for | ChimpVibe's runtime and its state-version upgrades; online play waits (section 5, question 1) |
| Lobby | Team colours and ready flow as the design for later online play | Offline: a single-player menu (team, bots, ring interval, start) | The online lobby until online play exists |
| Rendering | The look of today as the High-tier floor | Evani.Dev WebGPU paths by tier (section 4), look crate in WGSL, PBR material model | The WebGL2 renderer |
| Characters and weapons | Soldiers with team colours, view models | Real models through Stage 3 (glTF, skinning with blending and simple IK, LOD), converted with evani-convert | Procedural soldier meshes (kept only as the fallback while models stream) |
| Audio | Synthesized sounds | Played through `AudioMixer` voices; positional sound is new work | Nothing |
| Input | Mouse with pointer lock and the refused-lock free-look fallback, keys, touch, settings | On Evani.Dev's input and touch layers; the lock is released on Esc, blur, hidden tab and error | Nothing |
| HUD | Health, ammo, weapon, kill feed, minimap, ring status, core progress | Drawn in the canvas with the text layer and Stage 3's UI layer | DOM HUD |
| Size | Small download as a value | About 9 MB package *(est.)* instead of 168 KB, within the 10 MB playground cap | The 128 KiB client limit |

**Free roam** (every playground, docs/PLAYGROUNDS.md): the same fortress, the ring still turning, no bots, no score, timer or round end; the player can still shoot concrete. Falling into the chasm respawns at the base without scoring. `fn free_roam` returns true and a `free_roam_holds` test runs two minutes of scripted input.

**Map scale:** kept as is (section 5, question 3). It was tuned for 4 v 4 and its whole height is 16 m plus the chasm, which suits the tier triangle and draw caps.

## 3. Engine modules Hexagon needs, and where each stands

Status: **Has** (built today, with the evidence), **Stage 3** (an item of docs/STAGE3_PLAN.md), **Filed** (already in CORE_REQUESTS.md for a named later stage), **New** (not filed anywhere; Hexagon's build files it).

| Need | Status | Evidence or what is missing |
|---|---|---|
| App, ECS, schedule, fixed 60 Hz step, seeded RNG | Has | `crates/evani-core/src/time.rs` (`FixedStep`), `rng.rs` (`Pcg32`), `ecs/`; Hexagon's 20 Hz tick × 3 sub-steps maps onto the 60 Hz step |
| Params, formulas, live tuning, bake | Has | `crates/evani-core/src/params.rs`, `crates/evani-live`, `tools/evani-live-bridge` |
| Play mode and free roam | Has | `crates/evani-core/src/play.rs` (`PlayMode`, `free_roam_test`) |
| Keyboard, mouse buttons, gamepad actions | Has | `crates/evani-core/src/input.rs` |
| Mouse look with pointer lock (raw deltas, released on Esc, blur, hidden tab and error; the free-look fallback when refused) | New | No pointer-lock code in `crates/` today. A small evani-core addition; headless tests stub `requestPointerLock` |
| Touch: move stick and buttons | Has | `crates/evani-core/src/touch.rs` (feature `touch`: `Stick`, `Button`, `Zone`) |
| Touch: drag-to-look pad (relative aim on the right half) | New | `touch.rs` has sticks and buttons, no relative look pad; small addition to the same layer |
| First-person camera with its own view-model pass (narrow FOV, separate depth range, never clips walls) | New | `crates/evani-core/src/camera.rs` has poses and paths only; the forward pass has one camera per view |
| 3D forward path, HDR target, tonemap slot, 4x MSAA from Medium | Has | `crates/evani-render/src/gpu/frame.rs` (4x MSAA forward into `hdr`), `graph.rs` (render graph with declared targets), `assets/render_paths.json` |
| Custom meshes (procedural arena) | Has | `crates/evani-render` feature `meshes` (`lib.rs`, `count_includes_decals_and_custom_meshes`) |
| PBR material inputs (base colour, roughness, metallic, emissive) | Has (values only) | `crates/evani-core/src/plugin.rs` `MaterialDesc`; no textures per material yet |
| Textured, normal-mapped models; per-device texture families (ASTC, BC7, fallback); texture memory held to the tier | Stage 3 | docs/STAGE3_PLAN.md S3-3 (feature `models`) |
| Sampling metallic-roughness and occlusion maps in a realistic look | New | S3-2 keeps every metallic-roughness field in the material data, but S3-3's toon look does not sample them; the Hexagon look does |
| A look crate for the cinematic look | New | No realistic look exists (`looks/` has basic, cartoon2d, pixel16, toon). Section 5, question 2 |
| Kit material shader with a kit data buffer and discard (hole cut-outs in colour, depth and shadow passes) | New | Hexagon's `CUT` shader reads a hole table per 2 m bucket (`src/client/shaders.ts`); Evani.Dev has look slots, not kit material passes with their own storage buffer |
| Decals (scorch, soot, dents) | Has (core) | `crates/evani-render/src/world.rs` `draw_decal`; the pooled catalog `decals` effect is New in `evani-fx` |
| One sun shadow map with PCF, blob shadows | Has | `crates/evani-render/src/shadow.rs`, feature `shadows` (evani-fx `shadows`) |
| CPU frustum culling, up to 8 point lights, two shadow cascades, clustered forward with compute light binning, image-based lighting | Filed, Stage 5 | CORE_REQUESTS.md row "CPU frustum culling ... for kits 005 and 006"; ids `cpu-frustum-culling`, `point-lights`, `shadow-cascades-2`, `clustered-forward`, `image-based-lighting` |
| GPU-driven culling, four cascades, volumetric passes, temporal upscale (Ultra) | Filed, Stage 8 | CORE_REQUESTS.md row for Ultra; ids `gpu-driven-culling`, `shadow-cascades-4`, `volumetric-passes`, `temporal-upscale` |
| Compute pass kind and GPU particles | Filed (core-request plan item C) | CORE_REQUESTS.md particles row; `crates/evani-fx/src/particles.rs` is params-only today |
| Effects in `evani-fx` | Partly | Built: `bloom`, `vignette`, `shake`, `shadows`, `particles` (params-only), plus 2D ones (`crates/evani-fx/Cargo.toml`). New: `grade`, `grain`, `chroma`, `fog`, `ssao`, `ssr`, `godrays`, `flare`, `dof`, `motionblur`, `decals`, `destruction`, `volumetric`, `trails`, `baked`, `haze` |
| Analytic sky and atmosphere, auto exposure, temporal AA | New, and not in the catalog | Section 7 |
| glTF reader with meshopt and quantization | Stage 3 | docs/STAGE3_PLAN.md S3-2 (`evani-gltf`, module `models`) |
| Skinned animation: clip runtime, blend of two, masked additive, 1D blend spaces, two-bone and aim IK, GPU skinning (64-joint soft cap) | Stage 3 | docs/STAGE3_PLAN.md S3-7, S3-8, S3-9 (module `anim`); aim IK holds the weapon on target |
| LOD choice and per-tier model files | Stage 3 | docs/STAGE3_PLAN.md S3-4 |
| Text and a lightweight UI layer (HUD, menus, settings) | Has (bitmap text) / Stage 3 (MSDF text, box layout) | `crates/evani-render/src/text.rs` (feature `text`); docs/STAGE3_PLAN.md S3-13a and S3-13b (module `ui`) |
| Model converter and its quality gate (soldier, weapons, props within budget) | Stage 3 | docs/STAGE3_PLAN.md S3-5, S3-6 (`tools/evani-convert`) |
| Detailed assets streamed after the title screen, by tier (`boot` group, priority fetch, Cache Storage) | Stage 3 | docs/STAGE3_PLAN.md S3-4 and S3-11 |
| Per-device texture format and tier LODs, shared asset cache on evani.app | Stage 7 | STAGES.md Stage 7 |
| Audio: one-shots, looping voices with pitch and gain | Has | `crates/evani-core/src/audio.rs` (`voice_start`, `voice_set`) |
| Positional 3D audio (panning, distance) and synthesized sound recipes | New | No panner in `audio.rs` |
| Physics: kinematic player, projectiles, debris | Has as a switch (stub), kit-side solvers | `crates/evani-physics/src/lib.rs` is a stub; the genre's physics is `simple`, and Hexagon's character controller and hole collision come from `rules.ts`, ported into the kit as the kart racer does |
| Multiplayer: deterministic input and snapshot protocol, prediction, interpolation, lag compensation, loopback transport for bots | New (Stage 5 gate) | `crates/evani-net/src/lib.rs` is an M1 stub (two params, no transport); Stage 5's gate "The tactical FPS plays offline with bots through evani-net" |
| Online transport and a hosted match server | New, later | Nothing on evani.app runs game servers; Phase C in ROADMAP.md (section 5, question 1) |
| VR | Stub; Stage 9 | `crates/evani-xr/src/lib.rs` detects WebXR only; VR render paths filed for Stage 9 (`vr-stereo`) |
| Adaptive quality, tier warm-up, GPU timing | Has | `crates/evani-core/src/quality.rs` (`AqController`), `crates/evani-render/src/control.rs` (`TierWarmup`), `gpu/timing.rs` |
| Bench camera path, geocheck, look guard, review card, critic | Has | `apps/bench`, `tools/evani-geocheck` (the kit needs its 3D adapter), `tools/evani-lookguard`, `cargo xtask review` / `critic` |
| Ship guard and the 10 MB package cap | Has | `tools/evani-guide/src/playground.rs` (`PACKAGE_CAP_BYTES`), `cargo xtask build <id> --ship` |

## 4. Render targets per tier

The tier definitions, render paths, triangle and draw caps, budget points and VR comfort list are the catalog's (`catalog/tiers.json`, `assets/tiers/*.json`); this section fills in Hexagon's numbers inside them. All frame-time, resolution and size figures are *(est.)* until benched.

### 4.1 Frame time and resolution

Frame budget = whole frame at the target rate. GPU and CPU budgets leave about 15 % for the browser and frame pacing. CPU covers the 60 Hz simulation (8 bots, holes, collapse checks), render recording and wasm overhead on the main thread. A wall or floor collapse may spike the sim; the kit spreads the structure checks over ticks so no single step goes over 4 ms *(est.)*.

| Tier | Reference device | Output | Internal resolution | Frame target (floor) | GPU budget | CPU budget | Draws / triangles (catalog cap) |
|---|---|---|---|---|---|---|---|
| Low / phone | Samsung SM-G991B (Mali-G78); Lenovo TB305FU (Mali-G52) | phone landscape about 915×412 CSS px | scale 0.75, DPR ≤ 1.25: about 860×390 on the phone, about 1200×750 on the tablet; the controller may drop to 0.6 | 60 fps, 16.7 ms (floor 30 fps, 33.3 ms) | ≤ 13 ms | ≤ 6 ms | 150 / 100k |
| Medium | integrated laptop GPU, recent phones | 1920×1080 | scale 1.0, DPR ≤ 1.5: 1920×1080 | 60 fps, 16.7 ms | ≤ 12 ms | ≤ 5 ms | 300 / 400k |
| High | discrete desktop GPU (bench machine: RTX 4060 Ti) | 2560×1440 | scale 1.0, DPR ≤ 2: 2560×1440 | 60 fps, 16.7 ms (stretch 120 fps, 8.3 ms) | ≤ 10 ms (≤ 7 ms for 120) | ≤ 4 ms | 800 / 2M |
| Ultra | high-end desktop GPU | 2560×1440 and above (3840×2160) | 1440p native, or 3840×2160 from 2560×1440 with temporal upscale (0.67) | 60 fps, 16.7 ms | ≤ 14 ms | ≤ 4 ms | 2,500 indirect / 6M |
| VR (Stage 9) | standalone and PC VR headsets | per eye, stereo | the headset's recommended eye size at 1.0, foveated | 72-90 Hz, 13.9-11.1 ms | ≤ 9 ms at 90 Hz | ≤ 4 ms | 200 / 750k |

Known risk from Stage 2's device run (`.vibedeck/screenshots/stage2-devices/results.md`): the tablet already fails the Low bench at 22.4 ms on the kart racer's 3D phase, and the phone fails High at 20.6 ms. Hexagon's Low must therefore be cheaper per pixel than the kart racer's; on the tablet the honest target is the 30 fps floor. Phone tiers are proven on Chris's two devices, not on desktop.

Rough High pass budget at 1440p, for the bench to confirm *(est.)*: depth prepass 0.8 ms; two shadow cascades 1.5; clustered light binning 0.3; forward opaque with hole cut-outs 3.5; view model 0.3; particles and decals 0.8; GTAO half-res 0.8; fog 0.2; bloom 0.5; tonemap, grade, vignette 0.3; HUD 0.2. Total about 9.2 ms.

### 4.2 Render path and effects per tier

Render paths are the catalog's 3D path for each tier. Effects are catalog ids; each must also be an `evani-fx` feature. "Budget" is the look's points plus the effects that start on, against the tier's points (`assets/tiers/*.json`); the plan uses the chosen look `photoreal` (5 points on every tier; section 5, question 2, decided). With `realpbr` (4 points) each tier gains one point.

| | Low / phone | Medium | High | Ultra | VR (Stage 9) |
|---|---|---|---|---|---|
| Render path (catalog) | forward, one directional light, blob shadows, tonemap folded in | forward, instancing, CPU frustum culling, 1 cascade, up to 8 point lights, HDR | clustered forward, 2 cascades, image-based lighting, full HDR post, compute particles | GPU-driven culling, 4 cascades, compute particles, volumetric passes, temporal upscale | single-pass stereo forward, 4x MSAA, foveation, baked light plus one shadowed light |
| Anti-aliasing | none (scale 0.75) | MSAA 4x | MSAA 4x | temporal AA with upscale (section 7) | MSAA 4x |
| Lights | sun + baked sector lights | sun + 8 point lights | sun + clustered sector lamps, muzzle and blast lights | as High | sun + baked |
| Budget points | 7 | 11 | 16 | 24 | 9 |
| look `photoreal` (5) | on (reduced: full look from High) | on (reduced) | on (full) | on (full) | on (reduced) |
| `bloom` (2) | on | on | on | on | on |
| `shadows` (2) | off (blob) | on, 1 cascade 1024² | on, 2 cascades 2048² | on, 4 cascades 4096² | on, 1 cascade 1024² |
| `particles` (2) | off by budget (kit mesh flashes) | on, cap 10,000 | on, cap 50,000 | on, cap 250,000 | off by budget |
| `fog` (1) | off by budget | off by budget | on | on | off by budget |
| `decals` (1) | off by budget (core decals only) | off by budget | on | on | off by budget |
| `ssao` (3) | off (baked AO) | off (baked AO) | on, half-res GTAO | on, full-res GTAO | off |
| `destruction` (3) debris | off (dust puff) | off (dust puff) | off by budget | on | off |
| `volumetric` (3) | off | off | off | on (froxel fog, shadowed shafts) | off |
| `grade` (1), `vignette` (1) | off by budget | off by budget | off by budget | on | off by budget |
| `ssr`, `godrays`, `flare`, `dof`, `motionblur`, `grain`, `chroma` | off | off | off by budget | off by budget | off (comfort-off where listed) |
| `shake` (0) | on, light | on | on | on | off (comfort) |
| Points used | 7 of 7 | 11 of 11 | 16 of 16 | 24 of 24 | 9 of 9 |

Notes:
- The Ultra column is the target once Stage 8 has built four cascades, volumetric passes, temporal upscale and GPU-driven culling. Before that (Stage 5), Ultra runs the High path at native 1440p and above with MSAA 4x and two cascades, and keeps every Ultra effect that needs no Stage 8 work (full-resolution GTAO, debris, `grade`, `vignette`): 21 of 24 points without `volumetric`. The 3 freed points go to `ssr` (screen-space reflections, 3), so pre-Stage 8 Ultra is 24 of 24; when Stage 8 brings volumetric fog, it takes those 3 points back and SSR goes off unless question 5 revises the costs.
- The look carries the fixed photographic tonemap, grade and a light fixed vignette in its `tonemap` slot on every tier (so High matches today's grade and vignette); the `grade` effect (a swappable LUT) and the `vignette` effect (tunable, doubles as the hit and comfort vignette) are extra and only fit on Ultra.
- Gameplay never depends on a tier: holes, collapses and line of sight are identical on every tier and in every effect state (the kit's sim, deterministic). Only their drawing changes.
- Clarity (docs/KIT_PROCESS.md "Clarity"): a competitive shooter avoids depth of field, motion blur, chromatic aberration and heavy grain. They stay off in play on every tier. If a later budget review frees points, `dof` is used only for the sniper scope and `chroma` only as a short hit pulse.
- With today's points, High cannot hold debris, SSR, light shafts or the swappable grade together with shadows, particles and GTAO, and Ultra holds no SSR once Stage 8 volumetric fog is on. Section 5, question 5 asks whether the points may be revised from the bench.

### 4.3 Download

The playground rule (docs/PLAYGROUNDS.md, DECISIONS.md 2026-10-15) is a **10 MB cap on the whole uploaded package**, uncompressed on disk, enforced by `cargo xtask build <id> --ship`. It is stricter than "10 MB first load": everything a player can ever download from the playground, for any tier and any device, must fit in it. "First load" below is what loads before the title screen is playable; the rest streams after the title screen by tier, from inside the same package (docs/STAGE3_PLAN.md S3-4: per-tier files, a `boot` group for the title, the rest fetched by priority and kept in Cache Storage).

Stage 3's texture choice shapes this budget (docs/STAGE3_PLAN.md section 3, "Model loading" pick 2, and S3-3): no transcoder ships; each texture is stored ready-made per device family (ASTC for phones, BC7 for desktops, and a PNG or JPEG fallback), as KTX2 with no supercompression. So the package holds **every** family's copy, while each player downloads only one. At 8 bits per texel with mips, a 1024² texture is about 1.4 MB on disk per family, so textures are by far the scarcest part of the 10 MB.

| Package part | Size on disk *(est.)* |
|---|---|
| wasm (engine, render, about 12 effects, kit, look, net, `models`, `anim`, `ui`) and JS glue, stream shim, page, loader | 2.5 MB |
| Arena geometry | 0 (procedural, as today) |
| Models (soldier with 3 LODs, 4 weapons as view and world models, props), meshopt and quantized, per-tier files | 1.2 MB |
| Animation clips (Stage 3's compact clip format, inside the model files) | 0.3 MB |
| Fonts (MSDF atlases, the HUD's character set only) | 0.1 MB |
| Textures, ASTC family: Low 0.35, Medium adds 0.7, High adds 1.15 | 2.2 MB |
| Textures, BC7 family: same sets | 2.2 MB |
| Textures, PNG or JPEG fallback family: smaller, Low and Medium sets only | 0.6 MB |
| Audio (synthesized, as today; a few recorded impulses at most) | 0.2 MB |
| Sky, atmosphere and noise lookup tables | 0 (generated at start) |
| **Total package** | **9.3 MB of the 10.00 MB cap** |

What one player downloads (one texture family):

| Tier | First load (before the title screen plays) | Total fetched for the tier |
|---|---|---|
| Low / phone | 3.25 MB on disk, about 1.6 MB over the wire *(est.)*: code 2.5, lowest model LODs 0.3, Low textures 0.35, fonts 0.1; the arena draws at once because it is procedural | 3.75 MB (adds animations and audio) |
| Medium | same first load | 4.85 MB (adds middle LODs 0.4 and Medium textures 0.7) |
| High | same first load | 6.5 MB (adds full-detail models 0.5 and High textures 1.15) |
| Ultra | same first load | 6.5 MB, the same files as High; anything more needs Chris (section 5, question 4) |
| VR | same first load | as Medium, 4.85 MB |

A device that takes neither ASTC nor BC7 gets the PNG or JPEG fallback family, which holds only the Low and Medium sets: it uses Medium textures on every tier from Medium up (about 4.4 MB in total, since the fallback files are smaller), and the overlay says so.

If the measured package is over the cap, cut in this order: (1) halve the High texture additions in both GPU families (512² instead of 1024² class for non-hero surfaces), saves about 1.1 MB *(est.)*; (2) drop the middle model LOD (Medium uses the lowest), saves about 0.4 MB; (3) drop the recorded audio impulses, saves 0.2 MB. Step 1 alone brings a 10.5 MB result back to about 9.4 MB.

Consequences: Ultra differs from High by rendering (resolution, four cascades, volumetric fog, temporal upscale, GTAO at full resolution, debris), not by bigger textures. The arena's materials stay procedural, as today (Hexagon already draws steel plates, painted panels, poured concrete and worn paint from shader maths), so textures are kept for the soldier, the weapons and a few tiling detail maps. The research report's suggested 30-60 MB for High and 100-200 MB for Ultra do not fit a playground and are not planned.

## 5. Questions for Chris (decided)

**Chris's answer (2026-10-15): "Yes, all as recommended".** All six are decided as recommended below: bots first, the `photoreal` look, ring, destruction and map size kept, Ultra inside 10 MB, effect costs revised only from bench results with each change approved by Chris, built in Stage 5 as kit 006's demo. Also decided the same day (docs/STAGE3_PLAN.md decision 12): during Stage 3 one Hexagon soldier and one weapon go through the converter as extra test cases.

1. **Online play at launch, or bots first?** I recommend bots first: Hexagon launches as a single-player playground against bots, with online play later. Reason: playgrounds are single player, evani-net has no transport yet, and evani.app runs no game servers.
2. **Which look?** I recommend the catalog's "Gritty photoreal" look (`photoreal`) for Hexagon instead of the plain "Realistic PBR" listed for the FPS kit. Reason: it is the catalog look closest to Battlefield (photographic grade, volumetric atmosphere, grime) and it is full only from High, which is "best on strong PCs, lighter on phones".
3. **Keep the rotating ring, the concrete destruction and the map size?** I recommend keeping all three as they are. Reason: they are what makes Hexagon Hexagon, the destruction already runs in a tiny download, and the map was tuned for 4 against 4.
4. **May Ultra download more than the 10 MB playground package (for example sharper textures or a photo-scanned backdrop, streamed and cached)?** I recommend no for launch: Ultra stays inside 10 MB and looks better through lighting, not bigger files. Reason: the playground cap is on the whole package, and the shared asset cache that would make extras cheap arrives in Stage 7.
5. **May the effect costs ("budget points") be revised after measuring Hexagon?** I recommend yes, from bench results only, with each change approved by you. Reason: with today's numbers even High has to leave out debris, reflections and light shafts; the costs are estimates, and the measured cost on your devices should decide.
6. **When is Hexagon built?** I recommend in Stage 5, as the demo of kit 006 (the first-person shooter kit), right after the kart flagship. Reason: Stage 5 already plans the shooter kit, its lighting and its offline bots, so Hexagon gets them without a second shooter being built; Ultra's extra effects follow in Stage 8.

## 6. Where Hexagon fits

**Proposal: Hexagon becomes kit 006's demo game** (ROADMAP Phase A row 006, kit id `tactical-fps`, like "Sugarpeak Rally" is the kart racer's demo), built in **Stage 5**, which opens only after Gates 3 and 4. Not a separate flagship.

Why the kit and not a separate game:
- Stage 5 already plans kit 006 with the same needs (first person, hitscan, evani-net with bots) and the render paths Hexagon needs at Medium and High (frustum culling, point lights, two cascades, clustered forward, image-based lighting; CORE_REQUESTS.md). A separate flagship would build a second shooter on the same engine work.
- Stage 5's gate line "The tactical FPS plays offline with bots through evani-net" is exactly what Hexagon's bots-first launch needs.
- Hexagon already proves the hard parts (deterministic sim at an effective 60 Hz step, bots, destruction), so the kit starts from working rules.

Genre fit (`catalog/genres.json` `tactical-fps`): hitscan with recoil and movement spread: yes, added to pistol and sniper. Server-authoritative netcode with lag compensation: yes, through evani-net (loopback first). Bots for offline play: yes. Plant and defuse objective: Hexagon's core infiltration (hold the enemy core for 8 s) stands in for it. Round and economy with a buy menu: not in Hexagon (fixed four-weapon loadout, kills race). The kit's genre playbook, written in KIT_PROCESS step 1, records these two as deliberate differences for the critic. If Chris prefers a strict genre match, the alternative is to make Hexagon a later separate game on kit 006's crate.

Dependencies, in order:
1. **Stage 3** (hard dependency, Chris: "build after Stage 3"): glTF import with meshopt and KTX2, skinned animation with blending and simple IK, LOD, the text and UI layer, evani-convert, streaming detailed assets after the title screen by tier.
2. **Stage 4**: one-click publish and the size report (for the playground upload).
3. **Stage 5** (where Hexagon is built): the filed 3D render paths (`cpu-frustum-culling`, `point-lights`, `shadow-cascades-2`, `clustered-forward`, `image-based-lighting`), evani-net loopback with bots, the GPU particle system (core-request plan item C), and the new work in section 3 (pointer lock, look pad, view-model pass, kit material pass for hole cut-outs, positional audio, the look crate, the effects).
4. **Stage 8** (Ultra extras, the Battlefield end of the range): `shadow-cascades-4`, `volumetric-passes`, `temporal-upscale`, `gpu-driven-culling`. Until then Hexagon's Ultra runs the High path at native 1440p and above, with the extra effects the budget allows. Hexagon is the natural proof scene for those Stage 8 items.
5. **Stage 9**: VR (comfort risk: the turning ring and fast first-person movement need the comfort vignette and snap turning) and online play with Phase C servers.

## 7. Catalog and core needs to file (proposals, not made)

The catalog is data the code never contradicts, so these are proposals for Chris; none is applied by this plan.

- **Effects missing from `catalog/effects.json`:** temporal anti-aliasing (`taa`: jitter, history reprojection, neighbourhood clamp; the catalog mentions temporal upscale only as an Ultra render path); analytic sky and atmosphere (`sky`: precomputed transmittance and sky-view lookup tables, Frostbite-style); auto exposure (`exposure`: luminance histogram and eye adaptation). Each needs a cost in points.
- **`destruction` effect wording:** today "pre-fractured chunks or tile removal"; Hexagon's analytic hole cut-outs (shader discard against a hole list, Voronoi rims) are a third method worth listing.
- **Render core (to file in CORE_REQUESTS.md when Stage 5 opens):** first-person view-model pass; kit material pass with a kit storage buffer and discard in colour, depth and shadow passes; pointer lock with full release; touch look pad; positional audio.
- **High-tier anti-aliasing:** the catalog gives High MSAA 4x at render scale 1.0. Battlefield's look leans on temporal AA, which also handles shimmering highlights that MSAA misses. Propose `taa` with an optional 0.75 upscale on High only if the bench shows MSAA 4x at 1440p misses the 10 ms budget or the highlights shimmer.
- **Genre playbook:** `docs/playbooks/tactical-fps.md` does not exist; kit 006 writes it in KIT_PROCESS step 1, with Hexagon's two genre differences.
- **Look:** no new catalog look is needed: Chris chose `photoreal` (question 2, 2026-10-15), and ROADMAP row 006's "First look" column now says `photoreal`.

## 8. Build order (when Stage 5 opens, after the questions are answered)

1. Port the rules (arena build, ring, movement, weapons, holes, collapse) into the kit with deterministic tests, then bots, against the existing playtest pattern (`playtest_win*`, `playtest_lose*`).
2. evani-net loopback: inputs, snapshots, prediction, interpolation, lag compensation, with bots; the Stage 5 gate line.
3. The look crate and the PBR and IBL forward path at Low and Medium, with hole cut-outs and the view model; bench on Chris's phone and tablet first.
4. High: clustered lights, two cascades, GTAO, fog, decals; compare against today's WebGL2 Hexagon captures (the High floor in section 1).
5. Models through evani-convert, streaming by tier, HUD and menus on the text and UI layers, touch and pointer lock.
6. Ultra with the Stage 5 paths; Stage 8 adds the rest. Each step ends with `cargo xtask check`, the bench per tier and the package size against the table in 4.3.
