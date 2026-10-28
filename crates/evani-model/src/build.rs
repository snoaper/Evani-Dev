//! From intent to a placed part graph: class template + style profile +
//! intent → measures → parts (generators) → relationships solved → one
//! field per solid, gameplay anchors and the must list
//! (docs/model_research/02, 03, 06).
//!
//! M1 has no search: every adjective resolves to the centre of its range
//! (or default plus its steps), numbers pin measures, and the result is one
//! deterministic candidate. `fit_box` acts as a constraint first: when the
//! model's exact extents exceed the box, Levenberg-Marquardt moves the free
//! measures (within their ranges) until it fits.

use crate::expr;
use crate::generator::{self, Anchor, GenOut, PVal, Params, Proxy};
use crate::intent::{nearest, Diag};
use crate::sdf::Node;
use crate::vec::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Class templates (docs/model_research/03): the 18 classes plus the six
/// benchmark additions (decision D14).
pub const CLASSES: [&str; 25] = [
    "kart", "character_body", "character", "building", "tree", "rock", "fence", "sign", "crate", "barrel", "lamp_post", "bench", "door", "chest", "sword", "shield", "lollipop",
    "gumdrop", "candy_cane", "house", "tower", "mushroom", "cloud", "cactus", "bush",
];

pub fn class_template(class: &str) -> Option<Value> {
    let src = match class {
        "kart" => include_str!("../data/classes/kart.json"),
        "character_body" => include_str!("../data/classes/character_body.json"),
        "character" => include_str!("../data/classes/character.json"),
        "building" => include_str!("../data/classes/building.json"),
        "tree" => include_str!("../data/classes/tree.json"),
        "rock" => include_str!("../data/classes/rock.json"),
        "fence" => include_str!("../data/classes/fence.json"),
        "sign" => include_str!("../data/classes/sign.json"),
        "crate" => include_str!("../data/classes/crate.json"),
        "barrel" => include_str!("../data/classes/barrel.json"),
        "lamp_post" => include_str!("../data/classes/lamp_post.json"),
        "bench" => include_str!("../data/classes/bench.json"),
        "door" => include_str!("../data/classes/door.json"),
        "chest" => include_str!("../data/classes/chest.json"),
        "sword" => include_str!("../data/classes/sword.json"),
        "shield" => include_str!("../data/classes/shield.json"),
        "lollipop" => include_str!("../data/classes/lollipop.json"),
        "gumdrop" => include_str!("../data/classes/gumdrop.json"),
        "candy_cane" => include_str!("../data/classes/candy_cane.json"),
        "house" => include_str!("../data/classes/house.json"),
        "tower" => include_str!("../data/classes/tower.json"),
        "mushroom" => include_str!("../data/classes/mushroom.json"),
        "cloud" => include_str!("../data/classes/cloud.json"),
        "cactus" => include_str!("../data/classes/cactus.json"),
        "bush" => include_str!("../data/classes/bush.json"),
        _ => return None,
    };
    // Template iteration without a rebuild: with `EVANI_MODEL_CLASSES` set
    // to a folder, a `<class>.json` there replaces the built-in copy (dev
    // loops only; unset, the embedded templates are used).
    let local = std::env::var_os("EVANI_MODEL_CLASSES").and_then(|d| std::fs::read_to_string(std::path::Path::new(&d).join(format!("{class}.json"))).ok());
    let src = local.as_deref().unwrap_or(src);
    // A template file still being written (no parts yet) counts as missing.
    serde_json::from_str::<Value>(src).ok().filter(|t| t["parts"].as_array().is_some_and(|p| !p.is_empty()))
}

/// Blend layers, innermost first (part key `layer`).
pub const LAYERS: [&str; 3] = ["body", "garment", "accessory"];

/// A part spec's bone candidates: a string or a list of strings.
fn bone_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
        _ => Vec::new(),
    }
}

pub fn style_profile(style: &str) -> Option<Value> {
    let src = match style {
        "toon_chunky" => include_str!("../data/styles/toon_chunky.json"),
        _ => return None,
    };
    serde_json::from_str(src).ok()
}

/// Bodies used by the `seats` must.
pub fn body_ref(r: &str) -> Option<Value> {
    let src = match r {
        "bench/kart_driver" | "kart_driver" | "chibi_driver_v2" => include_str!("../data/bodies/kart_driver.json"),
        _ => return None,
    };
    serde_json::from_str(src).ok()
}

#[derive(Clone, Debug)]
pub struct Part {
    /// Unique name: the template name, plus `.l`/`.r` for split pairs and
    /// `.0`, `.1`... for counted copies.
    pub name: String,
    pub base: String,
    pub group: Option<String>,
    pub role: String,
    pub solid: String,
    pub kind: String,
    pub params: Params,
    pub shape: GenOut,
    pub q: Quat,
    pub t: V3,
    /// The field also covers the mirror image across x = 0.
    pub mirror_x: bool,
    /// This part is the mirror image (x -> -x) of its placement.
    pub reflect: bool,
    pub id: u32,
    /// Blend layer: 0 body, 1 garment, 2 accessory (templates with
    /// `blend_layers`).
    pub layer: u8,
    /// Colour region (templates with `regions`).
    pub region: Option<String>,
    /// Bone candidates (`{side}` resolved per vertex by the sign of x).
    pub bone: Vec<String>,
    /// Garment shells: the parts this one covers.
    pub over: Vec<usize>,
}

impl Part {
    /// The part's field in world space.
    pub fn field(&self) -> Node {
        let placed = self.shape.node.clone().at(self.q, self.t);
        let n = if self.mirror_x {
            Node::Mirror { axis: 0, child: placed.boxed() }
        } else if self.reflect {
            Node::Flip { axis: 0, child: placed.boxed() }
        } else {
            placed
        };
        n.label(self.id)
    }
    /// An anchor in world space (on the +x side for mirrored parts).
    pub fn anchor(&self, name: &str) -> Option<Anchor> {
        let a = self.shape.anchor(name)?;
        let mut p = add(qrot(self.q, a.p), self.t);
        let mut n = qrot(self.q, a.n);
        if self.reflect {
            p[0] = -p[0];
            n[0] = -n[0];
        }
        Some(Anchor { name: a.name.clone(), p, n, on_surface: a.on_surface })
    }
    /// Exact support (furthest extent) along unit direction `d`, world space.
    pub fn support(&self, d: V3) -> f64 {
        let s = |dd: V3| {
            let l = qrot(qconj(self.q), dd);
            dot(self.t, dd) + local_support(&self.shape, l)
        };
        if self.mirror_x {
            s(d).max(s([-d[0], d[1], d[2]]))
        } else if self.reflect {
            s([-d[0], d[1], d[2]])
        } else {
            s(d)
        }
    }
}

/// Support of a generated part in its own frame (exact for boxes,
/// cylinders, ellipsoids, capsules and wheels; bounds for the rest).
fn local_support(g: &GenOut, d: V3) -> f64 {
    fn node_support(n: &Node, d: V3) -> f64 {
        match n {
            Node::RoundBox { half, r } => {
                let r = r.min(half[0]).min(half[1]).min(half[2]).max(0.0);
                (half[0] - r) * d[0].abs() + (half[1] - r) * d[1].abs() + (half[2] - r) * d[2].abs() + r
            }
            Node::Cylinder { r, half_h, round } => {
                let round = round.min(*r).min(*half_h).max(0.0);
                let rad = (d[0] * d[0] + d[2] * d[2]).sqrt();
                (half_h - round) * d[1].abs() + (r - round) * rad + round
            }
            Node::Sphere { r } => *r,
            Node::Ellipsoid { radii } => ((radii[0] * d[0]).powi(2) + (radii[1] * d[1]).powi(2) + (radii[2] * d[2]).powi(2)).sqrt(),
            Node::Capsule { a, b, r } => dot(*a, d).max(dot(*b, d)) + r,
            Node::RoundCone { a, b, ra, rb } => (dot(*a, d) + ra).max(dot(*b, d) + rb),
            Node::Lathe { profile } => {
                let rad = (d[0] * d[0] + d[2] * d[2]).sqrt();
                profile.iter().map(|q| q[0] * rad + q[1] * d[1]).fold(f64::NEG_INFINITY, f64::max)
            }
            Node::Transform { q, t, child } => dot(*t, d) + node_support(child, qrot(qconj(*q), d)),
            Node::Union(ch) => ch.iter().map(|c| node_support(c, d)).fold(f64::NEG_INFINITY, f64::max),
            Node::Label { child, .. } | Node::Prepared { child, .. } => node_support(child, d),
            Node::Round { r, child } => node_support(child, d) + r,
            other => {
                // Corners of the guaranteed box.
                let b = other.bounds();
                let mut m = f64::NEG_INFINITY;
                for i in 0..8 {
                    let c = [if i & 1 == 0 { b.min[0] } else { b.max[0] }, if i & 2 == 0 { b.min[1] } else { b.max[1] }, if i & 4 == 0 { b.min[2] } else { b.max[2] }];
                    m = m.max(dot(c, d));
                }
                m
            }
        }
    }
    node_support(&g.node, d)
}

#[derive(Clone, Debug)]
pub struct Solid {
    pub name: String,
    pub parts: Vec<usize>,
    /// Origin of the solid's own frame (a wheel's hub; the body's is 0).
    pub origin: V3,
    /// For the right half of a split pair: the left solid it mirrors.
    pub mirror_of: Option<String>,
    pub loose: bool,
}

