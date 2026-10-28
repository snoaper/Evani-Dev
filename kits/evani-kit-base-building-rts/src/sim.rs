//! The skirmish simulation (lockstep-safe).
//!
//! Everything that changes the match goes through a [`Command`]: the
//! player's pointer and keys and the AI both push commands, and
//! [`Match::step`] applies them (sorted by team, in arrival order) before it
//! advances one fixed tick. Units, buildings and nodes are kept in id order
//! and every loop walks them in that order, so two machines given the same
//! commands stay in step. No wall-clock time, no hash-map iteration.

use crate::data::{bs, us, MapData, Types, BASTION, FOUNDRY, HUB, WORKER};
use crate::grid::{dist, dist_to_rect, FlowCache, Fog, Grid, EXPLORED};
use crate::Tune;

pub const PLAYER: u8 = 0;
pub const RIVAL: u8 = 1;

/// A worker's place in the gather loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gather {
    ToNode,
    /// Seconds of mining left.
    Mining(f32),
    ToHub,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    Idle,
    /// `group` units were sent to the same goal (they spread on arrival);
    /// `aggressive` is an attack-move.
    Move { goal: [f32; 2], group: u16, aggressive: bool },
    /// `auto`: picked up by itself (gives up beyond the leash from `anchor`).
    Attack { target: u32, auto: bool, anchor: [f32; 2] },
    Gather { node: u32, phase: Gather },
    Build { site: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub id: u32,
    pub team: u8,
    pub kind: usize,
    /// World `[x, z]`.
    pub pos: [f32; 2],
    pub hp: f32,
    pub order: Order,
    /// What to go back to after an automatic fight.
    pub resume: Option<Order>,
    pub cooldown: f32,
    pub carry: f32,
    /// Yaw in radians (0 faces +z, south).
    pub facing: f32,
    /// Ticks without progress on a move order.
    pub stuck: u16,
    pub moving: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    pub id: u32,
    pub team: u8,
    pub kind: usize,
    pub corner: [i32; 2],
    pub size: i32,
    pub hp: f32,
    pub built: bool,
    /// Construction progress 0..1.
    pub progress: f32,
    /// Unit kinds queued for training (front first).
    pub queue: Vec<usize>,
    /// Seconds into the front of the queue.
    pub train_t: f32,
    pub rally: Option<[f32; 2]>,
    pub cooldown: f32,
    /// Workers who raised it this tick.
    pub builders: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u32,
    pub cell: [i32; 2],
    pub pos: [f32; 2],
    pub amount: f32,
    pub start: f32,
}

/// A shot, for drawing only (tracer from shooter to target).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub team: u8,
    /// Seconds left.
    pub t: f32,
    pub heavy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandKind {
    Move { to: [f32; 2], aggressive: bool },
    Attack { target: u32 },
    Gather { node: u32 },
    Build { kind: usize, corner: [i32; 2] },
    Help { site: u32 },
    Train { kind: usize },
    Rally { to: [f32; 2] },
    /// Units stop; producers drop the last queued unit (refunded).
    Stop,
}

/// One order from one team: who (unit or building ids) and what.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub team: u8,
    pub ids: Vec<u32>,
    pub kind: CommandKind,
}

/// What a target is, for range checks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Circle([f32; 2], f32),
    Rect([f32; 4]),
}

impl Shape {
    fn gap(self, p: [f32; 2]) -> f32 {
        match self {
            Shape::Circle(c, r) => dist(p, c) - r,
            Shape::Rect(r) => dist_to_rect(p, r),
        }
    }

    fn center(self) -> [f32; 2] {
        match self {
            Shape::Circle(c, _) => c,
            Shape::Rect(r) => [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5],
        }
    }
}

/// A unit as every other unit sees it this tick.
#[derive(Debug, Clone, Copy)]
struct Snap {
    id: u32,
    team: u8,
    pos: [f32; 2],
    radius: f32,
}

