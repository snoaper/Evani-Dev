//! Check 7: score and text popups.
//! - `popup_covers_player`: a visible popup (alpha at least
//!   `popup_min_alpha`) overlaps the player's drawn rect;
//! - `popup_overlap`: two popups visible on the same tick overlap (they
//!   should stack or merge);
//! - `popup_lingers`: a popup stays visible longer than `popup_max_s` (1 s).
//!
//! Issues of one type join per popup text (`100`, `1UP`) with a count, and
//! point at the first case.
//!
//! Limits: popups are rects from the dump (the text's drawn box); the check
//! does not read glyph pixels, so a box that only touches the player in its
//! empty corners still counts.

use super::fmt_px;
use crate::geom::{inflate, intersect, union};
use crate::scene::{Popup, Rect};
use crate::{Check, CropSource, Ctx, Issue};
use std::collections::BTreeMap;

/// The drawn frame nearest `tick`, preferring frames with sprites.
pub fn frame_near(ctx: &Ctx, tick: u64) -> Option<usize> {
    let f = &ctx.scene.frames;
    let best = |sprites: bool| (0..f.len()).filter(|&i| !sprites || f[i].sprites.is_some()).min_by_key(|&i| f[i].tick.abs_diff(tick));
    match best(true) {
        Some(i) if f[i].tick.abs_diff(tick) <= 6 => Some(i),
        _ => best(false),
    }
}

fn crop_for(ctx: &Ctx, tick: u64, around: Rect) -> CropSource {
    match frame_near(ctx, tick) {
        Some(fi) => CropSource::Frame(fi, Some(inflate(around, 28.0))),
        None => CropSource::Level(inflate(around, 28.0)),
    }
}

struct Group {
    /// The popup of the first case.
    pop: usize,
    count: usize,
    first: (u64, Rect, Rect),
    worst: f32,
}

