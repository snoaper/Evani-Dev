//! The kit's pixel art: every frame is drawn in code on the art grid
//! (`target/look-bible.md`), in palette roles, then coloured from the active
//! look's ramps, so the kit never names a look. The frames go into the
//! renderer's sprite art as one pixel-art sheet (`tda.<name>`): a look that
//! ships a frame of the same name replaces it.
//!
//! * A pixel is 0 (clear), [`PURE`] (pure white, tinted when drawn) or
//!   `1 + role * 4 + shade` ([`c`]), shade 0 dark to 3 light.
//! * Each role reads the first look ramp of its list (`ramp.<name>`, four
//!   shades) and falls back to the look bible's colours ([`Colors`]).
//! * Scene frames also get a normal map `<name>.n` (for the 2D lights) and
//!   actor frames a hit-flash copy `<name>.w`.

use evani_core::plugin::LookData;
use evani_render::{ArtSheet, SpriteArt};

/// Palette roles.
pub const INK: u8 = 0;
pub const WALL: u8 = 1;
pub const FLOOR: u8 = 2;
pub const MOSS: u8 = 3;
pub const RED: u8 = 4;
pub const TEAL: u8 = 5;
pub const BRASS: u8 = 6;
pub const GLOW: u8 = 7;
pub const WHITE: u8 = 8;
pub const WOOD: u8 = 9;
pub const SKIN: u8 = 10;
const ROLES: usize = 11;

/// A pure white pixel (shadows, halos, arcs: the draw tints them).
pub const PURE: u8 = 255;

/// The pixel value of `role` at `shade` (0 dark to 3 light).
pub const fn c(role: u8, shade: u8) -> u8 {
    1 + role * 4 + if shade > 3 { 3 } else { shade }
}

/// The look ramps each role tries, first found wins, and the look-bible
/// colours (sRGB, dark to light) when the look has none of them.
const RAMPS: [(&[&str], [u32; 4]); ROLES] = [
    (&["ink"], [0x14121c, 0x2b2a35, 0x45435a, 0x6a6880]),
    (&["wall", "stone"], [0x2b2a35, 0x4a4a5e, 0x6c6d82, 0x9192a6]),
    (&["floor", "sand"], [0x6a6252, 0x8a7f6a, 0xa39a82, 0xc4bca4]),
    (&["moss", "leaf", "green"], [0x1f3f2a, 0x2e5e3a, 0x3e7d4b, 0x6aa86a]),
    (&["red"], [0x5a1c22, 0x8e2c2e, 0xc9453a, 0xf08070]),
    (&["teal"], [0x15474e, 0x226e74, 0x2f9aa0, 0x72d0c8]),
    (&["brass", "gold"], [0x6a4c18, 0x9e7a28, 0xe5b93c, 0xfff0a0]),
    (&["glow", "blue"], [0x2c3070, 0x4a54a8, 0x6e7bd8, 0xc3caff]),
    (&["white"], [0x8a8678, 0xb8b2a4, 0xf1ece0, 0xffffff]),
    (&["wood", "brown"], [0x3a2418, 0x5e3c26, 0x8a5a38, 0xb88458]),
    (&["skin", "sand"], [0x8a5a3c, 0xb27e58, 0xe0b088, 0xf4d8b4]),
];

/// Light colours when the look has no `light` palette: ambient, sun,
/// warm (torch), cool (magic), rim.
const LIGHTS: [u32; 5] = [0x5a6088, 0xffe8c0, 0xffa850, 0x6e7bd8, 0xfff0d0];

fn srgb_to_lin(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn lin_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// The colours of every role and shade, and the light colours.
#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    /// sRGB of `role * 4 + shade`.
    pub srgb: [[u8; 3]; ROLES * 4],
    /// Linear light colours: ambient, sun, warm, cool, rim.
    pub light: [[f32; 3]; 5],
}

impl Colors {
    pub fn new(look: Option<&LookData>) -> Self {
        let mut srgb = [[0; 3]; ROLES * 4];
        for (r, (names, def)) in RAMPS.iter().enumerate() {
            let ramp = names
                .iter()
                .find_map(|n| look.and_then(|l| l.palette(&format!("ramp.{n}"))).filter(|p| p.colors.len() >= 4));
            for s in 0..4 {
                srgb[r * 4 + s] = match ramp.and_then(|p| p.colors.get(s)) {
                    Some(l) => [lin_to_srgb(l[0]), lin_to_srgb(l[1]), lin_to_srgb(l[2])],
                    None => hex(def[s]),
                };
            }
        }
        let mut light = LIGHTS.map(|v| hex(v).map(srgb_to_lin));
        if let Some(p) = look.and_then(|l| l.palette("light")) {
            for (d, s) in light.iter_mut().zip(&p.colors) {
                *d = [s[0], s[1], s[2]];
            }
        }
        Self { srgb, light }
    }

    /// Linear RGBA of `role` at `shade`, for the draw code.
    pub fn lin(&self, role: u8, shade: u8, a: f32) -> [f32; 4] {
        let v = self.srgb[usize::from(role) * 4 + usize::from(shade.min(3))];
        [srgb_to_lin(v[0]), srgb_to_lin(v[1]), srgb_to_lin(v[2]), a]
    }

    fn rgba(&self, v: u8) -> [u8; 4] {
        match v {
            0 => [0; 4],
            PURE => [255; 4],
            v => {
                let s = self.srgb[usize::from(v - 1).min(ROLES * 4 - 1)];
                [s[0], s[1], s[2], 255]
            }
        }
    }
}

/// A small integer hash for fixed per-pixel variation.
pub fn hash(x: i32, y: i32, s: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77) ^ s.wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h
}

/// An indexed picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Pic {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

