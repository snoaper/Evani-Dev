//! evani-material-nature: nature and see-through materials for 3D games,
//! drawn as one depth-reading pass over analytic surfaces. No engine,
//! evani-fx or existing shader changes: it plugs into evani-render's public
//! post-effect interface and shader slots.
//!
//! * **Foliage and grass** glow when the sun is behind them; crowns get leaf
//!   cluster variation and grass a colour per blade cell.
//! * **Thin back-light**: ears, fruit and wax ([`Kind::ThinBlob`]), leaves and
//!   paper ([`Kind::ThinPatch`]), lampshades and candles lit from inside
//!   ([`Kind::Lamp`]).
//! * **Water**: code-made waves, Schlick Fresnel sky reflection, a GGX sun
//!   glint, a Beer-Lambert depth fade and shoreline foam.
//! * **Glass**: window panes and bottles with Fresnel, tint by the path
//!   through the glass and (High, Ultra) a bent view through bottles.
//!
//! A surface is picked by glTF material extensions or an `evani_kind` tag
//! (the glTF reader), by kit data ([`surfaces_from_json`]) or in code
//! ([`NatureSurfaces`]). Water and glass are left out of the game's solid
//! draw (the pass draws them); foliage, grass and thin surfaces stay solid
//! and the pass adds their glow.
//!
//! Every value is a live param (`material.nature.*`, [`params`]). Quality
//! is tiered ([`effect::Mode`]). All noise comes from integer hashes, and
//! the waves move with sim time, so a frame is the same on every run.
//!
//! Turning it on: list the crate as an optional dependency behind a game
//! feature and add [`NatureModule`] after the render module. A game whose
//! evani-render has `camera-range` on also turns on this crate's
//! `camera-range`.
//!
//! Credits: the back-light term is Filament's subsurface model (Apache-2.0),
//! the thickness attenuation follows Babylon.js's thin translucency
//! (Apache-2.0); see THIRD_PARTY_NOTICES.md.

pub mod effect;
pub mod gltf;
pub mod params;
pub mod surface;
pub mod water;
#[cfg(feature = "spectral-sea")]
pub mod sea;

#[cfg(test)]
mod tests;

use core::fmt::Write as _;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use evani_core::slots::{ShaderSlots, SlotRebuilds};
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_render::RenderWorld;
use evani_render::graph::{PassDesc, RenderGraph, TargetDesc, TargetKind};
use evani_render::post::{PostEffect, PostEffects};

pub use effect::{Mode, NatureEffect};
pub use gltf::GltfNature;
pub use params::NatureParams;
pub use surface::{Kind, NatureSurfaces, Surface, surfaces_from_json};
#[cfg(feature = "spectral-sea")]
pub use sea::{Sea, SeaEffect, SeaParams, SeaSurfaces};

/// Most surfaces each tier draws.
pub fn tier_cap(id: &str) -> usize {
    match id {
        "lite" => 10,
        "low" => 20,
        "medium" | "vr" => 40,
        "high" => 80,
        "ultra" => 120,
        _ => 40,
    }
}

/// The module (`evani.module.material_nature`).
#[derive(Default)]
pub struct NatureModule {
    surfaces: NatureSurfaces,
    params: Option<NatureParams>,
    light: Option<(ParamId, ParamId, ParamId)>,
    /// `render.clear_color`: the background the reflected sky matches.
    clear: Option<ParamId>,
    /// The slot body last handed to the renderer.
    sent: Option<String>,
    seen_rev: Option<u64>,
    pass_on: Option<bool>,
    /// The spectral sea (part `spectral-sea`): its resource, params and
    /// whether its pass is on.
    #[cfg(feature = "spectral-sea")]
    sea: sea::SeaSurfaces,
    #[cfg(feature = "spectral-sea")]
    sea_params: Option<sea::SeaParams>,
    #[cfg(feature = "spectral-sea")]
    sea_on: Option<bool>,
}

