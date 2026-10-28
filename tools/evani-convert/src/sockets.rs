//! Sockets: `extras.evani.sockets.<name> = { "p": [x, y, z], "q": [x, y, z, w] }`
//! (model frame, docs/MODEL_FORMAT.md "Sockets") become `anchor:<name>`
//! nodes that carry the rotation as well as the position. The engine already
//! reads an anchor's whole world matrix (`Gltf::anchors`) and
//! `Models::anchor_frame` hands kits its rotation, so a socket costs no
//! runtime code: the work is done here, at convert time, and a file without
//! sockets goes through untouched.
//!
//! - No `anchor:<name>` node: one is added at the scene root.
//! - An `anchor:<name>` node already there: it must stand at `p` (within
//!   0.1 mm); its rotation is set so its world rotation is `q`, and a
//!   rotation it already has must agree. Parents may move, turn and scale
//!   it evenly; a parent with a `matrix` or an uneven scale is refused.

use serde_json::{Value, json};

use crate::glb::Doc;

type V3 = [f64; 3];
type Q = [f64; 4];

/// The file lists sockets.
pub fn has(j: &Value) -> bool {
    j.pointer("/extras/evani/sockets").and_then(Value::as_object).is_some_and(|o| !o.is_empty())
}

fn nums<const N: usize>(v: Option<&Value>) -> Option<[f64; N]> {
    let a = v?.as_array().filter(|a| a.len() == N)?;
    let mut out = [0.0; N];
    for (k, x) in a.iter().enumerate() {
        out[k] = x.as_f64().filter(|x| x.is_finite())?;
    }
    Some(out)
}

fn qmul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn qconj(q: Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}

fn qrot(q: Q, v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v).map(|x| x * 2.0);
    let c = cross(u, t);
    [v[0] + t[0] * q[3] + c[0], v[1] + t[1] * q[3] + c[1], v[2] + t[2] * q[3] + c[2]]
}

fn qnorm(q: Q) -> Option<Q> {
    let l = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    (l.is_finite() && l > 1e-9).then(|| q.map(|x| x / l))
}

fn r6(x: f64) -> f64 {
    let v = (x * 1e6).round() / 1e6;
    if v == 0.0 { 0.0 } else { v }
}

/// A node's move, turn and even scale; `Err` for a matrix or uneven scale.
fn trs(n: &Value, what: &str) -> Result<(V3, Q, f64), String> {
    if n.get("matrix").is_some() {
        return Err(format!("{what} uses a matrix\nfix: export it with translation, rotation and scale (Blender's glTF exporter does by default)"));
    }
    let t = nums::<3>(n.get("translation")).unwrap_or([0.0; 3]);
    let q = nums::<4>(n.get("rotation")).and_then(qnorm).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let s = nums::<3>(n.get("scale")).unwrap_or([1.0; 3]);
    if (s[0] - s[1]).abs() > 1e-6 * s[0].abs().max(1.0) || (s[0] - s[2]).abs() > 1e-6 * s[0].abs().max(1.0) || s[0].abs() < 1e-9 {
        return Err(format!("{what} has an uneven or zero scale {s:?}, so a socket under it cannot keep its rotation\nfix: apply the scale in the modelling tool before export"));
    }
    Ok((t, q, s[0]))
}

/// World move, turn and scale of node `i` (its parents' and its own).
fn world(nodes: &[Value], parent: &[Option<usize>], i: usize) -> Result<(V3, Q, f64), String> {
    let mut chain = vec![i];
    while let Some(p) = chain.last().and_then(|&c| parent[c]) {
        if chain.contains(&p) || chain.len() > nodes.len() {
            return Err("the node tree has a loop".into());
        }
        chain.push(p);
    }
    let (mut t, mut q, mut s) = ([0.0; 3], [0.0, 0.0, 0.0, 1.0], 1.0);
    for &k in chain.iter().rev() {
        let name = nodes[k].get("name").and_then(Value::as_str).unwrap_or("?");
        let (ct, cq, cs) = trs(&nodes[k], &format!("node {k} ({name})"))?;
        let m = qrot(q, ct.map(|x| x * s));
        t = [t[0] + m[0], t[1] + m[1], t[2] + m[2]];
        q = qmul(q, cq);
        s *= cs;
    }
    Ok((t, q, s))
}

