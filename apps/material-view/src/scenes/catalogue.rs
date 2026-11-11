//! A catalogue material on its test shape (feature `material-catalogue`;
//! batch 1 build plan P14), drawn inside the art styles' scene in the style
//! that is open:
//!
//! * a style without a shape look (the engine's own light: Painterly 3D,
//!   Cinematic Realism...) draws the material exactly as a game's model
//!   does: the shape's glTF material is the catalogue row's
//!   (`evani_material_recipes::gltf::gltf_material`), so the recipe stage
//!   (or the row's family: cloth, skin, hair, eye) draws it;
//! * a style with a shape look (toon bands, outlines, pixel blocks, voxels,
//!   facets...) draws its treatment (treat.rs) fed with the row's colours A
//!   and B, its roughness and metalness. For a recipe row the treatment's
//!   surface parts also carry the row's block and a uv map, so the recipe
//!   stage's style detail step (evani-material-recipes `style`) keeps the
//!   row's own pattern inside the flat colours, simplified the style's way
//!   ([`style_of`]: banded and posterized, one sample per voxel cube, ink
//!   density on paper, glowing pattern edges on neon). The lines drawn over
//!   the shape (outline, neon trim) stay plain. The step's word is written
//!   each frame ([`RecipeStyle`]); its strength and step are live params
//!   (`kit.material_view.detail.<style>.*`).
//!
//! The model is built once per row, shape and style ([`Models::add_glb`],
//! then [`Models::replace_glb`] in place). A number, extra or condition
//! switch on a recipe row only rewrites the material's block
//! ([`BlockEdits::set_block`]): no model is rebuilt and no pipeline either.
//! A family row (no block) rebuilds its model when a switch moves.

use std::collections::HashMap;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::params::ParamRegistry;
use evani_core::plugin::PluginContext;
use evani_kit_material_view::catalogue::{CatalogueView, DetailParams, LibraryData};
use evani_kit_material_view::styles::{Finish, Render, Styles};
use evani_material_recipes::Block;
use evani_material_recipes::style::{Mode, RecipeStyle, Style as StyleDetail};
use evani_render::model::recipes::RECIPE_KEY;
use evani_material_recipes::catalog::Row;
use evani_material_recipes::gltf;
use evani_render::model::recipes::BlockEdits;
use evani_render::model::{ModelId, ModelInstance, Models};
use evani_render::{MeshId, RenderWorld};
use serde_json::{Value, json};

use super::common::{material, mesh};
use super::styles::{StylesScene, band_light, fnv, glb_fit, grounded, layout, mesh_from_glb, view_frame};
use crate::masks;
use crate::shapes::{self, Mesh, Pbr, Shape};
use crate::treat;

/// A proof's contact disc as a share of the scene shapes' (`styles::contact_radius`).
pub const PROOF_DISC: f32 = 0.7;
/// The streamed family models a catalogue row may sit on: the cushion scene's
/// velvet pouf and the head scene's bust.
pub const STREAMED: [(&str, &str); 2] = [("cushion", "layer-velvet"), ("head", "char-head")];

/// What the shape's model was last built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatKey {
    pub row: usize,
    pub shape: String,
    /// 0 for the material path; else the style, its look and the light.
    pub treat: u64,
    /// What else forces a rebuild: the written material for a family row
    /// (it has no block), the finish for a treated style. 0 for a recipe row
    /// on the material path, whose switches only rewrite its block.
    pub content: u64,
}

/// wasm: a fetch's result, filled in when the file arrives.
#[cfg(target_arch = "wasm32")]
type Fetched = Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>>;

/// A streamed family model's file, while it is read.
#[derive(Debug)]
enum Raw {
    #[cfg(target_arch = "wasm32")]
    Fetching(Fetched),
    Ready(Rc<Vec<u8>>),
    Failed,
}

/// The scene's state (a world resource).
#[derive(Debug, Default)]
pub struct CatScene {
    pub model: Option<ModelId>,
    pub key: Option<CatKey>,
    /// Models built (adds and replaces).
    pub builds: u64,
    /// Blocks rewritten live (a switch on a recipe row).
    pub block_edits: u64,
    /// The block the model carries now.
    pub block: Option<Block>,
    /// The row drawn this frame, with its model once ready.
    pub drawn: Option<(usize, Option<ModelId>)>,
    /// How many parts the model has (1 on the material path, more for a look).
    pub parts: usize,
    /// How the model sits: its centre and the scale that makes it radius 1
    /// (a family model drawn as its file has it); `None` for unit shapes.
    pub fit: Option<([f32; 3], f32)>,
    /// The written material the model carries (material path only).
    pub material: Option<Value>,
    raw: HashMap<&'static str, Raw>,
    handles: HashMap<&'static str, evani_render::model::ModelHandle>,
    meshes: HashMap<String, Rc<Mesh>>,
    /// A shape's own uvs, for one made of several cards (the rose: each
    /// petal in its own card space, as the mask wants).
    uvs: HashMap<String, Rc<Vec<[f32; 2]>>>,
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(CatScene::default());
    ctx.resource(RecipeStyle::default());
}

/// Reserve the open material's model id before any other model is added
/// (the styles scene calls it on its first frame): a one-tile stand-in that
/// every open row then replaces in place. Lite and Low show a model only
/// once its sun cones are baked, and the bake takes waiting models in the
/// order they were added, one at a time; an id added on first open queued
/// behind every model before it and a row could wait well over a minute
/// to show (material-view round 3: the Low proofs were grey stand-in
/// spheres). At the front of the queue it is baked first.
pub fn reserve(models: &mut Models, st: &mut CatScene) {
    if st.model.is_none() {
        let pbr = Pbr { base: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.8, emissive: [0.0; 3] };
        st.model = Some(models.add_glb("catalogue/open.glb", &shapes::glb(&shapes::tile(), &pbr)));
    }
}

/// No style detail this frame (the styles scene calls it first; [`draw`]
/// sets the word again while a recipe row is open in a shaped style).
pub fn style_off(world: &World) {
    if let Some(mut s) = world.try_res_mut::<RecipeStyle>() {
        s.0 = 0;
    }
}

/// The style detail step's settings for style `r` at model scale `ps`:
/// dither keeps the pattern as ink density, neon as glowing edges, every
/// other look as tone; the pattern's hue is kept as far as the style keeps
/// colour (`1 - desaturate`); a voxel style samples once per cube, the dither
/// lays its ordered pattern at half its cell. `detail` is the live (strength,
/// step).
pub fn style_of(r: &Render, detail: (f32, f32), ps: f32) -> StyleDetail {
    let mode = if r.dither {
        Mode::Dither
    } else if r.outline_glow {
        Mode::Neon
    } else {
        Mode::Tone
    };
    let cell = if r.voxel > 0.0 {
        r.voxel * ps
    } else if r.dither {
        0.5 * treat::DITHER_CELL * ps
    } else {
        0.0
    };
    StyleDetail { mode, strength: detail.0, step: detail.1, hue: 1.0 - r.desaturate.clamp(0.0, 1.0), cell }
}

