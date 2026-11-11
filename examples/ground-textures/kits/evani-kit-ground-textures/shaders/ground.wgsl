// Ground textures proof (TopGun W3): kit_surface over evani-render's kit
// textures. Our own code; the ideas are from Inigo Quilez's biplanar mapping
// and Mikkelsen's hex-tiling (ideas only).
//
// * Projection: flat ground samples world XZ (one projection); steep faces
//   sample the two world planes that best face the normal (biplanar), with
//   sharpened weights. Gradients are taken once at the top (uniform control
//   flow) and passed to textureSampleGrad, so mips never seam.
// * Anti-repeat: hex-tiling (3 rotated, offset samples blended per hex cell)
//   only on the near band; mid and far use one sample.
// * Far: detail fades by pixel footprint to the layer's mean colour (its
//   1 x 1 mip), so near and far agree; past the fade nothing is fetched.
// * Colour authority stays with the procedural recipe: the texture is
//   applied as a ratio to its mean.
//
// kit_data: [0] material slots rock, field, paving, textures on (0/1)
//           [1] fade start, fade end (tile per pixel), hex on, hex band
//           [2] tile metres rock, field, paving; biplanar sharpness
//           [3] normal strength, hex rotation (0..1), colour strength

fn gt_hash(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

fn gt_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = gt_hash(i).x;
    let b = gt_hash(i + vec2<f32>(1.0, 0.0)).x;
    let c = gt_hash(i + vec2<f32>(0.0, 1.0)).x;
    let d = gt_hash(i + vec2<f32>(1.0, 1.0)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn gt_fbm(p: vec2<f32>) -> f32 {
    return 0.5 * gt_noise(p) + 0.3 * gt_noise(p * 2.03 + 7.1) + 0.2 * gt_noise(p * 4.11 + 3.7);
}

// The procedural recipe (what the ground looked like before textures):
// a flat colour per surface with broad noise.
fn gt_recipe(kind: u32, p: vec3<f32>) -> vec3<f32> {
    let n = gt_fbm(p.xz * 0.15 + p.y * 0.1);
    if (kind == 0u) { return vec3<f32>(0.33, 0.17, 0.08) * (0.8 + 0.4 * n); }
    if (kind == 1u) { return vec3<f32>(0.06, 0.05, 0.03) * (0.8 + 0.4 * n) + vec3<f32>(0.0, 0.025, 0.0) * n; }
    return vec3<f32>(0.20, 0.195, 0.185) * (0.85 + 0.3 * n);
}

struct GtTex {
    ah: vec4<f32>,
    nra: vec4<f32>,
}

fn gt_fetch(tset: u32, layer: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> GtTex {
    if (tset == 0u) {
        return GtTex(textureSampleGrad(kit_tex0_ah, kit_wrap, uv, layer, dx, dy), textureSampleGrad(kit_tex0_nra, kit_wrap, uv, layer, dx, dy));
    }
    return GtTex(textureSampleGrad(kit_tex1_ah, kit_wrap, uv, layer, dx, dy), textureSampleGrad(kit_tex1_nra, kit_wrap, uv, layer, dx, dy));
}

fn gt_mean(tset: u32, layer: u32) -> vec3<f32> {
    if (tset == 0u) { return textureSampleLevel(kit_tex0_ah, kit_clamp, vec2<f32>(0.5), layer, 16.0).rgb; }
    return textureSampleLevel(kit_tex1_ah, kit_clamp, vec2<f32>(0.5), layer, 16.0).rgb;
}

fn gt_rot(a: f32, v: vec2<f32>) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

// Hex-tiling: the point's triangle on a triangle grid; each corner is a hex
// cell with its own offset and rotation. Three samples, blended by
// sharpened barycentric weights; the normal's xy turns back with its cell.
fn gt_hex(tset: u32, layer: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, turn: f32) -> GtTex {
    let s = vec2<f32>(uv.x - uv.y * 0.57735027, uv.y * 1.15470054) * 2.0;
    let b = floor(s);
    let f = fract(s);
    var c0 = b;
    var c1 = b + vec2<f32>(1.0, 0.0);
    var c2 = b + vec2<f32>(0.0, 1.0);
    var w = vec3<f32>(1.0 - f.x - f.y, f.x, f.y);
    if (f.x + f.y > 1.0) {
        c0 = b + vec2<f32>(1.0, 1.0);
        w = vec3<f32>(f.x + f.y - 1.0, 1.0 - f.y, 1.0 - f.x);
    }
    w = w * w * w;
    w = w / max(w.x + w.y + w.z, 1e-4);
    var out = GtTex(vec4<f32>(0.0), vec4<f32>(0.0));
    for (var k = 0u; k < 3u; k++) {
        let c = select(select(c2, c1, k == 1u), c0, k == 0u);
        let r = gt_hash(c);
        let a = r.x * 6.2831853 * turn;
        let t = gt_fetch(tset, layer, gt_rot(a, uv) + r * 7.0, gt_rot(a, dx), gt_rot(a, dy));
        let n = gt_rot(-a, t.nra.xy * 2.0 - 1.0) * 0.5 + 0.5;
        out.ah += t.ah * w[k];
        out.nra += vec4<f32>(n, t.nra.zw) * w[k];
    }
    return out;
}

fn gt_sample(tset: u32, layer: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, hex: bool, turn: f32) -> GtTex {
    if (hex) { return gt_hex(tset, layer, uv, dx, dy, turn); }
    return gt_fetch(tset, layer, uv, dx, dy);
}

// Tangent-space normal (OpenGL) from NRA.
fn gt_tn(nra: vec4<f32>, strength: f32) -> vec3<f32> {
    let xy = (nra.xy * 2.0 - 1.0) * strength;
    return vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
}

fn kit_surface(p: vec3<f32>, n: vec3<f32>, material: u32, nr: ptr<function, vec4<f32>>) -> vec4<f32> {
    let h0 = kit_data[0];
    let h1 = kit_data[1];
    let h2 = kit_data[2];
    let h3 = kit_data[3];
    // Gradients first, in uniform control flow, for every projection.
    let dxz = vec2<f32>(dpdx(p.x), dpdx(p.z));
    let dyz = vec2<f32>(dpdy(p.x), dpdy(p.z));
    let dzx = vec2<f32>(dpdx(p.z), -dpdx(p.y));
    let dzy = vec2<f32>(dpdy(p.z), -dpdy(p.y));
    let dxx = vec2<f32>(dpdx(p.x), -dpdx(p.y));
    let dxy = vec2<f32>(dpdy(p.x), -dpdy(p.y));
    var kind = 3u;
    if (material == u32(h0.x)) { kind = 0u; }
    if (material == u32(h0.y)) { kind = 1u; }
    if (material == u32(h0.z)) { kind = 2u; }
    if (kind == 3u || KIT_SHADOW_PASS) {
        return vec4<f32>(1.0);
    }
    let recipe = gt_recipe(kind, p);
    var tset = 1u;
    var layer = 0u;
    var tile = h2.x;
    if (kind == 1u) { tset = 0u; layer = 0u; tile = h2.y; }
    if (kind == 2u) { tset = 1u; layer = 1u; tile = h2.z; }
    // 0 until the layer's maps are on the GPU, then fading to 1 (W3c): the
    // detail fades in instead of popping, at the boot size and again never
    // (the swap to the tier size keeps it at 1).
    let ready = kit_tex_ready(tset, layer);
    if (h0.w < 0.5 || ready <= 0.0) {
        return vec4<f32>(recipe, 1.0);
    }
    let inv = 1.0 / tile;
    // Footprint: tiles per pixel on the flat projection (the largest).
    let foot = max(length(dxz), length(dyz)) * inv;
    let detail = (1.0 - smoothstep(h1.x, h1.y, foot)) * ready;
    let mean = max(gt_mean(tset, layer), vec3<f32>(0.02));
    if (detail <= 0.0) {
        return vec4<f32>(recipe, 1.0);
    }
    // Hex-tiling suits organic layers only: slab grids and furrows would
    // break. The field gets offsets (and rotation only if asked: furrows
    // have a direction).
    let hex = h1.z > 0.5 && foot < h1.w && kind == 1u;
    let an = abs(n);
    var ah = vec4<f32>(0.0);
    var nra = vec4<f32>(0.0);
    var bent = n;
    if (an.y > 0.7) {
        // Flat: world XZ, one projection.
        let t = gt_sample(tset, layer, p.xz * inv, dxz * inv, dyz * inv, hex, h3.y);
        ah = t.ah;
        nra = t.nra;
        let tn = gt_tn(t.nra, h3.x);
        bent = normalize(vec3<f32>(tn.x, tn.z * sign(n.y), -tn.y) * vec3<f32>(1.0, 1.0, 1.0) + vec3<f32>(n.x, 0.0, n.z));
    } else {
        // Steep: the two planes that best face the normal (biplanar).
        var wx = an.x;
        var wy = an.y;
        var wz = an.z;
        // Drop the weakest axis; sharpen the other two.
        let lo = min(wx, min(wy, wz));
        wx = select(wx, 0.0, wx == lo);
        wy = select(wy, 0.0, wy == lo && wx != 0.0);
        wz = select(wz, 0.0, wz == lo && wx != 0.0 && wy != 0.0);
        let k = h2.w;
        var w = pow(vec3<f32>(wx, wy, wz), vec3<f32>(k));
        w = w / max(w.x + w.y + w.z, 1e-4);
        var nsum = vec3<f32>(0.0);
        if (w.x > 0.0) {
            let t = gt_fetch(tset, layer, vec2<f32>(p.z, -p.y) * inv, dzx * inv, dzy * inv);
            let tn = gt_tn(t.nra, h3.x);
            ah += t.ah * w.x;
            nra += t.nra * w.x;
            nsum += vec3<f32>(tn.z * sign(n.x), tn.y, tn.x * sign(n.x)) * w.x;
        }
        if (w.y > 0.0) {
            let t = gt_fetch(tset, layer, p.xz * inv, dxz * inv, dyz * inv);
            let tn = gt_tn(t.nra, h3.x);
            ah += t.ah * w.y;
            nra += t.nra * w.y;
            nsum += vec3<f32>(tn.x, tn.z * sign(n.y), -tn.y) * w.y;
        }
        if (w.z > 0.0) {
            let t = gt_fetch(tset, layer, vec2<f32>(p.x, -p.y) * inv, dxx * inv, dxy * inv);
            let tn = gt_tn(t.nra, h3.x);
            ah += t.ah * w.z;
            nra += t.nra * w.z;
            nsum += vec3<f32>(tn.x * sign(n.z), tn.y, tn.z * sign(n.z)) * w.z;
        }
        bent = normalize(mix(n, nsum, 0.85));
    }
    // Detail as a ratio to the layer's mean, faded out by footprint.
    let rr = ah.rgb / mean;
    let ratio = mix(vec3<f32>(1.0), mix(vec3<f32>(dot(rr, vec3<f32>(0.3, 0.59, 0.11))), rr, h3.z), detail);
    let ao = mix(1.0, nra.w, detail);
    *nr = vec4<f32>(normalize(mix(n, bent, detail)), mix(0.95, nra.z, detail));
    return vec4<f32>(recipe * ratio * ao, 1.0);
}

fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
    var nr = vec4<f32>(n, 1.0);
    return kit_surface(p, n, material, &nr);
}
