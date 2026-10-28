//! Skeleton and skin weights for characters (docs/model_research/
//! plan-characters.md sections 2 and 5).
//!
//! A class template's `skeleton` lists joints in a T-pose (world rest
//! positions, parents first). Parts name their bone (`bone`, with `{side}`
//! resolved per vertex: x >= 0 is the character's left, `l`). A vertex
//! starts fully on its part's bone and blends towards the bone's parent
//! near the bone's head and towards the nearest child near its tail, with
//! smoothsteps sized by the joint radius. At most 3 influences, summing to
//! one in f32. Deterministic: fixed orders, no hashing.

use crate::build::Model;
use crate::sdf::Node;
use crate::vec::*;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    /// World rest position (T-pose).
    pub at: V3,
    /// Blend radius at this joint (the limb's half thickness).
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
}

/// Influences of one vertex: joint indices and weights (unused slots are
/// joint 0 with weight 0).
pub type Influence = ([u8; 4], [f32; 4]);

impl Skeleton {
    pub fn index(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j.name == name)
    }
    pub fn children(&self, i: usize) -> Vec<usize> {
        (0..self.joints.len()).filter(|&c| self.joints[c].parent == Some(i)).collect()
    }
    /// The bone segment of joint `i`: head at the joint, tail at its first
    /// child; a leaf runs two radii on along its parent's direction.
    pub fn segment(&self, i: usize) -> (V3, V3) {
        let j = &self.joints[i];
        if let Some(&c) = self.children(i).first() {
            return (j.at, self.joints[c].at);
        }
        let dir = match j.parent {
            Some(p) => norm_or(sub(j.at, self.joints[p].at), [0.0, 1.0, 0.0]),
            None => [0.0, 1.0, 0.0],
        };
        (j.at, add(j.at, mul(dir, 2.0 * j.radius.max(1e-3))))
    }
    fn dir(&self, i: usize) -> V3 {
        let (a, b) = self.segment(i);
        norm_or(sub(b, a), [0.0, 1.0, 0.0])
    }
}

/// Parse a template's `skeleton`. `at` evaluates a formula vector. Specs
/// with `mirror: "x"` and `{side}` in the name give `_l` at +x and `_r`
/// at -x (parent `{side}` resolved alike).
pub fn parse(spec: &Value, at: &dyn Fn(Option<&Value>) -> Result<V3, String>, num: &dyn Fn(&Value) -> Result<f64, String>) -> Result<Skeleton, String> {
    let mut sk = Skeleton::default();
    for js in spec.as_array().into_iter().flatten() {
        let name = js["name"].as_str().ok_or("skeleton: a joint without a name")?;
        let p = at(js.get("at"))?;
        let radius = match js.get("radius") {
            Some(r) => num(r)?,
            None => 0.05,
        };
        let sides: Vec<(&str, f64)> = if js["mirror"].as_str() == Some("x") { vec![("l", 1.0), ("r", -1.0)] } else { vec![("", 1.0)] };
        for (side, sx) in sides {
            let nm = name.replace("{side}", side);
            let parent = match js["parent"].as_str() {
                None => None,
                Some(pn) => {
                    let pn = pn.replace("{side}", side);
                    Some(sk.index(&pn).ok_or(format!("skeleton: `{nm}` names parent `{pn}` before it is defined"))?)
                }
            };
            if sk.index(&nm).is_some() {
                return Err(format!("skeleton: joint `{nm}` twice"));
            }
            sk.joints.push(Joint { name: nm, parent, at: [p[0] * sx, p[1], p[2]], radius });
        }
    }
    if sk.joints.len() > 255 {
        return Err("skeleton: more than 255 joints".into());
    }
    Ok(sk)
}

/// Resolve `{side}` by the sign of x (x >= 0 is the character's left).
pub fn side_name(bone: &str, x: f64) -> String {
    bone.replace("{side}", if x >= 0.0 { "l" } else { "r" })
}

