//! The `ui` font step: bake the fonts a game lists in `evani.toml`
//!
//! ```toml
//! [ui]
//! fonts = [{ file = "engine/atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf", charsets = ["latin"], name = "ui" }]
//! ```
//!
//! into an MTSDF atlas (`fonts/<name>.png`, decoded by the browser, so no
//! decoder ships) and a compact table (`fonts/<name>.evf`: advances, plane
//! and atlas bounds, kerning pairs, metrics; the format is
//! `evani_render::msdf::MsdfFont::parse`), plus the font's licence as
//! `fonts/<name>.LICENSE.txt`. `file` is relative to the game's folder, or
//! `engine/<path>` for the engine's fonts in `assets/fonts/`. `name`
//! defaults to the file name; the kit asks for it with
//! `world.fonts.request(name)`.
//!
//! The atlas comes from the pinned msdf-atlas-gen (`cargo xtask doctor
//! --fix`; tools/evani-guide/src/fonttools.rs). Kerning comes from the
//! font's GPOS `kern` feature (and the old `kern` table), read with
//! ttf-parser, because msdf-atlas-gen reads only the old table. Results are
//! cached in `target/evani-fonts/<app>/` by a key over the font, the
//! charsets, the settings and the tool version, so a rebuild runs the tool
//! only when something changed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Atlas settings (DECISIONS "MSDF text"): MTSDF, 24 atlas px per em, a 6
/// px distance range (outline and glow reach up to 3 px of atlas, about
/// 0.12 em), power-of-two atlas, y down.
pub const SIZE: u32 = 24;
pub const PXRANGE: u32 = 6;
const SETTINGS: &str = "mtsdf size=24 pxrange=6 potr yorigin=top v1";
/// The runtime's limits (`evani_render::msdf::MAX_GLYPHS`, the cell's 10
/// glyph bits).
pub const MAX_GLYPHS: usize = 1024;
const EM_FIXED: f32 = 4096.0;

// The `[ui] fonts` list, the charsets and the licence lookup are shared with
// the check gate (evani-guide), so `cargo xtask check` refuses what this
// step would.
pub use evani_guide::fonttools::{codepoints, entries, font_path, licence_of};

// ---- the .evf table ----------------------------------------------------------------

/// One glyph as the table stores it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvfGlyph {
    pub code: u32,
    pub advance: f32,
    /// `[left, top, right, bottom]` em, y down.
    pub plane: [f32; 4],
    /// `[left, top, right, bottom]` atlas px, y down.
    pub atlas: [f32; 4],
}

/// What goes into a `.evf` table.
#[derive(Debug, Clone, PartialEq)]
pub struct EvfFont {
    pub pxrange: f32,
    pub em_px: f32,
    pub line_height: f32,
    pub ascender: f32,
    pub descender: f32,
    pub atlas: [u32; 2],
    pub glyphs: Vec<EvfGlyph>,
    /// `(left glyph index, right glyph index, em)`.
    pub kerning: Vec<(u16, u16, f32)>,
}

fn fixed(v: f32) -> Result<[u8; 2], String> {
    let f = (v * EM_FIXED).round();
    if !(f32::from(i16::MIN)..=f32::from(i16::MAX)).contains(&f) {
        return Err(format!("font table: {v} em does not fit the table (|v| < 8 em)"));
    }
    Ok((f as i16).to_le_bytes())
}

/// Write the table (the inverse of `MsdfFont::parse` in evani-render).
pub fn write_evf(f: &EvfFont) -> Result<Vec<u8>, String> {
    let mut o = b"EVF1".to_vec();
    let half = |v: f32| -> Result<[u8; 2], String> {
        let h = (v * 2.0).round();
        if (0.0..=f32::from(u16::MAX)).contains(&h) { Ok((h as u16).to_le_bytes()) } else { Err(format!("font table: atlas bound {v} out of range")) }
    };
    let count = |n: usize, what: &str| u16::try_from(n).map_err(|_| format!("font table: too many {what}"));
    o.extend(count(f.glyphs.len(), "glyphs")?.to_le_bytes());
    o.extend(count(f.kerning.len(), "kerning pairs")?.to_le_bytes());
    o.extend(count(f.atlas[0] as usize, "atlas columns")?.to_le_bytes());
    o.extend(count(f.atlas[1] as usize, "atlas rows")?.to_le_bytes());
    for v in [f.pxrange, f.em_px, f.line_height, f.ascender, f.descender] {
        o.extend(v.to_le_bytes());
    }
    for g in &f.glyphs {
        o.extend(g.code.to_le_bytes());
        o.extend(fixed(g.advance)?);
        for v in g.plane {
            o.extend(fixed(v)?);
        }
        for v in g.atlas {
            o.extend(half(v)?);
        }
    }
    for &(a, b, v) in &f.kerning {
        o.extend(a.to_le_bytes());
        o.extend(b.to_le_bytes());
        o.extend(fixed(v)?);
    }
    Ok(o)
}