/// Whether a catalogue material is open.
pub fn is_open(world: &World) -> bool {
    world.try_res::<CatalogueView>().is_some_and(|v| v.open.is_some() && v.edited.is_some())
}

/// The finish a style's treatment gets from a row: colour A mixed a third of
/// the way to colour B (so a two-colour material keeps some of its second
/// colour), the row's roughness and metalness, and a glow for the light
/// recipes (fire, glow).
pub fn finish_of(written: &Value, colours: &[[u8; 3]; 3], recipe: &str) -> Finish {
    let lin = |c: u8| gltf::linear(c);
    let a = colours[0].map(lin);
    let b = colours[1].map(lin);
    let tint = [0, 1, 2].map(|k| a[k] * 0.67 + b[k] * 0.33);
    let pbr = written.get("pbrMetallicRoughness");
    let f = |k: &str, or: f32| pbr.and_then(|p| p.get(k)).and_then(Value::as_f64).map_or(or, |x| x as f32);
    let glow = if matches!(recipe, "fire" | "glow") { 0.8 } else { 0.0 };
    Finish { tint, roughness: f("roughnessFactor", 0.5).clamp(0.04, 1.0), metallic: f("metallicFactor", 0.0).clamp(0.0, 1.0), bump: 0.0, glow }
}

/// A mesh's uvs and tangents (one each per vertex).
pub type UvMap = (Vec<[f32; 2]>, Vec<[f32; 4]>);

/// A uv map for a mesh made in code (or read from a file without one):
/// round shapes wrap u once around and v from top to bottom; flat ones
/// project along their face's main axis. With tangents along u (w = 1).
pub fn uv_map(shape: &str, m: &Mesh) -> UvMap {
    let uvs: Vec<[f32; 2]> = m.positions.iter().zip(&m.normals).map(|(p, n)| uv_at(shape, *p, *n)).collect();
    let tangents = tangents_of(m, &uvs);
    (uvs, tangents)
}

/// The uv of point `p` with normal `n` on `shape` ([`uv_map`]'s rule).
fn uv_at(shape: &str, p: [f32; 3], n: [f32; 3]) -> [f32; 2] {
    use std::f32::consts::{PI, TAU};
    let round = |p: &[f32; 3]| p[2].atan2(p[0]) / TAU + 0.5;
    match shape {
        // A leaf card: u across, v from the tip (top) down, the mask's way.
        s if masks::CARDS.contains(&s) => [p[0] / 1.1 + 0.5, 0.5 - p[1] / 1.5],
        "ball" | "rock" | "blob" => {
            let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-6);
            [round(&p) * 2.0, (p[1] / l).clamp(-1.0, 1.0).acos() / PI]
        }
        "cylinder" if n[1].abs() < 0.5 => [round(&p) * 2.0, 0.5 - p[1] / 1.5],
        "ring" => {
            let r = (p[0] * p[0] + p[2] * p[2]).sqrt();
            [round(&p) * 3.0, p[1].atan2(r - 0.72) / TAU + 0.5]
        }
        _ => {
            let a = n.map(f32::abs);
            let (u, v) = if a[1] >= a[0] && a[1] >= a[2] {
                (p[0], -p[2])
            } else if a[0] >= a[2] {
                (-p[2] * n[0].signum(), -p[1])
            } else {
                (p[0] * n[2].signum(), -p[1])
            };
            [u * 0.5 + 0.5, v * 0.5 + 0.5]
        }
    }
}

/// A voxel model's uvs: every corner of a cube face takes the uv of its
/// cube's centre (cubes `v` wide, centred on multiples of `v`), so the
/// recipe gives each cube one colour, as a pixel-art block.
pub fn voxel_uv_map(shape: &str, m: &Mesh, v: f32) -> UvMap {
    let mut uvs = vec![[0.0f32; 2]; m.positions.len()];
    for tri in m.indices.as_chunks::<3>().0 {
        let t = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if t.iter().any(|&i| i >= m.positions.len()) {
            continue;
        }
        let c = shapes::scale(shapes::add(shapes::add(m.positions[t[0]], m.positions[t[1]]), m.positions[t[2]]), 1.0 / 3.0);
        let n = m.normals.get(t[0]).copied().unwrap_or([0.0, 1.0, 0.0]);
        // Half a cube in from the face, then to the cube's centre.
        let inside = shapes::sub(c, shapes::scale(n, 0.5 * v));
        let centre = if v > 0.0 { inside.map(|x| (x / v).round() * v) } else { inside };
        let uv = uv_at(shape, centre, n);
        for i in t {
            uvs[i] = uv;
        }
    }
    let tangents = tangents_of(m, &uvs);
    (uvs, tangents)
}

/// Tangents along u for uvs made elsewhere (a code-built shape's own
/// layout): [`tangents_of`].
#[cfg(feature = "material-studio")]
pub fn tangents_for(m: &Mesh, uvs: &[[f32; 2]]) -> Vec<[f32; 4]> {
    tangents_of(m, uvs)
}

