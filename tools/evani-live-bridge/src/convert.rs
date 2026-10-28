//! Drop-to-convert (S3-14, docs/LIVE_API.md 16): a model file from the
//! page (`POST /live/v1/convert`) or a local path (`model.convert`) goes
//! through evani-convert, one job at a time, at below-normal priority. The
//! results live under `<root>/.evani/convert/<hash>/out/` and are served
//! back at `/live/v1/convert/<hash>/<file>`.
//!
//! Safety rules (all tested): the upload's name never picks a path (the
//! folder is a hash of the content, the file name is the checked id); the
//! upload type comes from its first bytes; zip entries that climb out, are
//! absolute, carry a drive letter, a backslash or a NUL are refused
//! (zip-slip), and so are `.gltf` URIs that point outside the zip; the
//! converter runs without a shell; a second job while one runs is `busy`;
//! the input copy and the unpacked zip are removed when the job ends; on
//! the run limit the converter's whole process tree is ended; old
//! conversions are removed when a new job starts (newest 8, up to 7 days).

use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The biggest upload the bridge reads (a `.blend` with packed textures).
pub const MAX_UPLOAD: usize = 256 * 1024 * 1024;
/// The most a zip may unpack to, in total.
pub const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
/// The most entries a zip may hold.
const MAX_ENTRIES: usize = 4096;
/// A conversion that runs longer than this is stopped, with everything it
/// started (the gate renders and scores every tier, which takes minutes on
/// a big model).
const RUN_LIMIT: Duration = Duration::from_secs(30 * 60);
/// Finished conversions kept under `.evani/convert/`: the newest this many,
/// and none older than [`KEEP_AGE`] (the current and latest are always kept).
const KEEP_JOBS: usize = 8;
const KEEP_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Hex characters of the content hash used as the folder name.
const HASH_HEX: usize = 32;

/// Why a request was refused: an HTTP status, a stable code and a sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

fn refuse(status: u16, code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { status, code, message: message.into() }
}

/// What a job runs: the converter on `input`, writing into `out`.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub root: PathBuf,
    pub input: PathBuf,
    pub id: String,
    pub out: PathBuf,
    pub log: PathBuf,
}

pub type RunFuture = Pin<Box<dyn Future<Output = Result<i32, String>> + Send>>;
/// Runs one conversion and returns its exit code (0 passed, 1 converted with
/// problems, 2 failed). Swapped in tests so nothing real is spawned.
pub type Runner = Arc<dyn Fn(RunSpec) -> RunFuture + Send + Sync>;
/// Where the job's events go (the hub: `/ctl`, `/evani-dev` and the page).
pub type Sink = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// The upload's type, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Glb,
    Zip,
    Blend,
}

impl Kind {
    pub fn sniff(b: &[u8]) -> Option<Kind> {
        if b.starts_with(b"glTF") {
            Some(Kind::Glb)
        } else if b.starts_with(b"PK\x03\x04") {
            Some(Kind::Zip)
        } else if b.starts_with(b"BLENDER") || b.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) || b.starts_with(&[0x1F, 0x8B]) {
            // Plain, zstd-compressed (Blender 3+) or gzip-compressed (older) .blend.
            Some(Kind::Blend)
        } else {
            None
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Kind::Glb => "glb",
            Kind::Zip => "zip",
            Kind::Blend => "blend",
        }
    }
}

/// evani-convert's id rule: lowercase letters, digits, `-` and `_`, up to
/// 80, not starting with `-` or `_`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.starts_with(['_', '-'])
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// A file stem as an id (for a local path with no `id`).
fn id_from(stem: &str) -> String {
    let s: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .take(80)
        .collect();
    let s = s.trim_matches(['_', '-']).to_owned();
    if s.is_empty() { "model".into() } else { s }
}

fn is_hash(h: &str) -> bool {
    h.len() == HASH_HEX && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn hash_of(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s.truncate(HASH_HEX);
    s
}

// ---- the zip reader -------------------------------------------------------------------

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A zip entry name as a safe relative path, or why not. Refused: empty,
/// absolute (`/`), a drive letter or any `:`, a backslash, a NUL, and any
/// empty, `.` or `..` part. A trailing `/` marks a folder.
pub fn safe_entry(name: &str) -> Result<(PathBuf, bool), String> {
    let dir = name.ends_with('/');
    let body = name.strip_suffix('/').unwrap_or(name);
    if body.is_empty() {
        return Err(format!("zip entry {name:?} has no name"));
    }
    if body.starts_with('/') {
        return Err(format!("zip entry {name:?} is an absolute path"));
    }
    if body.contains(['\\', ':', '\0']) {
        return Err(format!("zip entry {name:?} has a backslash, a drive letter or a NUL"));
    }
    let mut out = PathBuf::new();
    for part in body.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("zip entry {name:?} climbs out of the zip"));
        }
        if windows_reserved(part) {
            return Err(format!("zip entry {name:?} uses a name Windows reserves (CON, NUL, COM1...) or ends in a dot or space"));
        }
        out.push(part);
    }
    Ok((out, dir))
}

