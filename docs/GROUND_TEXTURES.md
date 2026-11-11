# Ground textures: texture layers for the kit material hooks

evani-render feature **`kit-textures`** (TopGun W3). Photo-detail texture
layers for procedural ground and terrain: the kit's own `kit_material` /
`kit_surface` WGSL samples them, so the procedural recipes stay in charge
of layout and colour and the textures add close-up detail.

Off by default. A game that doesn't turn it on compiles none of it: its
shaders, bind groups and download are byte for byte what they were.

## What the engine gives the kit code

Bound next to `kit_data` (group 1) in the forward pass **and** in the shadow,
AO-depth and near-cascade passes (one layout, so the same kit WGSL compiles
in all of them):

```wgsl
@group(1) @binding(16) var kit_tex0_ah:  texture_2d_array<f32>; // set 0 "ground": albedo (sRGB) + height (A)
@group(1) @binding(17) var kit_tex0_nra: texture_2d_array<f32>; // set 0: normal X, Y (OpenGL), roughness, AO
@group(1) @binding(18) var kit_tex1_ah:  texture_2d_array<f32>; // set 1 "hero"
@group(1) @binding(19) var kit_tex1_nra: texture_2d_array<f32>;
@group(1) @binding(20) var kit_macro:    texture_2d_array<f32>; // game-written RGBA8, engine mips
@group(1) @binding(21) var kit_wrap:  sampler;  // repeat, trilinear, anisotropy by tier
@group(1) @binding(22) var kit_clamp: sampler;  // clamp to edge, trilinear
@group(1) @binding(23) var<uniform> kit_tex_state: array<vec4<f32>, 8>;
fn kit_tex_ready(tex_set: u32, layer: u32) -> f32;  // 0 until both maps of the layer are on the GPU, then fades to 1
const KIT_SHADOW_PASS: bool;                    // true in the shadow / AO modules
```

* Up to **16 layers per set**; the array layer is the layer number, picked
  per pixel by the kit code (no atlas: hardware wrap, per-layer mips,
  correct anisotropy).
* Nothing registered: 1 x 1 x 1 zero dummies, `kit_tex_ready` 0.
* Use `textureSampleGrad` with gradients taken at the top of the function
  (uniform control flow): material branches are non-uniform.
* The layer's **mean colour** is its last mip:
  `textureSampleLevel(kit_tex0_ah, kit_clamp, vec2(0.5), layer, 16.0)`.
* Adds 5 sampled textures and 2 samplers to the fragment stage; the forward
  pass uses about 3 of each before, well under WebGPU's default 16.

## Declaring layers

Either a `kit-textures.json` in the game's web root (`<app>/assets`, read
once at start; `cargo xtask check` reads the same file), or code. Code wins
for a set it has set.

```json
{
  "formats": ["astc", "bc7", "rgba"],
  "anisotropy": { "medium": 4, "high": 8, "ultra": 16 },
  "budget_mb": { "high": 25, "ultra": 60 },
  "boot_budget_mb": 4,
  "macro": { "low": 1024, "medium": 1024, "high": 2048, "ultra": 2048 },
  "fade_s": 0.5,
  "sets": [
    { "set": 0, "size": { "lite": 0, "low": 0, "medium": 512, "high": 1024, "ultra": 2048, "vr": 1024 },
      "layers": [ { "name": "grass", "path": "ground/grass" } ] },
    { "set": 1, "boot": 128, "size": { "lite": 0, "low": 0, "medium": 1024, "high": 2048, "ultra": 2048, "vr": 2048 },
      "layers": [ { "name": "sandstone", "path": "hero/sandstone" } ] }
  ]
}
```

```rust
use evani_render::kit_textures::{KitTextures, MacroDesc, TextureSetDesc};

let mut kt = world.res_mut::<KitTextures>();
kt.set(1, TextureSetDesc::default().tier("high", 2048).tier("medium", 1024).layer("sandstone", "hero/sandstone"))?;
kt.set_anisotropy("high", 8);
let m = kt.macro_array(MacroDesc { size: 2048, layers: 2 })?;
m.write(0, &rgba)?;                    // whole layer, size * size * 4 bytes
m.write_rect(1, x, y, w, h, &rgba)?;   // part of a layer
kt.layer_ready(1, 0);                  // both maps uploaded?
```