/// Tangents along u, from each triangle's uv slope summed per vertex (any
/// direction along the surface where the slope vanishes), w = 1.
fn tangents_of(m: &Mesh, uvs: &[[f32; 2]]) -> Vec<[f32; 4]> {
    let mut t = vec![[0.0f32; 3]; m.positions.len()];
    for tri in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if [a, b, c].iter().any(|&i| i >= m.positions.len() || i >= uvs.len()) {
            continue;
        }
        let (pa, pb, pc) = (m.positions[a], m.positions[b], m.positions[c]);
        let (ua, ub, uc) = (uvs[a], uvs[b], uvs[c]);
        let e1 = shapes::sub(pb, pa);
        let e2 = shapes::sub(pc, pa);
        let (du1, dv1, du2, dv2) = (ub[0] - ua[0], ub[1] - ua[1], uc[0] - ua[0], uc[1] - ua[1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 {
            continue;
        }
        let dir = shapes::scale(shapes::sub(shapes::scale(e1, dv2), shapes::scale(e2, dv1)), 1.0 / det);
        for i in [a, b, c] {
            t[i] = shapes::add(t[i], dir);
        }
    }
    t.iter()
        .zip(&m.normals)
        .map(|(t, n)| {
            // Along the surface, unit length; any direction when it vanished.
            let flat = shapes::sub(*t, shapes::scale(*n, shapes::dot(*t, *n)));
            let fallback = if n[1].abs() < 0.9 { shapes::cross([0.0, 1.0, 0.0], *n) } else { [1.0, 0.0, 0.0] };
            let u = shapes::normalize(flat, shapes::normalize(fallback, [1.0, 0.0, 0.0]));
            [u[0], u[1], u[2], 1.0]
        })
        .collect()
}

/// A .glb of `m` (positions, normals, uvs, tangents, u32 indices) with one
/// material, `material` (a glTF material object).
pub fn uv_glb(m: &Mesh, uvs: &[[f32; 2]], tangents: &[[f32; 4]], material: &Value) -> Result<Vec<u8>, String> {
    uv_glb_png(m, uvs, tangents, material, None)
}

/// [`uv_glb`] with a base colour texture: `png` is embedded and the material
/// (which must name texture 0) points at it (a leaf card's alpha mask).
pub fn uv_glb_png(m: &Mesh, uvs: &[[f32; 2]], tangents: &[[f32; 4]], material: &Value, png: Option<&[u8]>) -> Result<Vec<u8>, String> {
    uv_glb_parts_png(&[UvPart { mesh: m, uvs, tangents, material: material.clone() }], png)
}

/// One part of a [`uv_glb_parts`] model.
pub struct UvPart<'a> {
    pub mesh: &'a Mesh,
    pub uvs: &'a [[f32; 2]],
    pub tangents: &'a [[f32; 4]],
    /// A glTF material object.
    pub material: Value,
}

/// A .glb of several parts in one mesh: one primitive and one material per
/// part, in order, each with positions, normals, uvs, tangents and u32
/// indices. A part with no triangles keeps its material but gets no
/// primitive.
pub fn uv_glb_parts(parts: &[UvPart]) -> Result<Vec<u8>, String> {
    uv_glb_parts_png(parts, None)
}

/// [`uv_glb_parts`] with texture 0 embedded from `png` (a leaf card's alpha
/// mask), for the materials that name it.
pub fn uv_glb_parts_png(parts: &[UvPart], png: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut primitives = Vec::new();
    let mut push = |bytes: Vec<u8>, bin: &mut Vec<u8>| {
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len()}));
        bin.extend_from_slice(&bytes);
        views.len() - 1
    };
    let f = |v: f32| if v.is_finite() { v } else { 0.0 };
    let acc = |view: usize, ty: &str, count: usize, comp: u32| json!({"bufferView": view, "componentType": comp, "count": count, "type": ty});
    for (k, p) in parts.iter().enumerate() {
        let m = p.mesh;
        let n = m.positions.len();
        if n == 0 || m.indices.is_empty() {
            continue;
        }
        if p.uvs.len() != n || p.tangents.len() != n || m.normals.len() != n {
            return Err("catalogue shape: mismatched vertex lists".into());
        }
        let at = accessors.len();
        let lo = m.positions.iter().fold([f32::MAX; 3], |a, p| [a[0].min(p[0]), a[1].min(p[1]), a[2].min(p[2])]);
        let hi = m.positions.iter().fold([f32::MIN; 3], |a, p| [a[0].max(p[0]), a[1].max(p[1]), a[2].max(p[2])]);
        let mut pos = acc(push(m.positions.iter().flatten().flat_map(|x| f(*x).to_le_bytes()).collect(), &mut bin), "VEC3", n, 5126);
        pos["min"] = json!(lo);
        pos["max"] = json!(hi);
        accessors.push(pos);
        accessors.push(acc(push(m.normals.iter().flatten().flat_map(|x| f(*x).to_le_bytes()).collect(), &mut bin), "VEC3", n, 5126));
        accessors.push(acc(push(p.uvs.iter().flatten().flat_map(|x| f(*x).to_le_bytes()).collect(), &mut bin), "VEC2", n, 5126));
        accessors.push(acc(push(p.tangents.iter().flatten().flat_map(|x| f(*x).to_le_bytes()).collect(), &mut bin), "VEC4", n, 5126));
        accessors.push(acc(push(m.indices.iter().flat_map(|i| i.to_le_bytes()).collect(), &mut bin), "SCALAR", m.indices.len(), 5125));
        primitives.push(json!({"attributes": {"POSITION": at, "NORMAL": at + 1, "TEXCOORD_0": at + 2, "TANGENT": at + 3}, "indices": at + 4, "material": k, "mode": 4}));
    }
    if primitives.is_empty() {
        return Err("catalogue shape: no triangles".into());
    }
    let image = png.map(|png| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        push(png.to_vec(), &mut bin)
    });
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "Evani.Dev material-view catalogue"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "shape"}],
        "meshes": [{"primitives": primitives}],
        "materials": parts.iter().map(|p| p.material.clone()).collect::<Vec<_>>(),
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}],
    });
    if let Some(view) = image {
        doc["images"] = json!([{"bufferView": view, "mimeType": "image/png"}]);
        doc["textures"] = json!([{"source": 0}]);
    }
    gltf::glb_build(&doc, Some(&bin))
}

/// A treated part's glTF material: its flat colour or lit finish (as
/// `shapes::glb_parts` writes it) plus, on a surface part, the recipe row's
/// block (`extras.evani_recipe`), which the recipe stage's style detail step
/// reads. Lines over the shape (outline, neon trim) get no block.
fn part_material(p: &treat::Part, block: &Value) -> Value {
    let mut m: Value = serde_json::from_str(&shapes::material_json(&p.pbr, p.double_sided)).unwrap_or_else(|_| json!({"name": "material"}));
    if p.body {
        m["extras"] = json!({ RECIPE_KEY: block });
    }
    m
}

/// The material index of a streamed family model a row fills: the head's
/// `skin`, `eye` or `hair` part by the row's recipe; else the first.
fn target_material(doc: &Value, recipe: &str) -> usize {
    doc.get("materials")
        .and_then(Value::as_array)
        .and_then(|ms| ms.iter().position(|m| m.get("name").and_then(Value::as_str) == Some(recipe)))
        .unwrap_or(0)
}