fn seg_dist(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 0.0 { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    dist(p, add(a, mul(ab, t)))
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The nearest of a part's bone candidates for a vertex.
fn pick_bone(sk: &Skeleton, cands: &[String], v: V3) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for c in cands {
        let Some(i) = sk.index(&side_name(c, v[0])) else { continue };
        let (a, b) = sk.segment(i);
        let d = seg_dist(v, a, b);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|b| b.1)
}

/// Check the parts' bone names against the skeleton (each candidate for
/// both sides when it has `{side}`).
pub fn check_bones(model: &Model, sk: &Skeleton) -> Result<(), String> {
    for p in &model.parts {
        for b in &p.bone {
            for x in [1.0, -1.0] {
                let n = side_name(b, x);
                if sk.index(&n).is_none() {
                    return Err(format!("{}: bone `{n}` is not in the skeleton", p.name));
                }
            }
        }
    }
    Ok(())
}

/// Skin weights for a world-space mesh (`tri_part` gives each triangle's
/// part). The parts touching a vertex (through its triangles) and the part
/// whose own field is nearest to it each offer a bone; the vertex takes
/// the one whose segment is nearest (ties: the lower part index). On
/// coarse tiers a hand vertex can sit only on triangles labelled forearm
/// or sleeve, and still goes to the hand.
pub fn weights(model: &Model, sk: &Skeleton, pos: &[V3], tris: &[[u32; 3]], tri_part: &[u32]) -> Vec<Influence> {
    let mut vparts: Vec<Vec<usize>> = vec![Vec::new(); pos.len()];
    for (ti, t) in tris.iter().enumerate() {
        let Some(p) = tri_part.get(ti).map(|&p| p as usize) else { continue };
        for &v in t {
            if let Some(slot) = vparts.get_mut(v as usize)
                && !slot.contains(&p)
            {
                slot.push(p);
            }
        }
    }
    // The parts present in this mesh (dropped detail parts are not), by index.
    let mut present: Vec<usize> = tri_part.iter().map(|&p| p as usize).filter(|&p| p < model.parts.len()).collect();
    present.sort_unstable();
    present.dedup();
    let own: Vec<Node> = present.iter().map(|&i| model.parts[i].field().prepare()).collect();
    for (s, &v) in vparts.iter_mut().zip(pos) {
        let mut near: Option<(f64, usize)> = None;
        for (f, &i) in own.iter().zip(&present) {
            let d = f.dist(v);
            if near.is_none_or(|(bd, _)| d < bd) {
                near = Some((d, i));
            }
        }
        if let Some((_, i)) = near
            && !s.contains(&i)
        {
            s.push(i);
        }
        s.sort_unstable();
    }
    // Covered parts' fields, for garments without a bone of their own.
    let covered: Vec<bool> = (0..model.parts.len()).map(|i| model.parts.iter().any(|g| g.over.contains(&i))).collect();
    let fields: Vec<Option<Node>> = model.parts.iter().enumerate().map(|(i, p)| if covered[i] && !p.bone.is_empty() { Some(p.field().prepare()) } else { None }).collect();
    let all: Vec<String> = sk.joints.iter().skip(1).map(|j| j.name.clone()).collect();
    pos.iter()
        .enumerate()
        .map(|(vi, &v)| {
            let part_bone = |part: &crate::build::Part| {
                if !part.bone.is_empty() {
                    return pick_bone(sk, &part.bone, v);
                }
                // A garment follows the covered part nearest to the vertex.
                let mut best: Option<(f64, usize)> = None;
                for &c in &part.over {
                    if let Some(Some(f)) = fields.get(c) {
                        let d = f.dist(v);
                        if best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, c));
                        }
                    }
                }
                best.and_then(|(_, c)| pick_bone(sk, &model.parts[c].bone, v))
            };
            let mut bone: Option<(f64, usize)> = None;
            for part in vparts[vi].iter().filter_map(|&pi| model.parts.get(pi)) {
                if let Some(b) = part_bone(part) {
                    let (a, e) = sk.segment(b);
                    let d = seg_dist(v, a, e);
                    if bone.is_none_or(|(bd, _)| d < bd) {
                        bone = Some((d, b));
                    }
                }
            }
            let b = bone.map(|b| b.1).or_else(|| pick_bone(sk, &all, v)).unwrap_or(0);
            influence(sk, b, v)
        })
        .collect()
}