fn num(v: &Value, path: &[&str]) -> Result<f32, String> {
    let mut at = v;
    for p in path {
        at = &at[*p];
    }
    at.as_f64().map(|f| f as f32).ok_or_else(|| format!("msdf-atlas-gen json: no number at {}", path.join(".")))
}

/// The table's glyphs and metrics from msdf-atlas-gen's json (`-yorigin
/// top`), keeping only the listed code points (`only`), sorted by code.
pub fn from_atlas_json(json: &str, only: &BTreeSet<u32>) -> Result<EvfFont, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("msdf-atlas-gen json: {e}"))?;
    if v["atlas"]["yOrigin"].as_str() != Some("top") || v["atlas"]["type"].as_str() != Some("mtsdf") {
        return Err("msdf-atlas-gen json: expected an mtsdf atlas with yOrigin top".into());
    }
    let mut glyphs = Vec::new();
    for g in v["glyphs"].as_array().ok_or("msdf-atlas-gen json: no glyphs")? {
        let code = g["unicode"].as_u64().and_then(|c| u32::try_from(c).ok()).ok_or("msdf-atlas-gen json: a glyph without unicode")?;
        if !only.contains(&code) {
            continue;
        }
        let bounds = |key: &str| -> Result<[f32; 4], String> {
            if g[key].is_null() {
                return Ok([0.0; 4]);
            }
            Ok([num(g, &[key, "left"])?, num(g, &[key, "top"])?, num(g, &[key, "right"])?, num(g, &[key, "bottom"])?])
        };
        glyphs.push(EvfGlyph { code, advance: num(g, &["advance"])?, plane: bounds("planeBounds")?, atlas: bounds("atlasBounds")? });
    }
    glyphs.sort_by_key(|g| g.code);
    glyphs.dedup_by_key(|g| g.code);
    if glyphs.len() > MAX_GLYPHS {
        return Err(format!("{} glyphs; a font holds at most {MAX_GLYPHS}\n  fix: list fewer charsets for this font", glyphs.len()));
    }
    if !glyphs.iter().any(|g| g.code == u32::from(b'?')) {
        return Err("the font has no `?` glyph (the missing-glyph mark)\n  fix: use a font with a question mark".into());
    }
    Ok(EvfFont {
        pxrange: num(&v, &["atlas", "distanceRange"])?,
        em_px: num(&v, &["atlas", "size"])?,
        line_height: num(&v, &["metrics", "lineHeight"])?,
        // With yOrigin top the ascender comes out negative.
        ascender: num(&v, &["metrics", "ascender"])?.abs(),
        descender: num(&v, &["metrics", "descender"])?.abs(),
        atlas: [num(&v, &["atlas", "width"])? as u32, num(&v, &["atlas", "height"])? as u32],
        glyphs,
        kerning: Vec::new(),
    })
}

// ---- kerning -----------------------------------------------------------------------

