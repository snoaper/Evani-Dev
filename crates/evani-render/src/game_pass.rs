//! Game passes (feature `game-passes`; CORE_REQUESTS `game-gpu-passes`,
//! asked for by TopGun Tom): a kit or look adds its own GPU passes to the
//! engine's frame, with its own WGSL, without forking the renderer.
//!
//! * A pass is a full-screen draw ([`GamePass::fullscreen`]) or a compute
//!   dispatch ([`GamePass::compute`]). It names its inputs (engine targets
//!   such as `hdr`, `ldr` and `depth`, the kit's own targets, or last
//!   frame's copy of a history target), its one output, its place in the
//!   frame ([`Stage`]) and its tier behaviour ([`GamePass::min_tier`],
//!   [`GameTarget::tier_scale`]).
//! * The kit's own textures are [`GameTarget`]s: screen-sized (scaled),
//!   fixed 2D (LUTs) or 3D volumes (noise), any of them with a history copy
//!   (last frame's contents, for temporal passes).
//! * Every tunable is a registered parameter, `kit.<id>.<pass>.<name>` (plus
//!   `kit.<id>.<pass>.on`), so the live tools see and change it; the WGSL
//!   reads it as `param_<name>()`.
//! * The kit writes only the body: `fn pass_main(uv: vec2<f32>, px:
//!   vec2<f32>) -> vec4<f32>` (full-screen) or `fn pass_main(id: vec3<u32>)
//!   -> vec4<f32>` (compute, one texel of the output). The engine adds the
//!   bindings, the frame uniform, the vertex stage or the compute entry
//!   point, and the helpers ([`ENGINE_WGSL`]).
//! * [`add`] checks a pass when it is registered and refuses a bad one with
//!   a `fix:` line ([`GamePassError`]); `check` (feature
//!   `game-passes-check`, kit tests) also compiles every pass's WGSL.
//!
//! Without the feature none of this is compiled: games that don't use it
//! download nothing of it.

use evani_core::ecs::World;
use evani_core::params::{BoolParam, F32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};

use crate::graph::{PassDesc, RenderGraph, TargetDesc, TargetKind};

/// Most inputs one pass reads.
pub const MAX_INPUTS: usize = 4;
/// Most parameter floats one pass has (16 `vec4`s of uniform).
pub const MAX_PARAM_FLOATS: usize = 64;
/// Smallest `data` buffer, in `vec4` records.
pub const MIN_DATA: usize = 16;
/// Most `vec4` data records uploaded per pass (1 MB); the rest are ignored.
pub const MAX_DATA: usize = 1 << 16;
/// Largest edge of a fixed 2D game target.
pub const MAX_FIXED: u32 = 4096;
/// Largest edge of a volume game target.
pub const MAX_VOLUME: u32 = 256;
/// Words of the engine frame uniform ([`ENGINE_WGSL`] `EngineFrame`).
pub const ENGINE_WORDS: usize = 60;

/// Bindings of group 0 (fixed so the WGSL and the GPU layout agree).
pub mod binding {
    pub const FRAME: u32 = 0;
    pub const PARAMS: u32 = 1;
    pub const SAMP: u32 = 2;
    pub const SAMP_WRAP: u32 = 3;
    pub const DATA: u32 = 4;
    /// The first input; input `i` is at `INPUT0 + i`.
    pub const INPUT0: u32 = 5;
    /// The compute output (a storage texture).
    pub const OUT: u32 = 9;
}

/// Names the engine declares in every pass module: a parameter, input
/// alias or the kit's own code must not reuse them.
pub const RESERVED: &[&str] = &[
    "frame", "params", "samp", "samp_wrap", "data", "out_tex", "pass_main", "vs_main", "fs_main", "cs_main",
    "world_at", "view_ray", "ndc_of", "EngineFrame", "GameParams", "GameVsOut",
];

/// The frame uniform, the helpers and their meaning (every pass gets it).
pub const ENGINE_WGSL: &str = "// evani.shader.game_pass
struct EngineFrame {
    inv_view_proj: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    // xyz camera position (world), w seconds since start
    camera: vec4<f32>,
    // xyz render.light.direction (normalised), w render.light.intensity
    sun: vec4<f32>,
    // rgb render.light.color, w active tier index
    sun_color: vec4<f32>,
    // this pass's output: width, height, 1/width, 1/height
    size: vec4<f32>,
    // x frame number, y frame time (s), z history parity (0/1), w output depth (volumes, else 1)
    misc: vec4<f32>,
    // xyz render.ambient, w unused
    ambient: vec4<f32>,
    // rgb render.clear_color (the background), w unused
    background: vec4<f32>,
};
struct GameParams { v: array<vec4<f32>, 16> };
@group(0) @binding(0) var<uniform> frame: EngineFrame;
@group(0) @binding(1) var<uniform> params: GameParams;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var samp_wrap: sampler;
@group(0) @binding(4) var<storage, read> data: array<vec4<f32>>;
// Clip-space xy of a uv (0,0 top-left, 1,1 bottom-right).
fn ndc_of(uv: vec2<f32>) -> vec2<f32> { return vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0); }
// World position of a uv at a depth-buffer value (reversed Z: 1 near, 0 far; 0 is the sky).
fn world_at(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let h = frame.inv_view_proj * vec4<f32>(ndc_of(uv), depth, 1.0);
    return h.xyz / h.w;
}
// Unit direction from the camera through a uv.
fn view_ray(uv: vec2<f32>) -> vec3<f32> { return normalize(world_at(uv, 0.5) - frame.camera.xyz); }
";

const FULLSCREEN_WGSL: &str = "struct GameVsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> GameVsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: GameVsOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn fs_main(in: GameVsOut) -> @location(0) vec4<f32> { return pass_main(in.uv, in.pos.xy); }
";

/// Texel format of a game target. All four can be drawn into, sampled and
/// written by compute passes in WebGPU without optional features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rgba8Unorm,
    Rgba16Float,
    /// Not filterable: read it with `textureLoad`, not `textureSample`.
    R32Float,
    /// Not filterable: read it with `textureLoad`, not `textureSample`.
    Rgba32Float,
}

impl Format {
    /// The WGSL storage-texture format name.
    pub fn wgsl(self) -> &'static str {
        match self {
            Format::Rgba8Unorm => "rgba8unorm",
            Format::Rgba16Float => "rgba16float",
            Format::R32Float => "r32float",
            Format::Rgba32Float => "rgba32float",
        }
    }

    /// Can a filtering sampler read it (`textureSample`)?
    pub fn filterable(self) -> bool {
        matches!(self, Format::Rgba8Unorm | Format::Rgba16Float)
    }

    /// Bytes per texel.
    pub fn bytes(self) -> u32 {
        match self {
            Format::Rgba8Unorm | Format::R32Float => 4,
            Format::Rgba16Float => 8,
            Format::Rgba32Float => 16,
        }
    }
}

/// Size of a game target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// The render size times a scale (0.5: half resolution).
    Screen(f32),
    /// A fixed 2D size (a LUT).
    Fixed([u32; 2]),
    /// A fixed 3D size (a noise volume). Compute passes write it; full-screen
    /// passes read it.
    Volume([u32; 3]),
}

/// A texture a game pass owns.
#[derive(Debug, Clone, PartialEq)]
pub struct GameTarget {
    /// `<kit>.<name>`, e.g. `topgun.clouds`.
    pub name: &'static str,
    pub format: Format,
    pub size: Size,
    /// Keep last frame's contents readable ([`GamePass::reads_history`]).
    pub history: bool,
    /// Scale overrides of a screen-sized target per tier id.
    pub tier_scale: Vec<(&'static str, f32)>,
}

impl GameTarget {
    /// A screen-sized target at `scale` of the render size.
    pub fn screen(name: &'static str, format: Format, scale: f32) -> Self {
        Self { name, format, size: Size::Screen(scale), history: false, tier_scale: Vec::new() }
    }

