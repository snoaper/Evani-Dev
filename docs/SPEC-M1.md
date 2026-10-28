# Evani.Dev — Milestone 1: The Foundation (full brief, verbatim from Chris)

## Design pillars (from Chris, 2026-10-13 — foundational, every stage must honour them)
These are what set Evani.Dev apart. Treat them as hard requirements of the foundation, not marketing.
1. **AI is part of the engine, not bolted on.** No main web engine has AI features, and AI game makers only generate code on top of someone else's engine. Evani.Dev offers live tuning of parameters, shader math and weights, with measurement, auto-revert and bake into source.
   - In practice: everything tunable is a registered parameter, slot, curve or policy, all in the always-included core (evani-core), not the dev-only crate.
   - Every module, kit, look and effect must expose its tunables there, so an AI can reach it through the one control API.
   - Every change is measurable through telemetry and captures, reversible through undo and auto-revert, and bakeable into source.
   - New engine features aren't done until the AI can read and drive them.
2. **WebGPU-first with no legacy.** Most rivals still default to WebGL2. Evani.Dev has no WebGL or Canvas fallback, no webgl feature and no legacy code paths. If WebGPU is missing, the game shows a clear message.
3. **Modularity you can prove.** Every build gets a size report per module (xtask), against Unity's starting point of 8 MB or more.
   - Size is a tracked output of every build.
   - A module that isn't listed in evani.toml must measurably add zero bytes.
4. **Tiers, budgets, VR and determinism built in from the start.** Quality tiers, the effect points budget, the VR tier and comfort rules, and deterministic simulation are core concepts. Simulation uses a fixed step, seeded RNG and a reproducible camera path, so A/B comparisons and benches are fair.
   - Retrofitting any of these later is not acceptable.

README.md must open with these pillars, and DECISIONS.md must say how each one is enforced in code.

## Target layout (from Chris, follow-up message — this overrides folder placement below)

```
Evani.Dev  (the engine, one Cargo workspace)
├── Always included
│   ├── evani-core        loop, ECS, input, assets, audio, overlay
│   └── evani-render      WebGPU renderer, render graph, tiers
├── Optional modules      only if the game lists them in evani.toml
│   ├── evani-fx          effects framework; each effect is a feature (bloom, outline…)
│   ├── evani-physics     simple or rigid backend
│   ├── evani-net         multiplayer
│   └── evani-xr          VR, downloaded only on Enter VR
├── Kits                  one per genre
│   └── evani-kit-kart-racer, evani-kit-rts, evani-kit-platformer…
├── Looks                 one per art style
│   └── evani-look-toon, evani-look-pixel16, evani-look-photoreal…
└── Tools
    └── xtask / evani CLI  reads evani.toml, builds, reports sizes
```

Orchestrator decisions applying this to M1 (record in DECISIONS.md):
- Kits live in `kits/evani-kit-<genre>`, looks in `looks/evani-look-<style>`, each its own crate. The M1 example kit and look are real crates (`kits/evani-kit-example`, `looks/evani-look-basic`) used by apps/sandbox, so bake.export has a look/kit crate to write into.
- `evani-physics` (features `simple`, `rigid`) and `evani-net` are stub crates in M1: they compile, register with the engine and report in the overlay, but contain no real physics/networking yet. They exist so evani.toml module switches are real Cargo features from day one.
- `evani-xr` is a stub (feature-detect only) as in the brief.
- `evani-live` is a dev-only crate (not in the shipped tree above) plus `tools/evani-live-bridge`.
- The tools entry is `tools/xtask`, invocable as `cargo xtask …` (alias in .cargo/config.toml); an `evani` CLI name can wrap it later.
- Chris: "feel free to improve the engine, one Cargo workspace as you go along." Workers may improve the layout and design where it clearly helps (modularity, download size, clean plugin interfaces), as long as the two core ideas and the DONE WHEN list still hold. Record every such change, with its reason, in DECISIONS.md.

You are a senior Rust graphics and game-engine engineer. We are building Evani.Dev, a light, low-bandwidth, high-capability browser game engine that will be published on Evani.app so anyone can make their own browser games. Two ideas define it:
1. Modular: every game keeps only the modules it needs, and nothing else reaches its download.
2. Live and AI-driven: while a game runs, its rendering can be adapted in real time, including parameters, weights and the underlying math, by a person in the Look Lab or by an AI agent through the same control API. The result is then baked into the shipped game.

This is MILESTONE 1: THE FOUNDATION. Build the shared engine only. No genre gameplay yet. Every future genre kit (kart racer, RTS, platformer, and so on) and every art-style "look" will plug into what you build here, so the interfaces matter more than features.