impl Pic {
    pub fn new(w: i32, h: i32) -> Self {
        Self { w, h, px: vec![0; (w.max(0) * h.max(0)) as usize] }
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    /// The pixel at (`x`, `y`); 0 outside.
    pub fn at(&self, x: i32, y: i32) -> u8 {
        if self.inside(x, y) { self.px[(y * self.w + x) as usize] } else { 0 }
    }

    pub fn put(&mut self, x: i32, y: i32, v: u8) {
        if self.inside(x, y) {
            let i = (y * self.w + x) as usize;
            self.px[i] = v;
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, v: u8) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, v);
            }
        }
    }

    /// A block lit from the top left: top and left edges a shade up, bottom
    /// and right a shade down.
    pub fn boxed(&mut self, x: i32, y: i32, w: i32, h: i32, role: u8, shade: u8) {
        self.rect(x, y, w, h, c(role, shade));
        self.rect(x, y, w, 1, c(role, shade + 1));
        self.rect(x, y, 1, h, c(role, shade + 1));
        self.rect(x, y + h - 1, w, 1, c(role, shade.saturating_sub(1)));
        self.rect(x + w - 1, y + 1, 1, h - 1, c(role, shade.saturating_sub(1)));
    }

    /// A filled ellipse shaded as a sphere lit from the top left, shades
    /// `lo..=hi`.
    pub fn ellipse(&mut self, [cx, cy]: [f32; 2], [rx, ry]: [f32; 2], role: u8, lo: u8, hi: u8) {
        let (l0, l1, l2) = (-0.5_f32, -0.6_f32, 0.62_f32);
        let ln = (l0 * l0 + l1 * l1 + l2 * l2).sqrt();
        for y in (cy - ry).floor() as i32..=(cy + ry).ceil() as i32 {
            for x in (cx - rx).floor() as i32..=(cx + rx).ceil() as i32 {
                let u = (x as f32 + 0.5 - cx) / rx.max(0.1);
                let v = (y as f32 + 0.5 - cy) / ry.max(0.1);
                let d2 = u * u + v * v;
                if d2 > 1.0 {
                    continue;
                }
                let z = (1.0 - d2).sqrt();
                let lit = (u * l0 + v * l1 + z * l2) / ln;
                let s = if lit > 0.8 {
                    3
                } else if lit > 0.45 {
                    2
                } else if lit > 0.1 {
                    1
                } else {
                    0
                };
                self.put(x, y, c(role, s.clamp(lo, hi.max(lo))));
            }
        }
    }

    /// A pure-white filled ellipse filling the picture.
    fn oval(w: i32, h: i32) -> Self {
        let mut p = Self::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let u = (x as f32 + 0.5 - w as f32 * 0.5) / (w as f32 * 0.5);
                let v = (y as f32 + 0.5 - h as f32 * 0.5) / (h as f32 * 0.5);
                if u * u + v * v <= 1.0 {
                    p.put(x, y, PURE);
                }
            }
        }
        p
    }

    /// A one-pixel ink outline: clear pixels beside a non-ink pixel.
    pub fn ink(mut self) -> Self {
        let solid = |v: u8| v != 0 && !(1..=4).contains(&v);
        let src = self.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                if src.at(x, y) == 0
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| solid(src.at(x + dx, y + dy)))
                {
                    self.put(x, y, c(INK, 0));
                }
            }
        }
        self
    }

    /// The picture with `n` clear pixels around it.
    pub fn pad(&self, n: i32) -> Self {
        let mut p = Self::new(self.w + 2 * n, self.h + 2 * n);
        p.blit(self, n, n);
        p
    }

    /// `pad(1).ink()`: interior art with its outline.
    pub fn outlined(&self) -> Self {
        self.pad(1).ink()
    }

    pub fn flip_x(&self) -> Self {
        let mut p = Self::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                p.put(self.w - 1 - x, y, self.at(x, y));
            }
        }
        p
    }

    pub fn flip_y(&self) -> Self {
        let mut p = Self::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                p.put(x, self.h - 1 - y, self.at(x, y));
            }
        }
        p
    }

    pub fn transpose(&self) -> Self {
        let mut p = Self::new(self.h, self.w);
        for y in 0..self.h {
            for x in 0..self.w {
                p.put(y, x, self.at(x, y));
            }
        }
        p
    }

    /// Copy the non-clear pixels of `src` with its top-left at (`x`, `y`).
    pub fn blit(&mut self, src: &Pic, x: i32, y: i32) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                let v = src.at(sx, sy);
                if v != 0 {
                    self.put(x + sx, y + sy, v);
                }
            }
        }
    }

    pub fn crop(&self, x: i32, y: i32, w: i32, h: i32) -> Self {
        let mut p = Self::new(w, h);
        for yy in 0..h {
            for xx in 0..w {
                p.put(xx, yy, self.at(x + xx, y + yy));
            }
        }
        p
    }

    /// Replace every non-clear pixel through `f`.
    pub fn map(&self, f: impl Fn(u8) -> u8) -> Self {
        Self { w: self.w, h: self.h, px: self.px.iter().map(|&v| if v == 0 { 0 } else { f(v) }).collect() }
    }
}

/// A hand-drawn pattern: one string per row, `.` clear, other characters
/// from `key`: space-separated triplets `<char><role letter><shade>` (role
/// letters: k ink, s wall, f floor, m moss, r red, t teal, b brass, g glow,
/// w white, d wood, n skin, x pure white). Short rows end clear.
pub fn pat(rows: &[&str], key: &str) -> Pic {
    let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
    let mut p = Pic::new(w, rows.len() as i32);
    let mut map: Vec<(char, u8)> = Vec::new();
    for k in key.split_whitespace() {
        let ch: Vec<char> = k.chars().collect();
        let [ch0, role, shade] = ch[..] else { continue };
        let r = match role {
            'k' => INK,
            's' => WALL,
            'f' => FLOOR,
            'm' => MOSS,
            'r' => RED,
            't' => TEAL,
            'b' => BRASS,
            'g' => GLOW,
            'w' => WHITE,
            'd' => WOOD,
            'n' => SKIN,
            _ => {
                map.push((ch0, PURE));
                continue;
            }
        };
        let s = shade.to_digit(10).unwrap_or(0) as u8;
        map.push((ch0, c(r, s)));
    }
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if let Some((_, v)) = map.iter().find(|(k, _)| *k == ch) {
                p.put(x as i32, y as i32, *v);
            }
        }
    }
    p
}

fn shade_of(v: u8) -> f32 {
    match v {
        0 => 0.0,
        PURE => 3.0,
        v => f32::from((v - 1) % 4),
    }
}

