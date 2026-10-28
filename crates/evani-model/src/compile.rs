//! Per-tier compilation (docs/model_research/13, 14): every render tier is
//! re-extracted from the fields, never simplified from a finished mesh.
//!
//! Per tier: the triangle budget (from `must.tri_budget` or the template)
//! and the pixel size at the class's typical view distance set the cell
//! size and the merge tolerance. Detail parts thinner than the tier can
//! show are dropped (and listed with the reason); every other part is kept
//! at least 1.6 cells thick, so coarse tiers never break thin struts.
//! Loose solids (wheels) are meshed once in their own frame and instanced,
//! mirrored where the template pairs them. Every mesh goes through our own
//! checker; a failing tier is re-extracted at a slightly finer cell (at most
//! a few times, deterministically) before it is reported.

use crate::build::{self, Model};
use crate::intent::Diag;
use crate::mesh::{self, Mesh};
use crate::meshcheck;
use crate::vec::*;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Tier {
    pub name: String,
    pub budget: usize,
    /// Size of one pixel on the model at the class's typical distance (m).
    pub px: f64,
}

/// Screen heights per tier (render scale included) for the pixel size.
fn tier_height(name: &str) -> f64 {
    match name {
        "low" => 540.0,
        "medium" => 1080.0,
        "high" => 1440.0,
        "ultra" => 2160.0,
        "vr" => 1600.0,
        _ => 1080.0,
    }
}

/// The tiers to compile: the intent's `tri_budget`, else the template's,
/// else defaults scaled to the class.
pub fn tiers(model: &Model, only: Option<&[String]>) -> Vec<Tier> {
    let dist = model.template["view"]["distance_m"].as_f64().unwrap_or(6.0);
    let budgets = model.musts.get("tri_budget").and_then(Value::as_object).cloned().or_else(|| model.template["tri_budget"].as_object().cloned());
    let defaults = [("low", 3000u64), ("medium", 8000), ("high", 20000), ("ultra", 60000)];
    let mut out = Vec::new();
    for name in ["low", "medium", "high", "ultra", "vr"] {
        let budget = match &budgets {
            Some(b) => b.get(name).and_then(Value::as_u64),
            None => defaults.iter().find(|d| d.0 == name).map(|d| d.1),
        };
        let Some(budget) = budget else { continue };
        if let Some(o) = only
            && !o.iter().any(|x| x == name)
        {
            continue;
        }
        let tan30 = 0.577_350_269_189_625_8;
        out.push(Tier { name: name.to_string(), budget: budget as usize, px: 2.0 * dist * tan30 / tier_height(name) });
    }
    out
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub solid: String,
    pub t: V3,
    pub mirror: bool,
}

#[derive(Clone, Debug)]
pub struct SolidMesh {
    pub solid: String,
    /// In the solid's own frame (origin at the solid's origin).
    pub mesh: Mesh,
    pub instances: Vec<Instance>,
    pub report: meshcheck::Report,
}

#[derive(Clone, Debug)]
pub struct TierMesh {
    pub tier: Tier,
    pub h: f64,
    pub collapse_tol: f64,
    pub min_feature: f64,
    pub solids: Vec<SolidMesh>,
    /// Triangles drawn (instances counted).
    pub tris: usize,
    pub dropped: Vec<(String, String)>,
    pub attempts: u32,
    pub model: Model,
}

impl TierMesh {
    pub fn ok(&self) -> bool {
        self.solids.iter().all(|s| s.report.ok()) && self.tris <= self.tier.budget
    }
    /// All instances as one world mesh (for checks and previews).
    pub fn world_mesh(&self) -> Mesh {
        let mut out = Mesh::default();
        for s in &self.solids {
            for inst in &s.instances {
                let mut m = s.mesh.clone();
                for p in &mut m.pos {
                    if inst.mirror {
                        p[0] = -p[0];
                    }
                    *p = add(*p, inst.t);
                }
                if inst.mirror {
                    for n in &mut m.nrm {
                        n[0] = -n[0];
                    }
                    for t in &mut m.tris {
                        t.swap(1, 2);
                    }
                }
                out.append(&m);
            }
        }
        out
    }
}