/// Weights of one vertex on bone `b` with its parent and nearest child.
pub fn influence(sk: &Skeleton, b: usize, v: V3) -> Influence {
    let jb = &sk.joints[b];
    let mut raw: Vec<(usize, f64)> = Vec::with_capacity(3);
    // Towards the parent, near the bone's head (never onto the root).
    let mut a_p = 0.0;
    if let Some(p) = jb.parent
        && p != 0
    {
        let rs = jb.radius;
        let fade = 1.0 - smoothstep(0.5 * rs, rs, dist(v, jb.at));
        a_p = (1.0 - smoothstep(-0.5 * rs, 0.5 * rs, dot(sub(v, jb.at), sk.dir(b)))) * fade;
        raw.push((p, a_p));
    }
    // Towards the nearest child, near the bone's tail.
    let mut a_c = 0.0;
    let kids = sk.children(b);
    if let Some(&c) = kids.iter().min_by(|&&x, &&y| dist(v, sk.joints[x].at).total_cmp(&dist(v, sk.joints[y].at)).then(x.cmp(&y))) {
        let jc = &sk.joints[c];
        let rs = jc.radius;
        let fade = 1.0 - smoothstep(0.5 * rs, rs, dist(v, jc.at));
        a_c = smoothstep(-0.5 * rs, 0.5 * rs, dot(sub(v, jc.at), sk.dir(c))) * fade;
        raw.push((c, a_c));
    }
    raw.push((b, (1.0 - a_p - a_c).max(0.0)));
    raw.retain(|r| r.1 > 1e-4);
    if raw.is_empty() {
        raw.push((b, 1.0));
    }
    raw.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    let total: f64 = raw.iter().map(|r| r.1).sum();
    let mut j = [0u8; 4];
    let mut w = [0f32; 4];
    let mut acc = 0f32;
    for (k, r) in raw.iter().enumerate() {
        j[k] = r.0 as u8;
        if k + 1 == raw.len() {
            w[k] = (1.0 - acc).max(0.0);
        } else {
            w[k] = (r.1 / total) as f32;
            acc += w[k];
        }
    }
    (j, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm() -> Skeleton {
        let spec = serde_json::json!([
            { "name": "root", "at": [0, 0, 0] },
            { "name": "chest", "parent": "root", "at": [0, 1.3, 0], "radius": 0.12 },
            { "name": "upperarm_{side}", "parent": "chest", "at": [0.18, 1.45, 0], "radius": 0.06, "mirror": "x" },
            { "name": "forearm_{side}", "parent": "upperarm_{side}", "at": [0.47, 1.45, 0], "radius": 0.05, "mirror": "x" },
            { "name": "hand_{side}", "parent": "forearm_{side}", "at": [0.72, 1.45, 0], "radius": 0.04, "mirror": "x" }
        ]);
        let at = |v: Option<&Value>| -> Result<V3, String> {
            let a = v.and_then(Value::as_array).ok_or("at")?;
            Ok([a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)])
        };
        parse(&spec, &at, &|v| v.as_f64().ok_or("n".to_string())).unwrap()
    }

    #[test]
    fn mirrored_joints_come_in_pairs_with_parents_first() {
        let sk = arm();
        let names: Vec<&str> = sk.joints.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(names, ["root", "chest", "upperarm_l", "upperarm_r", "forearm_l", "forearm_r", "hand_l", "hand_r"]);
        assert_eq!(sk.joints[5].parent, Some(3));
        assert_eq!(sk.joints[3].at, [-0.18, 1.45, 0.0]);
        for (i, j) in sk.joints.iter().enumerate() {
            assert!(j.parent.is_none_or(|p| p < i));
        }
        // A leaf runs two radii along its parent's direction.
        let (a, b) = sk.segment(6);
        assert!((b[0] - a[0] - 0.08).abs() < 1e-12);
    }

    #[test]
    fn weights_blend_at_joints_and_stay_whole_mid_bone() {
        let sk = arm();
        let fa = sk.index("forearm_l").unwrap();
        let ua = sk.index("upperarm_l").unwrap();
        // Mid-forearm: all on the forearm.
        let (j, w) = influence(&sk, fa, [0.6, 1.45, 0.0]);
        assert_eq!((j[0] as usize, w[0]), (fa, 1.0));
        // At the elbow plane: half and half between upper arm and forearm.
        let (j, w) = influence(&sk, fa, [0.47, 1.45, 0.0]);
        assert!(j.contains(&(ua as u8)) && (w[0] - 0.5).abs() < 1e-6 && (w[1] - 0.5).abs() < 1e-6, "{j:?} {w:?}");
        // The upper arm's vertex at the same place agrees.
        let (j2, w2) = influence(&sk, ua, [0.47, 1.45, 0.0]);
        assert!(j2.contains(&(fa as u8)) && (w2[0] - 0.5).abs() < 1e-6);
        // Sums are one, at most 3 influences, unused slots zero.
        for x in 0..40 {
            let v = [0.15 + x as f64 * 0.016, 1.45 + (x % 5) as f64 * 0.01, 0.02];
            for b in [ua, fa, sk.index("hand_l").unwrap()] {
                let (_, w) = influence(&sk, b, v);
                assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6 && w[3] == 0.0);
            }
        }
        // Nothing blends onto the root: the chest (child of the root) stays whole at its head.
        let ch = sk.index("chest").unwrap();
        assert_eq!(influence(&sk, ch, [0.0, 1.25, 0.0]), ([ch as u8, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
    }
}
