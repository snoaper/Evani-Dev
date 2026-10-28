//! Skinned models on the CPU side (feature `skinning`, S3-9; turned on only
//! by a game's `anim` module). [`super::prepare`] reads skinned meshes too:
//! each skinned part gets a second vertex stream, [`SKIN_BYTES`] per vertex
//! (four joint indices as u8 and four weights as unorm8 summing to 255), its
//! node becomes the identity (glTF ignores a skinned mesh node's transform)
//! and the model keeps its bind matrices, `world(joint) x inverse bind`, the
//! pose a draw without joints shows. A kit draws a pose with
//! [`Models::draw_posed`], giving evani-anim's skinning matrices.
//!
//! The sun cone (Lite and Low, [`super::sun`]) is baked in bind pose: the
//! bake sees the skinned parts through [`proxies`] (skinned into model
//! space with the bind matrices) and [`unproxy`] carries each cone back into
//! its vertex's own space, so the shader turns it with the limb.

use evani_gltf::{Error, Gltf};

use super::{ModelInstance, Models, Part, Prepared, SHARED, VERTEX_BYTES};

/// Bytes per vertex of a skinned part's skin stream: joints (u8x4), weights
/// (unorm8x4).
pub const SKIN_BYTES: usize = 8;
/// A rig with more joints than this draws, but the converter report and
/// `cargo xtask check` warn: the soft cap of STAGE3_PLAN S3-9.
pub const SOFT_JOINTS: usize = 64;
/// Joints a skin can have at all (u8 joint indices).
pub const MAX_JOINTS: usize = 256;

/// An affine matrix as its three rows, row-major (evani-anim's `Mat43`):
/// `[m00 m01 m02 tx, m10 m11 m12 ty, m20 m21 m22 tz]`.
pub type Mat43 = [f32; 12];

const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

// Short error texts in the wasm (each long text is bytes in every download).
#[cfg(target_arch = "wasm32")]
macro_rules! err {
    ($code:literal, $text:literal) => {
        Error($code)
    };
}
#[cfg(not(target_arch = "wasm32"))]
macro_rules! err {
    ($code:literal, $text:literal) => {
        Error($text)
    };
}

/// This frame's posed draws: every [`Models::draw_posed`] call's matrices,
/// one after another, and each call's `(first, count)`. `offsets` is the
/// joint buffer's first matrix of each draw (as `Models::draws`), set by the
/// GPU side each frame ([`joint_offsets`]) and written into each instance by
/// `Models::stage` ([`set_joints`]).
#[derive(Debug, Default)]
pub(crate) struct Poses {
    pub(crate) mats: Vec<Mat43>,
    pub(crate) ranges: Vec<(u32, u32)>,
    pub(crate) offsets: Vec<u32>,
}

impl Poses {
    pub(crate) fn clear(&mut self) {
        self.mats.clear();
        self.ranges.clear();
        self.offsets.clear();
    }

    /// The matrices of a draw's `joints` ([`ModelInstance::joints`]), if
    /// it is posed with exactly `count` of them.
    pub(crate) fn of(&self, joints: u32, count: usize) -> Option<&[Mat43]> {
        let &(first, n) = self.ranges.get((joints as usize).checked_sub(1)?)?;
        if n as usize != count {
            return None;
        }
        self.mats.get(first as usize..first as usize + count)
    }
}

/// One frame's joint buffer: every draw's first matrix in `mats`
/// (`offsets`, in draw order; 0 for a model without a skin). A posed draw
/// gets its own matrices; every other draw of a skinned model shares one
/// copy of its bind palette (`bind_of`, by model index). `bind_at` is
/// scratch. Returns true when a posed draw had the wrong matrix count (it
/// draws the bind pose). Only the GPU side (browser) and tests call it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn joint_offsets<'a>(
    draws: &[ModelInstance],
    poses: &Poses,
    bind_of: impl Fn(usize) -> Option<&'a [Mat43]>,
    mats: &mut Vec<Mat43>,
    offsets: &mut Vec<u32>,
    bind_at: &mut Vec<(usize, u32)>,
) -> bool {
    mats.clear();
    offsets.clear();
    bind_at.clear();
    let mut wrong = false;
    for d in draws {
        let mi = d.model.0 as usize;
        let Some(bind) = bind_of(mi).filter(|b| !b.is_empty()) else {
            offsets.push(0);
            continue;
        };
        let at = mats.len() as u32;
        if let Some(m) = poses.of(d.joints, bind.len()) {
            mats.extend_from_slice(m);
            offsets.push(at);
            continue;
        }
        wrong |= d.joints != 0;
        match bind_at.iter().find(|b| b.0 == mi) {
            Some(&(_, o)) => offsets.push(o),
            None => {
                bind_at.push((mi, at));
                mats.extend_from_slice(bind);
                offsets.push(at);
            }
        }
    }
    wrong
}

