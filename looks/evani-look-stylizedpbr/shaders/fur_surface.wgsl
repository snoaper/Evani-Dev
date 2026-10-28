
// evani.shader.fur_surface
// evani-look-stylizedpbr feature `fur`: the baked clump map (R strand
// height, G clump shade, B noise) on fur materials. Projected in the part's
// own space on the plane facing the normal most, at the material's clump
// scale. Lite/Low draw only this (roots dark, tips lighter); higher tiers
// draw the shells and fins on top.
@group(1) @binding(9) var clump_map: texture_2d<f32>;

// The plane of the normal's largest axis.
fn fur_axis(n: vec3<f32>) -> u32 {
    let a = abs(n);
    return select(select(2u, 1u, a.y >= a.z), 0u, a.x >= a.y && a.x >= a.z);
}

fn fur_plane(q: vec3<f32>, axis: u32) -> vec2<f32> {
    return select(select(q.xy, q.zx, axis == 1u), q.yz, axis == 0u);
}

fn fur_sample(p: vec3<f32>, n: vec3<f32>, scale: f32) -> vec4<f32> {
    let q = p / max(scale, 1e-4);
    let qx = dpdx(q);
    let qy = dpdy(q);
    let axis = fur_axis(n);
    let t = textureSampleGrad(clump_map, map_sampler, fur_plane(q, axis), fur_plane(qx, axis), fur_plane(qy, axis));
    return vec4<f32>(t.rgb, 1.0);
}
