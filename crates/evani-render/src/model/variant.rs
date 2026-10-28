//! Part variants (feature `models`, S3-11): one model with nodes hidden or
//! recoloured, and other models attached at its `anchor:<name>` nodes, as a
//! recipe in the kit's JSON:
//!
//! ```json
//! { "base": "kart", "hide": ["wheel_rl"], "paint": { "body": [0.9, 0.2, 0.1] },
//!   "parts": [ { "model": "crate", "anchor": "item_slot", "colour": [1, 0.8, 0.4] } ] }
//! ```
//!
//! `base` and each part's `model` are model ids ([`Models::handle`]); `hide`
//! and `paint` name nodes of the base model; a part's `anchor` names an
//! `anchor:<name>` node of the base. `colour` is linear RGB(A), multiplied
//! into the base colour (default white). Names are looked up once, when the
//! base model is read (again after a tier switch), never per frame.
//!
//! Render only: the recipe changes what is drawn, never the simulation, so
//! the same recipe and the same events give the same picture. Anything
//! gameplay reads (where a seat is) comes from the kit's own data.

use serde_json::Value;

use super::{ModelHandle, ModelId, ModelInstance, Models, trs};

/// A model's place in the world: what an instance and an anchor give.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub position: [f32; 3],
    /// Rotation quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Frame {
    pub fn new(position: [f32; 3]) -> Self {
        Self { position, rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] }
    }

    pub fn rotation(mut self, q: [f32; 4]) -> Self {
        self.rotation = q;
        self
    }

    pub fn scale(mut self, s: [f32; 3]) -> Self {
        self.scale = s;
        self
    }

    /// The frame of a world matrix without shear (a mirror goes on x).
    pub fn from_matrix(m: &[f32; 16]) -> Self {
        let col = |c: usize| [m[c * 4], m[c * 4 + 1], m[c * 4 + 2]];
        let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let (a, b, c) = (col(0), col(1), col(2));
        let mut s = [len(a), len(b), len(c)];
        let cross = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        if cross[0] * c[0] + cross[1] * c[1] + cross[2] * c[2] < 0.0 {
            s[0] = -s[0];
        }
        let r = |row: usize, col: usize| m[col * 4 + row] / if s[col] == 0.0 { 1.0 } else { s[col] };
        let (r00, r11, r22) = (r(0, 0), r(1, 1), r(2, 2));
        let t = r00 + r11 + r22;
        let q = if t > 0.0 {
            let k = (t + 1.0).sqrt() * 2.0;
            [(r(2, 1) - r(1, 2)) / k, (r(0, 2) - r(2, 0)) / k, (r(1, 0) - r(0, 1)) / k, 0.25 * k]
        } else if r00 > r11 && r00 > r22 {
            let k = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
            [0.25 * k, (r(0, 1) + r(1, 0)) / k, (r(0, 2) + r(2, 0)) / k, (r(2, 1) - r(1, 2)) / k]
        } else if r11 > r22 {
            let k = (1.0 + r11 - r00 - r22).sqrt() * 2.0;
            [(r(0, 1) + r(1, 0)) / k, 0.25 * k, (r(1, 2) + r(2, 1)) / k, (r(0, 2) - r(2, 0)) / k]
        } else {
            let k = (1.0 + r22 - r00 - r11).sqrt() * 2.0;
            [(r(0, 2) + r(2, 0)) / k, (r(1, 2) + r(2, 1)) / k, 0.25 * k, (r(1, 0) - r(0, 1)) / k]
        };
        Self { position: [m[12], m[13], m[14]], rotation: q, scale: s }
    }

    /// The world matrix (column-major).
    pub fn matrix(&self) -> [f32; 16] {
        trs(self.rotation, self.scale, self.position)
    }
}

impl ModelInstance {
    /// An instance of `model` at `frame`.
    pub fn at(model: ModelId, frame: &Frame) -> Self {
        Self::new(model, frame.position).rotation(frame.rotation).scale(frame.scale)
    }

    pub fn hide(mut self, nodes: u32) -> Self {
        self.hide = nodes;
        self
    }
}

#[derive(Debug)]
struct Attach {
    model: ModelHandle,
    anchor: String,
    colour: [f32; 4],
}

/// The recipe's names resolved against one file of the base model.
#[derive(Debug)]
struct Cache {
    base: ModelId,
    hide: u32,
    paints: Vec<(u32, [f32; 4])>,
    anchors: Vec<Option<u16>>,
}

