
// evani.shader.biplanar
// evani-look-stylizedpbr feature `biplanar`: a tiling detail map (fabric
// weave: R = brightness, G = thread height, both around 0.5) laid on the
// part in its own space with two planar projections instead of three. The
// two planes are the ones facing the normal most; their weights fall to
// zero before a plane turns edge-on, so no stretched texels show. The
// screen derivatives are taken from the continuous position before the
// planes are chosen, so mip selection never jumps at a plane switch.
@group(1) @binding(7) var detail_map: texture_2d<f32>;

// The 2D coordinates of `q` in the plane whose normal is axis `a` (0 x, 1 y,
// 2 z).
fn biplanar_plane(q: vec3<f32>, a: u32) -> vec2<f32> {
    return select(select(q.xy, q.zx, a == 1u), q.yz, a == 0u);
}

fn biplanar_axis_value(v: vec3<f32>, a: u32) -> f32 {
    return select(select(v.z, v.y, a == 1u), v.x, a == 0u);
}

fn detail_sample(p: vec3<f32>, n: vec3<f32>, scale: f32) -> vec2<f32> {
    let q = p / max(scale, 1e-4);
    let qx = dpdx(q);
    let qy = dpdy(q);
    let a = abs(n);
    // Largest, smallest and middle axes of the normal.
    let big = select(select(2u, 1u, a.y >= a.z), 0u, a.x >= a.y && a.x >= a.z);
    let small = select(select(2u, 1u, a.y < a.z), 0u, a.x < a.y && a.x < a.z);
    let mid = 3u - big - small;
    let s0 = textureSampleGrad(detail_map, map_sampler, biplanar_plane(q, big), biplanar_plane(qx, big), biplanar_plane(qy, big)).rg;
    let s1 = textureSampleGrad(detail_map, map_sampler, biplanar_plane(q, mid), biplanar_plane(qx, mid), biplanar_plane(qy, mid)).rg;
    // Weights: zero below 0.55 (a plane well past 45 degrees), sharpened.
    var w = max(vec2<f32>(biplanar_axis_value(a, big), biplanar_axis_value(a, mid)) - vec2<f32>(0.55), vec2<f32>(0.0));
    w = w * w * w * w;
    let total = max(w.x + w.y, 1e-6);
    return (s0 * w.x + s1 * w.y) / total;
}
