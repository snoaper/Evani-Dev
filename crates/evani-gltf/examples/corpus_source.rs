//! Writes the float source model of the reader's test corpus
//! (`tests/fixtures/models/corpus.src.glb`): a UV sphere with normals,
//! tangents, UVs and a material using every core metallic-roughness texture;
//! a skinned tube with two joints; a line strip; anchors, a collision node,
//! an animation and a root `extras.evani`. gltfpack 1.3 then makes the
//! quantised and meshopt-compressed variants (tests/fixtures/models/README.md).
//!
//! `cargo run -p evani-gltf --example corpus_source -- <out.glb>`

use serde_json::{Value, json};
use std::f32::consts::{PI, TAU};

struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut v = json!({ "buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }
    fn floats(&mut self, v: &[f32], ty: &str, n: usize, target: Option<u32>, bounds: bool) -> usize {
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let mut a = json!({ "bufferView": view, "componentType": 5126, "count": v.len() / n, "type": ty });
        if bounds {
            let lo: Vec<f32> = (0..n).map(|k| v.iter().skip(k).step_by(n).copied().fold(f32::INFINITY, f32::min)).collect();
            let hi: Vec<f32> = (0..n).map(|k| v.iter().skip(k).step_by(n).copied().fold(f32::NEG_INFINITY, f32::max)).collect();
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }
    fn raw(&mut self, bytes: &[u8], ctype: u32, count: usize, ty: &str, target: Option<u32>) -> usize {
        let view = self.view(bytes, target);
        self.accessors.push(json!({ "bufferView": view, "componentType": ctype, "count": count, "type": ty }));
        self.accessors.len() - 1
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A 2x2 RGBA PNG with a stored (uncompressed) deflate block.
fn tiny_png() -> Vec<u8> {
    let raw: Vec<u8> = [[0u8, 230, 80, 50, 255, 40, 120, 220, 255], [0, 250, 210, 60, 255, 30, 160, 90, 255]].concat();
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    z.extend_from_slice(&raw);
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |ty: &[u8], body: &[u8]| {
        png.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(body);
        png.extend_from_slice(&c);
        png.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0, 0, 0]);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    png
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: corpus_source <out.glb>")?;
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() };

    // Sphere: 12 segments x 8 rings, seam duplicated.
    let (seg, rings, r) = (12usize, 8usize, 0.5f32);
    let (mut pos, mut nrm, mut tan, mut uv, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    for j in 0..=rings {
        let th = PI * j as f32 / rings as f32;
        for i in 0..=seg {
            let ph = TAU * i as f32 / seg as f32;
            let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
            pos.extend_from_slice(&[n[0] * r, n[1] * r, n[2] * r]);
            nrm.extend_from_slice(&n);
            tan.extend_from_slice(&[-ph.sin(), 0.0, ph.cos(), 1.0]);
            uv.extend_from_slice(&[i as f32 / seg as f32, j as f32 / rings as f32]);
        }
    }
    for j in 0..rings {
        for i in 0..seg {
            let a = (j * (seg + 1) + i) as u16;
            let b = a + seg as u16 + 1;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let sp = bin.floats(&pos, "VEC3", 3, Some(34962), true);
    let sn = bin.floats(&nrm, "VEC3", 3, Some(34962), false);
    let st = bin.floats(&tan, "VEC4", 4, Some(34962), false);
    let su = bin.floats(&uv, "VEC2", 2, Some(34962), false);
    let ib: Vec<u8> = idx.iter().flat_map(|x| x.to_le_bytes()).collect();
    let si = bin.raw(&ib, 5123, idx.len(), "SCALAR", Some(34963));

    // Skinned tube: 3 rings of 8, weights blend from root to tip.
    let (mut tp, mut tn, mut tj, mut tw, mut ti) = (vec![], vec![], vec![], vec![], vec![]);
    for k in 0..3 {
        let y = k as f32 * 0.5;
        for i in 0..8 {
            let a = TAU * i as f32 / 8.0;
            tp.extend_from_slice(&[0.1 * a.cos(), y, 0.1 * a.sin()]);
            tn.extend_from_slice(&[a.cos(), 0.0, a.sin()]);
            tj.extend_from_slice(&[0u8, 1, 0, 0]);
            let t = y;
            tw.extend_from_slice(&[1.0 - t, t, 0.0, 0.0]);
        }
    }
    for k in 0..2u16 {
        for i in 0..8u16 {
            let a = k * 8 + i;
            let b = k * 8 + (i + 1) % 8;
            ti.extend_from_slice(&[a, b, a + 8, b, b + 8, a + 8]);
        }
    }
    let ap = bin.floats(&tp, "VEC3", 3, Some(34962), true);
    let an = bin.floats(&tn, "VEC3", 3, Some(34962), false);
    let aj = bin.raw(&tj, 5121, 24, "VEC4", Some(34962));
    let aw = bin.floats(&tw, "VEC4", 4, Some(34962), false);
    let ib: Vec<u8> = ti.iter().flat_map(|x| x.to_le_bytes()).collect();
    let ai = bin.raw(&ib, 5123, ti.len(), "SCALAR", Some(34963));
    let ibm: Vec<f32> = [[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.8, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.8, -0.5, 0.0, 1.0]].concat();
    let am = bin.floats(&ibm, "MAT4", 16, None, false);

    // Line strip guide (lines mode, 6 points).
    let lp: Vec<f32> = (0..6).flat_map(|i| [i as f32 * 0.2 - 0.5, 0.05, -0.8]).collect();
    let lpa = bin.floats(&lp, "VEC3", 3, Some(34962), true);
    let li: Vec<u8> = [0u16, 1, 1, 2, 2, 3, 3, 4, 4, 5].iter().flat_map(|x| x.to_le_bytes()).collect();
    let lia = bin.raw(&li, 5123, 10, "SCALAR", Some(34963));

    // Animation: tip joint rotates about z, the body bobs.
    let times = bin.floats(&[0.0, 0.5, 1.0], "SCALAR", 1, None, true);
    let h = (PI / 12.0).sin();
    let rot = bin.floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, h, (PI / 12.0).cos(), 0.0, 0.0, 0.0, 1.0], "VEC4", 4, None, false);
    let bob = bin.floats(&[0.0, 0.6, 0.0, 0.0, 0.7, 0.0, 0.0, 0.6, 0.0], "VEC3", 3, None, false);

    let png = tiny_png();
    let img_view = bin.view(&png, None);
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }

    let tex = |i: usize| json!({ "index": i });
    let doc = json!({
        "asset": { "version": "2.0", "generator": "evani-gltf corpus_source" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [
            { "name": "corpus", "children": [1, 2, 3, 5, 6, 7, 9] },
            { "name": "body", "mesh": 0, "translation": [0.0, 0.6, 0.0], "extras": { "evani": { "kind": "solid" } } },
            { "name": "arm", "mesh": 1, "skin": 0 },
            { "name": "bone_root", "translation": [0.8, 0.0, 0.0], "children": [4] },
            { "name": "bone_tip", "translation": [0.0, 0.5, 0.0] },
            { "name": "anchor:seat", "translation": [0.0, 1.1, 0.0], "extras": { "evani": { "kind": "anchor", "normal": [0.0, 1.0, 0.0], "part": "body" } } },
            { "name": "anchor:hand", "translation": [0.8, 1.0, 0.0], "extras": { "evani": { "kind": "anchor", "normal": [1.0, 0.0, 0.0], "part": "hand" } } },
            { "name": "collision", "children": [8] },
            { "name": "col:body", "translation": [0.0, 0.6, 0.0], "extras": { "evani": { "kind": "collision", "solid": "body", "proxy": { "shape": "sphere", "radius": 0.5 } } } },
            { "name": "guide", "mesh": 2 }
        ],
        "meshes": [
            { "name": "body", "primitives": [{ "attributes": { "POSITION": sp, "NORMAL": sn, "TANGENT": st, "TEXCOORD_0": su }, "indices": si, "material": 0 }] },
            { "name": "arm", "primitives": [{ "attributes": { "POSITION": ap, "NORMAL": an, "JOINTS_0": aj, "WEIGHTS_0": aw }, "indices": ai, "material": 1 }] },
            { "name": "guide", "primitives": [{ "attributes": { "POSITION": lpa }, "indices": lia, "mode": 1, "material": 1 }] }
        ],
        "skins": [{ "joints": [3, 4], "inverseBindMatrices": am, "skeleton": 3 }],
        "materials": [
            {
                "name": "paint",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [0.9, 0.3, 0.2, 1.0], "baseColorTexture": tex(0),
                    "metallicFactor": 0.1, "roughnessFactor": 0.7, "metallicRoughnessTexture": tex(1)
                },
                "normalTexture": { "index": 2, "scale": 0.8 },
                "occlusionTexture": { "index": 3, "strength": 0.5 },
                "emissiveTexture": tex(4),
                "emissiveFactor": [0.2, 0.1, 0.0],
                "alphaMode": "MASK", "alphaCutoff": 0.4, "doubleSided": true
            },
            { "name": "metal", "pbrMetallicRoughness": { "baseColorFactor": [0.5, 0.5, 0.55, 1.0], "metallicFactor": 1.0, "roughnessFactor": 0.3 } }
        ],
        "textures": [{ "sampler": 0, "source": 0 }, { "sampler": 0, "source": 0 }, { "sampler": 0, "source": 0 }, { "sampler": 0, "source": 0 }, { "sampler": 0, "source": 0 }],
        "samplers": [{ "magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 10497 }],
        "images": [{ "bufferView": img_view, "mimeType": "image/png" }],
        "animations": [{
            "name": "wave",
            "samplers": [{ "input": times, "output": rot }, { "input": times, "output": bob }],
            "channels": [{ "sampler": 0, "target": { "node": 4, "path": "rotation" } }, { "sampler": 1, "target": { "node": 1, "path": "translation" } }]
        }],
        "buffers": [{ "byteLength": bin.data.len() }],
        "bufferViews": bin.views,
        "accessors": bin.accessors,
        "extras": { "evani": {
            "format": 1, "class": "corpus", "style": "toon", "tier": "low",
            "parts": [
                { "name": "body", "role": "body", "solid": "body" },
                { "name": "arm", "role": "limb", "parent": "body", "solid": "arm" },
                { "name": "hand", "role": "grip", "parent": "arm", "group": "hands" }
            ],
            "lods": [{ "node": "body", "min_screen": 0.25 }, { "node": "guide", "min_screen": 0.0 }],
            "textures": [{ "texture": 0, "name": "base", "sets": { "astc": "corpus.base.astc.ktx2", "bc7": { "uri": "corpus.base.bc7.ktx2" } } }]
        } }
    });
    let mut js = serde_json::to_vec(&doc)?;
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.data.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(js.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&js);
    glb.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin.data);
    std::fs::write(&out, &glb)?;
    println!("wrote {out} ({} bytes)", glb.len());
    Ok(())
}
