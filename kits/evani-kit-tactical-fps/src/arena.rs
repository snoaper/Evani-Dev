//! The Hexagon fortress: its layout from a seed, the sector frames, the
//! concrete floor tiles and cover cells, and the one solidity test
//! everything asks ([`Arena::probe`]). Ported from Hexagon's `rules.ts`
//! (sector frames, floor tiles, layout, `probe`, `liftAt`, `blocked`,
//! `inside`, `tryMove`, `carry`).
//!
//! Frames: Y is up. A point's polar angle in the XZ plane is `atan2(z, x)`.
//! Sector `k` is centred on angle `k * 60 deg`; its local frame is `(u, v)`:
//! `u` along the sector axis (distance from the centre towards the hexagon
//! edge it faces), `v` sideways. The middle level (`L = 1`) is the ring: its
//! local frame is the world turned by the ring angle.
//!
//! Concrete is one continuous material per level. Every level keeps a list
//! of hole spheres in its own frame ([`crate::holes`]); a hole removes all
//! concrete inside it, whatever cover cell or floor tile it belongs to.

use evani_core::rng::Pcg32;
use serde::Deserialize;

use crate::holes::Holes;

/// The arena layout data, shipped inside the wasm.
pub const ARENA_JSON: &str = include_str!("../data/arena.json");

/// Cover grid cells along `u` per sector.
pub const NU: usize = 24;
/// Cover grid cells across `v` per sector.
pub const NV: usize = 46;
/// Cover grid cells up per level.
pub const NY: usize = 4;
/// Cells in one sector's cover grid.
pub const PER: usize = NU * NV * NY;

/// 60 degrees.
pub const S60: f32 = core::f32::consts::FRAC_PI_3;
/// tan 30 degrees.
pub const TAN30: f32 = 0.577_350_26;
/// cos 30 degrees.
pub const COS30: f32 = 0.866_025_4;
/// sin 30 degrees.
pub const SIN30: f32 = 0.5;

/// What [`Arena::probe`] finds at a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solid {
    /// Nothing (air, a hole, a lift shaft, the chasm).
    Empty,
    /// Indestructible metal (walls, dividers, trim, rails).
    Metal,
    /// Concrete: the piece (cover structure or floor tile) it belongs to.
    Concrete(u32),
}

impl Solid {
    /// Anything solid at all.
    pub fn is_solid(self) -> bool {
        self != Solid::Empty
    }
}

/// Cover generator counts.
#[derive(Debug, Clone, Deserialize)]
pub struct CoverDef {
    /// Structures per sector template.
    pub structures: u32,
    /// Structures on the top level's base sectors (room for the core zone).
    pub structures_top_base: u32,
    /// Tallest cover per level (cells).
    pub max_height: [u32; 3],
}

/// Where a team's soldiers stand at a (re)spawn, in the base sector's frame.
#[derive(Debug, Clone, Deserialize)]
pub struct SpawnDef {
    /// `u` of even spawn slots.
    pub u_even: f32,
    /// `u` of odd spawn slots.
    pub u_odd: f32,
    /// `v` of slot 0.
    pub v0: f32,
    /// `v` step between slots.
    pub dv: f32,
    /// Slots before the row repeats.
    pub slots: u32,
}

/// The bots' route (Hexagon's `bot.ts`): the bottom level is split into two
/// halves by solid dividers, the ring cars and the top sectors are sealed.
#[derive(Debug, Clone, Deserialize)]
pub struct NavDef {
    /// The bottom level's halves, sectors in walking order.
    pub sides: [[usize; 3]; 2],
    /// Each half's crossing sector (where bots rise into a ring car).
    pub cross: [usize; 2],
    /// Distance from the centre to the bottom doorways' middle along a divider.
    pub door_s: f32,
    /// Wander range `u` in a held sector.
    pub wander_u: [f32; 2],
    /// Wander range `|v|`.
    pub wander_v: f32,
    /// Where bots stand in a ring car while it carries them (`u`).
    pub car_u: f32,
}

