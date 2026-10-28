//! The chase camera's occlusion probe: keeps rival karts from sitting
//! between the camera and the player's kart, and the camera from sitting
//! inside a rival.
//!
//! Each frame the line from the player's helmet to the camera's resting eye
//! is checked against the rivals' footprints ([`Hull`]) and heights. Where a
//! rival would block it, the eye is lifted just enough to look over the
//! rival's head: the lift ramps in softly as a rival nears the line (no
//! on/off), and the caller smooths it in time with a fast attack and a slow
//! release, so the view never snaps. A hard backstop ([`keep_out`]) then
//! stops the eye ever sitting inside a rival.
//!
//! The lift has a cap, and a rival hugging the player's tail (or one the
//! eye swings close to) can still sit in front of the camera, so rivals also
//! fade ([`fades`]): the forward shader drops pixels on a 2x2 screen-door
//! pattern for an instance colour alpha below 1, and the tonemap pass blends
//! the pattern into an even see-through fade. A rival whose box comes
//! within [`FADE_FAR`] of the eye fades out, and is gone by [`FADE_NEAR`],
//! so the near plane never cuts into a kart; with one player on the screen,
//! a rival nearer than the player's kart that covers much of the kart's
//! screen box fades to a quarter, so the player's own kart always reads.
//!
//! Pure functions on plain arrays; nothing is allocated.

use crate::hull::Hull;
use evani_render::math::Mat4;

/// Height of the player's helmet over the road (m): what the camera must
/// keep in view.
pub const HEAD: f32 = 1.7;
/// Height of a rival's tallest point over its ground (m): the helmet crest.
pub const KART_TOP: f32 = 2.3;
/// Height of a rival's nose end over its ground (m): the kart's bonnet is
/// low, its driver and wing are tall, so the volume slopes down towards the
/// nose (which points at the player when the rival is behind).
pub const NOSE_TOP: f32 = 0.95;
/// The camera's own size (m): its eye stays this far from a rival.
pub const EYE_RADIUS: f32 = 0.6;
/// How far over a rival's head the line of sight should pass (m).
pub const CLEARANCE: f32 = 0.35;
/// Most the eye is lifted over its resting height (m).
pub const MAX_LIFT: f32 = 2.6;
/// Metres either side of a rival's footprint over which its blocking fades
/// in, so the lift grows smoothly as the rival nears the line.
pub const RAMP: f32 = 1.8;
/// Samples along the line of sight.
const SAMPLES: usize = 16;
/// How fast the lift follows a rising need and a falling one (1/s).
pub const ATTACK: f32 = 9.0;
pub const RELEASE: f32 = 1.8;

/// A rival: its footprint, the ground under it and its top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub hull: Hull,
    pub base: f32,
    pub top: f32,
}

impl Block {
    /// A kart standing on `ground` with the footprint `hull`.
    pub fn kart(hull: Hull, ground: f32) -> Self {
        Self { hull, base: ground, top: ground + KART_TOP }
    }

    /// The block's height over the map point `p`: the full height from the
    /// tail to just ahead of the middle, sloping down to the nose.
    pub fn top_at(&self, p: [f32; 2]) -> f32 {
        let (a, b) = (self.hull.a, self.hull.b);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if l < 1e-4 {
            return self.top;
        }
        let c = self.hull.centre();
        let u = ((p[0] - c[0]) * d[0] + (p[1] - c[1]) * d[1]) / l;
        let k = (u - 0.2).clamp(0.0, 1.0);
        let nose = self.base + NOSE_TOP;
        self.top + (nose - self.top) * k
    }
}

