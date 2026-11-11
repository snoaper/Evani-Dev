// evani.shader.cond.blood
// Blood (bit 7): Voronoi splats of random radius, more cells splashed as the
// amount grows; fresh red and glossy, darkening to dry brown and going matt
// with kc.w blood.age. Splats fade out once their cells are under 1.5 px.
fn cond_blood(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let c = rc_voronoi2(rc_proj(i.world, i.g) * 5.0, b.seed ^ 77u, 0.8);
    let rad = 0.12 + 0.3 * fract(c.w * 13.0);
    let m = (1.0 - smoothstep(rad * 0.8, rad, c.x + (nz - 0.5) * 0.08)) * step(fract(c.w * 7.0), a) * rc_aa_world(f, 0.2);
    let age = rc_frame.kc.w;
    r.base = vec4<f32>(mix(o.base.rgb, mix(vec3<f32>(0.25, 0.006, 0.006), vec3<f32>(0.06, 0.02, 0.012), age), m), o.base.a);
    r.rough = mix(o.rough, mix(0.12, 0.7, age), m);
    r.metal = o.metal * (1.0 - m);
    return r;
}