/// The fortress geometry (`data/arena.json`), Hexagon's layout constants.
#[derive(Debug, Clone, Deserialize)]
pub struct ArenaDef {
    /// Display name.
    pub name: String,
    /// Inner edge of every level (the chasm in the middle is inside it).
    pub a_in: f32,
    /// Outer metal wall.
    pub a_out: f32,
    /// Half thickness of the dividers between sectors.
    pub divh: f32,
    /// Floor slab thickness.
    pub slab: f32,
    /// Floor heights of the three levels.
    pub fy: [f32; 3],
    /// Wall height on the top level (open sky above it).
    pub top_wall: f32,
    /// Inner rail height (bottom and top levels).
    pub rail_h: f32,
    /// Inner rail thickness.
    pub rail_t: f32,
    /// Lift shafts: sector-local `u`.
    pub lift_u: f32,
    /// Lift shafts: `|v|` (`+` rises bottom to middle, `-` middle to top).
    pub lift_v: f32,
    /// Lift shaft half size.
    pub lift_h: f32,
    /// Doorway in a divider: start along the divider.
    pub door0: f32,
    /// Doorway end along the divider.
    pub door1: f32,
    /// Doorway height.
    pub door_h: f32,
    /// Core zone centre `u` on the top level of each base sector.
    pub core_u: f32,
    /// Core zone radius.
    pub core_r: f32,
    /// Base sector of each team (Blue, Orange).
    pub base: [usize; 2],
    /// Divider kinds per level (`d` = between sector `d` and `d + 1`): 1 doorway, 0 solid.
    pub divs: [[u8; 6]; 3],
    /// Metal trim width at the inner edge per level.
    pub trim_in: [f32; 3],
    /// Floor tile row edges between the inner trim and the lift row.
    pub floor_rows: [f32; 3],
    /// Metal foot of the outer wall.
    pub floor_out_inset: f32,
    /// Metal trim along each divider.
    pub floor_slant: f32,
    /// Metal frame round each lift shaft.
    pub lift_frame: f32,
    /// Target floor tile size.
    pub tile_size: f32,
    /// Eye height.
    pub eye: f32,
    /// Body radius (collision).
    pub radius: f32,
    /// Body height (collision).
    pub height: f32,
    /// Hit cylinder radius.
    pub hit_r: f32,
    /// Hit cylinder height.
    pub hit_h: f32,
    /// Cover generator counts.
    pub cover: CoverDef,
    /// Spawn layout.
    pub spawn: SpawnDef,
    /// The bots' route through the fortress.
    pub nav: NavDef,
    /// Bot names.
    pub bot_names: Vec<String>,
}

impl ArenaDef {
    /// Parse `data/arena.json`.
    pub fn load() -> Result<Self, String> {
        serde_json::from_str(ARENA_JSON).map_err(|e| format!("data/arena.json: {e}"))
    }

    /// The level a height belongs to.
    pub fn level_of(&self, y: f32) -> usize {
        if y < self.fy[1] - self.slab {
            0
        } else if y < self.fy[2] - self.slab {
            1
        } else {
            2
        }
    }
}

/// A point's sector and its sector-local `(u, v)` (Hexagon's `sect`).
pub fn sect(x: f32, z: f32) -> (usize, f32, f32) {
    let k = (z.atan2(x) / S60).round() as i32;
    let k = k.rem_euclid(6) as usize;
    let (ck, sk) = sector_axis(k);
    (k, x * ck + z * sk, -x * sk + z * ck)
}

/// cos and sin of sector `k`'s axis.
pub fn sector_axis(k: usize) -> (f32, f32) {
    let a = k as f32 * S60;
    (a.cos(), a.sin())
}

/// Sector-local `(u, v)` of sector `k` to level-local world `(x, z)`.
pub fn to_world(k: usize, u: f32, v: f32) -> (f32, f32) {
    let (ck, sk) = sector_axis(k);
    (u * ck - v * sk, u * sk + v * ck)
}

