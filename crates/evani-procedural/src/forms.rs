//! Preview forms: everyday objects a finish can be seen on (Chris,
//! 2026-11-04: "it's good to show a ball but maybe we should also show a
//! door, a table, a fence ... so they can see how the material behaves in
//! other forms"). Each form is built from the base's made blocks and lathe
//! profiles at real sizes, its base on the ground, and comes back as parts
//! in the finish (`true`) or in plain fittings such as a door's handle
//! (`false`); the asset lays its own finish on the first and a dark metal on
//! the second. Only code that calls [`form`] downloads it (the library's
//! `preview` feature: the Material library view).

use evani_render::mesh::Mesh;

use crate::build::revolve;
use crate::detail::made_block;
use crate::geo::merge;

/// The forms in slider order (the asset's `preview` param; 0 is the asset's
/// own shape and is never built here).
pub const FORMS: [&str; 7] = ["own shape", "ball", "block", "door", "table", "fence", "steps"];

/// A `preview` param's description.
pub const HELP: &str = "Preview the finish on: 0 its own shape, 1 a ball, 2 a block, 3 a door, 4 a table, 5 a fence, 6 steps";

/// The form at slider value `x` (rounded), or `None` for the asset's own
/// shape. `wear` (0..1) chips the made blocks' edges; `seed` places them.
pub fn form(x: f32, wear: f32, seed: u32) -> Option<Vec<(Mesh, bool)>> {
    let k = x.round().max(0.0) as usize;
    let blk = |size: [f32; 3], bevel: f32, at: [f32; 3], s: u32| made_block(size, bevel, 10, wear, seed ^ s, at);
    Some(match k {
        1 => {
            let r = 0.45;
            let prof: Vec<[f32; 2]> = (0..=32)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 32.0;
                    [r * a.cos(), r + r * a.sin()]
                })
                .collect();
            vec![(revolve(&prof, 64), true)]
        }
        2 => vec![(blk([0.8, 0.8, 0.8], 0.01, [0.0; 3], 1), true)],
        3 => {
            // A panelled door in its frame: stiles, rails and four raised
            // panels on a slab, a round handle on a short spindle.
            let (w, h, t) = (0.9, 2.05, 0.045);
            let mut frame = Mesh::default();
            for x in [-w * 0.5 - 0.04, w * 0.5 + 0.04] {
                merge(&mut frame, &blk([0.07, h + 0.07, 0.12], 0.004, [x, 0.0, 0.0], 2));
            }
            merge(&mut frame, &blk([w + 0.15, 0.07, 0.12], 0.004, [0.0, h, 0.0], 3));
            let mut leaf = blk([w, h, t], 0.004, [0.0, 0.0, 0.0], 4);
            for (i, (px, py)) in [(-0.22f32, 0.12f32), (0.22, 0.12), (-0.22, 0.55), (0.22, 0.55)].into_iter().enumerate() {
                let (pw, ph) = (w * 0.34, h * 0.32);
                for (j, z) in [t * 0.5, -t * 0.5].into_iter().enumerate() {
                    merge(&mut leaf, &blk([pw, ph, 0.014], 0.005, [px * w * 1.05, py * h, z], 10 + (i * 2 + j) as u32));
                }
            }
            let mut fit = crate::build::rod([w * 0.4, h * 0.47, -t * 0.5 - 0.06], [w * 0.4, h * 0.47, t * 0.5 + 0.06], 0.009, 10);
            for z in [t * 0.5 + 0.06, -t * 0.5 - 0.06] {
                let mut knob = revolve(&[[0.0, -0.028], [0.028, -0.012], [0.03, 0.0], [0.028, 0.012], [0.0, 0.028]], 16);
                crate::shapes::rotate(&mut knob, [1.0, 0.0, 0.0], std::f32::consts::FRAC_PI_2);
                crate::geo::transform(&mut knob, [1.0; 3], [w * 0.4, h * 0.47, z]);
                merge(&mut fit, &knob);
            }
            merge(&mut leaf, &frame);
            vec![(leaf, true), (fit, false)]
        }
        4 => {
            // A plain table: a thick top with rounded edges on four square
            // legs joined by aprons.
            let (w, d, h) = (1.4, 0.8, 0.75);
            let mut m = blk([w, 0.04, d], 0.008, [0.0, h - 0.04, 0.0], 20);
            for (i, (sx, sz)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
                merge(&mut m, &blk([0.06, h - 0.04, 0.06], 0.004, [sx * (w * 0.5 - 0.08), 0.0, sz * (d * 0.5 - 0.08)], 21 + i as u32));
            }
            for (i, z) in [-(d * 0.5 - 0.08), d * 0.5 - 0.08].into_iter().enumerate() {
                merge(&mut m, &blk([w - 0.22, 0.09, 0.022], 0.003, [0.0, h - 0.13, z], 30 + i as u32));
            }
            for (i, x) in [-(w * 0.5 - 0.08), w * 0.5 - 0.08].into_iter().enumerate() {
                merge(&mut m, &blk([0.022, 0.09, d - 0.22], 0.003, [x, h - 0.13, 0.0], 32 + i as u32));
            }
            vec![(m, true)]
        }
        5 => {
            // A run of fence: four posts with capped tops, two rails, and
            // pales between them.
            let mut m = Mesh::default();
            let xs = [-1.5f32, -0.5, 0.5, 1.5];
            for (i, x) in xs.into_iter().enumerate() {
                merge(&mut m, &blk([0.09, 1.1, 0.09], 0.006, [x, 0.0, 0.0], 40 + i as u32));
            }
            for (i, y) in [0.28f32, 0.82].into_iter().enumerate() {
                merge(&mut m, &blk([3.0, 0.07, 0.035], 0.004, [0.0, y, 0.06], 50 + i as u32));
            }
            for k in 0..20 {
                let x = -1.43 + k as f32 * 0.15;
                if xs.iter().any(|p| (p - x).abs() < 0.07) {
                    continue;
                }
                merge(&mut m, &blk([0.08, 0.95 + 0.04 * ((k % 3) as f32), 0.018], 0.003, [x, 0.08, 0.09], 60 + k as u32));
            }
            vec![(m, true)]
        }
        6 => {
            // Three steps of made blocks, each tread overhanging a little.
            let mut m = Mesh::default();
            for i in 0..3u32 {
                let f = i as f32;
                merge(&mut m, &blk([1.2, 0.17, 0.95 - 0.3 * f], 0.01, [0.0, 0.17 * f, 0.15 * f], 70 + i));
            }
            vec![(m, true)]
        }
        _ => return None,
    })
}