#[derive(Clone, Debug)]
pub struct GameAnchor {
    pub name: String,
    pub p: V3,
    pub n: V3,
    pub part: String,
    /// Skinned models: the joint the anchor rides on.
    pub joint: Option<String>,
    /// A socket's rotation (unit `[x, y, z, w]`, model frame), from the
    /// template's `q`. `None` (the default) writes the anchor as before:
    /// translation only, no `extras.evani.sockets` entry.
    pub q: Option<Quat>,
}

/// A template anchor's optional `q`: four numbers (or formulas), normalised.
fn socket_q(v: Option<&Value>, vars: &BTreeMap<String, f64>, name: &str) -> Result<Option<Quat>, String> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(None) };
    let a = v.as_array().filter(|a| a.len() == 4).ok_or(format!("anchor {name}: q must be [x, y, z, w]"))?;
    let mut q = [0.0; 4];
    for (k, x) in a.iter().enumerate() {
        q[k] = expr::eval_json(x, vars)?;
    }
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if !l.is_finite() || l < 1e-9 {
        return Err(format!("anchor {name}: q has no length"));
    }
    Ok(Some([q[0] / l, q[1] / l, q[2] / l, q[3] / l]))
}

#[derive(Clone, Debug)]
pub struct Model {
    pub class: String,
    pub style: String,
    pub intent: Value,
    pub seed: u32,
    pub measures: BTreeMap<String, f64>,
    pub pinned: BTreeSet<String>,
    pub parts: Vec<Part>,
    pub solids: Vec<Solid>,
    pub anchors: Vec<GameAnchor>,
    pub musts: Map<String, Value>,
    pub notes: Vec<String>,
    pub template: Value,
    pub blend: f64,
    /// Parameters of parts changed by the intent, by template part name
    /// (for compile-level locality).
    pub removed: BTreeSet<String>,
    /// Blend radius per layer (body, garment, accessory) for templates
    /// with `blend_layers`; `None` keeps the single M1 blend.
    pub layer_blend: Option<[f64; 3]>,
    /// sRGB colour per region (template `regions`, intent `colours`);
    /// empty for models without regions.
    pub region_colours: BTreeMap<String, [u8; 3]>,
    /// T-pose skeleton (templates with `skeleton`).
    pub skeleton: Option<crate::rig::Skeleton>,
    /// Garment thickness probes (`checks` on `gen: layer` parts).
    pub garment_checks: Vec<GarmentProbe>,
}

/// A named point where a garment's thickness is measured.
#[derive(Clone, Debug)]
pub struct GarmentProbe {
    pub part: usize,
    pub name: String,
    pub p: V3,
    /// The garment's `thickness` parameter.
    pub target: f64,
}

/// A measured garment thickness: from the body layer's surface along its
/// normal to the garment's outer surface.
#[derive(Clone, Debug)]
pub struct Thickness {
    pub part: String,
    pub name: String,
    /// The point on the body surface and the body normal there.
    pub at: V3,
    pub n: V3,
    pub got: f64,
    pub target: f64,
    /// The ray never left the garment within `THICKNESS_CAP`.
    pub capped: bool,
    /// The ray met the body again inside the garment (the garment fills a
    /// crease; `got` is the crease's width along the ray).
    pub bridged: bool,
    /// Within 30% of the target (never when capped).
    pub pass: bool,
}

/// How far a thickness ray marches before it counts as a failure.
pub const THICKNESS_CAP: f64 = 0.3;

/// Measure every garment probe: project the point onto the body layer of
/// the garment's solid, step out along the body normal until the
/// garment's field is no longer inside, then bisect the crossing.
pub fn garment_thickness(model: &Model) -> Vec<Thickness> {
    let mut out = Vec::new();
    for g in &model.garment_checks {
        let part = &model.parts[g.part];
        let Some(s) = model.solids.iter().find(|s| s.name == part.solid) else { continue };
        let body: Vec<Node> = s.parts.iter().map(|&i| &model.parts[i]).filter(|p| p.layer == 0 && p.role != "cut").map(Part::field).collect();
        if body.is_empty() {
            continue;
        }
        let k = model.layer_blend.map_or(model.blend, |l| l[0]);
        let body = if body.len() == 1 { body.into_iter().next().unwrap_or(Node::Union(vec![])) } else { Node::SmoothUnion { k, children: body } }.prepare();
        let garment = part.field().prepare();
        let unit = |v: V3| {
            let l = dot(v, v).sqrt();
            if l > 1e-12 { mul(v, 1.0 / l) } else { [0.0, 1.0, 0.0] }
        };
        // Project onto the body surface (a few Newton steps on the field).
        let mut q = g.p;
        for _ in 0..16 {
            let sm = body.eval(q);
            q = sub(q, mul(unit(sm.g), sm.d));
        }
        let n = unit(body.eval(q).g);
        let mut capped = false;
        let mut bridged = false;
        let got = if garment.dist(q) >= 0.0 {
            0.0
        } else {
            // Stop at the garment's outside, or where the ray meets the body
            // again (a crease the garment bridges: its width is reported).
            let step = (g.target / 20.0).max(1e-4);
            let mut t = 0.0;
            let mut left = false;
            while garment.dist(add(q, mul(n, t + step))) < 0.0 {
                if t >= THICKNESS_CAP {
                    // Never left the garment within the cap: a failure,
                    // not a measurement.
                    capped = true;
                    break;
                }
                let bd = body.dist(add(q, mul(n, t + step)));
                if left && bd < 0.0 {
                    bridged = true;
                    break;
                }
                left |= bd > step;
                t += step;
            }
            if bridged || capped {
                t
            } else {
                let (mut lo, mut hi) = (t, t + step);
                for _ in 0..30 {
                    let mid = (lo + hi) * 0.5;
                    if garment.dist(add(q, mul(n, mid))) < 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                (lo + hi) * 0.5
            }
        };
        let pass = !capped && (got - g.target).abs() <= 0.3 * g.target;
        out.push(Thickness { part: part.name.clone(), name: g.name.clone(), at: q, n, got, target: g.target, capped, bridged, pass });
    }
    out
}

impl Model {
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.name == name)
    }
    pub fn parts_of(&self, base_or_group: &str) -> Vec<usize> {
        (0..self.parts.len()).filter(|&i| self.parts[i].base == base_or_group || self.parts[i].group.as_deref() == Some(base_or_group)).collect()
    }
    /// The world field of a solid, optionally without some parts.
    pub fn solid_field(&self, solid: &str, skip: &dyn Fn(&Part) -> bool) -> Option<Node> {
        let s = self.solids.iter().find(|s| s.name == solid)?;
        let mut f = match self.layer_blend {
            // Layered blends: body parts melt together softly, garments
            // hug the result more tightly, accessories tightest.
            Some(lb) => {
                let mut acc: Option<Node> = None;
                for (layer, k) in lb.iter().enumerate() {
                    let mut ch: Vec<Node> = acc.take().into_iter().collect();
                    ch.extend(s.parts.iter().map(|&i| &self.parts[i]).filter(|p| !skip(p) && p.role != "cut" && p.layer as usize == layer).map(Part::field));
                    acc = if ch.len() <= 1 {
                        ch.pop()
                    } else {
                        Some(Node::SmoothUnion { k: *k, children: ch })
                    };
                }
                acc?
            }
            None => {
                let kids: Vec<Node> = s.parts.iter().map(|&i| &self.parts[i]).filter(|p| !skip(p) && p.role != "cut").map(Part::field).collect();
                if kids.is_empty() {
                    return None;
                }
                if kids.len() == 1 { kids.into_iter().next()? } else { Node::SmoothUnion { k: self.blend, children: kids } }
            }
        };
        // Cut parts (recesses, grooves, openings) are subtracted from the solid.
        let cuts: Vec<Node> = s.parts.iter().map(|&i| &self.parts[i]).filter(|p| !skip(p) && p.role == "cut").map(Part::field).collect();
        if !cuts.is_empty() {
            f = Node::Subtract { a: f.boxed(), b: Node::Union(cuts).boxed(), k: 0.0 };
        }
        // Classes that sit flat on the ground are clipped at y = 0.
        if self.clip_floor() {
            let mut b = f.bounds().grow(0.01);
            b.min[1] = -0.01;
            f = Node::Intersect(vec![f, Node::Plane { n: [0.0, -1.0, 0.0], off: 0.0 }, Node::RoundBox { half: mul(b.size(), 0.5), r: 0.0 }.at(QID, b.centre())]);
        }
        Some(f)
    }
    /// The class template clips every solid at the ground plane.
    pub fn clip_floor(&self) -> bool {
        self.template["clip_floor"].as_bool() == Some(true)
    }
    /// The whole model as one world field (all solids, hard union).
    pub fn field(&self) -> Node {
        let kids: Vec<Node> = self.solids.iter().filter_map(|s| self.solid_field(&s.name, &|_| false)).collect();
        Node::Union(kids)
    }
    /// Exact extents [min, max] per axis from the parts' supports.
    pub fn extents(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in self.parts.iter().filter(|p| p.role != "cut") {
            for ax in 0..3 {
                let mut d = [0.0; 3];
                d[ax] = 1.0;
                b.max[ax] = b.max[ax].max(p.support(d));
                d[ax] = -1.0;
                b.min[ax] = b.min[ax].min(-p.support(d));
            }
        }
        if self.clip_floor() {
            b.min[1] = b.min[1].max(0.0);
        }
        b
    }
}

fn err(code: &'static str, msg: impl Into<String>, fix: impl Into<String>) -> Diag {
    Diag::err(code, 0, msg, fix)
}