/// A floor tile template: `[u0, u1, v0, v1]`; `v = +-99` runs out to the slanted trim.
pub type Tile = [f32; 4];

/// One concrete piece: a cover structure or a floor tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// Level.
    pub level: usize,
    /// Sector.
    pub sector: usize,
    /// A floor tile (else a cover structure).
    pub floor: bool,
    /// Bounds in the sector frame: u0, u1, v0, v1, y0, y1.
    pub bounds: [f32; 6],
}

/// One cover cell (a 1 m concrete cube of a cover structure).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Level.
    pub level: usize,
    /// Sector.
    pub sector: usize,
    /// Grid `u`.
    pub iu: u16,
    /// Grid `v`.
    pub iv: u16,
    /// Grid `y`.
    pub iy: u16,
    /// The cover piece it belongs to.
    pub piece: u32,
}

/// The arena of one match: layout, holes, fallen cells and the ring angle.
#[derive(Debug, Clone)]
pub struct Arena {
    /// The geometry constants.
    pub def: ArenaDef,
    /// The seed the layout came from.
    pub seed: u64,
    /// Floor tile templates per level.
    pub tiles: [Vec<Tile>; 3],
    /// Floor tile row edges per level.
    pub rows: [Vec<f32>; 3],
    /// First tile index of each row per level (plus the end).
    row_index: [Vec<usize>; 3],
    /// Every cover cell.
    pub cells: Vec<Cell>,
    /// Cell index + 1 per grid slot (`(L * 6 + k) * PER + (iu * NV + iv) * NY + iy`); 0 = none.
    grid: Vec<u16>,
    /// Every concrete piece.
    pub pieces: Vec<Piece>,
    /// First floor-tile piece of each level and sector (`L * 6 + k`).
    pub floor_first: Vec<u32>,
    /// The holes in the concrete.
    pub holes: Holes,
    /// Fallen cover cells (true = gone: its support was blown away, or it was shot through).
    pub gone: Vec<bool>,
    /// The ring angle (radians) and its cos and sin.
    pub ang: f32,
    /// cos of the ring angle.
    pub ca: f32,
    /// sin of the ring angle.
    pub sa: f32,
}

impl Arena {
    /// The seeded arena. Sectors `k` and `k + 3` share a layout (180 degree symmetry).
    pub fn new(def: ArenaDef, seed: u64) -> Self {
        let (tiles, rows, row_index) = floor_tiles(&def);
        let mut a = Arena {
            def,
            seed,
            tiles,
            rows,
            row_index,
            cells: Vec::new(),
            grid: vec![0; 18 * PER],
            pieces: Vec::new(),
            floor_first: Vec::new(),
            holes: Holes::new(),
            gone: Vec::new(),
            ang: 0.0,
            ca: 1.0,
            sa: 0.0,
        };
        for level in 0..3 {
            for k in 0..3 {
                let cells = sector_cells(&a.def, seed, level, k);
                for kk in [k, k + 3] {
                    // Structure index -> piece index.
                    let mut ids: Vec<(u32, u32)> = Vec::new();
                    for &(iu, iv, iy, st) in &cells {
                        let p = match ids.iter().find(|(s, _)| *s == st) {
                            Some(&(_, p)) => p,
                            None => {
                                let p = a.pieces.len() as u32;
                                a.pieces.push(Piece {
                                    level,
                                    sector: kk,
                                    floor: false,
                                    bounds: [99.0, -99.0, 99.0, -99.0, 99.0, -99.0],
                                });
                                ids.push((st, p));
                                p
                            }
                        };
                        a.cells.push(Cell { level, sector: kk, iu, iv, iy, piece: p });
                        let gi = (level * 6 + kk) * PER + (usize::from(iu) * NV + usize::from(iv)) * NY + usize::from(iy);
                        if let Some(slot) = a.grid.get_mut(gi) {
                            *slot = a.cells.len() as u16;
                        }
                        let u = a.def.a_in + f32::from(iu);
                        let v = f32::from(iv) - (NV / 2) as f32;
                        let y = a.def.fy[level] + f32::from(iy);
                        if let Some(piece) = a.pieces.get_mut(p as usize) {
                            let b = &mut piece.bounds;
                            *b = [b[0].min(u), b[1].max(u + 1.0), b[2].min(v), b[3].max(v + 1.0), b[4].min(y), b[5].max(y + 1.0)];
                        }
                    }
                }
            }
        }
        for level in 0..3 {
            for k in 0..6 {
                a.floor_first.push(a.pieces.len() as u32);
                for t in &a.tiles[level] {
                    let e = t[1] * TAN30 - a.def.floor_slant;
                    a.pieces.push(Piece {
                        level,
                        sector: k,
                        floor: true,
                        bounds: [t[0], t[1], t[2].max(-e), t[3].min(e), a.def.fy[level] - a.def.slab, a.def.fy[level]],
                    });
                }
            }
        }
        a.gone = vec![false; a.cells.len()];
        a
    }

