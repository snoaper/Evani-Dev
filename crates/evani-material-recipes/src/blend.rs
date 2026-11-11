//! The see-through pass on the CPU (batch 3 build plan 3.5, B3-P3): which
//! blocks draw blended and how ([`mode_of`]), and the far-to-near order the
//! renderer draws them in ([`sort_far_to_near`]). evani-render's
//! `material-recipes-blend` reads both; with it off no game calls them, so no
//! game downloads them.
//!
//! Two blend states only, as most engines ship (ideas from Godot and
//! PlayCanvas, MIT; the mode list's shape from Bevy's alpha modes, MIT or
//! Apache-2.0; all written fresh here):
//!
//! - [`OVER`], premultiplied alpha: glass, smoke and steam. The stage
//!   premultiplies the surface colour before lighting, so a pane's
//!   reflections are not dimmed by its low alpha.
//! - [`ADD`], additive: fire and holograms (the same blend state with alpha 0,
//!   so it needs no second pipeline and no sort).
//! - [`REFRACT`]: frosted, wavy and block glass, drawn after the copy of the
//!   opaque colour (feature `material-recipes-refract`), which they bend and
//!   blur; without the copy they draw as [`OVER`].

use crate::block::Block;

/// Drawn opaque (the model pass as before).
pub const OPAQUE: u8 = 0;
/// Premultiplied alpha over what is behind (glass, smoke, steam).
pub const OVER: u8 = 1;
/// Added to what is behind (fire, holograms).
pub const ADD: u8 = 2;
/// Bends and blurs a copy of what is behind (frosted, wavy and block glass).
pub const REFRACT: u8 = 3;

/// The recipe codes the modes read.
const ORGANIC: u8 = 22;
const GLASS: u8 = 24;
const GEM: u8 = 25;
const FIRE: u8 = 28;
const GLOW: u8 = 29;

/// The blend mode of block bytes 0 to 15 (`recipe`, `variant` and the first
/// numbers): glass draws [`OVER`], or [`REFRACT`] when it is a window or block
/// with frost (n1 at least 0.1) or waviness (n2 at least 0.2), or a glass
/// block; fire's flame [`ADD`], its smoke and steam [`OVER`] (embers and lava
/// stay opaque); glow's hologram [`ADD`]; a gem or an organic material with
/// its `clear` extra on (material-view round 4: clear quartz, slime)
/// [`OVER`]. Everything else [`OPAQUE`].
pub fn mode_of(b: &[u8; 32]) -> u8 {
    if clear(b) {
        return OVER;
    }
    match (b[0], b[1]) {
        (GLASS, 6) => REFRACT,
        (GLASS, 0) if b[14] >= 26 || b[15] >= 51 => REFRACT,
        (GLASS, _) => OVER,
        (FIRE, 0) | (GLOW, 4) => ADD,
        (FIRE, 3 | 4) => OVER,
        _ => OPAQUE,
    }
}

/// A gem (extras bit 2) or an organic material (bit 1) with its `clear`
/// extra on: a see-through body over what is behind.
fn clear(b: &[u8; 32]) -> bool {
    (b[0] == GEM && b[2] & 4 != 0) || (b[0] == ORGANIC && b[2] & 2 != 0)
}

/// [`mode_of`] of a block.
pub fn mode(block: &Block) -> u8 {
    mode_of(&block.0)
}

/// [`mode_of`] of a material's block words (binding 12's eight words, little
/// endian), as the renderer holds them.
pub fn mode_of_words(w: &[u32; 8]) -> u8 {
    let mut b = [0u8; 32];
    for (i, word) in w.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    mode_of(&b)
}

/// One see-through draw to order: its sort priority (the material's
/// `extras.evani_sort`, lower first), its squared distance from the camera
/// and a stable id (model, part, instance).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SortKey {
    pub priority: i32,
    pub dist2: f32,
    pub id: (u32, u32, u32),
}

/// Order see-through draws for drawing: priority first (lower draws first, so
/// a game can put a pane behind a smoke card by hand), then far to near, then
/// by id, so equal distances never flicker between frames. The order is total
/// (ids differ), so any correct sort gives this one; a small heap sort keeps
/// the standard library's stable sort (about 4 KB of code) out of the download.
pub fn sort_far_to_near(keys: &mut [SortKey]) {
    let n = keys.len();
    for i in (0..n / 2).rev() {
        sift(keys, i, n);
    }
    for end in (1..n).rev() {
        keys.swap(0, end);
        sift(keys, 0, end);
    }
}

/// `a` draws before `b`.
fn before(a: &SortKey, b: &SortKey) -> bool {
    a.priority.cmp(&b.priority).then(b.dist2.total_cmp(&a.dist2)).then(a.id.cmp(&b.id)).is_lt()
}

