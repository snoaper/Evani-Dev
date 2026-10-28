//! The kit's own review checks (Chris's review of kit 002): camera framing,
//! floating text, coin placement and reachability, and the pixel grid. They
//! play the level with the lookahead bot and measure what a 1920 x 1080
//! screen would show.

use std::rc::Rc;

use crate::bot::{self, Run, Sim};
use crate::draw::{Screen, ViewRule, camera};
use crate::game::{Game, Phase};
use crate::level::{Level, TILE, Tile};
use crate::{Ints, LEVEL_JSON, Tune};

/// The capture size the review uses.
pub const REVIEW_PX: [f32; 2] = [1920.0, 1080.0];

pub fn level() -> Rc<Level> {
    Rc::new(Level::from_json(LEVEL_JSON).unwrap())
}

pub fn sim<'a>(t: &'a Tune, n: &'a Ints) -> Sim<'a> {
    Sim { t, n, gravity: 980.0, dt: 1.0 / 60.0 }
}

/// Play the level with the bot, calling `on_tick` with the game before
/// every tick.
pub fn bot_run(t: &Tune, n: &Ints, on_tick: impl FnMut(usize, &Game)) -> (Game, Run) {
    let mut g = Game::new(level(), t, n);
    g.reset(Phase::Playing, t, n);
    let run = bot::play(&mut g, sim(t, n), 60 * 150, 30, on_tick);
    (g, run)
}

/// Terrain (solid ground, slopes, water and spikes) on screen below the
/// hero's feet line, as a fraction of the view, sampled every 2 art px.
pub fn terrain_below_feet(g: &Game, s: &Screen) -> f32 {
    let feet = g.hero.body.pos[1];
    let (mut n, mut hit) = (0u32, 0u32);
    let mut y = s.cam[1] + 1.0;
    while y < s.cam[1] + s.view[1] {
        let mut x = s.cam[0] + 1.0;
        while x < s.cam[0] + s.view[0] {
            n += 1;
            if y >= feet && !matches!(g.level.tile_at(x, y), Tile::Empty | Tile::Decoration { .. } | Tile::OneWay { .. }) {
                hit += 1;
            }
            x += 2.0;
        }
        y += 2.0;
    }
    if n == 0 { 0.0 } else { hit as f32 / n as f32 }
}

/// Is world pixel `(x, y)` terrain (solid, the filled part of a slope,
/// water or spikes)?
pub fn terrain_px(l: &Level, x: f32, y: f32) -> bool {
    let tile = l.tile_at(x, y);
    match tile {
        Tile::Solid { .. } | Tile::Water { .. } | Tile::Spikes => true,
        Tile::Slope { .. } => {
            let fx = (x / TILE).fract();
            let bottom = ((y / TILE).floor() + 1.0) * TILE;
            tile.slope_height(fx).is_some_and(|h| y >= bottom - h)
        }
        _ => false,
    }
}

/// The level's coins in runs: neighbours in the list less than 1.6 tiles
/// apart in x and 2 in y.
pub fn coin_groups(l: &Level) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    for (k, c) in l.coins.iter().enumerate() {
        match out.last_mut() {
            Some(g) if {
                let p = l.coins[g[g.len() - 1]];
                (c[0] - p[0]).abs() < 1.6 * TILE && (c[1] - p[1]).abs() < 2.0 * TILE
            } =>
            {
                g.push(k)
            }
            _ => out.push(vec![k]),
        }
    }
    out
}

