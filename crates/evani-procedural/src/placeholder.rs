//! The stand-in every not-yet-built asset gets: a simple shape picked from
//! its name ([`Hint`]), in its category's colour, with a size, a colour and
//! a seed (a lumpy shape varies with it). It makes the whole library list,
//! toggle and tune in the viewer from day one; a category worker replaces
//! one placeholder at a time with a real generator (docs/PROCEDURAL.md).

use crate::asset::{AssetInfo, Output, ProcAsset};
use crate::geo::{self, Mesh, MeshBuilder};
use crate::noise;
use crate::spec::{ParamSpec, Values};

/// The stand-in's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// A box (crates, buildings, furniture).
    Block,
    /// A sphere (heads, fruit, balls).
    Ball,
    /// A lumpy sphere that varies with the seed (rocks, bushes, clouds).
    Blob,
    /// An upright cylinder (poles, posts, barrels, tanks).
    Post,
    /// A cone (trees, mountains, roofs).
    Cone,
    /// A thin wide slab (roads, floors, terrain, fabric).
    Slab,
    /// A ring (wheels, rims, ripples).
    Ring,
    /// A long box along x (vehicles, weapons, boats).
    Long,
    /// A trunk with a cone on top (trees).
    Tree,
    /// A cluster of small seeded boxes (particles, debris, clutter).
    Scatter,
}

/// The placeholder's size param description (how a viewer tells a stand-in
/// from a built asset).
pub const SIZE_DESCRIBE: &str = "Size of the stand-in (m)";

/// Whether `a` is a placeholder (not built yet).
pub fn is_placeholder(a: &dyn ProcAsset) -> bool {
    a.params().iter().any(|p| p.describe == SIZE_DESCRIBE)
}

/// A placeholder asset: a [`Hint`] shape from its name.
#[derive(Debug, Clone)]
pub struct Placeholder {
    info: AssetInfo,
    hint: Hint,
    params: [ParamSpec; 2],
}

impl Placeholder {
    /// `color` is the category's colour (linear RGB).
    pub const fn new(category: &'static str, id: &'static str, name: &'static str, hint: Hint, color: [f32; 3]) -> Self {
        Self {
            info: AssetInfo::cpu(category, id, name),
            hint,
            params: [
                ParamSpec::num("size", 1.0, 0.1, 20.0).unit("m").describe(SIZE_DESCRIBE),
                ParamSpec::color("color", color).describe("Colour of the stand-in"),
            ],
        }
    }

    pub fn hint(&self) -> Hint {
        self.hint
    }
}

impl ProcAsset for Placeholder {
    fn info(&self) -> &AssetInfo {
        &self.info
    }

    fn params(&self) -> &[ParamSpec] {
        &self.params
    }

    fn generate(&self, v: &Values, seed: u64, _style: &str) -> Output {
        let s = v.f("size").max(0.01);
        Output::Mesh(shape(self.hint, s, seed as u32))
    }
}

fn sphere_at(r: [f32; 3], at: [f32; 3]) -> Mesh {
    let mut m = evani_render::mesh::sphere(16, 10);
    // The built-in sphere has radius 0.5.
    geo::transform(&mut m, [2.0 * r[0], 2.0 * r[1], 2.0 * r[2]], at);
    m
}

/// The stand-in for `hint`, about `s` metres, its base on y = 0.
pub fn shape(hint: Hint, s: f32, seed: u32) -> Mesh {
    match hint {
        Hint::Block => geo::segmented_box([s, s, s], 1),
        Hint::Ball => sphere_at([0.5 * s; 3], [0.0, 0.5 * s, 0.0]),
        Hint::Blob => {
            let mut m = sphere_at([0.5 * s; 3], [0.0, 0.5 * s, 0.0]);
            let c = [0.0, 0.5 * s, 0.0];
            for v in &mut m.vertices {
                let d = [v[0] - c[0], v[1] - c[1], v[2] - c[2]];
                let k = 1.0 + 0.35 * (noise::value3([d[0] * 3.0 / s, d[1] * 3.0 / s, d[2] * 3.0 / s], seed) - 0.5);
                v[0] = c[0] + d[0] * k;
                v[1] = (c[1] + d[1] * k * 0.8).max(0.0);
                v[2] = c[2] + d[2] * k;
            }
            geo::smooth_normals(&mut m);
            m
        }
        Hint::Post => {
            let mut m = evani_render::mesh::cylinder(16);
            // The built-in cylinder: radius 0.5, y from -0.5 to 0.5.
            geo::transform(&mut m, [0.3 * s, s, 0.3 * s], [0.0, 0.5 * s, 0.0]);
            m
        }
        Hint::Cone => {
            let mut m = evani_render::mesh::cone(16);
            geo::transform(&mut m, [s, s, s], [0.0, 0.5 * s, 0.0]);
            m
        }
        Hint::Slab => geo::segmented_box([s, 0.06 * s, s], 1),
        Hint::Ring => {
            let r = 0.5 * s;
            let t = 0.12 * s;
            let profile: Vec<[f32; 2]> = (0..=12).map(|i| {
                let a = i as f32 / 12.0 * std::f32::consts::TAU;
                [r + t * a.cos(), t + t * a.sin()]
            }).collect();
            geo::lathe(&profile, 24)
        }
        Hint::Long => geo::segmented_box([s, 0.35 * s, 0.4 * s], 1),
        Hint::Tree => {
            let mut trunk = evani_render::mesh::cylinder(10);
            geo::transform(&mut trunk, [0.12 * s, 0.4 * s, 0.12 * s], [0.0, 0.2 * s, 0.0]);
            let mut crown = evani_render::mesh::cone(14);
            geo::transform(&mut crown, [0.6 * s, 0.7 * s, 0.6 * s], [0.0, 0.4 * s + 0.35 * s, 0.0]);
            geo::merge(&mut trunk, &crown);
            trunk
        }
        Hint::Scatter => {
            let mut b = MeshBuilder::new().finish();
            for i in 0..9u32 {
                let r = |k: u32| noise::hash2(i as i32, k as i32, seed);
                let size = s * (0.12 + 0.12 * r(3));
                let mut c = geo::segmented_box([size; 3], 1);
                geo::transform(&mut c, [1.0; 3], [(r(1) - 0.5) * s, 0.0, (r(2) - 0.5) * s]);
                geo::merge(&mut b, &c);
            }
            b
        }
    }
}