/// 0 when `dist` (from the hull's segment) is `RAMP` outside the hull's
/// radius or more, 1 when it is inside the radius, smoothly between.
fn cover(hull: &Hull, p: [f32; 2]) -> f32 {
    let out = hull.distance_to_segment(p) - hull.radius;
    let x = (1.0 - out / RAMP).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How far the eye must rise over `eye` for the line from `head` to it to
/// pass over every block (0..[`MAX_LIFT`]). The need from each block is
/// weighted by how close the line runs to it, so it is continuous in the
/// blocks' positions.
pub fn lift_needed(head: [f32; 3], eye: [f32; 3], blocks: &[Block]) -> f32 {
    let mut need = 0.0f32;
    for i in 1..=SAMPLES {
        let s = i as f32 / SAMPLES as f32;
        let p = [head[0] + (eye[0] - head[0]) * s, head[2] + (eye[2] - head[2]) * s];
        for b in blocks {
            let w = cover(&b.hull, p);
            if w <= 0.0 {
                continue;
            }
            // The eye height at which the line passes `CLEARANCE` over the
            // block's top at this sample (the line is straight).
            let top = b.top_at(p);
            let want_y = if i == SAMPLES { top + EYE_RADIUS } else { head[1] + (top + CLEARANCE - head[1]) / s };
            need = need.max(w * (want_y - eye[1]));
        }
    }
    need.clamp(0.0, MAX_LIFT)
}

/// Eases `lift` towards `target`: quick up, slow down. `snap` jumps there.
pub fn ease_lift(lift: f32, target: f32, dt: f32, snap: bool) -> f32 {
    if snap {
        return target;
    }
    let rate = if target > lift { ATTACK } else { RELEASE };
    lift + (target - lift) * (1.0 - (-rate * dt).exp())
}

/// The eye moved up out of any block it sits in or beside (never sideways,
/// so it never swings across the road): the backstop for when the smoothing
/// lags behind a rival that appears under the camera.
pub fn keep_out(mut eye: [f32; 3], blocks: &[Block]) -> [f32; 3] {
    for b in blocks {
        let at = [eye[0], eye[2]];
        let d = b.hull.distance_to_segment(at);
        let top = b.top_at(at);
        if d < b.hull.radius + EYE_RADIUS && eye[1] < top + EYE_RADIUS && eye[1] > b.base - EYE_RADIUS {
            eye[1] = top + EYE_RADIUS;
        }
    }
    eye
}

/// Whether the straight line from `head` to `eye` passes through any block
/// (a footprint and below its top): what the probe is meant to prevent.
pub fn line_blocked(head: [f32; 3], eye: [f32; 3], blocks: &[Block]) -> bool {
    const N: usize = 48;
    (1..=N).any(|i| {
        let s = i as f32 / N as f32;
        let p = [head[0] + (eye[0] - head[0]) * s, head[1] + (eye[1] - head[1]) * s, head[2] + (eye[2] - head[2]) * s];
        blocks.iter().any(|b| b.hull.distance_to_segment([p[0], p[2]]) < b.hull.radius && p[1] < b.top_at([p[0], p[2]]) && p[1] > b.base)
    })
}

/// Half width and half length of a kart's box (m): body, wheels, wing and
/// driver with a little room. Its height runs from the ground to
/// [`KART_TOP`].
pub const BOX_HALF: [f32; 2] = [0.95, 1.5];
/// A rival whose box is this close to the eye (m) is not drawn at all...
pub const FADE_NEAR: f32 = 1.6;
/// ...and from this distance in it fades out (screen-door dither).
pub const FADE_FAR: f32 = 3.2;
/// The share of the player's screen box a nearer rival may cover before it
/// starts to fade, and the share by which the fade is fullest.
pub const COVER_FROM: f32 = 0.2;
pub const COVER_FULL: f32 = 0.5;
/// How much of a covering rival goes at the fullest (it keeps a quarter, so
/// the player still sees it is there).
pub const COVER_FADE: f32 = 0.75;
/// The share of a nearer rival's screen box cut off by the view's edges
/// before it starts to fade, and the share by which the fade is fullest.
pub const EDGE_FROM: f32 = 0.15;
pub const EDGE_FULL: f32 = 0.4;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Block {
    /// The box's ground centre (x, z) and its forward unit vector (x, z).
    fn frame(&self) -> ([f32; 2], [f32; 2]) {
        let (a, b) = (self.hull.a, self.hull.b);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
        let f = if l < 1e-5 { [0.0, 1.0] } else { [d[0] / l, d[1] / l] };
        (self.hull.centre(), f)
    }

    /// `p` in the box's own frame: (across, up from the ground, along).
    fn local(&self, p: [f32; 3]) -> [f32; 3] {
        let (c, f) = self.frame();
        let d = [p[0] - c[0], p[2] - c[1]];
        // Right of forward (x, z) = (f.z, -f.x).
        [d[0] * f[1] - d[1] * f[0], p[1] - self.base, d[0] * f[0] + d[1] * f[1]]
    }

    /// The eight corners of the kart's box.
    pub fn corners(&self) -> [[f32; 3]; 8] {
        let (c, f) = self.frame();
        let r = [f[1], -f[0]];
        let mut out = [[0.0; 3]; 8];
        for (i, o) in out.iter_mut().enumerate() {
            let sx = if i & 1 == 0 { -BOX_HALF[0] } else { BOX_HALF[0] };
            let sz = if i & 2 == 0 { -BOX_HALF[1] } else { BOX_HALF[1] };
            let y = if i & 4 == 0 { self.base } else { self.base + KART_TOP };
            *o = [c[0] + r[0] * sx + f[0] * sz, y, c[1] + r[1] * sx + f[1] * sz];
        }
        out
    }

    /// Distance (m) from `p` to the kart's box (0 inside it).
    pub fn box_distance(&self, p: [f32; 3]) -> f32 {
        let l = self.local(p);
        let dx = (l[0].abs() - BOX_HALF[0]).max(0.0);
        let dz = (l[2].abs() - BOX_HALF[1]).max(0.0);
        let dy = (-l[1]).max(l[1] - KART_TOP).max(0.0);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Where the ray from `o` along the unit `d` first meets the kart's box
    /// (its distance along the ray), if it does.
    pub fn ray_hit(&self, o: [f32; 3], d: [f32; 3]) -> Option<f32> {
        let lo = self.local(o);
        // The direction in the box's frame (no translation).
        let (_, f) = self.frame();
        let ld = [d[0] * f[1] - d[2] * f[0], d[1], d[0] * f[0] + d[2] * f[1]];
        let (min, max) = ([-BOX_HALF[0], 0.0, -BOX_HALF[1]], [BOX_HALF[0], KART_TOP, BOX_HALF[1]]);
        let (mut t0, mut t1) = (0.0f32, f32::MAX);
        for k in 0..3 {
            if ld[k].abs() < 1e-8 {
                if lo[k] < min[k] || lo[k] > max[k] {
                    return None;
                }
                continue;
            }
            let (a, b) = ((min[k] - lo[k]) / ld[k], (max[k] - lo[k]) / ld[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        (t0 <= t1).then_some(t0)
    }
}

/// The screen rectangle (normalised device x and y: min x, min y, max x,
/// max y) of `b`'s box seen through `view_proj`. None when it is wholly
/// behind the eye; the whole screen when it reaches behind the near plane.
pub fn screen_rect(view_proj: &Mat4, b: &Block) -> Option<[f32; 4]> {
    let m = view_proj;
    let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    let mut behind = 0;
    for c in b.corners() {
        let v = |i: usize| m[i] * c[0] + m[4 + i] * c[1] + m[8 + i] * c[2] + m[12 + i];
        let w = v(3);
        if w <= evani_render::shaders::NEAR {
            behind += 1;
            continue;
        }
        let (x, y) = (v(0) / w, v(1) / w);
        r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
    }
    match behind {
        8 => None,
        0 => Some(r),
        _ => Some([-1.0, -1.0, 1.0, 1.0]),
    }
}

/// The share (0..1) of the rectangle `of` that `by` covers.
pub fn cover_share(of: [f32; 4], by: [f32; 4]) -> f32 {
    let area = (of[2] - of[0]).max(0.0) * (of[3] - of[1]).max(0.0);
    if area <= 1e-9 {
        return 0.0;
    }
    let w = (of[2].min(by[2]) - of[0].max(by[0])).max(0.0);
    let h = (of[3].min(by[3]) - of[1].max(by[1])).max(0.0);
    (w * h / area).min(1.0)
}

/// How much of a rival to draw for its distance to the eye (1 all, 0 none).
pub fn near_fade(eye: [f32; 3], b: &Block) -> f32 {
    smoothstep(FADE_NEAR, FADE_FAR, b.box_distance(eye))
}

/// How much of a rival to draw for the share of the player's screen box it
/// covers.
pub fn cover_fade(share: f32) -> f32 {
    1.0 - COVER_FADE * smoothstep(COVER_FROM, COVER_FULL, share)
}

/// How much of a rival nearer than the player's kart to draw for the share
/// of its screen box cut off by the edges of the view: one hugging the
/// camera's side shows only a huge cut-off head in a corner (critic round 4,
/// video frame 6), so it fades like a covering one.
pub fn edge_fade(by: [f32; 4]) -> f32 {
    let cut = 1.0 - cover_share(by, [-1.0, -1.0, 1.0, 1.0]);
    1.0 - COVER_FADE * smoothstep(EDGE_FROM, EDGE_FULL, cut)
}

/// How much of each rival to draw in one chase view (1 all, 0 none), into
/// `out[i]` for `rivals[i]`: the near fade always, and with `cover` (one
/// player on the screen) the cover fade for rivals nearer the eye than the
/// player's kart `me`.
pub fn fades(view_proj: &Mat4, eye: [f32; 3], me: &Block, rivals: &[Block], cover: bool, out: &mut [f32]) {
    let mine = if cover { screen_rect(view_proj, me) } else { None };
    let my_d = me.box_distance(eye);
    for (b, o) in rivals.iter().zip(out.iter_mut()) {
        let mut f = near_fade(eye, b);
        if let Some(of) = mine
            && b.box_distance(eye) < my_d
            && let Some(by) = screen_rect(view_proj, b)
        {
            f = f.min(cover_fade(cover_share(of, by))).min(edge_fade(by));
        }
        *o = f;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kart_at(x: f32, z: f32) -> Block {
        Block::kart(Hull::new([x, 0.0, z], 0.0, 1.1), 0.0)
    }

    /// Pip at the origin facing +z; the resting eye 7 m behind, 3 m up.
    const HEAD_P: [f32; 3] = [0.0, HEAD, 0.0];
    const EYE: [f32; 3] = [0.0, 3.0, -7.0];

    #[test]
    fn a_clear_view_needs_no_lift() {
        assert_eq!(lift_needed(HEAD_P, EYE, &[]), 0.0);
        // A rival well to the side, and one well ahead of Pip.
        assert_eq!(lift_needed(HEAD_P, EYE, &[kart_at(6.0, -3.0), kart_at(0.0, 12.0)]), 0.0);
    }

    #[test]
    fn a_rival_between_the_camera_and_pip_lifts_the_eye_over_its_head() {
        let rival = [kart_at(0.0, -3.0)];
        assert!(line_blocked(HEAD_P, EYE, &rival), "the resting camera would look through it");
        let lift = lift_needed(HEAD_P, EYE, &rival);
        assert!(lift > 0.3, "{lift}");
        let eye = [EYE[0], EYE[1] + lift, EYE[2]];
        assert!(!line_blocked(HEAD_P, eye, &rival), "lift {lift} clears the view");
    }

    #[test]
    fn the_lift_grows_smoothly_as_a_rival_slides_onto_the_line() {
        // A rival crossing from the side onto the line, 3 m behind Pip: the
        // lift never jumps by more than a few cm per 5 cm of travel.
        let mut last = lift_needed(HEAD_P, EYE, &[kart_at(6.0, -3.0)]);
        let mut x = 6.0f32;
        while x > -0.001 {
            x -= 0.05;
            let l = lift_needed(HEAD_P, EYE, &[kart_at(x, -3.0)]);
            assert!((l - last).abs() < 0.12, "jump {} at x = {x}", l - last);
            last = l;
        }
        assert!(last > 0.3);
    }

    #[test]
    fn the_lift_never_passes_its_cap() {
        // A rival right behind Pip needs more than the cap.
        assert!(lift_needed(HEAD_P, EYE, &[kart_at(0.0, -2.4)]) <= MAX_LIFT);
        // Several at once.
        let pack = [kart_at(0.0, -2.6), kart_at(1.0, -4.0), kart_at(-1.0, -5.5), kart_at(0.0, -6.5)];
        assert!(lift_needed(HEAD_P, EYE, &pack) <= MAX_LIFT);
    }

    #[test]
    fn the_eye_never_ends_up_inside_a_rival() {
        // A rival right under the camera: the eye is put on top of it.
        let rival = [kart_at(0.3, -7.0)];
        let inside = [0.0, 1.5, -7.0];
        let out = keep_out(inside, &rival);
        assert!(out[1] >= rival[0].top_at([inside[0], inside[2]]) + EYE_RADIUS - 1e-5);
        assert_eq!((out[0], out[2]), (inside[0], inside[2]));
        // The resting eye over a rival's roof is left alone.
        assert_eq!(keep_out([0.0, 3.0, -7.0], &[kart_at(0.0, -20.0)]), [0.0, 3.0, -7.0]);
    }

    #[test]
    fn with_the_lift_and_the_backstop_no_rival_position_clips_the_eye_or_hides_pip() {
        // Sweep one rival over the whole area behind and round Pip.
        let (mut z, mut worst_clip) = (-12.0f32, 0.0f32);
        while z < 4.0 {
            let mut x = -5.0f32;
            while x <= 5.0 {
                // Pip's own hull sits at the origin: skip the overlap.
                let rival = [kart_at(x, z)];
                if crate::hull::contact(&Hull::new([0.0; 3], 0.0, 1.1), &rival[0].hull).is_none() {
                    let lift = lift_needed(HEAD_P, EYE, &rival);
                    let eye = keep_out([EYE[0], EYE[1] + lift, EYE[2]], &rival);
                    let d = rival[0].hull.distance_to_segment([eye[0], eye[2]]);
                    if d < rival[0].hull.radius + EYE_RADIUS {
                        worst_clip = worst_clip.max(rival[0].top_at([eye[0], eye[2]]) + EYE_RADIUS - eye[1]);
                    }
                    // Beyond the cap's reach (a rival hugging Pip's tail) the
                    // view may still touch a rival's roof; everywhere the
                    // rival is 3 m or more behind it is clear.
                    if z < -3.4 && z > -9.0 {
                        assert!(!line_blocked(HEAD_P, eye, &rival), "blocked by a rival at ({x}, {z}), lift {lift}");
                    }
                }
                x += 0.4;
            }
            z += 0.4;
        }
        assert!(worst_clip <= 1e-4, "the eye sat {worst_clip} m inside a rival");
    }

    #[test]
    fn the_lift_eases_up_fast_and_down_slowly() {
        let up = ease_lift(0.0, 2.0, 1.0 / 60.0, false);
        let down = 2.0 - ease_lift(2.0, 0.0, 1.0 / 60.0, false);
        assert!(up > 0.2 && up < 2.0, "{up}");
        assert!(down > 0.0 && down < up, "{down} vs {up}");
        assert_eq!(ease_lift(0.0, 2.0, 1.0 / 60.0, true), 2.0);
    }

    fn view(eye: [f32; 3]) -> Mat4 {
        let pose = evani_core::camera::CameraPose { eye, target: [0.0, 1.0, 3.0], up: [0.0, 1.0, 0.0], fov_y_deg: 70.0 };
        evani_render::shaders::view_proj(&pose, 16.0 / 9.0)
    }

    #[test]
    fn the_kart_box_measures_distance_and_catches_rays() {
        let k = kart_at(0.0, 0.0);
        assert_eq!(k.box_distance([0.0, 1.0, 0.0]), 0.0);
        assert!((k.box_distance([0.0, 1.0, -3.5]) - 2.0).abs() < 1e-4);
        assert!((k.box_distance([0.0, KART_TOP + 1.0, 0.0]) - 1.0).abs() < 1e-4);
        // A ray from behind straight at it enters the tail; one past its
        // side misses.
        let hit = k.ray_hit([0.0, 1.0, -10.0], [0.0, 0.0, 1.0]).expect("hits the tail");
        assert!((hit - (10.0 - BOX_HALF[1])).abs() < 1e-4, "{hit}");
        assert!(k.ray_hit([2.0, 1.0, -10.0], [0.0, 0.0, 1.0]).is_none());
        // Every corner is on the box's surface.
        for c in k.corners() {
            assert!(k.box_distance(c) < 1e-4);
        }
    }

    #[test]
    fn a_rival_near_the_eye_is_faded_out_and_one_far_off_is_whole() {
        assert_eq!(near_fade(EYE, &kart_at(0.0, -7.5)), 0.0, "the eye sits over its box");
        assert_eq!(near_fade(EYE, &kart_at(0.0, 0.0)), 1.0);
        // The fade grows smoothly with distance.
        let mut last = 0.0;
        for i in 0..40 {
            let f = near_fade(EYE, &kart_at(1.0 + i as f32 * 0.1, -7.0));
            assert!(f >= last - 1e-6 && f - last < 0.2, "{f} after {last}");
            last = f;
        }
    }

    #[test]
    fn a_rival_covering_pip_from_in_front_of_the_camera_fades_and_one_beside_does_not() {
        let me = kart_at(0.0, 0.0);
        let vp = view(EYE);
        let mut out = [1.0; 3];
        // Right behind Pip on the line of sight, beside Pip in full view, and ahead.
        let rivals = [kart_at(0.0, -3.4), kart_at(3.6, -1.5), kart_at(0.0, 8.0)];
        fades(&vp, EYE, &me, &rivals, true, &mut out);
        let share = cover_share(screen_rect(&vp, &me).unwrap(), screen_rect(&vp, &rivals[0]).unwrap());
        assert!(share > COVER_FULL, "{share}");
        let near = near_fade(EYE, &rivals[0]);
        assert!(near > 1.0 - COVER_FADE, "{near}");
        assert!((out[0] - (1.0 - COVER_FADE)).abs() < 1e-4, "{out:?}");
        assert_eq!(out[1..], [1.0, 1.0]);
        // Without the cover fade (two players) only the near fade applies.
        fades(&vp, EYE, &me, &rivals, false, &mut out);
        assert_eq!(out, [near, 1.0, 1.0]);
    }

    #[test]
    fn a_rival_hugging_the_camera_cut_off_by_the_corner_fades() {
        // Critic round 4 (video frame 6): a rival beside and just ahead of
        // the eye, out of the near fade's reach, showed as a huge unfaded
        // head cut off in the bottom-right corner.
        let me = kart_at(0.0, 0.0);
        let vp = view(EYE);
        let rival = [kart_at(2.8, -3.2)];
        assert!(near_fade(EYE, &rival[0]) > 0.9, "the near fade alone leaves it whole");
        let by = screen_rect(&vp, &rival[0]).unwrap();
        assert!(by[1] < -1.0, "cut off at the bottom: {by:?}");
        let mut out = [1.0];
        fades(&vp, EYE, &me, &rival, true, &mut out);
        assert!(out[0] <= 1.0 - COVER_FADE + 1e-3, "{out:?}");
        // A rival wholly in view is not cut and keeps all of itself.
        assert_eq!(edge_fade([-0.5, -0.5, 0.5, 0.5]), 1.0);
        // The fade grows smoothly as more of the box leaves the view.
        let mut last = 1.0;
        for i in 0..40 {
            let f = edge_fade([0.0, -1.0 - i as f32 * 0.05, 0.5, 0.0]);
            assert!(f <= last + 1e-6 && last - f < 0.15, "{f} after {last}");
            last = f;
        }
    }

    #[test]
    fn a_box_reaching_behind_the_eye_covers_the_whole_screen() {
        let vp = view(EYE);
        assert_eq!(screen_rect(&vp, &kart_at(0.0, -7.0)), Some([-1.0, -1.0, 1.0, 1.0]));
        assert_eq!(screen_rect(&vp, &kart_at(0.0, -20.0)), None);
        assert_eq!(cover_share([0.0, 0.0, 1.0, 1.0], [0.5, -1.0, 2.0, 2.0]), 0.5);
    }
}
