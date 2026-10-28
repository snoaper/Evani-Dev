//! Check 8: draw joins. Sprites of one layer that meet edge to edge (a
//! parallax strip repeated across the view, tiles, sky bands, a plane and
//! the fill under it) must meet with no gap at every camera position. A gap
//! of one or two art pixels between them shows whatever is behind as a thin
//! line across the screen, which flickers or runs along as the camera moves
//! (`join_gap`).
//!
//! The check works from the drawn sprites, not from pictures: per frame and
//! per world layer it rasterises every sprite's rect (coverage) and its
//! atlas pixels (opacity) on the art-pixel grid, then looks for runs of at
//! most `join_gap_px` uncovered pixels with opaque art of the same layer on
//! both sides, joined into lines at least `join_line_px` long. A pixel that
//! is inside a sprite's rect is never a gap (transparent pixels inside art
//! are drawn that way on purpose); only space between sprites counts, and
//! only where the art on both sides reaches the join. Only the part of a
//! line the player sees counts: gap pixels under opaque art of a nearer
//! layer (the ground, the HUD) are hidden, and a line with fewer than
//! `join_line_px` pixels left in view is not reported in that frame.
//!
//! It reads the sampled `frames` and, above all, the `sweep` frames: the
//! dump draws one frame per whole-art-pixel camera step, because a join that
//! breaks through rounding breaks only at some camera positions (for
//! example where a parallax offset lands on exactly half a pixel).
//!
//! Limits: art-pixel grid only (pixel-art scenes; a smooth-art scene skips
//! it); sprite tints under 0.5 alpha count as see-through; HUD layers are
//! skipped (tag a world layer's intended thin line, such as an outline
//! drawn as a gap, by its object name `"<layer name>: ..."`).

use super::fmt_px;
use crate::scene::{FrameDef, Rect, SpriteDef};
use crate::{Check, CropSource, Ctx, Issue};
use std::collections::BTreeMap;

/// One gap pixel run: where (grid px), how wide, and the sprites that
/// should have met there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Hit {
    /// Position across the gap (column for a vertical line).
    across: i32,
    /// Width of the gap.
    n: i32,
    /// Position along the line (row for a vertical line).
    along: i32,
    /// Sprite indices on each side.
    a: usize,
    b: usize,
}

/// A gap line in one frame, in that frame's grid pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub layer: i32,
    /// True: a vertical line (a gap between sprites side by side).
    pub vertical: bool,
    /// Grid rect `[x, y, w, h]` (art px from the view's top-left).
    pub rect: Rect,
    /// Sprite indices on each side of the gap.
    pub a: usize,
    pub b: usize,
}

/// What a sprite is, for messages and grouping.
fn label(sp: &SpriteDef, fills: &[usize]) -> String {
    match (sp.f, sp.builtin) {
        (Some(f), _) if fills.contains(&f) => "a fill".to_string(),
        (Some(f), _) => format!("frame {f}"),
        (None, Some(b)) => format!("built-in cell {b}"),
        (None, None) => "a sprite".to_string(),
    }
}

