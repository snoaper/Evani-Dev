//! The engine's own WGSL (forward, sprite, tonemap, present) and the CPU
//! side of their data layouts: the forward frame uniform, mesh instances and
//! sprite instances. Pure data, so the layouts are tested natively against
//! the WGSL (naga) and the GPU code only copies words.

use bytemuck::Zeroable;
use evani_core::camera::CameraPose;
use evani_core::plugin::MaterialDesc;

use crate::math::{self, Mat4};
use crate::world::{ATLAS_GRID, MeshInstance, Sprite};

/// One engine WGSL file.
#[derive(Debug, Clone, Copy)]
pub struct EngineShader {
    /// Shader id and `// evani.shader.<name>` marker.
    pub name: &'static str,
    /// Graph pass that runs it.
    pub pass: &'static str,
    pub source: &'static str,
    /// Size of its uniform struct in bytes.
    pub uniform_bytes: u32,
}

/// The forward WGSL plus its shadow-map lookup: the real one with the
/// `shadows` feature, a one-line stub without (so a game without shadows
/// ships none of the sampling code).
#[cfg(all(feature = "shadows", not(feature = "models")))]
const FORWARD_SOURCE: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/forward_shadows.wgsl")
);
#[cfg(not(any(feature = "shadows", feature = "models")))]
const FORWARD_SOURCE: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/forward_noshadow.wgsl")
);

pub const FORWARD: EngineShader = EngineShader {
    name: "forward",
    pass: "forward",
    #[cfg(all(not(any(feature = "shadow-cascades", feature = "pcss")), not(all(feature = "look-surface", not(feature = "relief")))))] source: FORWARD_SOURCE, #[cfg(all(any(feature = "shadow-cascades", feature = "pcss"), not(all(feature = "look-surface", not(feature = "relief")))))] source: FORWARD_EXT, #[cfg(all(feature = "look-surface", not(feature = "relief")))] source: FORWARD_LIT,
    uniform_bytes: FRAME_WORDS as u32 * 4,
};

/// The sprite WGSL plus its fragment stage: with `sprite-art` it also reads
/// pixel-art frames at level 0 on texel centres (the atlas has mips then);
/// without it, the plain sample (the same text as before the split).
#[cfg(all(feature = "sprite-art", not(feature = "msdf"), not(feature = "lights2d")))]
const SPRITE_SOURCE: &str = concat!(
    include_str!("../shaders/sprite.wgsl"),
    include_str!("../shaders/sprite_fs_art.wgsl")
); #[cfg(feature = "msdf")] const SPRITE_SOURCE: &str = utf8(&SPRITE_MSDF_BYTES);
#[cfg(all(not(feature = "sprite-art"), not(feature = "msdf")))]
const SPRITE_SOURCE: &str = concat!(
    include_str!("../shaders/sprite.wgsl"),
    include_str!("../shaders/sprite_fs.wgsl")
);

pub const SPRITE: EngineShader = EngineShader {
    name: "sprite",
    pass: "sprites",
    source: SPRITE_SOURCE,
    uniform_bytes: SPRITE_UNIFORM_BYTES,
};

pub const TONEMAP: EngineShader = EngineShader {
    name: "tonemap",
    pass: "tonemap",
    source: include_str!("../shaders/tonemap.wgsl"),
    uniform_bytes: 16,
};

pub const PRESENT: EngineShader = EngineShader {
    name: "present",
    pass: "present",
    source: include_str!("../shaders/present.wgsl"),
    uniform_bytes: 16,
};

/// The engine shaders a build compiles, indexed by the `gpu::pipelines`
/// constants. `FORWARD` is in the list only with the 3D path (feature
/// `forward`); without it the forward WGSL never reaches the download.
#[cfg(feature = "forward")]
pub const ENGINE_SHADERS: [EngineShader; 4] = [FORWARD, SPRITE, TONEMAP, PRESENT];
#[cfg(not(feature = "forward"))]
pub const ENGINE_SHADERS: [EngineShader; 3] = [SPRITE, TONEMAP, PRESENT];

/// Shadow-map depth pass (feature `shadows`): no slots, built once at
/// start-up, not part of [`ENGINE_SHADERS`].
#[cfg(feature = "shadows")]
pub const SHADOWS_DEPTH: EngineShader = EngineShader {
    name: "shadows_depth",
    pass: "shadows",
    source: include_str!("../shaders/shadows_depth.wgsl"),
    uniform_bytes: SHADOW_WORDS as u32 * 4,
};

/// Shadow depth uniform: light view-projection (16) + 16 unlit flags.
pub const SHADOW_WORDS: usize = 32;
/// Blob shadows the forward uniform holds; later ones are dropped.
pub const MAX_BLOBS: usize = 32;

/// Materials the forward uniform holds; higher indices clamp to the last.
pub const MAX_MATERIALS: usize = 16;
/// Where the shadow/blob tail of the frame uniform starts (after the
/// materials).
pub const SHADING_AT: usize = 16 + 5 * 4 + MAX_MATERIALS * 8;
/// Forward frame uniform: view_proj (16) + 5 vec4 + 16 materials x 2 vec4,
/// then the shading tail: light_vp (16) + shadow + blob_info + blobs, and
/// last the shadow tint (rgb + strength).
pub const FRAME_WORDS: usize = SHADING_AT + 16 + 4 + 4 + MAX_BLOBS * 4 + 4;
/// Mesh instance: pos+material, rotation, scale, colour (64 bytes).
pub const INSTANCE_WORDS: usize = 16;
/// Sprite instance: rect, uv rect, colour, rotation (64 bytes).
pub const SPRITE_WORDS: usize = 16;

/// Near and far clip planes (metres).
pub const NEAR: f32 = 0.1;
pub const FAR: f32 = 1000.0;

/// Light and background values of one frame (from the render.* params).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// Direction towards the light (any length).
    pub light_dir: [f32; 3],
    pub light_color: [f32; 3],
    pub intensity: f32,
    pub ambient: [f32; 3],
    pub fog_color: [f32; 3],
}

/// The shadow and blob part of one frame (the same for every view).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shading<'a> {
    /// Light view-projection of the shadow map (unused when it is off).
    pub light_vp: Mat4,
    /// Shadow map on (it then replaces the blobs).
    pub shadow_map: bool,
    /// How dark the shadow map's shadow is (0..1).
    pub strength: f32,
    /// One shadow-map texel in uv.
    pub texel: f32,
    /// PCF radius in texels (0 = one hardware-filtered tap, 1 = 3x3).
    pub pcf_radius: u32,
    /// Normal offset (m) and depth bias of the shadow-map lookup.
    pub normal_offset: f32,
    pub bias: f32,
    /// Blob shadows `[x, y, z, radius]` (used when the shadow map is off).
    pub blobs: &'a [[f32; 4]],
    pub blob_strength: f32,
    /// Shadow tint: linear RGB the shadows lean towards and how far (0..1).
    /// Strength 0 (the default) leaves shadows exactly as they were.
    pub tint: [f32; 3],
    pub tint_strength: f32,
}

impl Default for Shading<'_> {
    fn default() -> Self {
        Self {
            light_vp: math::IDENTITY,
            shadow_map: false,
            strength: 1.0,
            texel: 0.0,
            pcf_radius: 0,
            normal_offset: 0.0,
            bias: 0.0,
            blobs: &[],
            blob_strength: 0.0,
            tint: [1.0; 3],
            tint_strength: 0.0,
        }
    }
}

/// Fill the shading tail of the frame uniform (after [`write_frame`],
/// once per frame: every view shares it). No allocation.
pub fn write_shading(out: &mut [f32; FRAME_WORDS], s: &Shading) {
    let at = SHADING_AT;
    out[at..at + 16].copy_from_slice(&s.light_vp);
    out[at + 16..at + 20].copy_from_slice(&[
        if s.shadow_map { 1.0 } else { 0.0 },
        s.strength,
        s.texel,
        s.pcf_radius as f32,
    ]);
    let blobs = if s.shadow_map { &[][..] } else { &s.blobs[..s.blobs.len().min(MAX_BLOBS)] };
    out[at + 20..at + 24].copy_from_slice(&[blobs.len() as f32, s.blob_strength, s.normal_offset, s.bias]);
    let b = at + 24;
    out[b..].fill(0.0);
    for (i, v) in blobs.iter().enumerate() {
        out[b + i * 4..b + i * 4 + 4].copy_from_slice(v);
    }
    // The shadow tint is the last vec4 of the uniform.
    let t = FRAME_WORDS - 4;
    out[t..].copy_from_slice(&[s.tint[0], s.tint[1], s.tint[2], s.tint_strength.clamp(0.0, 1.0)]);
}

/// The shadow depth pass's uniform: `light_vp` and an unlit flag per
/// material slot (unlit materials cast no shadow).
pub fn write_shadow_uniform(out: &mut [f32; SHADOW_WORDS], light_vp: &Mat4, materials: &[MaterialDesc]) {
    out[..16].copy_from_slice(light_vp);
    for i in 0..MAX_MATERIALS {
        let unlit = materials.get(i).or(materials.last()).is_some_and(|m| m.unlit);
        out[16 + i] = if unlit { 1.0 } else { 0.0 };
    }
}

