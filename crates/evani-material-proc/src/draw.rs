//! Drawing: the baked scene as a world resource, and the `Stage::Frame`
//! system that installs the kit code and data once, re-sends the live
//! tuning when a material param changes, and draws every box (and, with
//! feature `sky`, the sky dome when the look has a `sky` material).

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamError, ParamRegistry};
use evani_core::plugin::{LookData, PluginContext};
use evani_render::kit_material::KitMaterial;
use evani_render::{MeshId, MeshInstance, RenderWorld};

use crate::bake::{Bake, bake};
use crate::layout::{pack, tune_range, write_tuning};
use crate::params::{MaterialParams, Tuning};
use crate::scene::Scene;
use crate::shader::{NO_SLOT, wgsl};

/// The look material the world is painted through.
pub const WORLD_MATERIAL: &str = "world";
/// The look material decals are painted through.
pub const DECAL_MATERIAL: &str = "decal";

/// The painted world (a world resource).
#[derive(Debug, Clone)]
pub struct ProcWorld {
    pub scene: Scene,
    /// The `kit_data` records.
    data: Vec<[f32; 4]>,
    /// The `(world, decal, sky)` slots the installed code was built for.
    installed: Option<(u32, u32, u32)>,
    data_sent: bool,
    /// The live tuning the records hold.
    tuning: Tuning,
    /// The material params' handles ([`install`] registers them) and the
    /// registry revision last read.
    params: Option<MaterialParams>,
    params_seen: Option<u64>,
    /// The sky dome's look (drawn only when the look has a `sky` material).
    #[cfg(feature = "sky")]
    pub sky: crate::sky::SkyParams,
    #[cfg(feature = "sky")]
    sky_params: Option<crate::sky::SkyHandles>,
    /// The sun direction, drift-time step and sky the sky records hold.
    #[cfg(feature = "sky")]
    sky_sent: Option<([f32; 3], i64, crate::sky::SkyParams)>,
    /// The sun and moon discs (feature `sky-discs`; off by default: the
    /// dome's own sun disc), their params and what the records hold.
    #[cfg(feature = "sky-discs")]
    pub discs: crate::discs::SkyDiscs,
    #[cfg(feature = "sky-discs")]
    discs_params: Option<crate::discs::DiscHandles>,
    #[cfg(feature = "sky-discs")]
    discs_sent: Option<(crate::discs::SkyDiscs, [f32; 3])>,
    /// Layers not drawn (bit `layer`), e.g. ceilings in an overview.
    pub hidden_layers: u32,
    /// Bake statistics: rays cast and texels baked.
    pub rays: u64,
    pub texels: u64,
}

impl ProcWorld {
    /// Bake and pack a scene.
    pub fn new(scene: Scene) -> Result<Self, String> {
        let b: Bake = bake(&scene);
        let data = pack(&scene, &b)?;
        Ok(ProcWorld {
            scene,
            data,
            installed: None,
            data_sent: false,
            tuning: Tuning::default(),
            params: None,
            params_seen: None,
            #[cfg(feature = "sky")]
            sky: crate::sky::SkyParams::default(),
            #[cfg(feature = "sky")]
            sky_params: None,
            #[cfg(feature = "sky")]
            sky_sent: None,
            #[cfg(feature = "sky-discs")]
            discs: crate::discs::SkyDiscs::default(),
            #[cfg(feature = "sky-discs")]
            discs_params: None,
            #[cfg(feature = "sky-discs")]
            discs_sent: None,
            hidden_layers: 0,
            rays: b.rays,
            texels: b.visible,
        })
    }

    /// A world of no boxes: only the sky dome is drawn (feature `sky`, when
    /// the look has a `sky` material), with the sky-only kit code.
    pub fn sky_only() -> Result<Self, String> {
        Self::new(Scene {
            boxes: Vec::new(),
            regions: Vec::new(),
            default_region: None,
        })
    }

    /// The packed records.
    pub fn data(&self) -> &[[f32; 4]] {
        &self.data
    }

    /// The tuning the records hold.
    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    /// Change the tuning (without params, or between param reads); the
    /// draw system sends it on its next run.
    pub fn set_tuning(&mut self, tuning: Tuning) {
        write_tuning(&mut self.data, &tuning);
        self.tuning = tuning;
        self.data_sent = false;
    }

    /// Register the material params (and with feature `sky` the sky params)
    /// in `reg`, so later frames read them.
    pub fn register_params(&mut self, reg: &mut ParamRegistry) -> Result<(), ParamError> {
        self.params = Some(MaterialParams::register(reg)?);
        #[cfg(feature = "sky")]
        {
            self.sky_params = Some(crate::sky::SkyHandles::register(reg, &self.sky)?);
        }
        #[cfg(feature = "sky-discs")]
        {
            self.discs_params = Some(crate::discs::DiscHandles::register(reg, &self.discs)?);
        }
        self.params_seen = None;
        Ok(())
    }
}

