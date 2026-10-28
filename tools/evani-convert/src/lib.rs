//! `cargo xtask convert <file>`: turn a model (`.glb`, `.gltf`, `.blend`, or
//! Evani.Model's per-tier files) into the engine's per-tier files
//! (docs/MODEL_FORMAT.md), their texture sets, a manifest and two reports.
//! STAGE3_PLAN item S3-5.
//!
//! The work is done by pinned external programs run as separate processes
//! (never linked): glTF-Transform (dedup, prune, weld, tangents, validate;
//! sharp for resizing), gltfpack (simplify, quantize, meshopt-compress) and
//! KTX-Software's `ktx` (ASTC 4x4, and UASTC transcoded to BC7). The order
//! is glTF-Transform's `optimize` order. This crate decides the settings,
//! joins each tier's levels of detail into one file, writes the Evani
//! metadata gltfpack drops, and checks every output with the engine's own
//! reader (crates/evani-gltf).
//!
//! Animated models (S3-7): the master keeps its glTF animations; each tier
//! file instead carries the compact clip block ([`clips`],
//! docs/MODEL_FORMAT.md "Clips"), baked within the tier's `clip_error_m`.

pub mod bc7;
pub mod clips;
pub mod fixture;
pub mod gate;
pub mod glb;
pub mod layers;
pub mod look;
pub mod merge;
pub mod metric;
pub mod normals;
pub mod report;
pub mod runner;
pub mod sockets;
pub mod texture;
pub mod tools;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use glb::Doc;
use texture::{ImageUse, Screen, SizeChoice};
use tools::{Tools, s};

/// Every tier the converter knows, in order (assets/tiers/*.json).
pub const ALL_TIERS: [&str; 6] = ["lite", "low", "medium", "high", "ultra", "vr"];
/// Made unless `--tiers` says otherwise.
pub const DEFAULT_TIERS: [&str; 4] = ["low", "medium", "high", "ultra"];
/// The LOD ladder (ratios, error limits, switch points, vertex bits).
pub const LADDER: &str = include_str!("../ladder.json");
const IMAGES_MJS: &str = include_str!("images.mjs");
const BLEND_PY: &str = include_str!("blend_export.py");

pub const USAGE: &str = "cargo xtask convert <file.glb|.gltf|.blend|<name>.<tier>.glb> [--id <id>] [--look <look> [--snap-palette]] [--min-distance <m>] [--tiers low,medium,high,ultra] [--clip-error <m>] [--out <folder>] [--keep-work] [--gate] [--gate-pairs <folder>]\n       cargo xtask convert --calibration <folder>          (the blind test page for the gate's thresholds)\n       cargo xtask convert --calibration-result <folder>   (thresholds suggested from the answers)";

#[derive(Clone, Debug)]
pub struct Options {
    pub input: PathBuf,
    pub id: Option<String>,
    /// Look matching (S3-12): drop the maps this look never samples, merge
    /// materials equal in what it reads (see look.rs).
    pub look: Option<String>,
    /// With `look`: also move untextured base colours to the look's palette
    /// (within a CIELAB distance, listed in the report).
    pub snap_palette: bool,
    pub min_distance: Option<f64>,
    /// Empty: the defaults (or, for Evani.Model files, the tiers present).
    pub tiers: Vec<String>,
    pub out: Option<PathBuf>,
    /// The engine checkout (catalog/, assets/tiers/, .evani/bin/).
    pub root: PathBuf,
    pub keep_work: bool,
    /// The worst clip shell error (metres) for every tier; `None`: each
    /// tier's `clip_error_m` from ladder.json.
    pub clip_error: Option<f64>,
    /// The quality gate (S3-6): render, score and search each tier's ladder.
    pub gate: bool,
    /// Also write blind-test pairs for the calibration page here (implies `gate`).
    pub gate_pairs: Option<PathBuf>,
}

impl Options {
    pub fn new(root: &Path, input: &Path) -> Options {
        Options { input: input.to_path_buf(), id: None, look: None, snap_palette: false, min_distance: None, tiers: Vec::new(), out: None, root: root.to_path_buf(), keep_work: false, clip_error: None, gate: false, gate_pairs: None }
    }

