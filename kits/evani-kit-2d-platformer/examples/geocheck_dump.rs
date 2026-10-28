//! Scene dump for the 2D geometry checker (geocheck-2d).
//!
//! Plays Bramble Hills with the kit's own bot and physics, draws sampled
//! ticks with the kit's real draw system (pixel16 look, 1920 x 1080), and
//! writes `scene.json` (the geocheck-2d scene format) plus `atlas.rgba`
//! (the kit's sprite sheet, raw RGBA8) into the folder given on the
//! command line.
//!
//! Run it through `cargo xtask geocheck 2d-platformer` (dump, then the
//! checker; report in `.vibedeck/geocheck/2d-platformer/`), or alone:
//! `cargo run -p evani-kit-2d-platformer --example geocheck_dump -- <out>`.
//!
//! Dev tooling: an example, so it never reaches a game download. Placement
//! comes from the kit itself (`draw::centred_top`, `draw::text_pops`,
//! `Screen::art_rect`); where the dump restates a draw rule (tile edges,
//! object feet), the comment names the kit code it mirrors.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use evani_core::App;
use evani_core::params::ParamRegistry;
use evani_kit_2d_platformer::art::{DECOS, TileArt};
use evani_kit_2d_platformer::bot::{self, Sim};
use evani_kit_2d_platformer::draw::{DrawState, Screen, centred_top, draw_system, layer, text_pops};
use evani_kit_2d_platformer::game::{Game, Phase, PopKind};
use evani_kit_2d_platformer::level::{Level, Tile};
use evani_kit_2d_platformer::{Platformer, PlatformerKit, PlatformerPlugin};
use evani_render::{RenderWorld, SHEET_CELL0, SpriteSheet};
use serde_json::{Value, json};
use std::rc::Rc;

const DT: f32 = 1.0 / 60.0;
const TARGET: [f32; 2] = [1920.0, 1080.0];
/// Camera-sweep steps (whole art pixels): the parallax factors 0.15, 0.4
/// and 0.7 repeat their sub-pixel offsets every 20 px, so 64 steps see each
/// offset at least three times.
const SWEEP: usize = 64;
/// Where the sweep's view starts (art px from the level's left edge): far
/// enough in that every plane's join between two copies is on screen (at
/// x 1600 the 512 px far strip's join sits near x 272 of the 480 px view,
/// the mid strip's near x 384, and the 256 px near strip always shows one).
/// Near the level start the far strip's join is off screen, so a sweep
/// there cannot see it break.
const SWEEP_X0: f32 = 1600.0;

