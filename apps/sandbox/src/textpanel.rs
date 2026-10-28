//! The MSDF text proof panel (S3-13a), only in a build with `ui` (the
//! sandbox ships without it, so its download is unchanged). It draws the
//! engine font (`[ui] fonts`, name `ui`) at several sizes from 12 to 200
//! px, a long line wrapped in all three alignments, outline, glow and
//! shadow, kerning pairs and a missing glyph showing `?`. Build it with
//! `trunk build --features ui` and `cargo xtask fonts sandbox --dist <dist>`.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_render::text::{self, Align, TextStyle};
use evani_render::{RenderWorld, Sprite};

/// The `Stage::Frame` system that draws the panel (after the scene, so it
/// sits on top).
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.textpanel", Stage::Frame, draw)
}

const LAYER: i16 = evani_render::msdf::HUD_LAYER + 40;
// On the HUD layers: drawn in the `ui` pass after the tone map and post
// effects, so ink at 1 lands as 255 with no bloom halo.
const INK: [f32; 4] = [1.0, 1.0, 0.97, 1.0];
const LONG: &str = "Sharp text at any size: this long line wraps inside its column, and each line aligns on its own.";

fn backdrop(rw: &mut RenderWorld, x: f32, y: f32, w: f32, h: f32) {
    rw.draw_sprite(Sprite {
        pos: [x + w * 0.5, y + h * 0.5],
        // The square cell's shape fills about 80% of the cell: scale up so
        // the dark part covers the whole panel.
        size: [w * 1.3, h * 1.3],
        rotation: 0.0,
        cell: evani_render::atlas::cell::SQUARE,
        // Opaque: the scene's bloom still glows through a little (sprites
        // sit under the post chain), so the panel itself hides the scene.
        color: [0.02, 0.025, 0.04, 1.0],
        layer: LAYER - 1,
    });
}

fn draw(world: &mut World) {
    let mut rw = world.res_mut::<RenderWorld>();
    let font = rw.fonts.request("ui");
    let [w, h] = rw.viewport_css;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let s = |size: f32| TextStyle { font, ..TextStyle::new(size, INK) };
    let pad = 16.0;
    let panel_w = (w - 2.0 * pad).min(1240.0);
    backdrop(&mut rw, pad, pad, panel_w, h - 2.0 * pad);
    let x = pad * 2.0;
    let mut y = pad * 2.0;
    // Sizes, small to large.
    // Literal labels: formatting a float would pull Rust's float printing
    // into the download for a proof panel.
    for (size, line) in [
        (12.0, "12 px: The quick brown fox jumps over the lazy dog. 0123456789"),
        (16.0, "16 px: The quick brown fox jumps over the lazy dog. 0123456789"),
        (24.0, "24 px: The quick brown fox jumps over the lazy dog. 0123456789"),
        (36.0, "36 px: The quick brown fox jumps over the lazy dog. 0123456789"),
    ] {
        // Wrap inside the panel, so a phone-width screen keeps every line.
        let st = TextStyle { max_width: panel_w - 2.0 * pad, ..s(size) };
        y += text::draw(&mut rw, [x, y], LAYER, line, &st)[1];
    }
    // Kerning and the missing glyph.
    y += text::draw(&mut rw, [x, y], LAYER, "Kerning: AV To Wa Ty  |  Missing glyph: [\u{4e2d}] [\u{1F600}]", &s(24.0))[1] + 8.0;
    // A long line wrapped in three columns: left, centre, right.
    let col = ((panel_w - 4.0 * pad) / 3.0).max(80.0);
    let mut tallest = 0.0f32;
    for (i, align) in [Align::Left, Align::Center, Align::Right].into_iter().enumerate() {
        let left = x + i as f32 * (col + pad);
        let anchor = match align {
            Align::Left => left,
            Align::Center => left + col * 0.5,
            Align::Right => left + col,
        };
        let st = TextStyle { max_width: col, align, ..s(18.0) };
        tallest = tallest.max(text::draw(&mut rw, [anchor, y], LAYER, LONG, &st)[1]);
    }
    y += tallest + 12.0;
    // Effects.
    let effects = [
        ("Outline", TextStyle { outline: Some([0.85, 0.25, 0.10, 1.0]), ..s(48.0) }),
        ("Glow", TextStyle { glow: Some([0.30, 0.70, 1.00, 0.9]), ..s(48.0) }),
        ("Shadow", TextStyle { shadow: Some([0.0, 0.0, 0.0, 0.9]), ..s(48.0) }),
    ];
    let mut ex = x;
    for (label, st) in &effects {
        ex += text::draw(&mut rw, [ex, y], LAYER, label, st)[0] + 40.0;
    }
    // Large: 200 px (edges stay sharp; fringes would show here first). On a
    // screen too short for it below the effects (a phone held sideways) it
    // goes beside them, right-aligned and clear of the bottom-left hint box,
    // as large as fits.
    let big = TextStyle { outline: Some([0.0, 0.0, 0.0, 1.0]), ..s(200.0) };
    // The backdrop's bottom edge.
    let bottom = h - pad;
    if bottom - (y + 64.0) >= 200.0 {
        text::draw(&mut rw, [x, y + 64.0], LAYER, "Ag 200", &big);
    } else {
        let size = (bottom - pad - y).clamp(24.0, 200.0);
        let st = TextStyle { align: Align::Right, ..TextStyle { size_px: size, ..big } };
        text::draw(&mut rw, [pad + panel_w - pad, y], LAYER, "Ag", &st);
    }
}