/// The normal map of `p` (raw RGBA): height from the shade and the distance
/// to the nearest clear pixel (outside the picture counts as solid, so tiles
/// stay flat at their edges).
pub fn normal_map(p: &Pic) -> Vec<u8> {
    let clear = |x: i32, y: i32| p.inside(x, y) && p.at(x, y) == 0;
    let mut height = vec![0.0_f32; p.px.len()];
    for y in 0..p.h {
        for x in 0..p.w {
            let v = p.at(x, y);
            if v == 0 {
                continue;
            }
            let mut d = 3;
            'ring: for r in 1..3_i32 {
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dy.abs()) == r && clear(x + dx, y + dy) {
                            d = r;
                            break 'ring;
                        }
                    }
                }
            }
            height[(y * p.w + x) as usize] = 0.4 * shade_of(v) / 3.0 + 0.6 * d as f32 / 3.0;
        }
    }
    let mut out = Vec::with_capacity(p.px.len() * 4);
    for y in 0..p.h {
        for x in 0..p.w {
            let i = (y * p.w + x) as usize;
            if p.px[i] == 0 {
                out.extend_from_slice(&[128, 128, 255, 0]);
                continue;
            }
            let hc = height[i];
            let h = |xx: i32, yy: i32| if p.inside(xx, yy) { height[(yy * p.w + xx) as usize] } else { hc };
            let nx = (h(x - 1, y) - h(x + 1, y)) * 1.5;
            let ny = (h(x, y + 1) - h(x, y - 1)) * 1.5;
            let len = (nx * nx + ny * ny + 1.0).sqrt();
            let enc = |n: f32| ((n / len * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[enc(nx), enc(ny), enc(1.0), 255]);
        }
    }
    out
}

// ---------------------------------------------------------------- frames

fn wall(mask: u8) -> Pic {
    let (n, e, s, w) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
    let mut p = Pic::new(16, 16);
    p.rect(0, 0, 16, 16, c(WALL, 0));
    for y in 0..16 {
        for x in 0..16 {
            if hash(x, y, 7).is_multiple_of(9) {
                p.put(x, y, c(WALL, 1));
            }
        }
    }
    if s {
        // The face strip: a lit lip, bricks, the dark base.
        p.rect(0, 9, 16, 1, c(INK, 0));
        p.rect(0, 10, 16, 1, c(WALL, 2));
        p.rect(0, 11, 16, 4, c(WALL, 1));
        p.rect(0, 13, 16, 1, c(WALL, 0));
        for (x, y0) in [(3, 11), (11, 11), (7, 14), (15, 14)] {
            p.rect(x, y0, 1, if y0 == 11 { 2 } else { 1 }, c(WALL, 0));
        }
        for x in [0, 4, 8, 12] {
            p.put(x, 11, c(WALL, 2));
        }
        p.rect(0, 15, 16, 1, c(INK, 0));
    }
    if n {
        p.rect(0, 0, 16, 1, c(INK, 0));
        p.rect(0, 1, 16, 1, c(WALL, 2));
    }
    if w {
        p.rect(0, 0, 1, 16, c(INK, 0));
        p.rect(1, if n { 1 } else { 0 }, 1, if s { 8 } else { 16 }, c(WALL, 2));
    }
    if e {
        p.rect(15, 0, 1, 16, c(INK, 0));
    }
    p
}

fn floor(i: u8) -> Pic {
    let s = 1 + (i & 1);
    let mut p = Pic::new(16, 16);
    p.boxed(0, 0, 16, 16, FLOOR, s);
    if i >= 2 {
        for (x, y) in [(4, 5), (5, 5), (10, 9), (11, 10), (12, 10), (6, 12)] {
            p.put(x, y, c(FLOOR, s - 1));
        }
        for (x, y) in [(4, 4), (10, 8), (11, 9)] {
            p.put(x, y, c(FLOOR, s + 1));
        }
    }
    p
}

fn pit() -> Pic {
    let mut p = Pic::new(16, 16);
    p.rect(0, 0, 16, 16, c(INK, 0));
    for y in 0..16 {
        for x in 0..16 {
            if hash(x, y, 3).is_multiple_of(13) {
                p.put(x, y, c(INK, 1));
            }
        }
    }
    p
}

fn pit_lip() -> Pic {
    let mut p = Pic::new(16, 16);
    p.rect(0, 0, 16, 2, c(FLOOR, 0));
    p.rect(0, 2, 16, 1, c(INK, 1));
    p
}

fn switch(down: bool) -> Pic {
    let mut p = Pic::new(16, 16);
    if down {
        p.ellipse([7.5, 8.0], [5.5, 4.5], WALL, 0, 2);
        p.ellipse([7.5, 8.0], [2.2, 2.0], GLOW, 2, 3);
        p.put(7, 7, PURE);
    } else {
        p.ellipse([7.5, 7.5], [5.5, 5.0], WALL, 1, 3);
        p.ellipse([7.5, 7.5], [2.2, 2.0], GLOW, 0, 1);
    }
    p.ink()
}

fn gate() -> Pic {
    let mut p = Pic::new(16, 16);
    for x in [2, 7, 12] {
        p.rect(x, 0, 1, 16, c(WALL, 3));
        p.rect(x + 1, 0, 1, 16, c(WALL, 2));
        p.rect(x + 2, 0, 1, 16, c(WALL, 1));
    }
    p.boxed(0, 2, 16, 2, WALL, 2);
    p.boxed(0, 12, 16, 2, WALL, 1);
    p.ink()
}

/// The locked door: 32 x 16, planks, brass bands and the keyhole plate.
fn door() -> Pic {
    let mut p = Pic::new(32, 16);
    for x in 0..32 {
        let s = match x % 5 {
            0 => 0,
            1 => 2,
            _ => 1,
        };
        p.rect(x, 0, 1, 16, c(WOOD, s));
    }
    for y in [3, 11] {
        p.rect(0, y, 32, 1, c(BRASS, 2));
        p.rect(0, y + 1, 32, 1, c(BRASS, 1));
        for x in (2..32).step_by(6) {
            p.put(x, y, c(BRASS, 3));
        }
    }
    p.boxed(13, 4, 6, 8, BRASS, 2);
    p.rect(15, 6, 2, 2, c(INK, 0));
    p.rect(15, 8, 2, 2, c(INK, 0));
    p.rect(0, 0, 32, 1, c(INK, 0));
    p.rect(0, 15, 32, 1, c(INK, 0));
    p.rect(0, 0, 1, 16, c(INK, 0));
    p.rect(31, 0, 1, 16, c(INK, 0));
    p
}

