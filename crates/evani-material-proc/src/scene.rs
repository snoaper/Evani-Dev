//! The world as the module sees it: boxes with a surface id per face, and
//! regions (floor-plan rectangles) that pick the style of the generic `WALL`
//! and `FLOOR` surfaces.
//!
//! Face order (`f = axis * 2 + positive`): `-x, +x, -y, +y, -z, +z` in the
//! box's own axes. A face's `(u, v)` axes: x faces use `(z, y)`, y faces
//! `(x, z)`, z faces `(x, y)`, so on an upright box `v` is up on every side.

/// A point or vector in metres (y up).
pub type V3 = [f32; 3];

/// Surface ids (what the shader paints a face as).
pub mod surface {
    /// Styled by the region in front of the face (its wall surface).
    pub const WALL: u32 = 0;
    /// Styled by the region the point is in (its floor surface).
    pub const FLOOR: u32 = 1;
    /// Lime plaster over stone: stains, speckle, fallen patches, base grime.
    pub const PLASTER: u32 = 2;
    /// Dressed sandstone blocks in 0.6 x 0.3 m courses.
    pub const SANDSTONE: u32 = 3;
    /// Board-formed concrete: panels, tie holes, rain streaks (floors: plain).
    pub const CONCRETE: u32 = 4;
    /// A soffit: dark concrete.
    pub const CEILING: u32 = 5;
    /// A wooden crate: frame, planks, brace, grain, nails.
    pub const WOOD_CRATE: u32 = 6;
    /// A painted, ribbed steel crate with worn edges and rust.
    pub const METAL_CRATE: u32 = 7;
    /// A stone step with a worn nosing.
    pub const STONE_STEP: u32 = 8;
    /// A raised stone platform: block sides, slab top.
    pub const PLATFORM: u32 = 9;
    /// A concrete ramp with grip grooves.
    pub const RAMP: u32 = 10;
    /// Flagstones.
    pub const PAVING: u32 = 11;
    /// Brick paving (variant 1 is a grey stone).
    pub const BRICK_PAVING: u32 = 12;
    /// Limestone trim (copings, sills).
    pub const TRIM_STONE: u32 = 13;
    /// Dark timber trim (lintels, beams, posts).
    pub const TRIM_WOOD: u32 = 14;
    /// A stencilled letter A (decals: the paint cuts out).
    pub const SIGN_A: u32 = 15;
    /// A stencilled letter B.
    pub const SIGN_B: u32 = 16;
    /// A stencilled arrow pointing along +u (variant 1: along -u).
    pub const ARROW: u32 = 17;
    /// Sandy ground: 2.4 m slabs, drifted sand, pebbles.
    pub const GROUND: u32 = 18;
    /// How many ids there are.
    pub const COUNT: u32 = 19;
}

/// One drawn box.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcBox {
    /// Centre (m).
    pub centre: V3,
    /// Half size along its own axes (m).
    pub half: V3,
    /// Rotation quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Surface id per face (see the module docs for the order).
    pub surfaces: [u32; 6],
    /// Style variant (0..=255; the surface decides what it means).
    pub variant: u32,
    /// Per-box pattern offset and tint (any value; only its low 16 bits are used).
    pub seed: u32,
    /// Patterns follow world coordinates instead of the box's own (axis-
    /// aligned boxes only): merged walls and floors then show no seams.
    pub world_pattern: bool,
    /// Blocks AO rays and hides the faces of other boxes it covers.
    pub occluder: bool,
    /// Gets per-face baked AO texels.
    pub bake_ao: bool,
    /// Drawn as a decal (the stencil surfaces cut out their paint).
    pub decal: bool,
    /// Draw layer (0..32): `ProcWorld::hidden_layers` can hide a layer.
    pub layer: u8,
}

impl ProcBox {
    /// An axis-aligned box from its min and max corners, one surface on
    /// every face, occluding and baked.
    pub fn aabb(min: V3, max: V3, surface: u32) -> Self {
        ProcBox {
            centre: [
                (min[0] + max[0]) * 0.5,
                (min[1] + max[1]) * 0.5,
                (min[2] + max[2]) * 0.5,
            ],
            half: [
                (max[0] - min[0]) * 0.5,
                (max[1] - min[1]) * 0.5,
                (max[2] - min[2]) * 0.5,
            ],
            rotation: [0.0, 0.0, 0.0, 1.0],
            surfaces: [surface; 6],
            variant: 0,
            seed: 0,
            world_pattern: false,
            occluder: true,
            bake_ao: true,
            decal: false,
            layer: 0,
        }
    }

    /// The world-space bounds `(min, max)`.
    pub fn bounds(&self) -> (V3, V3) {
        let ax = axes(self.rotation);
        let mut ext = [0.0f32; 3];
        for (i, e) in ext.iter_mut().enumerate() {
            *e = (0..3).map(|k| ax[k][i].abs() * self.half[k]).sum();
        }
        let c = self.centre;
        (
            [c[0] - ext[0], c[1] - ext[1], c[2] - ext[2]],
            [c[0] + ext[0], c[1] + ext[1], c[2] + ext[2]],
        )
    }

    /// `p` in the box's own frame (relative to the centre).
    pub fn to_local(&self, p: V3) -> V3 {
        rotate_inv(self.rotation, sub(p, self.centre))
    }

    /// A point of the box's own frame in the world.
    pub fn to_world(&self, l: V3) -> V3 {
        add(rotate(self.rotation, l), self.centre)
    }

    /// Is `p` inside, by more than `margin` on every axis?
    pub fn contains(&self, p: V3, margin: f32) -> bool {
        let l = self.to_local(p);
        (0..3).all(|i| l[i].abs() < self.half[i] - margin)
    }

    /// The world normal of face `f`.
    pub fn face_normal(&self, f: usize) -> V3 {
        let mut n = [0.0; 3];
        n[f / 2] = if f % 2 == 1 { 1.0 } else { -1.0 };
        rotate(self.rotation, n)
    }
}

/// A floor-plan rectangle that styles `WALL` and `FLOOR` faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    /// `[x0, z0, x1, z1]` (m).
    pub rect: [f32; 4],
    /// The surface `WALL` faces facing into it become.
    pub wall: u32,
    /// The surface `FLOOR` faces inside it become.
    pub floor: u32,
    /// The style variant for both.
    pub variant: u32,
}

/// Everything the module paints.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub boxes: Vec<ProcBox>,
    /// First match wins.
    pub regions: Vec<Region>,
    /// Where no region matches.
    pub default_region: Option<Region>,
}

/// A face's `(u, v)` axes.
pub fn uv_axes(axis: usize) -> (usize, usize) {
    match axis {
        0 => (2, 1),
        1 => (0, 2),
        _ => (0, 1),
    }
}

pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn normalize(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1e-12 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// `q * v * q^-1` (the shader's `rotate`, same operation order).
pub fn rotate(q: [f32; 4], v: V3) -> V3 {
    let qv = [q[0], q[1], q[2]];
    let t = scale(cross(qv, v), 2.0);
    add(add(v, scale(t, q[3])), cross(qv, t))
}

/// The inverse rotation (unit `q`).
pub fn rotate_inv(q: [f32; 4], v: V3) -> V3 {
    rotate([-q[0], -q[1], -q[2], q[3]], v)
}

/// The box's own axes in the world.
pub fn axes(q: [f32; 4]) -> [V3; 3] {
    [
        rotate(q, [1.0, 0.0, 0.0]),
        rotate(q, [0.0, 1.0, 0.0]),
        rotate(q, [0.0, 0.0, 1.0]),
    ]
}
