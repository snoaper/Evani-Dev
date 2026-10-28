//! The review dump (docs/KIT_PROCESS.md, "Review dump"): the `review_dump`
//! test measures the kit's real draw output with the demo's look
//! (cartoon2d) and, when `EVANI_REVIEW_DUMP` is set, writes
//! `$EVANI_REVIEW_DUMP/review.json` for the review's colour and grounding
//! checks (schema in `tools/xtask/src/kitchecks.rs`).
//!
//! * Colours: the six piece colours from the look's `pieces` palette
//!   (linear, as the renderer gets them, in sRGB hex) and, as the "player",
//!   the keyboard cursor's ring (`draw::CURSOR_RING`): what the player moves
//!   over the board. Besides its colour the cursor is a ring with an ink
//!   outline, and a picked piece lifts, grows and glows.
//! * Objects (gaps in CSS px): on level 1's board at rest, on a desktop and
//!   a phone layout, every piece (its "ground" is its cell: the gap is how
//!   far its drawn centre sits from the cell's centre, and its contact
//!   shadow is the ink shape drawn under it) and Puff (the gap from his
//!   lowest ink, the soles and body outline, to his ground line; his
//!   contact shadow is the ground shadow centred on that line).

use evani_core::App;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_render::{RenderWorld, Sprite};
use serde_json::{Value, json};

use crate::draw::{self, CURSOR_RING, DrawFx, Frame, Style, layer};
use crate::ecs::{Grid, Motion};
use crate::game::{Fns, FxFlags, Phase, PlayerInput, Request, SimWorld, Step, default_match_score_in};
use crate::layout::{CellLimits, layout};
use crate::level::levels_from_json;
use crate::{Match3Kit, Match3Plugin, Tune};

/// The review's 2D ground tolerance (kitchecks `GAP_2D_PX`).
const GAP_OK: f64 = 1.0;
/// The look's piece colour names, in palette order.
const NAMES: [&str; 6] = ["cherry", "toffee", "lemon", "mint", "berry", "grape"];
const MARKER: &str = "a pulsing light ring with an ink outline around the cursor's cell; a picked piece also lifts, grows 8 % and glows";

/// Linear 0..1 to an sRGB byte.
fn srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

