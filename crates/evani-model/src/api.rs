//! The operations behind the CLI and the MCP tools (docs/MODEL_API.md
//! section 2): compact JSON in and out, never vertex data. Work state lives
//! in a project-local folder (`.evani/model/<id>/`); every call is
//! reproducible from the saved canonical intent.
//!
//! M1 has no search: `make` returns one candidate (`k0`) built at the centre
//! of every adjective range, with all must tests run on the compiled tiers.

use crate::build::{self, Model};
use crate::collide;
use crate::compile::{self, TierMesh};
use crate::gltf;
use crate::intent::{self, Diag};
use crate::must;
use crate::sha256;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An error reply: `{ error, msg, fix }`.
pub fn error_json(d: &[Diag]) -> Value {
    match d.first() {
        Some(first) => {
            let mut v = first.to_json();
            if d.len() > 1 {
                v["more"] = json!(d[1..].iter().take(4).map(Diag::to_json).collect::<Vec<_>>());
            }
            v
        }
        None => json!({ "error": "unknown", "msg": "no detail", "fix": "retry" }),
    }
}

fn err(code: &'static str, msg: impl Into<String>, fix: impl Into<String>) -> Vec<Diag> {
    vec![Diag::err(code, 0, msg, fix)]
}

pub struct Workspace {
    pub root: PathBuf,
    pub threads: usize,
    /// Compiled tiers of the last intents (by content hash and tier name),
    /// so make then compile in one session compiles each tier once.
    cache: std::cell::RefCell<Vec<(String, TierMesh)>>,
}

impl Workspace {
    pub fn new(project: &Path) -> Self {
        Workspace { root: project.join(".evani").join("model"), threads: crate::rng::default_threads(), cache: Default::default() }
    }
    fn model_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }
    /// Read an intent from text, canonical JSON, a path or a model id.
    pub fn read_intent(&self, src: &str) -> Result<Value, Vec<Diag>> {
        let s = src.trim();
        if !s.contains('\n') && !s.starts_with('{') {
            let p = Path::new(s);
            if s.ends_with(".evmodel") || s.ends_with(".json") {
                let text = std::fs::read_to_string(p).map_err(|e| err("no_file", format!("cannot read {s}: {e}"), "give a path to an .evmodel file, the intent text, or a model id"))?;
                return intent::read(&text);
            }
            let saved = self.model_dir(s).join("intent.json");
            if saved.exists() {
                let text = std::fs::read_to_string(&saved).map_err(|e| err("no_file", e.to_string(), "make the model again"))?;
                return intent::read(&text);
            }
        }
        intent::read(src)
    }
    /// The requested tiers of an intent (every budgeted tier when `only` is
    /// `None`), each compiled on its own and once per session. Tiers are
    /// independent, so asking for fewer compiles only those and changes
    /// nothing in them.
    fn tiers(&self, j: &Value, only: Option<&[String]>) -> Result<Vec<TierMesh>, Vec<Diag>> {
        let exact = build::build(j, 0.0)?;
        let hash = intent::content_hash(j);
        let mut out = Vec::new();
        for tier in compile::tiers(&exact, only) {
            let key = format!("{hash}/{}", tier.name);
            let hit = self.cache.borrow().iter().find(|(k, _)| *k == key).map(|(_, t)| t.clone());
            let t = match hit {
                Some(t) => t,
                None => {
                    let t = compile::compile_tier(j, &tier, self.threads)?;
                    let mut c = self.cache.borrow_mut();
                    c.push((key, t.clone()));
                    // The last few intents' tiers.
                    if c.len() > 20 {
                        c.remove(0);
                    }
                    t
                }
            };
            out.push(t);
        }
        Ok(out)
    }
    fn save_intent(&self, id: &str, j: &Value) -> Result<(), Vec<Diag>> {
        let d = self.model_dir(id);
        std::fs::create_dir_all(&d).map_err(|e| err("io", e.to_string(), "check the project folder is writable"))?;
        std::fs::write(d.join("intent.json"), intent::canonical(j)).map_err(|e| err("io", e.to_string(), "check the project folder is writable"))
    }
}

pub fn model_id(j: &Value) -> String {
    let h = sha256::sha256(intent::canonical(j).as_bytes());
    format!("{}-{}", j["make"].as_str().unwrap_or("model").replace('/', "_"), &sha256::hex(&h)[..4])
}