fn visible(p: &Popup, min_alpha: f32) -> impl Iterator<Item = &crate::scene::PopSample> {
    p.samples.iter().filter(move |s| s.alpha >= min_alpha)
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let lim = &s.limits;
    let mut out = Vec::new();
    let dt = s.bot.as_ref().map_or(1.0 / 60.0, |b| if b.dt > 0.0 { b.dt } else { 1.0 / 60.0 });

    // Covers the player, and lingers.
    let mut covers: BTreeMap<String, Group> = BTreeMap::new();
    let mut lingers: BTreeMap<String, Group> = BTreeMap::new();
    for (pi, p) in s.popups.iter().enumerate() {
        let vis: Vec<_> = visible(p, lim.popup_min_alpha).collect();
        let (Some(a), Some(b)) = (vis.first(), vis.last()) else { continue };
        let life = b.t - a.t + dt;
        if life > lim.popup_max_s + dt * 0.5 {
            let g = lingers.entry(p.text.clone()).or_insert(Group { pop: pi, count: 0, first: (a.tick, a.rect, a.player.unwrap_or(a.rect)), worst: 0.0 });
            g.count += 1;
            g.worst = g.worst.max(life);
        }
        let hit = vis.iter().find_map(|v| v.player.and_then(|pr| intersect(v.rect, pr).map(|i| (v, pr, i))));
        if let Some((v, pr, i)) = hit {
            let g = covers.entry(p.text.clone()).or_insert(Group { pop: pi, count: 0, first: (v.tick, v.rect, pr), worst: 0.0 });
            g.count += 1;
            g.worst = g.worst.max(i[3]);
        }
    }
    let total = |text: &str| s.popups.iter().filter(|p| p.text == text).count();
    for (text, g) in covers {
        let (tick, rect, pr) = g.first;
        let lift = (rect[1] + rect[3]) - pr[1];
        // Highlight the popup and the player as they are in the cropped
        // frame (the nearest drawn frame may be a few ticks off).
        let (crop, hl) = match frame_near(ctx, tick) {
            Some(fi) => {
                let ft = s.frames[fi].tick;
                let near = s.popups[g.pop].samples.iter().filter(|v| v.player.is_some()).min_by_key(|v| v.tick.abs_diff(ft));
                let (r2, p2) = near.map_or((rect, pr), |v| (v.rect, v.player.unwrap_or(pr)));
                (CropSource::Frame(fi, Some(inflate(union(r2, p2), 28.0))), vec![r2, p2])
            }
            None => (CropSource::Level(inflate(union(rect, pr), 28.0)), vec![rect, pr]),
        };
        out.push(
            Issue::new(Check::Popups, "popup_covers_player", rect)
                .object(&format!("popup \"{text}\""))
                .tile(ctx.tile_of([rect[0], rect[1]]))
                .tick(tick)
                .sev(3, g.count as f32)
                .msg(
                    format!(
                        "{} of {} \"{text}\" popups are drawn over the player (first at tick {tick}, {}; up to {} px of overlap).",
                        g.count,
                        total(&text),
                        crate::checks::fmt_rect(rect),
                        fmt_px(g.worst)
                    ),
                    format!(
                        "Spawn the popup above the player's head, not at the pickup: at least {} px higher here (the top of the player's sprite minus the popup height), or push it sideways while it overlaps the player.",
                        fmt_px(lift.max(1.0))
                    ),
                )
                .crop(crop, hl),
        );
    }
    for (text, g) in lingers {
        let (tick, rect, _) = g.first;
        out.push(
            Issue::new(Check::Popups, "popup_lingers", rect)
                .object(&format!("popup \"{text}\""))
                .tile(ctx.tile_of([rect[0], rect[1]]))
                .tick(tick)
                .sev(2, g.worst)
                .msg(
                    format!("{} of {} \"{text}\" popups stay visible for up to {} s (limit {} s).", g.count, total(&text), fmt_px(g.worst), fmt_px(lim.popup_max_s)),
                    format!("Shorten the popup's life or fade it out so it is gone (alpha under {}) within {} s.", fmt_px(lim.popup_min_alpha), fmt_px(lim.popup_max_s)),
                )
                .crop(crop_for(ctx, tick, rect), vec![rect]),
        );
    }

    // Overlap between popups on the same tick.
    let mut by_tick: BTreeMap<u64, Vec<(usize, Rect)>> = BTreeMap::new();
    for (pi, p) in s.popups.iter().enumerate() {
        for v in visible(p, lim.popup_min_alpha) {
            by_tick.entry(v.tick).or_default().push((pi, v.rect));
        }
    }
    let mut seen: std::collections::BTreeSet<(usize, usize)> = Default::default();
    // (count, first tick, first rects, deepest overlap) per pair of texts.
    type PairHit = (usize, u64, Rect, Rect, f32);
    let mut pairs: BTreeMap<(String, String), PairHit> = BTreeMap::new();
    for (tick, list) in &by_tick {
        for a in 0..list.len() {
            for b in a + 1..list.len() {
                let ((pa, ra), (pb, rb)) = (list[a], list[b]);
                let Some(i) = intersect(ra, rb) else { continue };
                let key = (pa.min(pb), pa.max(pb));
                if !seen.insert(key) {
                    continue;
                }
                let (ta, tb) = (s.popups[key.0].text.clone(), s.popups[key.1].text.clone());
                let e = pairs.entry((ta, tb)).or_insert((0, *tick, ra, rb, 0.0));
                e.0 += 1;
                e.4 = e.4.max(i[3]);
            }
        }
    }
    for ((ta, tb), (count, tick, ra, rb, depth)) in pairs {
        let both = union(ra, rb);
        out.push(
            Issue::new(Check::Popups, "popup_overlap", both)
                .object(&format!("popups \"{ta}\" + \"{tb}\""))
                .tile(ctx.tile_of([both[0], both[1]]))
                .tick(tick)
                .sev(2, count as f32)
                .msg(
                    format!("{count} times a \"{ta}\" and a \"{tb}\" popup are drawn over each other (first at tick {tick}, up to {} px deep).", fmt_px(depth)),
                    format!(
                        "Stack them: start a new popup at least {} px above any live one it would cover (its height), or merge pickups within a short window into one running total.",
                        fmt_px(ra[3].max(rb[3]) + 1.0)
                    ),
                )
                .crop(crop_for(ctx, tick, both), vec![ra, rb]),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{PopSample, Scene};

    #[allow(clippy::too_many_arguments)]
    fn pop(text: &str, t0: u64, ticks: u64, x: f32, y: f32, rise: f32, fade_from: u64, player: Rect) -> Popup {
        let samples = (0..ticks)
            .map(|i| PopSample {
                tick: t0 + i,
                t: (t0 + i) as f32 / 60.0,
                rect: [x, (y - rise * i as f32 / 60.0).round(), 12.0, 5.0],
                alpha: if i < fade_from { 1.0 } else { 1.0 - (i - fade_from) as f32 / (ticks - fade_from) as f32 },
                player: Some(player),
            })
            .collect();
        Popup { id: format!("{text}@{t0}"), text: text.into(), samples }
    }

    #[test]
    fn a_popup_above_the_player_that_fades_in_time_passes() {
        let mut s = Scene::default();
        s.popups.push(pop("100", 0, 54, 40.0, 10.0, 20.0, 36, [40.0, 30.0, 12.0, 16.0]));
        s.popups.push(pop("100", 100, 54, 100.0, 10.0, 20.0, 36, [40.0, 30.0, 12.0, 16.0]));
        let v = check(&Ctx::new(&s, None));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn covering_overlapping_and_lingering_popups_fail() {
        let mut s = Scene::default();
        let player = [40.0, 30.0, 12.0, 16.0];
        // Spawns on the player.
        s.popups.push(pop("100", 0, 54, 40.0, 36.0, 20.0, 36, player));
        // A second one 3 ticks later, a pixel to the side: overlaps the first.
        s.popups.push(pop("100", 3, 54, 41.0, 36.0, 20.0, 36, player));
        // Visible for 1.5 s.
        s.popups.push(pop("1UP", 200, 90, 200.0, 0.0, 10.0, 80, [0.0, 100.0, 1.0, 1.0]));
        let v = check(&Ctx::new(&s, None));
        let kinds: Vec<&str> = v.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"popup_covers_player"), "{kinds:?}");
        assert!(kinds.contains(&"popup_overlap"), "{kinds:?}");
        assert!(kinds.contains(&"popup_lingers"), "{kinds:?}");
        let cover = v.iter().find(|i| i.kind == "popup_covers_player").unwrap();
        assert!(cover.message.starts_with("2 of 2"), "{}", cover.message);
        assert_eq!(v.iter().filter(|i| i.kind == "popup_lingers").count(), 1);
    }
}
