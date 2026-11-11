//! The alpha masks of the catalogue's leaf cards (B2-P8): a leaf, a needle
//! spray and a grass tuft, (critic round 1) a rose petal and (material-view
//! round 4) an oak leaf, a maple leaf, wheat with its ears, a loose hay pile
//! and a meadow tuft with seed heads, each a 128 x 128 white RGBA PNG whose
//! alpha is the outline. The model pass cuts a card to its base texture's
//! alpha, so a foliage row seen on a card has the shape of what it is. A row
//! picks its card by its catalogue `shape` (oak, maple, wheat...), else by
//! its variant. Dev only; written in code, so no asset and no stream manifest
//! entry.

use core::f32::consts::PI;

/// The mask is this many texels across (128 since round 4: wheat awns and
/// hay strands are a texel or two wide; 256 since round 5, for pine needles;
/// spruce and fir sprigs since its follow-up).
const SIDE: usize = 256;

/// The card shapes that carry a mask (the rose is a cluster of petal cards:
/// [`mask_of`]).
pub const CARDS: [&str; 12] = ["leaf", "spray", "tuft", "petal", "oak_leaf", "maple_leaf", "wheat", "hay", "meadow", "pine", "spruce", "fir"];

/// The mask a masked shape is cut by: its own, or the petal's for the rose;
/// `None` for an unmasked shape.
pub fn mask_of(shape: &str) -> Option<&'static str> {
    match shape {
        "rose" => Some("petal"),
        s => CARDS.iter().copied().find(|c| *c == s),
    }
}

/// A small hash of two integers, 0 to 1 (strand and stalk jitter).
fn h01(a: i32, b: i32) -> f32 {
    let mut x = (a as u32).wrapping_mul(0x9e37_79b9) ^ (b as u32).wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65_535.0
}

/// Along and across a straight stalk from `root` to `top`: t (0 at the root,
/// 1 at the top), the distance from its line, and which side of it.
fn stalk(p: (f32, f32), root: (f32, f32), top: (f32, f32)) -> (f32, f32, f32) {
    let (dx, dy) = (top.0 - root.0, top.1 - root.1);
    let l2 = (dx * dx + dy * dy).max(1e-6);
    let (px, py) = (p.0 - root.0, p.1 - root.1);
    let t = (px * dx + py * dy) / l2;
    let c = (px * dy - py * dx) / l2.sqrt();
    (t, c.abs(), c.signum())
}

/// Wheat stalk `i` of 7: a thin stem, an ear of kernels alternating side to
/// side on its top part, and awns (bristles) running on past it.
fn wheat_stalk(p: (f32, f32), i: i32) -> bool {
    let k = i as f32 - 3.0;
    let root = (0.5 + 0.012 * k, 1.0);
    let top = (0.5 + 0.075 * k + 0.02 * (h01(i, 1) - 0.5), 0.07 + 0.045 * k.abs() + 0.04 * h01(i, 2));
    let (t, d, side) = stalk(p, root, top);
    let ear = 0.7;
    if (0.0..ear).contains(&t) {
        return d < 0.008;
    }
    if (ear..=1.0).contains(&t) {
        let s = (t - ear) / (1.0 - ear);
        let bump = 0.7 + 0.3 * ((s * 8.0 + if side > 0.0 { 0.5 } else { 0.0 }) * PI).sin().abs();
        return d < 0.038 * (s * PI).sin().powf(0.45) * bump + 0.007;
    }
    // Awns: a centre one and two spreading from the ear's sides.
    let a = t - 1.0;
    a < 0.25 && (d < 0.0045 || (d - 0.012 - 0.22 * a).abs() < 0.0045)
}

/// Within `w` (tapering to 40% at the far end) of the segment from `a` to `b`.
fn near_seg(p: (f32, f32), a: (f32, f32), b: (f32, f32), w: f32) -> bool {
    let (t, d, _) = stalk(p, a, b);
    (0.0..=1.0).contains(&t) && d < w * (1.0 - 0.6 * t)
}

