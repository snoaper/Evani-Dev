// evani.shader.cond.dust
// Dust (bit 2): settles on top faces (world normal y) and in cavities (low AO
// and creases, weighted by kb.x dust.cavity), patchy with the shared noise;
// dull, dry and not metal. kd.y the colour. Lite: a flat tint toward it.
fn cond_dust(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let c = rc_lin(unpack4x8unorm(rc_frame.kd.y).rgb);
    if (rc_q(6u) == 0u) {
        r.base = vec4<f32>(mix(o.base.rgb, c, a * 0.35), o.base.a);
        return r;
    }
    let s = max(smoothstep(0.2, 0.9, o.n.y), max(1.0 - o.occ, -f.curv * 0.25) * rc_frame.kb.x);
    let d = clamp(s * a * (0.4 + nz), 0.0, 1.0);
    r.base = vec4<f32>(mix(o.base.rgb, c, d), o.base.a);
    r.rough = mix(o.rough, 0.92, d);
    r.metal = o.metal * (1.0 - d);
    return r;
}
