//! `--zfight`'s capture cache: the expensive part of the z-fight scan (the
//! native replay and the depth-maths test over every recorded camera) is
//! saved once, and later runs with the same build, scene and camera set
//! reuse it. `--report-only` re-renders the report from it in seconds;
//! `--sample N` scans a fixed, seeded subset of N cameras (its own key).
//!
//! Where: `<target>/evani-geocheck-cache/<app>/<key>/` (the cargo target
//! dir: `CARGO_TARGET_DIR`, else `<workspace>/target`; never committed).
//! Each entry holds `capture.json.gz` (what the report needs: the static
//! world's instances and custom meshes to rebuild the scene, the cameras,
//! the untagged issues, the "what ran" text and the run's notes), the
//! debug images' raw live captures in `shots/` (one per camera pose), and
//! `manifest.json` (written last; what went into the key, file by file, so
//! a stale `--report-only` can say what changed).
//!
//! The key is a SHA-256 over four parts; any change gives a new key:
//! - build: every file of the workspace crates the scan compiles in or
//!   captures with (evani-geocheck and its workspace dependencies, the
//!   app's crate and its dependencies, the live bridge, `tools/live-check`,
//!   files they `include_str!`/`include_bytes!` from elsewhere, the
//!   workspace `Cargo.toml` and `Cargo.lock`), the toolchain (`rustc -vV`)
//!   and the build profile. Left out: Markdown, `geocheck.toml` (its
//!   thresholds are in the scene part, its tags apply at report time) and
//!   [`REPORT_ONLY_FILES`], which only shape the report.
//! - scene: the app, the replay's length and sampling, the effective
//!   thresholds.
//! - cameras: which recorded cameras are scanned (all, or `--sample N` with
//!   its seed).
//! - settings: the depth setup, the viewport, the still-world switch, the
//!   tool's version and this cache's format.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use evani_core::rng::Pcg32;
use evani_geocheck::geom::Aabb;
use evani_geocheck::image::Camera;
use evani_geocheck::report::{Check, Issue, Severity};
use evani_geocheck::scene::{Config, Scene, default_label};
use evani_geocheck::shape::Shapes;
use evani_geocheck::zfight::Viewer;
use evani_render::mesh::{BUILTIN_MESH_COUNT, Mesh, MeshId};
use evani_render::world::MeshInstance;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// The cache's own format; bump it when [`Capture`]'s encoding changes.
pub const FORMAT: u32 = 1;
/// `--sample`'s seed: the same N cameras every time.
pub const SAMPLE_SEED: u64 = 0x6765_6f63_6865_636b;
/// Entries kept per app (the newest; older ones are removed on save).
const KEEP: usize = 8;
/// Files that only shape the report (its words, tables and debug images):
/// editing them keeps the key, so `--report-only` shows the change.
pub const REPORT_ONLY_FILES: [&str; 2] = ["tools/evani-geocheck/src/cli/zfight.rs", "tools/evani-geocheck/src/cli/png.rs"];

pub fn sha_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The cache folder: `<target>/evani-geocheck-cache`.
pub fn cache_root(root: &Path) -> PathBuf {
    let target = match std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()) {
        Some(t) => {
            let t = PathBuf::from(t);
            if t.is_absolute() { t } else { root.join(t) }
        }
        None => root.join("target"),
    };
    target.join("evani-geocheck-cache")
}

/// One part of the key: its hash and, in words, what went in.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: &'static str,
    pub hash: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    /// 16 hex digits of the SHA-256 over the parts.
    pub key: String,
    /// build, scene, cameras, settings.
    pub parts: Vec<Part>,
    /// The build part's files (path from the workspace root, SHA-256).
    pub files: Vec<(String, String)>,
}

impl Key {
    /// The key for `app` from the workspace at `root`: the build part is
    /// read from disk here, the other parts are given in words.
    pub fn compute(root: &Path, app: &str, scene: &str, cameras: &str, settings: &str) -> Result<Key, String> {
        let files = build_files(root, app)?;
        let mut b = String::new();
        for (p, h) in &files {
            b.push_str(&format!("{p} {h}\n"));
        }
        let tool = toolchain(root);
        b.push_str(&tool);
        Ok(Key::from_parts(files, &tool, scene, cameras, settings, &b))
    }

    pub fn from_parts(files: Vec<(String, String)>, tool: &str, scene: &str, cameras: &str, settings: &str, build: &str) -> Key {
        let part = |name, text: &str, hashed: &str| Part {
            name,
            hash: sha_hex(hashed.as_bytes()),
            text: text.to_string(),
        };
        let parts = vec![
            part("build", &format!("{} files; {}", files.len(), tool.replace('\n', "; ")), build),
            part("scene", scene, scene),
            part("cameras", cameras, cameras),
            part("settings", settings, settings),
        ];
        let all: String = parts.iter().map(|p| format!("{}={}\n", p.name, p.hash)).collect();
        Key {
            key: sha_hex(all.as_bytes())[..16].to_string(),
            parts,
            files,
        }
    }

