//! Check 1: collectibles, enemies and props must not overlap solid tiles
//! or decor beyond `overlap_px`; collectibles need `air_px` of clear air
//! (no terrain and no opaque decor pixel) all round. Objects that share an
//! `exclusive` group must not overlap each other beyond `overlap_px` either
//! (`overlap_object`), whatever their kind.
//!
//! Works on opaque pixels when the scene has an atlas (a coin in the
//! transparent corner of a tree's frame is fine), else on rects. Terrain
//! is the tile shapes (full tiles, slopes under their line, the top 4 px of
//! one-ways), not tile art.

use super::{fmt_px, fmt_rect};
use crate::geom::{Mask, inflate, overlap_box, overlaps, union};
use crate::scene::Kind;
use crate::{Check, CropSource, Ctx, Issue};

const TOP_BAND: f32 = 4.0;

/// Smallest move (up, left, right, down; up to 32 px) after which
/// `blocked` no longer holds for the shifted mask.
fn nudge(m: &Mask, blocked: impl Fn(&Mask, i32, i32) -> bool) -> Option<(i32, i32)> {
    for k in 1..=32 {
        for (dx, dy) in [(0, -k), (-k, 0), (k, 0), (0, k)] {
            if !blocked(m, dx, dy) {
                return Some((dx, dy));
            }
        }
    }
    None
}

