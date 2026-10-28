//! Check 6: bot level checks.
//! - `not_completable`: the bot's run did not reach the goal (or, without a
//!   bot trace, the reach model below cannot touch the goal).
//! - `unreachable_collectible`: a collectible the bot never touched and the
//!   reach model cannot touch either.
//! - `coin_arc`: a group of coins laid out as an arc that no real jump can
//!   follow within `arc_tol_px`.
//! - `hazard_hidden`: decor (or decoration tiles) drawn in front of a
//!   hazard covers more than `hazard_cover` of it.
//!
//! The reach model: standable spots every 8 px on every floor (full-tile
//! tops open above, one-way tops, slopes) plus along moving-platform paths,
//! where the player's collision box fits. From each reachable spot it flies
//! jumps with the scene's physics (rise gravity, `fall_mult` falling,
//! `max_fall`, the run-jump bonus, jump cuts after 2, 5, 9 and 14 ticks)
//! at seven horizontal speeds (run, walk, half walk, standing, both ways)
//! and walk-offs, and walks to neighbouring spots. The search starts at the
//! player's start and at every spot the bot stood on. A collectible counts
//! as reached when the player's box passes within half its size of its
//! centre (the kit-style pickup).
//!
//! Limits: horizontal speed is constant through a jump (no air steering),
//! the take-off is at full speed, enemies, stomps, springs and
//! drop-throughs are not modelled, and a moving platform counts as
//! standable anywhere on its path. Coin arcs are found automatically
//! (3 or more coins within 35 px of each other, highest coin inside the
//! group, at least 4 px of height difference).

use super::{fmt_px, fmt_rect};
use crate::geom::{centre, dist_point_rect, inflate, union};
use crate::scene::{Kind, Player, Rect, Shape};
use crate::{Check, CropSource, Ctx, Issue};
use std::collections::{BTreeMap, HashMap, VecDeque};

const STEP: f32 = 8.0;
const MAX_TICKS: u32 = 240;
const CUTS: [Option<u32>; 5] = [None, Some(2), Some(5), Some(9), Some(14)];
const ARC_LINK: f32 = 35.0;

#[derive(Debug, Clone, Copy)]
struct Node {
    x: f32,
    y: f32,
    plat: Option<usize>,
}

/// Something the model tries to touch: a collectible or the goal.
struct Target {
    obj: usize,
    c: [f32; 2],
    /// Pickup distance from the player's box (0 = the box must overlap the rect).
    pick: f32,
    rect: Rect,
}

/// What the reach model found.
pub struct Reach {
    /// Object indices of collectibles and goals the model touched.
    pub touched: Vec<usize>,
    /// Per object index: the nearest the player's box came (distance, box).
    pub nearest: BTreeMap<usize, (f32, Rect)>,
    pub nodes: usize,
    pub reachable: usize,
}

struct Model<'c, 'a> {
    ctx: &'c Ctx<'a>,
    p: &'c Player,
    dt: f32,
    map_w: f32,
    nodes: Vec<Node>,
    cols: HashMap<i32, Vec<usize>>,
    plat_nodes: Vec<Vec<usize>>,
    /// (platform, x, y, width, its nodes)
    plat_samples: Vec<(usize, f32, f32, f32, Vec<usize>)>,
    targets: Vec<Target>,
    target_cols: HashMap<i32, Vec<usize>>,
}

fn col(x: f32) -> i32 {
    ((x - STEP * 0.5) / STEP).round() as i32
}

