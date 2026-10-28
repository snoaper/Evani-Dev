# Publishing a game to the Evani.app library

## One command: `cargo xtask publish <id>` (Stage 4, 2026-10-16)

`cargo xtask publish <id>` does steps 1 to 4 below in one go, and never uploads: only the owner's click on Evani.App's publish card does (step 5).

```text
cargo xtask publish <id> [--dry-run] [--serve] [--port 8114] [--out <dir>] [--from <dir>] [--quick]
cargo xtask publish <id> --mark-published v5 [--out <dir>]
```

1. **Ship build** from `apps/<id>/evani.toml`, with the same guards as `cargo xtask build <id> --ship` (no dev-only code, free roam, the 10 MB cap, the engine notice). A game kept outside the engine folder, in its own Cargo workspace, ship-builds with the same guards through `cargo xtask build --app-dir <its app folder> --ship` (run by the engine its `evani-core` path points at; `build-facts.json` then also records the game's own commit). `publish` itself does not take `--app-dir` yet: steps 2 to 8 are done by hand for such a game, from its `target/evani-sizes/<id>-ship/` folder.
2. **Package** in a staging folder next to the destination (`publish/<id>/`, or `--out <dir>`): the build's files, the page with the Evani loading screen as the first script in `<head>` (a loader tag that is not first is moved there) and the smooth start, and the hand-made files carried over from the previous package (`README.md`, `LICENSE`, `preview.png`, `evani.json`, `evani-loader.js` with its title line set from the `<title>` of `apps/<id>/index.html`). Nothing else from the old folder is kept, so stale hashed `.js`/`.wasm` files go. The previous package is the destination itself, else `publish/<id>/`; in a worktree (which has no `publish/`) pass `--from <main checkout>/publish/<id>`.
3. **Preflight** on the staged folder (the same checks as `cargo xtask package-check <folder> <id>`): under 10 MB raw, no dev-only code, `preview.png` is a PNG of 512 px or more on each side, square or 16:9 (Evani.App's rule; a smaller one would go out unlisted), the loader present, titled and first, `README.md` + `LICENSE` + `ENGINE-LICENSE.txt`, every file the page loads is in the folder, one wasm and no stale engine file, `evani.json` matches `evani.toml`, a playground with free roam, the smooth start. Only when every check passes is the staging folder swapped in (two renames; the old folder is put back if the second fails, and an interrupted swap is recovered on the next run), so a failed run leaves `publish/<id>/` as it was; the rejected package is kept in `.evani/publish/<id>/failed/` to look at. A folder that is not a package (no `index.html` and loader) is never replaced. `--dry-run` never touches the destination: the package goes to `.evani/publish/<id>/dry-run/`.
4. **Size report**: by module first (one more ship build per optional module off, plus the base; `--quick` skips them), then by file, every figure labelled raw / gzip / Brotli. The 10 MB cap is on the raw whole folder; the 1.5 MB size line on the raw build files; the download line counts what a player's browser fetches (index.html and the files it loads, not README, licences or preview). Then **per-tier download targets** (S7-1, `catalog/tiers.json` `download_targets`, provisional until the kart racer is measured on Low and Ultra): one `PASS` or `FAIL` line per tier with code, boot and total against the tier's targets and the margin under the tightest, judged on the tier's heaviest texture family (ASTC, BC7 or the PNG/JPEG fallback) so a pass holds on every device. Code ceilings are 350 KB gzip for 2D games and 500 KB for 3D ones (a `3d` look or `models`). The verdicts go into `summary.json` (`sizes.tier_targets`); they report, they don't block the package, and `cargo xtask check` warns about a failed tier in the last summary. `cargo xtask bytes <id>` shows where those bytes went.
5. **Diff** against the version recorded as live (path, SHA-256 and size per file): NEW, MODIFIED, SAME, DELETED, the whole-folder size change and what a returning player fetches again; "nothing changed since vN, skip" when the package is identical. With no version recorded every file counts as NEW ("no previous manifest").
6. **Summary**: `summary.json` and `summary.md` (with `package.json`, the manifest) in `.evani/publish/<id>/`, outside the uploaded folder and gitignored: the suggested next version, sizes, diff, preflight and preview address.
7. **Preview**: `--serve` keeps serving the package at `http://localhost:8114/<id>/` (a subfolder, as on play.evani.app; `?mode=free` for free roam) until stopped.
8. **After the owner's click**: `cargo xtask publish <id> --mark-published v5` records the package folder as live `v5` (`.evani/publish/<id>/published/v5.json` and `live.json`), so the next diff is against what players get. A version is never rewritten with another package.

Serve the game over https (or localhost, using `adb reverse` for phone testing). Plain LAN http addresses hide WebGPU, so the game shows "This page needs a secure address".

The manual steps below are what the command automates, kept as the reference for what a package holds.

## The statement beside every game: `build-facts.json`

`cargo xtask build <id> --ship` and `cargo xtask publish <id>` write two files next to `index.html`, the same for every portal, never by hand:

- `ENGINE-LICENSE.txt`: the engine's MIT notice for the runtime (one line saying what it covers, then the root `LICENSE`; docs/LICENSING.md).
- `build-facts.json`: a short statement made from `evani.toml` and the ship build: game id, kit, look and licence; engine version and commit (the short hash baked into the wasm) and whether the engine had uncommitted changes; modules; code sizes (raw and gzip); network (none to other sites, unless the `net` module is listed); personal data, ads, analytics, tracking and cookies (none); storage (`localStorage` for settings and progress only); input methods (touch only if the kit declares touch controls); AI use (development assistance yes, generative AI while playing no); content rights and third-party files.

Nothing in the page loads the file, so it adds 0 bytes to the download (it is not in the wasm, js or html counts) and it counts only a few hundred bytes toward the 10 MB package cap. It deliberately omits toolchain versions, the build command and import lists.

The only author-written part is an optional table in `apps/<id>/evani.toml`:

```toml
[statement]
game_licence = "MIT"                       # default: "see the LICENSE file in this package"
third_party = ["Font X (SIL OFL 1.1)"]     # default: none
```

The fixed statements (no ads, analytics, tracking or cookies; AI use) are written once in `tools/xtask/src/build_facts.rs`; change them there if the engine ever changes.

## How the Stage 2 games were packaged by hand

How the three Stage 2 games (kart-racer, 2d-platformer, match-3) were packaged. The address of a playground on Evani.app uses the game's catalog id (`kart-racer`, `2d-platformer`, `match-3`), not its title.

1. **Build in a worktree**, never the main checkout:
   `git worktree add ../Evani.Dev-publish -b publish-stage2 m1-foundation`, then with `CARGO_TARGET_DIR` inside it and `CARGO_BUILD_JOBS=2` run `cargo xtask build <id> --ship`. It writes relative (`./`) paths into index.html, so the folder works from any subfolder, and runs the ship guard (no live module, no live code swapping, and no root-absolute path such as `/evani-...js`, which it refuses with an ERROR and a `fix:` line), prints the per-module size table and `playground package: X of 10.00 MB cap`; it fails over the cap or without free roam. The folder is `target/evani-sizes/<id>-ship/` (index.html, js, wasm, and `ENGINE-LICENSE.txt`). The build writes `ENGINE-LICENSE.txt` itself (a short notice that the package includes the Evani.Dev runtime, under the MIT licence, followed by the root `LICENSE`, Copyright (c) 2026 Chris Pearson), so copying the ship folder carries the notice; it fails the build if the notice is missing or incomplete.
2. **Package** (no manual path fix is needed any more) into `publish/<id>/` in the main checkout (untracked; the upload tool only takes project folders; do not commit it): copy the ship folder, then
   - download `https://evani.app/evani-loader.js` next to index.html, change only the `EVANI_BIOME_TITLE` line to the game's title, and make `<script src="evani-loader.js"></script>` the first script in `<head>` (we also add `<link rel="icon" href="data:,">` so the page makes no favicon 404);
   - smooth start (keeps the logo's entrance at a steady frame rate): run `node tools/smooth-bootstrap.mjs publish/<id>`. It rewrites the inline Trunk bootstrap in index.html so the wasm is fetched and compiled at once but the engine starts only after the loader's logo has landed (about 1.8 s), then the loader is held until frames flow again. It needs a loader that has `EvaniLoader.landed` and `EvaniLoader.hold()` (the improved copy in `.vibedeck/loader-fix/evani-loader.js` until Evani.app's own file has them; with an older loader the game simply starts at once, as before). Measured, cold start: the logo entrance went from 40 to 79 frames in 1.8 s (108 is perfect) to 94 to 107, the longest stall from 220 to 1380 ms down to 50 to 180 ms, and the loader now goes about 0.7 s later (the engine starts 1.8 s later but the old screen sat there for 2.3 s anyway). The template apps/<id>/index.html is not changed: dev runs start the engine at once;
   - add `README.md` (what it is, controls as they are today, Play and Free roam, made with Evani.Dev, original content; its licence line reads exactly `Game code: MIT. Includes the Evani.Dev runtime, also MIT (see ENGINE-LICENSE.txt).`; `publish` swaps any older line starting `Game code: MIT.` for this one by itself), `LICENSE` (the game's own licence, MIT, Copyright (c) 2026 Chris Pearson (@snoaper), unless the game's owner chose other terms; it covers the game's code and art, not the runtime), `preview.png` (a real in-game headless capture) and `evani.json` with `"freeRoam": true`, `"kit"` and `"look"` from the game's `evani.toml`.
3. **Run it from the folder**: serve its parent folder with a plain static server, so the game sits in a subfolder as on play.evani.app, and open it in headless Chrome (muted; pointer lock, fullscreen and keyboard lock stubbed). The loader shows, the game reaches its title with no console errors, and `?mode=free` logs `play mode: free roam`.
4. **Check**: first `cargo xtask package-check publish/<id> <id>`, which fails if the folder lacks `ENGINE-LICENSE.txt` (or has a README without the licence line); then `evani_publish_check` with kind `playground`, title, tagline, description (80+ characters), 2 to 8 tags, licence MIT (the game's own code; the runtime's MIT notice is inside the package), visibility public, `includes` (engine, kit, look, HUD) and origin. Size is still required by the tool (`small` stands in until the site drops world sizes).
5. **Publish**: only through the Publish card, which the user clicks. Later versions (touch controls, fixes) go out as new versions of the same item (`evani_publish_version`) after a fresh ship build from the updated branch.

Stage 2 ship sizes (raw, whole folder): kart-racer 0.93 MB, 2d-platformer 0.76 MB, match-3 0.76 MB, all far under the 10 MB cap.

## Published (2026-10-15)

Version 2 of all three (2026-10-15) fixed the blank screen caused by root-absolute paths (`/evani-...`) in index.html, which 404 when the game is served from a subfolder.

Chris clicked Publish on all three; each is public, MIT, version 1, as @snoaper, origin "own", size `small` as a stand-in (the site still asks for a world size; Chris: "upload anyway").

| Game | Title | Library id | Packed (compressed upload, from the publish confirmation; raw folder sizes above) |
| --- | --- | --- | --- |
| kart-racer | Sugarpeak Rally | `i_OZCWbnEu7vbV` | 646 KB |
| 2d-platformer | Fennel's Leap | `i_yF_pKu7_vZk_` | 336 KB |
| match-3 | Puff's Sweet Grid | `i_SIlVcml5oTsB` | 733 KB |

Updates (touch controls for the kart racer and the platformer, the match-3 critic fixes) go out with `evani_publish_version` on these ids after a fresh ship build.

Licence note (2026-10-28): the engine is MIT (DECISIONS.md, docs/LICENSING.md). The three games above were published before that; their next versions ship with the MIT `ENGINE-LICENSE.txt` and the README line above, and until then the live copies are the old packages.