/// Horizontal kerning in em between every pair of the table's glyphs, from
/// the font's GPOS `kern` feature (pair adjustment lookups, both formats;
/// values from all its lookups add up, as shaping does), else from the
/// old `kern` table. Sorted, zero pairs left out.
pub fn kerning(font_bytes: &[u8], glyphs: &[EvfGlyph]) -> Result<Vec<(u16, u16, f32)>, String> {
    use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
    let face = ttf_parser::Face::parse(font_bytes, 0).map_err(|e| format!("font: {e}"))?;
    let upem = f32::from(face.units_per_em().max(1));
    let ids: Vec<Option<ttf_parser::GlyphId>> = glyphs.iter().map(|g| char::from_u32(g.code).and_then(|c| face.glyph_index(c))).collect();
    let mut pairs: Vec<(u16, u16, f32)> = Vec::new();
    let mut lookups: Vec<ttf_parser::opentype_layout::Lookup> = Vec::new();
    if let Some(gpos) = face.tables().gpos {
        let mut seen = BTreeSet::new();
        for feature in gpos.features {
            if feature.tag != ttf_parser::Tag::from_bytes(b"kern") {
                continue;
            }
            for i in feature.lookup_indices {
                if seen.insert(i)
                    && let Some(l) = gpos.lookups.get(i)
                {
                    lookups.push(l);
                }
            }
        }
    }
    let pair_value = |sub: &PairAdjustment, a: ttf_parser::GlyphId, b: ttf_parser::GlyphId| -> Option<i16> {
        match sub {
            PairAdjustment::Format1 { coverage, sets } => {
                let set = sets.get(coverage.get(a)?)?;
                set.get(b).map(|(v1, _)| v1.x_advance)
            }
            PairAdjustment::Format2 { coverage, classes, matrix } => {
                if !coverage.contains(a) {
                    return None;
                }
                matrix.get((classes.0.get(a), classes.1.get(b))).map(|(v1, _)| v1.x_advance)
            }
        }
    };
    for (ia, a) in ids.iter().enumerate() {
        let Some(a) = *a else { continue };
        for (ib, b) in ids.iter().enumerate() {
            let Some(b) = *b else { continue };
            let mut units = 0i32;
            for l in &lookups {
                // Within one lookup the first subtable that holds the pair applies.
                let hit = l
                    .subtables
                    .into_iter::<PositioningSubtable>()
                    .find_map(|s| if let PositioningSubtable::Pair(p) = s { pair_value(&p, a, b) } else { None });
                units += i32::from(hit.unwrap_or(0));
            }
            if lookups.is_empty()
                && let Some(kern) = face.tables().kern
            {
                units = kern
                    .subtables
                    .into_iter()
                    .filter(|s| s.horizontal && !s.variable)
                    .find_map(|s| s.glyphs_kerning(a, b))
                    .map_or(0, i32::from);
            }
            let em = units as f32 / upem;
            if (em * EM_FIXED).round() != 0.0 {
                let idx = |i: usize| u16::try_from(i).map_err(|_| "font table: too many glyphs".to_string());
                pairs.push((idx(ia)?, idx(ib)?, em));
            }
        }
    }
    Ok(pairs)
}

// ---- the PNG -----------------------------------------------------------------------

/// Re-encode the atlas at the best compression png offers (msdf-atlas-gen
/// writes a quick PNG; this is the bytes the game downloads).
pub fn recompress_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = dec.read_info().map_err(|e| format!("atlas png: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("atlas png: too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("atlas png: {e}"))?;
    let data = buf.get(..info.buffer_size()).ok_or("atlas png: short frame")?;
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, info.width, info.height);
        enc.set_color(info.color_type);
        enc.set_depth(info.bit_depth);
        enc.set_compression(png::Compression::High);
        enc.set_filter(png::Filter::Adaptive);
        let mut w = enc.write_header().map_err(|e| format!("atlas png: {e}"))?;
        w.write_image_data(data).map_err(|e| format!("atlas png: {e}"))?;
    }
    // Keep whichever is smaller (the tool's own output is sometimes).
    Ok(if out.len() < bytes.len() { out } else { bytes.to_vec() })
}

// ---- baking ------------------------------------------------------------------------

