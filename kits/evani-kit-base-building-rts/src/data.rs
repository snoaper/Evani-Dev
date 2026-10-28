//! The kit's data files: the map (`data/map.json`) and the unit and
//! building types (`data/units.json`). The map is a few terrain rectangles
//! and node points, expanded into the grid here; with `mirror` the south
//! half is authored and the north half is its point reflection.

use serde::Deserialize;

/// The map, Cinder Ridge.
pub const MAP_JSON: &str = include_str!("../data/map.json");
/// Unit and building types with every stat.
pub const UNITS_JSON: &str = include_str!("../data/units.json");

/// What a grid cell is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Terrain {
    #[default]
    Grass,
    Rock,
    Water,
    Ford,
}

impl Terrain {
    /// Rock and water stop units; fords and grass don't.
    pub fn blocks(self) -> bool {
        matches!(self, Terrain::Rock | Terrain::Water)
    }
}

#[derive(Deserialize)]
struct RectJson {
    kind: Terrain,
    rect: [i32; 4],
}

#[derive(Deserialize)]
struct NodeJson {
    cell: [i32; 2],
    amount: f32,
}

#[derive(Deserialize)]
struct MapJson {
    name: String,
    size: [i32; 2],
    cell_m: f32,
    #[serde(default)]
    mirror: bool,
    terrain: Vec<RectJson>,
    nodes: Vec<NodeJson>,
    hub: [i32; 2],
    workers: u32,
    #[serde(default)]
    start_reveal_m: f32,
}

/// The map, expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct MapData {
    pub name: String,
    pub w: i32,
    pub h: i32,
    pub cell_m: f32,
    /// `w * h` cells, row by row from the north edge.
    pub terrain: Vec<Terrain>,
    /// Crystal nodes: cell and amount, the player's half first.
    pub nodes: Vec<([i32; 2], f32)>,
    /// The player's hub corner (lowest cell of its footprint).
    pub hub: [i32; 2],
    pub mirror: bool,
    pub workers: u32,
    /// Ground known (explored) around each hub when a match starts, metres.
    pub start_reveal_m: f32,
}

impl MapData {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let j: MapJson = serde_json::from_str(json).map_err(|e| format!("map.json: {e}"))?;
        let [w, h] = j.size;
        if !(8..=256).contains(&w) || !(8..=256).contains(&h) || j.cell_m <= 0.0 {
            return Err(format!("map.json: bad size {w}x{h} or cell {}", j.cell_m));
        }
        let mut terrain = vec![Terrain::Grass; (w * h) as usize];
        let mut paint = |r: [i32; 4], t: Terrain| {
            for y in r[1].max(0)..=r[3].min(h - 1) {
                for x in r[0].max(0)..=r[2].min(w - 1) {
                    terrain[(y * w + x) as usize] = t;
                }
            }
        };
        // Rectangles paint in order (fords after the river), each with its
        // mirror right after it, so the two halves always match.
        for r in &j.terrain {
            paint(r.rect, r.kind);
            if j.mirror {
                let m = [w - 1 - r.rect[2], h - 1 - r.rect[3], w - 1 - r.rect[0], h - 1 - r.rect[1]];
                paint(m, r.kind);
            }
        }
        let mut nodes: Vec<([i32; 2], f32)> = j.nodes.iter().map(|n| (n.cell, n.amount)).collect();
        if j.mirror {
            let n = nodes.len();
            for i in 0..n {
                let (c, a) = nodes[i];
                nodes.push(([w - 1 - c[0], h - 1 - c[1]], a));
            }
        }
        Ok(Self {
            name: j.name,
            w,
            h,
            cell_m: j.cell_m,
            terrain,
            nodes,
            hub: j.hub,
            mirror: j.mirror,
            workers: j.workers,
            start_reveal_m: j.start_reveal_m.max(0.0),
        })
    }

    /// A team's hub corner for a footprint of `size` cells (the rival's is
    /// the point reflection of the player's).
    pub fn hub_corner(&self, team: u8, size: i32) -> [i32; 2] {
        if team == 0 {
            self.hub
        } else {
            [self.w - self.hub[0] - size, self.h - self.hub[1] - size]
        }
    }
}

/// Unit stat names, in [`UnitType::stats`] order. Each is also the live
/// parameter `kit.base_building_rts.<unit>_<stat>`.
pub const UNIT_STATS: [&str; 9] = ["hp", "speed", "radius", "sight", "range", "damage", "cooldown", "cost", "train_s"];
/// Building stat names, in [`BuildingType::stats`] order (the footprint
/// size is fixed: it shapes the grid).
pub const BUILDING_STATS: [&str; 7] = ["hp", "cost", "build_s", "sight", "range", "damage", "cooldown"];

