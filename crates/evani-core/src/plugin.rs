//! Plugin interfaces: how genre kits, art-style looks and optional engine
//! modules plug into an [`crate::app::App`].
//!
//! * [`KitPlugin`]: components, systems, scenes, input actions, data files,
//!   tunable params and formulas for one genre.
//! * [`LookPlugin`]: palettes, materials, WGSL shader-slot bodies, post-pass
//!   settings, tunable params and procedural asset generators for one style.
//! * [`EngineModule`]: optional engine modules (render, fx, physics, net, xr,
//!   live) with per-frame hooks and an overlay status line.
//!
//! All three register through [`PluginContext`], which stamps an owner
//! (`kit.<name>`, `look.<name>` or the module label such as `fx`, dotted like
//! param names) on everything they add so the
//! schema and overlay can show where a value came from.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::ecs::{Component, Schedule, SystemDesc, World};
use crate::effects::StyleClasses;
use crate::formulas::{FormulaError, FormulaId, FormulaJson, Formulas};
use crate::input::{ActionDef, ActionId, BindingParseError, Input};
use crate::params::{
    BoolParam, EnumParam, F32Param, I32Param, ParamDef, ParamError, ParamId, ParamRegistry,
    Vec3Param,
};
use crate::rng::Pcg32;
use crate::scene::{Scene, SceneRegistry};
use crate::slots::{ShaderSlots, SlotError};
use crate::version::PartVersion;

/// Error from a plugin's `build`. Routed to the overlay by the app.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginError(pub String);

impl core::fmt::Display for PluginError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! plugin_error_from {
    ($($t:ty),*) => {$(
        impl From<$t> for PluginError {
            fn from(e: $t) -> Self {
                PluginError(e.to_string())
            }
        }
    )*};
}
plugin_error_from!(ParamError, FormulaError, BindingParseError, SlotError, String);

impl From<&str> for PluginError {
    fn from(e: &str) -> Self {
        PluginError(e.to_string())
    }
}

/// Static data files registered by kits (level layouts, tables...). Values
/// are usually `include_str!`ed so they ship inside the wasm.
#[derive(Debug, Default, Clone)]
pub struct DataFiles {
    files: BTreeMap<String, &'static str>,
}

impl DataFiles {
    pub fn get(&self, name: &str) -> Option<&'static str> {
        self.files.get(name).copied()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
}

/// What this build contains. World resource kept up to date by the app, so
/// tools (evani-live `schema.get`, the overlay) can list it.
#[derive(Debug, Clone, Default)]
pub struct BuildInfo {
    /// The game's name (`App::new`).
    pub game: String,
    /// Kit crate, e.g. `evani-kit-example`.
    pub kit: Option<String>,
    /// Look crate, e.g. `evani-look-basic`.
    pub look: Option<String>,
    /// `(id, label)` of every engine module, in registration order.
    pub modules: Vec<(&'static str, &'static str)>,
    /// Every Evani.Dev part compiled into this game with its crate version:
    /// core, then the modules that report one (in registration order), the
    /// kit and the look ([`crate::version`]).
    pub parts: Vec<PartVersion>,
}

