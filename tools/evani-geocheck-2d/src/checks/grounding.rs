//! Check 2: no floating decor or props, and enemies spawn on solid ground.
//!
//! A grounded thing's foot is its collision box bottom (`foot` + `hitbox`)
//! when given, else the lowest opaque pixel row of its sprite (columns
//! within 2 px of its lowest pixel, so a tree's canopy doesn't count).
//! Each foot column needs a floor (full tile top, slope line, one-way top
//! or a moving platform's start position) between `sink_px` above and
//! `float_px` below it; less than `support_frac` of the columns supported
//! is an issue. Wall-mounted things (`support: "wall"`) need a tile or
//! decor behind them.

use super::fmt_px;
use crate::geom::{inflate, overlaps};
use crate::scene::{Kind, Object, Support};
use crate::{Check, CropSource, Ctx, Issue};

/// Foot columns `(x, foot y)` of object `i`.
fn foot(ctx: &Ctx, i: usize, o: &Object) -> Vec<(f32, f32)> {
    if let (Some(f), Some(hb)) = (o.foot, o.hitbox) {
        let x0 = hb[0].floor() as i32;
        let x1 = (hb[0] + hb[2]).ceil() as i32;
        return (x0..x1).map(|x| (x as f32 + 0.5, f[1])).collect();
    }
    let b = ctx.masks[i].bottoms();
    let low = b.iter().map(|p| p.1).max().unwrap_or(0);
    b.into_iter().filter(|p| p.1 >= low - 2).map(|(x, y)| (x as f32 + 0.5, (y + 1) as f32)).collect()
}

