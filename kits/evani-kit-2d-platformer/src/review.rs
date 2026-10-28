//! What `cargo xtask review` reads from the kit (docs/KIT_PROCESS.md, "Kit
//! cooperation"): the playtest bot's win and lose (`playtest_win*`,
//! `playtest_lose*`) and `review_dump`, which measures the level's own
//! objects (gap to the ground under their drawn feet) and the units' main
//! colours and writes `$EVANI_REVIEW_DUMP/review.json`.

use serde_json::{Value, json};

use crate::checks::{bot_run, level};
use crate::game::{Controls, Game, Phase};
use crate::level::{Level, TILE};
use crate::{ART_JSON, Ints, Tune};

const DT: f32 = 1.0 / 60.0;

fn tick(g: &mut Game, t: &Tune, n: &Ints, c: Controls, ticks: usize) {
    for _ in 0..ticks {
        g.tick(c, t, n, t.gravity * t.gravity_scale, DT);
    }
}

#[test]
fn playtest_win_the_bot_reaches_the_castle_with_lives_to_spare() {
    let (t, n) = (Tune::default(), Ints::default());
    let (g, run) = bot_run(&t, &n, |_, _| {});
    assert!(run.won_at.is_some(), "the bot never reached the castle (phase {:?}, x {})", g.phase, g.hero.body.pos[0]);
    assert_eq!(g.phase, Phase::Won);
    assert!(g.lives > 0);
    // Not stuck: it gets there well inside the level's time.
    assert!(run.won_at.unwrap_or(usize::MAX) < 60 * 150, "{:?}", run.won_at);
}

#[test]
fn playtest_lose_walking_into_the_pit_costs_every_life() {
    let (t, n) = (Tune::default(), Ints::default());
    let mut g = Game::new(level(), &t, &n);
    g.reset(Phase::Playing, &t, &n);
    g.enemies.clear();
    let walk = Controls { x: 1.0, run: true, ..Controls::default() };
    let mut ticks = 0;
    while g.phase == Phase::Playing {
        tick(&mut g, &t, &n, walk, 1);
        ticks += 1;
        assert!(ticks < 60 * 120, "never lost: lives {} at {:?}", g.lives, g.hero.body.pos);
    }
    assert_eq!(g.phase, Phase::Lost);
    assert_eq!(g.lives, 0);
}

/// The ground height under `(x, y)`, within a tile either way.
fn ground_at(l: &Level, x: f32, y: f32) -> Option<f32> {
    l.surface(x - 1.0, x + 1.0, x, y - TILE, y + TILE, None).map(|s| s.y)
}

/// `{"name", "gap", "intentional"?, "contact_shadow"}`: `drawn_feet` is the
/// y the sprite's feet are drawn at; the gap is the air between it and the
/// ground (negative: sunk into it).
fn object(l: &Level, name: String, pos: [f32; 2], drawn_dy: f32, why: Option<&str>, shadow: bool) -> Value {
    let feet = pos[1] + drawn_dy;
    let gap = ground_at(l, pos[0], pos[1]).map_or(f64::MAX, |g| f64::from(g - feet));
    let mut o = json!({ "name": name, "gap": gap, "contact_shadow": shadow });
    if let Some(w) = why {
        o["intentional"] = json!(w);
    }
    o
}

/// `#rrggbb` of a ramp shade in art.json.
fn ramp_hex(art: &Value, ramp: &str, shade: usize) -> String {
    format!("#{}", art["ramps"][ramp][shade].as_str().expect("ramp colour"))
}

#[test]
fn review_dump() {
    let l = level();
    let art: Value = serde_json::from_str(ART_JSON).expect("art.json");
    let mut objects = Vec::new();
    objects.push(object(&l, "Fennel at the start".into(), l.start, 1.0, None, true));
    for (i, p) in l.beetles.iter().enumerate() {
        objects.push(object(&l, format!("beetle {i}"), *p, 1.0, None, true));
    }
    for (i, p) in l.hoppers.iter().enumerate() {
        objects.push(object(&l, format!("hopper {i}"), *p, 1.0, None, true));
    }
    for (i, p) in l.checkpoints.iter().enumerate() {
        objects.push(object(&l, format!("checkpoint flag {i}"), [p[0] + 3.0, p[1]], 1.0, None, false));
    }
    objects.push(object(&l, "castle gate".into(), l.goal, 0.0, None, false));
    for (i, p) in l.torches.iter().enumerate() {
        let wall = "a wall torch, mounted on the castle wall at head height";
        objects.push(object(&l, format!("torch {i}"), *p, 0.0, Some(wall), false));
    }
    for (i, d) in l.decorations.iter().enumerate() {
        let tucked = "drawn 2 px into the ground on purpose, so no seam of sky shows under its base";
        objects.push(object(&l, format!("{} {i}", d.kind), d.pos, 2.0, Some(tucked), false));
    }
    for (i, c) in l.coins.iter().enumerate() {
        let hover = "a coin hovers in its arc, which follows a real jump (the bot checks it)";
        objects.push(json!({ "name": format!("coin {i} at ({:.0}, {:.0})", c[0], c[1]), "gap": 0.0, "contact_shadow": false, "intentional": hover }));
    }

    // The three kinds of unit, by their main body colour (fur, red and blue
    // ramps' third shade), and what marks the player.
    let colours = json!({
        "player": { "name": "Fennel (fox fur)", "hex": ramp_hex(&art, "fur", 2), "marker": "teal scarf and white chest, always at the camera's focus" },
        "others": [
            { "name": "beetle (red shell)", "hex": ramp_hex(&art, "red", 2) },
            { "name": "hopper (blue frog)", "hex": ramp_hex(&art, "blue", 2) },
        ],
    });
    let dump = json!({ "colours": colours, "objects": objects });
    if let Ok(dir) = std::env::var("EVANI_REVIEW_DUMP") {
        std::fs::create_dir_all(&dir).expect("dump folder");
        let path = std::path::Path::new(&dir).join("review.json");
        std::fs::write(&path, serde_json::to_string_pretty(&dump).expect("json")).expect("write review.json");
        eprintln!("review_dump: wrote {}", path.display());
    }

    // The kit's own bar: the review's (1 px of air at most unless tagged).
    let bad: Vec<String> = dump["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .filter(|o| o.get("intentional").is_none() && o["gap"].as_f64().unwrap_or(f64::MAX).abs() > 1.0)
        .map(|o| format!("{} gap {}", o["name"], o["gap"]))
        .collect();
    eprintln!("review_dump: {} objects, {} not grounded", dump["objects"].as_array().map_or(0, Vec::len), bad.len());
    assert!(bad.is_empty(), "not grounded ({}): {}", bad.len(), bad.iter().take(20).cloned().collect::<Vec<_>>().join("; "));
}