/// One baked font in the cache.
#[derive(Debug, Clone)]
pub struct Baked {
    pub name: String,
    pub png: PathBuf,
    pub evf: PathBuf,
    pub licence: PathBuf,
    pub glyphs: usize,
    pub kerning: usize,
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Bake every `[ui] fonts` entry of the game in `app_dir` into
/// `target/evani-fonts/<app>/`, running the pinned tool only for fonts whose
/// inputs changed.
pub fn bake(root: &Path, app: &str, app_dir: &Path) -> Result<Vec<Baked>, String> {
    bake_into(root, &root.join("target").join("evani-fonts"), app, app_dir)
}

/// [`bake`] with the cache in `<cache_root>/<app>/` (a game outside the
/// engine caches in its own workspace's `target/evani-fonts`).
pub fn bake_into(root: &Path, cache_root: &Path, app: &str, app_dir: &Path) -> Result<Vec<Baked>, String> {
    let toml_path = app_dir.join("evani.toml");
    let text = std::fs::read_to_string(&toml_path).map_err(|e| format!("{}: {e}", toml_path.display()))?;
    let cache = cache_root.join(app);
    let mut out = Vec::new();
    for e in entries(&text)? {
        let font = font_path(root, app_dir, &e.file);
        let bytes = std::fs::read(&font).map_err(|err| format!("{app}: font {} ({}): {err}", e.file, font.display()))?;
        let licence = licence_of(&font).ok_or_else(|| {
            format!(
                "{app}: font {} has no licence file next to it\n  fix: put its licence (for an SIL OFL font, OFL.txt) in {} as OFL.txt or {}.LICENSE.txt; only open-licence fonts may ship (DECISIONS \"MSDF text\")",
                e.file,
                font.parent().map_or_else(String::new, |p| p.display().to_string()),
                font.file_stem().and_then(|s| s.to_str()).unwrap_or("<font>")
            )
        })?;
        let cps = codepoints(&e.charsets)?;
        if cps.len() > MAX_GLYPHS {
            return Err(format!("{app}: font {}: {} characters listed; a font holds at most {MAX_GLYPHS}\n  fix: list fewer charsets", e.name, cps.len()));
        }
        let key = sha_hex(
            format!(
                "{}|{}|{}|{}|{}",
                sha_hex(&bytes),
                cps.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
                SETTINGS,
                evani_guide::fonttools::VERSION,
                env!("CARGO_PKG_VERSION")
            )
            .as_bytes(),
        );
        let (png_out, evf_out, key_out) = (cache.join(format!("{}.png", e.name)), cache.join(format!("{}.evf", e.name)), cache.join(format!("{}.key", e.name)));
        let fresh = std::fs::read_to_string(&key_out).is_ok_and(|k| k.trim() == key) && png_out.is_file() && evf_out.is_file();
        if !fresh {
            let tool = evani_guide::fonttools::locate(root).map_err(|err| format!("{app}: font {} needs the font tool: {err}", e.name))?;
            std::fs::create_dir_all(&cache).map_err(|err| format!("{}: {err}", cache.display()))?;
            let cs = cache.join(format!("{}.charset.txt", e.name));
            let cs_text = cps.iter().map(|c| format!("0x{c:X}")).collect::<Vec<_>>().join(", ");
            std::fs::write(&cs, cs_text).map_err(|err| format!("{}: {err}", cs.display()))?;
            let (raw_png, json) = (cache.join(format!("{}.raw.png", e.name)), cache.join(format!("{}.json", e.name)));
            let size = SIZE.to_string();
            let range = PXRANGE.to_string();
            let run = Command::new(&tool)
                .arg("-font")
                .arg(&font)
                .arg("-charset")
                .arg(&cs)
                .args(["-type", "mtsdf", "-format", "png", "-size", &size, "-pxrange", &range, "-potr", "-yorigin", "top", "-threads", "2"])
                .arg("-imageout")
                .arg(&raw_png)
                .arg("-json")
                .arg(&json)
                .output()
                .map_err(|err| format!("cannot run {}: {err}", tool.display()))?;
            if !run.status.success() {
                return Err(format!("{app}: msdf-atlas-gen failed on {}: {}", e.file, String::from_utf8_lossy(&run.stderr).trim()));
            }
            let json_text = std::fs::read_to_string(&json).map_err(|err| format!("{}: {err}", json.display()))?;
            let mut table = from_atlas_json(&json_text, &cps).map_err(|m| format!("{app}: font {}: {m}", e.name))?;
            table.kerning = kerning(&bytes, &table.glyphs).map_err(|m| format!("{app}: font {}: {m}", e.name))?;
            let raw = std::fs::read(&raw_png).map_err(|err| format!("{}: {err}", raw_png.display()))?;
            std::fs::write(&png_out, recompress_png(&raw)?).map_err(|err| format!("{}: {err}", png_out.display()))?;
            std::fs::write(&evf_out, write_evf(&table)?).map_err(|err| format!("{}: {err}", evf_out.display()))?;
            std::fs::write(&key_out, &key).map_err(|err| format!("{}: {err}", key_out.display()))?;
        }
        let evf = std::fs::read(&evf_out).map_err(|err| format!("{}: {err}", evf_out.display()))?;
        let count = |at: usize| evf.get(at..at + 2).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])));
        out.push(Baked { name: e.name, png: png_out, evf: evf_out, licence, glyphs: count(4), kerning: count(6) });
    }
    Ok(out)
}

