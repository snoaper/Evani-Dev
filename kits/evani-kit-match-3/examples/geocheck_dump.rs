//! Scene dump for the 2D geometry checker (geocheck-2d).
//!
//! Plays Puff's Sweet Grid with the kit's own game (`SimWorld`) and bots
//! (best and weakest moves), draws every tick with the kit's real draw code
//! (`draw::draw_game`, cartoon2d look) on a desktop (1920 x 1080) and a
//! phone (390 x 844) layout, and writes `scene.json` (the geocheck-2d scene
//! format) into the folder given on the command line.
//!
//! Run it through `cargo xtask geocheck match-3` (dump, then the checker;
//! report in `.vibedeck/geocheck/match-3/`), or alone:
//! `cargo run -p evani-kit-match-3 --example geocheck_dump -- <out>`.
//!
//! The scene is one big canvas: each moment ("snapshot") is drawn at its
//! own place, desktop in the left column and phone in the right, one row
//! per moment. Every snapshot has:
//! - `hud panel` (decor): the DOM HUD panel, placed from `Layout::hud`, with
//!   its 6 x 8 px drop shadow (index.html `.hud-body`);
//! - `board tray` and `puff` (props in exclusive group `screen`): the drawn
//!   tray and mascot sprites' bounds;
//! - one prop per visible piece, from its drawn sprite (shape fill and ink
//!   outline, see `draw::shape_fill`), in exclusive group `pieces` unless it
//!   is swapping, bouncing back or shuffling (those pass through each other
//!   by design); pops are drawn above the board and are not pieces.
//!
//! A `sweep` snapshot per layout holds Puff's bounds over every drawn tick
//! of every game (all moods, hops and the sprout star), so a hop that
//! reaches the panel is caught even between the moments.
//!
//! Popups are the floating score texts and combo callouts, sampled every
//! tick from the kit's HUD model (`Match3Hud::floats`: the positions and
//! font sizes the DOM uses), boxed by `FloatView::rect`, opacity
//! `hud::float_opacity`; the "player" is the
//! moving piece (falling or swapping) the text covers most, the focus of
//! the eye at that moment.
//!
//! Dev tooling: an example, so it never reaches a game download.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use evani_core::App;
use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_kit_match_3::board::Rules;
use evani_kit_match_3::draw::{self, DrawFx, Frame, Style, layer};
use evani_kit_match_3::ecs::{Grid, Motion};
use evani_kit_match_3::game::{Fns, FxFlags, Game, Phase, PlayerInput, Request, SimWorld, Step, default_match_score_in};
use evani_kit_match_3::hud::{self, Match3Hud};
use evani_kit_match_3::layout::{CellLimits, Layout, layout};
use evani_kit_match_3::level::levels_from_json;
use evani_kit_match_3::{LEVELS_JSON, Match3Kit, Match3Plugin, Tune};
use evani_render::{RenderWorld, Sprite};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const DT: f32 = 1.0 / 60.0;
/// Column and row pitch of the snapshot grid (art px = CSS px).
const COL: f32 = 2000.0;
const ROW: f32 = 1200.0;
/// Below this opacity a piece or text counts as not shown.
const SHOWN: f32 = 0.1;

const FNS: Fns<'static> = Fns { match_score: &default_match_score_in };