/// A variant recipe ([`Variant::from_json`]), drawn with [`Variant::draw`].
#[derive(Debug)]
pub struct Variant {
    base: ModelHandle,
    hide: Vec<String>,
    paint: Vec<(String, [f32; 4])>,
    parts: Vec<Attach>,
    cache: Option<Cache>,
    /// Names the recipe uses that the base model does not have (`node
    /// <name>` or `anchor <name>`), found when the model was read: a typo to
    /// log once. Those entries are skipped.
    pub unresolved: Vec<String>,
}

fn colour(v: Option<&Value>) -> Result<[f32; 4], String> {
    let Some(v) = v else { return Ok([1.0; 4]) };
    let n: Option<Vec<f32>> = v.as_array().and_then(|a| a.iter().map(|x| x.as_f64().map(|f| f as f32)).collect());
    match n.as_deref() {
        Some([r, g, b]) => Ok([*r, *g, *b, 1.0]),
        Some([r, g, b, a]) => Ok([*r, *g, *b, *a]),
        _ => Err("variant: a colour is [r, g, b] or [r, g, b, a] in linear numbers".to_owned()),
    }
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key).and_then(Value::as_str).ok_or_else(|| format!("variant: \"{key}\" is missing or not a name"))
}

impl Variant {
    /// A recipe from the kit's JSON (see the module docs). Model ids become
    /// handles at once; nothing is read until they are.
    pub fn from_json(models: &mut Models, v: &Value) -> Result<Self, String> {
        let hide = match v.get("hide") {
            None => Vec::new(),
            Some(h) => h.as_array().and_then(|a| a.iter().map(|n| n.as_str().map(str::to_owned)).collect()).ok_or("variant: \"hide\" is a list of node names")?,
        };
        let mut paint = Vec::new();
        for (name, c) in v.get("paint").and_then(Value::as_object).into_iter().flatten() {
            paint.push((name.clone(), colour(Some(c))?));
        }
        let mut parts = Vec::new();
        for p in v.get("parts").and_then(Value::as_array).into_iter().flatten() {
            parts.push(Attach { model: models.handle(text(p, "model")?), anchor: text(p, "anchor")?.to_owned(), colour: colour(p.get("colour"))? });
        }
        Ok(Self { base: models.handle(text(v, "base")?), hide, paint, parts, cache: None, unresolved: Vec::new() })
    }

