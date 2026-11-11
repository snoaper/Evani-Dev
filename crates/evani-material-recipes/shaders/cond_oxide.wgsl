// evani.shader.cond.oxide
// Oxide (bit 5): rust, verdigris or tarnish, in colour C of the block (a rust
// brown if C is black). It starts in cavities (low AO, creases) and on edges
// and spreads in patches with the amount x kc.y oxide.strength; rough and
// not metal.
fn cond_oxide(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let s = max(1.0 - o.occ, abs(f.curv) * 0.03);
    let t = 1.0 - 0.6 * a * rc_frame.kc.y;
    let v = rc_fbm2(rc_proj(i.world, i.g) * 6.0, b.seed, 3u);
    let m = smoothstep(t, t + 0.1, s * 0.5 + v * 0.7 + nz * 0.2) * min(a * 8.0, 1.0);
    var c = b.col_c;
    if (dot(c, c) < 1e-6) {
        c = vec3<f32>(0.16, 0.05, 0.015);
    }
    r.base = vec4<f32>(mix(o.base.rgb, c * (0.6 + 0.8 * v), m), o.base.a);
    r.rough = mix(o.rough, 0.85, m);
    r.metal = o.metal * (1.0 - m);
    return r;
}