/// Light view-projection of a square shadow box: `half` metres around
/// `centre` across, `depth` metres either side along the light, seen from
/// `light_dir` (towards the light). The box moves in whole texels of a
/// `res`-texel map, so shadow edges don't crawl as the camera moves.
pub fn light_view_proj(centre: [f32; 3], half: f32, depth: f32, light_dir: [f32; 3], res: u32) -> Mat4 {
    let d = math::normalize(light_dir);
    let d = if d == [0.0; 3] { [0.0, 1.0, 0.0] } else { d };
    let up = if d[1].abs() > 0.99 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] };
    // Fixed orientation through the origin; only the box moves.
    let view = math::look_at([0.0; 3], [-d[0], -d[1], -d[2]], up);
    let c = math::transform_point(&view, centre);
    let half = half.max(0.5);
    let texel = 2.0 * half / res.max(1) as f32;
    let cx = (c[0] / texel).round() * texel;
    let cy = (c[1] / texel).round() * texel;
    // View space looks down -z: the centre sits at distance -c.z.
    let near = -c[2] - depth;
    let far = -c[2] + depth;
    let proj = math::ortho(cx - half, cx + half, cy - half, cy + half, near, far);
    math::mul(&proj, &view)
}

/// Projection * view for `camera` at `aspect` (width / height).
pub fn view_proj(camera: &CameraPose, aspect: f32) -> Mat4 {
    let proj = math::perspective(camera.fov_y_deg.to_radians(), aspect, NEAR, FAR); #[cfg(feature = "camera-range")] let proj = { let _ = proj; let (n, f) = crate::camera_range::current(); math::perspective(camera.fov_y_deg.to_radians(), aspect, n, f) };
    let view = math::look_at(camera.eye, camera.target, camera.up);
    math::mul(&proj, &view)
}

/// Fill the forward pass's frame uniform. No allocation.
pub fn write_frame(
    out: &mut [f32; FRAME_WORDS],
    view_proj: &Mat4,
    eye: [f32; 3],
    l: &Lighting,
    materials: &[MaterialDesc],
) {
    out[..16].copy_from_slice(view_proj);
    let d = math::normalize(l.light_dir);
    let vec4s = [
        [eye[0], eye[1], eye[2], 1.0],
        [d[0], d[1], d[2], 0.0],
        [
            l.light_color[0] * l.intensity,
            l.light_color[1] * l.intensity,
            l.light_color[2] * l.intensity,
            0.0,
        ],
        [l.ambient[0], l.ambient[1], l.ambient[2], 0.0],
        [l.fog_color[0], l.fog_color[1], l.fog_color[2], 1.0],
    ];
    for (i, v) in vec4s.iter().enumerate() {
        out[16 + i * 4..20 + i * 4].copy_from_slice(v);
    }
    for i in 0..MAX_MATERIALS {
        let at = 36 + i * 8;
        // No look material: plain white, lit.
        let (base, e, unlit) = match materials.get(i).or(materials.last()) {
            Some(m) => (m.base_color, m.emissive, m.unlit),
            None => ([1.0; 4], [0.0; 3], false),
        };
        out[at..at + 4].copy_from_slice(&base);
        out[at + 4..at + 8].copy_from_slice(&[e[0], e[1], e[2], if unlit { 1.0 } else { 0.0 }]);
    }
}

/// One mesh instance as the forward shader reads it (64 bytes, the layout of
/// [`INSTANCE_WORDS`] little-endian floats). `Pod`, so a whole slice goes to
/// the GPU as bytes in one copy.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    pub position: [f32; 3],
    /// Material index as a float (the fourth component of `position`).
    pub material: f32,
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub pad: f32,
    pub color: [f32; 4],
}

/// One sprite as the sprite shader reads it (64 bytes, [`SPRITE_WORDS`]
/// floats): rect, uv rect, colour, rotation.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteRaw {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub rotation: f32,
    pub pad: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<InstanceRaw>() == INSTANCE_WORDS * 4);
const _: () = assert!(std::mem::size_of::<SpriteRaw>() == SPRITE_WORDS * 4);

/// One mesh instance as the forward shader reads it.
#[inline]
pub fn instance_raw(m: &MeshInstance) -> InstanceRaw {
    InstanceRaw {
        position: m.position,
        material: m.material.min(MAX_MATERIALS as u32 - 1) as f32,
        rotation: m.rotation,
        scale: m.scale,
        pad: 0.0,
        color: m.color,
    }
}

/// Mesh instance as plain words (the layout reference used by tests).
pub fn pack_instance(m: &MeshInstance) -> [f32; INSTANCE_WORDS] {
    bytemuck::cast(instance_raw(m))
}

/// Pack `src` into `out`, grouped by mesh kind (kind 0 first, submission
/// order kept inside a kind: a counting sort, two passes, no allocation once
/// `out` has grown). `counts[k]` becomes the number of kind-`k` instances;
/// instances of an unknown kind are dropped, as the draw loop never reads
/// them. `out` is cleared first.
pub fn pack_instances<const K: usize>(src: &[MeshInstance], out: &mut Vec<InstanceRaw>, counts: &mut [u32; K]) {
    pack_instances_with_decals(src, &[], out, counts);
}

/// Depth offset of a decal, in normalised device depth (the forward shader
/// pulls a decal's depth this much toward the camera; the sign comes from
/// [`crate::depth::toward_camera`]). Worth about `offset * z^2 / near` of
/// world depth at distance z whichever way depth runs: enough that a line
/// lying on (or 1-2 cm above) a road never z-fights it at any distance,
/// small enough that real geometry a little in front still hides the decal
/// (at 50 m the offset is about 0.1 m of world depth, at 100 m about 0.4 m).
pub const DECAL_DEPTH_OFFSET: f32 = 4e-6;

/// [`pack_instances`] plus decals: `decals` are packed into the same runs
/// (after the meshes of their kind) with their instance pad set to
/// [`DECAL_DEPTH_OFFSET`] toward the camera ([`crate::depth::toward_camera`]),
/// which the forward shader adds to the clip depth. Same counting sort, no allocation once `out` has grown.
pub fn pack_instances_with_decals<const K: usize>(
    src: &[MeshInstance],
    decals: &[MeshInstance],
    out: &mut Vec<InstanceRaw>,
    counts: &mut [u32; K],
) {
    *counts = [0; K];
    for m in src.iter().chain(decals) {
        if let Some(c) = counts.get_mut(usize::from(m.mesh.0)) {
            *c += 1;
        }
    }
    let total: usize = counts.iter().map(|&c| c as usize).sum();
    out.clear();
    out.resize(total, InstanceRaw::zeroed());
    let mut next = [0usize; K];
    let mut at = 0;
    for (n, &c) in next.iter_mut().zip(counts.iter()) {
        *n = at;
        at += c as usize;
    }
    let tagged = src.iter().map(|m| (m, 0.0)).chain(decals.iter().map(|m| (m, crate::depth::toward_camera(DECAL_DEPTH_OFFSET))));
    for (m, pad) in tagged {
        if let Some(n) = next.get_mut(usize::from(m.mesh.0)) {
            if let Some(slot) = out.get_mut(*n) {
                *slot = instance_raw(m);
                slot.pad = pad;
            }
            *n += 1;
        }
    }
}

/// Shift a view-projection by a sub-pixel amount: `jitter_px` `[x, y]` in
/// pixels of a `size` target (x to the right, y down, as in the picture).
/// Live captures use it to render the same view twice, half a pixel apart,
/// so geocheck can tell flicker (z-fighting) from plain edges. Column-major:
/// row `r` of column `c` is `m[c * 4 + r]`; adding `k * row3` to row 0 moves
/// every clip-space x by `k * w`, i.e. NDC x by `k`.
pub fn jitter_view_proj(m: &Mat4, jitter_px: [f32; 2], size: [u32; 2]) -> Mat4 {
    let kx = 2.0 * jitter_px[0] / size[0].max(1) as f32;
    let ky = -2.0 * jitter_px[1] / size[1].max(1) as f32;
    let mut o = *m;
    for c in 0..4 {
        o[c * 4] += kx * m[c * 4 + 3];
        o[c * 4 + 1] += ky * m[c * 4 + 3];
    }
    o
}

/// The uv rectangle `[u0, v0, u1, v1]` of atlas cell `cell` (row-major,
/// half a texel inset so neighbours never bleed in with linear filtering).
pub fn atlas_uv(cell: u16, atlas_px: u32) -> [f32; 4] {
    let n = ATLAS_GRID;
    let c = u32::from(cell) % (n * n);
    let (x, y) = ((c % n) as f32, (c / n) as f32);
    let step = 1.0 / n as f32;
    let inset = 0.5 / atlas_px.max(1) as f32;
    [
        x * step + inset,
        y * step + inset,
        (x + 1.0) * step - inset,
        (y + 1.0) * step - inset,
    ]
}

/// Number of atlas cells.
pub const ATLAS_CELLS: usize = (ATLAS_GRID * ATLAS_GRID) as usize;

/// The uv rectangle of every atlas cell, computed once per frame.
pub fn atlas_uv_table(atlas_px: u32) -> [[f32; 4]; ATLAS_CELLS] {
    std::array::from_fn(|c| atlas_uv(c as u16, atlas_px))
}