    fn manifest(&self) -> Value {
        json!({
            "format": FORMAT,
            "key": self.key,
            "parts": self.parts.iter().map(|p| json!({"name": p.name, "hash": p.hash, "text": p.text})).collect::<Vec<_>>(),
            "files": self.files.iter().map(|(p, h)| json!([p, h])).collect::<Vec<_>>(),
        })
    }
}

/// The toolchain and profile the scan was built with.
fn toolchain(root: &Path) -> String {
    let rustc = std::process::Command::new("rustc")
        .arg("-vV")
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(|| "rustc: unknown".to_string(), |o| String::from_utf8_lossy(&o.stdout).trim().replace('\r', ""));
    let flags: Vec<String> = ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()).map(|v| format!("{k}={v}")))
        .collect();
    format!(
        "{rustc}\nprofile: debug_assertions={} {}-{}{}{}",
        cfg!(debug_assertions),
        std::env::consts::ARCH,
        std::env::consts::OS,
        if flags.is_empty() { "" } else { " " },
        flags.join(" ")
    )
}

/// `a/b/../c` as `a/c`, without touching the disk.
fn normal(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string().replace('\\', "/")
}

fn read_toml(p: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("read {}: {e}", p.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
}

/// The workspace crates `dir` depends on (normal, build and target-specific
/// dependencies, optional ones too; not dev-dependencies).
fn crate_deps(root: &Path, dir: &Path, ws: &Value) -> Result<Vec<PathBuf>, String> {
    let v = read_toml(&dir.join("Cargo.toml"))?;
    let mut tables: Vec<&Value> = vec![&v["dependencies"], &v["build-dependencies"]];
    if let Some(t) = v["target"].as_object() {
        for spec in t.values() {
            tables.push(&spec["dependencies"]);
            tables.push(&spec["build-dependencies"]);
        }
    }
    let mut out = Vec::new();
    for t in tables {
        let Some(t) = t.as_object() else { continue };
        for (name, spec) in t {
            if let Some(p) = spec["path"].as_str() {
                out.push(normal(&dir.join(p)));
            } else if spec["workspace"] == true
                && let Some(p) = ws["workspace"]["dependencies"][name.as_str()]["path"].as_str()
            {
                out.push(normal(&root.join(p)));
            }
        }
    }
    Ok(out)
}

/// Skipped when walking a crate: build output, hidden folders, packages.
fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "dist" || name == "node_modules"
}

/// Files of a crate that don't change what the scan captures: Markdown, and
/// `geocheck.toml` (thresholds are in the scene part; tags apply at report
/// time).
fn skip_file(name: &str) -> bool {
    name.ends_with(".md") || name == "geocheck.toml"
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        if p.is_dir() {
            if !skip_dir(&name) {
                walk(&p, out)?;
            }
        } else if !skip_file(&name) {
            out.push(p);
        }
    }
    Ok(())
}

/// The literal paths a Rust file pulls in with `include_str!` /
/// `include_bytes!`, resolved from its folder.
fn includes(file: &Path, text: &str) -> Vec<PathBuf> {
    let base = file.parent().unwrap_or(Path::new(""));
    let mut out = Vec::new();
    for mac in ["include_str!(\"", "include_bytes!(\""] {
        let mut rest = text;
        while let Some(i) = rest.find(mac) {
            rest = &rest[i + mac.len()..];
            if let Some(end) = rest.find('"') {
                out.push(normal(&base.join(&rest[..end])));
            }
        }
    }
    out
}

/// Every file of the build part, with its SHA-256 (sorted by path).
pub fn build_files(root: &Path, app: &str) -> Result<Vec<(String, String)>, String> {
    let ws = read_toml(&root.join("Cargo.toml"))?;
    let mut crates: Vec<PathBuf> = Vec::new();
    let mut todo: Vec<PathBuf> = ["tools/evani-geocheck".to_string(), format!("apps/{app}"), "tools/evani-live-bridge".to_string()]
        .iter()
        .map(|d| normal(&root.join(d)))
        .filter(|d| d.join("Cargo.toml").is_file())
        .collect();
    while let Some(d) = todo.pop() {
        if crates.contains(&d) {
            continue;
        }
        todo.extend(crate_deps(root, &d, &ws)?);
        crates.push(d);
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for c in &crates {
        walk(c, &mut files)?;
    }
    let live_check = root.join("tools").join("live-check");
    if live_check.is_dir() {
        walk(&live_check, &mut files)?;
    }
    for f in ["Cargo.toml", "Cargo.lock", ".cargo/config.toml", ".cargo/config", "rust-toolchain.toml", "rust-toolchain"] {
        let p = root.join(f);
        if p.is_file() {
            files.push(p);
        }
    }
    let report_only: Vec<PathBuf> = REPORT_ONLY_FILES.iter().map(|f| normal(&root.join(f))).collect();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    let mut i = 0;
    while i < files.len() {
        let p = normal(&files[i]);
        i += 1;
        if report_only.contains(&p) || !seen.insert(p.clone()) {
            continue;
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("read {}: {e}", p.display()))?;
        if p.extension().is_some_and(|x| x == "rs") {
            for inc in includes(&p, &String::from_utf8_lossy(&bytes)) {
                if inc.is_file() {
                    files.push(inc);
                }
            }
        }
        out.push((rel(root, &p), sha_hex(&bytes)));
    }
    out.sort();
    Ok(out)
}

/// `n` of `m` camera indices, fixed by the seed, in recording order.
pub fn sample(m: usize, n: usize, seed: u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..m).collect();
    let n = n.min(m);
    let mut rng = Pcg32::new(seed, 0x7a66_6967_6874);
    for i in 0..n {
        let left = u32::try_from(m - i).unwrap_or(u32::MAX);
        let j = i + rng.below(left) as usize;
        idx.swap(i, j);
    }
    let mut out = idx[..n].to_vec();
    out.sort_unstable();
    out
}