impl<'c, 'a> Model<'c, 'a> {
    fn new(ctx: &'c Ctx<'a>, p: &'c Player) -> Self {
        let s = ctx.scene;
        let g = &ctx.geo;
        let dt = s.bot.as_ref().map_or(1.0 / 60.0, |b| if b.dt > 0.0 { b.dt } else { 1.0 / 60.0 });
        let mut m = Model {
            ctx,
            p,
            dt,
            map_w: g.w as f32 * g.ts,
            nodes: Vec::new(),
            cols: HashMap::new(),
            plat_nodes: vec![Vec::new(); s.platforms.len()],
            plat_samples: Vec::new(),
            targets: Vec::new(),
            target_cols: HashMap::new(),
        };
        // Floors.
        let ncols = (m.map_w / STEP).floor() as i32;
        for c in 0..ncols {
            let x = STEP * 0.5 + c as f32 * STEP;
            let tx = g.cell(x);
            for ty in 0..g.h {
                let y = match g.shape(tx, ty) {
                    (Shape::Full, _) if !g.is_full(tx, ty - 1) && g.shape(tx, ty - 1).0 != Shape::Slope => ty as f32 * g.ts,
                    (Shape::Top, _) => ty as f32 * g.ts,
                    (Shape::Slope, Some(t)) => g.slope_y(t, x),
                    _ => continue,
                };
                if m.box_ok(x, y) {
                    m.cols.entry(c).or_default().push(m.nodes.len());
                    m.nodes.push(Node { x, y, plat: None });
                }
            }
        }
        // Moving platforms: samples along the path.
        for (pi, pd) in s.platforms.iter().enumerate() {
            let mut pts: Vec<[f32; 2]> = Vec::new();
            for w in pd.path.windows(2) {
                let (a, b) = (w[0], w[1]);
                let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let n = (len / STEP).ceil().max(1.0) as i32;
                for k in 0..n {
                    let f = k as f32 / n as f32;
                    pts.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
                }
            }
            if let Some(l) = pd.path.last() {
                pts.push(*l);
            }
            for sp in pts {
                let n = (pd.width / STEP).floor().max(1.0) as i32;
                let mut ids = Vec::new();
                for j in 0..n {
                    let x = sp[0] + (j as f32 + 0.5) * pd.width / n as f32;
                    if m.box_ok(x, sp[1]) {
                        ids.push(m.nodes.len());
                        m.plat_nodes[pi].push(m.nodes.len());
                        m.nodes.push(Node { x, y: sp[1], plat: Some(pi) });
                    }
                }
                m.plat_samples.push((pi, sp[0], sp[1], pd.width, ids));
            }
        }
        // Targets.
        for (i, o) in s.objects.iter().enumerate() {
            let pick = match o.kind {
                Kind::Collectible => (o.rect[2].min(o.rect[3]) * 0.5).max(2.0),
                Kind::Goal => 0.0,
                _ => continue,
            };
            let ti = m.targets.len();
            m.targets.push(Target { obj: i, c: centre(o.rect), pick, rect: o.rect });
            for c in g.cell(o.rect[0])..=g.cell(o.rect[0] + o.rect[2]) {
                m.target_cols.entry(c).or_default().push(ti);
            }
        }
        m
    }

    fn bx(&self, x: f32, y: f32) -> Rect {
        [x - self.p.w * 0.5, y - self.p.h, self.p.w, self.p.h]
    }

    fn box_ok(&self, x: f32, y: f32) -> bool {
        let b = self.bx(x, y);
        !self.ctx.geo.rect_hits_full(b) && !self.ctx.geo.rect_hits_hazard(b)
    }

    /// Nearest terrain node to `(x, y)` within `tol` vertically, 2 columns either side.
    fn land(&self, x: f32, y: f32, tol: f32) -> Option<usize> {
        let c = col(x);
        (c - 2..=c + 2)
            .filter_map(|c| self.cols.get(&c))
            .flatten()
            .copied()
            .filter(|&i| (self.nodes[i].y - y).abs() <= tol)
            .min_by(|&a, &b| (self.nodes[a].x - x).abs().total_cmp(&(self.nodes[b].x - x).abs()))
    }

    /// Any node (terrain or platform) near a point.
    fn nearest_any(&self, x: f32, y: f32, tol: f32) -> Option<usize> {
        (0..self.nodes.len())
            .filter(|&i| (self.nodes[i].y - y).abs() <= tol && (self.nodes[i].x - x).abs() <= STEP)
            .min_by(|&a, &b| {
                let d = |i: usize| (self.nodes[i].x - x).abs() + (self.nodes[i].y - y).abs();
                d(a).total_cmp(&d(b))
            })
    }

    /// A platform top crossed while falling from `y0` to `y1`.
    fn plat_landing(&self, x: f32, y0: f32, y1: f32) -> Option<(f32, usize)> {
        let hw = self.p.w * 0.5;
        let mut best: Option<(f32, usize)> = None;
        for (_, sx, sy, w, ids) in &self.plat_samples {
            if *sy >= y0 - 0.5 && *sy <= y1 && x + hw > *sx && x - hw < sx + w {
                let Some(&n) = ids.iter().min_by(|&&a, &&b| (self.nodes[a].x - x).abs().total_cmp(&(self.nodes[b].x - x).abs())) else { continue };
                if best.is_none_or(|b| *sy < b.0) {
                    best = Some((*sy, n));
                }
            }
        }
        best
    }

    fn touch(&self, b: Rect, touched: &mut [bool], nearest: &mut [Option<(f32, Rect)>]) {
        let g = &self.ctx.geo;
        for c in g.cell(b[0]) - 3..=g.cell(b[0] + b[2]) + 3 {
            let Some(list) = self.target_cols.get(&c) else { continue };
            for &ti in list {
                let t = &self.targets[ti];
                let d = if t.pick > 0.0 { dist_point_rect(t.c, b) } else if crate::geom::overlaps(b, t.rect) { 0.0 } else { dist_point_rect(t.c, b) };
                if d < t.pick || (t.pick == 0.0 && d == 0.0) {
                    touched[ti] = true;
                }
                if nearest[ti].is_none_or(|n| d < n.0) {
                    nearest[ti] = Some((d, b));
                }
            }
        }
    }