/// The uv table of an atlas texture `tex` pixels wide and high that holds the
/// built-in cells in its top-left `ATLAS_PX` square and a kit's sprite sheet
/// from `sheet_origin` (see [`crate::world::SpriteSheet`]): the built-in
/// cells first (always inset half a texel, as [`atlas_uv`]), then one entry
/// per sheet frame. Sheet frames use exact edges when `nearest` (crisp pixel
/// art: every sample hits a texel centre inside the frame) and the half-texel
/// inset otherwise. `out` is cleared first. Built once per sheet or filter
/// change, not per frame.
pub fn sprite_uv_table(tex: [u32; 2], sheet_origin: [u32; 2], frames: &[[u32; 4]], nearest: bool, out: &mut Vec<[f32; 4]>) {
    let (tw, th) = (tex[0].max(1) as f32, tex[1].max(1) as f32);
    let px = crate::atlas::ATLAS_CELL_PX;
    out.clear();
    if tex == [crate::atlas::ATLAS_PX; 2] {
        out.extend_from_slice(&atlas_uv_table(crate::atlas::ATLAS_PX));
    } else {
        for c in 0..ATLAS_CELLS as u32 {
            let (x, y) = ((c % ATLAS_GRID * px) as f32, (c / ATLAS_GRID * px) as f32);
            let s = px as f32;
            out.push([(x + 0.5) / tw, (y + 0.5) / th, (x + s - 0.5) / tw, (y + s - 0.5) / th]);
        }
    }
    let inset = if nearest { 0.0 } else { 0.5 };
    for f in frames {
        let x = (sheet_origin[0] + f[0]) as f32;
        let y = (sheet_origin[1] + f[1]) as f32;
        let (w, h) = (f[2] as f32, f[3] as f32);
        out.push([(x + inset) / tw, (y + inset) / th, (x + w - inset) / tw, (y + h - inset) / th]);
    }
}

/// One sprite as the sprite shader reads it, with a uv table from
/// [`atlas_uv_table`] or [`sprite_uv_table`]. A cell past the table wraps
/// onto the built-in cells.
#[inline]
pub fn sprite_raw(s: &Sprite, uvs: &[[f32; 4]]) -> SpriteRaw {
    let i = usize::from(s.cell);
    let uv = match uvs.get(i) {
        Some(uv) => *uv,
        None => uvs.get(i % ATLAS_CELLS).copied().unwrap_or([0.0; 4]),
    };
    SpriteRaw {
        pos: s.pos,
        size: s.size,
        uv,
        color: s.color,
        rotation: s.rotation,
        pad: [0.0; 3],
    }
}

/// Pack `src` into `out` in order (`out` is cleared first and keeps its
/// capacity).
pub fn pack_sprites(src: &[Sprite], atlas_px: u32, out: &mut Vec<SpriteRaw>) {
    pack_sprites_uv(src, &atlas_uv_table(atlas_px), out);
}

/// [`pack_sprites`] with a prepared uv table ([`sprite_uv_table`]).
pub fn pack_sprites_uv(src: &[Sprite], uvs: &[[f32; 4]], out: &mut Vec<SpriteRaw>) {
    out.clear();
    out.extend(src.iter().map(|s| sprite_raw(s, uvs)));
}

/// One sprite as plain words (the layout reference used by tests).
pub fn pack_sprite(s: &Sprite, atlas_px: u32) -> [f32; SPRITE_WORDS] {
    bytemuck::cast(sprite_raw(s, &atlas_uv_table(atlas_px)))
}

/// Full-screen pass writing the nearest of the four depth samples to
/// `frag_depth`: the single-sample `depth` target effects read. Nearest is
/// the largest depth ([`crate::depth::nearer`]: reversed depth).
pub const DEPTH_RESOLVE_WGSL: &str = "
@group(0) @binding(0) var depth_ms: texture_depth_multisampled_2d;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) p: vec4f) -> @builtin(frag_depth) f32 {
    let c = vec2i(p.xy);
    var d = textureLoad(depth_ms, c, 0);
    for (var s = 1; s < 4; s++) {
        d = max(d, textureLoad(depth_ms, c, s));
    }
    return d;
}
";

#[cfg(test)] #[allow(clippy::items_after_test_module, reason = "new declarations are appended at the end of this file so no shipped line moves")]
mod tests {
    use super::*;
    use crate::mesh::MeshId;
    use crate::splice::{Bodies, slot_names, splice};
    use evani_core::slots::ShaderSlots;

    fn validate(name: &str, src: &str) -> naga::Module {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        module
    }

    /// The screen-door fade as the shaders run it, on the CPU: the forward
    /// pass keeps a faded shape's pixel where its alpha reaches the 2x2
    /// threshold and marks it (alpha 0), and the tonemap pass takes four
    /// bilinear taps on each pixel's corners (a 3x3 tent) wherever one of
    /// them sees a mark. Every fade level comes out an even blend of the shape
    /// and what is behind it, with no dots left; pixels away from any faded
    /// shape stay sharp.
    #[test]
    fn a_screen_door_fade_resolves_to_an_even_blend() {
        // The thresholds of forward.wgsl's pattern, by (x & 1, y & 1).
        let th = |x: usize, y: usize| match (x & 1, y & 1) {
            (0, 0) => 0.125,
            (1, 0) => 0.625,
            (0, 1) => 0.875,
            _ => 0.375,
        };
        assert!(FORWARD.source.contains("select(select(0.125, 0.625, q.x == 1u), select(0.875, 0.375, q.x == 1u), q.y == 1u)"));
        assert!(FORWARD.source.contains("select(1.0, 0.0, in.color.a < 0.999)"));
        assert!(TONEMAP.source.contains("let marked = min(min(t0.a, t1.a), min(t2.a, t3.a)) < 0.999;"));
        const N: usize = 16;
        for (alpha, kept) in [(0.25f32, 0.25f32), (0.5, 0.5), (0.75, 0.75), (0.9, 1.0)] {
            // A faded shape (colour 1) over the middle of a sharp backdrop
            // (colour 0 with a lone bright pixel far from it).
            let mut img = [[(0.0f32, 1.0f32); N]; N];
            img[1][1] = (5.0, 1.0);
            for (y, row) in img.iter_mut().enumerate().take(13).skip(5) {
                for (x, px) in row.iter_mut().enumerate().take(13).skip(5) {
                    if alpha >= th(x, y) {
                        *px = (1.0, 0.0);
                    }
                }
            }
            let at = |x: i32, y: i32| img[y.clamp(0, N as i32 - 1) as usize][x.clamp(0, N as i32 - 1) as usize];
            // A bilinear tap on the corner up-left of (x, y): its 2x2 mean.
            let corner = |x: i32, y: i32| {
                let p = [at(x - 1, y - 1), at(x, y - 1), at(x - 1, y), at(x, y)];
                (p.iter().map(|q| q.0).sum::<f32>() / 4.0, p.iter().map(|q| q.1).sum::<f32>() / 4.0)
            };
            let out = |x: i32, y: i32| {
                let t = [corner(x, y), corner(x + 1, y), corner(x, y + 1), corner(x + 1, y + 1)];
                if t.iter().any(|c| c.1 < 0.999) { t.iter().map(|c| c.0).sum::<f32>() / 4.0 } else { at(x, y).0 }
            };
            for y in 7..11 {
                for x in 7..11 {
                    assert!((out(x, y) - kept).abs() < 1e-6, "alpha {alpha}: {} at ({x}, {y})", out(x, y));
                }
            }
            assert_eq!(out(1, 1), 5.0, "the backdrop stays sharp");
        }
    }

    #[test]
    fn engine_shaders_validate_with_every_body_set() {
        let mut slots = ShaderSlots::with_builtins(); #[cfg(feature = "pcss")] slots.declare(crate::shadow_ext::PCSS_PARAMS); #[cfg(feature = "relief")] slots.declare(evani_core::slots::MICRO_DETAIL); #[cfg(all(feature = "look-surface", not(feature = "relief")))] { slots.declare(crate::model::surface::SPECULAR); slots.declare(crate::model::surface::SURFACE_PARAMS); }
        // The build's list holds the forward shader only with `forward`;
        // every engine shader validates either way.
        assert_eq!(ENGINE_SHADERS.iter().any(|s| s.name == "forward"), cfg!(feature = "forward"));
        for sh in [FORWARD, SPRITE, TONEMAP, PRESENT] {
            assert!(sh.source.starts_with(&format!("// evani.shader.{}", sh.name)), "{}", sh.name);
            for bodies in [Bodies::Current, Bodies::EngineDefaults] {
                let s = splice(sh.source, &slots, bodies).unwrap();
                let module = validate(sh.name, &if sh.name == "forward" { s.source + &lit_test_stubs() } else { s.source });
                let size = module
                    .global_variables
                    .iter()
                    .find(|(_, v)| v.space == naga::AddressSpace::Uniform)
                    .map(|(_, v)| module.types[v.ty].inner.size(module.to_ctx()));
                assert_eq!(size, Some(sh.uniform_bytes), "{} uniform size", sh.name);
            }
        }
        // A look body for tonemap still validates.
        slots
            .set_default("tonemap", "    return color / (color + vec3<f32>(1.0));", "look.t")
            .unwrap();
        validate("tonemap", &splice(TONEMAP.source, &slots, Bodies::Current).unwrap().source);
    }

    #[test]
    fn depth_resolve_validates() {
        validate("depth_resolve", DEPTH_RESOLVE_WGSL);
    }

    #[test]
    fn the_depth_resolve_keeps_the_nearest_sample() {
        // depth::nearer is max under reversed depth; the WGSL must agree.
        let keep = if crate::depth::nearer(0.25, 0.75) == 0.75 { "max(" } else { "min(" };
        assert!(DEPTH_RESOLVE_WGSL.contains(&format!("d = {keep}d, textureLoad(depth_ms, c, s));")));
    }