/// Baked JSON (params / formulas) queued during build and applied by the app
/// at start, after every module has registered its params.
#[derive(Debug, Default, Clone)]
pub struct PendingBaked {
    pub params: Vec<(String, &'static str)>,
    pub formulas: Vec<(String, &'static str)>,
    /// Look post-pass param overrides, applied at start as well (first, so
    /// baked files win over them).
    pub overrides: Vec<(String, String, Value)>,
    /// `(owner, [(relative path, contents)])` from a crate's `BAKED` index
    /// (written by `tools/bake-index.rs`); see [`PluginContext::baked_files`].
    pub files: Vec<(String, &'static [(&'static str, &'static str)])>,
}

/// The `BAKED` index type a look or kit gets from `tools/bake-index.rs`.
pub type BakedFiles = &'static [(&'static str, &'static str)];

/// Registration surface handed to kits, looks and modules.
pub struct PluginContext<'a> {
    pub world: &'a mut World,
    pub schedule: &'a mut Schedule,
    pub scenes: &'a mut SceneRegistry,
    owner: String,
}

impl<'a> PluginContext<'a> {
    pub fn new(
        world: &'a mut World,
        schedule: &'a mut Schedule,
        scenes: &'a mut SceneRegistry,
        owner: String,
    ) -> Self {
        Self {
            world,
            schedule,
            scenes,
            owner,
        }
    }

    /// `kit.<name>`, `look.<name>` or a module label (`fx`, `render`...).
    pub fn owner(&self) -> &str {
        &self.owner
    }

    // ---- ECS ------------------------------------------------------------------

    pub fn component<T: Component>(&mut self, name: &'static str) {
        self.world.register_component::<T>(name);
    }

    pub fn resource<R: 'static>(&mut self, value: R) {
        self.world.insert_resource(value);
    }

    pub fn event<T: 'static>(&mut self) {
        self.world.add_event::<T>();
    }

    pub fn system(&mut self, desc: SystemDesc) {
        self.schedule.add(desc);
    }

    pub fn scene(&mut self, name: &str, factory: impl Fn() -> Box<dyn Scene> + 'static) {
        self.scenes.register(name, factory);
    }

    // ---- input & data -----------------------------------------------------------

    pub fn action(&mut self, def: ActionDef) -> ActionId {
        self.world.res_mut::<Input>().add_action(def)
    }

    /// On-screen touch controls for phones and tablets (feature `touch`;
    /// README "Touch controls"). Declare the actions first: every control
    /// names one. Replaces an earlier layout.
    #[cfg(feature = "touch")]
    pub fn touch_controls(&mut self, layout: crate::touch::TouchLayout) -> Result<(), PluginError> {
        let t = crate::touch::TouchControls::new(layout, &self.world.res::<Input>()).map_err(PluginError)?;
        self.world.insert_resource(t);
        Ok(())
    } #[doc = " Mouse-look through pointer lock (feature `pointer-lock`; README \"Pointer lock\"). Declares the lock; the kit then asks for it with [`crate::pointer_lock::request`] (or the resource) when play starts and gives it back with `release` on menus. The engine releases it on its own on Esc, blur, a hidden tab, unload, errors and panics."] #[cfg(feature = "pointer-lock")] pub fn pointer_lock(&mut self) { if self.world.try_res::<crate::pointer_lock::PointerLock>().is_none() { self.world.insert_resource(crate::pointer_lock::PointerLock::default()); } }

    pub fn data_file(&mut self, name: &str, contents: &'static str) {
        if let Some(mut d) = self.world.try_res_mut::<DataFiles>() {
            d.files.insert(name.to_string(), contents);
        }
    }

    // ---- params & formulas --------------------------------------------------------

    fn stamp(&self, mut def: ParamDef) -> ParamDef {
        if def.owner.is_empty() {
            def.owner = self.owner.clone();
        }
        def
    }

    pub fn param(&mut self, def: ParamDef) -> Result<ParamId, ParamError> {
        let def = self.stamp(def);
        self.world.res_mut::<ParamRegistry>().register(def)
    }

    pub fn param_f32(&mut self, def: ParamDef) -> Result<F32Param, ParamError> {
        let def = self.stamp(def);
        self.world.res_mut::<ParamRegistry>().register_f32(def)
    }

    pub fn param_i32(&mut self, def: ParamDef) -> Result<I32Param, ParamError> {
        let def = self.stamp(def);
        self.world.res_mut::<ParamRegistry>().register_i32(def)
    }

    pub fn param_bool(&mut self, def: ParamDef) -> Result<BoolParam, ParamError> {
        let def = self.stamp(def);
        self.world.res_mut::<ParamRegistry>().register_bool(def)
    }