/// The instance word that holds a draw's first joint matrix: the position
/// offset's w ([`super::instance_words`] writes 0 there; the skinned vertex
/// stages read `in.pos_offset.w`).
pub const JOINT_WORD: usize = 19;

/// Point the instance just written at the end of `words` at its first
/// joint matrix.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn set_joints(words: &mut [f32], first: u32) {
    let n = words.len();
    if let Some(w) = n.checked_sub(super::INSTANCE_WORDS - JOINT_WORD).and_then(|i| words.get_mut(i)) {
        *w = first as f32;
    }
}

impl Models {
    /// Draw a skinned model in a pose this frame: `joints` are its skinning
    /// matrices (evani-anim's `Rig::skinning`), one per joint of the model's
    /// skin, in the skin's joint order. A draw with the wrong number draws
    /// the bind pose (and the error log says so once); [`Models::draw`]
    /// always draws the bind pose.
    pub fn draw_posed(&mut self, mut i: ModelInstance, joints: &[Mat43]) {
        // The frame's first draw starts the paint list afresh, as in
        // `Models::draw_with` (else a frame opening with a posed draw keeps
        // the last frame's paints and the list grows until it is full).
        if self.draws.is_empty() {
            self.paints.clear();
        }
        self.skin.ranges.push((self.skin.mats.len() as u32, joints.len() as u32));
        self.skin.mats.extend_from_slice(joints);
        i.joints = self.skin.ranges.len() as u32;
        self.draws.push(i);
    }
}

/// Column-major 4x4 to three rows.
fn rows(c: &[f32; 16]) -> Mat43 {
    [c[0], c[4], c[8], c[12], c[1], c[5], c[9], c[13], c[2], c[6], c[10], c[14]]
}

/// The skinned meshes of `g` (what the static loop of [`super::prepare`]
/// skips): their parts, the bind matrices, and the bounding sphere again in
/// bind pose. One skin per model.
pub(super) fn add(g: &Gltf, world: &[[f32; 16]], level: &impl Fn(usize) -> Option<u8>, mat_of: &mut [Option<usize>], out: &mut Prepared) -> Result<(), Error> {
    let levels = out.lod_parts.len();
    let mut skin = None;
    for (ni, node) in g.nodes.iter().enumerate() {
        let (Some(si), Some((mesh, m))) = (node.skin, node.mesh.and_then(|m| g.meshes.get(m).map(|x| (m, x)))) else {
            continue;
        };
        let Some(lvl) = level(ni) else {
            continue;
        };
        if *skin.get_or_insert(si) != si {
            return Err(err!("K1", "model: more than one skin (one per model)"));
        }
        let joints = g.skins.get(si).map_or(0, |s| s.joints.len());
        for (pi, p) in m.primitives.iter().enumerate() {
            let mi = p.material.filter(|&m| m < g.materials.len()).unwrap_or(g.materials.len());
            let mat = match mat_of.get(mi).copied().flatten() {
                Some(i) => i,
                None => {
                    out.materials.push(super::material(g, g.materials.get(mi), &mut out.textures));
                    if let Some(s) = mat_of.get_mut(mi) {
                        *s = Some(out.materials.len() - 1);
                    }
                    out.materials.len() - 1
                }
            };
            if let Some(mut pt) = super::part(g, p, IDENTITY, mat, [ni, mesh, pi])? {
                pt.skin = weights(g, p, pt.vertices.len() / VERTEX_BYTES, joints)?;
                let tris = pt.indices.len() as u32 / 3;
                pt.lod = lvl;
                if let Some(k) = Some(usize::from(lvl)).filter(|&k| k < levels) {
                    out.lod_triangles[k] = out.lod_triangles[k].saturating_add(tris);
                    out.lod_parts[k] += 1;
                }
                if lvl == SHARED || lvl == 0 {
                    out.triangles = out.triangles.saturating_add(tris);
                }
                out.parts.push(pt);
            }
        }
    }
    let Some(s) = skin.and_then(|si| g.skins.get(si)) else {
        return Ok(());
    };
    if s.joints.len() > MAX_JOINTS {
        return Err(err!("K2", "model: a skin with more than 256 joints"));
    }
    let ibm = match s.inverse_bind_matrices {
        Some(a) => {
            let v = g.read_f32(a)?;
            if v.len() != s.joints.len() * 16 {
                return Err(err!("K3", "model: not one inverse bind matrix per joint"));
            }
            v
        }
        None => IDENTITY.repeat(s.joints.len()),
    };
    out.bind = s
        .joints
        .iter()
        .zip(ibm.as_chunks::<16>().0)
        .map(|(&j, b)| rows(&evani_gltf::mul(world.get(j).unwrap_or(&IDENTITY), b)))
        .collect();
    // The skinned parts' boxes are in mesh space: the sphere again, over
    // the bind pose.
    let boxes: Vec<Part> = out.parts.iter().map(|p| bind_box(p, &out.bind)).collect();
    (out.centre, out.radius) = super::sphere(&boxes);
    Ok(())
}

