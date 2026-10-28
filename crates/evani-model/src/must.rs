//! The must registry (docs/MODEL_API.md 1.6, docs/model_research/06):
//! every hard test decided by field maths and the compiled meshes, never
//! by looking at pictures. Each result carries the measured number and the
//! limit, so a refiner gets exact discrepancies.
//!
//! Distances from sampled surfaces are made conservative: a sample spacing
//! or an angular step can only hide a closer approach of at most its own
//! size, so that margin is subtracted before comparing with the limit.
//!
//! Where each kind is judged ([`scope`]):
//! - on the exact field, once: `clear`, `swing_clear`,
//!   `wheels_clear_chassis_at_steer`, `seats`, `symmetric`, `anchor`
//!   (they measure the designed shape; a tier never changes them);
//! - on every compiled tier, each with its own result: `stands`,
//!   `min_thickness`, `tri_budget` (they are properties of a mesh);
//! - on both: `fit_box`, `min_size`, `max_size` (exact supports, then each
//!   tier's mesh box) and `connected` (joint gaps on the field, one piece per
//!   solid on each tier).
//!
//! A tier is judged with tolerances its own cell size `h` justifies, never
//! more. The mesher puts one vertex per cell on the surface (merged vertices
//! may move off it by the merge tolerance, at least 1 mm here: the "merge
//! slack"), so sampling can lose an extreme by up to half a cell but cannot
//! create one outside the surface:
//! - `stands`: the lowest point may sit up to the larger of 2 mm and h/2
//!   above the ground, but no more than 2 mm (or the merge slack) below it;
//!   the contact footprint is every vertex within one cell of the lowest;
//! - `fit_box`, `max_size`: the tier's box may exceed the limit by the merge
//!   slack only; `min_size`: it may fall short by h/2;
//! - `min_thickness`: measured on the tier's own field (parts thickened to
//!   the tier's minimum feature, dropped details left out), at each vertex
//!   moved onto that field (at most a cell diagonal away: each vertex lies in a cell the surface crosses), with no tolerance.
//!
//! What the tier itself changes is not excused: a tier that thickens a thin
//! part past a `fit_box`, or into the ground, fails and is reported per tier.

use crate::build::{self, Model};
use crate::compile::TierMesh;
use crate::generator::{self, PVal, Params};
use crate::mesh::{self, Mesh};
use crate::sdf::Node;
use crate::vec::*;
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct MustResult {
    pub kind: String,
    pub pass: bool,
    pub got: String,
    pub need: String,
    /// The tier judged, or `None` for the exact field.
    pub tier: Option<String>,
}

impl MustResult {
    fn new(kind: &str, pass: bool, got: impl Into<String>, need: impl Into<String>) -> Self {
        MustResult { kind: kind.to_string(), pass, got: got.into(), need: need.into(), tier: None }
    }
    fn on(mut self, tier: &str) -> Self {
        self.tier = Some(tier.to_string());
        self
    }
    pub fn to_json(&self) -> Value {
        match &self.tier {
            Some(t) => json!({ "kind": self.kind, "tier": t, "got": self.got, "need": self.need }),
            None => json!({ "kind": self.kind, "got": self.got, "need": self.need }),
        }
    }
}

/// Where a must kind is judged (see the module notes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Once, on the exact field.
    Field,
    /// On every compiled tier.
    Tiers,
    /// On the exact field and on every compiled tier.
    Both,
}

pub fn scope(kind: &str) -> Scope {
    match kind {
        "stands" | "min_thickness" | "tri_budget" => Scope::Tiers,
        "fit_box" | "min_size" | "max_size" | "connected" => Scope::Both,
        _ => Scope::Field,
    }
}

/// Does every result of `kind` in `rs` judged on the field or on `tier`
/// (every tier when `None`) pass?
pub fn passes(rs: &[MustResult], kind: &str, tier: Option<&str>) -> bool {
    rs.iter().filter(|r| r.kind == kind && (tier.is_none() || r.tier.is_none() || r.tier.as_deref() == tier)).all(|r| r.pass)
}

/// The kinds in `rs`, in order, once each.
pub fn kinds(rs: &[MustResult]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in rs {
        if !out.contains(&r.kind) {
            out.push(r.kind.clone());
        }
    }
    out
}

fn mm(x: f64) -> String {
    format!("{:.1} mm", x * 1000.0)
}
fn m3(x: f64) -> String {
    format!("{x:.3} m")
}

/// Samples this close to a hinge axis are where the part is held.
pub const HINGE_ZONE: f64 = 0.05;

/// Surface samples of a field: the vertices of its mesh at cell `h`, meshed
/// on `threads` threads (1 on targets without threads; the samples do not
/// depend on it).
pub fn surface_samples(f: &Node, h: f64, threads: usize) -> Vec<V3> {
    let f = f.clone().prepare();
    mesh::extract(&f, f.bounds(), &mesh::Options::new(h, threads)).pos
}

/// Smallest distance from `samples` (moved by `xf`) to `field`.
pub fn min_distance(samples: &[V3], field: &Node, xf: &dyn Fn(V3) -> V3) -> f64 {
    samples.iter().map(|p| field.dist(xf(*p))).fold(f64::INFINITY, f64::min)
}