/// The match: one resource, rebuilt for every new match.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub grid: Grid,
    pub flow: FlowCache,
    /// Types with this tick's live stats.
    pub types: Types,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    pub nodes: Vec<Node>,
    pub crystal: [f32; 2],
    pub fog: [Fog; 2],
    pub next_id: u32,
    pub tick: u64,
    /// Commands for the next tick.
    pub pending: Vec<Command>,
    /// Set when a hub falls (never in free roam).
    pub winner: Option<u8>,
    pub free_roam: bool,
    /// Refusals for the player's HUD ("Not enough crystal").
    pub notices: Vec<&'static str>,
    pub shots: Vec<Shot>,
}

impl Match {
    pub fn new(map: &MapData, types: Types, tune: &Tune, free_roam: bool) -> Self {
        let grid = Grid::new(map);
        let fog = [Fog::new(&grid), Fog::new(&grid)];
        let mut m = Self {
            grid,
            flow: FlowCache::default(),
            types,
            units: Vec::new(),
            buildings: Vec::new(),
            nodes: Vec::new(),
            crystal: [tune.start_crystal; 2],
            fog,
            next_id: 1,
            tick: 0,
            pending: Vec::new(),
            winner: None,
            free_roam,
            notices: Vec::new(),
            shots: Vec::new(),
        };
        for team in [PLAYER, RIVAL] {
            let corner = map.hub_corner(team, m.types.size(HUB));
            m.add_building(team, HUB, corner, true);
        }
        for &(cell, amount) in &map.nodes {
            let id = m.take_id();
            m.grid.set_blocked(cell, true);
            let pos = m.grid.center(cell);
            m.nodes.push(Node { id, cell, pos, amount, start: amount });
        }
        for team in [PLAYER, RIVAL] {
            let hub = m.buildings.iter().position(|b| b.team == team && b.kind == HUB);
            for _ in 0..map.workers {
                if let Some(i) = hub {
                    let p = m.spawn_point(i);
                    m.spawn_unit(team, WORKER, p);
                }
            }
        }
        m.update_fog();
        if map.start_reveal_m > 0.0 {
            for b in m.buildings.iter().filter(|b| b.kind == HUB) {
                let r = m.grid.rect(b.corner, b.size);
                let ctr = [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5];
                m.fog[b.team as usize].explore(&m.grid, ctr, map.start_reveal_m);
            }
        }
        m
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn add_building(&mut self, team: u8, kind: usize, corner: [i32; 2], built: bool) -> u32 {
        let id = self.take_id();
        let size = self.types.size(kind);
        for y in corner[1]..corner[1] + size {
            for x in corner[0]..corner[0] + size {
                self.grid.set_blocked([x, y], true);
            }
        }
        let max = self.types.building(kind, bs::HP);
        self.buildings.push(Building {
            id,
            team,
            kind,
            corner,
            size,
            hp: if built { max } else { max * 0.1 },
            built,
            progress: if built { 1.0 } else { 0.0 },
            queue: Vec::new(),
            train_t: 0.0,
            rally: None,
            cooldown: 0.0,
            builders: 0,
        });
        id
    }

    fn spawn_unit(&mut self, team: u8, kind: usize, pos: [f32; 2]) -> u32 {
        let id = self.take_id();
        self.units.push(Unit {
            id,
            team,
            kind,
            pos,
            hp: self.types.unit(kind, us::HP),
            order: Order::Idle,
            resume: None,
            cooldown: 0.0,
            carry: 0.0,
            facing: if team == PLAYER { core::f32::consts::PI } else { 0.0 },
            stuck: 0,
            moving: false,
        });
        id
    }

    /// A free cell next to building `i`, on the side facing its rally point
    /// (or the map centre).
    fn spawn_point(&self, i: usize) -> [f32; 2] {
        let b = &self.buildings[i];
        let toward = b.rally.unwrap_or([0.0, 0.0]);
        let mut best: Option<([f32; 2], f32)> = None;
        for ring in 1..6 {
            let (x0, y0) = (b.corner[0] - ring, b.corner[1] - ring);
            let (x1, y1) = (b.corner[0] + b.size - 1 + ring, b.corner[1] + b.size - 1 + ring);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x != x0 && x != x1 && y != y0 && y != y1 {
                        continue;
                    }
                    if !self.grid.passable([x, y]) {
                        continue;
                    }
                    let p = self.grid.center([x, y]);
                    if self.units.iter().any(|u| dist(u.pos, p) < 0.9) {
                        continue;
                    }
                    let d = dist(p, toward);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((p, d));
                    }
                }
            }
            if let Some((p, _)) = best {
                return p;
            }
        }
        self.grid.center([b.corner[0] - 1, b.corner[1] - 1])
    }

    pub fn unit(&self, id: u32) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn building(&self, id: u32) -> Option<&Building> {
        self.buildings.iter().find(|b| b.id == id)
    }

    pub fn building_rect(&self, b: &Building) -> [f32; 4] {
        self.grid.rect(b.corner, b.size)
    }

    pub fn hub(&self, team: u8) -> Option<&Building> {
        self.buildings.iter().find(|b| b.team == team && b.kind == HUB)
    }

    /// Does `team` have a finished building of `kind`?
    pub fn has_built(&self, team: u8, kind: usize) -> bool {
        self.buildings.iter().any(|b| b.team == team && b.kind == kind && b.built)
    }

    /// Units plus queued units.
    pub fn supply(&self, team: u8) -> usize {
        self.units.iter().filter(|u| u.team == team).count()
            + self.buildings.iter().filter(|b| b.team == team).map(|b| b.queue.len()).sum::<usize>()
    }

    fn need(kind: usize) -> &'static str {
        match kind {
            HUB => "Needs a command hub",
            FOUNDRY => "Needs a foundry",
            _ => "Needs a bastion",
        }
    }

    /// Can `team` place a `kind` with its lowest cell at `corner`?
    pub fn can_place(&self, team: u8, kind: usize, corner: [i32; 2]) -> Result<(), &'static str> {
        let Some(t) = self.types.buildings.get(kind) else {
            return Err("Can't build that");
        };
        if !t.buildable {
            return Err("Can't build that");
        }
        if let Some(r) = t.requires
            && !self.has_built(team, r) {
                return Err(Self::need(r));
            }
        if self.crystal[team as usize & 1] < t.stats[bs::COST] {
            return Err("Not enough crystal");
        }
        let fog = &self.fog[team as usize & 1];
        for y in corner[1]..corner[1] + t.size {
            for x in corner[0]..corner[0] + t.size {
                if !self.grid.passable([x, y]) || fog.at(&self.grid, [x, y]) < EXPLORED {
                    return Err("Can't build there");
                }
            }
        }
        let rect = self.grid.rect(corner, t.size);
        if self.units.iter().any(|u| dist_to_rect(u.pos, rect) < 0.05) {
            return Err("Can't build there");
        }
        Ok(())
    }

    fn notice(&mut self, team: u8, text: &'static str) {
        if team == PLAYER {
            self.notices.push(text);
        }
    }

    fn apply(&mut self, c: Command, tune: &Tune) {
        let team = c.team;
        let mine = |u: &Unit| u.team == team && c.ids.contains(&u.id);
        match c.kind {
            CommandKind::Move { to, aggressive } => {
                let n = self.units.iter().filter(|u| mine(u)).count().min(u16::MAX as usize) as u16;
                for u in self.units.iter_mut().filter(|u| mine(u)) {
                    u.order = Order::Move { goal: to, group: n, aggressive };
                    u.resume = None;
                    u.stuck = 0;
                }
            }
            CommandKind::Attack { target } => {
                let enemy = self.unit(target).map(|u| u.team).or_else(|| self.building(target).map(|b| b.team));
                if enemy.is_some_and(|t| t != team) {
                    for u in self.units.iter_mut().filter(|u| mine(u)) {
                        u.order = Order::Attack { target, auto: false, anchor: u.pos };
                        u.resume = None;
                    }
                }
            }
            CommandKind::Gather { node } => {
                let Some(pos) = self.nodes.iter().find(|n| n.id == node).map(|n| n.pos) else {
                    return;
                };
                let n = self.units.iter().filter(|u| mine(u) && u.kind != WORKER).count() as u16;
                for u in self.units.iter_mut().filter(|u| mine(u)) {
                    u.resume = None;
                    u.stuck = 0;
                    u.order = if u.kind == WORKER {
                        Order::Gather { node, phase: if u.carry > 0.0 { Gather::ToHub } else { Gather::ToNode } }
                    } else {
                        Order::Move { goal: pos, group: n, aggressive: false }
                    };
                }
            }
            CommandKind::Build { kind, corner } => {
                if let Err(e) = self.can_place(team, kind, corner) {
                    self.notice(team, e);
                    return;
                }
                let site = self.grid.center([corner[0] + self.types.size(kind) / 2, corner[1] + self.types.size(kind) / 2]);
                let mut builders: Vec<u32> = self.units.iter().filter(|u| mine(u) && u.kind == WORKER).map(|u| u.id).collect();
                if builders.is_empty() {
                    // The nearest worker, idle ones first.
                    let pick = self
                        .units
                        .iter()
                        .filter(|u| u.team == team && u.kind == WORKER)
                        .min_by(|a, b| {
                            let ka = dist(a.pos, site) + if a.order == Order::Idle { 0.0 } else { 30.0 };
                            let kb = dist(b.pos, site) + if b.order == Order::Idle { 0.0 } else { 30.0 };
                            ka.total_cmp(&kb)
                        })
                        .map(|u| u.id);
                    builders.extend(pick);
                }
                if builders.is_empty() {
                    self.notice(team, "No worker to build it");
                    return;
                }
                self.crystal[team as usize & 1] -= self.types.building(kind, bs::COST);
                let id = self.add_building(team, kind, corner, false);
                for u in self.units.iter_mut().filter(|u| builders.contains(&u.id)) {
                    u.order = Order::Build { site: id };
                    u.resume = None;
                }
            }
            CommandKind::Help { site } => {
                if self.building(site).is_some_and(|b| b.team == team && !b.built) {
                    for u in self.units.iter_mut().filter(|u| mine(u) && u.kind == WORKER) {
                        u.order = Order::Build { site };
                        u.resume = None;
                    }
                }
            }
            CommandKind::Train { kind } => {
                let Some(t) = self.types.units.get(kind) else { return };
                let (cost, requires) = (t.stats[us::COST], t.requires);
                let qmax = tune.queue_max.max(1.0) as usize;
                let best = self
                    .buildings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.team == team && b.built && c.ids.contains(&b.id))
                    .filter(|(_, b)| self.types.buildings[b.kind].trains.contains(&kind))
                    .min_by_key(|(_, b)| b.queue.len())
                    .map(|(i, b)| (i, b.queue.len()));
                let Some((i, len)) = best else { return };
                if let Some(r) = requires
                    && !self.has_built(team, r) {
                        self.notice(team, Self::need(r));
                        return;
                    }
                if len >= qmax {
                    self.notice(team, "Queue is full");
                } else if self.crystal[team as usize & 1] < cost {
                    self.notice(team, "Not enough crystal");
                } else if self.supply(team) as f32 >= tune.pop_cap {
                    self.notice(team, "Supply cap reached");
                } else {
                    self.crystal[team as usize & 1] -= cost;
                    self.buildings[i].queue.push(kind);
                }
            }
            CommandKind::Rally { to } => {
                for b in self.buildings.iter_mut().filter(|b| b.team == team && c.ids.contains(&b.id)) {
                    b.rally = Some(to);
                }
            }
            CommandKind::Stop => {
                for u in self.units.iter_mut().filter(|u| mine(u)) {
                    u.order = Order::Idle;
                    u.resume = None;
                }
                let mut refund = 0.0;
                for b in self.buildings.iter_mut().filter(|b| b.team == team && c.ids.contains(&b.id)) {
                    if let Some(k) = b.queue.pop() {
                        refund += self.types.unit(k, us::COST);
                        if b.queue.is_empty() {
                            b.train_t = 0.0;
                        }
                    }
                }
                self.crystal[team as usize & 1] += refund;
            }
        }
    }

    /// Advance one fixed tick of `dt` seconds.
    pub fn step(&mut self, tune: &Tune, dt: f32) {
        self.tick += 1;
        let mut cmds = std::mem::take(&mut self.pending);
        // Stable: the player's then the AI's, each in the order given.
        cmds.sort_by_key(|c| c.team);
        for c in cmds {
            self.apply(c, tune);
        }
        if self.winner.is_some() {
            return;
        }
        self.produce(tune, dt);
        for b in &mut self.buildings {
            b.builders = 0;
        }
        let snap: Vec<Snap> = self
            .units
            .iter()
            .map(|u| Snap { id: u.id, team: u.team, pos: u.pos, radius: self.types.unit(u.kind, us::RADIUS) })
            .collect();
        let mut hits: Vec<(u32, f32)> = Vec::new();
        {
            let Match { units, buildings, nodes, grid, flow, crystal, types, shots, .. } = self;
            let mut ctx = Ctx { grid, flow, types, buildings, nodes, crystal, snap: &snap, hits: &mut hits, shots, tune, dt };
            for u in units.iter_mut() {
                ctx.update_unit(u);
            }
        }
        self.towers(&snap, &mut hits, dt);
        self.resolve(&hits);
        let every = tune.fog_ticks.max(1.0) as u64;
        if self.tick.is_multiple_of(every) {
            self.update_fog();
        }
        if !self.free_roam {
            for team in [PLAYER, RIVAL] {
                if self.hub(team).is_none() {
                    self.winner = Some(1 - team);
                }
            }
        }
        for s in &mut self.shots {
            s.t -= dt;
        }
        self.shots.retain(|s| s.t > 0.0);
    }

    /// Training: the front of every queue advances; finished units appear
    /// next to the building and head for its rally point (new workers with
    /// no rally point start gathering).
    fn produce(&mut self, tune: &Tune, dt: f32) {
        let mut supply = [self.supply_units(PLAYER), self.supply_units(RIVAL)];
        let mut ready = Vec::new();
        for (i, b) in self.buildings.iter_mut().enumerate() {
            if !b.built {
                continue;
            }
            let Some(&kind) = b.queue.first() else { continue };
            let t = b.team as usize & 1;
            if supply[t] as f32 >= tune.pop_cap {
                continue;
            }
            b.train_t += dt;
            if b.train_t >= self.types.unit(kind, us::TRAIN_S) {
                b.train_t = 0.0;
                b.queue.remove(0);
                supply[t] += 1;
                ready.push((i, kind));
            }
        }
        for (i, kind) in ready {
            let p = self.spawn_point(i);
            let (team, rally, hub_center) = {
                let b = &self.buildings[i];
                let r = self.grid.rect(b.corner, b.size);
                (b.team, b.rally, [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5])
            };
            let id = self.spawn_unit(team, kind, p);
            let order = match rally {
                Some(goal) => Order::Move { goal, group: 1, aggressive: kind != WORKER },
                None if kind == WORKER => match nearest_node(&self.nodes, hub_center, 30.0) {
                    Some(node) => Order::Gather { node, phase: Gather::ToNode },
                    None => Order::Idle,
                },
                None => Order::Idle,
            };
            if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                u.order = order;
            }
        }
    }

    fn supply_units(&self, team: u8) -> usize {
        self.supply(team)
            - self.buildings.iter().filter(|b| b.team == team).map(|b| b.queue.len()).sum::<usize>()
    }

    /// Bastions shoot the nearest enemy unit in range.
    fn towers(&mut self, snap: &[Snap], hits: &mut Vec<(u32, f32)>, dt: f32) {
        for b in &mut self.buildings {
            let damage = self.types.building(b.kind, bs::DAMAGE);
            if !b.built || damage <= 0.0 {
                continue;
            }
            b.cooldown = (b.cooldown - dt).max(0.0);
            if b.cooldown > 0.0 {
                continue;
            }
            let rect = self.grid.rect(b.corner, b.size);
            let range = self.types.building(b.kind, bs::RANGE);
            let target = snap
                .iter()
                .filter(|s| s.team != b.team)
                .map(|s| (s, dist_to_rect(s.pos, rect) - s.radius))
                .filter(|(_, d)| *d <= range)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((s, _)) = target {
                hits.push((s.id, damage));
                b.cooldown = self.types.building(b.kind, bs::COOLDOWN).max(0.05);
                let from = [(rect[0] + rect[2]) * 0.5, (rect[1] + rect[3]) * 0.5];
                self.shots.push(Shot { from, to: s.pos, team: b.team, t: 0.15, heavy: true });
            }
        }
    }

    /// Damage lands, the dead go, depleted nodes and fallen buildings free
    /// their cells.
    fn resolve(&mut self, hits: &[(u32, f32)]) {
        for &(id, dmg) in hits {
            if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                u.hp -= dmg;
            } else if let Some(b) = self.buildings.iter_mut().find(|b| b.id == id) {
                b.hp -= dmg;
            }
        }
        self.units.retain(|u| u.hp > 0.0);
        let mut freed = Vec::new();
        self.buildings.retain(|b| {
            let alive = b.hp > 0.0;
            if !alive {
                freed.push((b.corner, b.size));
            }
            alive
        });
        for (corner, size) in freed {
            for y in corner[1]..corner[1] + size {
                for x in corner[0]..corner[0] + size {
                    self.grid.set_blocked([x, y], false);
                }
            }
        }
        let mut gone = Vec::new();
        self.nodes.retain(|n| {
            if n.amount <= 0.0 {
                gone.push(n.cell);
                false
            } else {
                true
            }
        });
        for c in gone {
            self.grid.set_blocked(c, false);
        }
    }

    pub fn update_fog(&mut self) {
        for team in [PLAYER, RIVAL] {
            let mut sources: Vec<([f32; 2], f32)> = self
                .units
                .iter()
                .filter(|u| u.team == team)
                .map(|u| (u.pos, self.types.unit(u.kind, us::SIGHT)))
                .collect();
            for b in self.buildings.iter().filter(|b| b.team == team) {
                let r = self.grid.rect(b.corner, b.size);
                let sight = self.types.building(b.kind, bs::SIGHT) * if b.built { 1.0 } else { 0.5 };
                sources.push(([(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5], sight));
            }
            self.fog[team as usize].update(&self.grid, &sources);
        }
    }
}

