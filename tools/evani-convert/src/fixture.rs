//! Test fixtures written by code (all content original).
//!
//! [`rig_glb`] is `tests/fixtures/models/rig.src.glb`, the skinned, animated
//! model the clip baker (S3-7) is tested on: a six-joint rig (hips, spine,
//! neck, head, arm_l, hand_l) with a skinned tube and three clips:
//! - `idle` (2 s): a hips bob (LINEAR), a spine sway (LINEAR), a neck tilt
//!   that never changes and is not the rest pose (a constant track), head
//!   nods (STEP), and a hand scale equal to the rest pose (dropped).
//! - `wave` (1 s): the arm raised and lowered on a CUBICSPLINE curve, the
//!   hand waving with keys at 60 Hz (resampled to 30).
//! - `pose`: one key per channel (every track a constant).
//!
//! `cargo run -p evani-convert --example rig_source -- tests/fixtures/models/rig.src.glb`
//! writes it; a test checks the committed file is these bytes.

use serde_json::{Value, json};

use crate::glb::Doc;

/// Unit quaternion for `deg` degrees about a unit axis.
fn axis_angle(axis: [f32; 3], deg: f32) -> [f32; 4] {
    let h = deg.to_radians() * 0.5;
    let s = h.sin();
    [axis[0] * s, axis[1] * s, axis[2] * s, h.cos()]
}

fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [p[0], p[1], p[2]]
}