    pub fn parse(root: &Path, args: &[String]) -> Result<Options, String> {
        let mut it = args.iter();
        let mut input = None;
        let mut o = Options::new(root, Path::new(""));
        while let Some(a) = it.next() {
            let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value\nusage: {USAGE}"));
            match a.as_str() {
                "--id" => o.id = Some(val("--id")?),
                "--look" => o.look = Some(val("--look")?),
                "--snap-palette" => o.snap_palette = true,
                "--min-distance" => {
                    let v = val("--min-distance")?;
                    o.min_distance = Some(v.parse::<f64>().ok().filter(|d| *d > 0.0).ok_or_else(|| format!("--min-distance {v}: a distance in metres above 0"))?);
                }
                "--clip-error" => {
                    let v = val("--clip-error")?;
                    o.clip_error = Some(v.parse::<f64>().ok().filter(|d| d.is_finite() && *d > 0.0).ok_or_else(|| format!("--clip-error {v}: a distance in metres above 0"))?);
                }
                "--tiers" => o.tiers = val("--tiers")?.split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect(),
                "--out" => o.out = Some(PathBuf::from(val("--out")?)),
                "--keep-work" => o.keep_work = true,
                "--gate" => o.gate = true,
                "--gate-pairs" => {
                    o.gate_pairs = Some(PathBuf::from(val("--gate-pairs")?));
                    o.gate = true;
                }
                f if f.starts_with("--") => return Err(format!("unknown option {f}\nusage: {USAGE}")),
                f if input.is_none() => input = Some(PathBuf::from(f)),
                f => return Err(format!("one input file only (got {f} too)\nusage: {USAGE}")),
            }
        }
        o.input = input.ok_or_else(|| format!("no input file\nusage: {USAGE}"))?;
        if o.snap_palette && o.look.is_none() {
            return Err(format!("--snap-palette needs --look <look> (the palette is the look's)\nusage: {USAGE}"));
        }
        for t in &o.tiers {
            if !ALL_TIERS.contains(&t.as_str()) {
                return Err(format!("--tiers: unknown tier {t} (known: {})", ALL_TIERS.join(", ")));
            }
        }
        if let Some(id) = &o.id
            && !valid_id(id)
        {
            return Err(format!("--id {id}: use lowercase letters, digits, `-` and `_` (it names files and the root node)"));
        }
        Ok(o)
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && !id.starts_with(['_', '-']) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// A file stem as an id: lowercase, other characters as `_`.
pub fn id_from(stem: &str) -> String {
    let s: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' }).collect();
    let s = s.trim_matches(['_', '-']).to_owned();
    if s.is_empty() { "model".into() } else { s }
}

/// One level of detail from the ladder.
#[derive(Clone, Debug, PartialEq)]
pub struct Rung {
    pub ratio: f64,
    pub error: f64,
    pub min_screen: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TierLadder {
    pub lods: Vec<Rung>,
    pub vp: u32,
    pub vt: u32,
    pub vn: u32,
    /// The worst clip shell error (metres) for this tier (S3-7).
    pub clip_error_m: f64,
}

pub fn ladder(tier: &str) -> Result<TierLadder, String> {
    let j: Value = serde_json::from_str(LADDER).map_err(|e| format!("ladder.json: {e}"))?;
    let t = j.pointer(&format!("/tiers/{tier}")).ok_or_else(|| format!("ladder.json has no tier {tier}"))?;
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).ok_or_else(|| format!("ladder.json {tier}: no {k}"));
    let bits = |k: &str| t.get(k).and_then(Value::as_u64).and_then(|b| u32::try_from(b).ok()).ok_or_else(|| format!("ladder.json {tier}: no {k}"));
    let mut lods = Vec::new();
    for l in t.get("lods").and_then(Value::as_array).into_iter().flatten() {
        lods.push(Rung { ratio: num(l, "ratio")?, error: num(l, "error")?, min_screen: num(l, "min_screen")? });
    }
    if lods.is_empty() {
        return Err(format!("ladder.json {tier}: no lods"));
    }
    let vn = bits("vn")?;
    if vn != 8 && vn != 16 {
        // gltfpack keeps e.g. 12-bit normals in int16 at 1/16 length: not unit
        // length, so the Khronos validator rejects the file (ACCESSOR_VECTOR3_NON_UNIT).
        return Err(format!("ladder.json {tier}: vn {vn}: normal bits must be 8 or 16 (other widths are stored short of unit length)"));
    }
    let clip_error_m = num(t, "clip_error_m")?;
    if !(clip_error_m.is_finite() && clip_error_m > 0.0) {
        return Err(format!("ladder.json {tier}: clip_error_m {clip_error_m}: a distance in metres above 0"));
    }
    Ok(TierLadder { lods, vp: bits("vp")?, vt: bits("vt")?, vn, clip_error_m })
}

/// Is this one of Evani.Model's per-tier files (already simplified per tier)?
pub fn is_evani_model(json: &Value) -> bool {
    glb::generator(json).starts_with("Evani.Model") && json.pointer("/extras/evani/tier").and_then(Value::as_str).is_some()
}

/// What one conversion wrote.
#[derive(Debug)]
pub struct Outcome {
    pub out_dir: PathBuf,
    pub id: String,
    /// The JSON report (also written as `<id>.report.json`).
    pub report: Value,
    /// Problems the read-back, the validator or the budgets found (also in the report).
    pub errors: Vec<String>,
}

/// What one tier's conversion produced, for the report.
#[derive(Debug, Default)]
pub struct TierOut {
    pub tier: String,
    pub file: String,
    pub size: Option<SizeChoice>,
    pub rungs: Vec<Rung>,
    /// Images: (name, w, h, core file).
    pub images: Vec<(String, u32, u32, String)>,
    /// Files written for this tier, relative to the output folder.
    pub files: Vec<String>,
    pub note: Option<String>,
    /// The same file without meshopt compression (quantized only): the
    /// Khronos validator cannot look inside EXT_meshopt_compression, so the
    /// geometry is validated on this twin. Lives in the work folder.
    pub twin: Option<PathBuf>,
    /// The clip block baked into this tier's file (animated models, S3-7).
    pub clips: Option<clips::Baked>,
}

/// gltfpack once with `-cc` (the shipped file) and once without (the twin the
/// validator can read). `args` are everything but the output and `-cc`.
fn pack(cx: &Ctx, args: &[String], packed: &Path, what: &str) -> Result<PathBuf, String> {
    let twin = packed.with_extension("twin.glb");
    let mut a: Vec<String> = args.to_vec();
    a.extend(["-o".into(), s(packed), "-cc".into(), "-ce".into(), "ext".into()]);
    let v: Vec<&str> = a.iter().map(String::as_str).collect();
    cx.tools.gltfpack(&v, what)?;
    let mut a: Vec<String> = args.to_vec();
    a.extend(["-o".into(), s(&twin)]);
    let v: Vec<&str> = a.iter().map(String::as_str).collect();
    cx.tools.gltfpack(&v, &format!("{what}, uncompressed twin for the validator"))?;
    Ok(twin)
}

struct Ctx {
    tools: Tools,
    root: PathBuf,
    out: PathBuf,
    work: PathBuf,
    id: String,
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write(p: &Path, b: &[u8]) -> Result<(), String> {
    if let Some(d) = p.parent() {
        mkdir(d)?;
    }
    std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))
}

fn copy(a: &Path, b: &Path) -> Result<(), String> {
    if let Some(d) = b.parent() {
        mkdir(d)?;
    }
    std::fs::copy(a, b).map(|_| ()).map_err(|e| format!("copy {} to {}: {e}", a.display(), b.display()))
}

fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Convert one model. Errors that stop the run are `Err`; problems found in
/// finished files (read-back, validator, budgets) are listed in
/// `Outcome::errors` and the report.
pub fn convert(opt: &Options) -> Result<Outcome, String> {
    let tools = Tools::locate(&opt.root)?;
    let input = std::path::absolute(&opt.input).map_err(|e| format!("{}: {e}", opt.input.display()))?;
    if !input.is_file() {
        return Err(format!("{}: no such file", input.display()));
    }
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if let Some(look) = &opt.look {
        let dir = opt.root.join("looks").join(format!("evani-look-{look}"));
        if !dir.is_dir() {
            return Err(format!("--look {look}: no look crate at {}", dir.display()));
        }
    }
    let stem = input.file_name().and_then(|n| n.to_str()).unwrap_or("model").to_owned();
    let stem = stem.rsplit_once('.').map_or(stem.as_str(), |s| s.0).to_owned();

    // .blend: Blender's own glTF exporter, headless (decision 5).
    let mut work_tmp = None;
    let (src_path, src_doc) = match ext.as_str() {
        "glb" | "gltf" => (input.clone(), Doc::read_file(&input)?),
        "blend" => {
            let blender = blender(&opt.root).ok_or_else(|| {
                format!("skipped: {} is a .blend file and Blender {} is not installed\nfix: run `cargo xtask doctor --fix --blender` (or set EVANI_BLENDER to blender.exe), then convert again", input.display(), evani_guide::extools::pin("blender").map_or("", |p| p.version))
            })?;
            let tmp = opt.root.join(".evani").join("convert-tmp").join(id_from(&stem));
            mkdir(&tmp)?;
            let glb_path = tmp.join(format!("{}.glb", id_from(&stem)));
            export_blend(&blender, &input, &glb_path, &tmp)?;
            work_tmp = Some(tmp);
            (glb_path.clone(), Doc::read_file(&glb_path)?)
        }
        _ => return Err(format!("{}: not a .glb, .gltf or .blend file", input.display())),
    };

    let evani_model = is_evani_model(&src_doc.json);
    let model_stem = if evani_model { evani_model_stem(&stem) } else { stem.clone() };
    let id = match &opt.id {
        Some(i) => i.clone(),
        None => id_from(src_doc.json.pointer("/extras/evani/class").and_then(Value::as_str).filter(|c| !c.is_empty()).unwrap_or(&model_stem)),
    };
    let out = match &opt.out {
        Some(o) => std::path::absolute(o).map_err(|e| e.to_string())?,
        None => opt.root.join("assets").join("models").join(&id),
    };
    let work = out.join(".work");
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    mkdir(&work)?;
    mkdir(&out.join("textures"))?;
    write(&work.join("images.mjs"), IMAGES_MJS.as_bytes())?;
    let cx = Ctx { tools, root: opt.root.clone(), out: out.clone(), work: work.clone(), id: id.clone() };

    let result = if evani_model { convert_evani_model(&cx, opt, &input, &model_stem) } else { convert_general(&cx, opt, &src_path, &src_doc, &ext) };
    let (tiers, inputs, master_rel, notes, gate, look_match) = result?;
    let rep = report::build(&report::Inputs { root: &cx.root, out: &out, id: &id, opt, inputs: &inputs, master: &master_rel, tiers: &tiers, notes: &notes, tools: &cx.tools, evani_model, gate: gate.as_ref(), look_match: look_match.as_ref() })?;
    if !opt.keep_work {
        let _ = std::fs::remove_dir_all(&work);
    }
    if let Some(t) = work_tmp {
        let _ = std::fs::remove_dir_all(t);
    }
    Ok(Outcome { out_dir: out, id, errors: rep.1, report: rep.0 })
}

/// Input files with their role, for the report's hashes.
pub type InputList = Vec<(String, PathBuf)>;

/// Tiers, inputs, the master's file, notes, and the gate's report part.
/// The last part is the report's `look_match` (`--look` only).
type Converted = (Vec<TierOut>, InputList, String, Vec<String>, Option<Value>, Option<Value>);

/// `character.low` -> `character`.
fn evani_model_stem(stem: &str) -> String {
    match stem.rsplit_once('.') {
        Some((s, t)) if ALL_TIERS.contains(&t) => s.to_owned(),
        _ => stem.to_owned(),
    }
}

fn wanted_tiers(opt: &Options) -> Vec<String> {
    if opt.tiers.is_empty() { DEFAULT_TIERS.iter().map(|t| (*t).to_owned()).collect() } else { opt.tiers.clone() }
}

fn f(x: f64) -> String {
    let s = format!("{x}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

fn quant_args(l: &TierLadder) -> Vec<String> {
    vec!["-vp".into(), l.vp.to_string(), "-vt".into(), l.vt.to_string(), "-vn".into(), l.vn.to_string()]
}

/// Does a material with a normal map draw a primitive without tangents?
pub(crate) fn needs_tangents(j: &Value) -> bool {
    let mats = j.get("materials").and_then(Value::as_array).cloned().unwrap_or_default();
    j.get("meshes").and_then(Value::as_array).into_iter().flatten().flat_map(|m| m.get("primitives").and_then(Value::as_array).into_iter().flatten()).any(|p| {
        let normal_mapped = p.get("material").and_then(Value::as_u64).and_then(|m| mats.get(m as usize)).is_some_and(|m| m.get("normalTexture").is_some());
        normal_mapped && p.pointer("/attributes/TANGENT").is_none() && p.pointer("/attributes/TEXCOORD_0").is_some()
    })
}

/// A tier's ladder with a gate setting on it: level 0 takes the setting's
/// ratio and error, the lower levels scale with it, the vertex bits are the
/// setting's.
fn apply_setting(lad: &mut TierLadder, st: &gate::Setting) {
    let r0 = lad.lods[0].ratio;
    lad.lods[0].ratio = st.ratio;
    lad.lods[0].error = st.error;
    for r in lad.lods.iter_mut().skip(1) {
        r.ratio = (r.ratio * st.ratio / r0).clamp(0.001, 1.0);
    }
    (lad.vp, lad.vn, lad.vt) = (st.vp, st.vn, st.vt);
}

/// One tier's geometry as it ships: its LOD chain (or one level when the
/// model can't be merged), quantized and meshopt-compressed, with the levels
/// listed in `extras.evani.lods`. The tier files and the quality gate's
/// candidates both come from here, so the gate scores the geometry that
/// ships (the engine picks a level by screen size, and the levels share the
/// file).
struct Geometry<'a> {
    cx: &'a Ctx,
    clean: &'a Path,
    pack_input: &'a Path,
    single: bool,
    lod_cache: BTreeMap<String, Doc>,
}

impl Geometry<'_> {
    /// Write `lad`'s geometry to `packed`, plus the validator's uncompressed
    /// twin when `twin`. Returns the levels made and the twin.
    fn pack(&mut self, tier: &str, lad: &TierLadder, packed: &Path, twin: bool) -> Result<(Vec<Rung>, Option<PathBuf>), String> {
        let cx = self.cx;
        let run = |args: &[String], what: &str| -> Result<Option<PathBuf>, String> {
            if twin {
                return pack(cx, args, packed, what).map(Some);
            }
            let mut a = args.to_vec();
            a.extend(["-o".into(), s(packed), "-cc".into(), "-ce".into(), "ext".into()]);
            let v: Vec<&str> = a.iter().map(String::as_str).collect();
            cx.tools.gltfpack(&v, what)?;
            Ok(None)
        };
        let mut rungs = lad.lods.clone();
        let made = if self.single {
            rungs.truncate(1);
            let r = &rungs[0];
            let mut args: Vec<String> = vec!["-i".into(), s(self.pack_input)];
            if r.ratio < 1.0 {
                args.extend(["-si".into(), f(r.ratio), "-se".into(), f(r.error), "-sv".into()]);
            }
            args.extend(["-kn", "-ke", "-kv", "-tr"].map(String::from));
            args.extend(quant_args(lad));
            run(&args, &format!("gltfpack ({tier})"))?
        } else {
            let mut levels = Vec::new();
            for r in &rungs {
                let key = format!("r{}_e{}", f(r.ratio), f(r.error));
                if !self.lod_cache.contains_key(&key) {
                    let p = cx.work.join(format!("lod_{key}.glb"));
                    let mut args: Vec<String> = vec!["-i".into(), s(self.clean), "-o".into(), s(&p)];
                    if r.ratio < 1.0 {
                        args.extend(["-si".into(), f(r.ratio), "-se".into(), f(r.error), "-sv".into()]);
                    }
                    args.extend(["-noq", "-kn", "-ke", "-kv", "-tr"].map(String::from));
                    let a: Vec<&str> = args.iter().map(String::as_str).collect();
                    cx.tools.gltfpack(&a, &format!("gltfpack simplify {key}"))?;
                    self.lod_cache.insert(key.clone(), Doc::read_file(&p)?);
                }
                if let Some(d) = self.lod_cache.get(&key) {
                    levels.push(d.clone());
                }
            }
            let merged = merge::merge(&cx.id, &levels)?;
            // Beside the levels (their image URIs are relative to the work folder).
            let stem = packed.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mp = cx.work.join(format!("{stem}.merged.glb"));
            write(&mp, &merged.to_glb()?)?;
            let mut args: Vec<String> = vec!["-i".into(), s(&mp)];
            args.extend(["-kn", "-ke", "-kv", "-tr"].map(String::from));
            args.extend(quant_args(lad));
            run(&args, &format!("gltfpack compress ({tier})"))?
        };
        Ok((rungs, made))
    }

    /// `extras.evani.lods` for `rungs` (none for one level).
    fn lods_json(&self, rungs: &[Rung]) -> Option<Value> {
        (!self.single).then(|| Value::Array(rungs.iter().enumerate().map(|(k, r)| json!({ "node": merge::lod_node_name(&self.cx.id, k), "min_screen": r.min_screen })).collect()))
    }
}

/// glTF-Transform's clean of `input`: dedup, prune and weld (and tangents when
/// a normal map has none), after the engine's own normals for a mesh without
/// any (so gltfpack's vertex merge cannot change how it shades). Returns
/// `clean.gltf` in the context's work folder; `notes` (the main run's) hears
/// what was added.
fn clean_file(cx: &Ctx, input: &Path, tangents: bool, notes: Option<&mut Vec<String>>) -> Result<PathBuf, String> {
    let mut scratch = Vec::new();
    let notes = notes.unwrap_or(&mut scratch);
    let (a, b) = (cx.work.join("a.glb"), cx.work.join("b.glb"));
    let clean = cx.work.join("clean.gltf");
    let with_normals = cx.work.join("normals.glb");
    let base = match normals::add_engine_normals(input, &with_normals)? {
        0 => input.to_path_buf(),
        n => {
            notes.push(format!("normals written as the engine draws them (smooth over the input's own triangles) for {n} primitive(s) that had none"));
            with_normals
        }
    };
    cx.tools.gltf_transform(&["dedup", &s(&base), &s(&a)], "gltf-transform dedup")?;
    cx.tools.gltf_transform(&["prune", &s(&a), &s(&b), "--keep-leaves", "true", "--keep-attributes", "true", "--keep-solid-textures", "true"], "gltf-transform prune")?;
    if tangents {
        let c = cx.work.join("c.glb");
        cx.tools.gltf_transform(&["weld", &s(&b), &s(&c)], "gltf-transform weld")?;
        cx.tools.gltf_transform(&["tangents", &s(&c), &s(&clean)], "gltf-transform tangents")?;
        notes.push("tangents generated (MikkTSpace, glTF-Transform `tangents`): a normal map had none".into());
    } else {
        cx.tools.gltf_transform(&["weld", &s(&b), &s(&clean)], "gltf-transform weld")?;
    }
    Ok(clean)
}

/// Look matching (S3-12) of a general model: the matched document (`None`
/// when the look has no table or matching changed nothing) and what it did.
/// A `.gltf` is matched from its packed master, so its image files stay
/// reachable.
fn match_look(look_name: &str, opt: &Options, doc: &Doc, master: &Path, ext: &str) -> Result<(Option<Doc>, look::Match), String> {
    let Some(row) = look::row(look_name) else {
        let known: Vec<&str> = look::LOOKS.iter().map(|r| r.look).collect();
        let rep = look::Match { look: look_name.to_owned(), notes: vec![format!("no table for this look (looks with one: {}): every map kept, nothing merged", known.join(", "))], ..look::Match::default() };
        return Ok((None, rep));
    };
    let palette = if opt.snap_palette { look::palette(look_name) } else { None };
    let base = if ext == "gltf" { Doc::read_file(master)? } else { doc.clone() };
    let (matched, mut rep) = look::apply(&base, row, palette.as_deref());
    if opt.snap_palette && palette.is_none() {
        rep.notes.push("--snap-palette: this look has no palette, no colour moved".into());
    }
    Ok((rep.changed().then_some(matched), rep))
}

/// The general path: clean once, then per tier a LOD chain, textures, metadata.
fn convert_general(cx: &Ctx, opt: &Options, src: &Path, doc: &Doc, ext: &str) -> Result<Converted, String> {
    let mut notes = Vec::new();
    let master = cx.out.join(format!("{}.master.glb", cx.id));
    if ext == "gltf" {
        cx.tools.gltf_transform(&["copy", &s(src), &s(&master)], "gltf-transform copy (pack the .gltf master)")?;
    } else {
        copy(src, &master)?;
    }
    let inputs: InputList = vec![("input".into(), src.to_path_buf())];

    // Look matching (S3-12): the input cut down to what the look shows. The
    // master stays the original, so the gate scores the matched tier files
    // against it.
    let (matched, look_rep) = match &opt.look {
        Some(l) => match_look(l, opt, doc, &master, ext)?,
        None => (None, look::Match::default()),
    };
    notes.extend(look_rep.notes.iter().map(|n| format!("--look {}: {n}", look_rep.look)));
    // A relief height map is named only by the recipe in the material's
    // extras, which the clean would prune: park it in a free slot for the
    // clean and the packing (always, with or without --look).
    let looked = matched.is_some();
    let mut matched = matched;
    if look::has_relief_height(&doc.json) {
        let mut d = match matched.take() {
            Some(m) => m,
            None if ext == "gltf" => Doc::read_file(&master)?,
            None => doc.clone(),
        };
        let (n, parked) = look::park_relief(&mut d)?;
        notes.extend(n);
        if parked > 0 {
            notes.push(format!("relief: {parked} height map(s) kept through the clean (parked in a free material slot, put back as extras.evani.relief.height.index)"));
        }
        if parked > 0 || looked {
            matched = Some(d);
        }
    }
    // Sockets (`extras.evani.sockets`, {p, q}) become anchor nodes that
    // carry their rotation, so the engine's anchors hold it with no runtime code.
    if sockets::has(&doc.json) {
        let mut d = match matched.take() {
            Some(m) => m,
            None if ext == "gltf" => Doc::read_file(&master)?,
            None => doc.clone(),
        };
        notes.extend(sockets::apply(&mut d)?);
        matched = Some(d);
    }
    // Layered materials: the KHR sheen / clear coat / anisotropy /
    // iridescence factors become `extras.evani_layers`, which the clean keeps.
    if layers::has(&doc.json) {
        let mut d = match matched.take() {
            Some(m) => m,
            None if ext == "gltf" => Doc::read_file(&master)?,
            None => doc.clone(),
        };
        notes.extend(layers::apply(&mut d));
        matched = Some(d);
    }
    // Nature kinds (`extras.evani_kind` on a material, evani-material-nature)
    // ride in the root `extras.evani.nature` map too, keyed by material name,
    // so every tier file carries them whatever the packing does to materials.
    if let Some(kinds) = nature_kinds(&doc.json) {
        let mut d = match matched.take() {
            Some(m) => m,
            None if ext == "gltf" => Doc::read_file(&master)?,
            None => doc.clone(),
        };
        set_nature_kinds(&mut d.json, kinds, &cx.id);
        matched = Some(d);
    }
    let doc = matched.as_ref().unwrap_or(doc);
    let tangents = needs_tangents(&doc.json);

    // Animated: every animated node and joint gets a unique name first, so
    // the clips can find it in each tier file after gltfpack (S3-7).
    let anim_src = named_for_clips(doc, &cx.id, &mut notes)?;
    let clean_input = match (&anim_src, &matched) {
        (Some((named, _)), _) => {
            let p = cx.work.join("named.glb");
            write(&p, &named.to_glb()?)?;
            p
        }
        (None, Some(m)) => {
            let p = cx.work.join("matched.glb");
            write(&p, &m.to_glb()?)?;
            p
        }
        (None, None) => master.clone(),
    };

    // Clean: dedup, prune, weld (and tangents for normal maps without them).
    let clean = clean_file(cx, &clean_input, tangents, Some(&mut notes))?;
    let clean_doc = Doc::read_file(&clean)?;
    let uses = image_uses_with_files(&clean_doc.json, &cx.work)?;
    let g = evani_gltf::Gltf::from_glb(&std::fs::read(&master).map_err(|e| e.to_string())?).map_err(|e| format!("{}: the engine's reader refuses the master: {}", master.display(), e.0))?;
    let (lo, hi) = report::model_box(&g);
    let extent = (0..3).map(|i| f64::from(hi[i] - lo[i])).fold(0.0, f64::max);
    let min_distance = opt.min_distance.or_else(|| doc.json.pointer("/extras/evani/min_distance_m").and_then(Value::as_f64));
    if min_distance.is_none() && !uses.is_empty() {
        notes.push("no closest camera distance (extras.evani.min_distance_m or --min-distance): textures sized as if the model fills the screen height".into());
    }
    let single = if anim_src.is_some() { Some("the model is animated") } else { merge::mergeable(&clean_doc).err() };
    if let Some(why) = single {
        notes.push(format!(
            "one level of detail per tier: {why} (skins, animations, cameras and extension data such as lights point at indices across the file, which the merge does not remap, and baked clips name each tier file's nodes once; LOD chains for skinned and animated models are not built yet)"
        ));
    }
    // gltfpack's input: without the glTF animations (the tier files carry the
    // clip block instead; the master keeps them).
    let pack_input = if anim_src.is_some() {
        let p = cx.work.join("clean.noanim.glb");
        write(&p, &without_animations(&clean_doc).to_glb()?)?;
        p
    } else {
        clean.clone()
    };

    let geometry = std::cell::RefCell::new(Geometry { cx, clean: &clean, pack_input: &pack_input, single: single.is_some(), lod_cache: BTreeMap::new() });
    // The gate's candidate for a setting: the tier's geometry made exactly as the tier file's.
    let build = |tier: &str, st: &gate::Setting, out: &Path| -> Result<(), String> {
        let mut lad = ladder(tier)?;
        apply_setting(&mut lad, st);
        let mut g = geometry.borrow_mut();
        let (rungs, _) = g.pack(tier, &lad, out, false)?;
        let mut d = Doc::read_file(out)?;
        look::unpark_relief(&mut d.json)?;
        // As the tier file's metadata, less the texture sets and clips (the candidate has neither).
        let mut evani = doc.json.pointer("/extras/evani").cloned().filter(Value::is_object).unwrap_or_else(|| json!({ "format": 1, "class": cx.id }));
        evani["tier"] = Value::from(tier);
        if let Some(l) = g.lods_json(&rungs) {
            evani["lods"] = l;
        }
        if let Some(o) = evani.as_object_mut() {
            if g.single {
                o.remove("lods");
            }
            o.remove("textures");
            o.remove("clips");
        }
        set_root_extras(&mut d.json, doc.json.get("extras"), evani);
        write(out, &d.to_glb()?)
    };
    // What the same model would cost without matching (exact numbers for the
    // report): the original cleaned and packed the same way.
    let base_cx = Ctx { tools: cx.tools.clone(), root: cx.root.clone(), out: cx.work.join("baseline").join("out"), work: cx.work.join("baseline"), id: cx.id.clone() };
    let base_input = match (looked, &anim_src) {
        (true, None) => Some(baseline_prepare(&base_cx, &master, &uses)?),
        (true, Some(_)) => {
            notes.push("--look: bytes and draw calls saved are not measured for an animated model (matching still ran)".into());
            None
        }
        _ => None,
    };
    let base_geometry = base_input.as_ref().map(|b| std::cell::RefCell::new(Geometry { cx: &base_cx, clean: &b.clean, pack_input: &b.clean, single: b.single, lod_cache: BTreeMap::new() }));
    let mut base_cache: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut look_tiers: Vec<Value> = Vec::new();
    let mut ktx_cache: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut tiers = Vec::new();
    let mut gate = if opt.gate { gate::start_checked(&cx.root, &cx.out, &cx.work, &cx.id, &master, &cx.tools, opt.gate_pairs.clone(), &mut notes)? } else { None };
    for tier in wanted_tiers(opt) {
        let screen = Screen::load(&cx.root, &tier)?;
        let mut lad = ladder(&tier)?;
        let mut size = texture::size_for(&screen, extent, min_distance);
        // The quality gate: the cheapest setting on the tier's ladder that still passes.
        let mut gate_note = None;
        if let Some(g) = gate.as_mut() {
            let knobs = gate::Knobs::for_tier(&lad, (!uses.is_empty()).then_some(size.size), true)?;
            if let Some(st) = g.tune(&tier, &gate::Source::General { build: &build, uses: &uses }, &knobs)? {
                apply_setting(&mut lad, &st);
                if let Some(t) = st.texture {
                    size.size = t.max(64).min(size.size);
                }
                gate_note = Some(format!("quality gate: {}", st.describe()));
            }
        }
        let packed = cx.work.join(format!("{tier}.packed.glb"));
        let (rungs, twin) = geometry.borrow_mut().pack(&tier, &lad, &packed, true)?;

        // Textures for this tier.
        let mut t_out = TierOut { tier: tier.clone(), file: format!("{}.{tier}.glb", cx.id), size: Some(size.clone()), rungs: rungs.clone(), twin, note: gate_note, ..TierOut::default() };
        let encoded = encode_images(cx, &tier, &uses, size.size, &mut ktx_cache, &mut t_out)?;

        // Point the images at the core files, and write the Evani metadata.
        let mut d = Doc::read_file(&packed)?;
        rewrite_images(&mut d.json, &uses, &encoded)?;
        look::unpark_relief(&mut d.json)?;
        let mut evani = doc.json.pointer("/extras/evani").cloned().filter(Value::is_object).unwrap_or_else(|| json!({ "format": 1, "class": cx.id }));
        evani["tier"] = Value::from(tier.clone());
        if let Some(l) = geometry.borrow().lods_json(&rungs) {
            evani["lods"] = l;
        } else if let Some(o) = evani.as_object_mut() {
            // One level: an inherited `lods` list would name nodes this file does not have.
            o.remove("lods");
        }
        evani["textures"] = texture_sets(&d.json, &uses, &encoded);
        if let Some((_, src)) = &anim_src {
            t_out.clips = Some(attach_clips(&mut d, src, opt.clip_error.unwrap_or(lad.clip_error_m), &mut evani, &tier)?);
        } else if let Some(o) = evani.as_object_mut() {
            // An inherited `clips` would name a buffer view this file does not have.
            o.remove("clips");
        }
        set_root_extras(&mut d.json, doc.json.get("extras"), evani);
        let file = cx.out.join(&t_out.file);
        write(&file, &d.to_glb()?)?;
        if let Some(g) = gate.as_mut() {
            g.verify(&tier, &file)?;
        }
        if let (Some(bg), Some(bi)) = (&base_geometry, &base_input) {
            look_tiers.push(baseline_tier(&base_cx, &mut bg.borrow_mut(), bi, &tier, &lad, size.size, &mut base_cache, &packed, &file)?);
        }
        t_out.files.insert(0, t_out.file.clone());
        tiers.push(t_out);
    }
    let gate = gate.map(gate::Gate::finish).transpose()?;
    let look_json = opt.look.is_some().then(|| {
        let mut j = look_rep.to_json();
        j["tiers"] = Value::Array(look_tiers);
        j
    });
    Ok((tiers, inputs, rel(&cx.out, &master), notes, gate, look_json))
}

/// The original, cleaned like the matched file, for the report's "without
/// matching" numbers (S3-12).
struct Baseline {
    clean: PathBuf,
    single: bool,
    /// The images matching dropped (by content), as the original's clean file names them.
    dropped: Vec<(ImageUse, PathBuf)>,
}

fn baseline_prepare(base: &Ctx, master: &Path, kept: &[(ImageUse, PathBuf)]) -> Result<Baseline, String> {
    mkdir(&base.work)?;
    mkdir(&base.out.join("textures"))?;
    write(&base.work.join("images.mjs"), IMAGES_MJS.as_bytes())?;
    let mut original = Doc::read_file(master)?;
    let tangents = needs_tangents(&original.json);
    // The same relief parking as the matched run, so both clean alike.
    let input = if look::has_relief_height(&original.json) && look::park_relief(&mut original)?.1 > 0 {
        let p = base.work.join("parked.glb");
        write(&p, &original.to_glb()?)?;
        p
    } else {
        master.to_path_buf()
    };
    let clean = clean_file(base, &input, tangents, None)?;
    let clean_doc = Doc::read_file(&clean)?;
    let hash = |p: &Path| std::fs::read(p).map(|b| report::sha256_hex(&b)).map_err(|e| format!("{}: {e}", p.display()));
    let kept_hashes = kept.iter().map(|(_, p)| hash(p)).collect::<Result<Vec<_>, _>>()?;
    let mut dropped = Vec::new();
    for (u, p) in image_uses_with_files(&clean_doc.json, &base.work)? {
        if !kept_hashes.contains(&hash(&p)?) {
            dropped.push((u, p));
        }
    }
    Ok(Baseline { clean, single: merge::mergeable(&clean_doc).is_err(), dropped })
}

/// One tier's "without matching" numbers: the original packed with the same
/// ladder setting, and the dropped images encoded at the tier's size, set
/// against the matched tier file `now` (packed as `packed`).
#[allow(clippy::too_many_arguments)]
fn baseline_tier(base: &Ctx, geo: &mut Geometry<'_>, b: &Baseline, tier: &str, lad: &TierLadder, size: u32, cache: &mut BTreeMap<String, PathBuf>, packed: &Path, now: &Path) -> Result<Value, String> {
    let before_glb = base.work.join(format!("{tier}.packed.glb"));
    let (rungs, _) = geo.pack(tier, lad, &before_glb, false)?;
    // Its size as packed (the matched `packed` file is measured the same way),
    // then with the level list so the report counts level 0 only.
    let before_bytes = std::fs::metadata(&before_glb).map(|m| m.len()).map_err(|e| format!("{}: {e}", before_glb.display()))?;
    let mut counted = Doc::read_file(&before_glb)?;
    let mut evani = json!({ "format": 1, "class": base.id, "tier": tier });
    if let Some(l) = geo.lods_json(&rungs) {
        evani["lods"] = l;
    }
    set_root_extras(&mut counted.json, None, evani);
    write(&before_glb, &counted.to_glb()?)?;
    let draws = |p: &Path| -> Result<u64, String> {
        let g = evani_gltf::Gltf::from_glb(&std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: the engine's reader refuses it: {}", p.display(), e.0))?;
        Ok(report::levels(&g).first().map_or(0, |l| l.2))
    };
    let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).map_err(|e| format!("{}: {e}", p.display()));
    // The dropped images, as they would have shipped at this tier.
    let mut t = TierOut::default();
    encode_images(base, tier, &b.dropped, size, cache, &mut t)?;
    let (mut core, mut astc, mut bc7, mut astc_gpu, mut bc7_gpu) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for f in &t.files {
        let p = base.out.join(f);
        let n = len(&p)?;
        match report::kind_of(f).1 {
            "astc" => {
                astc += n;
                astc_gpu += texture::ktx2_level_bytes(&std::fs::read(&p).map_err(|e| e.to_string())?);
            }
            "bc7" => {
                bc7 += n;
                bc7_gpu += texture::ktx2_level_bytes(&std::fs::read(&p).map_err(|e| e.to_string())?);
            }
            _ => core += n,
        }
    }
    let glb_saved = before_bytes as i64 - len(packed)? as i64;
    Ok(json!({
        "tier": tier,
        "draw_calls": { "without": draws(&before_glb)?, "with": draws(now)? },
        "geometry_file_bytes": { "without": before_bytes, "with": len(packed)?, "saved": glb_saved },
        "maps_not_shipped": { "images": b.dropped.iter().map(|(u, _)| u.name.clone()).collect::<Vec<_>>(), "core_bytes": core, "astc_bytes": astc, "bc7_bytes": bc7, "astc_gpu_bytes": astc_gpu, "bc7_gpu_bytes": bc7_gpu },
        "download_saved": { "astc_device": glb_saved + astc as i64, "bc7_device": glb_saved + bc7 as i64, "all_files": glb_saved + (core + astc + bc7) as i64 },
    }))
}

/// Each image the clean file uses, with the file glTF-Transform wrote it to.
fn image_uses_with_files(j: &Value, work: &Path) -> Result<Vec<(ImageUse, PathBuf)>, String> {
    let images = j.get("images").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for u in texture::image_uses(j) {
        let uri = images.get(u.image).and_then(|i| i.get("uri")).and_then(Value::as_str).ok_or_else(|| format!("clean.gltf: image {} has no file", u.image))?;
        out.push((u, work.join(glb::uri_path(uri))));
    }
    Ok(out)
}

/// One image's files for one tier.
#[derive(Clone, Debug)]
struct Encoded {
    /// The source file in the work folder (to match gltfpack's URIs).
    src: PathBuf,
    core: String,
    mime: &'static str,
    sets: Vec<(&'static str, String)>,
}

/// Resize (sharp) and encode (ktx) every image for one tier.
fn encode_images(cx: &Ctx, tier: &str, uses: &[(ImageUse, PathBuf)], max: u32, cache: &mut BTreeMap<String, PathBuf>, t_out: &mut TierOut) -> Result<Vec<Encoded>, String> {
    if uses.is_empty() {
        return Ok(Vec::new());
    }
    let mut jobs = Vec::new();
    for (u, src) in uses {
        let core = cx.out.join("textures").join(format!("{}.{tier}.{}", u.name, u.core_ext()));
        let png = if u.core_ext() == "png" { core.clone() } else { cx.work.join("png").join(format!("{}.{tier}.png", u.name)) };
        mkdir(&cx.work.join("png"))?;
        jobs.push(json!({ "src": s(src), "max": max, "core": s(&core), "core_format": u.core_ext(), "png": s(&png) }));
    }
    let job_file = cx.work.join(format!("images.{tier}.json"));
    write(&job_file, Value::Array(jobs.clone()).to_string().as_bytes())?;
    cx.tools.node_script(&cx.work.join("images.mjs"), &[&s(&job_file)], &format!("resize textures ({tier}, sharp)"))?;
    let done: Value = serde_json::from_slice(&std::fs::read(format!("{}.out.json", s(&job_file))).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (((u, src), job), res) in uses.iter().zip(&jobs).zip(done.as_array().into_iter().flatten()) {
        let (w, h) = (res["w"].as_u64().unwrap_or(0) as u32, res["h"].as_u64().unwrap_or(0) as u32);
        let png = PathBuf::from(job["png"].as_str().unwrap_or_default());
        let core = PathBuf::from(job["core"].as_str().unwrap_or_default());
        let mut sets = Vec::new();
        for set in ["astc", "bc7"] {
            let key = format!("{}.{w}x{h}.{}.{set}", u.name, if u.role.srgb() { "srgb" } else { "linear" });
            let cached = match cache.get(&key) {
                Some(p) => p.clone(),
                None => {
                    let p = cx.work.join("ktx").join(format!("{key}.ktx2"));
                    mkdir(&cx.work.join("ktx"))?;
                    encode_ktx(&cx.tools, set, u.role.srgb(), &png, &p)?;
                    cache.insert(key, p.clone());
                    p
                }
            };
            let name = format!("textures/{}.{tier}.{set}.ktx2", u.name);
            copy(&cached, &cx.out.join(&name))?;
            t_out.files.push(name.clone());
            sets.push((set, name));
        }
        let core_rel = rel(&cx.out, &core);
        t_out.files.push(core_rel.clone());
        t_out.images.push((u.name.clone(), w, h, core_rel.clone()));
        out.push(Encoded { src: src.clone(), core: core_rel, mime: if u.core_ext() == "png" { "image/png" } else { "image/jpeg" }, sets });
    }
    Ok(out)
}

pub(crate) fn encode_ktx(t: &Tools, set: &str, srgb: bool, png: &Path, out: &Path) -> Result<(), String> {
    if set == "astc" {
        let a = texture::ktx_create_args("astc", srgb, &s(png), &s(out));
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        t.ktx(&a, &format!("ktx create ASTC {}", png.display()))?;
    } else {
        let mid = out.with_extension("uastc.ktx2");
        let a = texture::ktx_create_args("uastc", srgb, &s(png), &s(&mid));
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        t.ktx(&a, &format!("ktx create UASTC {}", png.display()))?;
        t.ktx(&["transcode", "--target", "bc7", &s(&mid), &s(out)], &format!("ktx transcode BC7 {}", png.display()))?;
        let _ = std::fs::remove_file(mid);
    }
    Ok(())
}

fn file_name(p: &str) -> String {
    let p = glb::uri_path(p).replace('\\', "/");
    p.rsplit('/').next().unwrap_or(&p).to_owned()
}

/// Point every image at its core file (`textures/<name>.<tier>.<png|jpg>`).
fn rewrite_images(j: &mut Value, uses: &[(ImageUse, PathBuf)], enc: &[Encoded]) -> Result<(), String> {
    let Some(Value::Array(images)) = j.get_mut("images") else { return Ok(()) };
    for im in images.iter_mut() {
        let uri = im.get("uri").and_then(Value::as_str).ok_or("a packed image has no URI (gltfpack -tr keeps them)")?;
        let name = file_name(uri);
        let k = enc.iter().position(|e| e.src.file_name().and_then(|n| n.to_str()) == Some(name.as_str())).ok_or_else(|| format!("packed image {uri} is not one of the model's images"))?;
        *im = json!({ "name": uses[k].0.name, "uri": enc[k].core, "mimeType": enc[k].mime });
    }
    Ok(())
}

/// `extras.evani.textures`: every texture with its per-device files.
fn texture_sets(j: &Value, uses: &[(ImageUse, PathBuf)], enc: &[Encoded]) -> Value {
    let images = j.get("images").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for (t, tex) in j.get("textures").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let Some(src) = tex.get("source").and_then(Value::as_u64) else { continue };
        let Some(uri) = images.get(src as usize).and_then(|i| i.get("uri")).and_then(Value::as_str) else { continue };
        let Some(k) = enc.iter().position(|e| e.core == uri) else { continue };
        let sets: Map<String, Value> = enc[k].sets.iter().map(|(f, u)| ((*f).to_owned(), Value::from(u.clone()))).collect();
        out.push(json!({ "texture": t, "name": uses[k].0.name, "sets": sets }));
    }
    Value::Array(out)
}

/// Each named material's `extras.evani_kind`, as a name-to-kind map (`None`
/// when no material has one).
fn nature_kinds(j: &Value) -> Option<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for m in j.get("materials").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(name), Some(kind)) = (m.get("name").and_then(Value::as_str), m.pointer("/extras/evani_kind").and_then(Value::as_str)) {
            out.insert(name.to_owned(), Value::from(kind));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Merge `kinds` into the root `extras.evani.nature` (an entry already there wins).
fn set_nature_kinds(j: &mut Value, kinds: serde_json::Map<String, Value>, id: &str) {
    if !j.get("extras").is_some_and(Value::is_object) {
        j["extras"] = json!({});
    }
    if !j["extras"].get("evani").is_some_and(Value::is_object) {
        j["extras"]["evani"] = json!({ "format": 1, "class": id });
    }
    if !j["extras"]["evani"].get("nature").is_some_and(Value::is_object) {
        j["extras"]["evani"]["nature"] = json!({});
    }
    if let Some(map) = j["extras"]["evani"]["nature"].as_object_mut() {
        for (k, v) in kinds {
            map.entry(k).or_insert(v);
        }
    }
}

/// gltfpack drops the root `extras`: put the input's back, with `evani` set.
fn set_root_extras(j: &mut Value, input_extras: Option<&Value>, evani: Value) {
    let mut ex = input_extras.cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
    ex["evani"] = evani;
    j["extras"] = ex;
}

fn missing_normal_map(stem: &str, tier: &str, file: &str, dir: &Path) -> String {
    format!(
        "{stem}.{tier}.glb lists the normal map {file} in extras.evani.normal_maps, but it is not beside it in {}\nfix: export the Evani.Model version again with its maps, or copy {file} next to the tier files",
        dir.display()
    )
}

/// Every normal map the tier files list in `extras.evani.normal_maps` sits
/// beside them. Run before the quality gate, so a missing picture fails at
/// once instead of after minutes of renders.
fn check_normal_maps(dir: &Path, stem: &str, tiers: &[String]) -> Result<(), String> {
    for tier in tiers {
        let doc = Doc::read_file(&dir.join(format!("{stem}.{tier}.glb")))?;
        for m in doc.json.pointer("/extras/evani/normal_maps/maps").and_then(Value::as_array).into_iter().flatten() {
            let Some(file) = m.get("file").and_then(Value::as_str) else { continue };
            if !dir.join(glb::uri_path(file)).is_file() {
                return Err(missing_normal_map(stem, tier, file, dir));
            }
        }
    }
    Ok(())
}

/// An animated input (S3-7): a copy whose animated nodes and skin joints all
/// have unique names (gltfpack keeps named nodes with `-kn`, so each tier
/// file's node is found again by name), and that copy opened by the engine's
/// reader to bake from. `None` for a model without animations.
fn named_for_clips(doc: &Doc, id: &str, notes: &mut Vec<String>) -> Result<Option<(Doc, evani_gltf::Gltf)>, String> {
    if doc.arr("animations").is_empty() {
        return Ok(None);
    }
    let mut wanted = std::collections::BTreeSet::new();
    for a in doc.arr("animations") {
        for c in a.get("channels").and_then(Value::as_array).into_iter().flatten() {
            if let Some(n) = c.pointer("/target/node").and_then(Value::as_u64) {
                wanted.insert(n as usize);
            }
        }
    }
    for sk in doc.arr("skins") {
        for j in sk.get("joints").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64) {
            wanted.insert(j as usize);
        }
    }
    let mut d = doc.clone();
    let names: Vec<String> = doc.arr("nodes").iter().map(|n| n.get("name").and_then(Value::as_str).unwrap_or("").to_owned()).collect();
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for n in &names {
        *count.entry(n.clone()).or_insert(0) += 1;
    }
    let mut renamed = Vec::new();
    for &n in &wanted {
        let Some(old) = names.get(n) else { continue };
        if !old.is_empty() && count.get(old) == Some(&1) {
            continue;
        }
        let mut k = 0;
        let new = loop {
            let c = if k == 0 { format!("{id}_node{n}") } else { format!("{id}_node{n}_{k}") };
            if !count.contains_key(&c) {
                break c;
            }
            k += 1;
        };
        count.insert(new.clone(), 1);
        if let Some(node) = d.json.pointer_mut(&format!("/nodes/{n}")) {
            node["name"] = Value::from(new.clone());
        }
        renamed.push(if old.is_empty() { format!("node {n} -> `{new}`") } else { format!("`{old}` (node {n}) -> `{new}`") });
    }
    if !renamed.is_empty() {
        let note = format!("animated nodes and joints renamed so the clips find them in each tier file (names must be unique): {}", renamed.join(", "));
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    let g = evani_gltf::Gltf::from_glb(&d.to_glb()?).map_err(|e| format!("the engine's reader refuses the animated master: {}", e.0))?;
    Ok(Some((d, g)))
}

/// The same document without its glTF animations.
fn without_animations(d: &Doc) -> Doc {
    let mut out = d.clone();
    if let Some(o) = out.json.as_object_mut() {
        o.remove("animations");
    }
    out
}

/// Bake the clips for one tier file and put the block in its binary chunk
/// as a new buffer view, listed in `extras.evani.clips`.
fn attach_clips(d: &mut Doc, src: &evani_gltf::Gltf, bound: f64, evani: &mut Value, tier: &str) -> Result<clips::Baked, String> {
    let tg = evani_gltf::Gltf::from_glb(&d.to_glb()?).map_err(|e| format!("{tier}: the engine's reader refuses the packed file: {}", e.0))?;
    let find = |n: usize| src.nodes.get(n).filter(|x| !x.name.is_empty()).and_then(|x| tg.node_by_name(&x.name));
    // The clips leave untracked channels at the tier file's rest pose, so it
    // must be the master's.
    let mut animated = std::collections::BTreeSet::new();
    for a in &src.animations {
        animated.extend(a.channels.iter().filter_map(|c| c.node));
    }
    for &n in &animated {
        let (Some(t), Some(m)) = (find(n), src.nodes.get(n)) else { continue };
        let x = &tg.nodes[t];
        let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(p, q)| (p - q).abs() <= 1e-5);
        let dot: f32 = (0..4).map(|k| x.rotation[k] * m.rotation[k]).sum();
        if x.matrix.is_some() || !close(&x.translation, &m.translation) || !close(&x.scale, &m.scale) || dot.abs() < 1.0 - 1e-6 {
            return Err(format!("{tier}: gltfpack changed the rest pose of the animated node `{}` (translation {:?} -> {:?}, rotation {:?} -> {:?}, scale {:?} -> {:?}); the clips need the master's", m.name, m.translation, x.translation, m.rotation, x.rotation, m.scale, x.scale));
        }
    }
    let baked = clips::bake(src, bound, &find).map_err(|e| format!("{tier}: {e}"))?;
    if d.arr("buffers").first().is_some_and(|b| b.get("uri").is_some() || b.pointer("/extensions/EXT_meshopt_compression/fallback").is_some()) {
        return Err(format!("{tier}: the packed file's buffer 0 is not its binary chunk; the clip block has nowhere to go"));
    }
    if d.arr("buffers").is_empty() {
        d.json["buffers"] = json!([{ "byteLength": 0 }]);
    }
    while !d.bin.len().is_multiple_of(4) {
        d.bin.push(0);
    }
    let offset = d.bin.len();
    d.bin.extend_from_slice(&baked.block);
    let views = d.json.as_object_mut().ok_or("the packed file's JSON is not an object")?.entry("bufferViews").or_insert_with(|| json!([]));
    let list = views.as_array_mut().ok_or("bufferViews is not a list")?;
    list.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": baked.block.len() }));
    let view = list.len() - 1;
    evani["clips"] = json!({
        "format": evani_guide::clipblock::FORMAT,
        "view": view,
        "rate": evani_guide::clipblock::RATE,
        "list": baked.clips.iter().map(|c| json!({ "name": c.name, "frames": c.frames })).collect::<Vec<_>>(),
    });
    Ok(baked)
}

/// The tiers whose `<stem>.<tier>.glb` sit in `dir`. None is an error that
/// names the files expected: an Evani.Model file renamed without its tier
/// (`kart.glb` instead of `kart.low.glb`) has no tier files beside it.
pub fn evani_model_tiers(input: &Path, dir: &Path, stem: &str) -> Result<Vec<&'static str>, String> {
    let present: Vec<&'static str> = ALL_TIERS.iter().copied().filter(|t| dir.join(format!("{stem}.{t}.glb")).is_file()).collect();
    if present.is_empty() {
        return Err(format!(
            "{}: an Evani.Model file (extras.evani.tier is set), but no Evani.Model tier file ({stem}.<tier>.glb, tiers {}) is in {}\nfix: keep Evani.Model's own names (<name>.<tier>.glb, for example {stem}.low.glb), or export the model again from Evani.Model",
            input.display(),
            ALL_TIERS.join(", "),
            dir.display()
        ));
    }
    Ok(present)
}

