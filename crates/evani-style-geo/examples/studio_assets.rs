//! The studio's asset tool (WP2, docs/studio/WORK_PACKAGES.md), dev only:
//!
//! - `tier <asset> <in.glb> <out.glb>`: an asset's studio source tier from a
//!   model tier (rest pose, unskinned, no clips; the regions from
//!   `catalog/assets/source_rules.json` baked in as one primitive per region
//!   with the `_EVANI_PART` byte), in the file's units. Then
//!   `cargo xtask convert <out.glb> --id <model> --tiers low --out apps/material-view/assets/models`.
//! - `picture <asset> <in.glb> <out.png> [px]`: the source in its region
//!   colours from the front, the side and three-quarter (a check picture).
//! - `thumb <asset> <out.png> [px]`: the asset browser's thumbnail (default
//!   128 px, transparent background), from the asset's own source as the app
//!   loads it, at the row's `frame` angle, 2x2 supersampled; written to
//!   `apps/material-view/assets/models/thumbs/<asset>.png`.
//! - `code <asset> <out.glb>`: a code-built asset (wall, roof, ground patch,
//!   test shapes) as a model file in metres, one primitive per region, for
//!   `cargo xtask convert` (the proof that the code sources pass the engine's
//!   model pipeline; tests/fixtures/models/studio/).
//!
//! cargo run -p evani-style-geo --example studio_assets -- <command> ...

use std::path::Path;

use evani_studio::Catalogs;
use evani_style_geo::source::{self, SourceRules};
use evani_style_geo::view::{Frame, Soup, VIEWS, picture, silhouette};

