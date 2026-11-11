# The engine pack: Evani.Dev inside every Evani.App install

Phase 1 of `.vibedeck/plans/engine-in-app-roadmap.md` (the "delivery framework"). Owner direction: the engine, the procedural modules, the materials, Evani.Model and the whole build toolchain are **inside the app install, for everybody, working offline straight after install**; only ready-made assets download, per game, from evani.app. System tools we may not legally ship are detected and the user is sent to the official installer; we never work around them.

This file is the contract between the engine (this repo) and Evani.App (`..\claude-code-desktop`, read-only from here). Sections 1-3 are built and tested on branch `engine-pack`; section 4 is what Evani.App builds; section 5 is the clean-machine proof; section 6 is the Mac design; section 7 the decisions.

## 1. The pack

```
evani-engine/<pack_version>/<platform>/      (what `cargo xtask engine-pack` writes)
  manifest.json          schema evani-engine-pack/1 (below)
  engine/                the allow-list of packaging/engine-pack.json at one commit,
                         read from git blobs (never the working tree), trimmed workspace
    .vibedeck/dev-harness/live.mjs, model.mjs     the Evani.dev tab's engine marker
  vendor/                `cargo vendor` of the pack's Cargo.lock (offline builds)
  cargo-config.toml      source-replacement template ({PACK} = this folder)
  toolchain/             rustc, cargo, rustdoc, clippy, rustfmt, host std, wasm32 std,
                         rust-lld (no .pdb, docs, rust-src or other targets)
  tools/bin/             evani (the engine's command line: every `cargo xtask` command),
                         cargo-xtask (so `cargo xtask ...` runs evani), evani-reqcheck,
                         evani-live-bridge, evani-convert, evani-model (ours, prebuilt),
                         trunk, wasm-bindgen, wasm-opt (upstream, SHA-256 checked)
  tools/convert/         gltfpack, ktx, gltf-transform, msdf-atlas-gen + their records
  LICENSE (MIT), THIRD_PARTY_NOTICES.md (generated: tools, toolchain, every vendored crate)
```

