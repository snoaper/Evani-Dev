// evani.shader.model
// Textured, normal-mapped models (feature `models`), in the forward pass
// beside the instanced meshes and lit the same way: the same frame uniform,
// toon_ramp, fog and rim slots, shadow map and blob shadows. Vertices stay
// quantised on the GPU and are dequantised here: u16 positions over the
// part's box (scale and offset per instance), i8 normals and tangents, u16
// UVs over the part's UV box. Materials: base colour (texture x factor x
// tint), a tangent-space normal map (MikkTSpace: bitangent = cross(n, t) *
// w), emissive, alpha opaque or mask, single- or double-sided (no culling
// in the pipeline: back faces of single-sided parts are discarded, those of
// double-sided parts lit from their own side). Every map is sampled before
// any discard.

struct ModelMaterial {
    base: vec4<f32>,
    // Emissive (rgb) and alpha cutoff (w).
    emissive_cutoff: vec4<f32>,
    // Alpha mask, double-sided, normal map, normal scale.
    flags: vec4<f32>,
    // KHR_texture_transform as two rows of a 2x3 matrix.
    uv0: vec4<f32>,
    uv1: vec4<f32>,
};

@group(1) @binding(0) var<uniform> model_mat: ModelMaterial;
@group(1) @binding(1) var base_map: texture_2d<f32>;
@group(1) @binding(2) var normal_map: texture_2d<f32>;
@group(1) @binding(3) var emissive_map: texture_2d<f32>;
@group(1) @binding(4) var map_sampler: sampler;

struct ModelIn {
    @location(0) pos: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    // Instance: world matrix rows (instance x node).
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    // Position scale (xyz; w = 1 when the part has tangents) and offset.
    @location(7) pos_scale: vec4<f32>,
    @location(8) pos_offset: vec4<f32>,
    // UV scale (xy) and offset (zw).
    @location(9) uv_dq: vec4<f32>,
    // Linear RGBA tint.
    @location(10) tint: vec4<f32>,
};

struct ModelOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    // -1 on a mirrored instance (its triangles wind the other way on
    // screen, so front_facing is flipped back), else 1.
    @location(5) @interpolate(flat) mirror: f32,
    // Baked open-sky cone (model/sun.rs): world direction (xyz) and the
    // cosine threshold (w), for the tiers without a shadow map.
    @location(6) sun: vec4<f32>,
};

@vertex
fn model_vs(in: ModelIn) -> ModelOut {
    let local = vec4<f32>(in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz, 1.0);
    let world = vec3<f32>(dot(in.row0, local), dot(in.row1, local), dot(in.row2, local));
    let c0 = vec3<f32>(in.row0.x, in.row1.x, in.row2.x);
    let c1 = vec3<f32>(in.row0.y, in.row1.y, in.row2.y);
    let c2 = vec3<f32>(in.row0.z, in.row1.z, in.row2.z);
    // Normals by the cofactor matrix (any scale), turned back out on a
    // mirrored instance; tangents as vectors, the handedness flipped there.
    let x12 = cross(c1, c2);
    let s = select(1.0, -1.0, dot(c0, x12) < 0.0);
    let cof = mat3x3<f32>(x12, cross(c2, c0), cross(c0, c1));
    // The cone: snorm8 x and y in the position's w (two bytes of a
    // Unorm16), z in the normal's w; length (1 + threshold) / 2. Length 0
    // is open sky everywhere (threshold -1 round the normal).
    let w = u32(round(in.pos.w * 65535.0));
    let cone = vec3<f32>(f32((i32(w << 24u)) >> 24u), f32((i32(w << 16u)) >> 24u), 0.0) / 127.0 + vec3<f32>(0.0, 0.0, in.normal.w);
    let cl = length(cone);
    let cd = select(cone, in.normal.xyz, cl < 0.01);
    var o: ModelOut;
    o.pos = frame.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = cof * in.normal.xyz * s;
    o.sun = vec4<f32>(cof * cd * s, 2.0 * cl - 1.0);
    o.tangent = vec4<f32>(mat3x3<f32>(c0, c1, c2) * in.tangent.xyz, in.tangent.w * s * in.pos_scale.w);
    o.uv = in.uv_dq.zw + in.uv_dq.xy * in.uv;
    o.color = in.tint;
    o.mirror = s;
    return o;
}