/// Coin `k` in level.json units (tiles).
fn coin_tile(l: &Level, k: usize) -> [f32; 2] {
    let c = l.coins[k];
    [c[0] / TILE - 0.5, c[1] / TILE - 0.5]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Framing (point 1): at 1920 x 1080, once the hero has stood on the
    /// ground for a second, terrain below the feet covers at most 35 % of
    /// the screen and the hero sits in the lower part of the frame; and the
    /// hero is always well inside the frame.
    #[test]
    fn camera_framing_keeps_the_ground_low_and_the_hero_in_frame() {
        let (t, n) = (Tune::default(), Ints::default());
        let mut s = Screen::fit(REVIEW_PX, REVIEW_PX, ViewRule::of(&t), true);
        assert_eq!(s.scale, 4.0);
        let (mut checked, mut worst) = (0usize, 0.0f32);
        let mut out = Vec::new();
        let mut bad = Vec::new();
        let sim = sim(&t, &n);
        let (_, run) = bot_run(&t, &n, |i, g| {
            if !g.hero.alive() || g.phase != Phase::Playing {
                return;
            }
            s.cam = camera(g, &s, &t, 1.0);
            let p = g.hero.body.pos;
            let (sx, sy) = (p[0] - s.cam[0], p[1] - s.cam[1]);
            // Always inside the frame with a margin.
            if sx < 24.0 || sx > s.view[0] - 24.0 || sy - t.hero_h < 16.0 || sy > s.view[1] - 8.0 {
                out.push((i, sx, sy));
            }
            // Every 5 ticks on the ground: stand still for 1.2 s on
            // a copy and look at the settled frame.
            if i % 5 != 0 || !g.hero.body.grounded {
                return;
            }
            let mut c = g.clone();
            for _ in 0..72 {
                c.tick(crate::game::Controls::default(), sim.t, sim.n, sim.gravity, sim.dt);
            }
            if !c.hero.alive() || !c.hero.body.grounded || c.phase != Phase::Playing {
                return;
            }
            let mut cs = s;
            cs.cam = camera(&c, &cs, &t, 1.0);
            checked += 1;
            let f = terrain_below_feet(&c, &cs);
            worst = worst.max(f);
            let fy = (c.hero.body.pos[1] - cs.cam[1]) / cs.view[1];
            // Feet in the lower part of the frame (the framing zone, about
            // 72 %) everywhere: the level has soil rows under its lowest
            // ground, so the view never has to rest on the level's bottom
            // (critic round 2, item 1: the valley sat at 93 %).
            let at_bottom = cs.cam[1] + cs.view[1] >= c.level.height_px() - 0.5;
            if f > 0.35 || at_bottom || !(0.6..=0.8).contains(&fy) {
                bad.push((i, c.hero.body.pos, f, fy));
            }
        });
        assert!(run.won_at.is_some(), "the bot did not finish");
        assert!(out.is_empty(), "hero near the frame edge: {:?}", &out[..out.len().min(8)]);
        assert!(bad.is_empty(), "settled frames off (tick, feet, terrain, feet y / view): {:?}", &bad[..bad.len().min(8)]);
        assert!(checked > 100, "too few settled frames checked ({checked})");
        eprintln!("framing: {checked} settled frames, worst terrain {:.0} %", worst * 100.0);
    }

    /// The vertical dead zone holds the camera still through a jump from
    /// flat ground.
    #[test]
    fn camera_holds_still_through_a_jump() {
        let (t, n) = (Tune::default(), Ints::default());
        let mut g = Game::new(level(), &t, &n);
        g.reset(Phase::Playing, &t, &n);
        let sim = sim(&t, &n);
        let c = |jump: bool, pressed: bool| crate::game::Controls { x: 0.0, jump, jump_pressed: pressed, run: false, down: false };
        for _ in 0..90 {
            g.tick(c(false, false), sim.t, sim.n, sim.gravity, sim.dt);
        }
        let ground = g.cam.focus[1];
        g.tick(c(true, true), sim.t, sim.n, sim.gravity, sim.dt);
        for _ in 0..60 {
            g.tick(c(true, false), sim.t, sim.n, sim.gravity, sim.dt);
            assert!((g.cam.focus[1] - ground).abs() < 0.01, "camera moved during the jump");
        }
        let _ = TILE;
    }

    /// Floating text (point 3), over a whole bot run at 1920 x 1080: never
    /// on the hero, never on each other, gone within a second, on whole
    /// art pixels; coin lines merge into running totals.
    #[test]
    fn floating_text_stays_clear_merges_and_fades() {
        use crate::draw::{TEXT_GAP, hero_box, text_pops};
        use crate::game::PopKind;
        let (t, n) = (Tune::default(), Ints::default());
        let mut s = Screen::fit(REVIEW_PX, REVIEW_PX, ViewRule::of(&t), true);
        let (mut frames, mut most, mut merged, mut bad) = (0usize, 0usize, 0u32, Vec::new());
        let coin = n.score_coin.max(0) as u32;
        let (_, run) = bot_run(&t, &n, |i, g| {
            s.cam = camera(g, &s, &t, 1.0);
            for p in &g.pops {
                assert!(p.t < 1.0, "pop alive {} s", p.t);
                if let PopKind::Score(v) = p.kind
                    && v > coin.max(n.score_stomp.max(0) as u32)
                {
                    merged = merged.max(v);
                }
            }
            let pops = text_pops(g, s.cam, s.view[0], 1.0);
            if pops.is_empty() {
                return;
            }
            frames += 1;
            most = most.max(pops.len());
            let hero = hero_box(g, s.cam, 1.0);
            for (k, p) in pops.iter().enumerate() {
                let b = p.at;
                if b.x.fract() != 0.0 || b.y.fract() != 0.0 {
                    bad.push((i, "off the pixel grid"));
                }
                if g.hero.alive() && b.hits(&hero, TEXT_GAP - 0.5) {
                    bad.push((i, "on the hero"));
                }
                if pops[..k].iter().any(|o| b.hits(&o.at, TEXT_GAP - 0.5)) {
                    bad.push((i, "on another pop"));
                }
            }
        });
        assert!(run.won_at.is_some());
        assert!(bad.is_empty(), "floating text: {:?}", &bad[..bad.len().min(8)]);
        assert!(frames > 100, "too few frames with floating text ({frames})");
        assert!(merged >= coin * 3, "no coin line merged into a total (best {merged})");
        eprintln!("floating text: {frames} frames, at most {most} at once, best combo {merged}");
    }

    /// Scores within half a second merge into one pop that restarts its
    /// life; later ones start a new pop; text is gone by 0.8 s.
    #[test]
    fn score_pops_merge_then_expire() {
        use crate::game::{POP_TEXT_LIFE, PopKind};
        let (t, n) = (Tune::default(), Ints::default());
        let mut g = Game::new(level(), &t, &n);
        g.reset(Phase::Playing, &t, &n);
        let sim = sim(&t, &n);
        let score = |g: &Game| g.pops.iter().filter_map(|p| if let PopKind::Score(v) = p.kind { Some(v) } else { None }).collect::<Vec<_>>();
        g.add_score(10, [100.0, 100.0]);
        for _ in 0..20 {
            g.tick(crate::game::Controls::default(), sim.t, sim.n, sim.gravity, sim.dt);
        }
        g.add_score(10, [110.0, 100.0]);
        assert_eq!(score(&g), vec![20]);
        for _ in 0..40 {
            g.tick(crate::game::Controls::default(), sim.t, sim.n, sim.gravity, sim.dt);
        }
        g.add_score(100, [120.0, 100.0]);
        assert_eq!(score(&g), vec![20, 100]);
        for _ in 0..(POP_TEXT_LIFE * 60.0) as usize + 1 {
            g.tick(crate::game::Controls::default(), sim.t, sim.n, sim.gravity, sim.dt);
        }
        assert!(score(&g).is_empty());
    }

    /// Coins (point 4): branching every plan the bot knows from every
    /// sixth tick of its run reaches every coin; every arc (a run of coins
    /// that is not level) is taken in a single jump; and every coin keeps
    /// clear air from the tiles and from every decoration's drawn box.
    #[test]
    fn every_coin_is_reachable_arcs_are_one_jump_and_coins_keep_clear() {
        let (t, n) = (Tune::default(), Ints::default());
        let sim = sim(&t, &n);
        let l = level();
        let mut reached = vec![false; l.coins.len()];
        let groups = coin_groups(&l);
        let arcs: Vec<&Vec<usize>> = groups.iter().filter(|g| g.iter().any(|&k| (l.coins[k][1] - l.coins[g[0]][1]).abs() > 0.5)).collect();
        let mut arc_ok = vec![false; arcs.len()];
        let (_, run) = bot_run(&t, &n, |i, g| {
            for (k, taken) in g.coins_taken.iter().enumerate() {
                reached[k] |= *taken;
            }
            if i % 6 != 0 || !g.hero.alive() || g.phase != Phase::Playing {
                return;
            }
            for taken in bot::branch_coins(g, sim) {
                for &(k, _) in &taken {
                    reached[k] = true;
                }
                for (a, arc) in arcs.iter().enumerate() {
                    let jump = taken.iter().find(|(k, _)| *k == arc[0]).and_then(|(_, j)| *j);
                    if jump.is_some() && arc.iter().all(|k| taken.iter().any(|(c, j)| c == k && *j == jump)) {
                        arc_ok[a] = true;
                    }
                }
            }
        });
        assert!(run.won_at.is_some());
        let missed: Vec<_> = (0..l.coins.len()).filter(|&k| !reached[k]).map(|k| coin_tile(&l, k)).collect();
        let split: Vec<_> = arcs.iter().zip(&arc_ok).filter(|(_, ok)| !**ok).map(|(a, _)| coin_tile(&l, a[0])).collect();
        assert!(arcs.len() >= 3, "only {} arcs", arcs.len());
        // Clear air: a 1 px ring round the coin frame touches no tile or
        // decoration.
        let data = crate::art::ArtData::from_json(crate::ART_JSON).unwrap();
        let art = crate::art::Art::build(&data, &l, &crate::art::Colors::new(&data, None), 1);
        let cf = art.coin[0];
        let decos: Vec<[f32; 4]> = l
            .decorations
            .iter()
            .filter_map(|d| {
                let i = crate::art::DECOS.iter().position(|k| *k == d.kind)?;
                let f = art.deco[i];
                let x0 = (d.pos[0] - f.w * 0.5).round();
                Some([x0, d.pos[1] + 2.0 - f.h, x0 + f.w, d.pos[1] + 2.0])
            })
            .collect();
        let mut clash = Vec::new();
        for (k, c) in l.coins.iter().enumerate() {
            let r = [c[0] - cf.w * 0.5 - 1.0, c[1] - cf.h * 0.5 - 1.0, c[0] + cf.w * 0.5 + 1.0, c[1] + cf.h * 0.5 + 1.0];
            if decos.iter().any(|d| r[0] < d[2] && d[0] < r[2] && r[1] < d[3] && d[1] < r[3]) {
                clash.push((coin_tile(&l, k), "decoration"));
            }
            let mut y = r[1];
            while y <= r[3] {
                let mut x = r[0];
                while x <= r[2] {
                    if terrain_px(&l, x, y) {
                        clash.push((coin_tile(&l, k), "tile"));
                        y = f32::MAX;
                        break;
                    }
                    x += 1.0;
                }
                y += 1.0;
            }
        }
        assert!(
            missed.is_empty() && split.is_empty() && clash.is_empty(),
            "coins nobody reaches (tile coords): {missed:?}; arcs no single jump takes, starting at: {split:?}; coins without clear air: {clash:?}"
        );
        eprintln!("coins: {} reached, {} arcs each in one jump", l.coins.len(), arcs.len());
    }

    /// The layout pushes text up off the hero and off each other.
    #[test]
    fn layout_pushes_text_clear() {
        use crate::draw::{TEXT_GAP, TextBox, layout_text};
        let hero = TextBox { x: 100.0, y: 100.0, w: 16.0, h: 23.0 };
        let b = TextBox { x: 102.0, y: 110.0, w: 10.0, h: 8.0 };
        let out = layout_text(&[b, b, TextBox { x: 95.0, y: 80.0, ..b }], Some(hero));
        for (k, o) in out.iter().enumerate() {
            assert!(!o.hits(&hero, TEXT_GAP - 0.5));
            assert!(out[..k].iter().all(|p| !o.hits(p, TEXT_GAP - 0.5)));
            assert_eq!(o.x, [102.0, 102.0, 95.0][k]);
        }
        assert_eq!(out[0].y, 100.0 - 1.0 - 8.0);
    }
}

