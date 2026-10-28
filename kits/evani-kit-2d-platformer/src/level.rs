//! The tile-map level (`data/level.json`): loader, validation and the
//! collision queries the bodies use.
//!
//! Coordinates are art pixels with y pointing down; a tile is [`TILE`]
//! pixels. Entity positions in the JSON are tile coordinates: a standing
//! entity at `[x, y]` has its feet on the bottom of tile `(x, y)`, so its
//! foot point is `((x + 0.5) * 16, (y + 1) * 16)`; coins sit at the tile
//! centre `((x + 0.5) * 16, (y + 0.5) * 16)`.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Pixels per tile.
pub const TILE: f32 = 16.0;

/// What a tile does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tile {
    #[default]
    Empty,
    /// A full block; `art` picks earth (0) or stone (1).
    Solid { art: u8 },
    /// Top surface only; `art` picks plank (0) or grassy ledge (1).
    OneWay { art: u8 },
    /// A floor line from `from` to `to` pixels above the tile bottom.
    Slope { from: u8, to: u8 },
    /// Hazard; `deep` is below the surface.
    Water { deep: bool },
    /// Hazard.
    Spikes,
    /// Drawn behind everything, never collides (`art` 0 = castle wall).
    Decoration { art: u8 },
}

impl Tile {
    pub fn is_solid(self) -> bool {
        matches!(self, Tile::Solid { .. })
    }

    pub fn is_slope(self) -> bool {
        matches!(self, Tile::Slope { .. })
    }

    pub fn is_hazard(self) -> bool {
        matches!(self, Tile::Water { .. } | Tile::Spikes)
    }

    /// Height of a slope's floor above the tile bottom at `fx` (0..1 across
    /// the tile).
    pub fn slope_height(self, fx: f32) -> Option<f32> {
        match self {
            Tile::Slope { from, to } => {
                let f = fx.clamp(0.0, 1.0);
                Some(from as f32 + (to as f32 - from as f32) * f)
            }
            _ => None,
        }
    }
}

/// Moving platform path mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    PingPong,
    Loop,
}

/// A moving platform (data). Positions are the left end of the top
/// surface in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformDef {
    pub width_px: f32,
    pub path: Vec<[f32; 2]>,
    /// Pixels per second; 0 means use the `platform_speed` param.
    pub speed: f32,
    pub mode: PathMode,
    pub wait_s: f32,
}

impl PlatformDef {
    /// Total path length in pixels (the closing leg included for loops).
    pub fn length(&self) -> f32 {
        let n = self.path.len();
        let mut l: f32 = self.path.windows(2).map(|w| dist(w[0], w[1])).sum();
        if self.mode == PathMode::Loop && n > 2 {
            l += dist(self.path[n - 1], self.path[0]);
        }
        l
    }

    /// Top-left of the platform at `t` seconds of level time: a pure
    /// function of time (deterministic, no state), with waits at each end
    /// (ping-pong) or at the first point (loop).
    pub fn pos_at(&self, t: f64, speed: f32) -> [f32; 2] {
        let n = self.path.len();
        if n == 0 {
            return [0.0, 0.0];
        }
        let len = self.length();
        if n == 1 || len <= 0.0 || speed <= 0.0 {
            return self.path[0];
        }
        let travel = (len / speed) as f64;
        let wait = self.wait_s.max(0.0) as f64;
        let d = match self.mode {
            PathMode::PingPong => {
                let period = 2.0 * (travel + wait);
                let p = t.rem_euclid(period);
                let s = if p < wait {
                    0.0
                } else if p < wait + travel {
                    (p - wait) / travel
                } else if p < 2.0 * wait + travel {
                    1.0
                } else {
                    1.0 - (p - 2.0 * wait - travel) / travel
                };
                s as f32 * len
            }
            PathMode::Loop => {
                let period = travel + wait;
                let p = t.rem_euclid(period);
                if p < wait { 0.0 } else { ((p - wait) / travel) as f32 * len }
            }
        };
        self.point_at(d)
    }

