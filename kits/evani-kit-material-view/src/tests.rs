use evani_core::App;
use evani_core::input::{Input, KeyCode, RawEvent};
use evani_core::params::{ParamDef, ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;

use super::*;
use crate::library::{Groups, panel_rows, under};

const DT: f64 = 1000.0 / 60.0;

/// A stand-in for the layered family: registers two of its params the way
/// evani-material-layers does, so the kit can be tested without the app.
fn fake_layers(ctx: &mut PluginContext) {
    for name in ["material.layers.clearcoat_strength", "material.layers.iridescence_strength", "material.layers.weave_scale"] {
        let _ = ctx.param_f32(ParamDef::f32(name, 1.0).range(0.0, 2.0));
    }
    let _ = ctx.param_enum(ParamDef::enumeration("material.layers.quality", &["auto", "cheap", "full"], 0));
}

fn key(a: &mut App, code: &str, down: bool) {
    let code = KeyCode::new(code).unwrap();
    a.world.res_mut::<Input>().handle(&RawEvent::Key { code, down });
}

fn app_with_layers() -> App {
    let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(MaterialViewKitPlugin);
    fake_layers(&mut a.context("test"));
    Viewer::enable(&mut a.world, "layers");
    a
}

fn data() -> Data {
    Data::shipped().unwrap()
}

#[test]
fn the_shipped_data_parses_and_cross_checks() {
    let d = data();
    assert_eq!(d.library.scenes.len(), 9);
    assert_eq!(d.library.scenes[0].id, "styles", "the art styles open first when built in");
    assert_eq!(d.library.scenes[1].id, "ball", "else the shader ball");
    assert!(d.library.scenes.iter().any(|s| s.id == "studio"), "the Asset & Style Studio's view");
    // The studio's procedural section (water, glass, sky) and its items' cameras.
    assert!(d.library.scenes.iter().any(|s| s.id == "procedural" && s.family == "studio"));
    for item in ["ocean", "water", "glass", "sky"] {
        let id = format!("studio.proc.{item}");
        assert!(d.material(&id).is_some_and(|(_, m)| m.scene == "procedural" && m.orbit.is_some()), "{id}");
    }
    assert_eq!(d.library.families.len(), 6);
    // The styles and the studio keep their params between rows (the
    // studio's record owns them).
    assert!(d.family("styles").is_some_and(|f| f.keep_params));
    assert!(d.family("studio").is_some_and(|f| f.keep_params));
    assert!(d.library.families.iter().filter(|f| f.id != "styles" && f.id != "studio").all(|f| !f.keep_params));
    assert_eq!(d.lights.presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Studio", "Overcast", "Sunset", "Night lamp", "Daylight"]);
    assert_eq!(d.rooms.rooms.len(), 24);
    let slots: usize = d.rooms.rooms.iter().map(|r| r.slots.len()).sum();
    assert_eq!(slots, 209);
    // Every family's slots open its default; GAP slots open nothing.
    for r in &d.rooms.rooms {
        for s in &r.slots {
            assert_eq!(d.slot_target(s).is_none(), s.family == "GAP", "{}", s.id);
        }
    }
}

#[test]
fn a_bad_reference_is_refused() {
    let lib = library::LIBRARY_JSON.replace("\"scene\": \"ball\", \"show\": \"bubble\"", "\"scene\": \"nowhere\", \"show\": \"bubble\"");
    assert_ne!(lib, library::LIBRARY_JSON);
    let e = Data::parse(&lib, library::LIGHTS_JSON, library::ROOMS_JSON).unwrap_err();
    assert!(e.contains("nowhere"), "{e}");
    let rooms = library::ROOMS_JSON.replacen("\"family\": \"nature\"", "\"family\": \"lava\"", 1);
    assert!(Data::parse(library::LIBRARY_JSON, library::LIGHTS_JSON, &rooms).unwrap_err().contains("lava"));
}

#[test]
fn groups_sort_params_into_base_finish_and_detail() {
    let g: Groups = data().library.groups;
    assert_eq!(g.group_of("material.relief.brick.color"), Some("base"));
    assert_eq!(g.group_of("material.nature.water.clarity"), Some("base"));
    assert_eq!(g.group_of("material.layers.clearcoat_strength"), Some("finish"));
    assert_eq!(g.group_of("material.nature.water.roughness"), Some("finish"));
    assert_eq!(g.group_of("material.layers.weave_scale"), Some("detail"));
    assert_eq!(g.group_of("material.relief.brick.mortar"), Some("detail"));
    assert_eq!(g.group_of("material.layers.quality"), Some("quality"));
    assert_eq!(g.group_of("material.character.slots"), None);
    assert!(under("material.relief.brick.color", "material.relief.brick"));
    assert!(!under("material.relief.bricks", "material.relief.brick"));
}

#[test]
fn starts_on_the_shader_ball_and_opens_materials() {
    let mut a = app_with_layers();
    a.run_headless(2, DT);
    let v = a.world.res::<Viewer>().clone();
    assert_eq!((v.scene.as_str(), v.material.as_str()), ("ball", "layers.clearcoat"));
    drop(v);
    // A live change, then opening another material resets the family first.
    a.world.res_mut::<ParamRegistry>().set_by_name("material.layers.clearcoat_strength", ParamValue::F32(0.2)).unwrap();
    a.world.res_mut::<Viewer>().push(Command::Material("layers.soap_film".into()));
    a.run_headless(1, DT);
    let reg = a.world.res::<ParamRegistry>();
    let get = |n: &str| reg.id(n).and_then(|id| reg.value(id));
    assert_eq!(get("material.layers.clearcoat_strength"), Some(ParamValue::F32(1.0)));
    assert_eq!(get("material.layers.iridescence_strength"), Some(ParamValue::F32(1.2)));
    drop(reg);
    assert_eq!(a.world.res::<Viewer>().material, "layers.soap_film");
    // A family this build lacks is not opened.
    a.world.res_mut::<Viewer>().push(Command::Scene("head".into()));
    a.run_headless(1, DT);
    assert_eq!(a.world.res::<Viewer>().scene, "ball");
}

#[test]
fn the_art_styles_open_first_and_keep_their_params() {
    let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(MaterialViewKitPlugin);
    styles::install(&mut a.context("material-view")).unwrap();
    Viewer::enable(&mut a.world, "styles");
    Viewer::enable(&mut a.world, "layers");
    a.run_headless(2, DT);
    assert_eq!(a.world.res::<Viewer>().scene, "styles");
    let p = *a.world.res::<styles::StyleParams>();
    a.world.res_mut::<ParamRegistry>().set_f32(p.bump, 0.66).unwrap();
    a.run_headless(1, DT);
    a.world.res_mut::<Viewer>().push(Command::Material("styles.row".into()));
    a.run_headless(1, DT);
    assert_eq!(a.world.res::<ParamRegistry>().f32(p.bump), 0.66, "opening the styles family resets nothing");
}

#[test]
fn the_panel_is_generated_from_the_registered_params() {
    let a = app_with_layers();
    let d = data();
    let reg = a.world.res::<ParamRegistry>();
    let fam = d.family("layers").unwrap();
    let (_, coat) = d.material("layers.clearcoat").unwrap();
    let rows = panel_rows(&reg, &d.library.groups, fam, Some(coat));
    let names: Vec<_> = rows.iter().map(|r| (r.group, r.def.name.as_str())).collect();
    assert_eq!(names, [("finish", "material.layers.clearcoat_strength"), ("quality", "material.layers.quality")]);
    // A material whose own params are missing falls back to the family's.
    let (_, velvet) = d.material("layers.velvet").unwrap();
    let rows = panel_rows(&reg, &d.library.groups, fam, Some(velvet));
    assert!(rows.iter().any(|r| r.def.name == "material.layers.weave_scale"));
}

#[test]
fn lights_change_only_when_the_viewer_changes_them() {
    let mut a = app_with_layers();
    a.run_headless(2, DT);
    let dir = |a: &App| {
        let reg = a.world.res::<ParamRegistry>();
        reg.id("render.light.direction").and_then(|id| reg.value(id))
    };
    let studio = viewer::sun_direction(42.0, 38.0);
    assert_eq!(dir(&a), Some(ParamValue::Vec3(studio)));
    // A live edit stays.
    a.world.res_mut::<ParamRegistry>().set_by_name("render.light.direction", ParamValue::Vec3([0.0, 1.0, 0.0])).unwrap();
    a.run_headless(5, DT);
    assert_eq!(dir(&a), Some(ParamValue::Vec3([0.0, 1.0, 0.0])));
    // A preset chip and a sun drag write it.
    a.world.res_mut::<Viewer>().push(Command::Light(2));
    a.run_headless(1, DT);
    assert_eq!(dir(&a), Some(ParamValue::Vec3(viewer::sun_direction(7.0, 250.0))));
    a.world.res_mut::<Viewer>().push(Command::Sun { daz: 10.0, del: 5.0 });
    a.run_headless(1, DT);
    assert_eq!(dir(&a), Some(ParamValue::Vec3(viewer::sun_direction(12.0, 260.0))));
}

#[test]
fn the_turntable_holds_while_paused_or_comparing() {
    let mut a = app_with_layers();
    a.run_headless(2, DT);
    let az = |a: &App| a.world.res::<evani_render::RenderWorld>().camera.eye;
    let before = az(&a);
    a.run_headless(30, DT);
    assert_ne!(az(&a), before, "it turns");
    for c in [Command::Pause(Some(true)), Command::Pause(Some(false)), Command::Compare(Some(true))] {
        let paused = c != Command::Pause(Some(false));
        a.world.res_mut::<Viewer>().push(c);
        a.run_headless(1, DT);
        let held = az(&a);
        a.run_headless(20, DT);
        assert_eq!(az(&a) == held, paused);
    }
}

#[test]
fn keys_drive_the_same_commands_as_the_page() {
    let mut a = app_with_layers();
    a.run_headless(2, DT);
    a.world.res_mut::<Viewer>().push(Command::Turntable(Some(false)));
    a.run_headless(1, DT);
    let az0 = a.world.res::<Viewer>().orbit.az;
    key(&mut a, "ArrowLeft", true);
    a.run_headless(30, DT);
    key(&mut a, "ArrowLeft", false);
    a.run_headless(1, DT);
    let az1 = a.world.res::<Viewer>().orbit.az;
    assert!((az1 - az0).rem_euclid(360.0) > 20.0, "{az0} -> {az1}");
    key(&mut a, "KeyR", true);
    a.run_headless(1, DT);
    key(&mut a, "KeyR", false);
    a.run_headless(1, DT);
    assert_eq!(a.world.res::<Viewer>().orbit.az, a.world.res::<Viewer>().home.az);
    // Every touch control names an action a key drives.
    let input = a.world.res::<Input>();
    for c in ["reset", "turntable", "pause"] {
        assert!(input.action_id(c).is_some(), "{c}");
    }
}

/// Free roam: two minutes of keys with no error and the viewer still there.
#[test]
fn free_roam_holds() {
    use evani_core::play::{PlayMode, ScriptKey, free_roam_test};
    let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(MaterialViewKitPlugin);
    let mut script = Vec::new();
    for i in 0..10 {
        let at = 2.0 + f64::from(i) * 10.0;
        for (k, key) in ["ArrowLeft", "KeyN", "KeyL", "KeyT", "KeyR"].iter().enumerate() {
            let t = at + k as f64;
            script.push(ScriptKey::down(t, key));
            script.push(ScriptKey::up(t + 0.5, key));
        }
    }
    let r = free_roam_test(&mut app, 120.0, &script).unwrap();
    assert_eq!(r.keys, script.len());
    assert!(r.sim_seconds > 119.0, "{r:?}");
    assert!(app.world.has_resource::<Viewer>());
}

/// Names of existing games, studios and brands that the viewer must never
/// show (critic round 1 found "Super Mario Odyssey" in a style header).
/// Matched as whole words, case-insensitive, in every data file the viewer
/// shows, the page and the material catalogue's names and search words.
const NOT_OURS: &[&str] = &[
    "mario", "nintendo", "animal crossing", "zelda", "breath of the wild", "genshin", "sea of thieves", "art of rally",
    "stardew", "octopath", "hd-2d", "dread delusion", "cuphead", "wytchwood", "okami", "ōkami", "hollow knight",
    "tearaway", "hylics", "obra dinn", "red dead", "fortnite", "minecraft", "roblox", "overwatch", "borderlands",
    "pixar", "ghibli", "disney", "pokemon", "pokémon", "lego", "kevlar", "perspex", "plexiglas", "bakelite",
    "vaseline", "tarmac", "velcro", "teflon", "formica", "gore-tex", "lycra", "styrofoam", "mithril", "vibranium",
    "adamantium", "kryptonite", "beskar", "lightsaber",
];

/// Whether `text` holds `name` as a whole word (or phrase).
fn has_word(text: &str, name: &str) -> bool {
    let t = text.to_lowercase();
    let mut from = 0;
    while let Some(at) = t[from..].find(name) {
        let s = from + at;
        let e = s + name.len();
        let before = t[..s].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = t[e..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = e;
    }
    false
}

#[test]
fn nothing_the_viewer_shows_names_an_existing_game_or_brand() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut texts: Vec<(String, String)> = ["styles.json", "library.json", "world_rooms.json", "planned.json", "lights.json"]
        .iter()
        .map(|f| (f.to_string(), std::fs::read_to_string(format!("{}/data/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()))
        .collect();
    texts.push(("index.html".into(), std::fs::read_to_string(format!("{root}/apps/material-view/index.html")).unwrap()));
    // The studio's asset catalogue: names, labels and search words are shown.
    texts.push(("assets.json".into(), std::fs::read_to_string(format!("{root}/catalog/assets/assets.json")).unwrap()));
    // The catalogue's shown text: names and search words (ids stay as they are).
    let cat: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{root}/catalog/materials/materials.json")).unwrap()).unwrap();
    let rows = cat.get("materials").or_else(|| cat.get("rows")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    assert!(rows.len() > 1000, "catalogue rows: {}", rows.len());
    for r in &rows {
        for k in ["name", "search_words", "look_note"] {
            if let Some(s) = r.get(k).and_then(|v| v.as_str()) {
                texts.push((format!("catalogue {k}"), s.to_string()));
            }
        }
    }
    let mut found = Vec::new();
    for (file, text) in &texts {
        for name in NOT_OURS {
            // Ids such as `plastic.bakelite` are not shown as words; only
            // the JSON's string values count.
            if has_word(text, name) {
                found.push(format!("{file}: {name}"));
            }
        }
    }
    assert!(found.is_empty(), "existing game or brand names: {found:?}");
    assert!(has_word("A Mario hat", "mario") && !has_word("marionette", "mario"));
}
