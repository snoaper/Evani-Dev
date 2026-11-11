# Game passes

A kit (or look) adds its own GPU passes to the engine's frame: terrain
shading, clouds, heat haze, HUD overlays, afterburner glow, LUTs, noise
volumes. It writes only the WGSL body; the engine adds the bindings, the
frame uniform, the entry point, the ordering, the tier rule and the live
parameters. evani-render feature `game-passes` (CORE_REQUESTS
`game-gpu-passes`, asked for by TopGun Tom). Games without the feature
download none of it, byte for byte.

## Turn it on

```toml
# kits/evani-kit-<id>/Cargo.toml
[dependencies]
evani-render = { workspace = true, features = ["forward", "game-passes"] }

[dev-dependencies]
# game_pass::check compiles every pass's WGSL in the kit's tests (never shipped)
evani-render = { workspace = true, features = ["forward", "game-passes-check"] }
```

The app must add the render module before the kit (every app does).

## A pass in a few lines

```rust
use evani_render::game_pass::{self, Format, GamePass, GameTarget, PassBlend, PassParam, Stage};

const HAZE: &str = "
fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(px), 0);              // reversed Z: 0 is the sky
    let wobble = sin(uv.y * 80.0 + frame.camera.w * 6.0) * param_strength() * frame.size.z;
    let c = textureSample(scene, samp, uv + vec2<f32>(wobble * step(0.0001, d), 0.0));
    return vec4<f32>(c.rgb, 1.0);
}";

fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
    game_pass::add(kit, GamePass::fullscreen("topgun.haze", HAZE)
        .at(Stage::BeforeTonemap)          // on `hdr`, before the tone map
        .reads("scene", "hdr")             // reading its own output gives a copy
        .reads_depth("depth")
        .min_tier("medium")                // off on Lite and Low
        .param(PassParam::f32("strength", 2.0, 0.0, 8.0).describe("Shimmer, in pixels")))?;
    Ok(())
}
```

`kit.topgun.haze.strength` and `kit.topgun.haze.on` are now live parameters
(`schema_get`, `param_set`, `sweep_run`, bakes); the WGSL reads the first as
`param_strength()`.

The working example is `kits/evani-kit-example/src/game_pass_demo.rs`: a
compute pass that fills a noise volume once, a half-resolution afterglow
with a history copy, a heat haze that reads four inputs, and a HUD ring.
The example kit's own tests build and compile these passes; a browser proof on the sandbox scene (a `game-pass-proof` feature) follows later.

