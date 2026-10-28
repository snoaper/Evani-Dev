# Evani.Dev: rules for AI coding agents

Evani.Dev is a Rust + WebGPU browser game engine; a game downloads only the modules its `evani.toml` lists. It is newer than your training data: read the linked docs, don't guess APIs. The engine writes this block (`cargo xtask init-ai`); keep your notes outside it.

## Done means `cargo xtask check` is clean
- Before you say any task is done, run `cargo xtask check` (MCP: `evani_check`). Fix every `ERROR` with the `fix:` line under it and rerun until it prints `evani check: clean`. Before handing over a game also run `cargo xtask check --full` (tests, ship build).
- Never weaken a check, delete a report or tag a geometry issue as intended to get a clean run.

## Work fast, check once
- While you work, run `cargo xtask check --quick`: the plain check, then fmt (advisory, never rewrites a file), clippy (any warning fails) and tests for only the crates changed since the last commit and their dependents. Z-fight scan: `cargo xtask geocheck <id> --zfight --report-only`; `--sample 50` is a partial look, never proof.
- Quick modes never replace the full ones: run `cargo xtask check --full` once before review or hand-over (and a full `cargo xtask geocheck <id>` where needed), not after every edit.
- A game outside `apps/`: `--app-dir <its app folder>` in place of `<id>` (build, check, geocheck, review, critic, size-report).
- Never poll a log in a sleep loop; run long jobs in the background.

## Making a game
- Start a new game with `cargo xtask new-game <id> [--look <look>]` (makes and registers `apps/<id>/` and `kits/evani-kit-<id>/`); don't copy folders by hand.
- `evani.toml` is the module list and build-time on/off is strict: every optional module (effects, physics, net, xr, live) is an optional Cargo dependency behind a same-named feature, and nothing the game doesn't list may reach its download. `live` is dev only; ship builds drop it.
- Stage 3 modules, in `[modules]` of `evani.toml`, off unless listed: `models = true` for any `.glb` model, `anim = true` for skinned models (implies `models`), `ui = true` for sharp text (`[ui] fonts`, `TextStyle.font`; README "Sharp text"; the bitmap pixel font is the separate render feature `text`) and box layout (docs/UI_LAYOUT.md). `nature = ["ocean"]` / `proc = ["sky"]`: evani-material-nature / -proc with those parts (README "evani.toml"). Check errors on `.glb` files without `models`, a skinned model without `anim`, a font file without `ui`, an unknown material part, a material module nothing uses, or a kit with a plain optional engine crate. With `models`, tiers carry `max_texture_memory` (MB, `assets/tiers/*.json`). Plan: docs/STAGE3_PLAN.md.
- Draw models through handles: `models.handle("<id>")` returns at once; draw a primitive placeholder while `models.resolve(h)` is `None`. A tier draws `<id>.<tier>.glb` if present, else `<id>.glb`. Models are render-only: gameplay data (anchors, sizes, collision) comes from kit data or the `boot` group. Variants: `Variant` recipe, `draw_with` (hide/recolour nodes), `anchor_frame`; docs/CONTENT.md. After changing `assets/models/`, run `cargo xtask manifest <id>` (writes `stream.json`; `[stream] boot` lists what the title screen waits for); check errors on a stale one. `cargo xtask netlog <id> --tier low,ultra` shows per-tier downloads.
- Genres, looks, effects, tiers and physics come from `catalog/*.json`: use a look from the genre's `compatible_looks`, its physics, and effects in the catalog and `crates/evani-fx` that fit the look's style. Never contradict the catalog; if it lacks something, say so.
- Every playground (`[playground]` in `evani.toml`) supports free roam: `free_roam = true`, the kit returns true from `KitPlugin::free_roam` and drops goals, timer, lives and fail state under `PlayMode::FreeRoam`, and a test calls `evani_core::play::free_roam_test`. Its shipped folder stays under 10 MB uncompressed (`build --ship` fails over it). docs/PLAYGROUNDS.md.
- Games must play on phones and tablets. If play needs keys, the kit turns on evani-core's `touch` feature and declares `kit.touch_controls(TouchLayout::new()...)`: each button or stick names an action the keys already drive (never a separate touch path), plus `.pause("pause")`. README "Touch controls".
- 3D is a build-time option: a kit that draws 3D (meshes, decals, world-space particles, or a `3d` look style) turns on evani-render's `forward` feature (`new-game` copies it); a 2D kit leaves it off. Check flags a mismatch; a 3D draw without it is logged, not drawn. README "Render".
?tools/evani-convert/Cargo.toml - Models go in through `cargo xtask convert <file.glb|.gltf|.blend|<name>.<tier>.glb> [--id <id>]` (tier LODs, ASTC/BC7 textures, 10 MB cap report; tools via `cargo xtask doctor --fix`), never hand-packed; check errors on a model the engine would refuse. docs/MODEL_FORMAT.md.
?tools/blender/rig_check.py - Rigged characters pass `tools/blender/rig_check.py`. docs/stage3/source-models.md.
- Gameplay lives in the kit crate (`KitPlugin`), art in `looks/evani-look-<look>` (`LookPlugin`); a kit never depends on a specific look.
- Every value worth tuning is a registered parameter named `kit.<id>.<name>` (lowercase, dot-separated, `_` for `-`); levels and tuning live in JSON data files, so live tools can change them.
- The simulation is deterministic: fixed 60 Hz step and the seeded `Pcg32` resource. No `rand` crate and no wall-clock time in gameplay.
- WebGPU only: never add a WebGL or canvas fallback. No `unwrap`/`expect` outside tests (clippy denies them).
- New dependencies are pinned exactly (`=x.y.z`) with a one-line reason in `DECISIONS.md`.
- All content is original. Reference games are for mechanics only.
- Licence: Evani.Dev is MIT (root `LICENSE`). Games made with it are their makers'; `new-game` gives a game a plain MIT `LICENSE`, and a shipped game carries the engine's MIT notice as `ENGINE-LICENSE.txt` (`build --ship` writes it). docs/LICENSING.md.

