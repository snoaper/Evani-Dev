// evani.shader.studio_tiltshift
// The studio's miniature pass (feature `studio-passes`, pass `studio.apply`,
// mode `tiltshift`): the Diorama style's tilt-shift. A band across the view
// (`focus`, 0 = top, 1 = bottom) and everything at the depth seen in its
// middle stay sharp; the rest blurs in a 16-tap disk whose radius grows with
// the distance from the band and from the focus depth (`blur`). The colours
// get a little more saturation and contrast, the way a toy model reads, by
// `grade`; at grade 0 it is a plain photographic depth of field (the
// procedural library's realism view: band 0.5, so only depth blurs) with a
// 1-step ordered dither so the soft backdrop never bands.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

struct Tilt {
    size: vec2<f32>,
    inv: vec2<f32>,
    focus: f32,
    blur: f32,
    // Half the height of the sharp band (0..1 of the view).
    band: f32,
    grade: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Tilt;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

fn dist(p: vec2<f32>) -> f32 {
    let t = clamp(vec2<i32>(floor(p)), vec2<i32>(0), vec2<i32>(u.size) - vec2<i32>(1));
    let d = textureLoad(depth_tex, t, 0);
    return 1.0 / max(u.inv.x + u.inv.y * max(d, 1e-7), 1e-9);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.pos.xy;
    let y = p.y / u.size.y;
    let zf = dist(vec2<f32>(u.size.x * 0.5, u.focus * u.size.y));
    let z = dist(p);
    let off_band = smoothstep(u.band, u.band + 0.3, abs(y - u.focus));
    let off_depth = clamp(abs(z - zf) / max(zf, 0.01) * 1.5 - 0.15, 0.0, 1.0);
    let coc = u.blur * max(off_band, off_depth * 0.8);
    let radius = coc * u.size.y * 0.012;
    var c = textureSampleLevel(src, samp, in.uv, 0.0).rgb;
    if (radius > 0.5) {
        // Only taps at about this depth or behind it count (procedural
        // realism round 9): a blurred backdrop pixel beside a sharp object
        // took the object's colour from the taps that landed on it, a dotted
        // halo along the silhouette.
        var sum = c;
        var wsum = 1.0;
        for (var k = 1u; k < 16u; k = k + 1u) {
            let a = f32(k) * 2.39996;
            let q = p + vec2<f32>(cos(a), sin(a)) * (sqrt(f32(k) / 15.0) * radius);
            let wk = select(1.0, 0.0, dist(q) < z * 0.9);
            sum = sum + textureSampleLevel(src, samp, q / u.size, 0.0).rgb * wk;
            wsum = wsum + wk;
        }
        c = sum / wsum;
    }
    let l = dot(c, vec3<f32>(0.3, 0.59, 0.11));
    let g = u.blur * clamp(u.grade, 0.0, 1.0);
    c = mix(vec3<f32>(l), c, 1.0 + 0.3 * g);
    c = (c - 0.5) * (1.0 + 0.12 * g) + 0.5;
    // Ordered 4x4 dither of one 8-bit step, faded out as grade rises.
    let q = vec2<u32>(p) & vec2<u32>(3u);
    let da = (((q.x ^ q.y) & 1u) << 1u) | (q.y & 1u);
    let db = ((((q.x >> 1u) ^ (q.y >> 1u)) & 1u) << 1u) | (q.y >> 1u);
    let bayer = f32(da * 4u + db) / 16.0;
    c = c + (bayer - 0.47) / 255.0 * (1.0 - clamp(u.grade, 0.0, 1.0));
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