/// Parts this tier drops: details thinner than `px_min` pixels.
fn dropped_parts(model: &Model, tier: &Tier, min_feature: f64) -> Vec<(String, String)> {
    let px_min = model_style_px(model);
    model
        .parts
        .iter()
        .filter(|p| (p.role == "detail" && (p.shape.min_dim < 0.5 * min_feature || p.shape.min_dim < px_min * tier.px)) || (p.role == "cut" && (p.shape.min_dim < min_feature || p.shape.min_dim < px_min * tier.px)))
        .map(|p| (p.name.clone(), format!("{} {:.0} mm thin: too thin for the {} tier (minimum feature {:.0} mm, {} pixels)", p.role, p.shape.min_dim * 1000.0, tier.name, min_feature * 1000.0, px_min)))
        .collect()
}

fn model_style_px(model: &Model) -> f64 {
    build::style_profile(&model.style).and_then(|s| s["rules"]["style.min_feature_px"].as_f64()).unwrap_or(2.0)
}

/// Mesh every solid of `model` at cell size `h`.
fn mesh_solids(model: &Model, drop: &[(String, String)], h: f64, tol: f64, threads: usize) -> Vec<SolidMesh> {
    let mut out = Vec::new();
    for s in &model.solids {
        if s.mirror_of.is_some() {
            continue;
        }
        let skip = |p: &build::Part| drop.iter().any(|d| d.0 == p.name);
        let Some(world) = model.solid_field(&s.name, &skip) else { continue };
        let local = world.at(QID, neg(s.origin)).prepare();
        let mut o = mesh::Options::new(h, threads);
        o.collapse_tol = tol;
        let m = mesh::extract(&local, local.bounds(), &o);
        let report = meshcheck::check(&m.pos, &m.tris, &meshcheck::Options { ref_normals: Some(m.nrm.clone()), ..Default::default() });
        let mut instances = vec![Instance { solid: s.name.clone(), t: s.origin, mirror: false }];
        for r in &model.solids {
            if r.mirror_of.as_deref() == Some(s.name.as_str()) {
                instances.push(Instance { solid: r.name.clone(), t: r.origin, mirror: true });
            }
        }
        out.push(SolidMesh { solid: s.name.clone(), mesh: m, instances, report });
    }
    out
}

fn drawn(solids: &[SolidMesh]) -> usize {
    solids.iter().map(|s| s.mesh.tris.len() * s.instances.len()).sum()
}

/// Compile one tier.
pub fn compile_tier(intent: &Value, tier: &Tier, threads: usize) -> Result<TierMesh, Vec<Diag>> {
    let exact = build::build(intent, 0.0)?;
    let diag = len(exact.extents().size());
    // First guess from a coarse pass: triangles scale with 1 / h^2.
    let tol_for = |h: f64| (tier.px * 0.15).min(h * 0.08);
    let h0 = diag / 40.0;
    let probe = mesh_solids(&exact, &[], h0, tol_for(h0), threads);
    let t0 = drawn(&probe).max(1) as f64;
    let mut h = h0 * (t0 / (tier.budget as f64 * 0.85)).sqrt();
    let mut last: Option<TierMesh> = None;
    for attempt in 1..=7u32 {
        let mf = 1.6 * h;
        let model = build::build(intent, mf)?;
        let dropped = dropped_parts(&exact, tier, mf);
        let tol = tol_for(h);
        let solids = mesh_solids(&model, &dropped, h, tol, threads);
        let tris = drawn(&solids);
        let clean = solids.iter().all(|s| s.report.ok());
        let tm = TierMesh { tier: tier.clone(), h, collapse_tol: tol, min_feature: mf, solids, tris, dropped, attempts: attempt, model };
        if tris > tier.budget {
            h *= (tris as f64 / tier.budget as f64).sqrt() * 1.03;
            last = Some(tm);
            continue;
        }
        if !clean {
            h *= 0.97;
            last = Some(tm);
            continue;
        }
        // Use the budget: one finer pass when well under it, keeping the
        // finer result only if it is clean and still within budget.
        if (tris as f64) < tier.budget as f64 * 0.8 {
            let h2 = h * (tris as f64 / (tier.budget as f64 * 0.9)).sqrt();
            let mf2 = 1.6 * h2;
            let model2 = build::build(intent, mf2)?;
            let dropped2 = dropped_parts(&exact, tier, mf2);
            let tol2 = tol_for(h2);
            let solids2 = mesh_solids(&model2, &dropped2, h2, tol2, threads);
            let tris2 = drawn(&solids2);
            if tris2 <= tier.budget && solids2.iter().all(|s| s.report.ok()) {
                return Ok(TierMesh { tier: tier.clone(), h: h2, collapse_tol: tol2, min_feature: mf2, solids: solids2, tris: tris2, dropped: dropped2, attempts: attempt + 1, model: model2 });
            }
        }
        return Ok(tm);
    }
    last.ok_or_else(|| vec![Diag::err("compile_failed", 0, format!("tier {} did not compile", tier.name), "lower the detail or raise the tier's budget")])
}

