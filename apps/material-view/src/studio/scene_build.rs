//! WP4: one view's model file. The styled mesh's parts (one per region)
//! each get their region's material, in the style's surface treatment, as
//! one `.glb` the scene swaps into its model slot in place.
//!
//! - **Region materials.** A region bound to a catalogue row wears that
//!   row's own glTF material (`evani_material_recipes::gltf::gltf_material`:
//!   the recipe stage draws its pattern, its block rides along). A
//!   `tone: points` region wears the row's *points* version
//!   ([`points_row`]): a bay coat's black legs, mane and tail. A kept region
//!   (eyes) keeps a dark wet look; a row that can't be drawn yet shows plain
//!   clay, never another material.
//! - **The surface stage.** A style with a surface treatment (toon bands,
//!   outlines, posterize, dither...) cuts each part with treat.rs fed the
//!   region's colours, under a fixed key light ([`band_light`]): the light
//!   presets never reach the model, so a light change rebuilds nothing. The
//!   recipe's block rides on the body parts, as in the catalogue scene.
//!   Parts of the same look are merged; past [`MAX_SLOTS`] the bands are
//!   reduced, then the treatment drops to the lit materials.
//! - **Wireframe.** The drawn shape's edges as thin bright strips over a
//!   dark body: the same shape the render view shows.

use evani_kit_material_view::studio::evani_studio::MAX_SLOTS;
use evani_kit_material_view::studio::evani_studio::geo::StyledMesh;
use evani_kit_material_view::styles::{Finish, Render};
use evani_material_recipes::catalog::{Catalog, Row};
use evani_material_recipes::gltf;
use evani_render::model::recipes::RECIPE_KEY;
use serde_json::{Value, json};

use crate::scenes::catalogue::{UvPart, finish_of, tangents_for, uv_glb_parts, uv_map};
use crate::shapes::{self, Mesh, Pbr};
use crate::treat;

/// How the view draws its asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Display {
    /// The asset in its style: shape, surface and screen pass.
    #[default]
    Render,
    /// The asset's own shape in its materials, without the style: to judge
    /// the material itself.
    Material,
    /// The drawn shape's edges.
    Wireframe,
}

impl Display {
    /// The ids, in the param's option order.
    pub const IDS: [&'static str; 3] = ["render", "material", "wireframe"];

    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::Material,
            2 => Self::Wireframe,
            _ => Self::Render,
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code, reason = "the viewport overlay (wasm) writes the param"))]
    pub fn index(self) -> u32 {
        match self {
            Self::Render => 0,
            Self::Material => 1,
            Self::Wireframe => 2,
        }
    }
}

/// How one region is dressed.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionLook {
    /// The glTF material object the region wears.
    pub material: Value,
    /// Its colours as the surface treatment takes them.
    pub finish: Finish,
    /// The recipe block (rides on treated body parts).
    pub block: Option<Value>,
}

fn plain(base: [f32; 3], roughness: f32, metallic: f32) -> RegionLook {
    let pbr = Pbr { base: [base[0], base[1], base[2], 1.0], metallic, roughness, emissive: [0.0; 3] };
    let material = serde_json::from_str(&shapes::material_json(&pbr, false)).unwrap_or_else(|_| json!({"name": "plain"}));
    RegionLook { material, finish: Finish { tint: base, roughness, metallic, bump: 0.0, glow: 0.0 }, block: None }
}

/// A kept region (the source's own colour; the eyes): dark and wet.
pub fn kept_look() -> RegionLook {
    plain([0.018, 0.014, 0.012], 0.12, 0.0)
}

/// A region that glows briefly (its part was just picked): its material's
/// emissive and its treated glow lifted by `glow` (linear RGB).
pub fn flash(look: &mut RegionLook, glow: [f32; 3]) {
    let old = look.material.get("emissiveFactor").and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect::<Vec<f32>>()).unwrap_or_default();
    let e: Vec<f32> = (0..3).map(|k| (old.get(k).copied().unwrap_or(0.0) + glow[k]).min(1.0)).collect();
    look.material["emissiveFactor"] = json!(e);
    look.finish.glow = (look.finish.glow + 0.35).min(1.0);
    look.finish.tint = std::array::from_fn(|k| (look.finish.tint[k] + glow[k] * 0.6).min(1.0));
}