    fn point_at(&self, mut d: f32) -> [f32; 2] {
        let n = self.path.len();
        let legs = if self.mode == PathMode::Loop && n > 2 { n } else { n - 1 };
        for i in 0..legs {
            let a = self.path[i];
            let b = self.path[(i + 1) % n];
            let l = dist(a, b);
            if d <= l || i + 1 == legs {
                let f = if l > 0.0 { (d / l).clamp(0.0, 1.0) } else { 0.0 };
                return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
            }
            d -= l;
        }
        self.path[0]
    }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
}

/// A decoration sprite standing on the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct Deco {
    pub kind: String,
    /// Foot point in pixels.
    pub pos: [f32; 2],
}

/// The loaded level.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub name: String,
    pub time_limit_s: f32,
    pub w: usize,
    pub h: usize,
    tiles: Vec<Tile>,
    /// Foot points in pixels.
    pub start: [f32; 2],
    pub goal: [f32; 2],
    pub beetles: Vec<[f32; 2]>,
    pub hoppers: Vec<[f32; 2]>,
    pub checkpoints: Vec<[f32; 2]>,
    /// Torch centres (pixels).
    pub torches: Vec<[f32; 2]>,
    /// Coin centres (pixels).
    pub coins: Vec<[f32; 2]>,
    pub decorations: Vec<Deco>,
    pub platforms: Vec<PlatformDef>,
    /// Clouds in the sky.
    pub clouds: u32,
}

#[derive(Deserialize)]
struct LegendJson {
    kind: String,
    #[serde(default)]
    art: Option<String>,
    #[serde(default)]
    from: Option<u8>,
    #[serde(default)]
    to: Option<u8>,
}

#[derive(Deserialize)]
struct DecoJson {
    kind: String,
    at: [f32; 2],
}

#[derive(Deserialize)]
struct EntitiesJson {
    start: Option<[f32; 2]>,
    goal: Option<[f32; 2]>,
    #[serde(default)]
    coins: Vec<[f32; 2]>,
    #[serde(default)]
    beetles: Vec<[f32; 2]>,
    #[serde(default)]
    hoppers: Vec<[f32; 2]>,
    #[serde(default)]
    checkpoints: Vec<[f32; 2]>,
    #[serde(default)]
    torches: Vec<[f32; 2]>,
    #[serde(default)]
    decorations: Vec<DecoJson>,
}

#[derive(Deserialize)]
struct PlatformJson {
    width: f32,
    path: Vec<[f32; 2]>,
    #[serde(default)]
    speed: f32,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    wait: f32,
}

#[derive(Deserialize, Default)]
struct BackgroundJson {
    #[serde(default)]
    clouds: u32,
}

#[derive(Deserialize)]
struct LevelJson {
    name: String,
    time_limit_s: f32,
    tile: f32,
    legend: BTreeMap<String, LegendJson>,
    rows: Vec<String>,
    entities: EntitiesJson,
    #[serde(default)]
    platforms: Vec<PlatformJson>,
    #[serde(default)]
    background: BackgroundJson,
}

fn legend_tile(c: &str, l: &LegendJson) -> Result<Tile, String> {
    let art = l.art.as_deref().unwrap_or("");
    Ok(match l.kind.as_str() {
        "empty" => Tile::Empty,
        "solid" => Tile::Solid { art: u8::from(art == "stone") },
        "oneway" => Tile::OneWay { art: u8::from(art == "ledge") },
        "slope" => {
            let (from, to) = (l.from.unwrap_or(0), l.to.unwrap_or(0));
            if from > 16 || to > 16 || from == to {
                return Err(format!("legend '{c}': slope heights must be 0..16 and differ"));
            }
            Tile::Slope { from, to }
        }
        "water" => Tile::Water { deep: art == "deep" },
        "spikes" => Tile::Spikes,
        "decoration" => Tile::Decoration { art: 0 },
        k => return Err(format!("legend '{c}': unknown kind '{k}'")),
    })
}