## Change from Chris (2026-10-13): no second Look Lab — this overrides the brief below
Evani.App already has a Look Lab: the **Evani.dev tab** (live engine view with "Talk to the AI"). Don't build a second one.
- apps/sandbox keeps only a minimal in-page debug panel (tier switch, effect toggles, overlay). No full Look Lab UI. (Overrides "apps/sandbox ... first version of the Look Lab" with live controls for every parameter and slot editing.)
- tools/evani-live-bridge is the "live server" the Evani.dev tab looks for. Make it discoverable per project, so choosing the engine folder in the tab's settings finds and starts it.
- docs/LIVE_API.md is the single contract between the engine and the tab: transport, auth token, and every command (schema.get, param.set, slot.set, curve.set, policy.set, effect.toggle, tier.set, telemetry.subscribe, capture.frame, bench.run, compare.ab, snapshot and history, bake.export), with request and response examples. Match the tab's current protocol where possible; otherwise list exactly what the tab must change.
- DONE WHEN check 6 is replaced (see below).

## TECH STACK
- Rust 2024 edition, stable toolchain, target wasm32-unknown-unknown, built with Trunk.
- WebGPU only, through wgpu with its WebGPU backend for wasm and without the webgl feature. No WebGL or Canvas fallback. WGSL shaders, one file per pass, loaded with include_str!.
- Engine crates (shipped in games): wasm-bindgen, wasm-bindgen-futures, web-sys, js-sys, wgpu, glam, bytemuck, serde, serde_json, console_error_panic_hook. Pin exact versions. Write the ECS, input, audio mixer, UI and asset pipeline yourself. Add nothing else without a one-line reason in DECISIONS.md.
- Dev tools (native, never shipped in a game) may use other crates. Record each choice in DECISIONS.md.
- wasm rules: requestAnimationFrame loop, time from performance.now() (never std::time::Instant), single-threaded, set any cfg flags wgpu needs in .cargo/config.toml.
- Release profile: opt-level = "z", lto = true, codegen-units = 1, panic = "abort", strip = true, then wasm-opt -Oz.

## WORKSPACE
Everything is an evani- package inside one Cargo workspace.
- crates/evani-core (always included): fixed-timestep loop (simulation at 60 Hz, interpolated rendering); sparse-set ECS with queries and ordered systems; input actions (keyboard, mouse, pointer and touch, Gamepad API) with JSON bindings and rebinding; asset manifest keyed by content hash with an async loader and progress; scene stack; event queue; seeded PCG RNG; save/load to localStorage; a basic Web Audio mixer (music and effect buses, started after the first user gesture); a debug overlay as a lightweight HTML element (fps, CPU ms, GPU ms, draw calls, entities, active tier, budget points, download size).
- crates/evani-render (always included): wgpu device and surface setup; a clear full-screen message when WebGPU is unavailable; resize via ResizeObserver with a per-tier DPR cap; a small render graph (passes declare their inputs and outputs, with HDR colour, depth and transient targets); a pipeline cache that can rebuild single pipelines at runtime; device-lost recovery; per-pass GPU timing via timestamp-query when available. Two render paths behind one narrow API: an instanced 3D mesh path (forward shading, one directional light, tonemapping) and an instanced 2D sprite batcher (texture atlas, layers). The 3D mesh path is the build-time feature `forward` (added later, CORE_REQUESTS `forward-optional`): 3D kits turn it on, 2D games download only the sprite path.
- crates/evani-fx (optional): the effects framework. An Effect trait where each effect declares its name, cost in points (0 to 3), a visual value weight, which style classes it supports (2D, pseudo-3D, 3D), whether it is comfort-off in VR, its tunable parameters, and the render-graph passes it adds. A budget calculator that, per tier, switches off the effects with the lowest value per point until look plus effects fit the tier's points. Every effect is its own Cargo feature of evani-fx. Implement exactly two reference effects to prove the interface: Vignette (one pass) and Bloom (multi-pass: bright-pass, then a dual-Kawase blur over half-resolution mips).
- crates/evani-live (optional, dev builds only): the live render-adaptation system described in LIVE RENDER ADAPTATION below.
- crates/evani-xr (optional): a stub only. Feature-detect WebXR with WebGPU and report the result in the overlay. Document in DECISIONS.md how this module will later be built as a separate wasm file that downloads only when a player presses Enter VR.
- tools/xtask: reads a game's evani.toml (the module manifest), turns it into Cargo features, builds with Trunk, runs wasm-opt, and prints the gzipped size of the build and of each module.
- tools/evani-live-bridge (native): a local WebSocket bridge to a running game, plus an MCP server that exposes the live control API as tools, so an AI agent such as Claude can connect and drive a running game.
- apps/sandbox: the test game and the first version of the "Look Lab". A lit 3D scene of instanced meshes plus a 2D sprite layer, with live controls for tier switching, effect toggles, every registered parameter, shader slot editing, and points used against the tier budget. The Look Lab uses the same live control API as the AI.
- apps/bench: spawns stress content and runs it at each tier along a fixed, deterministic camera path, printing pass or fail against the tier limits. On a real phone or tablet `?device=1` lets the first-run warm-up pick the tier and judges only that one (README, "Device mode").
- assets/tiers: lite.json, low.json, medium.json, high.json, ultra.json, vr.json (Lite, for budget tablets, added 2026-10-16; see DECISIONS.md). They are the source of truth; evani-core's build script compiles them into the tier table the game ships, so no tier JSON is parsed at start (2026-10-16, README "How data ships").