    #[test]
    fn passes_use_the_required_slots() {
        let s = |sh: EngineShader| slot_names(sh.source).collect::<Vec<_>>();
        // The soft-shadow tail (feature `pcss`) is declared last and adds one slot.
        let mut want: Vec<&str> = if cfg!(feature = "relief") { vec!["toon_ramp", "fog", "rim", "micro_detail"] } else if cfg!(all(feature = "look-surface", not(feature = "relief"))) { vec!["toon_ramp", "fog", "rim", "specular", "surface_params"] } else { vec!["toon_ramp", "fog", "rim"] };
        if cfg!(feature = "pcss") {
            want.push("pcss_params");
        }
        assert_eq!(s(FORWARD), want);
        assert_eq!(s(TONEMAP), ["exposure", "tonemap"]);
        assert!(s(SPRITE).is_empty() && s(PRESENT).is_empty());
    }

    #[test]
    fn frame_uniform_layout() {
        let mut out = [0.0; FRAME_WORDS];
        let mut glow = MaterialDesc::new("glow", [1.0, 0.5, 0.0, 1.0]);
        glow.emissive = [4.0, 2.0, 0.5];
        glow.unlit = true;
        let mats = [MaterialDesc::new("a", [0.2, 0.3, 0.4, 1.0]), glow];
        let l = Lighting {
            light_dir: [0.0, 2.0, 0.0],
            light_color: [1.0, 0.5, 0.25],
            intensity: 2.0,
            ambient: [0.1; 3],
            fog_color: [0.3; 3],
        };
        write_frame(&mut out, &math::IDENTITY, [1.0, 2.0, 3.0], &l, &mats);
        assert_eq!(&out[16..20], &[1.0, 2.0, 3.0, 1.0]);
        assert_eq!(&out[20..23], &[0.0, 1.0, 0.0]);
        assert_eq!(&out[24..27], &[2.0, 1.0, 0.5]);
        assert_eq!(&out[36..40], &[0.2, 0.3, 0.4, 1.0]);
        assert_eq!(&out[44..52], &[1.0, 0.5, 0.0, 1.0, 4.0, 2.0, 0.5, 1.0]);
        // Unused material slots repeat the last material.
        assert_eq!(&out[SHADING_AT - 8..SHADING_AT], &out[44..52]);
        assert_eq!(SHADING_AT * 4, 656);
        assert_eq!(FRAME_WORDS * 4, 1280);
    }

    #[test]
    fn shading_tail_layout() {
        let mut out = [7.0; FRAME_WORDS];
        let blobs = [[1.0, 0.0, 2.0, 0.8], [3.0, 0.5, 4.0, 1.2]];
        let mut s = Shading {
            blobs: &blobs,
            blob_strength: 0.6,
            normal_offset: 0.05,
            bias: 0.002,
            ..Shading::default()
        };
        write_shading(&mut out, &s);
        let at = SHADING_AT;
        assert_eq!(&out[at..at + 16], &math::IDENTITY);
        assert_eq!(&out[at + 16..at + 20], &[0.0, 1.0, 0.0, 0.0]);
        assert_eq!(&out[at + 20..at + 24], &[2.0, 0.6, 0.05, 0.002]);
        assert_eq!(&out[at + 24..at + 32], &[1.0, 0.0, 2.0, 0.8, 3.0, 0.5, 4.0, 1.2]);
        assert!(out[at + 32..FRAME_WORDS - 4].iter().all(|&w| w == 0.0));
        // The shadow tint is the last vec4; off by default (strength 0).
        assert_eq!(&out[FRAME_WORDS - 4..], &[1.0, 1.0, 1.0, 0.0]);
        // The shadow map replaces the blobs.
        s.shadow_map = true;
        s.texel = 1.0 / 1024.0;
        s.pcf_radius = 1;
        write_shading(&mut out, &s);
        assert_eq!(&out[at + 16..at + 20], &[1.0, 1.0, 1.0 / 1024.0, 1.0]);
        assert_eq!(out[at + 20], 0.0);
        assert!(out[at + 24..FRAME_WORDS - 4].iter().all(|&w| w == 0.0));
        // A tint is written clamped to 0..1 strength.
        s.tint = [0.5, 0.25, 0.75];
        s.tint_strength = 2.0;
        write_shading(&mut out, &s);
        assert_eq!(&out[FRAME_WORDS - 4..], &[0.5, 0.25, 0.75, 1.0]);
        s.tint = [1.0; 3];
        s.tint_strength = 0.0;
        // More than MAX_BLOBS: the rest are dropped.
        let many = [[0.0, 0.0, 0.0, 1.0]; MAX_BLOBS + 5];
        s.shadow_map = false;
        s.blobs = &many;
        write_shading(&mut out, &s);
        assert_eq!(out[at + 20], MAX_BLOBS as f32);
    }

    #[test]
    fn shadow_uniform_flags_unlit_materials() {
        let mut glow = MaterialDesc::new("glow", [1.0; 4]);
        glow.unlit = true;
        let mats = [MaterialDesc::new("a", [1.0; 4]), glow];
        let mut out = [0.0; SHADOW_WORDS];
        write_shadow_uniform(&mut out, &math::IDENTITY, &mats);
        assert_eq!(&out[16..19], &[0.0, 1.0, 1.0]);
        assert_eq!(out[31], 1.0, "slots past the list repeat the last material");
    }