    /// A fixed 2D target (`[width, height]`).
    pub fn fixed(name: &'static str, format: Format, size: [u32; 2]) -> Self {
        Self { name, format, size: Size::Fixed(size), history: false, tier_scale: Vec::new() }
    }

    /// A 3D target (`[width, height, depth]`).
    pub fn volume(name: &'static str, format: Format, size: [u32; 3]) -> Self {
        Self { name, format, size: Size::Volume(size), history: false, tier_scale: Vec::new() }
    }

    /// Keep last frame's contents (two textures, swapped every frame).
    pub fn history(mut self) -> Self {
        self.history = true;
        self
    }

    /// Use `scale` instead of the default on tier `tier` (screen targets).
    pub fn tier_scale(mut self, tier: &'static str, scale: f32) -> Self {
        self.tier_scale.push((tier, scale));
        self
    }

    pub fn is_volume(&self) -> bool {
        matches!(self.size, Size::Volume(_))
    }

    /// Texel size on tier `tier` at render size `screen`: `[w, h, depth]`.
    pub fn extent(&self, screen: [u32; 2], tier: &str) -> [u32; 3] {
        match self.size {
            Size::Screen(s) => {
                let s = self.tier_scale.iter().find(|(t, _)| *t == tier).map_or(s, |&(_, v)| v);
                let px = |n: u32| ((n as f32 * s).round() as u32).max(1);
                [px(screen[0]), px(screen[1]), 1]
            }
            Size::Fixed([w, h]) => [w.max(1), h.max(1), 1],
            Size::Volume([w, h, d]) => [w.max(1), h.max(1), d.max(1)],
        }
    }
}

/// Where a pass sits in the frame. A full-screen pass at an engine stage
/// draws into that stage's target unless [`GamePass::writes`] says
/// otherwise; dependencies on inputs and outputs still order it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stage {
    /// Before the scene: LUTs, noise, simulations (order -100).
    Start,
    /// On `hdr` after opaque 3D, before sprites (order 6).
    AfterOpaque,
    /// On `hdr` after sprites and bloom, before the tone map (order 90).
    #[default]
    BeforeTonemap,
    /// On `ldr` after the tone map, with the colour effects (order 160).
    AfterTonemap,
    /// On `ldr` over everything, the HUD included (order 950): HUD overlays.
    Overlay,
}

impl Stage {
    pub fn order(self) -> i32 {
        match self {
            Stage::Start => -100,
            Stage::AfterOpaque => 6,
            Stage::BeforeTonemap => 90,
            Stage::AfterTonemap => 160,
            Stage::Overlay => 950,
        }
    }

    /// The engine target a full-screen pass at this stage draws into.
    pub fn target(self) -> Option<&'static str> {
        match self {
            Stage::Start => None,
            Stage::AfterOpaque | Stage::BeforeTonemap => Some("hdr"),
            Stage::AfterTonemap | Stage::Overlay => Some("ldr"),
        }
    }
}

/// What an input reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A graph target this frame (`hdr`, `ldr`, a game target...). Reading
    /// the pass's own output gives a copy taken just before the draw.
    Target(&'static str),
    /// The scene depth (`texture_depth_2d`; needs the 3D path).
    Depth,
    /// Last frame's contents of a history game target.
    History(&'static str),
}

/// One input: the WGSL name it is bound as, and what it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    pub alias: &'static str,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fullscreen,
    Compute,
}

/// When a pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Run {
    #[default]
    EveryFrame,
    /// Once, then again only when its parameters change, its output is
    /// re-created (resize, tier) or [`GamePasses::rerun`] asks: LUTs, noise.
    Once,
}

/// How a full-screen pass's colour lands on its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassBlend {
    #[default]
    Replace,
    /// `dst + src`.
    Additive,
    /// `dst * src`.
    Multiply,
    /// `src * a + dst * (1 - a)`.
    Alpha,
}

/// A parameter's type, default and range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamKind {
    F32 { default: f32, min: f32, max: f32 },
    Vec3 { default: [f32; 3] },
    Color { default: [f32; 3] },
}

/// One tunable of a pass: `kit.<id>.<pass>.<name>`, `param_<name>()` in WGSL.
#[derive(Debug, Clone, PartialEq)]
pub struct PassParam {
    pub name: &'static str,
    pub kind: ParamKind,
    pub description: &'static str,
}

impl PassParam {
    pub fn f32(name: &'static str, default: f32, min: f32, max: f32) -> Self {
        Self { name, kind: ParamKind::F32 { default, min, max }, description: "" }
    }

    pub fn vec3(name: &'static str, default: [f32; 3]) -> Self {
        Self { name, kind: ParamKind::Vec3 { default }, description: "" }
    }

    /// A linear-RGB colour (0..1).
    pub fn color(name: &'static str, default: [f32; 3]) -> Self {
        Self { name, kind: ParamKind::Color { default }, description: "" }
    }

    pub fn describe(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    fn floats(&self) -> usize {
        match self.kind {
            ParamKind::F32 { .. } => 1,
            ParamKind::Vec3 { .. } | ParamKind::Color { .. } => 3,
        }
    }
}

/// A game pass as the kit describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct GamePass {
    /// `<kit>.<name>`, e.g. `topgun.haze`. Also the graph pass name.
    pub name: &'static str,
    pub kind: Kind,
    /// The kit's WGSL: `pass_main` and any helpers it calls.
    pub wgsl: String,
    pub stage: Stage,
    pub inputs: Vec<Input>,
    /// `None`: the stage's engine target (full-screen passes).
    pub output: Option<&'static str>,
    pub blend: PassBlend,
    pub run: Run,
    pub params: Vec<PassParam>,
    /// Off on tiers below this one (catalog order: lite, low, medium...).
    pub min_tier: Option<&'static str>,
    /// Overrides the stage's order among passes the inputs leave unordered.
    pub order: Option<i32>,
}

impl GamePass {
    fn new(name: &'static str, kind: Kind, wgsl: impl Into<String>) -> Self {
        Self {
            name,
            kind,
            wgsl: wgsl.into(),
            stage: Stage::default(),
            inputs: Vec::new(),
            output: None,
            blend: PassBlend::Replace,
            run: Run::EveryFrame,
            params: Vec::new(),
            min_tier: None,
            order: None,
        }
    }

    /// A full-screen draw: `fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32>`.
    pub fn fullscreen(name: &'static str, wgsl: impl Into<String>) -> Self {
        Self::new(name, Kind::Fullscreen, wgsl)
    }

    /// A compute dispatch over every texel of its output (a game target):
    /// `fn pass_main(id: vec3<u32>) -> vec4<f32>`. Defaults to [`Stage::Start`].
    pub fn compute(name: &'static str, wgsl: impl Into<String>) -> Self {
        Self { stage: Stage::Start, ..Self::new(name, Kind::Compute, wgsl) }
    }

    pub fn at(mut self, stage: Stage) -> Self {
        self.stage = stage;
        self
    }

    /// Read graph target `target` as `alias` (`texture_2d<f32>`, or
    /// `texture_3d<f32>` for a volume).
    pub fn reads(mut self, alias: &'static str, target: &'static str) -> Self {
        self.inputs.push(Input { alias, source: Source::Target(target) });
        self
    }