/// Copy baked fonts into a build folder's `fonts/`. Returns the bytes copied.
pub fn copy_to(baked: &[Baked], dist: &Path) -> Result<u64, String> {
    if baked.is_empty() {
        return Ok(0);
    }
    let dir = dist.join("fonts");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut total = 0;
    for b in baked {
        for (src, ext) in [(&b.png, "png"), (&b.evf, "evf"), (&b.licence, "LICENSE.txt")] {
            let to = dir.join(format!("{}.{ext}", b.name));
            total += std::fs::copy(src, &to).map_err(|e| format!("{} -> {}: {e}", src.display(), to.display()))?;
        }
    }
    Ok(total)
}

pub fn usage() -> &'static str {
    "  cargo xtask fonts <app> [--dist <dir>]           bake the game's `[ui] fonts` (MSDF atlas + table, cached); --dist copies them into <dir>/fonts"
}

/// `cargo xtask fonts <app> [--dist <dir>]`.
pub fn cmd_fonts(args: &[String], root: &Path) -> Result<(), String> {
    let app = args.first().filter(|a| !a.starts_with("--")).ok_or_else(|| format!("usage:\n{}", usage()))?;
    let dist = match args.iter().position(|a| a == "--dist") {
        None => None,
        Some(i) => Some(PathBuf::from(args.get(i + 1).ok_or("--dist needs a folder")?)),
    };
    let app_dir = root.join("apps").join(app);
    let baked = bake(root, app, &app_dir)?;
    if baked.is_empty() {
        println!("{app}: no `[ui] fonts` in evani.toml; nothing to bake");
        return Ok(());
    }
    for b in &baked {
        let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        println!(
            "{app}: font {}: {} glyphs, {} kerning pairs, atlas {:.1} KB, table {:.1} KB",
            b.name,
            b.glyphs,
            b.kerning,
            size(&b.png) as f64 / 1024.0,
            size(&b.evf) as f64 / 1024.0
        );
    }
    if let Some(d) = dist {
        let n = copy_to(&baked, &d)?;
        println!("{app}: copied {:.1} KB of fonts into {}", n as f64 / 1024.0, d.join("fonts").display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(glyphs: &[(u32, bool)]) -> String {
        let gs: Vec<String> = glyphs
            .iter()
            .map(|&(c, ink)| {
                if ink {
                    format!(r#"{{"unicode":{c},"advance":0.5,"planeBounds":{{"left":-0.1,"top":-0.8,"right":0.6,"bottom":0.1}},"atlasBounds":{{"left":0.5,"top":1.5,"right":17.5,"bottom":23.5}}}}"#)
                } else {
                    format!(r#"{{"unicode":{c},"advance":0.25}}"#)
                }
            })
            .collect();
        format!(
            r#"{{"atlas":{{"type":"mtsdf","distanceRange":6,"size":24,"width":256,"height":128,"yOrigin":"top"}},"metrics":{{"emSize":1,"lineHeight":1.24,"ascender":-0.95,"descender":0.29}},"glyphs":[{}],"kerning":[]}}"#,
            gs.join(",")
        )
    }

    #[test]
    fn only_listed_charsets_are_baked() {
        let ascii = codepoints(&["ascii".into()]).unwrap();
        assert!(ascii.contains(&('A' as u32)) && ascii.contains(&('~' as u32)));
        assert!(!ascii.contains(&('é' as u32)) && !ascii.contains(&0x2019));
        assert_eq!(ascii.len(), 95);
        let latin = codepoints(&["latin".into()]).unwrap();
        assert!(latin.contains(&0x2019) && latin.contains(&0x20AC) && !latin.contains(&('é' as u32)));
        let both = codepoints(&["latin".into(), "latin1".into()]).unwrap();
        assert!(both.contains(&('é' as u32)) && both.contains(&('ÿ' as u32)) && !both.contains(&0x0100));
        // Space and `?` are always there, whatever is listed.
        let l1 = codepoints(&["latin1".into()]).unwrap();
        assert!(l1.contains(&0x20) && l1.contains(&('?' as u32)) && !l1.contains(&('A' as u32)));
        assert!(evani_guide::fonttools::charset("cyrillic").unwrap_err().contains("fix:"));
        // A glyph the atlas holds but the list doesn't name stays out of the table.
        let only = codepoints(&["ascii".into()]).unwrap();
        let t = from_atlas_json(&json(&[(32, false), (63, true), (65, true), (233, true), (0x4E2D, true)]), &only).unwrap();
        assert_eq!(t.glyphs.iter().map(|g| g.code).collect::<Vec<_>>(), [32, 63, 65]);
        assert_eq!((t.ascender, t.descender, t.em_px, t.pxrange, t.atlas), (0.95, 0.29, 24.0, 6.0, [256, 128]));
        assert_eq!(t.glyphs[0].plane, [0.0; 4], "space has no ink");
        assert_eq!(t.glyphs[1].plane, [-0.1, -0.8, 0.6, 0.1]);
        // No `?` is refused, as is a list past the glyph limit.
        assert!(from_atlas_json(&json(&[(65, true)]), &only).unwrap_err().contains('?'));
        let many: Vec<(u32, bool)> = (0..1100).map(|i| (if i == 0 { 63 } else { 0x4E00 + i }, true)).collect();
        let all: BTreeSet<u32> = many.iter().map(|g| g.0).collect();
        assert!(from_atlas_json(&json(&many), &all).unwrap_err().contains("1024"));
    }

    #[test]
    fn the_engine_font_has_its_licence_and_kerning() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf();
        let font = root.join("assets/fonts/atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf");
        let lic = licence_of(&font).expect("the engine font ships its OFL");
        assert!(std::fs::read_to_string(lic).unwrap().contains("SIL OPEN FONT LICENSE"));
        // GPOS pair kerning: "AV" and "To" pull together in this font.
        let bytes = std::fs::read(&font).unwrap();
        let g = |c: char| EvfGlyph { code: c as u32, advance: 0.5, plane: [0.0; 4], atlas: [0.0; 4] };
        let glyphs = [g('A'), g('T'), g('V'), g('o')];
        let k = kerning(&bytes, &glyphs).unwrap();
        let get = |a: u16, b: u16| k.iter().find(|p| p.0 == a && p.1 == b).map_or(0.0, |p| p.2);
        assert!(get(0, 2) < 0.0, "AV kerns: {k:?}");
        assert!(get(1, 3) < 0.0, "To kerns: {k:?}");
        assert!(k.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)), "sorted");
    }

    #[test]
    fn evf_writer_matches_the_runtime_layout() {
        let t = EvfFont {
            pxrange: 6.0,
            em_px: 24.0,
            line_height: 1.25,
            ascender: 0.95,
            descender: 0.3,
            atlas: [256, 128],
            glyphs: vec![EvfGlyph { code: 63, advance: 0.5, plane: [0.0, -0.75, 0.5, 0.25], atlas: [0.5, 1.5, 12.5, 25.5] }],
            kerning: vec![(0, 0, -0.1)],
        };
        let b = write_evf(&t).unwrap();
        // Header 4 + 4 counts x 2 + 5 floats x 4, glyph 4 + 2 + 8 + 8, pair 6.
        assert_eq!(b.len(), 4 + 8 + 20 + 22 + 6);
        assert_eq!(&b[..4], b"EVF1");
        assert_eq!(u16::from_le_bytes([b[4], b[5]]), 1);
        assert!(write_evf(&EvfFont { kerning: vec![(0, 0, 9.0)], ..t }).is_err(), "past 8 em");
    }
}