/// A pine sprig (round 5: the spray read as a fern frond): a twig up the
/// middle, bundles of two or three long thin needles along it, each angled
/// forward towards the tip, shorter near the top, and a tuft at the end.
fn pine(p: (f32, f32)) -> bool {
    let bend = |v: f32| 0.5 + 0.03 * (v * PI).sin();
    if p.1 > 0.1 && p.1 < 0.97 && (p.0 - bend(p.1)).abs() < 0.011 {
        return true;
    }
    let mut k = 0;
    let mut v = 0.92f32;
    while v > 0.14 {
        let base = (bend(v), v);
        let len = 0.2 + 0.16 * ((v - 0.1) / 0.82).min(1.0).sqrt();
        for side in [-1.0f32, 1.0] {
            let n = if h01(k, side as i32 + 3) > 0.5 { 3 } else { 2 };
            for j in 0..n {
                let a = 0.5 + 0.22 * j as f32 + 0.12 * (h01(k * 5 + j, side as i32 + 9) - 0.5);
                let l = len * (0.85 + 0.15 * h01(k, j + 20));
                let tip = (base.0 + side * a.sin() * l, base.1 - a.cos() * l);
                if near_seg(p, base, tip, 0.008) {
                    return true;
                }
            }
        }
        k += 1;
        v -= 0.052;
    }
    // The end tuft: short needles fanning up from the twig's tip.
    (0..7).any(|j| {
        let a = (j as f32 - 3.0) * 0.16;
        let l = 0.1 + 0.05 * h01(j, 40);
        near_seg(p, (bend(0.14), 0.14), (bend(0.14) + a.sin() * l, 0.14 - a.cos() * l), 0.008)
    })
}

/// A short-needled conifer sprig, built as [`pine`] is (a twig up the middle
/// with needles along it, angled towards the tip) with its own needles
/// (round 5 follow-up: spruce and fir still opened on the fern-like spray).
/// Each node puts `angles.len()` needles on each side, `len` long, `step`
/// apart along the twig, `w` wide; the tip ends in a small bud of needles.
fn sprig(p: (f32, f32), len: f32, step: f32, angles: &[f32], w: f32) -> bool {
    let bend = |v: f32| 0.5 + 0.03 * (v * PI).sin();
    if p.1 > 0.1 && p.1 < 0.97 && (p.0 - bend(p.1)).abs() < 0.011 {
        return true;
    }
    let mut k = 0;
    let mut v = 0.93f32;
    while v > 0.13 {
        let base = (bend(v), v);
        // Shorter towards the tip, as a shoot grows.
        let l0 = len * (0.7 + 0.3 * ((v - 0.1) / 0.83).min(1.0).sqrt());
        for side in [-1.0f32, 1.0] {
            for (j, a0) in angles.iter().enumerate() {
                let j = j as i32;
                let a = a0 + 0.1 * (h01(k * 7 + j, side as i32 + 11) - 0.5);
                let l = l0 * (0.85 + 0.15 * h01(k, j + 30));
                let tip = (base.0 + side * a.sin() * l, base.1 - a.cos() * l);
                if near_seg(p, base, tip, w) {
                    return true;
                }
            }
        }
        k += 1;
        v -= step;
    }
    (0..5).any(|j| {
        let a = (j as f32 - 2.0) * 0.3;
        near_seg(p, (bend(0.13), 0.13), (bend(0.13) + a.sin() * len * 0.6, 0.13 - a.cos() * len * 0.6), w)
    })
}

