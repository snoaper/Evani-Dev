// evani.shader.recipes.cut_leaf_stage
// The colour pass's shading of an oak leaf spray (cut_leaf.wgsl, cut-out kind
// 7): each leaf its own shade and tilt, folded a little along its midrib, so
// a spray reads as separate small leaves, some catching the light and some
// in their neighbours' shade (g.d.y the leaf's shade); crisp edges, never
// blended with what is behind. Only the foliage
// material's shader carries it, mat_cut spliced to call it for kind 7
// (shader.rs `leaf_cut`). Written fresh for Evani.Dev.

fn rc_cut_leaf_n(f: MatFrame, g: RcCutG, n: vec3<f32>, o: RcCutOut) -> RcCutOut {
    var r = o;
    // Each leaf's own shade, a paler midrib, and the blade folded along it.
    // A far-filled card (extras bit 4: g.top 2 or 3, the rivers' crowns)
    // keeps the same mean shade with less spread, and once its leaves are
    // only a few pixels across they fade to the card's own shade and normal
    // (lv): each sub-pixel leaf's own tilt and shade glinted as grey speckle
    // over every distant crown. Other leaves: lv 1, as before.
    let fk = g.top > 1.5;
    let lv = select(1.0, smoothstep(1.5, 5.0, g.mc / max(0.5 * (length(f.duvx) + length(f.duvy)), 1e-7)), fk);
    r.k = mix(0.75, select(0.35, 0.5, fk) + select(0.8, 0.5, fk) * g.d.y, lv) * (1.0 + 0.25 * lv * (1.0 - smoothstep(0.0, 0.12, abs(g.d.x))));
    // A leaf's edge stays crisp: only its faintest pixels keep the
    // partial-coverage mark, so the tonemap pass doesn't blend most of each
    // leaf's rim with the sky behind (pale fringes round every leaf of a
    // crown), yet a crown's outline isn't dithered.
    r.c = select(1.0, o.c, o.c < 0.4);
    let ac = vec2<f32>(-g.na.y, g.na.x);
    let t = rc_tuv(f, n) * (g.na * (g.d.y - 0.5) * 1.1 + ac * (g.d.x * 0.6 + (fract(g.d.y * 7.0) - 0.5) * 1.3));
    r.n = normalize(n + t * lv);
    return r;
}
