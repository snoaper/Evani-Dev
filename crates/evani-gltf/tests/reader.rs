//! The S3-2 reader tests (docs/STAGE3_PLAN.md, S3-2). The fixtures are in
//! `tests/fixtures/models/` at the workspace root; their provenance (gltfpack
//! 1.3 options, Evani.Model commands, checksums) is in its README.md.
//!
//! A test crate: its helpers (outside `#[test]` functions, where the
//! workspace's allow-in-tests setting does not reach) fail loudly on a broken
//! fixture, which is what a test should do.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evani_gltf::meshopt::Filter;
use evani_gltf::{ComponentType, Error, Gltf, transform_point};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/models")
}

fn bytes(name: &str) -> Vec<u8> {
    let p = fixtures().join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn load(name: &str) -> Gltf {
    Gltf::from_glb(&bytes(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A `.glb` from a JSON document and a BIN chunk (both padded to 4 bytes).
fn glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
    let mut js = serde_json::to_vec(doc).expect("json");
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// A fixture's JSON document and BIN chunk, for edited copies.
fn parts(name: &str) -> (Value, Vec<u8>) {
    let b = bytes(name);
    let (js, bin) = evani_gltf::split_glb(&b).expect("split");
    (serde_json::from_slice(js).expect("json"), bin.unwrap_or_default().to_vec())
}

/// The accessors a mesh or skin uses, by what reads them.
fn filter_of(g: &Gltf, accessor: usize) -> Option<evani_gltf::meshopt::Filter> {
    let v = g.accessors[accessor].buffer_view?;
    g.buffer_views[v].meshopt.as_ref().map(|m| m.filter)
}

/// Tolerance for a value decoded through a lossy meshopt filter. Octahedral
/// (8-bit here) and quaternion filters re-quantise unit vectors: one
/// encode-decode trip moves a component by a step or two (1/127 = 0.008).
/// Exponential keeps the float to the encoder's mantissa bits (relative).
fn filter_tolerance(f: Filter, plain: f32) -> f32 {
    match f {
        Filter::Exponential => 1e-3 * plain.abs().max(1.0),
        _ => 0.02,
    }
}

/// One element of an accessor as raw bytes.
fn element(g: &Gltf, i: usize, e: usize) -> Vec<u8> {
    let (b, stride) = g.accessor_bytes(i).expect("bytes");
    let size = g.accessors[i].element_size();
    b.map_or(vec![0; size], |b| b[e * stride..][..size].to_vec())
}

/// Compare one primitive of the plain file with the same primitive of the
/// `-cc` file. gltfpack orders vertices differently when it compresses (it
/// optimises the order for the codec), so the comparison is by vertex
/// identity, not position in the buffer: each vertex is keyed by the bytes
/// of its unfiltered attributes (position, UV, joints, weights), the two
/// files must hold the same multiset of keys and the same triangles over
/// them, and each vertex's filtered attributes (octahedral normals and
/// tangents) must match the plain file's within [`filter_tolerance`].
fn same_primitive(a: &Gltf, b: &Gltf, pa: &evani_gltf::Primitive, pb: &evani_gltf::Primitive, what: &str) -> usize {
    assert_eq!(pa.mode, pb.mode, "{what}: mode");
    let mut names: Vec<&str> = pa.attributes.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    let mut names_b: Vec<&str> = pb.attributes.iter().map(|(n, _)| n.as_str()).collect();
    names_b.sort_unstable();
    assert_eq!(names, names_b, "{what}: attributes");
    let exact: Vec<&str> = names.iter().copied().filter(|n| matches!(filter_of(b, pb.attribute(n).expect("attr")), None | Some(Filter::None))).collect();
    let lossy: Vec<&str> = names.iter().copied().filter(|n| !exact.contains(n)).collect();
    assert!(exact.contains(&"POSITION"), "{what}: positions must be unfiltered");
    let count = a.accessors[pa.attribute("POSITION").expect("pos")].count;
    for n in &names {
        let (x, y) = (&a.accessors[pa.attribute(n).expect("a")], &b.accessors[pb.attribute(n).expect("b")]);
        assert_eq!((x.component_type, x.count, x.components, x.normalized), (y.component_type, y.count, y.components, y.normalized), "{what} {n}: accessor type");
    }
    let key = |g: &Gltf, p: &evani_gltf::Primitive, v: usize| -> Vec<u8> { exact.iter().flat_map(|n| element(g, p.attribute(n).expect("attr"), v)).collect() };
    let ka: Vec<Vec<u8>> = (0..count).map(|v| key(a, pa, v)).collect();
    let kb: Vec<Vec<u8>> = (0..count).map(|v| key(b, pb, v)).collect();
    let (mut sa, mut sb) = (ka.clone(), kb.clone());
    sa.sort_unstable();
    sb.sort_unstable();
    assert!(sa == sb, "{what}: the vertices differ");

    // Filtered attributes, vertex by vertex (any plain vertex with the same
    // key may match: keys repeat only where the source repeats a vertex).
    let la: Vec<Vec<f32>> = lossy.iter().map(|n| a.read_f32(pa.attribute(n).expect("a")).expect("plain")).collect();
    let lb: Vec<Vec<f32>> = lossy.iter().map(|n| b.read_f32(pb.attribute(n).expect("b")).expect("packed")).collect();
    for (vb, k) in kb.iter().enumerate() {
        let ok = ka.iter().enumerate().filter(|(_, x)| *x == k).any(|(va, _)| {
            lossy.iter().enumerate().all(|(li, n)| {
                let f = filter_of(b, pb.attribute(n).expect("b")).expect("filtered");
                let c = a.accessors[pa.attribute(n).expect("a")].components;
                (0..c).all(|j| (la[li][va * c + j] - lb[li][vb * c + j]).abs() <= filter_tolerance(f, la[li][va * c + j]))
            })
        });
        assert!(ok, "{what}: vertex {vb} filtered attributes {lossy:?} differ from the plain file");
    }

    // Indices: the same triangles (a triangle may be rotated by the codec,
    // never flipped), or for lines the same index sequence.
    let shapes = |g: &Gltf, p: &evani_gltf::Primitive, keys: &[Vec<u8>]| -> Vec<Vec<Vec<u8>>> {
        let idx = g.read_u32(p.indices.expect("indexed")).expect("indices");
        let n = if p.mode == 4 { 3 } else { 1 };
        let mut out: Vec<Vec<Vec<u8>>> = idx
            .chunks_exact(n)
            .map(|t| {
                let mut t: Vec<Vec<u8>> = t.iter().map(|&i| keys[i as usize].clone()).collect();
                let r = (0..t.len()).min_by(|&x, &y| t[x].cmp(&t[y])).unwrap_or(0);
                t.rotate_left(r);
                t
            })
            .collect();
        if p.mode == 4 {
            out.sort_unstable();
        }
        out
    };
    assert!(shapes(a, pa, &ka) == shapes(b, pb, &kb), "{what}: the triangles differ");
    count
}

/// Compare a plain gltfpack file with the same file made with `-cc`: every
/// mesh primitive by vertex identity ([`same_primitive`]) and every other
/// accessor (skin matrices, animation keys) in order, bytes identical when
/// unfiltered and within [`filter_tolerance`] when filtered. Returns
/// (vertices compared, other accessors compared, filtered values compared).
fn same_attributes(a: &Gltf, b: &Gltf, packed: &str) -> (usize, usize, usize) {
    let plain = "the plain file";
    assert_eq!((a.accessors.len(), a.meshes.len()), (b.accessors.len(), b.meshes.len()), "{plain} vs {packed}: accessor and mesh counts");
    assert!(b.buffer_views.iter().any(|v| v.meshopt.is_some()), "{packed} has no compressed view");
    let mut vertices = 0;
    let mut in_mesh = vec![false; a.accessors.len()];
    for (m, (ma, mb)) in a.meshes.iter().zip(&b.meshes).enumerate() {
        assert_eq!(ma.primitives.len(), mb.primitives.len());
        for (p, (pa, pb)) in ma.primitives.iter().zip(&mb.primitives).enumerate() {
            vertices += same_primitive(a, b, pa, pb, &format!("{packed} mesh {m} primitive {p}"));
            for &i in pa.attributes.iter().map(|(_, i)| i).chain(&pa.indices) {
                in_mesh[i] = true;
            }
        }
    }
    let (mut others, mut filtered) = (0, 0);
    for i in (0..a.accessors.len()).filter(|&i| !in_mesh[i]) {
        let (x, y) = (&a.accessors[i], &b.accessors[i]);
        assert_eq!((x.component_type, x.count, x.components, x.normalized), (y.component_type, y.count, y.components, y.normalized), "{packed} accessor {i}: type");
        others += 1;
        match filter_of(b, i) {
            None | Some(Filter::None) => {
                for e in 0..x.count {
                    assert_eq!(element(a, i, e), element(b, i, e), "{packed} accessor {i} element {e}");
                }
            }
            Some(f) => {
                let (vx, vy) = (a.read_f32(i).expect("plain"), b.read_f32(i).expect("packed"));
                for (k, (p, q)) in vx.iter().zip(&vy).enumerate() {
                    assert!((p - q).abs() <= filter_tolerance(f, *p), "{packed} accessor {i} ({f:?}) value {k}: {p} vs {q}");
                    filtered += 1;
                }
            }
        }
    }
    (vertices, others, filtered)
}

#[test]
fn decodes_gltfpack_corpus() {
    let (v1, o1, f1) = same_attributes(&load("corpus.q.glb"), &load("corpus.qc.glb"), "corpus.qc.glb");
    let (v2, o2, f2) = same_attributes(&load("corpus.qn.glb"), &load("corpus.qnc.glb"), "corpus.qnc.glb");
    let (v3, _, _) = same_attributes(&load("kart.low.q.glb"), &load("kart.low.qc.glb"), "kart.low.qc.glb");
    // Sphere 117 + tube 24 + line 6 vertices; skin matrices and 3 animation accessors.
    assert_eq!((v1, v2, o1, o2), (147, 147, 4, 4));
    assert!(v3 > 1000, "kart vertices compared: {v3}");
    assert!(f1 > 100 && f2 > 100, "filtered animation values compared: {f1} {f2}");

    // All three modes and all three filters are in the corpus.
    use evani_gltf::meshopt::{Filter, Mode};
    let qc = load("corpus.qc.glb");
    let m: Vec<_> = qc.buffer_views.iter().filter_map(|v| v.meshopt.as_ref()).collect();
    for mode in [Mode::Attributes, Mode::Triangles, Mode::Indices] {
        assert!(m.iter().any(|x| x.mode == mode), "corpus lacks {mode:?}");
    }
    for filter in [Filter::Octahedral, Filter::Quaternion, Filter::Exponential] {
        assert!(m.iter().any(|x| x.filter == filter), "corpus lacks {filter:?}");
    }

    // Decoded octahedral normals are unit length (the filter's own output).
    let mesh = &qc.meshes[0].primitives[0];
    let n = qc.read_f32(mesh.attribute("NORMAL").expect("normal")).expect("normals");
    for v in n.as_chunks::<3>().0 {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len - 1.0).abs() < 0.03, "normal length {len}");
    }

    // The .gltf with external buffers (-cf, no filters) decodes to the same
    // model as the plain .glb.
    let cf = load_cf(&mut |_| ());
    same_attributes(&load("corpus.q.glb"), &cf, "corpus.cf.gltf");
}

fn load_cf(seen: &mut dyn FnMut(&str)) -> Gltf {
    let json = bytes("corpus.cf.gltf");
    Gltf::from_gltf(&json, &mut |uri: &str| {
        seen(uri);
        std::fs::read(fixtures().join(uri)).ok()
    })
    .expect("corpus.cf.gltf")
}

/// World positions of every vertex of a mesh drawn by a node.
fn world_positions(g: &Gltf, world: &[[f32; 16]], node: usize) -> Vec<[f32; 3]> {
    let mesh = g.nodes[node].mesh.expect("mesh node");
    let p = g.read_f32(g.meshes[mesh].primitives[0].attribute("POSITION").expect("position")).expect("positions");
    p.as_chunks::<3>().0.iter().map(|v| transform_point(&world[node], *v)).collect()
}

#[test]
fn dequantizes_like_the_spec() {
    // Component rules (glTF 2.0 "Animations"/KHR_mesh_quantization table):
    // signed normalised max(c / MAX, -1), unsigned c / MAX, integers as is.
    let cases: [(ComponentType, &[u8], bool, f32); 12] = [
        (ComponentType::I8, &[0x7f], true, 1.0),
        (ComponentType::I8, &[0x81], true, -1.0),
        (ComponentType::I8, &[0x80], true, -1.0), // -128 clamps to -1
        (ComponentType::U8, &[0xff], true, 1.0),
        (ComponentType::U8, &[51], true, 0.2),
        (ComponentType::I16, &[0xff, 0x7f], true, 1.0),
        (ComponentType::I16, &[0x00, 0x80], true, -1.0), // -32768 clamps to -1
        (ComponentType::U16, &[0xff, 0xff], true, 1.0),
        (ComponentType::U16, &[0x00, 0x80], false, 32768.0),
        (ComponentType::I16, &[0x00, 0x80], false, -32768.0),
        (ComponentType::I8, &[0xfe], false, -2.0),
        (ComponentType::U8, &[200], false, 200.0),
    ];
    for (ty, b, norm, want) in cases {
        let got = ty.to_f32(b, norm);
        assert!((got - want).abs() < 1e-6, "{ty:?} {b:?} normalized={norm}: {got} != {want}");
    }

    // The node scale and offset undo position quantisation: the
    // non-skinned meshes land where the float source has them. Integer
    // positions (q) and normalised positions (qn) both.
    let src = load("corpus.src.glb");
    let sw = src.world_matrices();
    for name in ["corpus.q.glb", "corpus.qn.glb", "corpus.qc.glb", "corpus.qnc.glb"] {
        let g = load(name);
        let gw = g.world_matrices();
        for mesh_node in ["body", "guide"] {
            let want = world_positions(&src, &sw, src.node_by_name(mesh_node).expect("src node"));
            // gltfpack moves the mesh to an unnamed child of the named node.
            let named = g.node_by_name(mesh_node).expect("named node");
            let holder = g.nodes[named].children.iter().copied().find(|&c| g.nodes[c].mesh.is_some()).expect("mesh child");
            let got = world_positions(&g, &gw, holder);
            assert_eq!(want.len(), got.len(), "{name} {mesh_node}: vertex count");
            for p in &got {
                let d = want.iter().map(|q| (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f32>().sqrt()).fold(f32::INFINITY, f32::min);
                // 14-bit positions over a 1.6 m box: a step is 0.1 mm.
                assert!(d < 1e-3, "{name} {mesh_node}: vertex {p:?} is {d} from the source");
            }
        }
        // Normalised UVs times the KHR_texture_transform scale give the
        // source UV range [0, 1].
        let prim = &g.meshes[g.nodes[g.nodes[g.node_by_name("body").expect("body")].children[0]].mesh.expect("mesh")].primitives[0];
        let mat = &g.materials[prim.material.expect("material")];
        let tt = mat.base_color_texture.as_ref().and_then(|t| t.transform.as_ref()).expect("texture transform");
        let uv = g.read_f32(prim.attribute("TEXCOORD_0").expect("uv")).expect("uvs");
        let (lo, hi) = uv.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), &u| (l.min(u * tt.scale[0] + tt.offset[0]), h.max(u * tt.scale[0] + tt.offset[0])));
        assert!(lo.abs() < 1e-3 && (hi - 1.0).abs() < 1e-3, "{name}: uv range {lo}..{hi}");
    }
}

#[test]
fn fallback_buffers_are_ignored() {
    // qc: buffer 1 is the fallback, has no data in the file, and the model
    // still reads in full.
    let qc = load("corpus.qc.glb");
    assert_eq!(qc.buffers.len(), 2);
    assert!(qc.buffers[1].fallback && !qc.buffers[0].fallback);
    let (_, bin) = parts("corpus.qc.glb");
    assert!(bin.len() < qc.buffers[1].byte_length, "the BIN chunk does not hold the fallback data");
    for i in 0..qc.accessors.len() {
        qc.read_f32(i).unwrap_or_else(|e| panic!("accessor {i}: {e}"));
    }

    // .gltf: the fallback buffer's file is never requested.
    let mut asked = Vec::new();
    let cf = load_cf(&mut |u| asked.push(u.to_owned()));
    assert_eq!(asked, ["corpus.cf.bin"]);
    assert!(cf.buffers[1].fallback);
    assert_eq!(cf.buffers[1].uri.as_deref(), Some("corpus.cf.fallback.bin"));

    // A plain view that reads a fallback buffer is an error (it has no data).
    let (mut doc, bin) = parts("corpus.qc.glb");
    let views = doc["bufferViews"].as_array_mut().expect("views");
    views.push(json!({ "buffer": 1, "byteOffset": 0, "byteLength": 16 }));
    let v = views.len() - 1;
    doc["accessors"].as_array_mut().expect("accessors").push(json!({ "bufferView": v, "componentType": 5126, "count": 1, "type": "VEC4" }));
    assert!(Gltf::from_glb(&glb(&doc, &bin)).is_err());

    // And so is a compressed view whose source is the fallback buffer.
    let (mut doc, bin) = parts("corpus.qc.glb");
    let v = doc["bufferViews"].as_array_mut().expect("views").iter_mut().find(|v| v.get("extensions").is_some()).expect("compressed view");
    v["extensions"]["EXT_meshopt_compression"]["buffer"] = json!(1);
    assert!(Gltf::from_glb(&glb(&doc, &bin)).is_err());
}

#[test]
fn reads_evani_extras_and_anchors() {
    let src = load("corpus.src.glb");
    let e = src.evani.as_ref().expect("extras.evani");
    assert_eq!(e.format, 1);
    assert_eq!((e.class.as_deref(), e.tier.as_deref()), (Some("corpus"), Some("low")));
    assert_eq!(e.raw["style"], "toon", "unknown fields are kept");
    // Part tree.
    assert_eq!(e.parts.len(), 3);
    assert_eq!(e.children("body").map(|p| p.name.as_str()).collect::<Vec<_>>(), ["arm"]);
    let hand = e.part("hand").expect("hand");
    assert_eq!((hand.parent.as_deref(), hand.group.as_deref(), hand.role.as_str()), (Some("arm"), Some("hands"), "grip"));
    assert_eq!(e.part("body").and_then(|p| p.solid.as_deref()), Some("body"));
    // LOD thresholds.
    assert_eq!(e.lods.len(), 2);
    assert_eq!((e.lods[0].node.as_str(), e.lods[0].min_screen), ("body", 0.25));
    assert_eq!(e.lods[0].node_index, src.node_by_name("body"));
    assert_eq!(e.lods[1].min_screen, 0.0);
    // Texture sets (string or {uri}).
    assert_eq!(e.textures.len(), 1);
    assert_eq!((e.textures[0].texture, e.textures[0].name.as_str()), (0, "base"));
    let mut sets = e.textures[0].sets.clone();
    sets.sort();
    assert_eq!(sets, [("astc".to_owned(), "corpus.base.astc.ktx2".to_owned()), ("bc7".to_owned(), "corpus.base.bc7.ktx2".to_owned())]);

    // Every core metallic-roughness material field.
    let m = &src.materials[0];
    assert_eq!(m.base_color_factor, [0.9, 0.3, 0.2, 1.0]);
    assert_eq!((m.metallic_factor, m.roughness_factor), (0.1, 0.7));
    assert_eq!(m.emissive_factor, [0.2, 0.1, 0.0]);
    assert!(m.base_color_texture.is_some() && m.metallic_roughness_texture.is_some() && m.emissive_texture.is_some());
    assert_eq!(m.normal_texture.as_ref().map(|t| t.amount), Some(0.8));
    assert_eq!(m.occlusion_texture.as_ref().map(|t| t.amount), Some(0.5));
    assert_eq!((m.alpha_mode, m.alpha_cutoff, m.double_sided), (evani_gltf::AlphaMode::Mask, 0.4, true));
    assert_eq!(src.images.len(), 1);
    assert!(src.image_bytes(0).expect("image").is_some_and(|b| b.starts_with(b"\x89PNG")));

    // Anchors and the collision node, in the source and after gltfpack.
    for name in ["corpus.src.glb", "corpus.qc.glb", "corpus.qnc.glb"] {
        let g = load(name);
        let a: Vec<_> = g.anchors.iter().map(|a| (a.name.as_str(), [a.world[12], a.world[13], a.world[14]])).collect();
        assert_eq!(a, [("seat", [0.0, 1.1, 0.0]), ("hand", [0.8, 1.0, 0.0])], "{name}");
        assert_eq!(g.nodes[g.anchors[0].node].extras["evani"]["part"], "body", "{name}: anchor extras kept");
        let col = g.collision.expect("collision node");
        let kids: Vec<_> = g.nodes[col].children.iter().map(|&c| g.nodes[c].name.as_str()).collect();
        assert_eq!(kids, ["col:body"], "{name}");
        assert_eq!(g.nodes[g.nodes[col].children[0]].extras["evani"]["proxy"]["shape"], "sphere");
    }
    // gltfpack drops the root extras (docs/MODEL_FORMAT.md, "After gltfpack").
    assert!(load("corpus.qc.glb").evani.is_none());

    // A newer format is refused; format 0 and a missing format too.
    let (doc, bin) = parts("corpus.src.glb");
    for bad in [json!(2), json!(0), json!("1"), Value::Null] {
        let mut d = doc.clone();
        d["extras"]["evani"]["format"] = bad.clone();
        let r = Gltf::from_glb(&glb(&d, &bin));
        assert!(r.is_err(), "format {bad} was accepted");
    }
    let mut d = doc.clone();
    d["extras"]["evani"] = json!("not an object");
    assert!(Gltf::from_glb(&glb(&d, &bin)).is_err());
}

#[test]
fn reads_evani_model_fixtures() {
    let report: Value = serde_json::from_slice(&bytes("kart.evmodel.json")).expect("kart.evmodel.json");
    for (file, class) in [("kart.low.glb", "kart"), ("crate.low.glb", "crate"), ("barrel.low.glb", "barrel")] {
        let g = load(file);
        let e = g.evani.as_ref().unwrap_or_else(|| panic!("{file}: no extras.evani"));
        assert_eq!((e.format, e.class.as_deref(), e.tier.as_deref()), (1, Some(class), Some("low")), "{file}");
        assert!(!e.parts.is_empty(), "{file}: part tree");
        // Triangle count: every mesh node's triangles, as the writer counts.
        let tris: usize = g.nodes.iter().filter_map(|n| n.mesh).flat_map(|m| &g.meshes[m].primitives).map(|p| g.accessors[p.indices.expect("indexed")].count / 3).sum();
        assert_eq!(Some(tris as u64), e.raw["triangles"].as_u64(), "{file}: triangles");
        if file == "kart.low.glb" {
            assert_eq!(Some(tris as u64), report["tiers"]["low"]["tris"].as_u64(), "kart: report triangles");
        }
        // Every mesh attribute reads.
        for i in 0..g.accessors.len() {
            g.read_f32(i).unwrap_or_else(|err| panic!("{file} accessor {i}: {err}"));
        }
    }

    // The kart's anchors are the report's anchors, by name and part. The low
    // tier re-seats surface anchors onto its own coarser surface, so the
    // position matches within one of its voxel cells (extras.evani.cell_m);
    // the wheel hubs (axle centres, not on a surface) match exactly.
    for file in ["kart.low.glb", "kart.low.q.glb", "kart.low.qc.glb"] {
        let g = load(file);
        let cell = load("kart.low.glb").evani.and_then(|e| e.raw["cell_m"].as_f64()).expect("cell_m") as f32;
        let want = report["anchors"].as_array().expect("anchors");
        assert_eq!(g.anchors.len(), want.len(), "{file}: anchor count");
        for (a, w) in g.anchors.iter().zip(want) {
            assert_eq!(a.name, w["name"].as_str().expect("name"), "{file}");
            assert_eq!(g.nodes[a.node].extras["evani"]["part"], w["part"], "{file} {}", a.name);
            let at: Vec<f32> = w["at"].as_str().expect("at").split(',').map(|x| x.parse().expect("number")).collect();
            let d = ((a.world[12] - at[0]).powi(2) + (a.world[13] - at[1]).powi(2) + (a.world[14] - at[2]).powi(2)).sqrt();
            let tol = if a.name.starts_with("wheel_hub") { 1e-4 } else { cell };
            assert!(d <= tol, "{file} anchor {}: {d} from the report's 'at'", a.name);
        }
        // The collision node holds one `col:<name>` proxy per report entry.
        let col = g.collision.expect("collision node");
        let kids: Vec<String> = g.nodes[col].children.iter().map(|&c| g.nodes[c].name.clone()).collect();
        let want: Vec<String> = report["collision"].as_array().expect("collision").iter().map(|c| format!("col:{}", c["name"].as_str().expect("name"))).collect();
        assert_eq!(kids, want, "{file}: collision proxies");
        assert_eq!(g.nodes[col].extras["evani"]["kind"], "footprint");
    }
}

/// xorshift32: deterministic "random" bytes without the rand crate.
struct XorShift(u32);
impl XorShift {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

/// Reading must return (Ok or Err) and never panic or abort.
fn no_panic(case: &str, data: &[u8]) -> Result<Gltf, Error> {
    match std::panic::catch_unwind(|| Gltf::from_glb(data)) {
        Ok(r) => {
            // Whatever loaded must also read without panicking.
            if let Ok(g) = &r {
                for i in 0..g.accessors.len() {
                    let _ = g.read_f32(i);
                    let _ = g.read_u32(i);
                }
                for i in 0..g.images.len() {
                    let _ = g.image_bytes(i);
                }
                let _ = g.world_matrices();
            }
            r
        }
        Err(_) => panic!("{case}: the reader panicked"),
    }
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    let qc = bytes("corpus.qc.glb");

    // Container: bad magic, version, length fields.
    let mut b = qc.clone();
    b[0] = b'X';
    assert_eq!(no_panic("magic", &b).err(), Some(Error("glb: bad magic (not a glTF binary)")));
    let mut b = qc.clone();
    b[4] = 1;
    assert!(no_panic("version", &b).is_err());
    for len in [0u32, 19, qc.len() as u32 + 1, u32::MAX] {
        let mut b = qc.clone();
        b[8..12].copy_from_slice(&len.to_le_bytes());
        assert!(no_panic("length", &b).is_err(), "length {len}");
    }
    for json_len in [0u32, 3, 1 << 20, u32::MAX, u32::MAX - 7] {
        let mut b = qc.clone();
        b[12..16].copy_from_slice(&json_len.to_le_bytes());
        assert!(no_panic("json chunk length", &b).is_err(), "json length {json_len}");
    }
    let mut b = qc.clone();
    b[16..20].copy_from_slice(b"BIN\0");
    assert!(no_panic("chunk type", &b).is_err());

    // Truncated anywhere: with the header's length left alone, and with it
    // edited to the truncated size (so the chunks themselves are short).
    let js_end = 20 + u32::from_le_bytes([qc[12], qc[13], qc[14], qc[15]]) as usize;
    for cut in 0..qc.len() {
        let t = &qc[..cut];
        assert!(no_panic("truncated", t).is_err(), "cut {cut}");
        if cut >= 12 {
            let mut t = t.to_vec();
            t[8..12].copy_from_slice(&(cut as u32).to_le_bytes());
            let r = no_panic("truncated, length fixed", &t);
            // Cut inside the JSON or BIN chunk: an error. Cut exactly at a
            // chunk boundary drops the BIN chunk: still an error, since the
            // views read buffer 0.
            assert!(r.is_err(), "cut {cut} (json ends at {js_end})");
        }
    }

    // Byte flips in the BIN chunk (compressed streams, headers, tails):
    // every byte, three patterns. Ok or Err, never a panic.
    let mut rng = XorShift(0x5EED_1234);
    for at in js_end + 8..qc.len() {
        for x in [0xff, 0x80, (rng.next() as u8) | 1] {
            let mut b = qc.clone();
            b[at] ^= x;
            let _ = no_panic("bin flip", &b);
        }
    }
    // And in the JSON chunk (mostly parse errors, some valid-but-wrong).
    for at in (20..js_end).step_by(3) {
        let mut b = qc.clone();
        b[at] ^= 0x01;
        let _ = no_panic("json flip", &b);
    }

    // Random bytes: plain, and behind a valid header.
    for n in 0..2000usize {
        let len = (rng.next() as usize) % 512;
        let mut b: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if n % 2 == 1 && b.len() >= 20 {
            b[0..4].copy_from_slice(b"glTF");
            b[4..8].copy_from_slice(&2u32.to_le_bytes());
            b[8..12].copy_from_slice(&(len as u32).to_le_bytes());
            b[16..20].copy_from_slice(b"JSON");
        }
        assert!(no_panic("random", &b).is_err());
    }

    // Huge counts, offsets, lengths and strides edited into the JSON; bad
    // indices; a node cycle; an unsupported required extension.
    let (doc, bin) = parts("corpus.qc.glb");
    let huge = [json!(u64::MAX), json!(1u64 << 40), json!(u32::MAX), json!(-1), json!(1.5), json!("7")];
    type Edit = Box<dyn Fn(&mut Value, &Value)>;
    let mut edits: Vec<(String, Edit)> = Vec::new();
    for key in ["count", "byteOffset"] {
        edits.push((format!("accessor {key}"), Box::new(move |d, v| d["accessors"][1][key] = v.clone())));
    }
    for key in ["byteOffset", "byteLength", "byteStride"] {
        edits.push((format!("view {key}"), Box::new(move |d, v| d["bufferViews"][1][key] = v.clone())));
    }
    for key in ["count", "byteOffset", "byteLength", "byteStride"] {
        edits.push((format!("meshopt {key}"), Box::new(move |d, v| d["bufferViews"][1]["extensions"]["EXT_meshopt_compression"][key] = v.clone())));
    }
    edits.push(("buffer byteLength".into(), Box::new(|d, v| d["buffers"][0]["byteLength"] = v.clone())));
    edits.push(("view buffer".into(), Box::new(|d, v| d["bufferViews"][1]["buffer"] = v.clone())));
    edits.push(("accessor view".into(), Box::new(|d, v| d["accessors"][1]["bufferView"] = v.clone())));
    edits.push(("mesh material".into(), Box::new(|d, v| d["meshes"][0]["primitives"][0]["material"] = v.clone())));
    edits.push(("node child".into(), Box::new(|d, v| d["nodes"][3]["children"][0] = v.clone())));
    edits.push(("skin joint".into(), Box::new(|d, v| d["skins"][0]["joints"][0] = v.clone())));
    edits.push(("texture source".into(), Box::new(|d, v| d["textures"][0]["source"] = v.clone())));
    for (what, edit) in &edits {
        for v in &huge {
            let mut d = doc.clone();
            edit(&mut d, v);
            assert!(no_panic(what, &glb(&d, &bin)).is_err(), "{what} = {v} was accepted");
        }
    }
    // A node cycle, a node that is its own child, two parents.
    // (Node 3 is the root "corpus", node 4 its child "body", node 7
    // "bone_tip" the child of node 6.)
    for (node, children) in [(4usize, json!([3])), (3, json!([3, 4])), (4, json!([0, 7]))] {
        let mut d = doc.clone();
        d["nodes"][node]["children"] = children.clone();
        assert!(no_panic("hierarchy", &glb(&d, &bin)).is_err(), "node {node} children {children}");
    }
    let mut d = doc.clone();
    d["extensionsRequired"] = json!(["KHR_draco_mesh_compression"]);
    assert!(no_panic("extension", &glb(&d, &bin)).is_err());
    let mut d = doc.clone();
    d["asset"]["version"] = json!("1.0");
    assert!(no_panic("asset version", &glb(&d, &bin)).is_err());
    let mut d = doc.clone();
    d["accessors"] = json!({ "not": "an array" });
    assert!(no_panic("not an array", &glb(&d, &bin)).is_err());
    // MAT2 / MAT3 with 1- or 2-byte components have padded columns; the
    // reader refuses them rather than read shifted values (the engine only
    // uses float matrices). Float MAT3 is unpadded and is accepted as a type
    // (here it then fails on the corpus view's smaller stride, a later check).
    for (ty, ct, ok) in [("MAT3", 5121, false), ("MAT2", 5121, false), ("MAT3", 5123, false), ("MAT2", 5122, false), ("MAT3", 5126, true)] {
        let mut d = doc.clone();
        d["accessors"][0]["type"] = json!(ty);
        d["accessors"][0]["componentType"] = json!(ct);
        d["accessors"][0]["count"] = json!(1);
        d["accessors"][0].as_object_mut().map(|a| (a.remove("min"), a.remove("max")));
        let r = no_panic("padded matrix", &glb(&d, &bin));
        if ok {
            assert_ne!(r.err(), Some(Error("gltf: unsupported accessor type")), "{ty} {ct}");
        } else {
            assert_eq!(r.err(), Some(Error("gltf: unsupported accessor type")), "{ty} {ct}");
        }
    }

    // A .gltf whose buffer file is missing or short.
    let json = bytes("corpus.cf.gltf");
    assert!(Gltf::from_gltf(&json, &mut |_| None).is_err());
    assert!(Gltf::from_gltf(&json, &mut |_| Some(vec![0; 16])).is_err());
    assert!(Gltf::from_gltf(b"{", &mut |_| None).is_err());

    // An accessor with no buffer view (all zeros) can claim any count: one
    // too large to allocate is an error from the read, never an abort. The
    // counts are chosen so the test itself never allocates (the size
    // overflows before any memory is asked for).
    for (count, ty) in [(1u64 << 61, "VEC4"), (1u64 << 62, "VEC4"), (u64::MAX / 4 + 1, "SCALAR")] {
        let doc = json!({
            "asset": {"version": "2.0"},
            "accessors": [{"componentType": 5126, "count": count, "type": ty}],
        });
        let json = serde_json::to_vec(&doc).unwrap();
        let g = Gltf::from_gltf(&json, &mut |_| None).unwrap();
        assert_eq!(g.read_f32(0).err(), Some(Error("gltf: accessor too large")), "count {count} {ty}");
    }
}

#[test]
fn anim_source_is_off_in_game_builds() {
    // Only the converter may turn `anim-source` on, and no game, kit, look,
    // engine crate or xtask command does today: scan every manifest and the
    // xtask sources for the feature name.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    let mut scanned = 0;
    for dir in ["apps", "kits", "looks", "crates", "xtask"] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if p.is_dir() {
                    if !matches!(name.as_str(), "target" | "node_modules" | "pkg" | "dist" | ".git") {
                        stack.push(p);
                    }
                    continue;
                }
                let in_xtask = dir == "xtask" && name.ends_with(".rs");
                if name != "Cargo.toml" && !in_xtask {
                    continue;
                }
                // This crate declares the feature; that is the one allowed place.
                if p.parent().is_some_and(|x| x.ends_with("evani-gltf")) {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                if text.contains("anim-source") {
                    hits.push(p.display().to_string());
                }
            }
        }
    }
    assert!(scanned > 20, "scanned only {scanned} files");
    assert!(hits.is_empty(), "anim-source is turned on in: {hits:?}");
    // The test build itself (a stand-in for a game build) has it off unless
    // the converter's test run asks for it.
    if !cfg!(feature = "anim-source") {
        let src = load("corpus.src.glb");
        assert_eq!(format!("{src:?}").matches("animations").count(), 0, "animations parsed without anim-source");
    }
}

