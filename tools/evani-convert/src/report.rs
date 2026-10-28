//! The manifest, the read-back checks, the validator run and the two
//! reports (`<id>.report.json` and `<id>.report.md`).

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use evani_gltf::Gltf;
use evani_guide::playground::{PACKAGE_CAP_BYTES, mb};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::texture::{self, Screen};
use crate::tools::{Tools, s};
use crate::{InputList, Options, TierOut};

pub struct Inputs<'a> {
    pub root: &'a Path,
    pub out: &'a Path,
    pub id: &'a str,
    pub opt: &'a Options,
    pub inputs: &'a InputList,
    pub master: &'a str,
    pub tiers: &'a [TierOut],
    pub notes: &'a [String],
    pub tools: &'a Tools,
    pub evani_model: bool,
    /// The quality gate's part (`--gate`), from gate::Gate::finish.
    pub gate: Option<&'a Value>,
    /// What look matching did (`--look`, S3-12), from look::Match::to_json plus the per-tier numbers.
    pub look_match: Option<&'a Value>,
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

pub fn gzip_len(b: &[u8]) -> u64 {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(9));
    if e.write_all(b).is_err() {
        return 0;
    }
    e.finish().map_or(0, |v| v.len() as u64)
}

pub fn brotli_len(b: &[u8]) -> u64 {
    let mut w = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    if w.write_all(b).is_err() {
        return 0;
    }
    w.into_inner().len() as u64
}

/// Raw, gzip and brotli bytes of one file.
pub fn sizes(p: &Path) -> Result<(u64, u64, u64), String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((b.len() as u64, gzip_len(&b), brotli_len(&b)))
}

/// The model's box over every drawn vertex (file units, world space).
pub fn model_box(g: &Gltf) -> ([f32; 3], [f32; 3]) {
    let world = g.world_matrices();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (n, node) in g.nodes.iter().enumerate() {
        let Some(m) = node.mesh.and_then(|m| g.meshes.get(m)) else { continue };
        for p in &m.primitives {
            let Some(a) = p.attribute("POSITION").and_then(|a| g.accessors.get(a)) else { continue };
            let pts: Vec<[f32; 3]> = if a.min.len() == 3 && a.max.len() == 3 {
                (0..8).map(|c| [if c & 1 == 0 { a.min[0] } else { a.max[0] }, if c & 2 == 0 { a.min[1] } else { a.max[1] }, if c & 4 == 0 { a.min[2] } else { a.max[2] }]).collect()
            } else {
                p.attribute("POSITION").and_then(|i| g.read_f32(i).ok()).map(|v| v.as_chunks::<3>().0.to_vec()).unwrap_or_default()
            };
            for q in pts {
                let w = evani_gltf::transform_point(&world[n], q);
                for i in 0..3 {
                    lo[i] = lo[i].min(w[i]);
                    hi[i] = hi[i].max(w[i]);
                }
            }
        }
    }
    if lo[0] > hi[0] { ([0.0; 3], [0.0; 3]) } else { (lo, hi) }
}

/// Nodes in the subtree under `root` (inclusive).
fn subtree(g: &Gltf, root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut st = vec![root];
    while let Some(n) = st.pop() {
        if out.contains(&n) {
            continue;
        }
        out.push(n);
        if let Some(node) = g.nodes.get(n) {
            st.extend(node.children.iter().copied());
        }
    }
    out
}

fn scene_nodes(g: &Gltf) -> Vec<usize> {
    let roots = g.scene.and_then(|s| g.scenes.get(s)).or(g.scenes.first()).map(|s| s.nodes.clone()).unwrap_or_default();
    roots.into_iter().flat_map(|r| subtree(g, r)).collect()
}

/// (triangles, draw calls) of the meshes on `nodes`.
fn tris_and_draws(g: &Gltf, nodes: &[usize]) -> (u64, u64) {
    let (mut t, mut d) = (0u64, 0u64);
    for &n in nodes {
        let Some(m) = g.nodes.get(n).and_then(|n| n.mesh).and_then(|m| g.meshes.get(m)) else { continue };
        for p in &m.primitives {
            d += 1;
            if p.mode != 4 {
                continue;
            }
            let count = p.indices.and_then(|i| g.accessors.get(i)).or_else(|| p.attribute("POSITION").and_then(|a| g.accessors.get(a))).map_or(0, |a| a.count);
            t += (count / 3) as u64;
        }
    }
    (t, d)
}

/// Per level: (node name, triangles, draw calls). One level when the file has no `lods`.
pub fn levels(g: &Gltf) -> Vec<(String, u64, u64)> {
    match g.evani.as_ref().filter(|e| !e.lods.is_empty()) {
        Some(e) => e
            .lods
            .iter()
            .map(|l| {
                let nodes = l.node_index.map(|i| subtree(g, i)).unwrap_or_default();
                let (t, d) = tris_and_draws(g, &nodes);
                (l.node.clone(), t, d)
            })
            .collect(),
        None => {
            let (t, d) = tris_and_draws(g, &scene_nodes(g));
            vec![("(whole file)".into(), t, d)]
        }
    }
}