    /// Clear all damage (a new round).
    pub fn reset_damage(&mut self) {
        self.holes.clear();
        self.gone.iter_mut().for_each(|g| *g = false);
    }

    /// Cell centre in its level-local frame.
    pub fn cell_centre(&self, c: &Cell) -> [f32; 3] {
        let u = self.def.a_in + f32::from(c.iu) + 0.5;
        let v = f32::from(c.iv) - (NV / 2) as f32 + 0.5;
        let (x, z) = to_world(c.sector, u, v);
        [x, self.def.fy[c.level] + f32::from(c.iy) + 0.5, z]
    }

    /// Set the ring angle.
    pub fn set_ring(&mut self, a: f32) {
        self.ang = a;
        self.ca = a.cos();
        self.sa = a.sin();
    }

    /// A world point in level `level`'s own frame (ring-local on the middle level).
    pub fn to_level(&self, level: usize, x: f32, z: f32) -> (f32, f32) {
        if level == 1 {
            (x * self.ca + z * self.sa, -x * self.sa + z * self.ca)
        } else {
            (x, z)
        }
    }

    /// A level-frame point back to the world.
    pub fn from_level(&self, level: usize, x: f32, z: f32) -> (f32, f32) {
        if level == 1 {
            (x * self.ca - z * self.sa, x * self.sa + z * self.ca)
        } else {
            (x, z)
        }
    }

    /// The floor tile (template index) under sector-local `(u, v)` on `level`, or `None` for metal trim.
    pub fn floor_tile(&self, level: usize, u: f32, v: f32) -> Option<usize> {
        let rows = &self.rows[level];
        let (first, last) = (*rows.first()?, *rows.last()?);
        if u < first || u >= last || v.abs() > u * TAN30 - self.def.floor_slant {
            return None;
        }
        let mut r = 0;
        while r + 1 < rows.len() && u >= rows[r + 1] {
            r += 1;
        }
        let ri = &self.row_index[level];
        let (i0, i1) = (*ri.get(r)?, *ri.get(r + 1)?);
        (i0..i1).find(|&i| v >= self.tiles[level][i][2] && v < self.tiles[level][i][3])
    }

    /// The cover cell at a grid slot.
    pub fn cell_at(&self, level: usize, k: usize, iu: usize, iv: usize, iy: usize) -> Option<usize> {
        if iu >= NU || iv >= NV || iy >= NY {
            return None;
        }
        let g = *self.grid.get((level * 6 + k) * PER + (iu * NV + iv) * NY + iy)?;
        (g > 0).then(|| usize::from(g) - 1)
    }

