//! Check 4: pixel integrity. One pixel scale throughout and whole-pixel
//! positions:
//! - every frame uses the scene's `pixel_scale`, a whole number (`frame_scale`);
//! - every drawn sprite with an atlas frame covers exactly its frame size in
//!   art pixels: not stretched (`stretched`), not at another uniform scale
//!   (`mixed_scale`), HUD included (`hud_scale`); flat-colour fill frames
//!   (`atlas.fills`) may stretch;
//! - world sprites sit on whole art pixels, HUD sprites on whole device
//!   pixels (`subpixel`); when the dump has no drawn sprites, placed objects
//!   must have whole-pixel rects instead (joined into one issue).
//!
//! The same problem on the same layer and frame joins into one issue with a
//! count of the frames it shows in. Stretched and other-uniform-scale draws
//! on one layer share a cause (the sprite is scaled when drawn, e.g. a
//! shadow sized by height) and join into one issue listing both symptoms.
//!
//! Limits: sprites without an atlas frame (built-in cells) are checked for
//! position only; rotation is not in the dump.

use super::fmt_px;
use crate::scene::Rect;
use crate::{Check, CropSource, Ctx, Issue};
use std::collections::BTreeMap;

fn whole(v: f32, eps: f32) -> bool {
    (v - v.round()).abs() <= eps
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    kind: &'static str,
    layer: i32,
    frame: Option<usize>,
    sx: i32,
    sy: i32,
}

/// Frames a symptom shows in and its drawn sizes (art px x 10).
type Symptom = (Vec<usize>, std::collections::BTreeSet<(i32, i32)>);

struct Agg {
    frames: Vec<usize>,
    /// Atlas frames and drawn sizes (art px x 10) seen (stretch and scale
    /// issues join per layer).
    atlas_frames: std::collections::BTreeSet<usize>,
    drawn: std::collections::BTreeSet<(i32, i32)>,
    first: (usize, Rect),
    size: (f32, f32, f32, f32),
    /// Per symptom of a scaled sprite (0 stretched, 1 another uniform
    /// scale): the frames it shows in and the drawn sizes (art px x 10).
    symptom: [Symptom; 2],
}