    #[test]
    fn light_box_covers_the_centre_and_snaps_to_texels() {
        let dir = [0.5, 1.0, 0.35];
        let centre = [10.0, 0.0, -4.0];
        let vp = light_view_proj(centre, 20.0, 60.0, dir, 1024);
        let c = math::transform_point(&vp, centre);
        assert!(c[0].abs() < 0.01 && c[1].abs() < 0.01, "{c:?}");
        assert!((c[2] - 0.5).abs() < 1e-3, "{c:?}");
        // Higher along the light is nearer the light (smaller depth).
        let up = math::transform_point(&vp, [10.0 + 0.5, 1.0, -4.0 + 0.35]);
        assert!(up[2] < c[2], "{up:?}");
        // 30 m away is outside the 20 m half box.
        let far = math::transform_point(&vp, [40.0, 0.0, -4.0]);
        assert!(far[0].abs() > 1.0 || far[1].abs() > 1.0, "{far:?}");
        // Moving less than half a texel keeps the same matrix.
        let texel = 40.0 / 1024.0;
        let nudged = light_view_proj([10.0 + texel * 0.1, 0.0, -4.0], 20.0, 60.0, dir, 1024);
        let a = math::transform_point(&vp, [0.0; 3]);
        let b = math::transform_point(&nudged, [0.0; 3]);
        assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5);
        // Straight-down light still works.
        let down = light_view_proj(centre, 20.0, 60.0, [0.0, 1.0, 0.0], 1024);
        let c = math::transform_point(&down, centre);
        assert!(c[0].abs() < 0.01 && c[1].abs() < 0.01 && (c[2] - 0.5).abs() < 1e-3, "{c:?}");
    }

    #[cfg(feature = "shadows")]
    #[test]
    fn shadow_depth_shader_validates() {
        let m = validate("shadows_depth", SHADOWS_DEPTH.source);
        assert!(SHADOWS_DEPTH.source.starts_with("// evani.shader.shadows_depth"));
        let size = m
            .global_variables
            .iter()
            .find(|(_, v)| v.space == naga::AddressSpace::Uniform)
            .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()));
        assert_eq!(size, Some(SHADOWS_DEPTH.uniform_bytes));
        assert!(FORWARD.source.contains("evani.shader.shadows_sample"));
    }

    #[cfg(not(feature = "shadows"))]
    #[test]
    fn forward_without_shadows_has_no_sampler() {
        assert!(!FORWARD.source.contains("sampler_comparison"));
        assert!(!FORWARD.source.contains("evani.shader.shadows"));
    }

    #[test]
    fn instance_and_sprite_packing() {
        let mut m = MeshInstance::new(MeshId::SPHERE, [1.0, 2.0, 3.0]);
        m.material = 99;
        let p = pack_instance(&m);
        assert_eq!(&p[..4], &[1.0, 2.0, 3.0, 15.0]);
        assert_eq!(&p[4..8], &[0.0, 0.0, 0.0, 1.0]);
        let uv = atlas_uv(5, 256);
        let inset = 0.5 / 256.0;
        assert_eq!(uv, [0.25 + inset, 0.25 + inset, 0.5 - inset, 0.5 - inset]);
        let s = Sprite {
            pos: [10.0, 20.0],
            size: [8.0, 8.0],
            rotation: 0.5,
            cell: 0,
            color: [1.0; 4],
            layer: 0,
        };
        let w = pack_sprite(&s, 256);
        assert_eq!(&w[..4], &[10.0, 20.0, 8.0, 8.0]);
        assert_eq!(w[12], 0.5);
    }

    #[test]
    fn sheet_uv_table_follows_the_built_in_cells() {
        let mut t = Vec::new();
        // No sheet, square atlas: exactly the built-in table.
        sprite_uv_table([256, 256], [0, 256], &[], false, &mut t);
        assert_eq!(&t[..], &atlas_uv_table(256)[..]);
        // A sheet below: built-in cells keep their pixels, frames follow.
        let frames = [[0, 0, 16, 16], [17, 0, 16, 8]];
        sprite_uv_table([256, 512], [0, 256], &frames, true, &mut t);
        assert_eq!(t.len(), ATLAS_CELLS + 2);
        assert_eq!(t[5], [64.5 / 256.0, 64.5 / 512.0, 127.5 / 256.0, 127.5 / 512.0]);
        assert_eq!(t[16], [0.0, 256.0 / 512.0, 16.0 / 256.0, 272.0 / 512.0]);
        assert_eq!(t[17], [17.0 / 256.0, 0.5, 33.0 / 256.0, 264.0 / 512.0]);
        // Linear: half a texel inset.
        sprite_uv_table([256, 512], [0, 256], &frames, false, &mut t);
        assert_eq!(t[16], [0.5 / 256.0, 256.5 / 512.0, 15.5 / 256.0, 271.5 / 512.0]);
        // Sheet cells pack their frame; a cell past the table wraps.
        let mut s = Sprite {
            pos: [0.0; 2],
            size: [-16.0, 16.0],
            rotation: 0.0,
            cell: crate::world::SpriteSheet::cell(1),
            color: [1.0; 4],
            layer: 0,
        };
        assert_eq!(sprite_raw(&s, &t).uv, t[17]);
        assert_eq!(sprite_raw(&s, &t).size, [-16.0, 16.0]);
        s.cell = 21;
        assert_eq!(sprite_raw(&s, &t).uv, t[5]);
    }

    #[test]
    fn view_proj_puts_target_in_view() {
        let cam = CameraPose {
            eye: [0.0, 5.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
        };
        let vp = view_proj(&cam, 16.0 / 9.0);
        let c = math::transform_point(&vp, [0.0; 3]);
        assert!(c[0].abs() < 1e-5 && c[1].abs() < 1e-5);
        assert!(c[2] > 0.0 && c[2] < 1.0, "{c:?}");
    }

    /// The pre-bulk packing, kept as the layout reference: floats pushed one
    /// by one, little-endian, in the order the shaders read them.
    fn old_instance_words(m: &MeshInstance) -> [f32; INSTANCE_WORDS] {
        let [px, py, pz] = m.position;
        let [qx, qy, qz, qw] = m.rotation;
        let [sx, sy, sz] = m.scale;
        let [r, g, b, a] = m.color;
        let mat = m.material.min(MAX_MATERIALS as u32 - 1) as f32;
        [px, py, pz, mat, qx, qy, qz, qw, sx, sy, sz, 0.0, r, g, b, a]
    }

    fn old_sprite_words(s: &Sprite, atlas_px: u32) -> [f32; SPRITE_WORDS] {
        let uv = atlas_uv(s.cell, atlas_px);
        let [r, g, b, a] = s.color;
        [s.pos[0], s.pos[1], s.size[0], s.size[1], uv[0], uv[1], uv[2], uv[3], r, g, b, a, s.rotation, 0.0, 0.0, 0.0]
    }

    fn le_bytes(words: &[f32]) -> Vec<u8> {
        let mut v = Vec::new();
        for w in words {
            v.extend_from_slice(&w.to_le_bytes());
        }
        v
    }

    #[test]
    fn bulk_sprite_bytes_match_the_old_layout() {
        let sprites: Vec<Sprite> = (0..40u16)
            .map(|i| Sprite {
                pos: [f32::from(i) * 3.5, -f32::from(i)],
                size: [8.0 + f32::from(i), 4.0],
                rotation: f32::from(i) * 0.1,
                cell: i, // past the last cell wraps, as atlas_uv does
                color: [0.1, 0.2 * f32::from(i % 5), 0.3, 0.5],
                layer: 0,
            })
            .collect();
        let mut out = Vec::new();
        pack_sprites(&sprites, 256, &mut out);
        let old: Vec<u8> = sprites.iter().flat_map(|s| le_bytes(&old_sprite_words(s, 256))).collect();
        assert_eq!(bytemuck::cast_slice::<SpriteRaw, u8>(&out), &old[..]);
        assert_eq!(out.len() * 64, old.len());
        // Reuse keeps the capacity and replaces the contents.
        let cap = out.capacity();
        pack_sprites(&sprites[..3], 256, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out.capacity(), cap);
    }

    #[test]
    fn bulk_instance_bytes_match_the_old_layout_and_group_by_kind() {
        let kinds = [MeshId::SPHERE, MeshId::CUBE, MeshId::CUBE, MeshId::PLANE, MeshId::SPHERE, MeshId::CUBE];
        let src: Vec<MeshInstance> = kinds
            .iter()
            .enumerate()
            .map(|(i, &k)| {
                let mut m = MeshInstance::new(k, [i as f32, 2.0 * i as f32, -1.5]);
                m.rotation = [0.0, 0.5, 0.0, 0.5];
                m.scale = [1.0 + i as f32, 2.0, 3.0];
                m.material = if i == 4 { 99 } else { i as u32 };
                m.color = [0.25, 0.5, 0.75, 1.0];
                m
            })
            .collect();
        let mut out = Vec::new();
        let mut counts = [0u32; 3];
        pack_instances(&src, &mut out, &mut counts);
        // The previous draw loop: for each kind in id order, the matching
        // instances in submission order.
        let mut old = Vec::new();
        let mut old_counts = [0u32; 3];
        for (id, c) in old_counts.iter_mut().enumerate() {
            for m in src.iter().filter(|m| usize::from(m.mesh.0) == id) {
                old.extend_from_slice(&le_bytes(&old_instance_words(m)));
                *c += 1;
            }
        }
        assert_eq!(counts, old_counts);
        assert_eq!(counts.iter().sum::<u32>(), 6);
        assert_eq!(bytemuck::cast_slice::<InstanceRaw, u8>(&out), &old[..]);
        // An unknown kind is dropped, like before.
        let mut odd = src.clone();
        odd.push(MeshInstance::new(MeshId(200), [0.0; 3]));
        pack_instances(&odd, &mut out, &mut counts);
        assert_eq!(counts, old_counts);
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn decals_join_their_kind_run_with_a_depth_offset() {
        let meshes = [
            MeshInstance::new(MeshId::CUBE, [1.0, 0.0, 0.0]),
            MeshInstance::new(MeshId::PLANE, [2.0, 0.0, 0.0]),
            MeshInstance::new(MeshId::CUBE, [3.0, 0.0, 0.0]),
        ];
        let decals = [
            MeshInstance::new(MeshId::PLANE, [4.0, 0.0, 0.0]),
            MeshInstance::new(MeshId::CUBE, [5.0, 0.0, 0.0]),
        ];
        let mut out = Vec::new();
        let mut counts = [0u32; 3];
        pack_instances_with_decals(&meshes, &decals, &mut out, &mut counts);
        assert_eq!(counts, [3, 0, 2]);
        let xs: Vec<(f32, f32)> = out.iter().map(|r| (r.position[0], r.pad)).collect();
        let d = crate::depth::toward_camera(DECAL_DEPTH_OFFSET);
        assert_eq!(xs, [(1.0, 0.0), (3.0, 0.0), (5.0, d), (2.0, 0.0), (4.0, d)]);
        // Without decals it is exactly pack_instances.
        let mut plain = Vec::new();
        pack_instances(&meshes, &mut plain, &mut counts);
        pack_instances_with_decals(&meshes, &[], &mut out, &mut counts);
        assert_eq!(plain, out);
        assert!(out.iter().all(|r| r.pad == 0.0));
    }

    #[test]
    fn decal_offset_beats_depth_precision_but_not_real_occluders() {
        use crate::depth::{is_nearer, toward_camera};
        // Depth of a point straight ahead at distance z (the forward
        // shader's NDC depth), and the decal offset toward the camera.
        let cam = CameraPose {
            eye: [0.0, 0.0, 0.0],
            target: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
        };
        let vp = view_proj(&cam, 16.0 / 9.0);
        let depth = |z: f32| math::transform_point(&vp, [0.0, 0.0, -z])[2];
        for z in [5.0f32, 25.0, 100.0, 300.0] {
            // A line 1 cm above a road (both seen head-on along the view
            // ray): the line wins by far more than one f32 step of its depth.
            let road = depth(z);
            let line = depth(z - 0.01) + toward_camera(DECAL_DEPTH_OFFSET);
            assert!(is_nearer(line, road), "z {z}: {road} vs {line}");
            assert!((line - road).abs() > 8.0 * f32::EPSILON * road.abs(), "z {z}: {road} vs {line}");
        }
        // A wall 1 m in front of the decal at 25 m still hides it.
        assert!(is_nearer(depth(24.0), depth(25.0) + toward_camera(DECAL_DEPTH_OFFSET)));
    }

    #[test]
    fn jitter_shifts_every_point_by_the_same_subpixel_amount() {
        let cam = CameraPose {
            eye: [3.0, 4.0, 10.0],
            target: [0.0, 1.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 55.0,
        };
        let size = [1280u32, 720];
        let vp = view_proj(&cam, 1280.0 / 720.0);
        let j = jitter_view_proj(&vp, [0.5, 0.25], size);
        let px = |m: &Mat4, p: [f32; 3]| {
            let n = math::transform_point(m, p);
            [(n[0] + 1.0) * 0.5 * size[0] as f32, (1.0 - n[1]) * 0.5 * size[1] as f32, n[2]]
        };
        for p in [[0.0, 0.0, 0.0], [5.0, -1.0, -20.0], [-2.0, 3.0, 4.0]] {
            let (a, b) = (px(&vp, p), px(&j, p));
            assert!((b[0] - a[0] - 0.5).abs() < 1e-3, "{a:?} {b:?}");
            assert!((b[1] - a[1] - 0.25).abs() < 1e-3, "{a:?} {b:?}");
            assert!((b[2] - a[2]).abs() < 1e-6);
        }
        assert_eq!(jitter_view_proj(&vp, [0.0, 0.0], size), vp);
    }
}

// Models (feature `models`, S3-3): the forward WGSL with the model entry
// points (model_vs, model_fs) in the same module, so the model pipeline is
// spliced and rebuilt with the same slot bodies. Declared after everything
// else in this file: panic locations are in the wasm, so other games'
// downloads stay byte-identical only if no line above moves.
#[cfg(all(feature = "models", feature = "shadows", not(feature = "relief"), not(feature = "skinning")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/forward_shadows.wgsl")
);
#[cfg(all(feature = "models", not(feature = "shadows"), not(feature = "relief"), not(feature = "skinning")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/forward_noshadow.wgsl")
);
// The models build ships it without comments (S3-3 size, about 2 KB gzip).
#[cfg(feature = "models")]
#[allow(clippy::large_const_arrays)] const FORWARD_BYTES: [u8; wgsl_strip(FORWARD_FULL.as_bytes(), &mut [])] = wgsl_stripped(FORWARD_FULL.as_bytes());
#[cfg(feature = "models")]
const FORWARD_SOURCE: &str = utf8(&FORWARD_BYTES);