    /// What is at a world point (the one solidity test: collision, line of
    /// sight, hit tests and bots all ask it).
    pub fn probe(&self, x: f32, y: f32, z: f32) -> Solid {
        if y < self.def.fy[0] - self.def.slab {
            return Solid::Empty;
        }
        let level = self.def.level_of(y);
        let (mx, mz) = self.to_level(level, x, z);
        self.probe_level(level, mx, y, mz)
    }

    /// [`Arena::probe`] in a level's own frame; `y` must lie in that level's height band.
    pub fn probe_level(&self, level: usize, mx: f32, y: f32, mz: f32) -> Solid {
        let d = &self.def;
        let ly = y - d.fy[level];
        if level == 2 && ly >= d.top_wall {
            return Solid::Empty;
        }
        let (k, u, v) = sect(mx, mz);
        let av = v.abs();
        if u < d.a_in {
            return Solid::Empty;
        }
        if u > d.a_out {
            return Solid::Metal;
        }
        if ly < 0.0 {
            if level == 1 && (u - d.lift_u).abs() < d.lift_h && (v - d.lift_v).abs() < d.lift_h {
                return Solid::Empty;
            }
            if level == 2 && (u - d.lift_u).abs() < d.lift_h && (v + d.lift_v).abs() < d.lift_h {
                return Solid::Empty;
            }
            let Some(t) = self.floor_tile(level, u, v) else {
                return Solid::Metal;
            };
            if self.holes.holed(level, mx, y, mz) {
                return Solid::Empty;
            }
            let first = self.floor_first.get(level * 6 + k).copied().unwrap_or(0);
            return Solid::Concrete(first + t as u32);
        }
        let dd = av * COS30 - u * SIN30;
        if dd > -d.divh {
            let di = if v >= 0.0 { k } else { (k + 5) % 6 };
            let s = u * COS30 + av * SIN30;
            if !(d.divs[level][di] == 1 && s > d.door0 && s < d.door1 && ly < d.door_h) {
                return Solid::Metal;
            }
        }
        if level != 1 && u < d.a_in + d.rail_t && ly < d.rail_h {
            return Solid::Metal;
        }
        let iu = (u - d.a_in).floor();
        let iv = (v + (NV / 2) as f32).floor();
        let iy = ly.floor();
        if iu >= 0.0 && iv >= 0.0 && iy >= 0.0
            && let Some(c) = self.cell_at(level, k, iu as usize, iv as usize, iy as usize) {
                if self.gone[c] || self.holes.holed(level, mx, y, mz) {
                    return Solid::Empty;
                }
                return Solid::Concrete(self.cells[c].piece);
            }
        Solid::Empty
    }

    /// Lift column at a feet position (static frame): 0 none, else the level it rises to (1 or 2).
    pub fn lift_at(&self, x: f32, y: f32, z: f32) -> usize {
        let d = &self.def;
        if y < d.fy[0] - 0.1 || y > d.fy[2] + 1.6 {
            return 0;
        }
        let (_, u, v) = sect(x, z);
        if (u - d.lift_u).abs() > d.lift_h - 0.05 {
            return 0;
        }
        if (v - d.lift_v).abs() < d.lift_h - 0.05 && y < d.fy[1] + 1.6 {
            return 1;
        }
        if (v + d.lift_v).abs() < d.lift_h - 0.05 && y >= d.fy[1] - 0.6 {
            return 2;
        }
        0
    }

    /// Does a body (feet at `x, y, z`) touch anything, sampled with ring
    /// radius `scale` x the body radius and heights `hs`.
    fn overlaps(&self, x: f32, y: f32, z: f32, scale: f32, hs: &[f32; 5]) -> bool {
        let r = self.def.radius * scale;
        for &h in hs {
            let yy = y + h;
            if self.probe(x, yy, z).is_solid() {
                return true;
            }
            // On the middle level the samples turn with the ring, so a body
            // the ring carries keeps the same samples against the ring's
            // (holed) concrete and never gets one pushed into it by the turn.
            let (c, sn) = if self.def.level_of(yy) == 1 { (self.ca, self.sa) } else { (1.0, 0.0) };
            for i in 0..8 {
                let a = i as f32 * core::f32::consts::FRAC_PI_4;
                let (px, pz) = (a.cos() * r, a.sin() * r);
                if self.probe(x + px * c - pz * sn, yy, z + px * sn + pz * c).is_solid() {
                    return true;
                }
            }
        }
        false
    }

