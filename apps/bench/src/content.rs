//! Stress content, sized from a tier's own limits.
//!
//! * 3D phase: a grid of cubes and spheres (equal counts, so both mesh kinds
//!   and their different triangle counts are in play) plus a ground plane,
//!   sized to [`TRI_FILL`] of `max_triangles`.
//! * 2D phase: [`SPRITE_FILL`] of `max_sprites` drifting atlas sprites over a
//!   light 3D backdrop ([`BACKDROP_FILL`] of the triangle limit).
//! * Particle phase: the tier's whole `max_particles` cap through the shared
//!   particle system over the same backdrop: half world-space quads in the
//!   grid, half screen-space sparks on three sprite layers. The first frame
//!   fills the cap; every later frame emits [`RECYCLE_FRAMES`]th of it again,
//!   recycling the oldest, so uploads run every frame too.
//! * Model phase: copies of the textured, normal-mapped proof model
//!   ([`MODEL_URL`]) up to [`TRI_FILL`] of the triangle limit, over the
//!   same backdrop ([`model_plan`]).
//! * Character phase: [`crate::characters::CHARACTERS`] skinned rigs, each
//!   posed every frame, laid out like the models over the same backdrop.
//!
//! Everything is built once per phase from integer hashes (no RNG state), so
//! two runs draw exactly the same frames. Per frame only the camera (a pure
//! function of the frame number) and the drift of the sprites change.

use evani_core::camera::CameraPath;
use evani_core::particles::{Emit, ParticleQueue, Space};
use evani_core::tiers::TierPreset;
use evani_render::atlas::cell;
use evani_render::math::quat_axis_angle;
use evani_render::mesh::triangles_of;
use evani_render::shaders::MAX_BLOBS;
use evani_render::{MeshId, MeshInstance, RenderWorld, Sprite};

/// Share of the triangle limit the 3D phase draws.
pub const TRI_FILL: f32 = 0.90;
/// Share of the sprite limit the 2D phase draws.
pub const SPRITE_FILL: f32 = 0.90;
/// Share of the triangle limit the 2D phase's backdrop draws.
pub const BACKDROP_FILL: f32 = 0.05;
/// Simulated seconds per frame: the camera path advances by exactly this
/// every frame, whatever the real frame time is.
pub const FRAME_DT: f64 = 1.0 / 60.0;
/// Distance between grid cells (world units).
const SPACING: f32 = 1.6;
/// Seconds per camera orbit.
const ORBIT_S: f32 = 24.0;

/// Stress scene (`?stress=1`): triangles it draws on every tier. The same
/// content everywhere, so only the tier's own settings (render size, MSAA,
/// effects, shadow map) make one tier cost more than another.
pub const STRESS_TRIANGLES: u32 = 360_000;
/// Stress scene: full-canvas translucent layers over the scene. Fill is
/// cheap on the CPU and heavy on the GPU, so the GPU (not the CPU) bounds
/// the frame and stays clocked up while the bench measures.
///
/// Sized on an RTX 4060 Ti under Chrome's 60 Hz cap: 64 layers left High at
/// about 5 ms of 16.7 (a third busy) and the clock dropped, inflating every
/// pass; 400 held the clock at Low (8 ms, half busy). 500 keeps Low near
/// 60 % busy and the higher tiers GPU-bound.
pub const STRESS_FILL_LAYERS: u32 = 500;
/// Summed alpha of all fill layers (each gets its share), so any layer
/// count adds up to the same light veil.
const FILL_VEIL: f32 = 0.384;

/// Particle phase: every later frame re-emits this fraction of the cap
/// (`1 / RECYCLE_FRAMES`), recycling the oldest.
pub const RECYCLE_FRAMES: u32 = 60;
/// Particle phase: seconds a particle lives (longer than any phase, so the
/// ring stays full and only recycling replaces particles).
const PARTICLE_LIFE: f32 = 3_600.0;

/// Which phase's content to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    ThreeD,
    TwoD,
    Particles,
    /// Textured, normal-mapped models (S3-3) over the light backdrop.
    Models,
    /// Skinned rigs, each posed every frame (S3-9; [`crate::characters`]),
    /// over the light backdrop.
    Characters,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::ThreeD => "3D",
            Phase::TwoD => "2D",
            Phase::Particles => "particles",
            Phase::Models => "models",
            Phase::Characters => "characters",
        }
    }
}

