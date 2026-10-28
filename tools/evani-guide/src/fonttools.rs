//! The pinned font-atlas program `cargo xtask fonts` (and the `ui` build
//! step) drives: msdf-atlas-gen 1.4 (MIT, Viktor Chlumsky), a separate
//! program, never linked (STAGE3_PLAN decision 1). `cargo xtask doctor
//! --fix` downloads the exact release zip, checks its SHA-256 before
//! unpacking, installs it into the gitignored `.evani/bin/` and records
//! version and checksums in `.evani/bin/font-tools.json`; the font step
//! refuses a missing or changed program with an error naming that fix.
//! Modelled on S3-5's model-converter tools (`extools.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Where the program lives (gitignored with `.evani/`).
pub const BIN_DIR: &str = ".evani/bin";
/// The install record: version, source, download and program checksums.
pub const RECORD: &str = "font-tools.json";
/// The one line every "tool missing" error ends with.
pub const FIX: &str = "fix: run `cargo xtask doctor --fix` (installs the pinned msdf-atlas-gen into .evani/bin)";

/// The pin (Windows x64, the platform this engine is developed on).
pub const NAME: &str = "msdf-atlas-gen";
pub const VERSION: &str = "1.4";
pub const LICENCE: &str = "MIT";
pub const URL: &str = "https://github.com/Chlumsky/msdf-atlas-gen/releases/download/v1.4/msdf-atlas-gen-1.4-win64.zip";
pub const SHA256: &str = "55ab2bef57b9c7bf305b610989fa8eeda812d85bc93e632b8c39ce05b6e64e9d";
/// Install folder under `.evani/bin/`, and the program inside it (the zip
/// holds one folder with one exe, no DLLs; docs/stage3/spikes.md (f)).
pub const DIR: &str = "msdf-atlas-gen-1.4";
pub const EXE: &str = "msdf-atlas-gen/msdf-atlas-gen.exe";