    /// True when a standing body (feet at `x, y, z`) touches anything solid (the movement test).
    pub fn blocked(&self, x: f32, y: f32, z: f32) -> bool {
        let h = self.def.height;
        self.overlaps(x, y, z, 1.0, &[0.02, 0.45, 0.9, 1.35, h - 0.02])
    }

    /// True when a body is really inside something (a slimmer test, immune to contact rounding).
    pub fn inside(&self, x: f32, y: f32, z: f32) -> bool {
        // Between the 8 samples a cell corner can poke about 0.2 m into the
        // body unseen, so the "really inside" test uses half the radius.
        let h = self.def.height;
        self.overlaps(x, y, z, 0.5, &[0.08, 0.5, 0.9, 1.3, h - 0.08]) && self.blocked(x, y, z)
    }

    /// Move `p` by `d` as far as it goes; false when it was stopped.
    pub fn try_move(&self, p: &mut [f32; 3], d: [f32; 3]) -> bool {
        if !self.blocked(p[0] + d[0], p[1] + d[1], p[2] + d[2]) {
            p[0] += d[0];
            p[1] += d[1];
            p[2] += d[2];
            return true;
        }
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..5 {
            let m = (lo + hi) / 2.0;
            if self.blocked(p[0] + d[0] * m, p[1] + d[1] * m, p[2] + d[2] * m) {
                hi = m;
            } else {
                lo = m;
            }
        }
        // Stop a millimetre short of the contact, so later rounding (a ring
        // carry) never puts the body inside.
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        lo = (lo - 0.001 / len.max(1e-6)).max(0.0);
        p[0] += d[0] * lo;
        p[1] += d[1] * lo;
        p[2] += d[2] * lo;
        false
    }

    /// Ring-local sector `u` of a world point (is it over the ring floor?).
    fn ring_u(&self, x: f32, z: f32) -> f32 {
        let (mx, mz) = self.to_level(1, x, z);
        sect(mx, mz).1
    }

    /// Carry something standing on (or flying inside) the middle ring by the
    /// ring's turn `d`. Call with the ring angle still at its previous value.
    /// True when it was carried (turn its yaw too).
    pub fn carry(&self, p: &mut [f32; 3], vel: &mut [f32; 3], d: f32) -> bool {
        let def = &self.def;
        if d == 0.0 || p[1] < def.fy[1] - 0.3 || p[1] >= def.fy[2] - def.slab {
            return false;
        }
        if self.ring_u(p[0], p[2]) < def.a_in - 0.5 {
            return false;
        }
        // Rising through the static top lift hole, or poking up into a
        // crater in the static floor above: the body belongs to that
        // opening, not to the car below.
        if p[1] > def.fy[2] - def.slab - def.height
            && (self.lift_at(p[0], p[1], p[2]) == 2 || !self.probe(p[0], def.fy[2] - def.slab + 0.25, p[2]).is_solid())
        {
            return false;
        }
        let (c, s) = (d.cos(), d.sin());
        let (x, z) = (p[0] * c - p[2] * s, p[0] * s + p[2] * c);
        let (vx, vz) = (vel[0] * c - vel[2] * s, vel[0] * s + vel[2] * c);
        p[0] = x;
        p[2] = z;
        vel[0] = vx;
        vel[2] = vz;
        true
    }

