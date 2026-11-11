// evani.shader.recipes.cut_stage
// The colour pass's half of the cut-out (B3-P2, feature `cutout`): after the
// recipe, mat_cut drops the pixels in a hole (cut.wgsl's coverage against the
// 2x2 screen-door threshold), returns the coverage so the stage marks a kept
// edge pixel for the tonemap pass's blend, and shades the wires: round wire
// bends the normal across it (chain link, mesh), and where two sets cross
// the lower one darkens beside the upper (woven chain link, lattice slats;
// a grate's cross bars sit flush, a little darker). The holes are the same
// at every level and so are the crossings (lite draws what low draws, the
// same look on every device); medium adds the normals within 25 m. Like a leaf card's texture cut-out, the holes belong to the
// glTF alpha mask: only a MASK material (model_mat.flags.x) cuts them, after
// the lit stage's own alpha discard. Off with material.recipe.cut.enabled
// (off bit 4): solid.

struct RcCutOut {
    n: vec3<f32>,
    k: f32,
    c: f32,
};

fn mat_cut(f: MatFrame, uv: vec2<f32>, pos: vec2<f32>, n: vec3<f32>) -> RcCutOut {
    var o = RcCutOut(n, 1.0, 1.0);
    let kind = rc_cut_kind(rc_mat);
    if (kind == 0u || model_mat.flags.x < 0.5 || (rc_frame.misc.z & 4u) != 0u) {
        return o;
    }
    let g = rc_cut_geo(rc_mat, kind, uv);
    o.c = rc_cut_cov(g, f.duvx, f.duvy, rc_cut_mpx(rc_frame.misc.z));
    if (o.c < rc_cut_th(pos)) {
        discard;
    }
    let q = rc_q(0u);
    // The set this pixel's wire belongs to: the upper one where it covers,
    // else the nearer by its own width.
    let ub = g.top > 0.5;
    let du = abs(select(g.d.x, g.d.y, ub)) - select(g.hw.x, g.hw.y, ub);
    let onb = select(g.hw.y > 0.0 && abs(g.d.y) - g.hw.y < abs(g.d.x) - g.hw.x, ub, du < 0.0);
    let d = select(g.d.x, g.d.y, onb);
    let hw = max(select(g.hw.x, g.hw.y, onb), 1e-6);
    if ((kind == 2u || kind == 4u) && du >= 0.0) {
        // Beside the upper wire (or slat) the lower one lies in its shadow.
        o.k = mix(0.55, 1.0, smoothstep(0.0, 0.9 * hw, du));
    }
    if (kind == 1u && onb) {
        o.k = 0.85;
    }
    let near = rc_near(f) * rc_aa(f, hw * 4.0);
    if (q >= 2u && near > 0.0 && kind < 5u) {
        // Round wire across its width; flat slats and bars only round off
        // at their edges.
        let t = clamp(d / hw, -1.0, 1.0);
        let s = select(t, smoothstep(0.7, 1.0, abs(t)) * sign(t) * 0.7, kind == 1u || kind == 4u) * near;
        let a = rc_tuv(f, n) * select(g.na, g.nb, onb);
        o.n = normalize(n * sqrt(max(1.0 - s * s, 0.0)) + a * s);
    }
    return o;
}