    /// Fly one jump (or walk-off) from `n`; the landing node, if any.
    fn fly(&self, n: Node, vx0: f32, cut: Option<u32>, walk_off: bool, touched: &mut [bool], nearest: &mut [Option<(f32, Rect)>]) -> Option<usize> {
        let p = self.p;
        let g = &self.ctx.geo;
        let hw = p.w * 0.5;
        let (mut x, mut y, mut vx) = (n.x, n.y, vx0);
        let bonus = if vx0.abs() > p.walk_speed + 1.0 { p.run_jump_bonus } else { 0.0 };
        let mut vy = if walk_off { 0.0 } else { -(p.jump_speed + bonus) };
        let mut on_ground = walk_off;
        let bottom = g.h as f32 * g.ts;
        for k in 1..=MAX_TICKS {
            if !walk_off && cut == Some(k) && vy < 0.0 {
                vy *= p.jump_cut;
            }
            let gr = if vy > 0.0 { p.gravity * p.fall_mult } else { p.gravity };
            vy = (vy + gr * self.dt).min(p.max_fall);
            let nx = (x + vx * self.dt).clamp(hw, self.map_w - hw);
            if vx != 0.0 && g.rect_hits_full(self.bx(nx, y)) {
                vx = 0.0;
            } else {
                x = nx;
            }
            let ny = y + vy * self.dt;
            if vy <= 0.0 {
                if g.rect_hits_full(self.bx(x, ny)) {
                    vy = 0.0;
                } else {
                    y = ny;
                }
            } else {
                let floor = g.floor(x - hw, x + hw, x, y, ny, Some(y));
                let plat = self.plat_landing(x, y, ny);
                let land_terrain = match (floor, plat) {
                    (Some(f), Some((py, _))) => (f <= py).then_some(f),
                    (Some(f), None) => Some(f),
                    _ => None,
                };
                if let Some(fy) = land_terrain {
                    if on_ground && (fy - y).abs() <= 1.0 {
                        // Still walking towards the edge.
                        y = fy;
                        vy = 0.0;
                        if (x - n.x).abs() > 2.0 * STEP {
                            return None;
                        }
                        self.touch(self.bx(x, y), touched, nearest);
                        continue;
                    }
                    self.touch(self.bx(x, fy), touched, nearest);
                    return self.land(x, fy, 3.0);
                }
                if let Some((py, node)) = plat {
                    self.touch(self.bx(x, py), touched, nearest);
                    return Some(node);
                }
                on_ground = false;
                if g.rect_hits_full(self.bx(x, ny)) {
                    vy = 0.0;
                } else {
                    y = ny;
                }
            }
            let b = self.bx(x, y);
            self.touch(b, touched, nearest);
            if g.rect_hits_hazard(b) || y > bottom + 32.0 {
                return None;
            }
        }
        None
    }

    fn run(&self, seeds: &[usize]) -> Reach {
        let p = self.p;
        let mut touched = vec![false; self.targets.len()];
        let mut nearest: Vec<Option<(f32, Rect)>> = vec![None; self.targets.len()];
        let mut seen = vec![false; self.nodes.len()];
        let mut q: VecDeque<usize> = VecDeque::new();
        for &s in seeds {
            if !seen[s] {
                seen[s] = true;
                q.push_back(s);
            }
        }
        let mut speeds = vec![p.run_speed, p.walk_speed, p.walk_speed * 0.5, 0.0, -p.walk_speed * 0.5, -p.walk_speed, -p.run_speed];
        speeds.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        let mut reachable = 0;
        while let Some(i) = q.pop_front() {
            reachable += 1;
            let n = self.nodes[i];
            self.touch(self.bx(n.x, n.y), &mut touched, &mut nearest);
            let mut next: Vec<usize> = Vec::new();
            // Walk.
            if n.plat.is_none() {
                let c = col(n.x);
                for cc in [c - 1, c + 1] {
                    for &m in self.cols.get(&cc).map_or(&[][..], Vec::as_slice) {
                        let o = self.nodes[m];
                        if (o.y - n.y).abs() <= STEP + 0.5 && !self.ctx.geo.rect_hits_full(self.bx((n.x + o.x) * 0.5, n.y.min(o.y))) {
                            next.push(m);
                        }
                    }
                }
            }
            // Ride.
            if let Some(pi) = n.plat {
                next.extend_from_slice(&self.plat_nodes[pi]);
            }
            // Jump and walk off.
            for &vx in &speeds {
                for cut in CUTS {
                    if let Some(m) = self.fly(n, vx, cut, false, &mut touched, &mut nearest) {
                        next.push(m);
                    }
                }
                if vx != 0.0
                    && let Some(m) = self.fly(n, vx, None, true, &mut touched, &mut nearest)
                {
                    next.push(m);
                }
            }
            for m in next {
                if !seen[m] {
                    seen[m] = true;
                    q.push_back(m);
                }
            }
        }
        Reach {
            touched: (0..self.targets.len()).filter(|&t| touched[t]).map(|t| self.targets[t].obj).collect(),
            nearest: (0..self.targets.len()).filter_map(|t| nearest[t].map(|n| (self.targets[t].obj, n))).collect(),
            nodes: self.nodes.len(),
            reachable,
        }
    }
}