// Light let through by the face's own side (geometric N.L): 1 facing the
// sun, 0 turned away, a short linear blend across the terminator. Only
// clamp and arithmetic, so a test can evaluate it at compile time.
fn self_shadow(gl: f32) -> f32 {
    return clamp(gl * 10.0 + 0.5, 0.0, 1.0);
}

// Sun let through by the baked open-sky cone (cosine `d` of the sun to the
// cone's direction, threshold `t`): the model's own cast shadow on itself
// where there is no shadow map (catalog: Lite and Low have baked shadows).
fn sun_cone(d: f32, t: f32) -> f32 {
    return clamp((d - t) * 10.0 + 0.5, 0.0, 1.0);
}

@fragment
fn model_fs(in: ModelOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    let front = facing != (in.mirror < 0.0);
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv = vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3));
    let tex = textureSample(base_map, map_sampler, uv);
    let nm = textureSample(normal_map, map_sampler, uv).xyz;
    let em = textureSample(emissive_map, map_sampler, uv).rgb;
    let base = tex * model_mat.base * vec4<f32>(in.color.rgb, 1.0);
    if ((!front && model_mat.flags.y < 0.5) || (model_mat.flags.x > 0.5 && base.a < model_mat.emissive_cutoff.w)) {
        discard;
    }
    let side = select(-1.0, 1.0, front);
    let ng = normalize(in.normal);
    var n = ng;
    if (model_mat.flags.z > 0.5 && abs(in.tangent.w) > 0.5) {
        let t = normalize(in.tangent.xyz - ng * dot(ng, in.tangent.xyz));
        let b = cross(ng, t) * in.tangent.w;
        let m = (nm * 2.0 - 1.0) * vec3<f32>(model_mat.flags.w, model_mat.flags.w, 1.0);
        n = normalize(t * m.x + b * m.y + ng * m.z);
    }
    n = n * side;
    let g = ng * side;
    let dist = length(in.world - frame.eye.xyz);
    let ndl = dot(n, frame.light_dir.xyz);
    // A face turned away from the sun is in its own shadow on every tier:
    // the normal map can tip N.L above zero there, which the shadow map
    // darkens on Medium and up but nothing would on Lite and Low. Without
    // a shadow map the baked cone casts the model's shadow on itself.
    let cast_vis = select(sun_cone(dot(normalize(in.sun.xyz), frame.light_dir.xyz), in.sun.w), shadow_vis(in.world, g), frame.shadow.x > 0.5);
    let vis = cast_vis * blob_vis(in.world, g) * self_shadow(dot(g, frame.light_dir.xyz));
    let diffuse = slot_toon_ramp(mix(min(ndl, -0.1), ndl, vis));
    let ndv = dot(n, (frame.eye.xyz - in.world) / max(dist, 1e-4));
    var c = base.rgb * (frame.ambient.rgb + frame.light_color.rgb * diffuse) + model_mat.emissive_cutoff.rgb * em
        + slot_rim(ndv, ndl, dist, base.rgb);
    // Shadow tint, as the forward meshes do it.
    let k = clamp(1.0 - vis, 0.0, 1.0) * clamp(frame.shadow_tint.w, 0.0, 1.0);
    let w = vec3<f32>(0.2126, 0.7152, 0.0722);
    let ts = sqrt(max(frame.shadow_tint.rgb, vec3<f32>(0.0)));
    let hs = ts / max(dot(ts, w), 1e-3);
    let tt = hs * dot(sqrt(max(c, vec3<f32>(0.0))), w);
    let tc = tt / max(1.0, max(tt.r, max(tt.g, tt.b)));
    c = mix(c, tc * tc, k);
    let f = clamp(slot_fog(dist, in.world.y), 0.0, 1.0);
    return vec4<f32>(mix(c, frame.fog_color.rgb, f), 1.0);
}