impl NatureModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared surface list (also a world resource after `build`).
    pub fn surfaces(&self) -> NatureSurfaces {
        self.surfaces.clone()
    }

    /// The shared sea (part `spectral-sea`; also a world resource after
    /// `build`).
    #[cfg(feature = "spectral-sea")]
    pub fn sea(&self) -> sea::SeaSurfaces {
        self.sea.clone()
    }

    fn sync_slot(&mut self, world: &World) {
        let Some(p) = &self.params else {
            return;
        };
        let reg = world.res::<ParamRegistry>();
        if self.seen_rev == Some(reg.revision()) {
            return;
        }
        let want = params::body(&p.table(&reg));
        drop(reg);
        if self.sent.as_deref() == Some(want.as_str()) {
            self.seen_rev = Some(world.res::<ParamRegistry>().revision());
            return;
        }
        let queued = match world.try_res_mut::<SlotRebuilds>() {
            Some(mut rb) => {
                if rb.has_pending() {
                    return;
                }
                let slots = world.res::<ShaderSlots>();
                rb.request_detached(&slots, params::SLOT.name, &want, evani_core::time::now_ms())
                    .is_ok()
            }
            None => {
                let mut slots = world.res_mut::<ShaderSlots>();
                slots
                    .id(params::SLOT.name)
                    .is_some_and(|id| slots.set_body(id, &want).is_ok())
            }
        };
        if queued {
            self.sent = Some(want);
            self.seen_rev = Some(world.res::<ParamRegistry>().revision());
        }
    }
}