/// Art preview for development: `cargo test -p evani-kit-2d-platformer
/// dump_tiles -- --ignored` writes the level's tiles (kit colours, 1 px per
/// art pixel) as raw RGBA to `target/platformer-tiles.rgba` with its size
/// in `target/platformer-tiles.txt`.
#[cfg(test)]
mod preview {
    use super::*;
    use crate::art::{Art, ArtData, Colors, TileArt};

    /// Coin arcs for development: `cargo test -p evani-kit-2d-platformer
    /// jump_paths -- --ignored --nocapture` prints every airborne stretch of
    /// the bot's run (hero centre in tile units every half tile), the jumps
    /// the level's coin arcs are placed on.
    #[test]
    #[ignore]
    fn jump_paths() {
        let (t, n) = (Tune::default(), Ints::default());
        let mut air: Vec<[f32; 2]> = Vec::new();
        let print = |air: &mut Vec<[f32; 2]>| {
            if air.len() > 8 {
                let mut s = String::new();
                let mut next = (air[0][0] * 2.0).ceil() / 2.0;
                for w in air.windows(2) {
                    while next <= w[1][0] && next >= w[0][0] && w[1][0] > w[0][0] {
                        let k = (next - w[0][0]) / (w[1][0] - w[0][0]);
                        s += &format!(" [{next},{:.2}]", w[0][1] + (w[1][1] - w[0][1]) * k);
                        next += 0.5;
                    }
                }
                eprintln!("jump x {:.2}..{:.2}:{s}", air[0][0], air[air.len() - 1][0]);
            }
            air.clear();
        };
        bot_run(&t, &n, |_, g| {
            let p = g.hero.body.pos;
            if g.hero.body.grounded || !g.hero.alive() {
                print(&mut air);
            } else {
                air.push([p[0] / TILE - 0.5, (p[1] - t.hero_h * 0.5) / TILE - 0.5]);
            }
        });
    }

