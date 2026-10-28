//! The relief patterns, written once on the CPU: brick, cobble, tiles and
//! wood planks. Each returns, at a point of its tile (metres), a height
//! (0 = deepest joint, 1 = the face), a linear colour and a roughness.
//!
//! * Periodic: every pattern repeats exactly over [`Pattern::tile`], so the
//!   baked tile wraps without a seam (the hash lattice wraps too).
//! * Bit-exact on every platform: only `+ - * /`, `floor` and `sqrt` (all
//!   correctly rounded in IEEE 754) and integer hashes; no `sin`, `pow` or
//!   `exp`, whose results differ between maths libraries.
//! * Per-cell variation (each brick, stone, tile and plank has its own
//!   tint, height and wear from a hash of its cell).
//!
//! Ideas: per-brick hash variation and weighting the joint profile come from
//! the Godot procedural brick shader by arlez80 (MIT,
//! <https://godotshaders.com/shader/procedural-brick-shader/>); the stones
//! are a jittered-grid Voronoi diagram with the distance to the cell border
//! measured to the bisector of the two nearest points. Written from
//! scratch; no code is copied.

/// One sample of a pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// 0 = the bottom of a joint, 1 = the top of the face.
    pub height: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    /// 0 = mirror, 1 = matte.
    pub rough: f32,
}

/// Bricks in running bond. Sizes in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brick {
    pub length: f32,
    pub height: f32,
    /// The mortar joint's width.
    pub mortar: f32,
    /// How far in from the arris the face is rounded.
    pub bevel: f32,
    /// Per-brick tint and height variation (0 = every brick alike).
    pub variation: f32,
}

/// Cobbles: rounded stones in a Voronoi pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cobble {
    /// The mean stone size.
    pub size: f32,
    /// The joint between stones.
    pub gap: f32,
    pub variation: f32,
}

/// Square glazed tiles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tiles {
    pub size: f32,
    pub grout: f32,
    /// 0 = matte, 1 = mirror glaze.
    pub gloss: f32,
    pub variation: f32,
}

/// Wood planks, staggered, with growth rings and grain along the plank (u).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Planks {
    pub width: f32,
    pub length: f32,
    /// Growth rings per metre across the board.
    pub rings: f32,
    pub variation: f32,
}

impl Default for Brick {
    fn default() -> Self {
        Brick {
            length: 0.215,
            height: 0.065,
            mortar: 0.01,
            bevel: 0.006,
            variation: 1.0,
        }
    }
}

impl Default for Cobble {
    fn default() -> Self {
        Cobble {
            size: 0.14,
            gap: 0.014,
            variation: 1.0,
        }
    }
}

impl Default for Tiles {
    fn default() -> Self {
        Tiles {
            size: 0.3,
            grout: 0.009,
            gloss: 0.8,
            variation: 1.0,
        }
    }
}

impl Default for Planks {
    fn default() -> Self {
        Planks {
            width: 0.14,
            length: 1.2,
            rings: 70.0,
            variation: 1.0,
        }
    }
}

/// Bricks along u and courses up v in one tile.
pub const BRICK_COLS: i32 = 4;
pub const BRICK_ROWS: i32 = 8;
/// Stones per tile side.
pub const COBBLE_CELLS: i32 = 8;
/// Tiles per tile side.
pub const TILE_CELLS: i32 = 4;
/// Planks across one tile (v), and plank lengths along it (u).
pub const PLANK_ROWS: i32 = 6;
pub const PLANK_COLS: i32 = 2;

/// A pattern with its parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pattern {
    Brick(Brick),
    Cobble(Cobble),
    Tiles(Tiles),
    Planks(Planks),
}

/// The authored base colours (linear); the colour params multiply by
/// colour / these, so the bake never depends on a colour param.
pub const BRICK_COLOR: [f32; 3] = [0.36, 0.13, 0.07];
pub const BRICK_MORTAR: [f32; 3] = [0.42, 0.40, 0.35];
pub const COBBLE_COLOR: [f32; 3] = [0.30, 0.29, 0.27];
pub const COBBLE_SOIL: [f32; 3] = [0.10, 0.085, 0.065];
pub const TILES_COLOR: [f32; 3] = [0.16, 0.36, 0.38];
pub const TILES_GROUT: [f32; 3] = [0.40, 0.39, 0.36];
pub const WOOD_COLOR: [f32; 3] = [0.32, 0.17, 0.075];
pub const WOOD_GAP: [f32; 3] = [0.03, 0.02, 0.012];

