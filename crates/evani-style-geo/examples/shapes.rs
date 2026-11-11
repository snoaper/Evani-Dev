//! Dev look at the geometry stage: `cargo run -p evani-style-geo --release
//! --example shapes -- <tier.glb> <out dir>` writes the horse's source,
//! voxel and low-poly pictures (front, side, three-quarter) as PNGs and
//! prints each generator's time, triangles and checks. The committed check
//! is the crate's tests and `cargo xtask style-bake --shots`; this is the
//! quick loop.

use std::time::Instant;

use evani_studio::Catalogs;
use evani_style_geo::view::{Frame, Soup, VIEWS, picture};
use evani_style_geo::{GeometryKind, generator, standin};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let glb = args.get(1).ok_or("usage: shapes <tier.glb> <out dir> [asset]")?;
    let out = std::path::PathBuf::from(args.get(2).ok_or("usage: shapes <tier.glb> <out dir>")?);
    let asset_id = args.get(3).map_or("animal.horse", String::as_str);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let c = Catalogs::load(root)?;
    let asset = c.assets.asset(asset_id).ok_or("no such asset")?;
    let bytes = std::fs::read(glb).map_err(|e| e.to_string())?;
    let rest = evani_style_geo::rest::rest_from_glb(&bytes)?;
    for (n, p) in &rest.anchors {
        println!("anchor {n}: {p:?}");
    }
    let t = Instant::now();
    let src = standin::horse_source(&rest, asset)?;
    println!("source: {} verts, {} tris, {:.1} ms", src.positions.len(), src.indices.len() / 3, t.elapsed().as_secs_f64() * 1e3);
    let colour = |r: u8| standin::HORSE_COLOURS.get(usize::from(r)).copied().unwrap_or([1.0, 0.0, 1.0]);
    let src_soup = Soup::from_source(&src);
    let mut sets = vec![("source".to_string(), src_soup)];
    for s in &c.styles.styles {
        let kind = s.geometry.kind();
        if !matches!(kind, GeometryKind::Voxel | GeometryKind::Lowpoly | GeometryKind::Papercraft | GeometryKind::Lumpy | GeometryKind::Ridges) {
            continue;
        }
        if evani_style_geo::generator_version(kind) == 0 {
            continue;
        }
        let t = Instant::now();
        let m = generator(kind).generate(&src, asset, &s.geometry)?;
        println!("{} {}: {} tris, {} parts, {:.1} ms", s.id, kind.name(), m.triangles(), m.parts.len(), t.elapsed().as_secs_f64() * 1e3);
        sets.push((format!("{}-{}", kind.name(), s.id), Soup::from_styled(&m)));
    }
    for v in VIEWS {
        let frame = Frame::fit(&sets[0].1, v, 720, 540, 0.06);
        for (name, soup) in &sets {
            let img = picture(soup, &frame, &colour, [236, 238, 242]);
            let path = out.join(format!("{name}-{}.png", v.name));
            std::fs::write(&path, png(frame.w, frame.h, &img)).map_err(|e| e.to_string())?;
        }
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// A minimal RGB PNG (stored deflate blocks): dev pictures only.
fn png(w: usize, h: usize, rgb: &[u8]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        raw.extend(&rgb[y * w * 3..(y + 1) * w * 3]);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = b.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend(*b);
    }
    let (mut a, mut bsum) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        bsum = (bsum + a) % 65521;
    }
    z.extend(((bsum << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
