//! Geometry helpers: rects, the tile grid as collision shapes, and opaque
//! pixel masks of sprites.

use crate::scene::{Atlas, Object, Rect, Scene, Shape, TileDef};
use std::collections::HashMap;

pub fn overlaps(a: Rect, b: Rect) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

pub fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

pub fn inflate(r: Rect, d: f32) -> Rect {
    [r[0] - d, r[1] - d, r[2] + 2.0 * d, r[3] + 2.0 * d]
}

pub fn union(a: Rect, b: Rect) -> Rect {
    let x0 = a[0].min(b[0]);
    let y0 = a[1].min(b[1]);
    let x1 = (a[0] + a[2]).max(b[0] + b[2]);
    let y1 = (a[1] + a[3]).max(b[1] + b[3]);
    [x0, y0, x1 - x0, y1 - y0]
}

pub fn centre(r: Rect) -> [f32; 2] {
    [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]
}

/// Distance from a point to a rect (0 inside).
pub fn dist_point_rect(p: [f32; 2], r: Rect) -> f32 {
    let dx = (r[0] - p[0]).max(0.0).max(p[0] - (r[0] + r[2]));
    let dy = (r[1] - p[1]).max(0.0).max(p[1] - (r[1] + r[3]));
    (dx * dx + dy * dy).sqrt()
}

/// The tile grid as collision shapes.
pub struct Geo<'a> {
    pub ts: f32,
    pub w: i32,
    pub h: i32,
    cells: HashMap<(i32, i32), Vec<&'a TileDef>>,
}

impl<'a> Geo<'a> {
    pub fn new(scene: &'a Scene) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<&TileDef>> = HashMap::new();
        for t in &scene.tiles {
            cells.entry((t.x, t.y)).or_default().push(t);
        }
        Self { ts: scene.tile_size, w: scene.map[0] as i32, h: scene.map[1] as i32, cells }
    }

    pub fn tiles_at(&self, tx: i32, ty: i32) -> &[&'a TileDef] {
        self.cells.get(&(tx, ty)).map_or(&[], Vec::as_slice)
    }

    pub fn cell(&self, v: f32) -> i32 {
        (v / self.ts).floor() as i32
    }

    /// The colliding shape at a tile (full beats slope beats top).
    pub fn shape(&self, tx: i32, ty: i32) -> (Shape, Option<&'a TileDef>) {
        let mut best = (Shape::None, None);
        let rank = |s: Shape| match s {
            Shape::Full => 3,
            Shape::Slope => 2,
            Shape::Top => 1,
            Shape::None => 0,
        };
        for t in self.tiles_at(tx, ty) {
            if rank(t.shape) > rank(best.0) {
                best = (t.shape, Some(*t));
            }
        }
        best
    }

    pub fn is_full(&self, tx: i32, ty: i32) -> bool {
        self.shape(tx, ty).0 == Shape::Full
    }

    /// World y of a slope tile's floor at world x.
    pub fn slope_y(&self, t: &TileDef, x: f32) -> f32 {
        let [from, to] = t.slope.unwrap_or([0.0, 0.0]);
        let fx = ((x - t.x as f32 * self.ts) / self.ts).clamp(0.0, 1.0);
        (t.y + 1) as f32 * self.ts - (from + (to - from) * fx)
    }

    /// Is world point `(x, y)` inside terrain? One-way tops count for their
    /// top `top_band` pixels.
    pub fn solid_px(&self, x: f32, y: f32, top_band: f32) -> bool {
        let (tx, ty) = (self.cell(x), self.cell(y));
        match self.shape(tx, ty) {
            (Shape::Full, _) => true,
            (Shape::Slope, Some(t)) => y >= self.slope_y(t, x),
            (Shape::Top, _) => y < ty as f32 * self.ts + top_band,
            _ => false,
        }
    }