/// Evani.Model's per-tier files: compress and gate, never re-simplify.
fn convert_evani_model(cx: &Ctx, opt: &Options, input: &Path, stem: &str) -> Result<Converted, String> {
    let dir = input.parent().unwrap_or(Path::new("."));
    let present = evani_model_tiers(input, dir, stem)?;
    let tiers: Vec<String> = if opt.tiers.is_empty() { present.iter().map(|t| (*t).to_owned()).collect() } else { opt.tiers.clone() };
    for t in &tiers {
        if !present.contains(&t.as_str()) {
            return Err(format!("{}: no {stem}.{t}.glb beside it (Evani.Model's files present: {})", input.display(), present.join(", ")));
        }
    }
    // Before anything slow (the gate's renders): every listed normal map is there.
    check_normal_maps(dir, stem, &tiers)?;
    let mut notes = vec![format!("Evani.Model files ({}): already simplified per tier, so only compressed, encoded and checked (never re-simplified)", glb::generator(&Doc::read_file(input)?.json))];
    let top = ["ultra", "high", "vr", "medium", "low", "lite"].iter().find(|t| present.contains(t)).copied().unwrap_or("low");
    let master = cx.out.join(format!("{}.master.glb", cx.id));
    copy(&dir.join(format!("{stem}.{top}.glb")), &master)?;
    notes.push(format!("master: Evani.Model's {top} file, kept untouched"));
    let mut inputs: InputList = Vec::new();
    let mut out = Vec::new();
    let mut ktx_cache = BTreeMap::new();
    let mut gate = if opt.gate { gate::start_checked(&cx.root, &cx.out, &cx.work, &cx.id, &master, &cx.tools, opt.gate_pairs.clone(), &mut notes)? } else { None };
    for tier in &tiers {
        let src = dir.join(format!("{stem}.{tier}.glb"));
        inputs.push((format!("{tier} input"), src.clone()));
        let doc = Doc::read_file(&src)?;
        if !doc.arr("images").is_empty() {
            return Err(format!("{}: Evani.Model files carry no glTF images (format 1); this one has {}", src.display(), doc.arr("images").len()));
        }
        let mut lad = ladder(tier)?;
        let mut note = "Evani.Model tier: not re-simplified".to_owned();
        if let Some(g) = gate.as_mut() {
            let knobs = gate::Knobs::for_tier(&lad, None, false)?;
            if let Some(st) = g.tune(tier, &gate::Source::EvaniModel { input: &src }, &knobs)? {
                (lad.vp, lad.vn, lad.vt) = (st.vp, st.vn, st.vt);
                note += &format!("; quality gate: vertex bits vp {} vn {} vt {}", st.vp, st.vn, st.vt);
            }
        }
        let packed = cx.work.join(format!("{tier}.packed.glb"));
        // Animated (S3-7): gltfpack gets the file without its animations; the clips are baked from it.
        // Sockets: Evani.Model writes their rotation on the anchor nodes
        // already; the pass checks that and fills in any that lack it.
        let socketed = if sockets::has(&doc.json) {
            let mut d = doc.clone();
            for n in sockets::apply(&mut d)? {
                if !notes.contains(&n) {
                    notes.push(n);
                }
            }
            Some(d)
        } else {
            None
        };
        let anim = named_for_clips(socketed.as_ref().unwrap_or(&doc), &cx.id, &mut notes)?;
        let pack_src = match (&anim, &socketed) {
            (Some((named, _)), _) => {
                let p = cx.work.join(format!("{tier}.noanim.glb"));
                write(&p, &without_animations(named).to_glb()?)?;
                p
            }
            (None, Some(d)) => {
                let p = cx.work.join(format!("{tier}.sockets.glb"));
                write(&p, &d.to_glb()?)?;
                p
            }
            (None, None) => src.clone(),
        };
        let mut args: Vec<String> = vec!["-i".into(), s(&pack_src)];
        args.extend(["-kn", "-ke", "-kv"].map(String::from));
        args.extend(quant_args(&lad));
        let twin = pack(cx, &args, &packed, &format!("gltfpack compress ({tier})"))?;
        let mut t_out = TierOut { tier: tier.clone(), file: format!("{}.{tier}.glb", cx.id), note: Some(note), twin: Some(twin), ..TierOut::default() };

        // Normal maps: the PNG stays beside the file; the device sets are added.
        let mut evani = doc.json.pointer("/extras/evani").cloned().unwrap_or_else(|| json!({}));
        if let Some(Value::Array(maps)) = evani.pointer_mut("/normal_maps/maps") {
            for m in maps.iter_mut() {
                let Some(file) = m.get("file").and_then(Value::as_str).map(str::to_owned) else { continue };
                let png = dir.join(glb::uri_path(&file));
                if !png.is_file() {
                    return Err(missing_normal_map(stem, tier, &file, dir));
                }
                inputs.push((format!("{tier} normal map"), png.clone()));
                copy(&png, &cx.out.join(&file))?;
                t_out.files.push(file.clone());
                let base = file.trim_end_matches(".png");
                let mut sets = Map::new();
                for set in ["astc", "bc7"] {
                    let name = format!("textures/{base}.{set}.ktx2");
                    let key = format!("{file}.{set}");
                    let p = cx.work.join("ktx").join(format!("{key}.ktx2"));
                    if let std::collections::btree_map::Entry::Vacant(slot) = ktx_cache.entry(key) {
                        mkdir(&cx.work.join("ktx"))?;
                        encode_ktx(&cx.tools, set, false, &png, &p)?;
                        slot.insert(p.clone());
                    }
                    copy(&p, &cx.out.join(&name))?;
                    t_out.files.push(name.clone());
                    sets.insert(set.into(), Value::from(name));
                }
                m["sets"] = Value::Object(sets);
                let size = m.get("size").and_then(Value::as_u64).unwrap_or(0) as u32;
                t_out.images.push((format!("{base} (normal map)"), size, size, file));
            }
        }
        let mut d = Doc::read_file(&packed)?;
        evani["tier"] = Value::from(tier.clone());
        if let Some((_, g)) = &anim {
            t_out.clips = Some(attach_clips(&mut d, g, opt.clip_error.unwrap_or(lad.clip_error_m), &mut evani, tier)?);
        } else if let Some(o) = evani.as_object_mut() {
            o.remove("clips");
        }
        set_root_extras(&mut d.json, doc.json.get("extras"), evani);
        write(&cx.out.join(&t_out.file), &d.to_glb()?)?;
        if let Some(g) = gate.as_mut() {
            g.verify(tier, &cx.out.join(&t_out.file))?;
        }
        t_out.files.insert(0, t_out.file.clone());
        out.push(t_out);
    }
    let gate = gate.map(gate::Gate::finish).transpose()?;
    // Evani.Model files carry no materials (colours come from the vertices).
    let look_json = opt.look.as_ref().map(|l| json!({ "look": l, "has_table": look::row(l).is_some(), "notes": ["an Evani.Model file carries no glTF materials: nothing to match"], "tiers": [] }));
    Ok((out, inputs, rel(&cx.out, &master), notes, gate, look_json))
}