/// Smallest clearance while `samples` rotate about `pivot` round `axis`
/// through every angle from `from` to `to` degrees (1 degree steps), minus
/// the distance a sample can travel between two steps.
pub fn swept_clearance(samples: &[V3], field: &Node, pivot: V3, axis: V3, from: f64, to: f64) -> (f64, f64) {
    let reach = samples.iter().map(|p| dist(*p, pivot)).fold(0.0, f64::max);
    let steps = ((to - from).abs().ceil() as usize).max(1);
    let travel = reach * crate::dmath::sin(crate::dmath::deg((to - from).abs() / steps as f64 * 0.5)) * 2.0;
    let mut worst = (f64::INFINITY, from);
    for i in 0..=steps {
        let a = from + (to - from) * i as f64 / steps as f64;
        let q = quat_axis(axis, crate::dmath::deg(a));
        let d = min_distance(samples, field, &|p| add(qrot(q, sub(p, pivot)), pivot));
        if d < worst.0 {
            worst = (d, a);
        }
    }
    (worst.0 - travel * 0.5, worst.1)
}

/// Thickness inward from each sample along `-normal`, by sphere tracing
/// inside the solid until it exits. A sample may sit up to `slack` outside
/// the surface (5 mm at least) and is stepped in first.
pub fn thickness(field: &Node, p: V3, n: V3, slack: f64) -> f64 {
    let slack = slack.max(0.005);
    let mut t0 = 0.0;
    while field.dist(sub(p, mul(n, t0))) >= -1e-6 {
        t0 += 2e-4;
        if t0 > slack {
            return 0.0;
        }
    }
    let mut t = t0;
    for _ in 0..200 {
        let d = field.dist(sub(p, mul(n, t)));
        if d >= -1e-6 {
            return t - t0 + 1e-4;
        }
        t += (-d).max(1e-4);
        if t > 100.0 {
            break;
        }
    }
    t
}

/// Move `p` onto the surface of `field` by Newton steps (fields that are
/// not exact distances converge slowly, hence the step count; at most `reach`
/// away) and return the point and its outward unit normal.
fn onto_surface(field: &Node, p: V3, reach: f64) -> Option<(V3, V3)> {
    let mut x = p;
    for _ in 0..48 {
        let s = field.eval(x);
        let g2 = dot(s.g, s.g);
        if g2 <= 1e-24 {
            return None;
        }
        if s.d.abs() < 1e-6 {
            break;
        }
        x = sub(x, mul(s.g, s.d / g2));
    }
    let s = field.eval(x);
    if s.d.abs() > 1e-4 || dist(x, p) > reach {
        return None;
    }
    Some((x, norm_or(s.g, [0.0, 1.0, 0.0])))
}

/// Free width at `p` along x: march both ways until `field` is hit.
pub fn free_width(field: &Node, p: V3, max: f64) -> f64 {
    let mut total = 0.0;
    for s in [1.0, -1.0] {
        let mut t = 0.0;
        while t < max {
            let d = field.dist(add(p, [s * t, 0.0, 0.0]));
            if d < 1e-5 {
                break;
            }
            t += d;
        }
        total += t.min(max);
    }
    total
}

/// Number of connected pieces of a mesh.
pub fn components(m: &Mesh) -> usize {
    crate::meshcheck::check(&m.pos, &m.tris, &crate::meshcheck::Options { self_intersections: false, expect_closed: false, ..Default::default() }).components
}

/// Is the point inside the convex hull of `pts` (2D, x and z)?
fn inside_hull_xz(pts: &[[f64; 2]], c: [f64; 2]) -> bool {
    let mut p: Vec<[f64; 2]> = pts.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return false;
    }
    let cr = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut hull: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while hull.len() >= start + 2 && cr(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0 {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    (0..hull.len()).all(|i| cr(hull[i], hull[(i + 1) % hull.len()], c) >= 0.0)
}

/// The seated body for the `seats` must, built from its data file.
pub fn body_field(r: &Value, at: V3) -> Option<Node> {
    let mut kids = Vec::new();
    for p in r["parts"].as_array()? {
        let mut params = Params::new();
        let size: Vec<f64> = p["size"].as_array()?.iter().filter_map(Value::as_f64).collect();
        params.insert("size".into(), PVal::V(size));
        if let Some(b) = p["bevel"].as_f64() {
            params.insert("bevel".into(), PVal::N(b));
        }
        let g = generator::generate(p["gen"].as_str()?, &params, 0.0, 0).ok()?;
        let pos: Vec<f64> = p["at"].as_array()?.iter().filter_map(Value::as_f64).collect();
        let rot: Vec<f64> = p["rot"].as_array()?.iter().filter_map(Value::as_f64).collect();
        let q = quat_ypr(crate::dmath::deg(rot[0]), crate::dmath::deg(rot[1]), crate::dmath::deg(rot[2]));
        kids.push(g.node.at(q, add(at, [pos[0], pos[1], pos[2]])));
    }
    Some(Node::Union(kids))
}

/// Everything the musts look at.
pub struct Ctx<'a> {
    pub model: &'a Model,
    /// Compiled tiers, each judged on its own; may be empty for model-only
    /// checks (the tier kinds then fail with "no mesh").
    pub tiers: &'a [TierMesh],
    /// Sampling cell for part surfaces (metres).
    pub h: f64,
    /// Threads for surface sampling (results do not depend on it).
    pub threads: usize,
}