/// Bake `scene`, register its params, and add the world resource and its
/// draw system: everything a game needs after listing the module.
pub fn install(ctx: &mut PluginContext, scene: Scene) -> Result<(), String> {
    let mut pw = ProcWorld::new(scene)?;
    {
        let mut reg = ctx.world.res_mut::<ParamRegistry>();
        pw.register_params(&mut reg).map_err(|e| e.to_string())?;
    }
    ctx.resource(pw);
    ctx.system(system());
    Ok(())
}

fn luminance(c: [f32; 4]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The world slot and its base colour: the look's `world` material, else
/// the brightest lit non-metal (else slot 0).
pub fn world_slot(look: &LookData) -> (u32, [f32; 4]) {
    if let Some(i) = look.materials.iter().position(|m| m.name == WORLD_MATERIAL) {
        return (i as u32, look.materials[i].base_color);
    }
    look.materials
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.unlit && m.metallic < 0.5)
        .max_by(|a, b| luminance(a.1.base_color).total_cmp(&luminance(b.1.base_color)))
        .map_or((0, [1.0; 4]), |(i, m)| (i as u32, m.base_color))
}

/// The decal slot, when the look has one (and it is not the world slot).
pub fn decal_slot(look: &LookData, world: u32) -> Option<(u32, [f32; 4])> {
    let i = look
        .materials
        .iter()
        .position(|m| m.name == DECAL_MATERIAL)? as u32;
    (i != world).then(|| (i, look.materials[i as usize].base_color))
}

/// The instance colour that shows the kit's albedo unchanged through a
/// material of colour `base`.
pub fn white(base: [f32; 4]) -> [f32; 4] {
    let c = |i: usize| (1.0 / base[i].max(0.02)).min(50.0);
    [c(0), c(1), c(2), 1.0]
}

/// The sky slot, when the look has one (and it is not the world slot).
#[cfg(feature = "sky")]
pub fn sky_slot(look: &LookData, world: u32) -> Option<(u32, [f32; 4])> {
    let i = look
        .materials
        .iter()
        .position(|m| m.name == crate::sky::SKY_MATERIAL)? as u32;
    (i != world).then(|| (i, look.materials[i as usize].base_color))
}

/// Without the sky feature there is never a sky slot.
#[cfg(not(feature = "sky"))]
pub fn sky_slot(_look: &LookData, _world: u32) -> Option<(u32, [f32; 4])> {
    None
}

/// The sun direction the forward pass lights with (`render.light.direction`,
/// towards the sun), so the dome's sun disc sits where the shadows point.
#[cfg(feature = "sky")]
fn light_direction(reg: Option<&ParamRegistry>) -> [f32; 3] {
    use evani_core::params::ParamValue;
    reg.and_then(|r| r.id("render.light.direction").and_then(|id| r.value(id)))
        .and_then(|v| match v {
            ParamValue::Vec3(d) | ParamValue::Color(d) => Some(d),
            _ => None,
        })
        .unwrap_or([0.45, 0.8, 0.3])
}

/// The kit code for these `(world, decal, sky)` slots: with feature `sky`
/// and a scene of no boxes (a game that wants only the dome), the sky part
/// alone ([`crate::shader::sky_wgsl`]); otherwise the whole painter.
pub fn code_for(scene: &Scene, slots: (u32, u32, u32)) -> String {
    #[cfg(feature = "sky")]
    if scene.boxes.is_empty() {
        return crate::shader::sky_wgsl(slots.2);
    }
    #[cfg(not(feature = "sky"))]
    let _ = scene;
    wgsl(slots.0, slots.1, slots.2)
}

/// The system that paints and draws the world.
pub fn system() -> SystemDesc {
    SystemDesc::new("material_proc.draw", Stage::Frame, draw_world)
}