    /// Spawn point `n` of `team`: feet position and yaw (facing into the arena).
    pub fn spawn_point(&self, team: usize, n: usize) -> ([f32; 3], f32) {
        let d = &self.def;
        let k = d.base[team & 1];
        let s = &d.spawn;
        let u = if n % 2 == 1 { s.u_odd } else { s.u_even };
        let v = s.v0 + (n % s.slots.max(1) as usize) as f32 * s.dv;
        let (x, z) = to_world(k, u, v);
        ([x, d.fy[2] + 0.05, z], k as f32 * S60 + core::f32::consts::PI)
    }

    /// Is a feet position inside `team`'s own core zone (top level, its base sector)?
    pub fn in_core(&self, team: usize, p: [f32; 3]) -> bool {
        let d = &self.def;
        if p[1] < d.fy[2] - 0.3 || p[1] > d.fy[2] + 3.0 {
            return false;
        }
        let (k, u, v) = sect(p[0], p[2]);
        k == d.base[team & 1] && (u - d.core_u).hypot(v) < d.core_r
    }

    /// World centre of `team`'s core zone (on the floor).
    pub fn core_centre(&self, team: usize) -> [f32; 3] {
        let (x, z) = to_world(self.def.base[team & 1], self.def.core_u, 0.0);
        [x, self.def.fy[2], z]
    }
}

/// The floor tile templates, their row edges and row starts per level.
#[allow(clippy::type_complexity)]
fn floor_tiles(d: &ArenaDef) -> ([Vec<Tile>; 3], [Vec<f32>; 3], [Vec<usize>; 3]) {
    let lift_f = d.lift_h + d.lift_frame;
    let f_out = d.a_out - d.floor_out_inset;
    let mut tiles: [Vec<Tile>; 3] = Default::default();
    let mut rows: [Vec<f32>; 3] = Default::default();
    let mut ri: [Vec<usize>; 3] = Default::default();
    for level in 0..3 {
        let r = vec![
            d.a_in + d.trim_in[level],
            d.floor_rows[0],
            d.floor_rows[1],
            d.floor_rows[2],
            d.lift_u - lift_f,
            d.lift_u + lift_f,
            f_out,
        ];
        let (t, idx) = (&mut tiles[level], &mut ri[level]);
        for row in 0..r.len() - 1 {
            idx.push(t.len());
            let wa = r[row] * TAN30 - d.floor_slant;
            let (a, b) = (d.lift_v - lift_f, d.lift_v + lift_f);
            // The lift row leaves a gap round both lift shafts.
            let segs: Vec<[f32; 2]> = if row == 4 { vec![[-wa, -b], [-a, a], [b, wa]] } else { vec![[-wa, wa]] };
            for (s, seg) in segs.iter().enumerate() {
                let (s0, s1) = (seg[0], seg[1]);
                let n = ((s1 - s0) / d.tile_size).round().max(1.0) as usize;
                for i in 0..n {
                    let v0 = if s == 0 && i == 0 { -99.0 } else { s0 + (s1 - s0) * i as f32 / n as f32 };
                    let v1 = if s == segs.len() - 1 && i == n - 1 { 99.0 } else { s0 + (s1 - s0) * (i + 1) as f32 / n as f32 };
                    t.push([r[row], r[row + 1], v0, v1]);
                }
            }
        }
        idx.push(t.len());
        rows[level] = r;
    }
    (tiles, rows, ri)
}

/// Cells where no cover may stand (doorways, lift shafts, dividers, the core zone).
fn not_valid(d: &ArenaDef, level: usize, k: usize, iu: i32, iv: i32) -> bool {
    let u0 = d.a_in + iu as f32;
    let v0 = iv as f32 - (NV / 2) as f32;
    let (uc, vc) = (u0 + 0.5, v0 + 0.5);
    let avc = vc.abs();
    if u0 < d.a_in + 2.0 || u0 + 1.0 > d.a_out - 1.5 {
        return true;
    }
    let vmax = v0.abs().max((v0 + 1.0).abs());
    let dd = vmax * COS30 - u0 * SIN30;
    if dd > -1.6 {
        return true;
    }
    if (uc - d.lift_u).abs() < d.lift_h + 1.6 && (avc - d.lift_v).abs() < d.lift_h + 1.6 {
        return true;
    }
    if level == 0 {
        let s = uc * COS30 + avc * SIN30;
        if dd > -3.2 && s > d.door0 - 1.5 && s < d.door1 + 1.5 {
            return true;
        }
    }
    if level == 2 && k.is_multiple_of(3) {
        if uc > 28.5 && avc < 9.5 {
            return true;
        }
        if (uc - d.core_u).hypot(vc) < d.core_r + 1.8 {
            return true;
        }
    }
    false
}