/// The vertex-merge tolerance of a tier: merged vertices may sit this far
/// off the surface, so a tier's box may grow by it (1 mm at least).
fn merge_slack(t: &TierMesh) -> f64 {
    t.collapse_tol.max(0.001)
}

/// The world field a tier was meshed from: the model built at the tier's
/// minimum feature, without the details the tier dropped.
fn tier_field(t: &TierMesh) -> Node {
    let skip = |p: &build::Part| t.dropped.iter().any(|d| d.0 == p.name);
    Node::Union(t.model.solids.iter().filter_map(|s| t.model.solid_field(&s.name, &skip)).collect()).prepare()
}

impl Ctx<'_> {
    fn group_field(&self, name: &str) -> Option<Node> {
        let own: Vec<usize> = (0..self.model.parts.len()).filter(|&i| self.model.parts[i].base == name).collect();
        let idx = if own.is_empty() { self.model.parts_of(name) } else { own };
        if idx.is_empty() {
            return None;
        }
        Some(Node::Union(idx.iter().map(|&i| self.model.parts[i].field()).collect()).prepare())
    }
    fn rest_field(&self, without: &dyn Fn(&build::Part) -> bool) -> Node {
        let kids: Vec<Node> = self.model.solids.iter().filter_map(|s| self.model.solid_field(&s.name, without)).collect();
        Node::Union(kids).prepare()
    }
}

/// Run every must of the model: one result per kind for the exact field
/// and one per compiled tier, as [`scope`] says.
pub fn run_all(ctx: &Ctx) -> Vec<MustResult> {
    let musts = ctx.model.musts.clone();
    let mut out = Vec::new();
    for (k, a) in &musts {
        out.extend(run(ctx, k, a));
    }
    out
}

/// Judge one must: on the field, on every tier, or both ([`scope`]).
pub fn run(ctx: &Ctx, kind: &str, arg: &Value) -> Vec<MustResult> {
    let on_tiers = || ctx.tiers.iter().map(|t| judge(ctx, kind, arg, Some(t)).on(&t.tier.name));
    match scope(kind) {
        Scope::Field => vec![judge(ctx, kind, arg, None)],
        Scope::Both => std::iter::once(judge(ctx, kind, arg, None)).chain(on_tiers()).collect(),
        Scope::Tiers => {
            if ctx.tiers.is_empty() || (kind == "stands" && arg.as_str() == Some("none")) {
                vec![judge(ctx, kind, arg, None)]
            } else {
                on_tiers().collect()
            }
        }
    }
}