/// Hardware: the CPU part (the GPU probe arrives with the GPU path in M2).
pub fn describe_system() -> Value {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let gpu = nvidia_gpu();
    json!({
        "compute": "cpu",
        "why": "M1 runs the deterministic CPU path only; GPU exploration arrives in M2",
        "gpu": gpu,
        "cpu": { "threads": threads, "used": crate::rng::default_threads() },
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "budgets": { "make_s": 3, "edit_s": 1, "auto_level": 1, "note": "no search in M1: one candidate at range centres" },
        "tool": TOOL_VERSION
    })
}

/// The driver's own tool, when present (note 18), for the GPU's name and memory.
fn nvidia_gpu() -> Value {
    let out = std::process::Command::new("nvidia-smi").args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let line = s.lines().next().unwrap_or("");
            let mut it = line.split(',').map(str::trim);
            let name = it.next().unwrap_or("");
            let mib: f64 = it.next().and_then(|m| m.parse().ok()).unwrap_or(0.0);
            json!({ "name": name, "vendor": "nvidia", "mem_gib": (mib / 1024.0 * 10.0).round() / 10.0, "source": "nvidia-smi" })
        }
        _ => json!({ "name": "unknown", "source": "not probed (the wgpu adapter probe arrives in M2)" }),
    }
}

/// Run the musts on a model and its compiled tiers (each tier judged on its
/// own; see `must::scope`).
pub fn musts(model: &Model, tiers: &[TierMesh], threads: usize) -> Vec<must::MustResult> {
    must::run_all(&must::Ctx { model, tiers, h: 0.015, threads })
}

/// "passed/total" musts as judged for one tier (its own results and the
/// exact-field ones), or for every tier when `tier` is `None`.
fn must_count(rs: &[must::MustResult], tier: Option<&str>) -> String {
    let kinds = must::kinds(rs);
    let pass = kinds.iter().filter(|k| must::passes(rs, k, tier)).count();
    format!("{pass}/{}", kinds.len())
}

/// Musts that pass on the field and on every tier, the count per tier, and
/// every failing result (with its tier).
fn must_summary(rs: &[must::MustResult], tiers: &[TierMesh]) -> Value {
    let kinds = must::kinds(rs);
    let pass = kinds.iter().filter(|k| must::passes(rs, k, None)).count();
    let per_tier: Map<String, Value> = tiers.iter().map(|t| (t.tier.name.clone(), json!(must_count(rs, Some(&t.tier.name))))).collect();
    json!({ "pass": pass, "of": kinds.len(), "tiers": per_tier, "fail": rs.iter().filter(|r| !r.pass).map(must::MustResult::to_json).collect::<Vec<_>>() })
}

/// Must results as stored in metadata: kind, tier (absent for the exact
/// field), pass, got, need.
fn must_records<'a>(rs: impl Iterator<Item = &'a must::MustResult>) -> Value {
    let record = |r: &must::MustResult| match &r.tier {
        Some(t) => json!({ "kind": r.kind, "tier": t, "pass": r.pass, "got": r.got, "need": r.need }),
        None => json!({ "kind": r.kind, "pass": r.pass, "got": r.got, "need": r.need }),
    };
    json!(rs.map(record).collect::<Vec<_>>())
}

/// `model.make`: validate, build, compile, test. One candidate in M1.
pub fn make(ws: &Workspace, src: &str) -> Result<Value, Vec<Diag>> {
    let t0 = Instant::now();
    let j = ws.read_intent(src)?;
    let model = build::build(&j, 0.0)?;
    let tiers = ws.tiers(&j, None)?;
    let rs = musts(&model, &tiers, ws.threads);
    let id = model_id(&j);
    ws.save_intent(&id, &j)?;
    let mut measures = Map::new();
    for (k, v) in &model.measures {
        if model.pinned.contains(k) || j["parts"].as_object().is_some_and(|p| p.keys().any(|pk| k.starts_with(&format!("{pk}.")))) {
            measures.insert(k.clone(), json!(gltf::r6((v * 1000.0).round() / 1000.0)));
        }
    }
    let size = model.extents().size();
    Ok(json!({
        "model": id,
        "compute": "cpu",
        "level": 1,
        "explored": [1],
        "seed": model.seed,
        "ms": t0.elapsed().as_millis() as u64,
        "provenance": { "intent": intent::content_hash(&j), "style": format!("{}@1", model.style), "class": format!("{}@1", model.class), "tool": TOOL_VERSION },
        "top": [{
            "id": "k0",
            "must": must_summary(&rs, &tiers),
            "size_m": [gltf::r6((size[2] * 1000.0).round() / 1000.0), gltf::r6((size[0] * 1000.0).round() / 1000.0), gltf::r6((size[1] * 1000.0).round() / 1000.0)],
            "measures": Value::Object(measures),
            "tris": tiers.iter().map(|t| (t.tier.name.clone(), json!(t.tris))).collect::<Map<_, _>>(),
            "note": "M1: one candidate at range centres (search arrives in M2)"
        }],
        "notes": model.notes
    }))
}