const HEART: [&str; 6] = [".RR.RR.", "RQRRRRR", "RRRRRRr", ".RRRRr.", "..RRr..", "...r..."];
const HEART_KEY: &str = "Rr2 Qr3 rr1";

fn heart() -> Pic {
    pat(&HEART, HEART_KEY)
}

fn container() -> Pic {
    let mut p = Pic::new(11, 10);
    p.ellipse([5.5, 5.0], [5.5, 5.0], BRASS, 1, 3);
    p.blit(&heart(), 2, 2);
    p.outlined()
}

const KEY_ROWS: [&str; 10] = [".bBb.", "bQ.Bb", "bB.Bb", ".bBb.", "..B..", "..b..", "..BB.", "..b..", "..BBb", "..b.."];
const BRASS_KEY: &str = "bb1 Bb2 Qb3";
const GEM_ROWS: [&str; 6] = ["..Q..", ".bQB.", "bBQBb", "bBBBb", ".bBb.", "..b.."];

fn wand() -> Pic {
    pat(
        &[".gGg.", "gGWGg", "gGGGg", ".gGg.", "..b..", "..D..", "..d..", "..D..", "..d..", "..D..", "..d..", "..b.."],
        "gg1 Gg2 Wg3 bb2 dd1 Dd2",
    )
    .outlined()
}

fn shard() -> Pic {
    pat(
        &[
            "....x....",
            "...gWG...",
            "...gWG...",
            "..gGWGG..",
            "..gGWGG..",
            ".gGGWGGG.",
            ".gGGGWGG.",
            "gGGGGWGGg",
            ".gGGGGGg.",
            "..gGGGg..",
            "..gGGg...",
            "...gG....",
            "....g....",
        ],
        "gg1 Gg2 Wg3 xw3",
    )
    .outlined()
}

fn pedestal() -> Pic {
    let mut p = Pic::new(14, 8);
    p.boxed(2, 3, 10, 4, WALL, 1);
    p.boxed(0, 0, 14, 3, WALL, 2);
    p.boxed(0, 6, 14, 2, WALL, 1);
    p.outlined()
}

const HERO_KEY: &str = "rr1 Rr2 Qr3 nn2 Nn3 ek0 tt1 Tt2 Ut3 dd1 Dd2 bb2";

/// The hero, `dir` in [`crate::game::Facing`] order (up, down, left,
/// right), walk frame `f`.
fn hero(dir: usize, f: usize) -> Pic {
    let legs: [&str; 2] = if f == 0 { ["..dD..Dd..", "..dd..dd.."] } else { ["...dDDd...", "...dddd..."] };
    let side_legs: [&str; 2] = if f == 0 { ["...dD.d...", "...dd.dd.."] } else { ["....dD....", "....dd...."] };
    let (rows, shield_x): ([&str; 12], i32) = match dir {
        0 => (
            [
                "...rRRr...",
                "..rRRRRr..",
                ".rRQQRRRr.",
                "rrrrrrrrrr",
                ".dDDDDDDd.",
                ".dDDDDDDd.",
                "..dddddd..",
                ".tTTTTTTt.",
                "tTTUUTTTTt",
                "tTddddddTt",
                legs[0],
                legs[1],
            ],
            1,
        ),
        1 => (
            [
                "...rRRr...",
                "..rRQQRr..",
                ".rRRRRRRr.",
                "rrrrrrrrrr",
                ".nNNNNNNn.",
                ".nenNNnen.",
                "..nnnnnn..",
                ".tTTUUTTt.",
                "tTTTTTTTTt",
                "tTddbbddTt",
                legs[0],
                legs[1],
            ],
            8,
        ),
        _ => (
            [
                "..rRRRr...",
                ".rRQQRRr..",
                ".rRRRRRRr.",
                "..rrrrrrrr",
                "..dNNNNN..",
                "..dNNNeN..",
                "...nnnn...",
                "...tTTUt..",
                "..tTTTTTn.",
                "..tdbddt..",
                side_legs[0],
                side_legs[1],
            ],
            1,
        ),
    };
    let mut p = pat(&rows, HERO_KEY).pad(1);
    p.blit(&pat(&["tUt", "Tbt", "TTt", ".t."], "tt1 Tt2 Ut3 bb2"), shield_x, 8);
    let p = p.ink();
    if dir == 2 { p.flip_x() } else { p }
}

fn slime(f: usize) -> Pic {
    let mut p = Pic::new(12, 8);
    let (rx, ry, cy, eye) = if f == 0 { (5.5, 7.5, 8.0, 4) } else { (6.0, 6.0, 8.0, 5) };
    p.ellipse([6.0, cy], [rx, ry], MOSS, 0, 3);
    for x in [3, 8] {
        p.rect(x, eye, 1, 2, c(INK, 0));
    }
    p.outlined()
}

fn bat(f: usize) -> Pic {
    let rows: [&str; 7] = if f == 0 {
        [
            "A..............A",
            "AA....a..a....AA",
            "AAA..aAAAAa..AAA",
            ".AAAaAhAAhAaAAA.",
            "..AAAArAArAAAA..",
            "...A..aAAa..A...",
            "................",
        ]
    } else {
        [
            "......a..a......",
            ".....aAAAAa.....",
            "....aAhAAhAa....",
            "..AAAArAArAAAA..",
            ".AAAAAAAAAAAAAA.",
            "AAA..aAAAAa..AAA",
            "A.....a..a.....A",
        ]
    };
    pat(&rows, "ak1 Ak2 hk3 rr3").outlined()
}

fn husk(f: usize) -> Pic {
    let legs: [&str; 2] = if f == 0 { ["....dd.dd...", "....ss.ss..."] } else { ["...dd...dd..", "...ss...ss.."] };
    pat(
        &[
            ".....rR.....",
            "....rRRr....",
            "...sSSSSs...",
            "..sSHHSSSs..",
            "..sSSSSSSs..",
            "..skrRrrks..",
            "..sSSSSSSs..",
            "SHHSsssssSHS",
            "sSSsdDDDdsSs",
            ".ss.dDDDd.ss",
            "....dDDDd...",
            legs[0],
            legs[1],
        ],
        "rr1 Rr2 ss1 Ss2 Hs3 kk0 dd1 Dd2",
    )
    .outlined()
}

