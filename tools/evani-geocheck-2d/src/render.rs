//! Crop images: the level (tiles and objects from the atlas) or a drawn
//! frame (its sprites, as the game drew them) around a world rect,
//! nearest-neighbour upscaled, with the problem outlined in magenta.
//! One-pixel highlight rects (a real jump arc) are drawn as cyan dots.
//! Without an atlas, tiles and objects are flat colours by kind.

use crate::geom::{intersect, overlaps};
use crate::scene::{Atlas, Kind, Rect, Scene, Shape};
use crate::{CropSource, Issue};

const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
/// Widest crop written (output pixels).
const MAX_W: f32 = 1280.0;
/// Frame crops narrower than this are scaled up by a whole factor.
const MIN_W: f32 = 480.0;

fn srgb_to_lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn lin_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// An RGBA8 image over a world rect at `ppa` output pixels per art pixel.
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    x0: f32,
    y0: f32,
    ppa: f32,
    lut: Vec<f32>,
}

impl Canvas {
    pub fn new(world: Rect, ppa: f32, bg: [u8; 3]) -> Self {
        let w = (world[2] * ppa).round().max(1.0) as u32;
        let h = (world[3] * ppa).round().max(1.0) as u32;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[bg[0], bg[1], bg[2], 255]);
        }
        Self { w, h, rgba, x0: world[0], y0: world[1], ppa, lut: (0..=255u8).map(srgb_to_lin).collect() }
    }

    fn blend(&mut self, u: u32, v: u32, c: [u8; 4]) {
        if c[3] == 0 {
            return;
        }
        let o = ((v * self.w + u) * 4) as usize;
        let a = c[3] as u32;
        for (k, &ck) in c.iter().take(3).enumerate() {
            let d = self.rgba[o + k] as u32;
            self.rgba[o + k] = ((ck as u32 * a + d * (255 - a)) / 255) as u8;
        }
    }

    /// Output pixel span covering world rect `r` (clipped).
    fn span(&self, r: Rect) -> (u32, u32, u32, u32) {
        let u0 = ((r[0] - self.x0) * self.ppa).floor().max(0.0) as u32;
        let v0 = ((r[1] - self.y0) * self.ppa).floor().max(0.0) as u32;
        let u1 = (((r[0] + r[2] - self.x0) * self.ppa).ceil().max(0.0) as u32).min(self.w);
        let v1 = (((r[1] + r[3] - self.y0) * self.ppa).ceil().max(0.0) as u32).min(self.h);
        (u0, v0, u1, v1)
    }

    /// Draw atlas frame `f` (or a flat `fallback` colour) into world rect
    /// `r`, mirrored when `flip`, multiplied by the linear `tint`.
    pub fn quad(&mut self, atlas: Option<&Atlas>, f: Option<usize>, r: Rect, flip: bool, tint: [f32; 4], fallback: [u8; 4]) {
        if r[2] <= 0.0 || r[3] <= 0.0 {
            return;
        }
        let plain = tint == [1.0; 4];
        let src = match (atlas, f) {
            (Some(a), Some(f)) => a.frame_size(f).filter(|s| s[0] > 0 && s[1] > 0).map(|s| (a, f, s)),
            _ => None,
        };
        let flat = if src.is_none() && f.is_none() && atlas.is_some() {
            // A built-in white cell: the tint is the colour.
            [lin_to_srgb(tint[0]), lin_to_srgb(tint[1]), lin_to_srgb(tint[2]), (tint[3].clamp(0.0, 1.0) * 255.0).round() as u8]
        } else {
            fallback
        };
        let (u0, v0, u1, v1) = self.span(r);
        for v in v0..v1 {
            let wy = self.y0 + (v as f32 + 0.5) / self.ppa;
            let fy = (wy - r[1]) / r[3];
            if !(0.0..1.0).contains(&fy) {
                continue;
            }
            for u in u0..u1 {
                let wx = self.x0 + (u as f32 + 0.5) / self.ppa;
                let fx = (wx - r[0]) / r[2];
                if !(0.0..1.0).contains(&fx) {
                    continue;
                }
                let c = match src {
                    Some((a, f, [fw, fh])) => {
                        let su = ((fx * fw as f32) as u32).min(fw - 1);
                        let su = if flip { fw - 1 - su } else { su };
                        let sv = ((fy * fh as f32) as u32).min(fh - 1);
                        let p = a.frame_px(f, su, sv);
                        if plain {
                            p
                        } else {
                            [
                                lin_to_srgb(self.lut[p[0] as usize] * tint[0]),
                                lin_to_srgb(self.lut[p[1] as usize] * tint[1]),
                                lin_to_srgb(self.lut[p[2] as usize] * tint[2]),
                                (p[3] as f32 * tint[3].clamp(0.0, 1.0)).round() as u8,
                            ]
                        }
                    }
                    None => flat,
                };
                self.blend(u, v, c);
            }
        }
    }

    /// Outline world rect `r`, `t` output pixels thick; tiny rects become dots.
    pub fn outline(&mut self, r: Rect, c: [u8; 4], t: u32) {
        if r[2] <= 1.0 && r[3] <= 1.0 {
            let (u0, v0, u1, v1) = self.span(r);
            for v in v0..v1.max(v0 + 1).min(self.h) {
                for u in u0..u1.max(u0 + 1).min(self.w) {
                    self.blend(u, v, CYAN);
                }
            }
            return;
        }
        let (u0, v0, u1, v1) = self.span(r);
        if u1 <= u0 || v1 <= v0 {
            return;
        }
        for v in v0..v1 {
            for u in u0..u1 {
                let edge = u < u0 + t || u + t >= u1 || v < v0 + t || v + t >= v1;
                if edge {
                    self.blend(u, v, c);
                }
            }
        }
    }
}

