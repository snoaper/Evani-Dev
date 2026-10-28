//! Evani.Dev renderer (always included).
//!
//! * [`RenderGraph`]: targets and passes; effects add theirs, disabled passes
//!   are culled.
//! * [`RenderWorld`]: what kits fill each frame (camera, mesh instances,
//!   sprites); the renderer draws and clears it.
//! * Engine WGSL with shader slots spliced in ([`splice`]), compiled in GPU
//!   error scopes; slot bodies can be rebuilt at runtime through
//!   `evani_core::slots::SlotRebuilds` (the old pipeline keeps rendering on
//!   any error).
//! * Tier render scale and DPR cap, the first-run tier warm-up and the
//!   adaptive-quality controller ([`control`]).
//!
//! WebGPU only: wgpu is a wasm-only dependency built with just the `webgpu`
//! backend. Native builds (tests, tools) have no GPU path at all; there the
//! module only counts what would be drawn.

#[cfg(feature = "sprite-art")]
pub mod art;
pub mod atlas;
pub mod control; #[cfg(feature = "frustum-cull")] pub mod cull;
pub mod depth;
pub mod graph; #[cfg(feature = "lights2d")] pub mod lights2d;
pub mod math;
pub mod mesh;
#[cfg(feature = "particles")]
pub mod particles;
pub mod place;
pub mod post;
pub mod render_path;
pub mod shaders;
pub mod shadow;
pub mod splice;
pub mod spline;
#[cfg(feature = "text")] mod bitmap; #[cfg(any(feature = "text", feature = "msdf"))] // two lines, like before the split: later line numbers (panic locations) stay
pub mod text; #[cfg(feature = "msdf")] pub mod msdf;
pub mod world; #[cfg(feature = "point-lights")] pub mod light; #[cfg(feature = "kit-material")] pub mod kit_material; #[cfg(feature = "view-model")] pub mod view_model; #[cfg(any(feature = "shadow-cascades", feature = "pcss"))] mod shadow_ext; #[cfg(feature = "ambient")] mod ambient; #[cfg(feature = "pseudo3d")] pub mod pseudo3d; #[cfg(feature = "camera-range")] pub mod camera_range; #[cfg(any(feature = "hdr-color", feature = "fog-color"))] pub mod color_ext; #[cfg(feature = "prewarm")] pub mod prewarm; #[cfg(feature = "shadow-fit")] pub mod shadow_fit;

#[cfg(target_arch = "wasm32")]
mod gpu;

use core::fmt::Write;

use evani_core::ecs::World;
use evani_core::overlay::{ErrorLog, RenderStats};
use evani_core::params::{BoolParam, F32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use evani_core::quality::AqParams;

pub use graph::{GraphError, PassDesc, RenderGraph, TargetDesc, TargetKind};
pub use mesh::{MeshId, MeshLibrary};
pub use post::{Blend, EffectShader, PostEffect, PostEffects, PostRecorder, UniformFrame, put_words};
pub use render_path::{PathKind, RenderPathInfo, render_path};
pub use world::{MeshInstance, RenderWorld, SHEET_CELL0, Sprite, SpriteSheet, View};
#[cfg(feature = "sprite-art")]
pub use art::{ART_CELL0, ArtSheet, ArtSource, SpriteArt, SpriteEdge}; #[cfg(feature = "lights2d")] pub use lights2d::{Light2d, Lights2d};

/// Title and body of the message shown when WebGPU is missing.
pub const NO_WEBGPU_TITLE: &str = "This game needs WebGPU";
pub const NO_WEBGPU_BODY: &str = "Your browser or device doesn't offer WebGPU yet. \
    Try a recent Chrome, Edge or Safari, or update your graphics drivers.";

/// Title and body shown instead when the page is not on a secure address
/// (browsers hide `navigator.gpu` on plain http except localhost).
pub const INSECURE_TITLE: &str = "This page needs a secure address";
pub const INSECURE_BODY: &str = "Your browser can only use WebGPU on https or localhost addresses.     Open this game from its https link, or use the cable link (localhost) when testing on your phone.";

/// Why the game cannot start on the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoWebgpuReason {
    /// WebGPU is there and an adapter was found (or is still being asked for).
    Available,
    /// The page is not a secure context, so the browser hides WebGPU.
    InsecureContext,
    /// Secure page, but no `navigator.gpu` or no adapter: the device cannot.
    Unsupported,
}

