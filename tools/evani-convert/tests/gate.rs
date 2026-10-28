//! STAGE3_PLAN S3-6's quality-gate tests. The search and the views are
//! plain Rust; the rest renders in the engine's own renderer through
//! tools/convert-runner (headless, muted Chrome), so it needs Chrome, trunk
//! and the converter's tools: a test prints `SKIPPED` and why when they are
//! missing (`EVANI_REQUIRE_TOOLS=1` makes that a failure).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use evani_convert::gate::{self, Knobs, Rig, passes, score_views, search, worst_of};
use evani_convert::metric::Rgba;
use evani_convert::runner::{Job, Runner, gate_dist};
use evani_convert::tools::{Tools, s};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let t = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"));
    let p = std::path::absolute(t).unwrap().join("evani-gate-tests").join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// One Chrome at a time (and one build of the gate page).
static BROWSER: Mutex<()> = Mutex::new(());

fn skip(test: &str, why: &str) -> Option<(Tools, Runner)> {
    if std::env::var("EVANI_REQUIRE_TOOLS").is_ok_and(|v| v == "1") {
        panic!("{test}: EVANI_REQUIRE_TOOLS=1 but {why}");
    }
    println!("SKIPPED {test}: {why}");
    None
}

/// The converter's tools and a runner serving `serve`, or `None` (SKIPPED).
fn browser_or_skip(test: &str, serve: &Path) -> Option<(Tools, Runner)> {
    let tools = match Tools::locate(&root()) {
        Ok(t) => t,
        Err(e) => return skip(test, &format!("the converter's tools are missing ({e})")),
    };
    let dist = match gate_dist(&root()) {
        Ok(d) => d,
        Err(e) => return skip(test, &format!("the gate page could not be built ({e})")),
    };
    match Runner::start(&root(), &tools.node, &dist, serve) {
        Ok(r) => Some((tools, r)),
        Err(e) => skip(test, &format!("the runner could not start Chrome ({e})")),
    }
}

fn kart() -> PathBuf {
    root().join("tests").join("fixtures").join("models").join("kart.low.glb")
}

fn rig_of(file: &Path, w: u32, h: u32) -> Rig {
    let g = evani_gltf::Gltf::from_glb(&std::fs::read(file).unwrap()).unwrap();
    let (lo, hi) = evani_convert::report::model_box(&g);
    gate::rig(lo, hi, &gate::centroids(&g), w, h)
}

fn render(r: &mut Runner, serve: &Path, file: &str, rig: &Rig, out: &str) -> Vec<Rgba> {
    let job = Job { tier: "medium".into(), width: 1920, height: 1080, model: file.into(), place: rig.place, views: rig.views.iter().map(|v| v.pose).collect(), out: serve.join("shots").join(out) };
    r.render(&job).unwrap()
}