/// Whether the texel centre (u, v) is inside the outline (the tip at the top,
/// v = 0).
fn inside(shape: &str, u: f32, v: f32) -> bool {
    let x = (u - 0.5).abs();
    match shape {
        "leaf" => {
            let s = (1.0 - v - 0.08) / 0.9;
            ((0.0..=1.0).contains(&s) && x < 0.44 * (s * PI).sin().powf(0.75) * (1.0 - 0.25 * s)) || (v > 0.9 && x < 0.02)
        }
        "oak_leaf" => {
            // Obovate (widest past the middle) with rounded lobes and deep
            // rounded bays, the two sides' lobes offset; a short stalk.
            let s = (0.93 - v) / 0.88;
            let env = 0.43 * (s.clamp(0.0, 1.0).powf(0.8) * PI).sin().powf(0.6);
            let ph = (s - 0.12) * 4.4 + if u < 0.5 { 0.3 } else { 0.0 };
            let lobe = (ph * PI).sin().abs().sqrt();
            let w = if s < 0.14 { env } else { env * (0.42 + 0.58 * lobe) };
            ((0.0..=1.0).contains(&s) && x < w) || (v > 0.9 && v < 0.99 && x < 0.016)
        }
        "maple_leaf" => {
            // Palmate: five broad, pointed lobes with a few big teeth
            // fanning from (0.5, 0.8) (where the recipe's fanned veins
            // start) over a wide palm, notches between them, and a long
            // stalk. Critic round 6: narrow lobes on a small palm read as a
            // hemp leaf, so the lobes keep their width until near the
            // notch and the palm reaches a third of the way out.
            let (dx, dy) = (u - 0.5, v - 0.8);
            let r = (dx * dx + dy * dy).sqrt();
            let a = dx.atan2(-dy);
            let k = (a / 0.85).round().clamp(-2.0, 2.0);
            let off = (a - 0.85 * k).abs();
            let reach = [0.4, 0.58, 0.72, 0.58, 0.4][(k + 2.0) as usize];
            let teeth = 1.0 - 0.12 * (off * 9.0).sin().abs();
            let lobe = reach * (1.0 - (off / 0.45).powi(2)).max(0.0).powf(0.3) * teeth;
            (r < lobe.max(0.24) && a.abs() < 2.15) || r < 0.13 || (dy > 0.0 && dy < 0.19 && dx.abs() < 0.014)
        }
        "wheat" => {
            // Seven stalks with heavy ears, and two long leaves low down.
            let p = (u, v);
            (0..7).any(|i| wheat_stalk(p, i))
                || [-1.0f32, 1.0].iter().any(|&sd| {
                    let (t, d, _) = stalk(p, (0.5, 1.0), (0.5 + 0.34 * sd, 0.42));
                    (0.0..1.0).contains(&t) && d < 0.022 * (1.0 - t) * (t * 6.0).min(1.0)
                })
        }
        "hay" => {
            // A loose, low pile of dry strands crossing every way with gaps
            // between them, a few sticking out past its edge.
            let (px, py) = (u - 0.5, 1.0 - v);
            let dome = (px / 0.42).powi(2) + (py / 0.58).powi(2);
            [-1.15f32, -0.6, -0.15, 0.35, 0.8, 1.25].iter().enumerate().any(|(k, &th)| {
                let (c, s) = (th.cos(), th.sin());
                let across = (-px * s + py * c) / 0.04;
                let along = px * c + py * s;
                let line = across.floor() as i32;
                let seg = ((along + h01(k as i32, line)) / 0.22).floor() as i32;
                let keep = h01(line * 7 + k as i32, seg) > 0.42;
                let out = h01(line, 31 + k as i32) > 0.7;
                keep && (across - across.floor() - 0.5).abs() < 0.17 && dome < if out { 1.3 } else { 1.0 }
            })
        }
        "meadow" => {
            // Grass blades of every height leaning out from the root, and
            // three thin stems with seed heads: a spike, a nodding plume and
            // a small one.
            let p = (u, v);
            let blades = (0..13).any(|i| {
                let root = 0.22 + 0.56 * i as f32 / 12.0;
                let tip = 0.18 + 0.55 * h01(i, 5);
                let lean = (root - 0.5) * 0.5 + 0.12 * (h01(i, 6) - 0.5);
                let hh = (1.0 - v) / (1.0 - tip);
                (0.0..1.0).contains(&hh) && (u - root - lean * hh * hh).abs() < 0.02 * (1.0 - hh).powf(0.8) + 0.002
            });
            let stems = [((0.45, 1.0), (0.38, 0.04), 0.012), ((0.55, 1.0), (0.66, 0.1), 0.024), ((0.5, 1.0), (0.53, 0.26), 0.016)];
            let heads = stems.iter().enumerate().any(|(k, &(root, top, w))| {
                let (t, d, side) = stalk(p, root, top);
                let head = 0.78;
                if (0.0..head).contains(&t) {
                    return d < 0.004;
                }
                let s = (t - head) / (1.0 - head);
                let fluff = if k == 1 { 0.6 + 0.4 * ((s * 9.0 + if side > 0.0 { 0.5 } else { 0.0 }) * PI).sin().abs() } else { 1.0 };
                (head..=1.0).contains(&t) && d < w * (s * PI).sin().powf(0.5) * fluff + 0.004
            });
            blades || heads
        }
        "petal" => {
            // A narrow claw at the base widening into a broad rounded top
            // with a shallow dip in the middle of its rim.
            let s = 1.0 - v;
            let w = if s < 0.68 { 0.06 + 0.41 * (s / 0.68).powf(0.6) } else { 0.47 * (1.0 - ((s - 0.68) / 0.29).powi(2)).max(0.0).sqrt() };
            let rim = 0.97 - 0.05 * (-(x / 0.07).powi(2)).exp();
            s > 0.02 && s < rim && x < w
        }
        "pine" => pine((u, v)),
        // Spruce: short stiff needles all round the twig, seen as two
        // spreading pairs a side, close together.
        "spruce" => sprig((u, v), 0.085, 0.03, &[0.62, 1.12], 0.009),
        // Fir: flat needles in two ranks, a comb nearly square to the twig.
        "fir" => sprig((u, v), 0.115, 0.021, &[1.22], 0.011),
        "spray" => {
            // A frond: pointed at both ends, needle tips with gaps at the edge.
            let s = 1.0 - v;
            let w = 0.47 * (s.min(0.95) / 0.95 * PI).sin().powf(0.7);
            let tip = ((v + x * 0.7) * 14.0).fract() < 0.55;
            (x < w && (x < w * 0.6 || tip)) || (x < 0.015 && s < 0.97)
        }
        _ => {
            // Tapering blades fanning from the root.
            let h = 1.0 - v;
            let lean = (u - 0.5) * h * 0.5;
            let w = (u - lean) * 7.0;
            ((w - w.floor() - 0.5).abs() < 0.42 * (1.0 - h) + 0.03) && h < 0.95 - 0.25 * (w.floor() * 1.7).sin().abs()
        }
    }
}