/// Region colours for pictures, thumbnails and the tier's plain materials:
/// the horse a bay (as standin.rs), the proof props their default
/// materials, anything else (the cow, the dog, every later asset) from its
/// own materials ([`evani_style_geo::regioncolour`]), never a debug palette.
fn colours(asset: &evani_studio::assets::AssetRow) -> Result<Vec<[f32; 3]>, String> {
    if asset.id == "animal.horse" {
        return Ok(evani_style_geo::standin::HORSE_COLOURS.to_vec());
    }
    // The proof props in their default materials' colours (thumbnails).
    match asset.id.as_str() {
        "building.wall" => return Ok(vec![[0.56, 0.24, 0.17], [0.78, 0.74, 0.66], [0.56, 0.24, 0.17]]),
        "building.roof" => return Ok(vec![[0.62, 0.29, 0.19], [0.66, 0.36, 0.25], [0.56, 0.24, 0.17], [0.45, 0.3, 0.18]]),
        "land.patch" => return Ok(vec![[0.32, 0.5, 0.2]]),
        "shape.sphere" | "shape.cube" => return Ok(vec![[0.74, 0.74, 0.76]]),
        _ => {}
    }
    let p = root().join("catalog/materials/materials.json");
    let json = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    evani_style_geo::regioncolour::region_colours(asset, &json)
}

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A PNG (`channels` 3: RGB, 4: RGBA), deflated (flate2, already in the
/// workspace).
fn png(w: usize, h: usize, channels: usize, px: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity(h * (w * channels + 1));
    for row in px.chunks(w * channels) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&raw).map_err(|e| e.to_string())?;
    let z = z.finish().map_err(|e| e.to_string())?;
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// The thumbnail: `src` at the row's frame angle, `px` square, RGBA with a
/// clear background, drawn at twice the size and averaged down.
fn thumbnail(src: &evani_style_geo::SourceMesh, yaw: f32, pitch: f32, cols: &[[f32; 3]], px: usize) -> Vec<u8> {
    let soup = Soup::from_source(src);
    let big = px * 2;
    let view = evani_style_geo::view::View { name: "thumb", yaw, pitch };
    let f = Frame::fit(&soup, view, big, big, 0.06);
    let rgb = picture(&soup, &f, &|r| cols.get(usize::from(r)).copied().unwrap_or([0.6, 0.6, 0.6]), [0, 0, 0]);
    let mask = silhouette(&soup, &f);
    let mut out = vec![0u8; px * px * 4];
    for y in 0..px {
        for x in 0..px {
            let (mut c, mut a) = ([0u32; 3], 0u32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = (y * 2 + dy) * big + x * 2 + dx;
                if mask[i] {
                    a += 1;
                    for k in 0..3 {
                        c[k] += u32::from(rgb[i * 3 + k]);
                    }
                }
            }
            let o = (y * px + x) * 4;
            for k in 0..3 {
                out[o + k] = c[k].checked_div(a).unwrap_or(0) as u8;
            }
            out[o + 3] = (a * 255 / 4) as u8;
        }
    }
    out
}

/// The built-in rules, or a trial file named by `STUDIO_RULES` (to try
/// thresholds before they go into catalog/assets/source_rules.json).
fn rules() -> Result<SourceRules, String> {
    match std::env::var("STUDIO_RULES") {
        Ok(p) => SourceRules::parse(&std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?),
        Err(_) => SourceRules::builtin(),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let c = Catalogs::load(root())?;
    let get = |i: usize| args.get(i).ok_or_else(|| "usage: studio_assets tier|picture <asset> <in.glb> <out> [px] | thumb <asset> <out.png> [px] | code <asset> <out.glb>".to_owned());
    let asset = c.assets.asset(get(1)?).ok_or_else(|| format!("no asset `{}`", args[1]))?;
    let cols = colours(asset)?;
    match get(0)?.as_str() {
        "thumb" => {
            let out = get(2)?;
            let px: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(128);
            let src = source::load_source(&root().join(source::APP_MODELS), asset, asset.tiers.first().map_or("low", String::as_str))?;
            let img = thumbnail(&src, asset.frame.yaw, asset.frame.pitch, &cols, px);
            let bytes = png(px, px, 4, &img)?;
            std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}: {px} x {px}, {} bytes", bytes.len());
            return Ok(());
        }
        "code" => {
            let out = get(2)?;
            let shape = asset.source.shape.as_deref().ok_or_else(|| format!("{} isn't code-built", asset.id))?;
            let m = evani_style_geo::code::code_mesh(shape).ok_or_else(|| format!("no code shape `{shape}`"))?;
            let bytes = source::source_to_glb(&m, asset, &cols, "Evani.Dev studio code source (evani-style-geo code.rs, examples/studio_assets.rs)")?;
            std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}: {} bytes, {} triangles", bytes.len(), m.indices.len() / 3);
            return Ok(());
        }
        _ => {}
    }
    let raw = std::fs::read(get(2)?).map_err(|e| format!("{}: {e}", args[2]))?;
    let glb = source::first_lod_only(&raw)?;
    let out = get(3)?;
    match get(0)?.as_str() {
        "tier" => {
            let rules = rules()?;
            let rest = evani_style_geo::rest::rest_from_glb(&glb)?;
            let tags = source::tags_from_glb(&glb, rest.positions.len())?;
            let src = source::regions_from_rest(&rest, &tags, asset, rules.assets.get(&asset.id))?;
            let bytes = source::source_to_glb(&src, asset, &cols, "Evani.Dev studio source tier (evani-style-geo examples/studio_assets.rs)")?;
            std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
            let counts = source::region_counts(&src, asset.regions.len());
            let list: Vec<String> = asset.regions.iter().zip(&counts).map(|(r, n)| format!("{} {n}", r.id)).collect();
            println!("{out}: {} bytes, {} triangles, vertices per region: {}", bytes.len(), src.indices.len() / 3, list.join(", "));
        }
        "picture" => {
            let px: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(320);
            let rest = evani_style_geo::rest::rest_from_glb(&glb)?;
            let tags = source::tags_from_glb(&glb, rest.positions.len())?;
            let src = source::source_from_rest(&rest, &tags, asset, rules()?.assets.get(&asset.id))?;
            let soup = Soup::from_source(&src);
            let mut img = vec![0u8; px * 3 * px * 3];
            for (k, v) in VIEWS.iter().enumerate() {
                let f = Frame::fit(&soup, *v, px, px, 0.06);
                let p = picture(&soup, &f, &|r| cols.get(usize::from(r)).copied().unwrap_or([1.0, 0.0, 1.0]), [236, 236, 230]);
                for y in 0..px {
                    let d = (y * px * 3 + k * px) * 3;
                    img[d..d + px * 3].copy_from_slice(&p[y * px * 3..(y + 1) * px * 3]);
                }
            }
            std::fs::write(out, png(px * 3, px, 3, &img)?).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}: {} x {px}", px * 3);
        }
        other => return Err(format!("unknown command `{other}`")),
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("studio_assets: {e}");
            std::process::ExitCode::from(2)
        }
    }
}