## QUALITY TIERS (presets in JSON, nothing tier-specific hard-coded elsewhere)
- Low: 60 fps target, 30 floor; render scale 0.75×, DPR ≤ 1.25; ≤ 150 draw calls; ≤ 100k triangles or 5,000 sprites; 7 points.
- Medium: 60 fps; 1.0×, DPR ≤ 1.5; ≤ 300 draw calls; ≤ 400k triangles or 20,000 sprites; 11 points.
- High: 60–120 fps; 1.0×, DPR ≤ 2; ≤ 800 draw calls; ≤ 2M triangles or 100,000 sprites; 16 points.
- Ultra: 60 fps at 1440p; 1.0× native; ≤ 2,500 draw calls; ≤ 6M triangles or 500,000 sprites; 24 points.
- VR: 72–90 Hz stereo; ≤ 200 draw calls; ≤ 750k triangles or 10,000 sprites; 9 points. Preset only for now.
On first run, read adapter limits, run a 2-second warm-up, and start at the highest of Low to Ultra that holds its frame time. Remember the choice. Tiers switch at runtime without reloading, rebuilding pipelines and render targets.

## PLUGIN INTERFACES (the most important part of this milestone)
- KitPlugin trait: how a genre kit registers its components, systems, scenes, input actions, data files and tunable parameters.
- LookPlugin trait: how an art style provides palettes, materials, WGSL shader slots, post-pass settings, tunable parameters and procedural asset generators.
- evani.toml format: the game's name, its kit, its look, its effects, and optional modules (physics simple or rigid, net, xr, live).
Write one tiny example kit and look inside apps/sandbox to prove both traits work.

## ON AND OFF: TWO LEVELS (the core idea of Evani.Dev, design for it from the start)
1. Build time (per game, controls download size): evani.toml decides what exists in the build.
   - Every optional module (fx, physics, net, xr, live) is a Cargo feature or crate that can be left out.
   - Every effect is its own Cargo feature of evani-fx (for example features = ["bloom", "vignette"]).
   - Anything not listed is not compiled. No dead code, no unused shaders, no unused assets in the download.
   - Switching a module or effect off must never break the build: code that uses an optional module sits behind its feature flag.
   - evani-live is for development and the Look Lab only. Shipped games never contain it.
2. Run time (per player, controls performance): everything that was built can be switched on and off live.
   - Each compiled-in effect has a toggle in settings and in the overlay.
   - Each tier preset decides which effects start on; the budget calculator switches off the lowest value-per-point effects that don't fit.
   - VR forces comfort-off effects off.
   - Settings only show what this build contains; an effect that wasn't compiled in never appears as a greyed-out option.

## LIVE RENDER ADAPTATION (crates/evani-live and tools/evani-live-bridge)
The goal: a person or an AI agent can change how the game renders while it runs, see the result within a frame, measure it, and keep or revert it.
1. Everything tunable is a registered parameter. Effects, looks, kits, tier presets, the budget calculator and the adaptive-quality controller register their values in one parameter registry, each with a name, type, range, default, unit and one-line description. The registry exports a JSON schema. Changing a value applies on the next frame with no reload; GPU-side values live in uniform buffers, never baked into shader source.
2. The math lives in swappable slots. The render graph calls shader slots: WGSL functions with fixed signatures, such as tonemap(color) -> color, bloom_weight(mip) -> f32, toon_ramp(n_dot_l) -> f32, fog(distance, height) -> f32, exposure(luminance) -> f32. A slot's body can be replaced at runtime: validate it (error scopes and compilation info), build the new pipeline asynchronously, swap it in at a frame boundary, and keep the old pipeline if anything fails. CPU-side formulas (LOD distances, camera smoothing, adaptive-quality thresholds) are piecewise-linear curves with editable control points, or expressions in a tiny, safe, built-in expression evaluator (arithmetic, min, max, clamp, mix, smoothstep, pow, named variables; no loops, no general scripting).
3. Weights drive adaptation. Each effect has a cost and a visual value weight. The adaptive-quality controller (frame-time target, averaging window, hysteresis, degrade order by value per point, render-scale step and floor) is driven by a policy of registered weights, so an AI can tune how each game degrades and recovers.
4. One control API for people and AI. Commands, as JSON messages:
   - schema.get: every parameter, slot, curve, effect, tier and policy, with types, ranges and descriptions.
   - param.get / param.set: batched and atomic.
   - slot.get / slot.set: WGSL or expression source; returns a compile result with errors.
   - curve.get / curve.set, policy.get / policy.set.
   - effect.toggle, tier.set.
   - telemetry.subscribe: frame time, per-pass GPU ms, draw calls, triangles or sprites, budget points, at a chosen rate.
   - capture.frame: a downscaled PNG plus stats, optionally at a fixed simulation time.
   - bench.run: the fixed camera path for N seconds; returns stats and captures.
   - compare.ab: runs two snapshots on the same deterministic camera path; returns per-pass timings and paired captures.
   - snapshot.save / snapshot.restore, history.undo / history.redo.
   - bake.export (see point 6).
   The API is available in-page as window.evani.live, and through the bridge over WebSocket and as MCP tools. The bridge binds to localhost and requires a dev token.