Files, per layer, per tier size, per shipped format:
`<path>_ah.<size>.<fmt>.ktx2` and `<path>_nra.<size>.<fmt>.ktx2`.
`astc` (ASTC 4x4) and `bc7` hold `size` x `size`; `rgba` (RGBA8, for devices
without block compression) holds half that. Plain KTX2: no
supercompression (the server compresses on the wire), full mip chain. The
device takes ASTC, else BC7, else RGBA. A tier at size 0 (or not listed)
loads nothing for that set. Nothing is fetched before the tier settles
after warm-up, so a tier change at start costs no download.

## Streaming (W3c)

A set with a `"boot"` size (code: `.boot(128)`) loads in two waves, so a
mission starts on a small download and sharpens while it plays:

1. **Boot wave**: every wanted layer's two maps at the boot size. The set
   binds these arrays at once; each layer fades in (`kit_tex_ready` 0 to 1
   over `fade_s`, default 0.5 s of render time) as its pair lands.
2. **Tier wave**: once every wanted layer has its boot pair, the tier-size
   maps load into a second pair of arrays. When every wanted layer has them,
   the set swaps to them **in one go** (no layer ever drops back to blurry or
   to nothing) and the boot arrays are freed. `kit_tex_ready` stays at 1.

A set without a boot size (or with one not below the tier's size) loads in
one wave at the tier's size, each layer fading in as it lands.

```rust
kt.want(0, &[0, 3, 6]);     // this mission's ground layers only; the rest aren't fetched
kt.want_all(0);             // back to every layer (the default)
kt.hold_full(true);         // boot sizes only (e.g. until take-off); false lets the tier wave go
kt.boot_done();             // the tier settled and every wanted layer shows (title screen / start waits on it)
kt.stage(1, 0);             // LayerStage::{Off, Waiting, Loading, Coarse, Full, Failed}, for status lines
kt.loaded_bytes();          // layer bytes uploaded so far
kt.set_fade(0.3);           // fade-in seconds (0 = at once)
kt.macro_size_for("high");  // the manifest's "macro" size for a tier, for MacroDesc
```

* A layer wanted after the swap loads its tier-size pair only. A layer
  dropped from `want` is not fetched; one already shown keeps showing until
  the set swaps.
* A file that fails (404, a bad file) is logged once; its layer stays at
  ready 0 and the rest of the set carries on without it.
* `"macro": {"low": 1024, "high": 2048}` in the manifest gives the macro size
  per tier; the game reads it with `macro_size_for` when it makes the macro
  (the game writes the macro, so the engine never resizes it under it).
* **Tier sizes live in the game's manifest, not the engine's tier files.**
  The engine's `assets/tiers/*.json` are shared by every game and compiled
  into each download; a game's texture sizes there would change every
  game's bytes. The manifest's per-tier `size`, `anisotropy` and `macro` are
  the game's tier table for its textures.
* The browser's HTTP cache keeps the files between launches (serve them with
  a long `cache-control`, as for models); there is no separate cache.

## The macro texture

Game-written RGBA8, 1 to 4 layers, power-of-two size up to 4096, full mip
chain made by the engine. **`write_rect` is safe every frame**: only the
rect is uploaded, a rect rewritten before the next frame replaces the older
write (they never pile up), and the mips are rebuilt on the GPU only over
the rect's parent texels (one small scissored pass per level, all in one
submission before the frame). A 256 x 256 rect each frame is 256 KB of
upload and about 11 tiny passes touching ~22 k texels in all. After a GPU
device loss the macro comes back empty: write it again.

## Making layers: `cargo xtask convert-textures`

```
cargo xtask convert-textures layers.json --out <app>/assets
cargo xtask convert-textures --app-dir <game>/apps/<id>      # a game outside the engine (W3d)
cargo xtask convert-textures --app-dir <app> --force         # redo every layer
```

`--app-dir` reads `<app>/layers.json` (else the game folder's, two up, as
`examples/ground-textures` keeps it) and writes `<app>/assets`. A layer
whose inputs (file sizes and times) and settings match its last run is
skipped ("up to date"), so adding one layer to sixteen converts one.

`layers.json` is `kit-textures.json` plus a `source` per layer:

* `{ "folder": "downloads/cliff_side" }`: a downloaded map folder; the maps
  are found by their Poly Haven names (`_diff`, `_nor_gl`, `_nor_dx`,
  `_rough`, `_ao`, `_disp`, `_arm`) or ambientCG names (`_Color`,
  `_NormalGL`, `_NormalDX`, `_Roughness`, `_AmbientOcclusion`,
  `_Displacement`); the largest file wins when a map comes at several
  resolutions; an OpenGL normal wins over a DirectX one, which is flipped;
  an `_arm` pack (AO, roughness, metal) fills AO and roughness when their own
  maps are missing. PNG, JPEG, WebP or TIFF (not EXR).
* `{ "maps": { "albedo": "x_diff.png", "normal": "x_nor_gl.png", "roughness": "x_rough.png", "ao": "x_ao.png", "height": "x_disp.png" } }`
  (also `normal_dx`, `arm`): the files named one by one.
* `{ "procedural": "rock" | "field" | "paving", "seed": 7 }`: the engine's
  own original tiling recipes.

Photo maps decode with sharp from the pinned npm tree. Missing maps get
flat defaults (flat normal, roughness 0.8, AO 1, height 0.5). It writes
every tier size and the boot size in every format, and `kit-textures.json`
with each layer's mean colour (other layer fields, such as `licence` or
`tile_m`, pass through).

The mips are ours, not the encoder's: a Kaiser-windowed sinc 2x filter with
wrap addressing (textures tile), albedo filtered in linear light, normals
filtered as vectors and renormalised, and the length lost to averaging
folded into roughness (Toksvig), so bumpy rock and concrete don't sparkle at
grazing angles. ASTC and BC7 blocks come from the pinned `ktx` tool fed our
levels (`--levels N`, no resampling).

## The check

`cargo xtask check` (with `--app-dir` for games outside the engine) reads
`kit-textures.json` and errors on:

* a set that doesn't list every tier (`assets/tiers/*.json`);
* a layer file missing for a size some tier asks for, in any shipped format;
* a file the engine would refuse (not plain KTX2, supercompressed, a format
  other than its name says, the wrong size, no full mip chain);
* a tier whose download (one device's format, every set, boot files
  included) is over its `budget_mb` (defaults: lite and low 4, medium 12,
  high 25, ultra 60 MB);
* a tier whose **start** (every set's boot files, or its tier-size files
  for a set without a boot size) is over `boot_budget_mb` (default 4 MB);
* a boot size whose files are missing, or that isn't a power of two;
* a tier whose kit textures need more than half its `max_texture_memory`
  in GPU memory (both maps of every layer, plus the boot arrays while they
  stream);
* kit WGSL over the shader cap: the kit's `.wgsl` files plus the engine's
  forward module (24 KB allowance) over **60 KB**;
* the manifest without the feature.

At run time a forward module over 60 KB is reported in the ErrorLog too.

## Projection and anti-repeat (the proof kit's way)

`examples/ground-textures` (a game outside the engine's workspace, like
TopGun Tom) is the reference: `kits/evani-kit-ground-textures/shaders/ground.wgsl`.

* **Flat ground: world XZ**, one projection.
* **Steep faces: biplanar** (idea: Inigo Quilez): the two world planes that
  best face the normal, sharpened weights, explicit gradients per plane.
* **Hex-tiling** (idea: Mikkelsen 2022) on the near band only: three
  rotated, offset samples per hex cell; never stacked on biplanar.
* **Far**: detail fades by pixel footprint to the layer's mean; past the
  fade there are no detail fetches.
* Colour stays with the procedural recipe: the texture applies as a ratio
  to its mean, so near and far agree.

Its tunables are parameters (`kit.ground_textures.fade_start`, `fade_end`,
`hex`, `hex_band`, `biplanar_sharpness`, `normal_strength`, `tile_*`), so the
live tools can sweep them.

## Not yet (later)

* An AO output from `kit_surface` that darkens ambient only (multiply AO
  into the colour meanwhile).
* RGBA16F macro.
* BC5 normals for layers whose packed BC7 normals look blocky.
* Fetching only a file's missing top levels on the swap (HTTP ranges):
  today the tier wave downloads whole files, the boot wave's levels again
  included (a 64 px boot under a 1024 px tier is 0.4 % more).
