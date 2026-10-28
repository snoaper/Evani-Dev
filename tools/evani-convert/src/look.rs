//! Look matching (STAGE3_PLAN S3-12): the converter's optional step that cuts
//! a model down to what one look can show, before the clean, the tier files
//! and the quality gate. Three parts, all on the glTF JSON of the input:
//!
//! 1. **Drop** the material inputs the look never reads (for toon: the
//!    metal-rough and occlusion maps and the metallic and roughness factors),
//!    so glTF-Transform's `prune` removes the images nobody samples.
//! 2. **Merge** materials that are equal in everything the look reads (their
//!    names aside), so gltfpack can join the primitives that share one.
//! 3. **Snap** untextured base colours to the look's palette, only when asked
//!    for (`--snap-palette`) and only within [`SNAP_MAX_DELTA_E`].
//!
//! The one table: [`SLOTS`] names every material input the engine knows, and
//! [`LOOKS`] says which of them each look reads. A test below holds the table
//! equal to what the shaders and `fn material()` in evani-render really read,
//! so it cannot drift. Stage 5's `smooth-lighting` (CORE_REQUESTS) may make
//! toon read metal-rough and occlusion: that change is one row here
//! (DECISIONS, "Look matching").
//!
//! The gate still decides: the matched file is what the tiers are made from,
//! and the gate scores each tier against the original master.

use serde_json::{Value, json};

use crate::glb::Doc;
use crate::metric::lab;

/// One input of a glTF material that the engine knows about.
pub struct Slot {
    /// The name looks list in `reads`.
    pub name: &'static str,
    /// JSON pointer inside a material.
    pub path: &'static str,
    /// The field of `evani_gltf::Material` that holds it.
    pub field: &'static str,
    /// The WGSL texture binding that samples it (a map slot), if any.
    pub binding: Option<&'static str>,
}

/// Every material input of the engine's model pipeline, read or not.
pub const SLOTS: [Slot; 12] = [
    Slot { name: "base_color_factor", path: "/pbrMetallicRoughness/baseColorFactor", field: "base_color_factor", binding: None },
    Slot { name: "base_color_texture", path: "/pbrMetallicRoughness/baseColorTexture", field: "base_color_texture", binding: Some("base_map") },
    Slot { name: "metallic_factor", path: "/pbrMetallicRoughness/metallicFactor", field: "metallic_factor", binding: None },
    Slot { name: "roughness_factor", path: "/pbrMetallicRoughness/roughnessFactor", field: "roughness_factor", binding: None },
    Slot { name: "metallic_roughness_texture", path: "/pbrMetallicRoughness/metallicRoughnessTexture", field: "metallic_roughness_texture", binding: None },
    Slot { name: "normal_texture", path: "/normalTexture", field: "normal_texture", binding: Some("normal_map") },
    Slot { name: "occlusion_texture", path: "/occlusionTexture", field: "occlusion_texture", binding: None },
    Slot { name: "emissive_texture", path: "/emissiveTexture", field: "emissive_texture", binding: Some("emissive_map") },
    Slot { name: "emissive_factor", path: "/emissiveFactor", field: "emissive_factor", binding: None },
    Slot { name: "alpha_mode", path: "/alphaMode", field: "alpha_mode", binding: None },
    Slot { name: "alpha_cutoff", path: "/alphaCutoff", field: "alpha_cutoff", binding: None },
    Slot { name: "double_sided", path: "/doubleSided", field: "double_sided", binding: None },
];

/// Where a material keeps its relief recipe (S3-10). Its height map is a
/// texture only the recipe names, sampled by this binding when the `relief`
/// feature is on; matching never touches it (the look's `relief` reads it).
pub const RELIEF_PATH: &str = "/extras/evani/relief";
pub const RELIEF_BINDING: &str = "height_map";

/// What one look reads.
pub struct LookRow {
    pub look: &'static str,
    /// Slot names from [`SLOTS`]. A slot not listed is dropped.
    pub reads: &'static [&'static str],
}