/// Compile every requested tier.
pub fn compile(intent: &Value, only: Option<&[String]>, threads: usize) -> Result<Vec<TierMesh>, Vec<Diag>> {
    let exact = build::build(intent, 0.0)?;
    let mut out = Vec::new();
    for t in tiers(&exact, only) {
        out.push(compile_tier(intent, &t, threads)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kart_compiles_every_tier_within_budget_and_clean() {
        let j = crate::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        let tiers = compile(&j, None, 4).unwrap();
        assert_eq!(tiers.len(), 4);
        let mut prev = 0;
        for t in &tiers {
            assert!(t.ok(), "{}: {} tris (budget {}), {:?}", t.tier.name, t.tris, t.tier.budget, t.solids.iter().map(|s| (s.solid.clone(), s.report.summary())).collect::<Vec<_>>());
            assert!(t.tris > prev, "tiers must grow");
            assert!(t.tris as f64 > t.tier.budget as f64 * 0.5, "{} uses only {} of {}", t.tier.name, t.tris, t.tier.budget);
            prev = t.tris;
            // Body plus two wheel meshes, each instanced twice.
            assert_eq!(t.solids.len(), 3);
            assert_eq!(t.solids.iter().map(|s| s.instances.len()).sum::<usize>(), 5);
        }
        // The low tier drops the steering details, the ultra tier keeps them.
        assert!(tiers[0].dropped.iter().any(|d| d.0 == "steering_column"), "{:?}", tiers[0].dropped);
        assert!(tiers[3].dropped.is_empty(), "{:?}", tiers[3].dropped);
    }

    /// Every part's compiled piece: its field graph and placement, printed
    /// exactly (f64 Debug output round-trips every bit).
    fn pieces(m: &Model) -> Vec<(String, String)> {
        m.parts.iter().map(|p| (p.name.clone(), format!("{:?} {:?} {:?} {:?} {:?}", p.shape.node, p.q, p.t, p.reflect, p.solid))).collect()
    }

    fn mesh_bytes(m: &Mesh) -> String {
        format!("{:?} {:?} {:?} {:?}", m.pos, m.nrm, m.tris, m.tri_part)
    }

    /// Compile-level edit locality (plan-m1.md, section A in M1): changing one
    /// part's setting leaves every other part's compiled piece byte-identical,
    /// and at the same cell size every solid without the changed parts meshes
    /// to identical bytes. (Each tier picks its cell size from the whole
    /// model's triangle count, so the tier pipeline re-meshes at a new cell
    /// size; per-solid caching and budgets come with edits in M3.)
    fn locality(edited: &str, changed: &[&str], same_solids: &[&str], changed_solid: Option<&str>) {
        let base_src = include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel");
        let base = build::build(&crate::intent::parse_text(base_src).unwrap(), 0.0).unwrap();
        let edit = build::build(&crate::intent::parse_text(edited).unwrap(), 0.0).unwrap();
        // Nothing else moved: the free measures are where they were.
        for (k, v) in &base.measures {
            if !changed.iter().any(|c| k.starts_with(&format!("{c}."))) {
                assert_eq!(v.to_bits(), edit.measures[k].to_bits(), "{k} moved: {v} -> {}", edit.measures[k]);
            }
        }
        let (a, b) = (pieces(&base), pieces(&edit));
        assert_eq!(a.len(), b.len());
        let mut differ = 0;
        for ((na, pa), (nb, pb)) in a.iter().zip(&b) {
            assert_eq!(na, nb);
            let base_name = &base.part(na).unwrap().base;
            if changed.contains(&base_name.as_str()) {
                differ += usize::from(pa != pb);
            } else {
                assert_eq!(pa, pb, "part {na} changed although only {changed:?} were edited");
            }
        }
        assert!(differ > 0, "the edit changed nothing");
        // Meshes at one cell size: untouched solids byte-identical.
        let h = 0.04;
        let ma = mesh_solids(&base, &[], h, h * 0.08, 2);
        let mb = mesh_solids(&edit, &[], h, h * 0.08, 2);
        for s in same_solids {
            let (x, y) = (ma.iter().find(|m| m.solid == *s).unwrap(), mb.iter().find(|m| m.solid == *s).unwrap());
            assert!(mesh_bytes(&x.mesh) == mesh_bytes(&y.mesh), "solid {s} re-meshed differently");
        }
        if let Some(s) = changed_solid {
            let (x, y) = (ma.iter().find(|m| m.solid == s).unwrap(), mb.iter().find(|m| m.solid == s).unwrap());
            assert!(mesh_bytes(&x.mesh) != mesh_bytes(&y.mesh), "solid {s} should change");
        }
    }

    #[test]
    fn editing_one_part_leaves_every_other_compiled_piece_byte_identical() {
        let base_src = include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel");
        // A wheel: the body and the front wheels stay byte-identical.
        let narrower = base_src.replace("rear_tyres: { size: oversized }", "rear_tyres: { size: oversized, width: 0.4 m }");
        assert_ne!(narrower, base_src);
        locality(&narrower, &["rear_tyres"], &["body", "wheel_fl"], Some("wheel_rl"));
        // A body part (the wing and its struts, which follow its span): every
        // other part and all four wheels stay byte-identical.
        let wing = base_src.replace("wing: { type: flat, struts: 2, raised }", "wing: { type: flat, struts: 2, raised, span: 1.5 m }");
        assert_ne!(wing, base_src);
        locality(&wing, &["wing", "wing_struts"], &["wheel_fl", "wheel_rl"], Some("body"));
    }
}
#[cfg(test)]
mod probe {
    use super::*;
    #[test]
    #[ignore]
    fn probe_tiers() {
        let j = crate::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        let t0 = std::time::Instant::now();
        for t in compile(&j, None, 4).unwrap() {
            eprintln!("{}: h {:.4} tol {:.4} tris {}/{} attempts {} dropped {:?} {:?}", t.tier.name, t.h, t.collapse_tol, t.tris, t.tier.budget, t.attempts, t.dropped.iter().map(|d| d.0.clone()).collect::<Vec<_>>(), t.solids.iter().map(|s| (s.solid.clone(), s.mesh.tris.len(), s.report.summary())).collect::<Vec<_>>());
        }
        eprintln!("total {:?}", t0.elapsed());
    }
}

/// Largest normal map per tier (texels on a side).
pub fn map_cap(tier: &str) -> usize {
    match tier {
        "low" => 128,
        "medium" => 256,
        "high" => 512,
        _ => 1024,
    }
}

/// Charts, tangents and exact normal maps of the smooth field for every
/// solid of a tier (one texel per pixel at the class's view distance).
pub fn surfaces(tm: &TierMesh, threads: usize) -> Vec<crate::surface::SurfaceOut> {
    tm.solids
        .iter()
        .map(|s| {
            let solid = tm.model.solids.iter().find(|x| x.name == s.solid);
            let origin = solid.map(|x| x.origin).unwrap_or([0.0; 3]);
            let skip = |p: &build::Part| tm.dropped.iter().any(|d| d.0 == p.name);
            let field = tm.model.solid_field(&s.solid, &skip).map(|f| f.at(QID, neg(origin)).prepare());
            match field {
                Some(f) => crate::surface::build(&s.mesh, &f, 1.0 / tm.tier.px, map_cap(&tm.tier.name), threads),
                None => crate::surface::build(&s.mesh, &crate::sdf::Node::Sphere { r: 0.0 }, 1.0, 16, threads),
            }
        })
        .collect()
}
#[cfg(test)]
mod probe_surface {
    use super::*;
    #[test]
    #[ignore]
    fn probe_seams() {
        let j = crate::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        for t in compile(&j, None, 4).unwrap() {
            for (s, so) in t.solids.iter().zip(surfaces(&t, 4)) {
                eprintln!("{} {}: charts {} size {} seam {:.1} raw {:.1} bake {:.1} overlaps {} samples {} unresolved {}", t.tier.name, s.solid, so.charts, so.size, so.seam_max_deg, so.seam_raw_deg, so.bake_max_deg, so.overlaps, so.seam_samples, so.seam_unresolved);
            }
        }
    }
}