/// How many texels of `shape`'s mask are opaque (for tests).
#[cfg(test)]
pub fn opaque(shape: &str) -> usize {
    (0..SIDE * SIDE).filter(|i| inside(shape, (i % SIDE) as f32 / SIDE as f32, (i / SIDE) as f32 / SIDE as f32)).count()
}

/// CRC-32 (IEEE) of a PNG chunk's type and data.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// The mask of card `shape` as a PNG (stored, uncompressed deflate: 256 KB).
pub fn png(shape: &str) -> Vec<u8> {
    let mut raw = Vec::with_capacity(SIDE * (SIDE * 4 + 1));
    for y in 0..SIDE {
        raw.push(0);
        for x in 0..SIDE {
            // The border texels stay clear (round 5): a sampler that wraps
            // would carry an opaque bottom row (a tuft's roots) to the top as
            // a thin line.
            let edge = x == 0 || y == 0 || x + 1 == SIDE || y + 1 == SIDE;
            let on = !edge && inside(shape, (x as f32 + 0.5) / SIDE as f32, (y as f32 + 0.5) / SIDE as f32);
            raw.extend_from_slice(&[255, 255, 255, if on { 255 } else { 0 }]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (k, part) in chunks.iter().enumerate() {
        let n = u16::try_from(part.len()).unwrap_or(0);
        z.push(u8::from(k + 1 == chunks.len()));
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(part);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let side = u32::try_from(SIDE).unwrap_or(0).to_be_bytes();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&side);
    ihdr.extend_from_slice(&side);
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (ty, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend_from_slice(&data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each mask cuts a real shape: not nearly empty, not nearly the whole card.
    #[test]
    fn the_masks_cut_a_shape() {
        for s in CARDS {
            let on = opaque(s);
            // Stalks, strands and blades are open shapes: a smaller share.
            let floor = if matches!(s, "wheat" | "hay" | "meadow" | "tuft" | "pine" | "spruce" | "fir") { SIDE * SIDE / 14 } else { SIDE * SIDE / 8 };
            assert!(on > floor && on < SIDE * SIDE * 3 / 4, "{s}: {on} opaque texels");
        }
    }

    /// Round 4: no two cards cut the same outline (oak and maple were one
    /// oval; wheat, hay and meadow one tuft), so every row reads as itself.
    #[test]
    fn every_card_has_its_own_outline() {
        let bits = |s: &str| (0..SIDE * SIDE).map(|i| inside(s, (i % SIDE) as f32 / SIDE as f32, (i / SIDE) as f32 / SIDE as f32)).collect::<Vec<_>>();
        let all: Vec<_> = CARDS.iter().map(|s| bits(s)).collect();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate().skip(i + 1) {
                let diff = a.iter().zip(b).filter(|(x, y)| x != y).count();
                assert!(diff > SIDE * SIDE / 20, "{} and {} cut nearly the same outline ({diff} texels apart)", CARDS[i], CARDS[j]);
            }
        }
        assert_eq!(mask_of("rose"), Some("petal"));
        assert_eq!(mask_of("ball"), None);
    }

    /// The stored PNG is well formed: signature, then IHDR, IDAT and IEND with
    /// the right CRCs.
    #[test]
    fn the_png_is_well_formed() {
        let png = png("leaf");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let mut at = 8;
        let mut types = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
            let body = &png[at + 4..at + 8 + len];
            let crc = u32::from_be_bytes([png[at + 8 + len], png[at + 9 + len], png[at + 10 + len], png[at + 11 + len]]);
            assert_eq!(crc32(body), crc);
            types.push(String::from_utf8_lossy(&body[..4]).to_string());
            at += 12 + len;
        }
        assert_eq!(types, ["IHDR", "IDAT", "IEND"]);
    }
}