/// The model the model phase draws (the sandbox's proof model, served as
/// `models/` by the bench page).
pub const MODEL_URL: &str = "models/proof-figure.glb";
/// Height of the model (m), to space the copies.
const MODEL_HEIGHT: f32 = 2.19;

/// How many copies of a model with `model_triangles` the model phase
/// draws on `tier`: [`TRI_FILL`] of the triangle limit less the backdrop,
/// and at least one.
pub fn model_plan(tier: &TierPreset, model_triangles: u32) -> u32 {
    if model_triangles == 0 {
        return 0;
    }
    let budget = (tier.max_triangles as f32 * TRI_FILL) as u32;
    let backdrop = mesh_plan(triangle_budget(tier, Phase::Models)).1;
    (budget.saturating_sub(backdrop) / model_triangles).max(1)
}

/// How many objects fill a triangle budget: equal numbers of cubes and
/// spheres, plus one ground plane. Returns `(pairs, triangles drawn)`.
pub fn mesh_plan(triangle_budget: u32) -> (u32, u32) {
    let (cube, sphere, plane) = (
        triangles_of(MeshId::CUBE),
        triangles_of(MeshId::SPHERE),
        triangles_of(MeshId::PLANE),
    );
    let pair = (cube + sphere).max(1);
    let pairs = triangle_budget.saturating_sub(plane) / pair;
    (pairs, pairs * pair + plane)
}

/// Sprites to draw for a sprite limit.
pub fn sprite_plan(max_sprites: u32) -> u32 {
    (max_sprites as f32 * SPRITE_FILL) as u32
}

/// Triangle budget of a phase for a tier.
pub fn triangle_budget(tier: &TierPreset, phase: Phase) -> u32 {
    let fill = match phase {
        Phase::ThreeD => TRI_FILL,
        Phase::TwoD | Phase::Particles | Phase::Models | Phase::Characters => BACKDROP_FILL,
    };
    (tier.max_triangles as f32 * fill) as u32
}