/// Accessors and views over one growing binary buffer.
struct Bin {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut v = json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = Value::from(t);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }

    /// f32 data of `ty` (`SCALAR`, `VEC3`, `VEC4`, `MAT4`), with min and max when asked.
    fn f32s(&mut self, data: &[f32], ty: &str, comps: usize, bounds: bool, target: Option<u32>) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let mut a = json!({ "bufferView": view, "componentType": 5126, "count": data.len() / comps, "type": ty });
        if bounds {
            let lo: Vec<f32> = (0..comps).map(|c| data.iter().skip(c).step_by(comps).fold(f32::MAX, |m, &x| m.min(x))).collect();
            let hi: Vec<f32> = (0..comps).map(|c| data.iter().skip(c).step_by(comps).fold(f32::MIN, |m, &x| m.max(x))).collect();
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn ints(&mut self, data: &[u16], component: u32, ty: &str, comps: usize, target: Option<u32>) -> usize {
        let bytes: Vec<u8> = if component == 5121 { data.iter().map(|&x| x as u8).collect() } else { data.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let view = self.view(&bytes, target);
        self.accessors.push(json!({ "bufferView": view, "componentType": component, "count": data.len() / comps, "type": ty }));
        self.accessors.len() - 1
    }

    /// One animation sampler: times (with min and max, as glTF requires) and values.
    fn sampler(&mut self, times: &[f32], values: &[f32], ty: &str, comps: usize, interpolation: &str) -> Value {
        let input = self.f32s(times, "SCALAR", 1, true, None);
        let output = self.f32s(values, ty, comps, false, None);
        json!({ "input": input, "output": output, "interpolation": interpolation })
    }
}

/// A two-node clip for baker stress tests (not committed as a file): `root`
/// slides `slide_m` along x over `seconds` (LINEAR, two keys) and its child
/// `spinner`, 0.5 m up, turns slowly about y (a key every second, up to 30
/// degrees, one slow sine over the clip).
pub fn slide_glb(seconds: f32, slide_m: f32) -> Result<Vec<u8>, String> {
    let mut b = Bin { bin: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let slide = b.sampler(&[0.0, seconds], &[0.0, 0.0, 0.0, slide_m, 0.0, 0.0], "VEC3", 3, "LINEAR");
    let keys = seconds.ceil().max(1.0) as usize;
    let times: Vec<f32> = (0..=keys).map(|k| (k as f32).min(seconds)).collect();
    let turn: Vec<f32> = times.iter().flat_map(|&t| axis_angle([0.0, 1.0, 0.0], 30.0 * (std::f32::consts::TAU * t / seconds.max(1e-3)).sin())).collect();
    let spin = b.sampler(&times, &turn, "VEC4", 4, "LINEAR");
    let json = json!({
        "asset": { "version": "2.0", "generator": "Evani.Dev slide fixture (S3-7)" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "root", "children": [1] }, { "name": "spinner", "translation": [0.0, 0.5, 0.0] }],
        "animations": [{ "name": "slide", "samplers": [slide, spin], "channels": [
            { "sampler": 0, "target": { "node": 0, "path": "translation" } },
            { "sampler": 1, "target": { "node": 1, "path": "rotation" } },
        ] }],
        "buffers": [{ "byteLength": 0 }],
        "bufferViews": b.views,
        "accessors": b.accessors,
    });
    Doc { json, bin: b.bin }.to_glb()
}

/// One joint: (name, parent joint, rest translation, rest rotation).
type Joint = (&'static str, Option<usize>, [f32; 3], [f32; 4]);

/// The joints.
fn joints() -> [Joint; 6] {
    let id = [0.0, 0.0, 0.0, 1.0];
    [
        ("hips", None, [0.0, 1.0, 0.0], id),
        ("spine", Some(0), [0.0, 0.25, 0.0], id),
        ("neck", Some(1), [0.0, 0.25, 0.0], id),
        ("head", Some(2), [0.0, 0.1, 0.0], id),
        ("arm_l", Some(1), [0.15, 0.2, 0.0], axis_angle([0.0, 0.0, 1.0], -70.0)),
        ("hand_l", Some(4), [0.3, 0.0, 0.0], id),
    ]
}

/// `rig.src.glb`'s bytes.
pub fn rig_glb() -> Result<Vec<u8>, String> {
    rig_doc().to_glb()
}

/// `rig.static.glb`'s bytes: the rig's tube as a plain static mesh (the
/// same nodes, buffer and material; no skin, no JOINTS_0 or WEIGHTS_0, no
/// clips). GPU skinning (S3-9) is checked against it: the skinned rig in
/// bind pose must draw like this file.
pub fn rig_static_glb() -> Result<Vec<u8>, String> {
    let mut d = rig_doc();
    let j = &mut d.json;
    if let Some(o) = j.as_object_mut() {
        o.remove("skins");
        o.remove("animations");
        o["asset"]["generator"] = Value::from("Evani.Dev rig fixture, static copy (S3-9)");
    }
    if let Some(n) = j["nodes"].as_array_mut().and_then(|n| n.last_mut()).and_then(Value::as_object_mut) {
        n.remove("skin");
    }
    if let Some(a) = j["meshes"][0]["primitives"][0]["attributes"].as_object_mut() {
        a.remove("JOINTS_0");
        a.remove("WEIGHTS_0");
    }
    d.to_glb()
}

fn rig_doc() -> Doc {
    let mut b = Bin { bin: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let js = joints();

    // Rest world transforms (rigid) and the inverse bind matrices.
    let mut world: Vec<([f32; 3], [f32; 4])> = Vec::new();
    for (_, parent, t, r) in js {
        world.push(match parent {
            None => (t, r),
            Some(p) => {
                let (pt, pr) = world[p];
                let o = rotate(pr, t);
                ([pt[0] + o[0], pt[1] + o[1], pt[2] + o[2]], qmul(pr, r))
            }
        });
    }
    let mut ibm = Vec::new();
    for (t, r) in &world {
        // Inverse of T * R: R^-1 then -R^-1 t.
        let inv = [-r[0], -r[1], -r[2], r[3]];
        let cols = [rotate(inv, [1.0, 0.0, 0.0]), rotate(inv, [0.0, 1.0, 0.0]), rotate(inv, [0.0, 0.0, 1.0])];
        let it = rotate(inv, *t);
        for c in cols {
            ibm.extend_from_slice(&[c[0], c[1], c[2], 0.0]);
        }
        ibm.extend_from_slice(&[-it[0], -it[1], -it[2], 1.0]);
    }

    // A skinned tube up the body: 4 rings of 6, hips to neck.
    let (sides, rings) = (6usize, [(0.95f32, [(0u16, 1.0f32), (1, 0.0)]), (1.15, [(0, 0.5), (1, 0.5)]), (1.35, [(1, 1.0), (2, 0.0)]), (1.55, [(2, 1.0), (1, 0.0)])]);
    let (mut pos, mut nrm, mut jnt, mut wgt, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (y, w) in rings {
        for k in 0..sides {
            let a = (k as f32) / (sides as f32) * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            pos.extend_from_slice(&[0.08 * c, y, 0.08 * s]);
            nrm.extend_from_slice(&[c, 0.0, s]);
            jnt.extend_from_slice(&[w[0].0, w[1].0, 0, 0]);
            wgt.extend_from_slice(&[w[0].1, w[1].1, 0.0, 0.0]);
        }
    }
    for r in 0..rings.len() - 1 {
        for k in 0..sides {
            let (a, b2) = ((r * sides + k) as u16, (r * sides + (k + 1) % sides) as u16);
            let (c, d) = (a + sides as u16, b2 + sides as u16);
            idx.extend_from_slice(&[a, c, b2, b2, c, d]);
        }
    }
    let a_pos = b.f32s(&pos, "VEC3", 3, true, Some(34962));
    let a_nrm = b.f32s(&nrm, "VEC3", 3, false, Some(34962));
    let a_jnt = b.ints(&jnt, 5121, "VEC4", 4, Some(34962));
    let a_wgt = b.f32s(&wgt, "VEC4", 4, false, Some(34962));
    let a_idx = b.ints(&idx, 5123, "SCALAR", 1, Some(34963));
    let a_ibm = b.f32s(&ibm, "MAT4", 16, false, None);

    // Nodes: 0 rig, 1..=6 the joints, 7 the body.
    let mut nodes = vec![json!({ "name": "rig", "children": [1, 7] })];
    for (k, (name, _, t, r)) in js.iter().enumerate() {
        let children: Vec<usize> = js.iter().enumerate().filter(|(_, j)| j.1 == Some(k)).map(|(c, _)| c + 1).collect();
        let mut n = json!({ "name": name, "translation": t });
        if *r != [0.0, 0.0, 0.0, 1.0] {
            n["rotation"] = json!(r);
        }
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        nodes.push(n);
    }
    nodes.push(json!({ "name": "body", "mesh": 0, "skin": 0 }));
    let x = [1.0, 0.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let node = |name: &str| js.iter().position(|j| j.0 == name).map_or(0, |k| k + 1);

    // idle (2 s).
    let mut idle_s = Vec::new();
    let mut idle_c = Vec::new();
    let chan = |s: &mut Vec<Value>, c: &mut Vec<Value>, sampler: Value, n: usize, path: &str| {
        s.push(sampler);
        c.push(json!({ "sampler": s.len() - 1, "target": { "node": n, "path": path } }));
    };
    let bob: Vec<f32> = [1.0f32, 1.03, 1.0, 1.03, 1.0].iter().flat_map(|&y| [0.0, y, 0.0]).collect();
    chan(&mut idle_s, &mut idle_c, b.sampler(&[0.0, 0.5, 1.0, 1.5, 2.0], &bob, "VEC3", 3, "LINEAR"), node("hips"), "translation");
    let sway: Vec<f32> = [-4.0f32, 4.0, -4.0].iter().flat_map(|&d| axis_angle(z, d)).collect();
    chan(&mut idle_s, &mut idle_c, b.sampler(&[0.0, 1.0, 2.0], &sway, "VEC4", 4, "LINEAR"), node("spine"), "rotation");
    let tilt: Vec<f32> = [10.0f32, 10.0].iter().flat_map(|&d| axis_angle(x, d)).collect();
    chan(&mut idle_s, &mut idle_c, b.sampler(&[0.0, 2.0], &tilt, "VEC4", 4, "LINEAR"), node("neck"), "rotation");
    let nods: Vec<f32> = [0.0f32, 12.0, 0.0, 12.0].iter().flat_map(|&d| axis_angle(x, d)).collect();
    chan(&mut idle_s, &mut idle_c, b.sampler(&[0.0, 0.5, 1.0, 1.5], &nods, "VEC4", 4, "STEP"), node("head"), "rotation");
    chan(&mut idle_s, &mut idle_c, b.sampler(&[0.0, 2.0], &[1.0; 6], "VEC3", 3, "LINEAR"), node("hand_l"), "scale");

    // wave (1 s): CUBICSPLINE arm (in-tangent, value, out-tangent per key), 60 Hz hand.
    let mut wave_s = Vec::new();
    let mut wave_c = Vec::new();
    let rest_arm = js[4].3;
    let up = qmul(rest_arm, axis_angle(z, 120.0));
    let mut arm = Vec::new();
    for (k, q) in [rest_arm, up, rest_arm].iter().enumerate() {
        let tangent = if k == 1 { [0.0; 4] } else { [0.0, 0.0, if k == 0 { 0.9 } else { -0.9 }, 0.0] };
        arm.extend_from_slice(&tangent);
        arm.extend_from_slice(q);
        arm.extend_from_slice(&tangent);
    }
    chan(&mut wave_s, &mut wave_c, b.sampler(&[0.0, 0.5, 1.0], &arm, "VEC4", 4, "CUBICSPLINE"), node("arm_l"), "rotation");
    let times60: Vec<f32> = (0..=60).map(|k| k as f32 / 60.0).collect();
    let hand: Vec<f32> = times60.iter().flat_map(|&t| axis_angle(z, 25.0 * (std::f32::consts::TAU * 3.0 * t).sin())).collect();
    chan(&mut wave_s, &mut wave_c, b.sampler(&times60, &hand, "VEC4", 4, "LINEAR"), node("hand_l"), "rotation");

    // pose: one key each.
    let mut pose_s = Vec::new();
    let mut pose_c = Vec::new();
    chan(&mut pose_s, &mut pose_c, b.sampler(&[0.0], &[0.0, 0.9, 0.0], "VEC3", 3, "LINEAR"), node("hips"), "translation");
    chan(&mut pose_s, &mut pose_c, b.sampler(&[0.0], &axis_angle(x, 20.0), "VEC4", 4, "LINEAR"), node("spine"), "rotation");
    chan(&mut pose_s, &mut pose_c, b.sampler(&[0.0], &qmul(rest_arm, axis_angle(z, 40.0)), "VEC4", 4, "LINEAR"), node("arm_l"), "rotation");

    let json = json!({
        "asset": { "version": "2.0", "generator": "Evani.Dev rig fixture (S3-7)" },
        "scene": 0,
        "scenes": [{ "name": "rig", "nodes": [0] }],
        "nodes": nodes,
        "meshes": [{ "name": "body", "primitives": [{ "attributes": { "POSITION": a_pos, "NORMAL": a_nrm, "JOINTS_0": a_jnt, "WEIGHTS_0": a_wgt }, "indices": a_idx, "material": 0 }] }],
        "materials": [{ "name": "skin", "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.55, 0.4, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.8 } }],
        "skins": [{ "name": "rig", "joints": [1, 2, 3, 4, 5, 6], "skeleton": 1, "inverseBindMatrices": a_ibm }],
        "animations": [
            { "name": "idle", "samplers": idle_s, "channels": idle_c },
            { "name": "wave", "samplers": wave_s, "channels": wave_c },
            { "name": "pose", "samplers": pose_s, "channels": pose_c },
        ],
        "buffers": [{ "byteLength": 0 }],
        "bufferViews": b.views,
        "accessors": b.accessors,
    });
    Doc { json, bin: b.bin }
}