/// JOINTS_0 and WEIGHTS_0 of `n` vertices as the skin stream; the weights
/// renormalised to sum to exactly 255. Only the first four influences are
/// read (the converter keeps four).
fn weights(g: &Gltf, p: &evani_gltf::Primitive, n: usize, joints: usize) -> Result<Vec<u8>, Error> {
    let (Some(ja), Some(wa)) = (p.attribute("JOINTS_0"), p.attribute("WEIGHTS_0")) else {
        return Err(err!("K4", "model: a skinned mesh without JOINTS_0 and WEIGHTS_0"));
    };
    let j = g.read_u32(ja)?;
    let w = super::read(g, wa, 4, n)?;
    if j.len() != n * 4 || j.iter().any(|&x| x as usize >= joints) {
        return Err(err!("K5", "model: a joint index past the skin's joints"));
    }
    let mut out = vec![0u8; n * SKIN_BYTES];
    for ((o, jv), wv) in out.as_chunks_mut::<SKIN_BYTES>().0.iter_mut().zip(j.as_chunks::<4>().0).zip(w.as_chunks::<4>().0) {
        let sum: f32 = wv.iter().map(|x| x.max(0.0)).sum();
        let mut q = if sum > 0.0 { wv.map(|x| (x.max(0.0) / sum * 255.0).round() as u8) } else { [255, 0, 0, 0] };
        // Rounding can miss 255 by a step or two: the largest takes it up.
        let total: i32 = q.iter().map(|&x| i32::from(x)).sum();
        let big = (0..4).fold(0, |b, k| if q[k] > q[b] { k } else { b });
        q[big] = (i32::from(q[big]) + 255 - total).clamp(0, 255) as u8;
        o[..4].copy_from_slice(&jv.map(|x| x as u8));
        o[4..].copy_from_slice(&q);
    }
    Ok(out)
}

/// A vertex's blended skinning matrix (weights as the GPU reads them).
pub(crate) fn blend(bind: &[Mat43], sk: &[u8]) -> Mat43 {
    let mut m = [0.0f32; 12];
    for k in 0..4 {
        let w = f32::from(sk[4 + k]) / 255.0;
        if let Some(j) = bind.get(usize::from(sk[k])).filter(|_| w > 0.0) {
            for (a, b) in m.iter_mut().zip(j) {
                *a += w * b;
            }
        }
    }
    m
}

fn apply(m: &Mat43, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r * 4] * p[0] + m[r * 4 + 1] * p[1] + m[r * 4 + 2] * p[2] + m[r * 4 + 3])
}