/// `model.compile`: write one `.glb` per tier and the metadata file.
pub fn compile_model(ws: &Workspace, src: &str, only: Option<&[String]>, out: Option<&Path>) -> Result<Value, Vec<Diag>> {
    let t0 = Instant::now();
    let j = ws.read_intent(src)?;
    let id = model_id(&j);
    ws.save_intent(&id, &j)?;
    let model = build::build(&j, 0.0)?;
    let tiers = ws.tiers(&j, only)?;
    let rs = musts(&model, &tiers, ws.threads);
    let col = collide::build_collision(&model);
    let dir = out.map(Path::to_path_buf).unwrap_or_else(|| ws.model_dir(&id).join("v1"));
    std::fs::create_dir_all(&dir).map_err(|e| err("io", e.to_string(), "check the output folder is writable"))?;
    let mut meta = Map::new();
    meta.insert("notes".into(), json!(model.notes));
    meta.insert("tool".into(), json!(TOOL_VERSION));
    let mut tiers_out = Map::new();
    let mut hasher = sha256::Sha256::new();
    let mut mesh_ok = true;
    let mut gltf_ok = true;
    let (mut seam_max, mut bake_max, mut overlaps): (f64, f64, usize) = (0.0, 0.0, 0);
    for t in &tiers {
        // Charts, tangents and normal maps; the file carries the split mesh.
        let surf = compile::surfaces(t, ws.threads);
        let mut split = t.clone();
        let mut maps = Vec::new();
        for (s, so) in split.solids.iter_mut().zip(&surf) {
            s.mesh = so.mesh.clone();
            let f = format!("{}.{}.{}.normal.png", model.class, t.tier.name, s.solid);
            let png = crate::png::write_rgb(so.size, so.size, &so.normal_map);
            std::fs::write(dir.join(&f), &png).map_err(|e| err("io", e.to_string(), "check the output folder is writable"))?;
            hasher.update(&png);
            maps.push(json!({ "solid": s.solid, "file": f, "size": so.size, "texels_per_m": gltf::r6(so.texels_per_m), "charts": so.charts, "seam_max_deg": gltf::r6(so.seam_max_deg), "seam_raw_deg": gltf::r6(so.seam_raw_deg), "seam_samples": so.seam_samples, "seam_unresolved": so.seam_unresolved, "bake_max_deg": gltf::r6(so.bake_max_deg), "overlaps": so.overlaps }));
            seam_max = seam_max.max(so.seam_max_deg);
            bake_max = bake_max.max(so.bake_max_deg);
            overlaps += so.overlaps;
        }
        let gs: Vec<gltf::Surface> = surf.iter().map(|so| gltf::Surface { uv: so.uv.clone(), tangent: so.tangent.clone() }).collect();
        // Each file carries the exact-field musts and its own tier's, so a
        // tier's bytes do not depend on which other tiers were asked for.
        let mut tmeta = meta.clone();
        tmeta.insert("musts".into(), must_records(rs.iter().filter(|r| r.tier.is_none() || r.tier.as_deref() == Some(t.tier.name.as_str()))));
        tmeta.insert("normal_maps".into(), json!({ "maps": maps, "space": "tangent (MikkTSpace convention), +y up the chart", "material": "none: Evani.Model makes no materials; the look reads these maps (CORE_REQUESTS: relief display for imported models)" }));
        let bytes = gltf::write_tier(&split, &col, Some(&gs), &tmeta);
        let name = format!("{}.{}.glb", model.class, t.tier.name);
        std::fs::write(dir.join(&name), &bytes).map_err(|e| err("io", e.to_string(), "check the output folder is writable"))?;
        hasher.update(&bytes);
        mesh_ok &= t.solids.iter().all(|s| s.report.ok());
        gltf_ok &= gltf::read(&bytes).is_ok();
        tiers_out.insert(
            t.tier.name.clone(),
            json!({ "file": name.clone(), "must": must_count(&rs, Some(&t.tier.name)), "tris": t.tris, "budget": t.tier.budget, "kb": (bytes.len() as f64 / 1024.0).round() as u64, "dropped": t.dropped.iter().map(|d| d.0.clone()).collect::<Vec<_>>() }),
        );
    }
    let hash = format!("sha256:{}", sha256::hex(&hasher.finish()));
    // Garment thickness at the template's check points (only for
    // templates that name some, so other models' files are unchanged).
    let thickness: Vec<Value> = build::garment_thickness(&model)
        .iter()
        .map(|t| json!({ "part": t.part, "at": t.name, "got": gltf::r6((t.got * 1e4).round() / 1e4), "target": gltf::r6(t.target), "pass": t.pass, "capped": t.capped, "bridged": t.bridged, "on": t.at.map(|v| gltf::r6((v * 1e4).round() / 1e4)), "n": t.n.map(|v| gltf::r6((v * 1e3).round() / 1e3)), "tolerance": "30%" }))
        .collect();
    let mut metadata = json!({
        "model": id, "hash": hash, "intent": intent::content_hash(&j), "seed": model.seed, "class": model.class, "style": model.style,
        "anchors": model.anchors.iter().map(|a| json!({ "name": a.name, "at": gltf::r6(a.p[0]).to_string() + "," + &gltf::r6(a.p[1]).to_string() + "," + &gltf::r6(a.p[2]).to_string(), "part": a.part })).collect::<Vec<_>>(),
        "collision": col.proxies.iter().map(|p| json!({ "name": p.name, "solid": p.solid, "missed": gltf::r6(p.missed), "excess": gltf::r6(p.excess) })).collect::<Vec<_>>(),
        "footprint": col.footprint.as_ref().map(|f| json!({ "radius": gltf::r6(f.radius), "half_segment": gltf::r6(f.half_segment) })),
        "tiers": Value::Object(tiers_out.clone()),
        "musts": must_records(rs.iter()),
    });
    if !thickness.is_empty() {
        metadata["garment_thickness"] = json!(thickness);
    }
    std::fs::write(dir.join(format!("{}.evmodel.json", model.class)), serde_json::to_string_pretty(&metadata).unwrap_or_default()).map_err(|e| err("io", e.to_string(), "check the output folder is writable"))?;
    let mut res = json!({
        "model": id,
        "hash": hash,
        "ms": t0.elapsed().as_millis() as u64,
        "out": dir.to_string_lossy().replace('\\', "/"),
        "tiers": Value::Object(tiers_out),
        "anchors": model.anchors.len(),
        "collision": col.proxies.len(),
        "checks": { "must": must_count(&rs, None), "mesh": if mesh_ok { "ok" } else { "issues" }, "normal_maps": { "seam_max_deg": (seam_max * 10.0).round() / 10.0, "bake_max_deg": (bake_max * 10.0).round() / 10.0, "overlaps": overlaps }, "gltf": if gltf_ok { "ok (own reader; Khronos validator run separately)" } else { "unreadable" } },
        "fail": rs.iter().filter(|r| !r.pass).map(must::MustResult::to_json).collect::<Vec<_>>()
    });
    if !thickness.is_empty() {
        let flagged = thickness.iter().filter(|t| t["pass"] == json!(false)).count();
        res["checks"]["garment_thickness"] = json!({ "points": thickness, "flagged": flagged });
    }
    Ok(res)
}