fn hex(c: [f32; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", srgb(c[0]), srgb(c[1]), srgb(c[2]))
}

/// The real kit with the demo's look: its tuning and drawing style.
fn kit() -> (Tune, Style, crate::board::Rules) {
    let mut a = App::new("review").with_module(evani_render::RenderModule::new()).with_look(evani_look_cartoon2d::Cartoon2dLook).with_kit(Match3Plugin);
    a.start();
    a.run_headless(2, 1000.0 / 60.0);
    let k = *a.world.res::<Match3Kit>();
    let reg = a.world.res::<ParamRegistry>();
    let style = Style::from_look(&a.world.res::<LookData>(), &reg);
    (k.tune.read(&reg), style, k.rules(&reg))
}

/// Level 1 played to its first board at rest, then a second more (landing
/// squash over, no hint yet).
pub(crate) fn board_at_rest(tune: &Tune, rules: crate::board::Rules) -> SimWorld {
    let mut sim = SimWorld::new(levels_from_json(crate::LEVELS_JSON).unwrap(), rules);
    sim.request(Request::Start(0));
    let fns = Fns { match_score: &default_match_score_in };
    let mut rest = 0;
    for _ in 0..60 * 30 {
        sim.tick(&PlayerInput::default(), tune, &fns, &FxFlags::default(), 1.0 / 60.0);
        let g = sim.game();
        rest = if g.phase == Phase::Play && g.step == Step::Idle { rest + 1 } else { 0 };
        if rest >= 60 {
            break;
        }
    }
    assert_eq!(rest, 60, "level 1 came to rest");
    assert!(sim.game().hint.is_none(), "no hint wobble yet");
    sim
}

fn object(name: String, gap: f32, shadow: bool) -> Value {
    json!({"name": name, "gap": (f64::from(gap) * 100.0).round() / 100.0, "contact_shadow": shadow})
}

/// The lowest edge of a sprite (rotation included).
fn bottom(s: &Sprite) -> f32 {
    let (sn, cs) = s.rotation.sin_cos();
    s.pos[1] + (s.size[0].abs() * sn.abs() + s.size[1].abs() * cs.abs()) * 0.5
}

#[test]
fn review_dump() {
    let (tune, style, rules) = kit();
    let sim = board_at_rest(&tune, rules);

    let mut others: Vec<Value> = Vec::new();
    for (name, c) in NAMES.iter().zip(style.pieces) {
        others.push(json!({"name": name, "hex": hex(c)}));
    }
    let colours = json!({
        "player": {"name": "cursor ring", "hex": hex(CURSOR_RING), "marker": MARKER},
        "others": others,
    });

    let mut objects = Vec::new();
    for (screen, vw, vh) in [("desktop", 1920.0, 1080.0), ("phone", 390.0, 844.0)] {
        let (bw, bh) = {
            let b = &sim.grid().board;
            (b.w, b.h)
        };
        let l = layout(vw, vh, bw, bh, CellLimits { min_px: tune.min_cell_px, max_px: tune.max_cell_px });
        let f = Frame {
            layout: &l,
            style: &style,
            fx: DrawFx { juice: true, pop_scale: 1.3, shake: [0.0; 2] },
            t: 2.0,
            tune: &tune,
        };
        let mut rw = RenderWorld { viewport_css: l.viewport, ..Default::default() };
        draw::draw_game(&mut rw, &sim.world, &f);

        // Pieces in cell order: one shade sprite and one contact shadow each.
        let shade: Vec<&Sprite> = rw.sprites.iter().filter(|s| s.layer == layer::SHADE).collect();
        let shadows: Vec<&Sprite> = rw.sprites.iter().filter(|s| s.layer == layer::SHADOW).collect();
        let grid = sim.world.res::<Grid>();
        let board = &grid.board;
        let mut k = 0;
        for i in 0..board.len() {
            let Some(e) = grid.slots[i] else { continue };
            assert!(sim.world.get::<Motion>(e).is_some_and(|m| m.off == [0.0, 0.0] && m.swap_t < 0.0), "piece {i} at rest");
            let (x, y) = board.xy(i);
            let c = l.center(x as f32, y as f32);
            let s = shade[k];
            // Its shadow: the same shape, offset down-right, under it.
            let sh = shadows.get(k).is_some_and(|d| d.cell == s.cell && d.pos[1] > s.pos[1] && d.color[3] > 0.05);
            objects.push(object(format!("{screen}: piece r{y} c{x}"), c[1] - s.pos[1], sh));
            k += 1;
        }
        assert_eq!(k, shade.len(), "one shade sprite per piece");

        // Puff, standing (idle at rest): his lowest ink against his ground line.
        let [_, gy, _] = l.mascot;
        let low = rw.sprites.iter().filter(|s| s.layer > layer::MASCOT).map(bottom).fold(f32::MIN, f32::max);
        let shadow = rw.sprites.iter().any(|s| s.layer == layer::MASCOT && (s.pos[1] - gy).abs() < 0.5 && s.color[3] > 0.05);
        objects.push(object(format!("{screen}: Puff"), gy - low, shadow));
    }

    let dump = json!({"colours": colours, "objects": objects});
    if let Ok(dir) = std::env::var("EVANI_REVIEW_DUMP") {
        std::fs::create_dir_all(&dir).expect("dump folder");
        let path = std::path::Path::new(&dir).join("review.json");
        std::fs::write(&path, serde_json::to_string_pretty(&dump).expect("json")).expect("write review.json");
        eprintln!("review_dump: wrote {}", path.display());
    }

    // The kit's own bar: the review's.
    let objs = dump["objects"].as_array().expect("objects");
    let bad: Vec<String> = objs
        .iter()
        .filter(|o| !o["contact_shadow"].as_bool().unwrap_or(false) || o["gap"].as_f64().unwrap_or(f64::MAX).abs() > GAP_OK)
        .map(|o| format!("{} gap {} shadow {}", o["name"], o["gap"], o["contact_shadow"]))
        .collect();
    eprintln!("review_dump: {} objects, {} not grounded; colours {}", objs.len(), bad.len(), dump["colours"]);
    assert!(objs.len() > 2 * 60, "every piece on both screens");
    assert!(bad.is_empty(), "not grounded ({}): {}", bad.len(), bad.join("; "));
}