/// Indices into [`UNIT_STATS`] / [`UnitType::stats`].
pub mod us {
    pub const HP: usize = 0;
    pub const SPEED: usize = 1;
    pub const RADIUS: usize = 2;
    pub const SIGHT: usize = 3;
    pub const RANGE: usize = 4;
    pub const DAMAGE: usize = 5;
    pub const COOLDOWN: usize = 6;
    pub const COST: usize = 7;
    pub const TRAIN_S: usize = 8;
}

/// Indices into [`BUILDING_STATS`] / [`BuildingType::stats`].
pub mod bs {
    pub const HP: usize = 0;
    pub const COST: usize = 1;
    pub const BUILD_S: usize = 2;
    pub const SIGHT: usize = 3;
    pub const RANGE: usize = 4;
    pub const DAMAGE: usize = 5;
    pub const COOLDOWN: usize = 6;
}

/// Unit kinds, in `units.json` order.
pub const WORKER: usize = 0;
pub const TROOPER: usize = 1;
pub const RUMBLER: usize = 2;
/// Building kinds, in `units.json` order.
pub const HUB: usize = 0;
pub const FOUNDRY: usize = 1;
pub const BASTION: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct UnitType {
    pub id: String,
    pub stats: [f32; UNIT_STATS.len()],
    /// A building kind the team must have finished to train it.
    pub requires: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildingType {
    pub id: String,
    /// Footprint side in cells.
    pub size: i32,
    pub stats: [f32; BUILDING_STATS.len()],
    /// Unit kinds it trains.
    pub trains: Vec<usize>,
    pub requires: Option<usize>,
    pub buildable: bool,
}

/// Every unit and building type.
#[derive(Debug, Clone, PartialEq)]
pub struct Types {
    pub units: Vec<UnitType>,
    pub buildings: Vec<BuildingType>,
}

impl Types {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("units.json: {e}"))?;
        let list = |key: &str| v.get(key).and_then(|l| l.as_array()).cloned().ok_or_else(|| format!("units.json: no `{key}` list"));
        let (units_j, buildings_j) = (list("units")?, list("buildings")?);
        let id = |o: &serde_json::Value| o.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let num = |o: &serde_json::Value, k: &str| o.get(k).and_then(|n| n.as_f64()).unwrap_or(0.0) as f32;
        let b_ids: Vec<String> = buildings_j.iter().map(id).collect();
        let u_ids: Vec<String> = units_j.iter().map(id).collect();
        if u_ids != ["worker", "trooper", "rumbler"] || b_ids != ["hub", "foundry", "bastion"] {
            return Err(format!("units.json: expected units worker, trooper, rumbler and buildings hub, foundry, bastion in that order, got {u_ids:?} {b_ids:?}"));
        }
        let building = |o: &serde_json::Value| -> Result<Option<usize>, String> {
            match o.get("requires").and_then(|s| s.as_str()) {
                None => Ok(None),
                Some(r) => b_ids.iter().position(|b| b == r).map(Some).ok_or_else(|| format!("units.json: unknown requirement `{r}`")),
            }
        };
        let mut units = Vec::new();
        for o in &units_j {
            let mut stats = [0.0; UNIT_STATS.len()];
            for (s, name) in stats.iter_mut().zip(UNIT_STATS) {
                *s = num(o, name);
            }
            units.push(UnitType { id: id(o), stats, requires: building(o)? });
        }
        let mut buildings = Vec::new();
        for o in &buildings_j {
            let mut stats = [0.0; BUILDING_STATS.len()];
            for (s, name) in stats.iter_mut().zip(BUILDING_STATS) {
                *s = num(o, name);
            }
            let mut trains = Vec::new();
            for t in o.get("trains").and_then(|l| l.as_array()).into_iter().flatten() {
                let name = t.as_str().unwrap_or("");
                trains.push(u_ids.iter().position(|u| u == name).ok_or_else(|| format!("units.json: unknown unit `{name}`"))?);
            }
            buildings.push(BuildingType {
                id: id(o),
                size: (num(o, "size") as i32).clamp(1, 6),
                stats,
                trains,
                requires: building(o)?,
                buildable: o.get("buildable").and_then(|b| b.as_bool()).unwrap_or(true),
            });
        }
        Ok(Self { units, buildings })
    }

    pub fn unit(&self, kind: usize, stat: usize) -> f32 {
        self.units.get(kind).map_or(0.0, |u| u.stats[stat])
    }

    pub fn building(&self, kind: usize, stat: usize) -> f32 {
        self.buildings.get(kind).map_or(0.0, |b| b.stats[stat])
    }

    pub fn size(&self, kind: usize) -> i32 {
        self.buildings.get(kind).map_or(1, |b| b.size)
    }

    /// Display name with a capital ("Command hub", "Trooper").
    pub fn unit_name(kind: usize) -> &'static str {
        match kind {
            WORKER => "Worker",
            TROOPER => "Trooper",
            _ => "Rumbler",
        }
    }

    pub fn building_name(kind: usize) -> &'static str {
        match kind {
            HUB => "Command hub",
            FOUNDRY => "Foundry",
            _ => "Bastion",
        }
    }
}
