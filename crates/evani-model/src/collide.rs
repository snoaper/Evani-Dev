//! Collision from the part graph (docs/model_research/15): one simple proxy
//! per part, straight from its generator's parameters; detail parts merge
//! into their solid's proxies; moving parts (wheels) keep their own, named
//! after their solid; the class may also ask for the game's own footprint
//! (the kart racer's capsule). Every proxy is fit-checked against its part's
//! field by sampling: share of the part outside the proxy (must be ~0) and
//! of the proxy outside the part (reported).

use crate::build::{self, Model};
use crate::generator::Proxy;
use crate::sdf::Node;
use crate::vec::*;

#[derive(Clone, Debug)]
pub struct ColProxy {
    pub name: String,
    pub solid: String,
    pub proxy: Proxy,
    /// Share of the part's sampled volume outside the proxy.
    pub missed: f64,
    /// Share of the proxy's sampled volume outside the part.
    pub excess: f64,
}

#[derive(Clone, Debug)]
pub struct Footprint {
    /// Capsule in the ground plane: centre (x, z), radius, half segment along z.
    pub centre: [f64; 2],
    pub radius: f64,
    pub half_segment: f64,
}

#[derive(Clone, Debug)]
pub struct Collision {
    pub proxies: Vec<ColProxy>,
    pub footprint: Option<Footprint>,
}

fn proxy_box(p: &Proxy) -> Aabb {
    match p {
        Proxy::Box { c, half, q } => Aabb::centred([0.0; 3], *half).transformed(*q, *c),
        Proxy::Cylinder { c, q, r, half_h } => Aabb::centred([0.0; 3], [*r, *half_h, *r]).transformed(*q, *c),
        Proxy::Capsule { a, b, r } => Aabb::new(vmin(*a, *b), vmax(*a, *b)).grow(*r),
        Proxy::Sphere { c, r } => Aabb::centred(*c, [*r; 3]),
        Proxy::Hull { pts } => {
            let mut b = Aabb::EMPTY;
            for p in pts {
                b.add_point(*p);
            }
            b
        }
    }
}

/// Inside test for every proxy kind (hulls by their convex hull's box: an
/// over-estimate, reported as excess).
pub fn inside(p: &Proxy, x: V3) -> bool {
    crate::generator::proxy_contains(p, x, 0.0)
}

fn fit(field: &Node, proxy: &Proxy) -> (f64, f64) {
    let b = field.bounds().union(&proxy_box(proxy));
    let n = 14;
    let (mut part_in, mut missed, mut prox_in, mut excess) = (0usize, 0usize, 0usize, 0usize);
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let t = [(i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64, (k as f64 + 0.5) / n as f64];
                let x = add(b.min, hadamard(b.size(), t));
                let ip = field.dist(x) < 0.0;
                let iq = inside(proxy, x);
                if ip {
                    part_in += 1;
                    if !iq {
                        missed += 1;
                    }
                }
                if iq {
                    prox_in += 1;
                    if !ip {
                        excess += 1;
                    }
                }
            }
        }
    }
    (missed as f64 / part_in.max(1) as f64, excess as f64 / prox_in.max(1) as f64)
}

pub fn build_collision(model: &Model) -> Collision {
    let mut proxies = Vec::new();
    for p in &model.parts {
        if p.role == "detail" || p.role == "cut" {
            continue;
        }
        let field = p.field().prepare();
        let world = build::proxy_world(p);
        let mut push = |name: String, pr: Proxy| {
            let (missed, excess) = fit(&field, &pr);
            proxies.push(ColProxy { name, solid: p.solid.clone(), proxy: pr, missed, excess });
        };
        if p.mirror_x {
            // Both sides: the placed (+x) proxy and its mirror.
            let mut l = p.clone();
            l.mirror_x = false;
            let mut r = l.clone();
            r.reflect = true;
            let (left, right) = (build::proxy_world(&l), build::proxy_world(&r));
            let (lf, rf) = (l.field().prepare(), r.field().prepare());
            let (ml, el) = fit(&lf, &left);
            let (mr, er) = fit(&rf, &right);
            proxies.push(ColProxy { name: format!("{}.l", p.name), solid: p.solid.clone(), proxy: left, missed: ml, excess: el });
            proxies.push(ColProxy { name: format!("{}.r", p.name), solid: p.solid.clone(), proxy: right, missed: mr, excess: er });
        } else {
            push(p.name.clone(), world);
        }
    }
    let footprint = if model.template["collision"]["footprint"].as_str() == Some("capsule") {
        let e = model.extents();
        let radius = e.size()[0] * 0.5;
        let half_segment = (e.size()[2] * 0.5 - radius).max(0.0);
        let c = e.centre();
        Some(Footprint { centre: [c[0], c[2]], radius, half_segment })
    } else {
        None
    };
    Collision { proxies, footprint }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kart_proxies_cover_their_parts_and_the_footprint_matches_the_kit() {
        let j = crate::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        let m = build::build(&j, 0.0).unwrap();
        let c = build_collision(&m);
        assert!(c.proxies.len() >= 14, "{}", c.proxies.len());
        for p in &c.proxies {
            assert!(p.missed < 0.02, "{} misses {:.3} of its part", p.name, p.missed);
        }
        // The kit's hull: 1.1 m radius capsule with a 0.45 m half segment.
        let f = c.footprint.unwrap();
        assert!((f.radius - 1.1).abs() < 0.05 && (f.half_segment - 0.45).abs() < 0.05, "{f:?}");
        assert!(c.proxies.iter().any(|p| p.name == "front_tyres.r" && p.solid == "wheel_fr"));
    }
}