#[test]
fn search_is_monotonic_and_repeatable() {
    let lens = [8, 4, 4, 2, 4];
    let limit = [5, 2, 1, 1, 3];
    let run = || {
        let mut tried = Vec::new();
        let r = search(&lens, &mut |i: &[usize]| {
            tried.push(i.to_vec());
            Ok(i.iter().zip(&limit).all(|(a, b)| a <= b))
        })
        .unwrap();
        (r, tried)
    };
    let (a, tried) = run();
    assert_eq!(a.as_deref(), Some(&limit[..]), "the cheapest passing setting on every knob");
    // Repeatable: the same answer through the same trials.
    let (b, tried2) = run();
    assert_eq!((a, &tried), (b, &tried2));
    // Each setting is tried once; a binary search per knob, not a sweep.
    let mut uniq = tried.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), tried.len());
    assert!(tried.len() <= 1 + 3 + 2 + 2 + 1 + 2, "{} trials", tried.len());
    // Monotonic: one step cheaper on any knob than the answer fails.
    for k in 0..lens.len() {
        if limit[k] + 1 < lens[k] {
            let mut t = limit.to_vec();
            t[k] += 1;
            assert!(!t.iter().zip(&limit).all(|(a, b)| a <= b));
        }
    }
    // A failing start is a fail, never a guess.
    assert_eq!(search(&lens, &mut |_: &[usize]| Ok(false)).unwrap(), None);
    // The knob ladders run from the best setting down and hold the tier's own
    // setting, so a tier whose own setting fails backs off to a better one.
    for t in evani_convert::ALL_TIERS {
        let lad = evani_convert::ladder(t).unwrap();
        let k = Knobs::for_tier(&lad, Some(1024), true).unwrap();
        assert_eq!(k.ratio[0].0, 1.0, "{t}: the ladder starts unsimplified");
        assert!(k.ratio.contains(&(lad.lods[0].ratio, lad.lods[0].error)), "{t}: the tier's own ratio is on its ladder");
        assert!(k.ratio.windows(2).all(|w| w[0].0 > w[1].0), "{t}: ratios fall");
        assert_eq!(k.texture[0], Some(1024), "{t}: textures start at the screen-based size");
        assert!(k.texture.windows(2).all(|w| w[0] > w[1]) && k.texture.iter().all(|x| x.unwrap() >= 64), "{t}: textures fall, never below 64");
        assert_eq!((k.vp[0], k.vn[0], k.vt[0]), (16, 16, 16), "{t}");
        assert!(k.vp.contains(&lad.vp) && k.vn.contains(&lad.vn) && k.vt.contains(&lad.vt), "{t}: the tier's own bits are on its ladder");
        // What the tier file keeps when nothing passes: the tier's own setting, not the best rung.
        assert_eq!((k.own.ratio, k.own.error, k.own.texture, k.own.vp, k.own.vn, k.own.vt), (lad.lods[0].ratio, lad.lods[0].error, Some(1024), lad.vp, lad.vn, lad.vt), "{t}");
        assert!(k.vp.windows(2).all(|w| w[0] > w[1]) && k.vn.iter().all(|b| *b == 8 || *b == 16));
        let e = Knobs::for_tier(&lad, None, false).unwrap();
        assert_eq!((e.ratio.len(), e.texture.len()), (1, 1), "{t}: Evani.Model input is only re-quantised");
    }
}

#[test]
fn format_floor_sets_the_bar() {
    use evani_convert::gate::{FLOOR_MIN, FLOOR_SLACK, SSIM_MIN, WORST_MAX, bar_for, passes_at};
    // A clean floor below SSIM_MIN lowers the bar by the slack; one above it never raises it.
    assert_eq!(bar_for(81.0, 0.04), 81.0 - FLOOR_SLACK);
    assert_eq!(bar_for(99.0, 0.0), SSIM_MIN);
    // A broken best rung (low score, or a visible worst pixel) keeps the absolute bar.
    assert_eq!(bar_for(FLOOR_MIN - 1.0, 0.0), SSIM_MIN);
    assert_eq!(bar_for(85.0, WORST_MAX + 0.01), SSIM_MIN);
    // A floor well under what clean 16-bit rounding costs (81 to 85) is a fault.
    assert_eq!(bar_for(75.0, 0.04), SSIM_MIN);
    assert_eq!(bar_for(81.0, 0.04), 76.0);
    assert_eq!(bar_for(FLOOR_MIN, 0.0), FLOOR_MIN - FLOOR_SLACK, "the floor counts from FLOOR_MIN on");
    assert_eq!(bar_for(FLOOR_MIN - 0.01, 0.0), SSIM_MIN);
    // The floor itself passes its own bar; the worst-pixel ceiling still applies.
    let bar = bar_for(81.0, 0.04);
    assert!(passes_at(81.0, 0.04, bar));
    assert!(!passes_at(81.0, WORST_MAX + 0.01, bar));
    assert!(!passes_at(bar - 0.1, 0.0, bar));
    // The finished file must look like the gate's trial of its setting (kart_fixture once
    // scored 94 in the trial and 86 as the tier file, which was made another way).
    use evani_convert::gate::{DRIFT_MAX, drifted};
    assert!(!drifted(94.0, 94.0));
    assert!(drifted(94.1, 86.2));
    assert!(!drifted(94.0, 94.0 - DRIFT_MAX));
}