/// A region whose material can't be drawn yet (a planned row): plain clay.
pub fn clay_look() -> RegionLook {
    plain([0.42, 0.4, 0.37], 0.85, 0.0)
}

/// A catalogue row as a region's look; `points` for a `tone: points` region.
pub fn region_look(cat: &Catalog, row: &Row, points: bool) -> Result<RegionLook, String> {
    let row = if points { points_row(row) } else { row.clone() };
    let material = gltf::gltf_material(cat, &row).map_err(|e| format!("{}: {e}", row.id))?;
    let colours = cat.encode(&row).map(|b| b.unpack().colours).unwrap_or([[128; 3]; 3]);
    let finish = finish_of(&material, &colours, &row.recipe);
    let block = material.get("extras").and_then(|e| e.get(RECIPE_KEY)).cloned();
    Ok(RegionLook { material, finish, block })
}

/// Whether a `tone: points` region `region` wears the row's points version:
/// every one unless the row names them in `points_on` (a palomino's white
/// mane and tail over gold legs).
pub fn points_here(row: &Row, region: &str) -> bool {
    row.params.get("points_on").and_then(Value::as_array).is_none_or(|l| l.iter().any(|r| r.as_str() == Some(region)))
}

/// The row a region wears before its points: a row with `points_on` keeps
/// its points (and the recipe's thin-part points, which pull toward the same
/// colour) to the regions it names, so a palomino's legs, ears and shoulders
/// stay gold, not streaked white. Other rows are unchanged.
/// A row with `points_to` does the same: its thin-part points pull toward
/// its pattern colour (a roan's white), so only its points regions keep them.
pub fn region_row(row: &Row, region: &str, tone_points: bool) -> Row {
    let named = row.params.get("points_on").is_some() || row.params.get("points_to").is_some();
    if !named || (tone_points && points_here(row, region)) {
        return row.clone();
    }
    let mut out = row.clone();
    if let Some(n) = out.params.get_mut("numbers").and_then(Value::as_object_mut) {
        n.insert("points".into(), json!(0.0));
    }
    out
}

/// A coat's points version: its colours pulled toward its last colour (a
/// bay's black) by its `points` number. A row without points, or with fewer
/// than three colours, is unchanged (brick on a horse is brick all over).
/// A row with `points_to` pulls toward that colour instead, and its last
/// colour becomes it outright, so a red roan's legs are solid dark chestnut:
/// no white hairs, and the thin-part points darken rather than whiten.
pub fn points_row(row: &Row) -> Row {
    let mut out = row.clone();
    let amount = row.params.get("numbers").and_then(|n| n.get("points")).and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
    let Some(colours) = row.params.get("colours").and_then(Value::as_array) else { return out };
    let parsed: Vec<[u8; 3]> = colours.iter().filter_map(|c| c.as_str().and_then(evani_material_recipes::catalog::parse_hex_colour)).collect();
    if amount <= 0.0 || parsed.len() < 3 || parsed.len() != colours.len() {
        return out;
    }
    let own = row.params.get("points_to").and_then(Value::as_str).and_then(evani_material_recipes::catalog::parse_hex_colour);
    let to = own.unwrap_or(parsed[parsed.len() - 1]);
    let mixed: Vec<Value> = parsed
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let c = if i + 1 == parsed.len() { to } else { std::array::from_fn(|k| (f32::from(c[k]) + (f32::from(to[k]) - f32::from(c[k])) * amount).round().clamp(0.0, 255.0) as u8) };
            Value::String(format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
        })
        .collect();
    out.params.insert("colours".into(), Value::Array(mixed));
    out
}

/// The fixed key light the bands are cut from: the engine's default sun,
/// turned toward the asset's home view (`toward_eye`). Never the light
/// preset's sun, so a light change rebuilds nothing.
pub fn band_light(toward_eye: [f32; 3], r: &Render) -> [f32; 3] {
    treat::key_light(treat::SUN, toward_eye, r.side_light)
}