/// Decide the reason from what the browser reports. Pure, so it is tested natively.
pub fn no_webgpu_reason(secure: bool, has_gpu: bool, has_adapter: bool) -> NoWebgpuReason {
    if has_gpu && has_adapter {
        NoWebgpuReason::Available
    } else if !secure {
        NoWebgpuReason::InsecureContext
    } else {
        NoWebgpuReason::Unsupported
    }
}

/// The (title, body) to show for a reason; `None` when WebGPU is available.
pub fn no_webgpu_message(reason: NoWebgpuReason) -> Option<(&'static str, &'static str)> {
    match reason {
        NoWebgpuReason::Available => None,
        NoWebgpuReason::InsecureContext => Some((INSECURE_TITLE, INSECURE_BODY)),
        NoWebgpuReason::Unsupported => Some((NO_WEBGPU_TITLE, NO_WEBGPU_BODY)),
    }
}

/// `window.isSecureContext`; true when it cannot be read.
#[cfg(target_arch = "wasm32")]
pub(crate) fn secure_context() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w, &wasm_bindgen::JsValue::from_str("isSecureContext")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Is WebGPU exposed by the browser? Always false natively.
pub fn webgpu_available() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        evani_core::web::webgpu_exposed()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// The GPU name for the overlay from the browser's `GPUAdapterInfo`: its
/// `description` when there is one (Chrome fills it only with
/// `--enable-webgpu-developer-features`), else what it gives of vendor,
/// architecture and device ("nvidia lovelace"). Empty when the browser
/// gives nothing; the overlay then says "(adapter)".
pub fn adapter_label(description: &str, vendor: &str, architecture: &str, device: &str) -> String {
    let description = description.trim();
    if !description.is_empty() {
        return description.to_string();
    }
    let mut out = String::new();
    for part in [vendor, architecture, device].map(str::trim) {
        if !part.is_empty() && !out.split(' ').any(|w| w == part) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(part);
        }
    }
    out
}

/// Where adapter setup stands.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GpuStatus {
    #[default]
    NotStarted,
    Pending,
    Ready {
        adapter: String,
    },
    Unavailable(String),
    /// Native/headless run: nothing to draw on.
    Headless,
}

/// Handles of the renderer's registered tunables.
#[derive(Debug, Clone, Copy)]
pub struct RenderParams {
    pub exposure: F32Param,
    pub clear_color: Vec3Param,
    pub saturation: F32Param,
    pub ambient: Vec3Param,
    pub light_direction: Vec3Param,
    pub light_color: Vec3Param,
    pub light_intensity: F32Param,
    /// Shadow tint: the colour (linear RGB) shadows lean towards, and how far
    /// (0 = off, the default).
    pub shadow_tint: Vec3Param,
    pub shadow_tint_strength: F32Param,
    /// Crisp pixel art: nearest atlas sampling, exact sheet frame edges.
    pub pixel_art: BoolParam,
    /// The adaptive-quality policy (`aq.*`).
    pub aq: AqParams,
}

impl RenderParams {
    /// Light and background of this frame.
    pub fn lighting(&self, reg: &ParamRegistry) -> shaders::Lighting {
        shaders::Lighting {
            light_dir: reg.vec3(self.light_direction),
            light_color: reg.vec3(self.light_color),
            intensity: reg.f32(self.light_intensity),
            ambient: reg.vec3(self.ambient),
            fog_color: reg.vec3(self.clear_color),
        }
    }
}