/// Read the params when the registry changed; re-pack the tuning (and the
/// sky records) when they differ from what was sent.
fn sync_params(world: &World, pw: &mut ProcWorld, km: &mut KitMaterial, sky_on: bool) {
    let reg = world.try_res::<ParamRegistry>();
    let rev = reg.as_ref().map(|r| r.revision());
    if let (Some(h), Some(r)) = (pw.params.as_ref(), reg.as_ref())
        && pw.params_seen != rev
    {
        let t = h.read(r);
        if t != pw.tuning {
            write_tuning(&mut pw.data, &t);
            pw.tuning = t;
            if pw.data_sent {
                let range = tune_range();
                let d = km.data_mut();
                if d.len() >= range.end {
                    d[crate::layout::TUNE_HEADER] = pw.data[crate::layout::TUNE_HEADER];
                    d[range.clone()].copy_from_slice(&pw.data[range]);
                }
            }
        }
    }
    #[cfg(feature = "sky")]
    if sky_on {
        use crate::sky;
        let light = light_direction(reg.as_deref());
        let (sun, params) = match (pw.sky_params.as_ref(), reg.as_ref()) {
            (Some(h), Some(r)) => (h.sun(r, light), h.read(r)),
            _ => (light, pw.sky),
        };
        pw.sky = params;
        // The cloud drift clock, stepped (each change re-uploads kit_data).
        let step = world
            .try_res::<evani_core::time::Time>()
            .map_or(0, |t| (t.sim_seconds * sky::TIME_HZ).floor() as i64);
        let now = (sun, step, params);
        if pw.sky_sent != Some(now) && pw.data.len() >= sky::RECORD + 4 {
            // Wrapped daily, so the f32 time keeps its precision.
            let t = ((step as f64 / sky::TIME_HZ) % 86_400.0) as f32;
            let recs = params.records(sun, t);
            pw.data[sky::RECORD..sky::RECORD + 4].copy_from_slice(&recs);
            #[cfg(feature = "sky-horizon")]
            for (i, r) in horizon_records(&params) {
                if let Some(d) = pw.data.get_mut(i) {
                    *d = r;
                }
            }
            if pw.data_sent {
                let d = km.data_mut();
                if d.len() >= sky::RECORD + 4 {
                    d[sky::RECORD..sky::RECORD + 4].copy_from_slice(&recs);
                }
                #[cfg(feature = "sky-horizon")]
                for (i, r) in horizon_records(&params) {
                    if let Some(x) = d.get_mut(i) {
                        *x = r;
                    }
                }
            }
            pw.sky_sent = Some(now);
        }
        #[cfg(feature = "sky-discs")]
        {
            let discs = match (pw.discs_params.as_ref(), reg.as_ref()) {
                (Some(h), Some(r)) => h.read(r),
                _ => pw.discs,
            };
            pw.discs = discs;
            let at = crate::layout::SKY_DISCS;
            let n = crate::discs::RECORDS;
            if pw.discs_sent != Some((discs, sun)) && pw.data.len() >= at + n {
                let recs = discs.records(sun);
                pw.data[at..at + n].copy_from_slice(&recs);
                if pw.data_sent {
                    let d = km.data_mut();
                    if d.len() >= at + n {
                        d[at..at + n].copy_from_slice(&recs);
                    }
                }
                pw.discs_sent = Some((discs, sun));
            }
        }
    }
    #[cfg(not(feature = "sky"))]
    let _ = sky_on;
    pw.params_seen = rev;
}

/// Feature `sky-horizon`: the header records it fills, with their index.
#[cfg(feature = "sky-horizon")]
fn horizon_records(params: &crate::sky::SkyParams) -> [(usize, [f32; 4]); 6] {
    use crate::layout::{SKY_CLOUD_LIGHT, SKY_CLOUD_LOOK, SKY_CUMULUS, SKY_CUMULUS_LOOK, SKY_HORIZON, SKY_TONE};
    let h = &params.horizon;
    [
        (SKY_HORIZON, h.record()),
        (SKY_CLOUD_LOOK, h.look_record()),
        (SKY_TONE, h.tone_record()),
        (SKY_CLOUD_LIGHT, h.light_record()),
        (SKY_CUMULUS, h.volume.record()),
        (SKY_CUMULUS_LOOK, h.volume.look_record()),
    ]
}

/// Install the code and data when needed, then draw every box.
pub fn draw_world(world: &mut World) {
    let Some((ws, wbase, ds, ss)) = world.try_res::<LookData>().map(|l| {
        let (ws, wb) = world_slot(&l);
        (ws, wb, decal_slot(&l, ws), sky_slot(&l, ws))
    }) else {
        return;
    };
    let Some(mut pw) = world.try_res_mut::<ProcWorld>() else {
        return;
    };
    if let Some(mut km) = world.try_res_mut::<KitMaterial>() {
        let want = (ws, ds.map_or(NO_SLOT, |d| d.0), ss.map_or(NO_SLOT, |s| s.0));
        if pw.installed != Some(want) {
            km.set_wgsl(code_for(&pw.scene, want));
            pw.installed = Some(want);
        }
        sync_params(world, &mut pw, &mut km, ss.is_some());
        if !pw.data_sent {
            *km.data_mut() = pw.data.clone();
            pw.data_sent = true;
        }
    }
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else {
        return;
    };
    #[cfg(feature = "sky")]
    if let Some((slot, base)) = ss.filter(|_| pw.sky.visible) {
        // The dome round the eye: unlit, its colour from the view direction.
        let mut m = MeshInstance::new(MeshId::SPHERE, rw.camera.eye);
        m.scale = [2.0 * crate::sky::RADIUS; 3];
        m.material = slot;
        m.color = white(base);
        rw.draw_mesh(m);
    }
    let wcol = white(wbase);
    for b in &pw.scene.boxes {
        if pw.hidden_layers & (1u32 << (b.layer & 31)) != 0 {
            continue;
        }
        let mut m = MeshInstance::new(MeshId::CUBE, b.centre);
        m.rotation = b.rotation;
        m.scale = [2.0 * b.half[0], 2.0 * b.half[1], 2.0 * b.half[2]];
        if b.decal {
            let Some((slot, base)) = ds else { continue };
            m.material = slot;
            m.color = white(base);
            rw.draw_decal(m);
        } else {
            m.material = ws;
            m.color = wcol;
            rw.draw_mesh(m);
        }
    }
}
