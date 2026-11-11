// evani.shader.cond.mud
// Mud (bit 1): rises from the ground, so from world height ka.z (mud.ground,
// m; the fragment stage has no object origin) up to ka.w x the amount
// (mud.height, m), its edge broken up by the shared noise; dark and wet at
// the foot, drier, lighter and rougher at the top. kd.x the colour. Medium
// and up: it softens the normal.
fn cond_mud(b: RecBlock, a: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    let t = max(rc_frame.ka.w * a, 1e-3);
    let h = (i.world.y - rc_frame.ka.z + (nz - 0.5) * 0.4 * t) / t;
    let m = (1.0 - smoothstep(0.75, 1.0, h)) * min(a * 8.0, 1.0);
    let dry = clamp(h, 0.0, 1.0);
    let c = rc_lin(unpack4x8unorm(rc_frame.kd.x).rgb) * mix(0.55, 1.25, dry);
    r.base = vec4<f32>(mix(o.base.rgb, c, m), o.base.a);
    r.rough = mix(o.rough, mix(0.3, 0.95, dry), m);
    r.metal = o.metal * (1.0 - m);
    if (rc_q(6u) >= 2u) {
        r.n = normalize(mix(o.n, i.g, m * 0.6));
    }
    return r;
}