/// A part's vertices skinned with `bind` into model space (positions as the
/// GPU dequantises them).
pub(crate) fn bind_positions(part: &Part, bind: &[Mat43]) -> Vec<[f32; 3]> {
    part.vertices
        .as_chunks::<VERTEX_BYTES>()
        .0
        .iter()
        .zip(part.skin.as_chunks::<SKIN_BYTES>().0)
        .map(|(v, sk)| {
            let l = [0, 1, 2].map(|k| part.pos_offset[k] + part.pos_scale[k] * f32::from(u16::from_le_bytes([v[k * 2], v[k * 2 + 1]])) / 65535.0);
            apply(&blend(bind, sk), l)
        })
        .collect()
}

/// A part without its bytes, standing for its box in model space (static
/// parts as they are; skinned ones over their bind-pose positions).
fn bind_box(p: &Part, bind: &[Mat43]) -> Part {
    let (mut node, mut lo, mut ext) = (p.node, p.pos_offset, p.pos_scale);
    if !p.skin.is_empty() {
        let pos = bind_positions(p, bind);
        let flat: Vec<f32> = pos.iter().flatten().copied().collect();
        (lo, ext) = super::bounds::<3>(&flat);
        node = IDENTITY;
    }
    Part {
        vertices: Vec::new(),
        indices: Vec::new(),
        node,
        pos_scale: ext,
        pos_offset: lo,
        uv: p.uv,
        material: p.material,
        tangents: p.tangents,
        source: p.source,
        lod: p.lod,
        tags: p.tags,
        skin: Vec::new(),
    }
}

/// What the cone bake reads for a model's parts: skinned parts skinned with
/// `bind` into model space (positions quantised again over their new box,
/// normals turned by each vertex's blended matrix, node the identity), the
/// rest as they are. Also returns each skinned part's own vertex bytes (in
/// part order, `None` for static parts), which [`unproxy`] writes the cones
/// into. Nothing to do (an empty list) for a model without skinned parts.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn proxies(mut parts: Vec<Part>, bind: &[Mat43]) -> (Vec<Part>, Vec<Option<Vec<u8>>>) {
    if parts.iter().all(|p| p.skin.is_empty()) {
        return (parts, Vec::new());
    }
    let mut keep = Vec::with_capacity(parts.len());
    for p in &mut parts {
        if p.skin.is_empty() {
            keep.push(None);
            continue;
        }
        let pos = bind_positions(p, bind);
        let flat: Vec<f32> = pos.iter().flatten().copied().collect();
        let (lo, ext) = super::bounds::<3>(&flat);
        let mut v = p.vertices.clone();
        for ((o, q), sk) in v.as_chunks_mut::<VERTEX_BYTES>().0.iter_mut().zip(&pos).zip(p.skin.as_chunks::<SKIN_BYTES>().0) {
            for k in 0..3 {
                o[k * 2..k * 2 + 2].copy_from_slice(&super::unorm16(q[k], lo[k], ext[k]).to_le_bytes());
            }
            let n = cof(&blend(bind, sk), [0, 1, 2].map(|k| f32::from(o[8 + k] as i8) / 127.0));
            for (d, x) in o[8..11].iter_mut().zip(n) {
                *d = super::snorm8(x);
            }
        }
        keep.push(Some(core::mem::replace(&mut p.vertices, v)));
        (p.pos_offset, p.pos_scale, p.node) = (lo, ext, IDENTITY);
    }
    (parts, keep)
}

/// `n` turned by `m`'s cofactor (as the shader turns normals), normalised.
#[cfg(any(target_arch = "wasm32", test))]
fn cof(m: &Mat43, n: [f32; 3]) -> [f32; 3] {
    let c = |k: usize| [m[k], m[4 + k], m[8 + k]];
    let (c0, c1, c2) = (c(0), c(1), c(2));
    let x = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let (a, b, d) = (x(c1, c2), x(c2, c0), x(c0, c1));
    let s = if c0[0] * a[0] + c0[1] * a[1] + c0[2] * a[2] < 0.0 { -1.0 } else { 1.0 };
    norm([0, 1, 2].map(|r| (a[r] * n[0] + b[r] * n[1] + d[r] * n[2]) * s))
}

#[cfg(any(target_arch = "wasm32", test))]
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l > 1e-12 { a.map(|x| x / l) } else { [0.0; 3] }
}