    /// Does the world rect touch a full tile (slopes and one-ways never block sideways)?
    pub fn rect_hits_full(&self, r: Rect) -> bool {
        let (tx0, tx1) = (self.cell(r[0] + 0.01), self.cell(r[0] + r[2] - 0.01));
        let (ty0, ty1) = (self.cell(r[1] + 0.01), self.cell(r[1] + r[3] - 0.01));
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                if self.is_full(tx, ty) {
                    return true;
                }
            }
        }
        false
    }

    /// Does the world rect touch a hazard tile (shrunk by 2 px)?
    pub fn rect_hits_hazard(&self, r: Rect) -> bool {
        let r = inflate(r, -1.0);
        let (tx0, tx1) = (self.cell(r[0]), self.cell(r[0] + r[2]));
        let (ty0, ty1) = (self.cell(r[1]), self.cell(r[1] + r[3]));
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                for t in self.tiles_at(tx, ty) {
                    if t.hazard {
                        let tr = [tx as f32 * self.ts + 2.0, ty as f32 * self.ts + 2.0, self.ts - 4.0, self.ts - 4.0];
                        if overlaps(r, tr) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// The highest floor (smallest y) in `[lo, hi]` under feet spanning
    /// `x0..x1` with foot point `xc`: a slope under the foot point, else
    /// full-tile tops (open above) under the span, else one-way tops when
    /// `from_above` is the old feet y and they were at or above the top.
    pub fn floor(&self, x0: f32, x1: f32, xc: f32, lo: f32, hi: f32, from_above: Option<f32>) -> Option<f32> {
        if hi < lo {
            return None;
        }
        let (ty0, ty1) = (self.cell(lo), self.cell(hi));
        let tx = self.cell(xc);
        for ty in ty0..=ty1 {
            if let (Shape::Slope, Some(t)) = self.shape(tx, ty) {
                let sy = self.slope_y(t, xc);
                if sy >= lo && sy <= hi {
                    return Some(sy);
                }
            }
        }
        let mut best: Option<f32> = None;
        for tx in self.cell(x0)..=self.cell(x1 - 0.001) {
            for ty in ty0..=ty1 {
                let top = ty as f32 * self.ts;
                if top < lo || top > hi {
                    continue;
                }
                let ok = match self.shape(tx, ty).0 {
                    Shape::Full => !self.is_full(tx, ty - 1),
                    Shape::Top => from_above.is_some_and(|old| old <= top + 0.5),
                    _ => false,
                };
                if ok {
                    if best.is_none_or(|b| top < b) {
                        best = Some(top);
                    }
                    break;
                }
            }
        }
        best
    }

    /// World rect of a tile.
    pub fn tile_rect(&self, tx: i32, ty: i32) -> Rect {
        [tx as f32 * self.ts, ty as f32 * self.ts, self.ts, self.ts]
    }

    /// Terrain area of a tile (px²) inside `clip`: full tiles, slopes under
    /// their line, one-way tops as a 4 px band; hazards count in full.
    pub fn terrain_area(&self, tx: i32, ty: i32, clip: Rect) -> f32 {
        let tr = self.tile_rect(tx, ty);
        let Some(i) = intersect(tr, clip) else { return 0.0 };
        let (shape, t) = self.shape(tx, ty);
        let hazard = self.tiles_at(tx, ty).iter().any(|t| t.hazard);
        match shape {
            Shape::Full => i[2] * i[3],
            Shape::Slope => {
                // Sample columns.
                let mut a = 0.0;
                let n = i[2].ceil().max(1.0) as i32;
                for k in 0..n {
                    let x = i[0] + k as f32 + 0.5;
                    let sy = t.map_or(tr[1], |t| self.slope_y(t, x));
                    a += ((i[1] + i[3]) - sy.max(i[1])).max(0.0);
                }
                a
            }
            Shape::Top => intersect([tr[0], tr[1], tr[2], 4.0], clip).map_or(0.0, |r| r[2] * r[3]),
            Shape::None if hazard => i[2] * i[3],
            Shape::None => 0.0,
        }
    }
}

/// An opaque-pixel mask in world art pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    bits: Vec<bool>,
}

impl Mask {
    /// A solid rect (no art).
    pub fn rect(r: Rect) -> Self {
        let (x, y) = (r[0].round() as i32, r[1].round() as i32);
        let (w, h) = (r[2].round().max(0.0) as i32, r[3].round().max(0.0) as i32);
        Self { x, y, w, h, bits: vec![true; (w * h) as usize] }
    }

    /// The opaque pixels of atlas frame `f` drawn into `r` (nearest
    /// sampling when the sizes differ), mirrored when `flip`.
    pub fn frame(atlas: &Atlas, f: usize, r: Rect, flip: bool) -> Option<Self> {
        let [fw, fh] = atlas.frame_size(f)?;
        let mut m = Self::rect(r);
        if fw == 0 || fh == 0 || m.w == 0 || m.h == 0 {
            return Some(m);
        }
        for v in 0..m.h {
            for u in 0..m.w {
                let su = ((u as f32 + 0.5) * fw as f32 / m.w as f32) as u32;
                let sv = ((v as f32 + 0.5) * fh as f32 / m.h as f32) as u32;
                let su = if flip { fw - 1 - su.min(fw - 1) } else { su.min(fw - 1) };
                let a = atlas.frame_px(f, su, sv.min(fh - 1))[3];
                m.bits[(v * m.w + u) as usize] = a > 0;
            }
        }
        Some(m)
    }

    /// Is world pixel `(px, py)` opaque?
    pub fn get(&self, px: i32, py: i32) -> bool {
        let (u, v) = (px - self.x, py - self.y);
        u >= 0 && v >= 0 && u < self.w && v < self.h && self.bits[(v * self.w + u) as usize]
    }

    pub fn bounds(&self) -> Rect {
        [self.x as f32, self.y as f32, self.w as f32, self.h as f32]
    }

    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }

    /// Opaque pixels as world coordinates.
    pub fn pixels(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (0..self.h).flat_map(move |v| (0..self.w).filter_map(move |u| self.bits[(v * self.w + u) as usize].then_some((self.x + u, self.y + v))))
    }

    /// Grown by `d` pixels in every direction (square neighbourhood).
    pub fn dilate(&self, d: i32) -> Self {
        let (w, h) = (self.w + 2 * d, self.h + 2 * d);
        let mut out = Self { x: self.x - d, y: self.y - d, w, h, bits: vec![false; (w.max(0) * h.max(0)) as usize] };
        for (px, py) in self.pixels() {
            for dy in -d..=d {
                for dx in -d..=d {
                    let (u, v) = (px + dx - out.x, py + dy - out.y);
                    out.bits[(v * w + u) as usize] = true;
                }
            }
        }
        out
    }

    /// Lowest opaque row per column: `(world x, world y of the pixel row)`.
    pub fn bottoms(&self) -> Vec<(i32, i32)> {
        (0..self.w)
            .filter_map(|u| (0..self.h).rev().find(|v| self.bits[(v * self.w + u) as usize]).map(|v| (self.x + u, self.y + v)))
            .collect()
    }
}