/// Run the reach model from the player's start and every spot the bot stood on.
pub fn reach(ctx: &Ctx) -> Option<Reach> {
    let p = ctx.scene.player.as_ref()?;
    let m = Model::new(ctx, p);
    let mut seeds: Vec<usize> = Vec::new();
    if let Some(n) = m.nearest_any(p.start[0], p.start[1], 8.0).or_else(|| m.land(p.start[0], p.start[1], 24.0)) {
        seeds.push(n);
    }
    if let Some(b) = &ctx.scene.bot {
        let mut last: Option<usize> = None;
        for t in b.ticks.iter().step_by(3) {
            if t[2] < 0.5 {
                continue;
            }
            if let Some(n) = m.nearest_any(t[0], t[1], 6.0)
                && last != Some(n)
            {
                seeds.push(n);
                last = Some(n);
            }
        }
    }
    Some(m.run(&seeds))
}

// ---- Coin arcs --------------------------------------------------------------

/// Height below the apex of a jump `u` px from it along the travel
/// direction (negative = still rising), at horizontal speed `vx`.
/// Semi-implicit Euler at `dt`, as fixed-step games integrate.
fn drop_at(p: &Player, u: f32, vx: f32, fall_mult: f32, dt: f32) -> Option<f32> {
    let tau = u.abs() / vx.max(1e-3);
    if u <= 0.0 {
        let tmax = (p.jump_speed + if vx > p.walk_speed + 1.0 { p.run_jump_bonus } else { 0.0 }) / p.gravity.max(1e-3);
        if tau > tmax + dt {
            return None;
        }
        Some((p.gravity * (tau * tau - tau * dt) * 0.5).max(0.0))
    } else {
        let gf = p.gravity * fall_mult;
        let tc = p.max_fall / gf.max(1e-3);
        Some(if tau <= tc { gf * (tau * tau + tau * dt) * 0.5 } else { gf * (tc * tc + tc * dt) * 0.5 + p.max_fall * (tau - tc) })
    }
}

#[derive(Debug, Clone, Copy)]
struct Fit {
    err: f32,
    apex: [f32; 2],
    vx: f32,
    dir: f32,
}

fn fit_arc(p: &Player, pts: &[[f32; 2]], fall_mult: f32, dt: f32) -> Option<Fit> {
    let x0 = pts.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
    let x1 = pts.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
    let lo = 0.3 * p.walk_speed;
    let mut speeds: Vec<f32> = (0..=30).map(|k| lo + (p.run_speed - lo) * k as f32 / 30.0).collect();
    speeds.push(p.walk_speed);
    let mut best: Option<Fit> = None;
    for dir in [1.0f32, -1.0] {
        for &vx in &speeds {
            let mut ax = x0;
            while ax <= x1 {
                let r: Option<Vec<f32>> = pts.iter().map(|q| drop_at(p, (q[0] - ax) * dir, vx, fall_mult, dt).map(|h| q[1] - h)).collect();
                if let Some(r) = r {
                    let mx = r.iter().copied().fold(f32::MIN, f32::max);
                    let mn = r.iter().copied().fold(f32::MAX, f32::min);
                    let err = (mx - mn) * 0.5;
                    if best.is_none_or(|b| err < b.err) {
                        best = Some(Fit { err, apex: [ax, (mx + mn) * 0.5], vx, dir });
                    }
                }
                ax += 1.0;
            }
        }
    }
    best
}