    pub fn param_vec3(&mut self, def: ParamDef) -> Result<Vec3Param, ParamError> {
        let def = self.stamp(def);
        self.world.res_mut::<ParamRegistry>().register_vec3(def)
    }

    pub fn param_enum(&mut self, def: ParamDef) -> Result<EnumParam, ParamError> {
        let def = self.stamp(def);
        self.world.res_mut::<ParamRegistry>().register_enum(def)
    }

    /// Register a CPU formula (curve or expression).
    pub fn formula(
        &mut self,
        name: &str,
        description: &str,
        vars: &[&str],
        body: FormulaJson,
    ) -> Result<FormulaId, FormulaError> {
        let mut f = self.world.res_mut::<Formulas>();
        let id = f.register(name, description, vars, body)?;
        f.set_owner(id, &self.owner);
        Ok(id)
    }

    /// Queue a baked params file (`{"name": value}`), applied at start.
    pub fn baked_params(&mut self, json: &'static str) {
        let owner = self.owner.clone();
        self.world
            .res_mut::<PendingBaked>()
            .params
            .push((owner, json));
    }

    /// Queue a baked formulas file, applied at start.
    pub fn baked_formulas(&mut self, json: &'static str) {
        let owner = self.owner.clone();
        self.world
            .res_mut::<PendingBaked>()
            .formulas
            .push((owner, json));
    }

    /// Queue a crate's baked files (`bake.export` output, embedded with
    /// `include_str!` by `tools/bake-index.rs`), applied at start after
    /// every module has registered:
    /// * `params.json`, `policy.json`: `{"name": value}` param values;
    /// * `slots/<slot>.wgsl`: shader-slot body (via `ShaderSlots::set_default`);
    /// * `slots/<formula>.expr`: expression formula body;
    /// * `curves/<formula>.json`: `{"points": [[x, y], ...]}` curve body.
    ///
    /// Other paths (e.g. `CHANGES.md`) are ignored.
    pub fn baked_files(&mut self, files: BakedFiles) {
        let owner = self.owner.clone();
        self.world
            .res_mut::<PendingBaked>()
            .files
            .push((owner, files));
    }
}

// ---- kits -----------------------------------------------------------------------

/// A genre kit (kart racer, RTS, platformer...).
pub trait KitPlugin: 'static {
    /// Short id, e.g. `example` (crate `evani-kit-example`).
    fn name(&self) -> &'static str;
    /// Register components, systems, scenes, input actions, data files,
    /// params and formulas. Errors are shown in the overlay.
    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError>;
    /// Scene to push at start, if the game does not choose one.
    fn start_scene(&self) -> Option<&'static str> {
        None
    }
    /// The kit crate's version, shown in the overlay and `session.status`.
    /// Implement it as `Some(env!("CARGO_PKG_VERSION"))` in the kit's own
    /// crate (a default here would report evani-core's version instead).
    fn version(&self) -> Option<&'static str> {
        None
    }
    /// True when the kit supports free roam: while the
    /// [`crate::play::PlayMode`] resource is `FreeRoam` it runs the same
    /// world with no goals, timer, lives, fail state or game over. Every
    /// playground's kit must (docs/PLAYGROUNDS.md); a kit that returns false
    /// is played normally when free roam is asked for.
    fn free_roam(&self) -> bool {
        false
    }
    /// List the assets live focus can show on their own (`focus.list`,
    /// docs/LIVE_API.md 6.28). Dev builds only: implement it under the
    /// kit's `live` feature, which turns on `evani-core/focus`.
    #[cfg(feature = "focus")]
    fn focus_assets(&self, _assets: &mut crate::focus::FocusAssets) {}
}

// ---- looks ----------------------------------------------------------------------

/// Linear-RGBA palette.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<[f32; 4]>,
}

/// Minimal material description (evani-render maps it to GPU data).
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialDesc {
    pub name: String,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    /// Linear RGB, HDR allowed (drives bloom).
    pub emissive: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    /// Skip lighting (sprites, UI, toon outlines).
    pub unlit: bool,
}