fn bg(scene: &Scene) -> [u8; 3] {
    let parse = |s: &str| -> Option<[u8; 3]> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 {
            return None;
        }
        let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some([b(0)?, b(2)?, b(4)?])
    };
    scene.background.as_deref().and_then(parse).unwrap_or([40, 44, 52])
}

fn kind_colour(k: Kind) -> [u8; 4] {
    match k {
        Kind::Collectible => [250, 210, 40, 230],
        Kind::Enemy => [210, 50, 50, 230],
        Kind::Prop => [70, 130, 220, 230],
        Kind::Decor => [60, 160, 70, 200],
        Kind::Hazard => [240, 80, 40, 230],
        Kind::Platform => [150, 100, 60, 255],
        Kind::Goal => [160, 80, 200, 230],
        Kind::Player => [255, 255, 255, 230],
        Kind::Other => [180, 180, 180, 200],
    }
}

/// Paint the static level (tiles and objects in layer order) over the canvas.
pub fn paint_level(cv: &mut Canvas, scene: &Scene, atlas: Option<&Atlas>, world: Rect) {
    enum Item {
        Tile(usize),
        Obj(usize),
    }
    let ts = scene.tile_size;
    let mut items: Vec<(i32, u8, Item)> = Vec::new();
    for (i, t) in scene.tiles.iter().enumerate() {
        if overlaps([t.x as f32 * ts, t.y as f32 * ts, ts, ts], world) {
            items.push((t.layer, 0, Item::Tile(i)));
        }
    }
    for (i, o) in scene.objects.iter().enumerate() {
        if overlaps(o.rect, world) {
            items.push((o.layer, 1, Item::Obj(i)));
        }
    }
    items.sort_by_key(|(l, k, _)| (*l, *k));
    for (_, _, it) in items {
        match it {
            Item::Tile(i) => {
                let t = &scene.tiles[i];
                let r = [t.x as f32 * ts, t.y as f32 * ts, ts, ts];
                let fallback = match (t.shape, t.hazard) {
                    (_, true) => [220, 60, 60, 200],
                    (Shape::Full, _) | (Shape::Slope, _) => [120, 90, 60, 255],
                    (Shape::Top, _) => [150, 110, 70, 255],
                    (Shape::None, _) => [0, 0, 0, 0],
                };
                let a = if t.frame.is_some() { atlas } else { None };
                cv.quad(a, t.frame, r, false, [1.0; 4], fallback);
            }
            Item::Obj(i) => {
                let o = &scene.objects[i];
                let a = if o.frame.is_some() { atlas } else { None };
                cv.quad(a, o.frame, o.rect, o.flip, [1.0; 4], kind_colour(o.kind));
            }
        }
    }
}