/// The looks with a row. Every 3D look draws models with the same shader
/// (model.wgsl), but only toon has been checked and has a row; any other look
/// keeps every map and says so in the report.
pub const LOOKS: [LookRow; 1] = [LookRow {
    look: "toon",
    reads: &["base_color_factor", "base_color_texture", "normal_texture", "emissive_texture", "emissive_factor", "alpha_mode", "alpha_cutoff", "double_sided"],
}];

pub fn row(look: &str) -> Option<&'static LookRow> {
    LOOKS.iter().find(|r| r.look == look)
}

/// A look's palette (linear RGBA), if it has one.
pub fn palette(look: &str) -> Option<Vec<[f32; 4]>> {
    match look {
        "toon" => Some(evani_look_toon::PALETTE.to_vec()),
        _ => None,
    }
}

/// The largest CIELAB distance (about "clearly a different colour" at 10 to
/// 20) a base colour may move to reach a palette swatch.
pub const SNAP_MAX_DELTA_E: f32 = 15.0;

/// One base colour moved to a swatch.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub material: String,
    pub from: [f32; 4],
    pub to: [f32; 4],
    pub delta_e: f32,
}

/// What matching did, for the report.
#[derive(Clone, Debug, Default)]
pub struct Match {
    pub look: String,
    /// False: this look has no table row, nothing was changed.
    pub has_table: bool,
    /// Per dropped slot: how many materials had it.
    pub dropped: Vec<(String, usize)>,
    /// Textures (indices in the input) that no material uses any more.
    pub textures_dropped: Vec<usize>,
    pub materials_before: usize,
    pub materials_after: usize,
    /// Per kept material, the names of the ones folded into it.
    pub merged: Vec<(String, Vec<String>)>,
    /// Primitives in the file's meshes, before and after (the draw-call count
    /// per mesh instance; gltfpack joins what shares a material).
    pub primitives: usize,
    pub snap_asked: bool,
    pub snapped: Vec<Snap>,
    /// Materials the snap left alone, with why.
    pub snap_skipped: Vec<(String, String)>,
    /// Materials carrying a relief recipe (their maps are kept in full).
    pub relief_materials: usize,
    pub notes: Vec<String>,
}

impl Match {
    /// Did matching change the document?
    pub fn changed(&self) -> bool {
        !self.dropped.is_empty() || self.materials_after != self.materials_before || !self.snapped.is_empty()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "look": self.look,
            "has_table": self.has_table,
            "dropped_slots": self.dropped.iter().map(|(s, n)| json!({ "slot": s, "materials": n })).collect::<Vec<_>>(),
            "textures_dropped": self.textures_dropped,
            "materials": { "before": self.materials_before, "after": self.materials_after },
            "merged": self.merged.iter().map(|(k, v)| json!({ "kept": k, "folded": v })).collect::<Vec<_>>(),
            "primitives": self.primitives,
            "snap_palette": { "asked": self.snap_asked, "max_delta_e": SNAP_MAX_DELTA_E, "snapped": self.snapped.iter().map(|s| json!({ "material": s.material, "from": s.from, "to": s.to, "delta_e": s.delta_e })).collect::<Vec<_>>(), "skipped": self.snap_skipped.iter().map(|(m, w)| json!({ "material": m, "why": w })).collect::<Vec<_>>() },
            "relief_materials": self.relief_materials,
        })
    }
}

fn remove_pointer(v: &mut Value, ptr: &str) -> bool {
    let Some((parent, key)) = ptr.rsplit_once('/') else { return false };
    v.pointer_mut(parent).and_then(Value::as_object_mut).is_some_and(|o| o.remove(key).is_some())
}

fn name_of(m: &Value, i: usize) -> String {
    m.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map_or_else(|| format!("material {i}"), str::to_owned)
}

/// Every texture index a material still names in its standard slots.
fn texture_refs(m: &Value) -> Vec<usize> {
    SLOTS.iter().filter(|s| s.binding.is_some()).filter_map(|s| m.pointer(&format!("{}/index", s.path)).and_then(Value::as_u64)).map(|i| i as usize).collect()
}