/// One must on the exact field (`tier` `None`) or on one compiled tier.
fn judge(ctx: &Ctx, kind: &str, arg: &Value, tier: Option<&TierMesh>) -> MustResult {
    let model = ctx.model;
    // The box judged by the size kinds and how far it may grow and shrink:
    // exact supports (1 mm), or the tier's mesh. Mesh vertices sit on the
    // surface, so the box grows only by the merge slack; a rounded extreme
    // can fall between two vertices, so it may shrink by half a cell.
    let size_box = || match tier {
        None => (model.extents().size(), 0.001, 0.001),
        Some(t) => (t.world_mesh().bounds().size(), merge_slack(t), merge_slack(t).max(0.5 * t.h)),
    };
    match kind {
        "fit_box" => {
            let want: Vec<f64> = arg.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            if want.len() != 3 {
                return MustResult::new(kind, false, "bad argument", "[length, width, height] m");
            }
            let (s, slack, _) = size_box();
            let got = [s[2], s[0], s[1]];
            let pass = (0..3).all(|i| got[i] <= want[i] + slack);
            MustResult::new(kind, pass, format!("[{:.3}, {:.3}, {:.3}] m", got[0], got[1], got[2]), format!("<= [{}, {}, {}] m (+{})", want[0], want[1], want[2], mm(slack)))
        }
        "min_size" | "max_size" => {
            let (s, grow, shrink) = size_box();
            let slack = if kind == "min_size" { shrink } else { grow };
            let mut pass = true;
            let mut got = Vec::new();
            let mut need = Vec::new();
            for (name, v) in arg.as_object().into_iter().flatten() {
                let have = match name.as_str() {
                    "length" => s[2],
                    "width" => s[0],
                    _ => s[1],
                };
                let lim = v.as_f64().unwrap_or(0.0);
                let ok = if kind == "min_size" { have >= lim - slack } else { have <= lim + slack };
                pass &= ok;
                got.push(format!("{name} {have:.3} m"));
                need.push(format!("{name} {} {lim} m", if kind == "min_size" { ">=" } else { "<=" }));
            }
            MustResult::new(kind, pass, got.join(", "), format!("{} ({}{})", need.join(", "), if kind == "min_size" { "-" } else { "+" }, mm(slack)))
        }
        "clear" => {
            let parts: Vec<&str> = arg["parts"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let at_least = arg["at_least"].as_f64().unwrap_or(0.0);
            let (Some(fa), Some(fb)) = (parts.first().and_then(|p| ctx.group_field(p)), parts.get(1).and_then(|p| ctx.group_field(p))) else {
                return MustResult::new(kind, false, format!("missing part in {parts:?}"), "two parts of the model");
            };
            let d = min_distance(&surface_samples(&fa, ctx.h, ctx.threads), &fb, &|p| p) - ctx.h * 0.5;
            MustResult::new(kind, d >= at_least, mm(d), format!(">= {}", mm(at_least)))
        }
        "swing_clear" => {
            let part = arg["part"].as_str().unwrap_or("");
            let about = arg["about"].as_str().unwrap_or("");
            let angle = arg["angle"].as_f64().unwrap_or(0.0);
            let at_least = arg["at_least"].as_f64().unwrap_or(0.0);
            let (pn, an) = about.split_once('.').unwrap_or((about, "centre"));
            // Everything in the swinging part's own solid moves with it (a
            // lid with its bands, a door leaf with its handle and straps).
            // The solids' own fields, so their cuts (a lid's flat underside) count.
            let mut moving: Vec<String> = model.parts.iter().filter(|p| p.base == part).map(|p| p.solid.clone()).collect();
            moving.sort();
            moving.dedup();
            let kids: Vec<Node> = moving.iter().filter_map(|s| model.solid_field(s, &|_| false)).collect();
            let fp = if kids.is_empty() { None } else { Some(Node::Union(kids).prepare()) };
            let (Some(fp), Some(anchor)) = (fp, model.parts.iter().find(|p| p.base == pn).and_then(|p| p.anchor(an))) else {
                return MustResult::new(kind, false, format!("missing {part} or {about}"), "a part and its hinge anchor");
            };
            // A part fused into the same solid as its hinge cannot swing.
            if let Some(h) = model.parts.iter().find(|p| p.base == pn)
                && moving.contains(&h.solid)
            {
                return MustResult::new(kind, false, format!("{part} is fused to {pn} (solid {})", h.solid), "the swinging part in its own solid");
            }
            let rest = ctx.rest_field(&|p| moving.contains(&p.solid));
            // Away from the hinge itself (where the part is held).
            let axis = norm_or(anchor.n, [0.0, 1.0, 0.0]);
            let samples: Vec<V3> = surface_samples(&fp, ctx.h, ctx.threads)
                .into_iter()
                .filter(|p| {
                    let v = sub(*p, anchor.p);
                    len(sub(v, mul(axis, dot(v, axis)))) > HINGE_ZONE
                })
                .collect();
            let (d, at) = swept_clearance(&samples, &rest, anchor.p, axis, 0.0, angle);
            let d = d - ctx.h * 0.5;
            MustResult::new(kind, d >= at_least, format!("{} at {at:.0} deg", mm(d)), format!(">= {}", mm(at_least)))
        }
        "wheels_clear_chassis_at_steer" => {
            let angle = arg.as_f64().or_else(|| arg["angle"].as_f64()).unwrap_or(30.0);
            let at_least = arg["at_least"].as_f64().unwrap_or(0.01);
            let steer = &model.template["steer"];
            let axis: Vec<f64> = steer["axis"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or(vec![0.0, 1.0, 0.0]);
            let axis = [axis[0], axis[1], axis[2]];
            let against = steer["against"].as_str().unwrap_or("body");
            let Some(body) = model.solid_field(against, &|_| false).map(Node::prepare) else {
                return MustResult::new(kind, false, "no body", "a body solid");
            };
            let mut worst = (f64::INFINITY, 0.0, String::new());
            for base in steer["parts"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                for p in model.parts.iter().filter(|p| p.base == base) {
                    let f = p.field();
                    let pivot = p.anchor("hub").map(|a| a.p).unwrap_or(p.t);
                    let (d, at) = swept_clearance(&surface_samples(&f, ctx.h, ctx.threads), &body, pivot, axis, -angle, angle);
                    let d = d - ctx.h * 0.5;
                    if d < worst.0 {
                        worst = (d, at, p.name.clone());
                    }
                }
            }
            MustResult::new(kind, worst.0 >= at_least, format!("{} ({} at {:.0} deg)", mm(worst.0), worst.2, worst.1), format!(">= {} up to {angle} deg", mm(at_least)))
        }
        "seats" => {
            let r = arg.as_str().unwrap_or("");
            let Some(body) = build::body_ref(r) else {
                return MustResult::new(kind, false, format!("unknown body `{r}`"), "bench/kart_driver");
            };
            let seat = &model.template["seat"];
            let Some(anchor) = model.anchors.iter().find(|a| Some(a.name.as_str()) == seat["anchor"].as_str()) else {
                return MustResult::new(kind, false, "no seat anchor", "a driver_seat anchor");
            };
            let list = |k: &str| -> Vec<String> { seat[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
            let (seat_parts, contact) = (list("seat_parts"), list("contact_parts"));
            let Some(bf) = body_field(&body, anchor.p) else {
                return MustResult::new(kind, false, "bad body data", "a body file");
            };
            let samples = surface_samples(&bf, ctx.h, ctx.threads);
            let others = ctx.rest_field(&|p| seat_parts.contains(&p.base) || contact.contains(&p.base));
            let seat_f = ctx.rest_field(&|p| !seat_parts.contains(&p.base));
            let pen_other = min_distance(&samples, &others, &|p| p);
            let pen_seat = min_distance(&samples, &seat_f, &|p| p);
            let hip = body["hip_width"].as_f64().unwrap_or(0.0);
            let clearance = body["clearance"].as_f64().unwrap_or(0.0);
            let width = free_width(&ctx.rest_field(&|_| false), add(anchor.p, [0.0, 0.15, 0.0]), 2.0);
            let need_w = hip + 2.0 * clearance;
            let pass = width >= need_w && pen_other >= -0.005 && pen_seat >= -0.03;
            MustResult::new(
                kind,
                pass,
                format!("seat width {:.3} m, body clear of the kart by {}, sunk into the seat {}", width.min(2.0), mm(pen_other), mm((-pen_seat).max(0.0))),
                format!("width >= {need_w:.3} m, clear >= -5.0 mm, sunk <= 30.0 mm"),
            )
        }
        "stands" => {
            if arg.as_str() == Some("none") {
                return MustResult::new(kind, true, "floating", "none");
            }
            let Some(t) = tier else { return MustResult::new(kind, false, "no mesh", "a compiled tier") };
            let m = t.world_mesh();
            // One vertex per cell: the lowest vertex may sit up to half a
            // cell above the true lowest point, and the contact is every
            // vertex within one cell of it. Vertices sit on the surface, so
            // sampling never puts one below it: a sink past 2 mm (or the
            // merge slack) is the tier's shape, and fails.
            let lift = (0.5 * t.h).max(0.002);
            let sink = merge_slack(t).max(0.002);
            let reach = t.h.max(0.005);
            let lo = m.pos.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
            let contact: Vec<[f64; 2]> = m.pos.iter().filter(|p| p[1] < lo + reach).map(|p| [p[0], p[2]]).collect();
            // Volume centroid of the closed mesh.
            let (mut vol, mut c) = (0.0, [0.0; 3]);
            for t in &m.tris {
                let (a, b, d) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                let v = dot(a, cross(b, d)) / 6.0;
                vol += v;
                c = add(c, mul(add(add(a, b), d), v / 4.0));
            }
            let c = mul(c, 1.0 / vol.max(1e-12));
            let stable = inside_hull_xz(&contact, [c[0], c[2]]);
            let pass = lo <= lift && lo >= -sink && stable;
            MustResult::new(
                kind,
                pass,
                format!("lowest point {}, centre of mass {} the footprint", mm(lo), if stable { "inside" } else { "outside" }),
                format!("lowest point from -{} to +{} (half a {} cell), centre of mass over the contact within one cell", mm(sink), mm(lift), mm(t.h)),
            )
        }
        "connected" => {
            if arg.as_bool() == Some(false) {
                return MustResult::new(kind, true, "not required", "false");
            }
            let mut bad = Vec::new();
            match tier {
                // On a tier: one piece per solid.
                Some(t) => {
                    for s in &t.solids {
                        let n = components(&s.mesh);
                        if n != 1 {
                            bad.push(format!("{} has {n} pieces", s.solid));
                        }
                    }
                }
                // On the field: every joint closed.
                None => {
                    for (a, b, g) in &joint_gaps(ctx) {
                        bad.push(format!("gap {} between {a} and {b}", mm(*g)));
                    }
                }
            }
            let pass = bad.is_empty();
            let (ok, need) = if tier.is_some() { ("one piece per solid", "one piece per solid") } else { ("every joint closed", "joints touching") };
            MustResult::new(kind, pass, if pass { ok.to_string() } else { bad.join("; ") }, need)
        }
        "symmetric" => {
            let ax = match arg.as_str() {
                Some("y") => 1,
                Some("z") => 2,
                _ => 0,
            };
            let f = model.field().prepare();
            let b = f.bounds();
            let mut worst: f64 = 0.0;
            let n = 12;
            for k in 0..n {
                for j in 0..n {
                    for i in 0..n {
                        let t = [(i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64, (k as f64 + 0.5) / n as f64];
                        let p = add(b.min, hadamard(b.size(), t));
                        let mut q = p;
                        q[ax] = -q[ax];
                        let (a, c) = (f.dist(p), f.dist(q));
                        if a.abs() < 0.05 || c.abs() < 0.05 {
                            worst = worst.max((a - c).abs());
                        }
                    }
                }
            }
            MustResult::new(kind, worst <= 0.0005, format!("largest mirror difference {}", mm(worst)), "<= 0.5 mm")
        }
        "min_thickness" => {
            let lim = arg.as_f64().unwrap_or(0.0);
            let Some(t) = tier else { return MustResult::new(kind, false, "no mesh", "a compiled tier") };
            // The tier's own shape: the field it was meshed from, measured at
            // each mesh vertex moved onto it. A vertex is clamped to its cell,
            // so it may sit off the surface by up to the cell diagonal; further
            // off counts as 0 thickness.
            let f = tier_field(t);
            let m = t.world_mesh();
            let stride = (m.pos.len() / 4000).max(1);
            let mut worst = (f64::INFINITY, [0.0; 3]);
            for i in (0..m.pos.len()).step_by(stride) {
                let th = match onto_surface(&f, m.pos[i], 3f64.sqrt() * t.h) {
                    Some((p, n)) => thickness(&f, p, n, 0.0),
                    None => 0.0,
                };
                if th < worst.0 {
                    worst = (th, m.pos[i]);
                }
            }
            MustResult::new(kind, worst.0 >= lim, format!("{} at [{:.2}, {:.2}, {:.2}]", mm(worst.0), worst.1[0], worst.1[1], worst.1[2]), format!(">= {}", mm(lim)))
        }
        "tri_budget" => {
            let Some(t) = tier else { return MustResult::new(kind, false, "no mesh", "a compiled tier") };
            MustResult::new(kind, t.tris <= t.tier.budget, format!("{} tris", t.tris), format!("<= {}", t.tier.budget))
        }
        "anchor" => {
            let name = arg["name"].as_str().unwrap_or("");
            let at = arg["at"].as_str();
            let anchor = match at {
                Some(a) => match model.part(a) {
                    // A whole part by its full name (`hands.l`, `slats.0`): its centre.
                    Some(p) => p.anchor("centre").map(|x| (x.p, x.n)),
                    None => {
                        let (pn, an) = a.rsplit_once('.').unwrap_or((a, "centre"));
                        model.part(pn).or_else(|| model.parts.iter().find(|p| p.base == pn)).and_then(|p| p.anchor(an)).map(|x| (x.p, x.n))
                    }
                },
                None => model.anchors.iter().find(|x| x.name == name).map(|x| (x.p, x.n)),
            };
            let Some((p, n)) = anchor else {
                return MustResult::new(kind, false, format!("no anchor `{name}`"), "an anchor the template or `at` defines");
            };
            match arg["diameter"].as_f64() {
                None => MustResult::new(kind, true, m3(p[1]), format!("anchor `{name}` exported")),
                Some(want) => {
                    let f = model.field().prepare();
                    let d = diameter_at(&f, p, n);
                    let pass = (d - want).abs() <= want * 0.15;
                    MustResult::new(kind, pass, mm(d), format!("{} +- 15%", mm(want)))
                }
            }
        }
        other => MustResult::new(other, false, "unknown must kind", "a kind from docs/MODEL_API.md 1.6"),
    }
}

/// Mean width across `axis` at `p` (rays in 8 directions perpendicular).
pub fn diameter_at(f: &Node, p: V3, axis: V3) -> f64 {
    let a = norm_or(axis, [0.0, 1.0, 0.0]);
    let u = norm_or(cross(a, if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }), [0.0, 0.0, 1.0]);
    let v = cross(a, u);
    let mut sum = 0.0;
    for k in 0..4 {
        let ang = core::f64::consts::PI * k as f64 / 4.0;
        let (s, c) = crate::dmath::sin_cos(ang);
        let d = add(mul(u, c), mul(v, s));
        let mut w = 0.0;
        for dir in [d, neg(d)] {
            let mut t = 0.0;
            for _ in 0..200 {
                let x = f.dist(add(p, mul(dir, t)));
                if x >= -1e-6 {
                    break;
                }
                t += (-x).max(1e-5);
            }
            w += t;
        }
        sum += w;
    }
    sum / 4.0
}

/// Every `attached_at` or `rests_on` joint inside one solid whose parts do
/// not touch: (part, target, gap).
pub fn joint_gaps(ctx: &Ctx) -> Vec<(String, String, f64)> {
    let model = ctx.model;
    let mut out = Vec::new();
    for spec in model.template["parts"].as_array().into_iter().flatten() {
        let base = spec["name"].as_str().unwrap_or("");
        for rel in spec["relations"].as_array().into_iter().flatten() {
            let to = rel["to"].as_str().unwrap_or("").split('.').next().unwrap_or("");
            for p in model.parts.iter().filter(|p| p.base == base) {
                let Some(t) = model.parts.iter().find(|q| q.base == to && q.solid == p.solid) else { continue };
                let samples = surface_samples(&p.field(), ctx.h.min(p.shape.min_dim * 0.25).max(0.002), ctx.threads);
                let g = min_distance(&samples, &t.field().prepare(), &|x| x);
                if g > 0.0005 {
                    out.push((p.name.clone(), t.name.clone(), g));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::parse_text;

    fn ctx_for(src: &str, tiers: bool) -> (Model, Vec<TierMesh>) {
        let j = parse_text(src).unwrap();
        let m = build::build(&j, 0.0).unwrap();
        let t = if tiers { crate::compile::compile(&j, Some(&["low".to_string(), "ultra".to_string()]), 4).unwrap() } else { vec![] };
        (m, t)
    }

    /// The first failing result of a must (or its first result).
    fn one(rs: Vec<MustResult>) -> MustResult {
        rs.iter().find(|r| !r.pass).or(rs.first()).cloned().unwrap()
    }

    fn ctx<'a>(m: &'a Model, t: &'a [TierMesh]) -> Ctx<'a> {
        Ctx { model: m, tiers: t, h: 0.015, threads: 2 }
    }

    fn check(src: &str, kind: &str, arg: Value, tiers: bool) -> MustResult {
        let (m, t) = ctx_for(src, tiers);
        one(run(&ctx(&m, &t), kind, &arg))
    }

    /// Each tier is judged on its own mesh with its own tolerance, so asking
    /// for fewer tiers never changes a tier's result, and each kind reports
    /// where it was judged.
    #[test]
    fn musts_are_judged_per_tier_and_a_subset_measures_the_same() {
        let (m, both) = ctx_for("make: kart\n", true);
        let low_only = vec![both[0].clone()];
        for (kind, arg) in [("stands", json!("ground")), ("min_thickness", json!(0.03)), ("fit_box", json!([3.1, 2.21, 1.27])), ("connected", json!(true)), ("symmetric", json!("x"))] {
            let all = run(&ctx(&m, &both), kind, &arg);
            let low = run(&ctx(&m, &low_only), kind, &arg);
            let scope = scope(kind);
            let field_n = usize::from(scope != Scope::Tiers);
            let tier_n = if scope == Scope::Field { 0 } else { 1 };
            assert_eq!(all.len(), field_n + 2 * tier_n, "{kind}: {all:?}");
            assert_eq!(low.len(), field_n + tier_n, "{kind}: {low:?}");
            for r in &low {
                assert!(all.contains(r), "{kind}: {r:?} differs when judged alone");
                assert!(r.pass, "{r:?}");
            }
            assert_eq!(all.iter().filter(|r| r.tier.as_deref() == Some("ultra")).count(), tier_n);
        }
        // The kart's low tier stands within half its cell, not a fixed 2 mm.
        let s = &run(&ctx(&m, &low_only), "stands", &json!("ground"))[0];
        assert!(s.need.contains(&mm((0.5 * both[0].h).max(0.002))), "{s:?}");
    }

    #[test]
    fn size_musts_pass_and_fail() {
        let k = "make: kart\n";
        assert!(check(k, "fit_box", json!([3.1, 2.21, 1.27]), false).pass);
        assert!(!check(k, "fit_box", json!([2.0, 2.21, 1.27]), false).pass);
        assert!(check(k, "min_size", json!({ "length": 3.0 }), false).pass);
        assert!(!check(k, "min_size", json!({ "length": 3.5 }), false).pass);
        assert!(check(k, "max_size", json!({ "height": 1.3 }), false).pass);
        assert!(!check(k, "max_size", json!({ "height": 1.0 }), false).pass);
    }

    #[test]
    fn clear_and_swing_pass_and_fail() {
        let k = "make: kart\n";
        assert!(check(k, "clear", json!({ "parts": ["wing", "engine_cover"], "at_least": 0.05 }), false).pass);
        assert!(!check(k, "clear", json!({ "parts": ["wing", "engine_cover"], "at_least": 0.5 }), false).pass);
        // The bumper is fused to the nose, so it cannot swing.
        assert!(!check(k, "swing_clear", json!({ "part": "bumper", "about": "bumper.top", "angle": 30, "at_least": 0.005 }), false).pass);
        // A door leaf on a hinge swings 100 degrees clear of its wall, and
        // hits a stop block placed in its arc.
        let leaf = Node::RoundBox { half: [0.45, 1.0, 0.02], r: 0.0 }.at(QID, [0.5, 1.0, 0.03]);
        let samples = surface_samples(&leaf, 0.02, 2);
        let wall = Node::RoundBox { half: [1.0, 1.0, 0.05], r: 0.0 }.at(QID, [-1.02, 1.0, 0.0]).prepare();
        let (d, _) = swept_clearance(&samples, &wall, [0.0, 0.0, 0.06], [0.0, 1.0, 0.0], 0.0, -100.0);
        assert!(d > 0.0, "door swing clearance {d}");
        let stop = Node::Union(vec![wall.clone(), Node::RoundBox { half: [0.05; 3], r: 0.0 }.at(QID, [0.4, 1.0, 0.6])]).prepare();
        let (d, _) = swept_clearance(&samples, &stop, [0.0, 0.0, 0.06], [0.0, 1.0, 0.0], 0.0, -100.0);
        assert!(d < 0.0, "the stop is hit: {d}");
    }

    /// A wheel next to a box: touching starts just past 29 degrees.
    #[test]
    fn steer_clearance_finds_the_angle_of_contact() {
        let chassis = Node::RoundBox { half: [0.5, 0.3, 1.0], r: 0.0 }.prepare();
        let wheel = Node::Cylinder { r: 0.3, half_h: 0.1, round: 0.0 }.at(quat_axis([0.0, 0.0, 1.0], core::f64::consts::FRAC_PI_2), [0.0; 3]);
        // Rotated by a, the wheel's inner front corner sits at
        // x = hub - (0.1 cos a + 0.3 sin a); contact with x = 0.5 at a = 29 deg.
        let a = crate::dmath::deg(29.0);
        let hub_x = 0.5 + 0.1 * crate::dmath::cos(a) + 0.3 * crate::dmath::sin(a);
        let samples: Vec<V3> = surface_samples(&wheel, 0.004, 2).into_iter().map(|p| add(p, [hub_x, 0.0, 0.0])).collect();
        let (d28, _) = swept_clearance(&samples, &chassis, [hub_x, 0.0, 0.0], [0.0, 1.0, 0.0], -28.0, 28.0);
        let (d30, at) = swept_clearance(&samples, &chassis, [hub_x, 0.0, 0.0], [0.0, 1.0, 0.0], -30.0, 30.0);
        assert!(d28 > 0.0, "clear at 28 deg: {d28}");
        assert!(d30 < 0.0, "touching at 30 deg: {d30} at {at}");
        // The kart template clears its front wheels at 30 degrees by 1 cm.
        assert!(check("make: kart\n", "wheels_clear_chassis_at_steer", json!(30), false).pass);
        assert!(!check("make: kart\n", "wheels_clear_chassis_at_steer", json!({ "angle": 30, "at_least": 0.2 }), false).pass);
    }

    #[test]
    fn a_one_millimetre_joint_gap_is_found() {
        let a = Node::RoundBox { half: [0.2; 3], r: 0.02 };
        let b_touch = Node::RoundBox { half: [0.2; 3], r: 0.02 }.at(QID, [0.399, 0.0, 0.0]).prepare();
        let b_gap = Node::RoundBox { half: [0.2; 3], r: 0.02 }.at(QID, [0.401, 0.0, 0.0]).prepare();
        let s = surface_samples(&a, 0.01, 2);
        assert!(min_distance(&s, &b_touch, &|p| p) <= 0.0005);
        let g = min_distance(&s, &b_gap, &|p| p);
        assert!(g > 0.0005 && (g - 0.001).abs() < 1e-4, "gap {g}");
    }

    #[test]
    fn seats_pass_and_a_seat_2cm_too_narrow_fails() {
        assert!(check("make: kart\n", "seats", json!("bench/kart_driver"), false).pass, "{:?}", check("make: kart\n", "seats", json!("bench/kart_driver"), false));
        let ok = check("make: kart\nparts:\n  seat: { type: bucket }\n", "seats", json!("bench/kart_driver"), false);
        assert!(ok.pass, "{ok:?}");
        // Bolsters 2 cm closer on each side than the body plus clearance needs.
        let narrow = check("make: kart\nparts:\n  seat: { type: bucket, width: 0.78 }\n", "seats", json!("bench/kart_driver"), false);
        assert!(!narrow.pass, "{narrow:?}");
    }

    #[test]
    fn stands_connected_symmetric_thickness_budget_anchor() {
        let k = "make: kart\n";
        let (m, t) = ctx_for(k, true);
        let c = ctx(&m, &t);
        for (kind, arg) in [("stands", json!("ground")), ("connected", json!(true)), ("symmetric", json!("x")), ("min_thickness", json!(0.03)), ("tri_budget", json!({ "low": 3000, "ultra": 60000 })), ("anchor", json!({ "name": "driver_seat" }))] {
            let r = one(run(&c, kind, &arg));
            assert!(r.pass, "{r:?}");
        }
        assert!(!one(run(&c, "min_thickness", &json!(0.1))).pass);
        assert!(!one(run(&c, "anchor", &json!({ "name": "nope" }))).pass);
        assert!(!one(run(&c, "symmetric", &json!("z"))).pass);
        // A floating kart does not stand; an over-budget tier fails.
        let mut lifted = t.clone();
        for tm in &mut lifted {
            for s in &mut tm.solids {
                for i in &mut s.instances {
                    i.t[1] += 0.1;
                }
            }
        }
        assert!(!one(run(&ctx(&m, &lifted), "stands", &json!("ground"))).pass);
        let mut over = t.clone();
        over[0].tier.budget = 100;
        assert!(!one(run(&ctx(&m, &over), "tri_budget", &json!({ "low": 100 }))).pass);
        // Two separate pieces are not connected.
        let mut split = t.clone();
        let extra = split[0].solids[0].mesh.clone();
        let mut moved = extra.clone();
        for p in &mut moved.pos {
            p[1] += 5.0;
        }
        split[0].solids[0].mesh.append(&moved);
        assert!(!one(run(&ctx(&m, &split), "connected", &json!(true))).pass);
    }

    #[test]
    fn grip_diameter_anchor() {
        let f = Node::Cylinder { r: 0.015, half_h: 0.1, round: 0.0 }.prepare();
        assert!((diameter_at(&f, [0.0; 3], [0.0, 1.0, 0.0]) - 0.03).abs() < 1e-4);
    }
}
