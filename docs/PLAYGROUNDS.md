# Playgrounds: what the library calls a playable, remixable game

Chris picked the name on 2026-10-15 (DECISIONS.md, "Library naming"). This page is the wording and the site structure.

## The name

A **playground** is a small interactive example of what can be created with the Evani.Dev engine and the Evani.app library, published in the library. It is not limited by world size, only by a total package size of 10 MB (below). It is a starting point for your own game. The word replaces "biome" in everything a visitor reads.

Options weighed (concept page: `.vibedeck/concepts/library-naming/index.html`):

| Name | For | Against |
| --- | --- | --- |
| Starter games | Says playable and starting point at once | Sounds like a beginner's tier |
| Templates | Familiar from website builders | Sounds like a blank form, not a game |
| **Playgrounds** (chosen) | Fun; fits free roam and tinkering | Can undersell games with goals and levels, so cards always show the play modes |
| Games (every one remixable) | Plainest word | The "make it yours" idea rests on one button |

Glossary terms stay as they are. A **kit** is a playground's gameplay crate, a **look** is its art style, and a **model** is a prop or model pack. A playground is made from one kit and one look, plus whatever models it uses. The kinds in the library are **playgrounds** and **assets** (models and model packs; Chris, 2026-10-15). Inside an item, single models are still called models, as in "Made from".

## What a visitor can do with a playground

1. **Play**: the game as made, single player, with its goals and levels.
2. **Free roam**: the same world with no goals or timer; walk, drive or fly around it.
3. **Make it yours**: get your own copy in Evani.App, then remove things, add new ones, build new levels, change the look or tune it live in the Look Lab. You can publish the result as a new playground, credited back to the one it came from.

Play and Free roam are two modes of the same item, not two items. Playgrounds are single player; multiplayer is not part of this wording.

## Where it shows on the site

- Search box: "Search playgrounds and assets".
- Kind filter: Everything · Playgrounds · Assets. The stage filter (Any stage · Listed · Trusted ✓ · Featured ★) is unchanged.
- Card badge: "Playground". Under the title: the modes it offers, e.g. "Single player · Free roam". There is no world-size label.
- Card buttons: **▶ Play**, **Free roam**, **Make it yours**. Free roam is left out when a playground doesn't offer it.
- Detail page: "Made from" lists the kit, the look and the models used, and "Remixed from" links the parent when there is one.

## Copy

- Library intro: "Playgrounds are complete games made with Evani.Dev. Play them as they are, roam around them, or make one your own."
- Make it yours (button help): "Get your own copy to change. Remove things, add new ones, build levels, then share it as a new playground."
- Free roam (button help): "Explore the world at your own pace: no goals, no timer."
- Empty search: "No playgrounds match yet. Try another word, or start your own in Evani.App."

## What the site expects from a game (Evani.App, 2026-10-15)

- Free roam opens the preview with `?mode=free`, and the demo iframe gets `#mode=free`. A game that offers free roam reads that and drops its goals and timer.
- A library item can set `freeRoam: false` to hide the Free roam button. Playgrounds built with Evani.Dev always offer free roam (below), so they leave it on.
- "Made from" reads `item.kit` and `item.look` when they are present. The publish step should fill them from the game's `evani.toml`.

## Package size: 10 MB total

Chris (2026-10-15): a playground is "no longer limited by size ... at the moment it is 10mb" total file size. There is no world-size requirement (the old Small / Medium / Large 25, 50, 100 m sizes are gone). The cap is on the whole uploaded game: wasm, js, html and assets together, measured as the uncompressed size on disk of the ship folder (10 × 1024 × 1024 bytes), because that is what is uploaded. Evani.App enforces the same cap on upload.

