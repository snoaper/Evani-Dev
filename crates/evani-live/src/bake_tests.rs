//! The bake round trip: tune a live session, bake it into files, then build
//! the game the way it ships (no live module) from those files and check it
//! comes up exactly as tuned.

use std::path::{Path, PathBuf};

use evani_core::App;
use evani_core::ecs::World;
use evani_core::formulas::Formulas;
use evani_core::params::ParamRegistry;
use evani_core::plugin::{BakedFiles, EngineModule, PluginContext, PluginError};
use evani_core::slots::{ShaderSlots, SlotRebuilds};
use evani_core::tiers::{TierPreset, Tiers};
use serde_json::{Value, json};

use crate::bake::{BakePlan, plan};
use crate::{FullState, LiveModule, submit, take_replies};

const CURVE: &str = "kit.example.spin_falloff";
const EXPR: &str = "kit.example.boost_scale";

fn new_app(live: bool, extra: Option<BakedModule>) -> App {
    let mut a = App::new("sandbox")
        .with_kit(evani_kit_example::ExampleKitPlugin)
        .with_look(evani_look_basic::BasicLook)
        .with_module(evani_render::RenderModule::new())
        .with_module(evani_fx::FxModule::compiled());
    if live {
        a = a.with_module(LiveModule::new());
    }
    if let Some(m) = extra {
        a = a.with_module(m);
    }
    a.run_headless(2, 16.0);
    a
}

/// Stands in for the look and kit crates' `BAKED` tables, read from the
/// planned files instead of the repo.
struct BakedModule {
    files: Vec<BakedFiles>,
}

impl EngineModule for BakedModule {
    fn id(&self) -> &'static str {
        "evani.module.test_baked"
    }
    fn label(&self) -> &'static str {
        "test_baked"
    }
    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        for f in &self.files {
            ctx.baked_files(f);
        }
        Ok(())
    }
}

/// The crate's baked files under `dir`, as `tools/bake-index.rs` would embed them.
fn baked_from(dir: &Path) -> BakedFiles {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    let mut add = |rel: String, p: PathBuf| {
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push((
                Box::leak(rel.into_boxed_str()),
                Box::leak(text.into_boxed_str()),
            ));
        }
    };
    for name in ["params.json", "policy.json"] {
        add(name.to_string(), dir.join(name));
    }
    for sub in ["slots", "curves"] {
        if let Ok(rd) = std::fs::read_dir(dir.join(sub)) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                add(format!("{sub}/{n}"), e.path());
            }
        }
    }
    out.sort();
    Box::leak(out.into_boxed_slice())
}

fn send(a: &mut App, token: u64, cmd: &str, params: Value) -> Value {
    send_note(a, token, cmd, params, &format!("tuned {cmd}"))
}

fn send_note(a: &mut App, token: u64, cmd: &str, params: Value, note: &str) -> Value {
    let req = json!({ "type": "cmd", "v": 1, "id": format!("b-{token}"), "cmd": cmd,
                      "params": params, "note": note, "source": "mcp",
                      "opts": { "telemetry_ms": 0 } });
    submit(&a.world, token, &req.to_string());
    for _ in 0..40 {
        a.run_headless(1, 16.0);
        // No renderer: accept every slot body as compiled.
        let reqs = a.world.res_mut::<SlotRebuilds>().take_pending();
        for r in reqs {
            a.world
                .res_mut::<ShaderSlots>()
                .set_body(r.slot, &r.body)
                .unwrap();
            a.world.res_mut::<SlotRebuilds>().complete(
                r.handle,
                evani_core::slots::SlotCompileResult {
                    ok: true,
                    ms: 1.0,
                    kept: None,
                    messages: vec![],
                    pipelines_rebuilt: vec![],
                },
            );
        }
        let replies = take_replies(&a.world);
        if let Some((_, text)) = replies.into_iter().find(|(t, _)| *t == token) {
            a.run_headless(2, 16.0);
            let r: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(r["ok"], true, "{cmd} -> {r}");
            return r["result"].clone();
        }
    }
    panic!("no reply to {cmd}");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_plan(p: &BakePlan, dir: &Path) {
    for f in &p.files {
        let to = dir.join(&f.path);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, &f.contents).unwrap();
    }
}

fn param(world: &World, name: &str) -> Value {
    let reg = world.res::<ParamRegistry>();
    let id = reg.id(name).unwrap();
    reg.def(id).unwrap().value_to_json(&reg.value(id).unwrap())
}

