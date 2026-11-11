// evani.shader.cond.wear
// Wear (bit 4): follows convex edges (the curvature mat_frame measured,
// scaled by kb.w wear.edge), broken up by the shared noise, plus scratch
// lines from Medium (kc.x wear.scratches). Metal polishes (brighter and
// smoother), paint shows colour B (the layer under it), anything else
// scuffs lighter and rougher.
fn cond_wear(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let e = clamp(f.curv * rc_frame.kb.w, 0.0, 1.0);
    var w = smoothstep(0.55, 0.7, (e + (nz - 0.5) * 0.5) * a);
    if (rc_q(6u) >= 2u) {
        let p = rc_proj(i.world, i.g);
        let s = rc_vnoise2(vec2<f32>(p.x * 2.0 + p.y * 0.9, p.y * 70.0 - p.x * 30.0), b.seed);
        w = max(w, smoothstep(0.78, 0.84, s) * rc_frame.kc.x * a * 2.0 * rc_aa_world(f, 0.014));
    }
    var u = mix(o.base.rgb * 1.3 + vec3<f32>(0.04), min(o.base.rgb * 1.6 + vec3<f32>(0.03), vec3<f32>(1.0)), o.metal);
    if (b.recipe == 2u) {
        u = b.col_b;
    }
    r.base = vec4<f32>(mix(o.base.rgb, u, w), o.base.a);
    r.rough = mix(o.rough, mix(min(o.rough + 0.2, 1.0), o.rough * 0.5, o.metal), w);
    return r;
}