fn vec3_of(reg: &ParamRegistry, id: ParamId) -> Option<[f32; 3]> {
    match reg.value(id)? {
        ParamValue::Vec3(v) | ParamValue::Color(v) => Some(v),
        _ => None,
    }
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Largest per-channel scale [`pack_sky_tint`] keeps.
pub const SKY_TINT_MAX: f32 = 4.0;

/// Per-channel scale of the reflected sky: `sky.horizon` moved towards the
/// game's background colour (`render.clear_color`) by
/// `sky.match_background` (table row 0, w), so water and glass reflect the
/// sky the player sees, in colour and brightness. The zenith takes the
/// same scale. A background without hue keeps the horizon's hue and only
/// matches the brightness.
pub fn sky_tint(table: &[[f32; 4]; params::ROWS], background: Option<[f32; 3]>) -> [f32; 3] {
    let [hr, hg, hb, k] = table[0];
    let (Some(bg), true) = (background, k > 0.0) else {
        return [1.0; 3];
    };
    let k = k.min(1.0);
    let hor = [hr, hg, hb];
    let grey = (bg[0] - bg[1]).abs().max((bg[1] - bg[2]).abs()) < 1e-3;
    let mut out = [1.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let ratio = if grey {
            luma(bg).max(0.0) / luma(hor).max(1e-3)
        } else {
            bg[i].max(0.0) / hor[i].max(1e-3)
        };
        *o = (1.0 + (ratio - 1.0) * k).clamp(0.0, SKY_TINT_MAX);
    }
    out
}

/// [`sky_tint`] as one uniform float: three 8-bit steps of
/// `SKY_TINT_MAX / 255` packed as `r + 256 g + 65536 b`, an integer below
/// 2^24 and so exact in an f32 (the shader's `sky_k` unpacks it).
pub fn pack_sky_tint(t: [f32; 3]) -> f32 {
    let q = |v: f32| (v.clamp(0.0, SKY_TINT_MAX) / SKY_TINT_MAX * 255.0).round() as u32;
    (q(t[0]) + 256 * q(t[1]) + 65536 * q(t[2])) as f32
}

impl EngineModule for NatureModule {
    fn id(&self) -> &'static str {
        "evani.module.material_nature"
    }

    fn label(&self) -> &'static str {
        "nature"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = NatureParams::register(&mut ctx.world.res_mut::<ParamRegistry>())
            .map_err(|e| PluginError(format!("material.nature: {e}")))?;
        let body = params::body(&p.table(&ctx.world.res::<ParamRegistry>()));
        {
            let mut slots = ctx.world.res_mut::<ShaderSlots>();
            slots.declare(params::SLOT);
            slots
                .set_default(params::SLOT.name, &body, "material.nature")
                .map_err(|e| PluginError(format!("material.nature: {e}")))?;
        }
        self.sent = Some(body);
        {
            let reg = ctx.world.res::<ParamRegistry>();
            self.light = match (
                reg.id("render.light.direction"),
                reg.id("render.light.color"),
                reg.id("render.light.intensity"),
            ) {
                (Some(d), Some(c), Some(i)) => Some((d, c, i)),
                _ => None,
            };
            self.clear = reg.id("render.clear_color");
        }
        self.params = Some(p);
        let effect: Rc<dyn PostEffect> = Rc::new(NatureEffect {
            surfaces: self.surfaces.clone(),
        });
        match ctx.world.try_res_mut::<PostEffects>() {
            Some(mut pe) => pe.effects.push(effect),
            None => {
                return Err(PluginError(
                    "material.nature: no post-effect list (add the render module first)".into(),
                ));
            }
        }
        {
            let Some(mut g) = ctx.world.try_res_mut::<RenderGraph>() else {
                return Err(PluginError(
                    "material.nature: no render graph (add the render module first)".into(),
                ));
            };
            if g.target("depth").is_none() {
                return Err(PluginError(
                    "material.nature: no depth target (turn on evani-render's `forward` feature)".into(),
                ));
            }
            let err = |e: evani_render::graph::GraphError| PluginError(format!("material.nature: {e}"));
            g.add_target(TargetDesc::new(effect::SCENE, TargetKind::HdrColor).transient())
                .map_err(err)?;
            g.add_pass(
                PassDesc::new(effect::PASS, "material.nature")
                    .reads(&["hdr", "depth"])
                    .writes(&["hdr", effect::SCENE])
                    .uses_slots(&[params::SLOT.name])
                    .order(4),
            )
            .map_err(err)?;
            // Off until there is something to draw.
            g.set_enabled(effect::PASS, false);
        }
        self.pass_on = Some(false);
        ctx.resource(self.surfaces.clone());
        #[cfg(feature = "spectral-sea")]
        self.build_sea(ctx)?;
        Ok(())
    }

    fn frame(&mut self, world: &mut World) {
        let Some(p) = self.params.clone() else {
            return;
        };
        let st = &self.surfaces.0;
        let (on, mode, cap) = {
            let reg = world.res::<ParamRegistry>();
            let (tier, tcap) = world
                .try_res::<Tiers>()
                .map_or(("medium".to_string(), 40), |t| {
                    let id = t.active().id.clone();
                    let c = tier_cap(&id);
                    (id, c)
                });
            let refraction = reg.bool(p.refraction);
            let level = reg.f32(p.quality).round() as i32;
            let mode = if level >= 0 { Mode::for_quality(level, refraction) } else { Mode::for_tier(&tier, refraction) };
            if let Some((d, c, i)) = self.light {
                let dir = vec3_of(&reg, d).unwrap_or([0.4, 1.0, 0.3]);
                let col = vec3_of(&reg, c).unwrap_or([1.0; 3]);
                let k = match reg.value(i) {
                    Some(ParamValue::F32(k)) => k,
                    _ => 1.0,
                };
                st.sun.set(effect::norm(dir));
                st.sun_col.set(col.map(|v| v * k));
            }
            st.sky_tint.set(pack_sky_tint(sky_tint(&p.table(&reg), self.clear.and_then(|c| vec3_of(&reg, c)))));
            let cap = (reg.i32(p.max_surfaces).max(1) as usize).min(tcap);
            (reg.bool(p.enabled), mode, cap)
        };
        if let Some(rw) = world.try_res::<RenderWorld>() {
            // A capture from its own pose (a debug view, a camera path) swaps
            // the camera only when the frame is drawn: plan from that pose.
            #[cfg(feature = "capture")]
            let pose = world.try_res::<evani_core::capture::CaptureQueue>().and_then(|q| q.view_pose()).unwrap_or(rw.camera);
            #[cfg(not(feature = "capture"))]
            let pose = rw.camera;
            st.camera.set(pose);
        }
        #[cfg(feature = "capture")]
        st.jitter.set(world.try_res::<evani_core::capture::CaptureQueue>().and_then(|q| q.jitter_px).unwrap_or([0.0; 2]));
        if let Some(t) = world.try_res::<Time>() {
            st.time.set(t.render_seconds() as f32);
        }
        self.surfaces.plan(cap);
        let draw = on && self.surfaces.drawn() > 0;
        st.mode.set(if draw { mode } else { Mode::Off });
        if self.pass_on != Some(draw) {
            world.res_mut::<RenderGraph>().set_enabled(effect::PASS, draw);
            self.pass_on = Some(draw);
        }
        self.sync_slot(world);
        #[cfg(feature = "spectral-sea")]
        self.frame_sea(world);
    }

    fn status(&self, _world: &World, out: &mut String) {
        let m = match self.surfaces.mode() {
            Mode::Off => "off",
            Mode::Flat => "flat",
            Mode::Split => "split",
            Mode::Refract => "refract",
        };
        let _ = write!(out, "{m} · {} surfaces", self.surfaces.drawn());
    }
}