    #[test]
    #[ignore]
    fn dump_tiles() {
        let data = ArtData::from_json(crate::ART_JSON).unwrap();
        let level = level();
        let art = Art::build(&data, &level, &Colors::new(&data, None), 1);
        let (w, h) = (level.w * 16, level.h * 16);
        let mut out = vec![0u8; w * h * 4];
        // Sky behind.
        for px in out.chunks_mut(4) {
            px.copy_from_slice(&[0x90, 0xC0, 0xF0, 255]);
        }
        let sh = &art.sheet;
        for ty in 0..level.h {
            for tx in 0..level.w {
                let cell = match art.tile(tx as i32, ty as i32) {
                    TileArt::Still(c) | TileArt::Cycle(c) | TileArt::Back(c) => c,
                    TileArt::None => continue,
                };
                let [fx, fy, fw, fh] = sh.frames[(cell - evani_render::SpriteSheet::cell(0)) as usize];
                for y in 0..fh as usize {
                    for x in 0..fw as usize {
                        let s = ((fy as usize + y) * sh.width as usize + fx as usize + x) * 4;
                        if sh.rgba8[s + 3] == 0 {
                            continue;
                        }
                        let d = ((ty * 16 + y) * w + tx * 16 + x) * 4;
                        out[d..d + 4].copy_from_slice(&sh.rgba8[s..s + 4]);
                    }
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        std::fs::write(dir.join("platformer-tiles.rgba"), &out).unwrap();
        std::fs::write(dir.join("platformer-tiles.txt"), format!("{w} {h}")).unwrap();
    }
}