- **Pins:** `packaging/engine-pack.json` (checked in). Allow-list (not deny-list), excludes, the pack's workspace members, size parts, Rust version + commit + host per platform + targets + components, the prebuilt packages, every third-party tool (version, licence, upstream URL, upstream SHA-256, local fallbacks) and the system requirements per platform. Any pin change is a new `pack_version`.
- **What is in `engine/`:** engine runtime crates, procedural and material crates, `catalog/`, Evani.Model (crate + CLI + fur locks), looks, engine assets, dev tools source (xtask, guide, live bridge, convert, geocheck, lookguard, reqcheck, host, Blender add-on), the dev harness, AI guidance (AGENTS.md, CLAUDE.md, GEMINI.md, README, DECISIONS, docs/ without `docs/model_research/`), `.mcp.json`, the new-game templates (`kits/evani-kit-example`, `apps/sandbox` without its proof models) and `kits/evani-kit-kart-racer` (the geometry check's built-in adapter needs it to resolve; decision 7). Never: our other games' kits and apps, `.vibedeck/` records, `tests/`, `target/`, `.evani/`.
- **Manifest fields:** `schema`, `pack_version`, `engine_commit`, `created` (commit time, never the clock), `platform`, `live_protocol` (`evani-live/1`), `complete` (nothing missing), `release_ready` (complete and every bundled program checked against its upstream download's pinned SHA-256), `gaps[]`, `rust` {version, commit, host, targets, components}, `wasm_bindgen` {crate_version, cli_version} (the packer refuses a mismatch), `tools[]` {name, version, licence, path, sha256, sha256_upstream, source, upstream_verified}, `prebuilt[]` {package, path, sha256, bytes}, `requirements[]` (checker ids), `signing` {status `unsigned`|`signed`, signer, programs (count), unchanged[] (programs the signer left as they were: already signed upstream)}, `layout` (relative paths Evani.App needs), `parts[]` {name, files, bytes, bytes_compressed (gzip-6 stand-in)}, `totals`, `files[]` {path, sha256, bytes, `program: true` for every program release signing must sign, `signed_by` (the signer) in a signed pack}.

### Commands
- `cargo xtask engine-pack [--platform windows-x64|macos-arm64|macos-x64] [--out <dir>] [--build-dir <dir>] [--commit <rev>] [--tools-from <dir>] [--download [--cache <dir>]] [--archive] [--skip-prebuilt] [--allow-incomplete] [--force]`
- `--download`: trunk, wasm-bindgen and wasm-opt come from their pinned release URLs into `--cache` (default `<out>/.upstream`; reused between runs), and any archive whose SHA-256 is not the pinned `sha256_upstream` is deleted and the pack refused. Without it, local copies are packed and marked `upstream_verified: false`. All nine `sha256_upstream` values (three tools, three platforms) are recorded, each checked against the project's own published checksum (`.sha256` file or the release's asset digest).
- `--archive`: also writes the hostable form (route B, 4.8) to `<out>/hosted/`: `evani-engine-<v>-<platform>.tar.gz` (deterministic: sorted, fixed owner, mode and time; the same pack gives the same bytes) and `evani-engine-<v>-<platform>.json` (schema `evani-engine-download/1`: archive file, SHA-256 and size, unpacked size, the pack manifest's SHA-256, `complete`, `release_ready`, and the signature slot `<name>.json.sig`, status `unsigned` until Evani.App's release signing signs it). The pack folder itself is the installer-ready form (route A).
- `cargo xtask engine-pack --repro-check [..]`: packs twice into `repro-a/` and `repro-b/`, wiping the build folder before each, and requires byte-identical manifests (lists differing files otherwise).
- `cargo xtask engine-pack --verify <pack folder>`: re-hashes every file; reports changed, missing and extra files, and in a signed pack any program the manifest doesn't record as signed; also runs the clean-PC import check (below) and lists every program as static or allow-listed. Evani.App runs the same check (section 4.2) after placing a pack.
- `--sign-cmd "<command with {file}>" --signer <name>` (or `EVANI_PACK_SIGN_CMD` + `EVANI_PACK_SIGNER`): the signing hook (4.10). Every program in `toolchain/` and `tools/` is signed **before** anything is hashed, so the manifest holds the signed bytes.
- `cargo xtask doctor --requirements [--json]`: the requirements checker (section 3), same code as the pack's `evani-reqcheck`.

### Reproducibility
Files come from git blobs and pinned copies; file order is sorted; every file's modified time is set to the commit time; our tools are built from the pack's own `engine/` and `vendor/` in a fixed build folder (`--build-dir`, same absolute path each run) with `--release --locked --offline`, `--remap-path-prefix=<build>=/evani-pack`, `CARGO_INCREMENTAL=0` and (MSVC) `/Brepro`; the active `rustc -Vv` must equal the pinned version and commit or the packer refuses (a compiler change changes bytes: memory note "byte proofs and compiler drift").

### Clean-PC requirement: no program may need the Visual C++ runtime
A new Windows has neither the Visual C++ runtime (`VCRUNTIME140.dll`, `MSVCP140.dll`) nor, before Windows 10, the universal C runtime. A program that imports one does not start there (`STATUS_DLL_NOT_FOUND`, no output), so `evani-reqcheck` could not even say the C++ Build Tools are missing. We may not ship Microsoft's runtime DLLs, so:
- **Every program we compile for the pack is built with a static C runtime** (`-C target-feature=+crt-static`, windows-x64 only): the six prebuilt tools (`evani`, `cargo-xtask`, `evani-reqcheck`, `evani-live-bridge`, `evani-convert`, `evani-model`). The flag is set by the packer's own build step (`RUSTFLAGS` in `build_prebuilt`), never in a Cargo config, so normal game and dev builds are unchanged. It is part of the pinned build recipe: it changes bytes once and stays reproducible (`--repro-check`).
- **The import check** (`check_runtime`) reads the import table (normal and delay-load) of every `.exe`, `.dll` and `.node` in the pack, with a small built-in PE reader (no dependency). A program that imports `VCRUNTIME*`, `MSVCP*`, `MSVCR*`, `CONCRT*`, `VCOMP*`, `ucrtbase.dll` or `api-ms-win-crt-*` **fails the check** unless it is on `runtime_allow` in `packaging/engine-pack.json` with a reason. It runs in the pack build (before anything is hashed: a failing pack is not written) and in `--verify` (the allow-list is read from the pack's `manifest.json`, `runtime_allow`). A stale allow-list entry is reported. Mac packs are not checked.
- **Allow-list (upstream programs we cannot rebuild):** `tools/bin/trunk.exe` (decision 23: it only runs a game build, after the Build Tools, which bring the runtime), `ktx.exe` and `ktx.dll` (KTX-Software's release build; model texture import only) and sharp's `libvips-42.dll` (universal C runtime only, which Windows 10 and 11 include). Everything else in `toolchain/` and `tools/` imports no C runtime at all.

### Refusals
A program that imports the Visual C++ or universal C runtime and is not on the allow-list (above); active Rust not the pinned one; wasm-bindgen CLI pin not equal to the Cargo.lock crate; a workspace member outside the allow-list; a converter tool whose program no longer matches its install record; a missing part (unless `--allow-incomplete`, which writes `complete: false` and the gaps).

## 2. Engine changes that make a read-only pack work (this branch)
| Change | Where | Test |
|---|---|---|
| xtask finds the engine from `EVANI_ENGINE_ROOT`, else `<pack>/engine` when it runs from `<pack>/tools/bin/`, else its compiled-in checkout | `tools/xtask/src/engine_pack.rs` `engine_root_from`, `main.rs` `workspace_root` | `engine_root_comes_from_env_then_the_pack_then_the_checkout` |
| Converter and font tools read `EVANI_TOOLS_DIR` (the pack's `tools/convert`) instead of `<root>/.evani/bin` | `tools/evani-guide/src/extools.rs` `bin_dir`, `fonttools.rs` | `engine_pack_tools_dir_overrides_dot_evani_bin` |
| Live session files go to `<EVANI_STATE_DIR>/live/` instead of `<root>/.evani/live/` | `tools/evani-live-bridge/src/session.rs` `live_dir` | `state_dir_moves_live_files_out_of_the_engine` |
| The tab's launcher runs the pack's prebuilt bridge, never `cargo build` | `.vibedeck/dev-harness/live.mjs` (`inPack`) | `node --check`; exercised in section 5 step 6 |
| `doctor --requirements` | `tools/xtask/src/main.rs` | the checker's 10 tests |
| `[engine] version` pin: `check` ERROR on a pin this engine isn't (fix: that engine's own tools, or `upgrade-engine`), WARN on an unpinned game in a pack; builds refuse another engine's game; an unpinned outside game is pinned on its first build | `tools/evani-guide/src/engine_version.rs`, `check.rs`, xtask `load_app_dir_with`, `pin_after_build` | `engine_version` 6 tests |
| `upgrade-engine --app-dir <app> [--to <v>] [--dry-run] [--no-pictures] [--max-diff <percent>] [--shot-ms <ms>] [--chrome <exe>]`: a copy (in `EVANI_STATE_DIR/upgrade/<id>-<from>-to-<to>/`) is pinned, relinked, given this engine's `Cargo.lock`, ship-built and checked; only then the game itself changes (its old files are kept beside the copy). Never to an older engine. Before-and-after pictures of the start view and `upgrade-report.json`/`.md` go in that folder (4.4) | `tools/xtask/src/engines.rs`, `start_view.rs` | `an_upgrade_changes_the_game_only_after_the_copy_passes`, `the_start_view_is_pictured_before_and_after_and_a_large_change_flagged`, start_view 3 tests; a real dry run (Results, third pass) |
| `relink --app-dir <app>`: points a game's engine crates at this engine (same version) after its pack folder was renamed or the game moved PC; refused for another pinned version | `engine_version::relink` | `relink_*` 2 tests |
| `engines [--dir] [--games <folder>].. [--live <v>] [--apply] [--json]`: installed packs, games and pins, the retention plan (4.5); `--apply` renames and relinks, never deletes | `engines.rs`, `engine_version::plan/apply` | `retention_*`, `more_than_three_*` |
| `new-game <id> --app-dir <folder>`: the game in its own workspace outside the engine (below) | `tools/evani-guide/src/scaffold.rs` `new_game_outside` | `a_game_outside_the_engine_gets_its_own_pinned_workspace` |
| `publish --app-dir <app> [--dry-run] [--serve] [--quick] [--out] [--from] [--mark-published vN]`: the normal publish path (ship build, package, preflight, size report, diff, summary, local preview server) for a game outside the engine; the package (`<game>/publish/<id>/`), summaries and history (`<game>/.evani/publish/<id>/`) and builds stay in the game's workspace. Never uploads: only the owner's click on Evani.App's publish card does | xtask `publish.rs` `cmd_publish_app` | `options_parse`, `option_values_are_not_game_ids`; run on a game outside the engine (Results, third pass) |
| `live`/`dev --app-dir`; `live`/`dev` use the pack's prebuilt bridge (never `cargo build` into the pack); the engine's example games built by the bridge go under `EVANI_STATE_DIR/engine-games` | xtask `run_bridge`, bridge `game.rs` | read-only test |
| Every other run-time write goes under `EVANI_STATE_DIR` when set: model-convert staging (`convert/`), sweep pictures (`live/sweeps/`), the look guard's audit log (`look-guard/`), Blender convert temp (`convert-tmp/`), the check hook's counters (`check/`), the Model tab's session and workspace (`live/model/`, `model/`) | bridge `convert.rs`, `mcp.rs`; lookguard `audit.rs`; convert `lib.rs`; guide `check.rs`; xtask `live_args` | read-only test |
| In a pack, `init-ai`, `doctor --fix`, `new-game` without `--app-dir` and commands on the engine's own example games refuse with the fix (`new-game <id> --app-dir <folder>`) | guide `cli.rs`, xtask `load_app` | read-only test |
| Guide commands (`new-game`, `check`, `doctor`, `catalog`) use `EVANI_ENGINE_ROOT` / the pack's engine, not the folder they run in; a game folder is never mistaken for an engine (`crates/evani-core` required) | guide `cli::run_in`, `find_root` | read-only test |
| `tools/bin/evani` (the prebuilt xtask) and `tools/bin/cargo-xtask` (starts `evani`; the pack's `.cargo/config.toml` has no `xtask` alias), so `cargo xtask <cmd>` never compiles anything | `packaging/engine-pack.json` `prebuilt`, `tools/evani-cargo-xtask`, `pack_cargo_config` | `the_pack_drops_only_the_xtask_alias`, launcher test |
| **Read-only proof:** the real binary runs `new-game --app-dir`, `features`, `render-paths`, `manifest`, `check`, `relink`, `engines`, `catalog`, `doctor` and `xtask features` (Cargo's form) against a pack-shaped engine copy locked against writing (Windows deny rule on "write data/add file" and "add folder"); `init-ai`, `doctor --fix`, `new-game` inside, `build sandbox` must refuse; every engine file is byte-identical afterwards | `tools/xtask/tests/readonly_engine.rs` | passes (3 s) |

**A game outside the engine** (`new-game <id> --app-dir <folder>`):
```
<folder>/Cargo.toml        [workspace] members = apps/<id>, kits/evani-kit-<id>; [workspace.dependencies] with the engine's crates
                           by absolute path (one line each), the engine's package settings, lints and profiles
<folder>/Cargo.lock        the engine's (offline builds use the pack's vendored copies)
<folder>/apps/<id>/        from apps/sandbox; evani.toml with [engine] version = "<this engine>"
<folder>/kits/evani-kit-<id>/   from kits/evani-kit-example
<folder>/tools/bake-index.rs    the kit's shared build script
<folder>/AGENTS.md, CLAUDE.md, GEMINI.md, .gitignore
```
Every command then takes `--app-dir <folder>/apps/<id>`: `build [--ship]`, `features`, `render-paths`, `manifest`, `check [--full]`, `geocheck`, `netlog`, `review`, `critic`, `size-report`, `publish`, `live`, `dev`, `relink`, `upgrade-engine`. Tried on this PC from the checkout: `check` clean (one warning: no geometry report yet), ship build 616.5 KB raw / 246.6 KB gzip, pinned on its first build. Not yet with `--app-dir`: `bench`, `convert` into a game's `assets/` (it takes `--out`), `fonts`.

Not done yet (engine side): the outside game's guidance names `cargo xtask`/`evani` commands from the engine's block, and its `.mcp.json` / Stop hook are not written (Evani.App's sessions get the live tools from the app).

## 3. Requirements checker
Program: `tools/evani-reqcheck` (std only, no dependencies, about 0.6 MB debug; ships prebuilt as `<pack>/tools/bin/evani-reqcheck`). Runs before any Rust is installed.

- **Windows:** MSVC x64 `link.exe` through Microsoft's `vswhere.exe` (component `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`) and the default `Program Files\Microsoft Visual Studio\<year>\<edition>` folders; Windows SDK `kernel32.lib` through `reg.exe query HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots /v KitsRoot10` (64- and 32-bit views) and the default `Windows Kits\10` folder.
- **Mac:** `xcode-select -p` and `xcrun --find clang`.
- **Probe (the only thing that says ready):** with every tool found, it builds a tiny crate with a build script for `wasm32-unknown-unknown`, `--offline --locked`, with the pack's own `toolchain/` (found next to the program), in `--work` or `$EVANI_STATE_DIR/reqcheck` (never the pack). The build script proves the host linker and SDK; the wasm link proves rust-lld and the wasm std.
- **Words:** "To build games you need Microsoft's free C++ Build Tools, because Evani uses their linker to put together the small helper programs every game build runs on this PC first, and Microsoft doesn't allow us to include them in Evani.App." (SDK and Mac variants likewise), an install hint and the official link (`https://visualstudio.microsoft.com/visual-cpp-build-tools/`, `https://developer.microsoft.com/windows/downloads/windows-sdk/`, Apple: `xcode-select --install` / `https://developer.apple.com/xcode/resources/`). It never says a build can work without the tool.
- **Output:** text, or `--json` (schema `evani-requirements/1`: `platform`, `ready`, `requirements[]` {id, name, status found|missing, detail, message, why, install_url, install_hint}, `toolchain`, `probe` {status passed|failed|skipped, seconds, detail}, then `capabilities[]` and `runtime`: see 4.12, "requirements per capability"). Exit 0 ready to build games, 1 not ready, 2 usage. The Visual C++ runtime (`vc-runtime`, needed by model import only) is reported as its own capability and never changes `ready` or the exit code.
- **Open the installer:** `--open <id>` opens the official page in the default browser, or Apple's own dialog for the Command Line Tools.
- **Re-check:** run it again. Event-driven only (section 4.6); never a timer.
- **Test hook:** `EVANI_REQCHECK_HIDE=<id,..|all>` or `--hide` makes tools read as missing (it can only make the answer worse). Note: Windows re-sets `ProgramFiles` for every new process, so an environment can hide `Program Files (x86)`, the SDK folder and `reg.exe`, but not Visual Studio under `C:\Program Files`; the demo uses the hook for that one.

## 4. The contract with Evani.App
Facts from Evani.App (orchestrator, 2026-10-27): per-user NSIS install at `%LOCALAPPDATA%\Evani.App` (`rust-shell/shell/tauri.conf.json`, `docs/release.md`); `resources/` is user-writable but replaced on update (fast update copies `resources/app` and `resources/bridge`; full update renames the install aside and reinstalls: `rust-shell/scripts/shell-updater.mjs`), so **the pack must live outside the install folder**; per-user state belongs in `%APPDATA%\Evani.App`; disk clean-up classes in `src/shared/diskRetention.ts` (build: live one + newest 2; cache: 7 days unused; keep: never); updater is full-installer only (`shell/latest.json` + `.sig`); Windows signing is real (Trusted Signing, `rust-shell/scripts/sign.mjs`); Mac signing/notarising runs through Codemagic for the Electron build.

### 4.1 Where the pack lands
Route A (default, the owner's words: "included in the app install, so everybody gets it, no matter what") is below; route B and the update paths are in 4.8.
1. **In the installer:** `rust-shell/scripts/package.mjs` copies `evani-engine/<pack_version>/windows-x64/` into `resources/evani-engine/<pack_version>/` (staging only; LZMA compresses it inside the installer).
2. **On first start after an install or full update:** the app moves (same-volume rename, no second copy) `resources/evani-engine/<pack_version>/` to the data folder:
   `%APPDATA%\Evani.App\evani-dev\engines\.evani-build-evani-dev-engine-<pack_version>\` and writes the `.evani-live` marker in it (`diskRetention.ts` `LIVE_MARKER`). If the rename fails (another volume), it copies, verifies, then deletes the staging copy.
3. It then checks the pack: `manifest.json` schema `evani-engine-pack/1`, `platform` = this machine, `live_protocol` spoken by the tab, and every file's SHA-256 (the same check as `cargo xtask engine-pack --verify`; about 20 000 files, run once per new pack, off the UI thread, with progress). A failed check shows "Evani's engine files are damaged; reinstall Evani.App" and never runs a build from it.
4. The pack folder is read-only from then on: nothing Evani.App or the engine runs writes into it.
- Size note (for Evani.App to decide): `%APPDATA%` is the roaming profile; on managed PCs with roaming profiles about a gigabyte of engine would sync. `%LOCALAPPDATA%\Evani.App` is the install folder, so a local alternative would be `%LOCALAPPDATA%\Evani.App-data\evani-dev\engines\`. The contract works with either; Evani.App picks.

### 4.2 Per-user folders and the child environment
Data folder `D = <data>\evani-dev\`: `engines\` (packs, above), `state\` (`EVANI_STATE_DIR`), `targets\<game>\` (`CARGO_TARGET_DIR`, build class), `cargo-home\` (`CARGO_HOME`), `reqcheck\` (the probe's work folder), `assets\` (Phase 5 asset cache, cache class).

Every engine process Evani.App starts (live launcher, xtask, cargo, trunk, the checker) gets, **in the child's environment only** (never the user's PATH or registry, no rustup):
| Variable | Value |
|---|---|
| `PATH` | `<pack>\tools\bin;<pack>\toolchain\bin;` + the inherited PATH |
| `RUSTC` | `<pack>\toolchain\bin\rustc.exe` (`manifest.layout.rustc`) |
| `CARGO_HOME` | `D\cargo-home` with `config.toml` = `<pack>\cargo-config.toml`, `{PACK}` replaced by the pack folder (forward slashes) |
| `CARGO_NET_OFFLINE` | `true` |
| `CARGO_TARGET_DIR` | `D\targets\<game id>` |
| `EVANI_ENGINE_ROOT` | `<pack>\engine` |
| `EVANI_TOOLS_DIR` | `<pack>\tools\convert` |
| `EVANI_STATE_DIR` | `D\state` (live sessions, the Model tab, model-convert staging, sweep pictures, look-guard audit, check-hook counters, upgrade copies, the engine's example games' builds) |
| `EVANI_ENGINES_DIR` | `D\engines` (optional: the tools find it as the folder above the running pack) |
| `RUSTUP_TOOLCHAIN` | unset (a user's rustup must not take over) |
| `EVANI_NODE` | Evani.App's own `node` (optional when `node` is on PATH): the engine's browser runners (`upgrade-engine` pictures, model import's quality check) and gltf-transform run on it |
cwd: anything; the tools find the engine from `EVANI_ENGINE_ROOT` (or from their own place in `<pack>\tools\bin`), never from the working folder. Evani.App starts `<pack>\tools\bin\evani.exe <command>`; an AI agent in a game folder can type `cargo xtask <command>` (runs `cargo-xtask` -> `evani`) or `evani <command>` when `tools\bin` is on its PATH.

### 4.3 Engine lookup (`src/main/evaniDevCore.ts` `resolveEngine`, `EngineTarget.via`)
Add `via: 'bundled'`. Order:
1. The tab's settings folder (unchanged).
2. An open project that is an engine checkout (unchanged; Chris's developer mode: his checkout always wins).
3. **Bundled:** the installed pack whose `pack_version` equals the game's `[engine] version`; for a new or unpinned game, the pack with the `.evani-live` marker. `root` = `<pack>\engine` (the marker `.vibedeck/dev-harness/live.mjs` is there, so the existing launcher code works); the launcher sees `<pack>\manifest.json` and runs `tools\bin\evani-live-bridge` without compiling.
4. The default folder `C:\dev\evani-dev` (developer machines only; unchanged).
Before using a bundled pack: refuse one whose `live_protocol` the tab doesn't speak, with the existing "update" message, never a half-connection.
Session file: for `via: 'bundled'` read `D\state\live\session.json` (`EVANI_STATE_DIR`), not `<root>\.evani\live\session.json` (`SESSION_FILE`, used in `src/main/evaniDev.ts` at its two `readFileSync(... SESSION_FILE ...)` calls). The Model tab's session is `D\state\live\model\session.json` and its workspace `D\state\model`.
**Start live through the launcher, not the bridge:** `node <pack>\engine\.vibedeck\dev-harness\live.mjs --port <P> [--app-dir <game>\apps\<id> | --game sandbox] [--session D\state\live\session.json] [--quiet]` (4.12). The launcher adds `--root <pack>\engine` and runs `<pack>\tools\bin\evani-live-bridge serve ...` with stdio inherited, so the one look-guard key line Evani.App writes to its stdin reaches the bridge. Evani.App should not call `evani-live-bridge` itself: if the bridge's flags change, the launcher changes with them. A user's game is started with `--app-dir <game>\apps\<id>` (or `evani live --app-dir <app>`); the engine's own example (`--game sandbox`) still plays from a pack and builds under `D\state\engine-games`.

### 4.4 Per-game engine version and upgrades
- New games: `evani new-game <id> --app-dir <folder>` (folder in the user's own space, e.g. `Documents\Evani Games\<id>`); the game is pinned to the live engine.
- The tab reads `[engine] version` from the game's `evani.toml`. Pinned pack installed: start that pack's `evani` / bridge. Not installed: show "This game was made with engine X. Upgrade it to engine Y?" and on yes run the **new** engine's `evani upgrade-engine --app-dir <app>` (optionally `--dry-run` first to show the result before changing anything). It prints what it did; exit 0 = upgraded (or already on Y), non-zero = the game is unchanged and the copy is kept in `D\state\upgrade\`. Never switch silently.
- **Before-and-after pictures** (built into `upgrade-engine`): the start view of the game on its old engine and of the upgraded copy on the new one, each a ship build served on localhost and opened in headless, muted Chrome or Edge (the engine's `tools/bench-runner`, raw DevTools protocol, own fresh profile, pointer and keyboard locks stubbed) at 1280x720 on the Medium tier, one screenshot 5 s after the page starts (`--shot-ms`). "Before" is the game built now by its own installed pack (exactly the old engine; `evani build --app-dir <app> --ship` of that pack, in its environment), else its last ship build in its own workspace (named as such in the report), else none (the report says why). Files in `D\state\upgrade\<id>-<from>-to-<to>\`: `start-before.png`, `start-after.png`, `start-diff.png` (the after picture dimmed, every pixel that moved by more than 32 of 255 in a colour channel painted red) and `upgrade-report.json` (schema `evani-upgrade/1`: game, app_dir, from, to, dry_run, result `upgraded`|`dry-run`|`failed`, summary, copy, kept, pictures {taken, before {file, source}, after {file, source}, diff {file, changed_percent, mean_change, same_size, tolerance, max_percent, flagged}, size, tier, notes[]}) with `upgrade-report.md` beside it. More than `--max-diff` percent (default 10) of the pixels changed: `flagged: true` and a WARN line; the upgrade itself is not blocked by it. Pictures need Node (`EVANI_NODE`, else `node` on PATH) and Chrome or Edge with WebGPU; without them the upgrade still runs and `notes` says why there are none (`--no-pictures` skips them).
- **Recommended flow for the tab:** `upgrade-engine --dry-run`, show the two pictures (and the red changes when `flagged`) with Upgrade / Not now; on Upgrade run it again without `--dry-run`. Measured on this PC: the same game on two engines with identical code differed in 0.4% of pixels (animation drift between two loads), so 10% leaves a wide margin for noise while a changed look, a missing effect or a black screen is far above it.
- A game without a pin builds with the live pack and gets pinned to it on its first successful build. `check` warns about an unpinned game in a pack and errors on a pin the running engine isn't, with the fix.

### 4.5 App updates and old packs (owner decision: current plus pinned, at most 3 extra)
- A new pack version ships only in a **full** update (it changes `resources/evani-engine/`); fast updates (`resources/app`, `resources/bridge`) never touch the engine.
- On first start of the new version: place the new pack (4.1), move `.evani-live` to it, then for each older pack in `engines\`:
  - pinned by a known game (any `evani.toml` in the user's projects naming it): rename to `.evani-keep-evani-dev-engine-<ver>` (keep class: never offered for deletion);
  - not pinned: leave as `.evani-build-...` (build class: kept while among the newest 2, then offered for deletion with its size).
- Cap: at most **3** kept packs besides the live one. When a 4th would be kept, ask the user which game to upgrade (or which old engine to remove, showing the size and the games that use it); never delete a pinned pack silently.
- When the last game pinning a kept pack is upgraded or deleted, rename it back to `.evani-build-...` so the normal clean-up can offer it.
- **The engine does this for Evani.App:** `<live pack>\tools\bin\evani engines --games <folder> [--games <folder>]... --live <new version> --json` prints the plan (schema `evani-engines/1`: `packs[]` {version, dir, class build|keep|other, live, bytes, games[]}, `games[]` {app_dir, pin}, `steps[]` {action rename|mark-live|duplicate, ...}, `kept[]`, `over_cap`, `missing[]`); the same with `--apply` carries it out and reports each step. Run it after placing a new pack and after a game is upgraded or deleted. `over_cap: true` is the "ask the user" case; `missing[]` lists games pinned to a pack that isn't installed (offer the upgrade).
- **Renaming moves paths:** a game names the engine's crates by absolute path, so renaming its pack (build -> keep) would break its build. `engines --apply` relinks every game pinning a pack it renamed; a game it didn't find (not under `--games`) is fixed later by `evani relink --app-dir <app>` (the build's error says so).
- Nothing is downloaded to bring an old pack back.
- Check that `src/main/diskScanCore.ts` scans `<data>\evani-dev\` so these folders appear in the Disk tab with their classes.
- Separate item, not blocking Phase 1: every app update re-downloads the whole installer (about 350-420 MB with the engine). Component updates (fetch only changed pack parts by hash, as part of the app update) are recommended later; they need a new feed on Evani.App's side.

### 4.6 The requirements card
- Run `<pack>\tools\bin\evani-reqcheck.exe --json --work D\reqcheck` on the tab's first open, before the first build of a session, and on "Check again". Re-run it when the installer Evani.App opened exits, and when the app window regains focus while the card shows. Never on a timer.
- Card: one per missing requirement, using `message` and `install_hint` as written; buttons "Install build tools" (`evani-reqcheck --open <id>`, or open `install_url`; Mac: Apple's dialog), "Check again", "Not now". "Not now" leaves playing and editing working and blocks only builds.
- "Ready" only when `ready` is true (the probe passed). `probe.status = failed` shows "Evani tried a tiny test build and it did not work" with `probe.detail` in a closed Details section.
- Disk space (warn under 3 GB free on the data folder's drive) and WebView2 stay Evani.App's checks.

### 4.7 What Evani.App must do (their files, read-only from here)
0. Name the engine command `evani` (was `evani-xtask` in the first draft of this contract).
1. `rust-shell/scripts/package.mjs`: copy the platform's pack into `resources/evani-engine/<pack_version>/` (like the existing resources steps); merge the pack's `THIRD_PARTY_NOTICES.md` into the app's notices; have every program in the pack signed with `rust-shell/scripts/sign.mjs` (Trusted Signing) through the packer's signing hook, so the manifest holds the signed bytes (4.10); never sign the pack's programs again afterwards.
2. First start: place, mark live, verify (4.1); per-user folders and child environment (4.2).
3. `src/main/evaniDevCore.ts`: `resolveEngine` `via: 'bundled'` (4.3); refuse an unknown `live_protocol`; `SESSION_FILE` from `EVANI_STATE_DIR` for bundled packs.
4. `src/main/evaniDev.ts` / `evaniDevControl.ts`: start the launcher and every engine command with the 4.2 environment; read the game's `[engine] version`; the upgrade offer (4.4).
5. Pack retention on update with `.evani-build-` / `.evani-keep-` / `.evani-live` (4.5); make sure `src/main/diskScanCore.ts` scans the engines folder (`src/shared/diskRetention.ts` classes).
6. The requirements card and its event-driven re-check (4.6).
7. Mac (Electron build via Codemagic): the same pack for `macos-arm64`; every Mach-O in it signed with the Developer ID and hardened runtime before notarising (rustc loads its own dylibs: test whether the library-validation entitlement is needed).
8. Run the clean-machine proof (section 5) on a test build, never a published one.

### 4.8 Delivery routes and updates (both kept open; route A is the default)
Evani.App's updates have no deltas, so the pack must never ride along with a normal app update.
- **Route A (default): in the installer.** A fresh install's installer carries the pack (4.1). Updates:
  - fast updates (`resources/app`, `resources/bridge`) never touch the engine (0 MB of engine);
  - a full update whose release has the **same** `pack_version` as the installed one is built **without** the pack (the installer then has no `resources/evani-engine/`; first start sees the pack already in `D\engines\` and does nothing): 0 MB of engine;
  - a full update with a **new** `pack_version` carries the new pack (the only update that costs the engine's size), or the app takes it by route B.
  So Evani.App's packaging makes two installers per release when the engine is unchanged (fresh-install with the pack, update without), and the update feed points at the right one. The packer's output folder `evani-engine/<v>/<platform>/` is the installer-ready form.
- **Route B: hosted download.** `engine-pack --archive` writes `evani-engine-<v>-<platform>.tar.gz` and its descriptor `.json` (above). Evani.App's release signing signs the descriptor (same tool and key as `shell/latest.json`'s `.sig`; the slot is `<descriptor>.json.sig`), uploads both to a versioned URL (`.../evani-engine/<v>/<platform>/`), and the app downloads the pack when its engine version changes: check the descriptor's signature, download the archive, check its SHA-256, unpack into `D\engines\.evani-build-evani-dev-engine-<v>.partial\`, run the pack check (`evani engine-pack --verify` equivalent, 4.1 step 3), rename to the final name. The app installer and every app update then carry 0 MB of engine; only an engine change costs the archive. Offline-first is lost for a fresh install until the first download, so route A stays the default.
- **What an update costs** (Windows, this pack; see Results for the measured numbers): route A fresh install about +200-230 MB in the installer (LZMA); route A update with the same engine 0 MB; with a new engine about +200-230 MB; route B the archive (gzip) once per engine version, app updates 0 MB.
- Signing of what's inside the pack (every program with Trusted Signing, 4.7 item 1) is the same in both routes and happens inside the packer, before `manifest.json` is written (signing changes the files' bytes): the `--sign-cmd` hook, 4.10. Route B's descriptor names the signer (`programs_signed_by`).

### 4.9 Commands Evani.App runs (all `<pack>\tools\bin\evani.exe`, with the 4.2 environment)
| When | Command | Output |
|---|---|---|
| First start after placing a pack | `engine-pack --verify <pack>` (or the same check in TypeScript) | exit 0 = intact |
| The requirements card | `evani-reqcheck.exe --json --work D\reqcheck` | `evani-requirements/1` |
| New game | `new-game <id> --app-dir <folder> [--look <look>] [--genre <g>]` | the folder; app folder is `<folder>\apps\<id>` |
| Open a game | read `[engine] version` in `<app>\evani.toml`; start that pack's `live.mjs --port <P> --app-dir <app> --session D\state\live\session.json` (4.12) | `D\state\live\session.json` |
| Check / build | `check --app-dir <app> [--json]`, `build --app-dir <app> --ship` | the check report, `target\evani-sizes\<id>-ship\` |
| Publish (before the owner's publish card) | `publish --app-dir <app> [--dry-run] [--serve --port <P>]`; after the owner's click uploaded it: `publish --app-dir <app> --mark-published vN` | the package `<game>\publish\<id>\`, `<game>\.evani\publish\<id>\summary.json` (`evani.publish.summary/1`) |
| Upgrade | the new pack's `upgrade-engine --app-dir <app> [--dry-run]` | exit code; `D\state\upgrade\<id>-<from>-to-<to>\upgrade-report.json` (`evani-upgrade/1`) and the three start-view pictures |
| After an update, upgrade or deleting a game | `engines --games <folders..> --live <v> --json`, then `--apply` | `evani-engines/1` |
| A game moved or its pack renamed | `relink --app-dir <app>` | |

### 4.10 Signing the pack's programs (the `--sign-cmd` hook)
The packer runs one external command on every program in the pack, then hashes. Programs: in `toolchain/` and `tools/` only (never `engine/` or `vendor/`): Windows `.exe`, `.dll`, `.node`; Mac `.dylib`, `.node` and any Mach-O (found by its first bytes). Libraries are signed before executables (Mac needs a dylib signed before what loads it). The manifest lists every program (`program: true`) even in an unsigned pack, so Evani.App knows what to sign.
- **The command:** `--sign-cmd "<words>"`, double quotes group words, `{file}` is the program's full path (required); the same path is in `EVANI_SIGN_FILE`. It runs once per program, without a shell. `--signer "<name>"` is required and goes into the manifest (`signing.signer`, each program's `signed_by`). The command itself is never printed or recorded (it may carry settings).
- **Refusals:** a non-zero exit refuses the pack (nothing is written); a command that changed **no** program refuses too ("nothing was signed"): Evani.App's `sign.mjs` exits 0 and changes nothing when the owner's signing settings are missing, so a silent no-op can never produce a pack marked signed. Programs it leaves unchanged while others change (already signed upstream; `sign.mjs` skips a file with a valid signature) are listed in `signing.unchanged`.
- **Verify:** `--verify` on a signed pack re-hashes the signed bytes (passes), names the signer, and fails on any program changed after signing, or a program without `signed_by`.
- **Reproducibility:** real signatures carry a timestamp, so two signed packs differ; `--repro-check` is run unsigned (or with a deterministic test signer), and the unsigned pack's files are the input to signing.
- **How Evani.App plugs in `sign.mjs`** (Windows, `rust-shell/scripts/package.mjs`, release or `--sign` builds, after `withSignSettings` has put the Artifact Signing settings into the environment):
  ```
  cargo xtask engine-pack --platform windows-x64 --download --archive --out <out>
      --sign-cmd "node <claude-code-desktop>/rust-shell/scripts/sign.mjs {file}"
      --signer "Microsoft Trusted Signing (evanisigning / evani-public-trust, CN=Snoaper)"
  (run in the engine checkout at the release commit, with the pinned Rust)
  ```
  `sign.mjs <file>` already signs one file per call (it is Tauri's `signCommand`), so it needs no change. The packer inherits the environment, so the settings reach `sign.mjs` the same way they reach Tauri's bundler. Then copy `<out>/evani-engine/<v>/windows-x64/` into `resources/evani-engine/<v>/` as in 4.1; do not sign the pack's programs again in `package.mjs` (that would change bytes after the manifest and `--verify` would fail). A self or test build without `--sign` packs unsigned (no `--sign-cmd`).
- **Mac** (Codemagic, section 6): `--sign-cmd "codesign --sign <identity> --force --timestamp --options runtime {file}"` for everything except `rustc`, `rustdoc` and `clippy-driver`, which need the library-validation entitlement: a small wrapper script that adds `--entitlements` for those three names and is passed as the command.
- **Tested:** unit tests with a test signer that appends a marker (`a_signed_pack_is_hashed_after_signing_and_verify_catches_a_later_change`, `a_signer_that_fails_or_changes_nothing_refuses_the_pack`, `programs_are_found_by_platform_libraries_first`, `sign_commands_are_split_and_must_name_the_file`) and a whole pack built with a dummy signer (Results, third pass).

### 4.11 Model and font helpers, and Blender
- **Bundled, pinned with checksums** (`packaging/engine-pack.json` `tools`, kind `convert`, Windows x64): gltfpack 1.3, KTX-Software 4.4.2 (`ktx.exe` + `ktx.dll`), msdf-atlas-gen 1.4, each with its release `url` and the download's `sha256_upstream`; gltf-transform 4.5.1 is an npm package, pinned by the committed lockfile `tools/evani-convert/npm/package-lock.json` (sha512 per package; the pin is the lockfile itself, `lockfile`). These are the same pins `doctor --fix` installs from (`evani-guide` `extools::PINS`, `fonttools`); a test keeps the two lists equal (`helper_pins_equal_the_doctor_fix_pins`). The packer copies a helper from its `doctor --fix` install only when the install record's download checksum equals the pack's pin (and the program equals the recorded program checksum); otherwise it refuses with the fix. The manifest's `tools[]` entry carries `sha256_upstream` and `pinned_in` (`sha256_upstream` or `lockfile`).
- **Blender 5.2.2: pinned, never bundled** (kind `external`: GPL-3.0-or-later and about 400 MB; plan 4.3 "prompt only when a .blend is dropped"). The manifest lists it in `external_tools[]` {name, version, licence, url, sha256_upstream, install, bundled: false}. In a pack (`EVANI_TOOLS_DIR` set) Blender lives in the per-user `<EVANI_STATE_DIR>/tools/blender-5.2.2/` (with its own `convert-tools.json` record), never in the read-only pack: `evani doctor --fix --blender` downloads the pinned zip from blender.org, checks its SHA-256 and unpacks it there (every other `doctor --fix` stays refused in a pack). `convert` of a `.blend` without it says exactly that command. Evani.App: when the user drops a `.blend` and the converter says Blender is missing, offer "Install Blender (about 400 MB)" and run that command; nothing else needs Blender.
- **Mac:** no pins yet for the four helpers or Blender (the `doctor --fix` installer is Windows-only). Before a Mac pack can be complete, each needs its macOS release URL and SHA-256 checked against the project's published checksum, as decision 15. Open (needs the Mac release machine; section 6).
- The engine's two browser runners (`tools/bench-runner`, `tools/convert-runner`, 44 KB, no npm packages) are in the pack's `engine/` since this pass: model import's quality check and `upgrade-engine`'s pictures need them.

### 4.12 Contract changes
Entries are added here when something Evani.App relies on changes; newest last.
- **2026-10-27, live launcher options.** `.vibedeck/dev-harness/live.mjs` now takes, besides `--port`, `--game` and `--quiet`:
  - `--app-dir <folder>`: a game outside the engine (its app folder, the one with `evani.toml`), forwarded as `--app-dir`. Not together with `--game` (the launcher exits 2 and says so).
  - `--session <file>`: where the bridge writes its session file (in a pack: `D\state\live\session.json`), forwarded as `--session`.
  - Relative paths are taken from the launcher's working folder and passed on absolute.
  - The bridge command line is `serve --root <pack>\engine --port <P> [--game <g> | --app-dir <app>] [--session <file>] [--quiet]`.
  - **Evani.App starts live through `live.mjs` with these options and does not call `evani-live-bridge` directly**, so a bridge flag change stays our concern and the launcher's own options stay steady.
  - The launcher's stdio is inherited by the bridge: the look guard's one key line on stdin is read by the bridge as before.
  - `--print-command` prints `{"exe","args","cwd"}` as one JSON line and exits (no build, nothing started). `node tools/live-check/launcher-args.mjs` uses it to check both forms in a pack-shaped folder (6 checks).

- **2026-10-27, clean-PC pack (still `pack_version` 0.1.0; the pack's bytes change).** Evani.App's Windows Sandbox test found that the pack's own programs did not start on a fresh Windows (they imported `VCRUNTIME140.dll`). Changes Evani.App can rely on:
  - `tools/bin/evani.exe`, `cargo-xtask.exe`, `evani-reqcheck.exe`, `evani-live-bridge.exe`, `evani-convert.exe` and `evani-model.exe` now use a static C runtime and start on a Windows without the Visual C++ runtime, before the Build Tools are installed. `evani-reqcheck` can therefore report that the Build Tools are missing.
  - Still dynamic and allow-listed (manifest `runtime_allow`, each with a reason): upstream `trunk.exe` (run it only after `evani-reqcheck` says ready, as builds already require), `ktx.exe`/`ktx.dll` (model texture import) and `libvips-42.dll` (universal C runtime only). Do not start those on a PC where the requirements card is showing.
  - `manifest.json` has a new field `runtime_allow` (array of {path, reason}); `--verify` now also fails when a program imports the C runtime and is not on that list, and prints each program as static or allow-listed. No other field or command changed.
  - Evani.App's installer must not add the Visual C++ Redistributable or any Microsoft DLL to the pack; the pack's `--verify` would still pass, but Microsoft's terms apply.

- **2026-10-27, requirements per capability (schema `evani-requirements/1` unchanged; two fields added).** The checker now answers two questions, so the card can show both. Chris's rule: if a feature needs a tool, say so and check for it; never claim it works without it.
  - `capabilities[]` {id, label, ready, status, needs[]}: `build-games` ("Building games": `ready` | "needs Microsoft C++ Build Tools (+ Windows SDK)"; Mac: "needs Apple's Command Line Tools for Xcode") and `import-models` ("Importing models": `ready` | "needs the Microsoft Visual C++ runtime"). `needs` lists the missing requirement ids (`msvc-linker`, `windows-sdk`, `xcode-clt`; `vc-runtime`).
  - `runtime`: `null` on Mac, else {id `vc-runtime`, name, status found|missing, detail, message, why, install_url (`https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist`), install_hint, redist_url (`https://aka.ms/vs/17/release/vc_redist.x64.exe`, the x64 redistributable, enough for import only), build_tools_url (`https://visualstudio.microsoft.com/visual-cpp-build-tools/`), build_tools_include_it (`true`: the Build Tools bring the runtime, so a PC that gets them needs nothing more)}.
  - Detection: `vcruntime140.dll`, `vcruntime140_1.dll` and `msvcp140.dll` in `%SystemRoot%\System32` (what `ktx.exe`, `ktx.dll` and `libvips-42.dll` load; a registry flag could be set while a file is gone, so the files decide). Any one missing = missing.
  - **Unchanged:** `ready` and the exit code still mean "can build games" only (0 ready, 1 not, 2 usage); the runtime never changes them, and the text report keeps its lines and adds "What this computer can do:" with both capabilities. `requirements[]` does not list `vc-runtime`.
  - **For Evani.App:** show "Importing models" separately. When it is `needs`, do not start `ktx.exe`, `libvips` or the Convert import until `vc-runtime` is `found` or `import-models.ready` is true; show `message`, `install_hint` and the two links. `trunk.exe` still waits for `ready` (the Build Tools bring the runtime).
  - Test hook: `--hide vc-runtime` (or `all`) reads as missing; the unit tests simulate it that way, and with a System32 folder that lacks one file.
  - **Build recipe (no contract change):** the pack's builds are path independent: the build folder, `CARGO_HOME`, the target and vendor folders (all inside the build folder) and the Rust toolchain folder are remapped to fixed names (`--remap-path-prefix`, both slash styles, via `CARGO_ENCODED_RUSTFLAGS`, pack builds only), so `--build-dir` may differ between runs and a build folder with a space is allowed.

## 5. Clean-machine proof plan
Gate for any public release that carries the engine (plan Phase 0). Machine: fresh Windows 11 VM, standard (non-admin) user, no Visual Studio, no Rust, no Node. Each step's evidence goes in a report beside the plan.

| # | Step | Evidence | Run locally on 2026-10-27 |
|---|---|---|---|
| 1 | Build the pack twice (`--repro-check`) | byte-identical manifests | see "Results" below |
| 2 | `engine-pack --verify` on the pack | 0 changed/missing/extra | see "Results" |
| 3 | Install the test build | no admin prompt (screen recording) | needs Evani.App's side and a VM: not run |
| 4 | Networking off; open the Evani.dev tab | the card asks for the C++ Build Tools | Simulated: checker with the SDK hidden by environment and MSVC by the hook: both "missing" in plain words, probe skipped, exit 1; re-check with tools present: ready, exit 0 |
| 5 | Networking on only for Microsoft's installer; install; close it | the tab re-checks by itself and says ready | needs a VM: not run (no installs on the shared PC) |
| 6 | Networking off: new game, build, play, `check` clean | first-build time, disk used | Partly: the pack's own toolchain and vendor folder used offline with an empty `CARGO_HOME` (see "Results") |
| 7 | `--verify` on the installed pack | passes | as step 2, after placing |
| 8 | Defender | `Get-MpThreatDetection` empty; note SmartScreen | needs a VM |
| 9 | Uninstall | nothing left in the install folder; engines in the data folder listed in the Disk tab | needs a VM |
| 10 | Mac (when its build carries the pack) | Gatekeeper accepts, CLT dialog appears and clears, a build runs | needs a Mac |

First-build cache (owner decision 4: measure, don't ship): build the sandbox once from a clean target folder with the pack toolchain, then again with only the engine crates' outputs kept; record both times and the size of the reusable part. See "Results".

## Results (2026-10-27, this PC, branch `engine-pack`)

**Clean-PC pass (pack from commit 0642f135; static C runtime and import check):**
- **Cause:** Evani.App's Windows Sandbox run showed our six prebuilt programs and upstream `trunk.exe` importing `VCRUNTIME140.dll` (no output, STATUS_DLL_NOT_FOUND). rustc, cargo, wasm-bindgen and wasm-opt were already static.
- **Fix and result:** the six prebuilt programs are built with `-C target-feature=+crt-static`; the import check over all 34 PE files (`.exe`, `.dll`, `.node`) reports 30 static and 4 allow-listed (`trunk.exe`, `ktx.exe`, `ktx.dll`, `libvips-42.dll`), 0 dynamic. The check is also covered by a unit test that compiles a real program each way (the dynamic one imports `vcruntime140.dll` and six `api-ms-win-crt-*`; the static one only `kernel32.dll` and system DLLs).
- **Pack:** 17 042 files, 860.2 MB raw, 268.0 MB gzip; `--verify` 0 changed, missing or extra; `evani --version` prints `evani 0.1.0`. `--repro-check`: byte-identical manifests and archives (SHA-256 f27efefd...). Fixed 2026-10-27: pack builds map the build folder, the toolchain folder and the compiled-in checkout folder to fixed names (`build.rs` in xtask, `EVANI_PACK_BUILD=1`), and two packs built in different `--out` and `--build-dir` folders (`out7-a`, `out7-b`) have byte-identical manifests (SHA-256 cf199366...).
- **Limits of the local proof:** every Windows folder searched for `vcruntime140.dll` includes System32, which cannot be taken off a DLL search, so starting the programs with a scrubbed PATH only proves they run without any other copy; the import table (no `VCRUNTIME*`/`MSVCP*`/`api-ms-win-crt-*` in our programs) is the real evidence. The Sandbox run is the final proof.

**Third pass (pack from commit 43567e01; signing hook, `publish --app-dir`, upgrade pictures, helper pins):**
- **Reproducibility** (`--repro-check --download --archive`, build folder wiped before each run): byte-identical manifests (3 259 465 bytes, 17 041 file hashes) and byte-identical hostable archives (SHA-256 4eb00dba...). 13 min 57 s for both runs. Size: 17 041 files, 859.5 MB raw, 267.6 MB gzip (the two browser runners add 44 KB). Every bundled tool upstream-verified: the four model and font helpers were matched against their pinned download checksums (gltf-transform against the committed lockfile, CRLF checkout form).
- **Unsigned pack:** `signing.status: unsigned`, 32 programs listed (`program: true`) for Evani.App to sign. `--verify`: 17 041 files, 0 changed, missing or extra (6 s).
- **Dummy-signed pack** (`--sign-cmd "node dummy-sign.mjs {file}"`, a test signer that appends a marker, standing in for an Authenticode signature): all 32 programs (`.exe`, `.dll`, `.node` in `toolchain/` and `tools/`) signed, 0 left unchanged; all 32 end with the marker; for all 32 the manifest's SHA-256 is the signed file's; all 32 differ from the unsigned pack's hashes and no other file differs. `--verify` passes and names the signer. After appending bytes to one signed program (`tools/bin/evani-reqcheck.exe`), `--verify` fails with `changed: tools/bin/evani-reqcheck.exe`, exit 1.
- **Offline, from the pack locked against writing** (Windows deny rule for everyone; a test write was refused), Evani.App's child environment (the pack's `tools/bin` and `toolchain/bin` first on PATH, its `RUSTC`, an empty `CARGO_HOME` with the pack's vendor config, `CARGO_NET_OFFLINE`, no rustup): `evani new-game off-sea --app-dir` (pinned 0.1.0), `evani check --app-dir` clean (1 warning: no geometry report yet), `cargo xtask build --app-dir .. --ship` through the pack's `cargo-xtask` (615.4 KB raw, 246.4 KB gzip, ship guard passed), together 48 s on a warm machine; `evani publish --app-dir .. --dry-run --quick` with a carried-over README, licence, preview and loader: all 10 preflight checks PASS, verdict ready, package and summary in the game's own `.evani/publish/`; `evani doctor --fix` refused in the pack and named the Blender command; `evani engines` found the game. Then `--verify` on the pack: 17 041 files, 0 changed, missing or extra.
- **Upgrade with pictures, "before" built by the old pack:** with that pack installed as engine 0.1.0 (`EVANI_ENGINES_DIR`) and a stand-in engine 0.2.0 (a clean copy of this commit with `pack_version` raised), `upgrade-engine --app-dir <off-sea> --dry-run`: the before picture came from a ship build made by the 0.1.0 pack's own `evani` in its own environment ("built now with its own engine 0.1.0"), the copy built and checked clean on 0.2.0, both start views captured in headless, muted Chrome; 4.19% of pixels changed (not flagged), report and pictures in `state/upgrade/off-sea-0.1.0-to-0.2.0/`. Earlier, the same game on identical code differed by 0.39% (animation drift only; the diff picture shows red only along moving edges and the frame counter).
- **Checks:** `cargo xtask check` and `check --full` clean (tests and ship builds of all eight games; the same 8 warnings about this worktree); clippy `-D warnings` clean on evani-xtask and evani-guide; tests: evani-xtask 191 + the read-only test, evani-guide 117 (new: signing 4, publish options 2, upgrade pictures 1 + start view 3, helper pins 1, Blender folder 1). `check --full` was run from a copy of the `evani-xtask` program: run directly, its nested ship builds tried to relink the running program and Windows refused (not an engine fault).
- **`publish --app-dir` from the checkout** on a game outside the engine: dry run (all checks PASS), a local package plus the preview server (`--serve`, HTTP 200 on the game page, then stopped), and `--mark-published v1`; every file stayed in the game's workspace, nothing in the engine. Nothing was uploaded.

**Second pass (pack from commit baef25a5, `--download --archive`):**
- **Complete and release-ready:** `complete: true`, `release_ready: true`, no gaps. trunk 0.21.14, wasm-bindgen 0.2.129 and wasm-opt version_123 came from their release URLs, each archive matching the recorded upstream SHA-256; the model helpers from their `doctor --fix` records.
- **Size (Windows x64):** 17 036 files, 859.3 MB raw, 267.6 MB gzip; `tools/bin` grew from 40.6 to 50.3 MB (wasm-opt 8.6 MB, the `cargo-xtask` launcher, `evani` replacing `evani-xtask`). Engine source 12.9 MB, vendor 144.7 MB, toolchain 589.0 MB, model helpers 62.4 MB unchanged. Hostable archive (route B): 276 773 210 bytes (264.0 MB), unpacked 901 MB on disk.
- **Reproducibility:** `--repro-check` (both packs from one commit, build folder wiped before each, our six tools compiled twice): manifests byte-identical (3 256 927 bytes, 17 036 file hashes), and the two hostable archives byte-identical too (SHA-256 0a1a664f...). A first attempt differed in two docs only, because a commit landed between the runs while each run resolved `HEAD` itself; `--repro-check` now resolves the commit once.
- **Offline game build from the pack** (section 5 step 6): PATH = the pack's `tools/bin` and `toolchain/bin` plus Windows only, `RUSTC`, an empty `CARGO_HOME` with the pack's config, `CARGO_NET_OFFLINE`, `EVANI_ENGINE_ROOT`/`EVANI_TOOLS_DIR`/`EVANI_STATE_DIR`, no rustup, and the pack locked against writing for everyone: `evani new-game off-sea --app-dir <folder>` (pinned to 0.1.0), `evani check --app-dir` clean (1 warning: no geometry report yet), `cargo xtask build --app-dir ... --ship` through the pack's `cargo-xtask` with Trunk running the pack's wasm-opt offline: 615.4 KB raw, 246.4 KB gzip, ship guard passed; `evani engines` listed the pack and the game. New game + check + ship build about 2 minutes; build folder 244 MB. Then `engine-pack --verify` on the pack: 17 036 files, 0 changed, missing or extra.
- **Read-only test** (`tools/xtask/tests/readonly_engine.rs`) passes; **checks:** `cargo xtask check` and `check --full` clean (8 warnings, all about this worktree: geometry reports, an unapproved look limit, no installed live server); clippy `-D warnings` clean on every changed crate; tests: evani-guide 116, evani-xtask 183 + the read-only test, evani-live-bridge 96, evani-lookguard 39, evani-cargo-xtask 1.

**First pass:**
All outputs on `<scratch>\evani-enginepack\` (never in the repo). Builds at below-normal priority, 1-2 jobs, on a PC busy with other projects, so times are upper bounds.

**Windows x64 pack, part by part** (`repro-a`, commit 8fdac4e8; gzip-6 as a stand-in for the installer's LZMA):
| Part | Files | Raw MB | gzip MB |
|---|---|---|---|
| engine-runtime (12 crates) | 303 | 3.51 | 1.09 |
| procedural-and-materials (+ catalog) | 52 | 0.62 | 0.18 |
| evani-model (crate, CLI, fur locks) | 71 | 0.76 | 0.23 |
| looks-and-assets | 64 | 0.59 | 0.38 |
| dev-tools-source (+ harness, pins, 5 fixtures) | 198 | 4.83 | 2.07 |
| templates (example kit, sandbox) | 31 | 0.26 | 0.09 |
| geocheck-reference-kit (kart kit) | 40 | 0.78 | 0.24 |
| ai-guidance-and-docs | 40 | 1.34 | 0.53 |
| engine-other (Cargo files, licence) + pack files | 9 | 0.11 | 0.03 |
| **engine source subtotal** | 808 | **12.8** | **4.8** |
| vendor (233 crates) | 11 899 | 144.72 | 27.27 |
| toolchain (Rust 1.99.1, host + wasm32) | 113 | 588.99 | 196.10 |
| tools-bin (ours 8.7 MB: xtask 2.88, live bridge 2.50, convert 1.98, model 1.09, reqcheck 0.29; trunk 27.3; wasm-bindgen 6.1) | 7 | 40.62 | 13.42 |
| tools-convert (gltf-transform 4 196 files, ktx, gltfpack, msdf-atlas-gen) | 4 202 | 62.36 | 22.10 |
| **TOTAL** | 17 029 | **849.5** | **263.7** |

Inside the toolchain: `rustc_driver` DLL 196.5 MB, `rust-lld` 116.9 MB, the two `libcore` metadata files 65 + 43 MB, cargo 26.3 MB. Missing from this pack: wasm-opt (not on this PC; estimated 10-30 MB). Installer estimate: the pack adds about 200-230 MB under LZMA (gzip says 264 MB; LZMA solid compresses better), so the Windows installer goes from 166.7 MB to roughly 370-400 MB; on disk about 0.85 GB more, plus build folders per game.

**Mac (macos-arm64, macos-x64):** not packable on this PC (the packer refuses to fake another platform's toolchain and tools: `gaps` lists them). Platform-independent parts are the same bytes (engine 12.8 MB, vendor 144.7 MB); the toolchain and tools are estimated at a similar 600-700 MB raw. Needs a Mac release machine.

**Reproducibility:** `--repro-check` built the pack twice, wiping the build folder each time (so our five tools were compiled from scratch twice, 7 min 29 s each at 1 job): the two `manifest.json` files are byte-identical (3 255 678 bytes, 17 029 file hashes).

**Verify:** `--verify` on the first pack: 17 017 files, 0 changed, missing or extra. Run again on `repro-a` after the offline game build (step 6): 17 029 files, 0 changed, missing or extra (229 s), so building a game wrote nothing into the pack.

**Requirements checker demo** (`<scratch>\evani-enginepack\demo\reqcheck-demo.txt`): with `Program Files (x86)` and `SystemRoot` pointed at an empty folder (no `vswhere`, no `reg.exe`, no SDK folder) and the MSVC tools hidden by `EVANI_REQCHECK_HIDE` (Windows re-sets `ProgramFiles` for each process, so Visual Studio under `C:\Program Files` can't be hidden by environment): "Not ready", both tools MISSING with their plain sentence, install hint and official link, the test build "not run", exit 1. Re-run with the normal environment: both found, test build passed in 0.7 s, "Ready", exit 0. The pack's own prebuilt checker, run with only system folders on PATH, found the pack's toolchain beside it and passed its probe in 6.4 s.

**Offline game build from the pack only** (step 6, partly): PATH = the pack's `tools/bin` and `toolchain/bin` plus Windows, `RUSTC` = the pack's rustc, an empty `CARGO_HOME` with the pack's `cargo-config.toml`, `CARGO_NET_OFFLINE`, no rustup: `cargo build --offline --locked -p evani-sandbox --target wasm32-unknown-unknown` in the pack's `engine/` built in 193 s (63 crates, 2 jobs). The first try failed on `tools/bake-index.rs` missing from the allow-list; fixed, with a test (`files_members_reach_outside_their_folder_are_in_the_pack`). Not covered yet: Trunk (needs wasm-opt), the tab, `check` from inside a pack.

**First-build cache (measured, not shipped):** the debug build folder after that first build is 839 MB; removing only the sandbox crate's own outputs (159 MB) and rebuilding took 3 s instead of 193 s. So a cache of the engine crates' build outputs for this feature set is about 680 MB on disk and saves about 3 minutes on this PC for this game's dev build (more on a slow laptop; each feature set and profile needs its own). Not shipped (owner decision 4).

**Checks on the branch:** `cargo xtask check` clean; `cargo xtask check --full` clean (tests, ship builds of all eight games; 8 warnings, all about this fresh worktree: no installed live server, old or missing geometry reports); `cargo clippy -D warnings` clean on evani-reqcheck, evani-xtask, evani-guide and evani-live-bridge; new tests: evani-reqcheck 10, engine_pack 14, extools 1, live-bridge session 1.

## 6. Mac packs (design only; nothing here has run)
Evani.App's Mac build is the Electron app, built on Codemagic (`codemagic.yaml` workflow `mac-release`, `mac_mini_m2`, started by hand, never publishes), signed with the Developer ID Application certificate and notarised with the "Evani Codemagic" App Store Connect API key by electron-builder; both Mac kinds (arm64, x64) come out of one run. The engine pack joins that run; this PC can't make it (the packer refuses another platform's toolchain).

**Where it is built.** A new step in `mac-release`, after "Rust for the process helper" and before "Signing": for each of `macos-arm64` and `macos-x64`, `cargo xtask engine-pack --platform <p> --download --cache $HOME/evani-upstream --out $CM_BUILD_DIR/engine-pack --build-dir /tmp/evani-pack-build`. Each needs, on the Codemagic Mac:
- Rust exactly as pinned (`rustup toolchain install 1.99.1 --profile minimal -c clippy,rustfmt -t wasm32-unknown-unknown`, plus `-t x86_64-apple-darwin` for the Intel pack); the packer refuses any other `rustc -Vv`. The existing step installs `stable`; the pack step installs the pinned version beside it (never replacing the default).
- The engine checkout at the release commit (Evani.App's repo pulls the engine as a pinned commit or submodule: an Evani.App decision; the packer reads git blobs from that commit).
- Network to GitHub for the three upstream tools (`--download`, checked against `sha256_upstream`, cached between runs with Codemagic's `cache_paths: $HOME/evani-upstream`).
- The model helpers (gltfpack, ktx, gltf-transform, msdf-atlas-gen) have only Windows pins today; Mac pins (`macos-*` entries in `packaging/engine-pack.json`, each with its release URL and SHA-256) are needed before a Mac pack can be complete. Until then a Mac pack is `--allow-incomplete` with those gaps listed and `complete: false`, and Evani.App shows model import as unavailable on Mac.
- Intel pack on an Apple-silicon builder: the toolchain is copied from the `x86_64-apple-darwin` toolchain (rustup installs it on arm64 and runs it through Rosetta 2 for the packer's `--version` checks), and our five tools are built with `--target x86_64-apple-darwin`. Packer change needed: build prebuilt tools for a `--target` other than the host and take the toolchain from the matching rustup toolchain (`rustup toolchain install 1.99.1-x86_64-apple-darwin`). Not built yet; small.

**Signing and notarisation.** Every Mach-O in the pack must carry the Developer ID signature with the hardened runtime and a secure timestamp, or Gatekeeper blocks it the first time Evani.App starts it (the pack is copied out of the app bundle into the data folder, so it is not covered by the app's own signature):
- Sign, in this order, after the pack is built and before it is copied into the app: every `*.dylib` in `toolchain/lib` and `toolchain/lib/rustlib/*/lib` (rustc's own `librustc_driver-*.dylib`, LLVM), then every executable in `toolchain/bin`, `toolchain/lib/rustlib/*/bin` (`rust-lld`, `gcc-ld/*`), `tools/bin` and `tools/convert`: `codesign --sign "$identity" --force --timestamp --options runtime <file>`. Use the same identity and unlocked keychain the Signing step prepares.
- Entitlements: rustc loads its own `librustc_driver` dylib and cargo starts rustc, build scripts and proc-macro dylibs that are compiled on the user's Mac (unsigned, not ours). With the hardened runtime, library validation would refuse to load an unsigned dylib into a signed process: `rustc` loading a freshly built proc-macro is exactly that. So `rustc` (and `clippy-driver`, `rustdoc`) get `com.apple.security.cs.disable-library-validation`; nothing else gets any entitlement. Test this first on the Codemagic Mac: build a game that uses a proc-macro crate (serde_derive) from the signed pack.
- Notarisation: Apple notarises a container, not a folder. The pack's Mach-Os are notarised as part of the app (they sit inside `Evani.App.app/Contents/Resources/evani-engine/<ver>/` when electron-builder notarises the app), and the ticket is stapled to the app. Files copied out of the app keep their own code signatures; Gatekeeper checks those signatures and the notarisation record online (the ticket for each file is on Apple's servers, keyed by its code-directory hash), so a copied file still passes. For route B (a separate download) the archive is zipped, notarised on its own (`xcrun notarytool submit pack.zip --wait`), and the files inside are signed the same way; a zip can't be stapled, so the first start needs the network once, like any notarised download.
- Quarantine: Evani.App writes the pack itself (copy out of its bundle, or its own download), so the files carry no `com.apple.quarantine` attribute unless the download code adds it; it must not.
- electron-builder: the pack goes in as an `extraResources` folder per Mac kind; `afterPack` (scripts/release/afterPack.cjs) is where the signing loop above runs, since electron-builder's own signing pass only signs the app's known binaries and would re-sign with its own entitlements. Check that the app-level `codesign --verify --deep --strict` step (already in the workflow) still passes with the pack inside.

**Checks on the Mac run** (added to "Check both Mac apps"): `engine-pack --verify` on each pack (the signatures change the Mach-O bytes, so the manifest must be written after signing: the packer needs a `--sign-cmd <cmd>` hook that runs on every Mach-O before hashing; not built yet), `codesign --verify --strict` on every Mach-O, `spctl --assess --type execute` on `tools/bin/evani`, `evani-reqcheck --json` (finds the Command Line Tools on the Codemagic Mac, probe passes), and an offline test build of the sandbox copy (`evani new-game` into a temp folder, then `evani build --app-dir ... --ship`) with network off for that step.

**Requirements on a user's Mac:** the Xcode Command Line Tools (the linker); the checker already detects them and opens Apple's own install dialog (section 3).

**Size:** as Windows, about 600-700 MB raw per Mac kind (the arm64 and x64 packs are separate downloads; the app for each kind carries only its own).

**Open for Evani.App:** how the Mac app gets the engine commit (submodule, pinned copy, or a pack built on the Windows PC's schedule); whether Mac goes route A (in the dmg) or B (download on first start: the dmg stays small, which matters more on Mac where electron-builder already gives differential updates by blockmap).

## 7. Decisions (branch `engine-pack`, recorded for the join)
1. The installer grows to whatever the full engine and toolchain need; trimmed as far as possible (one host std, one wasm std, no pdb/docs/sources, only wasm-opt from binaryen, model helpers trimmed to their programs). Sizes are reported per part and platform. (Owner.)
2. Tools we may not ship (Microsoft C++ Build Tools / Windows SDK, Xcode Command Line Tools) are detected and the user is sent to the official installer with "To build games you need <tool>, because <why>"; we never claim to build without them. Everything else is bundled. (Owner.)
3. Old engines on disk: the current one plus every version a user's game pins, capped at 3 extra. Mapped to Evani.App's classes: live = `.evani-build-` + `.evani-live`; pinned = `.evani-keep-`; unpinned = `.evani-build-` (newest 2 kept). (Owner; mapping by the Evani.Dev orchestrator.)
4. First-build cache: measured in Phase 1, not shipped. (Owner.)
5. Full-installer app updates: a separate Evani.App item, not blocking. (Owner.)
6. The requirements checker is its own std-only program (`evani-reqcheck`), not only `xtask doctor --requirements`, so it is small and runs before any Rust; xtask runs the same code. (This branch.)
7. `kits/evani-kit-kart-racer` (0.78 MB) ships in the pack: `tools/evani-geocheck` has an optional dependency on it, and Cargo resolves optional dependencies, so the workspace can't load without it. It is the geometry check's built-in example adapter, not a game we ship. (This branch.)
8. The pack is `engine/` + `toolchain/` + `tools/` + `vendor/` in one folder; there is no separate `ai/` or `templates/` folder: the AI guidance and the new-game templates are already in `engine/` where the commands read them. (This branch.)
9. Tools without an upstream-verified download are packed from local copies and marked `upstream_verified: false`; such a pack is `complete` but never `release_ready`. (This branch.)
10. The pack lives outside the install folder (Evani.App's update replaces `resources/`): staged in `resources/evani-engine/`, moved to the data folder on first start. (This branch, from Evani.App's facts.)
11. The engine command in the pack is `evani` (the prebuilt xtask), with a `cargo-xtask` launcher beside it and no `xtask` alias in the pack's `.cargo/config.toml`, so `cargo xtask` and `evani` are the same and nothing is ever compiled to run a command. The launcher is its own std-only crate (about 0.2 MB) rather than a second copy of the 3 MB tool. (This branch.)
12. A game outside the engine has the engine's own layout (`apps/<id>`, `kits/evani-kit-<id>`, `tools/bake-index.rs`) so the templates' relative paths and every doc keep working; an engine root now also needs `crates/evani-core`, so a game folder is never taken for an engine. (This branch.)
13. Retention renames packs between Evani.App's build and keep classes, which moves the absolute engine paths in pinned games; the engine relinks them (`engines --apply`, `relink`) rather than giving up the class names Evani.App's disk clean-up already understands. (This branch.)
14. wasm-opt is pinned to binaryen `version_123`, the version Trunk 0.21.14 itself downloads and uses on this PC, so pack builds and checkout builds run the same optimiser (was an unconfirmed `version_125`). (This branch.)
15. Upstream checksums are recorded only after matching the project's own published checksum (trunk and binaryen `.sha256` files, wasm-bindgen's release asset digest). (This branch.)
16. In a pack, the engine's own example games still play in the Evani.dev tab (built under `EVANI_STATE_DIR`), but `evani build sandbox` and friends refuse and point at `new-game --app-dir`: a user's work always lives in their own folder. (This branch.)
17. Route A stays the default (owner's words); route B (hosted, signed descriptor) is built in the packer so Evani.App can choose per platform; neither route lets a normal app update carry the engine. (Orchestrator's addition, this branch.)
18. The signing hook runs inside the packer, before hashing, with one external command per program (`--sign-cmd` with `{file}`, `--signer`); a signer that changes no program refuses the pack, so Evani.App's `sign.mjs` without its settings can never yield a pack marked signed. Real signatures carry timestamps, so the reproducibility check is run unsigned. (This branch.)
19. `upgrade-engine` takes the pictures itself and only flags a large difference (default over 10% of pixels); it never blocks the upgrade on it. The tab is expected to run `--dry-run`, show the pictures, then upgrade on the user's yes. (This branch.)
20. "Before" is built by the game's own installed pack when there is one (exactly the old engine), else its last ship build, named as such in the report. (This branch.)
21. Blender is pinned (version, URL, SHA-256) but never bundled; in a pack it is installed per user into the state folder by `doctor --fix --blender`. The four helpers' pins live in both `extools`/`fonttools` (what `doctor --fix` installs) and `packaging/engine-pack.json`, kept equal by a test. (This branch.)
22. The Intel Mac pack (built with `--target x86_64-apple-darwin` on the Apple-silicon builder, section 6) is left open: it needs a Mac to build and test. Mac pins for the helpers are open for the same reason. (This branch.)
23. **trunk stays upstream's build, allow-listed.** Building trunk from source with a static C runtime would mean vendoring its whole dependency tree (hundreds of crates, a second lockfile) into the pack and trusting a build we cannot compare with the upstream SHA-256 we verify today. trunk only runs a game build, which needs the C++ Build Tools first (they bring the runtime), and the requirements checker (static) gates builds. So trunk is on `runtime_allow` with that reason; the same check keeps it honest (it fails if any other program, or trunk's replacement, changes). No loose Microsoft DLL is shipped. (This branch.)
