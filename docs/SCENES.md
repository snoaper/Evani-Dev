# Scenes: fly-around environments on Evani.app

Chris's rule (2026-11-08): "Make it a rule that all Scenes become scenes in Evani.app website that you can free roam with like a flying thing". Playgrounds are game examples; a Scene is its own kind (the sun, moon and meadow; a landscape, sky or water).

Every scene the studio makes (a whole environment, not a single object) gets a matching Scene built from its best round. It reuses the playground build rules (docs/PLAYGROUNDS.md):

- `cargo xtask new-game <id>` makes it; `free_roam = true` under `[playground]` and the kit's `free_roam` returns true. There is no character: the player is a free-flying camera.
- The camera flies on keys and mouse, and on touch with sticks (`touch` feature, `kit.touch_controls(TouchLayout::new()...)`, `.pause("pause")`). No goals, timer or fail state.
- A kit test calls `free_roam_test`. A Scene has no size cap: The 10 MB download cap applies only to games (and playgrounds, which are game examples); the library, Scenes, viewers and tools have no size cap. `build --ship` prints its size without a limit (the `scene-` id prefix is what exempts it, `evani_guide::playground::size_cap`). Keep it light anyway: every byte is a visitor's download.
- Run `cargo xtask publish <id> --dry-run`, preview it (`--serve`, a localhost link or a screenshot), then it waits for Chris's click on the publish card. Nothing is uploaded without it.
- Name it `scene-<name>`: the id prefix is what marks it as a Scene for now. `evani.toml` has no separate Scene field yet, so the Scene kind on Evani.app is chosen on Evani.App's publish card.
- The first one, the Sun and moon meadow (`scene-sunmoon-meadow`, from Asset Studio column 38's round 8), is the template for the rest: copy its pattern, not its folders. Its kit has the free-flying camera (`Flyer`, keys + mouse with pointer lock on click, touch move stick, look pad and up/down buttons), a slow day/dusk/night clock (`kit.scene_sunmoon_meadow.time`, `day_minutes`) and the round's scene in `src/scene.rs`.