/// Render one issue's crop.
pub fn crop(scene: &Scene, atlas: Option<&Atlas>, issue: &Issue) -> Canvas {
    let bgc = bg(scene);
    let mut cv = match &issue.crop {
        CropSource::Level(r) => {
            let ppa = (560.0 / r[2].max(1.0)).round().clamp(2.0, 8.0);
            let mut cv = Canvas::new(*r, ppa, bgc);
            paint_level(&mut cv, scene, atlas, *r);
            cv
        }
        CropSource::Frame(_, sub) | CropSource::Sweep(_, sub) => {
            let f = match &issue.crop {
                CropSource::Sweep(fi, _) => scene.sweep.get(*fi),
                CropSource::Frame(fi, _) => scene.frames.get(*fi),
                CropSource::Level(_) => None,
            };
            let Some(f) = f else {
                let r = issue.px;
                let mut cv = Canvas::new(r, 4.0, bgc);
                paint_level(&mut cv, scene, atlas, r);
                return cv;
            };
            let view = [f.cam[0], f.cam[1], f.view[0], f.view[1]];
            let region = sub.and_then(|s| intersect(s, view)).unwrap_or(view);
            let k = f.scale.max(1.0);
            let ppa = k / (region[2] * k / MAX_W).ceil().max(1.0);
            // Small crops (a one-pixel join, a lone sprite) are blown up by a
            // whole factor, nearest-neighbour, so the flagged pixels read at
            // normal viewing size with hard edges.
            let ppa = ppa * (MIN_W / (region[2] * ppa)).floor().clamp(1.0, 8.0);
            let mut cv = Canvas::new(region, ppa, bgc);
            match &f.sprites {
                Some(sprites) => {
                    let mut order: Vec<usize> = (0..sprites.len()).collect();
                    order.sort_by_key(|&i| sprites[i].l);
                    for i in order {
                        let sp = &sprites[i];
                        let r = [f.cam[0] + sp.x / k, f.cam[1] + sp.y / k, sp.w.abs() / k, sp.h / k];
                        let fallback = [lin_to_srgb(sp.c[0]), lin_to_srgb(sp.c[1]), lin_to_srgb(sp.c[2]), (sp.c[3].clamp(0.0, 1.0) * 255.0) as u8];
                        cv.quad(atlas, sp.f, r, sp.w < 0.0, sp.c, fallback);
                    }
                }
                None => {
                    paint_level(&mut cv, scene, atlas, region);
                    cv.quad(None, None, f.player, false, [1.0; 4], [255, 255, 255, 140]);
                }
            }
            cv
        }
    };
    let t = (cv.ppa / 2.0).round().clamp(2.0, 4.0) as u32;
    for h in &issue.highlights {
        cv.outline(*h, MAGENTA, t);
    }
    cv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Check;
    use crate::checks::testkit::{ground, obj};

    #[test]
    fn level_crops_draw_tiles_objects_and_a_magenta_outline() {
        let mut s = ground(8, 6, 4);
        s.objects.push(obj("coin", Kind::Collectible, [40.0, 40.0, 8.0, 8.0]));
        let issue = Issue::new(Check::Overlaps, "clear_air", [40.0, 40.0, 8.0, 8.0]).crop(CropSource::Level([16.0, 16.0, 64.0, 64.0]), vec![[40.0, 40.0, 8.0, 8.0]]);
        let cv = crop(&s, None, &issue);
        assert_eq!((cv.w, cv.h), (512, 512)); // 8x
        let px = |u: u32, v: u32| {
            let o = ((v * cv.w + u) * 4) as usize;
            [cv.rgba[o], cv.rgba[o + 1], cv.rgba[o + 2]]
        };
        assert_eq!(px(8 * 24, 8 * 24), [255, 0, 255]); // the outline corner
        let coin = px(8 * 28, 8 * 28); // the coin, inside (blended at alpha 230)
        assert!(coin[0] > 200 && coin[1] > 180 && coin[2] < 80, "{coin:?}");
        assert_eq!(px(8, 8 * 60), [120, 90, 60]); // ground
    }
}
