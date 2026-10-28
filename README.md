# Evani.dev

**A light, modular browser game engine in Rust and WebGPU.** Each game downloads only the engine modules its `evani.toml` lists, so a small game stays small and a big world spends its bytes on content, not on engine code you don't use.

Licence: [MIT](LICENSE). Copyright (c) 2026 Chris Pearson.

> **This repository is the Evani.dev engine only.** Evani.app (the website, platform and desktop app that can host and publish games) is a separate product and is not part of this repository or its licence. Some docs mention it because the engine's tools can pack a game for its library and talk to its live-editing tab; nothing in here needs it to build or run a game.

## Quick start

You need Rust (stable, 1.90 or newer) with the `wasm32-unknown-unknown` target, [Trunk](https://trunkrs.dev) for browser builds, and a browser with WebGPU (recent Chrome or Edge; Firefox and Safari as their WebGPU support allows).

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked

cargo xtask check                    # the engine's gate: prints "evani check: clean" when all is well
cargo test --workspace               # every crate's tests, natively (no browser needed)

cd apps/kart-racer && trunk serve    # play an example game at http://127.0.0.1:8080
cargo xtask build kart-racer --ship  # a release build plus a size report per module
cargo xtask new-game my-game --look toon   # start your own game (a kit crate and an app)
```

`cargo xtask doctor --fix` fetches the optional native helpers (model converter tools, the font baker, the live MCP server) when you need them.

## Modules

Every game always gets `evani-core` (loop, ECS, input, assets, audio, parameters, tiers, plugins) and `evani-render` (the WebGPU renderer). Everything else is optional and adds zero bytes unless the game lists it:

| Module | What it adds |
|---|---|
| `evani-fx` | effects, one Cargo feature each (bloom, vignette, outline, particles, shadows, fog, decals, grading, 2D lights and more) |
| `evani-physics` | simple physics and the static-world `collide` feature (raycasts, character mover) |
| `evani-combat` | shooter combat on `collide`: hitscan, guns, health, grenades, a bot brain |
| `evani-net` | multiplayer: deterministic lockstep with a referee |
| `evani-xr` | VR (feature detection) |
| `evani-gltf`, `evani-anim` | `.glb` models, LOD tiers, skinned animation |
| `evani-ui` | sharp MSDF text and box layout |
| `evani-material-*` | layered, character, nature (ocean) and procedural (sky, relief) materials |
| `evani-destruct` | breakable walls and debris |
| `evani-model` | Evani.Model, the engine's own AI-driven 3D modeler and model format (a dev tool) |
| `evani-live` (dev only) | the live control API for tools and AI agents; never in a shipped game |

Looks (art styles) live in `looks/evani-look-*`, gameplay in `kits/evani-kit-*`. Genres, looks, effects, tiers and physics are described as data in `catalog/`.

## Example games

Each example is a kit (gameplay) plus an app (the game), and all of them are MIT like the engine:

| Game | Folder | Look |
|---|---|---|
| Kart racer | `apps/kart-racer`, `kits/evani-kit-kart-racer` | toon |
| 2D platformer | `apps/2d-platformer`, `kits/evani-kit-2d-platformer` | pixel16 |
| Match-3 | `apps/match-3`, `kits/evani-kit-match-3` | cartoon2d |
| Top-down action-adventure | `apps/top-down-action-adventure`, `kits/evani-kit-top-down-action-adventure` | pixelhd |
| Base-building RTS | `apps/base-building-rts`, `kits/evani-kit-base-building-rts` | stylizedpbr |
| Tactical FPS | `apps/tactical-fps`, `kits/evani-kit-tactical-fps` | photoreal |
| Sandbox (engine test scene) | `apps/sandbox`, `kits/evani-kit-example` | basic |

`apps/bench` is the per-tier GPU bench and `apps/model-view` a model viewer page used by the live tools.

## Docs

- This README, below: architecture, building, `evani.toml`, adding kits, looks and effects, touch controls.
- [AGENTS.md](AGENTS.md): the rules for AI coding agents (also read by Claude Code, Cursor and Gemini through `CLAUDE.md`, `.cursor/` and `GEMINI.md`).
- [docs/](docs/): the live API ([LIVE_API.md](docs/LIVE_API.md)), the kit process, geometry checks, models, playgrounds, licensing and more.
- [DECISIONS.md](DECISIONS.md): why things are the way they are. [STAGES.md](STAGES.md) and [ROADMAP.md](ROADMAP.md): the staged plan.
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md): third-party code and assets with their licences.

---

# Engine reference

## Design pillars

These four pillars set Evani.Dev apart. They are hard requirements of the foundation. [DECISIONS.md](DECISIONS.md) explains how the code enforces each one.

1. **AI is part of the engine, not bolted on.** Everything tunable is a registered parameter, shader slot, curve/formula or policy, and all of them live in the always-included `evani-core`. Every module, kit, look and effect exposes its tunables there, so a person or an AI can read and drive all of it through one control API (`window.evani.live`, [docs/LIVE_API.md](docs/LIVE_API.md)). Every change can be measured, reverted and baked into source. A new engine feature isn't done until the AI can read and drive it. The engine also ships its own rules for AI coding agents: `cargo xtask init-ai` writes them into AGENTS.md (with thin pointers for Claude Code, Cursor and Gemini), every new game gets them, and one gate, `cargo xtask check` (MCP `evani_check`), tells an agent what is wrong and how to fix it ([docs/AI_GUIDANCE.md](docs/AI_GUIDANCE.md)).
2. **WebGPU-first, with no legacy.** There is no WebGL or Canvas fallback, no `webgl` feature and no legacy code path. If the browser has no WebGPU, the game shows a clear full-screen message.
3. **Modularity you can prove.** Every build gets a size report per module (`cargo xtask build <app>`), measured against Unity's web starting point of 8 MB or more. A module that isn't listed in `evani.toml` is an absent optional dependency, so it adds zero bytes.
4. **Tiers, budgets, VR and determinism built in from the start.** Quality tiers, the effect points budget, the VR tier and its comfort rules, and deterministic simulation (fixed 60 Hz step, seeded PCG32, ordered ECS) are all core concepts.

## Architecture

```
Always included
  crates/evani-core      loop, ECS, input, assets, audio, overlay, save, scenes,
                         the param registry, curves, expressions, formulas, shader slots,
                         tiers, the effect budget calculator, plugin traits, browser runner
  crates/evani-render    WebGPU renderer: render graph, instanced meshes, sprites, tonemap,
                         shader-slot splicing and hot-swap, GPU timing, tiers at runtime,
                         adaptive quality, device-loss recovery, "no WebGPU" message
Optional modules (only if the game's evani.toml lists them)
  crates/evani-fx        effects framework; each effect is a Cargo feature (bloom, vignette)
  crates/evani-physics   physics, `simple` or `rigid` backend (M1: stub); feature `collide`:
                         static triangle world, tagged raycasts, line of sight, sphere and
                         capsule casts, character mover, `physics.collide.*` tunables
                         (evani_physics::collide; docs/CHIMPDUTY_INTAKE.md)
  crates/evani-combat    shooter combat on `collide`: hitscan, guns (recoil, spread, range
                         falloff), health and armour, grenades, rockets, blasts, routes,
                         a bot brain, `combat.*` tunables (docs/CHIMPDUTY_INTAKE.md)
  crates/evani-net       multiplayer (M1: stub)
  crates/evani-xr        VR (M1: feature detection only)
Dev only (never in a shipped game)
  crates/evani-live      the live control API (window.evani.live)
  tools/evani-live-bridge  local bridge for the Evani.dev tab and MCP
Kits and looks
  kits/evani-kit-<genre>   e.g. kits/evani-kit-example
  looks/evani-look-<style> e.g. looks/evani-look-basic
Apps and tools
  apps/sandbox           test game with a minimal debug panel (tier switch, effect toggles, overlay)
  apps/bench             stress content at every tier along the fixed camera path, pass or fail per tier (GPU bench in headless Chrome via `cargo xtask bench`)
  tools/xtask            `cargo xtask`: evani.toml to Cargo features, Trunk build, size report
assets/tiers/*.json      the quality tier presets (lite, low, medium, high, ultra, vr)
```

A game is an `App`:

```rust
let app = App::new("my-game")
    .with_module(RenderModule::new())          // always
    .with_module(evani_fx::FxModule::compiled()) // behind the game's `fx` feature
    .with_kit(MyKit)
    .with_look(MyLook);
evani_core::web::run(app);                     // wasm: rAF loop, input, overlay, panic hook
// natively: app.run_headless(frames, dt_ms) for tests and tools
```

Each frame runs input, then as many fixed 60 Hz ticks as are due (stages `First`, `PreUpdate`, `Update`, `PostUpdate` and `Last`, then the top scene and events), then the `Frame` stage with `Time::alpha` set, then each module's `frame` and the audio. The overlay updates a few times per second. A hidden tab pauses the loop.

**Frame order.** Module frame steps run in the order the modules were added, except that a module whose `EngineModule::draws` returns `true` (the render module) runs after all the others. So the picture is always drawn with what every module set in that same frame (the nature pass's camera copy, fog, effect switches, draws pushed by a module), even though the render module must be added first (the other modules add their passes to its graph when they are built). Build, start, the overlay and the version line keep the add order. A game-local module that only updates state needs nothing; one that draws the picture itself returns `true` from `draws`. Guarded by `drawing_modules_run_after_every_other_frame_step` (evani-core), `a_later_modules_draws_land_in_the_same_frame` (evani-render) and `the_ocean_is_drawn_with_this_frames_camera` (evani-material-nature).

## Building and running

```
cargo test --workspace
cargo xtask build sandbox            # Trunk release build + per-module size report
cargo xtask build sandbox --ship     # same without the dev-only live module
cargo xtask build --app-dir ../my-game/app --ship   # a game in its own repo/workspace (also: features, render-paths, manifest)
cargo xtask check --app-dir ../my-game/app [--full]  # the per-game gate on that game (also: geocheck, netlog)
cargo xtask review --app-dir ../my-game/app          # its review card (also: critic, size-report [--max-file-bytes N] [--max-total-bytes N])
cargo xtask sizes                    # full / no bloom / no fx / no live table
cargo xtask size-report              # ship builds of sandbox and kart-racer: raw / gzip / brotli per file, wasm-opt proof
cd apps/sandbox && trunk serve       # dev server (default features = evani.toml)
cargo run -p evani-sandbox           # native headless run, prints the overlay
cargo xtask bench                    # GPU bench of every tier in headless Chrome (needs trunk, Node 22+, Chrome)
cargo run -p evani-bench             # the same content natively, CPU time only (no GPU)
cargo xtask review sandbox           # the kit review card in .vibedeck/review/sandbox/ (see "Review card")
cargo xtask check --quick            # while you work: static checks + fmt (advisory), clippy (warnings fail) and tests for the crates changed since the last commit and their dependents
cargo xtask check --full             # once, before review or hand-over: also every game's tests and the ship build
```

`check --quick` picks the crates from `git` (staged, unstaged and untracked files against `HEAD`) and `cargo metadata` (transitive reverse dependents), prints each crate with its reason, reuses the normal target folder and keeps the load low (about half the cores for jobs and test threads, `CARGO_BUILD_JOBS` wins, below-normal priority on Windows). It never writes formatting, and it is not a substitute for `--full` (docs/AI_GUIDANCE.md "The check gate").

Package names carry the `evani-` prefix, so it is `-p evani-sandbox`, not `-p sandbox`. Size builds go to `target/evani-sizes/<app>-<variant>/`, with a JSON report beside them.

## Render

Games must be served over https. For phone testing use localhost through `adb reverse`; a plain LAN address like `http://192.168.x.x` hides WebGPU, and the game then shows "This page needs a secure address".

`evani-render` draws through WebGPU only (wgpu 30, `webgpu` backend; no WebGL). The graph is `forward` (lit instanced meshes into HDR `rgba16float` + `depth32float`), `sprites` (screen-space atlas sprites, alpha-blended into HDR), the effect passes, `tonemap` (HDR to `rgba8unorm-srgb` through the `exposure` and `tonemap` slots, then `render.saturation`) and `present` (copy to the canvas, sRGB-encoded by hand when the canvas format is not sRGB). Disabled passes are skipped. Effect targets such as bloom's half-resolution mips come from a pool keyed by size, format and usage.

**The 3D path is a build-time option** (`evani-render` feature `forward`). Everything 3D lives behind it: the `forward` pass and its WGSL, the `depth` target, 4x MSAA and the depth resolve, the built-in and custom meshes, the lighting and shading uniforms, decals and world-space particles. A 3D kit turns it on in its own `Cargo.toml` (`evani-render = { workspace = true, features = ["forward"] }`; `cargo xtask new-game` writes it, and `meshes`, `shadows` and the `outline` effect turn it on too). A 2D game leaves it off: its graph is `sprites` (which then clears `hdr` to `render.clear_color`), the effects, `tonemap` and `present`, and none of the 3D code reaches its download (measured 2026-10-16: the platformer's ship download is 14.6 KB gzip smaller, match-3's 10.8 KB). The `render.*` lighting parameters stay registered either way, so looks and the live tools see the same schema. A 3D draw in a build without `forward` (a `MeshInstance`, a decal or a `Space::World3d` particle) is not silently dropped: the renderer logs one overlay error naming the fix, and `cargo xtask check` flags a kit whose look is 3D, or whose source draws meshes, while no crate turns `forward` on.

**What kits use.** One narrow resource, `RenderWorld`. A `Stage::Frame` system sets the camera and pushes this frame's draws; the renderer draws them and clears the lists (capacity is kept). The renderer draws after every module's frame step (see "Frame order" above), so a module that copies the camera or pushes draws in its `frame` lands in the same picture:

```rust
let mut rw = world.res_mut::<RenderWorld>();
rw.camera = CameraPath::orbit([0.0; 3], 26.0, 12.0, 60.0).pose(time.render_seconds());
let mut m = MeshInstance::new(MeshId::CUBE, [x, 0.0, z]);   // CUBE, SPHERE, PLANE, BEVEL_CUBE, CYLINDER built in
m.rotation = math::quat_axis_angle([0.0, 1.0, 0.0], angle);
m.material = RenderWorld::material(&look, "glow");           // the look's materials
rw.draw_mesh(m);
rw.draw_sprite(Sprite { pos, size, rotation, cell: atlas::cell::STAR, color, layer: 0 });
```

For placeholder models, `BEVEL_CUBE` (a unit cube with chamfered edges, flat shaded, 44 triangles) and `CYLINDER` (radius 0.5 along Y, 16 sides, 64 triangles) give bevelled silhouettes that catch the light and the ink outlines, where raw cubes and 768-triangle spheres read as primitives.

**Streamed models** (`models`). `let h = models.handle("proof-figure")` returns a `ModelHandle` at once; while `models.resolve(h)` is `None` the kit draws its own placeholder from the meshes above, then `models.draw(ModelInstance::new(id, pos))` with the id it returns. Nothing is fetched until the tier has settled (after the first-run warm-up), then each tier downloads `<id>.<tier>.glb` if the folder has one, else the shared `<id>.glb`, and never another tier's files or a shared file it has its own copy of. LODs inside a file (`<id>_lodK` nodes) are picked by projected screen size with hysteresis. `apps/<id>/assets/models/stream.json` (asset manifest v2: `tier`, `priority`, `group`, content hash and size per file) is written by `cargo xtask manifest <id>`; `[stream] boot = [...]` in evani.toml names the files the title screen waits for, the rest download in the background by priority after it. Files of 64 KB and up go into Cache Storage keyed by content hash (re-fetched when evicted or changed); `navigator.storage.persist()` is asked only on Chromium, where it never prompts. Render only: the simulation never reads a streamed file. `cargo xtask netlog <id> --tier low,ultra` records what each tier really downloads in headless Chrome and fails a wrong file.

**Skinned models** (`evani-render` feature `skinning`, turned on only by a game's `anim` module; without it a game downloads none of it). A skinned `.glb` is drawn by the GPU: linear blend skinning in the model vertex shader, four joints per vertex (u8 indices, unorm8 weights), all of a frame's joint matrices (4x3 each) in one read-only storage buffer with a per-instance offset, and skinned shadows. `models.draw(instance)` draws the bind pose (it looks like the static mesh); `models.draw_posed(instance, rig.skinning(&skin))` draws evani-anim's pose for that one instance (one call per character; a wrong matrix count draws the bind pose). On Lite and Low, which have no shadow map, the self-shadow sun cone is baked once in bind pose and turned with each vertex's limb, so a character keeps the same look on every tier. Rigs over 64 joints still draw but the converter and `cargo xtask check` warn. The sandbox's `skin-proof` feature shows it (`?skin=bind`, `?skin=static`, or three rigs: bind pose and two clips). Format: docs/MODEL_FORMAT.md "Skins".

**Shadows.** With the `shadows` effect on and a tier that gives a `shadow_map` size (Medium 1024, High 2048, Ultra 4096, VR 1024), the renderer draws one directional shadow map from `render.light.direction` (a depth-only pass over a box that follows the camera) and looks it up with PCF in the forward pass; `fx.shadows.distance`, `strength`, `bias` and `normal_offset` tune it. Otherwise (Low, the effect off, or a game without it) kits ground things with blob shadows: `rw.draw_blob(ground, radius)` darkens upward faces in a soft disc (at most 32 a frame, push the important ones first; `rw.blob_strength` sets the darkness).

**Opt-in render settings from the ChimpDuty intake** (each off unless the game's crates turn the feature on, so a game without them ships byte for byte as before; details, numbers and how to switch over in docs/CHIMPDUTY_INTAKE.md):

- *Camera planes* (evani-render `camera-range`, or evani-fx `camera-range` so decals, the outline and fog follow): params `render.camera.near` (default 0.1 m, 0.005 to 10) and `render.camera.far` (default 1000 m, 10 to 100000; kept at least 10x near) for every world projection. A first-person game sets near to 0.01 to 0.03.
- *First-person models* (`view-model-models`): `models.draw_view_model(instance)` (camera space: x right, y up, -z forward) draws a loaded model in the view-model layer with its own field of view, near plane and depth, so a held gun never clips into a wall. Static parts, finest level, no shadow.
- *Looks asking for an effect* (evani-fx `look-effects`): `look.request_effect("fog")` in `LookPlugin::build` (or `request_effect_from(name, min_points)`). **The rule: the tier still wins on cost.** The request only counts for an effect compiled into the game and only on tiers with at least the request's points (default 9: VR, Medium, High, Ultra; Low and Lite veto it); a look's own `post_effect(name, false)` still turns it off; the effect then goes through the tier's budget, the VR comfort rule and adaptive quality like any other, wanted on but never forced.
- *HDR colours* (`hdr-color`): `render.clear_color` takes linear values up to 64, so a bright horizon above 1 goes through exposure and the tonemap like a lit surface. *Own fog colour* (`fog-color`): `render.fog.own_color` (default off: the fog fades to the background, as before) and `render.fog.color` (HDR too with `hdr-color`).
- *Shader pre-warm* (`prewarm`): the canvas keeps its loading state after the engine pipelines exist until the kit's material code set in its first frames is built, no pipeline build has been in flight for 3 frames (at least 8 frames held), and the GPU queue has confirmed the work is done; capped at 6 s. A `prewarm::Prewarm` resource inserted before the first frame changes the numbers and reports `held_ms`.
- *Stable soft shadows* (`pcss-stable`, implies `pcss`): the PCSS lookup compares each tap with the receiver plane's own depth and uses a fixed tap pattern, so a floor under a low sun no longer speckles under a lying body. Same `pcss_params` slot.
- *Shadow fit* (evani-render `shadow-fit`, or evani-fx `shadow-fit`, implies `shadows`): for cameras far from what they look at (a sea, an island map, a strategy board). `fx.shadows.fit = "view"` (default `fixed`, the plain box) makes the shadow box's half-width the camera's reach (eye to target, plus the distance from the box centre to each player's target in split-screen) plus `fx.shadows.view_depth` (30 m), rounded up to whole `fx.shadows.fit_step`s (8 m), capped at `fx.shadows.fit_max` (400 m), never below the plain box. It grows at once but shrinks only when the wanted size is more than half a step below the next step down, so a camera hovering at a step edge keeps one box and one texel grid. `fx.shadows.auto_bias` (default off) keeps the normal offset at 1.5 texels and the depth bias at 0.5 texel or more (the far box's texels: half the map with two cascades); the set `bias` / `normal_offset` stay the minimum.

Sprites are in CSS pixels (`rw.viewport_css` is the canvas size). The atlas is 4 x 4 cells: cell 0 is the look's `checker` generator, the rest are white shapes (tint them with `color`). See `kits/evani-kit-example/src/draw.rs` (the sandbox scene).

**Sprite art** (`evani-render` feature `sprite-art`, turned on by a kit that draws named art; off, a game downloads none of it). A look supplies art from a generator named `sprites` that returns `GeneratedAsset::SpriteSheet { width, height, rgba8, frames: [(name, [x, y, w, h])], pixel_art }`; the render module adds it to the `SpriteArt` resource when the game starts, and a kit can add its own sheets (`SpriteArt::add(ArtSource::Kit, ArtSheet)`). A kit asks by name: `art.cell_or("piece.heart", atlas::cell::CIRCLE)` gives the frame's cell, or the built-in shape when the look has none, so a kit never depends on a look (resolve names after start; a name's cell never changes once given). The built-in cells, the kit's `SpriteSheet` and every named frame are packed into one atlas (deterministic shelf packer, 8 px extruded gutters, atlas up to the device's `max_texture_dimension_2d`; what doesn't fit is an overlay error, never a crash) with a 4-level mip chain built on the CPU, so large art stays smooth when drawn small. Pixel-art sheets keep level 0 and are sampled on texel centres.

**Text** (pixel text; `evani-render` feature `text`, which turns on `sprite-art`; off, a game downloads none of the font or the layout; independent of the sharp text below, which does not need it). One original 5 x 9 bitmap font, ASCII 32 to 126, drawn as text art in `bitmap.rs`, is baked when the game starts into an engine sheet of the `SpriteArt` (the first one, so every glyph's cell is constant) at two sizes: as drawn (10 px line height) and smoothed 2x (20 px). Per glyph it holds a white fill frame `glyph.<code>` and an outline frame `glyph.<code>.outline` (the large size adds `.x2`); the draw tints them, so one baked set serves every colour. Draw with `evani_render::text::draw(&mut rw, [x, y], layer, "Score 120", &TextStyle { size_px: 20.0, color, outline: Some(black), shadow: Some(shade), align: Align::Center })`; it pushes sprites on one layer (shadows, then outlines, then fills) and returns the `[width, height]` that `text::measure(text, &style)` gives without drawing. `pos` is the top of the line, `pos.x` the left edge, middle or right edge by `align`. Sizes 10 and 20 (and their multiples) are drawn without any resampling; other sizes scale the nearer baked size. The advance and the start of the line are whole pixels, so layout is exact and repeatable; a character outside ASCII 32 to 126 (and a newline) draws `?`. There is no wrapping or widgets here: Stage 3's text and UI layer grows from this API.

**Sharp text** (MSDF, `evani-render` feature `msdf`, independent of the pixel font: it needs neither `text` nor `sprite-art`, so a `ui` game ships none of the bitmap font; a game gets it through the `ui` module). The default `TextStyle` names the bitmap font (`FontId::BITMAP`): with only `msdf` on it draws nothing and measures `[0, 0]`, so name a font from `rw.fonts.request`; a game that wants both turns on `evani-render/text` as well. List each font in `evani.toml`: `[ui] fonts = [{ file = "engine/atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf", charsets = ["latin"], name = "ui" }]` (`file` is relative to the game folder, or `engine/<path>` for the engine's `assets/fonts/<path>`; `name` defaults to the file name; charsets `ascii`, `latin` = ASCII plus typographic punctuation, `latin1` = the Latin-1 letters; the font's licence file must sit next to it, and only open-licence fonts such as SIL OFL are allowed, for UI text). `cargo xtask build` bakes each with the pinned msdf-atlas-gen (installed by `cargo xtask doctor --fix` into `.evani/bin`, version and checksum recorded; `cargo xtask fonts <id> [--dist <dir>]` bakes by hand, cached in `target/evani-fonts/`): MTSDF, 24 px em, 6 px range, into `fonts/<name>.png` (decoded by the browser, so no image decoder ships), `fonts/<name>.evf` (a compact table: metrics, advances, glyph boxes, GPOS kerning pairs) and `fonts/<name>.LICENSE.txt`. In code: `let font = rw.fonts.request("ui");` once, then `text::draw(&mut rw, [x, y], layer, text, &TextStyle { font, max_width: 300.0, align: Align::Center, outline: Some(black), ..TextStyle::new(24.0, white) })`. Layout: advances and kerning, greedy word wrap at `max_width` (`\n` always breaks; spaces at a wrap are dropped), each line left, centred or right. Effects (outline, glow, shadow) come from the one atlas. A character the font lacks draws `?`; text in a font that hasn't arrived yet draws nothing. `text::measure_in(&rw, text, &style)` measures without drawing. Up to 4 fonts of 1024 glyphs each. Complex scripts and typed input stay in the DOM. `cargo xtask check` errors on a listed font that is missing, has no licence file or has an unknown charset.

**Box layout** (`evani-ui`, no dependency, no allocation; a game gets it through the `ui` module). A row/column box tree built each frame (`Ui::open`, `Ui::text`, `Ui::close`), laid out once with a measure callback (wire it to `text::measure_in`), read back as rectangles and text runs to draw. Fit, grow and fixed sizes, padding, gap, alignment, text wrap, scrolling. Guide and the sandbox's HUD panel: docs/UI_LAYOUT.md.

**GPU params** (uniforms, never baked into WGSL): `render.exposure`, `render.saturation`, `render.clear_color`, `render.ambient`, `render.light.direction`, `render.light.color`, `render.light.intensity`. Per-frame uniforms go through a 3 x 64 KB ring with 256-byte dynamic offsets and one upload per frame.

**Depth.** The forward pass uses reversed-Z: the perspective maps the near plane (0.1 m) to depth 1 and the far plane (1000 m) to 0, the depth buffer clears to 0 and depth-tested pipelines compare Greater, which keeps float precision even out to the far plane (thin parts 75 to 150 m away no longer z-fight). The convention lives in one module, `evani_render::depth` (`REVERSED`, `CLEAR`, `COMPARE`, `TOWARD_CAMERA`, `is_nearer`, `inverse_distance`); kits, looks and tools that read or write depth use it and never hard-code a direction. The shadow map keeps its own orthographic depth (clear 1, Less). The far plane stays finite.

**MSAA.** Tiers say `msaa` (1 or 4): in a game with `forward`, Medium and up draw the forward pass at 4x and resolve it into `hdr`; the depth is resolved only while an effect reads it. The overlay shows `msaa 4x` while it is on.

**Pixel art.** `render.pixel_art` sets the canvas's CSS `image-rendering: pixelated`, so the browser's upscale of a lower-resolution canvas is nearest-neighbour instead of smooth. No shader or pass is involved.

**Tiers.** The canvas is CSS size x min(devicePixelRatio, tier `dpr_cap`) x tier `render_scale` x the adaptive-quality scale (a ResizeObserver tracks the CSS size). `tier.set` or the panel switches tier on the next frame: targets and bind groups are rebuilt, pipelines are reused, the budget re-runs. The VR preset can only be picked inside an immersive WebXR session (`tier.set vr` elsewhere is `needs_xr_session`). On the first run a 2-second warm-up picks the highest auto tier that holds its frame target and saves it (SaveData key `render.tier`); later runs load it.

**Adaptive quality** (ships in games): judges work time (CPU ms in the frame callback plus GPU ms, smoothed), never the vsync-bound frame interval, so a light frame at 60 Hz is never "slow". Over its target it degrades with hysteresis and a cooldown, first by pausing the effect with the lowest value per point (the panel shows it as "on, paused to hold frame rate"), then by stepping the render scale down 0.05 at a time to a floor of 80 % of the tier's scale; it recovers in reverse. It judges the window's 80th percentile (`aq.percentile`), not the average, so one slow frame never cuts quality. Frames stalled on purpose (captures, a held clock, bench or A/B runs) are not judged, nor are frames during a pipeline rebuild or shader compile, a tier switch or a live change, or for `aq.grace_ms` (1 s) after one (the overlay says "quality settling"). Each step's saving is measured when it is taken and scaled to today's work when deciding whether it fits back, and a step that stays blocked is tried anyway after a growing wait, so everything comes back after a load ends. **Fast recovery:** once work stays under `aq.fast_pct` (50 %) of the target for `aq.fast_ms` (1 s), every paused step whose measured cost still fits comes back, one per frame, so after a load ends full quality is back in about 1.5 s (unit test: under 5 s at 30, 60 and 144 Hz); slower steps still come back one per upgrade window near the limit. A fast recovery that has to be cut again within 10 s doubles its wait, so a load that comes and goes cannot make it flap. It ignores the first 1.5 s after a device start or recovery, resets on a tier change and when the tab is shown again, and doubles the cooldown under prefers-reduced-motion. Every number is a param: `aq.enabled`, `aq.target_ms`, `aq.window_ms`, `aq.degrade_pct`, `aq.hysteresis_pct`, `aq.degrade_frames`, `aq.upgrade_frames`, `aq.cooldown_ms`, `aq.render_scale_step`, `aq.render_scale_floor`, `aq.degrade_order`, `aq.percentile`, `aq.grace_ms`, `aq.fast_pct`, `aq.fast_ms`.

**Slot rebuilds** (for the live stage): `SlotRebuilds::request(&slots, "tonemap", body, now_ms)` returns a handle. The renderer recompiles every pipeline that uses the slot inside an error scope and swaps them in at the next frame boundary, only if all compiled; `take_result(handle)` then gives a `SlotCompileResult` (`ok`, `ms`, `kept`, `messages` with body line and column, `pipelines_rebuilt`). A rejected body keeps the old pipelines and writes a note to the overlay. With the `hooks` feature (the sandbox's `live` feature turns it on) the page also has `window.__engine.slotRebuild(name, body)` (a Promise of that result) and `window.__engine.loseDevice()`.

**Errors.** Uncaptured GPU errors go to the overlay. A lost device is reported and everything is rebuilt on a new device. The overlay shows fps, CPU ms, GPU ms and per-pass GPU ms (timestamp queries; without them a submit-to-done estimate marked `~`), the work time against its target, draw calls, triangles, sprites, the render size and the budget points.

## Bench

`apps/bench` spawns stress content sized from each tier's own limits and runs it on every tier (Lite, Low, Medium, High, Ultra, VR) along the fixed camera path. Per tier there are two phases, each with a settle part (drawn, not measured: pipelines, targets and the effect budget settle) and a measured part:

- **3D:** a grid of cubes and spheres filling 90% of `max_triangles`.
- **2D:** 90% of `max_sprites` drifting atlas sprites over a light 3D backdrop.
- **Characters** (S3-9): 256 skinned rigs over the same backdrop, each posed every frame by evani-anim (three clips at staggered times) and drawn with its own joint matrices, so the CPU posing and the GPU skinning are both in the measured frames. Its triangles count towards the triangle check and its content-drawn check.

The camera is `CameraPath` advanced by exactly 1/60 s per frame, so every run sees the same frames. Adaptive quality is switched off (`aq.enabled = false`) and the tier is forced, so it measures the tier, not the controller.

```
cargo xtask bench                       # build (Trunk release), serve, run headless Chrome, print the table
cargo xtask bench --tier low,vr         # some tiers only
cargo xtask bench --no-build            # reuse target/evani-bench/dist
cargo xtask bench --shots               # also save a picture per phase in target/evani-bench/shots (longer phases)
cargo xtask bench --frames 600 --warmup 60 --chrome <path> --timeout-s 300
cargo xtask bench --runs 3              # three runs, then the mean of them (each run's table too)
cargo xtask bench --tier high --msaa 1  # force MSAA 1 or 4 on every tier (what MSAA costs)
cargo xtask bench --off bloom,outline   # switch effects off for the run (what they cost)
```

It needs `trunk`, Node 22 or newer and Chrome (or Edge, or Playwright's headless shell; `EVANI_CHROME` or `--chrome` picks one). `tools/bench-runner/run.mjs` is the small Node script that starts Chrome (`--headless=new --enable-unsafe-webgpu --enable-webgpu-developer-features`, frame-rate limit and vsync off), talks to it over raw CDP, runs one fresh page per tier at the tier's size (its `target_resolution` if it has one, else 1920 x 1080) and stubs `requestPointerLock`, `requestFullscreen` and `keyboard.lock`. The command exits non-zero if any tier fails or the run errors.

Each phase settles for `warmup` frames counted after the last pipeline compile or swap (at most 10 x `warmup` in all), so a fast tier never measures before its effects' pipelines are ready. Under the table, `gpu passes <tier>` lists the mean GPU ms of every pass in the 3D phase.

**GPU cost order check.** The run fails unless GPU time rises Lite <= Low <= Medium <= High <= Ultra (VR is mono and left out). It judges the median GPU ms per frame (with `--runs N`, the median of the runs), printed as `gpu median ms per frame`. A tier may measure up to 10 % (`ORDER_TOLERANCE_PCT`) cheaper than the one below, and differences under 0.05 ms (`ORDER_TOLERANCE_MS`, the timestamp resolution) never fail: run-to-run noise of one tier on the same machine is a few percent, and the real step between two tiers is two to four times the work. Medians, because GPU timestamps on a desktop card swing several times over with its clock state (a lightly loaded card clocks down and every pass then reads slower) and a few slow frames move a mean but not a median. Compare tiers only from the bench (uncapped, the card clocked up), never from the overlay of a vsync-capped game: there a light tier leaves the card idling at a low clock and its passes read slower than a heavy tier's.

Each row of the table: tier, measured frames, avg / p95 / p99 frame ms (the worse of the two phases), GPU ms (timestamp queries, `n/a` without them), draw calls (largest seen), triangles (3D phase), sprites (2D phase), budget points used/total and the verdict. A tier passes when:

| Check | Rule (numbers from `assets/tiers/<tier>.json`) |
|---|---|
| frame-time target | avg frame ms <= 1000 / `fps_target`, plus 10% (the warm-up's tolerance) |
| frame-time floor | p99 frame ms <= 1000 / `fps_floor`, only where the JSON gives one (Low) |
| draw calls | <= `max_draw_calls` |
| triangles | <= `max_triangles` (3D phase) |
| sprites | <= `max_sprites` (2D phase) |
| budget points | used <= `points` |
| content drawn | the renderer really drew at least 80% of what the phase was sized to (a run that draws nothing fails) |
| render errors | no errors logged during the tier |

VR runs mono at the VR preset (`(mono, preset only)` is printed under the table); there is no stereo view in M1. The page shows the same table and sets `window.__bench = { done, error, mode, progress, results }` (the `results` are the rows plus both phases' numbers). `apps/bench/index.html` can also be opened by hand with `?tier=low,high&warmup=N&frames=N`.

**Device mode** (`?device=1`, for a real phone or tablet opened by hand): the saved tier is cleared so the renderer's own first-run warm-up runs, exactly as on a game's first visit, while the bench draws the 3D content of whichever tier the warm-up is trying (each tier's own content, not the `?stress=1` scene, which is sized for a desktop card and would push every phone down to Lite). Then every tier is benched as usual and only the tier the warm-up picked is judged: the page starts with `device picked <tier>: PASS` (or `FAIL` and the failed checks), the other rows read `info PASS` / `info FAIL ...`, and `window.__bench` gains `device_pick` and `device_pass`. The saved tier is put back at the end. If no tier is picked within 30 s (the tab hidden, no GPU timing) the run ends with an error. On a phone the page runs under the display's vsync, so 16.7 ms is the best possible frame time.

`cargo run -p evani-bench` is the native run of the same content and checks: the render module only counts what it would draw, frame ms is the engine's CPU time and GPU ms is n/a. It stays as a quick check that needs no browser.

## Publish

`cargo xtask publish <id>` turns `apps/<id>/evani.toml` into a checked package for the Evani.app library in one command: the ship build, the package put together in a staging folder and swapped into `publish/<id>/` (or `--out <dir>`) only when the preflight passes, the size report (by module, then by file; raw, gzip and Brotli; the 10 MB cap is on the raw whole folder; the catalog sets no per-tier size targets, and the report says so), and the diff against the version recorded as live (NEW / MODIFIED / SAME / DELETED, the size change, "nothing changed, skip" when identical). `--dry-run` writes only to `.evani/publish/<id>/dry-run/`; `--serve` serves the package at `http://localhost:8114/<id>/` for a look first; `--quick` skips the per-module builds; `--from <dir>` names the previous package to carry README, licences, preview, `evani.json` and the loader over from. The summary (`summary.json`, `summary.md`) and the version history stay in `.evani/publish/<id>/`, outside the uploaded folder.

It never uploads anything. After you upload a package yourself, `cargo xtask publish <id> --mark-published v5` records it as live so the next diff is against it. `cargo xtask package-check <folder> <id>` runs the same preflight on any package folder. Details: docs/PUBLISHING.md.

## Review card

`cargo xtask review <app-or-kit>` (an app under `apps/`, or the app whose `evani.toml` names that kit) makes the card a kit is handed over with (docs/KIT_PROCESS.md step 9) in `.vibedeck/review/<app>/`:

- `card.md`: a plain summary on top (speed test, download size and its change, what's in it, pictures, what to try, checklist counts), then the details: the bench table per tier (Lite, Low, Medium, High, Ultra, VR) with each failing check and its numbers, the shipped build's gzip sizes and wasm markers with the build-time on/off check (any `evani.module.*` or `evani.shader.*` the manifest doesn't list fails it, except ones the app, kit or look declare themselves), the modules and effects from `evani.toml`, the three captures next to the reference image prompt, what to try, and the "Done when" checklist parsed from docs/KIT_PROCESS.md with build and size, bench and docs ticked or failed automatically and the rest marked for a hand check.
- `card.json`: the same data, `capture-1.png` to `capture-3.png`: 640 px wide `capture.frame` shots on the game's bench camera path at sim time 0, 20 and 40 s (`tools/live-check/capture.mjs --times`, which reuses this project's bridge or starts one).
- `.vibedeck/review/history.json`: the size of every card made, for the change against the previous card (the previous card for the same kit; else the nearest earlier kit in ROADMAP order with a card; else the most recent card).

The bench is rerun only when `target/evani-bench/results.json` is missing or older than any file under `crates/`, `apps/bench/`, `assets/tiers/` or `Cargo.lock`; `--rebench` forces it. The reference image prompt comes from the kit's SPEC.md ("Reference image prompt" heading), else from the catalog template when the kit and look are catalog entries, else the card says there is none. What to try comes from `[review] try = [..]` in `evani.toml`, else a "What to try" section in the kit README. Options: `--rebench`, `--port <P>` (bridge port, default 8096), `--chrome <path>`. It exits non-zero only when the card is incomplete (no captures or no bench result); FAILs are findings on the card.

## evani.toml

Each app has a module manifest next to its `Cargo.toml`:

```toml
name = "sandbox"
kit = "example"                   # crate kits/evani-kit-example
look = "basic"                    # crate looks/evani-look-basic
effects = ["bloom", "vignette"]   # each is an evani-fx feature; [] = no fx module at all

[modules]
physics = "simple"                # "simple" | "rigid" | "none"
net = true
xr = true
live = true                       # dev only; --ship always drops it
# models = true                   # Stage 3: .glb models (off unless listed)
# anim = true                     # Stage 3: skinned / animated models; implies models
# ui = true                       # Stage 3: fonts and menus
# nature = ["ocean"]              # evani-material-nature and these of its parts; [] = the crate alone
# proc = ["sky"]                  # evani-material-proc and these of its parts (sky, relief)

[review]                          # optional, for `cargo xtask review`; never a feature
try = ["Toggle bloom in the panel"]

[playground]                      # a game for the Evani.app library; never a feature
free_roam = true                  # required: every playground supports free roam
```

`[playground]` marks a game made for the Evani.app library (docs/PLAYGROUNDS.md). Every playground supports free roam, so it must say `free_roam = true`, its kit must return true from `KitPlugin::free_roam`, and a `#[test]` must call `evani_core::play::free_roam_test`. `cargo xtask check` reports each gap as an ERROR and `cargo xtask build <app> --ship` refuses the build. `cargo xtask new-game` adds the table; the sandbox and the current kits' games don't have it yet. The whole ship folder (wasm, js, html, assets) must stay under 10 MB uncompressed (PACKAGE_CAP_BYTES in evani-guide); `build --ship` prints the total and refuses a bigger one, and `check` warns when the last ship build is over. There is no world-size requirement.

`cargo xtask build <app>` turns this into the app crate's features and always passes `--no-default-features`. Non-empty `effects` turns on `fx` plus one feature per effect. Physics turns on `physics-simple` or `physics-rigid`, and `net`, `xr` and `live` turn on features of the same name. So do the Stage 3 modules `models`, `anim` and `ui` (`anim` implies `models`; the app crates have the features already, and each gains its optional engine crate when that crate lands, so a game that lists none of them downloads none of it; `models` also turns on evani-core's `texture-budget`, which adds `TierPreset::max_texture_memory`, in megabytes, from `assets/tiers/*.json`). `cargo xtask check` makes it an ERROR to ship `.glb` files without `models`, a skinned model without `anim`, or a font file without `ui`. The material modules `nature = [..]` and `proc = [..]` turn on the app features `nature` / `proc` (the crate: evani-material-nature, evani-material-proc) and `nature-<part>` / `proc-<part>` for each listed part (one of the crate's own features, e.g. `nature-ocean`, `proc-sky`); `[]` is the crate with no optional part, and an absent key builds none of it. The kit or the app carries the crate as an optional dependency (`nature = ["dep:evani-material-nature"]`, `nature-ocean = ["nature", "evani-material-nature/ocean"]`, or forwarded to a kit feature). `check` makes it an ERROR to list a part the crate has no feature for, to list a module nothing depends on, or for the kit's own Cargo.toml to depend plainly on any optional engine crate (every game on the kit would download it). The app crate forwards each feature to an optional dependency, for example `bloom = ["fx", "evani-fx/bloom"]`, so anything not listed is never compiled. xtask refuses an `evani.toml` that names a feature the crate doesn't have, or a kit or look the crate doesn't depend on.

The size report gives the raw and gzip size of the wasm, the JS and the total. Then it rebuilds once with each listed module switched off and prints the difference, and once with everything optional off (the "base"). It also lists the `evani.module.*` and `evani.shader.*` markers found in the wasm, which proves what a build contains.

## How to add things

### A kit

1. Create `kits/evani-kit-<genre>` (add it to the workspace members and `[workspace.dependencies]`).
2. Implement `KitPlugin`: `name()` returns `<genre>`, and `build(&self, kit: &mut PluginContext)` registers components (`kit.component::<T>("<genre>.thing")`), systems (`kit.system(SystemDesc::new(name, Stage::Update, f))`), scenes, input actions (`kit.action(ActionDef::button("jump", &["key:Space", "pad:button:0"])?)`), data files, params and formulas. `start_scene()` names the first scene.
3. Name every param `kit.<genre>.*`. The context stamps the owner `kit.<genre>` on it.
3a. A kit that draws 3D (meshes, decals, world-space particles; any game whose look's style is `3d`) turns on the 3D path in its `Cargo.toml`: `evani-render = { workspace = true, features = ["forward"] }` (see Render). A 2D kit leaves it off.
4. Add it to the game crate and to its `evani.toml` (`kit = "<genre>"`).
5. Free roam (every playground needs it): read the `PlayMode` resource (`evani_core::play`; `?mode=free` or `#mode=free` on the web, `App::with_play_mode` before `.with_kit(..)` natively). Under `PlayMode::FreeRoam` keep the same world with no goals, timer, lives, fail state or game over. Report fail events on `PlayOutcomes` in every mode, return true from `KitPlugin::free_roam`, and add a test that runs `free_roam_test(&mut app, seconds, &script)` with scripted keys.

See `kits/evani-kit-example` for the smallest complete one (its `free_roam_holds` test is the free-roam pattern).

`cargo xtask new-game <id> --genre <genre> [--look <look>] [--effects a,b|none]` starts from a catalog genre (Stage 6, "New Game"): the look must be one of the genre's `compatible_looks` (with no `--look`, the first that has a crate), `[modules] physics` is the genre's, every effect must be in `catalog/effects.json`, work with the look's style class and exist in `evani-fx` (without `--effects`, the template's bloom, vignette and shadows that fit the look), and `kits/<id>/SPEC.md` is written with the gameplay systems and the pinned reference image prompt built from the catalog's scene line. `cargo xtask catalog [--json]` lists the genres ranked by most played (the catalog's `reach_index`; there are no live play counts). Without these flags `new-game` is unchanged.

#### How a kit puts gameplay objects in the ECS

Karts, props, pickups and projectiles are entities, not entries in a kit resource: then the overlay's `entities` count, the ordered system loop, deterministic iteration and every later engine feature that reads the world (snapshots, review captures, networking) see them. Per-race or per-level settings and scores can stay in a resource.

```rust
struct RaceObject;                                 // marker on everything a race spawns
struct Kart { slot: u8, speed: f32 }

// build(): kit.component::<Kart>("kart_racer.kart"); kit.component::<RaceObject>("kart_racer.object");
let e = world.spawn();
world.insert(e, Kart { slot: 0, speed: 0.0 });
world.insert(e, RaceObject);

for e in world.sorted_entities::<Kart, _>(|k| k.slot) { /* fixed gameplay order */ }
for (a, b) in pairs {                                 // kart against kart
    if let Some((mut ka, mut kb)) = world.get2_mut::<Kart>(a, b) { /* bump both */ }
}
world.despawn_all::<RaceObject>();                    // reset the race in one call
let karts = world.count::<Kart>();
```

- `world.query::<&mut Kart>().for_each(|e, k| ...)` (and tuples such as `(&Kart, &mut Pos)`) iterates in dense order: deterministic for the same spawn and despawn history, but a despawn swap-removes, so the order changes. When gameplay depends on order (who wins a tie, which pair resolves first) give the component an explicit slot and use `sorted_entities`.
- `entities_with::<T>()` returns the list when the loop changes the world (spawning, despawning, pairs).
- Tunables stay params (`kit.<genre>.*`), read in the system each tick; the component holds state, not tuning.

### A look

Implement `LookPlugin`: `name()`, `points()` (the look's own budget cost), `styles()` (`StyleClasses::THREE_D`, `PSEUDO_3D` or `TWO_D`), and `build(&self, look: &mut LookContext)`. In `build` you can:

- add palettes and materials (`look.palette`, `look.material`);
- set shader slot bodies (`look.shader_slot("tonemap", BODY)?`);
- ask for effects to be on or off (`look.post_effect("bloom", true)`);
- override another module's param default (`look.post_param("fx.bloom.threshold", json!(1.1))`);
- add procedural generators (`look.generator("checker", f)`);
- add params named `look.<style>.*`.

A look can veto an effect but cannot force one past the tier budget. See `looks/evani-look-basic`.

### A parameter

```rust
let speed = ctx.param_f32(
    ParamDef::f32("kit.example.spin_speed", 1.5)
        .range(0.0, 20.0).unit("rad/s")
        .describe("How fast the example spinners turn."),
)?;
// hot path, no allocation:
let v = world.res::<ParamRegistry>().f32(speed);
```

The types are `f32`, `i32`, `bool`, `vec3`, `color` and `enumeration`. Values are clamped to the range. Add `.gpu()` for values that feed uniforms (dirty tracking via `revision()` / `changed_since`). Add `.bake(BakeTarget::…)` (`Look`, `Kit`, `Tier`, `Policy`, `None`) to choose where `bake.export` writes the value; the default `Auto` picks from the owner (`kit.*` → kit, `tier.*` → tier, `aq`/`budget` → policy, `live.*` → never, anything else → look). The param shows up in `schema.get` and is live-tunable with no other code. Shipped games get the baked value back through the crate's baked files (see "Baked files" below).

### A curve or formula

```rust
ctx.formula("kit.example.boost_scale", "Speed scale from the boost button",
    &["held", "boost"], FormulaJson::Expr("mix(1, boost, held)".into()))?;
ctx.formula("look.basic.fog_curve", "Fog by distance", &["d"],
    FormulaJson::Curve(Curve::new(&[[0.0, 0.0], [50.0, 0.4], [200.0, 1.0]])?))?;
let y = world.res::<Formulas>().eval(id, &[held, boost]);  // no allocation
```

Expressions support numbers, `+ - * /`, unary minus, parentheses, `min`, `max`, `clamp`, `mix`, `smoothstep`, `pow`, `abs` and named variables. They are parsed once to bytecode, with length and depth limits, and have no loops.

Give a curve axis ranges so live edits are clamped and `curve.get` reports them: `world.res_mut::<Formulas>().set_ranges(id, Some([x0, x1]), Some([y0, y1]))`. Live, a `FormulaJson::Curve` is edited with `curve.set` and a `FormulaJson::Expr` with `slot.set` (as an expression slot). The owner prefix (`kit.*` or `look.*`, stamped by the context) decides which crate `bake.export` writes it into.

### An effect

1. Add a feature to `crates/evani-fx/Cargo.toml` and a module `src/<name>.rs` behind `#[cfg(feature = "<name>")]`.
2. Implement `evani_render::PostEffect` and `Effect`:
   - `info()` returns a static `EffectInfo` (name, cost 0–3, value weight, styles, `vr_comfort_off`);
   - `register` adds the effect's `fx.<name>.*` params;
   - `passes` adds its passes (and any pooled targets) to the `RenderGraph`;
   - `shaders` lists its WGSL files as `EffectShader`s (`include_str!`, first line `// evani.shader.<name>`, the slots it uses, blend mode, whether it samples an input);
   - `write_uniforms` packs one shader's uniforms from the params (no allocation);
   - `record(pass, reg, rec)` issues the pass's draws on the `PostRecorder` (`rec.draw(shader, src, dst, uniforms)`, usually through `draw_effect`). The renderer owns the pipelines, bind groups and targets.
3. Add it to `FxModule::compiled()` and `FxModule::COMPILED`, and forward the feature from each game crate (`<name> = ["fx", "evani-fx/<name>"]`).

The fx module registers `fx.<name>.enabled` and `fx.<name>.value_weight` for you, and the native tests validate every WGSL file with naga.

### Particles

Sparks, sparkles, dust and confetti go through the shared GPU particle system; a kit never keeps its own particle pool. From a fixed-step system, push into the always-present `evani_core::particles::ParticleQueue` resource: `queue.emit(Emit::new(pos, vel, color, life, size, shape, space))` (then set `gravity`, `drag`, `spin`, `aspect`, `size_end`, `additive` on the `Emit` as needed), or `queue.burst(&mut rng, n, base, Spread { speed, life, size })` for `n` at once with randomness from the kit's own `Pcg32`. `space` is `Space::Screen2d { layer }` (CSS pixels, y down, drawn between the sprite layers) or `Space::World3d` (world units, camera-facing quads, depth-tested); `shape` is an atlas cell (a built-in shape, or a named frame with `sprite-art`). Kits call it without a feature: the system only exists when the game lists the `particles` effect in `evani.toml`, otherwise every emit is a no-op and nothing reaches the download. Particles are visual only and never feed back into the game; at most the tier's `max_particles` (lowered by `fx.particles.max`) live at once, and the oldest is recycled at the cap. `queue.emit_ago(ticks_ago, emit)` stamps an emit with an earlier tick so it appears already aged (a kit that replays a moment to capture it uses this); `queue.clear()` drops every live and waiting particle (a restart or a new level). `fx.particles.density` and `fx.particles.gravity` are hints a kit may scale its emits by.

### Pseudo-3D road

The classic arcade road view (OutRun-style: a segment-projected road with curves, hills, roadside sprites and a scrolling backdrop) is the evani-render feature `pseudo3d` (`crates/evani-render/src/pseudo3d.rs`, GPU side `gpu/pseudo3d.rs`, `shaders/pseudo3d.wgsl`). A kit turns it on with `evani-render = { workspace = true, features = ["pseudo3d"] }`; it needs neither `forward` nor anything else, and games without it download none of it. Build the track once with `RoadTrack` (`straight`, `bend`, `hill`, `add(enter, hold, leave, curve, rise)`, `close`, `sprite`, and `scatter(&mut rng, ...)` with the kit's `Pcg32`), insert `RoadScene::new(track)` as a resource, and move `scene.camera.z` (distance along the road) and `scene.camera.x` (lateral, in road half-widths) from the fixed step. Gameplay reads the road under a car with `track.curve_at(z, seg)` and `track.height_at(z, seg)`; the track is built with plain arithmetic, so it is the same on every machine. The renderer projects it every frame into quads drawn back to front in one instanced call at the start of the sprites pass, so the kit's car and HUD sprites sit on top; colours are `scene.look` (`RoadLook`). Live tunables: `render.pseudo3d.curve`, `.hill`, `.draw_distance`, `.segment_length`, `.fog`, `.camera_height`, `.fov` and `.road_width`. The sandbox proof is `trunk build --features pseudo3d-proof` in `apps/sandbox` (`src/proof_road.rs`).

### A shader slot

A slot is a WGSL function with a fixed signature (`slot_<name>`) whose body can be replaced, by a look at build time or live at runtime. The built-in slots are `tonemap` and `exposure` (tonemap pass), `bloom_weight` (bloom), `toon_ramp` and `fog` (forward), declared in `evani_core::slots`. To add one:

1. Declare a `SlotSignature` (name, params, return type, description, default body) with `ShaderSlots::declare`.
2. Put a line `//@slot <name>` at module scope in the WGSL of the pass that uses it, and call `slot_<name>(...)` there. `evani_render::splice` replaces the marker with the current body and keeps a line map, so compiler messages point at body lines.
3. For an effect shader, list the slot in its `EffectShader.slots` (the fx tests check that markers and list agree).

Nothing else: the pipeline cache finds every pipeline whose WGSL uses the slot, so live rebuilds (`SlotRebuilds`) cover the new slot too. A look supplies its own body with `LookContext::shader_slot("tonemap", BODY)?`. Live, `slot.set` replaces the body: the page checks its structure, the renderer recompiles through `SlotRebuilds` and swaps at a frame boundary, and the old pipeline keeps running if it fails. `bake.export` writes WGSL slot bodies to the look crate as `slots/<name>.wgsl`.

### Baked files

`bake.export` writes into the game's crates: `params.json`, `policy.json`, `slots/*.wgsl`, `slots/*.expr` and `curves/*.json` in the look or kit crate (plus `CHANGES.md`), and `assets/tiers/<tier>.json`. A shipped build without live reproduces them at build time, with no live code and no file access at runtime. Each look and kit crate:

1. sets the shared build script in its `Cargo.toml`: `build = "../../tools/bake-index.rs"` (it embeds the crate's baked files as `BAKED`);
2. includes the generated list: `include!(concat!(env!("OUT_DIR"), "/baked.rs"));`
3. hands it to the core in `build`: `ctx.baked_files(BAKED);` (`look.baked_files(BAKED)` in a look).

The core applies baked files at start, after every module has registered and after a look's `post_param` overrides, so baked values win. Unknown names become overlay notes, because the module may be compiled out. See `looks/evani-look-basic` and `kits/evani-kit-example`.

### How data ships

The JSON files stay the source of truth: `catalog/*.json`, `assets/*.json` and kit data are what people, the live tools and `bake.export` read and write. What reaches the download is kept small:

- **Tier presets are compiled.** `crates/evani-core/build.rs` reads `assets/tiers/*.json` at build time and writes a static table (`BUILTIN_TIERS`); `Tiers::builtin()` builds the presets from it, so no game parses tier JSON at start and the JSON text is not in the wasm. A malformed tier file fails the build. The build reruns when any tier file changes, so a `bake.export` into `assets/tiers/<tier>.json` reaches the next build (and `cargo xtask dev --hot`) by itself. Live tuning is unchanged: `tier.set`, tier-scoped `effect.toggle` and `tier.<id>.<field>` params edit the running `Tiers` resource, not the table. `Tiers::from_builtin_json()` still parses the JSON (tools and tests; a test checks both give the same presets).
- **Render paths** are a Rust table already (`evani_render::render_path`); `assets/render_paths.json` is for `cargo xtask check`. The catalog is never read at runtime.
- **Baked params, curves and slots** (`params.json`, `curves/*.json`, ...) and kit data (levels, art) are still embedded as JSON and parsed at start: they go through the parameter registry's JSON values, which ship anyway.
- **No float formatting in shipped code.** `format!("{:.1}", x)`, `{}` or `{:?}` on an `f32`/`f64`, and `f32::clamp` with bounds the compiler can't check (its panic message prints the bounds) link core's float-to-decimal code into the game. Shipped code uses `evani_core::num` instead: `fixed(x, n)` writes exactly the `{:.N}` text, `short(x, n)` the `{}` text for numbers with up to `n` decimals (CSS lengths), `short_debug` the `{:?}` text, `clamp` the clamp without the panic. Tests compare each with the standard library. Tests, native tools and dev-only code may format floats as usual.

## Touch controls

Phones and tablets have no keys, so a game that needs them declares on-screen controls. The layer lives in `evani-core` behind the feature `touch` (`crates/evani-core/src/touch.rs`, page side `src/web/touch.rs`); a kit opts in with `evani-core = { workspace = true, features = ["touch"] }` in its `Cargo.toml`, and games whose kit doesn't download none of it. Games played by tapping the board (match-3) need nothing: taps are the primary pointer.

- **Controls only set actions.** Every button and stick names an action the kit already binds to keys and pads (`kit.action(..)` first). A held button sets it like a held key (same `down`, `just_pressed`, `just_released`, frame for frame); a stick sets an axis. Analog values are quantised to 1/127 steps before `Input` sees them and touch input is applied once per frame like every other device, so the simulation can't tell touch from keys and stays deterministic. Two buttons on one axis (left -1, right +1) add up like `keys:KeyA/KeyD`.
- **Controls:** `Button::hold` (down while the finger is on it) and `Button::tap` (one press edge per touch); `.value(-1.0)` for one side of an axis, `.primary()` for the thumb's main button, `.at(Anchor, x, y)` in `vmin` from a corner inside the safe area. `Stick::horizontal` / `vertical` (axis lock) / `two_axis`, dynamic by default (the base appears under the thumb inside its `Zone`, `Zone::LEFT_HALF`) or `.fixed()`, radial dead zone 0.12 by default (`.dead_zone(0.10..0.15)`) rescaled so steering isn't jumpy; y is down-positive like a gamepad. `TouchLayout::pause(action)` adds the small top-right pause button touch players need; `pause_at(action, anchor, x, y)` puts it clear of the game's HUD (the kart's timer and minimap sit top right). A stick's label sits above its ring. Check a screenshot that no control covers the HUD. `hold_while_shown(action)` holds an action while the layer is in use (auto-accelerate).
- **Fingers:** Pointer Events, one pointer id per finger and one finger per control, `setPointerCapture` so sliding off still releases; `pointerup`, `pointercancel` and `lostpointercapture` all release; window blur, a hidden tab and any key (Esc included) let go of everything. Touches on controls never reach the game's own pointer handling.
- **Shown only on touch:** hidden unless the page matches `(pointer: coarse)` at start; the first touch going down shows it; the first key or gamepad input hides it (a tablet with a keyboard switches back and forth). The kit's own switch, `TouchControls::set_enabled` (or `evani_core::touch::set_enabled(world, on)`), turns it off on title screens and menus played by tapping the page's buttons; it sits under the page's `#ui` menus (`z-index: 850`).
- **Page rules (built in):** `touch-action: none`, no selection, long-press menu or tap highlight on the controls, `overscroll-behavior: none`, `viewport-fit=cover` added to the viewport meta and safe-area insets around every control, sizes in `vmin` with floors (never under 48 CSS px; primary 64 to 88 px; sticks 96 to 176 px). It never asks for fullscreen or pointer lock. Restyle with `#evani-touch [data-id=jump] { ... }` in the game's CSS.

```rust
use evani_core::touch::{Anchor, Button, Stick, TouchLayout, Zone};

// Kart racer (in build(), after the p1_* actions): auto-accelerate under touch,
// steer with the left thumb, drift and item under the right thumb, pause left of the minimap.
kit.touch_controls(
    TouchLayout::new()
        .hold_while_shown("p1_throttle")
        .stick(Stick::horizontal("steer", "p1_steer").dynamic(Zone::LEFT_HALF).at(Anchor::BottomLeft, 8.0, 16.0).label("Steer"))
        .button(Button::hold("drift", "p1_drift", "Drift").primary().at(Anchor::BottomRight, 4.0, 6.0))
        .button(Button::tap("item", "p1_item", "Item").at(Anchor::BottomRight, 28.0, 4.0))
        .button(Button::hold("brake", "p1_brake", "Brake").at(Anchor::BottomRight, 6.0, 30.0))
        .pause_at("pause", Anchor::TopRight, 26.0, 3.0), // left of the timer and minimap
)?;

// 2D platformer: left/right on the left, a big jump on the right.
kit.touch_controls(
    TouchLayout::new()
        .button(Button::hold("left", "move", "\u{25C0}").value(-1.0).primary().at(Anchor::BottomLeft, 4.0, 5.0))
        .button(Button::hold("right", "move", "\u{25B6}").primary().at(Anchor::BottomLeft, 26.0, 5.0))
        .button(Button::hold("jump", "jump", "Jump").primary().at(Anchor::BottomRight, 5.0, 5.0))
        .pause("pause"),
)?;
```

The platformer keeps coyote time (about 80 to 100 ms of jump after leaving a ledge) and a jump buffer (about 100 ms of remembered press) in its fixed-step sim; they matter more on touch, where taps are less precise. Turn the layer off while the page's menus are up (`set_enabled(world, false)` on the title, pause and results screens). Landscape is the primary layout; leave the middle of the screen clear. Tests: `cargo test -p evani-core --features touch touch`.

### Look pad (touch look)

For a first-person game the same layer has a drag area that turns the view (evani-core feature `touch-look`, which brings `touch`; games that don't turn it on carry none of it): `TouchLayout::look_pad(LookPad::new("look", "look_x", "look_y"))`. It adds the finger's movement (CSS pixels, in 1/16 px steps, `.sensitivity(1.5)` to scale, `.invert_y()`) to the two axis actions every frame, in the same units as `mouse:dx` / `mouse:dy`. So the kit binds its look actions once (`ActionDef::axis("look_x", &["mouse:dx", "pad:axis:2"])`) and the mouse, the gamepad and the pad all drive that one action; there is no separate touch path. The pad is invisible, covers the right half below the top quarter by default (`.zone(Zone::..)`), loses to any button or stick placed over it, and releases on lift, cancel, blur and a hidden tab like every control.

## Pointer lock

Mouse-look on a desktop: the feature `pointer-lock` of `evani-core` (a kit turns it on with `evani-core = { workspace = true, features = ["pointer-lock"] }` and declares `kit.pointer_lock()` in `build()`; games that don't carry none of it, not even a listener). The lock hides the cursor and gives raw, unbounded movement through the same `mouse:dx` / `mouse:dy` bindings, so look is one action for mouse, pad and touch. The kit asks with `evani_core::pointer_lock::request(world)` when play starts (the browser grants it at the player's next click or key) and gives the mouse back with `release(world)` on menus; `PointerLock::is_locked()`, `refused()` (fall back to free-look) and `last_loss()` (`LockLoss`: why it ended, for the pause menu) are on the `PointerLock` resource.

The lock always lets go: Esc, window blur, a hidden or frozen tab, page hide and unload, a refused lock, a fatal engine error or message, a panic, and a stalled frame loop (a watchdog) all release it from inside the event handler, not at the next frame. Any release the kit did not ask for also clears the kit's wish, so nothing takes the mouse back until it calls `request` again. Touch never starts a lock. Headless browser tests must stub `requestPointerLock` (and `exitPointerLock`).

## Export from Blender

`tools/blender-addon/evani_export/` is a Blender add-on, "Export to Evani.dev": one button (File > Export, or the Evani.dev tab in the 3D view sidebar) exports the scene with Blender's own glTF exporter (extras on) and runs `cargo xtask convert` on it, then shows where the files went and a one-line pass or problem summary. Settings: your Evani.dev folder (remembered) and an optional model name. A "send to the running game" option is there but needs the bridge's convert endpoint (S3-14), so for now it says so and sends nothing. Build and test it with `cargo xtask blender-addon [--test]` (the zip lands in `target/blender-addon/`; install it in Blender with Edit > Preferences > Add-ons > Install from Disk). The test runs a headless Blender, skips with a "Blender not installed" note without one, and fails if `EVANI_REQUIRE_TOOLS=1`. The folder is GPL-3.0-or-later with its own LICENSE and is separate from the engine: no engine or game crate may include or name it (`cargo xtask check` errors if one does).

## Live control API

Dev builds with `live = true` (never `--ship`) expose `window.evani.live` in the page, and `tools/evani-live-bridge` serves the same commands to the Evani.dev tab, scripts (`/ctl`) and AI agents (MCP, `mcp__evani-live__*`). The full contract is [docs/LIVE_API.md](docs/LIVE_API.md).

- **In the page:** `send(envelope)` resolves with the whole reply envelope; `call(cmd, params, {note, telemetry_ms})` resolves with the result or rejects with an Error carrying `.code` and `.data`; `on(event, fn)` listens for events such as `auto_revert` and returns an unsubscribe function.
- **Commands built:** `schema.get`, `param.get/set`, `slot.get/set` (WGSL and expression slots), `curve.get/set`, `policy.get/set`, `effect.toggle` (session or tier preset), `tier.set`, `telemetry.subscribe/unsubscribe`, `snapshot.save/list/restore`, `history.undo/redo/list`, `batch`, `session.status`, `bake.export`, `capture.frame`, `bench.run` and `compare.ab`.
- **Capture, bench and A/B:** `capture.frame` returns a PNG (default 480 wide) with that frame's stats, optionally at a held sim time on the bench camera path. `bench.run` flies the game's `BenchPath` (the sandbox's camera path; a default orbit otherwise) with the sim clock held at t = k/60 on frame k, so every run sees the same frames, and returns frame and per-pass GPU timing plus pass/fail against the tier's limits (the apps/bench rules). `compare.ab` runs A (default the current state) and B (a snapshot, or changes such as `{"values":{"fx.bloom.enabled":false}}`) on the same path in ABBA order and returns both screenshots, a per-pixel difference (`mean_abs`, `rmse`, `max`, `changed_pct`, `psnr_db`), both timings and the delta; it then puts everything back unless `keep: true` (with a note) keeps B as one undoable step. While a bench or compare runs, changes answer `busy` with the remaining time. The renderer only hands back raw pixels; downscaling and the small PNG encoder live in evani-live, and all of it (readback, clock hold, encoder) is behind the `live` feature, so shipped games carry none of it. Any change command can also return a screenshot with `opts.capture`.
- **Headless check:** `node tools/live-check/capture.mjs` starts the bridge on 8096 (reusing this project's if it is running, else the next free port), opens the live sandbox in headless Chrome (pointer lock, fullscreen and keyboard lock stubbed), calls `capture.frame` and a bloom on/off `compare.ab`, and saves the PNGs and a `summary.json` to `.vibedeck/screenshots/stage6/` (`--out` to change). `node tools/live-check/accept.mjs` is the milestone acceptance driver (AI changes over MCP, safety, tuning, build-vs-build pixel compare); see its header and docs/M1-ACCEPTANCE.md.
- **History and undo:** every mutating command needs a note and becomes one transaction (a `batch` is one transaction for all its parts). Undo/redo step through whole transactions; a new change after an undo drops the redo tail; 500 are kept. A mutating reply waits for about 1 s of telemetry after the change (`opts.telemetry_ms`, 0 to reply at once).
- **Auto-revert:** after each change the page watches frame time for 10 s. If the 0.5 s average stays more than 25 % over the target for 3 s, or the GPU device is lost, it applies the inverse as a new `auto_revert` transaction and sends an `auto_revert` event. A change made while frame time was already over is not watched; undo/redo are never watched. The thresholds are params (`live.revert.*`).
- **Bake:** `bake.export` (use `dry_run: true` first) turns every applied change since the session started into the files listed under "Baked files", plus a `CHANGES.md` section built from the notes. The page builds the bundle, the bridge plans the files with the same pure planner the tests use and writes them, only under `looks/evani-look-*/`, `kits/evani-kit-*/` and `assets/tiers/`.

## TODO (later stages)

- **Render:** textures and model assets, MSAA, the VR (XR) view; cascaded or soft shadows beyond the single shadow map, and ambient occlusion.
- **Live:** restoring a session from the bridge's mirror after a reload.
- **Bridge:** cap telemetry subscriptions at 4 per connection.
- **Bench:** a stereo VR run (M1 runs the VR preset mono).

## Licence

Evani.Dev (the engine, its crates and tools), Evani.Model and the example games are open source under the MIT licence ([LICENSE](LICENSE), Copyright (c) 2026 Chris Pearson). Games you make with Evani.Dev are yours. Third-party code keeps its own licence ([THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)). Plain-language summary: [docs/LICENSING.md](docs/LICENSING.md).