/// `model.check`: musts and mesh checks per tier.
pub fn check(ws: &Workspace, src: &str, only: Option<&[String]>) -> Result<Value, Vec<Diag>> {
    let j = ws.read_intent(src)?;
    let model = build::build(&j, 0.0)?;
    let tiers = ws.tiers(&j, only)?;
    let rs = musts(&model, &tiers, ws.threads);
    let mut mesh = Map::new();
    for t in &tiers {
        let mut issues = Vec::new();
        for s in &t.solids {
            for (k, n) in s.report.summary() {
                issues.push(format!("{} {k} {n}", s.solid));
            }
        }
        let watertight = t.solids.iter().all(|s| s.report.watertight());
        let manifold = t.solids.iter().all(|s| s.report.manifold());
        let crossings: usize = t.solids.iter().map(|s| s.report.count(crate::meshcheck::Kind::SelfIntersection)).sum();
        mesh.insert(t.tier.name.clone(), json!({ "must": must_count(&rs, Some(&t.tier.name)), "watertight": watertight, "manifold": manifold, "self_intersections": crossings, "tris": t.tris, "budget": t.tier.budget, "issues": issues }));
    }
    Ok(json!({ "model": model_id(&j), "must": must_summary(&rs, &tiers), "mesh": Value::Object(mesh) }))
}