/// Gap lines in one frame (see the module docs).
pub fn frame_lines(ctx: &Ctx, f: &FrameDef) -> Vec<Line> {
    let s = ctx.scene;
    let Some(sprites) = &f.sprites else { return Vec::new() };
    let k = f.scale.max(1e-3);
    let (gw, gh) = (f.view[0].ceil().max(0.0) as i32, f.view[1].ceil().max(0.0) as i32);
    if gw == 0 || gh == 0 {
        return Vec::new();
    }
    let gap_max = s.limits.join_gap_px.max(1.0).floor() as i32;
    let line_min = s.limits.join_line_px.max(1.0);
    // Every layer from the world up (HUD too: it hides what is under it),
    // nearest first, so `front` holds the opaque art in front of each layer.
    let mut layers: Vec<i32> = sprites.iter().map(|sp| sp.l).filter(|&l| l >= s.layers.world_min).collect();
    layers.sort_unstable_by(|a, b| b.cmp(a));
    layers.dedup();
    let size = (gw * gh) as usize;
    let mut front = vec![false; size];
    let mut out = Vec::new();
    for layer in layers {
        // Coverage (any sprite rect) and the opaque sprite on top per pixel.
        let mut cover = vec![false; size];
        let mut owner: Vec<Option<usize>> = vec![None; size];
        for (si, sp) in sprites.iter().enumerate().filter(|(_, sp)| sp.l == layer) {
            let (x0, y0) = ((sp.x / k).round() as i32, (sp.y / k).round() as i32);
            let (w, h) = ((sp.w.abs() / k).round() as i32, (sp.h / k).round() as i32);
            if w <= 0 || h <= 0 {
                continue;
            }
            let solid = sp.c[3] >= 0.5;
            let art = match (sp.f, ctx.atlas) {
                (Some(fid), Some(atlas)) if !atlas.is_fill(fid) => atlas.frame_size(fid).filter(|s| s[0] > 0 && s[1] > 0).map(|s| (atlas, fid, s)),
                _ => None,
            };
            for y in y0.max(0)..(y0 + h).min(gh) {
                for x in x0.max(0)..(x0 + w).min(gw) {
                    let i = (y * gw + x) as usize;
                    cover[i] = true;
                    if !solid {
                        continue;
                    }
                    let opaque = match art {
                        Some((atlas, fid, [fw, fh])) => {
                            let u = (((x - x0) as f32 + 0.5) * fw as f32 / w as f32) as u32;
                            let v = (((y - y0) as f32 + 0.5) * fh as f32 / h as f32) as u32;
                            let u = if sp.w < 0.0 { fw - 1 - u.min(fw - 1) } else { u.min(fw - 1) };
                            atlas.frame_px(fid, u, v.min(fh - 1))[3] >= 128
                        }
                        None => true,
                    };
                    if opaque {
                        owner[i] = Some(si);
                    }
                }
            }
        }
        if layer >= s.layers.hud_min {
            for (f, o) in front.iter_mut().zip(&owner) {
                *f |= o.is_some();
            }
            continue;
        }
        // Runs of uncovered pixels with opaque art of this layer on both
        // sides, across rows (vertical lines) and down columns (horizontal).
        for vertical in [true, false] {
            let (len_across, len_along) = if vertical { (gw, gh) } else { (gh, gw) };
            let at = |across: i32, along: i32| if vertical { (along * gw + across) as usize } else { (across * gw + along) as usize };
            let mut hits: Vec<Hit> = Vec::new();
            for along in 0..len_along {
                let mut c = 1;
                while c < len_across {
                    if cover[at(c, along)] {
                        c += 1;
                        continue;
                    }
                    let start = c;
                    while c < len_across && !cover[at(c, along)] {
                        c += 1;
                    }
                    let n = c - start;
                    if n <= gap_max
                        && c < len_across
                        && let (Some(a), Some(b)) = (owner[at(start - 1, along)], owner[at(c, along)])
                    {
                        hits.push(Hit { across: start, n, along, a, b });
                    }
                }
            }
            // Join hits at the same place into lines.
            hits.sort_by_key(|h| (h.across, h.n, h.along));
            let mut i = 0;
            while i < hits.len() {
                let h0 = hits[i];
                let mut j = i + 1;
                while j < hits.len() && hits[j].across == h0.across && hits[j].n == h0.n && hits[j].along == hits[j - 1].along + 1 {
                    j += 1;
                }
                // Only what the player sees: the stretch of the line that
                // nearer opaque art does not hide, at least `join_line_px`
                // pixels of it (a line hidden by the ground is not reported).
                let shown: Vec<i32> = hits[i..j].iter().filter(|h| (0..h.n).any(|d| !front[at(h.across + d, h.along)])).map(|h| h.along).collect();
                if shown.len() as f32 >= line_min
                    && let (Some(&lo), Some(&hi)) = (shown.first(), shown.last())
                {
                    let (along, shown_len) = (lo as f32, (hi - lo + 1) as f32);
                    let rect = if vertical {
                        [h0.across as f32, along, h0.n as f32, shown_len]
                    } else {
                        [along, h0.across as f32, shown_len, h0.n as f32]
                    };
                    out.push(Line { layer, vertical, rect, a: h0.a, b: h0.b });
                }
                i = j;
            }
        }
        for (f, o) in front.iter_mut().zip(&owner) {
            *f |= o.is_some();
        }
    }
    out
}