/// Sift the heap's root `r` down within `keys[..n]` (the last draw on top).
fn sift(keys: &mut [SortKey], mut r: usize, n: usize) {
    loop {
        let mut c = 2 * r + 1;
        if c >= n {
            return;
        }
        if c + 1 < n && before(&keys[c], &keys[c + 1]) {
            c += 1;
        }
        if !before(&keys[r], &keys[c]) {
            return;
        }
        keys.swap(r, c);
        r = c;
    }
}

/// How much of the sun a see-through block's shadow blocks, 0 to 1
/// (`shaders/blend_shadow.wgsl`'s `rc_blend_density`, line for line), or
/// `None` for a block that is not see-through (the cut-out's holes or a solid
/// shadow decide). Glass blocks one minus what comes through (its body's
/// alpha, its tint's brightness to the thickness's power), smoke and steam a
/// third, flames and holograms nothing, a clear gem 0.4 and clear slime 0.6.
pub fn shadow_density(b: &[u8; 32]) -> Option<f32> {
    let (r, v) = (b[0], b[1]);
    if (r == FIRE && v == 0) || (r == GLOW && v == 4) {
        return Some(0.0);
    }
    if r == FIRE && (v == 3 || v == 4) {
        return Some(0.33);
    }
    if clear(b) {
        return Some(if r == GEM { 0.4 } else { 0.6 });
    }
    if r != GLASS {
        return None;
    }
    let u = |i: usize| f32::from(b[i]) / 255.0;
    let (th, frost) = (u(13), u(14));
    let a = match v {
        1 => 0.5,
        3 => 0.55 + 0.25 * th,
        6 => 0.05 + 0.08 * th + 0.35 * frost + 0.3,
        _ => 0.05 + 0.08 * th + 0.35 * frost,
    };
    let lum = 0.2126 * u(4) * u(4) + 0.7152 * u(5) * u(5) + 0.0722 * u(6) * u(6);
    Some(1.0 - (1.0 - a.clamp(0.0, 1.0)) * lum.max(1e-4).powf(0.5 + 2.0 * th))
}