A ground pass in a real scene: `examples/proc-field/kits/evani-kit-proc-field/src/facet.rs`
(`FLOOR_WGSL`, the faceted ball's scene) finds the ground from the depth
(`world_at`), varies its light by world-space value noise (mottling and fine
grain, each faded where it would fall under about two pixels, so no glitter)
and darkens both surfaces around a contact point, for about 0.1 ms at
1600x900. Any game can copy it as it is: it is a plain `GamePass::fullscreen`
at `Stage::BeforeTonemap` with live `PassParam`s, switched off through its
`on` parameter where a scene does not want it.

## What a pass declares

| Builder | Meaning |
|---|---|
| `GamePass::fullscreen(name, wgsl)` | One full-screen triangle; the kit writes `fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32>` (uv 0,0 top-left; px the pixel centre). |
| `GamePass::compute(name, wgsl)` | One invocation per texel of its output game target; the kit writes `fn pass_main(id: vec3<u32>) -> vec4<f32>` (the value stored at `id`). Default stage `Start`. |
| `.at(Stage::…)` | Place in the frame: `Start` (-100, before the scene), `AfterOpaque` (6, `hdr` after 3D, before sprites), `BeforeTonemap` (90, `hdr`), `AfterTonemap` (160, `ldr`), `Overlay` (950, `ldr` over the HUD). Inputs and outputs still order it. |
| `.reads(alias, target)` | Bind a graph target as `alias`: `hdr`, `ldr`, an effect's target, or a game target (`texture_2d<f32>`, or `texture_3d<f32>` for a volume). Its writers run first. |
| `.reads_depth(alias)` | The scene depth as `texture_depth_2d` (read with `textureLoad`). Needs `forward`. |
| `.reads_history(alias, target)` | Last frame's contents of a history game target (temporal passes). |
| `.writes(target)` | Its one output: `hdr`, `ldr` or a 2D game target (full-screen), a game target of any size (compute). Default: the stage's target. |
| `.blend(PassBlend::…)` | `Replace` (default), `Additive`, `Multiply`, `Alpha` (full-screen passes). |
| `.once()` | Run once, then again only when its parameters change, its output is re-created, or `GamePasses::rerun(name)`. |
| `.param(PassParam::f32 / vec3 / color)` | A live parameter `kit.<id>.<pass>.<name>`, read as `param_<name>()`. At most 64 floats per pass. |
| `.min_tier("medium")` | Off on lower tiers (catalog order). A pass reading a game target whose only writer is off goes off too. |
| `.order(n)` | Overrides the stage's order among passes the inputs leave unordered. |

Game targets (`game_pass::add_target`, before the passes that use them):

| Builder | Meaning |
|---|---|
| `GameTarget::screen(name, format, scale)` | The render size times `scale`; `.tier_scale("low", 0.25)` per tier. |
| `GameTarget::fixed(name, format, [w, h])` | A fixed 2D texture (LUTs), up to 4096. |
| `GameTarget::volume(name, format, [w, h, d])` | A 3D texture (noise, weather), up to 256 per edge; written by compute passes. |
| `.history()` | Two textures swapped every frame: writers write this frame's, `reads_history` reads last frame's. |

Formats: `Rgba8Unorm`, `Rgba16Float` (filterable), `R32Float`,
`Rgba32Float` (read with `textureLoad`). All can be drawn into, sampled and
written by compute passes in WebGPU without optional features.

Names are `<kit>.<name>` in lowercase (`topgun.clouds`), so no two kits or
the engine clash.

## What every pass module has

```wgsl
struct EngineFrame {
    inv_view_proj: mat4x4<f32>,  view_proj: mat4x4<f32>,
    camera: vec4<f32>,      // xyz eye (world), w seconds since start
    sun: vec4<f32>,         // xyz render.light.direction (normalised), w intensity
    sun_color: vec4<f32>,   // rgb render.light.color, w active tier index
    size: vec4<f32>,        // this pass's output: w, h, 1/w, 1/h
    misc: vec4<f32>,        // frame number, frame time (s), history parity, output depth
    ambient: vec4<f32>,     // render.ambient
    background: vec4<f32>,  // render.clear_color
};
var<uniform> frame: EngineFrame;
var samp: sampler;                         // linear, clamp to edge
var samp_wrap: sampler;                    // linear, repeat
var<storage, read> data: array<vec4<f32>>; // GamePasses::data_mut(name)
fn ndc_of(uv) -> vec2<f32>;  fn world_at(uv, depth) -> vec3<f32>;  fn view_ray(uv) -> vec3<f32>;
```

These names, `out_tex`, `pass_main` and the entry points are reserved.
Bulk per-frame values (afterburner positions, HUD symbols, cloud layers) go
in `data`: `world.res_mut::<GamePasses>().data_mut("topgun.hud")`.

The camera is the main camera (`RenderWorld::camera`); with split-screen
views a pass sees the main camera only.

## Errors become tools

* `game_pass::add` / `add_target` refuse a bad declaration at build with
  what is wrong and a `fix:` line (unknown or misspelt target, a name
  outside the kit's scope, no `pass_main`, more than 4 inputs, depth without
  the 3D path, history of a target that keeps none, a compute pass writing
  `hdr`, a reserved WGSL name, an unknown tier, more than 64 parameter
  floats...). The kit's build fails and the overlay shows it.
* `game_pass::check(&app.world)` (feature `game-passes-check`, kit tests)
  compiles the render graph and validates every pass's whole WGSL module
  with naga, naming the pass and the line.
* `cargo xtask check` reports an ERROR for a kit that uses `game_pass::`
  without such a test, or without `game-passes-check` in its
  dev-dependencies.
* In the browser, a pass whose pipeline still fails is reported once in the
  ErrorLog and skipped; the rest of the frame draws.

## Not in this first slice

* Passes that draw geometry (a terrain vertex shader with depth testing):
  today terrain stays CPU meshes through `MeshLibrary`.
* Engine shaders reading a game target (the forward pass reading a cloud
  shadow map, an `aerial` slot): the `atmosphere` and `volumetric-clouds`
  rows build on this.
* Uploading texture data from the CPU into a game target (heightmaps):
  generate it in a compute pass, or pass values through `data`.
* More than one output per pass (MRT), and hot-swapping a pass's WGSL live
  (its parameters are live; its code changes with `cargo xtask dev --hot`).