/// The model shadow-map depth pass: the shadow uniform of [`SHADOWS_DEPTH`]
/// plus the model vertex stage (`model_shadow_vs`). Static WGSL, no slots;
/// shipped without comments like the forward source.
#[cfg(all(feature = "models", feature = "shadows", not(feature = "skinning")))]
const MODEL_SHADOWS_FULL: &str = concat!(
    include_str!("../shaders/shadows_depth.wgsl"),
    include_str!("../shaders/model_shadows.wgsl")
);
#[cfg(all(feature = "models", feature = "shadows"))]
const MODEL_SHADOWS_BYTES: [u8; wgsl_strip(MODEL_SHADOWS_FULL.as_bytes(), &mut [])] = wgsl_stripped(MODEL_SHADOWS_FULL.as_bytes());
#[cfg(all(feature = "models", feature = "shadows"))]
pub const MODEL_SHADOWS: &str = utf8(&MODEL_SHADOWS_BYTES);

/// Does `s[i..]` start with `p`?
#[cfg(any(feature = "models", feature = "msdf", feature = "shadow-cascades", feature = "pcss", feature = "lights2d"))]
const fn starts_at(s: &[u8], i: usize, p: &[u8]) -> bool {
    if i + p.len() > s.len() {
        return false;
    }
    let mut k = 0;
    while k < p.len() {
        if s[i + k] != p[k] {
            return false;
        }
        k += 1;
    }
    true
}

/// WGSL without its `//` comments (to the end of the line), at compile
/// time. Marker comments stay: `//@slot` lines (the splicer's) and
/// `// evani.` names (the ship build's and the tests' markers). Newlines
/// stay, so compiler line numbers still match the `.wgsl` files. WGSL has
/// no string literals, so `//` always starts a comment. Writes what fits
/// into `out` and returns the stripped length.
#[cfg(any(feature = "models", feature = "msdf", feature = "shadow-cascades", feature = "pcss", feature = "lights2d"))]
const fn wgsl_strip(s: &[u8], out: &mut [u8]) -> usize {
    let (mut i, mut n) = (0, 0);
    while i < s.len() {
        if starts_at(s, i, b"//") && !starts_at(s, i, b"//@") && !starts_at(s, i, b"// evani.") {
            while i < s.len() && s[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if n < out.len() {
            out[n] = s[i];
        }
        n += 1;
        i += 1;
    }
    n
}

#[cfg(any(feature = "models", feature = "msdf", feature = "shadow-cascades", feature = "pcss", feature = "lights2d"))]
const fn wgsl_stripped<const N: usize>(s: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let n = wgsl_strip(s, &mut out);
    assert!(n == N, "wgsl_strip: length changed");
    out
}

/// A compile-time check that the stripped bytes are still text.
#[cfg(any(feature = "models", feature = "msdf", feature = "shadow-cascades", feature = "pcss", feature = "lights2d"))]
const fn utf8(b: &'static [u8]) -> &'static str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("wgsl_strip: not UTF-8"),
    }
}

#[cfg(all(test, feature = "models"))]
mod strip_tests {
    use super::*;

    /// The shipped models WGSL keeps every line (compiler messages point at
    /// the right `.wgsl` line), every marker, and no other comment.
    #[test]
    fn models_wgsl_ships_without_comments_but_with_markers_and_lines() {
        #[cfg_attr(not(feature = "shadows"), allow(unused_mut))]
        let mut pairs = vec![(FORWARD_FULL, FORWARD_SOURCE)];
        #[cfg(feature = "shadows")]
        pairs.push((MODEL_SHADOWS_FULL, MODEL_SHADOWS));
        for (full, shipped) in pairs {
            assert!(shipped.len() < full.len());
            assert_eq!(full.lines().count(), shipped.lines().count());
            for (a, b) in full.lines().zip(shipped.lines()) {
                let code = a.find("//").map_or(a, |k| &a[..k]);
                if a.contains("//@") || a.contains("// evani.") {
                    assert_eq!(a, b);
                } else {
                    assert_eq!(code.trim_end(), b.trim_end(), "{a}");
                }
                assert!(!b.contains("//") || b.contains("//@") || b.contains("// evani."), "{b}");
            }
        }
        assert!(FORWARD_SOURCE.contains("// evani.shader.forward") && FORWARD_SOURCE.contains("// evani.shader.model"));
        assert_eq!(wgsl_strip(b"a // x\n//@slot t\n// evani.k\nb//\n", &mut []), "a \n//@slot t\n// evani.k\nb\n".len());
    }
}

// The `msdf` sprite source (it replaces the sprite WGSL in a `ui` game) also
// ships without comments, keeping its `// evani.shader.sprite_msdf` marker
// and every line (S3-13a size).
#[cfg(feature = "msdf")]
const SPRITE_MSDF_FULL: &str = include_str!("../shaders/sprite_msdf.wgsl");
#[cfg(feature = "msdf")]
const SPRITE_MSDF_BYTES: [u8; wgsl_strip(SPRITE_MSDF_FULL.as_bytes(), &mut [])] = wgsl_stripped(SPRITE_MSDF_FULL.as_bytes());

#[cfg(all(test, feature = "msdf"))]
mod msdf_strip_tests {
    use super::*;

    #[test]
    fn msdf_wgsl_ships_without_comments_but_with_marker_and_lines() {
        assert!(SPRITE_SOURCE.len() < SPRITE_MSDF_FULL.len());
        assert_eq!(SPRITE_SOURCE.lines().count(), SPRITE_MSDF_FULL.lines().count());
        assert!(SPRITE_SOURCE.starts_with("// evani.shader.sprite_msdf"));
        assert!(SPRITE_SOURCE.lines().skip(1).all(|l| !l.contains("//")));
        assert!(naga::front::wgsl::parse_str(SPRITE_SOURCE).is_ok());
    }
}

// Surface relief (feature `relief`, S3-10): the model WGSL plus its relief
// fragment stage (model_relief_fs, the micro_detail slot), stripped like
// the models source above. Declared last for the same reason.
#[cfg(all(feature = "relief", feature = "shadows", not(feature = "skinning")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/model_relief.wgsl"),
    include_str!("../shaders/forward_shadows.wgsl")
);
#[cfg(all(feature = "relief", not(feature = "shadows"), not(feature = "skinning")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/model_relief.wgsl"),
    include_str!("../shaders/forward_noshadow.wgsl")
);
// Skinned models (feature `skinning`, S3-9, turned on by a game's `anim`):
// the same modules with the skinned vertex stages after the static ones
// (model_skin_vs in the forward module, model_skin_shadow_vs in the model
// shadow module). Declared at the end so no line above moves.
#[cfg(all(feature = "skinning", feature = "shadows", not(feature = "relief")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/model_skin.wgsl"),
    include_str!("../shaders/forward_shadows.wgsl")
);
#[cfg(all(feature = "skinning", not(feature = "shadows"), not(feature = "relief")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/model_skin.wgsl"),
    include_str!("../shaders/forward_noshadow.wgsl")
);
#[cfg(all(feature = "skinning", feature = "shadows"))]
const MODEL_SHADOWS_FULL: &str = concat!(
    include_str!("../shaders/shadows_depth.wgsl"),
    include_str!("../shaders/model_shadows.wgsl"),
    include_str!("../shaders/model_skin_shadows.wgsl")
);
// Relief and skinning together: both extra stages in one module.
#[cfg(all(feature = "relief", feature = "skinning", feature = "shadows"))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/model_relief.wgsl"),
    include_str!("../shaders/model_skin.wgsl"),
    include_str!("../shaders/forward_shadows.wgsl")
);
#[cfg(all(feature = "relief", feature = "skinning", not(feature = "shadows")))]
const FORWARD_FULL: &str = concat!(
    include_str!("../shaders/forward.wgsl"),
    include_str!("../shaders/model.wgsl"),
    include_str!("../shaders/model_relief.wgsl"),
    include_str!("../shaders/model_skin.wgsl"),
    include_str!("../shaders/forward_noshadow.wgsl")
);