impl Level {
    /// Parse and validate `level.json`. Errors name the row and column.
    pub fn from_json(src: &str) -> Result<Self, String> {
        let j: LevelJson = serde_json::from_str(src).map_err(|e| format!("level.json: {e}"))?;
        if (j.tile - TILE).abs() > f32::EPSILON {
            return Err(format!(
                "level.json: tile must be {}, not {}",
                evani_core::num::short(TILE, 4),
                evani_core::num::short(j.tile, 4)
            ));
        }
        let mut legend = BTreeMap::new();
        for (c, l) in &j.legend {
            let mut chars = c.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                return Err(format!("legend key '{c}' must be one character"));
            };
            legend.insert(ch, legend_tile(c, l)?);
        }
        let h = j.rows.len();
        if h == 0 {
            return Err("level.json: no rows".into());
        }
        let w = j.rows[0].chars().count();
        let mut tiles = Vec::with_capacity(w * h);
        for (y, row) in j.rows.iter().enumerate() {
            if row.chars().count() != w {
                return Err(format!("row {y}: {} columns, expected {w}", row.chars().count()));
            }
            for (x, ch) in row.chars().enumerate() {
                let t = legend
                    .get(&ch)
                    .copied()
                    .ok_or_else(|| format!("row {y}, column {x}: unknown tile '{ch}'"))?;
                tiles.push(t);
            }
        }
        let e = &j.entities;
        let feet = |p: [f32; 2]| [(p[0] + 0.5) * TILE, (p[1] + 1.0) * TILE];
        let centre = |p: [f32; 2]| [(p[0] + 0.5) * TILE, (p[1] + 0.5) * TILE];
        let start = e.start.map(feet).ok_or("entities: no start")?;
        let goal = e.goal.map(feet).ok_or("entities: no goal")?;
        let in_bounds = |what: &str, p: [f32; 2]| {
            if p[0] < 0.0 || p[1] < 0.0 || p[0] >= w as f32 || p[1] >= h as f32 {
                // `{p:?}` text without shipping float formatting.
                let (px, py) = (evani_core::num::short_debug(p[0], 4), evani_core::num::short_debug(p[1], 4));
                Err(format!("entities: {what} at [{px}, {py}] is outside the {w} x {h} map"))
            } else {
                Ok(())
            }
        };
        in_bounds("start", e.start.unwrap_or_default())?;
        in_bounds("goal", e.goal.unwrap_or_default())?;
        for (what, list) in [
            ("coin", &e.coins),
            ("beetle", &e.beetles),
            ("hopper", &e.hoppers),
            ("checkpoint", &e.checkpoints),
            ("torch", &e.torches),
        ] {
            for p in list {
                in_bounds(what, *p)?;
            }
        }
        let mut platforms = Vec::new();
        for (i, p) in j.platforms.iter().enumerate() {
            if p.path.len() < 2 {
                return Err(format!("platforms[{i}]: a path needs two or more points"));
            }
            if p.width <= 0.0 {
                return Err(format!("platforms[{i}]: width must be positive"));
            }
            let mode = match p.mode.as_deref().unwrap_or("pingpong") {
                "pingpong" => PathMode::PingPong,
                "loop" => PathMode::Loop,
                m => return Err(format!("platforms[{i}]: unknown mode '{m}'")),
            };
            platforms.push(PlatformDef {
                width_px: p.width * TILE,
                path: p.path.iter().map(|q| [q[0] * TILE, q[1] * TILE]).collect(),
                speed: p.speed.max(0.0),
                mode,
                wait_s: p.wait.max(0.0),
            });
        }
        let level = Self {
            name: j.name,
            time_limit_s: j.time_limit_s.max(1.0),
            w,
            h,
            tiles,
            start,
            goal,
            beetles: e.beetles.iter().copied().map(feet).collect(),
            hoppers: e.hoppers.iter().copied().map(feet).collect(),
            checkpoints: e.checkpoints.iter().copied().map(feet).collect(),
            torches: e.torches.iter().copied().map(centre).collect(),
            coins: e.coins.iter().copied().map(centre).collect(),
            decorations: e.decorations.iter().map(|d| Deco { kind: d.kind.clone(), pos: feet(d.at) }).collect(),
            platforms,
            clouds: j.background.clouds,
        };
        Ok(level)
    }

    pub fn width_px(&self) -> f32 {
        self.w as f32 * TILE
    }

    /// Per column: the row where a bottomless pit's opening starts, or
    /// `None` for a column with ground at the map's bottom. A pit is a run
    /// of columns whose bottom row is empty; it opens below the lower of the
    /// two grounds beside it (rows from there down are the gap, rows above
    /// are open air the background shows through).
    pub fn pit_lips(&self) -> Vec<Option<i32>> {
        let open = |tx: i32| self.tile(tx, self.h as i32 - 1) == Tile::Empty;
        // The first row with something to stand on in column `tx`.
        let top = |tx: i32| (0..self.h as i32).find(|&ty| matches!(self.tile(tx, ty), Tile::Solid { .. } | Tile::Slope { .. }));
        let mut lips = vec![None; self.w];
        let mut tx = 0;
        while tx < self.w as i32 {
            if !open(tx) {
                tx += 1;
                continue;
            }
            let end = (tx..self.w as i32).find(|&x| !open(x)).unwrap_or(self.w as i32);
            let sides = [tx - 1, end].into_iter().filter(|&x| x >= 0 && x < self.w as i32).filter_map(top);
            if let Some(lip) = sides.max() {
                for x in tx..end {
                    lips[x as usize] = Some(lip);
                }
            }
            tx = end;
        }
        lips
    }

    pub fn height_px(&self) -> f32 {
        self.h as f32 * TILE
    }

    /// The tile at `(tx, ty)`. Left and right of the map are solid walls;
    /// above and below it is empty (falling out of the bottom is a hazard
    /// the game checks).
    pub fn tile(&self, tx: i32, ty: i32) -> Tile {
        if tx < 0 || tx >= self.w as i32 {
            return Tile::Solid { art: 0 };
        }
        if ty < 0 || ty >= self.h as i32 {
            return Tile::Empty;
        }
        self.tiles[ty as usize * self.w + tx as usize]
    }

    /// The tile under a pixel position.
    pub fn tile_at(&self, x: f32, y: f32) -> Tile {
        self.tile(cell(x), cell(y))
    }

    /// The highest landable static surface (smallest y) in `[lo, hi]`
    /// under a body whose feet span `x0..x1` with its foot point at `xc`.
    /// The foot column's slope wins when there is one (slopes use only the
    /// foot point); otherwise solid tops under the whole foot span, and
    /// one-way tops when `oneway_from` is `Some(old_y)` and the feet were
    /// at or above that top (within half a pixel).
    pub fn surface(&self, x0: f32, x1: f32, xc: f32, lo: f32, hi: f32, oneway_from: Option<f32>) -> Option<Surface> {
        if hi < lo {
            return None;
        }
        let (ty0, ty1) = (cell(lo), cell(hi));
        // Slopes under the foot point.
        let tx = cell(xc);
        let fx = (xc - tx as f32 * TILE) / TILE;
        for ty in ty0..=ty1 {
            let t = self.tile(tx, ty);
            if let Some(hgt) = t.slope_height(fx) {
                let sy = (ty + 1) as f32 * TILE - hgt;
                if sy >= lo && sy <= hi {
                    return Some(Surface { y: sy, kind: SurfaceKind::Slope });
                }
            }
        }
        let mut best: Option<Surface> = None;
        for tx in cell(x0)..=cell(x1 - 0.001) {
            for ty in ty0..=ty1 {
                let top = ty as f32 * TILE;
                if top < lo || top > hi {
                    continue;
                }
                let kind = match self.tile(tx, ty) {
                    Tile::Solid { .. } if !self.tile(tx, ty - 1).is_solid() => SurfaceKind::Solid,
                    Tile::OneWay { .. } if oneway_from.is_some_and(|old| old <= top + 0.5) => SurfaceKind::OneWay,
                    _ => continue,
                };
                if best.is_none_or(|b| top < b.y) {
                    best = Some(Surface { y: top, kind });
                }
                break;
            }
        }
        best
    }
}