impl Pattern {
    /// The tile the pattern repeats over (u, v), in metres.
    pub fn tile(&self) -> [f32; 2] {
        match self {
            Pattern::Brick(b) => [
                BRICK_COLS as f32 * (b.length + b.mortar),
                BRICK_ROWS as f32 * (b.height + b.mortar),
            ],
            Pattern::Cobble(c) => [COBBLE_CELLS as f32 * c.size; 2],
            Pattern::Tiles(t) => [TILE_CELLS as f32 * (t.size + t.grout); 2],
            Pattern::Planks(p) => [PLANK_COLS as f32 * p.length, PLANK_ROWS as f32 * p.width],
        }
    }

    /// The relief depth (m) from height 0 to height 1.
    pub fn depth(&self) -> f32 {
        match self {
            Pattern::Brick(b) => (b.mortar * 1.1).clamp(0.004, 0.03),
            Pattern::Cobble(c) => (c.size * 0.22).clamp(0.01, 0.06),
            Pattern::Tiles(t) => (t.grout * 0.8).clamp(0.001, 0.01),
            Pattern::Planks(_) => 0.004,
        }
    }

    /// The authored colour the colour param is relative to.
    pub fn base_color(&self) -> [f32; 3] {
        match self {
            Pattern::Brick(_) => BRICK_COLOR,
            Pattern::Cobble(_) => COBBLE_COLOR,
            Pattern::Tiles(_) => TILES_COLOR,
            Pattern::Planks(_) => WOOD_COLOR,
        }
    }

    /// The pattern at `(u, v)` (metres; any value, it wraps).
    pub fn sample(&self, u: f32, v: f32) -> Sample {
        let t = self.tile();
        self.sample_unwrapped(wrap(u, t[0]), wrap(v, t[1]))
    }

    /// The pattern without wrapping `(u, v)` first: periodic by its own
    /// construction (the seam test calls it a tile apart).
    pub fn sample_unwrapped(&self, u: f32, v: f32) -> Sample {
        match self {
            Pattern::Brick(b) => brick(b, u, v),
            Pattern::Cobble(c) => cobble(c, u, v),
            Pattern::Tiles(x) => tiles(x, u, v),
            Pattern::Planks(p) => planks(p, u, v),
        }
    }
}

// ---- deterministic maths ------------------------------------------------------

/// `x` into `[0, period)`.
pub fn wrap(x: f32, period: f32) -> f32 {
    let r = x - (x / period).floor() * period;
    if r >= period || r < 0.0 { 0.0 } else { r }
}