fn r(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn rv(a: [f32; 4]) -> Value {
    json!([r(a[0]), r(a[1]), r(a[2]), r(a[3])])
}

fn off(a: [f32; 4], o: [f32; 2]) -> [f32; 4] {
    [a[0] + o[0], a[1] + o[1], a[2], a[3]]
}

fn union(a: Option<[f32; 4]>, b: [f32; 4]) -> [f32; 4] {
    match a {
        None => b,
        Some(a) => {
            let x0 = a[0].min(b[0]);
            let y0 = a[1].min(b[1]);
            let x1 = (a[0] + a[2]).max(b[0] + b[2]);
            let y1 = (a[1] + a[3]).max(b[1] + b[3]);
            [x0, y0, x1 - x0, y1 - y0]
        }
    }
}

fn inter_area(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// A sprite's screen bounds (rotation included).
fn bounds(s: &Sprite) -> [f32; 4] {
    let (c, si) = (s.rotation.cos().abs(), s.rotation.sin().abs());
    let w = s.size[0].abs() * c + s.size[1].abs() * si;
    let h = s.size[0].abs() * si + s.size[1].abs() * c;
    [s.pos[0] - w * 0.5, s.pos[1] - h * 0.5, w, h]
}

fn layer_bounds(sprites: &[Sprite], lo: i16, hi: i16) -> Option<[f32; 4]> {
    sprites.iter().filter(|s| s.layer >= lo && s.layer <= hi).fold(None, |acc, s| Some(union(acc, bounds(s))))
}

/// The two screens.
#[derive(Clone, Copy)]
struct Screen {
    name: &'static str,
    size: [f32; 2],
    col: f32,
}

const SCREENS: [Screen; 2] = [
    Screen { name: "desktop", size: [1920.0, 1080.0], col: 0.0 },
    Screen { name: "phone", size: [390.0, 844.0], col: 1.0 },
];

/// The moments, one row each.
const MOMENTS: [&str; 7] = ["title", "board", "cascade", "specials", "won", "lost", "sweep"];

fn row_of(moment: &str) -> f32 {
    MOMENTS.iter().position(|m| *m == moment).unwrap() as f32
}

/// One drawn piece.
struct PieceBox {
    name: String,
    rect: [f32; 4],
    opacity: f32,
    /// Falling or swapping: where the eye is.
    moving: bool,
    /// Passing through a neighbour by design (swap, bounce-back, shuffle).
    crossing: bool,
}

struct Env {
    tune: Tune,
    style: Style,
    rules: Rules,
    shake_px: f32,
    shake_freq: f32,
    pop_scale: f32,
}

/// The pieces as drawn this frame: each board piece's shade sprite (one per
/// drawn piece, in cell order, at `layer::SHADE`) with the shape's fill and
/// the ink outline.
fn pieces(w: &World, sprites: &[Sprite], env: &Env) -> Vec<PieceBox> {
    let g = w.res::<Game>();
    let grid = w.res::<Grid>();
    let board = &grid.board;
    let mut shade = sprites.iter().filter(|s| s.layer == layer::SHADE);
    let mut out = Vec::new();
    for i in 0..board.len() {
        let Some(e) = grid.slots.get(i).copied().flatten() else { continue };
        let (Some(pc), Some(m)) = (board.cells[i], w.get::<Motion>(e).map(|m| *m)) else { continue };
        let t = &env.tune;
        let v = draw::piece_cell(board, g.step, i, &m, t.swap_s, t.shuffle_s, t.invalid_slide);
        let enter = draw::enter_opacity(v[1]);
        // `draw_game` skips a piece at 0 and the painter drops sprites at
        // or under 0.003 alpha.
        if enter <= 0.003 {
            continue;
        }
        let s = shade.next().expect("one shade sprite per drawn piece");
        if enter < SHOWN {
            continue;
        }
        let fill = draw::shape_fill(draw::piece_shape(&pc));
        let o = env.style.outline_px;
        let (pw, ph) = (s.size[0].abs() * fill + 2.0 * o, s.size[1].abs() * fill + 2.0 * o);
        let (x, y) = board.xy(i);
        let crossing = m.swap_t >= 0.0 || matches!(g.step, Step::Invalid { a, b, .. } if a == i || b == i) || matches!(g.step, Step::Shuffle { .. });
        out.push(PieceBox {
            name: format!("piece r{y} c{x}"),
            rect: [s.pos[0] - pw * 0.5, s.pos[1] - ph * 0.5, pw, ph],
            opacity: enter,
            moving: m.off[1] < 0.0 || m.swap_t >= 0.0,
            crossing,
        });
    }
    out
}

struct Drawn {
    sprites: Vec<Sprite>,
    shake: [f32; 2],
}

/// Draw the game in `w` on layout `l` at sim time `t`, as `draw_system`
/// does (juice on, the game's own shake).
fn draw(w: &World, l: &Layout, env: &Env, t: f32) -> Drawn {
    let trauma = w.res::<Game>().trauma;
    let shake = draw::shake_offset(trauma, t, env.shake_px, env.shake_freq);
    let frame = Frame { layout: l, style: &env.style, fx: DrawFx { juice: true, pop_scale: env.pop_scale, shake }, t, tune: &env.tune };
    let mut rw = RenderWorld { viewport_css: l.viewport, ..Default::default() };
    draw::draw_game(&mut rw, w, &frame);
    Drawn { sprites: rw.sprites.clone(), shake }
}

/// One snapshot's objects, offset to its place on the canvas.
fn objects(prefix: &str, l: &Layout, w: &World, d: &Drawn, env: &Env, o: [f32; 2]) -> Vec<Value> {
    let mut v = Vec::new();
    let h = l.hud;
    v.push(json!({"name": format!("{prefix}/hud panel"), "kind": "decor", "rect": rv(off([h[0], h[1], h[2] + 6.0, h[3] + 8.0], o)), "layer": 100, "support": "none"}));
    if let Some(t) = layer_bounds(&d.sprites, layer::TRAY_INK, layer::TRAY) {
        v.push(json!({"name": format!("{prefix}/board tray"), "kind": "prop", "rect": rv(off(t, o)), "layer": layer::TRAY, "support": "none", "exclusive": "screen"}));
    }
    if let Some(m) = layer_bounds(&d.sprites, layer::MASCOT, i16::MAX) {
        v.push(json!({"name": format!("{prefix}/puff"), "kind": "prop", "rect": rv(off(m, o)), "layer": layer::MASCOT, "support": "none", "exclusive": "screen"}));
    }
    for p in pieces(w, &d.sprites, env) {
        let mut ob = json!({"name": format!("{prefix}/{}", p.name), "kind": "prop", "rect": rv(off(p.rect, o)), "layer": layer::SHADE, "support": "none"});
        if !p.crossing {
            ob["exclusive"] = json!("pieces");
        }
        v.push(ob);
    }
    v
}

#[derive(Default)]
struct PopRec {
    text: String,
    samples: Vec<Value>,
}

/// What the runs collect.
#[derive(Default)]
struct Rec {
    objects: Vec<Value>,
    popups: BTreeMap<String, PopRec>,
    /// Puff's bounds over every drawn tick, per screen.
    puff: BTreeMap<&'static str, [f32; 4]>,
    /// Which moments were found, per screen.
    found: Vec<String>,
    notes: Vec<String>,
    tick: u64,
}

impl Rec {
    fn snapshot(&mut self, scr: Screen, moment: &str, l: &Layout, w: &World, d: &Drawn, env: &Env) {
        let key = format!("{}/{moment}", scr.name);
        if self.found.contains(&key) {
            return;
        }
        let o = [scr.col * COL, row_of(moment) * ROW];
        self.objects.extend(objects(&key, l, w, d, env, o));
        self.found.push(key);
    }
}

fn new_sim(env: &Env) -> SimWorld {
    SimWorld::new(levels_from_json(LEVELS_JSON).unwrap(), env.rules)
}

/// The layout `draw_system` makes for the board in `w` on screen `scr`.
fn screen_layout(w: &World, scr: Screen, env: &Env) -> Layout {
    let lim = CellLimits { min_px: env.tune.min_cell_px, max_px: env.tune.max_cell_px.max(env.tune.min_cell_px) };
    let b = &w.res::<Grid>().board;
    layout(scr.size[0], scr.size[1], b.w, b.h, lim)
}

/// One tick of `sim` on screen `scr`: step, lay out and draw as
/// `draw_system` does, sample popups and Puff.
fn tick(sim: &mut SimWorld, scr: Screen, env: &Env, rec: &mut Rec, game_id: &str) -> (Drawn, Layout) {
    sim.tick(&PlayerInput::default(), &env.tune, &FNS, &FxFlags::default(), DT);
    let lay = screen_layout(&sim.world, scr, env);
    sim.game_mut().layout = lay;
    let l = &lay;
    rec.tick += 1;
    let t = rec.tick as f32 * DT;
    let d = draw(&sim.world, l, env, t);
    if let Some(m) = layer_bounds(&d.sprites, layer::MASCOT, i16::MAX) {
        let u = union(rec.puff.get(scr.name).copied(), m);
        rec.puff.insert(scr.name, u);
    }
    let mut hud = Match3Hud::default();
    hud.update(&sim.world, &env.style, d.shake, env.tune.result_delay_s);
    if !hud.floats.is_empty() {
        // Popups sit over the board snapshot of their screen, so their
        // crops show a board under them.
        let o = [scr.col * COL, row_of("board") * ROW];
        let moving: Vec<[f32; 4]> = pieces(&sim.world, &d.sprites, env).into_iter().filter(|p| p.moving && p.opacity >= SHOWN).map(|p| p.rect).collect();
        for f in &hud.floats {
            let rect = f.rect();
            let player = moving.iter().copied().filter(|m| inter_area(*m, rect) > 0.0).max_by(|a, b| inter_area(*a, rect).total_cmp(&inter_area(*b, rect)));
            let id = format!("{}/{game_id}/{}", scr.name, f.id);
            let p = rec.popups.entry(id).or_default();
            p.text = f.text.clone();
            let mut s = json!({"tick": rec.tick, "t": r(t), "rect": rv(off(rect, o)), "alpha": r(hud::float_opacity(f.age))});
            if let Some(pr) = player {
                s["player"] = rv(off(pr, o));
            }
            p.samples.push(s);
        }
    }
    (d, lay)
}

/// The title: the attract board plays itself for a few seconds.
fn title(scr: Screen, env: &Env, rec: &mut Rec) {
    let mut sim = new_sim(env);
    let mut last = None;
    for _ in 0..60 * 4 {
        last = Some(tick(&mut sim, scr, env, rec, "title"));
    }
    let (d, l) = last.unwrap();
    rec.snapshot(scr, "title", &l, &sim.world, &d, env);
}

/// Play `level` with the best (or weakest) move each time the board is
/// idle, taking the moments as they come.
fn play(scr: Screen, env: &Env, rec: &mut Rec, level: usize, best: bool) -> Phase {
    let mut sim = new_sim(env);
    sim.request(Request::Start(level));
    let game_id = format!("level{}-{}", level + 1, if best { "best" } else { "weakest" });
    for _ in 0..60 * 60 * 5 {
        let (d, lay) = tick(&mut sim, scr, env, rec, &game_id);
        let l = &lay;
        let (phase, step, phase_t, cascade) = {
            let g = sim.game();
            (g.phase, g.step, g.phase_t, g.cascade)
        };
        let blasts = !sim.grid().blasts.is_empty();
        if phase == Phase::Play && step == Step::Idle {
            // A board at rest has a piece in every playable cell.
            let grid = sim.grid();
            let empty: Vec<usize> = (0..grid.board.len()).filter(|&i| grid.board.mask[i] && grid.board.cells[i].is_none()).collect();
            assert!(empty.is_empty(), "{}: level {} at rest has empty playable cells {empty:?}", scr.name, level + 1);
            drop(grid);
            rec.snapshot(scr, "board", l, &sim.world, &d, env);
        }
        // Mid-fall in the second round of a cascade or later.
        let falling = sim.grid().slots.iter().flatten().any(|&e| sim.world.get::<Motion>(e).is_some_and(|m| m.off[1] < -0.4));
        if step == Step::Fall && cascade >= 2 && falling {
            rec.snapshot(scr, "cascade", l, &sim.world, &d, env);
        }
        if blasts {
            rec.snapshot(scr, "specials", l, &sim.world, &d, env);
        }
        match phase {
            Phase::Won | Phase::Lost if phase_t >= env.tune.result_delay_s => {
                let m = if phase == Phase::Won { "won" } else { "lost" };
                rec.snapshot(scr, m, l, &sim.world, &d, env);
                // Let the last texts finish.
                for _ in 0..90 {
                    tick(&mut sim, scr, env, rec, &game_id);
                }
                return phase;
            }
            Phase::Play if step == Step::Idle => {
                let mv = {
                    let grid = sim.grid();
                    if best { grid.board.best_move() } else { grid.board.worst_move() }
                };
                if let Some(m) = mv {
                    sim.request(Request::Swap(m.a, m.b));
                }
            }
            _ => {}
        }
    }
    panic!("{}: level {} did not end", scr.name, level + 1);
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "geocheck-out".into()));
    std::fs::create_dir_all(&out).unwrap();

    // The real kit with the demo's look: its baked tuning, rules and style.
    let mut a = App::new("geocheck").with_module(evani_render::RenderModule::new()).with_look(evani_look_cartoon2d::Cartoon2dLook).with_kit(Match3Plugin);
    a.start();
    a.run_headless(2, 1000.0 / 60.0);
    let env = {
        let k = *a.world.res::<Match3Kit>();
        let reg = a.world.res::<ParamRegistry>();
        let style = Style::from_look(&a.world.res::<LookData>(), &reg);
        Env {
            tune: k.tune.read(&reg),
            style,
            rules: k.rules(&reg),
            shake_px: draw::param_f32(&reg, "fx.shake.amount", 0.35) * k.tune.read(&reg).shake_px_per_unit,
            shake_freq: draw::param_f32(&reg, "fx.shake.frequency", 18.0),
            pop_scale: draw::param_f32(&reg, "fx.juice.pop_scale", 1.3),
        }
    };
    let lim = CellLimits { min_px: env.tune.min_cell_px, max_px: env.tune.max_cell_px.max(env.tune.min_cell_px) };

    let mut rec = Rec::default();
    let levels = levels_from_json(LEVELS_JSON).unwrap();
    for scr in SCREENS {
        // Every level's board shape on this screen (the layout depends on it).
        let lay = |lv: usize| {
            let sh = &levels[lv].shape;
            layout(scr.size[0], scr.size[1], sh.w, sh.h, lim)
        };
        title(scr, &env, &mut rec);
        let mut results = Vec::new();
        for lv in 0..levels.len() {
            results.push((lv, "best", play(scr, &env, &mut rec, lv, true)));
        }
        results.push((0, "weakest", play(scr, &env, &mut rec, 0, false)));
        rec.notes.push(format!(
            "{}: {}.",
            scr.name,
            results.iter().map(|(lv, b, p)| format!("level {} ({b} moves) {}", lv + 1, p.as_str())).collect::<Vec<_>>().join(", ")
        ));
        // Puff's bounds over every tick, with the panel and tray of the board moment.
        let o = [scr.col * COL, row_of("sweep") * ROW];
        let l = lay(0);
        let h = l.hud;
        rec.objects.push(json!({"name": format!("{}/sweep/hud panel", scr.name), "kind": "decor", "rect": rv(off([h[0], h[1], h[2] + 6.0, h[3] + 8.0], o)), "layer": 100, "support": "none"}));
        if let Some(p) = rec.puff.get(scr.name).copied() {
            rec.objects.push(json!({"name": format!("{}/sweep/puff (every tick)", scr.name), "kind": "prop", "rect": rv(off(p, o)), "layer": layer::MASCOT, "support": "none", "exclusive": "screen"}));
        }
        let b = l.board;
        let pad = l.cell * 0.16;
        rec.objects.push(json!({"name": format!("{}/sweep/board tray", scr.name), "kind": "prop", "rect": rv(off([b[0] - pad, b[1] - pad, b[2] + 2.0 * pad, b[3] + 2.0 * pad], o)), "layer": layer::TRAY, "support": "none", "exclusive": "screen"}));
        rec.found.push(format!("{}/sweep", scr.name));
    }
    let want: Vec<String> = SCREENS.iter().flat_map(|s| MOMENTS.iter().map(move |m| format!("{}/{m}", s.name))).collect();
    let missing: Vec<&String> = want.iter().filter(|k| !rec.found.contains(k)).collect();
    assert!(missing.is_empty(), "moments not reached: {missing:?}");

    let popups: Vec<Value> = rec.popups.into_iter().map(|(id, p)| json!({"id": id, "text": p.text, "samples": p.samples})).collect();
    let n_pop = popups.len();
    let mut notes = vec![
        "Scene dumped by the kit's geocheck_dump example: the kit's own game (SimWorld), bots (best and weakest moves) and draw code (draw::draw_game, cartoon2d look, juice and shake on), every tick, on a desktop (1920 x 1080) and a phone (390 x 844) layout.".to_string(),
        format!("Snapshots: desktop in the left column (x 0), phone in the right (x {COL}); one row per moment, {ROW} px apart: {}. The sweep row holds Puff's bounds over every drawn tick.", MOMENTS.join(", ")),
        "Smooth art (pixel_art false): the cartoon2d look draws vector-like atlas shapes at any size, so check 4 (whole pixels, one pixel scale) does not apply.".to_string(),
        "Pieces: the drawn shade sprite times the shape's fill plus the ink outline. Swapping, bouncing-back and shuffling pieces pass through each other by design and are left out of group `pieces`; pops (drawn above the board) are not pieces. Pieces under 10 % opacity (fading in above the board) are left out.".to_string(),
        format!("Popups: {n_pop} floating texts (score and combo callouts) sampled every tick from the kit's HUD model, boxes from FloatView::rect (the font size the page is given, hud::float_font; 0.6 em per character of bold Trebuchet MS, 1.1 em high; times hud::float_scale), opacity from hud::float_opacity. Score texts sit at their match; combo callouts at hud::callout_anchor, off the board. The player rect is the moving piece (falling or swapping) the text covers most."),
        "Every board at rest (Play, idle) was checked to have a piece in every playable cell.".to_string(),
        "Not in the dump: the DOM title, level intro, pause, win and lose cards and the phone banner under the HUD (page overlays the native dump cannot draw); the review's page screenshots cover them. Sparkles and blast beams are short effects and not objects.".to_string(),
    ];
    notes.extend(rec.notes);
    let rows = MOMENTS.len() as f32;
    let scene = json!({
        "format": "evani-geocheck-2d/1",
        "name": "match-3",
        "source": "kits/evani-kit-match-3/examples/geocheck_dump.rs",
        "tile_size": 16.0,
        "map": [((COL * 2.0) / 16.0).ceil() as u32, ((ROW * rows) / 16.0).ceil() as u32],
        "pixel_scale": 1.0,
        "pixel_art": false,
        "background": "#fff4e2",
        "objects": rec.objects,
        "popups": popups,
        "notes": notes,
    });
    let path = out.join("scene.json");
    std::fs::write(&path, serde_json::to_string(&scene).unwrap()).unwrap();
    println!("geocheck_dump: {} objects, {n_pop} popups -> {}", scene["objects"].as_array().map_or(0, Vec::len), path.display());
}
