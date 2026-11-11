// evani.shader.cond.wet
// Wet (condition bit 0): darkens the colour (porous surfaces much more than
// metal) and lowers roughness; puddles gather only on flat, up-facing pixels,
// from world-space noise (rc_frame.ka.x wet.puddles, ka.y wet.puddle_scale in
// patches per metre). Lite: the darkening only. Medium and up: puddles also
// flatten the normal.
fn cond_wet(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let q = rc_q(6u);
    r.base = vec4<f32>(o.base.rgb * mix(1.0, mix(0.45, 0.85, o.metal), a), o.base.a);
    if (q == 0u) {
        return r;
    }
    r.rough = mix(o.rough, o.rough * 0.3 + 0.04, a);
    let v = mix(rc_vnoise2(i.world.xz * rc_frame.ka.y, 11u), nz, 0.3);
    let p = smoothstep(0.85, 0.97, i.g.y) * smoothstep(0.0, 0.08, rc_frame.ka.x * a - v);
    r.base = vec4<f32>(r.base.rgb * (1.0 - 0.3 * p), o.base.a);
    r.rough = mix(r.rough, 0.02, p);
    if (q >= 2u) {
        r.n = normalize(mix(r.n, i.g, p));
    }
    return r;
}