/// The nearest node with crystal left within `max` metres of `from`.
pub fn nearest_node(nodes: &[Node], from: [f32; 2], max: f32) -> Option<u32> {
    nodes
        .iter()
        .filter(|n| n.amount > 0.0)
        .map(|n| (n.id, dist(n.pos, from)))
        .filter(|(_, d)| *d <= max)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// What a unit's update may touch.
struct Ctx<'a> {
    grid: &'a Grid,
    flow: &'a mut FlowCache,
    types: &'a Types,
    buildings: &'a mut Vec<Building>,
    nodes: &'a mut Vec<Node>,
    crystal: &'a mut [f32; 2],
    snap: &'a [Snap],
    hits: &'a mut Vec<(u32, f32)>,
    shots: &'a mut Vec<Shot>,
    tune: &'a Tune,
    dt: f32,
}

impl Ctx<'_> {
    fn target(&self, id: u32) -> Option<(u8, Shape)> {
        if let Some(s) = self.snap.iter().find(|s| s.id == id) {
            return Some((s.team, Shape::Circle(s.pos, s.radius)));
        }
        self.buildings
            .iter()
            .find(|b| b.id == id)
            .map(|b| (b.team, Shape::Rect(self.grid.rect(b.corner, b.size))))
    }

    /// The nearest enemy within `sight`: units first, then buildings.
    fn acquire(&self, u: &Unit, sight: f32) -> Option<u32> {
        let unit = self
            .snap
            .iter()
            .filter(|s| s.team != u.team)
            .map(|s| (s.id, dist(s.pos, u.pos) - s.radius))
            .filter(|(_, d)| *d <= sight)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = unit {
            return Some(id);
        }
        self.buildings
            .iter()
            .filter(|b| b.team != u.team)
            .map(|b| (b.id, dist_to_rect(u.pos, self.grid.rect(b.corner, b.size))))
            .filter(|(_, d)| *d <= sight)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// One step's motion toward `goal` along the shared flow field.
    fn steer(&mut self, pos: [f32; 2], goal: [f32; 2], step: f32) -> [f32; 2] {
        let cur = self.grid.cell_of(pos);
        let gc = self.grid.cell_of(goal);
        let aim = if cur == gc || dist(pos, goal) < self.grid.cell {
            goal
        } else {
            match self.flow.get(self.grid, gc).next(self.grid, cur) {
                Some(n) => self.grid.center(n),
                None => goal,
            }
        };
        let d = dist(pos, aim);
        if d < 1e-4 {
            return [0.0, 0.0];
        }
        let s = step.min(d) / d;
        [(aim[0] - pos[0]) * s, (aim[1] - pos[1]) * s]
    }

    fn update_unit(&mut self, u: &mut Unit) {
        let (tune, dt) = (self.tune, self.dt);
        let stat = |s| self.types.unit(u.kind, s);
        let (speed, radius, sight, range, damage, cooldown) =
            (stat(us::SPEED), stat(us::RADIUS), stat(us::SIGHT), stat(us::RANGE), stat(us::DAMAGE), stat(us::COOLDOWN));
        u.cooldown = (u.cooldown - dt).max(0.0);
        let step = speed * dt;
        let mut v = [0.0f32; 2];
        let order = u.order;
        match order {
            Order::Idle => {
                if u.kind != WORKER
                    && let Some(target) = self.acquire(u, sight) {
                        u.resume = Some(Order::Idle);
                        u.order = Order::Attack { target, auto: true, anchor: u.pos };
                    }
            }
            Order::Move { goal, group, aggressive } => {
                let found = if aggressive && u.kind != WORKER { self.acquire(u, sight) } else { None };
                if let Some(target) = found {
                    u.resume = Some(order);
                    u.order = Order::Attack { target, auto: true, anchor: u.pos };
                } else if dist(u.pos, goal) <= 0.5 + tune.arrive_spread * f32::from(group.max(1)).sqrt() || u.stuck > 120 {
                    u.order = Order::Idle;
                    u.stuck = 0;
                } else {
                    v = self.steer(u.pos, goal, step);
                }
            }
            Order::Attack { target, auto, anchor } => match self.target(target) {
                Some((team, shape)) if team != u.team => {
                    if auto && dist(u.pos, anchor) > tune.chase_leash {
                        u.order = u.resume.take().unwrap_or(Order::Idle);
                    } else if shape.gap(u.pos) - radius <= range {
                        let c = shape.center();
                        u.facing = (c[0] - u.pos[0]).atan2(c[1] - u.pos[1]);
                        if u.cooldown <= 0.0 && damage > 0.0 {
                            self.hits.push((target, damage));
                            u.cooldown = cooldown.max(0.05);
                            self.shots.push(Shot { from: u.pos, to: c, team: u.team, t: 0.12, heavy: u.kind == crate::data::RUMBLER });
                        }
                    } else {
                        v = self.steer(u.pos, shape.center(), step);
                    }
                }
                _ => u.order = u.resume.take().unwrap_or(Order::Idle),
            },
            Order::Gather { node, phase } => self.gather(u, node, phase, radius, step, &mut v),
            Order::Build { site } => {
                let Some(b) = self.buildings.iter_mut().find(|b| b.id == site && !b.built) else {
                    u.order = Order::Idle;
                    return;
                };
                let rect = self.grid.rect(b.corner, b.size);
                if dist_to_rect(u.pos, rect) - radius <= tune.reach {
                    if f32::from(b.builders) < tune.max_builders {
                        b.builders += 1;
                        let build_s = self.types.building(b.kind, bs::BUILD_S).max(0.1);
                        let max = self.types.building(b.kind, bs::HP);
                        b.progress += dt / build_s;
                        b.hp = (b.hp + max * 0.9 * dt / build_s).min(max);
                        if b.progress >= 1.0 {
                            b.progress = 1.0;
                            b.built = true;
                        }
                    }
                } else {
                    let c = [(rect[0] + rect[2]) * 0.5, (rect[1] + rect[3]) * 0.5];
                    v = self.steer(u.pos, c, step);
                }
            }
        }
        self.motion(u, v, radius, step);
    }

    fn gather(&mut self, u: &mut Unit, node: u32, phase: Gather, radius: f32, step: f32, v: &mut [f32; 2]) {
        let tune = self.tune;
        if u.kind != WORKER {
            u.order = Order::Idle;
            return;
        }
        match phase {
            Gather::ToNode => match self.nodes.iter().find(|n| n.id == node && n.amount > 0.0).map(|n| (n.cell, n.pos)) {
                Some((cell, pos)) => {
                    if dist_to_rect(u.pos, self.grid.rect(cell, 1)) - radius <= tune.reach {
                        u.order = Order::Gather { node, phase: Gather::Mining(tune.gather_s) };
                    } else {
                        *v = self.steer(u.pos, pos, step);
                    }
                }
                None => {
                    let from = self.nodes.iter().find(|n| n.id == node).map_or(u.pos, |n| n.pos);
                    u.order = match nearest_node(self.nodes.as_slice(), from, 24.0) {
                        Some(next) => Order::Gather { node: next, phase: Gather::ToNode },
                        None => Order::Idle,
                    };
                }
            },
            Gather::Mining(t) => {
                let t = t - self.dt;
                if t > 0.0 {
                    u.order = Order::Gather { node, phase: Gather::Mining(t) };
                } else {
                    if let Some(n) = self.nodes.iter_mut().find(|n| n.id == node) {
                        let take = tune.gather_amount.min(n.amount).max(0.0);
                        n.amount -= take;
                        u.carry += take;
                    }
                    u.order = Order::Gather { node, phase: Gather::ToHub };
                }
            }
            Gather::ToHub => {
                let hub = self
                    .buildings
                    .iter()
                    .filter(|b| b.team == u.team && b.kind == HUB && b.built)
                    .map(|b| self.grid.rect(b.corner, b.size))
                    .min_by(|a, b| dist_to_rect(u.pos, *a).total_cmp(&dist_to_rect(u.pos, *b)));
                match hub {
                    None => u.order = Order::Idle,
                    Some(r) => {
                        if dist_to_rect(u.pos, r) - radius <= tune.reach {
                            self.crystal[u.team as usize & 1] += u.carry;
                            u.carry = 0.0;
                            u.order = Order::Gather { node, phase: Gather::ToNode };
                        } else {
                            *v = self.steer(u.pos, [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5], step);
                        }
                    }
                }
            }
        }
    }

    /// Move by `v` plus separation from nearby units, sliding along
    /// blocked cells.
    fn motion(&mut self, u: &mut Unit, v: [f32; 2], radius: f32, step: f32) {
        let mut push = [0.0f32; 2];
        for s in self.snap {
            if s.id == u.id {
                continue;
            }
            let (dx, dz) = (u.pos[0] - s.pos[0], u.pos[1] - s.pos[1]);
            let min = radius + s.radius;
            let d2 = dx * dx + dz * dz;
            if d2 >= min * min {
                continue;
            }
            if d2 < 1e-8 {
                // Exactly on top: split by id, the same on every machine.
                let a = (u.id % 16) as f32 * core::f32::consts::FRAC_PI_8;
                push[0] += a.cos() * 0.02;
                push[1] += a.sin() * 0.02;
                continue;
            }
            let d = d2.sqrt();
            let k = (min - d) / d * self.tune.separation * 0.5;
            push[0] += dx * k;
            push[1] += dz * k;
        }
        let cap = (step.max(0.02)) * 1.5;
        let pl = (push[0] * push[0] + push[1] * push[1]).sqrt();
        if pl > cap {
            push = [push[0] / pl * cap, push[1] / pl * cap];
        }
        let old = u.pos;
        let want = [old[0] + v[0] + push[0], old[1] + v[1] + push[1]];
        let g = self.grid;
        if g.passable(g.cell_of(want)) {
            u.pos = want;
        } else if g.passable(g.cell_of([want[0], old[1]])) {
            u.pos = [want[0], old[1]];
        } else if g.passable(g.cell_of([old[0], want[1]])) {
            u.pos = [old[0], want[1]];
        }
        u.moving = v[0] != 0.0 || v[1] != 0.0;
        if u.moving {
            u.facing = v[0].atan2(v[1]);
        }
        if matches!(u.order, Order::Move { .. }) && dist(old, u.pos) < step * 0.15 {
            u.stuck = u.stuck.saturating_add(1);
        } else {
            u.stuck = 0;
        }
    }
}

/// Is `id` a building of `kind` that `team` may still raise?
pub fn is_site(m: &Match, team: u8, id: u32) -> bool {
    m.building(id).is_some_and(|b| b.team == team && !b.built)
}

/// Kinds a producer offers, in key order (E then R).
pub fn trains(m: &Match, b: &Building) -> Vec<usize> {
    m.types.buildings.get(b.kind).map(|t| t.trains.clone()).unwrap_or_default()
}

/// The bastion kind, for the build cycle.
pub const BUILD_CYCLE: [usize; 2] = [FOUNDRY, BASTION];