// Shadow cascades and soft shadows (S5-R4, features `shadow-cascades` and
// `pcss`): the forward source (whichever variant above) cut at its shadow
// lookup, `// evani.shader.shadows_sample` (forward_shadows.wgsl is always
// the last file), with the new lookup appended, stripped of comments, at
// compile time. `FORWARD.source` is this text, so the splicer and live slot
// rebuilds see the `pcss_params` slot. Declared last: no line above moves.
#[cfg(all(feature = "shadow-cascades", not(feature = "pcss")))]
const SHADOW_TAIL: &str = concat!(
    include_str!("../shaders/shadows_head.wgsl"),
    include_str!("../shaders/shadows_two.wgsl"),
    include_str!("../shaders/shadows_pcf.wgsl")
);
#[cfg(all(feature = "pcss", not(feature = "shadow-cascades"), not(feature = "pcss-stable")))]
const SHADOW_TAIL: &str = concat!(
    include_str!("../shaders/shadows_head.wgsl"),
    include_str!("../shaders/shadows_one.wgsl"),
    include_str!("../shaders/shadows_pcss.wgsl")
);
#[cfg(all(feature = "pcss", feature = "shadow-cascades", not(feature = "pcss-stable")))]
const SHADOW_TAIL: &str = concat!(
    include_str!("../shaders/shadows_head.wgsl"),
    include_str!("../shaders/shadows_two.wgsl"),
    include_str!("../shaders/shadows_pcss.wgsl")
);

/// Where `p` first starts in `s` (a compile error if it never does).
#[cfg(any(feature = "shadow-cascades", feature = "pcss"))]
const fn find(s: &[u8], p: &[u8]) -> usize {
    let mut i = 0;
    while i < s.len() {
        if starts_at(s, i, p) {
            return i;
        }
        i += 1;
    }
    panic!("shadow lookup marker not found")
}

#[cfg(any(feature = "shadow-cascades", feature = "pcss"))]
const FORWARD_CUT: usize = find(FORWARD_SOURCE.as_bytes(), b"// evani.shader.shadows_sample");
#[cfg(any(feature = "shadow-cascades", feature = "pcss"))]
const SHADOW_TAIL_BYTES: [u8; wgsl_strip(SHADOW_TAIL.as_bytes(), &mut [])] = wgsl_stripped(SHADOW_TAIL.as_bytes());
#[cfg(any(feature = "shadow-cascades", feature = "pcss"))]
#[allow(clippy::large_const_arrays)] const FORWARD_EXT_BYTES: [u8; FORWARD_CUT + SHADOW_TAIL_BYTES.len()] = forward_ext();

/// The forward source up to its shadow lookup, then the new lookup.
#[cfg(any(feature = "shadow-cascades", feature = "pcss"))]
const fn forward_ext<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    let src = FORWARD_SOURCE.as_bytes();
    let mut i = 0;
    while i < N {
        out[i] = if i < FORWARD_CUT { src[i] } else { SHADOW_TAIL_BYTES[i - FORWARD_CUT] };
        i += 1;
    }
    out
}

#[cfg(any(feature = "shadow-cascades", feature = "pcss"))]
const FORWARD_EXT: &str = utf8(&FORWARD_EXT_BYTES);
// The sprite WGSL with 2D lights (feature `lights2d`, not with `msdf` yet):
// `lights2d.wgsl`, the art read plus the light buffer, shipped without
// comments. Kept at the end of the file so games without it compile the
// lines above unchanged.
#[cfg(all(feature = "lights2d", not(feature = "msdf")))]
const SPRITE_LIGHTS2D_FULL: &str = include_str!("../shaders/lights2d.wgsl");
#[cfg(all(feature = "lights2d", not(feature = "msdf")))]
const SPRITE_LIGHTS2D_BYTES: [u8; wgsl_strip(SPRITE_LIGHTS2D_FULL.as_bytes(), &mut [])] = wgsl_stripped(SPRITE_LIGHTS2D_FULL.as_bytes());
#[cfg(all(feature = "lights2d", not(feature = "msdf")))]
const SPRITE_SOURCE: &str = utf8(&SPRITE_LIGHTS2D_BYTES);
#[cfg(all(feature = "lights2d", not(feature = "msdf")))]
const SPRITE_UNIFORM_BYTES: u32 = crate::lights2d::UNIFORM_WORDS as u32 * 4;
#[cfg(not(all(feature = "lights2d", not(feature = "msdf"))))]
const SPRITE_UNIFORM_BYTES: u32 = 16;

// The lit surface pass (feature `look-surface` without `relief`, S5-R5b):
// the forward source (whichever variant above, with the S5-R4 shadow tail
// when that is on), then shaders/model_lit.wgsl without comments, then the
// lit_* glue its fragment stage calls, picked by feature. The glue calls
// what the runtime patches add (gpu/lights.rs: point_lights; gpu/ambient.rs:
// amb_occlusion, amb_sh, amb_env) and never holds their hook text, so they
// patch the other stages exactly as before. With `relief` on, the relief
// stage wins (both use material bindings 5 to 7). Declared last: no line
// above moves.
#[cfg(all(feature = "look-surface", not(feature = "relief"), not(any(feature = "shadow-cascades", feature = "pcss"))))]
const LIT_BASE: &str = FORWARD_SOURCE;
#[cfg(all(feature = "look-surface", not(feature = "relief"), any(feature = "shadow-cascades", feature = "pcss")))]
const LIT_BASE: &str = FORWARD_EXT;
#[cfg(all(feature = "look-surface", not(feature = "relief")))]
const LIT_FULL: &str = include_str!("../shaders/model_lit.wgsl");
#[cfg(all(feature = "look-surface", not(feature = "relief")))]
const LIT_BYTES: [u8; wgsl_strip(LIT_FULL.as_bytes(), &mut [])] = wgsl_stripped(LIT_FULL.as_bytes());
#[cfg(all(feature = "look-surface", not(feature = "relief"), feature = "point-lights"))]
const LIT_POINTS: &str = "\nfn lit_points(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {\n    return point_lights(p, n, frag);\n}\n";
#[cfg(all(feature = "look-surface", not(feature = "relief"), not(feature = "point-lights")))]
const LIT_POINTS: &str = "\nfn lit_points(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n";
#[cfg(all(feature = "look-surface", not(feature = "relief"), feature = "ambient"))]
const LIT_AO: &str = "fn lit_ao(frag: vec4<f32>) -> f32 {\n    return amb_occlusion(frag);\n}\n";
#[cfg(all(feature = "look-surface", not(feature = "relief"), not(feature = "ambient")))]
const LIT_AO: &str = "fn lit_ao(frag: vec4<f32>) -> f32 {\n    return 1.0;\n}\n";
#[cfg(all(feature = "look-surface", not(feature = "relief"), feature = "ibl"))]
const LIT_SKY: &str = "fn lit_sky(n: vec3<f32>) -> vec3<f32> {\n    return mix(frame.ambient.rgb, amb_sh(n), clamp(amb.sun_color.w, 0.0, 1.0));\n}\nfn lit_env(r: vec3<f32>, rough: f32) -> vec3<f32> {\n    return amb_env(r, rough);\n}\n";
#[cfg(all(feature = "look-surface", not(feature = "relief"), not(feature = "ibl")))]
const LIT_SKY: &str = "fn lit_sky(n: vec3<f32>) -> vec3<f32> {\n    return frame.ambient.rgb;\n}\nfn lit_env(r: vec3<f32>, rough: f32) -> vec3<f32> {\n    return lit_hemi(r);\n}\n";
#[cfg(all(feature = "look-surface", not(feature = "relief"), not(feature = "material-layers")))]
const LIT_PARTS: [&[u8]; 5] = [LIT_BASE.as_bytes(), &LIT_BYTES, LIT_POINTS.as_bytes(), LIT_AO.as_bytes(), LIT_SKY.as_bytes()];

/// The total length of `parts`.
#[cfg(all(feature = "look-surface", not(feature = "relief")))]
const fn lit_len(parts: &[&[u8]]) -> usize {
    let (mut i, mut n) = (0, 0);
    while i < parts.len() {
        n += parts[i].len();
        i += 1;
    }
    n
}

/// `parts` one after another.
#[cfg(all(feature = "look-surface", not(feature = "relief")))]
const fn lit_join<const N: usize>(parts: &[&[u8]]) -> [u8; N] {
    let mut out = [0u8; N];
    let (mut i, mut n) = (0, 0);
    while i < parts.len() {
        let mut k = 0;
        while k < parts[i].len() && n < N {
            out[n] = parts[i][k];
            n += 1;
            k += 1;
        }
        i += 1;
    }
    out
}

#[cfg(all(feature = "look-surface", not(feature = "relief")))]
#[allow(clippy::large_const_arrays)] const FORWARD_LIT_BYTES: [u8; lit_len(&LIT_PARTS)] = lit_join(&LIT_PARTS);
#[cfg(all(feature = "look-surface", not(feature = "relief")))]
const FORWARD_LIT: &str = utf8(&FORWARD_LIT_BYTES);

/// What the runtime patches add that the lit glue calls (point_lights,
/// amb_occlusion, amb_sh, amb_env and the ambient uniform), as stubs, so
/// the tests validate the spliced forward source without the GPU patches.
#[cfg(test)]
fn lit_test_stubs() -> String {
    let mut s = String::new();
    if cfg!(all(feature = "look-surface", not(feature = "relief"), feature = "point-lights")) {
        s.push_str("\nfn point_lights(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n");
    }
    if cfg!(all(feature = "look-surface", not(feature = "relief"), feature = "ambient")) {
        s.push_str("\nfn amb_occlusion(frag: vec4<f32>) -> f32 {\n    return 1.0;\n}\n");
    }
    if cfg!(all(feature = "look-surface", not(feature = "relief"), feature = "ibl")) {
        s.push_str("\nstruct LitStubAmb {\n    sun_color: vec4<f32>,\n};\n@group(0) @binding(15) var<uniform> amb: LitStubAmb;\nfn amb_sh(n: vec3<f32>) -> vec3<f32> {\n    return n;\n}\nfn amb_env(r: vec3<f32>, rough: f32) -> vec3<f32> {\n    return r * rough;\n}\n");
    }
    s
}