- `cargo xtask build <id> --ship` prints `playground package: X of 10.00 MB cap` for a game marked `[playground]`, and fails with an ERROR and a `fix:` line when the total is over. The limit is one constant, `PACKAGE_CAP_BYTES` in `tools/evani-guide/src/playground.rs`.
- It also writes `ENGINE-LICENSE.txt` into that folder (the Evani.Dev runtime's MIT notice; docs/LICENSING.md) and fails if the notice is missing; `cargo xtask package-check publish/<id> <id>` runs the whole publish preflight on a package. A new game needs nothing extra: the notice is generated, and the package README carries the licence line in docs/PUBLISHING.md.
- `cargo xtask publish <id>` builds, packages and preflights a playground in one command (the cap, the loader, the licences, free roam and the rest; docs/PUBLISHING.md), prints the size report (raw, gzip and Brotli, the cap on the raw whole folder) and the diff against the live version, and never uploads: only the owner's click on Evani.App's card does.
- `cargo xtask check` warns when the last ship build in `target/evani-sizes/<id>-ship` is over the cap (it never builds).
- Keeping under it: fewer or smaller textures, sounds and models, and modules switched off in `evani.toml` (`build --ship --diff` shows each module's size).

## Every playground supports free roam

Chris's rule (2026-10-15, DECISIONS.md): "make sure games do support free roam when building for Evani.app website and uploading a playground". How Evani.Dev keeps it:

- **The engine reads the mode.** `evani_core::play::PlayMode` is `Play` (the default) or `FreeRoam`, a world resource set before the kit is built. On the web `App::new` reads `mode=free` from the page's query or fragment (`?mode=free`, `#mode=free`, also among other `&` pairs); anything else is Play. Natively, tests call `App::with_play_mode(PlayMode::FreeRoam)` before `.with_kit(..)`. In free roam the game logs `play mode: free roam` at start, which a headless check can read from the console.
- **The kit drops its fail state.** A kit that supports free roam returns true from `KitPlugin::free_roam` and, while `FreeRoam`, skips its goals, timer, lives, fail state and game over: same world, same controls. It reports fail events in the `PlayOutcomes` resource (`game_over()`, `timer_expired()`, `life_lost()`) in both modes. A kit that returns false is played normally when free roam is asked for.
- **A test proves it.** `evani_core::play::free_roam_test(&mut app, seconds, &script)` runs the game at 60 Hz with scripted keys (`ScriptKey::down(at, "Space")`, `ScriptKey::up(..)`) and fails on any game over, timer expiry, lost life or logged error. The kit adds its own checks on `app` afterwards (the player still in the world). The example kit's `free_roam_holds` test is the pattern.
- **The game declares it.** A game made for the library has this in its `evani.toml`:

  ```toml
  [playground]
  free_roam = true
  ```

  `cargo xtask new-game` adds it, and the example kit supports free roam, so new games start compliant. Remove the table for a game that isn't for the library. Evani.App's publish check reads this exact field (or `"freeRoam": true` in an `evani.json`), so keep the name `free_roam`.
- **Check and ship enforce it.** For a game with `[playground]`, `cargo xtask check` (and MCP `evani_check`) reports an ERROR with a `fix:` line when `free_roam = true` is missing, when the kit has no `fn free_roam`, or when no `#[test]` in the kit or the game calls `free_roam_test`. `cargo xtask build <id> --ship` refuses to build until they are closed, and the review card's item 29 (docs/KIT_PROCESS.md) checks it.
- **Existing games.** The sandbox, kart-racer and 2d-platformer are not marked as playgrounds yet. Free roam for the kart racer and the platformer is Stage 2 kit work (ROADMAP.md); each becomes a playground when it has it.

## What changes where

- This repo: user-facing "biome" wording becomes "playground" (STAGES.md). `catalog/genres.json` "Procedural biomes" is a gameplay feature of a genre (generated terrain zones), not this term, and stays.
- Evani.App and evani.app: the library's kind name, filters, cards, publish flow and loading screen wording. That is Evani.App's project. Internal identifiers (the `biome` kind value, the publish tool arguments) stay until Chris confirms a code rename, and only the words a person reads change now.