pub fn check(ctx: &Ctx) -> Vec<Issue> {
    let s = ctx.scene;
    let eps = s.limits.pixel_eps.max(1e-4);
    let mut out = Vec::new();
    // Frame scales.
    let bad: Vec<usize> = (0..s.frames.len()).filter(|&i| !whole(s.frames[i].scale, eps) || (s.frames[i].scale - s.pixel_scale).abs() > eps).collect();
    if let Some(&first) = bad.first() {
        let f = &s.frames[first];
        let view = [f.cam[0], f.cam[1], f.view[0], f.view[1]];
        out.push(
            Issue::new(Check::Pixels, "frame_scale", view)
                .object("screen")
                .tick(f.tick)
                .sev(3, bad.len() as f32)
                .msg(
                    format!("{} of {} frames draw at {} device px per art pixel; the game's pixel scale is {}.", bad.len(), s.frames.len(), fmt_px(f.scale), fmt_px(s.pixel_scale)),
                    "Use one whole-number scale for every frame (round the fit to a whole number and letterbox the rest).".into(),
                )
                .crop(CropSource::Frame(first, None), vec![]),
        );
    }
    // Sprites.
    let mut agg: BTreeMap<Key, Agg> = BTreeMap::new();
    let mut sprite_frames = 0;
    for (fi, f) in s.frames.iter().enumerate() {
        let Some(sprites) = &f.sprites else { continue };
        sprite_frames += 1;
        let k = f.scale.max(1e-3);
        for sp in sprites {
            let hud = sp.l >= s.layers.hud_min;
            let w = sp.w.abs();
            // World rect of the sprite (HUD: where it sits over the world).
            let world = [f.cam[0] + sp.x / k, f.cam[1] + sp.y / k, w / k, sp.h / k];
            let mut note = |kind: &'static str, sx: f32, sy: f32, size: (f32, f32, f32, f32)| {
                // Stretch and scale problems join per layer into one issue (one
                // cause, e.g. a shadow scaled with height, shows stretched at
                // many sizes and, at some heights, at exactly half size).
                let sym = match kind {
                    "stretched" => Some(0),
                    "mixed_scale" => Some(1),
                    _ => None,
                };
                let key = if sym.is_some() {
                    Key { kind: "scaled", layer: sp.l, frame: None, sx: 0, sy: 0 }
                } else {
                    Key { kind, layer: sp.l, frame: sp.f, sx: (sx * 100.0).round() as i32, sy: (sy * 100.0).round() as i32 }
                };
                let a = agg.entry(key).or_insert(Agg { frames: Vec::new(), atlas_frames: Default::default(), drawn: Default::default(), first: (fi, world), size, symptom: Default::default() });
                if a.frames.last() != Some(&fi) {
                    a.frames.push(fi);
                }
                if let Some(f) = sp.f {
                    a.atlas_frames.insert(f);
                }
                let d = ((size.2 * 10.0).round() as i32, (size.3 * 10.0).round() as i32);
                a.drawn.insert(d);
                if let Some(i) = sym {
                    let (fr, dr) = &mut a.symptom[i];
                    if fr.last() != Some(&fi) {
                        fr.push(fi);
                    }
                    dr.insert(d);
                }
            };
            let pos_ok = if hud { whole(sp.x, eps) && whole(sp.y, eps) } else { whole(sp.x / k, eps) && whole(sp.y / k, eps) };
            if !pos_ok {
                note("subpixel", 1.0, 1.0, (sp.x / k, sp.y / k, 0.0, 0.0));
            }
            let (Some(fid), Some(atlas)) = (sp.f, ctx.atlas) else { continue };
            if atlas.is_fill(fid) {
                continue;
            }
            let Some([fw, fh]) = atlas.frame_size(fid) else { continue };
            if fw == 0 || fh == 0 {
                continue;
            }
            let sx = w / (fw as f32 * k);
            let sy = sp.h / (fh as f32 * k);
            let size = (fw as f32, fh as f32, w / k, sp.h / k);
            if hud {
                if (sx - 1.0).abs() > eps || (sy - 1.0).abs() > eps {
                    note("hud_scale", sx, sy, size);
                }
            } else if (sx - sy).abs() > eps {
                note("stretched", sx, sy, size);
            } else if (sx - 1.0).abs() > eps {
                note("mixed_scale", sx, sy, size);
            }
        }
    }
    // HUD scale issues join across frames into one per layer.
    let mut hud_seen: BTreeMap<i32, usize> = BTreeMap::new();
    for (key, a) in &agg {
        let (fi, rect) = a.first;
        let fr = &s.frames[fi];
        let lname = ctx.layer_name(key.layer);
        let fname = match (key.frame, a.atlas_frames.len()) {
            (Some(f), _) => format!("frame {f}"),
            (None, 0) => "a built-in cell".to_string(),
            (None, 1) => format!("frame {}", a.atlas_frames.iter().next().copied().unwrap_or(0)),
            (None, _) => format!("frames {}", a.atlas_frames.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(", ")),
        };
        let list = |drawn: &std::collections::BTreeSet<(i32, i32)>| {
            let v: Vec<String> = drawn.iter().take(6).map(|&(w, h)| format!("{} x {}", fmt_px(w as f32 / 10.0), fmt_px(h as f32 / 10.0))).collect();
            let more = if drawn.len() > v.len() { format!(" and {} more", drawn.len() - v.len()) } else { String::new() };
            format!("{}{more}", v.join(", "))
        };
        let sizes = list(&a.drawn);
        let count = format!("{} of {sprite_frames} sampled frames (first at tick {})", a.frames.len(), fr.tick);
        let sx = key.sx as f32 / 100.0;
        // A scaled sprite reports as "stretched" when any draw is stretched
        // (both symptoms listed), else as "mixed_scale".
        let kind = match key.kind {
            "scaled" if a.symptom[0].0.is_empty() => "mixed_scale",
            "scaled" => "stretched",
            k => k,
        };
        let symptoms = || {
            let (sf, sd) = &a.symptom[0];
            let (uf, ud) = &a.symptom[1];
            format!(
                "stretched (pixels not square) at {} in {} frame(s), and at another uniform scale at {} in {} frame(s); {} of {sprite_frames} sampled frames in all (first at tick {})",
                list(sd),
                sf.len(),
                list(ud),
                uf.len(),
                a.frames.len(),
                fr.tick
            )
        };
        let (msg, fix, sev) = match kind {
            "subpixel" => (
                format!("{lname} {fname} sits off the pixel grid (at x {}, y {} art px) in {count}.", fmt_px(a.size.0), fmt_px(a.size.1)),
                "Round its position to whole art pixels before drawing (snap after the camera offset).".to_string(),
                2,
            ),
            "stretched" if !a.symptom[1].0.is_empty() => (
                format!("{lname} {fname} ({} x {} art px) is drawn scaled instead of 1:1: {}. One cause: the sprite is scaled when it is drawn.", fmt_px(a.size.0), fmt_px(a.size.1), symptoms()),
                "Draw it at 1:1; make size, squash or stretch changes as extra frames drawn at the art scale instead of scaling the sprite.".to_string(),
                2,
            ),
            "stretched" => (
                format!(
                    "{lname} {fname} ({} x {} art px) is drawn stretched, so its pixels are not square: at {} ({} drawn size(s)), in {count}.",
                    fmt_px(a.size.0),
                    fmt_px(a.size.1),
                    sizes,
                    a.drawn.len()
                ),
                "Draw it at 1:1; make squash, stretch or size changes as extra frames drawn at the art scale instead of scaling the sprite.".to_string(),
                2,
            ),
            "mixed_scale" => (
                format!("{lname} {fname} is drawn at another uniform scale ({sizes} instead of {} x {}) in {count}.", fmt_px(a.size.0), fmt_px(a.size.1)),
                "Draw it at the art scale (1 art pixel = 1 sheet pixel), or redraw the art at the size it is shown.".to_string(),
                2,
            ),
            _ => {
                let n = hud_seen.entry(key.layer).or_insert(0);
                *n += 1;
                if *n > 1 {
                    continue;
                }
                let device = sx * fr.scale;
                (
                    format!(
                        "{lname} pixels are {} device px while the world's are {} ({sx:.2}x the art pixel), in {count}: two pixel sizes on screen.",
                        fmt_px(device),
                        fmt_px(fr.scale)
                    ),
                    format!("Draw the {lname} at the art pixel scale ({} device px), or tag it if a different UI pixel is intended.", fmt_px(fr.scale)),
                    2,
                )
            }
        };
        out.push(
            Issue::new(Check::Pixels, kind, rect)
                .object(&format!("{lname} {fname}"))
                .tick(fr.tick)
                .sev(sev, a.frames.len() as f32)
                .msg(msg, fix)
                .crop(CropSource::Frame(fi, Some(crate::geom::inflate(rect, 24.0))), vec![rect]),
        );
    }
    // Placed objects: only when the dump has no drawn sprites. With sprites,
    // what is drawn is the truth (a game may keep fractional hit boxes and
    // snap at draw time; the sprite check above catches a missing snap).
    if sprite_frames == 0 {
        let off: Vec<_> = s.objects.iter().filter(|o| !whole(o.rect[0], eps) || !whole(o.rect[1], eps)).collect();
        if let Some(o) = off.first() {
            let names: Vec<&str> = off.iter().take(6).map(|o| o.name.as_str()).collect();
            let more = if off.len() > names.len() { format!(" and {} more", off.len() - names.len()) } else { String::new() };
            out.push(
                Issue::new(Check::Pixels, "subpixel", o.rect)
                    .object(&o.name)
                    .tile(ctx.tile_of([o.rect[0], o.rect[1]]))
                    .sev(2, off.len() as f32)
                    .msg(
                        format!(
                            "{} placed object(s) are not on whole art pixels ({}{more}); first: {} at x {:.2}, y {:.2}.",
                            off.len(),
                            names.join(", "),
                            o.name,
                            o.rect[0],
                            o.rect[1]
                        ),
                        format!("Round placements to whole art pixels (e.g. {} to x {}, y {}), or snap sprites to the pixel grid when drawing.", o.name, o.rect[0].round(), o.rect[1].round()),
                    )
                    .crop(CropSource::Level(crate::geom::inflate(o.rect, 24.0)), vec![o.rect]),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Atlas, FrameDef, Scene, SpriteDef};

    fn scene(sprites: Vec<SpriteDef>) -> (Scene, Atlas) {
        let mut s = Scene { pixel_scale: 4.0, map: [10, 10], ..Scene::default() };
        s.layers.hud_min = 200;
        s.frames.push(FrameDef { tick: 7, scale: 4.0, cam: [0.0, 0.0], view: [100.0, 60.0], sprites: Some(sprites), ..FrameDef::default() });
        let atlas = Atlas { width: 32, height: 32, rgba: vec![255; 32 * 32 * 4], frames: vec![[0, 0, 10, 16], [10, 0, 4, 4], [14, 0, 6, 7]], fills: vec![1] };
        (s, atlas)
    }

    fn sp(f: usize, x: f32, y: f32, w: f32, h: f32, l: i32) -> SpriteDef {
        SpriteDef { f: Some(f), x, y, w, h, l, ..SpriteDef::default() }
    }

    #[test]
    fn one_to_one_whole_pixel_sprites_pass() {
        let (s, a) = scene(vec![
            sp(0, 40.0, 80.0, 40.0, 64.0, 90),
            sp(0, 40.0, 80.0, -40.0, 64.0, 90), // mirrored
            sp(1, 0.0, 0.0, 400.0, 100.0, 0),   // a fill may stretch
            sp(2, 8.0, 8.0, 24.0, 28.0, 200),   // HUD at the art scale
        ]);
        let v = check(&Ctx::new(&s, Some(&a)));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn stretched_mixed_hud_and_subpixel_sprites_fail() {
        let (mut s, a) = scene(vec![
            sp(0, 40.0, 80.0, 48.0, 56.0, 90),  // squash: 12 x 14 from 10 x 16
            sp(0, 40.0, 80.0, 80.0, 128.0, 80), // 2x
            sp(2, 8.0, 8.0, 18.0, 21.0, 200),   // HUD at 3 device px
            sp(0, 42.0, 80.0, 40.0, 64.0, 70),  // half an art pixel off
        ]);
        s.objects.push(crate::checks::testkit::obj("coin", crate::scene::Kind::Collectible, [10.5, 20.0, 8.0, 8.0]));
        let v = check(&Ctx::new(&s, Some(&a)));
        let kinds: Vec<&str> = v.iter().map(|i| i.kind.as_str()).collect();
        for k in ["stretched", "mixed_scale", "hud_scale", "subpixel"] {
            assert!(kinds.contains(&k), "{k} in {kinds:?}");
        }
        // Only the drawn sprite: with sprites in the dump the off-grid hit box
        // of the coin is not judged (the draw is the truth).
        assert_eq!(kinds.iter().filter(|k| **k == "subpixel").count(), 1);
        // A frame at another scale.
        s.frames[0].scale = 4.5;
        assert!(check(&Ctx::new(&s, Some(&a))).iter().any(|i| i.kind == "frame_scale"));
    }

    #[test]
    fn stretched_and_half_size_draws_on_one_layer_are_one_issue() {
        // A shadow scaled with height: stretched in one frame, exactly half
        // size in another.
        let (mut s, a) = scene(vec![sp(2, 40.0, 80.0, 16.0, 16.0, 10)]); // 4 x 4 art px from 6 x 7
        let mut f2 = s.frames[0].clone();
        f2.tick = 13;
        f2.sprites = Some(vec![sp(2, 40.0, 80.0, 12.0, 14.0, 10)]); // 3 x 3.5: half size
        s.frames.push(f2);
        let v = check(&Ctx::new(&s, Some(&a)));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].kind, "stretched");
        assert!(v[0].message.contains("stretched (pixels not square) at 4 x 4 in 1 frame(s)"), "{}", v[0].message);
        assert!(v[0].message.contains("another uniform scale at 3 x 3.5 in 1 frame(s)"), "{}", v[0].message);
        assert!(v[0].message.contains("2 of 2 sampled frames"), "{}", v[0].message);
        // Only the half-size draw: a mixed_scale issue on its own.
        s.frames.remove(0);
        let v = check(&Ctx::new(&s, Some(&a)));
        assert_eq!(v.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), vec!["mixed_scale"]);
    }

    #[test]
    fn without_drawn_sprites_off_grid_placements_join_into_one_issue() {
        let mut s = Scene { pixel_scale: 4.0, map: [10, 10], ..Scene::default() };
        for i in 0..3 {
            s.objects.push(crate::checks::testkit::obj(&format!("coin {i}"), crate::scene::Kind::Collectible, [10.0 + 16.0 * i as f32, 20.5, 8.0, 8.0]));
        }
        s.objects.push(crate::checks::testkit::obj("coin 9", crate::scene::Kind::Collectible, [90.0, 20.0, 8.0, 8.0]));
        let v = check(&Ctx::new(&s, None));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.starts_with("3 placed object(s)"), "{}", v[0].message);
    }
}
