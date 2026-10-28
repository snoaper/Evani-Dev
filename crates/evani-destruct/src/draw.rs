//! Draw helpers: push a [`Structure`] and a [`Debris`] pool into the
//! [`RenderWorld`] with the built-in cube and sphere meshes (no new shader).
//! A cell is drawn as one 1 m cube while all 27 of its samples stand, else as
//! the 3 x 3 x 3 lattice of 1/3 m cubes whose sample is not holed, so what
//! you see is exactly what the support rules count. Floor samples work the
//! same with a 2 x 2 x 2 lattice.

use evani_core::plugin::LookData;
use evani_render::math::{quat_axis_angle, quat_mul};
use evani_render::{MeshId, MeshInstance, RenderWorld};

use crate::debris::Debris;
use crate::structure::{FULL_CELL, FloorKind, Structure};

/// Which look materials and tints the helpers use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawStyle {
    pub concrete: u32,
    pub metal: u32,
    pub debris: u32,
    pub dust: u32,
    pub concrete_color: [f32; 4],
    pub metal_color: [f32; 4],
    pub debris_color: [f32; 4],
    pub dust_color: [f32; 4],
}

impl DrawStyle {
    /// Concrete in the look's `stone`, metal in `slate`, rubble and dust in
    /// `stone` (each falls back to `accent`).
    pub fn from_look(look: &LookData) -> Self {
        let pick = |n: &str| {
            if look.materials.iter().any(|m| m.name == n) {
                RenderWorld::material(look, n)
            } else {
                RenderWorld::material(look, "accent")
            }
        };
        let stone = pick("stone");
        Self {
            concrete: stone,
            metal: pick("slate"),
            debris: stone,
            dust: stone,
            concrete_color: [0.92, 0.90, 0.86, 1.0],
            metal_color: [0.55, 0.58, 0.62, 1.0],
            debris_color: [0.78, 0.76, 0.72, 1.0],
            dust_color: [0.50, 0.48, 0.44, 1.0],
        }
    }
}

const THIRD: [f32; 3] = [1.0 / 6.0, 0.5, 5.0 / 6.0];

fn cube(
    p: [f32; 3],
    rot: [f32; 4],
    scale: [f32; 3],
    material: u32,
    color: [f32; 4],
) -> MeshInstance {
    let mut m = MeshInstance::new(MeshId::CUBE, p);
    m.rotation = rot;
    m.scale = scale;
    m.material = material;
    m.color = color;
    m
}