fn jget<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    Some(cur)
}

/// Template part names and group names.
fn part_names(t: &Value) -> Vec<String> {
    let mut v: Vec<String> = t["parts"].as_array().map(|a| a.iter().filter_map(|p| p["name"].as_str().map(String::from)).collect()).unwrap_or_default();
    if let Some(g) = t["groups"].as_object() {
        v.extend(g.keys().cloned());
    }
    v.sort();
    v.dedup();
    v
}

/// Words a part spec may use (for suggestions).
fn allowed_keys(t: &Value, style: &Value, class: &str, part: &str) -> Vec<String> {
    let mut v = vec!["shape".to_string(), "type".to_string()];
    if let Some(sp) = jget(style, &["classes", class, part]) {
        for sect in ["values", "flags"] {
            if let Some(m) = sp[sect].as_object() {
                v.extend(m.keys().cloned());
            }
        }
    }
    if let Some(m) = t["measures"].as_object() {
        for k in m.keys() {
            if let Some(rest) = k.strip_prefix(&format!("{part}.")) {
                v.push(rest.to_string());
            }
        }
    }
    if let Some(c) = t["counts"].as_object() {
        for k in c.keys() {
            if let Some(rest) = k.strip_prefix(&format!("{part}.")) {
                v.push(rest.to_string());
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

struct Ops {
    variants: Vec<(String, String, String)>,
    values: Vec<(String, f64)>,
    steps: Vec<(String, f64)>,
    pins: Vec<(String, f64)>,
    counts: Vec<(String, f64)>,
    removed: BTreeSet<String>,
    gen_override: BTreeMap<String, (String, Value)>,
}

fn read_parts(intent: &Value, t: &Value, style: &Value, class: &str, diags: &mut Vec<Diag>) -> Ops {
    let mut ops = Ops { variants: vec![], values: vec![], steps: vec![], pins: vec![], counts: vec![], removed: BTreeSet::new(), gen_override: BTreeMap::new() };
    let names = part_names(t);
    let modifiers = style["modifiers"].as_object().cloned().unwrap_or_default();
    let Some(parts) = intent.get("parts").and_then(Value::as_object) else { return ops };
    for (pname, spec) in parts {
        if !names.contains(pname) {
            let near = nearest(pname, names.iter().cloned());
            diags.push(err("unknown_part", format!("`{class}` has no part `{pname}`"), format!("did you mean: {} (new parts with `kind:` arrive in M3)", near.join(", "))));
            continue;
        }
        if spec.as_str() == Some("remove") {
            ops.removed.insert(pname.clone());
            continue;
        }
        let Some(m) = spec.as_object() else {
            diags.push(err("bad_part", format!("`parts.{pname}` is not a map"), "write `{ adjective, measure: value }` or `remove`"));
            continue;
        };
        let style_part = jget(style, &["classes", class, pname]);
        for (k, v) in m {
            // Variants: shape / type words.
            if (k == "shape" || k == "type") && v.is_string() {
                let w = v.as_str().unwrap_or("");
                let key = format!("{pname}.{k}");
                match jget(t, &["variants", &key]) {
                    Some(vs) if vs.get(w).is_some() => ops.variants.push((pname.clone(), key, w.to_string())),
                    Some(vs) => {
                        let near = nearest(w, vs.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default().into_iter());
                        diags.push(err("unknown_variant", format!("`{pname}` has no {k} `{w}`"), format!("use one of: {}", near.join(", "))));
                    }
                    None => diags.push(err("unknown_variant", format!("`{pname}` has no `{k}` choices"), "leave it out")),
                }
                continue;
            }
            // Value adjectives: `stance: low`.
            if let (Some(sp), Some(w)) = (style_part, v.as_str())
                && let Some(def) = sp.get("values").and_then(|x| x.get(k))
            {
                let measure = def["measure"].as_str().unwrap_or("");
                match def["words"].get(w).and_then(Value::as_array) {
                    Some(r) if r.len() == 2 => {
                        let c = (r[0].as_f64().unwrap_or(0.0) + r[1].as_f64().unwrap_or(0.0)) * 0.5;
                        ops.values.push((measure.to_string(), c));
                    }
                    _ => {
                        let words = def["words"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
                        let near = nearest(w, words.into_iter());
                        diags.push(err("unknown_adjective", format!("`{pname}.{k}` has no word `{w}`"), format!("use one of: {}", near.join(", "))));
                    }
                }
                continue;
            }
            // Counts: `exhausts: 2`.
            let ck = format!("{pname}.{k}");
            if let Some(measure) = jget(t, &["counts", &ck]).and_then(Value::as_str) {
                match v.as_f64() {
                    Some(n) if n >= 0.0 && n.fract() == 0.0 => ops.counts.push((measure.to_string(), n)),
                    _ => diags.push(err("bad_count", format!("`{pname}.{k}` must be a whole number"), format!("write `{k}: 2`"))),
                }
                continue;
            }
            // Pinned numbers: `length: 0.6 m`.
            if v.is_number() && t["measures"].get(&ck).is_some() {
                ops.pins.push((ck, v.as_f64().unwrap_or(0.0)));
                continue;
            }
            // Flag adjectives with modifiers: `wide`, `very_wide`, `slightly_wide`.
            if v == &Value::Bool(true) {
                let (factor, base) = match k.split_once('_') {
                    Some((m, rest)) if modifiers.contains_key(m) => (modifiers[m].as_f64().unwrap_or(1.0), rest),
                    _ => (1.0, k.as_str()),
                };
                if let Some(list) = style_part.and_then(|sp| sp.get("flags")).and_then(|f| f.get(base)).and_then(Value::as_array) {
                    for e in list {
                        if let (Some(mname), Some(st)) = (e.get(0).and_then(Value::as_str), e.get(1).and_then(Value::as_f64)) {
                            ops.steps.push((mname.to_string(), st * factor));
                        }
                    }
                    continue;
                }
            }
            if ["rests_on", "flush_with", "attached_at", "centred_on", "aligned", "mirrored", "array_along", "kind", "from"].contains(&k.as_str()) {
                diags.push(err("not_in_m1", format!("`{pname}.{k}`: changing relationships or adding parts arrives in M3"), "use the template's relationships for now"));
                continue;
            }
            let near = nearest(k, allowed_keys(t, style, class, pname).into_iter());
            diags.push(err("unknown_word", format!("`{pname}` has no `{k}`"), format!("did you mean: {}", near.join(", "))));
        }
    }
    // Generator overrides from variants.
    for (pname, key, w) in &ops.variants {
        if let Some(v) = jget(t, &["variants", key, w])
            && let Some(g) = v.get("gen").and_then(Value::as_str)
        {
            ops.gen_override.insert(pname.clone(), (g.to_string(), v.get("params").cloned().unwrap_or(Value::Null)));
        }
    }
    ops
}

fn params_from(p: &Value, vars: &BTreeMap<String, f64>) -> Result<Params, String> {
    let mut out = Params::new();
    if let Some(m) = p.as_object() {
        for (k, v) in m {
            let pv = match v {
                Value::Array(a) => {
                    let mut nums = Vec::with_capacity(a.len());
                    for x in a {
                        nums.push(expr::eval_json(x, vars)?);
                    }
                    PVal::V(nums)
                }
                Value::String(s) if ["axis"].contains(&k.as_str()) => PVal::W(s.clone()),
                other => PVal::N(expr::eval_json(other, vars)?),
            };
            out.insert(k.clone(), pv);
        }
    }
    Ok(out)
}

fn vec_formula(v: Option<&Value>, vars: &BTreeMap<String, f64>) -> Result<V3, String> {
    match v.and_then(Value::as_array) {
        Some(a) if a.len() == 3 => Ok([expr::eval_json(&a[0], vars)?, expr::eval_json(&a[1], vars)?, expr::eval_json(&a[2], vars)?]),
        _ => Ok([0.0; 3]),
    }
}

/// Build the model for an intent (canonical JSON). `min_feature` thickens
/// thin parts (0 for the exact model; a render tier passes its own).
pub fn build(intent: &Value, min_feature: f64) -> Result<Model, Vec<Diag>> {
    let class_full = intent.get("make").and_then(Value::as_str).unwrap_or("");
    let class = class_full.split('/').next().unwrap_or("");
    let Some(t) = class_template(class) else {
        return Err(vec![err("unknown_class", format!("no class template `{class}`"), format!("use one of: {}", nearest(class, CLASSES.iter().map(|c| c.to_string())).join(", ")))]);
    };
    let style_name = intent.get("style").and_then(Value::as_str).or(t["default_style"].as_str()).unwrap_or("toon_chunky").to_string();
    let Some(style) = style_profile(&style_name) else {
        return Err(vec![err("unknown_style", format!("no style profile `{style_name}`"), "use `style: toon_chunky`")]);
    };
    let mut diags = Vec::new();
    let mut notes = Vec::new();
    if class_full.contains('/') {
        diags.push(err("not_in_m1", format!("template variants such as `{class_full}` arrive in M3"), format!("use `make: {class}`")));
    }
    if intent.get("like").is_some() {
        notes.push("`like` guides the search (M2) and analogies (M3); ignored in M1".to_string());
    }
    if intent.get("relief").is_some() {
        notes.push("relief layers are compiled from M4; recorded, not built in M1".to_string());
    }
    if style["status"].as_str().is_some_and(|s| s.starts_with("uncalibrated")) {
        notes.push(format!("style `{style_name}` is uncalibrated (calibration in M3)"));
    }

    // Measures.
    let mut measures = BTreeMap::new();
    let mut ranges: BTreeMap<String, (f64, f64, f64)> = BTreeMap::new();
    if let Some(m) = t["measures"].as_object() {
        for (k, d) in m {
            measures.insert(k.clone(), d["default"].as_f64().unwrap_or(0.0));
            ranges.insert(k.clone(), (d["min"].as_f64().unwrap_or(f64::NEG_INFINITY), d["max"].as_f64().unwrap_or(f64::INFINITY), d["step"].as_f64().unwrap_or(0.0)));
        }
    }
    let ops = read_parts(intent, &t, &style, class, &mut diags);
    for (_, key, w) in &ops.variants {
        if let Some(set) = jget(&t, &["variants", key, w, "set"]).and_then(Value::as_object) {
            for (k, v) in set {
                measures.insert(k.clone(), v.as_f64().unwrap_or(0.0));
            }
        }
    }
    for (k, v) in &ops.values {
        measures.insert(k.clone(), *v);
    }
    for (k, s) in &ops.steps {
        let step = ranges.get(k).map(|r| r.2).unwrap_or(0.0);
        if let Some(m) = measures.get_mut(k) {
            *m += s * step;
        }
    }
    let mut pinned = BTreeSet::new();
    for (k, v) in ops.counts.iter().chain(ops.pins.iter()) {
        if let Some((lo, hi, _)) = ranges.get(k)
            && (*v < *lo || *v > *hi)
        {
            diags.push(err("out_of_range", format!("`{k}` = {v} is outside {lo} to {hi}"), format!("pick a value from {lo} to {hi}")));
        }
        measures.insert(k.clone(), *v);
        pinned.insert(k.clone());
    }
    for (k, (lo, hi, _)) in &ranges {
        if !pinned.contains(k)
            && let Some(m) = measures.get_mut(k)
        {
            *m = m.clamp(*lo, *hi);
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    let seed = crate::intent::seed(intent);
    let blend = style["rules"]["style.blend"].as_f64().unwrap_or(0.0);
    let musts = merged_musts(&t, intent);
    let mut model = Model {
        class: class.to_string(),
        style: style_name,
        intent: intent.clone(),
        seed,
        measures,
        pinned,
        parts: vec![],
        solids: vec![],
        anchors: vec![],
        musts,
        notes,
        template: t.clone(),
        blend,
        removed: ops.removed.clone(),
        layer_blend: None,
        region_colours: BTreeMap::new(),
        skeleton: None,
        garment_checks: vec![],
    };
    // Colour regions: template defaults, intent overrides.
    if let Some(regs) = t["regions"].as_object() {
        for (r, c) in regs {
            let rgb = c.as_str().and_then(crate::intent::parse_hex_colour).ok_or_else(|| vec![err("template", format!("region `{r}` colour is not #rrggbb"), "this is a template bug; report it")])?;
            model.region_colours.insert(r.clone(), rgb);
        }
    }
    if let Some(cols) = intent.get("colours").and_then(Value::as_object) {
        let mut bad = Vec::new();
        for (r, c) in cols {
            match (model.region_colours.contains_key(r), c.as_str().and_then(crate::intent::parse_hex_colour)) {
                (true, Some(rgb)) => {
                    model.region_colours.insert(r.clone(), rgb);
                }
                (true, None) => bad.push(err("bad_colour", format!("colour for `{r}` is not #rrggbb"), format!("write `{r}: \"#7a5636\"`"))),
                (false, _) if model.region_colours.is_empty() => bad.push(err("no_regions", format!("class `{class}` has no colour regions"), "remove `colours`")),
                (false, _) => bad.push(err("unknown_region", format!("no colour region `{r}`"), format!("use one of: {}", nearest(r, model.region_colours.keys().cloned()).join(", ")))),
            }
        }
        if !bad.is_empty() {
            return Err(bad);
        }
    }
    place(&mut model, &t, &style, &ops, min_feature).map_err(|e| vec![err("template", e, "this is a template bug; report it")])?;

    // fit_box as a constraint: free measures move (within their ranges)
    // until the exact extents fit.
    if let Some(fb) = model.musts.get("fit_box").and_then(Value::as_array) {
        let want: Vec<f64> = fb.iter().filter_map(Value::as_f64).collect();
        if want.len() == 3 && !fits(&model, &want) {
            fit_to_box(&mut model, &t, &style, &ops, &want, &ranges, min_feature).map_err(|e| vec![err("template", e, "this is a template bug; report it")])?;
        }
    }
    Ok(model)
}

fn merged_musts(t: &Value, intent: &Value) -> Map<String, Value> {
    let mut m = t["musts"].as_object().cloned().unwrap_or_default();
    if let Some(im) = intent.get("must").and_then(Value::as_object) {
        for (k, v) in im {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

/// Box excess in the order length (z), width (x), height (y).
pub fn box_excess(model: &Model, want: &[f64]) -> [f64; 3] {
    let e = model.extents();
    let s = e.size();
    [s[2] - want[0], s[0] - want[1], s[1] - want[2]]
}

fn fits(model: &Model, want: &[f64]) -> bool {
    box_excess(model, want).iter().all(|&x| x <= 0.0005)
}

fn fit_to_box(model: &mut Model, t: &Value, style: &Value, ops: &Ops, want: &[f64], ranges: &BTreeMap<String, (f64, f64, f64)>, mf: f64) -> Result<(), String> {
    let candidates: Vec<String> = ranges
        .iter()
        // Counts and measures marked `fit: false` stay where the intent put them.
        .filter(|(k, r)| !model.pinned.contains(*k) && r.2 > 0.0 && !k.ends_with(".roundness") && !k.ends_with(".rake") && !k.ends_with(".yaw"))
        .filter(|(k, _)| t["measures"][k.as_str()]["count"].as_bool() != Some(true) && t["measures"][k.as_str()]["fit"].as_bool() != Some(false))
        .map(|(k, _)| k.clone())
        .collect();
    // Keep only the measures that move the extents (one probe each).
    let ex0 = box_excess(model, want);
    let mut free = Vec::new();
    for k in candidates {
        let mut m = model.clone();
        let (lo, hi, step) = ranges[&k];
        let v = m.measures[&k];
        let probe = if v - step >= lo { v - step } else { (v + step).min(hi) };
        m.measures.insert(k.clone(), probe);
        if place(&mut m, t, style, ops, mf).is_ok() {
            let ex = box_excess(&m, want);
            if (0..3).any(|i| (ex[i] - ex0[i]).abs() > 1e-6 && ex0[i] > -0.0005) {
                free.push(k);
            }
        }
    }
    if free.is_empty() {
        return Ok(());
    }
    let x0: Vec<f64> = free.iter().map(|k| model.measures[k]).collect();
    let lo: Vec<f64> = free.iter().map(|k| ranges[k].0).collect();
    let hi: Vec<f64> = free.iter().map(|k| ranges[k].1).collect();
    let steps: Vec<f64> = free.iter().map(|k| ranges[k].2).collect();
    let h: Vec<f64> = steps.iter().map(|s| s * 0.05).collect();
    let base = model.clone();
    let eval = |x: &[f64]| -> Vec<f64> {
        let mut m = base.clone();
        for (i, k) in free.iter().enumerate() {
            m.measures.insert(k.clone(), x[i]);
        }
        let mut r: Vec<f64> = match place(&mut m, t, style, ops, mf) {
            Ok(()) => box_excess(&m, want).iter().map(|e| (e + 0.001).max(0.0) * 100.0).collect(),
            Err(_) => vec![1e3; 3],
        };
        // Stay near the intended values (in steps).
        for i in 0..x.len() {
            r.push((x[i] - x0[i]) / steps[i].max(1e-9) * 0.01);
        }
        r
    };
    let (x, _) = crate::solve::levenberg_marquardt(&eval, &x0, &lo, &hi, &h, 30);
    for (i, k) in free.iter().enumerate() {
        model.measures.insert(k.clone(), x[i]);
    }
    place(model, t, style, ops, mf)?;
    let moved: Vec<String> = free.iter().enumerate().filter(|(i, _)| (x[*i] - x0[*i]).abs() > 1e-6).map(|(i, k)| format!("{k} {:.3}->{:.3}", x0[i], x[i])).collect();
    if !moved.is_empty() {
        model.notes.push(format!("fit_box moved: {}", moved.join(", ")));
    }
    Ok(())
}

/// Generate and place every part from the current measures.
fn place(model: &mut Model, t: &Value, style: &Value, ops: &Ops, mf: f64) -> Result<(), String> {
    let mut vars = model.measures.clone();
    if let Some(r) = style["rules"].as_object() {
        for (k, v) in r {
            vars.insert(k.clone(), v.as_f64().unwrap_or(0.0));
        }
    }
    // Formulas may keep thin parts above the tier's minimum feature.
    vars.insert("min_feature".to_string(), mf);
    // A class may blend its parts more softly than the style (clouds, bushes).
    if let Some(b) = t.get("blend") {
        model.blend = expr::eval_json(b, &vars)?.max(0.0);
    }
    // Layered blends (characters): one radius per layer.
    if let Some(lb) = t.get("blend_layers") {
        let mut k = [0.0; 3];
        for (i, name) in LAYERS.iter().enumerate() {
            k[i] = match lb.get(*name) {
                Some(f) => expr::eval_json(f, &vars)?.max(0.0),
                None => 0.0,
            };
        }
        model.layer_blend = Some(k);
    }
    let groups = t["groups"].as_object().cloned().unwrap_or_default();
    let removed_parts: BTreeSet<String> = {
        let mut s = BTreeSet::new();
        for r in &ops.removed {
            s.insert(r.clone());
            if let Some(g) = groups.get(r).and_then(Value::as_array) {
                for x in g {
                    if let Some(n) = x.as_str() {
                        s.insert(n.to_string());
                    }
                }
            }
        }
        s
    };
    let tparts = t["parts"].as_array().cloned().unwrap_or_default();
    let mut parts: Vec<Part> = Vec::new();
    // Per created part: its template spec index.
    let mut spec_of: Vec<usize> = Vec::new();
    for (si, sp) in tparts.iter().enumerate() {
        let name = sp["name"].as_str().unwrap_or("").to_string();
        let group = sp["group"].as_str().map(String::from).or_else(|| {
            groups.iter().find(|(_, members)| members.as_array().is_some_and(|a| a.iter().any(|m| m.as_str() == Some(name.as_str())))).map(|(g, _)| g.clone())
        });
        if removed_parts.contains(&name) || group.as_ref().is_some_and(|g| removed_parts.contains(g) && ops.removed.contains(g)) {
            continue;
        }
        let (kind, pspec) = match ops.gen_override.get(&name) {
            Some((g, p)) => {
                let mut merged = sp["params"].clone();
                if let (Some(m), Some(o)) = (merged.as_object_mut(), p.as_object()) {
                    for (k, v) in o {
                        m.insert(k.clone(), v.clone());
                    }
                    if g == "ellipsoid" {
                        m.remove("bevel");
                        m.remove("taper");
                    }
                }
                (g.clone(), merged)
            }
            None => (sp["gen"].as_str().unwrap_or("").to_string(), sp["params"].clone()),
        };
        let params = params_from(&pspec, &vars)?;
        let rot = vec_formula(sp.get("rot"), &vars)?;
        let q = quat_ypr(crate::dmath::deg(rot[0]), crate::dmath::deg(rot[1]), crate::dmath::deg(rot[2]));
        let at = vec_formula(sp.get("at"), &vars)?;
        let role = sp["role"].as_str().unwrap_or("body").to_string();
        let solid = sp["solid"].as_str().unwrap_or("body").to_string();
        let count = match sp.get("count") {
            Some(c) => expr::eval_json(c, &vars)?.round().max(0.0) as usize,
            None => 1,
        };
        let spread = match sp.get("spread") {
            Some(s) => expr::eval_json(s, &vars)?,
            None => 0.0,
        };
        let mirror = sp["mirror"].as_str() == Some("x");
        let split = sp["split"].as_str() == Some("x");
        let role = if sp["op"].as_str() == Some("cut") { "cut".to_string() } else { role };
        let layer = match sp["layer"].as_str() {
            None => 0,
            Some(l) => LAYERS.iter().position(|x| *x == l).ok_or(format!("{name}: layer `{l}` is not one of {}", LAYERS.join(", ")))? as u8,
        };
        let region = sp["region"].as_str().map(String::from);
        if let Some(regs) = t["regions"].as_object()
            && role != "cut"
            && !region.as_ref().is_some_and(|r| regs.contains_key(r))
        {
            return Err(format!("{name}: needs a `region` from the template's regions ({})", regs.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        let bone = bone_list(&sp["bone"]);
        let seed = model.seed ^ (crate::sha256::sha256(name.as_bytes())[0] as u32);
        // Garment shells are built once the parts they cover are placed.
        let mut shape = if kind == "layer" {
            GenOut { node: Node::Sphere { r: mf.max(1e-3) }, anchors: Vec::new(), proxy: Proxy::Sphere { c: [0.0; 3], r: mf.max(1e-3) }, min_dim: mf, chart: generator::Chart::Free }
        } else {
            generator::generate(&kind, &params, mf, seed).map_err(|e| format!("{name}: {e}"))?
        };
        // Template-declared anchors in the part's own frame.
        if let Some(extra) = sp["anchors"].as_object() {
            for (an, def) in extra {
                let p = vec_formula(def.get("at"), &vars)?;
                let n = norm_or(vec_formula(def.get("n"), &vars)?, [0.0, 1.0, 0.0]);
                shape.anchors.retain(|a| &a.name != an);
                shape.anchors.push(Anchor { name: an.clone(), p, n, on_surface: def["on_surface"].as_bool().unwrap_or(false) });
            }
        }
        // Copy layout: a line along `spread_axis` (default x) or a ring
        // round the y axis (`layout: ring`, `radius`, `start`, `arc` in
        // degrees), with optional seeded jitter per copy.
        let ring = sp["layout"].as_str() == Some("ring");
        let ring_r = match sp.get("radius") {
            Some(r) => expr::eval_json(r, &vars)?,
            None => 0.0,
        };
        let ring_start = match sp.get("start") {
            Some(r) => expr::eval_json(r, &vars)?,
            None => 0.0,
        };
        let ring_arc = match sp.get("arc") {
            Some(r) => expr::eval_json(r, &vars)?,
            None => 360.0,
        };
        // Copies of a line layout step along `spread_axis` (x, y or z) or
        // along any direction `spread_dir` (a formula vector).
        let spread_dir = match sp.get("spread_dir") {
            Some(d) => norm_or(vec_formula(Some(d), &vars)?, [1.0, 0.0, 0.0]),
            None => match sp["spread_axis"].as_str() {
                Some("y") => [0.0, 1.0, 0.0],
                Some("z") => [0.0, 0.0, 1.0],
                _ => [1.0, 0.0, 0.0],
            },
        };
        let jitter = vec_formula(sp.get("jitter"), &vars)?;
        let jitter_yaw = match sp.get("jitter_yaw") {
            Some(r) => expr::eval_json(r, &vars)?,
            None => 0.0,
        };
        // Parts that must jitter together (a picket and its tip) share the
        // seed of the part named by `jitter_with`.
        let jseed = match sp["jitter_with"].as_str() {
            Some(other) => model.seed ^ (crate::sha256::sha256(other.as_bytes())[0] as u32),
            None => seed,
        };
        let mut jrng = crate::rng::Pcg32::new(jseed as u64, 41);
        for i in 0..count {
            let mut tpos = at;
            let mut qi = q;
            if ring {
                let a = if (ring_arc - 360.0).abs() < 1e-9 || count < 2 { ring_start + ring_arc * i as f64 / count.max(1) as f64 } else { ring_start + ring_arc * i as f64 / (count - 1) as f64 };
                let (s, c) = crate::dmath::sin_cos(crate::dmath::deg(a));
                tpos = [at[0] + ring_r * s, at[1], at[2] + ring_r * c];
                qi = qmul(quat_axis([0.0, 1.0, 0.0], crate::dmath::deg(a)), q);
            } else if count > 1 {
                tpos = add(tpos, mul(spread_dir, (i as f64 - (count as f64 - 1.0) * 0.5) * spread));
            }
            if jitter != [0.0; 3] || jitter_yaw != 0.0 {
                for (k, j) in jitter.iter().enumerate() {
                    tpos[k] += jrng.range(-j.abs(), j.abs());
                }
                let yaw = jrng.range(-jitter_yaw.abs(), jitter_yaw.abs());
                qi = qmul(quat_axis([0.0, 1.0, 0.0], crate::dmath::deg(yaw)), qi);
            }
            let pn = if count > 1 { format!("{name}.{i}") } else { name.clone() };
            let sides: Vec<(&str, bool)> = if split { vec![("l", false), ("r", true)] } else { vec![("", false)] };
            for (side, reflect) in sides {
                let full = if side.is_empty() { pn.clone() } else { format!("{pn}.{side}") };
                let solid_name = solid.replace("{side}", side);
                parts.push(Part {
                    name: full,
                    base: name.clone(),
                    group: group.clone(),
                    role: role.clone(),
                    solid: solid_name,
                    kind: kind.clone(),
                    params: params.clone(),
                    shape: shape.clone(),
                    q: qi,
                    t: tpos,
                    mirror_x: mirror,
                    reflect,
                    id: parts.len() as u32,
                    layer,
                    region: region.clone(),
                    bone: bone.clone(),
                    over: Vec::new(),
                });
                spec_of.push(si);
            }
        }
    }

    // Relationships, in dependency order.
    let n = parts.len();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        // A garment shell waits for every part it covers.
        if parts[i].kind == "layer" {
            let names: Vec<String> = tparts[spec_of[i]]["over"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
            let over: Vec<usize> = (0..n).filter(|&j| j != i && names.contains(&parts[j].base)).collect();
            if over.is_empty() {
                return Err(format!("{}: `over` names no placed part ({})", parts[i].name, names.join(", ")));
            }
            deps[i].extend(over.iter().copied());
            parts[i].over = over;
        }
        for rel in tparts[spec_of[i]]["relations"].as_array().into_iter().flatten() {
            let to = rel["to"].as_str().unwrap_or("");
            let target = to.split('.').next().unwrap_or("");
            for (j, p) in parts.iter().enumerate() {
                if p.base == target && j != i {
                    deps[i].push(j);
                }
            }
        }
    }
    let (order, cyclic) = crate::solve::dependency_order(n, &deps);
    if !cyclic.is_empty() {
        return Err(format!("relationship cycle through {}", cyclic.iter().map(|&i| parts[i].name.clone()).collect::<Vec<_>>().join(", ")));
    }
    let mut probes = Vec::new();
    for &i in &order {
        if parts[i].kind == "layer" {
            let covered: Vec<Node> = parts[i].over.iter().map(|&j| parts[j].field()).collect();
            let sp = &tparts[spec_of[i]];
            let mut shape = layer_shell(sp, &parts[i].params, &covered, &vars, mf).map_err(|e| format!("{}: {e}", parts[i].name))?;
            // Thickness probes (serde_json maps iterate in key order).
            if let Some(cs) = sp.get("checks").and_then(Value::as_object) {
                let target = match parts[i].params.get("thickness") {
                    Some(PVal::N(v)) => *v,
                    _ => 0.02,
                };
                for (name, v) in cs {
                    let p = vec_formula(Some(v), &vars).map_err(|e| format!("{}: check {name}: {e}", parts[i].name))?;
                    probes.push(GarmentProbe { part: i, name: name.clone(), p, target });
                }
            }
            // Template-declared anchors (world space for shells) win.
            for a in parts[i].shape.anchors.drain(..) {
                shape.anchors.retain(|x| x.name != a.name);
                shape.anchors.push(a);
            }
            parts[i].shape = shape;
            parts[i].q = QID;
            parts[i].t = [0.0; 3];
        }
        for rel in tparts[spec_of[i]]["relations"].as_array().cloned().unwrap_or_default() {
            let kind = rel["kind"].as_str().unwrap_or("");
            let to = rel["to"].as_str().unwrap_or("");
            let (tname, tanchor) = to.split_once('.').unwrap_or((to, "centre"));
            let Some(j) = parts.iter().position(|p| p.base == tname) else {
                return Err(format!("{}: relation target `{to}` is missing", parts[i].name));
            };
            match kind {
                "attached_at" => {
                    let off = vec_formula(rel.get("offset"), &vars)?;
                    let ta = parts[j].anchor(tanchor).ok_or(format!("{to}: no anchor"))?;
                    let selfa = rel["self"].as_str().unwrap_or("centre");
                    let la = parts[i].shape.anchor(selfa).ok_or(format!("{}: no anchor {selfa}", parts[i].name))?.p;
                    let mut tp = ta.p;
                    // The +x copy of a mirrored target.
                    if parts[j].mirror_x && tp[0] < 0.0 {
                        tp[0] = -tp[0];
                    }
                    parts[i].t = sub(add(tp, off), qrot(parts[i].q, la));
                    if parts[i].reflect {
                        parts[i].t[0] = parts[i].t[0].abs();
                    }
                }
                "rests_on" => {
                    let embed = match rel.get("embed") {
                        Some(e) => expr::eval_json(e, &vars)?,
                        None => 0.0,
                    };
                    let target = parts[j].field().prepare();
                    let samples = underside_samples(&parts[i]);
                    let shift = crate::solve::contact_shift(&samples, &target, [0.0, -1.0, 0.0], embed).ok_or(format!("{}: nothing below to rest on", parts[i].name))?;
                    parts[i].t[1] -= shift;
                }
                "centred_on" => {
                    let c = parts[j].anchor("centre").map(|a| a.p).unwrap_or(parts[j].t);
                    for ax in rel["axes"].as_str().unwrap_or("xz").chars() {
                        let k = match ax {
                            'x' => 0,
                            'y' => 1,
                            _ => 2,
                        };
                        parts[i].t[k] = c[k];
                    }
                }
                "aligned" => {
                    parts[i].q = parts[j].q;
                }
                other => return Err(format!("relation `{other}` is not supported in M1")),
            }
        }
    }

    // Solids.
    let mut solids: Vec<Solid> = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        match solids.iter_mut().find(|s| s.name == p.solid) {
            Some(s) => s.parts.push(i),
            None => solids.push(Solid { name: p.solid.clone(), parts: vec![i], origin: [0.0; 3], mirror_of: None, loose: false }),
        }
    }
    for s in &mut solids {
        let first = &parts[s.parts[0]];
        if first.role == "wheel" {
            s.loose = true;
            s.origin = first.anchor("hub").map(|a| a.p).unwrap_or(first.t);
        }
        if first.reflect {
            s.mirror_of = solids_name_left(&s.name);
        }
    }
    model.parts = parts;
    model.solids = solids;
    model.garment_checks = probes;

    // Skeleton (characters).
    if let Some(sk) = t.get("skeleton") {
        let at = |v: Option<&Value>| vec_formula(v, &vars);
        let num = |v: &Value| expr::eval_json(v, &vars);
        let sk = crate::rig::parse(sk, &at, &num)?;
        crate::rig::check_bones(model, &sk)?;
        if model.solids.iter().any(|s| s.mirror_of.is_some() || s.loose) {
            return Err("a skinned template needs one solid per piece without split or loose solids".into());
        }
        model.skeleton = Some(sk);
    }

    // Gameplay anchors: on a part (`at`) or on a joint (`joint` + `offset`).
    let mut anchors = Vec::new();
    for a in t["anchors"].as_array().into_iter().flatten() {
        let name = a["name"].as_str().unwrap_or("");
        let q = socket_q(a.get("q"), &vars, name)?;
        if let Some(jn) = a["joint"].as_str() {
            let j = model.skeleton.as_ref().and_then(|sk| sk.index(jn).map(|i| sk.joints[i].at)).ok_or(format!("anchor {name}: no joint `{jn}`"))?;
            let off = vec_formula(a.get("offset"), &vars)?;
            let n = norm_or(vec_formula(a.get("n"), &vars)?, [0.0, 1.0, 0.0]);
            anchors.push(GameAnchor { name: name.to_string(), p: add(j, off), n, part: String::new(), joint: Some(jn.to_string()), q });
            continue;
        }
        let at = a["at"].as_str().unwrap_or("");
        if let Some(mut ga) = resolve_anchor(model, name, at) {
            ga.q = q;
            anchors.push(ga);
        }
    }
    model.anchors = anchors;
    Ok(())
}

/// A half-space `n·(p − at) ≤ 0` from a spec `{n, at, mirror}`; with
/// `mirror: "x"` it applies to |x| (both sides alike).
struct Half {
    node: Node,
    n: V3,
    off: f64,
    mirror: bool,
}

fn half_space(h: &Value, vars: &BTreeMap<String, f64>) -> Result<Half, String> {
    let n = norm_or(vec_formula(h.get("n"), vars)?, [0.0, 1.0, 0.0]);
    let at = vec_formula(h.get("at"), vars)?;
    let off = dot(n, at);
    let mirror = h["mirror"].as_str() == Some("x");
    let plane = Node::Plane { n, off };
    let node = if mirror { Node::Mirror { axis: 0, child: plane.boxed() } } else { plane };
    Ok(Half { node, n, off, mirror })
}

/// Shrink a box to an axis-aligned half-space (others leave it as is).
fn clip_box(b: &mut Aabb, h: &Half) {
    for k in 0..3 {
        if h.n[k].abs() < 1.0 - 1e-9 {
            continue;
        }
        if h.n[k] > 0.0 {
            b.max[k] = b.max[k].min(h.off);
            if h.mirror && k == 0 {
                b.min[0] = b.min[0].max(-h.off);
            }
        } else if !(h.mirror && k == 0) {
            b.min[k] = b.min[k].max(-h.off);
        }
    }
}

/// Distance along `dir` from `from` to the first surface of `f`, if any
/// within `max` (sphere tracing on the field's safe distance).
pub fn march(f: &Node, from: V3, dir: V3, max: f64) -> Option<f64> {
    let mut t = 0.0;
    for _ in 0..512 {
        let d = f.dist(add(from, mul(dir, t)));
        if d < 1e-5 {
            return Some(t);
        }
        t += d.max(1e-4);
        if t > max {
            return None;
        }
    }
    None
}

/// A garment shell (`gen: layer`) in world space: the covered parts'
/// fields grown by `thickness`, kept inside the `keep` half-spaces, minus
/// the `cuts` (each an intersection of half-spaces), with an optional
/// `drape`: below `below` the garment hangs as a hollow skirt, `ease`
/// wider than the body at the top, its half-width growing `flare` per
/// metre of drop down to the hem (the keep facing down), open underneath.
fn layer_shell(sp: &Value, params: &Params, covered: &[Node], vars: &BTreeMap<String, f64>, mf: f64) -> Result<GenOut, String> {
    let num = |k: &str, d: f64| match params.get(k) {
        Some(PVal::N(v)) => *v,
        _ => d,
    };
    let thick = num("thickness", 0.02).max(1e-4);
    let fill = num("fill", 0.0).max(0.0);
    let base = if covered.len() == 1 { covered[0].clone() } else { Node::SmoothUnion { k: fill, children: covered.to_vec() } };
    let grown = Node::Round { r: thick, child: base.boxed() };
    let mut b = grown.bounds();
    let mut keeps = Vec::new();
    for h in sp["keep"].as_array().into_iter().flatten() {
        let h = half_space(h, vars)?;
        clip_box(&mut b, &h);
        keeps.push(h);
    }
    if (0..3).any(|k| b.min[k] >= b.max[k]) {
        return Err("the keep bands leave nothing".to_string());
    }
    let bbox = |b: &Aabb| Node::RoundBox { half: mul(b.size(), 0.5), r: 0.0 }.at(QID, b.centre());
    let mut ch = vec![grown.clone()];
    ch.extend(keeps.iter().map(|h| h.node.clone()));
    ch.push(bbox(&b));
    let mut node = Node::Intersect(ch);
    let top = b.max[1];
    let mut bottom = b.min[1];
    if let Some(dr) = sp.get("drape") {
        let ev = |k: &str, d: f64| -> Result<f64, String> {
            match dr.get(k) {
                Some(v) => expr::eval_json(v, vars),
                None => Ok(d),
            }
        };
        let below = ev("below", b.min[1])?;
        let ease = ev("ease", 0.0)?;
        let flare = ev("flare", 0.0)?;
        let wall = ev("wall", thick)?.max(mf);
        let hem = keeps.iter().find(|h| !h.mirror && h.n[1] < -1.0 + 1e-9).map(|h| -h.off).ok_or("a drape needs a keep facing down (the hem)")?;
        let drop = below - hem;
        if drop <= wall {
            return Err(format!("drape: `below` {below} must be above the hem {hem}"));
        }
        // The body's half-sizes at `below` (before cuts, which may open the front).
        let pf = grown.clone().prepare();
        let gb = pf.bounds();
        let reach = |from: V3, dir: V3| march(&pf, from, dir, 4.0).map(|t| dot(add(from, mul(dir, t)), dir)).ok_or("drape: the shell does not cross `below`".to_string());
        let px = -reach([gb.max[0] + 0.01, below, 0.0], [-1.0, 0.0, 0.0])?;
        let nx = reach([gb.min[0] - 0.01, below, 0.0], [1.0, 0.0, 0.0])?;
        let zf = -reach([0.0, below, gb.max[2] + 0.01], [0.0, 0.0, -1.0])?;
        let zb = reach([0.0, below, gb.min[2] - 0.01], [0.0, 0.0, 1.0])?;
        let ax = px.max(-nx) + ease;
        let cz = (zf + zb) * 0.5;
        let az = (zf - zb) * 0.5 + ease;
        let s = 1.0 + flare * drop / ax;
        let slope = -(s - 1.0) / drop;
        let rc = 0.6 * ax.min(az);
        let tall = drop + 0.4;
        let frustum = |hx: f64, hz: f64, r: f64| Node::Taper { slope, child: Node::RoundBox { half: [hx, tall * 0.5, hz], r }.at(QID, [0.0, -drop * 0.5, 0.0]).boxed() }.at(QID, [0.0, below, cz]);
        let band = |lo: f64, hi: f64| [Node::Plane { n: [0.0, -1.0, 0.0], off: -lo }, Node::Plane { n: [0.0, 1.0, 0.0], off: hi }];
        // The skirt is widest at the hem (scale `s` there), so a box at the
        // hem's size holds it exactly and keeps its bounds (the model's
        // extents) tight; the taper's own bounds reach past the hem.
        let hem_box = Node::RoundBox { half: [ax * s, drop * 0.5, az * s], r: 0.0 }.at(QID, [0.0, (hem + below) * 0.5, cz]);
        let mut outer = vec![frustum(ax, az, rc), hem_box];
        outer.extend(band(hem, below));
        let mut inner = vec![frustum(ax - wall, az - wall, (rc - wall).max(0.0))];
        inner.extend(band(hem - 0.1, below - wall));
        let skirt = Node::Subtract { a: Node::Intersect(outer).boxed(), b: Node::Intersect(inner).boxed(), k: 0.0 };
        let upper = Node::Intersect(vec![node, Node::Plane { n: [0.0, -1.0, 0.0], off: -below }]);
        node = Node::SmoothUnion { k: fill, children: vec![upper, skirt] };
        bottom = hem;
    }
    let cuts: Vec<Node> = sp["cuts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| c.as_array().into_iter().flatten().map(|h| half_space(h, vars).map(|h| h.node)).collect::<Result<Vec<Node>, String>>().map(Node::Intersect))
        .collect::<Result<_, _>>()?;
    if !cuts.is_empty() {
        node = Node::Subtract { a: node.boxed(), b: Node::Union(cuts).boxed(), k: 0.0 };
    }
    // Anchors: centre, top, bottom and the front surface at mid height.
    let fb = node.bounds();
    let c = fb.centre();
    let mid = (top + bottom) * 0.5;
    let pf = node.clone().prepare();
    let front_z = march(&pf, [0.0, mid, fb.max[2] + 0.01], [0.0, 0.0, -1.0], 4.0).map(|t| fb.max[2] + 0.01 - t).unwrap_or(fb.max[2]);
    let anchors = vec![
        Anchor { name: "centre".into(), p: [0.0, mid, c[2]], n: [0.0, 1.0, 0.0], on_surface: false },
        Anchor { name: "top".into(), p: [0.0, top, c[2]], n: [0.0, 1.0, 0.0], on_surface: false },
        Anchor { name: "bottom".into(), p: [0.0, bottom, c[2]], n: [0.0, -1.0, 0.0], on_surface: false },
        Anchor { name: "front".into(), p: [0.0, mid, front_z], n: [0.0, 0.0, 1.0], on_surface: true },
    ];
    let mut pts = Vec::with_capacity(8);
    for i in 0..8 {
        pts.push([if i & 1 == 0 { fb.min[0] } else { fb.max[0] }, if i & 2 == 0 { fb.min[1] } else { fb.max[1] }, if i & 4 == 0 { fb.min[2] } else { fb.max[2] }]);
    }
    Ok(GenOut { node, anchors, proxy: Proxy::Hull { pts }, min_dim: thick, chart: generator::Chart::Free })
}

fn solids_name_left(name: &str) -> Option<String> {
    name.strip_suffix('r').map(|s| format!("{s}l"))
}

/// `part.anchor`, `part.l.anchor`, `counted.first.anchor` / `.last.`.
fn resolve_anchor(model: &Model, name: &str, at: &str) -> Option<GameAnchor> {
    let segs: Vec<&str> = at.split('.').collect();
    let (pname, aname) = match segs.as_slice() {
        [p, a] => (p.to_string(), a.to_string()),
        [p, which, a] => {
            let candidates: Vec<&Part> = model.parts.iter().filter(|x| x.base == *p).collect();
            let pick = match *which {
                "first" => candidates.first().map(|x| x.name.clone()),
                "last" => candidates.last().map(|x| x.name.clone()),
                side => Some(format!("{p}.{side}")),
            }?;
            (pick, a.to_string())
        }
        _ => return None,
    };
    let part = model.part(&pname)?;
    let a = part.anchor(&aname)?;
    Some(GameAnchor { name: name.to_string(), p: a.p, n: a.n, part: pname, joint: None, q: None })
}

/// Points on a part's underside (its own field meshed coarsely).
fn underside_samples(p: &Part) -> Vec<V3> {
    let f = p.field().prepare();
    let b = f.bounds();
    let s = b.size();
    let h = (s[0].min(s[2]) / 8.0).max(0.005);
    let m = crate::mesh::extract(&f, b, &crate::mesh::Options::new(h, 1));
    let low = b.min[1] + s[1] * 0.35;
    m.pos.iter().copied().filter(|q| q[1] <= low).collect()
}

/// Check the intent's top-level fields that M1 reads, without building.
pub fn describe_measures(model: &Model) -> Vec<(String, f64)> {
    model.measures.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

/// A hard proxy summary used by tests and the collision module.
pub fn proxy_world(p: &Part) -> Proxy {
    let tr = |v: V3| {
        let mut w = add(qrot(p.q, v), p.t);
        if p.reflect {
            w[0] = -w[0];
        }
        w
    };
    match &p.shape.proxy {
        Proxy::Box { c, half, q } => Proxy::Box { c: tr(*c), half: *half, q: if p.reflect { mirror_quat(qmul(p.q, *q)) } else { qmul(p.q, *q) } },
        Proxy::Cylinder { c, q, r, half_h } => Proxy::Cylinder { c: tr(*c), q: if p.reflect { mirror_quat(qmul(p.q, *q)) } else { qmul(p.q, *q) }, r: *r, half_h: *half_h },
        Proxy::Capsule { a, b, r } => Proxy::Capsule { a: tr(*a), b: tr(*b), r: *r },
        Proxy::Sphere { c, r } => Proxy::Sphere { c: tr(*c), r: *r },
        Proxy::Hull { pts } => Proxy::Hull { pts: pts.iter().map(|v| tr(*v)).collect() },
    }
}

/// The rotation of a mirrored frame for symmetric proxies (boxes and
/// cylinders are symmetric, so mirroring maps to a proper rotation).
fn mirror_quat(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], q[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kart(src: &str) -> Model {
        let j = crate::intent::parse_text(src).unwrap();
        build(&j, 0.0).unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// A template anchor's `q` is read, normalised, and refused when it is
    /// not four numbers or has no length; no `q` is no rotation.
    #[test]
    fn socket_q_reads_and_normalises() {
        let vars = BTreeMap::new();
        assert_eq!(socket_q(None, &vars, "a").unwrap(), None);
        assert_eq!(socket_q(Some(&Value::Null), &vars, "a").unwrap(), None);
        let q = socket_q(Some(&serde_json::json!([0, 2, 0, 2])), &vars, "a").unwrap().unwrap();
        assert!((q[1] - 0.5f64.sqrt()).abs() < 1e-12 && (q[3] - 0.5f64.sqrt()).abs() < 1e-12);
        for bad in [serde_json::json!([0, 0, 0, 0]), serde_json::json!([1, 0, 0]), serde_json::json!("x")] {
            assert!(socket_q(Some(&bad), &vars, "a").is_err(), "{bad}");
        }
    }

    #[test]
    fn the_base_kart_builds_with_its_parts_solids_and_anchors() {
        let m = kart("make: kart\nstyle: toon_chunky\n");
        let names: Vec<&str> = m.parts.iter().map(|p| p.name.as_str()).collect();
        for want in ["chassis", "nose", "side_pods", "bumper", "engine_cover", "exhausts.0", "exhausts.1", "wing", "wing_struts", "seat_base", "seat_back", "steering_wheel", "front_tyres.l", "front_tyres.r", "rear_tyres.l", "rear_tyres.r"] {
            assert!(names.contains(&want), "missing {want} in {names:?}");
        }
        let solids: Vec<&str> = m.solids.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(solids, vec!["body", "wheel_fl", "wheel_fr", "wheel_rl", "wheel_rr"]);
        let hub = m.anchors.iter().find(|a| a.name == "wheel_hub.fl").unwrap();
        assert!((hub.p[0] - 0.9).abs() < 1e-12 && (hub.p[1] - 0.33).abs() < 1e-12 && (hub.p[2] - 0.95).abs() < 1e-12);
        let fr = m.anchors.iter().find(|a| a.name == "wheel_hub.fr").unwrap();
        assert!((fr.p[0] + 0.9).abs() < 1e-12);
        // rests_on: the seat sits on the tub top, 1 cm embedded.
        let seat = m.part("seat_base").unwrap();
        assert!((seat.t[1] - 0.08 - (0.51 - 0.01)).abs() < 1e-5, "seat y {}", seat.t[1]);
        // attached_at: the steering wheel centre is on the column's upper end.
        let col = m.part("steering_column").unwrap().anchor("back").unwrap();
        let sw = m.part("steering_wheel").unwrap().anchor("centre").unwrap();
        assert!(dist(col.p, sw.p) < 1e-12);
    }

    #[test]
    fn adjectives_variants_counts_and_removal() {
        let base = kart("make: kart\n");
        let k = kart("make: kart\nparts:\n  chassis: { stance: very_low, very_wide }\n  rear_tyres: { size: very_oversized }\n  engine_cover: { shape: bubble, exhausts: 4 }\n  wing: remove\n");
        assert!((k.measures["chassis.ride_height"] - 0.16).abs() < 1e-12);
        assert!((k.measures["chassis.width"] - (base.measures["chassis.width"] + 2.0 * 0.08).min(1.2)).abs() < 1e-12);
        assert!((k.measures["rear_tyres.radius"] - 0.48).abs() < 1e-12);
        assert_eq!(k.parts.iter().filter(|p| p.base == "exhausts").count(), 4);
        assert!(k.part("wing").is_none() && k.part("wing_struts").is_none());
        assert_eq!(k.part("engine_cover").unwrap().kind, "ellipsoid");
        let e = build(&crate::intent::parse_text("make: kart\nparts:\n  chasis: { wide }\n").unwrap(), 0.0).unwrap_err();
        assert_eq!(e[0].code, "unknown_part");
        assert!(e[0].fix.contains("chassis"));
        let e = build(&crate::intent::parse_text("make: kart\nparts:\n  chassis: { wiide }\n").unwrap(), 0.0).unwrap_err();
        assert!(e[0].fix.contains("wide"), "{e:?}");
        let e = build(&crate::intent::parse_text("make: crat\n").unwrap(), 0.0).unwrap_err();
        assert_eq!(e[0].code, "unknown_class");
        assert!(e[0].fix.contains("crate"), "{e:?}");
    }

    #[test]
    fn the_base_kart_fits_its_benchmark_box() {
        let m = kart(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel"));
        let ex = box_excess(&m, &[3.1, 2.21, 1.27]);
        assert!(ex.iter().all(|&e| e <= 0.001), "excess {ex:?}, size {:?}", m.extents().size());
    }

    #[test]
    fn fit_box_moves_free_measures_until_it_fits() {
        let m = kart("make: kart\nmust:\n  fit_box: [2.9, 2.21, 1.27] m\n");
        let ex = box_excess(&m, &[2.9, 2.21, 1.27]);
        assert!(ex[0] <= 0.0011, "length excess {ex:?}");
        assert!(m.notes.iter().any(|n| n.starts_with("fit_box moved")), "{:?}", m.notes);
    }
}

#[cfg(test)]
mod shells {
    use super::*;

    fn shell_params(thick: f64) -> Params {
        let mut p = Params::new();
        p.insert("thickness".into(), PVal::N(thick));
        p
    }

    #[test]
    fn a_layer_shell_covers_its_parts_at_the_given_thickness_inside_its_bands() {
        let vars = BTreeMap::new();
        let body = vec![Node::Sphere { r: 0.3 }.label(0), Node::Sphere { r: 0.2 }.at(QID, [0.0, -0.4, 0.0]).label(1)];
        let sp: Value = serde_json::json!({ "keep": [{ "n": [0, 1, 0], "at": [0, "0.1", 0] }, { "n": [1, 0, 0], "at": [0.25, 0, 0], "mirror": "x" }] });
        let g = layer_shell(&sp, &shell_params(0.02), &body, &vars, 0.004).unwrap();
        let f = g.node.clone().prepare();
        // Contains the covered parts (inside the bands).
        for p in [[0.2, 0.0, 0.0], [-0.2, 0.0, 0.0], [0.0, 0.05, 0.29], [0.0, -0.5, 0.0], [0.0, -0.59, 0.0]] {
            assert!(f.dist(p) < 0.0, "{p:?} is not covered");
        }
        // Thickness at a probe: the surface sits 0.02 out from the sphere.
        let t = march(&f, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], 2.0).unwrap();
        assert!((1.0 - t - 0.32).abs() < 1e-3, "front at {}", 1.0 - t);
        // The bands clip it: nothing above y = 0.1 or beyond |x| = 0.25.
        assert!(f.dist([0.0, 0.2, 0.0]) > 0.0);
        assert!(f.dist([0.28, 0.0, 0.0]) > 0.0 && f.dist([-0.28, 0.0, 0.0]) > 0.0);
        assert!(f.bounds().max[1] <= 0.1 + 1e-9 && f.bounds().max[0] <= 0.25 + 1e-9);
        // Anchors and proxy.
        // Front at mid height (y = -0.26): on the grown upper sphere, z about 0.186.
        assert!(g.anchor("front").is_some_and(|a| (a.p[2] - (0.32f64 * 0.32 - 0.26 * 0.26).sqrt()).abs() < 0.01));
        assert!(matches!(g.proxy, Proxy::Hull { ref pts } if pts.len() == 8));
    }

    #[test]
    fn the_marshal_builds_with_layers_regions_skeleton_and_joint_anchors() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/model_bench/characters/frontier_marshal.evmodel")).unwrap();
        let j = crate::intent::parse_text(&src).unwrap();
        assert!(crate::intent::validate(&j).is_empty(), "{:?}", crate::intent::validate(&j));
        let m = build(&j, 0.0128).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(m.layer_blend, Some([0.03, 0.006, 0.003]));
        assert_eq!(m.region_colours["leather"], [0x7b, 0x52, 0x33]);
        let sk = m.skeleton.as_ref().unwrap();
        // root, hips, spine, chest, neck, head and eight left/right pairs.
        assert_eq!(sk.joints.len(), 22);
        for a in ["hand_l", "hand_r", "seat", "head_look", "head_top", "foot_l", "foot_r"] {
            assert!(m.anchors.iter().any(|x| x.name == a && x.joint.is_some()), "{a}");
        }
        let coat = m.part("coat").unwrap();
        assert_eq!(coat.over.len(), 4);
        assert!(m.parts.iter().filter(|p| p.base == "fringe").count() == 13);
        // The coat's thickness probes: measured off the body layer.
        let th = garment_thickness(&m);
        assert_eq!(th.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["armpit", "elbow", "shoulder"]);
        for t in &th {
            assert!(t.got > 0.0 && !t.capped && t.pass, "{t:?}");
        }
        // Over a single part the offset is the target (the elbow is on the
        // upper arm and forearm tubes).
        assert!(th[1].pass && (th[1].got - 0.02).abs() < 0.002, "{:?}", th[1]);
    }

    #[test]
    fn a_draped_shell_hangs_as_a_hollow_skirt_and_cuts_open() {
        let vars = BTreeMap::new();
        let body = vec![Node::Sphere { r: 0.3 }.label(0)];
        let sp: Value = serde_json::json!({
            "keep": [{ "n": [0, -1, 0], "at": [0, -0.25, 0] }],
            "drape": { "below": "0", "ease": 0.02, "flare": 0.2 },
            "cuts": [[{ "n": [1, 0, 0], "at": [0.05, 0, 0], "mirror": "x" }, { "n": [0, 0, -1], "at": [0, 0, 0] }]]
        });
        let g = layer_shell(&sp, &shell_params(0.02), &body, &vars, 0.004).unwrap();
        let f = g.node.clone().prepare();
        // In the skirt wall at y = -0.2 (half-width about 0.36 to 0.37).
        assert!(f.dist([0.366, -0.2, 0.0]) < 0.0);
        // Hollow inside, open below the hem, nothing under it.
        assert!(f.dist([0.0, -0.24, 0.0]) > 0.0);
        assert!(f.dist([0.366, -0.3, 0.0]) > 0.0);
        // The cut opens the front where |x| < 0.05 and keeps the rest.
        assert!(f.dist([0.2, 0.1, 0.2]) < 0.0 && f.dist([-0.2, 0.1, 0.2]) < 0.0);
        assert!(f.dist([0.0, 0.1, 0.27]) > 0.0, "the front cut is open");
        assert!(f.dist([0.0, 0.1, -0.27]) < 0.0, "the back is closed");
    }
}