    /// Read the scene depth as `alias` (`texture_depth_2d`, `textureLoad`).
    pub fn reads_depth(mut self, alias: &'static str) -> Self {
        self.inputs.push(Input { alias, source: Source::Depth });
        self
    }

    /// Read last frame's contents of history target `target` as `alias`.
    pub fn reads_history(mut self, alias: &'static str, target: &'static str) -> Self {
        self.inputs.push(Input { alias, source: Source::History(target) });
        self
    }

    pub fn writes(mut self, target: &'static str) -> Self {
        self.output = Some(target);
        self
    }

    pub fn blend(mut self, blend: PassBlend) -> Self {
        self.blend = blend;
        self
    }

    /// Run once (and again on change): see [`Run::Once`].
    pub fn once(mut self) -> Self {
        self.run = Run::Once;
        self
    }

    pub fn param(mut self, p: PassParam) -> Self {
        self.params.push(p);
        self
    }

    pub fn min_tier(mut self, tier: &'static str) -> Self {
        self.min_tier = Some(tier);
        self
    }

    pub fn order(mut self, order: i32) -> Self {
        self.order = Some(order);
        self
    }

    /// The target it draws into or writes.
    pub fn output_target(&self) -> Option<&'static str> {
        self.output.or(match self.kind {
            Kind::Fullscreen => self.stage.target(),
            Kind::Compute => None,
        })
    }
}

/// How an input is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// `texture_2d<f32>`, filterable.
    Color,
    /// `texture_2d<f32>`, not filterable (`textureLoad` only).
    ColorUnfilterable,
    /// `texture_depth_2d`.
    Depth,
    /// `texture_3d<f32>`.
    Volume { filterable: bool },
}

impl InputKind {
    fn wgsl(self) -> &'static str {
        match self {
            InputKind::Color | InputKind::ColorUnfilterable => "texture_2d<f32>",
            InputKind::Depth => "texture_depth_2d",
            InputKind::Volume { .. } => "texture_3d<f32>",
        }
    }
}

/// Where the output goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// An engine colour target (`hdr`, `ldr`, an effect's target).
    Engine(TargetKind),
    /// A game target of this format; `volume` for 3D.
    Game { format: Format, volume: bool },
}

/// A checked pass: its bindings resolved and its parameters registered.
#[derive(Debug, Clone)]
pub struct Registered {
    pub pass: GamePass,
    pub inputs: Vec<InputKind>,
    pub output: (&'static str, OutputKind),
    /// Inputs that read the pass's own output (bound to a copy).
    pub copies_output: bool,
    /// Word offset of each parameter in the params uniform.
    pub offsets: Vec<usize>,
    pub on: Option<BoolParam>,
    pub handles: Vec<ParamHandle>,
}

/// A registered parameter.
#[derive(Debug, Clone, Copy)]
pub enum ParamHandle {
    F32(F32Param),
    Vec3(Vec3Param),
}

impl Registered {
    /// Fill the params uniform from the current values; returns the words.
    pub fn param_words(&self, reg: &ParamRegistry, out: &mut [f32; MAX_PARAM_FLOATS]) -> usize {
        let mut end = 0;
        for (&o, h) in self.offsets.iter().zip(&self.handles) {
            let vals: [f32; 3];
            let v: &[f32] = match *h {
                ParamHandle::F32(p) => {
                    vals = [reg.f32(p), 0.0, 0.0];
                    &vals[..1]
                }
                ParamHandle::Vec3(p) => {
                    vals = reg.vec3(p);
                    &vals
                }
            };
            if let Some(dst) = out.get_mut(o..o + v.len()) {
                dst.copy_from_slice(v);
                end = end.max(o + v.len());
            }
        }
        end
    }

    /// The whole WGSL module of this pass.
    pub fn wgsl(&self) -> String {
        compose_wgsl(self)
    }
}

/// The game passes and targets of this build (a world resource the render
/// module inserts with the feature).
#[derive(Debug, Clone, Default)]
pub struct GamePasses {
    targets: Vec<GameTarget>,
    passes: Vec<Registered>,
    data: Vec<(u64, Vec<[f32; 4]>)>,
    reruns: Vec<u64>,
}

impl GamePasses {
    pub fn targets(&self) -> &[GameTarget] {
        &self.targets
    }

    pub fn target(&self, name: &str) -> Option<&GameTarget> {
        self.targets.iter().find(|t| t.name == name)
    }

    pub fn passes(&self) -> &[Registered] {
        &self.passes
    }

    pub fn pass(&self, name: &str) -> Option<&Registered> {
        self.passes.iter().find(|p| p.pass.name == name)
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.passes.iter().position(|p| p.pass.name == name)
    }

    /// The `data` records of pass `name` (its WGSL reads them as `data`).
    pub fn data(&self, name: &str) -> &[[f32; 4]] {
        self.index(name).and_then(|i| self.data.get(i)).map_or(&[], |(_, d)| d.as_slice())
    }

    /// Change the `data` records of pass `name`; uploaded before the next
    /// frame. `None` for an unknown pass.
    pub fn data_mut(&mut self, name: &str) -> Option<&mut Vec<[f32; 4]>> {
        let i = self.index(name)?;
        let (generation, d) = self.data.get_mut(i)?;
        *generation += 1;
        Some(d)
    }

    /// Data generation of pass `i` (bumped by every [`GamePasses::data_mut`]).
    pub fn data_generation(&self, i: usize) -> u64 {
        self.data.get(i).map_or(0, |(g, _)| *g)
    }

    /// Run a [`Run::Once`] pass again on the next frame.
    pub fn rerun(&mut self, name: &str) -> bool {
        match self.index(name).and_then(|i| self.reruns.get_mut(i)) {
            Some(r) => {
                *r += 1;
                true
            }
            None => false,
        }
    }

    pub fn rerun_generation(&self, i: usize) -> u64 {
        self.reruns.get(i).copied().unwrap_or(0)
    }
}

/// A pass or target refused, with what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamePassError {
    /// The pass or target name.
    pub name: String,
    pub what: String,
    pub fix: String,
}

impl GamePassError {
    fn new(name: &str, what: impl Into<String>, fix: impl Into<String>) -> Self {
        Self { name: name.to_string(), what: what.into(), fix: fix.into() }
    }
}

impl core::fmt::Display for GamePassError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "game pass {}: {}. fix: {}", self.name, self.what, self.fix)
    }
}

impl From<GamePassError> for PluginError {
    fn from(e: GamePassError) -> Self {
        PluginError(e.to_string())
    }
}

/// The name prefix an owner's passes and targets carry: `kit.topgun` ->
/// `topgun`, `look.toon` -> `toon`, a module label as is.
pub fn scope_of(owner: &str) -> &str {
    owner.split_once('.').map_or(owner, |(_, rest)| rest)
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('a'..='z'))
        && c.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && !s.contains("__")
}

/// `<scope>.<name>` with lowercase dotted parts.
fn check_name(name: &str, scope: &str) -> Result<(), GamePassError> {
    let ok_chars = name.split('.').all(is_ident);
    let local = name.strip_prefix(scope).and_then(|r| r.strip_prefix('.'));
    if !ok_chars || local.is_none_or(str::is_empty) {
        let tail = name.rsplit('.').next().filter(|t| is_ident(t)).unwrap_or("my_pass");
        return Err(GamePassError::new(
            name,
            format!("names must be `{scope}.<name>` in lowercase letters, digits and `_`"),
            format!("name it \"{scope}.{tail}\""),
        ));
    }
    Ok(())
}