#[test]
fn bake_round_trip_rebuilds_the_tuned_session_without_live() {
    let mut a = new_app(true, None);
    let tonemap = {
        let s = a.world.res::<ShaderSlots>();
        s.get(s.id("tonemap").unwrap()).unwrap().body.clone()
    };
    let new_tonemap = format!("{tonemap}\n// tuned live");
    let active = a.world.res::<Tiers>().active().id.clone();
    let steps: Vec<(&str, Value)> = vec![
        (
            "param.set",
            json!({ "values": { "render.saturation": 1.35 } }),
        ),
        (
            "param.set",
            json!({ "values": { "kit.example.spin_speed": 4.25 } }),
        ),
        ("policy.set", json!({ "set": { "hysteresis_pct": 15 } })),
        (
            "slot.set",
            json!({ "name": "tonemap", "source": new_tonemap }),
        ),
        (
            "slot.set",
            json!({ "name": EXPR, "source": "mix(1, boost * 2, held)" }),
        ),
        (
            "curve.set",
            json!({ "curves": { CURVE: [[0.5, 1.0], [1.0, 2.0], [1.5, 0.5]] } }),
        ),
        (
            "effect.toggle",
            json!({ "effect": "vignette", "on": false, "scope": "tier" }),
        ),
        // Undone: must not be baked.
        (
            "param.set",
            json!({ "values": { "kit.example.count": 99 } }),
        ),
    ];
    let mut token = 0;
    for (cmd, p) in steps {
        token += 1;
        send(&mut a, token, cmd, p);
    }
    token += 1;
    send(&mut a, token, "history.undo", json!({}));
    let tuned = FullState::capture(&a.world);
    let hist_len = a.world.res::<crate::LiveSession>().history.len();

    token += 1;
    let res = send(
        &mut a,
        token,
        "bake.export",
        json!({ "title": "Round trip" }),
    );
    let bundle = &res["bundle"];
    assert_eq!(bundle["look_crate"], "evani-look-basic");
    assert_eq!(bundle["kit_crate"], "evani-kit-example");
    let names: Vec<&str> = bundle["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"kit.example.count"), "{names:?}");
    // bake.export is not a transaction.
    assert_eq!(a.world.res::<crate::LiveSession>().history.len(), hist_len);

    let root = repo_root();
    let p = plan(
        bundle,
        |rel| std::fs::read_to_string(root.join(rel)).ok(),
        "2026-10-14 12:00",
    )
    .unwrap();
    let paths: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
    for want in [
        "looks/evani-look-basic/params.json",
        "looks/evani-look-basic/policy.json",
        "looks/evani-look-basic/slots/tonemap.wgsl",
        "looks/evani-look-basic/CHANGES.md",
        "kits/evani-kit-example/params.json",
        "kits/evani-kit-example/slots/kit.example.boost_scale.expr",
        "kits/evani-kit-example/curves/kit.example.spin_falloff.json",
    ] {
        assert!(paths.contains(&want), "{want} not in {paths:?}");
    }
    let md = &p
        .files
        .iter()
        .find(|f| f.path.ends_with("CHANGES.md"))
        .unwrap()
        .contents;
    assert!(md.contains("UTC — Round trip (tx 1–7)"), "{md}");
    assert!(
        md.contains("**render.saturation** 1.05 → 1.35 — tuned param.set *(AI)*"),
        "{md}"
    );

    let dir = std::env::temp_dir().join(format!("evani-bake-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write_plan(&p, &dir);

    // Tiers: the planned preset equals the session's.
    let tier_file = format!("assets/tiers/{active}.json");
    let planned = p.files.iter().find(|f| f.path == tier_file).unwrap();
    let preset: TierPreset = serde_json::from_str(&planned.contents).unwrap();
    let want = a
        .world
        .res::<Tiers>()
        .presets()
        .iter()
        .find(|t| t.id == active)
        .cloned();
    assert_eq!(Some(preset), want);

    // The shipped game: no live module, baked files from the temp dir.
    let shipped = new_app(
        false,
        Some(BakedModule {
            files: vec![
                baked_from(&dir.join("kits/evani-kit-example")),
                baked_from(&dir.join("looks/evani-look-basic")),
            ],
        }),
    );
    let w = &shipped.world;
    assert!(w.try_res::<crate::LiveSession>().is_none());
    for name in [
        "render.saturation",
        "kit.example.spin_speed",
        "aq.hysteresis_pct",
        "kit.example.count",
    ] {
        assert_eq!(param(w, name), param(&a.world, name), "{name}");
    }
    assert_eq!(param(w, "kit.example.count"), json!(64));
    {
        let s = w.res::<ShaderSlots>();
        assert_eq!(s.get(s.id("tonemap").unwrap()).unwrap().body, new_tonemap);
    }
    let shipped_state = FullState::capture(w);
    let f = w.res::<Formulas>();
    for name in [CURVE, EXPR] {
        let id = f.id(name).unwrap();
        let tuned_body = tuned.formulas.iter().find(|(i, _)| *i == id).map(|x| &x.1);
        let got = shipped_state
            .formulas
            .iter()
            .find(|(i, _)| *i == id)
            .map(|x| &x.1);
        assert_eq!(got, tuned_body, "{name}");
    }
    // Every bakeable param matches (live.* params only exist with live).
    let reg = w.res::<ParamRegistry>();
    for (id, v) in &shipped_state.params {
        let name = &reg.def(*id).unwrap().name;
        // Follows the tier preset, which ships through assets/tiers (checked above).
        if name == "fx.vignette.enabled" {
            continue;
        }
        let live_reg = a.world.res::<ParamRegistry>();
        let lid = live_reg.id(name).unwrap();
        let tv = tuned.params.iter().find(|(i, _)| *i == lid).unwrap().1;
        assert_eq!(*v, tv, "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changes_md_lists_every_note_for_a_value_changed_twice() {
    let mut a = new_app(true, None);
    for (i, (v, note)) in [(1.1, "warmer"), (1.3, "even warmer"), (1.4, "warmer")]
        .into_iter()
        .enumerate()
    {
        send_note(
            &mut a,
            i as u64 + 1,
            "param.set",
            json!({ "values": { "render.saturation": v } }),
            note,
        );
    }
    let res = send(&mut a, 10, "bake.export", json!({ "title": "Notes" }));
    let p = plan(&res["bundle"], |_| None, "2026-10-14 12:00").unwrap();
    let md = &p
        .files
        .iter()
        .find(|f| f.path.ends_with("CHANGES.md"))
        .unwrap()
        .contents;
    assert!(md.contains("2026-10-14 12:00 UTC — Notes"), "{md}");
    assert!(
        md.contains("**render.saturation** 1.05 → 1.4 — warmer *(AI)*; even warmer *(AI)*\n"),
        "{md}"
    );
    assert!(
        p.to_result()["rebuild"]
            .as_str()
            .unwrap()
            .starts_with("cargo xtask build ")
    );
}

/// lean-data: the tier presets ship as a table compiled from
/// `assets/tiers/*.json` (evani-core build.rs), and live editing still
/// works on top of it: values set live (a param and a tier preset's
/// effects) come back from a dry-run `bake.export`, the plan writes the
/// tier into its JSON file (the source of truth the next build compiles),
/// and nothing is written to disk.
#[test]
fn live_edits_and_dry_run_bake_work_over_the_compiled_tier_table() {
    assert_eq!(
        Tiers::builtin().unwrap().presets(),
        Tiers::from_builtin_json().unwrap().presets()
    );
    let mut a = new_app(true, None);
    let active = a.world.res::<Tiers>().active().id.clone();
    send(&mut a, 1, "param.set", json!({ "values": { "render.saturation": 1.2 } }));
    assert!((param(&a.world, "render.saturation").as_f64().unwrap() - 1.2).abs() < 1e-6);
    send(
        &mut a,
        2,
        "effect.toggle",
        json!({ "effect": "vignette", "on": false, "scope": "tier" }),
    );
    let res = send(&mut a, 3, "bake.export", json!({ "title": "Lean data", "dry_run": true }));
    let bundle = &res["bundle"];
    assert_eq!(bundle["dry_run"], true);
    let names: Vec<&str> = bundle["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"render.saturation"), "{bundle}");
    let root = repo_root();
    let p = plan(bundle, |rel| std::fs::read_to_string(root.join(rel)).ok(), "2026-10-16 12:00").unwrap();
    let tier_file = format!("assets/tiers/{active}.json");
    let planned = p.files.iter().find(|f| f.path == tier_file).unwrap();
    let preset: TierPreset = serde_json::from_str(&planned.contents).unwrap();
    assert!(!preset.effects_on.iter().any(|e| e == "vignette"), "{}", planned.contents);
    // Apart from the tuned effects, the planned file is the compiled preset.
    let mut compiled = Tiers::builtin().unwrap().presets().iter().find(|t| t.id == active).cloned().unwrap();
    compiled.effects_on = preset.effects_on.clone();
    assert_eq!(preset, compiled);
    // Dry run: the repo's tier file is untouched.
    let on_disk = std::fs::read_to_string(root.join(&tier_file)).unwrap();
    let disk: TierPreset = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(
        Some(&disk),
        Tiers::from_builtin_json().unwrap().presets().iter().find(|t| t.id == active)
    );
}