/// `meshcheck` over the drawn triangles of `nodes` (world space, positions
/// welded so UV seams don't read as open edges).
fn meshcheck(g: &Gltf, nodes: &[usize]) -> Value {
    let world = g.world_matrices();
    let mut pos: Vec<[f64; 3]> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut seen: std::collections::BTreeMap<[i64; 3], u32> = std::collections::BTreeMap::new();
    for &n in nodes {
        let Some(m) = g.nodes.get(n).and_then(|x| x.mesh).and_then(|m| g.meshes.get(m)) else { continue };
        for p in m.primitives.iter().filter(|p| p.mode == 4) {
            let Some(pi) = p.attribute("POSITION") else { continue };
            let Ok(v) = g.read_f32(pi) else { continue };
            let map: Vec<u32> = v
                .as_chunks::<3>()
                .0
                .iter()
                .map(|c| {
                    let w = evani_gltf::transform_point(&world[n], *c);
                    let key = [(f64::from(w[0]) * 1e5).round() as i64, (f64::from(w[1]) * 1e5).round() as i64, (f64::from(w[2]) * 1e5).round() as i64];
                    *seen.entry(key).or_insert_with(|| {
                        pos.push([f64::from(w[0]), f64::from(w[1]), f64::from(w[2])]);
                        (pos.len() - 1) as u32
                    })
                })
                .collect();
            let idx: Vec<u32> = match p.indices {
                Some(i) => g.read_u32(i).unwrap_or_default(),
                None => (0..map.len() as u32).collect(),
            };
            for t in idx.as_chunks::<3>().0 {
                let get = |k: u32| map.get(k as usize).copied().unwrap_or(0);
                tris.push([get(t[0]), get(t[1]), get(t[2])]);
            }
        }
    }
    let opt = evani_model::meshcheck::Options { ref_normals: None, expect_closed: false, self_intersections: tris.len() <= 200_000 };
    let r = evani_model::meshcheck::check(&pos, &tris, &opt);
    let issues: serde_json::Map<String, Value> = r.summary().into_iter().map(|(k, n)| (k.to_owned(), Value::from(n))).collect();
    json!({ "triangles": r.triangles, "vertices": r.vertices, "components": r.components, "self_intersections_checked": opt.self_intersections, "issues": issues })
}

/// `gltf-transform validate`: (errors, warnings, the error lines).
pub fn validate(tools: &Tools, file: &Path) -> Result<(u64, u64, Vec<String>), String> {
    let what = format!("gltf-transform validate {}", file.display());
    let (code, text) = tools.gltf_transform_any(&["validate", &s(file), "--format", "csv"], &what)?;
    if !matches!(code, Some(0 | 1)) {
        return Err(format!("{what} could not run ({code:?}):\n{text}"));
    }
    let (mut e, mut w, mut lines) = (0, 0, Vec::new());
    for l in text.lines().filter(|l| !l.starts_with("code,") && l.contains(',')) {
        let mut parts = l.rsplitn(3, ',');
        let pointer = parts.next().unwrap_or("");
        let sev = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("");
        match sev {
            "0" => {
                e += 1;
                let line = format!("{} at {pointer}", rest.split(',').next().unwrap_or(rest));
                // One line per kind and place (a bad attribute repeats per vertex).
                if lines.len() < 10 && !lines.contains(&line) {
                    lines.push(line);
                }
            }
            "1" => w += 1,
            _ => {}
        }
    }
    if code == Some(1) && e == 0 {
        return Err(format!("{what} failed without listing an error:\n{text}"));
    }
    Ok((e, w, lines))
}

fn named_nodes(g: &Gltf) -> BTreeSet<String> {
    g.nodes.iter().map(|n| n.name.clone()).filter(|n| !n.is_empty()).collect()
}

fn anchor_names(g: &Gltf) -> BTreeSet<String> {
    g.anchors.iter().map(|a| a.name.clone()).collect()
}

/// Every URI a tier file points at: images, texture sets, normal-map sets.
pub use evani_guide::modelcheck::referenced_files;

/// The read-back: the engine's reader opens each tier file, the shared model
/// rules pass (`evani_guide::modelcheck`: unit normals, LOD nodes, every
/// referenced file present, KTX2 the engine reads), and what the input had
/// (anchors, named nodes, triangles for Evani.Model) is still there.
fn read_back(x: &Inputs, t: &TierOut, g: &Gltf, input: &Gltf, problems: &mut Vec<String>) {
    let file = &t.file;
    match &g.evani {
        None => problems.push(format!("{file}: no extras.evani (gltfpack drops the root extras; the converter must write them back)")),
        Some(e) => {
            if e.tier.as_deref() != Some(t.tier.as_str()) {
                problems.push(format!("{file}: extras.evani.tier is {:?}, not {}", e.tier, t.tier));
            }
            if !x.evani_model && e.textures.len() != g.textures.len() {
                problems.push(format!("{file}: {} textures but {} texture sets", g.textures.len(), e.textures.len()));
            }
        }
    }
    let (a_in, a_out) = (anchor_names(input), anchor_names(g));
    if a_in != a_out {
        problems.push(format!("{file}: anchors differ from the input (input {a_in:?}, file {a_out:?})"));
    }
    let lost: Vec<String> = named_nodes(input).difference(&named_nodes(g)).cloned().collect();
    if !lost.is_empty() {
        problems.push(format!("{file}: named nodes lost: {}", lost.join(", ")));
    }
    clip_read_back(t, g, input, problems);
    let base = x.out.join(file).parent().map(Path::to_path_buf).unwrap_or_else(|| x.out.to_path_buf());
    problems.extend(evani_guide::modelcheck::problems(g, &base).into_iter().map(|p| format!("{file}: {p}")));
    if x.evani_model {
        let (ti, _) = tris_and_draws(input, &scene_nodes(input));
        let (to, _) = tris_and_draws(g, &scene_nodes(g));
        if ti != to {
            problems.push(format!("{file}: {to} triangles, the Evani.Model input has {ti} (its files must never be re-simplified)"));
        }
    }
}