/// Turn every socket into an anchor node with its rotation. Returns notes
/// for the report; an inconsistent socket is an error with a `fix:` line.
pub fn apply(doc: &mut Doc) -> Result<Vec<String>, String> {
    let Some(list) = doc.json.pointer("/extras/evani/sockets").and_then(Value::as_object).cloned() else { return Ok(Vec::new()) };
    let mut notes = Vec::new();
    for (name, s) in &list {
        let p = nums::<3>(s.get("p")).ok_or(format!("socket {name}: p must be [x, y, z]\nfix: write extras.evani.sockets.{name}.p as three numbers in the model frame"))?;
        let q = nums::<4>(s.get("q")).and_then(qnorm).ok_or(format!("socket {name}: q must be a rotation [x, y, z, w]\nfix: write extras.evani.sockets.{name}.q as a unit quaternion"))?;
        let nodes = doc.json.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut parent = vec![None; nodes.len()];
        for (k, n) in nodes.iter().enumerate() {
            for c in n.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64) {
                if let Some(slot) = parent.get_mut(c as usize) {
                    *slot = Some(k);
                }
            }
        }
        let node_name = format!("anchor:{name}");
        match nodes.iter().position(|n| n.get("name").and_then(Value::as_str) == Some(node_name.as_str())) {
            None => {
                let k = nodes.len();
                let node = json!({ "name": node_name, "translation": p.map(r6), "rotation": q.map(r6), "extras": { "evani": { "kind": "anchor", "socket": true } } });
                let j = &mut doc.json;
                if !j.get("nodes").is_some_and(Value::is_array) {
                    j["nodes"] = json!([]);
                }
                if let Some(a) = j["nodes"].as_array_mut() {
                    a.push(node);
                }
                let scene = j.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
                let roots = j.pointer_mut(&format!("/scenes/{scene}/nodes")).and_then(Value::as_array_mut).ok_or(format!("socket {name}: the file has no scene to put its anchor in\nfix: export the model with a scene"))?;
                roots.push(json!(k));
                notes.push(format!("socket {name}: added anchor:{name} at {p:?} with its rotation"));
            }
            Some(i) => {
                let (pt, pq, ps) = match parent[i] {
                    Some(par) => world(&nodes, &parent, par)?,
                    None => ([0.0; 3], [0.0, 0.0, 0.0, 1.0], 1.0),
                };
                let (own_t, own_q, _) = trs(&nodes[i], &node_name)?;
                // Where the node stands now, in the model frame.
                let at = qrot(pq, own_t.map(|x| x * ps));
                let at = [pt[0] + at[0], pt[1] + at[1], pt[2] + at[2]];
                let off = ((at[0] - p[0]).powi(2) + (at[1] - p[1]).powi(2) + (at[2] - p[2]).powi(2)).sqrt();
                if off > 1e-4 {
                    return Err(format!(
                        "socket {name}: p {p:?} is {:.1} mm from where anchor:{name} stands ({at:?})\nfix: write the same point in both, or drop one of them",
                        off * 1000.0
                    ));
                }
                let local = qmul(qconj(pq), q);
                let had = nodes[i].get("rotation").is_some();
                if had && qmul(qconj(own_q), local)[3].abs() < 0.999_999 {
                    return Err(format!("socket {name}: anchor:{name} is already turned differently from q\nfix: write the same rotation in both, or drop the node's rotation"));
                }
                if let Some(n) = doc.json.pointer_mut(&format!("/nodes/{i}")) {
                    n["rotation"] = json!(local.map(r6));
                }
                notes.push(format!("socket {name}: anchor:{name} {} its rotation", if had { "already had" } else { "now carries" }));
            }
        }
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(nodes: Value, sockets: Value) -> Doc {
        let n = nodes.as_array().map_or(0, Vec::len);
        Doc { json: json!({ "asset": { "version": "2.0" }, "scene": 0, "scenes": [{ "nodes": (0..n.min(1)).collect::<Vec<_>>() }], "nodes": nodes, "extras": { "evani": { "format": 1, "sockets": sockets } } }), bin: Vec::new() }
    }

    const QY90: Q = [0.0, std::f64::consts::FRAC_1_SQRT_2, 0.0, std::f64::consts::FRAC_1_SQRT_2];

    #[test]
    fn a_socket_without_a_node_gets_one_at_the_root() {
        let mut d = doc(json!([{ "name": "body" }]), json!({ "right_hand": { "p": [0.3, 1.0, 0.1], "q": QY90 } }));
        let notes = apply(&mut d).unwrap();
        assert_eq!(notes.len(), 1);
        let n = &d.json["nodes"][1];
        assert_eq!(n["name"], "anchor:right_hand");
        assert_eq!(n["translation"], json!([0.3, 1.0, 0.1]));
        assert_eq!(n["rotation"], json!(QY90.map(r6)));
        assert_eq!(d.json["scenes"][0]["nodes"], json!([0, 1]));
    }

    #[test]
    fn an_existing_anchor_under_a_turned_parent_gets_the_local_rotation() {
        // Parent turned a quarter about y and moved; the anchor at local (1, 0, 0)
        // stands at world (0, 0, -1) + (0, 2, 0).
        let nodes = json!([{ "name": "root", "translation": [0.0, 2.0, 0.0], "rotation": QY90, "children": [1] }, { "name": "anchor:grip", "translation": [1.0, 0.0, 0.0] }]);
        let mut d = doc(nodes, json!({ "grip": { "p": [0.0, 2.0, -1.0], "q": QY90 } }));
        apply(&mut d).unwrap();
        // World rotation = parent (QY90) * local, so local is the identity.
        let local = nums::<4>(d.json["nodes"][1].get("rotation")).unwrap();
        assert!((local[3].abs() - 1.0).abs() < 1e-6, "{local:?}");
    }

    #[test]
    fn disagreements_and_bad_sockets_are_errors() {
        let far = doc(json!([{ "name": "anchor:grip", "translation": [1.0, 0.0, 0.0] }]), json!({ "grip": { "p": [0.0, 0.0, 0.0], "q": [0, 0, 0, 1] } }));
        let turned = doc(json!([{ "name": "anchor:grip", "rotation": QY90 }]), json!({ "grip": { "p": [0.0, 0.0, 0.0], "q": [0, 0, 0, 1] } }));
        let no_q = doc(json!([{ "name": "a" }]), json!({ "grip": { "p": [0.0, 0.0, 0.0] } }));
        let zero_q = doc(json!([{ "name": "a" }]), json!({ "grip": { "p": [0.0, 0.0, 0.0], "q": [0, 0, 0, 0] } }));
        let squashed = doc(json!([{ "name": "r", "scale": [1.0, 2.0, 1.0], "children": [1] }, { "name": "anchor:grip" }]), json!({ "grip": { "p": [0.0, 0.0, 0.0], "q": [0, 0, 0, 1] } }));
        for (what, mut d) in [("far", far), ("turned", turned), ("no q", no_q), ("zero q", zero_q), ("squashed", squashed)] {
            let e = apply(&mut d).expect_err(what);
            assert!(e.contains("fix:"), "{what}: {e}");
        }
    }

    /// End to end: Evani.Model writes a socket (the template's `q`), this
    /// pass agrees with it, and the engine's reader hands the anchor's
    /// rotation on. The same model without `q` writes no rotation and no
    /// `sockets` entry, as before.
    #[test]
    fn evani_model_socket_reaches_the_engine_reader() {
        let j = evani_model::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        let tiers = evani_model::compile::compile(&j, Some(&["low".to_string()]), 2).unwrap();
        let tm = &tiers[0];
        let col = evani_model::collide::build_collision(&tm.model);
        let plain = evani_model::gltf::write_tier(tm, &col, None, &serde_json::Map::new());
        let pd = Doc::read_glb(&plain).unwrap();
        assert!(!has(&pd.json), "no socket without q");
        assert!(pd.json["nodes"].as_array().unwrap().iter().filter(|n| n["name"].as_str().is_some_and(|s| s.starts_with("anchor:"))).all(|n| n.get("rotation").is_none()));

        let mut tm2 = tm.clone();
        let seat = tm2.model.anchors.iter_mut().find(|a| a.name == "driver_seat").unwrap();
        seat.q = Some(QY90);
        let p = seat.p;
        let with = evani_model::gltf::write_tier(&tm2, &col, None, &serde_json::Map::new());
        let mut d = Doc::read_glb(&with).unwrap();
        assert_eq!(d.json["extras"]["evani"]["sockets"]["driver_seat"]["q"], json!(QY90.map(r6)));
        let notes = apply(&mut d).unwrap();
        assert_eq!(notes, ["socket driver_seat: anchor:driver_seat already had its rotation"]);
        let g = evani_gltf::Gltf::from_glb(&d.to_glb().unwrap()).unwrap();
        let w = g.anchors.iter().find(|a| a.name == "driver_seat").unwrap().world;
        // A quarter turn about y: the anchor's x axis points along -z.
        assert!((w[0]).abs() < 1e-5 && (w[2] + 1.0).abs() < 1e-5, "{w:?}");
        for k in 0..3 {
            assert!((f64::from(w[12 + k]) - p[k]).abs() < 1e-5);
        }
    }

    #[test]
    fn a_file_without_sockets_is_untouched() {
        let mut d = Doc { json: json!({ "nodes": [{ "name": "anchor:seat", "translation": [1.0, 0.0, 0.0] }], "extras": { "evani": { "format": 1 } } }), bin: Vec::new() };
        let before = d.json.clone();
        assert!(!has(&d.json));
        assert!(apply(&mut d).unwrap().is_empty());
        assert_eq!(d.json, before);
    }
}
