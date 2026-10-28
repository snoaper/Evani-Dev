//! Check 5: camera framing. On every drawn frame where the player stands
//! on the ground:
//! - `player_too_high`: the feet sit above the framing zone (`zone_top`,
//!   default 2/3 of the view height from the top: the lower third);
//! - `ground_not_visible`: the feet sit below `zone_bottom` (the ground
//!   under the player is cut off);
//! - `terrain_heavy`: terrain below the feet (tile shapes, hazards in full)
//!   fills more than `max_terrain` (35 %) of the view.
//!
//! Consecutive failing grounded frames join into one issue; runs shorter
//! than `camera_min_s` are ignored (a camera catching up after a landing).
//! For the zone (not terrain), a run is also ignored when every frame in it
//! is within `land_grace_s` (1 s) of landing on a new height and the camera
//! moves towards the feet over that grace: a vertical dead zone only follows
//! after a landing. Staying out longer than the grace is reported.
//! The fix says how far the camera would have to move, and whether the map
//! edge stops it.
//!
//! Limits: only sampled frames are seen; airborne frames are skipped (the
//! zone is ground-anchored); background art is not terrain.

use super::fmt_px;
use crate::geom::union;
use crate::scene::{FrameDef, Rect};
use crate::{Check, CropSource, Ctx, Issue};

fn feet(f: &FrameDef) -> f32 {
    f.player[1] + f.player[3]
}

fn feet_frac(f: &FrameDef) -> f32 {
    (feet(f) - f.cam[1]) / f.view[1].max(1.0)
}

/// Share of a view (top-left `cam`, size `view`) that terrain below world
/// height `feet` takes.
pub fn terrain_share(ctx: &Ctx, cam: [f32; 2], view: [f32; 2], feet: f32) -> f32 {
    let bottom = cam[1] + view[1];
    let top = feet.max(cam[1]);
    if bottom <= top || view[0] <= 0.0 {
        return 0.0;
    }
    let clip: Rect = [cam[0], top, view[0], bottom - top];
    let g = &ctx.geo;
    let mut a = 0.0;
    for ty in g.cell(clip[1]).max(0)..=g.cell(clip[1] + clip[3] - 0.001).min(g.h - 1) {
        for tx in g.cell(clip[0]).max(0)..=g.cell(clip[0] + clip[2] - 0.001).min(g.w - 1) {
            a += g.terrain_area(tx, ty, clip);
        }
    }
    a / (view[0] * view[1]).max(1.0)
}

/// Whether grounded frame `fi` is inside the landing grace: the player landed
/// on a new height (feet more than 1 px from the last grounded frame before
/// this grounded stretch) at most `land_grace_s` before it, and within the
/// grace after that landing the camera moves towards the feet (up when they
/// sit too high, `up`; down otherwise). A vertical dead zone only follows
/// after a landing, so the feet are briefly out of the zone by design.
fn in_landing_grace(s: &crate::scene::Scene, fi: usize, up: bool) -> bool {
    let grace = s.limits.land_grace_s;
    if grace <= 0.0 || !s.frames[fi].grounded {
        return false;
    }
    let mut land = fi;
    while land > 0 && s.frames[land - 1].grounded {
        land -= 1;
    }
    let lf = &s.frames[land];
    if s.frames[fi].t - lf.t > grace {
        return false;
    }
    let Some(before) = s.frames[..land].iter().rev().find(|f| f.grounded) else { return false };
    if (feet(before) - feet(lf)).abs() <= 1.0 {
        return false;
    }
    // Moving towards: at some point in the grace the camera has gone at
    // least half a pixel the right way (it may turn back later, e.g. when
    // the player jumps on).
    let sign = if up { -1.0 } else { 1.0 };
    s.frames[land..].iter().take_while(|f| f.t - lf.t <= grace).any(|f| (f.cam[1] - lf.cam[1]) * sign > 0.5)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    TooHigh,
    NoGround,
    Terrain,
}