#[test]
fn blank_render_and_skinned_master_are_caught() {
    use evani_convert::gate::{COVER_MIN, GatePlan, check_drawn, coverage, skinned_parts, skinned_plan};
    // A blank picture (a sky gradient, nothing on it) is caught: blank against blank would score 100.
    let (w, h) = (64, 32);
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for _ in 0..w {
            px.extend([(100 + y * 3) as u8, 140, 200, 255]);
        }
    }
    let blank = Rgba::new(w, h, px.clone()).unwrap();
    assert_eq!(coverage(&blank), 0.0);
    let err = check_drawn(&[blank.clone(), blank.clone()], "test").unwrap_err();
    assert!(err.contains("drew nothing"), "{err}");
    // A straight left-to-right ramp is backdrop too.
    let ramp: Vec<u8> = (0..h).flat_map(|_| (0..w).flat_map(|x| [(x * 4) as u8, 90, (250 - x * 3) as u8, 255])).collect();
    let ramp = Rgba::new(w, h, ramp).unwrap();
    assert_eq!(coverage(&ramp), 0.0);
    assert!(check_drawn(&[ramp], "test").is_err());
    // A model in the middle of one view counts, and only that view needs it.
    for y in 8..24 {
        for x in 20..44 {
            px[(y * w + x) * 4..][..3].copy_from_slice(&[30, 30, 30]);
        }
    }
    let drawn = Rgba::new(w, h, px).unwrap();
    let c = coverage(&drawn);
    assert!((c - (16.0 * 24.0) / (64.0 * 32.0)).abs() < 1e-9, "{c}");
    assert!(c > COVER_MIN);
    check_drawn(&[blank, drawn], "test").unwrap();
    // Skinned meshes: the engine draws them once `anim` lands, so the gate skips an all-skinned master.
    let models = root().join("tests").join("fixtures").join("models");
    let parts = |f: &str| skinned_parts(&evani_convert::glb::Doc::read_file(&models.join(f)).unwrap().json);
    let rig = parts("rig.src.glb");
    assert!(rig.0 > 0 && rig.0 == rig.1, "rig.src.glb is skinned all through: {rig:?}");
    assert!(matches!(skinned_plan(rig), GatePlan::Skip(n) if n.starts_with("not gated: skinned model")));
    let corpus = parts("corpus.src.glb");
    assert!(corpus.1 > 0 && corpus.1 < corpus.0, "corpus.src.glb has one skinned part: {corpus:?}");
    assert!(matches!(skinned_plan(corpus), GatePlan::Run(Some(n)) if n.contains("not see them")));
    let kart = parts("kart.low.glb");
    assert_eq!(kart.1, 0);
    assert_eq!(skinned_plan(kart), GatePlan::Run(None));
}