impl CatScene {
    /// The bytes of a streamed family model, once in.
    fn raw(&mut self, shape: &'static str, id: &'static str, models: &mut Models, log: &mut Vec<String>) -> Option<Rc<Vec<u8>>> {
        if let Some(Raw::Ready(b)) = self.raw.get(shape) {
            return Some(b.clone());
        }
        if matches!(self.raw.get(shape), Some(Raw::Failed)) {
            return None;
        }
        let h = *self.handles.entry(shape).or_insert_with(|| models.handle(id));
        let mid = models.resolve(h)?;
        let url = models.url(mid)?.to_string();
        let got: Option<Result<Vec<u8>, String>> = match self.raw.get(shape) {
            #[cfg(target_arch = "wasm32")]
            Some(Raw::Fetching(cell)) => cell.borrow_mut().take(),
            _ => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/");
                    let file = url.strip_prefix(evani_render::model::stream::FOLDER).unwrap_or(&url);
                    Some(std::fs::read(format!("{DIR}{file}")).map_err(|e| format!("{file}: {e}")))
                }
                #[cfg(target_arch = "wasm32")]
                {
                    use evani_core::assets::AssetSource;
                    let cell: Fetched = Rc::new(std::cell::RefCell::new(None));
                    let into = cell.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let r = evani_core::web::FetchSource.fetch(&url).await;
                        *into.borrow_mut() = Some(r);
                    });
                    self.raw.insert(shape, Raw::Fetching(cell));
                    None
                }
            }
        };
        match got? {
            Ok(b) => {
                let b = Rc::new(b);
                self.raw.insert(shape, Raw::Ready(b.clone()));
                Some(b)
            }
            Err(e) => {
                self.raw.insert(shape, Raw::Failed);
                log.push(format!("material-view: catalogue: the {shape} model: {e}"));
                None
            }
        }
    }

    /// The shape's mesh (centred, about radius 1), once it is in.
    fn mesh(&mut self, shape: &str, styles: &mut StylesScene, models: &mut Models, log: &mut Vec<String>) -> Option<Rc<Mesh>> {
        if let Some(m) = self.meshes.get(shape) {
            return Some(m.clone());
        }
        let m = match shape {
            "tile" => Rc::new(shapes::tile()),
            "card" => Rc::new(shapes::card()),
            "slope" => Rc::new(shapes::slope()),
            "lawn" => Rc::new(shapes::lawn()),
            "rock" => Rc::new(shapes::rock()),
            "leaf" | "spray" | "tuft" | "oak_leaf" | "maple_leaf" | "wheat" | "hay" | "meadow" | "pine" | "spruce" | "fir" => Rc::new(shapes::leaf_card()),
            "petal" => Rc::new(shapes::petal_card()),
            "crystal" => Rc::new(shapes::crystal()),
            "blob" => Rc::new(shapes::blob()),
            "slab" => Rc::new(shapes::slab()),
            "pie" => Rc::new(shapes::pie()),
            "rose" => {
                let (m, uvs) = shapes::rose();
                self.uvs.insert(shape.to_string(), Rc::new(uvs));
                Rc::new(m)
            }
            "cushion" | "head" => {
                let (s, id) = STREAMED.iter().find(|(s, _)| *s == shape).copied()?;
                let raw = self.raw(s, id, models, log)?;
                match mesh_from_glb(&raw) {
                    Ok(m) => Rc::new(m),
                    Err(e) => {
                        log.push(format!("material-view: catalogue: the {shape} model: {e}"));
                        return None;
                    }
                }
            }
            other => {
                let s = match other {
                    "cube" => Shape::Cube,
                    "cylinder" => Shape::Cylinder,
                    "monkey" => Shape::Monkey,
                    "teapot" => Shape::Teapot,
                    "ring" => Shape::Ring,
                    _ => Shape::Sphere,
                };
                styles.base(s, models, log)?
            }
        };
        self.meshes.insert(shape.to_string(), m.clone());
        Some(m)
    }

    /// The model's bytes for `key`: the row's material on the shape, or the
    /// style's treatment. `None` while the shape isn't in.
    #[allow(clippy::too_many_arguments)]
    fn bytes(&mut self, key: &CatKey, written: &Value, recipe: &str, look: Option<(&Render, [f32; 3])>, fin: &Finish, styles: &mut StylesScene, models: &mut Models, log: &mut Vec<String>) -> Option<(Vec<u8>, usize)> {
        if let Some((r, light)) = look {
            let m = self.mesh(&key.shape, styles, models, log)?;
            let geo = treat::geometry(&m, r, 0.0);
            let parts = treat::shade(&geo, r, fin, light);
            // A recipe row: its block on the surface parts, with uvs (one per
            // voxel cube), so its pattern shows inside the style.
            let Some(block) = written.get("extras").and_then(|e| e.get(RECIPE_KEY)) else {
                let list: Vec<(&Mesh, &Pbr, bool)> = parts.iter().map(|p| (&p.mesh, &p.pbr, p.double_sided)).collect();
                return Some((shapes::glb_parts(&list), parts.len()));
            };
            let maps: Vec<UvMap> = parts
                .iter()
                .map(|p| if r.voxel > 0.0 { voxel_uv_map(&key.shape, &p.mesh, r.voxel) } else { uv_map(&key.shape, &p.mesh) })
                .collect();
            let list: Vec<UvPart> = parts
                .iter()
                .zip(&maps)
                .map(|(p, (uvs, tangents))| UvPart { mesh: &p.mesh, uvs, tangents, material: part_material(p, block) })
                .collect();
            return match uv_glb_parts(&list) {
                Ok(b) => Some((b, parts.len())),
                Err(e) => {
                    log.push(e);
                    None
                }
            };
        }
        // A family model keeps its own uvs and parts; the row fills its part.
        if let Some((s, id)) = STREAMED.iter().find(|(s, _)| *s == key.shape).copied() {
            let raw = self.raw(s, id, models, log)?;
            // Its unit mesh too: where its lowest point sits (grounding).
            self.mesh(&key.shape, styles, models, log)?;
            let at = gltf::glb_parts(&raw).ok().map_or(0, |(doc, _)| target_material(&doc, recipe));
            return match gltf::glb_with_material(&raw, Some(at), written) {
                Ok(b) => Some((b, 1)),
                Err(e) => {
                    log.push(format!("material-view: catalogue: {e}"));
                    None
                }
            };
        }
        let m = self.mesh(&key.shape, styles, models, log)?;
        let (uvs, tangents) = match self.uvs.get(&key.shape) {
            Some(own) if own.len() == m.positions.len() => (own.to_vec(), tangents_of(&m, own)),
            _ => uv_map(&key.shape, &m),
        };
        let result = if let Some(mask) = masks::mask_of(&key.shape) {
            // A card is cut to its outline by the base texture's alpha, and
            // seen from both sides.
            let mut mat = written.clone();
            mat["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": 0});
            mat["alphaMode"] = json!("MASK");
            mat["alphaCutoff"] = json!(0.5);
            mat["doubleSided"] = json!(true);
            uv_glb_png(&m, &uvs, &tangents, &mat, Some(&masks::png(mask)))
        } else {
            uv_glb(&m, &uvs, &tangents, written)
        };
        match result {
            Ok(b) => Some((b, 1)),
            Err(e) => {
                log.push(e);
                None
            }
        }
    }
}