fn check_ident(owner: &str, what: &str, s: &str) -> Result<(), GamePassError> {
    if !is_ident(s) || RESERVED.contains(&s) {
        return Err(GamePassError::new(
            owner,
            format!("{what} `{s}` is not a usable WGSL name (lowercase letters, digits, `_`; not one of {})", RESERVED.join(", ")),
            format!("rename it, e.g. \"{}\"", if is_ident(s) { format!("my_{s}") } else { "my_value".into() }),
        ));
    }
    Ok(())
}

/// Tier ids in catalog order (from the `Tiers` resource when there is one).
fn tier_ids(world: &World) -> Vec<String> {
    world
        .try_res::<evani_core::tiers::Tiers>()
        .map(|t| t.presets().iter().map(|p| p.id.clone()).collect())
        .unwrap_or_default()
}

fn check_tier(name: &str, tier: &str, known: &[String]) -> Result<(), GamePassError> {
    if !known.is_empty() && !known.iter().any(|t| t == tier) {
        return Err(GamePassError::new(
            name,
            format!("unknown tier `{tier}`"),
            format!("use one of {}", known.join(", ")),
        ));
    }
    Ok(())
}

/// Check a target against the graph and the tiers.
pub fn validate_target(t: &GameTarget, scope: &str, graph: &RenderGraph, tiers: &[String]) -> Result<(), GamePassError> {
    check_name(t.name, scope)?;
    if graph.target(t.name).is_some() {
        return Err(GamePassError::new(t.name, "a target with this name already exists", "give the target another name"));
    }
    let bad = |what: String, fix: &str| Err(GamePassError::new(t.name, what, fix));
    match t.size {
        Size::Screen(s) if !(s > 0.0 && s <= 2.0) => return bad(format!("screen scale {s} is outside 0..2"), "use a scale such as 0.5 (half resolution) or 1.0"),
        Size::Fixed([w, h]) if w == 0 || h == 0 || w > MAX_FIXED || h > MAX_FIXED => {
            return bad(format!("fixed size {w}x{h} is outside 1..{MAX_FIXED}"), "use a size such as 256x64");
        }
        Size::Volume([w, h, d]) if [w, h, d].iter().any(|&n| n == 0 || n > MAX_VOLUME) => {
            return bad(format!("volume {w}x{h}x{d} is outside 1..{MAX_VOLUME} per edge"), "use a size such as 64x64x64");
        }
        _ => {}
    }
    if t.history && t.is_volume() {
        return bad("a volume can't keep history".into(), "drop `.history()` or make it a 2D target");
    }
    for &(tier, s) in &t.tier_scale {
        check_tier(t.name, tier, tiers)?;
        if !matches!(t.size, Size::Screen(_)) {
            return bad("only screen-sized targets scale by tier".into(), "drop `.tier_scale(..)` or use `GameTarget::screen`");
        }
        if !(s > 0.0 && s <= 2.0) {
            return bad(format!("tier `{tier}` scale {s} is outside 0..2"), "use a scale such as 0.5");
        }
    }
    Ok(())
}

