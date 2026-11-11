//! The building-block graph, `catalog/procedural/blocks.json` (docs/ASSET_STUDIO_GUIDE.md
//! "Building blocks"): what each landscape and block is made of. The same checks as
//! `tools/procedural/gen_blocks.mjs`, so `cargo xtask check --full` guards the catalogue:
//! every id is a library asset, a catalogue material or a planned block, every link says
//! why it helps its parent and how (its kind, layer / scatter / base, with valid params),
//! and the graph has no cycles.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const BLOCKS: &str = include_str!("../../../catalog/procedural/blocks.json");
const LIBRARY: &str = include_str!("../../../catalog/procedural/library.json");
const MATERIALS: &str = include_str!("../../../catalog/materials/materials.json");

type Needs = BTreeMap<String, Vec<String>>;

fn needs_of(blocks: &Value) -> Needs {
    let mut out = Needs::new();
    for (id, list) in blocks["needs"].as_object().unwrap() {
        let kids = list
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["block"].as_str().unwrap().to_string())
            .collect();
        out.insert(id.clone(), kids);
    }
    out
}

/// The first cycle found, as the id path that closes on itself, or `None` for a DAG.
fn find_cycle(needs: &Needs) -> Option<Vec<String>> {
    fn visit(
        id: &str,
        needs: &Needs,
        on: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(at) = on.iter().position(|x| x == id) {
            let mut cyc = on[at..].to_vec();
            cyc.push(id.to_string());
            return Some(cyc);
        }
        if done.contains(id) {
            return None;
        }
        on.push(id.to_string());
        for k in needs.get(id).into_iter().flatten() {
            if let Some(c) = visit(k, needs, on, done) {
                return Some(c);
            }
        }
        on.pop();
        done.insert(id.to_string());
        None
    }
    let mut done = BTreeSet::new();
    needs
        .keys()
        .find_map(|id| visit(id, needs, &mut Vec::new(), &mut done))
}

/// The problems with one link's kind and params (the same rules as
/// gen_blocks.mjs `checkEdge`): a known kind, every required param, no
/// unknown one, values in range. `child` is "asset", "material" or "planned".
fn check_edge(x: &Value, child: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(kind) = x.get("kind") else {
        return vec!["no kind (layer, scatter or base)".into()];
    };
    let (req, opt): (&[&str], &[&str]) = match kind.as_str() {
        Some("layer") => (&["mask", "blend", "falloff"], &["condition"]),
        Some("scatter") => (&["density", "scale", "lod", "impostor"], &["slope", "height", "avoid_water"]),
        Some("base") => (&["join"], &[]),
        _ => return vec![format!("unknown kind {kind} (a link is a layer, a scatter or a base)")],
    };
    let Some(p) = x.get("params").and_then(Value::as_object) else {
        return vec![format!("a {kind} link needs params ({})", req.join(", "))];
    };
    let miss: Vec<&str> = req.iter().copied().filter(|k| !p.contains_key(*k)).collect();
    if !miss.is_empty() {
        out.push(format!("the {kind} link is missing {}", miss.join(", ")));
    }
    let extra: Vec<&String> = p.keys().filter(|k| !req.contains(&k.as_str()) && !opt.contains(&k.as_str())).collect();
    if !extra.is_empty() {
        out.push(format!("unknown param(s) {extra:?}"));
    }
    let num = |k: &str| p.get(k).and_then(Value::as_f64);
    let pair = |k: &str, lo: f64, hi: f64| {
        p.get(k).and_then(Value::as_array).is_some_and(|a| {
            a.len() == 2 && a.iter().all(|v| v.as_f64().is_some_and(|v| (lo..=hi).contains(&v))) && a[0].as_f64() <= a[1].as_f64()
        })
    };
    match kind.as_str() {
        Some("layer") => {
            let terms = p.get("mask").and_then(Value::as_array).cloned().unwrap_or_default();
            if p.contains_key("mask") && terms.is_empty() {
                out.push("mask must list one or more terms".into());
            }
            for t in &terms {
                let range = match t["by"].as_str() {
                    Some("slope") => (0.0, 90.0),
                    Some("height" | "noise") => (0.0, 1.0),
                    Some("curvature") => (-1.0, 1.0),
                    _ => {
                        out.push(format!("mask term {t}: by must be slope, height, curvature or noise"));
                        continue;
                    }
                };
                let (mn, mx) = (t.get("min").and_then(Value::as_f64), t.get("max").and_then(Value::as_f64));
                if mn.is_none() && mx.is_none() {
                    out.push(format!("mask term {t}: needs a min or a max threshold"));
                }
                if [mn, mx].iter().flatten().any(|v| *v < range.0 || *v > range.1) {
                    out.push(format!("mask term {t}: thresholds must be from {} to {}", range.0, range.1));
                }
            }
            if p.contains_key("blend") && !num("blend").is_some_and(|b| b > 0.0 && b <= 1.0) {
                out.push("blend must be above 0 and at most 1".into());
            }
            if p.contains_key("falloff") && !num("falloff").is_some_and(|f| (0.0..=1.0).contains(&f)) {
                out.push("falloff must be from 0 to 1".into());
            }
            let conds = ["wet", "mud", "dust", "snow", "wear", "oxide", "growth", "blood"];
            if let Some(c) = p.get("condition")
                && !c.as_str().is_some_and(|c| conds.contains(&c))
            {
                out.push(format!("condition {c} is not an engine shader layer"));
            }
        }
        Some("scatter") => {
            if child == "material" {
                out.push("a material can't be scattered".into());
            }
            if p.contains_key("density") && !num("density").is_some_and(|d| d > 0.0 && d <= 1000.0) {
                out.push("density must be above 0 and at most 1000".into());
            }
            if p.contains_key("scale") && !pair("scale", 0.001, 100.0) {
                out.push("scale must be [min, max]".into());
            }
            if p.contains_key("lod") && !num("lod").is_some_and(|l| l > 0.0) {
                out.push("lod must be above 0".into());
            }
            if p.contains_key("impostor") && !num("impostor").is_some_and(|i| i == 0.0 || num("lod").is_some_and(|l| i >= l)) {
                out.push("impostor must be 0 (none) or at least lod".into());
            }
            if p.contains_key("slope") && !pair("slope", 0.0, 90.0) {
                out.push("slope must be [min, max] degrees".into());
            }
            if p.contains_key("height") && !pair("height", 0.0, 1.0) {
                out.push("height must be [min, max] from 0 to 1".into());
            }
        }
        _ => match p.get("join").and_then(Value::as_str) {
            Some("surface") => {}
            Some("merge" | "part") if child == "material" => out.push("a material has no geometry: use join surface".into()),
            Some("merge" | "part") => {}
            _ if p.contains_key("join") => out.push("join must be merge, part or surface".into()),
            _ => {}
        },
    }
    out
}