/// A floor under `x` between `y - up` and `y + down`, from tiles or platforms.
fn floor_at(ctx: &Ctx, x: f32, y: f32, up: f32, down: f32) -> Option<f32> {
    let mut best = ctx.geo.floor(x - 0.5, x + 0.5, x, y - up, y + down, Some(y - up));
    for o in ctx.scene.objects.iter().filter(|o| o.kind == Kind::Platform) {
        let top = o.rect[1];
        if x >= o.rect[0] && x < o.rect[0] + o.rect[2] && top >= y - up && top <= y + down && best.is_none_or(|b| top < b) {
            best = Some(top);
        }
    }
    best
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let lim = &s.limits;
    let mut out = Vec::new();
    for (i, o) in s.objects.iter().enumerate() {
        let (kind, sev) = match o.kind {
            Kind::Enemy => ("enemy_not_grounded", 4),
            Kind::Decor => ("floating_decor", 3),
            Kind::Prop | Kind::Goal => ("floating_prop", 3),
            _ => continue,
        };
        match o.support() {
            Support::None => {}
            Support::Wall => {
                let c = inflate([o.rect[0] + o.rect[2] * 0.25, o.rect[1] + o.rect[3] * 0.25, o.rect[2] * 0.5, o.rect[3] * 0.5], 0.0);
                let (tx0, tx1) = (ctx.geo.cell(c[0]), ctx.geo.cell(c[0] + c[2]));
                let (ty0, ty1) = (ctx.geo.cell(c[1]), ctx.geo.cell(c[1] + c[3]));
                let tile_behind = (ty0..=ty1).any(|ty| (tx0..=tx1).any(|tx| !ctx.geo.tiles_at(tx, ty).is_empty()));
                let decor_behind = s.objects.iter().enumerate().any(|(j, d)| j != i && d.kind == Kind::Decor && overlaps(c, d.rect));
                if !tile_behind && !decor_behind {
                    out.push(
                        Issue::new(Check::Grounding, kind, o.rect)
                            .object(&o.name)
                            .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                            .sev(sev, 1.0)
                            .msg(format!("{} is wall-mounted but has no wall or decor behind it.", o.name), format!("Move {} onto a wall, or put a wall tile behind it.", o.name))
                            .crop(CropSource::Level(inflate(o.rect, 24.0)), vec![o.rect]),
                    );
                }
            }
            Support::Ground => {
                let cols = foot(ctx, i, o);
                if cols.is_empty() {
                    continue;
                }
                let supported = |dx: f32, dy: f32| cols.iter().filter(|(x, y)| floor_at(ctx, x + dx, y + dy, lim.sink_px, lim.float_px).is_some()).count();
                let n = supported(0.0, 0.0);
                let frac = n as f32 / cols.len() as f32;
                if frac >= lim.support_frac {
                    continue;
                }
                // How far down is the floor under the unsupported columns?
                let gaps: Vec<f32> = cols.iter().filter_map(|(x, y)| floor_at(ctx, *x, *y, 0.0, 128.0).map(|f| f - y)).collect();
                let gap = gaps.iter().copied().fold(f32::NAN, f32::min);
                let foot_y = cols[0].1;
                let fix = if (1..=32).any(|k| supported(-(k as f32), 0.0) == cols.len()) || (1..=32).any(|k| supported(k as f32, 0.0) == cols.len()) {
                    let k = (1..=32).find(|k| supported(-(*k as f32), 0.0) == cols.len() || supported(*k as f32, 0.0) == cols.len()).unwrap_or(0);
                    let left = supported(-(k as f32), 0.0) == cols.len();
                    format!("Move {} {} {k} px so its whole foot stands on the ground.", o.name, if left { "left" } else { "right" })
                } else if gap.is_finite() && gap > 0.0 {
                    format!("Lower {} by {} px onto the ground (foot at y {}).", o.name, fmt_px(gap.ceil()), fmt_px(foot_y + gap.ceil()))
                } else {
                    format!("Put ground under {} or move it onto a floor.", o.name)
                };
                let what = if n == 0 {
                    if gap.is_finite() { format!("floats {} px above the ground", fmt_px(gap)) } else { "has no ground under it".to_string() }
                } else {
                    format!("stands on ground under only {:.0} % of its foot (overhangs)", frac * 100.0)
                };
                let foot_rect = [cols[0].0 - 0.5, foot_y - 2.0, cols.len() as f32, 4.0];
                out.push(
                    Issue::new(Check::Grounding, kind, o.rect)
                        .object(&o.name)
                        .tile(ctx.tile_of([o.rect[0] + o.rect[2] * 0.5, foot_y]))
                        .sev(sev, if gap.is_finite() { gap } else { 64.0 })
                        .msg(format!("{} ({}) {what}.", o.name, o.kind.as_str()), fix)
                        .crop(CropSource::Level(inflate(o.rect, 24.0)), vec![o.rect, foot_rect]),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::testkit::{ground, obj, remove_tile};
    use crate::scene::{Kind, Support};

    fn run(s: &crate::scene::Scene) -> Vec<Issue> {
        check(&Ctx::new(s, None))
    }

    #[test]
    fn decor_on_the_ground_passes_floating_and_overhanging_decor_fails() {
        let mut s = ground(12, 6, 4); // floor top y = 64
        s.objects.push(obj("bush ok", Kind::Decor, [16.0, 50.0, 16.0, 16.0])); // 2 px tucked in
        assert!(run(&s).is_empty(), "{:?}", run(&s));
        s.objects.push(obj("bush high", Kind::Decor, [48.0, 40.0, 16.0, 16.0])); // bottom 56: 8 px up
        remove_tile(&mut s, 8, 4);
        remove_tile(&mut s, 8, 5);
        s.objects.push(obj("rock edge", Kind::Decor, [124.0, 50.0, 16.0, 14.0])); // 124..140, pit 128..144
        let v = run(&s);
        let high = v.iter().find(|i| i.object.as_deref() == Some("bush high")).unwrap();
        assert_eq!(high.kind, "floating_decor");
        assert!(high.message.contains("floats 8 px"), "{}", high.message);
        assert!(high.fix.contains("Lower bush high by 8 px"), "{}", high.fix);
        let edge = v.iter().find(|i| i.object.as_deref() == Some("rock edge")).unwrap();
        assert!(edge.message.contains("overhangs") || edge.message.contains("no ground"), "{}", edge.message);
        assert!(edge.fix.contains("left 12 px"), "{}", edge.fix);
    }

    #[test]
    fn enemies_need_ground_under_their_feet() {
        let mut s = ground(12, 6, 4);
        let mut ok = obj("beetle ok", Kind::Enemy, [20.0, 51.0, 12.0, 14.0]);
        ok.foot = Some([26.0, 64.0]);
        ok.hitbox = Some([21.0, 54.0, 10.0, 10.0]);
        s.objects.push(ok.clone());
        assert!(run(&s).is_empty());
        let mut air = ok.clone();
        air.name = "beetle air".into();
        air.foot = Some([60.0, 30.0]);
        air.hitbox = Some([55.0, 20.0, 10.0, 10.0]);
        air.rect = [54.0, 17.0, 12.0, 14.0];
        s.objects.push(air);
        let mut torch = obj("torch", Kind::Prop, [100.0, 10.0, 6.0, 12.0]);
        torch.support = Some(Support::Wall);
        s.objects.push(torch);
        let v = run(&s);
        assert!(v.iter().any(|i| i.kind == "enemy_not_grounded" && i.object.as_deref() == Some("beetle air")));
        assert!(v.iter().any(|i| i.kind == "floating_prop" && i.object.as_deref() == Some("torch")));
        assert_eq!(v.len(), 2, "{v:?}");
    }
}