impl MaterialDesc {
    pub fn new(name: &str, base_color: [f32; 4]) -> Self {
        Self {
            name: name.to_string(),
            base_color,
            emissive: [0.0; 3],
            roughness: 0.6,
            metallic: 0.0,
            unlit: false,
        }
    }
}

/// What a procedural generator is asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct GenRequest {
    pub name: String,
    /// Texture edge length or mesh resolution.
    pub size: u32,
    pub seed: u64,
    pub params: BTreeMap<String, f32>,
}

/// CPU-side mesh data (evani-render uploads it).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeneratedAsset {
    /// RGBA8, sRGB-encoded, row-major.
    Texture {
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    },
    Mesh(MeshData),
    /// Named sprite frames for the renderer's sprite art (`evani-render`
    /// feature `sprite-art`; a look supplies them from its `sprites`
    /// generator). RGBA8, sRGB-encoded, straight alpha, row-major; `frames`
    /// are `(name, [x, y, w, h])` inside the sheet. `pixel_art`: crisp
    /// frames (one mip level, sampled on texel centres).
    SpriteSheet {
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
        frames: Vec<(String, [u32; 4])>,
        pixel_art: bool,
    },
}

/// A procedural asset generator: deterministic given the request seed.
pub type GeneratorFn = fn(&GenRequest, &mut Pcg32) -> GeneratedAsset;

/// Everything the active look provided. World resource.
#[derive(Debug, Clone, Default)]
pub struct LookData {
    pub name: String,
    pub points: u32,
    pub styles: StyleClasses,
    pub palettes: Vec<Palette>,
    pub materials: Vec<MaterialDesc>,
    /// Effects the look wants on/off by default (`(effect, on)`).
    pub post_effects: Vec<(String, bool)>,
    pub generators: Vec<(String, GeneratorFn)>,
}

impl LookData {
    pub fn palette(&self, name: &str) -> Option<&Palette> {
        self.palettes.iter().find(|p| p.name == name)
    }

    pub fn material(&self, name: &str) -> Option<&MaterialDesc> {
        self.materials.iter().find(|m| m.name == name)
    }

    /// Run a generator by name with a seeded RNG (deterministic).
    pub fn generate(&self, req: &GenRequest) -> Option<GeneratedAsset> {
        let (_, f) = self.generators.iter().find(|(n, _)| *n == req.name)?;
        let mut rng = Pcg32::seeded(req.seed);
        Some(f(req, &mut rng))
    }
}

/// Registration surface for looks: everything in [`PluginContext`] plus
/// look-specific methods.
pub struct LookContext<'a, 'b> {
    pub ctx: &'b mut PluginContext<'a>,
}

impl<'a> core::ops::Deref for LookContext<'a, '_> {
    type Target = PluginContext<'a>;
    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

impl core::ops::DerefMut for LookContext<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx
    }
}

impl LookContext<'_, '_> {
    pub fn palette(&mut self, name: &str, colors: &[[f32; 4]]) {
        self.ctx.world.res_mut::<LookData>().palettes.push(Palette {
            name: name.to_string(),
            colors: colors.to_vec(),
        });
    }

    pub fn material(&mut self, m: MaterialDesc) {
        self.ctx.world.res_mut::<LookData>().materials.push(m);
    }

    /// Provide this look's body for a shader slot (e.g. `tonemap`).
    pub fn shader_slot(&mut self, slot: &str, body: &str) -> Result<(), SlotError> {
        let owner = self.ctx.owner().to_string();
        self.ctx
            .world
            .res_mut::<ShaderSlots>()
            .set_default(slot, body, &owner)
    }

    /// Prefer an effect on or off (if compiled in; the tier budget still
    /// decides).
    pub fn post_effect(&mut self, effect: &str, on: bool) {
        self.ctx
            .world
            .res_mut::<LookData>()
            .post_effects
            .push((effect.to_string(), on));
    }