/// A part ready to write.
struct Out {
    mesh: Mesh,
    /// Where the pattern is sampled (a voxel's cell centre), per vertex.
    samples: Vec<[f32; 3]>,
    material: Value,
    /// The part's own pattern coordinates (a code-built asset's region laid
    /// its own way: roof tiles down the slope); `None`: the projection.
    uvs: Option<Vec<[f32; 2]>>,
}

/// The layout `uv` names: `studio:<shape>` is the studio's projection with a
/// code-built shape's own per-region layout ([`code_uv`]) on top.
fn split_uv(uv: &str) -> (&str, Option<&str>) {
    match uv.strip_prefix("studio:") {
        Some(shape) => (STUDIO_UV, Some(shape)),
        None => (uv, None),
    }
}

/// A code-built shape's own uvs for one region's part: the projection where
/// the shape has no rule, its rule where it has (at the sample point when
/// there is one: a voxel's cell centre).
fn part_uvs(uv: &str, region: u8, mesh: &Mesh, samples: &[[f32; 3]]) -> Option<Vec<[f32; 2]>> {
    let (base, shape) = split_uv(uv);
    let shape = shape?;
    let at: Vec<[f32; 3]> = if samples.len() == mesh.positions.len() { samples.to_vec() } else { mesh.positions.clone() };
    let rule: Vec<Option<[f32; 2]>> = at.iter().zip(&mesh.normals).map(|(p, n)| evani_style_geo::code::code_uv(shape, region, *p, *n)).collect();
    if rule.iter().all(Option::is_none) {
        return None;
    }
    let plain = uv_map(base, &Mesh { positions: at, normals: mesh.normals.clone(), indices: mesh.indices.clone() }).0;
    Some(rule.iter().zip(plain).map(|(r, p)| r.unwrap_or(p)).collect())
}

/// A part's uvs and tangents: its own when it has them, else the layout's.
fn uvs_of(uv: &str, p: &Out) -> (Vec<[f32; 2]>, Vec<[f32; 4]>) {
    let (base, _) = split_uv(uv);
    if let Some(u) = p.uvs.as_ref().filter(|u| u.len() == p.mesh.positions.len()) {
        return (u.clone(), tangents_for(&p.mesh, u));
    }
    // The pattern at each vertex's sample point (a voxel's cell centre: one
    // colour per cube), else at the vertex.
    if p.samples.len() == p.mesh.positions.len() {
        let at = Mesh { positions: p.samples.clone(), normals: p.mesh.normals.clone(), indices: p.mesh.indices.clone() };
        let (uvs, _) = uv_map(base, &at);
        let (_, tangents) = uv_map(base, &p.mesh);
        (uvs, tangents)
    } else {
        uv_map(base, &p.mesh)
    }
}

fn mesh_of(p: &evani_kit_material_view::studio::evani_studio::geo::StyledPart) -> Mesh {
    Mesh { positions: p.positions.clone(), normals: p.normals.clone(), indices: p.indices.clone() }
}

/// Append `b` to `a` (same material).
fn merge(a: &mut Out, b: Out, uv: &str) {
    if a.uvs.is_some() || b.uvs.is_some() {
        let mut ua = uvs_of(uv, a).0;
        ua.extend(uvs_of(uv, &b).0);
        a.uvs = Some(ua);
        a.samples.clear();
    }
    let base = a.mesh.positions.len() as u32;
    a.mesh.positions.extend(b.mesh.positions);
    a.mesh.normals.extend(b.mesh.normals);
    a.mesh.indices.extend(b.mesh.indices.iter().map(|i| base + i));
    if a.uvs.is_none() {
        a.samples.extend(b.samples);
    }
}

/// Merge parts wearing the same material, keeping first-seen order.
fn merged(parts: Vec<Out>, uv: &str) -> Vec<Out> {
    let mut out: Vec<Out> = Vec::new();
    for p in parts {
        match out.iter_mut().find(|o| o.material == p.material) {
            Some(o) => merge(o, p, uv),
            None => out.push(p),
        }
    }
    out
}