fn move_text(name: &str, d: Option<(i32, i32)>) -> String {
    match d {
        Some((0, dy)) if dy < 0 => format!("Move {name} up {} px.", -dy),
        Some((0, dy)) => format!("Move {name} down {dy} px."),
        Some((dx, _)) if dx < 0 => format!("Move {name} left {} px.", -dx),
        Some((dx, _)) => format!("Move {name} right {dx} px."),
        None => format!("Move {name} more than two tiles away, or remove it."),
    }
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let lim = &s.limits;
    let geo = &ctx.geo;
    let solid = |x: i32, y: i32| geo.solid_px(x as f32 + 0.5, y as f32 + 0.5, TOP_BAND);
    let decor: Vec<usize> = (0..s.objects.len()).filter(|i| s.objects[*i].kind == Kind::Decor).collect();
    let mut out = Vec::new();
    for (i, o) in s.objects.iter().enumerate() {
        let m = &ctx.masks[i];
        match o.kind {
            Kind::Collectible => {
                let air = lim.air_px.round().max(0.0) as i32;
                let ring = m.dilate(air);
                if let Some((b, n)) = overlap_box(&ring, solid) {
                    let inside = overlap_box(m, solid).map_or(0, |(_, n)| n);
                    let d = nudge(&ring, |r, dx, dy| r.pixels().any(|(x, y)| solid(x + dx, y + dy)));
                    let what = if inside > 0 { format!("is inside terrain ({inside} px)") } else { format!("has no {air} px of air from terrain ({n} px of its {air}-px air ring touch terrain)") };
                    out.push(
                        Issue::new(Check::Overlaps, "clear_air", o.rect)
                            .object(&o.name)
                            .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                            .sev(3, n as f32)
                            .msg(format!("{} {what}, at {}.", o.name, fmt_rect(b)), move_text(&o.name, d))
                            .crop(CropSource::Level(inflate(o.rect, 24.0)), vec![o.rect, b]),
                    );
                }
                for &j in &decor {
                    let dm = &ctx.masks[j];
                    if !overlaps(inflate(o.rect, lim.air_px), s.objects[j].rect) {
                        continue;
                    }
                    if let Some((b, n)) = overlap_box(&ring, |x, y| dm.get(x, y)) {
                        let d = nudge(&ring, |r, dx, dy| r.pixels().any(|(x, y)| dm.get(x + dx, y + dy)));
                        let inside = overlap_box(m, |x, y| dm.get(x, y)).map_or(0, |(_, n)| n);
                        let what = if inside > 0 { format!("overlaps {} by {inside} px", s.objects[j].name) } else { format!("is within {} px of {}", fmt_px(lim.air_px), s.objects[j].name) };
                        out.push(
                            Issue::new(Check::Overlaps, "clear_air", o.rect)
                                .object(&o.name)
                                .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                                .sev(3, n as f32)
                                .msg(
                                    format!("{} {what} (collectibles need clear air), at {}.", o.name, fmt_rect(b)),
                                    format!("{} Or move {} away.", move_text(&o.name, d), s.objects[j].name),
                                )
                                .crop(CropSource::Level(inflate(union(o.rect, s.objects[j].rect), 12.0)), vec![o.rect, b]),
                        );
                    }
                }
            }
            Kind::Enemy | Kind::Prop | Kind::Goal => {
                let sev = if o.kind == Kind::Enemy { 4 } else { 3 };
                if let Some((b, n)) = overlap_box(m, solid) {
                    let depth = b[2].min(b[3]);
                    if depth > lim.overlap_px {
                        let d = nudge(m, |r, dx, dy| overlap_box(r, |x, y| solid(x + dx, y + dy)).is_some_and(|(b, _)| b[2].min(b[3]) > lim.overlap_px));
                        out.push(
                            Issue::new(Check::Overlaps, "overlap_terrain", o.rect)
                                .object(&o.name)
                                .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                                .sev(sev, depth)
                                .msg(format!("{} ({}) sinks {} px into terrain ({n} px), at {}.", o.name, o.kind.as_str(), fmt_px(depth), fmt_rect(b)), move_text(&o.name, d))
                                .crop(CropSource::Level(inflate(o.rect, 24.0)), vec![o.rect, b]),
                        );
                    }
                }
                for &j in &decor {
                    if j == i || !overlaps(o.rect, s.objects[j].rect) {
                        continue;
                    }
                    let dm = &ctx.masks[j];
                    if let Some((b, n)) = overlap_box(m, |x, y| dm.get(x, y)) {
                        let depth = b[2].min(b[3]);
                        if depth > lim.overlap_px {
                            out.push(
                                Issue::new(Check::Overlaps, "overlap_decor", o.rect)
                                    .object(&o.name)
                                    .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                                    .sev(sev - 1, depth)
                                    .msg(
                                        format!("{} ({}) overlaps {} by {} px ({n} px), at {}.", o.name, o.kind.as_str(), s.objects[j].name, fmt_px(depth), fmt_rect(b)),
                                        format!("Move {} or {} apart by at least {} px.", o.name, s.objects[j].name, fmt_px(depth - lim.overlap_px)),
                                    )
                                    .crop(CropSource::Level(inflate(union(o.rect, s.objects[j].rect), 12.0)), vec![o.rect, b]),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    exclusive(ctx, &mut out);
    out
}

/// Objects in one exclusive group must not overlap each other by more than
/// `overlap_px` (pieces settled in their cells, screen panels side by side).
fn exclusive(ctx: &Ctx, out: &mut Vec<Issue>) {
    let s = ctx.scene;
    let lim = &s.limits;
    let mut groups: std::collections::BTreeMap<&str, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, o) in s.objects.iter().enumerate() {
        if let Some(g) = o.exclusive.as_deref() {
            groups.entry(g).or_default().push(i);
        }
    }
    for (g, members) in groups {
        for (k, &i) in members.iter().enumerate() {
            for &j in &members[k + 1..] {
                let (a, b) = (&s.objects[i], &s.objects[j]);
                if !overlaps(a.rect, b.rect) {
                    continue;
                }
                let bm = &ctx.masks[j];
                let Some((bx, n)) = overlap_box(&ctx.masks[i], |x, y| bm.get(x, y)) else { continue };
                let depth = bx[2].min(bx[3]);
                if depth <= lim.overlap_px {
                    continue;
                }
                out.push(
                    Issue::new(Check::Overlaps, "overlap_object", a.rect)
                        .object(&a.name)
                        .tile(ctx.tile_of([a.rect[0], a.rect[1]]))
                        .sev(3, depth)
                        .msg(
                            format!("{} overlaps {} by {} px ({n} px); both are in exclusive group \"{g}\", at {}.", a.name, b.name, fmt_px(depth), fmt_rect(bx)),
                            format!("Move {} or {} apart by at least {} px.", a.name, b.name, fmt_px(depth - lim.overlap_px)),
                        )
                        .crop(CropSource::Level(inflate(union(a.rect, b.rect), 12.0)), vec![a.rect, b.rect, bx]),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::testkit::{ground, obj};
    use crate::scene::Kind;

    fn run(s: &crate::scene::Scene) -> Vec<Issue> {
        check(&Ctx::new(s, None))
    }

    #[test]
    fn coins_in_air_pass_and_coins_in_ground_or_a_canopy_fail() {
        let mut s = ground(10, 6, 4); // floor top at y = 64
        s.objects.push(obj("coin ok", Kind::Collectible, [20.0, 40.0, 10.0, 10.0]));
        assert!(run(&s).is_empty(), "{:?}", run(&s));
        // Sunk into the floor.
        s.objects.push(obj("coin low", Kind::Collectible, [60.0, 58.0, 10.0, 10.0]));
        // Inside a tree's canopy (rect masks without an atlas).
        s.objects.push(obj("tree", Kind::Decor, [100.0, 16.0, 40.0, 48.0]));
        s.objects.push(obj("coin canopy", Kind::Collectible, [110.0, 20.0, 10.0, 10.0]));
        let v = run(&s);
        let names: Vec<_> = v.iter().map(|i| i.object.clone().unwrap_or_default()).collect();
        assert!(names.contains(&"coin low".to_string()), "{names:?}");
        assert!(names.contains(&"coin canopy".to_string()), "{names:?}");
        assert!(!names.contains(&"coin ok".to_string()));
        assert!(v.iter().all(|i| i.kind == "clear_air"));
        let low = v.iter().find(|i| i.object.as_deref() == Some("coin low")).unwrap();
        assert_eq!(low.fix, "Move coin low up 6 px.");
    }

    #[test]
    fn enemies_may_tuck_a_few_pixels_but_not_more() {
        let mut s = ground(10, 6, 4);
        s.objects.push(obj("beetle ok", Kind::Enemy, [20.0, 52.0, 12.0, 14.0])); // 2 px into the floor
        assert!(run(&s).is_empty());
        s.objects.push(obj("beetle sunk", Kind::Enemy, [60.0, 56.0, 12.0, 14.0])); // 6 px
        s.objects.push(obj("bush", Kind::Decor, [100.0, 48.0, 20.0, 16.0]));
        s.objects.push(obj("flag", Kind::Prop, [104.0, 40.0, 10.0, 24.0])); // inside the bush
        let v = run(&s);
        assert!(v.iter().any(|i| i.kind == "overlap_terrain" && i.object.as_deref() == Some("beetle sunk")));
        assert!(v.iter().any(|i| i.kind == "overlap_decor" && i.object.as_deref() == Some("flag")));
        assert!(!v.iter().any(|i| i.object.as_deref() == Some("beetle ok")));
    }

    #[test]
    fn exclusive_groups_must_not_overlap_but_other_groups_may() {
        let mut s = crate::scene::Scene { tile_size: 16.0, map: [20, 20], ..crate::scene::Scene::default() };
        let piece = |name: &str, x: f32, y: f32, g: &str| crate::scene::Object { support: Some(crate::scene::Support::None), exclusive: Some(g.into()), ..obj(name, Kind::Prop, [x, y, 20.0, 20.0]) };
        // Neighbours touching or tucked 2 px pass.
        s.objects.push(piece("a", 0.0, 0.0, "pieces"));
        s.objects.push(piece("b", 20.0, 0.0, "pieces"));
        s.objects.push(piece("c", 38.0, 0.0, "pieces"));
        assert!(run(&s).is_empty(), "{:?}", run(&s));
        // Two pieces in one cell fail.
        s.objects.push(piece("d", 2.0, 2.0, "pieces"));
        // A piece in another group may sit on top.
        s.objects.push(piece("pop", 0.0, 0.0, "pops"));
        let v = run(&s);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].kind, "overlap_object");
        assert_eq!(v[0].object.as_deref(), Some("a"));
        assert!(v[0].message.contains("overlaps d by 18 px"), "{}", v[0].message);
    }
}