/// Count what `world` would draw (draw calls, triangles, sprites) into
/// `stats`, the way the GPU path does: one draw per mesh kind in use per
/// view (split-screen draws the instances once per view) and one for all
/// sprites.
pub fn count_draws(world: &RenderWorld, stats: &mut RenderStats) {
    count_draws_with(world, None, stats);
}

/// [`count_draws`] with the custom meshes of `lib` (feature `meshes`; without
/// it the renderer drops custom-mesh instances, and so does the count).
/// Decals join their mesh kind's draw.
pub fn count_draws_with(world: &RenderWorld, lib: Option<&MeshLibrary>, stats: &mut RenderStats) {
    let mut draws = 0;
    let mut tris = 0u32;
    for id in 0..mesh::MESH_SLOTS as u16 {
        let n = world.meshes.iter().chain(&world.decals).filter(|m| m.mesh.0 == id).count() as u32;
        if n > 0 {
            let per = match lib.and_then(|l| l.get(MeshId(id))) {
                Some(m) => m.triangles(),
                None => mesh::triangles_of(MeshId(id)),
            };
            draws += 1;
            tris = tris.saturating_add(n.saturating_mul(per));
        }
    }
    let views = world.view_count() as u32;
    draws *= views;
    tris = tris.saturating_mul(views);
    if !world.sprites.is_empty() {
        draws += 1;
    }
    stats.draw_calls = draws;
    stats.triangles = tris;
    stats.sprites = world.sprites.len() as u32;
}

/// True when this build has the 3D path (feature `forward`): the forward
/// pass and its WGSL, the depth target, MSAA, meshes, decals and
/// world-space particles. 2D games build without it.
pub const FORWARD_PATH: bool = cfg!(feature = "forward");

/// The error for a frame that asked for 3D draws (mesh instances, decals,
/// world-space particles) in a build without the 3D path, or `None` when
/// `forward_on` or nothing 3D was asked for. The renderer logs it once, so
/// a 3D draw is never dropped silently.
pub fn dropped_3d(forward_on: bool, rw: &RenderWorld, world_particles: bool) -> Option<String> {
    if forward_on {
        return None;
    }
    let count = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    let mut what = Vec::new();
    if !rw.meshes.is_empty() {
        what.push(count(rw.meshes.len(), "mesh instance"));
    }
    if !rw.decals.is_empty() {
        what.push(count(rw.decals.len(), "decal"));
    }
    if world_particles {
        what.push("world-space particles (Space::World3d)".to_string());
    }
    if what.is_empty() {
        return None;
    }
    Some(format!(
        "3D draws dropped ({}): this build has no 3D path. Turn on the renderer's `forward` feature in the kit's Cargo.toml: evani-render = {{ workspace = true, features = [\"forward\"] }}",
        what.join(", ")
    ))
}

/// The render module (`evani.module.render`).
#[derive(Default)]
pub struct RenderModule {
    params: Option<RenderParams>,
    /// The dropped-3D error was logged (once per run).
    warned_3d: bool,
    #[cfg(target_arch = "wasm32")]
    gpu: gpu::Renderer,
    #[cfg(not(target_arch = "wasm32"))]
    status: GpuStatus,
}

impl RenderModule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn params(&self) -> Option<RenderParams> {
        self.params
    }

    pub fn status(&self) -> GpuStatus {
        #[cfg(target_arch = "wasm32")]
        {
            self.gpu.status()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.status.clone()
        }
    }
}