// Stable soft shadows (feature `pcss-stable`, ChimpDuty intake limit 5): the
// PCSS tail with a receiver-plane depth per tap and a fixed tap pattern
// (shaders/shadows_pcss_stable.wgsl; its one- and two-cascade lookups pass
// the normal). Replaces the `pcss` tails above, which leave out
// `pcss-stable`. Declared last: no line above moves.
#[cfg(all(feature = "pcss-stable", not(feature = "shadow-cascades")))]
const SHADOW_TAIL: &str = concat!(
    include_str!("../shaders/shadows_head.wgsl"),
    include_str!("../shaders/shadows_one_stable.wgsl"),
    include_str!("../shaders/shadows_pcss_stable.wgsl")
);
#[cfg(all(feature = "pcss-stable", feature = "shadow-cascades"))]
const SHADOW_TAIL: &str = concat!(
    include_str!("../shaders/shadows_head.wgsl"),
    include_str!("../shaders/shadows_two_stable.wgsl"),
    include_str!("../shaders/shadows_pcss_stable.wgsl")
);

#[cfg(all(test, feature = "pcss-stable"))]
mod pcss_stable_tests {
    use super::*;
    use crate::splice::{Bodies, splice};
    use evani_core::slots::ShaderSlots;

    /// The forward source carries the stable lookup, not the hashed one.
    #[test]
    fn forward_uses_the_stable_tail() {
        assert!(FORWARD.source.contains("fn pcss_plane("), "receiver plane");
        assert!(FORWARD.source.contains("n: vec3<f32>) -> f32"), "the filter takes the normal");
        assert!(!FORWARD.source.contains("pcss_spin"), "no per-texel hash spin");
        assert!(FORWARD.source.contains("//@slot pcss_params"), "the slot stays");
    }

    /// The forward shader with the stable tail validates (naga), with the
    /// slot defaults and the lit-surface stubs.
    #[test]
    fn stable_forward_validates() {
        let mut slots = ShaderSlots::with_builtins();
        slots.declare(crate::shadow_ext::PCSS_PARAMS); #[cfg(feature = "relief")] slots.declare(evani_core::slots::MICRO_DETAIL); #[cfg(all(feature = "look-surface", not(feature = "relief")))] { slots.declare(crate::model::surface::SPECULAR); slots.declare(crate::model::surface::SURFACE_PARAMS); }
        for bodies in [Bodies::Current, Bodies::EngineDefaults] {
            let s = splice(FORWARD.source, &slots, bodies).unwrap();
            let src = s.source + &lit_test_stubs();
            naga::front::wgsl::parse_str(&src)
                .map_err(|e| e.emit_to_string(&src))
                .and_then(|m| naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&m).map(|_| ()).map_err(|e| format!("{e:?}")))
                .unwrap();
        }
    }

    /// `pcss_plane` in shadows_pcss_stable.wgsl, line for line.
    fn plane(m: &math::Mat4, n: [f32; 3], cap: f32) -> [f32; 2] {
        let row = |r: usize| [m[r], m[4 + r], m[8 + r]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let l = |r: usize| dot(row(r), n) / dot(row(r), row(r)).max(1e-12);
        let lz = if l(2).abs() > 1e-6 { l(2) } else { -1e-6 };
        let g = [-2.0 * l(0) / lz, 2.0 * l(1) / lz];
        let len = (g[0] * g[0] + g[1] * g[1]).sqrt();
        if len > cap { [g[0] * cap / len, g[1] * cap / len] } else { g }
    }

    fn uvz(m: &math::Mat4, p: [f32; 3]) -> [f32; 3] {
        let c = math::transform_point(m, p);
        [c[0] * 0.5 + 0.5, 0.5 - c[1] * 0.5, c[2]]
    }

    /// The receiver plane predicts the shadow-map depth of the floor a few
    /// texels away under a low sun (the slope that made the old search
    /// count the floor as its own blocker), and of a tilted surface.
    #[test]
    fn receiver_plane_predicts_the_surface_depth() {
        for (light, n) in [([1.0, 0.22, 0.35], [0.0, 1.0, 0.0]), ([-0.3, 0.8, 0.5], [0.0, 1.0, 0.0]), ([0.4, 0.5, -0.6], [0.6, 0.8, 0.0])] {
            let m = light_view_proj([2.0, 0.0, -3.0], 12.0, 30.0, light, 2048);
            let p = [1.0, 0.0, -2.0];
            // A step along the surface (perpendicular to n).
            let t = math::normalize([n[1], -n[0], 0.0]);
            let b = [n[1] * 0.0 - n[2] * t[1], n[2] * t[0] - n[0] * 0.0, n[0] * t[1] - n[1] * t[0]];
            let g = plane(&m, n, 1e9);
            let a = uvz(&m, p);
            for (u, v) in [(0.07, 0.0), (0.0, 0.05), (-0.04, 0.09)] {
                let q = [p[0] + t[0] * u + b[0] * v, p[1] + t[1] * u + b[1] * v, p[2] + t[2] * u + b[2] * v];
                let c = uvz(&m, q);
                let want = c[2] - a[2];
                let got = g[0] * (c[0] - a[0]) + g[1] * (c[1] - a[1]);
                assert!((want - got).abs() < 1e-5 + want.abs() * 1e-3, "light {light:?}: {want} vs {got}");
            }
        }
        // A floor at a low sun slopes away from the light: the old search's
        // constant bias would have to cover a whole search radius of it.
        let m = light_view_proj([0.0; 3], 12.0, 30.0, [1.0, 0.15, 0.0], 2048);
        let g = plane(&m, [0.0, 1.0, 0.0], 1e9);
        let texel = 1.0 / 2048.0;
        let step = g[0].hypot(g[1]) * 6.0 * texel;
        assert!(step > 1e-4, "6 texels of floor change depth by {step}");
        // Grazing surfaces are capped.
        let c = plane(&m, [0.0, 1.0, 0.0], 0.5);
        assert!(((c[0] * c[0] + c[1] * c[1]).sqrt() - 0.5).abs() < 1e-5);
        assert_eq!(plane(&m, [0.0; 3], 1.0), [0.0, 0.0], "no normal, no plane");
    }

    /// The compare margin, in metres along the light, for a floor under a
    /// 14 degree sun on a coarse map (80 m across 1024 texels): the first
    /// version's whole texel of slope (cap 4) was over 30 cm and hid real
    /// shadows; a fifth of a texel (cap 16), as the shader uses now, is a
    /// fifth of that.
    #[test]
    fn the_margin_stays_small_at_a_low_sun() {
        let a = 14f32.to_radians();
        let m = light_view_proj([0.0; 3], 40.0, 60.0, [a.cos(), a.sin(), 0.0], 1024);
        let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let uv_per_m = 0.5 * len([m[0], m[4], m[8]]);
        let z_per_m = len([m[2], m[6], m[10]]);
        let texel = 1.0 / 1024.0;
        let margin_m = |cap: f32, part: f32| {
            let g = plane(&m, [0.0, 1.0, 0.0], cap * z_per_m / uv_per_m);
            part * g[0].hypot(g[1]) * texel / z_per_m
        };
        let old = margin_m(4.0, 1.0);
        let now = margin_m(16.0, 0.2);
        assert!(old > 0.3, "the first version's margin: {old} m");
        assert!(now < 0.07 && now < old * 0.25, "now {now} m against {old} m");
    }
}

// Layered materials (feature `material-layers` without `relief`; crate
// evani-material-layers): the lit surface pass's parts with the layer BRDF
// (the crate's layers_brdf.wgsl) and shaders/model_lit_layers.wgsl, both
// without comments, between model_lit.wgsl and the lit_* glue. The
// pipelines draw `model_layered_fs` (gpu/models.rs LIT_FS); the plain lit
// stage stays in the module unused. Replaces the five-part LIT_PARTS above,
// which leaves out `material-layers`. Declared last: no line above moves.
#[cfg(all(feature = "material-layers", not(feature = "relief")))]
const LAYERS_BRDF_BYTES: [u8; wgsl_strip(evani_material_layers::BRDF_WGSL.as_bytes(), &mut [])] = wgsl_stripped(evani_material_layers::BRDF_WGSL.as_bytes());
#[cfg(all(feature = "material-layers", not(feature = "relief")))]
const LAYERS_FULL: &str = include_str!("../shaders/model_lit_layers.wgsl");
#[cfg(all(feature = "material-layers", not(feature = "relief")))]
const LAYERS_BYTES: [u8; wgsl_strip(LAYERS_FULL.as_bytes(), &mut [])] = wgsl_stripped(LAYERS_FULL.as_bytes());
#[cfg(all(feature = "material-layers", not(feature = "relief")))]
const LIT_PARTS: [&[u8]; 7] = [LIT_BASE.as_bytes(), &LIT_BYTES, &LAYERS_BRDF_BYTES, &LAYERS_BYTES, LIT_POINTS.as_bytes(), LIT_AO.as_bytes(), LIT_SKY.as_bytes()];
