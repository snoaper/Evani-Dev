//! The map grid, flow fields and fog of war.
//!
//! * [`Grid`]: 2 m cells; rock, water, buildings and crystal nodes block.
//!   World x runs east, z south; the map is centred on the origin.
//! * [`FlowField`]: a Dijkstra integration field from one goal cell (10
//!   straight, 14 diagonal, no corner cutting, ties by cell index). Every
//!   unit heading for the same cell shares it ([`FlowCache`]), so a group
//!   costs one grid pass, not one search per unit.
//! * [`Fog`]: per team and cell, unexplored / explored / visible.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::data::{MapData, Terrain};

/// Cost of an unreachable cell.
pub const UNREACHABLE: u32 = u32::MAX;

/// Neighbour offsets in a fixed order (straight first), with step costs.
const DIRS: [(i32, i32, u32); 8] = [
    (1, 0, 10),
    (-1, 0, 10),
    (0, 1, 10),
    (0, -1, 10),
    (1, 1, 14),
    (-1, 1, 14),
    (1, -1, 14),
    (-1, -1, 14),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub w: i32,
    pub h: i32,
    pub cell: f32,
    pub terrain: Vec<Terrain>,
    /// Terrain, building or node in the cell.
    pub blocked: Vec<bool>,
    /// Bumped whenever `blocked` changes (drops cached flow fields).
    pub rev: u32,
}

impl Grid {
    pub fn new(map: &MapData) -> Self {
        let blocked = map.terrain.iter().map(|t| t.blocks()).collect();
        Self {
            w: map.w,
            h: map.h,
            cell: map.cell_m,
            terrain: map.terrain.clone(),
            blocked,
            rev: 1,
        }
    }

    pub fn idx(&self, c: [i32; 2]) -> Option<usize> {
        if c[0] >= 0 && c[1] >= 0 && c[0] < self.w && c[1] < self.h {
            Some((c[1] * self.w + c[0]) as usize)
        } else {
            None
        }
    }

    pub fn passable(&self, c: [i32; 2]) -> bool {
        self.idx(c).is_some_and(|i| !self.blocked[i])
    }

    pub fn terrain_at(&self, c: [i32; 2]) -> Terrain {
        self.idx(c).map_or(Terrain::Rock, |i| self.terrain[i])
    }

    /// Half the map's width and depth in metres.
    pub fn half(&self) -> [f32; 2] {
        [self.w as f32 * self.cell * 0.5, self.h as f32 * self.cell * 0.5]
    }

    /// The cell holding world point `p` (`[x, z]`).
    pub fn cell_of(&self, p: [f32; 2]) -> [i32; 2] {
        let h = self.half();
        [((p[0] + h[0]) / self.cell).floor() as i32, ((p[1] + h[1]) / self.cell).floor() as i32]
    }

    /// Centre of a cell in world `[x, z]`.
    pub fn center(&self, c: [i32; 2]) -> [f32; 2] {
        let h = self.half();
        [(c[0] as f32 + 0.5) * self.cell - h[0], (c[1] as f32 + 0.5) * self.cell - h[1]]
    }

    /// World rectangle `[x0, z0, x1, z1]` of a footprint.
    pub fn rect(&self, corner: [i32; 2], size: i32) -> [f32; 4] {
        let h = self.half();
        let x0 = corner[0] as f32 * self.cell - h[0];
        let z0 = corner[1] as f32 * self.cell - h[1];
        [x0, z0, x0 + size as f32 * self.cell, z0 + size as f32 * self.cell]
    }

    pub fn set_blocked(&mut self, c: [i32; 2], b: bool) {
        if let Some(i) = self.idx(c) {
            let b = b || self.terrain[i].blocks();
            if self.blocked[i] != b {
                self.blocked[i] = b;
                self.rev = self.rev.wrapping_add(1);
            }
        }
    }

    /// Can a unit step from `a` to its neighbour `b` (no corner cutting)?
    fn step_ok(&self, a: [i32; 2], b: [i32; 2]) -> bool {
        if !self.passable(b) {
            return false;
        }
        if a[0] != b[0] && a[1] != b[1] {
            return self.passable([b[0], a[1]]) && self.passable([a[0], b[1]]);
        }
        true
    }
}

/// Distance from `p` to the rectangle `r` (`[x0, z0, x1, z1]`); 0 inside.
pub fn dist_to_rect(p: [f32; 2], r: [f32; 4]) -> f32 {
    let dx = (r[0] - p[0]).max(p[0] - r[2]).max(0.0);
    let dz = (r[1] - p[1]).max(p[1] - r[3]).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

pub fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// A Dijkstra integration field toward one goal cell.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowField {
    pub goal: [i32; 2],
    /// The [`Grid::rev`] it was built for.
    pub rev: u32,
    pub cost: Vec<u32>,
}