impl Kind {
    fn slug(self) -> &'static str {
        match self {
            Kind::TooHigh => "player_too_high",
            Kind::NoGround => "ground_not_visible",
            Kind::Terrain => "terrain_heavy",
        }
    }
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let lim = &s.limits;
    let grounded: Vec<usize> = (0..s.frames.len()).filter(|&i| s.frames[i].grounded && s.frames[i].view[1] > 0.0).collect();
    if grounded.is_empty() {
        return Vec::new();
    }
    // Typical spacing between sampled frames, for run durations.
    let spacing = if s.frames.len() > 1 {
        let (a, b) = (&s.frames[0], &s.frames[s.frames.len() - 1]);
        ((b.t - a.t) / (s.frames.len() - 1) as f32).max(0.0)
    } else {
        0.0
    };
    let mut out = Vec::new();
    for kind in [Kind::TooHigh, Kind::NoGround, Kind::Terrain] {
        // (position in `grounded`, frame index, how bad)
        let bad: Vec<(usize, usize, f32)> = grounded
            .iter()
            .enumerate()
            .filter_map(|(gi, &fi)| {
                let f = &s.frames[fi];
                let v = match kind {
                    Kind::TooHigh => lim.zone_top - feet_frac(f),
                    Kind::NoGround => feet_frac(f) - lim.zone_bottom,
                    Kind::Terrain => terrain_share(ctx, f.cam, f.view, feet(f)) - lim.max_terrain,
                };
                (v > 0.0).then_some((gi, fi, v))
            })
            .collect();
        let mut runs: Vec<Vec<(usize, usize, f32)>> = Vec::new();
        for b in bad {
            match runs.last_mut() {
                Some(r) if r.last().is_some_and(|l| l.0 + 1 == b.0) => r.push(b),
                _ => runs.push(vec![b]),
            }
        }
        for run in runs {
            if matches!(kind, Kind::TooHigh | Kind::NoGround) && run.iter().all(|r| in_landing_grace(s, r.1, kind == Kind::TooHigh)) {
                continue;
            }
            let (Some(first), Some(last)) = (run.first(), run.last()) else { continue };
            let (f0, f1) = (&s.frames[first.1], &s.frames[last.1]);
            let dur = f1.t - f0.t + spacing;
            if dur < lim.camera_min_s {
                continue;
            }
            let Some(worst) = run.iter().copied().max_by(|a, b| a.2.total_cmp(&b.2)) else { continue };
            let wf = &s.frames[worst.1];
            let px = run.iter().fold(wf.player, |acc, r| union(acc, s.frames[r.1].player));
            // Crop: the frame with drawn sprites nearest the worst one.
            let crop_fi = run.iter().filter(|r| s.frames[r.1].sprites.is_some()).min_by_key(|r| r.1.abs_diff(worst.1)).map_or(worst.1, |r| r.1);
            let cf = &s.frames[crop_fi];
            let (vw, vh) = (cf.view[0], cf.view[1]);
            let zone: Rect = [cf.cam[0], cf.cam[1] + lim.zone_top * vh, vw, (lim.zone_bottom - lim.zone_top) * vh];
            let below: Rect = [cf.cam[0], feet(cf), vw, (cf.cam[1] + vh - feet(cf)).max(0.0)];
            let span = format!("for {} s from tick {} to {} (x {} to {})", fmt_px(dur), f0.tick, f1.tick, fmt_px(px[0]), fmt_px(px[0] + px[2]));
            let pct = |v: f32| format!("{:.0} %", v * 100.0);
            let map_h = s.map[1] as f32 * s.tile_size;
            let rows_for = |px: f32| (px / s.tile_size).ceil() as i32;
            let sev = match kind {
                Kind::NoGround => 3,
                _ => 2,
            };
            let (msg, fix, hl) = match kind {
                Kind::TooHigh => {
                    let want = feet(wf) - lim.zone_top * wf.view[1];
                    let rise = wf.cam[1] - want;
                    let fix = if want < 0.0 {
                        format!(
                            "The camera would have to rise {} px, but the map top stops it: the level is {} rows ({} px) and the view {} px tall. Add {} rows of sky above the map (or let the camera show sky above row 0) so the ground anchor can sit in the lower third.",
                            fmt_px(rise),
                            s.map[1],
                            fmt_px(map_h),
                            fmt_px(wf.view[1]),
                            rows_for(-want)
                        )
                    } else {
                        format!("Raise the camera's follow point {} px so the feet sit in the lower third.", fmt_px(rise.ceil()))
                    };
                    (
                        format!(
                            "The player stands with feet at {} of the screen height {span} (worst at tick {}, {}); the framing zone is the lower third ({} to {}).",
                            pct(feet_frac(wf)),
                            wf.tick,
                            pct(feet_frac(wf)),
                            pct(lim.zone_top),
                            pct(lim.zone_bottom)
                        ),
                        fix,
                        vec![cf.player, zone],
                    )
                }
                Kind::NoGround => {
                    let drop = feet(wf) - (wf.cam[1] + lim.zone_bottom * wf.view[1]);
                    (
                        format!("The player's feet sit at {} of the screen height {span}: the ground under them is cut off (worst at tick {}).", pct(feet_frac(wf)), wf.tick),
                        format!("Lower the camera {} px so the ground under the player shows.", fmt_px(drop.ceil())),
                        vec![cf.player, zone],
                    )
                }
                Kind::Terrain => {
                    let share = terrain_share(ctx, wf.cam, wf.view, feet(wf));
                    let k = (1..=wf.view[1].ceil() as i32).find(|&k| terrain_share(ctx, [wf.cam[0], wf.cam[1] - k as f32], wf.view, feet(wf)) <= lim.max_terrain);
                    let fix = match k {
                        Some(k) if wf.cam[1] - (k as f32) < 0.0 => format!(
                            "Raise the camera {k} px to bring it under {}; the map top stops it there, so add {} rows of sky above the map, or thin the ground under this stretch.",
                            pct(lim.max_terrain),
                            rows_for(k as f32 - wf.cam[1])
                        ),
                        Some(k) => format!("Raise the camera {k} px here (frame more sky), or thin the ground under this stretch, to bring terrain under {}.", pct(lim.max_terrain)),
                        None => "Thin the ground mass under this stretch or zoom the camera in.".to_string(),
                    };
                    (
                        format!("Terrain below the player fills {} of the screen {span} (worst at tick {}); the limit is {}.", pct(share), wf.tick, pct(lim.max_terrain)),
                        fix,
                        vec![cf.player, below],
                    )
                }
            };
            out.push(
                Issue::new(Check::Camera, kind.slug(), px)
                    .object("camera")
                    .tile(ctx.tile_of([wf.player[0] + wf.player[2] * 0.5, feet(wf) - 0.5]))
                    .tick(wf.tick)
                    .sev(sev, worst.2 * dur.max(0.1))
                    .msg(msg, fix)
                    .crop(CropSource::Frame(crop_fi, None), hl),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::testkit::ground;
    use crate::scene::{FrameDef, Scene};

    /// A view 160 x 90 px; the player 16 px tall standing on the floor.
    fn frames(s: &mut Scene, cam_y: f32, floor_y: f32, n: u64) {
        for i in 0..n {
            s.frames.push(FrameDef {
                tick: i * 6,
                t: i as f32 * 0.1,
                scale: 4.0,
                cam: [0.0, cam_y],
                view: [160.0, 90.0],
                player: [40.0, floor_y - 16.0, 12.0, 16.0],
                grounded: true,
                sprites: None,
            });
        }
    }

    #[test]
    fn feet_in_the_lower_third_with_little_ground_pass() {
        // Floor top at y 64, two rows of ground; the view 90 tall from y 0:
        // feet at 71 %, terrain 26 px of 90 below the feet (29 %).
        let mut s = ground(12, 6, 4);
        frames(&mut s, 0.0, 64.0, 10);
        let v = check(&Ctx::new(&s, None));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn a_high_player_and_heavy_terrain_fail_as_one_run_each() {
        // Floor top at y 32 in a 6-row map: feet at 36 % of the view, ground
        // fills 58 px of 90 under them.
        let mut s = ground(12, 6, 2);
        frames(&mut s, 0.0, 32.0, 10);
        let v = check(&Ctx::new(&s, None));
        let kinds: Vec<&str> = v.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(kinds, vec!["player_too_high", "terrain_heavy"], "{v:?}");
        // The camera sits at the map top, so the fix asks for sky rows.
        assert!(v[0].fix.contains("rows of sky"), "{}", v[0].fix);
        // A single short blip is not reported.
        let mut s2 = ground(12, 6, 2);
        frames(&mut s2, 0.0, 32.0, 1);
        assert!(check(&Ctx::new(&s2, None)).is_empty());
    }

    /// Stand at feet 64 (71 %), jump, land at feet 54 (60 %: too high) and
    /// stand there `after` frames with the camera rising `rise` px a frame.
    fn landing(after: u64, rise: f32) -> Scene {
        let mut s = ground(12, 6, 4);
        let mut push = |i: u64, cam_y: f32, feet: f32, grounded: bool| {
            s.frames.push(FrameDef {
                tick: i * 6,
                t: i as f32 * 0.1,
                scale: 4.0,
                cam: [0.0, cam_y],
                view: [160.0, 90.0],
                player: [40.0, feet - 16.0, 12.0, 16.0],
                grounded,
                sprites: None,
            });
        };
        for i in 0..5 {
            push(i, 0.0, 64.0, true);
        }
        push(5, 0.0, 40.0, false);
        push(6, 0.0, 45.0, false);
        let mut cam = 0.0;
        for i in 0..after {
            push(7 + i, cam, 54.0, true);
            cam -= rise;
        }
        s
    }

    fn too_high(s: &Scene) -> usize {
        check(&Ctx::new(s, None)).iter().filter(|i| i.kind == "player_too_high").count()
    }

    #[test]
    fn a_camera_catching_up_after_landing_on_a_new_height_has_a_grace() {
        // 0.7 s out of the zone while the camera rises: within the 1 s grace.
        assert_eq!(too_high(&landing(7, 0.5)), 0);
        // The camera does not move towards the feet: reported.
        assert_eq!(too_high(&landing(7, 0.0)), 1);
        // Out of the zone longer than the grace (camera too slow): reported.
        assert_eq!(too_high(&landing(15, 0.1)), 1);
        // The grace is configurable; 0 turns it off.
        let mut s = landing(7, 0.5);
        s.limits.land_grace_s = 0.0;
        assert_eq!(too_high(&s), 1);
    }

    #[test]
    fn feet_cut_off_at_the_bottom_fail() {
        let mut s = ground(12, 8, 6); // floor top at y 96
        frames(&mut s, 10.0, 96.0, 5); // view 10..100: feet at 96 %
        s.limits.zone_bottom = 0.9; // the zone ends at 90 %
        let v = check(&Ctx::new(&s, None));
        assert!(v.iter().any(|i| i.kind == "ground_not_visible"), "{v:?}");
    }
}