/// Groups of 3+ collectibles within `ARC_LINK` px of each other, with the
/// highest one inside the group and some height to them.
fn arc_groups(ctx: &Ctx) -> Vec<Vec<usize>> {
    let s = ctx.scene;
    let coins: Vec<usize> = (0..s.objects.len()).filter(|&i| s.objects[i].kind == Kind::Collectible).collect();
    let mut parent: Vec<usize> = (0..coins.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    for a in 0..coins.len() {
        for b in a + 1..coins.len() {
            let (ca, cb) = (centre(s.objects[coins[a]].rect), centre(s.objects[coins[b]].rect));
            if ((ca[0] - cb[0]).powi(2) + (ca[1] - cb[1]).powi(2)).sqrt() <= ARC_LINK {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                parent[ra] = rb;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, &c) in coins.iter().enumerate() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(c);
    }
    groups
        .into_values()
        .filter_map(|mut g| {
            if g.len() < 3 {
                return None;
            }
            g.sort_by(|&a, &b| centre(s.objects[a].rect)[0].total_cmp(&centre(s.objects[b].rect)[0]));
            let c: Vec<[f32; 2]> = g.iter().map(|&i| centre(s.objects[i].rect)).collect();
            // One coin per column: rows and grids are not arcs.
            if c.windows(2).any(|w| (w[1][0] - w[0][0]).abs() < 2.0) {
                return None;
            }
            let top = (0..c.len()).min_by(|&a, &b| c[a][1].total_cmp(&c[b][1]))?;
            let spread = c.iter().map(|q| q[1]).fold(f32::MIN, f32::max) - c[top][1];
            (top > 0 && top + 1 < c.len() && spread >= 4.0).then_some(g)
        })
        .collect()
}

/// The bot's airborne path nearest the coins: the largest distance from a
/// coin centre to the player's centre at the coin's x.
fn bot_arc_dev(ctx: &Ctx, pts: &[[f32; 2]]) -> Option<f32> {
    let b = ctx.scene.bot.as_ref()?;
    let h = ctx.scene.player.as_ref().map_or(0.0, |p| p.h);
    let (x0, x1) = (pts[0][0], pts[pts.len() - 1][0]);
    let mut best: Option<f32> = None;
    let mut i = 0;
    while i < b.ticks.len() {
        if b.ticks[i][2] > 0.5 {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.ticks.len() && b.ticks[i][2] < 0.5 {
            i += 1;
        }
        let seg = &b.ticks[start..i];
        let sx0 = seg.iter().map(|t| t[0]).fold(f32::MAX, f32::min);
        let sx1 = seg.iter().map(|t| t[0]).fold(f32::MIN, f32::max);
        if sx0 > x0 + 4.0 || sx1 < x1 - 4.0 {
            continue;
        }
        let dev = pts
            .iter()
            .map(|q| seg.iter().min_by(|a, b| (a[0] - q[0]).abs().total_cmp(&(b[0] - q[0]).abs())).map_or(f32::MAX, |t| (q[1] - (t[1] - h * 0.5)).abs()))
            .fold(0.0f32, f32::max);
        if best.is_none_or(|b| dev < b) {
            best = Some(dev);
        }
    }
    best
}

fn coin_arcs(ctx: &Ctx, out: &mut Vec<Issue>) {
    let s = ctx.scene;
    let Some(p) = s.player.as_ref() else { return };
    let dt = s.bot.as_ref().map_or(1.0 / 60.0, |b| if b.dt > 0.0 { b.dt } else { 1.0 / 60.0 });
    for g in arc_groups(ctx) {
        let pts: Vec<[f32; 2]> = g.iter().map(|&i| centre(s.objects[i].rect)).collect();
        let Some(fit) = fit_arc(p, &pts, p.fall_mult, dt) else { continue };
        if fit.err <= s.limits.arc_tol_px {
            continue;
        }
        let sym = fit_arc(p, &pts, 1.0, dt);
        let names: Vec<&str> = g.iter().map(|&i| s.objects[i].name.as_str()).collect();
        let span = g.iter().fold(s.objects[g[0]].rect, |a, &i| union(a, s.objects[i].rect));
        let moves: Vec<String> = g
            .iter()
            .zip(&pts)
            .filter_map(|(&i, q)| {
                let want = fit.apex[1] + drop_at(p, (q[0] - fit.apex[0]) * fit.dir, fit.vx, p.fall_mult, dt).unwrap_or(0.0);
                let d = (want - q[1]).round() as i32;
                (d != 0).then(|| format!("{} {} {} px", s.objects[i].name, if d > 0 { "down" } else { "up" }, d.abs()))
            })
            .collect();
        let mut why = String::new();
        if fit.vx >= p.run_speed - 0.5 {
            why.push_str(&format!(
                " The arc is wider and flatter than a jump at full run speed ({} px/s) covers, so even the fastest jump drops away from the outer coins.",
                fmt_px(p.run_speed)
            ));
        }
        if let Some(sf) = sym
            && sf.err + 1.0 < fit.err
            && (p.fall_mult - 1.0).abs() > 0.05
        {
            why.push_str(&format!(
                " The coins are laid out symmetrically (a jump that fell as slowly as it rose would miss by {} px), but the player falls {}x faster{}, so real arcs are steeper after the top.",
                fmt_px(sf.err.round()),
                fmt_px(p.fall_mult),
                if p.max_fall < 1.0e5 { format!(" and at most {} px/s", fmt_px(p.max_fall)) } else { String::new() }
            ));
        }
        let bot = match bot_arc_dev(ctx, &pts) {
            Some(d) => format!(" The bot's own jump through them passes up to {} px from a coin centre.", fmt_px(d.round())),
            None => " The bot never jumped through them.".to_string(),
        };
        // Real arc path for the crop.
        let arc: Vec<Rect> = (0..=24)
            .filter_map(|k| {
                let x = span[0] - 8.0 + (span[2] + 16.0) * k as f32 / 24.0;
                drop_at(p, (x - fit.apex[0]) * fit.dir, fit.vx, p.fall_mult, dt).map(|h| [x.round(), (fit.apex[1] + h).round(), 1.0, 1.0])
            })
            .collect();
        let mut hl = vec![span];
        hl.extend(arc);
        out.push(
            Issue::new(Check::Bot, "coin_arc", span)
                .object(&format!("{} .. {}", names[0], names[names.len() - 1]))
                .tile(ctx.tile_of([span[0], span[1]]))
                .sev(2, fit.err)
                .msg(
                    format!(
                        "The {} coins at {} form an arc no real jump follows: the closest jump (top at x {}, {} px/s {}) misses a coin centre by {} px (limit {}).{why}{bot}",
                        g.len(),
                        fmt_rect(span),
                        fmt_px(fit.apex[0]),
                        fmt_px(fit.vx.round()),
                        if fit.dir > 0.0 { "rightwards" } else { "leftwards" },
                        fmt_px(fit.err.round()),
                        fmt_px(s.limits.arc_tol_px)
                    ),
                    format!("Lay the coins on the real jump arc: {}.", if moves.is_empty() { "re-place them along a recorded jump".to_string() } else { moves.join(", ") }),
                )
                .crop(CropSource::Level(inflate(span, 20.0)), hl),
        );
    }
}

// ---- Hidden hazards ---------------------------------------------------------

fn hidden_hazards(ctx: &Ctx, out: &mut Vec<Issue>) {
    let s = ctx.scene;
    let g = &ctx.geo;
    let mut hazards: Vec<(String, Rect, i32)> = s.objects.iter().filter(|o| o.kind == Kind::Hazard).map(|o| (o.name.clone(), o.rect, o.layer)).collect();
    // Hazard tiles, joined into runs along a row.
    let mut rows: BTreeMap<(i32, i32, String), Vec<i32>> = BTreeMap::new();
    for t in s.tiles.iter().filter(|t| t.hazard) {
        rows.entry((t.y, t.layer, t.id.clone())).or_default().push(t.x);
    }
    for ((y, layer, id), mut xs) in rows {
        xs.sort_unstable();
        xs.dedup();
        for (a, b) in super::runs(&xs) {
            let r = [a as f32 * g.ts, y as f32 * g.ts, (b - a + 1) as f32 * g.ts, g.ts];
            hazards.push((format!("hazard '{id}' tiles x {a}-{b}, y {y}"), r, layer));
        }
    }
    for (name, r, layer) in hazards {
        let front: Vec<usize> = (0..s.objects.len()).filter(|&i| s.objects[i].kind == Kind::Decor && s.objects[i].layer > layer && crate::geom::overlaps(s.objects[i].rect, r)).collect();
        let front_tile = |x: i32, y: i32| g.tiles_at(g.cell(x as f32 + 0.5), g.cell(y as f32 + 0.5)).iter().any(|t| !t.hazard && t.shape == Shape::None && t.layer > layer);
        let (mut total, mut covered) = (0usize, 0usize);
        let mut b: Option<Rect> = None;
        for y in r[1] as i32..(r[1] + r[3]) as i32 {
            for x in r[0] as i32..(r[0] + r[2]) as i32 {
                total += 1;
                if front.iter().any(|&i| ctx.masks[i].get(x, y)) || front_tile(x, y) {
                    covered += 1;
                    let px = [x as f32, y as f32, 1.0, 1.0];
                    b = Some(b.map_or(px, |b| union(b, px)));
                }
            }
        }
        let share = covered as f32 / total.max(1) as f32;
        if share > s.limits.hazard_cover {
            let by: Vec<&str> = front.iter().map(|&i| s.objects[i].name.as_str()).collect();
            out.push(
                Issue::new(Check::Bot, "hazard_hidden", r)
                    .object(&name)
                    .tile(ctx.tile_of([r[0], r[1]]))
                    .sev(4, share)
                    .msg(
                        format!("{:.0} % of {name} is hidden behind {} drawn in front of it.", share * 100.0, if by.is_empty() { "decoration tiles".to_string() } else { by.join(", ") }),
                        format!("Move the decor off the hazard, or draw it behind the hazard (a layer under {layer}), so at most {:.0} % is covered.", s.limits.hazard_cover * 100.0),
                    )
                    .crop(CropSource::Level(inflate(r, 24.0)), b.map_or(vec![r], |b| vec![r, b])),
            );
        }
    }
}

// ---- The check --------------------------------------------------------------

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let mut out = Vec::new();
    let reach = reach(ctx);
    let bot_touched: Vec<&str> = s.bot.as_ref().map_or(Vec::new(), |b| b.touched.iter().map(String::as_str).collect());
    let model_goal = |r: &Reach| s.objects.iter().enumerate().any(|(i, o)| o.kind == Kind::Goal && r.touched.contains(&i));

    // Completion.
    match (&s.bot, &reach) {
        (Some(b), _) if !b.completed => {
            let last = b.ticks.last().copied().unwrap_or([0.0, 0.0, 0.0]);
            let pr = s.player.as_ref().map_or([last[0] - 5.0, last[1] - 14.0, 10.0, 14.0], |p| [last[0] - p.w * 0.5, last[1] - p.h, p.w, p.h]);
            let model = match &reach {
                Some(r) if model_goal(r) => " The reach model can touch the goal, so the route exists but the bot's controls miss it.",
                Some(_) => " The reach model cannot touch the goal either.",
                None => "",
            };
            out.push(
                Issue::new(Check::Bot, "not_completable", pr)
                    .object("level")
                    .tile(ctx.tile_of([last[0], last[1] - 1.0]))
                    .tick(b.end_tick)
                    .sev(5, 1.0)
                    .msg(
                        format!("The bot did not finish the level in {} ticks; it ended at x {}, y {}.{model}", b.end_tick, fmt_px(last[0].round()), fmt_px(last[1].round())),
                        "Check the jump or hazard just past where the bot stopped; widen the gap's landing or lower the ledge so a run jump makes it.".into(),
                    )
                    .crop(CropSource::Level(inflate(pr, 64.0)), vec![pr]),
            );
        }
        (None, Some(r)) if !model_goal(r) && s.objects.iter().any(|o| o.kind == Kind::Goal) => {
            let gi = s.objects.iter().position(|o| o.kind == Kind::Goal).unwrap_or(0);
            let o = &s.objects[gi];
            out.push(
                Issue::new(Check::Bot, "not_completable", o.rect)
                    .object(&o.name)
                    .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                    .sev(5, 1.0)
                    .msg(format!("No bot trace, and the reach model cannot touch {} from the start.", o.name), "Add a route to the goal, or dump a bot run so the check can use it.".into())
                    .crop(CropSource::Level(inflate(o.rect, 48.0)), vec![o.rect]),
            );
        }
        _ => {}
    }

    // Reachability.
    if let Some(r) = &reach {
        for (i, o) in s.objects.iter().enumerate() {
            if o.kind != Kind::Collectible || r.touched.contains(&i) || bot_touched.contains(&o.name.as_str()) {
                continue;
            }
            let c = centre(o.rect);
            let pick = (o.rect[2].min(o.rect[3]) * 0.5).max(2.0);
            let (fix, hl) = match r.nearest.get(&i) {
                Some(&(d, b)) => {
                    let np = [c[0].clamp(b[0], b[0] + b[2]), c[1].clamp(b[1], b[1] + b[3])];
                    let k = ((d - pick + 1.0) / d.max(1e-3)).clamp(0.0, 1.0);
                    let (dx, dy) = (((np[0] - c[0]) * k).round() as i32, ((np[1] - c[1]) * k).round() as i32);
                    let dir = |v: i32, neg: &str, pos: &str| if v == 0 { None } else { Some(format!("{} {} px", if v < 0 { neg } else { pos }, v.abs())) };
                    let mv: Vec<String> = [dir(dy, "up", "down"), dir(dx, "left", "right")].into_iter().flatten().collect();
                    (
                        format!(
                            "The nearest any modelled jump gets is {} px from it (the player's box at {}). Move {} {}, or add a ledge or platform under it.",
                            fmt_px(d.round()),
                            fmt_rect(b),
                            o.name,
                            mv.join(" and ")
                        ),
                        vec![o.rect, b],
                    )
                }
                None => (format!("No reachable spot is within three tiles of it. Move {} onto the route, or add a ledge or platform to reach it.", o.name), vec![o.rect]),
            };
            out.push(
                Issue::new(Check::Bot, "unreachable_collectible", o.rect)
                    .object(&o.name)
                    .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                    .sev(4, r.nearest.get(&i).map_or(999.0, |n| n.0))
                    .msg(
                        format!(
                            "{} at {} was not touched by the bot and no jump the reach model tries (walk, run, cut jumps and walk-offs from {} reachable spots) touches it.",
                            o.name,
                            fmt_rect(o.rect),
                            r.reachable
                        ),
                        fix,
                    )
                    .crop(CropSource::Level(inflate(hl.iter().fold(o.rect, |a, b| union(a, *b)), 24.0)), hl),
            );
        }
    }
    coin_arcs(ctx, &mut out);
    hidden_hazards(ctx, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::testkit::{earth, ground, obj, remove_tile};
    use crate::scene::{BotTrace, Player, Scene};

    fn player() -> Player {
        Player {
            w: 10.0,
            h: 14.0,
            gravity: 980.0,
            fall_mult: 1.6,
            max_fall: 330.0,
            jump_speed: 330.0,
            run_jump_bonus: 25.0,
            jump_cut: 0.45,
            walk_speed: 90.0,
            run_speed: 150.0,
            start: [24.0, 128.0],
            sprite: None,
        }
    }

    /// 30 x 10 tiles, floor top at y 128.
    fn level() -> Scene {
        let mut s = ground(30, 10, 8);
        s.player = Some(player());
        s
    }

    fn coin(name: &str, cx: f32, cy: f32) -> crate::scene::Object {
        obj(name, Kind::Collectible, [cx - 4.0, cy - 4.0, 8.0, 8.0])
    }

    #[test]
    fn coins_in_jump_reach_pass_and_one_too_high_fails() {
        let mut s = level();
        // A standing jump rises about 330² / 1960 = 56 px less half a step: the box top reaches about 61.
        s.objects.push(coin("coin low", 100.0, 120.0));
        s.objects.push(coin("coin jump", 160.0, 66.0));
        s.objects.push(coin("coin sky", 220.0, 20.0));
        let v = check(&Ctx::new(&s, None));
        let un: Vec<&str> = v.iter().filter(|i| i.kind == "unreachable_collectible").filter_map(|i| i.object.as_deref()).collect();
        assert_eq!(un, vec!["coin sky"], "{v:?}");
        let sky = v.iter().find(|i| i.object.as_deref() == Some("coin sky")).unwrap();
        assert!(sky.fix.contains("down"), "{}", sky.fix);
        // The bot touched it: no issue.
        s.bot = Some(BotTrace { dt: 1.0 / 60.0, completed: true, touched: vec!["coin sky".into()], ..BotTrace::default() });
        assert!(!check(&Ctx::new(&s, None)).iter().any(|i| i.kind == "unreachable_collectible"));
    }

    #[test]
    fn a_coin_over_a_high_ledge_is_reached_by_jumping_up_the_steps() {
        let mut s = level();
        // A two-tile step at x 12..16 (top y 96), then a coin above it.
        for x in 12..16 {
            for y in 6..8 {
                s.tiles.push(earth(x, y, if y == 6 { "t" } else { "" }));
            }
        }
        s.objects.push(coin("coin ledge", 224.0, 40.0));
        let v = check(&Ctx::new(&s, None));
        assert!(!v.iter().any(|i| i.kind == "unreachable_collectible"), "{v:?}");
    }

    #[test]
    fn an_unfinished_bot_run_fails() {
        let mut s = level();
        // A pit too wide to cross.
        for x in 8..30 {
            for y in 8..10 {
                remove_tile(&mut s, x, y);
            }
        }
        s.objects.push(obj("gate", Kind::Goal, [440.0, 96.0, 16.0, 32.0]));
        s.tiles.push(earth(27, 8, "t"));
        s.tiles.push(earth(28, 8, "t"));
        s.bot = Some(BotTrace { dt: 1.0 / 60.0, completed: false, end_tick: 600, ticks: vec![[120.0, 128.0, 1.0]], touched: vec![] });
        let v = check(&Ctx::new(&s, None));
        let nc = v.iter().find(|i| i.kind == "not_completable").expect("not_completable");
        assert!(nc.message.contains("cannot touch the goal"), "{}", nc.message);
    }

    #[test]
    fn wide_symmetric_coin_arcs_fail_and_real_arcs_pass() {
        let p = player();
        let dt = 1.0 / 60.0;
        // Coins on a real arc: top at x 200, y 70, 120 px/s rightwards.
        let mut s = level();
        for (k, dx) in [-36.0f32, -18.0, 0.0, 18.0, 36.0].iter().enumerate() {
            let y = 70.0 + drop_at(&p, *dx, 120.0, p.fall_mult, dt).unwrap();
            s.objects.push(coin(&format!("real {k}"), 200.0 + dx, y.round()));
        }
        let v = check(&Ctx::new(&s, None));
        assert!(!v.iter().any(|i| i.kind == "coin_arc"), "{v:?}");
        // A wide, flat, symmetric arc (5 coins over 96 px, 24 px high): too
        // flat for a jump at run speed, and the player falls faster than it rises.
        let mut s = level();
        for (k, (dx, dy)) in [(-48.0f32, 24.0f32), (-24.0, 6.0), (0.0, 0.0), (24.0, 6.0), (48.0, 24.0)].iter().enumerate() {
            s.objects.push(coin(&format!("sym {k}"), 300.0 + dx, 60.0 + dy));
        }
        let v = check(&Ctx::new(&s, None));
        let arc = v.iter().find(|i| i.kind == "coin_arc").expect("coin_arc");
        assert!(arc.message.contains("run speed") && arc.message.contains("symmetrically"), "{}", arc.message);
        assert!(arc.fix.contains("px"), "{}", arc.fix);
        // A flat row is not an arc.
        let mut s = level();
        for k in 0..5 {
            s.objects.push(coin(&format!("row {k}"), 100.0 + 12.0 * k as f32, 100.0));
        }
        assert!(!check(&Ctx::new(&s, None)).iter().any(|i| i.kind == "coin_arc"));
    }

    #[test]
    fn decor_in_front_of_spikes_is_flagged() {
        let mut s = level();
        s.tiles.push(crate::scene::TileDef { x: 10, y: 7, id: "^".into(), layer: 60, hazard: true, ..crate::scene::TileDef::default() });
        let mut bush = obj("bush", Kind::Decor, [160.0, 108.0, 16.0, 20.0]);
        bush.layer = 50; // behind: fine
        s.objects.push(bush.clone());
        assert!(!check(&Ctx::new(&s, None)).iter().any(|i| i.kind == "hazard_hidden"));
        s.objects[0].layer = 70; // in front: hides it
        let v = check(&Ctx::new(&s, None));
        assert!(v.iter().any(|i| i.kind == "hazard_hidden" && i.message.contains("bush")), "{v:?}");
    }
}