/// A floor found under a body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    pub y: f32,
    pub kind: SurfaceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Solid,
    Slope,
    OneWay,
    /// Moving platform `i`.
    Platform(usize),
}

/// Tile index of a pixel coordinate.
pub fn cell(v: f32) -> i32 {
    (v / TILE).floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: &str = include_str!("../data/level.json");

    #[test]
    fn the_first_pit_opens_below_the_ground_beside_it() {
        let l = Level::from_json(LEVEL).unwrap();
        let lips = l.pit_lips();
        assert_eq!(lips.len(), l.w);
        let pit: Vec<usize> = (0..l.w).filter(|&x| lips[x].is_some()).collect();
        assert_eq!(pit, (28..38).collect::<Vec<_>>(), "the 10-tile pit by the start is the only one");
        assert!(pit.iter().all(|&x| lips[x] == Some(12)));
    }

    #[test]
    fn bramble_hills_loads_and_validates() {
        let l = Level::from_json(LEVEL).unwrap();
        assert_eq!(l.name, "Bramble Hills");
        assert!((200..=220).contains(&l.w), "{}", l.w);
        // 15 rows of play plus 5 of deep soil under the lowest ground, so
        // the camera never rests on the map's bottom (critic round 2).
        assert_eq!(l.h, 20);
        assert!(l.coins.len() >= 40);
        assert_eq!(l.platforms.len(), 2);
        assert!(!l.beetles.is_empty() && !l.hoppers.is_empty() && !l.checkpoints.is_empty());
        // Start stands on ground, the goal is far to the right.
        let s = l.surface(l.start[0] - 5.0, l.start[0] + 5.0, l.start[0], l.start[1] - 2.0, l.start[1] + 2.0, None);
        assert_eq!(s.map(|s| s.y), Some(l.start[1]));
        assert!(l.goal[0] > l.width_px() * 0.85);
        // Every kind of tile the spec lists is in the map.
        let mut seen = [false; 7];
        for y in 0..l.h as i32 {
            for x in 0..l.w as i32 {
                let i = match l.tile(x, y) {
                    Tile::Empty => 0,
                    Tile::Solid { .. } => 1,
                    Tile::OneWay { .. } => 2,
                    Tile::Slope { .. } => 3,
                    Tile::Water { .. } => 4,
                    Tile::Spikes => 5,
                    Tile::Decoration { .. } => 6,
                };
                seen[i] = true;
            }
        }
        assert!(seen.iter().all(|s| *s), "{seen:?}");
    }

    #[test]
    fn bad_levels_name_the_row_and_column() {
        let base = r###"{"name":"t","time_limit_s":10,"tile":16,"legend":{".":{"kind":"empty"},"#":{"kind":"solid"}},
            "rows":ROWS,"entities":{"start":[0,0],"goal":[1,0]}}"###;
        let e = Level::from_json(&base.replace("ROWS", r###"["..","#?"]"###)).unwrap_err();
        assert!(e.contains("row 1, column 1"), "{e}");
        let e = Level::from_json(&base.replace("ROWS", "[\"..\",\"###\"]")).unwrap_err();
        assert!(e.contains("row 1"), "{e}");
        let no_goal = base.replace("ROWS", r###"["..","##"]"###).replace(r###","goal":[1,0]"###, "");
        assert!(Level::from_json(&no_goal).unwrap_err().contains("goal"));
        let one_point = base
            .replace("ROWS", r###"["..","##"]"###)
            .replace(r###""entities""###, r###""platforms":[{"width":2,"path":[[0,0]]}],"entities""###);
        assert!(Level::from_json(&one_point).unwrap_err().contains("two or more"));
        assert!(Level::from_json("{").is_err());
    }

    #[test]
    fn platforms_are_a_pure_function_of_time() {
        let p = PlatformDef {
            width_px: 32.0,
            path: vec![[0.0, 0.0], [100.0, 0.0]],
            speed: 50.0,
            mode: PathMode::PingPong,
            wait_s: 0.5,
        };
        assert_eq!(p.pos_at(0.25, 50.0), [0.0, 0.0]);
        assert_eq!(p.pos_at(1.5, 50.0), [50.0, 0.0]);
        assert_eq!(p.pos_at(2.75, 50.0), [100.0, 0.0]);
        assert_eq!(p.pos_at(4.0, 50.0), [50.0, 0.0]);
        assert_eq!(p.pos_at(5.0 + 1.5, 50.0), [50.0, 0.0]);
    }
}