/// The parts lit by their materials (no treatment): one per region part,
/// so the model's parts are the bindings.
fn lit_parts(mesh: &StyledMesh, looks: &[RegionLook], uv: &str) -> Vec<Out> {
    let clay = clay_look();
    mesh.parts
        .iter()
        .map(|p| {
            let look = looks.get(usize::from(p.region)).unwrap_or(&clay);
            let m = mesh_of(p);
            let uvs = part_uvs(uv, p.region, &m, &p.samples);
            Out { mesh: m, samples: p.samples.clone(), material: look.material.clone(), uvs }
        })
        .collect()
}

/// The parts cut by the surface treatment `r`.
fn treated_parts(mesh: &StyledMesh, looks: &[RegionLook], r: &Render, light: [f32; 3], uv: &str) -> Vec<Out> {
    let clay = clay_look();
    let mut parts = Vec::new();
    for p in &mesh.parts {
        let look = looks.get(usize::from(p.region)).unwrap_or(&clay);
        for t in treat::shade(&mesh_of(p), r, &look.finish, light) {
            let mut m: Value = serde_json::from_str(&shapes::material_json(&t.pbr, t.double_sided)).unwrap_or_else(|_| json!({"name": "part"}));
            if t.body
                && let Some(b) = &look.block
            {
                m["extras"] = json!({ RECIPE_KEY: b });
            }
            let uvs = part_uvs(uv, p.region, &t.mesh, &[]);
            parts.push(Out { mesh: t.mesh, samples: Vec::new(), material: m, uvs });
        }
    }
    merged(parts, uv)
}

/// Thin bright strips along every edge of `mesh` (each lifted a hair off
/// the surface), `width` wide (model units).
pub fn wire_mesh(mesh: &StyledMesh, width: f32) -> Mesh {
    use std::collections::HashSet;
    /// More edges than this are skipped (a wireframe past it is a grey blur).
    const MAX_EDGES: usize = 60_000;
    let mut out = Mesh::default();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let key = |p: [f32; 3]| -> u64 {
        let q = p.map(|x| ((x * 4096.0).round() as i64 & 0x1f_ffff) as u64);
        (q[0] << 42) | (q[1] << 21) | q[2]
    };
    'parts: for p in &mesh.parts {
        for tri in p.indices.as_chunks::<3>().0 {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let (Some(&pa), Some(&pb)) = (p.positions.get(a as usize), p.positions.get(b as usize)) else { continue };
                let (ka, kb) = (key(pa), key(pb));
                if !seen.insert((ka.min(kb), ka.max(kb))) {
                    continue;
                }
                if seen.len() > MAX_EDGES {
                    break 'parts;
                }
                let na = p.normals.get(a as usize).copied().unwrap_or([0.0, 1.0, 0.0]);
                let nb = p.normals.get(b as usize).copied().unwrap_or([0.0, 1.0, 0.0]);
                let n = shapes::normalize(shapes::add(na, nb), [0.0, 1.0, 0.0]);
                let d = shapes::sub(pb, pa);
                let side = shapes::scale(shapes::normalize(shapes::cross(d, n), [1.0, 0.0, 0.0]), width * 0.5);
                let lift = shapes::scale(n, width * 0.6);
                let base = out.positions.len() as u32;
                for (q, s) in [(pa, 1.0), (pa, -1.0), (pb, 1.0), (pb, -1.0)] {
                    out.positions.push(shapes::add(shapes::add(q, lift), shapes::scale(side, s)));
                    out.normals.push(n);
                }
                out.indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
            }
        }
    }
    out
}

/// What [`build`] makes a view's model from.
pub struct Build<'a> {
    pub mesh: &'a StyledMesh,
    /// Per region of the asset, in its order.
    pub looks: &'a [RegionLook],
    /// The style's surface treatment (`None`: the lit materials).
    pub surface: Option<&'a Render>,
    /// The key light for the bands ([`band_light`]).
    pub light: [f32; 3],
    pub display: Display,
    /// The wireframe's strip width (model units).
    pub wire: f32,
    /// How the pattern is laid on the shape (catalogue `uv_map`): [`STUDIO_UV`]
    /// for an asset, `"ball"` for a material ball (round, seamless, the
    /// catalogue's own ball the rows were tuned on).
    pub uv: &'a str,
}