/// Animated input (S3-7): the tier file carries the clip block instead of
/// glTF animations, the block reads back with one clip per input animation,
/// and every clip is within the error bound. (The block's own layout is the
/// shared model check's, `evani_guide::modelcheck::clip_problems`.)
fn clip_read_back(t: &TierOut, g: &Gltf, input: &Gltf, problems: &mut Vec<String>) {
    let file = &t.file;
    if input.animations.is_empty() {
        if g.extras.pointer("/evani/clips").is_some() {
            problems.push(format!("{file}: extras.evani.clips, but the input has no animations"));
        }
        return;
    }
    if !g.animations.is_empty() {
        problems.push(format!("{file}: still has {} glTF animations (tier files carry the clip block; only the master keeps them)", g.animations.len()));
    }
    let Some(view) = g.extras.pointer("/evani/clips/view").and_then(Value::as_u64) else {
        problems.push(format!("{file}: the input is animated but the file has no extras.evani.clips"));
        return;
    };
    let block = usize::try_from(view).ok().and_then(|v| g.view_bytes(v).ok()).unwrap_or(&[]);
    match evani_guide::clipblock::parse(block) {
        Err(e) => problems.push(format!("{file}: the clip block does not read back: {e}")),
        Ok(c) if c.len() != input.animations.len() => problems.push(format!("{file}: {} clips for {} animations in the input", c.len(), input.animations.len())),
        Ok(_) => {}
    }
    match &t.clips {
        None => problems.push(format!("{file}: no clip report for an animated input")),
        Some(b) => {
            for c in b.clips.iter().filter(|c| !c.within) {
                let tag = format!("clip `{}`:", c.name);
                let why: Vec<&str> = b.notes.iter().filter(|n| n.starts_with(&tag)).map(String::as_str).collect();
                problems.push(format!("{file}: clip `{}` is {:.6} m off at its worst (node `{}`), over the {} m bound even with every key kept. {}", c.name, c.worst_shell_error_m, c.worst_node, b.bound_m, why.join(" ")));
            }
        }
    }
}

/// The `clips` entry of one tier in the JSON report.
fn clips_json(t: &TierOut) -> Value {
    let Some(b) = &t.clips else { return Value::Null };
    json!({
        "bound_m": b.bound_m,
        "block_bytes": b.block.len(),
        "notes": b.notes,
        "clips": b.clips.iter().map(|c| json!({
            "name": c.name,
            "frames": c.frames,
            "tracks": c.tracks,
            "constant": c.constant,
            "rest": c.rest,
            "keys": c.keys,
            "rot_bits": c.rot_bits,
            "bytes": c.bytes,
            "worst_shell_error_m": c.worst_shell_error_m,
            "worst_node": c.worst_node,
            "within": c.within,
        })).collect::<Vec<_>>(),
    })
}

fn read_gltf(p: &Path) -> Result<Gltf, String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Gltf::from_glb(&b).map_err(|e| format!("{}: the engine's reader refuses it: {} {}", p.display(), e.0, ""))
}

pub(crate) fn kind_of(name: &str) -> (&'static str, &'static str) {
    if name.ends_with(".glb") {
        ("mesh", "glb")
    } else if name.ends_with(".astc.ktx2") {
        ("texture", "astc")
    } else if name.ends_with(".bc7.ktx2") {
        ("texture", "bc7")
    } else if name.ends_with(".png") {
        ("image", "png")
    } else if name.ends_with(".jpg") {
        ("image", "jpg")
    } else {
        ("data", "")
    }
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1024 * 1024 { format!("{:.2} MB", b as f64 / 1048576.0) } else { format!("{:.1} KB", b as f64 / 1024.0) }
}