/// One reported join: every frame it shows in.
struct Agg {
    first: (CropSource, Rect, u64),
    frames: usize,
    sweep_cams: Vec<f32>,
    widest: f32,
    longest: f32,
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    if !s.pixel_art {
        return Vec::new();
    }
    let fills: &[usize] = ctx.atlas.map_or(&[], |a| a.fills.as_slice());
    let mut agg: BTreeMap<(i32, bool, String, String), Agg> = BTreeMap::new();
    let lists = [(false, &s.frames), (true, &s.sweep)];
    let mut checked = [0usize; 2];
    for (sweep, list) in lists {
        for (fi, f) in list.iter().enumerate() {
            let Some(sprites) = &f.sprites else { continue };
            checked[usize::from(sweep)] += 1;
            // One entry per join kind per frame (a strip repeated three times
            // across the view counts once).
            let mut seen: Vec<(i32, bool, String, String)> = Vec::new();
            for line in frame_lines(ctx, f) {
                let (la, lb) = (label(&sprites[line.a], fills), label(&sprites[line.b], fills));
                let key = (line.layer, line.vertical, la, lb);
                let world = [f.cam[0] + line.rect[0], f.cam[1] + line.rect[1], line.rect[2], line.rect[3]];
                let (wide, long) = if line.vertical { (line.rect[2], line.rect[3]) } else { (line.rect[3], line.rect[2]) };
                let crop = if sweep { CropSource::Sweep(fi, Some(crate::geom::inflate(world, 24.0))) } else { CropSource::Frame(fi, Some(crate::geom::inflate(world, 24.0))) };
                let a = agg.entry(key.clone()).or_insert(Agg { first: (crop, world, f.tick), frames: 0, sweep_cams: Vec::new(), widest: 0.0, longest: 0.0 });
                a.widest = a.widest.max(wide);
                a.longest = a.longest.max(long);
                if !seen.contains(&key) {
                    a.frames += 1;
                    if sweep {
                        a.sweep_cams.push(f.cam[0]);
                    }
                    seen.push(key);
                }
            }
        }
    }
    let total = checked[0] + checked[1];
    let mut out = Vec::new();
    for ((layer, vertical, la, lb), a) in agg {
        let lname = ctx.layer_name(layer);
        let (crop, rect, tick) = a.first;
        let dir = if vertical { "vertical" } else { "horizontal" };
        let side = if vertical { "side by side" } else { "one above the other" };
        let pair = if la == lb { format!("two copies of {la}") } else { format!("{la} and {lb}") };
        let cams = if a.sweep_cams.is_empty() {
            String::new()
        } else {
            let mut v = a.sweep_cams.clone();
            v.sort_by(f32::total_cmp);
            v.dedup();
            let list: Vec<String> = v.iter().take(8).map(|c| fmt_px(*c)).collect();
            let more = if v.len() > list.len() { format!(" and {} more", v.len() - list.len()) } else { String::new() };
            format!(" In the camera sweep it opens at camera x {}{more} and is closed at the other steps, so it flickers or runs along as the camera moves.", list.join(", "))
        };
        out.push(
            Issue::new(Check::Joins, "join_gap", rect)
                .object(&format!("{lname}: {pair}"))
                .tick(tick)
                .sev(3, a.frames as f32)
                .msg(
                    format!(
                        "{lname}: {pair}, drawn {side}, leave a {} art px gap where they should meet, so a thin {dir} line of what is behind shows through (up to {} art px long), in {} of {total} frames checked ({} sampled, {} camera-sweep steps).{cams}",
                        fmt_px(a.widest),
                        fmt_px(a.longest),
                        a.frames,
                        checked[0],
                        checked[1]
                    ),
                    "Snap a repeated strip once and step it by whole art pixels (each copy at snap(start) + i x width), and snap with floor(v + 0.5) rather than round-half-away-from-zero, which rounds -1.5 down to -2 but 510.5 up to 511 and so opens a 1 px gap where a strip crosses the screen's left edge. Tag it only if the gap is drawn on purpose.".into(),
                )
                // Marked two art px out, so the line itself stays visible
                // inside the mark.
                .crop(crop, vec![crate::geom::inflate(rect, 2.0)]),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Atlas, Scene};

    /// Frame 0: a 10 x 4 strip, opaque edge to edge. Frame 1: a 10 x 4
    /// frame whose last column is transparent (an outline drawn as a gap).
    /// Frame 2: a fill.
    fn atlas() -> Atlas {
        let (w, h) = (32u32, 8u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..4 {
            for x in 0..20 {
                if x == 19 {
                    continue;
                }
                let o = ((y * w + x) * 4) as usize;
                rgba[o..o + 4].copy_from_slice(&[90, 160, 90, 255]);
            }
        }
        Atlas { width: w, height: h, rgba, frames: vec![[0, 0, 10, 4], [10, 0, 10, 4], [30, 0, 1, 1]], fills: vec![2] }
    }

    fn sp(f: usize, x: f32, y: f32, w: f32, h: f32, l: i32) -> SpriteDef {
        // Device px at scale 4.
        SpriteDef { f: Some(f), x: x * 4.0, y: y * 4.0, w: w * 4.0, h: h * 4.0, l, ..SpriteDef::default() }
    }

    fn scene(sweep: Vec<Vec<SpriteDef>>) -> Scene {
        let mut s = Scene { pixel_scale: 4.0, map: [10, 10], ..Scene::default() };
        s.layers.world_min = 10;
        s.layers.hud_min = 200;
        s.layer_names.insert("10".into(), "far hills".into());
        for (i, sprites) in sweep.into_iter().enumerate() {
            s.sweep.push(FrameDef { tick: 0, scale: 4.0, cam: [i as f32, 0.0], view: [40.0, 20.0], sprites: Some(sprites), ..FrameDef::default() });
        }
        s
    }

    /// A strip of frame-0 copies across the view, copy `i` at `x0 + i * 10`
    /// rounded the way a buggy draw does (`round` per copy) or snapped once.
    fn strip(off: f32, buggy: bool) -> Vec<SpriteDef> {
        let start = if buggy { -off } else { (-off + 0.5).floor() };
        (0..5).map(|i| sp(0, (start + i as f32 * 10.0).round(), 6.0, 10.0, 4.0, 10)).collect()
    }

    #[test]
    fn a_strip_rounded_per_copy_opens_a_gap_at_half_pixel_offsets() {
        // Offsets 0, 0.25, 0.5, 0.75: -0.5 rounds away to -1, 9.5 to 10.
        let s = scene((0..4).map(|i| strip(i as f32 * 0.25 + 1.0, true)).collect());
        let a = atlas();
        let v = check(&Ctx::new(&s, Some(&a)));
        assert_eq!(v.len(), 1, "{v:?}");
        let i = &v[0];
        assert_eq!((i.check, i.kind.as_str()), (Check::Joins, "join_gap"));
        assert_eq!(i.object.as_deref(), Some("far hills: two copies of frame 0"));
        assert!(i.message.contains("1 art px gap"), "{}", i.message);
        assert!(i.message.contains("in 1 of 4 frames"), "{}", i.message);
        assert!(i.message.contains("camera x 2 and is closed"), "{}", i.message);
        // The gap: one column, the strip's 4 rows, at world x 2 + 8.
        assert_eq!(i.px, [10.0, 6.0, 1.0, 4.0]);
        // Its crop (the gap plus 24 px, inside the view: x 2..35, y 0..20)
        // is blown up to 12 output px per art px (scale 4 times 3, the
        // largest whole factor that keeps it within 480 px), so the
        // one-column gap shows as a 12 px stripe of background between the
        // strip's copies.
        let cv = crate::render::crop(&s, Some(&a), i);
        assert_eq!((cv.w, cv.h), (33 * 12, 20 * 12));
        let px = |wx: f32, wy: f32| {
            let o = ((((wy * 12.0) as u32) * cv.w + ((wx - 2.0) * 12.0) as u32) * 4) as usize;
            [cv.rgba[o], cv.rgba[o + 1], cv.rgba[o + 2]]
        };
        assert_eq!(px(10.5, 7.5), [40, 44, 52]);
        assert_eq!(px(9.5, 7.5), [90, 160, 90]);
        assert_eq!(px(11.5, 7.5), [90, 160, 90]);
    }

    #[test]
    fn a_strip_snapped_once_passes_at_every_offset() {
        let s = scene((0..8).map(|i| strip(i as f32 * 0.25, false)).collect());
        let v = check(&Ctx::new(&s, Some(&atlas())));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn a_gap_between_fills_one_above_the_other_is_a_horizontal_line() {
        // Two sky bands with a row between them.
        let s = scene(vec![vec![sp(2, 0.0, 0.0, 40.0, 5.0, 10), sp(2, 0.0, 6.0, 40.0, 5.0, 10)]]);
        let v = check(&Ctx::new(&s, Some(&atlas())));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("horizontal line"), "{}", v[0].message);
        assert_eq!(v[0].px, [0.0, 5.0, 40.0, 1.0]);
    }

    #[test]
    fn transparent_art_edges_wide_spacing_short_lines_and_hud_pass() {
        let s = scene(vec![vec![
            // Frame 1's last column is transparent: its 1 px gap to the next
            // copy is in the art (an outline), not a broken join.
            sp(1, 0.0, 0.0, 10.0, 4.0, 10),
            sp(1, 11.0, 0.0, 10.0, 4.0, 10),
            // Spacing wider than join_gap_px.
            sp(0, 0.0, 8.0, 10.0, 4.0, 10),
            sp(0, 13.0, 8.0, 10.0, 4.0, 10),
            // A gap line only 2 px long.
            sp(0, 0.0, 17.0, 10.0, 2.0, 10),
            sp(0, 11.0, 17.0, 10.0, 2.0, 10),
            // HUD glyphs a pixel apart.
            sp(0, 0.0, 0.0, 10.0, 4.0, 200),
            sp(0, 11.0, 0.0, 10.0, 4.0, 200),
        ]]);
        let v = check(&Ctx::new(&s, Some(&atlas())));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn a_gap_hidden_by_nearer_art_is_not_reported_and_a_half_hidden_one_is_trimmed() {
        // Two copies with a gap at column 10, rows 6..14.
        let gap = vec![sp(0, 0.0, 6.0, 10.0, 8.0, 10), sp(0, 11.0, 6.0, 10.0, 8.0, 10)];
        // Ground (layer 60) over all eight rows: nothing shows.
        let mut hidden = gap.clone();
        hidden.push(sp(0, 5.0, 6.0, 10.0, 8.0, 60));
        assert!(check(&Ctx::new(&scene(vec![hidden]), Some(&atlas()))).is_empty());
        // Ground over rows 2..8 only: rows 8..14 still show, so it is
        // reported, its rect trimmed to them and marked 2 px out.
        let mut half = gap;
        half.push(sp(0, 5.0, 2.0, 10.0, 6.0, 60));
        let v = check(&Ctx::new(&scene(vec![half]), Some(&atlas())));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].px, [10.0, 8.0, 1.0, 6.0], "{v:?}");
        assert_eq!(v[0].highlights, vec![[8.0, 6.0, 5.0, 10.0]]);
    }

    #[test]
    fn an_intended_gap_is_skipped_by_its_tag() {
        let mut s = scene(vec![strip(1.5, true)]);
        s.geocheck.push(crate::tags::Tag { check: "joins".into(), object: Some("far hills*".into()), reason: "test: a drawn divider".into(), ..Default::default() });
        let v = crate::run_all(&s, Some(&atlas()), &[]).unwrap_or_default();
        let joins: Vec<&Issue> = v.iter().filter(|i| i.check == Check::Joins).collect();
        assert_eq!(joins.len(), 1, "{v:?}");
        assert!(joins[0].tagged.is_some());
    }

    #[test]
    fn smooth_art_scenes_skip_the_check() {
        let mut s = scene(vec![strip(1.5, true)]);
        s.pixel_art = false;
        assert!(check(&Ctx::new(&s, Some(&atlas()))).is_empty());
    }
}