/// The baked proxies back as the parts they stand for: each skinned
/// vertex's cone (model space in the proxy) carried into its own space with
/// the transpose of its blended bind matrix (the inverse of how the shader
/// turns it, up to scale) and written into the part's own bytes from
/// `keep`. Static parts come back as they are.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn unproxy(mut baked: Vec<Part>, keep: &mut Vec<Option<Vec<u8>>>, bind: &[Mat43]) -> Vec<Part> {
    for (p, k) in baked.iter_mut().zip(keep.iter_mut()) {
        let Some(mut own) = k.take() else {
            continue;
        };
        for ((o, b), sk) in own.as_chunks_mut::<VERTEX_BYTES>().0.iter_mut().zip(p.vertices.as_chunks::<VERTEX_BYTES>().0).zip(p.skin.as_chunks::<SKIN_BYTES>().0) {
            let c = [b[6], b[7], b[11]].map(|x| f32::from(x as i8) / 127.0);
            let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            let m = blend(bind, sk);
            let local = norm([0, 1, 2].map(|k| m[k] * c[0] + m[4 + k] * c[1] + m[8 + k] * c[2]));
            [o[6], o[7], o[11]] = local.map(|x| super::snorm8(x * l));
        }
        p.vertices = own;
    }
    keep.clear();
    baked
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

    use super::*;
    use crate::model::{INSTANCE_WORDS, ModelId, count_draws, instance_words, prepare, sun};
    use crate::world::RenderWorld;
    use evani_core::overlay::RenderStats;

    fn fixture(name: &str) -> Gltf {
        let path = format!("{}/../../tests/fixtures/models/{name}", env!("CARGO_MANIFEST_DIR"));
        Gltf::from_glb(&std::fs::read(&path).unwrap()).unwrap()
    }

    fn bytes(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/../../tests/fixtures/models/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn skinned(p: &Prepared) -> Vec<&Part> {
        p.parts.iter().filter(|x| !x.skin.is_empty()).collect()
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// The skin stream is what the pipeline and the shaders read: joints
    /// as the file's JOINTS_0 wherever a weight is above 0, weights as
    /// unorm8 summing to exactly 255 and within rounding of the file's
    /// (normalised), SKIN_BYTES per vertex at locations 11 and 12, the first
    /// matrix in the instance's position-offset w. A skinned part's node is
    /// the identity and the model keeps one bind matrix per joint.
    #[test]
    fn skin_stream_is_what_the_shader_reads() {
        let g = fixture("rig.src.glb");
        let p = prepare(&g).unwrap();
        let parts = skinned(&p);
        assert_eq!(parts.len(), 1, "the rig's tube");
        assert_eq!(p.bind.len(), g.skins[0].joints.len());
        let part = parts[0];
        assert_eq!(part.node, IDENTITY);
        let prim = &g.meshes[part.source[1]].primitives[part.source[2]];
        let j = g.read_u32(prim.attribute("JOINTS_0").unwrap()).unwrap();
        let w = g.read_f32(prim.attribute("WEIGHTS_0").unwrap()).unwrap();
        let n = part.vertices.len() / VERTEX_BYTES;
        assert_eq!(part.skin.len(), n * SKIN_BYTES);
        for (i, sk) in part.skin.as_chunks::<SKIN_BYTES>().0.iter().enumerate() {
            let q = &sk[4..];
            assert_eq!(q.iter().map(|&x| u32::from(x)).sum::<u32>(), 255, "vertex {i}");
            let sum: f32 = w[i * 4..i * 4 + 4].iter().sum();
            for k in 0..4 {
                let want = w[i * 4 + k] / sum;
                assert!((f32::from(q[k]) / 255.0 - want).abs() <= 2.5 / 255.0, "vertex {i} weight {k}");
                if want > 0.0 {
                    assert_eq!(u32::from(sk[k]), j[i * 4 + k], "vertex {i} joint {k}");
                }
            }
        }
        // The pipeline's vertex formats and stride, and the shaders' inputs.
        let gpu: String = include_str!("../gpu/models/skin.rs").split_whitespace().collect();
        assert!(gpu.contains("vertex_attr_array![11=>Uint8x4,12=>Unorm8x4]"), "skin vertex formats changed");
        assert!(gpu.contains("array_stride:SKIN_BYTESasu64"));
        for wgsl in [include_str!("../../shaders/model_skin.wgsl"), include_str!("../../shaders/model_skin_shadows.wgsl")] {
            assert!(wgsl.contains("@location(11) j: vec4<u32>, @location(12) wt: vec4<f32>"));
            assert!(wgsl.contains("let b = u32(in.pos_offset.w);"));
            assert!(wgsl.contains("var<storage, read> joints: array<mat3x4<f32>>;"));
        }
        // Word 19 is the position offset's w, which instance_words leaves 0.
        let mut words = Vec::new();
        instance_words(&ModelInstance::new(ModelId(0), [1.0, 2.0, 3.0]), part, &[], &mut words);
        assert_eq!(&words[16..19], &part.pos_offset);
        assert_eq!(words[JOINT_WORD], 0.0);
        // The model check warns and refuses at these same counts.
        let guide = include_str!("../../../../tools/evani-guide/src/modelcheck.rs");
        assert!(guide.contains(&format!("pub const SOFT_JOINTS: usize = {SOFT_JOINTS};")) && guide.contains(&format!("pub const MAX_JOINTS: usize = {MAX_JOINTS};")));
    }

    fn mats(k: f32, n: usize) -> Vec<Mat43> {
        (0..n).map(|j| [k + j as f32; 12]).collect()
    }

    /// Each draw's first joint matrix: a posed draw gets its own copy, every
    /// unposed draw of a skinned model shares one bind palette, a static
    /// model gets 0, a posed draw with the wrong count falls back to the
    /// bind palette (and says so); the offset lands in word 19 of the
    /// instance just written; and the frame's end clears the poses.
    #[test]
    fn joint_offsets_per_instance() {
        let mut m = Models::default();
        let rig = m.add_glb("models/rig.glb", &bytes("rig.src.glb"));
        let kart = m.add_glb("models/kart.low.glb", &bytes("kart.low.glb"));
        let bind = prepare(&fixture("rig.src.glb")).unwrap().bind;
        let n = bind.len();
        let (a, b) = (mats(100.0, n), mats(200.0, n));
        m.draw_posed(ModelInstance::new(rig, [0.0; 3]), &a);
        m.draw(ModelInstance::new(rig, [1.0, 0.0, 0.0]));
        m.draw(ModelInstance::new(kart, [2.0, 0.0, 0.0]));
        m.draw(ModelInstance::new(rig, [3.0, 0.0, 0.0]));
        m.draw_posed(ModelInstance::new(rig, [4.0, 0.0, 0.0]), &b);
        m.draw_posed(ModelInstance::new(rig, [5.0, 0.0, 0.0]), &a[..n - 1]);
        let bind_of = |mi: usize| (mi == rig.0 as usize).then_some(bind.as_slice());
        let (mut out, mut offsets, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
        let wrong = joint_offsets(&m.draws, &m.skin, bind_of, &mut out, &mut offsets, &mut scratch);
        let n32 = n as u32;
        assert_eq!(offsets, [0, n32, 0, n32, 2 * n32, n32]);
        assert!(wrong, "the last draw had one matrix too few");
        assert_eq!(out.len(), 3 * n, "two poses and one bind palette");
        assert_eq!((&out[..n], &out[n..2 * n], &out[2 * n..]), (a.as_slice(), bind.as_slice(), b.as_slice()));
        // Without the wrong draw nothing is reported.
        m.draws.pop();
        assert!(!joint_offsets(&m.draws, &m.skin, bind_of, &mut out, &mut offsets, &mut scratch));
        // set_joints points only the instance just written.
        let part = prepare(&fixture("rig.src.glb")).unwrap().parts.remove(0);
        let mut words = Vec::new();
        for d in &m.draws[..2] {
            instance_words(d, &part, &[], &mut words);
        }
        set_joints(&mut words, 2 * n32);
        assert_eq!((words[JOINT_WORD], words[INSTANCE_WORDS + JOINT_WORD]), (0.0, (2 * n) as f32));
        // Models::stage (what the GPU side stages) writes each draw's own
        // offset into each of the model's instances, in draw order.
        assert_eq!(part.lod, SHARED);
        m.skin.offsets = offsets.clone();
        words.clear();
        m.stage(rig.0 as usize, &part, &mut words);
        let staged: Vec<f32> = words.chunks(INSTANCE_WORDS).map(|w| w[JOINT_WORD]).collect();
        assert_eq!(staged, [0.0, n as f32, n as f32, (2 * n) as f32], "the rig's four draws (the kart's is not staged)");
        // The native frame end clears this frame's poses with the draws.
        let mut s = RenderStats::default();
        count_draws(&mut m, &RenderWorld::default(), None, &mut s);
        assert!(m.draws.is_empty() && m.skin.mats.is_empty() && m.skin.ranges.is_empty() && m.skin.offsets.is_empty());
        assert!(s.triangles > 0);
        m.draw_posed(ModelInstance::new(rig, [0.0; 3]), &a);
        assert_eq!(m.draws[0].joints, 1, "pose numbers restart each frame");
    }

    /// S3-11's hidden nodes and recolours reach skinned parts as static
    /// ones: the skinned body carries its node tags, a draw hiding it stages
    /// nothing, a painted draw gets its tint, and every staged instance keeps
    /// its own joint offset. A frame that opens with a posed draw starts the
    /// paint list afresh.
    #[test]
    fn skinned_parts_hide_and_paint() {
        let mut m = Models::default();
        let rig = m.add_glb("models/rig.glb", &bytes("rig.src.glb"));
        let body = m.node_bit(rig, "body").expect("the skinned mesh node is named");
        let p = prepare(&fixture("rig.src.glb")).unwrap();
        let part = p.parts.iter().find(|x| !x.skin.is_empty()).unwrap().clone();
        assert_ne!(part.tags & body, 0, "the skinned part is under its node");
        let n = p.bind.len();
        let a = mats(100.0, n);
        let red = [0.5, 0.25, 1.0, 1.0];
        for frame in 0..2 {
            m.draw_posed(ModelInstance::new(rig, [0.0; 3]).hide(body), &a);
            m.draw_posed(ModelInstance::new(rig, [1.0, 0.0, 0.0]), &a);
            m.draw_with(ModelInstance::new(rig, [2.0, 0.0, 0.0]), &[(body, red)]);
            assert_eq!(m.paints.len(), 1, "frame {frame}: one paint, not last frame's too");
            let bind_of = |mi: usize| (mi == rig.0 as usize).then_some(p.bind.as_slice());
            let (mut mats_out, mut offsets, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
            assert!(!joint_offsets(&m.draws, &m.skin, bind_of, &mut mats_out, &mut offsets, &mut scratch));
            m.skin.offsets = offsets;
            let mut words = Vec::new();
            m.stage(rig.0 as usize, &part, &mut words);
            assert_eq!(words.len(), 2 * INSTANCE_WORDS, "the hidden draw stages nothing");
            let w: Vec<&[f32]> = words.chunks(INSTANCE_WORDS).collect();
            assert_eq!((w[0][JOINT_WORD], w[1][JOINT_WORD]), (n as f32, (2 * n) as f32), "posed, then the bind palette");
            assert_eq!((&w[0][24..], &w[1][24..]), (&[1.0; 4][..], &red[..]), "only the painted draw is recoloured");
            count_draws(&mut m, &RenderWorld::default(), None, &mut RenderStats::default());
        }
    }

    /// The native half of `bind_pose_draws_like_the_static_mesh`: the rig's
    /// tube skinned with its bind palette is the file's mesh (positions
    /// within a quantisation step), and in model space it is what the
    /// static copy (`rig.static.glb`, the rig without its skin) prepares:
    /// positions, normals and the bounding sphere.
    #[test]
    fn bind_pose_matches_the_static_mesh() {
        let g = fixture("rig.src.glb");
        let p = prepare(&g).unwrap();
        let part = skinned(&p)[0];
        let pos = bind_positions(part, &p.bind);
        let prim = &g.meshes[part.source[1]].primitives[part.source[2]];
        let want = g.read_f32(prim.attribute("POSITION").unwrap()).unwrap();
        let world = g.world_matrices()[part.source[0]];
        for (i, q) in pos.iter().enumerate() {
            let w = [0, 1, 2].map(|r| world[r] * want[i * 3] + world[4 + r] * want[i * 3 + 1] + world[8 + r] * want[i * 3 + 2] + world[12 + r]);
            for k in 0..3 {
                assert!((q[k] - w[k]).abs() <= part.pos_scale[k] / 65535.0 + 1e-5, "vertex {i}: {q:?} vs {w:?}");
            }
        }
        let st = prepare(&fixture("rig.static.glb")).unwrap();
        assert!(skinned(&st).is_empty() && st.bind.is_empty(), "the static copy has no skin");
        let (sp, sn) = sun::model_space(&st.parts[0]);
        let (prox, _) = proxies(p.parts.clone(), &p.bind);
        let (kp, kn) = sun::model_space(&prox[0]);
        assert_eq!(sp.len(), kp.len());
        for i in 0..sp.len() {
            for k in 0..3 {
                assert!((sp[i][k] - kp[i][k]).abs() <= 2.0 * st.parts[0].pos_scale[k] / 65535.0 + 1e-5, "vertex {i}");
            }
            assert!(dot(sn[i], kn[i]) > 0.999, "normal {i}");
        }
        assert!((st.radius - p.radius).abs() < 1e-3 && (0..3).all(|k| (st.centre[k] - p.centre[k]).abs() < 1e-3), "the same bounding sphere");
    }

    /// The sun cone through proxies and back: the cone the bake writes in
    /// model space (on the proxy) is the cone the shader turns the vertex's
    /// own back into (`cof(blend) x transpose(blend) = det(blend)`), on a
    /// palette far from the identity (turns, a scale and a translation),
    /// threshold kept; the part's other bytes are its own; static parts are
    /// left as baked.
    #[test]
    fn sun_cone_survives_the_bind_pose_round_trip() {
        let g = fixture("rig.src.glb");
        let mut p = prepare(&g).unwrap();
        // Joint k: a turn of 30 + 25k degrees about z, scaled 1.5, moved.
        p.bind = (0..p.bind.len())
            .map(|k| {
                let (s, c) = (30.0 + 25.0 * k as f32).to_radians().sin_cos();
                [1.5 * c, -1.5 * s, 0.0, 0.3, 1.5 * s, 1.5 * c, 0.0, -0.2, 0.0, 0.0, 1.5, 0.1]
            })
            .collect();
        let mut parts = prepare(&fixture("kart.low.glb")).unwrap().parts;
        let statics = parts.len();
        parts.extend(p.parts.iter().cloned());
        let own = p.parts[0].vertices.clone();
        let (mut prox, mut keep) = proxies(parts, &p.bind);
        assert_eq!(keep.len(), prox.len());
        assert!(keep[..statics].iter().all(Option::is_none) && keep[statics].is_some());
        assert_eq!(prox[statics].node, IDENTITY);
        sun::bake(&mut prox);
        let baked: Vec<([f32; 3], f32)> = prox[statics].vertices.as_chunks::<VERTEX_BYTES>().0.iter().map(|v| sun::decode(v)).collect();
        let kart_baked = prox[0].vertices.clone();
        let back = unproxy(prox, &mut keep, &p.bind);
        assert!(keep.is_empty());
        assert_eq!(back[0].vertices, kart_baked, "a static part is left as baked");
        let part = &back[statics];
        let mut lit = 0;
        for (i, ((v, o), sk)) in part.vertices.as_chunks::<VERTEX_BYTES>().0.iter().zip(own.as_chunks::<VERTEX_BYTES>().0).zip(part.skin.as_chunks::<SKIN_BYTES>().0).enumerate() {
            for k in (0..VERTEX_BYTES).filter(|k| ![6, 7, 11].contains(k)) {
                assert_eq!(v[k], o[k], "vertex {i} byte {k} is the part's own");
            }
            let (want, wt) = baked[i];
            let (local, t) = sun::decode(v);
            assert!((t - wt).abs() <= 3.0 / 127.0, "vertex {i} threshold {t} vs {wt}");
            if dot(want, want) < 0.5 {
                continue;
            }
            lit += 1;
            let got = cof(&blend(&p.bind, sk), local);
            assert!(dot(got, want) > 0.98, "vertex {i}: {got:?} vs {want:?}");
        }
        assert!(lit > part.vertices.len() / VERTEX_BYTES / 2, "most vertices have a cone");
    }
}