/// Write the manifest and both reports; returns the JSON report and the problems.
pub fn build(x: &Inputs) -> Result<(Value, Vec<String>), String> {
    let mut problems = Vec::new();
    let mut notes: Vec<String> = x.notes.to_vec();

    // Inputs and their hashes.
    let mut inputs = Vec::new();
    for (role, p) in x.inputs {
        let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        inputs.push(json!({ "role": role, "file": s(p).replace('\\', "/"), "bytes": b.len(), "sha256": sha256_hex(&b) }));
    }
    let master_path = x.out.join(x.master);
    let master_g = read_gltf(&master_path)?;
    let master_levels = levels(&master_g);
    // GPU skinning's soft cap (S3-9): a rig over 64 joints is a note (it
    // draws), the same warning `cargo xtask check` gives.
    notes.extend(evani_guide::modelcheck::skin_warnings(&master_g).into_iter().map(|w| format!("{}: {w}", x.master)));

    // Per tier.
    let mut tiers_json = Vec::new();
    let mut all_files: BTreeSet<String> = BTreeSet::new();
    let mut validator = Vec::new();
    let mut checks = vec![json!({ "what": "master", "file": x.master, "meshcheck": meshcheck(&master_g, &scene_nodes(&master_g)) })];
    for t in x.tiers {
        let screen = Screen::load(x.root, &t.tier)?;
        let path = x.out.join(&t.file);
        let g = read_gltf(&path)?;
        let input_g = if x.evani_model {
            let src = x.inputs.iter().find(|(r, _)| r == &format!("{} input", t.tier)).map(|(_, p)| p.clone()).unwrap_or_else(|| master_path.clone());
            read_gltf(&src)?
        } else {
            master_g.clone()
        };
        read_back(x, t, &g, &input_g, &mut problems);
        let lv = levels(&g);
        let (v_err, v_warn, v_lines) = validate(x.tools, &path)?;
        if v_err > 0 {
            problems.push(format!("{}: the Khronos validator found {v_err} errors: {}", t.file, v_lines.join("; ")));
        }
        // The validator skips EXT_meshopt_compression's buffers, so the
        // geometry is validated on the uncompressed (quantized) twin.
        let (g_err, g_warn, g_lines) = match &t.twin {
            Some(tw) => validate(x.tools, tw)?,
            None => (0, 0, Vec::new()),
        };
        if g_err > 0 {
            problems.push(format!("{}: the Khronos validator found {g_err} errors in its geometry (uncompressed twin): {}", t.file, g_lines.join("; ")));
        }
        validator.push(json!({ "file": t.file, "errors": v_err, "warnings": v_warn, "error_lines": v_lines, "geometry": { "checked": t.twin.is_some(), "errors": g_err, "warnings": g_warn, "error_lines": g_lines } }));
        let lod0_nodes = g.evani.as_ref().and_then(|e| e.lods.first()).and_then(|l| l.node_index).map(|i| subtree(&g, i)).unwrap_or_else(|| scene_nodes(&g));
        checks.push(json!({ "what": format!("{} level 0", t.tier), "file": t.file, "meshcheck": meshcheck(&g, &lod0_nodes) }));

        let mut files = Vec::new();
        let (mut raw, mut gz, mut br) = (0, 0, 0);
        let (mut astc_gpu, mut bc7_gpu, mut astc_dl, mut bc7_dl) = (0u64, 0u64, 0u64, 0u64);
        for f in &t.files {
            let p = x.out.join(f);
            let (r, z, b) = sizes(&p)?;
            raw += r;
            gz += z;
            br += b;
            let (_, format) = kind_of(f);
            match format {
                "astc" | "bc7" => {
                    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let gpu = texture::ktx2_level_bytes(&bytes);
                    if format == "astc" {
                        astc_gpu += gpu;
                        astc_dl += r;
                    } else {
                        bc7_gpu += gpu;
                        bc7_dl += r;
                    }
                }
                "glb" => {
                    astc_dl += r;
                    bc7_dl += r;
                }
                _ => {}
            }
            all_files.insert(f.clone());
            files.push(json!({ "file": f, "raw": r, "gzip": z, "brotli": b }));
        }
        let budget = screen.max_texture_mb.map(|m| (m * 1048576.0) as u64);
        let tex_over = budget.is_some_and(|b| astc_gpu.max(bc7_gpu) > b);
        if tex_over {
            problems.push(format!("{}: texture memory {} is over the {} tier budget {}", t.file, fmt_bytes(astc_gpu.max(bc7_gpu)), t.tier, fmt_bytes(budget.unwrap_or(0))));
        }
        let (lod0_tris, lod0_draws) = lv.first().map_or((0, 0), |l| (l.1, l.2));
        // The tier limits are for a whole frame: one model's most detailed
        // level over them can never fit, so it is a problem like texture memory.
        let tris_over = screen.max_triangles.is_some_and(|m| lod0_tris > m);
        let draws_over = screen.max_draw_calls.is_some_and(|m| lod0_draws > m);
        if tris_over {
            problems.push(format!("{}: level 0 has {lod0_tris} triangles, over the {} tier's whole-frame limit of {}", t.file, t.tier, screen.max_triangles.unwrap_or(0)));
        }
        if draws_over {
            problems.push(format!("{}: level 0 needs {lod0_draws} draw calls, over the {} tier's whole-frame limit of {}", t.file, t.tier, screen.max_draw_calls.unwrap_or(0)));
        }
        tiers_json.push(json!({
            "tier": t.tier,
            "file": t.file,
            "note": t.note,
            "screen": { "height": screen.height, "height_from": screen.height_from, "render_scale": screen.render_scale, "dpr_cap": screen.dpr_cap, "pixels": screen.pixels() },
            "texture_size": t.size.as_ref().map(|c| json!({ "size": c.size, "fraction": c.fraction, "floor": c.floor, "cap": c.cap, "screen_px": c.screen_px })),
            "images": t.images.iter().map(|(n, w, h, core)| json!({ "name": n, "w": w, "h": h, "core": core })).collect::<Vec<_>>(),
            "lods": lv.iter().enumerate().map(|(k, (node, tris, draws))| {
                let r = t.rungs.get(k);
                json!({ "node": node, "triangles": tris, "draw_calls": draws, "ratio": r.map(|r| r.ratio), "error": r.map(|r| r.error), "min_screen": r.map(|r| r.min_screen) })
            }).collect::<Vec<_>>(),
            "triangles": lod0_tris,
            "draw_calls": lod0_draws,
            "budget": { "max_triangles": screen.max_triangles, "max_draw_calls": screen.max_draw_calls, "triangles_over": tris_over, "draw_calls_over": draws_over },
            "texture_memory": { "astc": astc_gpu, "bc7": bc7_gpu, "budget": budget, "over": tex_over },
            "download": { "astc_device": astc_dl, "bc7_device": bc7_dl },
            "files": files,
            "bytes": { "raw": raw, "gzip": gz, "brotli": br },
            "clips": clips_json(t),
        }));
    }

    // Manifest (AssetManifest v1 entries, plus tier and format).
    let mut assets = Vec::new();
    for f in &all_files {
        let b = std::fs::read(x.out.join(f)).map_err(|e| e.to_string())?;
        let (kind, format) = kind_of(f);
        let tier = x.tiers.iter().find(|t| t.files.contains(f)).map(|t| t.tier.clone());
        assets.push(json!({ "name": format!("models/{}/{f}", x.id), "hash": evani_core::assets::content_hash_hex(&b), "size": b.len(), "url": f, "kind": kind, "tier": tier, "format": format }));
    }
    let manifest_name = format!("{}.manifest.json", x.id);
    let manifest = json!({ "version": 1, "assets": assets });
    let mtext = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())? + "\n";
    std::fs::write(x.out.join(&manifest_name), &mtext).map_err(|e| e.to_string())?;
    evani_core::assets::AssetManifest::from_json(&mtext).map_err(|e| format!("{manifest_name}: the engine's manifest reader refuses it: {e}"))?;
    all_files.insert(manifest_name.clone());

    // The package: everything a game would ship (not the master, the reports or the work folder).
    let (mut p_raw, mut p_gz, mut p_br) = (0, 0, 0);
    for f in &all_files {
        let (r, z, b) = sizes(&x.out.join(f))?;
        p_raw += r;
        p_gz += z;
        p_br += b;
    }
    let over_cap = p_raw > PACKAGE_CAP_BYTES;
    if over_cap {
        notes.push(format!("the package ({}) is over the 10 MB playground cap by itself: every tier's copy of every texture counts (lower --tiers, a --min-distance, or smaller source textures bring it down)", mb(p_raw)));
    }
    let report = json!({
        "format": 1,
        "id": x.id,
        "converter": format!("evani-convert {}", env!("CARGO_PKG_VERSION")),
        "evani_model_input": x.evani_model,
        "look": x.opt.look,
        "min_distance_m": x.opt.min_distance,
        "tools": crate::tools::versions().into_iter().map(|(n, v)| json!({ "name": n, "version": v })).collect::<Vec<_>>(),
        "inputs": inputs,
        "master": { "file": x.master, "bytes": std::fs::metadata(&master_path).map(|m| m.len()).unwrap_or(0), "levels": master_levels.iter().map(|(n, t, d)| json!({ "node": n, "triangles": t, "draw_calls": d })).collect::<Vec<_>>() },
        "tiers": tiers_json,
        "manifest": manifest_name,
        "package": { "files": all_files.len(), "raw": p_raw, "gzip": p_gz, "brotli": p_br, "cap": PACKAGE_CAP_BYTES, "over_cap": over_cap },
        "meshcheck": checks,
        "validator": validator,
        "read_back": { "reader": "evani-gltf", "problems": problems },
        "gate": x.gate,
        "look_match": x.look_match,
        "notes": notes,
    });
    let jtext = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
    std::fs::write(x.out.join(format!("{}.report.json", x.id)), jtext).map_err(|e| e.to_string())?;
    std::fs::write(x.out.join(format!("{}.report.md", x.id)), markdown(&report)).map_err(|e| e.to_string())?;
    // The gate's findings fail the run too (they are listed in its own section).
    if let Some(g) = x.gate {
        problems.extend(g["problems"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
    }
    Ok((report, problems))
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// The report for people.
pub fn markdown(r: &Value) -> String {
    let mut m = String::new();
    let id = r["id"].as_str().unwrap_or("");
    m += &format!("# Model conversion: {id}\n\n");
    m += &format!("{}; tools: {}.\n\n", r["converter"].as_str().unwrap_or(""), r["tools"].as_array().into_iter().flatten().map(|t| format!("{} {}", t["name"].as_str().unwrap_or(""), t["version"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", "));
    m += "## Inputs\n\n| Role | File | Bytes | SHA-256 |\n|---|---|---:|---|\n";
    for i in r["inputs"].as_array().into_iter().flatten() {
        m += &format!("| {} | {} | {} | `{}` |\n", i["role"].as_str().unwrap_or(""), i["file"].as_str().unwrap_or(""), u(&i["bytes"]), i["sha256"].as_str().unwrap_or(""));
    }
    m += &format!("\nMaster kept untouched: `{}` ({}), never downloaded by a game.\n\n", r["master"]["file"].as_str().unwrap_or(""), fmt_bytes(u(&r["master"]["bytes"])));
    m += "## Tiers\n\n| Tier | File | Triangles per level | Draw calls | Texture size | Texture memory (ASTC / BC7 / budget) | Phone download | Desktop download | Tier files raw / gzip / brotli |\n|---|---|---|---:|---:|---|---:|---:|---|\n";
    for t in r["tiers"].as_array().into_iter().flatten() {
        let lods: Vec<String> = t["lods"].as_array().into_iter().flatten().map(|l| u(&l["triangles"]).to_string()).collect();
        let no_images = t["images"].as_array().is_none_or(Vec::is_empty);
        let ts = if no_images { "no textures".into() } else { t["texture_size"]["size"].as_u64().map_or("-".into(), |s| s.to_string()) };
        let tm = &t["texture_memory"];
        m += &format!(
            "| {} | `{}` | {}{} | {}{} | {} | {} / {} / {}{} | {} | {} | {} / {} / {} |\n",
            t["tier"].as_str().unwrap_or(""),
            t["file"].as_str().unwrap_or(""),
            lods.join(" / "),
            if t["budget"]["triangles_over"].as_bool() == Some(true) { " **over**" } else { "" },
            u(&t["draw_calls"]),
            if t["budget"]["draw_calls_over"].as_bool() == Some(true) { " **over**" } else { "" },
            ts,
            fmt_bytes(u(&tm["astc"])),
            fmt_bytes(u(&tm["bc7"])),
            tm["budget"].as_u64().map_or("-".into(), fmt_bytes),
            if tm["over"].as_bool() == Some(true) { " **over**" } else { "" },
            fmt_bytes(u(&t["download"]["astc_device"])),
            fmt_bytes(u(&t["download"]["bc7_device"])),
            fmt_bytes(u(&t["bytes"]["raw"])),
            fmt_bytes(u(&t["bytes"]["gzip"])),
            fmt_bytes(u(&t["bytes"]["brotli"])),
        );
    }
    let p = &r["package"];
    m += &format!(
        "\n**Package** (every tier file, texture and the manifest; not the master or reports): {} files, {} raw, {} gzip, {} brotli, against the 10 MB playground cap: **{}**.\n\n",
        u(&p["files"]),
        fmt_bytes(u(&p["raw"])),
        fmt_bytes(u(&p["gzip"])),
        fmt_bytes(u(&p["brotli"])),
        if p["over_cap"].as_bool() == Some(true) { "over" } else { "within" }
    );
    let animated: Vec<&Value> = r["tiers"].as_array().into_iter().flatten().filter(|t| t["clips"].is_object()).collect();
    if !animated.is_empty() {
        m += "## Animation clips\n\nEach tier file carries the compact clip block (docs/MODEL_FORMAT.md \"Clips\") instead of the glTF animations, which only the master keeps. Error is the worst distance between the baked clip and the master's animation resampled at 30 Hz, at shell points around every joint, over every frame.\n\n| Tier | Clip | Frames | Tracks (constant) | At rest, dropped | Keys | Rotation bits | Bytes | Worst shell error | Worst node | Bound |\n|---|---|---:|---:|---:|---:|---:|---:|---:|---|---|\n";
        for t in &animated {
            let c = &t["clips"];
            for k in c["clips"].as_array().into_iter().flatten() {
                m += &format!(
                    "| {} | `{}` | {} | {} ({}) | {} | {} | {} | {} | {:.3} mm | `{}` | {}{} mm |\n",
                    t["tier"].as_str().unwrap_or(""),
                    k["name"].as_str().unwrap_or(""),
                    u(&k["frames"]),
                    u(&k["tracks"]),
                    u(&k["constant"]),
                    u(&k["rest"]),
                    u(&k["keys"]),
                    u(&k["rot_bits"]),
                    u(&k["bytes"]),
                    k["worst_shell_error_m"].as_f64().unwrap_or(0.0) * 1000.0,
                    k["worst_node"].as_str().unwrap_or(""),
                    if k["within"].as_bool() == Some(true) { "within " } else { "**over** " },
                    c["bound_m"].as_f64().unwrap_or(0.0) * 1000.0,
                );
            }
            m += &format!("| {} | (block) | | | | | | {} | | | |\n", t["tier"].as_str().unwrap_or(""), u(&c["block_bytes"]));
        }
        m += "\n";
        let mut seen: Vec<String> = Vec::new();
        for t in &animated {
            for n in t["clips"]["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let line = format!("- {}: {n}\n", t["tier"].as_str().unwrap_or(""));
                if !seen.contains(&line) {
                    m += &line;
                    seen.push(line);
                }
            }
        }
        if !seen.is_empty() {
            m += "\n";
        }
    }
    m += "## Files\n\n| File | Raw | Gzip | Brotli |\n|---|---:|---:|---:|\n";
    for t in r["tiers"].as_array().into_iter().flatten() {
        for f in t["files"].as_array().into_iter().flatten() {
            m += &format!("| `{}` | {} | {} | {} |\n", f["file"].as_str().unwrap_or(""), u(&f["raw"]), u(&f["gzip"]), u(&f["brotli"]));
        }
    }
    m += "\n## Checks\n\nMesh findings come from evani-model's meshcheck on level 0 in world space. They are findings, not errors: `self_intersection` counts crossing triangle pairs, also between separate parts that sit inside each other (wheels in a body), and simplified levels can add `duplicate` or `non_manifold_edge`.\n\n| What | Triangles | Components | Mesh findings |\n|---|---:|---:|---|\n";
    for c in r["meshcheck"].as_array().into_iter().flatten() {
        let mc = &c["meshcheck"];
        let issues: Vec<String> = mc["issues"].as_object().into_iter().flatten().map(|(k, v)| format!("{k} {v}")).collect();
        m += &format!("| {} | {} | {} | {} |\n", c["what"].as_str().unwrap_or(""), u(&mc["triangles"]), u(&mc["components"]), if issues.is_empty() { "none".into() } else { issues.join(", ") });
    }
    m += "\nKhronos validator (glTF-Transform `validate`): ";
    m += &r["validator"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| format!("`{}` {} errors, {} warnings (geometry, on the uncompressed twin: {} errors, {} warnings)", v["file"].as_str().unwrap_or(""), u(&v["errors"]), u(&v["warnings"]), u(&v["geometry"]["errors"]), u(&v["geometry"]["warnings"])))
        .collect::<Vec<_>>()
        .join("; ");
    m += ". The validator cannot look inside EXT_meshopt_compression, so each file's geometry is also validated on a twin made with the same settings without `-cc`.";
    let probs = r["read_back"]["problems"].as_array().cloned().unwrap_or_default();
    m += &format!("\n\nRead back with the engine's reader (evani-gltf): {}\n", if probs.is_empty() { "every file opens; anchors, named nodes, levels and texture files are all there.".to_owned() } else { format!("**{} problems**", probs.len()) });
    for p in &probs {
        m += &format!("- {}\n", p.as_str().unwrap_or(""));
    }
    if r["look_match"].is_object() {
        m += &look_markdown(&r["look_match"]);
    }
    if r["gate"].is_object() {
        m += &gate_markdown(&r["gate"]);
    }
    let notes = r["notes"].as_array().cloned().unwrap_or_default();
    if !notes.is_empty() {
        m += "\n## Notes\n\n";
        for n in notes {
            m += &format!("- {}\n", n.as_str().unwrap_or(""));
        }
    }
    m
}

fn setting_text(s: &Value) -> String {
    if !s.is_object() {
        return "-".into();
    }
    let tex = s["texture_size"].as_u64().map_or(String::new(), |t| format!(", textures {t}"));
    format!("ratio {} (error {}){tex}, bits {}/{}/{}", s["ratio"], s["error"], s["vp"], s["vn"], s["vt"])
}

/// The look matching section (S3-12): what was dropped, merged and moved, and
/// what it saved per tier.
fn look_markdown(l: &Value) -> String {
    let mut m = format!("\n## Look matching (`{}`)\n\n", l["look"].as_str().unwrap_or(""));
    if l["has_table"].as_bool() != Some(true) {
        m += "This look has no row in the converter's look table, so nothing was changed.\n";
        return m;
    }
    let dropped: Vec<String> = l["dropped_slots"].as_array().into_iter().flatten().map(|d| format!("`{}` ({} materials)", d["slot"].as_str().unwrap_or(""), d["materials"])).collect();
    m += &format!(
        "- Dropped (the look never reads): {}\n- Materials: {} before, {} after ({} merged groups)\n- Primitives in the file: {}\n",
        if dropped.is_empty() { "nothing".to_owned() } else { dropped.join(", ") },
        l["materials"]["before"],
        l["materials"]["after"],
        l["merged"].as_array().map_or(0, Vec::len),
        l["primitives"]
    );
    if l["relief_materials"].as_u64().unwrap_or(0) > 0 {
        m += &format!("- {} material(s) carry a relief recipe: their height, normal and base maps are kept.\n", l["relief_materials"]);
    }
    let sp = &l["snap_palette"];
    if sp["asked"].as_bool() == Some(true) {
        let snapped = sp["snapped"].as_array().cloned().unwrap_or_default();
        m += &format!("- `--snap-palette`: {} base colour(s) moved (limit {} CIELAB), {} left alone\n", snapped.len(), sp["max_delta_e"], sp["skipped"].as_array().map_or(0, Vec::len));
        for s in &snapped {
            m += &format!("  - `{}`: {} to {} (distance {:.1})\n", s["material"].as_str().unwrap_or(""), s["from"], s["to"], s["delta_e"].as_f64().unwrap_or(0.0));
        }
    }
    let tiers = l["tiers"].as_array().cloned().unwrap_or_default();
    if !tiers.is_empty() {
        m += "\nWithout matching, the same model on the same ladder setting:\n\n| Tier | Draw calls without / with | Maps not shipped | Phone download saved | Desktop download saved | Texture memory saved (ASTC / BC7) |\n|---|---:|---|---:|---:|---|\n";
        for t in &tiers {
            let n = |v: &Value| v.as_i64().unwrap_or(0);
            let maps = t["maps_not_shipped"]["images"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            m += &format!(
                "| {} | {} / {} | {} | {} | {} | {} / {} |\n",
                t["tier"].as_str().unwrap_or(""),
                t["draw_calls"]["without"],
                t["draw_calls"]["with"],
                if maps.is_empty() { "-".to_owned() } else { maps },
                fmt_bytes(n(&t["download_saved"]["astc_device"]).max(0) as u64),
                fmt_bytes(n(&t["download_saved"]["bc7_device"]).max(0) as u64),
                fmt_bytes(n(&t["maps_not_shipped"]["astc_gpu_bytes"]).max(0) as u64),
                fmt_bytes(n(&t["maps_not_shipped"]["bc7_gpu_bytes"]).max(0) as u64)
            );
        }
    }
    m
}

/// The quality gate's section (S3-6).
fn gate_markdown(g: &Value) -> String {
    let th = &g["thresholds"];
    let mut m = format!(
        "\n## Quality gate\n\nEvery tier rendered in the engine's own renderer ({}) from {} fixed views ({}), original against converted, scored inside the model's part of each picture. A setting passes when the worst view has worst pixel (99.9th percentile of our colour and edge difference) at most **{}** and SSIMULACRA2 at least the tier's bar: **{}** or, when lower, the tier's format floor (its best rung: unsimplified, 16-bit vertices, full textures, whose sub-pixel vertex rounding SSIMULACRA2 already scores below 100) less **{}**. The floor counts only when the best rung itself scores at least **{}** with worst pixel at most **{}**; otherwise the bar stays **{}** and the tier fails as a broken conversion. Thresholds **{}**. Each tier's ladder runs from the best rung toward cheaper ones (ratio, textures, then position/normal/UV bits) and includes the tier's own setting; the cheapest passing setting makes the tier file, which is then rendered and scored again. {} renders in {} s.\n\n",
        g["renderer"].as_str().unwrap_or(""),
        g["views"].as_array().map_or(0, Vec::len),
        g["views"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
        th["worst_pixel_max"],
        th["ssimulacra2_min"],
        th["floor_slack"],
        th["floor_min"],
        th["worst_pixel_max"],
        th["ssimulacra2_min"],
        th["status"].as_str().unwrap_or(""),
        u(&g["renders"]),
        g["seconds"],
    );
    m += "| Tier | Render | Compared with | Best rung | Floor | Bar | Chosen | Tried | Tier file: worst SSIMULACRA2 | Worst pixel | Verdict | Sheet |\n|---|---|---|---|---:|---:|---|---:|---:|---:|---|---|\n";
    for t in g["tiers"].as_array().into_iter().flatten() {
        let f = &t["final"];
        m += &format!(
            "| {} | {}x{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            t["tier"].as_str().unwrap_or(""),
            t["render"][0],
            t["render"][1],
            t["reference"].as_str().unwrap_or(""),
            setting_text(&t["start"]),
            if t["floor_used"].as_bool() == Some(false) { format!("{} (not used)", t["floor"]) } else { t["floor"].to_string() },
            t["ssimulacra2_bar"],
            if t["chosen"].is_object() { setting_text(&t["chosen"]) } else { "**start fails**".into() },
            t["trials"].as_array().map_or(0, Vec::len),
            f["ssimulacra2"],
            f["worst_pixel"],
            if f["pass"].as_bool() == Some(true) { "pass" } else { "**fail**" },
            t["sheet"].as_str().map_or("-".into(), |s| format!("[{s}]({s})")),
        );
    }
    m += "\nPer view, the finished tier file (SSIMULACRA2 / worst pixel):\n\n| Tier |";
    let names: Vec<&str> = g["views"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    for n in &names {
        m += &format!(" {n} |");
    }
    m += "\n|---|";
    m += &"---:|".repeat(names.len());
    m += "\n";
    for t in g["tiers"].as_array().into_iter().flatten() {
        m += &format!("| {} |", t["tier"].as_str().unwrap_or(""));
        for v in t["final"]["views"].as_array().into_iter().flatten() {
            m += &format!(" {} / {} |", v["ssimulacra2"], v["worst_pixel"]);
        }
        m += "\n";
    }
    let probs: Vec<&str> = g["problems"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !probs.is_empty() {
        m += &format!("\n**{} gate problems**\n", probs.len());
        for p in probs {
            m += &format!("- {p}\n");
        }
    }
    if u(&g["calibration_pairs"]) > 0 {
        m += &format!("\n{} blind-test pairs written for the calibration page.\n", u(&g["calibration_pairs"]));
    }
    m
}

/// The output folder's files, for tests: relative paths, sorted.
pub fn list_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut st = vec![dir.to_path_buf()];
    while let Some(d) = st.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                st.push(p);
            } else if let Ok(r) = p.strip_prefix(dir) {
                out.push(r.to_path_buf());
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_flags_a_tier_over_its_frame_limits() {
        let tier = |over: bool| {
            json!({
                "tier": "low", "file": "x.low.glb", "lods": [{ "triangles": 50000 }], "draw_calls": 3,
                "images": [], "budget": { "max_triangles": 40000, "max_draw_calls": 100, "triangles_over": over, "draw_calls_over": false },
                "texture_memory": { "astc": 0, "bc7": 0, "budget": 0, "over": false }, "download": {}, "bytes": {}, "files": []
            })
        };
        let r = |over: bool| json!({ "id": "x", "tiers": [tier(over)], "package": {}, "master": {} });
        let row = |over: bool| markdown(&r(over)).lines().find(|l| l.starts_with("| low |")).map(str::to_owned).unwrap_or_default();
        assert!(row(true).contains("50000 **over**"), "{}", row(true));
        assert!(!row(false).contains("**over**"), "{}", row(false));
    }
}