#[cfg(feature = "spectral-sea")]
impl NatureModule {
    /// The sea's params, its pass (`nature.sea`, before the nature pass,
    /// off until a sea is set) and its resource.
    fn build_sea(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = sea::SeaParams::register(&mut ctx.world.res_mut::<ParamRegistry>())
            .map_err(|e| PluginError(format!("material.nature: {e}")))?;
        let effect: Rc<dyn PostEffect> = Rc::new(sea::SeaEffect {
            sea: self.sea.clone(),
            params: p.clone(),
        });
        match ctx.world.try_res_mut::<PostEffects>() {
            Some(mut pe) => pe.effects.push(effect),
            None => return Err(PluginError("material.nature: no post-effect list (add the render module first)".into())),
        }
        {
            let Some(mut g) = ctx.world.try_res_mut::<RenderGraph>() else {
                return Err(PluginError("material.nature: no render graph (add the render module first)".into()));
            };
            let err = |e: evani_render::graph::GraphError| PluginError(format!("material.nature: {e}"));
            g.add_target(TargetDesc::new(sea::SCENE, TargetKind::HdrColor).transient()).map_err(err)?;
            // The field sea's wave textures (a small fixed atlas) and its
            // mesh's own depth.
            let [fw, fh] = sea::field::FIELD_SIZE;
            g.add_target(TargetDesc::new(sea::field::FIELD, TargetKind::HdrColor).fixed(fw, fh)).map_err(err)?;
            g.add_target(TargetDesc::new(sea::field::ZBUF, TargetKind::Depth).transient()).map_err(err)?;
            g.add_pass(
                PassDesc::new(sea::PASS, "material.nature")
                    .reads(&["hdr", "depth"])
                    .writes(&["hdr", sea::SCENE, sea::field::FIELD, sea::field::ZBUF])
                    .order(3),
            )
            .map_err(err)?;
            g.set_enabled(sea::PASS, false);
        }
        self.sea_on = Some(false);
        self.sea_params = Some(p);
        ctx.resource(self.sea.clone());
        Ok(())
    }

    /// This frame's camera, time, light and level for the sea; its pass on
    /// only while a sea is set and the module is enabled.
    fn frame_sea(&mut self, world: &mut World) {
        let Some(p) = self.sea_params.clone() else { return };
        let st = &self.sea.0;
        let ns = &self.surfaces.0;
        let (enabled, level) = {
            let reg = world.res::<ParamRegistry>();
            let tier = world.try_res::<Tiers>().map_or_else(|| "medium".to_string(), |t| t.active().id.clone());
            (p.enabled(&reg), sea::level(p.quality(&reg), &tier))
        };
        // The light and sky the nature frame step worked out this frame.
        st.sun.set(ns.sun.get());
        st.sun_col.set(ns.sun_col.get());
        st.sky_tint.set(ns.sky_tint.get());
        st.camera.set(ns.camera.get());
        #[cfg(feature = "capture")]
        st.jitter.set(ns.jitter.get());
        if let Some(t) = world.try_res::<Time>() {
            st.time.set(t.render_seconds());
        }
        let draw = enabled && st.sea.get().is_some();
        st.level.set(if draw { level as i32 } else { -1 });
        if self.sea_on != Some(draw) {
            world.res_mut::<RenderGraph>().set_enabled(sea::PASS, draw);
            self.sea_on = Some(draw);
        }
    }
}