    /// Override another module's param default (e.g. `fx.bloom.threshold`).
    /// Applied at start; ignored with a note if that param is compiled out.
    pub fn post_param(&mut self, name: &str, value: Value) {
        let owner = self.ctx.owner().to_string();
        self.ctx
            .world
            .res_mut::<PendingBaked>()
            .overrides
            .push((owner, name.to_string(), value));
    }

    pub fn generator(&mut self, name: &str, f: GeneratorFn) {
        self.ctx
            .world
            .res_mut::<LookData>()
            .generators
            .push((name.to_string(), f));
    }
}

/// An art style (toon, pixel16, photoreal...).
pub trait LookPlugin: 'static {
    /// Short id, e.g. `basic` (crate `evani-look-basic`).
    fn name(&self) -> &'static str;
    /// Budget points the look itself costs on every tier.
    fn points(&self) -> u32 {
        0
    }
    /// Which style classes it renders (decides which effects apply).
    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }
    fn build(&self, look: &mut LookContext) -> Result<(), PluginError>;
    /// The look crate's version, as `Some(env!("CARGO_PKG_VERSION"))` in the
    /// look's own crate (see [`KitPlugin::version`]).
    fn version(&self) -> Option<&'static str> {
        None
    }
}

// ---- engine modules ----------------------------------------------------------------

/// An optional engine module. Its crate is only linked when the game's
/// evani.toml lists it, so a module that is off is truly absent.
pub trait EngineModule: 'static {
    /// Stable id, `evani.module.<name>` (xtask scans builds for it).
    fn id(&self) -> &'static str;
    /// Short label for the overlay (`fx`, `physics`...).
    fn label(&self) -> &'static str;
    fn build(&mut self, _ctx: &mut PluginContext) -> Result<(), PluginError> {
        Ok(())
    }
    /// After all plugins are built and baked values applied.
    fn start(&mut self, _world: &mut World) {}
    /// Once per rendered frame after the simulation (renderers draw here,
    /// using `Time::alpha` to interpolate).
    fn frame(&mut self, _world: &mut World) {}
    /// Live code swapping (dev builds only): `frame` called through
    /// subsecond's jump table, so a patched module crate draws with its new
    /// code from the next frame. Never override this.
    #[cfg(feature = "hotpatch")]
    #[doc(hidden)]
    fn __evani_hot_frame(&mut self, world: &mut World) {
        let f: fn(&mut Self, &mut World) = <Self as EngineModule>::frame;
        subsecond::HotFn::current(f).call((self, world));
    }
    fn visibility_changed(&mut self, _world: &mut World, _hidden: bool) {}
    /// Append one status line (no newline) for the overlay. Called a few
    /// times per second into a reused buffer.
    fn status(&self, _world: &World, out: &mut String) {
        out.push_str("on");
    }
    /// The module crate's version, as `Some(env!("CARGO_PKG_VERSION"))` in
    /// the module's own crate. `None` (the default) marks a game-local
    /// module (a debug panel, a game's UI), which is not an Evani.Dev part
    /// and is left out of the versions line.
    fn version(&self) -> Option<&'static str> {
        None
    }
    /// What this build of the module compiled in, when that varies per game
    /// (fx: its effects). Read once, when the module is added.
    fn version_features(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// The wire protocol the module speaks, if any (live: `evani-live/1`).
    fn protocol(&self) -> Option<&'static str> {
        None
    }
    /// `true` for a module that draws the picture in its `frame` (the render
    /// module). [`crate::App`] runs every other module's `frame` first, in
    /// the order they were added, then the drawing modules' (in theirs), so
    /// whatever a module sets in its frame step (a camera copy, a uniform,
    /// a draw) reaches the frame drawn the same frame, wherever the game
    /// added it. Default `false`: a module's `frame` updates state only.
    /// Add the render module first as before: the order modules are built,
    /// started and listed in is unchanged.
    fn draws(&self) -> bool {
        false
    }
}
