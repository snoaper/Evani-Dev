# Material shaders: one small shader per material, and the budget

Every catalogue material draws with its own small shader. There is no
all-in-one model shader: one that carried every compiled recipe took the
browser 21–28 s to compile on an RTX 4060 Ti (236.8 KB of WGSL), and every
model pipeline waited for it. A material's own shader is 36–58 KB (the
module the browser gets, measured in the material viewer's ship build) and
compiles in about 0.6–3.7 s, in the background.

## How it works (evani-render `material-split`, on with `material-recipes`)

- **The key.** A material's shader is keyed by (pass, sample count, recipe,
  character kind): the pass is opaque, see-through (blend), refracting glass
  or skinned; the sample count is 1x or 4x MSAA (glass is always 1x); the
  recipe is the block's recipe code, or 0 when the build has no stage case
  for it (no recipe, a character recipe, one the game doesn't list: all of
  them fall through to the lit stage's own values, so they share the
  no-recipe shader); the character kind is plain, skin, eye or hair.
- **The text.** The forward module through the character patch (only that
  kind's branch of `char_shade`), then the recipe patch with only that
  recipe in the stage and only that pass's see-through code, then the slot
  bodies and the engine's patches, then cut down to what the pipeline's
  entry points reach, without comments or indentation
  (evani-material-recipes `split::shake`). The condition
  layers stay the build's whole set behind the runtime mask, so turning a
  layer on never needs a new shader. This is the engine's own WGSL assembly
  (no new dependency); the idea of a bounded permutation key and building
  only the used code follows naga_oil and Bevy's shader cache (both
  MIT/Apache-2.0), with no code taken from either.
- **Built lazily, off the main thread.** A draw asks for its material's
  key; the build queue compiles it with `createRenderPipelineAsync` (up to
  four at once), and until it is in the material draws with the plain
  stand-in (an 11 KB `model_vs` + `model_fs` module) or, for a see-through
  one, opaque. A key no draw has asked for in half a second and not started
  is dropped, so the 1x keys stop once the 4x pass takes over. The 4x pass
  starts only once each finished 1x material has its 4x shader, so nothing
  steps back to the stand-in.
- **Live edits.** A slot edit (rim, fog, tone map, toon ramp...) rebuilds
  the material shaders drawn in the last half second that use the slot,
  together, in the background, from the variant cache when the body was seen
  before (160 pipelines kept); the others are dropped and rebuilt with the
  new body when next drawn. A Look Lab recipe edit rewrites the material's
  block: a new recipe code is a new key, built the same way.
- **The kit shadow pipeline** compiles off the main thread too, cut down the
  same way to what its `vs_main` and `fs_main` reach; until it is in, the
  shadow pass uses the plain mesh shadows (the previous kit pipeline after a
  code change). No pipeline on the web is created with the blocking call
  before its off-thread compile has finished.

## The budget (`cargo xtask check`, area `shaders`)

| Rule | Limit | Why |
|---|---|---|
| Each material shader | under **60 KB** (`split::MODULE_BUDGET`) | about what the browser parses, validates and compiles in well under a second off the main thread |
| Its recipe stage, a plain material | under **32 KB** (`STAGE_BUDGET` = 60 KB minus the 28 KB `TEMPLATE_ALLOWANCE`) | the check sizes each stage exactly as the game builds it; the largest today is 29.6 KB |
| Its stage, a character material (skin, eye, hair) | under **24 KB** (`CHAR_STAGE_BUDGET` = 60 KB minus the 36 KB `CHAR_ALLOWANCE`) | the kind's shading is in the rest of the module; its stage is the no-recipe one, 14.5 KB today |
| Distinct material shaders, a game | at most **12** (`GAME_PERMUTATIONS`) | each is compiled at start-up (once or twice, 1x and 4x); 12 keep a cold start within a few seconds |
| Distinct material shaders, a tool app (`kind = "tool"`) | at most **64** (`TOOL_PERMUTATIONS`) | the material viewer draws the whole catalogue (36 today) |

A permutation is one (recipe or none, character kind, pass); the check counts
the no-recipe shader, each listed stage recipe, each listed character recipe
(skin, hair, eye), and with `blend` the no-recipe and each see-through recipe
(glass, fire, glow, gem, organic) again, and with `refraction` glass once
more. A part (`wood-layouts`) and a condition layer add none.

The rest of a module (the forward module, lights, sky, and a character
kind's shading) is held under its allowance (28 KB plain, 36 KB character)
by evani-render's recipe shader test (`tests/recipe_wgsl.rs`,
`this_build_compiles_the_patched_stage`), which builds every real module of
the build it runs in, keeps 4 KB for the light and sky patches the web build
adds (measured 1.4–3.3 KB), checks each module under 60 KB, and validates
each with naga. Each module is cut to exactly the entry points its pipeline
compiles (evani-render `model::recipes::material_entries`, the list the GPU
side uses too): one vertex stage and the drawn stage, so with `skinning` the
skinned pass is its own module (`model_skin_vs`) and every other pass keeps
`model_vs` only. A plain material's module carries no character frame or
coverage mask (`split::char_only`: it reads neither).

Measured 2026-10-30 in the test with every app's options together (what
`cargo xtask check --quick` builds when the renderer changes: recipes,
character kinds, layers, skinning, point lights): the plain part 24.0 KB
(skinned 24.4 KB), a character part up to 31.8 KB (skinned 32.2 KB), the
largest module 54.3 KB, every one under its allowance with the 4 KB margin.
Before, the test cut every module to both vertex stages (1.1–1.5 KB the
engine never compiles) and plain modules kept about 0.75 KB of character
code they never run; with the Low cast shadows (+0.45 KB) that put plain
modules over the 28 KB allowance, in the viewer's own build too.

Measured in the material viewer (2026-10-29, ship build, headless Chrome on
an RTX 4060 Ti): the plain part 23.3 KB, a character part 28.0–30.1 KB, the
largest module 52.9 KB before the web patches; in the browser the material
shaders were 39.3–44.3 KB for the scenes measured. Every scene reaches its
finished look in 5.0–7.9 s from page start (it was 48–75 s with the
all-in-one shader) and in 0.7–1.3 s on a reload (48–75 s); no main-thread
task is over 0.29 s and the device is never lost. The pictures are the same
as before at Low and Ultra in every scene.

Both errors come with a `fix:` line: make the recipe's WGSL smaller (move
rarely used variants into an optional part, like `wood-layouts`; share
helpers through `rc_shared.wgsl`), or list fewer recipes or condition layers
in `[materials]`.