/// A path part Windows treats as a device or silently changes: `CON`,
/// `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9` (any case, with or
/// without an extension: `nul.txt` is the device too), and any part ending
/// in a dot or a space (Windows drops them, so two entries could land on
/// one file).
fn windows_reserved(part: &str) -> bool {
    if part.ends_with(['.', ' ']) {
        return true;
    }
    let stem = part.split('.').next().unwrap_or(part).trim_end_matches(' ').to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s => {
            let b = s.as_bytes();
            b.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) && (b'1'..=b'9').contains(&b[3])
        }
    }
}

/// Unpack a stored or deflated zip into `dest`, every entry checked by
/// [`safe_entry`], the total capped at `cap` bytes, every CRC checked.
pub fn unzip(zip: &[u8], dest: &Path, cap: u64) -> Result<Vec<PathBuf>, String> {
    // End of central directory: the last 22 bytes plus up to 64 KiB of comment.
    let start = zip.len().saturating_sub(22 + 0xFFFF);
    let eocd = (start..=zip.len().saturating_sub(22))
        .rev()
        .find(|&i| zip[i..].starts_with(b"PK\x05\x06"))
        .ok_or("not a zip (no end of central directory)")?;
    let bad = || "the zip is cut short or damaged".to_string();
    let count = usize::from(u16_at(zip, eocd + 10).ok_or_else(bad)?);
    let cd_size = u32_at(zip, eocd + 12).ok_or_else(bad)?;
    let cd_at = u32_at(zip, eocd + 16).ok_or_else(bad)?;
    if count == 0xFFFF || cd_size == u32::MAX || cd_at == u32::MAX {
        return Err("zip64 files are not supported; zip the .gltf with a plain zip tool".into());
    }
    if count > MAX_ENTRIES {
        return Err(format!("the zip has {count} entries (most {MAX_ENTRIES})"));
    }
    let mut at = cd_at as usize;
    let mut total = 0u64;
    let mut files = Vec::new();
    for _ in 0..count {
        if u32_at(zip, at) != Some(0x0201_4b50) {
            return Err(bad());
        }
        let flags = u16_at(zip, at + 8).ok_or_else(bad)?;
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let crc = u32_at(zip, at + 16).ok_or_else(bad)?;
        let packed = u64::from(u32_at(zip, at + 20).ok_or_else(bad)?);
        let size = u64::from(u32_at(zip, at + 24).ok_or_else(bad)?);
        let name_len = usize::from(u16_at(zip, at + 28).ok_or_else(bad)?);
        let extra_len = usize::from(u16_at(zip, at + 30).ok_or_else(bad)?);
        let comment_len = usize::from(u16_at(zip, at + 32).ok_or_else(bad)?);
        let local = u32_at(zip, at + 42).ok_or_else(bad)? as usize;
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?;
        at += 46 + name_len + extra_len + comment_len;
        let name = std::str::from_utf8(name).map_err(|_| "a zip entry name is not UTF-8".to_string())?;
        let (rel, is_dir) = safe_entry(name)?;
        if flags & 1 != 0 {
            return Err(format!("zip entry {name:?} is encrypted"));
        }
        let path = dest.join(&rel);
        if is_dir {
            std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            continue;
        }
        total = total.saturating_add(size);
        if total > cap {
            return Err(format!("the zip unpacks to more than {} MiB", cap / (1024 * 1024)));
        }
        if u32_at(zip, local) != Some(0x0403_4b50) {
            return Err(bad());
        }
        let lname = usize::from(u16_at(zip, local + 26).ok_or_else(bad)?);
        let lextra = usize::from(u16_at(zip, local + 28).ok_or_else(bad)?);
        let data_at = local + 30 + lname + lextra;
        let data = zip.get(data_at..data_at.saturating_add(packed as usize)).ok_or_else(bad)?;
        let mut out = Vec::with_capacity(size.min(64 * 1024 * 1024) as usize);
        match method {
            0 => out.extend_from_slice(data),
            8 => {
                flate2::read::DeflateDecoder::new(data)
                    .take(size + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("zip entry {name:?}: {e}"))?;
            }
            m => return Err(format!("zip entry {name:?} uses compression method {m}; only stored and deflate are read")),
        }
        if out.len() as u64 != size {
            return Err(format!("zip entry {name:?} is not the size its header says"));
        }
        let mut c = flate2::Crc::new();
        c.update(&out);
        if c.sum() != crc {
            return Err(format!("zip entry {name:?} fails its CRC check"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &out).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(rel);
    }
    Ok(files)
}

/// Every external URI a `.gltf` names must stay inside its folder: no
/// scheme, no absolute path, no backslash, no `..` (after percent-decoding).
pub fn gltf_uris_ok(gltf: &[u8]) -> Result<(), String> {
    let v: Value = serde_json::from_slice(gltf).map_err(|e| format!("the .gltf is not JSON: {e}"))?;
    for list in ["buffers", "images"] {
        for item in v[list].as_array().into_iter().flatten() {
            let Some(uri) = item["uri"].as_str() else { continue };
            if uri.starts_with("data:") {
                continue;
            }
            let decoded = percent_decode(uri);
            let bad = decoded.contains([':', '\\', '\0'])
                || decoded.starts_with('/')
                || decoded.split('/').any(|p| p == "..");
            if bad {
                return Err(format!("the .gltf points outside its zip ({uri:?})"));
            }
        }
    }
    Ok(())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(h) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(h, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- the job slot ---------------------------------------------------------------------

struct State {
    root: PathBuf,
    dir: PathBuf,
    /// The running job's id; `Some` while one runs.
    busy: Mutex<Option<String>>,
    /// The latest finished job's hash (`model.report` with no hash).
    latest: Mutex<Option<String>>,
    runner: Mutex<Runner>,
    sink: Mutex<Option<Sink>>,
    seq: AtomicU64,
    /// The run limit in ms ([`RUN_LIMIT`]; shorter in tests).
    limit_ms: AtomicU64,
}

impl State {
    fn emit(&self, event: &str, data: Value) {
        let sink = self.sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(s) = sink {
            s(event, data);
        }
    }
}

/// Frees the job slot however the job ends (a panic included).
struct Slot(Arc<State>);

impl Drop for Slot {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

/// The convert job slot and its folder (`<root>/.evani/convert`).
#[derive(Clone)]
pub struct Converter {
    st: Arc<State>,
}

/// What a job converts.
enum Input<'a> {
    Upload(&'a [u8]),
    Local(&'a Path),
}

impl Converter {
    pub fn new(root: &Path) -> Self {
        let st = State {
            root: root.to_path_buf(),
            dir: root.join(".evani").join("convert"),
            busy: Mutex::new(None),
            latest: Mutex::new(None),
            runner: Mutex::new(default_runner()),
            sink: Mutex::new(None),
            seq: AtomicU64::new(1),
            limit_ms: AtomicU64::new(RUN_LIMIT.as_millis() as u64),
        };
        Self { st: Arc::new(st) }
    }

    /// Swap the runner (tests).
    #[cfg(test)]
    pub fn set_runner(&self, r: Runner) {
        *self.st.runner.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = r;
    }

    /// Shorten the run limit (tests).
    #[cfg(test)]
    pub fn set_limit(&self, d: Duration) {
        self.st.limit_ms.store(d.as_millis() as u64, Ordering::Relaxed);
    }

    /// Where events go (set once the hub is shared).
    pub fn set_sink(&self, s: Sink) {
        *self.st.sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(s);
    }

    /// Is a job running? Its id if so.
    pub fn running(&self) -> Option<String> {
        self.st.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Take the slot, or `busy` (409).
    fn claim(&self) -> Result<(Slot, String), Refusal> {
        let mut b = self.st.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(job) = &*b {
            return Err(refuse(409, "busy", format!("A conversion is already running ({job}); try again when it is done.")));
        }
        let job = format!("cv-{}", self.st.seq.fetch_add(1, Ordering::Relaxed));
        *b = Some(job.clone());
        drop(b);
        Ok((Slot(self.st.clone()), job))
    }

    /// `POST /live/v1/convert`: the body is a `.glb`, a zipped `.gltf` or a
    /// `.blend`. Starts the job and answers at once (`{job, hash, id}`).
    /// Blocking file work: call from `spawn_blocking`.
    pub fn start_upload(&self, body: &[u8], id: Option<&str>) -> Result<Value, Refusal> {
        self.start(Input::Upload(body), id, None)
    }

    /// `model.convert {path, id?}` with its `note`: a local `.glb`, `.gltf`
    /// (converted in place), `.zip` or `.blend`. Blocking file work.
    pub fn start_path(&self, path: &Path, id: Option<&str>, note: Option<&str>) -> Result<Value, Refusal> {
        self.start(Input::Local(path), id, note)
    }

    fn start(&self, input: Input<'_>, id: Option<&str>, note: Option<&str>) -> Result<Value, Refusal> {
        let note = note.map(str::trim).filter(|n| !n.is_empty()).map(|n| n.chars().take(500).collect::<String>());
        let id = match (id, &input) {
            (Some(i), _) if valid_id(i) => i.to_string(),
            (Some(i), _) => {
                return Err(refuse(400, "bad_request", format!("id {i:?}: lowercase letters, digits, - and _ only, up to 80, not starting with - or _")));
            }
            (None, Input::Local(p)) => id_from(&p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()),
            (None, Input::Upload(_)) => "model".into(),
        };
        // The cheap refusals come before the slot is taken.
        let (bytes, kind) = match &input {
            Input::Upload(b) => {
                if b.len() > MAX_UPLOAD {
                    return Err(refuse(413, "too_large", format!("The upload is over {} MiB.", MAX_UPLOAD / (1024 * 1024))));
                }
                let kind = Kind::sniff(b).ok_or_else(|| refuse(415, "unsupported", "Send a .glb, a zipped .gltf or a .blend file."))?;
                (std::borrow::Cow::Borrowed(*b), Some(kind))
            }
            Input::Local(p) => {
                let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                if !["glb", "gltf", "zip", "blend"].contains(&ext.as_str()) {
                    return Err(refuse(400, "unsupported", format!("{}: a .glb, .gltf, .zip or .blend file", p.display())));
                }
                let b = std::fs::read(p).map_err(|e| refuse(404, "not_found", format!("{}: {e}", p.display())))?;
                let kind = match ext.as_str() {
                    "glb" => Some(Kind::Glb),
                    "zip" => Some(Kind::Zip),
                    "blend" => Some(Kind::Blend),
                    _ => None, // .gltf: converted where it is
                };
                (std::borrow::Cow::Owned(b), kind)
            }
        };
        let (slot, job) = self.claim()?;
        let hash = hash_of(&bytes);
        {
            let latest = self.st.latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            let keep: Vec<&str> = [Some(hash.as_str()), latest.as_deref()].into_iter().flatten().collect();
            prune(&self.st.dir, &keep, std::time::SystemTime::now());
        }
        let folder = self.st.dir.join(&hash);
        let out = folder.join("out");
        // A fresh run: an earlier result for the same content is replaced.
        for sub in ["in", "src", "out"] {
            let _ = std::fs::remove_dir_all(folder.join(sub));
        }
        let io = |e: std::io::Error| refuse(500, "io_error", format!("{}: {e}", folder.display()));
        let (input_path, temp): (PathBuf, Vec<PathBuf>) = match (kind, &input) {
            // A local .gltf (from an agent with the token) may use `../`.
            (None, Input::Local(p)) => (p.to_path_buf(), Vec::new()),
            // An upload always has a kind (sniffed above).
            (None, Input::Upload(_)) => return Err(refuse(415, "unsupported", "Send a .glb, a zipped .gltf or a .blend file.")),
            (Some(Kind::Zip), _) => {
                let src = folder.join("src");
                std::fs::create_dir_all(&src).map_err(io)?;
                let unpacked = unzip(&bytes, &src, MAX_UNPACKED).map_err(|e| {
                    let _ = std::fs::remove_dir_all(&src);
                    refuse(400, "unsafe_zip", e)
                });
                let files = unpacked?;
                let gltfs: Vec<&PathBuf> = files.iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("gltf"))).collect();
                let one = match gltfs.as_slice() {
                    [one] => src.join(one),
                    found => {
                        let _ = std::fs::remove_dir_all(&src);
                        return Err(refuse(400, "unsupported", format!("The zip needs exactly one .gltf file (it has {}).", found.len())));
                    }
                };
                let text = std::fs::read(&one).map_err(io)?;
                if let Err(e) = gltf_uris_ok(&text) {
                    let _ = std::fs::remove_dir_all(&src);
                    return Err(refuse(400, "unsafe_zip", e));
                }
                (one, vec![src])
            }
            (Some(k), _) => {
                let dir = folder.join("in");
                std::fs::create_dir_all(&dir).map_err(io)?;
                let p = dir.join(format!("{id}.{}", k.ext()));
                std::fs::write(&p, &bytes).map_err(io)?;
                (p, vec![dir])
            }
        };
        std::fs::create_dir_all(&out).map_err(io)?;
        let spec = RunSpec { root: self.st.root.clone(), input: input_path, id: id.clone(), out, log: folder.join("convert.log") };
        let accepted = json!({ "job": job, "hash": hash, "id": id, "bytes": bytes.len() });
        let mut received = json!({ "job": job, "hash": hash, "stage": "received", "message": format!("Converting {id}…") });
        if let Some(n) = &note {
            received["note"] = json!(n);
        }
        self.st.emit("convert_progress", received);
        let st = self.st.clone();
        let runner = st.runner.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let limit = Duration::from_millis(st.limit_ms.load(Ordering::Relaxed));
        tokio::spawn(async move {
            let _slot = slot;
            st.emit("convert_progress", json!({ "job": job, "hash": hash, "stage": "converting", "message": format!("Running the converter on {id}") }));
            // On the limit the run future is dropped, and with it the
            // converter's whole process tree (`run_tree`).
            let run = tokio::time::timeout(limit, runner(spec)).await;
            let exit = match run {
                Ok(Ok(code)) => code,
                Ok(Err(e)) => {
                    let _ = std::fs::write(folder.join("convert.log"), &e);
                    2
                }
                Err(_) => {
                    use std::io::Write;
                    let log = std::fs::OpenOptions::new().create(true).append(true).open(folder.join("convert.log"));
                    if let Ok(mut f) = log {
                        let _ = writeln!(f, "\nstopped after the run limit ({} s)", limit.as_secs());
                    }
                    2
                }
            };
            for t in &temp {
                let _ = std::fs::remove_dir_all(t);
            }
            let mut done = finish(&st.dir, &job, &hash, &id, exit);
            if let Some(n) = note {
                done["note"] = json!(n);
            }
            let _ = std::fs::write(folder.join("job.json"), serde_json::to_vec_pretty(&done).unwrap_or_default());
            if done["files"].as_array().is_some_and(|f| !f.is_empty()) {
                *st.latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hash.clone());
            }
            st.emit("convert_done", done);
        });
        Ok(accepted)
    }

    /// `model.report {hash?}`: the job record and its report.json (the
    /// latest job when no hash is given).
    pub fn report(&self, hash: Option<&str>) -> Result<Value, Refusal> {
        let hash = match hash {
            Some(h) if is_hash(h) => h.to_string(),
            Some(h) => return Err(refuse(400, "bad_request", format!("hash {h:?} is not a convert hash"))),
            None => self
                .st
                .latest
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
                .ok_or_else(|| refuse(404, "no_report", "No conversion has finished yet."))?,
        };
        let folder = self.st.dir.join(&hash);
        let job: Value = std::fs::read(folder.join("job.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| refuse(404, "no_report", format!("No finished conversion {hash}.")))?;
        let id = job["id"].as_str().unwrap_or("");
        let report = read_json(&folder.join("out").join(format!("{id}.report.json")));
        Ok(json!({ "job": job, "report": report }))
    }

    /// `GET /live/v1/convert/<hash>/<file>`: a file under `<hash>/out/`.
    pub fn file(&self, rest: &str) -> Option<PathBuf> {
        let (hash, file) = rest.trim_start_matches('/').split_once('/')?;
        if !is_hash(hash) || file.is_empty() {
            return None;
        }
        crate::server::safe_join(&self.st.dir.join(hash).join("out"), file)
    }
}

/// Remove old conversions under `dir`: only folders named like a convert
/// hash, never those in `keep`; the newest [`KEEP_JOBS`] stay unless older
/// than [`KEEP_AGE`]. A folder's age is its `job.json`'s (else its own).
fn prune(dir: &Path, keep: &[&str], now: std::time::SystemTime) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut found: Vec<(std::time::SystemTime, PathBuf, String)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !is_hash(&name) {
                return None;
            }
            let p = e.path();
            let at = std::fs::metadata(p.join("job.json")).or_else(|_| e.metadata()).and_then(|m| m.modified()).ok()?;
            Some((at, p, name))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (i, (at, p, name)) in found.iter().enumerate() {
        if keep.contains(&name.as_str()) {
            continue;
        }
        let old = now.duration_since(*at).is_ok_and(|age| age > KEEP_AGE);
        if i >= KEEP_JOBS || old {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

fn read_json(p: &Path) -> Value {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

/// The `convert_done` record: files written, the report summary, URLs.
fn finish(dir: &Path, job: &str, hash: &str, id: &str, exit: i32) -> Value {
    let out = dir.join(hash).join("out");
    let mut files = Vec::new();
    list_files(&out, &out, &mut files);
    files.sort();
    let report = read_json(&out.join(format!("{id}.report.json")));
    let log_tail = std::fs::read_to_string(dir.join(hash).join("convert.log"))
        .map(|l| {
            let lines: Vec<&str> = l.lines().rev().take(12).collect();
            lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        })
        .unwrap_or_default();
    json!({
        "job": job, "hash": hash, "id": id, "exit": exit,
        "ok": exit == 0, "converted": exit <= 1 && report.is_object(),
        "base_url": format!("/live/v1/convert/{hash}/"),
        "master": files.iter().any(|f| *f == format!("{id}.master.glb")).then(|| format!("{id}.master.glb")),
        "files": files,
        "report": summary(&report),
        "log": log_tail,
        "at": crate::hub::now_ms(),
    })
}

fn list_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            list_files(base, &p, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            out.push(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"));
        }
    }
}

/// The report panel's numbers: per tier its file, sizes, draw calls,
/// triangles and gate scores; the package total; the notes.
pub fn summary(r: &Value) -> Value {
    if !r.is_object() {
        return Value::Null;
    }
    let gate_of = |tier: &str| {
        r["gate"]["tiers"].as_array().into_iter().flatten().find(|g| g["tier"] == tier).map(|g| {
            let f = &g["final"];
            json!({ "pass": f["pass"], "ssimulacra2": f["ssimulacra2"], "worst_pixel": f["worst_pixel"],
                    "worst_view": f["worst_view"], "sheet": g["sheet"], "bar": g["ssimulacra2_bar"] })
        })
    };
    let tiers: Vec<Value> = r["tiers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let tier = t["tier"].as_str().unwrap_or("");
            json!({ "tier": tier, "file": t["file"], "bytes": { "raw": t["bytes"]["raw"], "gzip": t["bytes"]["gzip"] },
                    "draw_calls": t["draw_calls"], "triangles": t["lods"][0]["triangles"], "gate": gate_of(tier) })
        })
        .collect();
    let p = &r["package"];
    json!({
        "id": r["id"], "master": r["master"], "tiers": tiers,
        "package": { "raw": p["raw"], "gzip": p["gzip"], "cap": p["cap"], "over_cap": p["over_cap"] },
        "gated": r["gate"].is_object(),
        "notes": r["notes"],
    })
}

/// The started converter's process tree. Dropped before the process exits
/// (the run limit, or the job dropped), it ends the whole tree: cargo,
/// evani-convert and the Blender or gate processes they started. On
/// Windows a kill-on-close job object also holds the tree, so it ends
/// even if the bridge itself is killed.
struct Tree {
    pid: Option<u32>,
    exited: bool,
    #[cfg(windows)]
    _job: Option<crate::code::job::Job>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        if !self.exited
            && let Some(pid) = self.pid
        {
            crate::code::kill_tree(pid);
        }
        // `_job` closes after this: whatever is still in it ends too.
    }
}

/// Run `cmd` (no shell) and return its exit code; if this future is
/// dropped first, the process and everything it started are ended.
pub(crate) async fn run_tree(cmd: &mut tokio::process::Command) -> Result<i32, String> {
    cmd.kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("evani-convert didn't start: {e}"))?;
    let mut tree = Tree {
        pid: child.id(),
        exited: false,
        #[cfg(windows)]
        _job: child.raw_handle().and_then(|h| crate::code::job::Job::adopt(h as _)),
    };
    let status = child.wait().await.map_err(|e| format!("evani-convert: {e}"))?;
    tree.exited = true;
    Ok(status.code().unwrap_or(2))
}

/// The real runner: `EVANI_CONVERT` (an evani-convert exe) when set, else
/// `cargo run --release --quiet -p evani-convert -- …` from the root. No
/// shell, below-normal priority; its whole process tree is ended if the
/// job is dropped or runs past the limit (`run_tree`).
fn default_runner() -> Runner {
    Arc::new(|spec: RunSpec| {
        Box::pin(async move {
            let args = [
                spec.input.to_string_lossy().into_owned(),
                "--id".into(),
                spec.id.clone(),
                "--out".into(),
                spec.out.to_string_lossy().into_owned(),
                "--gate".into(),
            ];
            let mut cmd = match std::env::var_os("EVANI_CONVERT") {
                Some(exe) => tokio::process::Command::new(exe),
                None => {
                    let mut c = tokio::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
                    c.args(["run", "--release", "--quiet", "-p", "evani-convert", "--"]);
                    c
                }
            };
            cmd.args(&args).current_dir(&spec.root).stdin(std::process::Stdio::null());
            let log = std::fs::File::create(&spec.log).map_err(|e| format!("{}: {e}", spec.log.display()))?;
            let log2 = log.try_clone().map_err(|e| format!("{}: {e}", spec.log.display()))?;
            cmd.stdout(log).stderr(log2);
            crate::game::below_normal(&mut cmd);
            run_tree(&mut cmd).await
        }) as RunFuture
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A stored-entry zip (no compression) of these files, names as given.
    pub fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in files {
            let mut c = flate2::Crc::new();
            c.update(data);
            let at = out.len() as u32;
            let n = name.as_bytes();
            let head = |sig: u32, central: bool| {
                let mut h = Vec::new();
                h.extend_from_slice(&sig.to_le_bytes());
                if central {
                    h.extend_from_slice(&20u16.to_le_bytes());
                }
                h.extend_from_slice(&20u16.to_le_bytes()); // version needed
                h.extend_from_slice(&0u16.to_le_bytes()); // flags
                h.extend_from_slice(&0u16.to_le_bytes()); // stored
                h.extend_from_slice(&[0, 0, 0, 0]); // time, date
                h.extend_from_slice(&c.sum().to_le_bytes());
                h.extend_from_slice(&(data.len() as u32).to_le_bytes());
                h.extend_from_slice(&(data.len() as u32).to_le_bytes());
                h.extend_from_slice(&(n.len() as u16).to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes()); // extra
                if central {
                    h.extend_from_slice(&[0; 6]); // comment, disk, internal attrs
                    h.extend_from_slice(&[0; 4]); // external attrs
                    h.extend_from_slice(&at.to_le_bytes());
                }
                h.extend_from_slice(n);
                h
            };
            out.extend(head(0x0403_4b50, false));
            out.extend_from_slice(data);
            cd.extend(head(0x0201_4b50, true));
        }
        let cd_at = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_at.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-convert-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sniffing_and_ids() {
        assert_eq!(Kind::sniff(b"glTF\x02\0\0\0"), Some(Kind::Glb));
        assert_eq!(Kind::sniff(b"PK\x03\x04...."), Some(Kind::Zip));
        assert_eq!(Kind::sniff(b"BLENDER-v300"), Some(Kind::Blend));
        assert_eq!(Kind::sniff(&[0x28, 0xB5, 0x2F, 0xFD, 0]), Some(Kind::Blend));
        assert_eq!(Kind::sniff(b"<html>"), None);
        assert!(valid_id("proof-crate") && valid_id("a_1"));
        assert!(!valid_id("Crate") && !valid_id("../x") && !valid_id("-x") && !valid_id("") && !valid_id(&"a".repeat(81)));
        assert_eq!(id_from("My Crate (2)"), "my_crate__2");
        assert_eq!(hash_of(b"x").len(), HASH_HEX);
        assert!(is_hash(&hash_of(b"x")) && !is_hash("../../x"));
    }

    #[test]
    fn zip_slip_entries_are_refused() {
        for evil in ["../evil.txt", "a/../../evil.txt", "/etc/evil", "C:/evil.txt", "C:evil.txt", "a\\..\\evil.txt", "a/./b", "a//b", "a\0b", ".."] {
            assert!(safe_entry(evil).is_err(), "{evil:?} must be refused");
            let d = temp("slip");
            let zip = zip_of(&[("model.gltf", b"{}"), (evil, b"pwned")]);
            let r = unzip(&zip, &d.join("src"), MAX_UNPACKED);
            assert!(r.is_err(), "{evil:?}: {r:?}");
            assert!(!d.join("evil.txt").exists() && !d.parent().unwrap().join("evil.txt").exists());
            let _ = std::fs::remove_dir_all(&d);
        }
        assert_eq!(safe_entry("textures/a b.png").unwrap(), (PathBuf::from("textures").join("a b.png"), false));
        assert!(safe_entry("textures/").unwrap().1);
        // Windows device names and parts Windows would rename.
        for evil in ["CON", "con.png", "tex/nul", "Nul.txt", "aux.tar.gz", "PRN", "com1", "COM9.bin", "lpt3", "a/LPT1.txt", "tex./a.png", "a.png.", "a ", "dir /x.png", "nul /x"] {
            assert!(safe_entry(evil).is_err(), "{evil:?} must be refused");
            let d = temp("reserved");
            let zip = zip_of(&[("model.gltf", b"{}"), (evil, b"pwned")]);
            assert!(unzip(&zip, &d.join("src"), MAX_UNPACKED).is_err(), "{evil:?}");
            let _ = std::fs::remove_dir_all(&d);
        }
        // Near misses stay allowed.
        for fine in ["console.png", "com10.bin", "com0.bin", "lpt.png", "nully/x.png", "auxiliary.bin", "tex/a.b.png", ".hidden"] {
            assert!(safe_entry(fine).is_ok(), "{fine:?} is a plain name");
        }
    }

    #[test]
    fn zip_reads_stored_and_deflated_and_caps_the_total() {
        let d = temp("zip");
        let zip = zip_of(&[("m/model.gltf", b"{\"asset\":{}}"), ("m/tex/a.png", b"png")]);
        let files = unzip(&zip, &d, MAX_UNPACKED).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(std::fs::read(d.join("m").join("tex").join("a.png")).unwrap(), b"png");
        // Over the cap: refused.
        assert!(unzip(&zip_of(&[("big.bin", &[7u8; 2048])]), &d, 1024).unwrap_err().contains("more than"));
        // A damaged CRC is refused.
        let mut bad = zip_of(&[("x.bin", b"hello")]);
        bad[30 + 5] = b'j';
        assert!(unzip(&bad, &d, MAX_UNPACKED).unwrap_err().contains("CRC"));
        // Deflate: hand-patch one entry to method 8 over a deflated body.
        use std::io::Write;
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&[b'z'; 4000]).unwrap();
        let packed = enc.finish().unwrap();
        let mut zip = zip_of(&[("z.bin", &packed)]);
        let mut c = flate2::Crc::new();
        c.update(&[b'z'; 4000]);
        let cd = u32_at(&zip, zip.len() - 6).unwrap() as usize;
        for h in [0usize, cd] {
            let (m, crc, usz) = if h == 0 { (8, 14, 22) } else { (10, 16, 24) };
            zip[h + m..h + m + 2].copy_from_slice(&8u16.to_le_bytes());
            zip[h + crc..h + crc + 4].copy_from_slice(&c.sum().to_le_bytes());
            zip[h + usz..h + usz + 4].copy_from_slice(&4000u32.to_le_bytes());
        }
        unzip(&zip, &d, MAX_UNPACKED).unwrap();
        assert_eq!(std::fs::read(d.join("z.bin")).unwrap().len(), 4000);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn gltf_uris_must_stay_inside() {
        assert!(gltf_uris_ok(br#"{"buffers":[{"uri":"model.bin"}],"images":[{"uri":"tex/a%20b.png"},{"uri":"data:image/png;base64,AA"}]}"#).is_ok());
        for uri in ["../secret.bin", "%2e%2e/secret.bin", "/etc/passwd", "C:/x.bin", "file:///x", "a\\..\\b", "http://evil/x"] {
            let g = format!(r#"{{"buffers":[{{"uri":{}}}]}}"#, Value::String(uri.into()));
            assert!(gltf_uris_ok(g.as_bytes()).is_err(), "{uri}");
        }
    }

    #[test]
    fn summary_reads_the_report_shape() {
        let r = json!({ "id": "kart", "master": "kart.master.glb",
            "tiers": [{ "tier": "low", "file": "kart.low.glb", "bytes": { "raw": 100, "gzip": 60 }, "draw_calls": 3, "lods": [{ "triangles": 900 }] }],
            "package": { "raw": 200, "gzip": 120, "cap": 10485760, "over_cap": false },
            "gate": { "tiers": [{ "tier": "low", "sheet": "gate/kart.low.sheet.png", "ssimulacra2_bar": 90,
                "final": { "pass": true, "ssimulacra2": 91.5, "worst_pixel": 0.1, "worst_view": "back" } }] },
            "notes": [] });
        let s = summary(&r);
        assert_eq!(s["tiers"][0]["gate"]["worst_view"], "back");
        assert_eq!(s["tiers"][0]["triangles"], 900);
        assert_eq!(s["package"]["gzip"], 120);
        assert_eq!(summary(&Value::Null), Value::Null);
    }

    #[test]
    fn old_conversions_are_pruned() {
        let dir = std::env::temp_dir().join(format!("evani-conv-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let now = std::time::SystemTime::now();
        // Twelve conversions, one hour apart (0 is the newest).
        let names: Vec<String> = (0..12u32).map(|i| format!("{i:032x}")).collect();
        for (i, n) in names.iter().enumerate() {
            let f = dir.join(n);
            std::fs::create_dir_all(f.join("out")).unwrap();
            std::fs::write(f.join("job.json"), b"{}").unwrap();
            let file = std::fs::File::options().write(true).open(f.join("job.json")).unwrap();
            file.set_modified(now - Duration::from_secs(3600 * i as u64)).unwrap();
        }
        // Not a convert folder: never touched.
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        // The newest 8 stay, 8 to 10 go, 11 is kept (the latest job's).
        prune(&dir, &[names[11].as_str()], now);
        for (i, n) in names.iter().enumerate() {
            assert_eq!(dir.join(n).exists(), i < KEEP_JOBS || i == 11, "{i}");
        }
        assert!(dir.join("notes").is_dir());
        // A week on, all but the kept one are too old.
        prune(&dir, &[names[0].as_str()], now + KEEP_AGE + Duration::from_secs(3600));
        for (i, n) in names.iter().enumerate() {
            assert_eq!(dir.join(n).exists(), i == 0, "{i}");
        }
        assert!(dir.join("notes").is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The run limit ends the converter and everything it started: here
    /// `cmd` running `ping`, which keeps writing to a file if only `cmd`
    /// is killed.
    #[cfg(windows)]
    #[tokio::test]
    async fn run_limit_ends_the_whole_process_tree() {
        let root = std::env::temp_dir().join(format!("evani-conv-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let conv = Converter::new(&root);
        conv.set_limit(Duration::from_secs(1));
        let ticks = root.join("ticks.txt");
        let t = ticks.clone();
        conv.set_runner(Arc::new(move |_spec: RunSpec| {
            let t = t.clone();
            Box::pin(async move {
                let out = std::fs::File::create(&t).map_err(|e| e.to_string())?;
                let mut cmd = tokio::process::Command::new("cmd");
                cmd.args(["/c", "ping", "-n", "30", "127.0.0.1"]).stdin(std::process::Stdio::null()).stdout(out);
                run_tree(&mut cmd).await
            }) as RunFuture
        }));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, Value)>();
        conv.set_sink(Arc::new(move |e: &str, d: Value| {
            let _ = tx.send((e.to_string(), d));
        }));
        let local = root.join("tree.glb");
        std::fs::write(&local, b"glTF\x02 tree").unwrap();
        let started = std::time::Instant::now();
        let accepted = conv.start_path(&local, None, Some("  tree test  ")).unwrap();
        let mut received_note = Value::Null;
        let done = loop {
            let (event, data) = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
            if data["stage"] == "received" {
                received_note = data["note"].clone();
            }
            if event == "convert_done" {
                break data;
            }
        };
        assert!(started.elapsed() < Duration::from_secs(10), "stopped at the limit, not after ping's 30 s");
        assert_eq!(done["exit"], 2, "{done}");
        assert_eq!(done["note"], "tree test", "the note is in the job record");
        assert_eq!(received_note, "tree test", "and in the first progress event");
        let folder = root.join(".evani").join("convert").join(accepted["hash"].as_str().unwrap());
        let log = std::fs::read_to_string(folder.join("convert.log")).unwrap();
        assert!(log.contains("stopped after the run limit"), "{log}");
        let job: Value = serde_json::from_slice(&std::fs::read(folder.join("job.json")).unwrap()).unwrap();
        assert_eq!(job["note"], "tree test");
        // ping wrote while it ran, and writes nothing once the job is done.
        let before = std::fs::metadata(&ticks).unwrap().len();
        assert!(before > 0, "ping ran");
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert_eq!(std::fs::metadata(&ticks).unwrap().len(), before, "ping was ended with cmd");
        assert!(conv.running().is_none(), "the slot is free");
        let _ = std::fs::remove_dir_all(&root);
    }
}