/// Cheap integer hash to a float in 0..1.
fn hash01(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// One sprite's fixed numbers; position is `uv + velocity * t`, wrapped,
/// scaled to the canvas each frame.
#[derive(Debug, Clone, Copy)]
struct SpriteSeed {
    u: f32,
    v: f32,
    du: f32,
    dv: f32,
    size: f32,
    cell: u16,
    layer: i16,
    color: [f32; 4],
}

/// The content of one phase, ready to draw.
pub struct Stress {
    meshes: Vec<MeshInstance>,
    /// Blob shadows on the ground under objects (3D phase): what a game
    /// on a tier without a shadow map (Low) draws to ground its objects.
    blobs: Vec<[f32; 4]>,
    sprites: Vec<SpriteSeed>,
    /// Full-canvas fill layers drawn over the scene (stress scene only).
    fill_layers: u32,
    /// Particles the particle phase keeps alive (the tier's cap; 0 in the
    /// other phases).
    pub particles: u32,
    /// Side of the object grid (world units): world particles fill it.
    extent: f32,
    /// Triangles the meshes add up to.
    pub triangles: u32,
    pub sprite_count: u32,
    camera: CameraPath,
}

impl Stress {
    /// Build a phase's content for `tier`. `materials` are the look's
    /// `(ground, accent, glow)` material indices.
    pub fn build(tier: &TierPreset, phase: Phase, materials: [u32; 3]) -> Self {
        let sprites = match phase {
            Phase::ThreeD | Phase::Particles | Phase::Models | Phase::Characters => 0,
            Phase::TwoD => sprite_plan(tier.max_sprites),
        };
        let mut s = Self::build_with(triangle_budget(tier, phase), sprites, phase == Phase::ThreeD, 0, materials);
        if phase == Phase::Particles {
            s.particles = tier.max_particles;
        }
        s
    }

    /// The stress scene: [`STRESS_TRIANGLES`] of objects with blob shadows
    /// and [`STRESS_FILL_LAYERS`] fill layers, the same for every tier.
    pub fn build_stress(materials: [u32; 3]) -> Self {
        Self::build_stress_with(STRESS_FILL_LAYERS, materials)
    }

    /// The stress scene with `fill_layers` fill layers (`?fill=N`, to tune
    /// the load on another GPU).
    pub fn build_stress_with(fill_layers: u32, materials: [u32; 3]) -> Self {
        Self::build_with(STRESS_TRIANGLES, 0, true, fill_layers, materials)
    }

    fn build_with(
        triangle_budget: u32,
        sprite_total: u32,
        with_blobs: bool,
        fill_layers: u32,
        materials: [u32; 3],
    ) -> Self {
        let [ground, accent, glow] = materials;
        let (pairs, triangles) = mesh_plan(triangle_budget);
        let n = pairs * 2;
        let side = (n as f32).sqrt().ceil().max(1.0) as u32;
        let extent = side as f32 * SPACING;
        let half = (side as f32 - 1.0) * SPACING * 0.5;

        let mut meshes = Vec::with_capacity(n as usize + 1);
        let mut g = MeshInstance::new(MeshId::PLANE, [0.0, -0.8, 0.0]);
        g.scale = [extent + 40.0, 1.0, extent + 40.0];
        g.material = ground;
        meshes.push(g);
        for i in 0..n {
            let (col, row) = (i % side, i / side);
            // Cubes and spheres alternate, so each gets exactly `pairs`.
            let mesh = if i.is_multiple_of(2) { MeshId::CUBE } else { MeshId::SPHERE };
            let wave = 0.5 * ((col as f32 * 0.35).sin() + (row as f32 * 0.29).cos());
            let mut m = MeshInstance::new(
                mesh,
                [col as f32 * SPACING - half, wave, row as f32 * SPACING - half],
            );
            m.rotation = quat_axis_angle(
                [hash01(i, 11) - 0.5, 1.0, hash01(i, 12) - 0.5],
                hash01(i, 13) * core::f32::consts::TAU,
            );
            m.scale = [0.9; 3];
            if i.is_multiple_of(7) {
                m.material = glow;
            } else {
                m.material = accent;
                let h = hash01(i, 3);
                m.color = [0.6 + 0.4 * h, 0.7 + 0.3 * (1.0 - h), 1.0, 1.0];
            }
            meshes.push(m);
        }

        // As many blob shadows as the renderer takes, spread over the grid.
        let mut blobs = Vec::new();
        if with_blobs && n > 0 {
            let every = (n / MAX_BLOBS as u32).max(1);
            for i in (0..n).step_by(every as usize).take(MAX_BLOBS) {
                let m = &meshes[i as usize + 1];
                blobs.push([m.position[0], -0.8, m.position[2], 0.8]);
            }
        }

        let shapes = [cell::CIRCLE, cell::RING, cell::STAR, cell::DIAMOND, cell::PLUS, cell::GLOW, cell::CHECKER];
        let mut sprites = Vec::with_capacity(sprite_total as usize);
        for k in 0..sprite_total {
            let (a, b, c, d) = (hash01(k, 1), hash01(k, 2), hash01(k, 5), hash01(k, 7));
            let shape = shapes[k as usize % shapes.len()];
            sprites.push(SpriteSeed {
                u: a,
                v: b,
                du: (c - 0.3) * 0.08,
                dv: (d - 0.5) * 0.03,
                size: 5.0 + 9.0 * d,
                cell: shape,
                // Submitted in layer order, so the renderer's sort is a no-op.
                layer: ((u64::from(k) * 3) / u64::from(sprite_total.max(1))) as i16,
                color: if shape == cell::CHECKER {
                    [1.0, 1.0, 1.0, 0.8]
                } else {
                    [0.5 + 0.5 * a, 0.5 + 0.5 * b, 0.6 + 0.4 * c, 0.75]
                },
            });
        }

        // A camera that sees the whole grid; the path is a pure function of time.
        let camera = CameraPath::orbit(
            [0.0, 0.0, 0.0],
            (extent * 0.55).max(18.0),
            (extent * 0.30).max(8.0),
            ORBIT_S,
        )
        .with_bob((extent * 0.03).max(0.5), 8.0);

        Self {
            meshes,
            blobs,
            sprites,
            fill_layers,
            particles: 0,
            extent,
            triangles,
            sprite_count: sprite_total + fill_layers,
            camera,
        }
    }

    /// Emits of frame `frame` of the particle phase: the whole cap on the
    /// first frame, `1 / RECYCLE_FRAMES` of it on every later one. Each
    /// particle comes from integer hashes of its running number, so every
    /// run emits the same ones.
    pub fn emit_particles(&self, q: &mut ParticleQueue, frame: u32, viewport: [f32; 2]) {
        if self.particles == 0 {
            return;
        }
        let per = (self.particles / RECYCLE_FRAMES).max(1);
        let (first, n) = if frame == 0 {
            (0, self.particles)
        } else {
            (self.particles.wrapping_add((frame - 1).wrapping_mul(per)), per)
        };
        let [w, h] = viewport;
        let shapes = [cell::CIRCLE, cell::STAR, cell::DIAMOND, cell::PLUS, cell::GLOW, cell::RING];
        let half = self.extent * 0.5;
        for i in first..first.wrapping_add(n) {
            let (a, b, c, d) = (hash01(i, 21), hash01(i, 22), hash01(i, 23), hash01(i, 24));
            let color = [0.6 + 0.6 * a, 0.5 + 0.5 * b, 0.4 + 0.8 * c, 0.85];
            let shape = shapes[i as usize % shapes.len()];
            let mut e = if i.is_multiple_of(2) {
                // World: drifting motes over the grid.
                Emit::new(
                    [(a * 2.0 - 1.0) * half, 0.2 + 4.0 * d, (b * 2.0 - 1.0) * half],
                    [(c - 0.5) * 1.2, (d - 0.5) * 0.8, (a - 0.5) * 1.2],
                    color,
                    PARTICLE_LIFE,
                    0.15 + 0.3 * c,
                    shape,
                    Space::World3d,
                )
            } else {
                // Screen: sparks on sprite layers 0 to 2.
                Emit::new(
                    [a * w, b * h, 0.0],
                    [(c - 0.5) * 80.0, (d - 0.5) * 80.0, 0.0],
                    color,
                    PARTICLE_LIFE,
                    3.0 + 6.0 * d,
                    shape,
                    Space::Screen2d { layer: ((i / 2) % 3) as i16 },
                )
            };
            // Drag settles every particle within speed / drag of its start.
            e.drag = 0.5;
            e.spin = (b - 0.5) * 4.0;
            e.additive = i.is_multiple_of(3);
            q.emit(e);
        }
    }

    /// Where the model phase's `n` copies stand: `[x, y, z, yaw]` on a
    /// square grid centred on the backdrop, feet on its ground, spaced so
    /// they don't touch; the camera's orbit keeps them in view.
    pub fn model_placements(&self, n: u32) -> Vec<[f32; 4]> {
        let side = (n as f32).sqrt().ceil().max(1.0) as u32;
        let spacing = (self.extent / side as f32).max(MODEL_HEIGHT * 1.2);
        let half = (side as f32 - 1.0) * spacing * 0.5;
        (0..n)
            .map(|i| {
                let (col, row) = (i % side, i / side);
                [col as f32 * spacing - half, -0.8, row as f32 * spacing - half, hash01(i, 31) * core::f32::consts::TAU]
            })
            .collect()
    }

    /// Put frame `frame` of the phase into the render world. `viewport` is
    /// the canvas in CSS pixels.
    pub fn draw(&self, rw: &mut RenderWorld, frame: u32, viewport: [f32; 2]) {
        let t = f64::from(frame) * FRAME_DT;
        rw.camera = self.camera.pose(t);
        rw.meshes.extend_from_slice(&self.meshes);
        for b in &self.blobs {
            rw.draw_blob([b[0], b[1], b[2]], b[3]);
        }
        let [w, h] = viewport;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        // Fill layers: each covers the whole canvas (layer 0, under any
        // other sprite).
        let alpha = FILL_VEIL / self.fill_layers.max(1) as f32;
        for _ in 0..self.fill_layers {
            rw.draw_sprite(Sprite {
                pos: [w * 0.5, h * 0.5],
                size: [w, h],
                rotation: 0.0,
                cell: cell::SQUARE,
                color: [1.0, 1.0, 1.0, alpha],
                layer: 0,
            });
        }
        let tf = t as f32;
        rw.sprites.reserve(self.sprites.len());
        for s in &self.sprites {
            let x = s.u + s.du * tf;
            let y = s.v + s.dv * tf;
            rw.draw_sprite(Sprite {
                pos: [(x - x.floor()) * w, (y - y.floor()) * h],
                size: [s.size, s.size],
                rotation: 0.0,
                cell: s.cell,
                color: s.color,
                layer: s.layer,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::tiers::Tiers;

    fn tiers() -> Vec<TierPreset> {
        Tiers::builtin().unwrap().presets().to_vec()
    }

    #[test]
    fn plan_fills_about_ninety_percent_without_going_over() {
        for t in tiers() {
            let (_, tris) = mesh_plan(triangle_budget(&t, Phase::ThreeD));
            assert!(tris <= t.max_triangles, "{}", t.id);
            assert!(
                tris as f32 >= t.max_triangles as f32 * 0.89,
                "{}: {tris} of {}",
                t.id,
                t.max_triangles
            );
            let s = sprite_plan(t.max_sprites);
            assert!(s <= t.max_sprites && s as f32 >= t.max_sprites as f32 * 0.89);
        }
    }

    #[test]
    fn built_content_matches_the_plan() {
        let t = Tiers::builtin().unwrap().get("low").cloned().unwrap();
        let s = Stress::build(&t, Phase::TwoD, [0, 1, 2]);
        let (_, tris) = mesh_plan(triangle_budget(&t, Phase::TwoD));
        let counted: u32 = s.meshes.iter().map(|m| triangles_of(m.mesh)).sum();
        assert_eq!(counted, tris);
        assert_eq!(s.sprites.len() as u32, sprite_plan(t.max_sprites));
        let s3 = Stress::build(&t, Phase::ThreeD, [0, 1, 2]);
        assert!(s3.sprites.is_empty());
        let cubes = s3.meshes.iter().filter(|m| m.mesh == MeshId::CUBE).count();
        let spheres = s3.meshes.iter().filter(|m| m.mesh == MeshId::SPHERE).count();
        assert_eq!(cubes, spheres);
    }

    #[test]
    fn stress_scene_is_fixed_and_fills_the_canvas() {
        let s = Stress::build_stress([0, 1, 2]);
        assert_eq!(s.triangles, mesh_plan(STRESS_TRIANGLES).1);
        assert!(s.triangles as f32 >= STRESS_TRIANGLES as f32 * 0.99);
        assert_eq!(s.blobs.len(), MAX_BLOBS);
        let mut rw = RenderWorld::default();
        s.draw(&mut rw, 10, [1440.0, 810.0]);
        assert_eq!(rw.sprites.len(), STRESS_FILL_LAYERS as usize);
        assert!(rw.sprites.iter().all(|p| p.size == [1440.0, 810.0] && p.pos == [720.0, 405.0]));
        assert_eq!(rw.meshes.len(), s.meshes.len());
        // The normal phases carry no fill layers.
        let t = Tiers::builtin().unwrap().get("high").cloned().unwrap();
        let mut rw = RenderWorld::default();
        Stress::build(&t, Phase::ThreeD, [0, 1, 2]).draw(&mut rw, 10, [1920.0, 1080.0]);
        assert!(rw.sprites.is_empty());
    }

    #[test]
    fn drawing_is_deterministic_and_layers_stay_sorted() {
        let t = Tiers::builtin().unwrap().get("low").cloned().unwrap();
        let s = Stress::build(&t, Phase::TwoD, [0, 1, 2]);
        let mut a = RenderWorld::default();
        let mut b = RenderWorld::default();
        s.draw(&mut a, 37, [800.0, 600.0]);
        s.draw(&mut b, 37, [800.0, 600.0]);
        assert_eq!(a.camera, b.camera);
        assert_eq!(a.sprites, b.sprites);
        assert_eq!(a.sprites.len(), s.sprite_count as usize);
        assert!(a.sprites.windows(2).all(|w| w[0].layer <= w[1].layer));
        assert!(a.sprites.iter().all(|p| (0.0..=800.0).contains(&p.pos[0])));
        // The camera follows the path at exactly 1/60 s per frame.
        let want = s.camera.pose(37.0 / 60.0);
        assert_eq!(a.camera, want);
        let mut c = RenderWorld::default();
        s.draw(&mut c, 38, [800.0, 600.0]);
        assert_ne!(c.camera, a.camera);
    }
}