/// The stipple's threshold at shadow-map texel `(x, y)` (`rc_bs_th`: a 4x4
/// ordered pattern, so any 4x4 block keeps a density's share of texels).
pub fn stipple_threshold(x: u32, y: u32) -> f32 {
    const M: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
    (M[((y & 3) * 4 + (x & 3)) as usize] + 0.5) / 16.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glass block: variant, tint, thickness and frost.
    fn glass(variant: u8, tint: [u8; 3], th: u8, frost: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = 24;
        b[1] = variant;
        b[4..7].copy_from_slice(&tint);
        b[13] = th;
        b[14] = frost;
        b
    }

    /// The share of the sun a stippled shadow lets through, over a 16 x 16
    /// patch of the shadow map filtered as the engine's PCF does (an even
    /// average): what is not dropped blocks.
    fn light_through(b: &[u8; 32]) -> f32 {
        let d = shadow_density(b).unwrap_or(1.0);
        let mut kept = 0;
        for y in 0..16 {
            for x in 0..16 {
                if d >= stipple_threshold(x, y) {
                    kept += 1;
                }
            }
        }
        1.0 - kept as f32 / 256.0
    }

    #[test]
    fn glass_shadows_let_light_through() {
        let clear = glass(0, [230, 244, 240], 26, 0);
        let frosted = glass(0, [238, 244, 244], 64, 178);
        let red = glass(3, [176, 16, 32], 102, 0);
        let (c, f, r) = (light_through(&clear), light_through(&frosted), light_through(&red));
        assert!(c > 0.6, "a clear pane lets most of the sun through: {c}");
        assert!(f < c && f > 0.15, "frost milks it: {f}");
        assert!(r < 0.15, "deep red stained glass blocks most: {r}");
        // The stipple keeps the density's share to within one level in 16.
        for b in [clear, frosted, red] {
            let d = shadow_density(&b).unwrap_or(0.0);
            assert!((1.0 - light_through(&b) - d).abs() <= 1.0 / 16.0 + 1e-6);
        }
        let mut flame = [0u8; 32];
        flame[0] = 28;
        assert_eq!(light_through(&flame), 1.0, "a flame casts no shadow");
        assert_eq!(shadow_density(&[0; 32]), None, "a solid block is the cut-out's or solid");
    }

    #[test]
    fn the_threshold_covers_every_level_once() {
        let mut t: Vec<f32> = (0..16).map(|k| stipple_threshold(k % 4, k / 4)).collect();
        t.sort_by(f32::total_cmp);
        for (k, v) in t.iter().enumerate() {
            assert!((v - (k as f32 + 0.5) / 16.0).abs() < 1e-6);
        }
    }

    #[test]
    fn the_shader_computes_the_table_from_bits() {
        // blend_shadow.wgsl's rc_bs_th: no table, the bits of x ^ y and y.
        for y in 0u32..4 {
            for x in 0u32..4 {
                let d = x ^ y;
                let m = (d & 1) * 8 + (y & 1) * 4 + (d & 2) + (y >> 1);
                assert_eq!((m as f32 + 0.5) / 16.0, stipple_threshold(x, y), "({x}, {y})");
            }
        }
    }

    fn block(recipe: u8, variant: u8, frost: u8, wavy: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = recipe;
        b[1] = variant;
        b[14] = frost;
        b[15] = wavy;
        b
    }

    #[test]
    fn modes_follow_recipe_and_variant() {
        assert_eq!(mode_of(&block(24, 0, 0, 13)), OVER, "clear window, a touch of wave");
        assert_eq!(mode_of(&block(24, 0, 128, 0)), REFRACT, "frosted");
        assert_eq!(mode_of(&block(24, 0, 0, 80)), REFRACT, "wavy");
        assert_eq!(mode_of(&block(24, 3, 0, 0)), OVER, "stained");
        assert_eq!(mode_of(&block(24, 1, 0, 200)), OVER, "bottles never refract");
        assert_eq!(mode_of(&block(24, 6, 0, 0)), REFRACT, "glass block");
        assert_eq!(mode_of(&block(28, 0, 0, 0)), ADD, "flame");
        assert_eq!(mode_of(&block(28, 3, 0, 0)), OVER, "smoke");
        assert_eq!(mode_of(&block(28, 4, 0, 0)), OVER, "steam");
        assert_eq!(mode_of(&block(28, 1, 0, 0)), OPAQUE, "embers");
        assert_eq!(mode_of(&block(29, 4, 0, 0)), ADD, "hologram");
        assert_eq!(mode_of(&block(29, 0, 0, 0)), OPAQUE, "lamp");
        assert_eq!(mode_of(&block(1, 0, 0, 0)), OPAQUE);
        assert_eq!(mode_of(&[0; 32]), OPAQUE);
        // The clear extra (material-view round 4): only its own bit, only on
        // its own recipe.
        let with = |r: u8, v: u8, x: u8| {
            let mut b = block(r, v, 0, 0);
            b[2] = x;
            b
        };
        assert_eq!(mode_of(&with(25, 3, 4 | 1)), OVER, "clear quartz");
        assert_eq!(mode_of(&with(25, 0, 1 | 2)), OPAQUE, "a ruby without clear");
        assert_eq!(mode_of(&with(22, 3, 2 | 1)), OVER, "clear slime");
        assert_eq!(mode_of(&with(22, 3, 1)), OPAQUE, "beans in sauce");
        assert_eq!(mode_of(&with(21, 5, 2)), OPAQUE, "a wallpaper's motif bit is not clear");
        assert_eq!(shadow_density(&with(25, 3, 4)), Some(0.4));
        assert_eq!(shadow_density(&with(22, 3, 2)), Some(0.6));
        assert_eq!(shadow_density(&with(25, 3, 1)), None);
    }

    #[test]
    fn words_read_as_bytes() {
        let b = block(24, 0, 128, 0);
        let mut w = [0u32; 8];
        for (i, c) in b.chunks(4).enumerate() {
            w[i] = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        assert_eq!(mode_of_words(&w), REFRACT);
    }

    #[test]
    fn far_draws_first_and_ties_keep_their_order() {
        let k = |priority, dist2, id| SortKey { priority, dist2, id: (id, 0, 0) };
        let mut v = vec![k(0, 4.0, 3), k(0, 100.0, 1), k(0, 4.0, 2), k(0, 25.0, 4), k(-1, 1.0, 9)];
        sort_far_to_near(&mut v);
        let ids: Vec<u32> = v.iter().map(|s| s.id.0).collect();
        assert_eq!(ids, [9, 1, 4, 2, 3], "priority, then far to near, then id");
        // Every distance strictly not nearer than the next, within a priority.
        assert!(v[1..].windows(2).all(|w| w[0].dist2 >= w[1].dist2));
    }

    #[test]
    fn the_heap_sort_matches_the_standard_sort() {
        let mut x = 12345u32;
        let mut next = move || {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            x >> 8
        };
        for n in [0usize, 1, 2, 3, 7, 64, 500] {
            let keys: Vec<SortKey> = (0..n).map(|i| SortKey { priority: (next() % 3) as i32 - 1, dist2: (next() % 9) as f32, id: (0, 0, i as u32) }).collect();
            let mut a = keys.clone();
            sort_far_to_near(&mut a);
            let mut b = keys;
            b.sort_by(|a, b| a.priority.cmp(&b.priority).then(b.dist2.total_cmp(&a.dist2)).then(a.id.cmp(&b.id)));
            assert_eq!(a, b, "n = {n}");
        }
    }
}
