// evani.shader.cond.growth
// Growth (bit 6): moss and lichen on shaded, damp faces: low AO and facing
// away from the sun, never on top faces; patches kc.z growth.scale per
// metre, kd.z the colour. Rough, not metal, a little occluded.
fn cond_growth(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let s = (1.0 - o.occ + max(-dot(i.g, frame.light_dir.xyz), 0.0)) * (1.0 - smoothstep(0.5, 0.85, i.g.y));
    let v = rc_fbm2(rc_proj(i.world, i.g) * rc_frame.kc.z, 5u, 3u);
    let m = smoothstep(1.0 - a, 1.15 - a, s * 0.5 + v * 0.6 + nz * 0.1) * min(a * 8.0, 1.0);
    let c = rc_lin(unpack4x8unorm(rc_frame.kd.z).rgb) * (0.6 + 0.8 * v);
    r.base = vec4<f32>(mix(o.base.rgb, c, m), o.base.a);
    r.rough = mix(o.rough, 0.95, m);
    r.metal = o.metal * (1.0 - m);
    r.occ = o.occ * (1.0 - 0.25 * m);
    return r;
}