fn r(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Value {
    json!([r(x), r(y), r(w), r(h)])
}

fn frame_of(cell: u16) -> Option<usize> {
    cell.checked_sub(SHEET_CELL0).map(usize::from)
}

/// The frame's drawn sprites in device pixels relative to the view's
/// top-left (the kit's own `Screen::art_rect` undoes the per-axis CSS
/// mapping and the framing origin); mirrored sprites keep a negative width.
fn sprites_of(rw: &RenderWorld, screen: &Screen) -> Vec<Value> {
    let k = screen.scale;
    rw.sprites
        .iter()
        .map(|s| {
            let ar = screen.art_rect(s);
            let (w, h) = (ar[2] * k * s.size[0].signum(), ar[3] * k);
            let (x, y) = (ar[0] * k, ar[1] * k);
            let mut v = json!({"x": r(x), "y": r(y), "w": r(w), "h": r(h), "l": s.layer,
                "c": [r(s.color[0]), r(s.color[1]), r(s.color[2]), r(s.color[3])]});
            match frame_of(s.cell) {
                Some(f) => v["f"] = json!(f),
                None => v["builtin"] = json!(s.cell),
            }
            v
        })
        .collect()
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "geocheck-out".into()));
    std::fs::create_dir_all(&out).unwrap();

    // The real app: kit + pixel16 look + renderer resources.
    let mut a = App::new("geocheck")
        .with_module(evani_render::RenderModule::new())
        .with_look(evani_look_pixel16::Pixel16Look)
        .with_kit(PlatformerPlugin);
    a.start();
    {
        let mut rw = a.world.res_mut::<RenderWorld>();
        rw.viewport_css = TARGET;
        rw.target_px = [TARGET[0] as u32, TARGET[1] as u32];
    }
    // Parallax on, as evani-fx reports it in the game (evani.toml lists it;
    // it costs 1 point), so the frames draw the sky bands, the three planes,
    // the clouds and the far castle the player sees. Shake stays off: it
    // only moves the camera by whole art pixels.
    static PARALLAX: evani_core::effects::EffectInfo = evani_core::effects::EffectInfo {
        name: "parallax",
        cost: 1,
        value: 1.0,
        styles: evani_core::effects::StyleClasses::TWO_D,
        vr_comfort_off: false,
        description: "",
    };
    a.world.insert_resource(evani_core::effects::EffectsStatus {
        effects: vec![evani_core::effects::EffectStatus {
            info: &PARALLAX,
            enabled: None,
            weight: None,
            passes: &[],
            state: evani_core::effects::EffectState::On,
        }],
    });
    a.run_headless(3, 1000.0 / 60.0);
    a.world.res_mut::<RenderWorld>().clear();
    draw_system(&mut a.world);

    let kit = *a.world.res::<PlatformerKit>();
    let (t, n, gravity) = {
        let reg = a.world.res::<ParamRegistry>();
        let t = kit.tune.read(&reg);
        (t, kit.ints(&reg), kit.gravity(&reg, &t))
    };
    let art = a.world.res::<DrawState>().art.clone().expect("art built");
    let sheet = a.world.res::<SpriteSheet>().clone();
    std::fs::write(out.join("atlas.rgba"), &sheet.rgba8).unwrap();
    let level: Rc<Level> = Rc::clone(&a.world.res::<Platformer>().game.level);

    // ---- Tiles ------------------------------------------------------------
    let open = |t: Tile| !matches!(t, Tile::Solid { .. } | Tile::Slope { .. });
    let mut tiles = Vec::new();
    for ty in 0..level.h as i32 {
        for tx in 0..level.w as i32 {
            let tile = level.tile(tx, ty);
            let (id, shape, group, tl, hazard) = match tile {
                Tile::Empty => continue,
                Tile::Solid { art: 0 } => ("#", "full", Some("earth"), layer::TILES, false),
                Tile::Solid { .. } => ("B", "full", Some("stone"), layer::TILES, false),
                Tile::OneWay { art: 0 } => ("=", "top", Some("plank"), layer::TILES, false),
                Tile::OneWay { .. } => ("-", "top", Some("ledge"), layer::TILES, false),
                Tile::Slope { .. } => ("/", "slope", Some("earth"), layer::TILES, false),
                Tile::Water { deep } => (if deep { "w" } else { "~" }, "none", Some("water"), layer::WATER, true),
                Tile::Spikes => ("^", "none", None, layer::TILES, true),
                Tile::Decoration { .. } => ("C", "none", Some("castle"), layer::CASTLE, false),
            };
            // Edge art as the kit draws it (art.rs `tile_key`): earth and
            // stone show grass or light/shadow on open top, left and right
            // sides; one-ways end caps; water its surface; castle its rims.
            let mut edges = String::new();
            match tile {
                Tile::Solid { .. } => {
                    for (c, (dx, dy)) in [('t', (0, -1)), ('l', (-1, 0)), ('r', (1, 0))] {
                        if open(level.tile(tx + dx, ty + dy)) {
                            edges.push(c);
                        }
                    }
                }
                Tile::OneWay { .. } => {
                    edges.push('t');
                    for (c, dx) in [('l', -1), ('r', 1)] {
                        if level.tile(tx + dx, ty) != tile {
                            edges.push(c);
                        }
                    }
                }
                Tile::Slope { .. } => edges.push('t'),
                Tile::Water { .. } => {
                    if !matches!(level.tile(tx, ty - 1), Tile::Water { .. }) {
                        edges.push('t');
                    }
                }
                Tile::Decoration { .. } => {
                    for (c, (dx, dy)) in [('t', (0, -1)), ('l', (-1, 0)), ('r', (1, 0))] {
                        if !matches!(level.tile(tx + dx, ty + dy), Tile::Decoration { .. }) {
                            edges.push(c);
                        }
                    }
                }
                _ => {}
            }
            let frame = match art.tile(tx, ty) {
                TileArt::Still(c) | TileArt::Back(c) | TileArt::Cycle(c) => frame_of(c),
                TileArt::None => None,
            };
            let mut v = json!({"x": tx, "y": ty, "id": id, "layer": tl, "shape": shape, "edges": edges, "hazard": hazard});
            if let Some(g) = group {
                v["group"] = json!(g);
            }
            // An earth tile beside a stone pillar is the transition: the art
            // bonds the pillar's blocks into the dirt with a dark seam
            // (art.rs `stone_seam`, key bits 10/11 of `tile_key`).
            let stone = |t: Tile| matches!(t, Tile::Solid { art: 1 });
            if tile == (Tile::Solid { art: 0 }) && (stone(level.tile(tx - 1, ty)) || stone(level.tile(tx + 1, ty))) {
                v["transition"] = json!(true);
            }
            if let Some(f) = frame {
                v["frame"] = json!(f);
            }
            if let Tile::Slope { from, to } = tile {
                v["slope"] = json!([from, to]);
            }
            tiles.push(v);
        }
    }

    // ---- Objects (as the draw system places them) --------------------------
    let at_feet = |w: f32, h: f32, x: f32, y: f32| [(x - w * 0.5).round(), y - h, w, h];
    let mut objects = Vec::new();
    for (i, c) in level.coins.iter().enumerate() {
        let f = art.coin[0];
        let rc = at_feet(f.w, f.h, c[0], centred_top(f.h, c[1]) + f.h);
        objects.push(json!({"name": format!("coin {i}"), "kind": "collectible", "rect": rect(rc[0], rc[1], rc[2], rc[3]),
            "frame": frame_of(f.cell), "layer": layer::COINS, "support": "none", "centre": [r(c[0]), r(c[1])]}));
    }
    let enemies = level.beetles.iter().map(|p| ("beetle", *p, art.beetle[0], 5.0, 10.0)).chain(level.hoppers.iter().map(|p| ("hopper", *p, art.hopper[0], 5.0, 11.0)));
    for (i, (kind, p, f, hw, h)) in enemies.enumerate() {
        let rc = at_feet(f.w, f.h, p[0], p[1] + 1.0);
        objects.push(json!({"name": format!("{kind} {i}"), "kind": "enemy", "rect": rect(rc[0], rc[1], rc[2], rc[3]),
            "frame": frame_of(f.cell), "flip": true, "layer": layer::ENEMIES, "support": "ground",
            "foot": [r(p[0]), r(p[1])], "hitbox": rect(p[0] - hw, p[1] - h, 2.0 * hw, h)}));
    }
    for (i, d) in level.decorations.iter().enumerate() {
        let Some(k) = DECOS.iter().position(|k| *k == d.kind) else { continue };
        let f = art.deco[k];
        let rc = at_feet(f.w, f.h, d.pos[0], d.pos[1] + 2.0);
        objects.push(json!({"name": format!("{} {i}", d.kind), "kind": "decor", "rect": rect(rc[0], rc[1], rc[2], rc[3]),
            "frame": frame_of(f.cell), "layer": layer::DECO, "support": "ground"}));
    }
    for (i, p) in level.checkpoints.iter().enumerate() {
        let f = art.flag[0];
        let rc = at_feet(f.w, f.h, p[0] + 3.0, p[1] + 1.0);
        objects.push(json!({"name": format!("checkpoint {i}"), "kind": "prop", "rect": rect(rc[0], rc[1], rc[2], rc[3]),
            "frame": frame_of(f.cell), "layer": layer::DECO, "support": "ground"}));
    }
    {
        let f = art.gate;
        let rc = at_feet(f.w, f.h, level.goal[0], level.goal[1]);
        objects.push(json!({"name": "castle gate", "kind": "goal", "rect": rect(rc[0], rc[1], rc[2], rc[3]),
            "frame": frame_of(f.cell), "layer": layer::DECO, "support": "ground"}));
    }
    for (i, p) in level.torches.iter().enumerate() {
        let f = art.torch;
        let rc = at_feet(f.w, f.h, p[0], p[1]);
        objects.push(json!({"name": format!("torch {i}"), "kind": "prop", "rect": rect(rc[0], rc[1], rc[2], rc[3]),
            "frame": frame_of(f.cell), "layer": layer::DECO, "support": "wall"}));
    }
    let mut platforms = Vec::new();
    for (i, d) in level.platforms.iter().enumerate() {
        platforms.push(json!({"name": format!("platform {i}"), "width": d.width_px, "path": d.path}));
        let p = d.path[0];
        let f = art.platform[0];
        objects.push(json!({"name": format!("platform {i}"), "kind": "platform", "rect": rect(p[0], p[1], d.width_px, f.h),
            "layer": layer::PLATFORMS, "support": "none"}));
    }
    let hero_sprite = art.hero.iter().fold([0.0f32, 0.0f32], |m, f| [m[0].max(f.w), m[1].max(f.h)]);

    // ---- Bot run ----------------------------------------------------------
    let mut g = Game::new(Rc::clone(&level), &t, &n);
    g.reset(Phase::Playing, &t, &n);
    let sim = Sim { t: &t, n: &n, gravity, dt: DT };
    let mut ticks: Vec<Value> = Vec::new();
    let mut snaps: Vec<(usize, Game)> = Vec::new();
    let mut cams: Vec<(usize, Game)> = Vec::new();
    let mut start_game: Option<Game> = None;
    let mut pops: std::collections::BTreeMap<String, (String, Vec<Value>)> = Default::default();
    let mut was_grounded = true;
    let mut spawn_ticks: Vec<usize> = Vec::new();
    let mut pop_count = 0usize;
    let run = bot::play(&mut g, sim, 60 * 120, 30, |i, g| {
        let h = &g.hero.body;
        ticks.push(json!([r(h.pos[0]), r(h.pos[1]), u8::from(h.grounded)]));
        let landed = h.grounded && !was_grounded;
        was_grounded = h.grounded;
        let hr = at_feet(hero_sprite[0], hero_sprite[1], h.pos[0], h.pos[1] + 1.0);
        // Score and 1UP popups, laid out by the kit's own `draw::text_pops`
        // (screen space with the camera at 0,0 = world art pixels).
        let placed = text_pops(g, [0.0, 0.0], level.width_px(), 1.0);
        let texts = g.pops.iter().filter(|p| matches!(p.kind, PopKind::Score(_) | PopKind::OneUp));
        for (p, tp) in texts.zip(placed) {
            let spawn = i as f32 - p.t / DT;
            let id = format!("{} from tick {} at {:.0},{:.0}", tp.text, spawn.round(), p.pos[0], p.pos[1]);
            if !pops.contains_key(&id) {
                pop_count += 1;
                spawn_ticks.push(i);
            }
            pops.entry(id).or_insert_with(|| (tp.text.clone(), Vec::new())).1.push(json!({
                "tick": i, "t": r(i as f32 * DT), "rect": rect(tp.at.x, tp.at.y, tp.at.w, tp.at.h), "alpha": r(tp.alpha),
                "player": rect(hr[0], hr[1], hr[2], hr[3])}));
        }
        if i % 6 == 0 {
            cams.push((i, g.clone()));
        }
        if i == 0 {
            start_game = Some(g.clone());
        }
        if i % 30 == 0 || landed || spawn_ticks.last().is_some_and(|s| i == *s + 4) {
            snaps.push((i, g.clone()));
        }
    });
    let _ = pop_count;
    let touched: Vec<String> = g.coins_taken.iter().enumerate().filter(|(_, t)| **t).map(|(i, _)| format!("coin {i}")).collect();

    // ---- Frames (the kit's real draw system) -------------------------------
    let mut frames = Vec::new();
    let snap_ticks: std::collections::BTreeSet<usize> = snaps.iter().map(|(i, _)| *i).collect();
    let mut all: Vec<(usize, Game, bool)> = cams.into_iter().map(|(i, g)| (i, g, false)).collect();
    for (i, g) in snaps {
        if let Some(e) = all.iter_mut().find(|e| e.0 == i) {
            e.2 = true;
        } else {
            all.push((i, g, true));
        }
    }
    all.sort_by_key(|e| e.0);
    for (i, snap, with_sprites) in all {
        let grounded = snap.hero.body.grounded;
        let hp = snap.hero.body.pos;
        {
            let mut p = a.world.res_mut::<Platformer>();
            p.game = snap;
            if p.game.phase == Phase::Title {
                p.game.phase = Phase::Playing;
            }
        }
        a.world.res_mut::<evani_core::time::Time>().alpha = 1.0;
        a.world.res_mut::<RenderWorld>().clear();
        draw_system(&mut a.world);
        let screen = a.world.res::<DrawState>().screen;
        let hr = at_feet(hero_sprite[0], hero_sprite[1], hp[0], hp[1] + 1.0);
        let mut fr = json!({"tick": i, "t": r(i as f32 * DT), "scale": screen.scale, "cam": [screen.cam[0], screen.cam[1]],
            "view": [r(screen.view[0]), r(screen.view[1])], "player": rect(hr[0], hr[1], hr[2], hr[3]), "grounded": grounded});
        if with_sprites && snap_ticks.contains(&i) {
            fr["sprites"] = json!(sprites_of(&a.world.res::<RenderWorld>(), &screen));
        }
        frames.push(fr);
    }

    // ---- Camera sweep (check 8, draw joins) --------------------------------
    // The level-start game drawn with the view one whole art pixel further right
    // each step: a join that breaks through rounding (a parallax strip at a
    // half-pixel offset) breaks only at some camera positions.
    let mut sweep = Vec::new();
    if let Some(base) = &start_game {
        for step in 0..SWEEP {
            let view = a.world.res::<DrawState>().screen.view;
            {
                let mut p = a.world.res_mut::<Platformer>();
                p.game = base.clone();
                p.game.phase = Phase::Playing;
                // `draw::camera` centres the focus in x: the view's left
                // edge lands on SWEEP_X0 + step.
                p.game.cam.focus[0] = SWEEP_X0 + step as f32 + view[0] * 0.5;
                p.game.cam.prev = p.game.cam.focus;
            }
            a.world.res_mut::<evani_core::time::Time>().alpha = 1.0;
            a.world.res_mut::<RenderWorld>().clear();
            draw_system(&mut a.world);
            let screen = a.world.res::<DrawState>().screen;
            sweep.push(json!({"tick": base.tick, "t": r(base.tick as f32 * DT), "scale": screen.scale, "cam": [screen.cam[0], screen.cam[1]],
                "view": [r(screen.view[0]), r(screen.view[1])], "grounded": false,
                "sprites": sprites_of(&a.world.res::<RenderWorld>(), &screen)}));
        }
    }

    let popups: Vec<Value> = pops.into_iter().map(|(id, (text, samples))| json!({"id": id, "text": text, "samples": samples})).collect();
    let white = frame_of(art.white.cell);
    let sky = art.sky.get(art.sky.len() / 2).copied().unwrap_or([0.3, 0.5, 0.9, 1.0]);
    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    let bg = format!("#{:02x}{:02x}{:02x}", to_srgb(sky[0]), to_srgb(sky[1]), to_srgb(sky[2]));

    let layer_names: serde_json::Map<String, Value> = [
        (layer::SKY, "sky"),
        (layer::FAR, "far hills"),
        (layer::FAR + 1, "far castle"),
        (layer::MID, "mid hills"),
        (layer::NEAR, "near hills"),
        (layer::CASTLE, "castle"),
        (layer::DECO, "decor"),
        (layer::TILES, "tiles"),
        (layer::PLATFORMS, "platforms"),
        (layer::COINS, "coins"),
        (layer::SHADOWS, "shadows"),
        (layer::ENEMIES, "enemies"),
        (layer::HERO, "hero"),
        (layer::WATER, "water"),
        (layer::POPS, "popups"),
        (layer::BANNER, "level clear banner"),
        (layer::HUD, "HUD"),
    ]
    .into_iter()
    .map(|(l, n)| (l.to_string(), json!(n)))
    .collect();
    let scene = json!({
        "format": "evani-geocheck-2d/1",
        "name": level.name,
        "source": "kits/evani-kit-2d-platformer (Bramble Hills), bot run + draw_system at 1920x1080, pixel16",
        "tile_size": 16.0,
        "map": [level.w, level.h],
        "pixel_scale": a.world.res::<DrawState>().screen.scale,
        "background": bg,
        "atlas": {"image": "atlas.rgba", "width": sheet.width, "height": sheet.height, "frames": sheet.frames, "fills": white.map(|w| vec![w]).unwrap_or_default()},
        "layers": {"world_min": layer::FAR, "hud_min": layer::HUD},
        "layer_names": layer_names,
        "notes": [
            "There is no docs/playbooks folder yet (STAGES.md names genre playbooks there) and the catalog gives no 2D framing zone, so the camera check uses Chris's numbers: feet in the lower third of the view, terrain below the player at most 35 % of the screen.",
            "Scene dumped by the kit's geocheck_dump example: the kit's own level data, bot (bot::play) and draw system at 1920 x 1080 with the pixel16 look. Frames are sampled every 6 ticks (camera) and every 30 ticks, on landings and 4 ticks after each popup spawns (sprites).",
            format!("The camera sweep (check 8, draw joins) draws the level-start game {SWEEP} times with the view's left edge at x {SWEEP_X0}, {}, ... {}: one frame per whole-art-pixel camera step, so every sub-pixel offset of the three parallax planes is seen with each plane's join between two copies on screen.", SWEEP_X0 + 1.0, SWEEP_X0 + SWEEP as f32 - 1.0),
            "Popup boxes and alpha come from the kit's own draw::text_pops (eased rise, fade, laid out clear of the hero box and of each other), with the camera at 0,0 so the boxes are in world art pixels; the popup player rect is the hero sprite at its feet, as in the first dump."
        ],
        "groups": {
            "earth": {"caps": "tlr", "meets": []},
            "stone": {"caps": "tlr", "meets": []},
            "plank": {"caps": "lr", "meets": []},
            "ledge": {"caps": "lr", "meets": []},
            "water": {"caps": "t", "meets": []},
            "castle": {"caps": "", "meets": []}
        },
        "tiles": tiles,
        "objects": objects,
        "platforms": platforms,
        "player": {"w": t.hero_w, "h": t.hero_h, "gravity": gravity, "fall_mult": t.fall_gravity_mult, "max_fall": t.max_fall,
            "jump_speed": t.jump_speed, "run_jump_bonus": t.run_jump_bonus, "jump_cut": t.jump_cut, "walk_speed": t.walk_speed,
            "run_speed": t.run_speed, "start": level.start, "sprite": hero_sprite},
        "bot": {"dt": DT, "completed": run.won_at.is_some(), "end_tick": run.won_at.unwrap_or(run.controls.len()), "ticks": ticks, "touched": touched},
        "frames": frames,
        "sweep": sweep,
        "popups": popups,
        // Intentional cases tagged in the level data, passed through.
        "geocheck": serde_json::from_str::<Value>(evani_kit_2d_platformer::LEVEL_JSON).ok().and_then(|v| v.get("geocheck").cloned()).unwrap_or(json!([]))
    });
    std::fs::write(out.join("scene.json"), serde_json::to_string(&scene).unwrap()).unwrap();
    eprintln!(
        "wrote {} ({} tiles, {} objects, {} frames, {} popups, bot {} at tick {:?})",
        out.join("scene.json").display(),
        scene["tiles"].as_array().map_or(0, Vec::len),
        scene["objects"].as_array().map_or(0, Vec::len),
        scene["frames"].as_array().map_or(0, Vec::len),
        scene["popups"].as_array().map_or(0, Vec::len),
        if run.won_at.is_some() { "won" } else { "did not finish" },
        run.won_at
    );
}