/// PCG-style integer hash (the same mix as the shader's `mp_pcg`).
pub fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// A hash of a lattice point and a seed, in `[0, 1)` (24 bits, exact).
pub fn h2(x: i32, y: i32, seed: u32) -> f32 {
    let h = pcg((x as u32) ^ pcg((y as u32).wrapping_add(seed.wrapping_mul(1013))));
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// `i` wrapped into `0..n`.
fn wi(i: i32, n: i32) -> i32 {
    i.rem_euclid(n.max(1))
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

fn scale3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Value noise on a lattice that wraps every `(px, py)` cells; `x`, `y` in
/// cells.
pub fn pnoise(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (tx, ty) = (smooth(x - fx), smooth(y - fy));
    let h = |dx: i32, dy: i32| h2(wi(ix + dx, px), wi(iy + dy, py), seed);
    lerp(lerp(h(0, 0), h(1, 0), tx), lerp(h(0, 1), h(1, 1), tx), ty)
}

/// Periodic fractal noise: `oct` octaves of [`pnoise`], each twice the
/// frequency (and period) of the last; about 0..1.
pub fn pfbm(x: f32, y: f32, px: i32, py: i32, seed: u32, oct: u32) -> f32 {
    let (mut t, mut a, mut norm, mut f) = (0.0, 0.5, 0.0, 1i32);
    for k in 0..oct {
        t += a * pnoise(x * f as f32, y * f as f32, px * f, py * f, seed.wrapping_add(k * 7));
        norm += a;
        a *= 0.5;
        f *= 2;
    }
    t / norm
}

/// Lattice cells across a tile side of `len` metres at `per_m` cells a
/// metre (at least 1), so a noise of that frequency wraps with the tile.
fn cells(len: f32, per_m: f32) -> i32 {
    ((len * per_m) + 0.5).floor().max(1.0) as i32
}

/// Periodic fbm over a tile `t` at about `per_m` lattice cells a metre.
fn tile_fbm(u: f32, v: f32, t: [f32; 2], per_m: f32, seed: u32, oct: u32) -> f32 {
    let (px, py) = (cells(t[0], per_m), cells(t[1], per_m));
    pfbm(u / t[0] * px as f32, v / t[1] * py as f32, px, py, seed, oct)
}

// ---- brick ----------------------------------------------------------------------

fn brick(b: &Brick, u: f32, v: f32) -> Sample {
    let t = [
        BRICK_COLS as f32 * (b.length + b.mortar),
        BRICK_ROWS as f32 * (b.height + b.mortar),
    ];
    let (ml, mh) = (b.length + b.mortar, b.height + b.mortar);
    let row = (v / mh).floor();
    let odd = (row as i32) & 1 == 1;
    let x = u + if odd { ml * 0.5 } else { 0.0 };
    let col = (x / ml).floor();
    let (fx, fy) = (x - col * ml, v - row * mh);
    let id = (wi(col as i32, BRICK_COLS), wi(row as i32, BRICK_ROWS));
    let half = b.mortar * 0.5;
    // Signed distance into the brick from its nearest edge (< 0 in mortar).
    let dx = (fx - half).min(ml - half - fx);
    let dy = (fy - half).min(mh - half - fy);
    let d = dx.min(dy);
    let var = b.variation;
    // Worn, chipped arrises: noise eats into the bevel near the edges.
    let chip = tile_fbm(u, v, t, 90.0, 11, 3);
    let d_worn = d - b.bevel * 0.9 * smooth((chip - 0.45) * 3.0);
    let hb = h2(id.0, id.1, 3);
    let face = 0.9 + 0.07 * var * (hb - 0.5) + 0.05 * tile_fbm(u, v, t, 40.0, 12, 2);
    let mortar_h = 0.12 + 0.12 * tile_fbm(u, v, t, 160.0, 13, 2);
    let profile = if d_worn <= 0.0 {
        0.0
    } else {
        let q = (d_worn / b.bevel.max(1e-4)).min(1.0);
        // Rounded arris: a quarter circle out of the joint.
        (q * (2.0 - q)).sqrt()
    };
    let height = lerp(mortar_h, face, profile);
    // Colour: each brick its own tint (redder or browner, lighter or
    // darker), fine speckle, darker fired spots; the mortar sandy.
    let h_l = h2(id.0, id.1, 5);
    let h_c = h2(id.0, id.1, 6);
    let lum = 1.0 + var * 0.42 * (h_l - 0.5);
    let warm = var * 0.16 * (h_c - 0.5);
    let speck = tile_fbm(u, v, t, 220.0, 14, 2);
    let blotch = tile_fbm(u, v, t, 18.0, 15, 3);
    let mut c = [
        BRICK_COLOR[0] * (1.0 + warm),
        BRICK_COLOR[1] * (1.0 - warm * 0.6),
        BRICK_COLOR[2] * (1.0 - warm),
    ];
    c = scale3(c, lum * (0.82 + 0.3 * blotch) * (0.9 + 0.2 * speck));
    let mortar = scale3(BRICK_MORTAR, 0.85 + 0.3 * tile_fbm(u, v, t, 120.0, 16, 2));
    let color = mix3(mortar, c, smooth(profile * 2.0));
    let rough = lerp(0.97, 0.86 + 0.08 * speck, profile);
    Sample {
        height,
        color,
        rough,
    }
}

// ---- cobble ---------------------------------------------------------------------

/// The jittered point of stone cell `(i, j)` (cells; wraps every `n`).
fn cobble_point(i: i32, j: i32, n: i32) -> [f32; 2] {
    let (a, b) = (wi(i, n), wi(j, n));
    [
        i as f32 + 0.15 + 0.7 * h2(a, b, 21),
        j as f32 + 0.15 + 0.7 * h2(a, b, 22),
    ]
}

fn cobble(c: &Cobble, u: f32, v: f32) -> Sample {
    let n = COBBLE_CELLS;
    let t = [n as f32 * c.size; 2];
    let p = [u / c.size, v / c.size];
    let (ci, cj) = (p[0].floor() as i32, p[1].floor() as i32);
    // Nearest point.
    let (mut best, mut bd, mut bc) = ([0.0f32; 2], f32::MAX, (0, 0));
    for dj in -1..=1 {
        for di in -1..=1 {
            let q = cobble_point(ci + di, cj + dj, n);
            let d = (q[0] - p[0]) * (q[0] - p[0]) + (q[1] - p[1]) * (q[1] - p[1]);
            if d < bd {
                bd = d;
                best = q;
                bc = (ci + di, cj + dj);
            }
        }
    }
    // Distance (cells) to the nearest cell border: the bisector of the
    // nearest point and each other one.
    let mut edge = f32::MAX;
    for dj in -2..=2 {
        for di in -2..=2 {
            let (i, j) = (bc.0 + di, bc.1 + dj);
            if (i, j) == bc {
                continue;
            }
            let q = cobble_point(i, j, n);
            let (ex, ey) = (q[0] - best[0], q[1] - best[1]);
            let len = (ex * ex + ey * ey).sqrt();
            if len < 1e-6 {
                continue;
            }
            let (mx, my) = ((q[0] + best[0]) * 0.5, (q[1] + best[1]) * 0.5);
            let dd = ((mx - p[0]) * ex + (my - p[1]) * ey) / len;
            edge = edge.min(dd);
        }
    }
    let edge_m = edge * c.size;
    let id = (wi(bc.0, n), wi(bc.1, n));
    let var = c.variation;
    // A domed stone out of a sandy joint, rough at its rim.
    let rim = tile_fbm(u, v, t, 60.0, 23, 3);
    let e = edge_m - c.gap * 0.5 - c.gap * 0.6 * (rim - 0.5);
    let soil_h = 0.06 * tile_fbm(u, v, t, 120.0, 24, 2);
    let (height, dome) = if e <= 0.0 {
        (soil_h, 0.0)
    } else {
        let q = (e / (c.size * 0.32)).min(1.0);
        let dome = (q * (2.0 - q)).sqrt();
        let top = 0.82 + 0.18 * (1.0 - var * 0.6 * h2(id.0, id.1, 25));
        (lerp(soil_h, top, dome) + 0.04 * tile_fbm(u, v, t, 80.0, 26, 2) * dome, dome)
    };
    // Each stone its own grey, brown or blue-grey; worn tops a little paler.
    let hs = h2(id.0, id.1, 27);
    let hl = h2(id.0, id.1, 28);
    let tints = [[1.0, 1.0, 1.0], [1.12, 1.0, 0.86], [0.9, 0.96, 1.08]];
    let tint = tints[((hs * 3.0) as usize).min(2)];
    let tint = mix3([1.0; 3], tint, var.min(1.0));
    let lum = (1.0 + var * 0.45 * (hl - 0.5)) * (0.85 + 0.3 * tile_fbm(u, v, t, 30.0, 29, 3));
    let stone = [
        COBBLE_COLOR[0] * tint[0] * lum,
        COBBLE_COLOR[1] * tint[1] * lum,
        COBBLE_COLOR[2] * tint[2] * lum,
    ];
    let stone = scale3(stone, 1.0 + 0.12 * dome);
    let soil = scale3(COBBLE_SOIL, 0.8 + 0.4 * tile_fbm(u, v, t, 90.0, 30, 2));
    let color = mix3(soil, stone, smooth(dome * 3.0));
    let rough = if dome > 0.0 {
        lerp(0.85, 0.62 + 0.1 * hl, dome)
    } else {
        1.0
    };
    Sample {
        height: height.clamp(0.0, 1.0),
        color,
        rough,
    }
}

// ---- tiles ----------------------------------------------------------------------

fn tiles(x: &Tiles, u: f32, v: f32) -> Sample {
    let m = x.size + x.grout;
    let t = [TILE_CELLS as f32 * m; 2];
    let (cx, cy) = ((u / m).floor(), (v / m).floor());
    let (fx, fy) = (u - cx * m, v - cy * m);
    let id = (wi(cx as i32, TILE_CELLS), wi(cy as i32, TILE_CELLS));
    let half = x.grout * 0.5;
    let d = (fx - half).min(m - half - fx).min((fy - half).min(m - half - fy));
    let var = x.variation;
    // Each tile set a touch out of level: a small tilt, so the glaze
    // catches the light differently tile to tile.
    let (tx, ty) = (h2(id.0, id.1, 41) - 0.5, h2(id.0, id.1, 42) - 0.5);
    let tilt = var * 0.25 * (tx * (fx - m * 0.5) + ty * (fy - m * 0.5)) / m;
    let bevel = x.grout.max(0.006);
    let profile = if d <= 0.0 {
        0.0
    } else {
        let q = (d / bevel).min(1.0);
        (q * (2.0 - q)).sqrt()
    };
    let grout_h = 0.25 + 0.1 * tile_fbm(u, v, t, 150.0, 43, 2);
    let height = lerp(grout_h, 0.85 + tilt, profile);
    let hl = h2(id.0, id.1, 44);
    let glaze = tile_fbm(u, v, t, 14.0, 45, 3);
    let lum = (1.0 + var * 0.18 * (hl - 0.5)) * (0.9 + 0.2 * glaze);
    // The glaze pools darker towards the rim of each tile.
    let rim = 1.0 - 0.12 * (1.0 - smooth(d / (x.size * 0.12)));
    let tile_c = scale3(TILES_COLOR, lum * rim);
    let grout = scale3(TILES_GROUT, 0.85 + 0.3 * tile_fbm(u, v, t, 110.0, 46, 2));
    let color = mix3(grout, tile_c, smooth(profile * 2.0));
    // Each tile's glaze a little different, so the sheen varies tile to tile.
    let glaze_r = (1.0 - x.gloss).clamp(0.02, 1.0) + 0.06 * glaze + 0.12 * var * (hl - 0.5);
    let rough = lerp(0.95, glaze_r.min(1.0), profile);
    Sample {
        height: height.clamp(0.0, 1.0),
        color,
        rough,
    }
}

// ---- planks ---------------------------------------------------------------------

fn planks(p: &Planks, u: f32, v: f32) -> Sample {
    let t = [PLANK_COLS as f32 * p.length, PLANK_ROWS as f32 * p.width];
    let row = (v / p.width).floor();
    let r = wi(row as i32, PLANK_ROWS);
    // Each row's butt joints are shifted by its own amount.
    let off = h2(r, 0, 51) * p.length;
    let x = wrap(u - off, t[0]);
    let col = (x / p.length).floor();
    let k = wi(col as i32, PLANK_COLS);
    let (fx, fy) = (x - col * p.length, v - row * p.width);
    let gap = 0.003;
    let half = gap * 0.5;
    let d_side = (fy - half).min(p.width - half - fy);
    let d_end = (fx - half).min(p.length - half - fx);
    let d = d_side.min(d_end);
    let var = p.variation;
    let hp = h2(r, k, 52);
    // Growth rings: the distance from a pith line below the board, which
    // wanders along the plank, gives the arched bands of flat-sawn boards.
    let wander = tile_fbm(u, v, t, 1.2, 53 + k as u32, 3) - 0.5;
    let pith_v = p.width * (0.5 + 0.9 * (h2(r, k, 54) - 0.5)) + 0.05 * wander;
    let pith_d = 0.03 + 0.06 * h2(r, k, 55) + 0.02 * wander;
    let dv = fy - pith_v;
    let rad = (dv * dv + pith_d * pith_d).sqrt();
    let grain_n = tile_fbm(u, v, t, 6.0, 56, 3);
    let phase = rad * p.rings + 0.8 * grain_n;
    let ring = phase - phase.floor();
    // Latewood: a narrow dark band in each ring.
    let late = smooth((ring - 0.68) * 5.0) * (1.0 - smooth((ring - 0.92) * 12.0));
    // Fine grain: long streaks along the plank.
    let px = cells(t[0], 3.0);
    let py = cells(t[1], 160.0);
    let streak = pfbm(u / t[0] * px as f32, v / t[1] * py as f32, px, py, 57, 2);
    let profile = if d <= 0.0 {
        0.0
    } else {
        let q = (d / 0.002).min(1.0);
        (q * (2.0 - q)).sqrt()
    };
    // Boards cup a little across their width; latewood stands proud.
    let across = (fy / p.width - 0.5) * 2.0;
    let cup = var * 0.06 * (h2(r, k, 58) - 0.3) * across * across;
    let face = 0.88 - cup + 0.05 * late + 0.02 * streak;
    let height = lerp(0.0, face, profile);
    let lum = (1.0 + var * 0.4 * (hp - 0.5)) * (1.0 - 0.38 * late) * (0.86 + 0.24 * streak);
    let hue = var * 0.12 * (h2(r, k, 59) - 0.5);
    let wood = [
        WOOD_COLOR[0] * lum * (1.0 + hue),
        WOOD_COLOR[1] * lum,
        WOOD_COLOR[2] * lum * (1.0 - hue),
    ];
    let color = mix3(WOOD_GAP, wood, smooth(profile * 2.0));
    let rough = lerp(1.0, 0.58 + 0.14 * late + 0.06 * streak, profile);
    Sample {
        height: height.clamp(0.0, 1.0),
        color,
        rough,
    }
}