fn spitter(f: usize) -> Pic {
    let mouth: [&str; 2] = if f == 0 { ["msSSkkkkSSsm", "mmsSSSSSSsmm"] } else { ["msSkkkkkkSsm", "mmskkrrkksmm"] };
    pat(
        &[
            "...sSSSSs...",
            "..sSHHSSSs..",
            ".sSHSSSSSSs.",
            ".sSSSSSSSSs.",
            ".sSkkSSkkSs.",
            ".sSkrSSkrSs.",
            ".sSSSSSSSSs.",
            mouth[0],
            mouth[1],
            "MmmsssssmmMm",
            ".MmmmmmmmmM.",
        ],
        "ss1 Ss2 Hs3 kk0 rr3 mm1 Mm2",
    )
    .outlined()
}

/// The Warden: `bright` lights its core and eyes.
fn warden(bright: bool) -> Pic {
    let mut p = Pic::new(22, 22);
    p.boxed(0, 9, 4, 9, WALL, 1);
    p.boxed(18, 9, 4, 9, WALL, 1);
    p.boxed(0, 16, 4, 4, WALL, 2);
    p.boxed(18, 16, 4, 4, WALL, 2);
    p.ellipse([11.0, 12.5], [8.0, 9.0], RED, 0, 3);
    p.rect(5, 2, 12, 2, c(BRASS, 2));
    p.rect(5, 3, 12, 1, c(BRASS, 1));
    for x in [5, 8, 11, 13, 16] {
        p.put(x, 1, c(BRASS, 3));
    }
    p.rect(10, 2, 2, 1, c(GLOW, 3));
    let (lo, hi) = if bright { (2, 3) } else { (0, 1) };
    for x in [7, 13] {
        p.rect(x, 7, 2, 2, c(INK, 0));
        p.put(x + 1, 7, c(GLOW, hi));
    }
    p.ellipse([11.0, 14.5], [2.6, 2.6], GLOW, lo, hi);
    if bright {
        p.put(10, 13, PURE);
    }
    p.outlined()
}

fn bolt() -> Pic {
    let mut p = Pic::new(7, 7);
    p.ellipse([3.5, 3.5], [3.4, 3.4], GLOW, 1, 3);
    p.put(3, 3, PURE);
    p
}

fn seed() -> Pic {
    let mut p = Pic::new(4, 4);
    p.ellipse([2.0, 2.0], [2.0, 2.0], RED, 1, 3);
    p.outlined()
}

/// The sword pointing right.
fn sword() -> Pic {
    pat(&["b.........", "bddBBBBBBQ", "b..bbbbbb."], "bb1 Bb2 Qb3 dd1").outlined()
}

/// The swing arc for a sword pointing right: a crescent.
fn arc() -> Pic {
    let mut p = Pic::new(14, 16);
    for y in 0..16 {
        for x in 0..14 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5 - 8.0);
            let outer = (fx + 2.0).hypot(fy) <= 14.0;
            let inner = (fx + 5.0).hypot(fy) <= 14.0;
            if outer && !inner {
                p.put(x, y, PURE);
            }
        }
    }
    p
}

fn spark(f: usize) -> Pic {
    let mut p = Pic::new(7, 7);
    let v = c(WHITE, 3);
    match f {
        0 => {
            p.rect(3, 2, 1, 3, v);
            p.rect(2, 3, 3, 1, v);
        }
        1 => {
            p.rect(3, 0, 1, 7, c(BRASS, 3));
            p.rect(0, 3, 7, 1, c(BRASS, 3));
            p.put(3, 3, v);
            for (x, y) in [(1, 1), (5, 1), (1, 5), (5, 5)] {
                p.put(x, y, v);
            }
        }
        _ => {
            for (x, y) in [(0, 0), (6, 0), (0, 6), (6, 6), (3, 0), (0, 3), (6, 3), (3, 6)] {
                p.put(x, y, c(BRASS, 2));
            }
        }
    }
    p
}

fn dust(f: usize) -> Pic {
    let mut p = Pic::new(5, 4);
    match f {
        0 => p.ellipse([2.5, 2.0], [2.5, 2.0], WHITE, 1, 2),
        1 => p.ellipse([2.5, 2.5], [1.8, 1.5], WHITE, 1, 2),
        _ => {
            p.put(1, 2, c(WHITE, 1));
            p.put(3, 1, c(WHITE, 1));
        }
    }
    p
}

/// A dithered pure-white halo.
fn glow() -> Pic {
    const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    let mut p = Pic::new(24, 24);
    for y in 0..24 {
        for x in 0..24 {
            let r = (x as f32 + 0.5 - 12.0).hypot(y as f32 + 0.5 - 12.0);
            let t = (1.0 - r / 12.0).max(0.0);
            let b = f32::from(BAYER[((y % 4) * 4 + x % 4) as usize]) / 16.0;
            if b < t * t * 1.6 {
                p.put(x, y, PURE);
            }
        }
    }
    p
}

fn torch(f: usize) -> Pic {
    let flame: [&str; 7] = match f {
        0 => ["...r..", "..rR..", ".rRbR.", ".RbBbr", ".RbxbR", "..bxb.", "..rRr."],
        1 => ["..r...", "..Rr..", ".rRbr.", "rRbBR.", "RbBxbR", ".bxxb.", ".rRRr."],
        _ => ["....r.", "...rR.", "..rbR.", ".rbBbR", ".RbxbR", "..bxb.", "..rRr."],
    };
    let mut rows: Vec<&str> = flame.to_vec();
    rows.extend_from_slice(&[".sSSs.", "..Dd..", "..Dd..", ".sSSs.", "..ss.."]);
    pat(&rows, "Bb3 bb2 Rr2 rr1 xw3 dd1 Dd2 ss1 Ss2").outlined()
}

fn crack() -> Pic {
    let mut p = Pic::new(16, 16);
    let path = [(3, 4), (4, 5), (5, 5), (6, 6), (7, 7), (7, 8), (8, 9), (9, 9), (10, 10), (11, 11), (12, 11)];
    for (x, y) in path {
        p.put(x, y, c(INK, 0));
        p.put(x, y + 1, c(FLOOR, 0));
    }
    for (x, y) in [(7, 6), (8, 5), (9, 4), (10, 12), (10, 13)] {
        p.put(x, y, c(INK, 1));
    }
    p
}