pub fn exe_path(root: &Path) -> PathBuf {
    root.join(BIN_DIR).join(DIR).join(EXE)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_file(p: &Path) -> Result<String, String> {
    std::fs::read(p).map(|b| sha256_hex(&b)).map_err(|e| format!("{}: {e}", p.display()))
}

fn read_record(root: &Path) -> Value {
    std::fs::read_to_string(root.join(BIN_DIR).join(RECORD))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

/// `Ok(())` when the pinned program is installed and unchanged, else why not.
pub fn state(root: &Path) -> Result<(), String> {
    let exe = exe_path(root);
    if !exe.is_file() {
        return Err(format!("{NAME} {VERSION} is not installed ({} missing)", exe.display()));
    }
    let rec = read_record(root);
    let r = &rec[NAME];
    if r["version"].as_str() != Some(VERSION) {
        return Err(format!("the installed {NAME} is {}, not the pinned {VERSION}", r["version"].as_str().unwrap_or("(unrecorded)")));
    }
    match (sha256_file(&exe), r["exe_sha256"].as_str()) {
        (Ok(h), Some(want)) if h == want => Ok(()),
        (Ok(_), Some(_)) => Err(format!("{NAME} in {BIN_DIR} differs from the one installed (checksum)")),
        _ => Err(format!("{NAME} in {BIN_DIR} has no recorded checksum")),
    }
}

/// The program to run, or an error naming the fix.
pub fn locate(root: &Path) -> Result<PathBuf, String> {
    state(root).map(|()| exe_path(root)).map_err(|e| format!("{e}\n{FIX}"))
}

/// The `doctor` note when the program isn't current (only games with
/// `[ui] fonts` need it, so it is never a doctor failure).
pub fn line(root: &Path) -> Option<String> {
    state(root).err().map(|e| format!("{BIN_DIR}/{DIR}: font tool: {e} (needed only by games with `[ui] fonts`)"))
}

fn system32(exe: &str) -> PathBuf {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    PathBuf::from(sys).join("System32").join(exe)
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Install the pin when it isn't current. `Ok(true)` when it installed.
pub fn install(root: &Path) -> Result<bool, String> {
    if state(root).is_ok() {
        return Ok(false);
    }
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(format!(
            "{NAME}: no pinned download for this platform yet (only Windows x64); build {NAME} {VERSION} yourself into {}",
            root.join(BIN_DIR).join(DIR).display()
        ));
    }
    let bin = root.join(BIN_DIR);
    let dl = bin.join("downloads");
    std::fs::create_dir_all(&dl).map_err(|e| e.to_string())?;
    let file = dl.join("msdf-atlas-gen-1.4-win64.zip");
    if !(file.is_file() && sha256_file(&file)? == SHA256) {
        run(Command::new(system32("curl.exe")).args(["-sSfL", "-o"]).arg(&file).arg(URL), &format!("download {URL}"))?;
        let got = sha256_file(&file)?;
        if got != SHA256 {
            let _ = std::fs::remove_file(&file);
            return Err(format!("{URL}: checksum {got} is not the pinned {SHA256}; nothing was installed"));
        }
    }
    let dir = bin.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    run(Command::new(system32("tar.exe")).arg("-xf").arg(&file).arg("-C").arg(&dir), &format!("unpack {}", file.display()))?;
    let exe = exe_path(root);
    if !exe.is_file() {
        return Err(format!("{NAME}: unpacked, but {} is not there", exe.display()));
    }
    let mut rec = read_record(root);
    rec[NAME] = json!({
        "version": VERSION,
        "licence": LICENCE,
        "source": URL,
        "download_sha256": SHA256,
        "exe": format!("{DIR}/{EXE}"),
        "exe_sha256": sha256_file(&exe)?,
    });
    let text = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())?;
    crate::write_file(&bin.join(RECORD), &format!("{text}\n"))?;
    Ok(true)
}

// ---- the `[ui] fonts` list (shared by the font step and the check gate) ----------

/// The named character sets a font entry can list.
pub const CHARSETS: &[(&str, &str)] = &[
    ("ascii", "printable ASCII, U+0020 to U+007E"),
    ("latin", "ASCII plus typographic quotes, dashes, ellipsis, bullet, euro, degree, copyright and trade mark"),
    ("latin1", "the Latin-1 supplement, U+00A0 to U+00FF (accented letters)"),
];

/// The code points of one named charset.
pub fn charset(name: &str) -> Result<Vec<u32>, String> {
    let ascii = || (0x20..=0x7E).collect::<Vec<u32>>();
    match name {
        "ascii" => Ok(ascii()),
        "latin" => {
            let mut v = ascii();
            v.extend([0x2018, 0x2019, 0x201C, 0x201D, 0x2013, 0x2014, 0x2026, 0x2022, 0x20AC, 0x00B0, 0x00A9, 0x2122]);
            Ok(v)
        }
        "latin1" => Ok((0xA0..=0xFF).collect()),
        _ => Err(format!(
            "unknown charset `{name}`\n  fix: use one of {}",
            CHARSETS.iter().map(|(n, d)| format!("`{n}` ({d})")).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Every code point the listed charsets name, plus space and `?` (the
/// missing-glyph mark), sorted.
pub fn codepoints(charsets: &[String]) -> Result<std::collections::BTreeSet<u32>, String> {
    let mut set: std::collections::BTreeSet<u32> = [0x20, u32::from(b'?')].into_iter().collect();
    for c in charsets {
        set.extend(charset(c)?);
    }
    Ok(set)
}

/// One `[ui] fonts` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEntry {
    pub file: String,
    pub name: String,
    pub charsets: Vec<String>,
}

/// At most this many fonts per game (`evani_render::msdf::MAX_FONTS`).
pub const MAX_FONTS: usize = 4;

/// The `[ui] fonts` list of an `evani.toml` (empty when there is none):
/// `fonts = [{ file, charsets = ["latin"], name }]`; `charsets` defaults to
/// `["latin"]`, `name` to the file name in lowercase with `-`.
pub fn entries(evani_toml: &str) -> Result<Vec<FontEntry>, String> {
    let t: toml::Table = evani_toml.parse().map_err(|e| format!("evani.toml: {e}"))?;
    let Some(list) = t.get("ui").and_then(|u| u.get("fonts")) else {
        return Ok(Vec::new());
    };
    let list = list.as_array().ok_or("evani.toml: `ui.fonts` must be a list of { file, charsets, name }")?;
    let mut out: Vec<FontEntry> = Vec::new();
    for e in list {
        let file = e.get("file").and_then(|v| v.as_str()).ok_or("evani.toml: every `ui.fonts` entry needs `file` (a .ttf or .otf)")?;
        let charsets = match e.get("charsets") {
            None => vec!["latin".to_string()],
            Some(v) => v
                .as_array()
                .ok_or("evani.toml: `charsets` must be a list of names")?
                .iter()
                .map(|c| c.as_str().map(str::to_string).ok_or_else(|| "evani.toml: every charset must be a string".to_string()))
                .collect::<Result<Vec<_>, _>>()?,
        };
        for c in &charsets {
            charset(c).map_err(|m| format!("evani.toml `ui.fonts` {file}: {m}"))?;
        }
        let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or("font");
        let name = match e.get("name") {
            Some(v) => v.as_str().ok_or("evani.toml: a font's `name` must be a string")?.to_string(),
            None => slug(stem),
        };
        if name.is_empty() || name != slug(&name) {
            return Err(format!("evani.toml `ui.fonts`: name `{name}` must be lowercase letters, digits and `-`"));
        }
        if out.iter().any(|o| o.name == name) {
            return Err(format!("evani.toml `ui.fonts`: two fonts are named `{name}`\n  fix: give one a different `name`"));
        }
        out.push(FontEntry { file: file.to_string(), name, charsets });
    }
    if out.len() > MAX_FONTS {
        return Err(format!("evani.toml `ui.fonts`: at most {MAX_FONTS} fonts"));
    }
    Ok(out)
}

fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            o.push(c);
        } else if !o.ends_with('-') && !o.is_empty() {
            o.push('-');
        }
    }
    o.trim_end_matches('-').to_string()
}

/// The font file of an entry: `engine/<path>` is the engine's
/// `assets/fonts/<path>`, anything else is relative to the game's folder.
pub fn font_path(root: &Path, app_dir: &Path, file: &str) -> PathBuf {
    match file.strip_prefix("engine/") {
        Some(rest) => root.join("assets").join("fonts").join(rest),
        None => app_dir.join(file),
    }
}

/// The licence file next to a font: `<stem>.LICENSE.txt`, `OFL.txt`,
/// `OFL-1.1.txt`, `LICENSE.txt` or `LICENSE` in its folder.
pub fn licence_of(font: &Path) -> Option<PathBuf> {
    let dir = font.parent()?;
    let stem = font.file_stem()?.to_str()?;
    [format!("{stem}.LICENSE.txt"), "OFL.txt".into(), "OFL-1.1.txt".into(), "LICENSE.txt".into(), "LICENSE".into()]
        .into_iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// The check gate's font rules for one game: `(is_error, what, fix)`. A
/// game with `ui` must list fonts that parse, exist and carry their licence
/// (the build would fail later otherwise); fonts listed without `ui` are a
/// note (they are not shipped).
pub fn list_problems(root: &Path, app: &str, ui: bool, evani_toml: &str) -> Vec<(bool, String, String)> {
    list_problems_in(root, &root.join("apps").join(app), &format!("apps/{app}"), ui, evani_toml)
}

/// [`list_problems`] for the game in `dir` (a game outside the engine too),
/// named `shown` in messages; `engine/<path>` fonts are the engine's at `root`.
pub fn list_problems_in(root: &Path, dir: &Path, shown: &str, ui: bool, evani_toml: &str) -> Vec<(bool, String, String)> {
    let list = match entries(evani_toml) {
        Ok(l) => l,
        Err(e) => {
            let (what, fix) = e.split_once("\n  fix: ").map_or((e.as_str(), "Correct the `[ui] fonts` list (README \"Text\")."), |(w, f)| (w, f));
            return vec![(true, what.to_string(), fix.to_string())];
        }
    };
    if list.is_empty() {
        return Vec::new();
    }
    if !ui {
        return vec![(
            false,
            format!("evani.toml lists {} font(s) under [ui] but not the `ui` module, so they are not baked or shipped", list.len()),
            format!("Nothing to fix unless the game should draw them: then add `ui = true` under [modules] in {shown}/evani.toml."),
        )];
    }
    let mut out = Vec::new();
    for e in &list {
        let font = font_path(root, dir, &e.file);
        if !font.is_file() {
            out.push((true, format!("the `[ui] fonts` entry `{}` names {}, which does not exist", e.name, font.display()), "Fix the `file` path: relative to the game's folder, or `engine/<path>` for assets/fonts/.".to_string()));
        } else if licence_of(&font).is_none() {
            out.push((
                true,
                format!("the font {} has no licence file next to it, so it may not ship", e.file),
                "Put the font's licence beside it as OFL.txt or <font>.LICENSE.txt (only open-licence fonts, e.g. SIL OFL, ship; DECISIONS \"MSDF text\").".to_string(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_entries_parse_with_defaults_and_refuse_bad_ones() {
        let e = entries("name = \"x\"\n[ui]\nfonts = [{ file = \"engine/a/My Font-Regular.ttf\" }, { file = \"f.otf\", charsets = [\"ascii\", \"latin1\"], name = \"title\" }]").unwrap();
        assert_eq!(e[0], FontEntry { file: "engine/a/My Font-Regular.ttf".into(), name: "my-font-regular".into(), charsets: vec!["latin".into()] });
        assert_eq!(e[1].name, "title");
        assert!(entries("name = \"x\"").unwrap().is_empty());
        assert!(entries("[ui]\nfonts = [{ file = \"a.ttf\", charsets = [\"klingon\"] }]").unwrap_err().contains("klingon"));
        assert!(entries("[ui]\nfonts = [{ file = \"a.ttf\" }, { file = \"b/a.ttf\" }]").unwrap_err().contains("two fonts"));
        assert!(entries("[ui]\nfonts = [{ file = \"a.ttf\", name = \"Big Title\" }]").is_err());
        let root = Path::new("/r");
        assert_eq!(font_path(root, &root.join("apps/g"), "engine/x/a.ttf"), root.join("assets").join("fonts").join("x/a.ttf"));
        assert_eq!(font_path(root, &root.join("apps/g"), "fonts/a.ttf"), root.join("apps/g").join("fonts/a.ttf"));
    }

    #[test]
    fn the_check_refuses_unlicensed_or_missing_fonts_and_notes_fonts_without_ui() {
        let r = crate::testutil::temp_root("fontlist");
        let toml = "[ui]\nfonts = [{ file = \"fonts/a.ttf\" }]";
        let p = list_problems(&r, "g", true, toml);
        assert!(p.len() == 1 && p[0].0 && p[0].1.contains("does not exist"), "{p:?}");
        crate::write_file(&r.join("apps/g/fonts/a.ttf"), "font").unwrap();
        let p = list_problems(&r, "g", true, toml);
        assert!(p.len() == 1 && p[0].0 && p[0].1.contains("no licence"), "{p:?}");
        crate::write_file(&r.join("apps/g/fonts/OFL.txt"), "SIL OPEN FONT LICENSE").unwrap();
        assert!(list_problems(&r, "g", true, toml).is_empty());
        let p = list_problems(&r, "g", false, toml);
        assert!(p.len() == 1 && !p[0].0, "a note without ui: {p:?}");
        let p = list_problems(&r, "g", true, "[ui]\nfonts = [{ file = \"a.ttf\", charsets = [\"x\"] }]");
        assert!(p[0].0 && p[0].2.contains("`ascii`"), "{p:?}");
        assert!(list_problems(&r, "g", true, "name = \"g\"").is_empty());
    }

    #[test]
    fn missing_or_changed_font_tool_names_the_fix() {
        let r = crate::testutil::temp_root("fonttools");
        let e = locate(&r).unwrap_err();
        assert!(e.contains("msdf-atlas-gen 1.4 is not installed") && e.contains("cargo xtask doctor --fix"), "{e}");
        assert!(line(&r).is_some());
        crate::write_file(&exe_path(&r), "a program").unwrap();
        assert!(state(&r).is_err(), "no record");
        let rec = json!({ NAME: { "version": VERSION, "exe_sha256": sha256_hex(b"a program") } });
        crate::write_file(&r.join(BIN_DIR).join(RECORD), &rec.to_string()).unwrap();
        assert_eq!(locate(&r).unwrap(), exe_path(&r));
        assert!(line(&r).is_none());
        crate::write_file(&exe_path(&r), "another program").unwrap();
        assert!(locate(&r).unwrap_err().contains("checksum"));
    }

    #[test]
    fn the_download_is_pinned_by_checksum() {
        assert_eq!(SHA256.len(), 64);
        assert!(URL.contains(VERSION) && DIR.contains(VERSION));
    }
}