/// Blender: `EVANI_BLENDER`, the pinned install in .evani/bin, or the
/// earlier hand install in .evani/tools of the main checkout.
pub fn blender(root: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EVANI_BLENDER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Ok(p) = evani_guide::extools::locate(root, "blender") {
        return Some(p);
    }
    Some(root.join(".evani").join("tools").join("blender-5.2.2-windows-x64").join("blender.exe")).filter(|p| p.is_file())
}

fn export_blend(blender: &Path, input: &Path, out: &Path, tmp: &Path) -> Result<(), String> {
    let script = tmp.join("blend_export.py");
    write(&script, BLEND_PY.as_bytes())?;
    // `--disable-autoexec` before the file: a dropped .blend's own scripts
    // (drivers, registered text blocks) never run; only our export script does.
    tools::run(
        std::process::Command::new(blender)
            .args(["-b", "--factory-startup", "--disable-autoexec", "-t", "2"])
            .arg(input)
            .arg("--python")
            .arg(&script)
            .arg("--")
            .arg(out),
        "Blender glTF export",
    )?;
    if !out.is_file() {
        return Err(format!("Blender ran but wrote no {}", out.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nature_kinds_ride_in_the_root_extras() {
        let mut j = json!({ "asset": { "version": "2.0" }, "materials": [
            { "name": "pond", "extras": { "evani_kind": "water" } },
            { "name": "rock" },
            { "extras": { "evani_kind": "glass" } },
            { "name": "lamp", "extras": { "evani_kind": "lamp" } }
        ] });
        assert!(nature_kinds(&json!({ "materials": [{ "name": "rock" }] })).is_none());
        let kinds = nature_kinds(&j).unwrap();
        assert_eq!(kinds.len(), 2, "named and tagged only");
        j["extras"] = json!({ "author": "me", "evani": { "nature": { "lamp": "candle" } } });
        set_nature_kinds(&mut j, kinds, "m");
        assert_eq!(j["extras"]["evani"]["nature"], json!({ "pond": "water", "lamp": "candle" }), "an entry already there wins");
        assert_eq!(j["extras"]["author"], "me");
        let mut bare = json!({ "materials": [{ "name": "pond", "extras": { "evani_kind": "water" } }] });
        let k = nature_kinds(&bare).unwrap();
        set_nature_kinds(&mut bare, k, "m");
        assert_eq!(bare["extras"]["evani"], json!({ "format": 1, "class": "m", "nature": { "pond": "water" } }));
    }

    #[test]
    fn every_tier_has_a_valid_ladder() {
        for t in ALL_TIERS {
            let l = ladder(t).unwrap();
            assert!(l.lods.windows(2).all(|w| w[0].ratio > w[1].ratio && w[0].min_screen > w[1].min_screen), "{t}: levels run from most detailed down");
            assert_eq!(l.lods.last().map(|r| r.min_screen), Some(0.0), "{t}: the last level draws at any size");
        }
        assert_eq!(ladder("ultra").unwrap().lods[0].ratio, 1.0, "ultra level 0 is the master's geometry");
    }

    #[test]
    fn renamed_evani_model_file_names_the_tier_files() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/models");
        // kart.low.glb sits there: its stem `kart` finds the low tier.
        assert_eq!(evani_model_tiers(&fixtures.join("kart.low.glb"), &fixtures, "kart").unwrap(), ["low"]);
        // The same file renamed `kart_fixture.glb` has no tier files beside it.
        let e = evani_model_tiers(&fixtures.join("kart_fixture.glb"), &fixtures, "kart_fixture").unwrap_err();
        assert!(e.contains("kart_fixture.<tier>.glb") && e.contains("\nfix: "), "{e}");
    }

    #[test]
    fn options_parse_and_ids() {
        let o = Options::parse(Path::new("."), &["m.glb".into(), "--id".into(), "kart_01".into(), "--min-distance".into(), "3".into(), "--tiers".into(), "low,ultra".into()]).unwrap();
        assert_eq!((o.id.as_deref(), o.min_distance, o.tiers.len()), (Some("kart_01"), Some(3.0), 2));
        assert!(Options::parse(Path::new("."), &["m.glb".into(), "--id".into(), "Kart".into()]).is_err());
        assert!(Options::parse(Path::new("."), &["m.glb".into(), "--tiers".into(), "huge".into()]).is_err());
        assert_eq!(id_from("Frontier Marshal.master"), "frontier_marshal_master");
        assert_eq!(evani_model_stem("character.low"), "character");
    }

    #[test]
    fn missing_normal_map_is_refused_before_the_gate() {
        let dir = std::env::temp_dir().join(format!("evani-convert-nm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let doc = Doc { json: json!({ "asset": { "version": "2.0" }, "extras": { "evani": { "normal_maps": { "maps": [{ "file": "m.low.body.normal.png" }] } } } }), bin: Vec::new() };
        std::fs::write(dir.join("m.low.glb"), doc.to_glb().unwrap()).unwrap();
        let tiers = vec!["low".to_owned()];
        let e = check_normal_maps(&dir, "m", &tiers).unwrap_err();
        assert!(e.contains("m.low.body.normal.png") && e.contains("fix:"), "{e}");
        std::fs::write(dir.join("m.low.body.normal.png"), b"png").unwrap();
        assert!(check_normal_maps(&dir, "m", &tiers).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