impl FlowField {
    pub fn build(grid: &Grid, goal: [i32; 2]) -> Self {
        let n = (grid.w * grid.h) as usize;
        let mut cost = vec![UNREACHABLE; n];
        let mut heap = BinaryHeap::new();
        // A blocked goal (a building, a node, a rock) seeds its blocked
        // cells within two of the goal, so units stop at its edge.
        if let Some(gi) = grid.idx(goal) {
            if grid.blocked[gi] {
                let mut stack = vec![goal];
                while let Some(c) = stack.pop() {
                    let Some(i) = grid.idx(c) else { continue };
                    if !grid.blocked[i] || cost[i] == 0 || (c[0] - goal[0]).abs() > 2 || (c[1] - goal[1]).abs() > 2 {
                        continue;
                    }
                    cost[i] = 0;
                    heap.push(Reverse((0u32, i as u32)));
                    for (dx, dy, _) in &DIRS[..4] {
                        stack.push([c[0] + dx, c[1] + dy]);
                    }
                }
            } else {
                cost[gi] = 0;
                heap.push(Reverse((0u32, gi as u32)));
            }
        }
        while let Some(Reverse((c, i))) = heap.pop() {
            if c > cost[i as usize] {
                continue;
            }
            let at = [i as i32 % grid.w, i as i32 / grid.w];
            for (dx, dy, step) in DIRS {
                let nb = [at[0] + dx, at[1] + dy];
                if !grid.step_ok(at, nb) {
                    continue;
                }
                let Some(ni) = grid.idx(nb) else { continue };
                let nc = c.saturating_add(step);
                if nc < cost[ni] {
                    cost[ni] = nc;
                    heap.push(Reverse((nc, ni as u32)));
                }
            }
        }
        Self { goal, rev: grid.rev, cost }
    }

    pub fn cost_at(&self, grid: &Grid, c: [i32; 2]) -> u32 {
        grid.idx(c).map_or(UNREACHABLE, |i| self.cost[i])
    }

    /// The neighbour of `from` one step closer to the goal, if any.
    pub fn next(&self, grid: &Grid, from: [i32; 2]) -> Option<[i32; 2]> {
        let here = self.cost_at(grid, from);
        let mut best: Option<([i32; 2], u32)> = None;
        for (dx, dy, _) in DIRS {
            let nb = [from[0] + dx, from[1] + dy];
            let c = self.cost_at(grid, nb);
            if c == UNREACHABLE || c >= here {
                continue;
            }
            // Into a seed (the goal's own blocked cells) is never a step.
            if !grid.passable(nb) || (dx != 0 && dy != 0 && !grid.step_ok(from, nb)) {
                continue;
            }
            if best.is_none_or(|(_, b)| c < b) {
                best = Some((nb, c));
            }
        }
        best.map(|(c, _)| c)
    }
}

/// The shared fields, newest last; a field is dropped when the grid changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlowCache {
    pub fields: Vec<FlowField>,
}

/// How many fields the cache keeps.
pub const FLOW_CACHE: usize = 24;

impl FlowCache {
    pub fn get(&mut self, grid: &Grid, goal: [i32; 2]) -> &FlowField {
        self.fields.retain(|f| f.rev == grid.rev);
        let i = match self.fields.iter().position(|f| f.goal == goal) {
            Some(i) => i,
            None => {
                if self.fields.len() >= FLOW_CACHE {
                    self.fields.remove(0);
                }
                self.fields.push(FlowField::build(grid, goal));
                self.fields.len() - 1
            }
        };
        &self.fields[i]
    }
}

pub const UNEXPLORED: u8 = 0;
pub const EXPLORED: u8 = 1;
pub const VISIBLE: u8 = 2;

/// One team's fog of war.
#[derive(Debug, Clone, PartialEq)]
pub struct Fog {
    pub cells: Vec<u8>,
    /// Bumped on every update (the draw caches follow it).
    pub rev: u32,
}

impl Fog {
    pub fn new(grid: &Grid) -> Self {
        Self { cells: vec![UNEXPLORED; (grid.w * grid.h) as usize], rev: 0 }
    }

    pub fn at(&self, grid: &Grid, c: [i32; 2]) -> u8 {
        grid.idx(c).map_or(UNEXPLORED, |i| self.cells[i])
    }

    pub fn visible(&self, grid: &Grid, p: [f32; 2]) -> bool {
        self.at(grid, grid.cell_of(p)) == VISIBLE
    }

    /// Marks unseen cells whose centre lies within `r` metres of `p` as
    /// explored (the start of a match: the ground around a base is known).
    pub fn explore(&mut self, grid: &Grid, p: [f32; 2], r: f32) {
        let n = (r / grid.cell).ceil() as i32 + 1;
        let c0 = grid.cell_of(p);
        for y in (c0[1] - n).max(0)..=(c0[1] + n).min(grid.h - 1) {
            for x in (c0[0] - n).max(0)..=(c0[0] + n).min(grid.w - 1) {
                let i = (y * grid.w + x) as usize;
                if self.cells[i] == UNEXPLORED && dist(grid.center([x, y]), p) <= r {
                    self.cells[i] = EXPLORED;
                }
            }
        }
        self.rev = self.rev.wrapping_add(1);
    }

    /// Visible falls back to explored, then every source (`[x, z]`, sight
    /// in metres) lights the cells whose centre it sees.
    pub fn update(&mut self, grid: &Grid, sources: &[([f32; 2], f32)]) {
        for c in &mut self.cells {
            if *c == VISIBLE {
                *c = EXPLORED;
            }
        }
        for &(p, sight) in sources {
            let r = (sight / grid.cell).ceil() as i32 + 1;
            let c0 = grid.cell_of(p);
            for y in (c0[1] - r).max(0)..=(c0[1] + r).min(grid.h - 1) {
                for x in (c0[0] - r).max(0)..=(c0[0] + r).min(grid.w - 1) {
                    if dist(grid.center([x, y]), p) <= sight {
                        self.cells[(y * grid.w + x) as usize] = VISIBLE;
                    }
                }
            }
        }
        self.rev = self.rev.wrapping_add(1);
    }
}
