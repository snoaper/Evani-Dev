// evani.shader.cond.snow
// Snow (bit 3): covers up-facing pixels, reaching down to slopes as steep as
// snow.angle as the amount grows (kb.y is its cosine), its edge broken up by
// the shared noise. kb.z snow.frost lightens what the snow leaves bare.
// Lite: a white tint on up-facing pixels. Medium and up: the snow smooths
// the normal. High and up: frost and snow sparkle.
fn cond_snow(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let q = rc_q(6u);
    let w = vec3<f32>(0.85, 0.88, 0.92);
    if (q == 0u) {
        r.base = vec4<f32>(mix(o.base.rgb, w, a * 0.6 * clamp(i.g.y, 0.0, 1.0)), o.base.a);
        return r;
    }
    let t = mix(1.05, rc_frame.kb.y, a);
    let s = smoothstep(t, t + 0.1, o.n.y + (nz - 0.5) * 0.2);
    let fr = rc_frame.kb.z * a * (1.0 - s);
    r.base = vec4<f32>(mix(mix(o.base.rgb, w, fr * 0.35), w, s), o.base.a);
    r.rough = mix(mix(o.rough, 0.5, fr), 0.65, s);
    r.metal = o.metal * (1.0 - s);
    if (q >= 2u) {
        r.n = normalize(mix(o.n, i.g, s * 0.7));
    }
    if (q >= 3u) {
        let h = rc_u01(rc_hash3(vec3<i32>(floor(i.world * 220.0)), 7u));
        r.glint = r.glint + vec3<f32>(step(0.985, h) * (s + fr) * rc_aa_world(f, 0.0045) * 2.0);
    }
    return r;
}