fn moss() -> Pic {
    let mut p = Pic::new(16, 16);
    let blobs = [(4.0, 11.0, 4.0), (8.5, 13.0, 3.0), (12.0, 12.0, 2.5), (2.5, 6.0, 2.0)];
    for y in 0..16 {
        for x in 0..16 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = blobs.iter().map(|(bx, by, r)| (fx - bx).hypot(fy - by) / r).fold(f32::MAX, f32::min);
            if d < 1.0 {
                let s = if d > 0.8 {
                    0
                } else if hash(x, y, 5).is_multiple_of(3) {
                    2
                } else {
                    1
                };
                p.put(x, y, c(MOSS, s));
            }
        }
    }
    p
}

fn pot() -> Pic {
    let mut p = Pic::new(10, 10);
    p.ellipse([5.0, 6.0], [5.0, 4.0], WOOD, 0, 3);
    p.boxed(2, 0, 6, 3, WOOD, 2);
    p.rect(3, 0, 4, 1, c(INK, 0));
    p.outlined()
}

fn bones() -> Pic {
    pat(
        &[".WW.....ww..", "WxwWWWWwWxW.", ".WW..ww.ww..", "....wWWw....", "...wxWWWw...", "...wWkWkw...", "....wWWw...."],
        "ww1 Ww2 xw3 kk0",
    )
    .outlined()
}

fn rubble() -> Pic {
    let mut p = Pic::new(12, 8);
    p.boxed(4, 0, 5, 5, WALL, 1);
    p.boxed(0, 3, 5, 4, WALL, 2);
    p.boxed(7, 4, 5, 4, WALL, 2);
    p.boxed(3, 6, 3, 2, WALL, 1);
    p.outlined()
}

fn banner() -> Pic {
    let mut p = Pic::new(8, 16);
    p.rect(0, 0, 8, 1, c(BRASS, 2));
    p.rect(1, 1, 6, 13, c(RED, 2));
    p.rect(1, 1, 1, 13, c(RED, 3));
    p.rect(6, 1, 1, 13, c(RED, 1));
    for x in [1, 2, 4, 5] {
        p.put(x, 14, c(RED, 1));
    }
    for x in [1, 4] {
        p.put(x, 15, c(RED, 1));
    }
    p.rect(3, 5, 2, 4, c(BRASS, 2));
    p.put(3, 5, c(BRASS, 3));
    p.outlined()
}

fn rack() -> Pic {
    let mut p = Pic::new(14, 12);
    for x in [4, 7, 10] {
        p.rect(x, 0, 1, 10, c(WALL, 3));
        p.put(x, 8, c(BRASS, 2));
    }
    p.boxed(0, 0, 2, 12, WOOD, 1);
    p.boxed(12, 0, 2, 12, WOOD, 1);
    p.boxed(0, 2, 14, 2, WOOD, 2);
    p.boxed(0, 9, 14, 2, WOOD, 1);
    p.outlined()
}

fn cobweb() -> Pic {
    let mut p = Pic::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = fx.hypot(fy);
            if r > 13.0 {
                continue;
            }
            let a = fy.atan2(fx).to_degrees();
            let spoke = [0.0_f32, 30.0, 60.0, 90.0].iter().any(|s| ((a - s).abs() * r.to_radians()) < 0.6);
            let ring = [4.5_f32, 8.5, 12.0].iter().any(|rr| (r - rr).abs() < 0.5);
            if spoke || ring {
                p.put(x, y, c(WHITE, if r < 6.0 { 2 } else { 1 }));
            }
        }
    }
    p
}

fn pillar() -> Pic {
    let mut p = Pic::new(14, 14);
    p.boxed(0, 9, 14, 5, WALL, 1);
    for y in 2..10 {
        for x in 2..12 {
            let s = match x {
                2..=3 => 3,
                4..=6 => 2,
                7..=9 => 1,
                _ => 0,
            };
            p.put(x, y, c(WALL, s));
        }
    }
    p.ellipse([7.0, 2.5], [5.0, 2.5], WALL, 2, 3);
    p.outlined()
}

fn helmet() -> Pic {
    let mut p = Pic::new(8, 6);
    p.ellipse([4.0, 5.0], [4.0, 5.0], WALL, 1, 3);
    p.rect(2, 3, 4, 1, c(INK, 0));
    p.rect(0, 5, 2, 1, c(RED, 2));
    p.outlined()
}

fn puddle() -> Pic {
    let mut p = Pic::new(14, 7);
    p.blit(&Pic::oval(14, 7).map(|_| c(GLOW, 0)), 0, 0);
    p.blit(&Pic::oval(10, 4).map(|_| c(GLOW, 1)), 2, 1);
    p.put(4, 2, c(WHITE, 2));
    p.put(5, 2, c(WHITE, 2));
    p
}

fn backdrop() -> Pic {
    let mut p = Pic::new(16, 16);
    p.rect(0, 0, 16, 16, c(INK, 1));
    for y in 0..16 {
        for x in 0..16 {
            let seam = y % 4 == 3 || (x + if (y / 4) % 2 == 0 { 0 } else { 4 }) % 8 == 7;
            if seam {
                p.put(x, y, c(INK, 0));
            } else if hash(x, y, 11).is_multiple_of(23) {
                p.put(x, y, c(MOSS, 0));
            }
        }
    }
    p
}

fn hud_heart(fill: usize) -> Pic {
    let full = heart();
    match fill {
        2 => full,
        1 => {
            let mut p = full.clone();
            for y in 0..p.h {
                for x in 4..p.w {
                    if p.at(x, y) != 0 {
                        p.put(x, y, c(RED, 0));
                    }
                }
            }
            p
        }
        _ => full.map(|_| c(RED, 0)),
    }
}

// ---------------------------------------------------------------- the sheet

/// What extra frames a picture gets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The frame only.
    Plain,
    /// Plus a normal map `.n`.
    Scene,
    /// Plus `.n` and a white hit flash `.w`.
    Actor,
}

/// The built sheet and every frame's size and fallback tint.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub sheet: ArtSheet,
    /// `(name, [w, h], tint)` sorted by name; the tint is the frame's most
    /// common colour (linear), drawn on a white cell when no art is there.
    pub frames: Vec<(String, [u32; 2], [f32; 4])>,
}