5. Feedback for the AI. Every change returns what happened: the new values, the compile result, and the next second of telemetry. Captures and A/B runs use the deterministic camera path and fixed simulation time, so comparisons are fair.
6. Bake. When a person or AI is satisfied, bake.export writes the session into source files: tier preset JSON, the look's parameter JSON, slot .wgsl files and curve files in the look or kit crate, plus a CHANGES.md describing what changed and why (from the change notes attached to each command). A rebuild without evani-live reproduces the tuned look exactly, with zero live overhead.
7. Safety. Every change is a versioned transaction with undo and redo. Values are clamped to their ranges. Shader patches compile before they swap. If frame time stays more than 25% over the tier target for 3 seconds after a change, or the device is lost, the change auto-reverts and the reason is reported. The live system never runs in a shipped build.

## RULES
- No heap allocations in per-frame hot paths; preallocate, pool and reuse. Per-frame uniforms go in a persistent ring buffer.
- No unwrap or expect on fallible browser or GPU calls in release paths; errors show in the overlay.
- Deterministic simulation: fixed step, seeded RNG, no HashMap iteration order in game logic.
- Pause when the tab is hidden. Respect prefers-reduced-motion.
- Unit tests (native) for the ECS, fixed-step loop, RNG determinism, the budget calculator, the parameter registry, the expression evaluator, curves and undo/redo.

## HOW TO WORK
Work in small steps: workspace and build pipeline first, then core, render, fx, live (registry, slots, API), the bridge and MCP server, sandbox and Look Lab, then bench. After each step, run cargo check, cargo clippy and cargo test, build the wasm, and commit. If something isn't covered here, make the sensible call and record it in DECISIONS.md rather than stopping. Keep a README with the architecture, the evani.toml format, the live control API, and how to add a kit, a look, an effect, a parameter and a shader slot.

## DONE WHEN
1. trunk serve opens the sandbox: a lit 3D scene and a sprite layer at a steady 60 fps on a normal laptop.
2. A browser without WebGPU shows the clear message, not a blank page.
3. Switching tiers at runtime works without a reload, and the overlay shows tier, fps, CPU and per-pass GPU ms, draw calls and budget points.
4. Bloom and Vignette toggle live, and each tier's budget switches off what doesn't fit, by value per point.
5. Both levels of on/off are proven:
   - Build time: building the sandbox with and without "bloom" in evani.toml compiles cleanly both ways; xtask shows the size drop, and bloom's shader is absent from the smaller build. Building with fx removed entirely also compiles and is smaller again. A build without "live" contains no live code.
   - Run time: in the full build, Bloom and Vignette toggle live, and tier switches turn them on or off per the budget.
6. (Replaced 2026-10-13) Opening the Evani.dev tab with the engine folder selected connects to the running sandbox, and "Talk to the AI" can change Bloom, rewrite the tonemap slot and bake, with each change visible within one frame. (The MCP tools and per-command telemetry plus capture from LIVE RENDER ADAPTATION still stand.)
7. Safety: a WGSL patch with an error is rejected with the compiler message while the old shader keeps running; a change that pushes frame time over budget auto-reverts within 3 seconds; undo and redo work across 20 changes.
8. compare.ab returns per-pass GPU ms and paired captures for two snapshots on the fixed camera path.
9. bake.export writes the tuned presets, parameters, slots and curves into source files, and a rebuild without evani-live looks identical to the tuned session.
10. All tests pass, and apps/bench prints pass or fail for every tier.
11. README and DECISIONS.md explain everything a second developer needs to add the first genre kit.