fn primitive_count(j: &Value) -> usize {
    j.get("meshes").and_then(Value::as_array).into_iter().flatten().map(|m| m.get("primitives").and_then(Value::as_array).map_or(0, Vec::len)).sum()
}

/// Why materials may not be merged in this file (they are referenced by
/// something other than a primitive's `material`).
fn merge_blocker(j: &Value) -> Option<&'static str> {
    let used = |name: &str| j.get("extensionsUsed").and_then(Value::as_array).is_some_and(|a| a.iter().any(|e| e.as_str() == Some(name)));
    if used("KHR_materials_variants") {
        Some("KHR_materials_variants points at materials by index")
    } else if used("KHR_animation_pointer") {
        Some("KHR_animation_pointer animates materials by index")
    } else {
        None
    }
}

/// The nearest swatch to a linear RGBA colour: (swatch, CIELAB distance).
pub fn nearest(color: [f32; 4], palette: &[[f32; 4]]) -> Option<([f32; 4], f32)> {
    let l = lab([color[0], color[1], color[2]]);
    palette
        .iter()
        .map(|p| {
            let q = lab([p[0], p[1], p[2]]);
            (*p, ((l[0] - q[0]).powi(2) + (l[1] - q[1]).powi(2) + (l[2] - q[2]).powi(2)).sqrt())
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Match `doc` to `row`. `palette`: snap untextured base colours to it.
/// The returned document has the same nodes, scenes, extras, meshes and
/// geometry; only the materials (and `primitive.material`) differ.
pub fn apply(doc: &Doc, row: &LookRow, palette: Option<&[[f32; 4]]>) -> (Doc, Match) {
    let mut out = doc.clone();
    let mut rep = Match { look: row.look.to_owned(), has_table: true, snap_asked: palette.is_some(), ..Match::default() };
    let materials: Vec<Value> = out.json.get("materials").and_then(Value::as_array).cloned().unwrap_or_default();
    rep.materials_before = materials.len();
    rep.materials_after = materials.len();
    rep.primitives = primitive_count(&out.json);
    if materials.is_empty() {
        rep.notes.push("the model has no materials: nothing to match".into());
        return (out, rep);
    }
    rep.relief_materials = materials.iter().filter(|m| m.pointer(RELIEF_PATH).is_some()).count();

    let before_refs: std::collections::BTreeSet<usize> = materials.iter().flat_map(texture_refs).collect();

    // 1. Drop what the look never reads.
    let mut mats = materials;
    for slot in SLOTS.iter().filter(|s| !row.reads.contains(&s.name)) {
        let n = mats.iter_mut().map(|m| remove_pointer(m, slot.path)).filter(|r| *r).count();
        if n > 0 {
            rep.dropped.push((slot.name.to_owned(), n));
        }
    }
    // An emptied pbrMetallicRoughness is just noise.
    for m in &mut mats {
        if m.get("pbrMetallicRoughness").and_then(Value::as_object).is_some_and(|o| o.is_empty())
            && let Some(o) = m.as_object_mut()
        {
            o.remove("pbrMetallicRoughness");
        }
    }
    let after_refs: std::collections::BTreeSet<usize> = mats.iter().flat_map(texture_refs).collect();
    rep.textures_dropped = before_refs.difference(&after_refs).copied().collect();

    // 2. Snap untextured base colours to the palette (only when asked).
    if let Some(p) = palette {
        // The shader multiplies the factor by a vertex colour: the distance
        // check cannot see what a moved factor does to those.
        let vertex_coloured: std::collections::BTreeSet<usize> = out
            .json
            .get("meshes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|m| m.get("primitives").and_then(Value::as_array).into_iter().flatten())
            .filter(|p| p.pointer("/attributes/COLOR_0").is_some())
            .filter_map(|p| p.get("material").and_then(Value::as_u64).map(|m| m as usize))
            .collect();
        for (i, m) in mats.iter_mut().enumerate() {
            let name = name_of(m, i);
            if vertex_coloured.contains(&i) {
                rep.snap_skipped.push((name, "drawn with vertex colours (COLOR_0)".into()));
                continue;
            }
            if m.pointer("/pbrMetallicRoughness/baseColorTexture").is_some() {
                rep.snap_skipped.push((name, "has a base colour map".into()));
                continue;
            }
            let Some(f) = m.pointer("/pbrMetallicRoughness/baseColorFactor").and_then(Value::as_array).filter(|a| a.len() == 4) else {
                rep.snap_skipped.push((name, "no base colour factor".into()));
                continue;
            };
            let from: Vec<f32> = f.iter().filter_map(Value::as_f64).map(|x| x as f32).collect();
            if from.len() != 4 {
                rep.snap_skipped.push((name, "base colour factor is not numbers".into()));
                continue;
            }
            let from = [from[0], from[1], from[2], from[3]];
            let Some((swatch, de)) = nearest(from, p) else { continue };
            if de > SNAP_MAX_DELTA_E {
                rep.snap_skipped.push((name, format!("nearest swatch is {de:.1} away (limit {SNAP_MAX_DELTA_E})")));
                continue;
            }
            let to = [swatch[0], swatch[1], swatch[2], from[3]];
            if to != from {
                m["pbrMetallicRoughness"]["baseColorFactor"] = json!(to);
                rep.snapped.push(Snap { material: name, from, to, delta_e: de });
            }
        }
    }

    // 3. Merge materials equal in what is left (their names aside).
    let mut keep: Vec<usize> = Vec::new(); // new index -> position in `mats`
    let mut remap: Vec<usize> = Vec::with_capacity(mats.len());
    let stripped: Vec<Value> = mats
        .iter()
        .map(|m| {
            let mut c = m.clone();
            if let Some(o) = c.as_object_mut() {
                o.remove("name");
            }
            c
        })
        .collect();
    if let Some(why) = merge_blocker(&out.json) {
        rep.notes.push(format!("materials not merged: {why}"));
        remap.extend(0..mats.len());
        keep.extend(0..mats.len());
    } else {
        for i in 0..mats.len() {
            match keep.iter().position(|&k| stripped[k] == stripped[i]) {
                Some(n) => remap.push(n),
                None => {
                    remap.push(keep.len());
                    keep.push(i);
                }
            }
        }
        for (n, &k) in keep.iter().enumerate() {
            let folded: Vec<String> = (0..mats.len()).filter(|&i| i != k && remap[i] == n).map(|i| name_of(&mats[i], i)).collect();
            if !folded.is_empty() {
                rep.merged.push((name_of(&mats[k], k), folded));
            }
        }
    }
    rep.materials_after = keep.len();
    out.json["materials"] = Value::Array(keep.iter().map(|&k| mats[k].clone()).collect());
    if let Some(meshes) = out.json.get_mut("meshes").and_then(Value::as_array_mut) {
        for p in meshes.iter_mut().flat_map(|m| m.get_mut("primitives").and_then(Value::as_array_mut).into_iter().flatten()) {
            if let Some(m) = p.get("material").and_then(Value::as_u64) {
                p["material"] = Value::from(remap.get(m as usize).copied().unwrap_or(m as usize));
            }
        }
    }
    (out, rep)
}

/// The material slots a relief height map may be parked in, best first: two
/// the toon look never reads (a look that read them would be matched first,
/// and a slot already in use is skipped).
const PARK_SLOTS: [&str; 2] = ["/occlusionTexture", "/pbrMetallicRoughness/metallicRoughnessTexture"];

/// A relief height map is named only by `extras.evani.relief.height.index`,
/// which glTF-Transform's `prune` (and gltfpack) cannot see: they would drop
/// the picture and leave the index pointing at nothing. So before the clean it
/// is parked in a free standard slot of its material, marked in the recipe
/// (`height.parked` = the slot's pointer), and [`unpark_relief`] puts the index
/// back after packing. Returns the notes to show (a height map that found no
/// free slot is lost, and says so) and how many maps were parked; an error
/// for a material whose `pbrMetallicRoughness` is not an object.
pub fn park_relief(doc: &mut Doc) -> Result<(Vec<String>, usize), String> {
    let n_textures = doc.json.get("textures").and_then(Value::as_array).map_or(0, Vec::len);
    // Textures whose picture another map (a slot a material really has) also uses:
    // one picture gets one encoding, so such a height map is not encoded as height data.
    let source = |j: &Value, t: usize| j.pointer(&format!("/textures/{t}/source")).and_then(Value::as_u64);
    let used_images: std::collections::BTreeSet<u64> = doc.json.get("materials").and_then(Value::as_array).into_iter().flatten().flat_map(texture_refs).filter_map(|t| source(&doc.json, t)).collect();
    let shared: std::collections::BTreeSet<usize> = (0..n_textures).filter(|t| source(&doc.json, *t).is_some_and(|s| used_images.contains(&s))).collect();
    let mut notes = Vec::new();
    let mut parked = 0;
    let Some(mats) = doc.json.get_mut("materials").and_then(Value::as_array_mut) else { return Ok((notes, 0)) };
    for (i, m) in mats.iter_mut().enumerate() {
        let Some(h) = m.pointer("/extras/evani/relief/height/index").and_then(Value::as_u64) else { continue };
        if h as usize >= n_textures {
            continue; // the engine's own check names a height map that is not there
        }
        let name = name_of(m, i);
        if shared.contains(&(h as usize)) {
            notes.push(format!("relief: the height map of {name} is also used as another map; it ships as that map (its encoding), not as a lossless linear height map"));
        }
        let Some(slot) = PARK_SLOTS.iter().find(|s| m.pointer(s).is_none()) else {
            notes.push(format!("relief: the height map of {name} could not be kept (its occlusion and metal-rough slots are in use); the tier files carry no height map for it"));
            continue;
        };
        let Some((parent, key)) = slot.rsplit_once('/') else { continue };
        if parent.is_empty() {
            m[key] = json!({ "index": h });
        } else {
            // A missing `pbrMetallicRoughness` becomes an object; one that is
            // there but not an object is a broken file, said as an error
            // (indexing it would panic).
            let pbr = m.as_object_mut().map(|o| o.entry("pbrMetallicRoughness").or_insert_with(|| json!({})));
            let Some(pbr) = pbr.and_then(Value::as_object_mut) else {
                return Err(format!("material {name}: pbrMetallicRoughness is not an object"));
            };
            pbr.insert(key.to_owned(), json!({ "index": h }));
        }
        m["extras"]["evani"]["relief"]["height"]["parked"] = json!(slot);
        parked += 1;
    }
    Ok((notes, parked))
}

/// Does any material name a relief height map?
pub fn has_relief_height(j: &Value) -> bool {
    j.get("materials").and_then(Value::as_array).is_some_and(|ms| ms.iter().any(|m| m.pointer("/extras/evani/relief/height/index").is_some()))
}

/// Undo [`park_relief`] on a packed file: `height.index` becomes the texture
/// the parked slot now holds (packing may have renumbered the textures), and
/// the slot and the marker go. An error when a marked slot is gone: the height
/// map would be lost silently.
pub fn unpark_relief(j: &mut Value) -> Result<usize, String> {
    let mut n = 0;
    let Some(mats) = j.get_mut("materials").and_then(Value::as_array_mut) else { return Ok(0) };
    for (i, m) in mats.iter_mut().enumerate() {
        let Some(slot) = m.pointer("/extras/evani/relief/height/parked").and_then(Value::as_str).map(str::to_owned) else { continue };
        let idx = m.pointer(&format!("{slot}/index")).and_then(Value::as_u64).ok_or_else(|| format!("material {i}: the parked relief height map ({slot}) is gone after packing"))?;
        remove_pointer(m, &slot);
        if m.get("pbrMetallicRoughness").and_then(Value::as_object).is_some_and(|o| o.is_empty())
            && let Some(o) = m.as_object_mut()
        {
            o.remove("pbrMetallicRoughness");
        }
        if let Some(h) = m.pointer_mut("/extras/evani/relief/height").and_then(Value::as_object_mut) {
            h.remove("parked");
            h.insert("index".into(), Value::from(idx));
        }
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap_or(Path::new(".")).to_path_buf()
    }

    fn read(rel: &str) -> String {
        let p = root().join(rel);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    /// The text of `fn material(` in model.rs: everything the engine reads
    /// from a glTF material (the test module that follows is cut off).
    fn engine_material_fn() -> String {
        let src = read("crates/evani-render/src/model.rs");
        let src = src.split("#[cfg(test)]").next().unwrap_or(&src).to_owned();
        let start = src.find("fn material(").unwrap_or_else(|| panic!("model.rs has no fn material("));
        let rest = &src[start..];
        let end = rest.find("\n}\n").map_or(rest.len(), |e| e + 3);
        rest[..end].to_owned()
    }

    /// Texture bindings a shader declares in group 1 and samples.
    fn sampled(wgsl: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in wgsl.lines() {
            let l = line.trim();
            if let Some(i) = l.find("textureSample") {
                let args = &l[i..];
                if let Some(open) = args.find('(') {
                    let first: String = args[open + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !first.is_empty() && !out.contains(&first) {
                        out.push(first);
                    }
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn the_table_matches_what_the_engine_reads() {
        let body = engine_material_fn();
        let model = read("crates/evani-render/shaders/model.wgsl");
        let relief = read("crates/evani-render/shaders/model_relief.wgsl");
        // Every texture the shaders sample is a slot's binding, or the relief height map.
        let mut table: Vec<String> = SLOTS.iter().filter_map(|s| s.binding).map(str::to_owned).collect();
        table.push(RELIEF_BINDING.to_owned());
        table.sort();
        assert_eq!(sampled(&relief), table, "model_relief.wgsl samples these maps");
        let mut base = sampled(&model);
        base.sort();
        let mut want: Vec<String> = SLOTS.iter().filter_map(|s| s.binding).map(str::to_owned).collect();
        want.sort();
        assert_eq!(base, want, "model.wgsl samples these maps");
        // Every slot is read by fn material() exactly when a look row could list it.
        let read_by_engine: Vec<&str> = SLOTS.iter().filter(|s| body.contains(&format!("m.{}", s.field))).map(|s| s.name).collect();
        for r in &LOOKS {
            let mut listed: Vec<&str> = r.reads.to_vec();
            listed.sort_unstable();
            let mut engine = read_by_engine.clone();
            engine.sort_unstable();
            assert_eq!(listed, engine, "look {}: the `reads` list must be what fn material() reads", r.look);
            for n in r.reads {
                assert!(SLOTS.iter().any(|s| s.name == *n), "look {}: unknown slot {n}", r.look);
            }
        }
        // The relief recipe is read from the material's extras, never dropped.
        assert!(read("crates/evani-render/src/model/relief.rs").contains("\"relief\""));
    }

    #[test]
    fn a_looks_own_shader_slots_sample_no_textures() {
        // Toon's slot bodies get everything through the shared model shader's
        // three maps; its own WGSL must not sample another one.
        let dir = root().join("looks/evani-look-toon/src");
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).flatten() {
            if e.path().extension().is_some_and(|x| x == "rs" || x == "wgsl") {
                let t = std::fs::read_to_string(e.path()).unwrap_or_default();
                seen += 1;
                assert!(!t.contains("textureSample"), "{} samples a texture: the toon row in look.rs must say which", e.path().display());
            }
        }
        assert!(seen > 0);
    }

    fn mat(name: &str, mr: Value) -> Value {
        json!({ "name": name, "pbrMetallicRoughness": mr, "doubleSided": true })
    }

    fn doc(materials: Vec<Value>, prims: Vec<usize>) -> Doc {
        let prims: Vec<Value> = prims.into_iter().map(|m| json!({ "attributes": { "POSITION": 0 }, "material": m })).collect();
        Doc { json: json!({ "asset": { "version": "2.0" }, "materials": materials, "meshes": [{ "primitives": prims }], "nodes": [{ "name": "a", "mesh": 0 }], "scenes": [{ "nodes": [0] }] }), bin: Vec::new() }
    }

    #[test]
    fn drops_merges_and_remaps() {
        let toon = row("toon").unwrap();
        let d = doc(
            vec![
                mat("red", json!({ "baseColorFactor": [1.0, 0.0, 0.0, 1.0], "metallicFactor": 0.2, "roughnessFactor": 0.9 })),
                mat("also red", json!({ "baseColorFactor": [1.0, 0.0, 0.0, 1.0], "metallicFactor": 0.8 })),
                mat("blue", json!({ "baseColorFactor": [0.0, 0.0, 1.0, 1.0] })),
            ],
            vec![0, 1, 2, 1],
        );
        let (m, rep) = apply(&d, toon, None);
        assert_eq!((rep.materials_before, rep.materials_after), (3, 2));
        assert_eq!(rep.merged, vec![("red".to_owned(), vec!["also red".to_owned()])]);
        let prims: Vec<u64> = m.json["meshes"][0]["primitives"].as_array().unwrap().iter().map(|p| p["material"].as_u64().unwrap()).collect();
        assert_eq!(prims, [0, 0, 1, 0]);
        assert!(m.json["materials"][0]["pbrMetallicRoughness"].get("metallicFactor").is_none());
        assert_eq!(m.json["nodes"], d.json["nodes"]);
        assert_eq!(m.json["scenes"], d.json["scenes"]);
    }

    #[test]
    fn different_materials_stay_different() {
        let toon = row("toon").unwrap();
        let mut a = mat("a", json!({ "baseColorFactor": [1.0, 0.0, 0.0, 1.0] }));
        let mut b = a.clone();
        b["doubleSided"] = json!(false);
        let mut c = a.clone();
        c["emissiveFactor"] = json!([0.1, 0.1, 0.1]);
        a["name"] = json!("a");
        let (_, rep) = apply(&doc(vec![a, b, c], vec![0, 1, 2]), toon, None);
        assert_eq!(rep.materials_after, 3);
    }

    #[test]
    fn variants_block_the_merge() {
        let toon = row("toon").unwrap();
        let mut d = doc(vec![mat("a", json!({})), mat("b", json!({}))], vec![0, 1]);
        d.json["extensionsUsed"] = json!(["KHR_materials_variants"]);
        let (_, rep) = apply(&d, toon, None);
        assert_eq!(rep.materials_after, 2);
        assert!(rep.notes.iter().any(|n| n.contains("not merged")));
    }

    fn relief_doc(extra: Value) -> Doc {
        let mut m = json!({ "name": "plate", "normalTexture": { "index": 1 }, "extras": { "evani": { "relief": { "height": { "index": 2, "depth": 0.03 }, "micro": { "pattern": "knurl", "size": 0.02 } } } } });
        for (k, v) in extra.as_object().unwrap() {
            m[k] = v.clone();
        }
        let mut d = doc(vec![m], vec![0]);
        d.json["textures"] = json!([{ "source": 0 }, { "source": 1 }, { "source": 2 }]);
        d
    }

    #[test]
    fn a_relief_height_map_is_parked_and_comes_back() {
        let mut d = relief_doc(json!({}));
        let (notes, n) = park_relief(&mut d).unwrap();
        assert_eq!((n, notes.len()), (1, 0));
        assert_eq!(d.json["materials"][0]["occlusionTexture"]["index"], 2);
        assert_eq!(d.json["materials"][0]["extras"]["evani"]["relief"]["height"]["parked"], "/occlusionTexture");
        // Packing renumbered the textures: the slot says where the picture went.
        d.json["materials"][0]["occlusionTexture"]["index"] = json!(0);
        assert_eq!(unpark_relief(&mut d.json), Ok(1));
        let m = &d.json["materials"][0];
        assert!(m.get("occlusionTexture").is_none() && m["extras"]["evani"]["relief"]["height"].get("parked").is_none());
        assert_eq!(m["extras"]["evani"]["relief"]["height"]["index"], 0);
        assert_eq!(m["extras"]["evani"]["relief"]["micro"]["pattern"], "knurl");
    }

    #[test]
    fn parking_takes_the_second_slot_and_says_when_none_is_free() {
        let mut d = relief_doc(json!({ "occlusionTexture": { "index": 0 } }));
        assert_eq!(park_relief(&mut d).unwrap().1, 1);
        assert_eq!(d.json["materials"][0]["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"], 2);
        let mut full = relief_doc(json!({ "occlusionTexture": { "index": 0 }, "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 1 } } }));
        let (notes, n) = park_relief(&mut full).unwrap();
        assert_eq!(n, 0);
        assert!(notes[0].contains("could not be kept"), "{notes:?}");
        // A parked slot that packing lost is an error, never a silent loss.
        let mut gone = relief_doc(json!({}));
        park_relief(&mut gone).unwrap();
        gone.json["materials"][0].as_object_mut().unwrap().remove("occlusionTexture");
        assert!(unpark_relief(&mut gone.json).is_err());
    }

    #[test]
    fn a_broken_pbr_field_is_an_error_not_a_panic() {
        let mut d = relief_doc(json!({ "occlusionTexture": { "index": 0 }, "pbrMetallicRoughness": 5 }));
        let e = park_relief(&mut d).unwrap_err();
        assert!(e.contains("pbrMetallicRoughness is not an object"), "{e}");
    }

    #[test]
    fn a_height_map_shared_with_another_map_is_noted() {
        let mut d = relief_doc(json!({ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }));
        d.json["textures"] = json!([{ "source": 0 }, { "source": 1 }, { "source": 0 }]); // height (texture 2) is the base colour's picture
        let (notes, n) = park_relief(&mut d).unwrap();
        assert_eq!(n, 1);
        assert!(notes.iter().any(|n| n.contains("also used as another map")), "{notes:?}");
        let (clean, _) = park_relief(&mut relief_doc(json!({}))).unwrap();
        assert!(clean.is_empty(), "no note when the picture is its own");
    }

    #[test]
    fn snap_leaves_vertex_coloured_materials_alone() {
        let toon = row("toon").unwrap();
        let pal = palette("toon").unwrap();
        let near = [pal[0][0], pal[0][1], pal[0][2], 1.0];
        let mut d = doc(vec![mat("tinted", json!({ "baseColorFactor": near })), mat("plain", json!({ "baseColorFactor": near }))], vec![0, 1]);
        d.json["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"] = json!(1);
        let (m, rep) = apply(&d, toon, Some(&pal));
        assert!(rep.snap_skipped.iter().any(|(n, why)| n == "tinted" && why.contains("COLOR_0")), "{:?}", rep.snap_skipped);
        assert_eq!(m.json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"], json!(near));
    }

    #[test]
    fn snap_stays_within_the_limit_and_keeps_alpha() {
        let toon = row("toon").unwrap();
        let pal = palette("toon").unwrap();
        let near = [pal[0][0] * 0.97, pal[0][1] * 1.05, pal[0][2], 0.5];
        let d = doc(vec![mat("near", json!({ "baseColorFactor": near })), mat("far", json!({ "baseColorFactor": [0.02, 0.02, 0.02, 1.0] })), mat("textured", json!({ "baseColorTexture": { "index": 0 } }))], vec![0, 1, 2]);
        let (m, rep) = apply(&d, toon, Some(&pal));
        assert_eq!(rep.snapped.len(), 1, "{:?}", rep.snap_skipped);
        assert_eq!(rep.snapped[0].to[3], 0.5);
        assert!(rep.snapped[0].delta_e <= SNAP_MAX_DELTA_E);
        assert_eq!(rep.snap_skipped.len(), 2);
        assert!(m.json["materials"][1]["pbrMetallicRoughness"]["baseColorFactor"][0].as_f64().unwrap() < 0.05);
        // Not asked: nothing moves.
        let (_, none) = apply(&d, toon, None);
        assert!(none.snapped.is_empty());
    }
}