/// What the report needs from a scan.
#[derive(Debug, Clone)]
pub struct Capture {
    pub tier: String,
    /// Look material names by index (labels).
    pub materials: Vec<String>,
    /// Draw slot where the static world's decals start.
    pub decal_from: usize,
    /// The static world: (draw slot, instance).
    pub statics: Vec<(usize, MeshInstance)>,
    /// The app's custom meshes, in id order from `BUILTIN_MESH_COUNT`.
    pub library: Vec<Mesh>,
    pub open_meshes: Vec<u16>,
    /// Cameras the replay recorded.
    pub cameras_total: usize,
    /// `--sample`: the indices scanned (of `cameras_total`).
    pub sample: Option<Vec<usize>>,
    /// The cameras scanned.
    pub viewers: Vec<Viewer>,
    /// The z-fight issues, untagged, in the order the test found them.
    pub issues: Vec<Issue>,
    /// The "what ran" text.
    pub ran: String,
    /// The run's notes (after the config's notes).
    pub notes: Vec<String>,
}

// ---- lossless encoding: floats as their bits

fn f64v(x: f64) -> Value {
    json!(x.to_bits())
}

fn f32v(x: f32) -> Value {
    json!(x.to_bits())
}

fn get_u64(v: &Value, what: &str) -> Result<u64, String> {
    v.as_u64().ok_or_else(|| format!("bad {what}"))
}

fn get_f64(v: &Value, what: &str) -> Result<f64, String> {
    get_u64(v, what).map(f64::from_bits)
}

fn get_f32(v: &Value, what: &str) -> Result<f32, String> {
    get_u64(v, what).and_then(|b| u32::try_from(b).map_err(|_| format!("bad {what}"))).map(f32::from_bits)
}

fn get_usize(v: &Value, what: &str) -> Result<usize, String> {
    get_u64(v, what).and_then(|b| usize::try_from(b).map_err(|_| format!("bad {what}")))
}

fn get_str(v: &Value, what: &str) -> Result<String, String> {
    v.as_str().map(str::to_string).ok_or_else(|| format!("bad {what}"))
}

fn arr<'a>(v: &'a Value, what: &str) -> Result<&'a Vec<Value>, String> {
    v.as_array().ok_or_else(|| format!("bad {what}"))
}

fn v3v(p: [f64; 3]) -> Value {
    json!(p.map(f64::to_bits))
}

fn get_v3(v: &Value, what: &str) -> Result<[f64; 3], String> {
    let a = arr(v, what)?;
    if a.len() != 3 {
        return Err(format!("bad {what}"));
    }
    Ok([get_f64(&a[0], what)?, get_f64(&a[1], what)?, get_f64(&a[2], what)?])
}

fn get_f32s<const N: usize>(a: &[Value], from: usize, what: &str) -> Result<[f32; N], String> {
    let mut out = [0.0; N];
    for (k, o) in out.iter_mut().enumerate() {
        *o = get_f32(a.get(from + k).ok_or_else(|| format!("short {what}"))?, what)?;
    }
    Ok(out)
}

/// The key marking an encoded f64 inside an issue's `data` (JSON numbers
/// would not come back bit for bit).
const F64_TAG: &str = "\u{0}f64";

fn enc_data(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => {
            let mut m = Map::new();
            m.insert(F64_TAG.into(), json!(n.as_f64().unwrap_or(0.0).to_bits()));
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(enc_data).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), enc_data(v))).collect()),
        v => v.clone(),
    }
}

fn dec_data(v: &Value) -> Result<Value, String> {
    Ok(match v {
        Value::Object(o) if o.len() == 1 && o.contains_key(F64_TAG) => {
            let x = get_f64(&o[F64_TAG], "data float")?;
            serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
        }
        Value::Array(a) => Value::Array(a.iter().map(dec_data).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| Ok((k.clone(), dec_data(v)?))).collect::<Result<_, String>>()?),
        v => v.clone(),
    })
}

