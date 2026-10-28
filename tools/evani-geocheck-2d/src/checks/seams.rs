//! Check 3: autotile seams. Within one draw layer:
//! - two tiles of different groups may touch only if one draws its edge on
//!   that side, one is a transition tile, or the groups list each other in
//!   `meets` (`seam`);
//! - two tiles of the same group must not draw an edge between them
//!   (`inner_edge`);
//! - a side listed in the group's `caps` must draw its edge where the
//!   neighbour is empty (`uncapped`). Map borders are skipped.
//!
//! Neighbouring problems of the same kind join into one issue.
//!
//! Limits: it trusts the `edges` the dump reports (what the autotiler drew);
//! it does not look at the tile art itself.

use super::runs;
use crate::scene::TileDef;
use crate::{Check, CropSource, Ctx, Issue};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    kind: &'static str,
    a: String,
    b: String,
    /// Side of the first tile: `r` (vertical seam line) or `b` (horizontal),
    /// or the open side for `uncapped`.
    side: char,
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let ts = s.tile_size;
    let mut any: HashMap<(i32, i32, i32), &TileDef> = HashMap::new();
    for t in &s.tiles {
        any.insert((t.layer, t.x, t.y), t);
    }
    let group = |t: &TileDef| t.group.clone();
    let meets = |a: &str, b: &str| s.groups.get(a).is_some_and(|g| g.meets.iter().any(|m| m == b));
    let caps = |g: &str| s.groups.get(g).map_or(String::new(), |g| g.caps.clone());
    // Seam records: key -> tile positions (of the first tile).
    let mut rec: BTreeMap<Key, Vec<(i32, i32)>> = BTreeMap::new();
    for t in &s.tiles {
        let Some(ga) = group(t) else { continue };
        for (side, opp, dx, dy) in [('r', 'l', 1, 0), ('b', 't', 0, 1)] {
            let Some(n) = any.get(&(t.layer, t.x + dx, t.y + dy)) else { continue };
            let Some(gb) = group(n) else { continue };
            let edge = t.edges.contains(side) || n.edges.contains(opp);
            if ga == gb {
                if edge {
                    rec.entry(Key { kind: "inner_edge", a: ga.clone(), b: gb, side }).or_default().push((t.x, t.y));
                }
            } else if !(edge || t.transition || n.transition || meets(&ga, &gb) || meets(&gb, &ga)) {
                rec.entry(Key { kind: "seam", a: ga.clone(), b: gb, side }).or_default().push((t.x, t.y));
            }
        }
        let c = caps(&ga);
        for (side, dx, dy) in [('t', 0, -1), ('l', -1, 0), ('r', 1, 0), ('b', 0, 1)] {
            let (nx, ny) = (t.x + dx, t.y + dy);
            if !c.contains(side) || t.edges.contains(side) || nx < 0 || ny < 0 || nx >= s.map[0] as i32 || ny >= s.map[1] as i32 {
                continue;
            }
            // A slope's side where its floor line meets the tile bottom has no
            // face to cap (the slope itself is the edge there).
            if let Some([hl, hr]) = t.slope
                && ((side == 'l' && hl <= 0.0) || (side == 'r' && hr <= 0.0))
            {
                continue;
            }
            if !any.contains_key(&(t.layer, nx, ny)) {
                rec.entry(Key { kind: "uncapped", a: ga.clone(), b: String::new(), side }).or_default().push((t.x, t.y));
            }
        }
    }
    let mut out = Vec::new();
    for (k, mut cells) in rec {
        // Group along the seam line: vertical lines join on y, horizontal on x.
        let along_y = matches!(k.side, 'r' | 'l');
        cells.sort_by_key(|&(x, y)| if along_y { (x, y) } else { (y, x) });
        let mut lines: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for (x, y) in cells {
            if along_y { lines.entry(x).or_default().push(y) } else { lines.entry(y).or_default().push(x) }
        }
        for (fixed, v) in lines {
            for (a, b) in runs(&v) {
                let (x0, y0, x1, y1) = if along_y { (fixed, a, fixed, b) } else { (a, fixed, b, fixed) };
                let n = (b - a + 1) as f32;
                let tiles_rect = [x0 as f32 * ts, y0 as f32 * ts, (x1 - x0 + 1) as f32 * ts, (y1 - y0 + 1) as f32 * ts];
                let line = match k.side {
                    'r' => [(x0 + 1) as f32 * ts - 1.0, y0 as f32 * ts, 2.0, n * ts],
                    'l' => [x0 as f32 * ts - 1.0, y0 as f32 * ts, 2.0, n * ts],
                    't' => [x0 as f32 * ts, y0 as f32 * ts - 1.0, n * ts, 2.0],
                    _ => [x0 as f32 * ts, (y0 + 1) as f32 * ts - 1.0, n * ts, 2.0],
                };
                let span = if n > 1.0 {
                    if along_y { format!("tiles x {x0}, y {y0}-{y1}") } else { format!("tiles x {x0}-{x1}, y {y0}") }
                } else {
                    format!("tile x {x0}, y {y0}")
                };
                let (msg, fix, sev, name) = match k.kind {
                    "seam" => {
                        let dir = if k.side == 'r' { format!("{} on the left, {} on the right", k.a, k.b) } else { format!("{} above {}", k.a, k.b) };
                        (
                            format!("{} meets {} with no edge, cap or transition tile ({dir}) along {} tile(s), {span}.", k.a, k.b, n as i32),
                            format!(
                                "Add transition tiles between {} and {} along this line (or a cap/edge on one side); if the two are designed to touch, list \"{}\" in group {}'s `meets`.",
                                k.a, k.b, k.b, k.a
                            ),
                            3,
                            format!("seam {}|{}", k.a, k.b),
                        )
                    }
                    "inner_edge" => (
                        format!("{} draws an edge inside its own mass along {} tile(s), {span}.", k.a, n as i32),
                        format!("Retile so the {} tiles here use interior variants.", k.a),
                        1,
                        format!("inner edge {}", k.a),
                    ),
                    _ => {
                        let side = match k.side {
                            't' => "top",
                            'l' => "left side",
                            'r' => "right side",
                            _ => "bottom",
                        };
                        (
                            format!("{}'s open {side} has no edge or cap along {} tile(s), {span}.", k.a, n as i32),
                            format!("Use the {} tile with a {side} edge here (or add a cap).", k.a),
                            2,
                            format!("uncapped {}", k.a),
                        )
                    }
                };
                let px = crate::geom::inflate(tiles_rect, if k.kind == "seam" { 8.0 } else { 0.0 });
                out.push(
                    Issue::new(Check::Seams, k.kind, px)
                        .object(&name)
                        .tile([x0, y0])
                        .sev(sev, n)
                        .msg(msg, fix)
                        .crop(CropSource::Level(crate::geom::inflate(tiles_rect, 24.0)), vec![line]),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::testkit::{earth, ground};
    use crate::scene::{Group, Shape, TileDef};

    fn stone(x: i32, y: i32, edges: &str) -> TileDef {
        TileDef { x, y, id: "B".into(), shape: Shape::Full, group: Some("stone".into()), edges: edges.into(), layer: 60, ..TileDef::default() }
    }

    #[test]
    fn a_clean_autotiled_floor_passes() {
        let s = ground(8, 6, 4);
        assert!(check(&Ctx::new(&s, None)).is_empty(), "{:?}", check(&Ctx::new(&s, None)));
    }

    #[test]
    fn a_stone_column_in_grass_without_transition_fails() {
        let mut s = ground(8, 8, 4);
        s.groups.insert("stone".into(), Group { caps: "tlr".into(), meets: vec![] });
        // Replace column x = 3 (rows 4..8) with stone; the grass tile above it at row 3.
        s.tiles.retain(|t| t.x != 3);
        for y in 4..8 {
            s.tiles.push(stone(3, y, ""));
        }
        s.tiles.push(earth(3, 3, "tlr"));
        let v = check(&Ctx::new(&s, None));
        let seams: Vec<_> = v.iter().filter(|i| i.kind == "seam").collect();
        // Earth left of the column, stone right of earth, earth above stone.
        assert_eq!(seams.len(), 3, "{v:?}");
        assert!(seams.iter().any(|i| i.message.contains("along 4 tile(s)")));
        // With a transition the seams go.
        for t in s.tiles.iter_mut().filter(|t| t.x == 3 && t.y >= 4) {
            t.transition = true;
        }
        let v = check(&Ctx::new(&s, None));
        assert!(v.iter().all(|i| i.kind != "seam"), "{v:?}");
    }

    #[test]
    fn open_sides_need_caps_and_inner_edges_are_flagged() {
        let mut s = ground(8, 6, 4);
        for t in s.tiles.iter_mut() {
            if t.y == 4 && (t.x == 2 || t.x == 3) {
                t.edges.clear(); // grass missing on top
            }
            if t.y == 5 && t.x == 5 {
                t.edges = "t".into(); // grass drawn under the floor top
            }
        }
        let v = check(&Ctx::new(&s, None));
        let un: Vec<_> = v.iter().filter(|i| i.kind == "uncapped").collect();
        assert_eq!(un.len(), 1, "{v:?}");
        assert!(un[0].message.contains("along 2 tile(s)"));
        assert!(v.iter().any(|i| i.kind == "inner_edge"));
    }

    #[test]
    fn a_slopes_zero_height_side_needs_no_cap() {
        let mut s = ground(8, 6, 4);
        // A down-right slope on the floor: its high left side faces air (a
        // real open face), its right side ends at height 0 (nothing to cap).
        s.tiles.push(TileDef { shape: Shape::Slope, slope: Some([16.0, 0.0]), ..earth(3, 3, "t") });
        let v = check(&Ctx::new(&s, None));
        let un: Vec<_> = v.iter().filter(|i| i.kind == "uncapped").collect();
        assert_eq!(un.len(), 1, "{v:?}");
        assert!(un[0].message.contains("left"), "{:?}", un[0].message);
    }
}