/// Bounding box and count of the pixels of `m` where `hit` is true.
pub fn overlap_box(m: &Mask, mut hit: impl FnMut(i32, i32) -> bool) -> Option<(Rect, usize)> {
    let mut b: Option<[i32; 4]> = None;
    let mut n = 0;
    for (x, y) in m.pixels() {
        if hit(x, y) {
            n += 1;
            b = Some(match b {
                None => [x, y, x, y],
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
            });
        }
    }
    b.map(|[x0, y0, x1, y1]| ([x0 as f32, y0 as f32, (x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32], n))
}

/// The opaque mask of an object: its atlas frame when there is one, else its rect.
pub fn object_mask(o: &Object, atlas: Option<&Atlas>) -> Mask {
    match (o.frame, atlas) {
        (Some(f), Some(a)) => Mask::frame(a, f, o.rect, o.flip).unwrap_or_else(|| Mask::rect(o.rect)),
        _ => Mask::rect(o.rect),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_helpers() {
        assert!(overlaps([0.0, 0.0, 10.0, 10.0], [9.0, 9.0, 5.0, 5.0]));
        assert!(!overlaps([0.0, 0.0, 10.0, 10.0], [10.0, 0.0, 5.0, 5.0]));
        assert_eq!(intersect([0.0, 0.0, 10.0, 10.0], [5.0, 5.0, 10.0, 10.0]), Some([5.0, 5.0, 5.0, 5.0]));
        assert_eq!(dist_point_rect([15.0, 5.0], [0.0, 0.0, 10.0, 10.0]), 5.0);
    }

    #[test]
    fn mask_dilate_and_bottoms() {
        let m = Mask::rect([2.0, 2.0, 2.0, 3.0]);
        assert_eq!(m.count(), 6);
        let d = m.dilate(1);
        assert_eq!(d.count(), 4 * 5);
        assert_eq!(m.bottoms(), vec![(2, 4), (3, 4)]);
    }
}