fn child_kind(id: &str) -> &'static str {
    match id.split_once(':') {
        Some(("material", _)) => "material",
        Some(("planned", _)) => "planned",
        _ => "asset",
    }
}

#[test]
fn the_link_schema_refuses_unknown_kinds_missing_params_and_bad_values() {
    let v = |s: &str| -> Value { serde_json::from_str(s).unwrap() };
    let ok = |s: &str, c: &str| assert_eq!(check_edge(&v(s), c), Vec::<String>::new(), "{s}");
    let bad = |s: &str, c: &str, want: &str| {
        let p = check_edge(&v(s), c);
        assert!(p.iter().any(|x| x.contains(want)), "{s}: {p:?} lacks {want}");
    };
    ok(r#"{"block":"x","kind":"layer","params":{"mask":[{"by":"slope","min":35}],"blend":1,"falloff":0.1,"condition":"snow"}}"#, "material");
    ok(r#"{"block":"x","kind":"scatter","params":{"density":0.4,"scale":[0.6,1.4],"lod":25,"impostor":0}}"#, "asset");
    ok(r#"{"block":"x","kind":"base","params":{"join":"surface"}}"#, "material");
    bad(r#"{"block":"x"}"#, "asset", "no kind");
    bad(r#"{"block":"x","kind":"sprinkle","params":{}}"#, "asset", "unknown kind \"sprinkle\"");
    bad(r#"{"block":"x","kind":"layer"}"#, "asset", "needs params (mask, blend, falloff)");
    bad(r#"{"block":"x","kind":"layer","params":{"mask":[{"by":"slope","min":35}]}}"#, "asset", "missing blend, falloff");
    bad(r#"{"block":"x","kind":"scatter","params":{"density":0.4}}"#, "asset", "missing scale, lod, impostor");
    bad(r#"{"block":"x","kind":"base","params":{}}"#, "asset", "missing join");
    bad(r#"{"block":"x","kind":"base","params":{"join":"surface","size":3}}"#, "asset", "unknown param(s)");
    bad(r#"{"block":"x","kind":"layer","params":{"mask":[{"by":"light","max":1}],"blend":1,"falloff":0}}"#, "asset", "by must be");
    bad(r#"{"block":"x","kind":"layer","params":{"mask":[{"by":"slope"}],"blend":1,"falloff":0}}"#, "asset", "needs a min or a max");
    bad(r#"{"block":"x","kind":"layer","params":{"mask":[{"by":"slope","min":120}],"blend":1,"falloff":0}}"#, "asset", "from 0 to 90");
    bad(r#"{"block":"x","kind":"layer","params":{"mask":[{"by":"slope","min":3}],"blend":0,"falloff":2,"condition":"lava"}}"#, "asset", "not an engine shader layer");
    bad(r#"{"block":"x","kind":"scatter","params":{"density":0.4,"scale":[0.6,1.4],"lod":25,"impostor":10}}"#, "asset", "impostor must be 0");
    bad(r#"{"block":"x","kind":"scatter","params":{"density":0.4,"scale":[0.6,1.4],"lod":25,"impostor":0}}"#, "material", "can't be scattered");
    bad(r#"{"block":"x","kind":"base","params":{"join":"merge"}}"#, "material", "no geometry");
    bad(r#"{"block":"x","kind":"base","params":{"join":"glue"}}"#, "asset", "join must be");
}

#[test]
fn the_building_block_graph_names_real_blocks_and_says_why() {
    let blocks: Value = serde_json::from_str(BLOCKS).unwrap();
    let lib: Value = serde_json::from_str(LIBRARY).unwrap();
    let mats: Value = serde_json::from_str(MATERIALS).unwrap();
    let mut assets = BTreeSet::new();
    for c in lib["categories"].as_array().unwrap() {
        for a in c["assets"].as_array().unwrap() {
            assets.insert(format!(
                "{}-{}",
                c["id"].as_str().unwrap(),
                a["id"].as_str().unwrap()
            ));
        }
    }
    let materials: BTreeSet<String> = mats["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str().map(str::to_string))
        .collect();
    let planned: BTreeSet<String> = blocks["planned"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let known = |id: &str| match id.split_once(':') {
        Some(("material", m)) => materials.contains(m),
        Some(("planned", p)) => planned.contains(p),
        Some(_) => false,
        None => assets.contains(id),
    };
    let mut used = BTreeSet::new();
    for (id, list) in blocks["needs"].as_object().unwrap() {
        assert!(known(id), "needs.{id}: no such block");
        let list = list.as_array().unwrap();
        assert!(!list.is_empty(), "needs.{id}: lists no blocks");
        let mut seen = BTreeSet::new();
        for x in list {
            let b = x["block"].as_str().unwrap();
            assert!(
                known(b),
                "needs.{id}: {b} is not a library asset, material:<id> or planned:<id>"
            );
            assert_ne!(b, id, "needs.{id}: a block can't need itself");
            assert!(seen.insert(b), "needs.{id}: {b} is listed twice");
            let why = x["why"].as_str().unwrap_or("").trim();
            assert!(
                !why.is_empty() && why.len() <= 200 && !why.contains('\n'),
                "needs.{id} -> {b}: needs a one-line why"
            );
            let p = check_edge(x, child_kind(b));
            assert!(p.is_empty(), "needs.{id} -> {b}: {p:?}");
            used.insert(b.to_string());
        }
    }
    for p in &planned {
        assert!(
            used.contains(&format!("planned:{p}")),
            "planned.{p}: no parent uses it"
        );
    }
    let lands = blocks["landscapes"].as_object().unwrap();
    for key in [
        "terrain-mountains",
        "water-rivers",
        "terrain-rolling-hills",
        "water-shorelines",
    ] {
        assert!(lands.contains_key(key), "{key} is a landscape");
    }
    for key in lands.keys() {
        assert!(
            assets.contains(key),
            "landscapes.{key}: not a library asset"
        );
        assert!(
            blocks["needs"][key].is_array(),
            "landscapes.{key}: has no blocks"
        );
    }
    assert_eq!(
        find_cycle(&needs_of(&blocks)),
        None,
        "the building-block graph has a cycle"
    );
}

#[test]
fn a_cycle_in_the_building_blocks_is_found_with_its_path() {
    let mut needs = Needs::new();
    needs.insert(
        "terrain-mountains".into(),
        vec!["rocks-medium-rocks".into(), "sky-dynamic-sky-dome".into()],
    );
    needs.insert("rocks-medium-rocks".into(), vec!["rocks-pebbles".into()]);
    assert_eq!(find_cycle(&needs), None);
    needs.insert("rocks-pebbles".into(), vec!["terrain-mountains".into()]);
    let cyc = find_cycle(&needs).unwrap();
    assert_eq!(cyc.first(), cyc.last(), "the path closes on itself");
    assert_eq!(cyc.len(), 4, "{cyc:?}");
    for id in ["terrain-mountains", "rocks-medium-rocks", "rocks-pebbles"] {
        assert!(cyc.iter().any(|x| x == id), "{id} is on the cycle {cyc:?}");
    }
}
