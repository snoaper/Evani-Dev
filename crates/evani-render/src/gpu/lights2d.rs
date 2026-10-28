//! GPU side of the 2D dynamic lights (feature `lights2d`; see
//! `crate::lights2d`). No targets or passes of its own: each frame it marks
//! the packed sprites the lights reach and writes the light buffer, which
//! the sprites pass binds as its uniform (`shaders/lights2d.wgsl`).

use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::ParamRegistry;
use evani_core::tiers::Tiers;

use crate::art::SpriteArt;
use crate::lights2d::{self, LightParams, Lights2d, UNIFORM_WORDS};
use crate::shaders::SpriteRaw;
use crate::world::RenderWorld;

#[derive(Default)]
pub struct GpuLights2d {
    params: Option<LightParams>,
    /// [`SpriteArt::revision`] the normal table was built from.
    art_rev: Option<u64>,
    normals: Vec<u16>,
}

impl GpuLights2d {
    /// Mark this frame's packed sprites and return the sprite uniform.
    /// With the effect off (or a tier without lights) nothing is marked and
    /// no light is written: every sprite draws unlit.
    pub fn prepare(
        &mut self,
        world: &World,
        rw: &RenderWorld,
        raw: &mut [SpriteRaw],
        uvs: &[[f32; 4]],
        art_base: usize,
        viewport: [f32; 2],
    ) -> [f32; UNIFORM_WORDS] {
        let reg = world.res::<ParamRegistry>();
        let params = *self.params.get_or_insert_with(|| LightParams::lookup(&reg));
        let cap = {
            let fx = world.try_res::<EffectsStatus>();
            let tiers = world.res::<Tiers>();
            lights2d::active_cap(fx.as_deref(), &tiers.active().id)
        };
        let lights = world.try_res::<Lights2d>();
        let Some(lights) = lights.as_deref().filter(|_| cap > 0) else {
            let mut u = [0.0; UNIFORM_WORDS];
            u[0] = viewport[0];
            u[1] = viewport[1];
            return u;
        };
        if let Some(art) = world.try_res::<SpriteArt>()
            && self.art_rev != Some(art.revision())
        {
            self.normals = lights2d::normal_table(&art);
            self.art_rev = Some(art.revision());
        }
        lights2d::mark(&rw.sprites, raw, &self.normals, uvs, art_base, rw.hud_layers, lights.unlit_from);
        lights2d::uniform(&reg, &params, lights, cap, viewport)
    }
}