fn shade(c: [f32; 4], i: usize) -> [f32; 4] {
    let h = (i as u32).wrapping_mul(2_654_435_761) >> 28;
    let k = 0.94 + 0.06 * (h as f32 / 15.0);
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// Push a cell's cubes; `place` maps an unmoved point to where it is now.
fn cell_cubes(
    rw: &mut RenderWorld,
    corner: [f32; 3],
    mask: u32,
    rot: [f32; 4],
    st: &DrawStyle,
    color: [f32; 4],
    place: &impl Fn([f32; 3]) -> [f32; 3],
) {
    if mask == FULL_CELL {
        rw.draw_mesh(cube(
            place([corner[0] + 0.5, corner[1] + 0.5, corner[2] + 0.5]),
            rot,
            [1.0; 3],
            st.concrete,
            color,
        ));
        return;
    }
    for (iz, fz) in THIRD.iter().enumerate() {
        for (iy, fy) in THIRD.iter().enumerate() {
            for (ix, fx) in THIRD.iter().enumerate() {
                if mask & (1 << (ix + 3 * iy + 9 * iz)) == 0 {
                    continue;
                }
                let p = [corner[0] + fx, corner[1] + fy, corner[2] + fz];
                rw.draw_mesh(cube(place(p), rot, [1.0 / 3.0; 3], st.concrete, color));
            }
        }
    }
}

/// Push a floor sample's cubes.
#[allow(clippy::too_many_arguments)]
fn tile_cubes(
    rw: &mut RenderWorld,
    c: [f32; 2],
    top: f32,
    s: f32,
    slab: f32,
    kind: FloorKind,
    mask: u8,
    rot: [f32; 4],
    st: &DrawStyle,
    place: &impl Fn([f32; 3]) -> [f32; 3],
) {
    let (mat, color) = if kind == FloorKind::Metal {
        (st.metal, st.metal_color)
    } else {
        (st.concrete, st.concrete_color)
    };
    if mask == 0xFF {
        rw.draw_mesh(cube(
            place([c[0], top - slab * 0.5, c[1]]),
            rot,
            [s, slab, s],
            mat,
            color,
        ));
        return;
    }
    let q = s * 0.25;
    for iz in 0..2 {
        for iy in 0..2 {
            for ix in 0..2 {
                if mask & (1 << (ix + 2 * iy + 4 * iz)) == 0 {
                    continue;
                }
                let p = [
                    c[0] + if ix == 0 { -q } else { q },
                    top - slab * (0.25 + 0.5 * iy as f32),
                    c[1] + if iz == 0 { -q } else { q },
                ];
                rw.draw_mesh(cube(
                    place(p),
                    rot,
                    [s * 0.5, slab * 0.5, s * 0.5],
                    mat,
                    color,
                ));
            }
        }
    }
}

/// Draw a structure: standing cells, the floor, and every falling block.
pub fn draw_structure(rw: &mut RenderWorld, s: &Structure, st: &DrawStyle) {
    let id = [0.0, 0.0, 0.0, 1.0];
    let still = |p: [f32; 3]| p;
    for c in 0..s.desc().cells.len() {
        if let Some(mask) = s.cell_state(c) {
            cell_cubes(
                rw,
                s.cell_corner(c),
                mask,
                id,
                st,
                shade(st.concrete_color, c),
                &still,
            );
        }
    }
    let top = s.desc().origin[1];
    let slab = s.desc().slab;
    if let Some(f) = &s.desc().floor {
        for i in 0..f.kinds.len() {
            if let Some((kind, mask)) = s.tile_state(i) {
                tile_cubes(
                    rw,
                    s.sample_centre(i),
                    top,
                    f.spacing,
                    slab,
                    kind,
                    mask,
                    id,
                    st,
                    &still,
                );
            }
        }
    }
    for f in s.falls() {
        let rot = f.rotation();
        let place = |p: [f32; 3]| f.transform(p);
        for &(c, mask) in f.cells() {
            cell_cubes(
                rw,
                s.cell_corner(c),
                mask,
                rot,
                st,
                shade(st.concrete_color, c),
                &place,
            );
        }
        if let Some(g) = &s.desc().floor {
            for &(i, mask) in f.samples() {
                tile_cubes(
                    rw,
                    s.sample_centre(i),
                    top,
                    g.spacing,
                    slab,
                    FloorKind::Concrete,
                    mask,
                    rot,
                    st,
                    &place,
                );
            }
        }
    }
}

/// Draw the rubble: tumbling cubes, and dust as small spheres that grow and
/// shrink away (the forward pass is opaque, so opacity scales their size).
pub fn draw_debris(rw: &mut RenderWorld, d: &Debris, st: &DrawStyle) {
    let mut i = 0usize;
    d.each_chunk(|c| {
        // A second fixed turn per slot keeps neighbouring chunks from lining up.
        let tilt = quat_axis_angle([0.6, 0.0, 0.8], (i % 7) as f32 * 0.7);
        let rot = quat_mul(quat_axis_angle(c.axis, c.angle), tilt);
        let k = 0.8 + 0.2 * ((i * 37 % 11) as f32 / 10.0);
        rw.draw_mesh(cube(
            c.position,
            rot,
            [c.size, c.size * k, c.size * (1.8 - k)],
            st.debris,
            shade(st.debris_color, i),
        ));
        i += 1;
    });
    for q in d.dust_puffs() {
        let size = q.size() * q.opacity() * 0.8;
        if size < 0.01 {
            continue;
        }
        let mut m = MeshInstance::new(MeshId::LOW_SPHERE, q.position);
        m.scale = [size; 3];
        m.material = st.dust;
        let c = st.dust_color;
        m.color = [c[0] * q.shade, c[1] * q.shade, c[2] * q.shade, 1.0];
        rw.draw_mesh(m);
    }
}