/// An asset's pattern layout: projected along each face's main axis.
pub const STUDIO_UV: &str = "studio";

/// The layout for an asset: a code-built shape's own (`studio:<shape>`:
/// roof tiles down the slope, ridge tiles along the ridge), else
/// [`STUDIO_UV`].
pub fn asset_uv(asset: &evani_kit_material_view::studio::evani_studio::AssetRow) -> String {
    match asset.source.shape.as_deref() {
        Some(shape) if evani_style_geo::code::SHAPES.contains(&shape) => format!("studio:{shape}"),
        _ => STUDIO_UV.to_string(),
    }
}

/// The view's model file and its part (primitive) count.
pub fn build(b: &Build) -> Result<(Vec<u8>, usize), String> {
    let parts = match (b.display, b.surface) {
        (Display::Wireframe, _) => {
            let body = plain([0.035, 0.04, 0.05], 0.7, 0.0);
            let mut body = merged(lit_parts(b.mesh, &vec![body; b.looks.len().max(1)], b.uv), b.uv);
            let glow = Pbr { base: [0.5, 0.85, 1.0, 1.0], metallic: 0.0, roughness: 0.5, emissive: [0.45, 0.8, 1.0] };
            let wire = Out {
                mesh: wire_mesh(b.mesh, b.wire),
                samples: Vec::new(),
                material: serde_json::from_str(&shapes::material_json(&glow, true)).unwrap_or_else(|_| json!({"name": "wire"})),
                uvs: None,
            };
            if !wire.mesh.indices.is_empty() {
                body.push(wire);
            }
            body
        }
        (Display::Render, Some(r)) => {
            let mut r = r.clone();
            let mut parts = treated_parts(b.mesh, b.looks, &r, b.light, b.uv);
            while parts.len() > MAX_SLOTS && r.bands > 1 {
                r.bands -= 1;
                parts = treated_parts(b.mesh, b.looks, &r, b.light, b.uv);
            }
            if parts.len() > MAX_SLOTS { lit_parts(b.mesh, b.looks, b.uv) } else { parts }
        }
        _ => lit_parts(b.mesh, b.looks, b.uv),
    };
    let maps: Vec<_> = parts.iter().map(|p| uvs_of(b.uv, p)).collect();
    let list: Vec<UvPart> = parts.iter().zip(&maps).map(|(p, (uvs, tangents))| UvPart { mesh: &p.mesh, uvs, tangents, material: p.material.clone() }).collect();
    let bytes = uv_glb_parts(&list)?;
    Ok((bytes, list.iter().filter(|p| !p.mesh.indices.is_empty()).count()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_kit_material_view::studio::evani_studio::geo::StyledPart;

    fn cat() -> Catalog {
        Catalog::bundled().unwrap()
    }

    fn row<'a>(c: &'a Catalog, id: &str) -> &'a Row {
        c.rows.iter().find(|r| r.id == id).unwrap()
    }

    /// A bay's points are its black; brick has no points and stays brick.
    #[test]
    fn a_bay_coat_has_black_points_and_brick_stays_brick() {
        let c = cat();
        let bay = row(&c, "horse.coat.bay");
        let pts = points_row(bay);
        let cols: Vec<String> = pts.params["colours"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        // 85% of the way from 521f10 to 1a1410.
        assert_eq!(cols[0], "221610");
        assert_eq!(cols[2], "1a1410");
        let body = region_look(&c, bay, false).unwrap();
        let legs = region_look(&c, bay, true).unwrap();
        let lum = |f: &Finish| f.tint[0] + f.tint[1] + f.tint[2];
        assert!(lum(&legs.finish) < lum(&body.finish) * 0.6, "{:?} vs {:?}", legs.finish, body.finish);
        let brick = c.rows.iter().find(|r| r.recipe == "masonry" && r.status == "ready").unwrap();
        assert_eq!(points_row(brick).params, brick.params);
    }

    /// A palomino (studio material pass): a near-white mane and tail, gold
    /// legs; a row without `points_on` (the bay) has points on every region.
    #[test]
    fn a_palomino_has_a_white_mane_and_tail_on_gold_legs() {
        let c = cat();
        let pal = row(&c, "horse.coat.palomino");
        assert!(points_here(pal, "mane") && points_here(pal, "tail"));
        assert!(!points_here(pal, "leg"));
        let bay = row(&c, "horse.coat.bay");
        assert!(["leg", "mane", "tail"].iter().all(|r| points_here(bay, r)));
        let body = region_look(&c, pal, false).unwrap();
        let mane = region_look(&c, pal, true).unwrap();
        let lum = |f: &Finish| f.tint[0] + f.tint[1] + f.tint[2];
        let (t, b) = (mane.finish.tint, body.finish.tint);
        assert!(lum(&mane.finish) > lum(&body.finish) * 1.4, "{:?} vs {:?}", mane.finish, body.finish);
        let warmth = |c: [f32; 3]| (c[0] - c[2]) / c[0];
        assert!(warmth(t) < 0.6 * warmth(b), "the mane is near white, not gold: {t:?} vs {b:?}");
        // Its legs and body draw no thin-part points (no white streaks);
        // its mane keeps them; the bay keeps its points everywhere.
        let pts = |r: &Row| r.params["numbers"]["points"].as_f64().unwrap();
        assert_eq!(pts(&region_row(pal, "leg", true)), 0.0);
        assert_eq!(pts(&region_row(pal, "coat", false)), 0.0);
        assert!(pts(&region_row(pal, "mane", true)) > 0.8);
        assert_eq!(region_row(bay, "coat", false), *bay);
        assert_eq!(region_row(bay, "leg", true), *bay);
        assert!(b[0] > b[1] && b[1] > b[2] * 1.3, "the body is gold: {b:?}");
    }

    /// A red roan (studio material pass): its legs, mane and tail are solid
    /// dark chestnut (`points_to`), not white; its hair colour there turns
    /// dark too, so no white hairs; its coat draws no thin-part points.
    #[test]
    fn a_red_roan_has_solid_dark_chestnut_points() {
        let c = cat();
        let rr = row(&c, "horse.coat.red_roan");
        let to = rr.params["points_to"].as_str().unwrap().to_string();
        let pts = points_row(rr);
        let cols: Vec<&str> = pts.params["colours"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(cols[2], to, "the roan hairs on the points are the points colour");
        let body = region_look(&c, rr, false).unwrap();
        let legs = region_look(&c, rr, true).unwrap();
        let lum = |f: &Finish| f.tint[0] + f.tint[1] + f.tint[2];
        assert!(lum(&legs.finish) < lum(&body.finish) * 0.8, "{:?} vs {:?}", legs.finish, body.finish);
        let t = legs.finish.tint;
        assert!(t[0] > t[1] * 1.5 && t[0] > t[2] * 1.5, "the legs are chestnut, not grey or white: {t:?}");
        let n = |r: &Row| r.params["numbers"]["points"].as_f64().unwrap();
        assert_eq!(n(&region_row(rr, "coat", false)), 0.0);
        assert!(["leg", "mane", "tail"].iter().all(|r| points_here(rr, r) && n(&region_row(rr, r, true)) > 0.5));
    }

    fn cube(region: u8, at: [f32; 3]) -> StyledPart {
        let p: Vec<[f32; 3]> = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]
            .iter()
            .map(|v: &[f32; 3]| [v[0] * 0.3 + at[0], v[1] * 0.3 + at[1], v[2] * 0.3 + at[2]])
            .collect();
        let normals = p.iter().map(|v| shapes::normalize(shapes::sub(*v, at), [0.0, 1.0, 0.0])).collect();
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        StyledPart { region, positions: p, normals, indices, samples: Vec::new() }
    }

    /// Lit: one part per region part; a toon surface stays within
    /// the slot cap; the wireframe adds one strip part; all are valid glTF
    /// the engine reads.
    #[test]
    fn every_display_builds_a_valid_model() {
        let c = cat();
        let bay = row(&c, "horse.coat.bay");
        let looks = vec![region_look(&c, bay, false).unwrap(), region_look(&c, bay, true).unwrap(), kept_look()];
        let mesh = StyledMesh { parts: vec![cube(0, [0.0; 3]), cube(1, [1.0, 0.0, 0.0]), cube(2, [0.0, 1.0, 0.0]), cube(0, [-1.0, 0.0, 0.0])] };
        let toon = Render { bands: 3, outline: 0.02, ..Render::default() };
        for (display, surface, want) in [(Display::Render, None, 4), (Display::Material, Some(&toon), 4), (Display::Wireframe, None, 2)] {
            let (bytes, parts) = build(&Build { mesh: &mesh, looks: &looks, surface, light: treat::SUN, display, wire: 0.01, uv: STUDIO_UV }).unwrap();
            assert_eq!(parts, want, "{display:?}");
            let g = evani_gltf::Gltf::from_glb(&bytes).unwrap();
            evani_render::model::prepare(&g).unwrap();
        }
        let (_, parts) = build(&Build { mesh: &mesh, looks: &looks, surface: Some(&toon), light: treat::SUN, display: Display::Render, wire: 0.01, uv: STUDIO_UV }).unwrap();
        assert!(parts > 3 && parts <= MAX_SLOTS, "{parts}");
    }

    /// The roof (a code-built shape with its own per-part layout) builds a
    /// valid model in every display, lit and toon, plain and voxel; its
    /// tile part's uvs are the shape's own (down the slope).
    #[test]
    fn the_roof_builds_with_its_own_layout_in_every_display() {
        use evani_kit_material_view::studio::evani_studio::Catalogs;
        use evani_style_geo::GeometryGenerator;
        let c = cat();
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let cats = Catalogs::load(root).unwrap();
        let row = cats.assets.asset("building.roof").unwrap();
        let src = evani_style_geo::code::code_source("gable_roof").unwrap();
        let plain = evani_style_geo::Passthrough.generate(&src, row, &evani_style_geo::Geometry::None).unwrap();
        let voxel = cats.styles.style("WK08").unwrap();
        let vox = evani_style_geo::generator(voxel.geometry.kind()).generate(&src, row, &voxel.geometry).unwrap();
        let tiles = region_look(&c, row_of(&c, "roof.tile.terracotta"), false).unwrap();
        let brick = region_look(&c, row_of(&c, "brick.red.common"), false).unwrap();
        let looks = vec![tiles.clone(), tiles, brick, kept_look()];
        let toon = Render { bands: 3, outline: 0.02, ..Render::default() };
        let uv = asset_uv(row);
        assert_eq!(uv, "studio:gable_roof");
        for mesh in [&plain, &vox] {
            for (display, surface) in [(Display::Render, Some(&toon)), (Display::Render, None), (Display::Material, None), (Display::Wireframe, None)] {
                let (bytes, parts) = build(&Build { mesh, looks: &looks, surface, light: treat::SUN, display, wire: 0.01, uv: &uv }).unwrap_or_else(|e| panic!("{display:?}: {e}"));
                assert!(parts > 0, "{display:?}");
                let g = evani_gltf::Gltf::from_glb(&bytes).unwrap();
                evani_render::model::prepare(&g).unwrap_or_else(|e| panic!("{display:?}: {e:?}"));
            }
        }
        // The tile part's own uvs: the shape's rule, not the projection.
        let t = plain.parts.iter().find(|p| p.region == 0).unwrap();
        let m = mesh_of(t);
        let own = part_uvs(&uv, 0, &m, &t.samples).unwrap();
        let (p, n) = (m.positions[0], m.normals[0]);
        assert_eq!(Some(own[0]), evani_style_geo::code::code_uv("gable_roof", 0, p, n));
    }

    fn row_of<'a>(c: &'a Catalog, id: &str) -> &'a Row {
        row(c, id)
    }

    /// The wireframe has one strip (two triangles) per distinct edge.
    #[test]
    fn the_wireframe_draws_each_edge_once() {
        let mesh = StyledMesh { parts: vec![cube(0, [0.0; 3])] };
        let w = wire_mesh(&mesh, 0.01);
        // A cube cut into 12 triangles has 18 distinct edges.
        assert_eq!(w.indices.len() / 6, 18);
    }
}