/// What [`validate`] resolves: the input bindings, the output (name and
/// kind) and whether an input reads the output (bound to a copy).
pub type Resolved = (Vec<InputKind>, (&'static str, OutputKind), bool);

/// Check a pass and resolve its bindings. `targets`: the game targets so
/// far; `known_passes`: names already taken.
pub fn validate(
    p: &GamePass,
    scope: &str,
    graph: &RenderGraph,
    targets: &[GameTarget],
    tiers: &[String],
) -> Result<Resolved, GamePassError> {
    let name = p.name;
    let err = |what: String, fix: String| Err(GamePassError::new(name, what, fix));
    check_name(name, scope)?;
    if graph.pass(name).is_some() {
        return err("a pass with this name already exists".into(), "give the pass another name".into());
    }
    if !p.wgsl.contains("fn pass_main") {
        let sig = match p.kind {
            Kind::Fullscreen => "fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32>",
            Kind::Compute => "fn pass_main(id: vec3<u32>) -> vec4<f32>",
        };
        return err("its WGSL has no `pass_main`".into(), format!("define `{sig} {{ ... }}` (the engine adds the bindings and entry point)"));
    }
    if p.inputs.len() > MAX_INPUTS {
        return err(format!("{} inputs, at most {MAX_INPUTS}", p.inputs.len()), "combine inputs into one game target, or split the pass".into());
    }
    let game = |n: &str| targets.iter().find(|t| t.name == n);
    let known = || {
        let mut v: Vec<&str> = graph.targets().iter().filter(|t| t.kind.is_texture() && t.kind != TargetKind::Depth).map(|t| t.name).collect();
        v.extend(targets.iter().map(|t| t.name));
        v.join(", ")
    };
    // Output.
    let Some(out) = p.output_target() else {
        return err(
            "it writes nothing".into(),
            match p.kind {
                Kind::Compute => "add `.writes(\"<game target>\")`: compute passes write one game target".into(),
                Kind::Fullscreen => "add `.writes(\"hdr\")` (or a game target), or place it with `.at(Stage::BeforeTonemap)`".into(),
            },
        );
    };
    let output = match (game(out), graph.target(out)) {
        (Some(t), _) => {
            if p.kind == Kind::Fullscreen && t.is_volume() {
                return err(format!("full-screen passes can't draw into volume `{out}`"), "write it from a compute pass (`GamePass::compute`)".into());
            }
            OutputKind::Game { format: t.format, volume: t.is_volume() }
        }
        (None, Some(td)) => {
            if p.kind == Kind::Compute {
                return err(format!("compute passes write game targets, not `{out}`"), "add a `GameTarget` and write that, then read it from a full-screen pass".into());
            }
            if td.kind == TargetKind::Swapchain || td.kind == TargetKind::Depth || !td.kind.is_texture() {
                return err(format!("it can't draw into `{out}`"), "draw into `hdr`, `ldr` or a game target".into());
            }
            OutputKind::Engine(td.kind)
        }
        (None, None) => return err(format!("it writes unknown target `{out}`"), format!("add it with `game_pass::add_target` first, or write one of: {}", known())),
    };
    // Inputs.
    let mut kinds = Vec::with_capacity(p.inputs.len());
    let mut copies = false;
    for (i, inp) in p.inputs.iter().enumerate() {
        check_ident(name, "input alias", inp.alias)?;
        if p.inputs[..i].iter().any(|q| q.alias == inp.alias) || p.params.iter().any(|q| q.name == inp.alias) {
            return err(format!("input alias `{}` is used twice", inp.alias), "give each input and parameter its own name".into());
        }
        let kind = match inp.source {
            Source::Depth => {
                if graph.target("depth").is_none() {
                    return err("it reads the scene depth, but this build has no 3D path".into(), "turn on the renderer's `forward` feature in the kit's Cargo.toml, or drop `.reads_depth(..)`".into());
                }
                InputKind::Depth
            }
            Source::History(t) => match game(t) {
                Some(gt) if gt.history => if gt.format.filterable() { InputKind::Color } else { InputKind::ColorUnfilterable },
                Some(_) => return err(format!("it reads the history of `{t}`, which keeps none"), format!("add `.history()` to the `{t}` target")),
                None => return err(format!("it reads the history of unknown target `{t}`"), "add the target with `game_pass::add_target(.. .history())` first".into()),
            },
            Source::Target(t) => {
                if t == out {
                    if p.kind == Kind::Compute {
                        return err(format!("a compute pass can't read the target it writes (`{t}`)"), "read last frame's copy with `.reads_history(..)` (give the target `.history()`)".into());
                    }
                    copies = true;
                }
                match (game(t), graph.target(t)) {
                    (Some(gt), _) => match (gt.is_volume(), gt.format.filterable()) {
                        (true, f) => InputKind::Volume { filterable: f },
                        (false, true) => InputKind::Color,
                        (false, false) => InputKind::ColorUnfilterable,
                    },
                    (None, Some(td)) => match td.kind {
                        TargetKind::Depth => InputKind::Depth,
                        k if k.is_texture() => InputKind::Color,
                        TargetKind::Swapchain => return err("it reads the swapchain (the canvas)".into(), "read `ldr` instead: the canvas is write-only".into()),
                        _ => return err(format!("`{t}` is a buffer, not a texture"), "read a texture target".into()),
                    },
                    (None, None) => return err(format!("it reads unknown target `{t}`"), format!("add it with `game_pass::add_target` first, or read one of: {}", known())),
                }
            }
        };
        kinds.push(kind);
    }
    // Parameters.
    let mut floats = 0;
    for (i, prm) in p.params.iter().enumerate() {
        check_ident(name, "parameter", prm.name)?;
        if prm.name == "on" || p.params[..i].iter().any(|q| q.name == prm.name) {
            return err(format!("parameter `{}` is declared twice (or is the built-in `on`)", prm.name), "give each parameter its own name".into());
        }
        if let ParamKind::F32 { default, min, max } = prm.kind
            && !(min <= default && default <= max && min.is_finite() && max.is_finite())
        {
            return err(format!("parameter `{}`: default {default} is outside {min}..{max}", prm.name), "make min <= default <= max".into());
        }
        floats = align_for(floats, prm) + prm.floats();
    }
    if floats > MAX_PARAM_FLOATS {
        return err(format!("its parameters need {floats} floats, at most {MAX_PARAM_FLOATS}"), "move bulk values into `data` (GamePasses::data_mut)".into());
    }
    if let Some(t) = p.min_tier {
        check_tier(name, t, tiers)?;
    }
    Ok((kinds, (out, output), copies))
}

/// Offset of a parameter that comes after `at` floats (vec3s start on a
/// whole `vec4`).
fn align_for(at: usize, p: &PassParam) -> usize {
    match p.kind {
        ParamKind::F32 { .. } => at,
        _ => at.div_ceil(4) * 4,
    }
}

fn graph_err(name: &str, e: crate::graph::GraphError) -> GamePassError {
    GamePassError::new(name, e.to_string(), "check the names of its inputs and outputs")
}

/// Add a game target (call from the kit's `build`, before the passes that
/// use it). Needs the render module.
pub fn add_target(ctx: &mut PluginContext, t: GameTarget) -> Result<(), PluginError> {
    let scope = scope_of(ctx.owner()).to_string();
    let tiers = tier_ids(ctx.world);
    let Some(mut graph) = ctx.world.try_res_mut::<RenderGraph>() else {
        return Err(GamePassError::new(t.name, "no render graph", "add the render module before the kit").into());
    };
    validate_target(&t, &scope, &graph, &tiers)?;
    graph.add_target(TargetDesc::new(t.name, TargetKind::Game)).map_err(|e| graph_err(t.name, e))?;
    drop(graph);
    match ctx.world.try_res_mut::<GamePasses>() {
        Some(mut gp) => gp.targets.push(t),
        None => return Err(PluginError("game passes: no `GamePasses` resource (add the render module before the kit)".into())),
    }
    Ok(())
}

/// Add a game pass (call from the kit's `build`, after its targets). Checks
/// it, registers its parameters and puts it in the render graph.
pub fn add(ctx: &mut PluginContext, p: GamePass) -> Result<(), PluginError> {
    let owner = ctx.owner().to_string();
    let scope = scope_of(&owner).to_string();
    let tiers = tier_ids(ctx.world);
    let (inputs, output, copies) = {
        let Some(graph) = ctx.world.try_res::<RenderGraph>() else {
            return Err(GamePassError::new(p.name, "no render graph", "add the render module before the kit").into());
        };
        let Some(gp) = ctx.world.try_res::<GamePasses>() else {
            return Err(PluginError("game passes: no `GamePasses` resource (add the render module before the kit)".into()));
        };
        validate(&p, &scope, &graph, &gp.targets, &tiers)?
    };
    // Parameters: kit.<id>.<pass>.<name>.
    let local = p.name.strip_prefix(&scope).and_then(|r| r.strip_prefix('.')).unwrap_or(p.name);
    let full = |n: &str| format!("{owner}.{local}.{n}");
    let on = ctx.param_bool(ParamDef::bool(&full("on"), true).describe("Run this game pass").gpu())?;
    let mut offsets = Vec::with_capacity(p.params.len());
    let mut handles = Vec::with_capacity(p.params.len());
    let mut at = 0;
    for prm in &p.params {
        at = align_for(at, prm);
        offsets.push(at);
        at += prm.floats();
        let h = match prm.kind {
            ParamKind::F32 { default, min, max } => ParamHandle::F32(ctx.param_f32(ParamDef::f32(&full(prm.name), default).range(min, max).describe(prm.description).gpu())?),
            ParamKind::Vec3 { default } => ParamHandle::Vec3(ctx.param_vec3(ParamDef::vec3(&full(prm.name), default).describe(prm.description).gpu())?),
            ParamKind::Color { default } => ParamHandle::Vec3(ctx.param_vec3(ParamDef::color(&full(prm.name), default).describe(prm.description).gpu())?),
        };
        handles.push(h);
    }
    // The graph pass: inputs order it (history reads don't: they read last
    // frame); a full-screen pass on an engine target modifies it in place.
    let mut reads: Vec<&'static str> = p
        .inputs
        .iter()
        .filter_map(|i| match i.source {
            Source::Target(t) => Some(t),
            Source::Depth => Some("depth"),
            Source::History(_) => None,
        })
        .collect();
    if matches!(output.1, OutputKind::Engine(_)) && !reads.contains(&output.0) {
        reads.push(output.0);
    }
    let order = p.order.unwrap_or(p.stage.order());
    let desc = match p.kind {
        Kind::Fullscreen => PassDesc::new(p.name, "game"),
        Kind::Compute => PassDesc::compute(p.name, "game"),
    }
    .reads(&reads)
    .writes(&[output.0])
    .order(order);
    {
        let Some(mut graph) = ctx.world.try_res_mut::<RenderGraph>() else {
            return Err(GamePassError::new(p.name, "no render graph", "add the render module before the kit").into());
        };
        graph.add_pass(desc).map_err(|e| graph_err(p.name, e))?;
    }
    let reg = Registered { pass: p, inputs, output, copies_output: copies, offsets, on: Some(on), handles };
    if let Some(mut gp) = ctx.world.try_res_mut::<GamePasses>() {
        gp.passes.push(reg);
        gp.data.push((0, Vec::new()));
        gp.reruns.push(0);
    }
    Ok(())
}

/// The full WGSL module of a pass: engine header, inputs, parameter
/// accessors, the entry point and the kit's code.
pub fn compose_wgsl(r: &Registered) -> String {
    use core::fmt::Write as _;
    let p = &r.pass;
    let mut s = String::with_capacity(ENGINE_WGSL.len() + p.wgsl.len() + 1024);
    s.push_str(ENGINE_WGSL);
    for (i, (inp, kind)) in p.inputs.iter().zip(&r.inputs).enumerate() {
        let _ = writeln!(s, "@group(0) @binding({}) var {}: {};", binding::INPUT0 + i as u32, inp.alias, kind.wgsl());
    }
    let comp = ["x", "y", "z", "w"];
    for (prm, &o) in p.params.iter().zip(&r.offsets) {
        let (v, c) = (o / 4, o % 4);
        match prm.kind {
            ParamKind::F32 { .. } => {
                let _ = writeln!(s, "fn param_{}() -> f32 {{ return params.v[{v}].{}; }}", prm.name, comp[c]);
            }
            _ => {
                let _ = writeln!(s, "fn param_{}() -> vec3<f32> {{ return params.v[{v}].xyz; }}", prm.name);
            }
        }
    }
    match (p.kind, r.output.1) {
        (Kind::Compute, OutputKind::Game { format, volume }) => {
            let (dim, wg, guard, coord) = if volume {
                ("3d", "4, 4, 4", "any(id >= size)", "id")
            } else {
                ("2d", "8, 8, 1", "any(id.xy >= size)", "id.xy")
            };
            let _ = writeln!(s, "@group(0) @binding({}) var out_tex: texture_storage_{dim}<{}, write>;", binding::OUT, format.wgsl());
            let _ = writeln!(
                s,
                "@compute @workgroup_size({wg}) fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {{\n    let size = textureDimensions(out_tex);\n    if ({guard}) {{ return; }}\n    textureStore(out_tex, {coord}, pass_main(id));\n}}"
            );
        }
        _ => s.push_str(FULLSCREEN_WGSL),
    }
    s.push_str("// ---- the game's code ----\n");
    s.push_str(&p.wgsl);
    s.push('\n');
    s
}

/// Per frame (render module): each pass runs while its `on` parameter is
/// true, the active tier reaches its `min_tier`, and every game target it
/// reads has an enabled writer.
pub fn frame(world: &mut World) {
    let Some(gp) = world.try_res::<GamePasses>() else {
        return;
    };
    if gp.passes.is_empty() {
        return;
    }
    let (ids, active) = match world.try_res::<evani_core::tiers::Tiers>() {
        Some(t) => (t.presets().iter().map(|p| p.id.clone()).collect::<Vec<_>>(), t.active_index()),
        None => (Vec::new(), 0),
    };
    let reg = world.res::<ParamRegistry>();
    let mut graph = world.res_mut::<RenderGraph>();
    let want: Vec<(&'static str, bool)> = gp
        .passes
        .iter()
        .map(|r| {
            let on = r.on.is_none_or(|h| reg.bool(h));
            let tier_ok = r.pass.min_tier.is_none_or(|t| ids.iter().position(|i| i == t).is_none_or(|need| active >= need));
            (r.pass.name, on && tier_ok)
        })
        .collect();
    // Readers of a game target whose writers are all off are off too (in
    // graph order, so it carries down a chain).
    let order: Vec<&'static str> = graph.ordered().map(|p| p.name).collect();
    let mut enabled: Vec<(&'static str, bool)> = Vec::with_capacity(want.len());
    for name in order {
        let Some(&(_, mut on)) = want.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        if let Some(r) = gp.pass(name) {
            for inp in &r.pass.inputs {
                if let Source::Target(t) = inp.source
                    && gp.target(t).is_some()
                {
                    let writers = gp.passes.iter().filter(|w| w.output.0 == t && w.pass.name != name);
                    let mut any = false;
                    let mut live = false;
                    for w in writers {
                        any = true;
                        live |= enabled.iter().find(|(n, _)| *n == w.pass.name).is_some_and(|e| e.1);
                    }
                    on &= !any || live;
                }
            }
        }
        enabled.push((name, on));
    }
    for (name, on) in enabled {
        if graph.pass(name).is_some_and(|p| p.enabled != on) {
            graph.set_enabled(name, on);
        }
    }
}

/// Check every game pass of a built app (call it in a kit test, with
/// evani-render's `game-passes-check` feature in the kit's
/// dev-dependencies): the render graph compiles with them, and each
/// pass's whole WGSL module parses and validates. Returns the number of
/// passes checked, or every problem with its fix.
#[cfg(feature = "game-passes-check")]
pub fn check(world: &World) -> Result<usize, String> {
    let Some(gp) = world.try_res::<GamePasses>() else {
        return Err("no game passes: build the app with the render module (with the `game-passes` feature) before the kit".into());
    };
    let mut problems = Vec::new();
    if let Some(graph) = world.try_res::<RenderGraph>() {
        let mut g = graph.clone();
        if let Err(e) = g.compile() {
            problems.push(format!("render graph: {e}. fix: check the inputs and outputs of the game passes"));
        }
    } else {
        problems.push("no render graph. fix: add the render module before the kit".to_string());
    }
    for r in &gp.passes {
        if let Err(e) = validate_wgsl(r) {
            problems.push(format!("game pass {}: {e}. fix: correct `pass_main` (see docs/GAME_PASSES.md for what the engine declares)", r.pass.name));
        }
    }
    if problems.is_empty() { Ok(gp.passes.len()) } else { Err(problems.join("\n")) }
}

/// Parse and validate one pass's whole module with naga.
#[cfg(feature = "game-passes-check")]
pub fn validate_wgsl(r: &Registered) -> Result<(), String> {
    let src = compose_wgsl(r);
    let module = naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| e.emit_to_string(&src))?;
    Ok(())
}

/// Inverse of a column-major 4x4 matrix (`None` when singular).
pub fn invert(m: &crate::math::Mat4) -> Option<crate::math::Mat4> {
    let a = |c: usize, r: usize| m[c * 4 + r];
    let mut inv = [0.0f32; 16];
    // Cofactors by 2x2 sub-determinants (rows 0-1 and 2-3).
    let s0 = a(0, 0) * a(1, 1) - a(1, 0) * a(0, 1);
    let s1 = a(0, 0) * a(1, 2) - a(1, 0) * a(0, 2);
    let s2 = a(0, 0) * a(1, 3) - a(1, 0) * a(0, 3);
    let s3 = a(0, 1) * a(1, 2) - a(1, 1) * a(0, 2);
    let s4 = a(0, 1) * a(1, 3) - a(1, 1) * a(0, 3);
    let s5 = a(0, 2) * a(1, 3) - a(1, 2) * a(0, 3);
    let c5 = a(2, 2) * a(3, 3) - a(3, 2) * a(2, 3);
    let c4 = a(2, 1) * a(3, 3) - a(3, 1) * a(2, 3);
    let c3 = a(2, 1) * a(3, 2) - a(3, 1) * a(2, 2);
    let c2 = a(2, 0) * a(3, 3) - a(3, 0) * a(2, 3);
    let c1 = a(2, 0) * a(3, 2) - a(3, 0) * a(2, 2);
    let c0 = a(2, 0) * a(3, 1) - a(3, 0) * a(2, 1);
    let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
    if det.abs() < 1e-20 || !det.is_finite() {
        return None;
    }
    let d = 1.0 / det;
    // The formula reads the column-major storage as the transpose; its
    // result is then the transposed inverse in the same reading, so it
    // goes back with the same index order.
    let mut set = |i: usize, j: usize, v: f32| inv[i * 4 + j] = v * d;
    set(0, 0, a(1, 1) * c5 - a(1, 2) * c4 + a(1, 3) * c3);
    set(0, 1, -a(0, 1) * c5 + a(0, 2) * c4 - a(0, 3) * c3);
    set(0, 2, a(3, 1) * s5 - a(3, 2) * s4 + a(3, 3) * s3);
    set(0, 3, -a(2, 1) * s5 + a(2, 2) * s4 - a(2, 3) * s3);
    set(1, 0, -a(1, 0) * c5 + a(1, 2) * c2 - a(1, 3) * c1);
    set(1, 1, a(0, 0) * c5 - a(0, 2) * c2 + a(0, 3) * c1);
    set(1, 2, -a(3, 0) * s5 + a(3, 2) * s2 - a(3, 3) * s1);
    set(1, 3, a(2, 0) * s5 - a(2, 2) * s2 + a(2, 3) * s1);
    set(2, 0, a(1, 0) * c4 - a(1, 1) * c2 + a(1, 3) * c0);
    set(2, 1, -a(0, 0) * c4 + a(0, 1) * c2 - a(0, 3) * c0);
    set(2, 2, a(3, 0) * s4 - a(3, 1) * s2 + a(3, 3) * s0);
    set(2, 3, -a(2, 0) * s4 + a(2, 1) * s2 - a(2, 3) * s0);
    set(3, 0, -a(1, 0) * c3 + a(1, 1) * c1 - a(1, 2) * c0);
    set(3, 1, a(0, 0) * c3 - a(0, 1) * c1 + a(0, 2) * c0);
    set(3, 2, -a(3, 0) * s3 + a(3, 1) * s1 - a(3, 2) * s0);
    set(3, 3, a(2, 0) * s3 - a(2, 1) * s1 + a(2, 2) * s0);
    Some(inv)
}

/// The engine frame uniform words for a camera and light (the per-pass
/// `size` and `misc.w` are filled at draw time).
pub struct FrameInputs {
    pub view_proj: crate::math::Mat4,
    pub eye: [f32; 3],
    pub seconds: f32,
    pub light_dir: [f32; 3],
    pub light_intensity: f32,
    pub light_color: [f32; 3],
    pub tier: u32,
    pub frame: u64,
    pub dt: f32,
    pub parity: u32,
    pub ambient: [f32; 3],
    pub background: [f32; 3],
}

/// Fill `out` (the `EngineFrame` layout) from `f`; `size` and the output
/// depth are per pass ([`set_output_size`]).
pub fn engine_words(f: &FrameInputs, out: &mut [f32; ENGINE_WORDS]) {
    let inv = invert(&f.view_proj).unwrap_or([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    out[..16].copy_from_slice(&inv);
    out[16..32].copy_from_slice(&f.view_proj);
    let l = crate::math::normalize(f.light_dir);
    let rows: [[f32; 4]; 7] = [
        [f.eye[0], f.eye[1], f.eye[2], f.seconds],
        [l[0], l[1], l[2], f.light_intensity],
        [f.light_color[0], f.light_color[1], f.light_color[2], f.tier as f32],
        [1.0, 1.0, 1.0, 1.0],
        [(f.frame % (1 << 24)) as f32, f.dt, f.parity as f32, 1.0],
        [f.ambient[0], f.ambient[1], f.ambient[2], 0.0],
        [f.background[0], f.background[1], f.background[2], 0.0],
    ];
    for (i, r) in rows.iter().enumerate() {
        out[32 + i * 4..36 + i * 4].copy_from_slice(r);
    }
}

/// Write a pass's output size into its copy of the engine words.
pub fn set_output_size(out: &mut [f32; ENGINE_WORDS], size: [u32; 3]) {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    out[44..48].copy_from_slice(&[w, h, 1.0 / w, 1.0 / h]);
    out[51] = size[2].max(1) as f32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    const HAZE: &str = "fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
        let d = textureLoad(depth, vec2<i32>(px), 0);
        let c = textureSample(scene, samp, uv);
        return vec4<f32>(c.rgb * param_tint() * param_strength() + vec3<f32>(d), 1.0);
    }";
    const LUT: &str = "fn pass_main(id: vec3<u32>) -> vec4<f32> { return vec4<f32>(vec3<f32>(id) / 64.0, param_gain()); }";

    struct Kit(fn(&mut PluginContext) -> Result<(), PluginError>);
    impl evani_core::plugin::KitPlugin for Kit {
        fn name(&self) -> &'static str {
            "tg"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            (self.0)(kit)
        }
    }

    fn app(f: fn(&mut PluginContext) -> Result<(), PluginError>) -> App {
        App::new("t").with_module(crate::RenderModule::new()).with_kit(Kit(f))
    }

    fn errors(a: &App) -> Vec<String> {
        let log = a.world.res::<evani_core::overlay::ErrorLog>();
        log.entries().map(|e| e.message.clone()).collect()
    }

    fn good(kit: &mut PluginContext) -> Result<(), PluginError> {
        add_target(kit, GameTarget::volume("tg.noise", Format::Rgba8Unorm, [32, 32, 32]))?;
        add_target(kit, GameTarget::screen("tg.trail", Format::Rgba16Float, 0.5).history().tier_scale("low", 0.25))?;
        add(kit, GamePass::compute("tg.noise", LUT).writes("tg.noise").once().param(PassParam::f32("gain", 1.0, 0.0, 2.0)))?;
        add(
            kit,
            GamePass::fullscreen("tg.trail", "fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> { return mix(textureSample(scene, samp, uv), textureSample(prev, samp, uv), 0.8); }")
                .reads("scene", "hdr")
                .reads_history("prev", "tg.trail")
                .writes("tg.trail"),
        )?;
        add(
            kit,
            GamePass::fullscreen("tg.haze", HAZE)
                .at(Stage::BeforeTonemap)
                .reads("scene", "hdr")
                .reads_depth("depth")
                .reads("noise", "tg.noise")
                .reads("trail", "tg.trail")
                .param(PassParam::f32("strength", 0.5, 0.0, 2.0).describe("How strong the haze is"))
                .param(PassParam::color("tint", [1.0, 0.9, 0.8]))
                .min_tier("medium"),
        )?;
        Ok(())
    }

    /// A kit's passes join the frame where their inputs put them, their
    /// parameters are live parameters, and their uniforms pack in order.
    #[cfg(feature = "forward")]
    #[test]
    fn passes_join_the_graph_with_live_params() {
        let mut a = app(good);
        a.start();
        assert!(errors(&a).iter().all(|e| !e.contains("game pass") && !e.contains("render graph")), "{:?}", errors(&a));
        let g = a.world.res::<RenderGraph>();
        let order: Vec<&str> = g.ordered().map(|p| p.name).collect();
        let at = |n: &str| order.iter().position(|&p| p == n).unwrap_or(usize::MAX);
        assert!(at("tg.noise") < at("tg.haze") && at("tg.trail") < at("tg.haze"), "{order:?}");
        assert!(at("tg.haze") < at("tonemap") && at("sprites") < at("tg.haze"), "{order:?}");
        assert_eq!(g.pass("tg.noise").map(|p| p.kind), Some(crate::graph::PassKind::Compute));
        assert_eq!(g.target("tg.trail").map(|t| t.kind), Some(TargetKind::Game));
        drop(g);
        let reg = a.world.res::<ParamRegistry>();
        for n in ["kit.tg.haze.strength", "kit.tg.haze.tint", "kit.tg.haze.on", "kit.tg.noise.gain", "kit.tg.trail.on"] {
            assert!(reg.id(n).is_some(), "{n} not registered");
        }
        let gp = a.world.res::<GamePasses>();
        let haze = gp.pass("tg.haze").unwrap();
        assert_eq!(haze.offsets, vec![0, 4]);
        let mut w = [0.0; MAX_PARAM_FLOATS];
        assert_eq!(haze.param_words(&reg, &mut w), 7);
        assert_eq!(&w[..7], &[0.5, 0.0, 0.0, 0.0, 1.0, 0.9, 0.8]);
        assert_eq!(haze.inputs, vec![InputKind::Color, InputKind::Depth, InputKind::Volume { filterable: true }, InputKind::Color]);
        assert!(haze.copies_output, "reading hdr while drawing into it binds a copy");
        assert_eq!(gp.target("tg.trail").unwrap().extent([800, 600], "low"), [200, 150, 1]);
        assert_eq!(gp.target("tg.trail").unwrap().extent([800, 600], "high"), [400, 300, 1]);
    }

    /// Every generated module parses and validates (naga): the engine
    /// header, inputs, accessors, entry points and the kit's code agree.
    #[cfg(all(feature = "game-passes-check", feature = "forward"))]
    #[test]
    fn generated_wgsl_validates() {
        let mut a = app(good);
        a.start();
        assert_eq!(check(&a.world), Ok(3));
        // A typo in the kit's code is reported with the pass name and a fix.
        fn typo(kit: &mut PluginContext) -> Result<(), PluginError> {
            add(kit, GamePass::fullscreen("tg.bad", "fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> { return param_nope(); }").writes("hdr"))
        }
        let mut b = app(typo);
        b.start();
        let e = check(&b.world).unwrap_err();
        assert!(e.contains("game pass tg.bad") && e.contains("fix:"), "{e}");
    }

    fn refused(f: fn(&mut PluginContext) -> Result<(), PluginError>) -> String {
        let a = app(f);
        errors(&a).into_iter().find(|e| e.contains("game pass")).unwrap_or_default()
    }

    /// Errors become tools: each bad declaration is refused at build with
    /// what is wrong and a `fix:` line.
    #[test]
    fn bad_declarations_are_refused_with_a_fix() {
        type Declare = fn(&mut PluginContext) -> Result<(), PluginError>;
        let cases: [(Declare, &str); 9] = [
            (|k| add(k, GamePass::fullscreen("haze", HAZE).writes("hdr")), "name it \"tg.haze\""),
            (|k| add(k, GamePass::fullscreen("tg.x", "fn main() {}").writes("hdr")), "has no `pass_main`"),
            (|k| add(k, GamePass::fullscreen("tg.x", HAZE).reads("scene", "nope").writes("hdr")), "unknown target `nope`"),
            (|k| add(k, GamePass::fullscreen("tg.x", HAZE).reads("s", "swapchain").writes("hdr")), "read `ldr` instead"),
            (|k| add(k, GamePass::compute("tg.x", LUT).writes("hdr")), "compute passes write game targets"),
            (|k| add(k, GamePass::compute("tg.x", LUT)), "it writes nothing"),
            (|k| add(k, GamePass::fullscreen("tg.x", HAZE).reads_history("p", "tg.none").writes("hdr")), "history of unknown target"),
            (|k| add(k, GamePass::fullscreen("tg.x", HAZE).reads("frame", "hdr").writes("hdr")), "not a usable WGSL name"),
            (|k| add(k, GamePass::fullscreen("tg.x", HAZE).writes("hdr").min_tier("mega")), "unknown tier `mega`"),
        ];
        for (f, want) in cases {
            let e = refused(f);
            assert!(e.contains(want) && e.contains("fix:"), "want `{want}` in {e:?}");
        }
        let e = refused(|k| {
            add_target(k, GameTarget::screen("tg.t", Format::Rgba16Float, 0.5))?;
            add(k, GamePass::fullscreen("tg.x", HAZE).reads_history("p", "tg.t").writes("hdr"))
        });
        assert!(e.contains("add `.history()`"), "{e}");
        let e = refused(|k| add_target(k, GameTarget::volume("tg.v", Format::Rgba8Unorm, [512, 8, 8])));
        assert!(e.contains("outside 1..256"), "{e}");
        let e = refused(|k| {
            add_target(k, GameTarget::volume("tg.v", Format::Rgba8Unorm, [8, 8, 8]))?;
            add(k, GamePass::fullscreen("tg.x", HAZE).writes("tg.v"))
        });
        assert!(e.contains("compute pass"), "{e}");
    }

    /// The tier rule and the `on` parameter switch a pass, and a reader of
    /// a game target goes off with its only writer.
    #[cfg(feature = "forward")]
    #[test]
    fn tiers_and_on_switch_passes() {
        let mut a = app(good);
        a.start();
        let set_tier = |a: &mut App, id: &str| assert!(a.world.res_mut::<evani_core::tiers::Tiers>().set_active(id), "{id}");
        let on = |a: &App, n: &str| a.world.res::<RenderGraph>().pass(n).unwrap().enabled;
        set_tier(&mut a, "low");
        frame(&mut a.world);
        assert!(!on(&a, "tg.haze") && on(&a, "tg.trail"));
        set_tier(&mut a, "high");
        frame(&mut a.world);
        assert!(on(&a, "tg.haze"));
        let id = a.world.res::<ParamRegistry>().id("kit.tg.noise.on").unwrap();
        a.world.res_mut::<ParamRegistry>().set(id, evani_core::params::ParamValue::Bool(false)).unwrap();
        frame(&mut a.world);
        assert!(!on(&a, "tg.noise") && !on(&a, "tg.haze"), "the haze reads the noise only the noise pass writes");
    }

    #[cfg(feature = "forward")]
    #[test]
    fn data_and_reruns_bump_generations() {
        let mut a = app(good);
        a.start();
        let mut gp = a.world.res_mut::<GamePasses>();
        let i = gp.passes().iter().position(|p| p.pass.name == "tg.haze").unwrap();
        assert_eq!(gp.data_generation(i), 0);
        gp.data_mut("tg.haze").unwrap().push([1.0, 2.0, 3.0, 4.0]);
        assert_eq!((gp.data_generation(i), gp.data("tg.haze").len()), (1, 1));
        assert!(gp.data_mut("tg.none").is_none());
        assert!(gp.rerun("tg.noise") && !gp.rerun("tg.none"));
    }

    /// Without the 3D path there is no depth to read, and the pass says so.
    #[cfg(not(feature = "forward"))]
    #[test]
    fn depth_needs_the_3d_path() {
        let e = refused(|k| add(k, GamePass::fullscreen("tg.x", HAZE).reads_depth("depth").writes("hdr")));
        assert!(e.contains("no 3D path") && e.contains("`forward`"), "{e}");
    }

    #[test]
    fn inverse_undoes_the_view_projection() {
        let cam = evani_core::camera::CameraPose { eye: [3.0, 4.0, 10.0], target: [0.0, 1.0, 0.0], ..evani_core::camera::CameraPose::default() };
        let vp = crate::shaders::view_proj(&cam, 16.0 / 9.0);
        let inv = invert(&vp).unwrap();
        let id = crate::math::mul(&vp, &inv);
        for (i, v) in id.iter().enumerate() {
            let want = if i % 5 == 0 { 1.0 } else { 0.0 };
            assert!((v - want).abs() < 1e-3, "{i}: {v}");
        }
        assert!(invert(&[0.0; 16]).is_none());
        let mut w = [0.0; ENGINE_WORDS];
        let f = FrameInputs { view_proj: vp, eye: cam.eye, seconds: 2.0, light_dir: [0.0, 2.0, 0.0], light_intensity: 3.0, light_color: [1.0; 3], tier: 2, frame: 7, dt: 0.016, parity: 1, ambient: [0.1; 3], background: [0.2; 3] };
        engine_words(&f, &mut w);
        set_output_size(&mut w, [400, 200, 1]);
        assert_eq!(&w[32..40], &[3.0, 4.0, 10.0, 2.0, 0.0, 1.0, 0.0, 3.0]);
        assert_eq!(&w[44..48], &[400.0, 200.0, 0.0025, 0.005]);
        assert_eq!((w[43], w[48], w[50]), (2.0, 7.0, 1.0));
    }
}