/// `model.sheet`: one combined picture (blueprint: four views).
pub fn sheet(ws: &Workspace, src: &str, kind: &str, out: Option<&Path>) -> Result<Value, Vec<Diag>> {
    if kind != "blueprint" {
        return Err(err("not_in_m1", format!("sheet kind `{kind}` arrives in M3 or M4"), "use kind blueprint"));
    }
    let j = ws.read_intent(src)?;
    let id = model_id(&j);
    // Only the last budgeted tier is drawn, so only it is compiled.
    let exact = build::build(&j, 0.0)?;
    let last: Vec<String> = compile::tiers(&exact, None).last().map(|t| t.name.clone()).into_iter().collect();
    let tiers = ws.tiers(&j, Some(&last))?;
    let Some(t) = tiers.last() else { return Err(err("no_tier", "nothing compiled", "add a tri_budget")) };
    let mesh = t.world_mesh();
    let field = t.model.field().prepare();
    let caption = format!("{} {} {} TRIS", id, t.tier.name, t.tris);
    let img = crate::preview::blueprint(&mesh, Some(&field), &caption, 512, ws.threads);
    let path = match out {
        Some(p) => p.to_path_buf(),
        None => ws.model_dir(&id).join("sheets").join(format!("{kind}.png")),
    };
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| err("io", e.to_string(), "check the folder is writable"))?;
    }
    std::fs::write(&path, crate::png::write(&img)).map_err(|e| err("io", e.to_string(), "check the folder is writable"))?;
    Ok(json!({ "sheet": path.to_string_lossy().replace('\\', "/"), "caption": format!("front, side, top, three-quarter of {id} at the {} tier", t.tier.name) }))
}

/// `model.compare`: silhouettes and region measures of a model against a
/// target `.glb` (docs/model_research/plan-characters.md section 8). `src` is
/// an intent or model id (only `tier` is compiled, through the session cache,
/// and its world mesh is measured in memory) or a `.glb` file of ours.
pub fn compare(ws: &Workspace, src: &str, target: &Path, tier: &str, size: usize, out: Option<&Path>) -> Result<Value, Vec<Diag>> {
    let t0 = Instant::now();
    let tb = std::fs::read(target).map_err(|e| err("no_file", format!("cannot read the target {}: {e}", target.display()), "give --target the path of a .glb file"))?;
    let tshape = crate::compare::load_glb(&tb)?;
    drop(tb);
    let s = src.trim();
    let (ours, label, tier_out, default_out) = if s.to_ascii_lowercase().ends_with(".glb") {
        let b = std::fs::read(s).map_err(|e| err("no_file", format!("cannot read {s}: {e}"), "give a path to a .glb file, an .evmodel intent or a model id"))?;
        let stem = Path::new(s).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "model".into());
        (crate::compare::load_glb(&b)?, stem.clone(), Value::Null, ws.root.join("compare").join(format!("{stem}.compare.png")))
    } else {
        let j = ws.read_intent(src)?;
        let id = model_id(&j);
        let exact = build::build(&j, 0.0)?;
        let names: Vec<String> = compile::tiers(&exact, None).iter().map(|t| t.name.clone()).collect();
        if !names.iter().any(|n| n == tier) {
            return Err(err("no_tier", format!("the model has no `{tier}` tier"), format!("use --tier with one of: {}", names.join(", "))));
        }
        let tiers = ws.tiers(&j, Some(&[tier.to_string()]))?;
        let Some(t) = tiers.last() else { return Err(err("no_tier", "nothing compiled", "add a tri_budget")) };
        let path = ws.model_dir(&id).join("sheets").join(format!("compare-{tier}.png"));
        (crate::compare::Shape::from_mesh(t.world_mesh()), id, json!(tier), path)
    };
    let c = crate::compare::compare(&ours, &tshape, size, ws.threads);
    let path = out.map(Path::to_path_buf).unwrap_or(default_out);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| err("io", e.to_string(), "check the folder is writable"))?;
    }
    std::fs::write(&path, crate::png::write_rgb(c.sheet_w, c.sheet_h, &c.sheet)).map_err(|e| err("io", e.to_string(), "check the folder is writable"))?;
    let mut v = c.to_json();
    v["model"] = json!(label);
    v["sheet"] = json!(path.to_string_lossy().replace('\\', "/"));
    v["tier"] = tier_out;
    v["tris_ours"] = json!(ours.mesh.tris.len());
    v["tris_target"] = json!(tshape.mesh.tris.len());
    v["size"] = json!(size.clamp(64, 2048));
    v["ms"] = json!(t0.elapsed().as_millis() as u64);
    Ok(v)
}
