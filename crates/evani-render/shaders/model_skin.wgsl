// evani.shader.model_skin
// Skinned models (feature `skinning`, S3-9): linear blend skinning in the
// vertex stage, after model.wgsl in the same module (same material, same
// fragment stage). Every character's joint matrices are in one read-only
// storage buffer, 3x4 each (three rows, used as `vec4(p, 1.0) * m`), and
// the instance's position-offset w holds the first one. Joints are u8x4
// (indices into the skin's joint list), weights unorm8x4 summing to one.
// The blended matrix is folded into the instance rows, so the rest is
// model_vs as it is: normals and the baked sun cone (baked in bind pose,
// in the vertex's own space) turn by the cofactor, so the cone follows the
// limb; tangents turn as vectors.

@group(2) @binding(0) var<storage, read> joints: array<mat3x4<f32>>;

@vertex
fn model_skin_vs(in: ModelIn, @location(11) j: vec4<u32>, @location(12) wt: vec4<f32>) -> ModelOut {
    let b = u32(in.pos_offset.w);
    let m = joints[b + j.x] * wt.x + joints[b + j.y] * wt.y + joints[b + j.z] * wt.z + joints[b + j.w] * wt.w;
    // Instance rows x skin matrix (both affine).
    let r0 = in.row0.x * m[0] + in.row0.y * m[1] + in.row0.z * m[2] + vec4<f32>(0.0, 0.0, 0.0, in.row0.w);
    let r1 = in.row1.x * m[0] + in.row1.y * m[1] + in.row1.z * m[2] + vec4<f32>(0.0, 0.0, 0.0, in.row1.w);
    let r2 = in.row2.x * m[0] + in.row2.y * m[1] + in.row2.z * m[2] + vec4<f32>(0.0, 0.0, 0.0, in.row2.w);
    let local = vec4<f32>(in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz, 1.0);
    let world = vec3<f32>(dot(r0, local), dot(r1, local), dot(r2, local));
    let c0 = vec3<f32>(r0.x, r1.x, r2.x);
    let c1 = vec3<f32>(r0.y, r1.y, r2.y);
    let c2 = vec3<f32>(r0.z, r1.z, r2.z);
    let x12 = cross(c1, c2);
    let s = select(1.0, -1.0, dot(c0, x12) < 0.0);
    let cof = mat3x3<f32>(x12, cross(c2, c0), cross(c0, c1));
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
