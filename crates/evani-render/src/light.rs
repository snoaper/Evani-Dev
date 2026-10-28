//! Point lights (feature `point-lights`, S5-R3): a kit pushes [`Light`]s
//! each frame with [`RenderWorld::draw_light`]; the forward pass adds them to
//! lit meshes, models and skinned models (unlit materials and HUD sprites are
//! unaffected). The tier's `max_point_lights` caps how many are drawn: over
//! it, the lights nearest the camera are kept ([`nearest`]). With `clustered`
//! a compute pass bins them into screen tiles x depth slices so many lights
//! cost little; without it (or on a tier capped at 8 or less) each lit pixel
//! loops over at most [`MAX_LINEAR`].

use crate::world::RenderWorld;

/// Lights the plain (unclustered) path applies per pixel.
pub const MAX_LINEAR: usize = 8;

/// One point light. Its reach ends smoothly at `range` metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub position: [f32; 3],
    /// Linear RGB, usually 0..1; scaled by `intensity`.
    pub color: [f32; 3],
    /// Metres; nothing is lit beyond it.
    pub range: f32,
    /// Brightness multiplier (1 = `color` as given at close range).
    pub intensity: f32,
}

impl Light {
    pub fn new(position: [f32; 3], color: [f32; 3], range: f32, intensity: f32) -> Self {
        Self { position, color, range, intensity }
    }
}

impl RenderWorld {
    /// A point light for this frame. Lights with no range or no intensity
    /// are dropped.
    pub fn draw_light(&mut self, l: Light) {
        if l.range > 0.0 && l.intensity > 0.0 {
            self.lights.push(l);
        }
    }
}

/// The lights to draw this frame: at most `cap` indices into `lights`, the
/// ones whose sphere comes nearest any of `eyes` (distance minus range), in
/// no particular order. O(n x cap), no sort; `scores` is scratch space.
pub fn nearest(lights: &[Light], eyes: &[[f32; 3]], cap: usize, out: &mut Vec<usize>, scores: &mut Vec<f32>) {
    out.clear();
    scores.clear();
    if lights.len() <= cap {
        out.extend(0..lights.len());
        return;
    }
    if cap == 0 {
        return;
    }
    let score = |l: &Light| {
        let mut best = f32::MAX;
        for e in eyes {
            let d = [l.position[0] - e[0], l.position[1] - e[1], l.position[2] - e[2]];
            best = best.min((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - l.range);
        }
        best
    };
    for (i, l) in lights.iter().enumerate() {
        let s = score(l);
        if out.len() < cap {
            out.push(i);
            scores.push(s);
            continue;
        }
        // Replace the farthest kept light if this one is nearer.
        let (mut worst, mut worst_s) = (0, f32::MIN);
        for (k, &v) in scores.iter().enumerate() {
            if v > worst_s {
                (worst, worst_s) = (k, v);
            }
        }
        if s < worst_s {
            out[worst] = i;
            scores[worst] = s;
        }
    }
}