#[cfg(feature = "anim-source")]
#[test]
fn anim_source_reads_channels_and_samplers() {
    for name in ["corpus.src.glb", "corpus.q.glb", "corpus.qc.glb"] {
        let g = load(name);
        assert_eq!(g.animations.len(), 1, "{name}");
        let a = &g.animations[0];
        assert_eq!(a.name, "wave");
        assert_eq!(a.channels.len(), 2);
        let mut paths: Vec<_> = a.channels.iter().map(|c| (c.path.as_str(), c.node.map(|n| g.nodes[n].name.as_str()))).collect();
        paths.sort();
        assert_eq!(paths, [("rotation", Some("bone_tip")), ("translation", Some("body"))], "{name}");
        for c in &a.channels {
            let s = &a.samplers[c.sampler];
            assert_eq!(s.interpolation, "LINEAR");
            // gltfpack resamples to 30 frames a second (31 keys); the
            // source has 3. Check the key nearest the midpoint.
            let t = g.read_f32(s.input).expect("times");
            assert!(t.len() >= 3 && t.last().is_some_and(|&e| (e - 1.0).abs() < 1e-3), "{name}: times {t:?}");
            let k = (0..t.len()).min_by(|&a, &b| (t[a] - 0.5).abs().total_cmp(&(t[b] - 0.5).abs())).unwrap_or(0);
            assert!((t[k] - 0.5).abs() < 1e-3);
            let v = g.read_f32(s.output).expect("values");
            if c.path == "rotation" {
                // The tip turns 30 degrees about z at the midpoint.
                let h = (std::f32::consts::PI / 12.0).sin();
                assert!((v[k * 4 + 2] - h).abs() < 0.01, "{name}: rotation z {}", v[k * 4 + 2]);
            } else {
                assert!((v[k * 3 + 1] - 0.7).abs() < 0.01, "{name}: bob height {}", v[k * 3 + 1]);
            }
        }
    }
}