#[test]
fn geometry_signature_sees_geometry_not_textures() {
    use evani_convert::gate::{geometry_difference, geometry_signature};
    use serde_json::{Value, json};
    let j = evani_convert::glb::Doc::read_file(&kart()).unwrap().json;
    let sig = geometry_signature(&j);
    assert!(sig.len() > 1 && sig.iter().any(|l| l.contains("POSITION")), "{sig:?}");
    assert_eq!(geometry_difference(&j, &j), None);
    // Other texture sets, materials and metadata (a textured tier file against its trial) are not geometry.
    let mut tex = j.clone();
    tex["images"] = json!([{ "uri": "kart.low.ktx2", "mimeType": "image/ktx2" }]);
    tex["materials"] = json!([{ "name": "other" }]);
    tex["extras"]["evani"]["textures"] = json!({ "astc": ["kart.low.ktx2"] });
    assert_eq!(geometry_difference(&j, &tex), None);
    // A level list, a vertex count or an attribute format that differs is.
    let mut lods = j.clone();
    lods["extras"]["evani"]["lods"] = json!([{ "node": "kart_lod0", "min_screen": 0.0 }]);
    let d = geometry_difference(&j, &lods).unwrap();
    assert!(d.contains("lods"), "{d}");
    let mut fewer = j.clone();
    let a = fewer["meshes"][0]["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    fewer["accessors"][a]["count"] = Value::from(3);
    assert!(geometry_difference(&j, &fewer).unwrap().contains("POSITION 3 "));
    let mut bits = j.clone();
    bits["accessors"][a]["componentType"] = Value::from(5122);
    assert!(geometry_difference(&j, &bits).is_some());
    // A node that no longer draws (a level of a chain missing): fewer drawn primitives.
    let mut one = j.clone();
    let k = one["nodes"].as_array().unwrap().iter().position(|n| n.get("mesh").is_some()).unwrap();
    one["nodes"][k].as_object_mut().unwrap().remove("mesh");
    assert!(geometry_signature(&one).len() < sig.len());
    assert!(geometry_difference(&j, &one).unwrap().contains("drawn primitives against"));
}

/// The tier file the gate passes is the file it judged (kart_fixture once
/// scored 94 in its trial and 86 as the tier file, made another way).
#[test]
fn gate_judges_the_file_that_ships() {
    let _one = BROWSER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = scratch("ships");
    // Tools, gate page and Chrome present (the convert below starts its own runner).
    let Some((_, probe)) = browser_or_skip("gate_judges_the_file_that_ships", &dir) else { return };
    drop(probe);
    // The kart as a general model (not an Evani.Model tier file), so it gets an LOD chain.
    let mut d = evani_convert::glb::Doc::read_file(&kart()).unwrap();
    if let Some(e) = d.json.pointer_mut("/extras/evani").and_then(serde_json::Value::as_object_mut) {
        e.remove("tier");
    }
    let input = dir.join("kart.glb");
    std::fs::write(&input, d.to_glb().unwrap()).unwrap();
    let mut o = evani_convert::Options::new(&root(), &input);
    o.id = Some("kart_ships".into());
    o.out = Some(dir.join("out"));
    o.tiers = vec!["medium".into()];
    o.gate = true;
    let out = evani_convert::convert(&o).unwrap();
    let t = &out.report["gate"]["tiers"][0];
    assert_eq!(t["tier"], "medium");
    assert_eq!(t["geometry_same"], true, "the tier file is the gate's candidate: {t}");
    let drift = t["drift"].as_f64().unwrap();
    assert!(drift.abs() <= evani_convert::gate::DRIFT_MAX, "drift {drift}");
    assert!(!out.errors.iter().any(|e| e.contains("geometry") || e.contains("under the gate's trial")), "{:?}", out.errors);
    // The general path's LOD chain was what both were made of.
    let file = evani_convert::glb::Doc::read_file(&out.out_dir.join("kart_ships.medium.glb")).unwrap().json;
    let lods = file.pointer("/extras/evani/lods").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
    assert!(lods > 1, "kart_ships.medium.glb has {lods} levels");
}

#[test]
fn views_are_deterministic() {
    let a = rig_of(&kart(), 1920, 1080);
    let b = rig_of(&kart(), 1920, 1080);
    assert_eq!(a, b);
    assert_eq!(a.views.len(), 8);
    let names: Vec<&str> = a.views.iter().map(|v| v.name).collect();
    assert_eq!(names, ["front", "right", "back", "left", "high front-right", "high back-left", "low front", "close-up"]);
    for v in &a.views {
        // Every eye is outside the normalised model (radius 1) with room for the near plane.
        let d = v.pose.eye.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!(d > 1.1, "{}: eye {d} from the centre", v.name);
        assert!(v.crop[0] < v.crop[2] && v.crop[1] < v.crop[3], "{}: crop {:?}", v.name, v.crop);
    }
    let _g = BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    let serve = scratch("deterministic");
    std::fs::copy(kart(), serve.join("kart.glb")).unwrap();
    let Some((_t, mut r)) = browser_or_skip("views_are_deterministic", &serve) else { return };
    let one = render(&mut r, &serve, "kart.glb", &a, "one");
    let two = render(&mut r, &serve, "kart.glb", &a, "two");
    assert_eq!(one.len(), 8);
    for (i, (x, y)) in one.iter().zip(&two).enumerate() {
        assert_eq!((x.w, x.h), (1920, 1080), "view {i}: the medium tier renders at the window size");
        assert!(x.px == y.px, "view {i}: two runs gave different pixels");
        let lit = x.px.as_chunks::<4>().0.iter().filter(|p| p[0] != x.px[0] || p[1] != x.px[1] || p[2] != x.px[2]).count();
        assert!(lit > 1000, "view {i}: the model is not in the picture ({lit} pixels differ from the corner)");
    }
    // The blank-render guard sees the real kart in every view.
    for (i, x) in one.iter().enumerate() {
        assert!(gate::coverage(x) > gate::COVER_MIN, "view {i}: coverage {}", gate::coverage(x));
    }
}

#[test]
fn identical_input_scores_perfect() {
    let _g = BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    let serve = scratch("identical");
    std::fs::copy(kart(), serve.join("a.glb")).unwrap();
    std::fs::copy(kart(), serve.join("b.glb")).unwrap();
    let Some((_t, mut r)) = browser_or_skip("identical_input_scores_perfect", &serve) else { return };
    let rig = rig_of(&kart(), 1920, 1080);
    let a = render(&mut r, &serve, "a.glb", &rig, "a");
    let b = render(&mut r, &serve, "b.glb", &rig, "b");
    let v = score_views(&rig, &a, &b).unwrap();
    for x in &v {
        assert_eq!((x.ssim, x.worst), (100.0, 0.0), "{v:?}");
    }
    let (ssim, worst, _) = worst_of(&v);
    assert!(passes(ssim, worst));
}

#[test]
fn damaged_model_fails() {
    let _g = BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    let serve = scratch("damaged");
    let src = root().join("tests").join("model_bench").join("ref").join("kart_01_pip").join("mesh.glb");
    std::fs::copy(&src, serve.join("orig.glb")).unwrap();
    let Some((tools, mut r)) = browser_or_skip("damaged_model_fails", &serve) else { return };
    // 10% of the triangles kept, as hard as gltfpack goes.
    let damaged = serve.join("damaged.glb");
    tools.gltfpack(&["-i", &s(&src), "-o", &s(&damaged), "-si", "0.1", "-sa", "-kn"], "gltfpack -si 0.1").unwrap();
    let rig = rig_of(&src, 1920, 1080);
    let a = render(&mut r, &serve, "orig.glb", &rig, "orig");
    let b = render(&mut r, &serve, "damaged.glb", &rig, "damaged");
    let v = score_views(&rig, &a, &b).unwrap();
    let (ssim, worst, k) = worst_of(&v);
    println!("damaged: worst view {} SSIMULACRA2 {ssim:.2}, worst pixel {worst:.3}", rig.views[k].name);
    assert!(!passes(ssim, worst), "10% of the triangles passed the gate: {v:?}");
    // And the undamaged original against itself still passes (the gate is not just strict).
    let again = render(&mut r, &serve, "orig.glb", &rig, "again");
    let (s2, w2, _) = worst_of(&score_views(&rig, &a, &again).unwrap());
    assert!(passes(s2, w2));
}