/// The written glTF material of an edited row (an error is logged once by
/// the caller and draws nothing).
fn written(lib: &LibraryData, edited: &Row) -> Result<Value, String> {
    gltf::gltf_material(&lib.0.cat, edited).map_err(|e| format!("material-view: catalogue: {e}"))
}

/// Draw the open row on its shape in style `style` (called by the styles
/// scene in place of its own set).
pub fn draw(world: &mut World, styles: &Styles, style: usize) {
    let Some(view) = world.try_res::<CatalogueView>().map(|v| v.clone()) else { return };
    let (Some(i), Some(edited)) = (view.open, view.edited.as_ref()) else { return };
    let Some(lib) = world.try_res::<LibraryData>().map(|l| l.clone()) else { return };
    let Some(row) = lib.0.cat.rows.get(i) else { return };
    let Ok(recipe) = lib.0.cat.recipe_for(row) else { return };
    let mut log = Vec::new();
    let mat = match written(&lib, edited) {
        Ok(m) => m,
        Err(e) => {
            world.res_mut::<ErrorLog>().error(e);
            return;
        }
    };
    let colours = lib.0.cat.encode(edited).map(|b| b.unpack().colours).unwrap_or([[128; 3]; 3]);
    let fin = finish_of(&mat, &colours, &recipe.id);
    let render = styles.styles.get(style).and_then(|s| s.render.as_ref()).filter(|r| r.shaped());
    let light = band_light(world, render);
    let treat_key = render.map_or(0, |r| {
        let sun = light.map(|x| (x / 0.02).round() as i32);
        fnv(format!("{style}|{sun:?}|{r:?}").as_bytes()) | 1
    });
    let content = if render.is_some() {
        // The block too: the treated parts carry it (no live block edit).
        fnv(format!("{:?}", (fin.tint, fin.roughness, fin.metallic, fin.glow, view.block.map(|b| b.to_hex()))).as_bytes())
    } else if view.block.is_some() {
        0
    } else {
        fnv(mat.to_string().as_bytes())
    };
    let key = CatKey { row: i, shape: view.shape.clone(), treat: treat_key, content };
    let Some((cam, canvas, free)) = view_frame(world) else { return };
    let mut placeholder = None;
    let at;
    {
        let Some(mut models) = world.try_res_mut::<Models>() else { return };
        let Some(mut styles_st) = world.try_res_mut::<StylesScene>() else { return };
        let Some(mut st) = world.try_res_mut::<CatScene>() else { return };
        super::native_stream(world, &mut models);
        let bottom = st.meshes.get(&view.shape).map_or(-1.0, |m| m.positions.iter().map(|p| p[1]).fold(0.0_f32, f32::min));
        let shadow = super::styles::contact_radius(st.meshes.get(&view.shape).map(|m| m.as_ref()));
        let (planned, s) = layout(1, &cam, canvas, free);
        let placed = grounded(&planned, s, &cam, bottom);
        let Some(&(mut p, ps)) = placed.first() else { return };
        // The style detail step for a recipe row in a shaped style: its word
        // this frame; a voxel model moves (less than half a cube) so its
        // cubes are the shader's cells.
        if let Some(r) = render
            && view.block.is_some()
        {
            let id = styles.styles.get(style).map_or("", |s| s.id.as_str());
            let live = world.try_res::<DetailParams>().zip(world.try_res::<ParamRegistry>()).and_then(|(d, reg)| d.get(&reg, id));
            let sd = style_of(r, live.unwrap_or((r.detail, r.detail_step)), ps);
            if r.voxel > 0.0 {
                p = p.map(|x| sd.cell_centre(x));
            }
            if let Some(mut w) = world.try_res_mut::<RecipeStyle>() {
                w.0 = sd.pack();
            }
        }
        at = (p, ps, bottom, shadow);
        if st.key.as_ref() != Some(&key) || st.model.is_none() {
            let look = render.map(|r| (r, light));
            if let Some((bytes, parts)) = st.bytes(&key, &mat, &recipe.id, look, &fin, &mut styles_st, &mut models, &mut log) {
                st.builds += 1;
                st.parts = parts;
                st.model = Some(match st.model {
                    Some(m) => {
                        models.replace_glb(m, &bytes);
                        m
                    }
                    None => models.add_glb("catalogue/open.glb", &bytes),
                });
                // A family model on the material path is drawn as its file
                // has it: move and scale it like the unit shapes.
                st.fit = None;
                if render.is_none()
                    && let Some((sh, _)) = STREAMED.iter().find(|(s, _)| *s == key.shape)
                    && let Some(Raw::Ready(raw)) = st.raw.get(sh)
                    && let Ok((c, r)) = glb_fit(raw)
                    && r > 1e-6
                {
                    st.fit = Some((c, 1.0 / r));
                }
                st.key = Some(key);
                // The new model carries the edited row's block already.
                st.block = if render.is_none() { view.block } else { None };
                st.material = render.is_none().then(|| mat.clone());
            }
        } else if render.is_none()
            && view.block.is_some()
            && st.block != view.block
            && let (Some(m), Some(b)) = (st.model, view.block)
        {
            // A switch moved on a recipe row: only its block changes.
            world.res_mut::<BlockEdits>().set_block(m, 0, &b);
            st.block = Some(b);
            st.block_edits += 1;
        }
        let id = st.model.filter(|&m| st.key.is_some() && models.ready(m));
        match id {
            Some(m) => {
                let (c, k) = st.fit.unwrap_or(([0.0; 3], 1.0));
                let s = ps * k;
                let at = [p[0] - c[0] * s, p[1] - c[1] * s, p[2] - c[2] * s];
                models.draw(ModelInstance::new(m, at).scale([s; 3]));
            }
            None => placeholder = Some((p, ps, fin.tint)),
        }
        st.drawn = Some((i, id));
    }
    super::StandIns::add(world, usize::from(placeholder.is_some()));
    let stone = material(world, "stone");
    let mut rw = world.res_mut::<RenderWorld>();
    if let Some((p, ps, t)) = placeholder {
        rw.draw_mesh(mesh(MeshId::SPHERE, p, [1.8 * ps; 3], stone, [t[0], t[1], t[2], 1.0]));
    }
    // A contact shadow where the shape meets the floor (one material alone
    // needs no selection marker; the old glowing disc read as a pale blob).
    {
        let (p, ps, bottom, shadow) = at;
        // The disc is 0.7 of the scene shapes' (critic round 7: on Low a
        // flat slab's 1.8x-footprint disc dwarfed its short sideways cast, so
        // fish, cheese and jerky got a round smudge where Ultra casts left).
        if p[1] + bottom * ps < 0.05 {
            super::common::contact(&mut rw, [p[0], 0.0, p[2]], PROOF_DISC * shadow * ps, -2.0 * bottom * ps);
        }
    }
    drop(rw);
    if !log.is_empty() {
        let mut errors = world.res_mut::<ErrorLog>();
        for e in log {
            errors.error(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::{ParamRegistry, ParamValue};
    use evani_kit_material_view::catalogue::{OPEN, SHAPE, cond_param, number_param};
    use evani_kit_material_view::styles::{StyleCommand, StyleView, StylesData};
    use evani_kit_material_view::{Command, Viewer};

    const DT: f64 = 1000.0 / 60.0;

    fn open(id: &str) -> evani_core::App {
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene("styles".into()));
        app.run_headless(3, DT);
        let _ = app.world.res_mut::<ParamRegistry>().set_json(OPEN, &json!(id));
        app.run_headless(8, DT);
        app
    }

    fn errors(app: &evani_core::App) -> u32 {
        app.world.res::<ErrorLog>().error_count()
    }

    /// The first style with no shape look (the material path).
    fn plain_style(app: &evani_core::App) -> usize {
        app.world.res::<StylesData>().0.styles.iter().position(|s| s.render.as_ref().is_none_or(|r| !r.shaped())).unwrap()
    }

    fn style(app: &mut evani_core::App, i: usize) {
        app.world.res_mut::<StyleView>().push(StyleCommand::Style(i));
        app.run_headless(4, DT);
    }

    /// A fur row opens on the shader ball, through the recipe stage, with
    /// only its own conditions on offer; a slider changes its block without
    /// rebuilding the model.
    #[test]
    fn a_fur_row_opens_and_a_slider_only_rewrites_its_block() {
        let mut app = open("horse.coat.bay");
        let p = plain_style(&app);
        style(&mut app, p);
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
        {
            let st = app.world.res::<CatScene>();
            let (_, id) = st.drawn.unwrap();
            assert!(id.is_some(), "the model is ready");
            assert_eq!(st.key.as_ref().unwrap().shape, "ball");
            let m = st.material.as_ref().unwrap();
            assert_eq!(m["extras"]["evani_material"], "horse.coat.bay");
            assert!(m["extras"]["evani_recipe"].as_str().is_some_and(|h| h.len() == 64));
        }
        let lib = app.world.res::<LibraryData>().clone();
        let (_, row) = lib.0.find("horse.coat.bay").unwrap();
        assert!(!row.conditions.contains(&"oxide".to_string()), "fur lists no rust");
        let builds = app.world.res::<CatScene>().builds;
        let _ = app.world.res_mut::<BlockEdits>().take();
        let _ = app.world.res_mut::<ParamRegistry>().set_by_name(&number_param("pattern_contrast"), ParamValue::F32(1.0));
        let _ = app.world.res_mut::<ParamRegistry>().set_by_name(&cond_param("snow", "on"), ParamValue::Bool(true));
        app.run_headless(3, DT);
        let st = app.world.res::<CatScene>();
        assert_eq!(st.builds, builds, "no model rebuilt for a slider");
        assert!(st.block_edits >= 1);
        drop(st);
        let edits = app.world.res_mut::<BlockEdits>().take();
        let last = edits.last().expect("a block edit was queued");
        let b = Block::from_hex(&Block(bytes_of(last.2)).to_hex()).unwrap().unpack();
        assert_eq!(b.cond_mask & 0b1000, 0b1000, "snow on");
        assert_eq!(app.world.res::<CatalogueView>().block.unwrap().unpack().cond_mask, b.cond_mask);
    }

    fn bytes_of(words: [u32; 8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, w) in words.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    /// Velvet opens on the cushion; since B4-P3b the recipe stage draws all
    /// of it (pile and sheen), so it carries its block and no layers data.
    #[test]
    fn a_family_row_opens_in_its_family_scene() {
        let mut app = open("cloth.velvet.red");
        let p = plain_style(&app);
        style(&mut app, p);
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
        let st = app.world.res::<CatScene>();
        assert_eq!(st.key.as_ref().unwrap().shape, "cushion");
        assert!(st.drawn.unwrap().1.is_some(), "the pouf streamed in and drew");
        let m = st.material.as_ref().unwrap();
        assert!(m["extras"].get("evani_recipe").is_some(), "{m}");
        assert!(m["extras"].get("evani_layers").is_none(), "{m}");
        assert!(st.block.is_some());
        // Drawn as its file has it, so it is moved and scaled like the unit
        // shapes (it stands on the floor at the shapes' size).
        let (_, k) = st.fit.expect("the pouf is fitted");
        assert!(k.is_finite() && k > 0.0);
    }

    /// Three styles on the same material: the material path draws the row's
    /// own material, a shaped style its treatment (more parts), each a
    /// rebuild of the one model.
    #[test]
    fn styles_apply_to_the_open_material() {
        let mut app = open("metal.copper.polished");
        let data = app.world.res::<StylesData>().0.clone();
        let shaped: Vec<usize> = data.styles.iter().enumerate().filter(|(_, s)| s.render.as_ref().is_some_and(|r| r.shaped())).map(|(i, _)| i).take(2).collect();
        let p = plain_style(&app);
        style(&mut app, p);
        let model = app.world.res::<CatScene>().model;
        assert_eq!(app.world.res::<CatScene>().parts, 1);
        for s in shaped {
            let before = app.world.res::<CatScene>().builds;
            style(&mut app, s);
            let st = app.world.res::<CatScene>();
            assert_eq!(st.model, model, "one model, replaced in place");
            assert!(st.builds > before, "style {s} rebuilt");
            assert!(st.material.is_none(), "style {s} draws its treatment");
        }
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
    }

    /// The style detail step: a recipe row in a shaped style carries its
    /// block on the surface parts only (the outline stays plain) and sets the
    /// step's word, which the live strength param moves; the realistic style
    /// clears it. Voxels take one uv per cube.
    #[test]
    fn shaped_styles_keep_the_recipe_pattern() {
        use evani_kit_material_view::catalogue::detail_param;
        let mut app = open("wood.plank.weathered");
        let data = app.world.res::<StylesData>().0.clone();
        let find = |pred: &dyn Fn(&Render) -> bool| data.styles.iter().position(|s| s.render.as_ref().is_some_and(|r| r.shaped() && pred(r))).unwrap();
        let cel = find(&|r| r.bands == 2 && r.outline > 0.0 && !r.outline_glow);
        style(&mut app, cel);
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
        let word = app.world.res::<RecipeStyle>().0;
        let sd = StyleDetail::unpack(word);
        assert_eq!(sd.mode, Mode::Tone);
        assert!((sd.strength - 1.0).abs() < 0.02, "{sd:?}");
        // The model's parts: blocks on the surface, none on the outline.
        let id = data.styles[cel].id.clone();
        let mesh = shapes::build(Shape::Sphere).unwrap();
        let r = data.styles[cel].render.clone().unwrap();
        let fin = Finish { tint: [0.5, 0.3, 0.2], roughness: 0.6, metallic: 0.0, bump: 0.0, glow: 0.0 };
        let parts = treat::shade(&treat::geometry(&mesh, &r, 0.0), &r, &fin, treat::SUN);
        let block = json!("00".repeat(32));
        let with: Vec<bool> = parts.iter().map(|p| part_material(p, &block).get("extras").is_some()).collect();
        assert!(with.iter().any(|&b| b) && with.last() == Some(&false), "{with:?}");
        // The live strength moves the word without a rebuild.
        let builds = app.world.res::<CatScene>().builds;
        let _ = app.world.res_mut::<ParamRegistry>().set_by_name(&detail_param(&id, "strength"), ParamValue::F32(0.0));
        app.run_headless(3, DT);
        assert_eq!(StyleDetail::unpack(app.world.res::<RecipeStyle>().0).strength, 0.0);
        assert_eq!(app.world.res::<CatScene>().builds, builds, "no rebuild for the detail strength");
        // The realistic style: the plain recipe stage.
        let p = plain_style(&app);
        style(&mut app, p);
        assert_eq!(app.world.res::<RecipeStyle>().0, 0);
        // Voxels: one uv per cube, and the dither and neon modes.
        let vox = find(&|r| r.voxel > 0.0 && r.bands == 0);
        style(&mut app, vox);
        let sd = StyleDetail::unpack(app.world.res::<RecipeStyle>().0);
        assert!(sd.mode == Mode::Tone && sd.cell > 0.0, "{sd:?}");
        let r = data.styles[vox].render.clone().unwrap();
        let geo = treat::geometry(&mesh, &r, 0.0);
        let (uvs, _) = voxel_uv_map("ball", &geo, r.voxel);
        for q in geo.indices.as_chunks::<6>().0.iter().take(50) {
            let u = q.map(|i| uvs[i as usize]);
            assert!(u.iter().all(|x| *x == u[0]), "one uv per cube face: {u:?}");
        }
        style(&mut app, find(&|r| r.dither));
        assert_eq!(StyleDetail::unpack(app.world.res::<RecipeStyle>().0).mode, Mode::Dither);
        style(&mut app, find(&|r| r.outline_glow));
        assert_eq!(StyleDetail::unpack(app.world.res::<RecipeStyle>().0).mode, Mode::Neon);
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
    }

    /// Every shape draws a ready row without an error.
    #[test]
    fn every_shape_draws() {
        let mut app = open("rock.granite.grey");
        let p = plain_style(&app);
        style(&mut app, p);
        for s in evani_kit_material_view::catalogue::SHAPES {
            let _ = app.world.res_mut::<ParamRegistry>().set_json(SHAPE, &json!(s));
            app.run_headless(6, DT);
            let st = app.world.res::<CatScene>();
            assert!(st.drawn.unwrap().1.is_some(), "{s} drew");
            if s == "auto" {
                assert_eq!(st.key.as_ref().unwrap().shape, "rock", "stone opens on the rock");
            }
        }
        assert_eq!(errors(&app), 0, "{:?}", app.world.res::<ErrorLog>());
    }

    #[test]
    fn uv_maps_cover_the_shapes() {
        let m = shapes::tile();
        let (uv, t) = uv_map("tile", &m);
        assert_eq!(uv.len(), m.positions.len());
        assert!(t.iter().all(|t| (t[0] * t[0] + t[1] * t[1] + t[2] * t[2] - 1.0).abs() < 1e-3));
        let ball = shapes::build(Shape::Sphere).unwrap();
        let (uv, _) = uv_map("ball", &ball);
        let (lo, hi) = uv.iter().fold((f32::MAX, f32::MIN), |(lo, hi), u| (lo.min(u[1]), hi.max(u[1])));
        assert!(lo < 0.02 && hi > 0.98, "v runs pole to pole: {lo}..{hi}");
        let glb = uv_glb(&ball, &uv_map("ball", &ball).0, &uv_map("ball", &ball).1, &json!({"name": "x"})).unwrap();
        let g = evani_gltf::Gltf::from_glb(&glb).unwrap();
        assert!(evani_render::model::prepare(&g).is_ok());
    }

    /// B2-P8: a leaf card loads alpha-masked and double-sided with its mask
    /// embedded; the bank and the rock are well-formed meshes with unit normals.
    #[test]
    fn the_leaf_card_is_masked_and_the_new_shapes_are_sound() {
        let m = shapes::leaf_card();
        for s in masks::CARDS {
            let (uv, t) = uv_map(s, &m);
            let mat = json!({"name": "x", "pbrMetallicRoughness": {"baseColorFactor": [0.2, 0.5, 0.2, 1.0], "baseColorTexture": {"index": 0}}, "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": true});
            let glb = uv_glb_png(&m, &uv, &t, &mat, Some(&masks::png(s))).unwrap();
            let g = evani_gltf::Gltf::from_glb(&glb).unwrap();
            let p = evani_render::model::prepare(&g).unwrap();
            assert_eq!(p.materials[0].words[8], 1.0, "{s}: alpha mask");
            assert_eq!(p.materials[0].words[9], 1.0, "{s}: double-sided");
            assert!(p.textures.first().and_then(|t| t.image.as_ref()).is_some_and(|png| png.starts_with(&[0x89, 0x50, 0x4e, 0x47])), "{s}: the mask is embedded");
        }
        // The card's uv covers the mask: tip at v = 0, the sides at u = 0 and 1.
        let (uv, _) = uv_map("leaf", &m);
        let (lo, hi) = uv.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), u| ([lo[0].min(u[0]), lo[1].min(u[1])], [hi[0].max(u[0]), hi[1].max(u[1])]));
        assert!(lo[0].abs() < 1e-5 && (hi[0] - 1.0).abs() < 1e-5 && lo[1].abs() < 1e-5 && (hi[1] - 1.0).abs() < 1e-5, "{lo:?} {hi:?}");
        for (name, mesh) in [("slope", shapes::slope()), ("lawn", shapes::lawn()), ("rock", shapes::rock())] {
            assert!(!mesh.positions.is_empty() && mesh.indices.len() % 3 == 0, "{name}");
            assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.positions.len()), "{name}");
            assert!(mesh.normals.iter().all(|n| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-3), "{name}: unit normals");
            assert!(mesh.positions.iter().flatten().all(|c| c.is_finite() && c.abs() < 3.0), "{name}");
        }
        // The rock is lumpy: its radius varies.
        let r: Vec<f32> = shapes::rock().positions.iter().map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()).collect();
        let (lo, hi) = r.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
        assert!(hi - lo > 0.1, "radius {lo}..{hi}");
    }

    /// Round 4: the quartz cluster, the slime blob and the rose are sound
    /// meshes; the quartz is flat-faced with every face turned outwards (it
    /// is single sided), the blob rests on a flat foot, and the rose's own
    /// uvs cover the petal mask once per petal.
    #[test]
    fn the_crystal_blob_and_rose_are_sound() {
        let basic = |name: &str, m: &Mesh| {
            assert!(!m.positions.is_empty() && m.indices.len().is_multiple_of(3), "{name}");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{name}");
            assert!(m.normals.len() == m.positions.len() && m.normals.iter().all(|n| (shapes::dot(*n, *n) - 1.0).abs() < 1e-3), "{name}: unit normals");
            assert!(m.positions.iter().flatten().all(|c| c.is_finite() && c.abs() < 1.6), "{name}: about radius 1");
        };
        let q = shapes::crystal();
        basic("crystal", &q);
        let centre = q.positions.iter().fold([0.0f32; 3], |a, p| shapes::add(a, *p)).map(|x| x / q.positions.len() as f32);
        for t in q.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| q.positions[i as usize]);
            let f = shapes::cross(shapes::sub(b, a), shapes::sub(c, a));
            assert!(shapes::dot(f, f) > 1e-10, "crystal: degenerate {t:?}");
            for i in t {
                assert!(shapes::dot(q.normals[*i as usize], f) > 0.0, "crystal: flat faces");
            }
        }
        // Outwards: each upright side faces away from the cluster's middle.
        let up_faces = q.indices.as_chunks::<3>().0.iter().filter(|t| q.normals[t[0] as usize][1].abs() < 0.3).count();
        let out = q.indices.as_chunks::<3>().0.iter().filter(|t| q.normals[t[0] as usize][1].abs() < 0.3).filter(|t| shapes::dot(q.normals[t[0] as usize], shapes::sub(q.positions[t[0] as usize], centre)) > 0.0).count();
        assert!(out * 10 >= up_faces * 9, "crystal: {out} of {up_faces} sides face out");
        let blob = shapes::blob();
        basic("blob", &blob);
        let bottom = blob.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(blob.positions.iter().filter(|p| p[1] < bottom + 0.03).count() > 50, "the blob rests on a flat foot");
        let (rose, uvs) = shapes::rose();
        basic("rose", &rose);
        assert_eq!(uvs.len(), rose.positions.len());
        assert!(uvs.iter().all(|u| (-1e-5..=1.00001).contains(&u[0]) && (-1e-5..=1.00001).contains(&u[1])));
        assert_eq!(uvs.iter().filter(|u| u[0].abs() < 1e-5 && u[1].abs() < 1e-5).count(), 17, "17 petals, each its own card");
        assert_eq!(masks::mask_of("rose"), Some("petal"));
    }

    /// Critic round 1: the rose opens on a cupped petal card, and the
    /// garden's leaf is a leaf-shaped blade. Both are sound: no degenerate
    /// triangle, unit normals, every vertex normal on its triangles' side,
    /// and the petal's uv covers its mask like the leaf card's.
    #[test]
    fn the_petal_and_the_leaf_blade_are_sound() {
        let tri_ok = |name: &str, m: &Mesh| {
            assert!(m.indices.len().is_multiple_of(3) && m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{name}");
            for t in m.indices.as_chunks::<3>().0 {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                let f = shapes::cross(shapes::sub(b, a), shapes::sub(c, a));
                assert!(shapes::dot(f, f).sqrt() > 1e-5, "{name}: a degenerate triangle {t:?}");
                for i in t {
                    assert!(shapes::dot(m.normals[*i as usize], f) > 0.0, "{name}: vertex {i}'s normal points against its triangle");
                }
            }
            assert!(m.normals.iter().all(|n| (shapes::dot(*n, *n) - 1.0).abs() < 1e-3), "{name}: unit normals");
        };
        let petal = shapes::petal_card();
        tri_ok("petal", &petal);
        // Cupped: the sides stand forward of the middle.
        let z_at = |x: f32| petal.positions.iter().filter(|p| (p[0] - x).abs() < 1e-4 && p[1].abs() < 0.2).map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!(z_at(0.55) > z_at(0.0) + 0.1, "the petal is cupped");
        let (uv, _) = uv_map("petal", &petal);
        let (lo, hi) = uv.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), u| ([lo[0].min(u[0]), lo[1].min(u[1])], [hi[0].max(u[0]), hi[1].max(u[1])]));
        assert!(lo[0].abs() < 1e-5 && (hi[0] - 1.0).abs() < 1e-5 && lo[1].abs() < 1e-5 && (hi[1] - 1.0).abs() < 1e-5, "{lo:?} {hi:?}");
        // The blade is a closed thin solid: every edge in exactly two
        // triangles, no degenerate one, and no vertex normal against all of
        // its triangles (where top and underside meet, it lies between).
        let blade = shapes::leaf_blade(0.06, 0.03);
        let mut edges = std::collections::HashMap::new();
        for t in blade.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| blade.positions[i as usize]);
            let f = shapes::cross(shapes::sub(b, a), shapes::sub(c, a));
            assert!(shapes::dot(f, f).sqrt() > 1e-6, "leaf blade: a degenerate triangle {t:?}");
            for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((p.min(q), p.max(q))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&n| n == 2), "leaf blade: open or non-manifold edges");
        for (v, n) in blade.normals.iter().enumerate() {
            let against = blade.indices.as_chunks::<3>().0.iter().filter(|t| t.contains(&(v as u32))).all(|t| {
                let [a, b, c] = t.map(|i| blade.positions[i as usize]);
                shapes::dot(*n, shapes::cross(shapes::sub(b, a), shapes::sub(c, a))) < 0.0
            });
            assert!(!against, "leaf blade: vertex {v}'s normal points against all its triangles");
        }
        // Pointed at both ends, widest near the middle, under 0.5 across.
        let w: f32 = blade.positions.iter().map(|p| p[1].abs()).fold(0.0, f32::max);
        assert!(w > 0.15 && w <= 0.25, "half width {w}");
        assert!(blade.positions.iter().filter(|p| p[0].abs() > 0.49).all(|p| p[1].abs() < 1e-6), "the base and the tip are points");
        assert!(blade.positions.len() < 64, "light: {} vertices", blade.positions.len());
        assert!(blade.positions.iter().any(|p| p[2] < -0.02), "the underside sinks along the midrib");
    }
}