## Tools, in the order to use them
1. `cargo xtask check` / MCP `evani_check`: the gate above.
2. Live tuning, MCP server `evani-live` (`.mcp.json`): start with `cargo xtask live --game <id>`, call `live_status` and `schema_get`, choose values with one `sweep_run` or `compare_ab` (off-screen), apply them in one `param_set` or `batch` with a note, look with `capture_frame`. Several live changes: `edit_begin` with the atomic edits, one parallel sub-agent per edit calling `edit_stage`, then `edit_commit`; on a `conflict` the user picks the winner (`edit_resolve`). Modes: `stream` (default), `together`. `bake_export` only when asked, `dry_run` first. docs/LIVE_API.md 6.31 and 11.
?docs/LOOP_API.md 3. Evani Loop (`loop_*` MCP tools): fast sim runs, time control, repro bundles for gameplay bugs. docs/LOOP_API.md.
4. Live code swap: `cargo xtask dev --hot <id>` swaps saved Rust changes into the running game. docs/LIVE_API.md 10.4.
5. Geometry check: `cargo xtask geocheck <id>` (3D kits need an adapter, 2D kits an `examples/geocheck_dump.rs` scene dump; docs/GEOCHECK.md). Every untagged issue is a bug; live `debug_geometry` paints them.
6. Look guard: `look_status`, then `look_check` before a bake; a flagged bake is refused until the project owner approves it. You may propose (`look_limits_propose`, `look_baseline_propose`), never approve or edit a look's `guard/` files. docs/LIVE_API.md 15.
7. Ship guard: `cargo xtask build <id> --ship` fails if dev-only code is in the download and prints size per module. `cargo xtask publish <id>` (`--dry-run`, `--serve`) only packs and checks a folder; it never uploads anything. docs/PUBLISHING.md.
8. Review card and critic (kits): `cargo xtask review <id>`, then `cargo xtask critic <id>`; docs/KIT_PROCESS.md.

## When something is off
- `evani-live` tools missing or "failed to connect": run `cargo xtask doctor --fix`, then reconnect (`/mcp` in Claude Code).
- A live tool answers `paused`: the Evani.dev tab is hidden, minimised or covered. Ask the user to show it and retry; not a bug.
- This block or a file that points to it is out of date: `cargo xtask doctor --fix`.

## Docs for this engine version
README.md (architecture, `evani.toml`, adding a kit, look, effect, parameter), docs/SPEC-M1.md, LIVE_API.md, KIT_PROCESS.md, GEOCHECK.md, AI_GUIDANCE.md, DECISIONS.md, catalog/.