fn opt<T>(v: &Value, f: impl Fn(&Value) -> Result<T, String>) -> Result<Option<T>, String> {
    if v.is_null() { Ok(None) } else { f(v).map(Some) }
}

fn enc_issue(i: &Issue) -> Value {
    json!({
        "check": i.check.name(),
        "kind": i.kind,
        "objects": i.objects,
        "items": i.items,
        "at": v3v(i.at),
        "aabb": i.aabb.map(|b| json!([v3v(b.min), v3v(b.max)])),
        "measure": f64v(i.measure),
        "unit": i.unit,
        "detail": i.detail,
        "fix": i.fix,
        "severity": match i.severity { Severity::Error => "error", Severity::Warning => "warning" },
        "tagged": i.tagged,
        "crop": i.crop,
        "pixels": i.pixels,
        "time": i.time.map(f64::to_bits),
        "data": enc_data(&i.data),
    })
}

fn dec_issue(v: &Value) -> Result<Issue, String> {
    let check = Check::parse(&get_str(&v["check"], "issue check")?).ok_or("unknown issue check")?;
    let mut i = Issue::new(check, &get_str(&v["kind"], "issue kind")?, get_v3(&v["at"], "issue at")?);
    i.objects = arr(&v["objects"], "issue objects")?.iter().map(|o| get_str(o, "object")).collect::<Result<_, _>>()?;
    i.items = arr(&v["items"], "issue items")?.iter().map(|o| get_usize(o, "item")).collect::<Result<_, _>>()?;
    i.aabb = opt(&v["aabb"], |b| {
        Ok(Aabb {
            min: get_v3(&b[0], "aabb")?,
            max: get_v3(&b[1], "aabb")?,
        })
    })?;
    i.measure = get_f64(&v["measure"], "measure")?;
    i.unit = get_str(&v["unit"], "unit")?;
    i.detail = get_str(&v["detail"], "detail")?;
    i.fix = get_str(&v["fix"], "fix")?;
    i.severity = match v["severity"].as_str() {
        Some("error") => Severity::Error,
        Some("warning") => Severity::Warning,
        _ => return Err("bad severity".into()),
    };
    i.tagged = opt(&v["tagged"], |t| get_str(t, "tagged"))?;
    i.crop = opt(&v["crop"], |t| get_str(t, "crop"))?;
    i.pixels = opt(&v["pixels"], |p| {
        let a = arr(p, "pixels")?;
        let g = |k: usize| a.get(k).and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).ok_or("bad pixels");
        Ok([g(0)?, g(1)?, g(2)?, g(3)?])
    })?;
    i.time = opt(&v["time"], |t| get_f64(t, "time"))?;
    i.data = dec_data(&v["data"])?;
    Ok(i)
}

fn enc_inst(slot: usize, m: &MeshInstance) -> Value {
    let mut a = vec![json!(slot), json!(m.mesh.0)];
    a.extend(m.position.iter().chain(&m.rotation).chain(&m.scale).map(|x| f32v(*x)));
    a.push(json!(m.material));
    a.extend(m.color.iter().map(|x| f32v(*x)));
    Value::Array(a)
}

fn dec_inst(v: &Value) -> Result<(usize, MeshInstance), String> {
    let a = arr(v, "instance")?;
    let slot = get_usize(a.first().ok_or("short instance")?, "slot")?;
    let mesh = u16::try_from(get_u64(a.get(1).ok_or("short instance")?, "mesh")?).map_err(|_| "bad mesh id")?;
    let mut m = MeshInstance::new(MeshId(mesh), get_f32s::<3>(a, 2, "position")?);
    m.rotation = get_f32s::<4>(a, 5, "rotation")?;
    m.scale = get_f32s::<3>(a, 9, "scale")?;
    m.material = u32::try_from(get_u64(a.get(12).ok_or("short instance")?, "material")?).map_err(|_| "bad material")?;
    m.color = get_f32s::<4>(a, 13, "color")?;
    Ok((slot, m))
}

fn enc_viewer(v: &Viewer) -> Value {
    let c = &v.cam;
    json!({
        "cam": [v3v(c.eye), v3v(c.target), v3v(c.up), f64v(c.fov_y_deg), f64v(c.aspect)],
        "height_px": f64v(v.height_px),
        "label": v.label,
        "time": v.time.map(f64::to_bits),
    })
}

fn dec_viewer(v: &Value) -> Result<Viewer, String> {
    let c = arr(&v["cam"], "camera")?;
    if c.len() != 5 {
        return Err("bad camera".into());
    }
    Ok(Viewer {
        cam: Camera {
            eye: get_v3(&c[0], "eye")?,
            target: get_v3(&c[1], "target")?,
            up: get_v3(&c[2], "up")?,
            fov_y_deg: get_f64(&c[3], "fov")?,
            aspect: get_f64(&c[4], "aspect")?,
        },
        height_px: get_f64(&v["height_px"], "height")?,
        label: get_str(&v["label"], "label")?,
        time: opt(&v["time"], |t| get_f64(t, "time"))?,
    })
}