/// The cover cells of one sector template: `(iu, iv, iy, structure)`,
/// deduplicated (a cell keeps its first structure). Hexagon's `sectorCells`,
/// on a seeded Pcg32 stream per level and sector.
fn sector_cells(d: &ArenaDef, seed: u64, level: usize, k: usize) -> Vec<(u16, u16, u16, u32)> {
    let mut r = Pcg32::new(seed, (level * 3 + k + 1) as u64);
    let mut out: Vec<(u16, u16, u16, u32)> = Vec::new();
    let mut seen = vec![false; PER];
    let max_h = d.cover.max_height[level] as i32;
    let n = if level == 2 && k == 0 { d.cover.structures_top_base } else { d.cover.structures };
    let mut put = |out: &mut Vec<(u16, u16, u16, u32)>, st: u32, iu: i32, iv: i32, iy: i32| {
        if iu < 0 || iu >= NU as i32 || iv < 0 || iv >= NV as i32 || iy < 0 || iy >= max_h {
            return;
        }
        if not_valid(d, level, k, iu, iv) {
            return;
        }
        let key = (iu as usize * NV + iv as usize) * NY + iy as usize;
        if seen[key] {
            return;
        }
        seen[key] = true;
        out.push((iu as u16, iv as u16, iy as u16, st));
    };
    for st in 0..n {
        let u = d.a_in + 3.0 + r.next_f32() * (d.a_out - d.a_in - 7.0);
        let half = u * TAN30 - 3.0;
        let v = (r.next_f32() * 2.0 - 1.0) * half;
        let iu = (u - d.a_in).floor() as i32;
        let iv = (v + (NV / 2) as f32).floor() as i32;
        let t = r.next_f32();
        if t < 0.4 {
            // A low wall, along or across the sector.
            let len = 3 + (r.next_f32() * 4.0) as i32;
            let h = if r.next_f32() < 0.35 { 2 } else { 1 };
            let along = r.next_f32() < 0.5;
            for i in 0..len {
                for y in 0..h {
                    if along {
                        put(&mut out, st, iu + i, iv, y);
                    } else {
                        put(&mut out, st, iu, iv + i, y);
                    }
                }
            }
        } else if t < 0.6 {
            // A pillar.
            let sz = if r.next_f32() < 0.5 { 1 } else { 2 };
            for a in 0..sz {
                for b in 0..sz {
                    for y in 0..3 {
                        put(&mut out, st, iu + a, iv + b, y);
                    }
                }
            }
        } else if t < 0.8 {
            // A small bunker with one open side.
            let open = (r.next_f32() * 4.0) as i32;
            for a in 0..3 {
                for b in 0..3 {
                    if a == 1 && b == 1 {
                        continue;
                    }
                    let gap = (open == 0 && a == 0 && b == 1)
                        || (open == 1 && a == 2 && b == 1)
                        || (open == 2 && b == 0 && a == 1)
                        || (open == 3 && b == 2 && a == 1);
                    if gap {
                        continue;
                    }
                    for y in 0..2 {
                        put(&mut out, st, iu + a, iv + b, y);
                    }
                }
            }
        } else {
            // A step block.
            for a in 0..2 {
                for b in 0..2 {
                    put(&mut out, st, iu + a, iv + b, 0);
                }
            }
            put(&mut out, st, iu, iv, 1);
        }
    }
    out
}