struct Builder<'a> {
    colors: &'a Colors,
    items: Vec<(String, u32, u32, Vec<u8>)>,
    meta: Vec<(String, [u32; 2], [f32; 4])>,
}

impl Builder<'_> {
    fn raw(&mut self, name: String, w: i32, h: i32, rgba: Vec<u8>) {
        self.items.push((name, w.max(1) as u32, h.max(1) as u32, rgba));
    }

    fn add(&mut self, name: &str, p: &Pic, kind: Kind) {
        let name = format!("tda.{name}");
        let rgba: Vec<u8> = p.px.iter().flat_map(|&v| self.colors.rgba(v)).collect();
        // The most common non-ink colour, for the plain fallback.
        let mut counts = [0u32; 256];
        for &v in &p.px {
            counts[usize::from(v)] += 1;
        }
        let best = (5..=255usize)
            .max_by_key(|&v| counts[v])
            .filter(|&v| counts[v] > 0)
            .or_else(|| (1..=4usize).max_by_key(|&v| counts[v]).filter(|&v| counts[v] > 0))
            .unwrap_or(255);
        let px = self.colors.rgba(best as u8);
        let tint = [srgb_to_lin(px[0]), srgb_to_lin(px[1]), srgb_to_lin(px[2]), 1.0];
        self.meta.push((name.clone(), [p.w.max(1) as u32, p.h.max(1) as u32], tint));
        if kind != Kind::Plain {
            self.raw(format!("{name}.n"), p.w, p.h, normal_map(p));
        }
        if kind == Kind::Actor {
            let flash: Vec<u8> = p.px.iter().flat_map(|&v| if v == 0 { [0; 4] } else { [255; 4] }).collect();
            self.raw(format!("{name}.w"), p.w, p.h, flash);
        }
        self.raw(name, p.w, p.h, rgba);
    }

    /// Shelf-pack every frame into one 512-wide sheet.
    fn finish(mut self) -> Built {
        const W: u32 = 512;
        let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
        let mut places = Vec::with_capacity(self.items.len());
        for (_, w, h, _) in &self.items {
            if x + w > W {
                x = 0;
                y += row;
                row = 0;
            }
            places.push([x, y]);
            x += w;
            row = row.max(*h);
        }
        let height = (y + row).max(1);
        let mut rgba8 = vec![0u8; (W * height * 4) as usize];
        let mut frames = Vec::with_capacity(self.items.len());
        for ((name, w, h, px), [fx, fy]) in self.items.drain(..).zip(places) {
            for row in 0..h {
                let src = (row * w * 4) as usize;
                let dst = (((fy + row) * W + fx) * 4) as usize;
                let n = (w * 4) as usize;
                if let (Some(d), Some(s)) = (rgba8.get_mut(dst..dst + n), px.get(src..src + n)) {
                    d.copy_from_slice(s);
                }
            }
            frames.push((name, [fx, fy, w, h]));
        }
        self.meta.sort_by(|a, b| a.0.cmp(&b.0));
        Built { sheet: ArtSheet { width: W, height, rgba8, frames, pixel_art: true }, frames: self.meta }
    }
}

/// Draw every frame in `colors` and pack them.
pub fn build(colors: &Colors) -> Built {
    use Kind::{Actor, Plain, Scene};
    let mut b = Builder { colors, items: Vec::new(), meta: Vec::new() };
    for m in 0..16u8 {
        b.add(&format!("wall.{m}"), &wall(m), Scene);
    }
    for i in 0..4u8 {
        b.add(&format!("floor.{i}"), &floor(i), Scene);
    }
    b.add("pit", &pit(), Scene);
    b.add("pit.lip", &pit_lip(), Plain);
    b.add("switch.up", &switch(false), Scene);
    b.add("switch.down", &switch(true), Scene);
    b.add("gate", &gate(), Scene);
    let d = door();
    let halves = [d.crop(0, 0, 16, 16), d.crop(16, 0, 16, 16)];
    for (i, h) in halves.iter().enumerate() {
        b.add(&format!("door.h.{i}"), h, Scene);
        b.add(&format!("door.v.{i}"), &h.transpose(), Scene);
    }
    b.add("heart", &heart().outlined(), Scene);
    b.add("container", &container(), Scene);
    b.add("key", &pat(&KEY_ROWS, BRASS_KEY).outlined(), Scene);
    b.add("gem", &pat(&GEM_ROWS, BRASS_KEY).outlined(), Scene);
    b.add("wand", &wand(), Scene);
    b.add("shard", &shard(), Scene);
    b.add("pedestal", &pedestal(), Scene);
    for (di, dn) in DIRS.iter().enumerate() {
        for f in 0..2 {
            b.add(&format!("hero.{dn}.{f}"), &hero(di, f), Actor);
        }
    }
    for f in 0..2 {
        b.add(&format!("slime.{f}"), &slime(f), Actor);
        b.add(&format!("bat.{f}"), &bat(f), Actor);
        b.add(&format!("husk.{f}"), &husk(f), Actor);
        b.add(&format!("spitter.{f}"), &spitter(f), Actor);
        b.add(&format!("warden.{f}"), &warden(f == 1), Actor);
    }
    b.add("bolt", &bolt(), Plain);
    b.add("seed", &seed(), Scene);
    let sw = sword();
    let down = sw.transpose();
    for (dn, p) in DIRS.iter().zip([down.flip_y(), down, sw.flip_x(), sw]) {
        b.add(&format!("sword.{dn}"), &p, Scene);
    }
    let ar = arc();
    let ad = ar.transpose();
    for (dn, p) in DIRS.iter().zip([ad.flip_y(), ad, ar.flip_x(), ar]) {
        b.add(&format!("arc.{dn}"), &p, Plain);
    }
    for (n, (w, h)) in ["s", "m", "l"].iter().zip([(10, 3), (14, 4), (22, 6)]) {
        b.add(&format!("shadow.{n}"), &Pic::oval(w, h), Plain);
    }
    for f in 0..3 {
        b.add(&format!("spark.{f}"), &spark(f), Plain);
        b.add(&format!("dust.{f}"), &dust(f), Plain);
        b.add(&format!("torch.{f}"), &torch(f), Scene);
    }
    b.add("glow", &glow(), Plain);
    b.add("crack", &crack(), Scene);
    b.add("moss", &moss(), Scene);
    b.add("pot", &pot(), Scene);
    b.add("bones", &bones(), Scene);
    b.add("rubble", &rubble(), Scene);
    b.add("banner", &banner(), Scene);
    b.add("rack", &rack(), Scene);
    let web = cobweb();
    b.add("cobweb.r", &web.flip_x(), Plain);
    b.add("cobweb", &web, Plain);
    b.add("pillar", &pillar(), Scene);
    b.add("helmet", &helmet(), Scene);
    b.add("puddle", &puddle(), Scene);
    b.add("backdrop", &backdrop(), Plain);
    for f in 0..3 {
        b.add(&format!("hud.heart.{f}"), &hud_heart(f), Plain);
    }
    b.add(
        "hud.key",
        &pat(&[".bBb.", "b...b", ".bBb.", "..B..", "..b..", "..BB.", "..b..", "..Bb.", "..b.."], BRASS_KEY),
        Plain,
    );
    let mut gem7 = GEM_ROWS.to_vec();
    gem7.insert(4, "bBBBb");
    b.add("hud.gem", &pat(&gem7, BRASS_KEY), Plain);
    b.finish()
}