impl Capture {
    pub fn to_json(&self) -> Value {
        json!({
            "format": FORMAT,
            "tier": self.tier,
            "materials": self.materials,
            "decal_from": self.decal_from,
            "statics": self.statics.iter().map(|(s, m)| enc_inst(*s, m)).collect::<Vec<_>>(),
            "library": self.library.iter().map(|m| json!({
                "vertices": m.vertices.iter().flatten().map(|x| x.to_bits()).collect::<Vec<_>>(),
                "indices": m.indices,
            })).collect::<Vec<_>>(),
            "open_meshes": self.open_meshes,
            "cameras_total": self.cameras_total,
            "sample": self.sample,
            "viewers": self.viewers.iter().map(enc_viewer).collect::<Vec<_>>(),
            "issues": self.issues.iter().map(enc_issue).collect::<Vec<_>>(),
            "ran": self.ran,
            "notes": self.notes,
        })
    }

    pub fn from_json(v: &Value) -> Result<Capture, String> {
        if v["format"].as_u64() != Some(u64::from(FORMAT)) {
            return Err(format!("format {} (this tool reads format {FORMAT})", v["format"]));
        }
        let library = arr(&v["library"], "library")?
            .iter()
            .map(|m| {
                let flat: Vec<f32> = arr(&m["vertices"], "vertices")?.iter().map(|x| get_f32(x, "vertex")).collect::<Result<_, _>>()?;
                let (chunks, rest) = flat.as_chunks::<6>();
                if !rest.is_empty() {
                    return Err("bad vertices".to_string());
                }
                let vertices = chunks.to_vec();
                let indices = arr(&m["indices"], "indices")?
                    .iter()
                    .map(|x| get_u64(x, "index").and_then(|i| u32::try_from(i).map_err(|_| "bad index".to_string())))
                    .collect::<Result<_, _>>()?;
                Ok(Mesh { vertices, indices })
            })
            .collect::<Result<_, String>>()?;
        Ok(Capture {
            tier: get_str(&v["tier"], "tier")?,
            materials: arr(&v["materials"], "materials")?.iter().map(|m| get_str(m, "material")).collect::<Result<_, _>>()?,
            decal_from: get_usize(&v["decal_from"], "decal_from")?,
            statics: arr(&v["statics"], "statics")?.iter().map(dec_inst).collect::<Result<_, _>>()?,
            library,
            open_meshes: arr(&v["open_meshes"], "open_meshes")?
                .iter()
                .map(|x| get_u64(x, "open mesh").and_then(|i| u16::try_from(i).map_err(|_| "bad open mesh".to_string())))
                .collect::<Result<_, _>>()?,
            cameras_total: get_usize(&v["cameras_total"], "cameras_total")?,
            sample: opt(&v["sample"], |s| arr(s, "sample")?.iter().map(|x| get_usize(x, "sample index")).collect())?,
            viewers: arr(&v["viewers"], "viewers")?.iter().map(dec_viewer).collect::<Result<_, _>>()?,
            issues: arr(&v["issues"], "issues")?.iter().map(dec_issue).collect::<Result<_, _>>()?,
            ran: get_str(&v["ran"], "ran")?,
            notes: arr(&v["notes"], "notes")?.iter().map(|n| get_str(n, "note")).collect::<Result<_, _>>()?,
        })
    }

    /// The static world, rebuilt the way the scan built it.
    pub fn scene(&self, cfg: Config) -> Scene {
        let mut shapes = Shapes::new();
        for (n, m) in self.library.iter().enumerate() {
            shapes.add_mesh(MeshId((BUILTIN_MESH_COUNT + n) as u16), m.clone());
        }
        for &id in &self.open_meshes {
            shapes.allow_open(MeshId(id));
        }
        let materials = &self.materials;
        let label = |m: &MeshInstance| default_label(m, materials);
        let from = self.decal_from;
        Scene::build_with(&self.statics, &mut shapes, &label, cfg, &|i| i >= from)
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

/// Save `cap` (already encoded) under `dir`; the manifest goes last, so an
/// entry without one is incomplete. Then drop all but the newest entries.
pub fn save(dir: &Path, key: &Key, cap: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    if let Some(top) = dir.parent().and_then(Path::parent) {
        let ignore = top.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|e| format!("write {}: {e}", ignore.display()))?;
        }
    }
    let _ = std::fs::remove_file(dir.join("manifest.json"));
    let text = serde_json::to_vec(cap).map_err(|e| e.to_string())?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&text).map_err(|e| e.to_string())?;
    let bytes = gz.finish().map_err(|e| e.to_string())?;
    write_atomic(&dir.join("capture.json.gz"), &bytes)?;
    let man = serde_json::to_vec_pretty(&key.manifest()).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("manifest.json"), &man)?;
    prune(dir.parent().unwrap_or(dir), dir);
    Ok(())
}