impl EngineModule for RenderModule {
    fn id(&self) -> &'static str {
        "evani.module.render"
    }

    fn label(&self) -> &'static str {
        "render"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        ctx.resource(RenderGraph::standard());
        ctx.resource(RenderWorld::default()); #[cfg(feature = "models")] ctx.resource(model::Models::default()); #[cfg(feature = "relief")] ctx.world.res_mut::<evani_core::slots::ShaderSlots>().declare(evani_core::slots::MICRO_DETAIL); #[cfg(all(feature = "look-surface", not(feature = "relief")))] { let mut s = ctx.world.res_mut::<evani_core::slots::ShaderSlots>(); s.declare(model::surface::SPECULAR); s.declare(model::surface::SURFACE_PARAMS); } #[cfg(feature = "kit-material")] ctx.resource(kit_material::KitMaterial::default()); #[cfg(feature = "pseudo3d")] pseudo3d::register(ctx)?; #[cfg(all(feature = "material-layers", not(feature = "relief")))] evani_material_layers::register(ctx)?; #[cfg(feature = "char-materials")] evani_material_character::install(ctx)?;
        ctx.resource(PostEffects::default()); #[cfg(feature = "pcss")] ctx.world.res_mut::<evani_core::slots::ShaderSlots>().declare(shadow_ext::PCSS_PARAMS); #[cfg(feature = "shadow-cascades")] shadow_ext::register(ctx)?; #[cfg(feature = "ambient")] ambient::register(ctx)?; #[cfg(feature = "frustum-cull")] ctx.resource(cull::CullStats::default()); #[cfg(feature = "camera-range")] camera_range::register(ctx)?; #[cfg(feature = "shadow-fit")] shadow_fit::register(ctx)?;
        // The shared particle system drains the core queue from now on.
        #[cfg(feature = "particles")]
        {
            ctx.resource(particles::ParticleSystem::new());
            ctx.world.res_mut::<evani_core::particles::ParticleQueue>().attach();
        }
        #[cfg(feature = "sprite-art")]
        {
            #[allow(unused_mut)]
            let mut sprite_art = art::SpriteArt::default();
            // The engine's sheets go in first, so their cells never move.
            #[cfg(feature = "text")]
            sprite_art
                .add(art::ArtSource::Engine, text::font_sheet())
                .map_err(|e| PluginError(format!("{}: {e}", text::FONT_ID)))?;
            ctx.resource(sprite_art);
        } #[cfg(feature = "lights2d")] ctx.resource(lights2d::Lights2d::default()); #[cfg(all(feature = "lights2d", feature = "msdf"))] if let Some(mut log) = ctx.world.try_res_mut::<ErrorLog>() { log.error("lights2d: not drawn in a game with the ui module (sharp text) yet"); }
        let exposure = ctx.param_f32(
            ParamDef::f32("render.exposure", 1.0)
                .range(0.0, 8.0)
                .step(0.01)
                .unit("x")
                .describe("Linear exposure applied before the tonemap slot")
                .gpu(),
        )?;
        let clear_color = ctx.param_vec3(
            ParamDef::color("render.clear_color", [0.02, 0.025, 0.035])
                .describe("Background and fog colour (linear RGB)")
                .gpu(),
        )?; #[cfg(any(feature = "hdr-color", feature = "fog-color"))] color_ext::register(ctx)?;
        let saturation = ctx.param_f32(
            ParamDef::f32("render.saturation", 1.0)
                .range(0.0, 2.0)
                .step(0.01)
                .unit("x")
                .describe("Colour saturation after the tonemap slot (1 = unchanged)")
                .gpu(),
        )?;
        let ambient = ctx.param_vec3(
            ParamDef::color("render.ambient", [0.08, 0.09, 0.12])
                .describe("Ambient light added to every lit surface (linear RGB)")
                .gpu(),
        )?;
        let light_direction = ctx.param_vec3(
            ParamDef::vec3("render.light.direction", [0.4, 1.0, 0.3])
                .range(-1.0, 1.0)
                .step(0.01)
                .describe("Direction towards the directional light (normalised on the GPU side)")
                .gpu(),
        )?;
        let light_color = ctx.param_vec3(
            ParamDef::color("render.light.color", [1.0, 0.96, 0.9])
                .describe("Directional light colour (linear RGB)")
                .gpu(),
        )?;
        let light_intensity = ctx.param_f32(
            ParamDef::f32("render.light.intensity", 2.5)
                .range(0.0, 20.0)
                .step(0.01)
                .unit("x")
                .describe("Directional light intensity")
                .gpu(),
        )?;
        let shadow_tint = ctx.param_vec3(
            ParamDef::color("render.shadow_tint", [1.0, 1.0, 1.0])
                .describe("Colour shadows lean towards (linear RGB; shadowed parts keep their brightness and take this hue). Only matters with a strength above 0")
                .gpu(),
        )?;
        let shadow_tint_strength = ctx.param_f32(
            ParamDef::f32("render.shadow_tint_strength", 0.0)
                .range(0.0, 1.0)
                .step(0.01)
                .describe("How far shadows (shadow map and blob shadows) lean towards the shadow tint (0 = not at all, the default)")
                .gpu(),
        )?;
        let pixel_art = ctx.param_bool(
            ParamDef::bool("render.pixel_art", false)
                .describe("Crisp pixel art: sample the sprite atlas with nearest filtering and exact frame edges (pixel looks turn it on)")
                .gpu(),
        )?;
        let aq = AqParams::register(&mut ctx.world.res_mut::<ParamRegistry>())?;
        self.params = Some(RenderParams {
            exposure,
            clear_color,
            saturation,
            ambient,
            light_direction,
            light_color,
            light_intensity,
            shadow_tint,
            shadow_tint_strength,
            pixel_art,
            aq,
        });
        Ok(())
    }

    fn start(&mut self, world: &mut World) {
        let compiled = world.res_mut::<RenderGraph>().compile();
        if let Err(e) = compiled {
            world.res_mut::<ErrorLog>().error(format!("render graph: {e}"));
        }
        // The look's sprite art (its `sprites` generator): after every plugin
        // is built and before the first frame, so kits can resolve names.
        #[cfg(feature = "sprite-art")]
        {
            let look = world.res::<evani_core::plugin::LookData>();
            if let Err(e) = world.res_mut::<art::SpriteArt>().add_look(&look) {
                world.res_mut::<ErrorLog>().error(e);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.status = GpuStatus::Headless;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.gpu.start(world, self.params);
        }
    }

    fn frame(&mut self, world: &mut World) { #[cfg(feature = "importance")] importance::drive(world);
        #[cfg(feature = "particles")]
        particles::ParticleSystem::frame(world);
        // A build without the 3D path reports a 3D draw once instead of
        // dropping it silently (compiled out with `forward`).
        if !FORWARD_PATH && !self.warned_3d {
            #[cfg(feature = "particles")]
            let world_particles = world
                .try_res::<particles::ParticleSystem>()
                .is_some_and(|ps| ps.groups().iter().any(|g| g.key == particles::KEY_WORLD));
            #[cfg(not(feature = "particles"))]
            let world_particles = false;
            let dropped = dropped_3d(FORWARD_PATH, &world.res::<RenderWorld>(), world_particles);
            if let Some(msg) = dropped {
                world.res_mut::<ErrorLog>().error(msg);
                self.warned_3d = true;
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(p) = self.params {
                self.gpu.frame(world, &p);
            } #[cfg(feature = "models")] if let Some(mut m) = world.try_res_mut::<model::Models>() { m.draws.clear(); }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut rw = world.res_mut::<RenderWorld>();
            let lib = world.try_res::<MeshLibrary>();
            let mut stats = world.res_mut::<RenderStats>();
            count_draws_with(&rw, lib.as_deref(), &mut stats); #[cfg(feature = "models")] if let Some(mut m) = world.try_res_mut::<model::Models>() { let t = world.try_res::<evani_core::tiers::Tiers>().map(|t| t.active().id.clone()); model::count_draws(&mut m, &rw, t.as_deref(), &mut stats); }
            // What the GPU would draw: the live particles, one draw per
            // screen layer and one per view for the world group.
            #[cfg(feature = "particles")]
            if let Some(ps) = world.try_res::<particles::ParticleSystem>() {
                stats.draw_calls += ps.draw_calls(rw.view_count() as u32);
                stats.particles = ps.live();
            }
            rw.clear();
        } #[cfg(feature = "lights2d")] if let Some(mut l) = world.try_res_mut::<lights2d::Lights2d>() { l.clear(); }
    }

    fn visibility_changed(&mut self, _world: &mut World, _hidden: bool) {
        #[cfg(target_arch = "wasm32")]
        self.gpu.visibility(_world, _hidden);
    }

    /// The frame step draws: it runs after every other module's.
    fn draws(&self) -> bool {
        true
    }

    fn status(&self, world: &World, out: &mut String) {
        match self.status() {
            GpuStatus::NotStarted => out.push_str("not started"),
            GpuStatus::Pending => out.push_str("webgpu: requesting adapter"),
            GpuStatus::Ready { adapter } => {
                out.push_str("webgpu ok ");
                out.push_str(if adapter.is_empty() { "(adapter)" } else { &adapter });
            }
            GpuStatus::Unavailable(e) => {
                out.push_str("webgpu unavailable: ");
                out.push_str(&e);
            }
            GpuStatus::Headless => out.push_str("headless (no GPU natively)"),
        }
        #[cfg(target_arch = "wasm32")]
        self.gpu.status_extra(out);
        let passes = world.try_res::<RenderGraph>().map_or(0, |g| g.ordered().count());
        let _ = write!(out, " · passes {passes}"); #[cfg(feature = "frustum-cull")] if let Some(c) = world.try_res::<cull::CullStats>() { let _ = write!(out, " · culled {}/{}", c.culled, c.judged); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    #[test]
    fn no_webgpu_reason_covers_every_case() {
        use NoWebgpuReason::*;
        for secure in [true, false] {
            assert_eq!(no_webgpu_reason(secure, true, true), Available);
        }
        // Plain http on a LAN address: the browser hides navigator.gpu.
        assert_eq!(no_webgpu_reason(false, false, false), InsecureContext);
        assert_eq!(no_webgpu_reason(false, true, false), InsecureContext);
        assert_eq!(no_webgpu_reason(false, false, true), InsecureContext);
        // Secure page without WebGPU or an adapter: the device cannot.
        assert_eq!(no_webgpu_reason(true, false, false), Unsupported);
        assert_eq!(no_webgpu_reason(true, true, false), Unsupported);
        assert_eq!(no_webgpu_reason(true, false, true), Unsupported);
    }

    #[test]
    fn no_webgpu_messages_are_right() {
        use NoWebgpuReason::*;
        assert_eq!(no_webgpu_message(Available), None);
        let (t, b) = no_webgpu_message(InsecureContext).unwrap();
        assert_eq!(t, "This page needs a secure address");
        assert!(b.contains("https or localhost") && !b.contains("Try a recent"));
        let (t, b) = no_webgpu_message(Unsupported).unwrap();
        assert_eq!(t, "This game needs WebGPU");
        assert!(b.contains("Try a recent Chrome"));
    }

    #[test]
    fn adapter_label_uses_what_the_browser_gives() {
        // Chrome with --enable-webgpu-developer-features
        assert_eq!(
            adapter_label("NVIDIA GeForce RTX 4060 Ti", "nvidia", "lovelace", ""),
            "NVIDIA GeForce RTX 4060 Ti"
        );
        // Chrome by default: no description
        assert_eq!(adapter_label("", "nvidia", "lovelace", ""), "nvidia lovelace");
        assert_eq!(adapter_label(" ", "intel", "", "0x46a6"), "intel 0x46a6");
        assert_eq!(adapter_label("", "apple", "apple", ""), "apple", "no repeats");
        // Nothing at all (the overlay says "(adapter)")
        assert_eq!(adapter_label("", "", "", ""), "");
    }

    #[test]
    fn registers_params_and_compiles_graph() {
        let mut app = App::new("t").with_module(RenderModule::new());
        app.run_headless(1, 16.0);
        assert!(app.world.res::<RenderGraph>().is_compiled());
        let reg = app.world.res::<ParamRegistry>();
        for name in [
            "render.exposure",
            "render.saturation",
            "render.light.intensity",
            "render.shadow_tint_strength",
            "aq.target_ms",
        ] {
            assert!(reg.f32_handle(name).is_some(), "{name}");
        }
        for name in ["render.ambient", "render.light.direction", "render.light.color", "render.shadow_tint", "render.pixel_art", "aq.enabled"] {
            assert!(reg.id(name).is_some(), "{name}");
        }
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
    }

    #[test]
    fn shadow_tint_is_off_by_default_and_limited_to_0_1() {
        let mut app = App::new("t").with_module(RenderModule::new());
        app.run_headless(1, 16.0);
        let mut reg = app.world.res_mut::<ParamRegistry>();
        let strength = reg.f32_handle("render.shadow_tint_strength").unwrap();
        // Off unless a look sets it: strength 0, neutral white tint.
        assert_eq!(reg.f32(strength), 0.0);
        let tint = reg.id("render.shadow_tint").unwrap();
        assert_eq!(reg.value(tint), Some(evani_core::params::ParamValue::Color([1.0, 1.0, 1.0])));
        // Live-tunable, and a value above 1 is refused.
        use evani_core::params::ParamValue;
        reg.set_by_name("render.shadow_tint_strength", ParamValue::F32(0.6)).unwrap();
        assert!((reg.f32(strength) - 0.6).abs() < 1e-6);
        let _ = reg.set_by_name("render.shadow_tint_strength", ParamValue::F32(1.5));
        assert!(reg.f32(strength) <= 1.0, "strength above 1 is refused or clamped");
    }

    #[test]
    fn native_frame_counts_and_clears() {
        let mut app = App::new("t").with_module(RenderModule::new());
        app.run_headless(1, 16.0);
        {
            let mut rw = app.world.res_mut::<RenderWorld>();
            for i in 0..5 {
                rw.draw_mesh(MeshInstance::new(MeshId::CUBE, [i as f32, 0.0, 0.0]));
            }
            rw.draw_mesh(MeshInstance::new(MeshId::PLANE, [0.0; 3]));
            rw.draw_sprite(Sprite {
                pos: [10.0, 10.0],
                size: [8.0, 8.0],
                rotation: 0.0,
                cell: 1,
                color: [1.0; 4],
                layer: 0,
            });
        }
        app.run_headless(1, 16.0);
        let s = app.world.res::<RenderStats>().clone();
        assert_eq!(s.draw_calls, 3);
        assert_eq!(s.triangles, 5 * mesh::triangles_of(MeshId::CUBE) + mesh::triangles_of(MeshId::PLANE));
        assert_eq!(s.sprites, 1);
        assert!(app.world.res::<RenderWorld>().meshes.is_empty());
        // Two split-screen views: the instance draws double, sprites don't.
        {
            let mut rw = app.world.res_mut::<RenderWorld>();
            rw.draw_mesh(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
            let cam = rw.camera;
            rw.views.push(View::new(cam, [0.0, 0.0, 1.0, 0.5]));
            rw.views.push(View::new(cam, [0.0, 0.5, 1.0, 0.5]));
        }
        app.run_headless(1, 16.0);
        let s = app.world.res::<RenderStats>().clone();
        assert_eq!(s.draw_calls, 2);
        assert_eq!(s.triangles, 2 * mesh::triangles_of(MeshId::CUBE));
        assert!(app.world.res::<RenderWorld>().views.is_empty());
    }

    #[test]
    fn count_includes_decals_and_custom_meshes() {
        let mut w = RenderWorld::default();
        w.draw_mesh(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
        w.draw_decal(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
        w.draw_decal(MeshInstance::new(MeshId::PLANE, [0.0; 3]));
        let mut s = RenderStats::default();
        count_draws(&w, &mut s);
        assert_eq!(s.draw_calls, 2);
        assert_eq!(s.triangles, 2 * mesh::triangles_of(MeshId::CUBE) + mesh::triangles_of(MeshId::PLANE));
        // A custom mesh draws (and counts) only with the `meshes` feature.
        let mut lib = MeshLibrary::default();
        let id = lib.add(mesh::cube()).unwrap();
        w.draw_mesh(MeshInstance::new(id, [0.0; 3]));
        count_draws_with(&w, Some(&lib), &mut s);
        let custom = u32::from(cfg!(feature = "meshes"));
        assert_eq!(s.draw_calls, 2 + custom);
        assert_eq!(s.triangles, (2 + custom) * mesh::triangles_of(MeshId::CUBE) + mesh::triangles_of(MeshId::PLANE));
    }

    /// The guard's message, in both builds: nothing with the 3D path or
    /// with nothing 3D asked for; otherwise what was dropped and the fix.
    #[test]
    fn dropped_3d_names_what_and_the_fix() {
        let mut rw = RenderWorld::default();
        assert_eq!(dropped_3d(false, &rw, false), None, "a sprites-only frame drops nothing");
        rw.draw_mesh(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
        rw.draw_decal(MeshInstance::new(MeshId::PLANE, [0.0; 3]));
        assert_eq!(dropped_3d(true, &rw, true), None, "the 3D path draws them");
        let msg = dropped_3d(false, &rw, true).unwrap();
        assert!(!msg.contains("1 mesh instances") && !msg.contains("1 decals"), "{msg}");
        rw.draw_decal(MeshInstance::new(MeshId::PLANE, [1.0; 3]));
        assert!(dropped_3d(false, &rw, false).unwrap().contains("2 decals"));
        for part in ["1 mesh instance", "1 decal", "Space::World3d", "features = [\"forward\"]", "Cargo.toml"] {
            assert!(msg.contains(part), "{part}: {msg}");
        }
        let only_particles = dropped_3d(false, &RenderWorld::default(), true).unwrap();
        assert!(only_particles.contains("world-space particles") && !only_particles.contains("mesh"));
    }

    /// A 3D draw in this build: reported once in the ErrorLog without the
    /// 3D path (`cargo test -p evani-render` builds it so), nothing with it
    /// (`cargo test --workspace` turns `forward` on through the 3D kits).
    #[test]
    fn a_3d_draw_without_the_forward_path_is_reported_once() {
        let mut app = App::new("t").with_module(RenderModule::new());
        app.run_headless(1, 16.0);
        for _ in 0..3 {
            app.world.res_mut::<RenderWorld>().draw_mesh(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
            app.run_headless(1, 16.0);
        }
        let log = app.world.res::<ErrorLog>();
        let dropped: Vec<_> = log.entries().filter(|e| e.message.starts_with("3D draws dropped")).collect();
        assert_eq!(dropped.len(), usize::from(!FORWARD_PATH), "{:?}", dropped.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(log.error_count(), u32::from(!FORWARD_PATH));
    }
}

// Textured models (feature `models`, S3-3). Declared after everything else
// in this file: panic locations (file and line) are in the wasm, so other
// games' downloads stay byte-identical only if no line above moves.
#[cfg(feature = "models")]
pub mod texture;
#[cfg(feature = "models")]
pub mod model; #[cfg(feature = "importance")] pub mod importance;
#[cfg(feature = "kit-surface")]
pub mod kit_surface;
#[cfg(test)]
mod frame_order_tests;