/// Facing names in [`crate::game::Facing`] order.
pub const DIRS: [&str; 4] = ["up", "down", "left", "right"];

// ---------------------------------------------------------------- cells

/// A frame to draw: its sprite cell, hit-flash cell, art size and the tint
/// to draw it with (white over real art; a role colour over the plain
/// white cell when no sheet has the frame).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct F {
    pub cell: u16,
    pub flash: u16,
    pub w: f32,
    pub h: f32,
    pub tint: [f32; 4],
}

/// Every frame the draw code uses, resolved once per art revision.
#[derive(Debug, Clone, PartialEq)]
pub struct Cells {
    pub wall: [F; 16],
    pub floor: [F; 4],
    pub pit: F,
    pub pit_lip: F,
    pub switch: [F; 2],
    pub gate: F,
    pub door_h: [F; 2],
    pub door_v: [F; 2],
    pub heart: F,
    pub container: F,
    pub key: F,
    pub gem: F,
    pub wand: F,
    pub shard: F,
    pub pedestal: F,
    /// `[facing][walk frame]`.
    pub hero: [[F; 2]; 4],
    pub slime: [F; 2],
    pub bat: [F; 2],
    pub husk: [F; 2],
    pub spitter: [F; 2],
    pub warden: [F; 2],
    pub bolt: F,
    pub seed: F,
    pub sword: [F; 4],
    pub arc: [F; 4],
    pub shadow: [F; 3],
    pub spark: [F; 3],
    pub dust: [F; 3],
    pub glow: F,
    pub torch: [F; 3],
    pub crack: F,
    pub moss: F,
    pub pot: F,
    pub bones: F,
    pub rubble: F,
    pub banner: F,
    pub rack: F,
    pub cobweb: [F; 2],
    pub pillar: F,
    pub helmet: F,
    pub puddle: F,
    pub backdrop: F,
    pub hud_heart: [F; 3],
    pub hud_key: F,
    pub hud_gem: F,
}

impl Cells {
    /// Resolve every frame: its cell from `art` (any sheet; a look's frame of
    /// the same name wins), its size from the kit's sheet, `white` and the
    /// frame's tint when no sheet has it.
    pub fn resolve(art: Option<&SpriteArt>, built: &Built, white: u16) -> Self {
        let f = |name: &str| -> F {
            let full = format!("tda.{name}");
            let (size, tint) = built
                .frames
                .binary_search_by(|(n, _, _)| n.as_str().cmp(&full))
                .ok()
                .and_then(|i| built.frames.get(i))
                .map_or(([16, 16], [1.0; 4]), |(_, s, t)| (*s, *t));
            let (cell, tint) = match art.and_then(|a| a.cell(&full)) {
                Some(cell) => (cell, [1.0; 4]),
                None => (white, tint),
            };
            let flash = art.and_then(|a| a.cell(&format!("{full}.w"))).unwrap_or(cell);
            F { cell, flash, w: size[0] as f32, h: size[1] as f32, tint }
        };
        let n2 = |base: &str| [f(&format!("{base}.0")), f(&format!("{base}.1"))];
        let n3 = |base: &str| [f(&format!("{base}.0")), f(&format!("{base}.1")), f(&format!("{base}.2"))];
        let d4 = |base: &str| DIRS.map(|d| f(&format!("{base}.{d}")));
        Self {
            wall: core::array::from_fn(|m| f(&format!("wall.{m}"))),
            floor: core::array::from_fn(|i| f(&format!("floor.{i}"))),
            pit: f("pit"),
            pit_lip: f("pit.lip"),
            switch: [f("switch.up"), f("switch.down")],
            gate: f("gate"),
            door_h: n2("door.h"),
            door_v: n2("door.v"),
            heart: f("heart"),
            container: f("container"),
            key: f("key"),
            gem: f("gem"),
            wand: f("wand"),
            shard: f("shard"),
            pedestal: f("pedestal"),
            hero: DIRS.map(|d| n2(&format!("hero.{d}"))),
            slime: n2("slime"),
            bat: n2("bat"),
            husk: n2("husk"),
            spitter: n2("spitter"),
            warden: n2("warden"),
            bolt: f("bolt"),
            seed: f("seed"),
            sword: d4("sword"),
            arc: d4("arc"),
            shadow: [f("shadow.s"), f("shadow.m"), f("shadow.l")],
            spark: n3("spark"),
            dust: n3("dust"),
            glow: f("glow"),
            torch: n3("torch"),
            crack: f("crack"),
            moss: f("moss"),
            pot: f("pot"),
            bones: f("bones"),
            rubble: f("rubble"),
            banner: f("banner"),
            rack: f("rack"),
            cobweb: [f("cobweb"), f("cobweb.r")],
            pillar: f("pillar"),
            helmet: f("helmet"),
            puddle: f("puddle"),
            backdrop: f("backdrop"),
            hud_heart: n3("hud.heart"),
            hud_key: f("hud.key"),
            hud_gem: f("hud.gem"),
        }
    }
}