/// Remove an app's oldest entries past [`KEEP`] (never `keep`).
fn prune(app_dir: &Path, keep: &Path) {
    let Ok(rd) = std::fs::read_dir(app_dir) else { return };
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p != keep)
        .map(|p| (std::fs::metadata(p.join("manifest.json")).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    entries.sort();
    let extra = (entries.len() + 1).saturating_sub(KEEP);
    for (_, p) in entries.into_iter().take(extra) {
        let _ = std::fs::remove_dir_all(p);
    }
}

/// Load the capture saved under `dir`.
pub fn load(dir: &Path) -> Result<Capture, String> {
    if !dir.join("manifest.json").is_file() {
        return Err(format!("no saved capture at {}", dir.display()));
    }
    let bytes = std::fs::read(dir.join("capture.json.gz")).map_err(|e| format!("read {}: {e}", dir.join("capture.json.gz").display()))?;
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut text).map_err(|e| format!("unpack {}: {e}", dir.display()))?;
    let v: Value = serde_json::from_slice(&text).map_err(|e| format!("parse {}: {e}", dir.display()))?;
    Capture::from_json(&v).map_err(|e| format!("the saved capture at {} can't be read: {e}", dir.display()))
}

/// Why `--report-only` has no capture to use: what differs from the newest
/// saved one (with the same camera set when there is one).
/// `app_dir`: the app's cache folder (`<target>/evani-geocheck-cache/<app>`).
pub fn explain_missing(app_dir: &Path, app: &str, key: &Key, why: &str, rerun: &str) -> String {
    let mut head = format!(
        "--report-only: no saved z-fight capture matches this build, scene and camera set (key {}; {why}).",
        key.key
    );
    let mut saved: Vec<(std::time::SystemTime, Value)> = std::fs::read_dir(app_dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = e.path().join("manifest.json");
            let t = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            let v: Value = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
            Some((t, v))
        })
        .collect();
    saved.sort_by_key(|s| std::cmp::Reverse(s.0));
    let cameras = key.parts.iter().find(|p| p.name == "cameras").map(|p| p.hash.clone()).unwrap_or_default();
    let part_hash = |m: &Value, name: &str| {
        m["parts"].as_array().into_iter().flatten().find(|p| p["name"] == name).map(|p| (p["hash"].as_str().unwrap_or("").to_string(), p["text"].as_str().unwrap_or("").to_string()))
    };
    let best = saved
        .iter()
        .find(|(_, m)| part_hash(m, "cameras").is_some_and(|c| c.0 == cameras))
        .or_else(|| saved.first());
    match best {
        None => head.push_str(&format!("\nNo capture is saved for {app} yet (in {}).", app_dir.display())),
        Some((_, m)) => {
            head.push_str(&format!("\nWhat changed since the newest saved capture (key {}):", m["key"].as_str().unwrap_or("?")));
            if m["format"].as_u64() != Some(u64::from(FORMAT)) {
                head.push_str(&format!("\n- the cache format: saved {}, this tool {FORMAT}", m["format"]));
            }
            for p in &key.parts {
                let Some((h, text)) = part_hash(m, p.name) else { continue };
                if h == p.hash {
                    continue;
                }
                if p.name == "build" {
                    let old: std::collections::BTreeMap<String, String> = m["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|f| Some((f[0].as_str()?.to_string(), f[1].as_str()?.to_string())))
                        .collect();
                    let now: std::collections::BTreeMap<String, String> = key.files.iter().cloned().collect();
                    let mut diff: Vec<String> = now
                        .iter()
                        .filter_map(|(f, h)| match old.get(f) {
                            None => Some(format!("{f} (new)")),
                            Some(o) if o != h => Some(format!("{f} (changed)")),
                            _ => None,
                        })
                        .collect();
                    diff.extend(old.keys().filter(|f| !now.contains_key(*f)).map(|f| format!("{f} (gone)")));
                    let shown: Vec<String> = diff.iter().take(8).cloned().collect();
                    let more = if diff.len() > shown.len() { format!(", and {} more", diff.len() - shown.len()) } else { String::new() };
                    if diff.is_empty() {
                        head.push_str(&format!("\n- the build: the toolchain or profile (saved: {text}; now: {})", p.text));
                    } else {
                        head.push_str(&format!("\n- the build: {} file(s) differ: {}{more}", diff.len(), shown.join(", ")));
                    }
                } else {
                    // Parts are "; "-separated items: name only the ones that differ.
                    let old: Vec<&str> = text.split("; ").collect();
                    let now: Vec<&str> = p.text.split("; ").collect();
                    let diff: Vec<String> = if old.len() == now.len() {
                        old.iter().zip(&now).filter(|(a, b)| a != b).map(|(a, b)| format!("saved \"{a}\", now \"{b}\"")).collect()
                    } else {
                        vec![format!("saved \"{text}\", now \"{}\"", p.text)]
                    };
                    head.push_str(&format!("\n- the {}: {}", p.name, diff.join("; ")));
                }
            }
        }
    }
    head.push_str(&format!("\nRun without --report-only ({rerun}) to make a new capture; the next --report-only uses it."));
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-geocheck-cache-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &Path, f: &str, text: &str) {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A tiny workspace: geocheck depends on a core crate through the
    /// workspace table, the app on a kit by path; the kit includes a file
    /// from outside its crate.
    fn workspace(name: &str) -> PathBuf {
        let r = tmp(name);
        put(&r, "Cargo.toml", "[workspace]\nmembers = []\n[workspace.dependencies]\ncore = { path = \"crates/core\" }\n");
        put(&r, "Cargo.lock", "# lock\n");
        put(&r, "tools/evani-geocheck/Cargo.toml", "[package]\nname = \"g\"\n[dependencies]\ncore = { workspace = true }\n[dev-dependencies]\ndevonly = { path = \"../../crates/devonly\" }\n");
        put(&r, "tools/evani-geocheck/src/main.rs", "fn main() {}\n");
        put(&r, "tools/evani-geocheck/src/cli/zfight.rs", "// report\n");
        put(&r, "tools/evani-geocheck/src/cli/png.rs", "// png\n");
        put(&r, "crates/core/Cargo.toml", "[package]\nname = \"core\"\n");
        put(&r, "crates/core/src/lib.rs", "pub fn a() {}\n");
        put(&r, "crates/devonly/Cargo.toml", "[package]\nname = \"devonly\"\n");
        put(&r, "crates/devonly/src/lib.rs", "\n");
        put(&r, "apps/demo/Cargo.toml", "[package]\nname = \"demo\"\n[target.'cfg(target_arch = \"wasm32\")'.dependencies]\nkit = { path = \"../../kits/kit\", optional = true }\n");
        put(&r, "apps/demo/src/main.rs", "fn main() {}\n");
        put(&r, "apps/demo/geocheck.toml", "[thresholds]\n");
        put(&r, "apps/demo/README.md", "words\n");
        put(&r, "kits/kit/Cargo.toml", "[package]\nname = \"kit\"\n");
        put(&r, "kits/kit/src/lib.rs", "const T: &str = include_str!(\"../../../assets/tiers/high.json\");\n");
        put(&r, "kits/kit/target/junk.bin", "x");
        put(&r, "assets/tiers/high.json", "{}\n");
        put(&r, "tools/live-check/capture.mjs", "// capture\n");
        r
    }

    fn key(r: &Path, scene: &str, cams: &str, set: &str) -> Key {
        Key::compute(r, "demo", scene, cams, set).unwrap()
    }

    #[test]
    fn the_key_changes_with_each_input_and_only_those() {
        let r = workspace("key");
        let files: Vec<String> = build_files(&r, "demo").unwrap().into_iter().map(|f| f.0).collect();
        for f in ["Cargo.lock", "tools/evani-geocheck/src/main.rs", "crates/core/src/lib.rs", "kits/kit/src/lib.rs", "assets/tiers/high.json", "tools/live-check/capture.mjs", "apps/demo/src/main.rs"] {
            assert!(files.contains(&f.to_string()), "{f} in the build part: {files:?}");
        }
        for f in ["tools/evani-geocheck/src/cli/zfight.rs", "tools/evani-geocheck/src/cli/png.rs", "apps/demo/geocheck.toml", "apps/demo/README.md", "kits/kit/target/junk.bin", "crates/devonly/src/lib.rs"] {
            assert!(!files.contains(&f.to_string()), "{f} left out");
        }
        let base = key(&r, "scene", "all", "settings");
        assert_eq!(base, key(&r, "scene", "all", "settings"), "same inputs, same key");
        assert_eq!(base.key.len(), 16);
        // Each part on its own.
        assert_ne!(base.key, key(&r, "scene2", "all", "settings").key, "scene");
        assert_ne!(base.key, key(&r, "scene", "sample 50", "settings").key, "cameras");
        assert_ne!(base.key, key(&r, "scene", "all", "settings2").key, "settings");
        // The build: engine, kit, included data, the lock file.
        for f in ["crates/core/src/lib.rs", "kits/kit/src/lib.rs", "assets/tiers/high.json", "Cargo.lock"] {
            let before = std::fs::read_to_string(r.join(f)).unwrap();
            put(&r, f, &format!("{before}// edit\n"));
            let k = key(&r, "scene", "all", "settings");
            assert_ne!(base.key, k.key, "editing {f}");
            put(&r, f, &before);
        }
        // Report-only files, docs and tags keep it.
        put(&r, "tools/evani-geocheck/src/cli/zfight.rs", "// a new table\n");
        put(&r, "apps/demo/README.md", "other words\n");
        put(&r, "apps/demo/geocheck.toml", "[[tag]]\n");
        assert_eq!(base.key, key(&r, "scene", "all", "settings").key);
        // A new crate file counts.
        put(&r, "crates/core/src/extra.rs", "\n");
        assert_ne!(base.key, key(&r, "scene", "all", "settings").key);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn a_stale_report_only_says_what_changed() {
        let r = workspace("stale");
        let none = key(&r, "scene", "all", "set");
        let app_dir = r.join("cache/demo");
        let msg = explain_missing(&app_dir, "demo", &none, "no saved capture", "cargo xtask geocheck demo --zfight");
        assert!(msg.contains("No capture is saved for demo yet"), "{msg}");
        assert!(msg.contains("Run without --report-only"), "{msg}");
        // Save one, then change a file and the thresholds.
        let dir = app_dir.join(&none.key);
        save(&dir, &none, &json!({"format": FORMAT})).unwrap();
        put(&r, "crates/core/src/lib.rs", "pub fn b() {}\n");
        let now = key(&r, "scene with zfight_px 5", "all", "set");
        let msg = explain_missing(&app_dir, "demo", &now,"no saved capture", "cargo xtask geocheck demo --zfight");
        assert!(msg.contains(&format!("key {}", none.key)), "{msg}");
        assert!(msg.contains("crates/core/src/lib.rs (changed)"), "{msg}");
        assert!(msg.contains("the scene: saved \"scene\", now \"scene with zfight_px 5\""), "{msg}");
        assert!(!msg.contains("the cameras"), "{msg}");
        assert!(r.join("cache/.gitignore").is_file(), "the cache folder is never committed");
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn the_sample_is_fixed_by_its_seed() {
        let a = sample(700, 50, SAMPLE_SEED);
        assert_eq!(a, sample(700, 50, SAMPLE_SEED), "same subset every time");
        assert_eq!(a.len(), 50);
        assert!(a.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
        assert!(a.iter().all(|&i| i < 700));
        assert_ne!(a, sample(700, 50, SAMPLE_SEED + 1));
        assert_ne!(a, (0..50).collect::<Vec<_>>(), "spread, not the first 50");
        assert_eq!(sample(10, 50, SAMPLE_SEED), (0..10).collect::<Vec<_>>(), "never more than there are");
    }

    #[test]
    fn a_capture_comes_back_bit_for_bit() {
        let mut i = Issue::new(Check::ZFight, "thin slab", [0.1, -0.0, 1e-300]);
        i.objects = vec!["road (cube)".into()];
        i.items = vec![3, 4];
        i.aabb = Some(Aabb { min: [0.1 + 0.2, 1.0 / 3.0, -5.5], max: [2.0, f64::MAX, 7.0] });
        i.measure = 0.1 + 0.2;
        i.time = Some(1.0 / 3.0);
        i.pixels = Some([1, 2, 3, 4]);
        i.data = json!({"epsilon_m": 0.1 + 0.2, "n": 7, "list": [1.0 / 3.0, "x", null, true], "nested": {"z": 2.5e-17}});
        let cam = Camera { eye: [1.0 / 3.0, 2.0, 3.0], target: [0.0; 3], up: [0.0, 1.0, 0.0], fov_y_deg: 60.0, aspect: 1280.0 / 720.0 };
        let mut m = MeshInstance::new(MeshId(9), [0.1, 0.2, 0.3]);
        m.rotation = [0.0, 0.70710677, 0.0, 0.70710677];
        m.color = [0.3, 0.4, 0.5, 1.0];
        m.material = 4;
        let cap = Capture {
            tier: "high".into(),
            materials: vec!["road".into()],
            decal_from: 1,
            statics: vec![(0, m)],
            library: vec![Mesh { vertices: vec![[0.1, 0.2, 0.3, 0.0, 1.0, 0.0]], indices: vec![0, 0, 0] }],
            open_meshes: vec![9],
            cameras_total: 700,
            sample: Some(vec![1, 5]),
            viewers: vec![Viewer { cam, height_px: 720.0, label: "title".into(), time: Some(0.1) }],
            issues: vec![i],
            ran: "ran".into(),
            notes: vec!["note".into()],
        };
        let text = serde_json::to_string(&cap.to_json()).unwrap();
        let back = Capture::from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(serde_json::to_string(&back.to_json()).unwrap(), text);
        assert_eq!(back.issues[0].to_json(), cap.issues[0].to_json());
        assert_eq!(back.issues[0].data, cap.issues[0].data);
        assert_eq!(back.viewers, cap.viewers);
        assert_eq!(back.statics[0].1.rotation, m.rotation);
        assert_eq!(back.sample, Some(vec![1, 5]));
        // A different format is refused.
        let mut v = cap.to_json();
        v["format"] = json!(FORMAT + 1);
        assert!(Capture::from_json(&v).unwrap_err().contains("format"));
    }
}