    /// [`Variant::from_json`] for the recipe as text (a JSON file's contents).
    pub fn from_text(models: &mut Models, text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("variant: {e}"))?;
        Self::from_json(models, &v)
    }

    /// Resolve the recipe's names against `base`.
    fn resolve(&mut self, models: &Models, base: ModelId) {
        let mut un = Vec::new();
        let mut node = |n: &str| {
            models.node_bit(base, n).unwrap_or_else(|| {
                un.push(format!("node {n}"));
                0
            })
        };
        let hide = self.hide.iter().fold(0, |h, n| h | node(n));
        let paints = self.paint.iter().map(|(n, c)| (node(n), *c)).collect();
        let anchors = self
            .parts
            .iter()
            .map(|p| {
                let a = models.anchor(base, &p.anchor);
                if a.is_none() {
                    un.push(format!("anchor {}", p.anchor));
                }
                a
            })
            .collect();
        self.unresolved = un;
        self.cache = Some(Cache { base, hide, paints, anchors });
    }

    /// Draw the variant at `at`: the base with its nodes hidden and
    /// recoloured, each part at its anchor. `false` (draw the placeholder)
    /// until the base and every part that can load are ready, so a variant
    /// never shows half built.
    pub fn draw(&mut self, models: &mut Models, at: Frame) -> bool {
        let Some(base) = models.resolve(self.base) else { return false };
        if self.parts.iter().any(|p| models.resolve(p.model).is_none() && !models.handle_failed(p.model)) {
            return false;
        }
        if self.cache.as_ref().is_none_or(|c| c.base != base) {
            self.resolve(models, base);
        }
        let Some(c) = &self.cache else { return false };
        models.draw_with(ModelInstance::at(base, &at).hide(c.hide), &c.paints);
        for (p, a) in self.parts.iter().zip(&c.anchors) {
            if let (Some(id), Some(a)) = (models.resolve(p.model), a)
                && let Some(f) = models.anchor_frame(base, *a, &at)
            {
                models.draw(ModelInstance::at(id, &f).tint(p.colour));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::{INSTANCE_WORDS, count_draws, prepare};
    use super::*;
    use crate::world::RenderWorld;
    use evani_core::overlay::RenderStats;
    use evani_gltf::Gltf;

    fn bytes(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/../../tests/fixtures/models/{name}.low.glb", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    /// A `Models` with handles for the kart and the crate, asked for but not
    /// delivered yet.
    fn asked() -> Models {
        let mut m = Models::default();
        m.set_manifest(None);
        m.handle("kart");
        m.handle("crate");
        m.sync_tier("low");
        m
    }

    /// Deliver every asked-for file and run the native frame twice (ready,
    /// then the handles pick the ready files up).
    fn ready(m: &mut Models) {
        for (id, url) in m.take_requests() {
            let name = url.trim_start_matches("models/").trim_end_matches(".glb");
            m.deliver(id, Ok(&bytes(name)));
        }
        let rw = RenderWorld::default();
        for _ in 0..2 {
            count_draws(m, &rw, Some("low"), &mut RenderStats::default());
        }
    }

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    /// The recipe's part stands at the base's anchor, whatever the base's
    /// place, turn and scale; its colour and the base's hidden and painted
    /// nodes come through; a wrong name is reported, not fatal.
    #[test]
    fn variant_from_parts_uses_anchors() {
        let mut m = asked();
        let recipe = r#"{ "base": "kart", "hide": ["wheel_rl"], "paint": { "body": [1, 0, 0] },
            "parts": [ { "model": "crate", "anchor": "item_slot", "colour": [0, 1, 0, 1] }, { "model": "crate", "anchor": "nope" } ] }"#;
        let mut v = Variant::from_json(&mut m, &json(recipe)).unwrap();
        let at = Frame::new([5.0, 0.0, 2.0]).rotation([0.0, 0.5f32.sqrt(), 0.0, 0.5f32.sqrt()]).scale([2.0; 3]);
        assert!(!v.draw(&mut m, at) && m.draws.is_empty(), "the placeholder until the models are read");
        ready(&mut m);
        assert!(v.draw(&mut m, at));
        assert!(v.unresolved == ["anchor nope"], "{:?}", v.unresolved);
        assert_eq!(m.draws.len(), 2, "the base and the one part whose anchor exists");

        // The anchor's place, worked out from the file by hand: a quarter
        // turn about y sends (x, y, z) to (z, y, -x).
        let g = Gltf::from_glb(&bytes("kart")).unwrap();
        let w = g.anchors.iter().find(|a| a.name == "item_slot").unwrap().world;
        let (a, s) = ([w[12], w[13], w[14]], 2.0);
        let want = [5.0 + a[2] * s, a[1] * s, 2.0 - a[0] * s];
        let part = m.draws[1];
        for k in 0..3 {
            assert!((part.position[k] - want[k]).abs() < 1e-4, "axis {k}: {:?} vs {want:?}", part.position);
        }
        assert!(part.rotation.iter().zip(at.rotation).map(|(p, q)| p * q).sum::<f32>().abs() > 0.9999, "the anchor is unturned, so the part turns with the base");
        assert!((part.scale[0] - 2.0).abs() < 1e-5);
        assert_eq!(part.tint, [0.0, 1.0, 0.0, 1.0]);
        let base = m.draws[0];
        assert_eq!(base.hide, m.node_bit(base.model, "wheel_rl").unwrap());
        assert_eq!(m.paints_of(&base), [(m.node_bit(base.model, "body").unwrap(), [1.0, 0.0, 0.0, 1.0])]);
        assert_eq!(m.anchor_frame(base.model, m.anchor(base.model, "item_slot").unwrap(), &at).unwrap().position, part.position);

        // A moved base moves the part with it; a mirrored base still decomposes.
        let mirrored = Frame::from_matrix(&trs(at.rotation, [-2.0, 2.0, 2.0], at.position));
        assert!(mirrored.scale[0] < 0.0 && (mirrored.scale[1] - 2.0).abs() < 1e-5);
        assert!(mirrored.rotation.iter().zip(at.rotation).map(|(p, q)| p * q).sum::<f32>().abs() > 0.9999);
        // Bad recipes are errors, not panics.
        for bad in [r#"{}"#, r#"{ "base": "kart", "hide": 3 }"#, r#"{ "base": "kart", "paint": { "body": [1, 2] } }"#, r#"{ "base": "kart", "parts": [ { "model": "crate" } ] }"#] {
            assert!(Variant::from_json(&mut m, &json(bad)).is_err(), "{bad}");
        }
    }

    /// A socket (an anchor node with a rotation, as `cargo xtask convert`
    /// writes for `extras.evani.sockets`): `anchor_frame` turns the part by
    /// the base's turn times the socket's, and a point of the part lands at
    /// `base · (q · local + p)`. No new engine code: the anchor's world matrix
    /// already carries the rotation.
    #[test]
    fn anchor_frame_carries_a_socket_rotation() {
        let src = bytes("kart");
        let (json, bin) = evani_gltf::split_glb(&src).unwrap();
        let mut j: Value = serde_json::from_slice(json).unwrap();
        let h = 0.5f32.sqrt();
        let qx = [h, 0.0, 0.0, h]; // a quarter turn about x
        for n in j["nodes"].as_array_mut().unwrap() {
            if n["name"] == "anchor:item_slot" {
                n["rotation"] = serde_json::json!(qx);
            }
        }
        let mut js = serde_json::to_vec(&j).unwrap();
        while !js.len().is_multiple_of(4) {
            js.push(b' ');
        }
        let bin = bin.unwrap_or_default();
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&((12 + 8 + js.len() + 8 + bin.len()) as u32).to_le_bytes());
        glb.extend_from_slice(&(js.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&js);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(bin);

        let mut m = Models::default();
        m.set_manifest(None);
        let kart = m.handle("kart");
        m.sync_tier("low");
        for (id, _) in m.take_requests() {
            m.deliver(id, Ok(&glb));
        }
        let rw = RenderWorld::default();
        for _ in 0..2 {
            count_draws(&mut m, &rw, Some("low"), &mut RenderStats::default());
        }
        let id = m.resolve(kart).unwrap();
        let slot = m.anchor(id, "item_slot").unwrap();
        let qy = [0.0, h, 0.0, h];
        let at = Frame::new([5.0, 0.0, 2.0]).rotation(qy);
        let f = m.anchor_frame(id, slot, &at).unwrap();
        // Expected turn: qy * qx.
        let want = [h * h, h * h, -h * h, h * h];
        assert!(f.rotation.iter().zip(want).map(|(a, b)| a * b).sum::<f32>().abs() > 0.9999, "{:?} vs {want:?}", f.rotation);
        // Untouched without a base turn: the socket's own rotation.
        let f0 = m.anchor_frame(id, slot, &Frame::new([0.0; 3])).unwrap();
        assert!(f0.rotation.iter().zip(qx).map(|(a, b)| a * b).sum::<f32>().abs() > 0.9999, "{:?}", f0.rotation);
    }

    /// S3-12: a file the converter matched to the toon look (`--look toon`:
    /// maps dropped, materials merged, primitives joined) still has its node
    /// names and anchors, so a recipe hides and paints a named node and stands
    /// a part at an anchor exactly as on an unmatched file. The fixture is
    /// written by `look_match_fixture_is_the_converter_output` in evani-convert.
    #[test]
    fn variant_works_on_a_look_matched_file() {
        let mut m = Models::default();
        m.set_manifest(None);
        m.handle("look-match");
        m.handle("crate");
        m.sync_tier("low");
        ready(&mut m);
        let g = Gltf::from_glb(&bytes("look-match")).unwrap();
        assert_eq!(g.materials.len(), 2, "the three paints were merged into one material");
        assert!(g.materials.iter().all(|x| x.metallic_roughness_texture.is_none() && x.occlusion_texture.is_none()), "the maps toon never samples are gone");
        let recipe = r#"{ "base": "look-match", "hide": ["trim"], "paint": { "body": [1, 0, 0] },
            "parts": [ { "model": "crate", "anchor": "seat", "colour": [0, 1, 0, 1] } ] }"#;
        let mut v = Variant::from_json(&mut m, &json(recipe)).unwrap();
        let at = Frame::new([1.0, 0.0, 2.0]).scale([1.5; 3]);
        assert!(v.draw(&mut m, at));
        assert!(v.unresolved.is_empty(), "{:?}", v.unresolved);
        assert_eq!(m.draws.len(), 2, "the base and the part at the anchor");
        let base = m.draws[0];
        let (trim, body) = (m.node_bit(base.model, "trim").unwrap(), m.node_bit(base.model, "body").unwrap());
        assert_eq!(base.hide, trim, "the named node is hidden");
        assert_eq!(m.paints_of(&base), [(body, [1.0, 0.0, 0.0, 1.0])], "the named node is painted");
        let seat = m.anchor(base.model, "seat").expect("the anchor survived matching");
        assert_eq!(m.anchor_frame(base.model, seat, &at).unwrap().position, m.draws[1].position, "the part stands at the anchor");
        // The hidden node's part adds nothing; the body's still draws.
        let parts = prepare(&g).unwrap().parts;
        // (The file carries its LOD levels as parts too: two materials, so two parts a level.)
        assert!(parts.iter().all(|p| (p.tags & trim != 0) != (p.tags & body != 0)), "every part sits under exactly one of the two named nodes");
        assert!(parts.iter().any(|p| p.tags & trim != 0) && parts.iter().any(|p| p.tags & body != 0));
        let materials: std::collections::BTreeSet<_> = parts.iter().map(|p| p.material).collect();
        assert_eq!(materials.len(), 2, "two materials in all, as merged");
    }

    /// A hidden node's parts add no instance, no draw call and no
    /// triangles; showing it again brings them back; other instances of the
    /// same model are untouched; a recolour reaches only its node's parts.
    #[test]
    fn hidden_part_draws_nothing() {
        let mut m = asked();
        ready(&mut m);
        let h = m.handle("kart");
        let id = m.resolve(h).unwrap();
        let g = Gltf::from_glb(&bytes("kart")).unwrap();
        let parts = prepare(&g).unwrap().parts;
        let n = parts.len();
        let words = |m: &Models| {
            let mut out = Vec::new();
            for p in &parts {
                m.stage(id.0 as usize, p, &mut out);
            }
            out
        };
        let counts = |m: &mut Models| {
            let mut s = RenderStats::default();
            count_draws(m, &RenderWorld::default(), None, &mut s);
            (s.draw_calls, s.triangles)
        };
        let rl = m.node_bit(id, "wheel_rl").unwrap();
        let body = m.node_bit(id, "body").unwrap();
        assert_eq!(parts.iter().filter(|p| p.tags & rl != 0).count(), 1, "one part under wheel_rl");

        m.draw(ModelInstance::new(id, [0.0; 3]));
        let (shown_calls, shown_tris) = {
            assert_eq!(words(&m).len(), n * INSTANCE_WORDS);
            counts(&mut m)
        };
        assert_eq!(shown_calls as usize, n);

        m.draw(ModelInstance::new(id, [0.0; 3]).hide(rl));
        assert_eq!(words(&m).len(), (n - 1) * INSTANCE_WORDS, "the hidden node's part adds nothing");
        let (calls, tris) = counts(&mut m);
        assert_eq!(calls as usize, n - 1);
        let wheel_tris = parts.iter().find(|p| p.tags & rl != 0).unwrap().indices.len() as u32 / 3;
        assert_eq!(tris, shown_tris - wheel_tris);

        // Two instances: one hides it, one shows it.
        m.draw(ModelInstance::new(id, [0.0; 3]).hide(rl));
        m.draw(ModelInstance::new(id, [3.0, 0.0, 0.0]));
        assert_eq!(words(&m).len(), (2 * n - 1) * INSTANCE_WORDS);
        m.draws.clear();

        // Hidden bits that match nothing, and show (hide 0), draw everything.
        m.draw(ModelInstance::new(id, [0.0; 3]).hide(1 << 31).hide(0));
        assert_eq!(words(&m).len(), n * INSTANCE_WORDS);
        m.draws.clear();

        // A recolour multiplies the tint of the parts under its node only.
        m.draw_with(ModelInstance::new(id, [0.0; 3]).tint([0.5, 1.0, 1.0, 1.0]), &[(body, [1.0, 0.5, 0.25, 1.0])]);
        let w = words(&m);
        for (i, p) in parts.iter().enumerate() {
            let tint = &w[i * INSTANCE_WORDS + 24..(i + 1) * INSTANCE_WORDS];
            let want = if p.tags & body != 0 { [0.5, 0.5, 0.25, 1.0] } else { [0.5, 1.0, 1.0, 1.0] };
            assert_eq!(tint, want, "part {i}");
        }
        // The paint list starts over with each frame's first draw.
        m.draws.clear();
        m.draw(ModelInstance::new(id, [0.0; 3]));
        assert!(m.paints.is_empty());
    }
}
