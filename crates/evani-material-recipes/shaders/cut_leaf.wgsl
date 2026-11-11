// evani.shader.recipes.cut_leaf
// Cut-out kind 7 (cut.wgsl), the procedural oak's leaf sprays (the Asset
// Studio oak session): foliage's leaf variant with extras bit 3. A card is a
// spray of four shoot tips (a 2x2 grid of the card, each nudged off its
// square's middle), each tip four to six long lobed oak leaves in a
// rosette round it or alternate along a short shoot, each leaf its own angle, length and lobing; about one tip in six
// is bare, so the spray has gaps. The card's whole uv cell picks the spray
// (cards a whole unit apart differ); the fractional uv draws it. Only the
// foliage material's shader and the shadow pass carry this text, with
// cut.wgsl spliced to reach it (shader.rs `leaf_cut`: rc_cut_kind's kind 7,
// rc_cut_geo's call, rc_cut_cov's far fade at least g.top), so no other
// material's recipe stage changes by a byte. Uses nothing but RcCutG, like
// cut.wgsl.
// Written fresh for Evani.Dev.

// A hash of cell c and item j in 0..1 (integer only: every device the same).
fn rc_cut_h(c: vec2<f32>, j: u32) -> f32 {
    let ci = vec2<i32>(c);
    var x = bitcast<u32>((ci.x * 73856093) ^ (ci.y * 19349663)) + j * 2654435761u;
    x = (x ^ (x >> 16u)) * 0x7feb352du;
    x = (x ^ (x >> 15u)) * 0x846ca68bu;
    return f32((x ^ (x >> 16u)) & 0xffffu) / 65535.0;
}

// The oak leaf spray at uv. m the distance to the nearest leaf (uv), d.y
// that leaf's own shade (0..1: the tip's and the leaf's), na its direction
// (uv, base to tip), d.x the offset across it (-1..1 over its half width);
// top the spray's share of the card, which a leaf fades to once it is a few
// pixels across (a card seen edge-on keeps its leaves).
fn rc_cut_leaf(uv: vec2<f32>) -> RcCutG {
    var g: RcCutG;
    let c = floor(uv);
    let sq = floor((uv - c) * 2.0);
    let id = vec2<f32>(c.x * 2.0 + sq.x, sq.y + 7.0);
    // The tip: its square's middle, nudged; its rosette turned at random.
    let q = (uv - c) * 2.0 - sq - vec2<f32>(0.5) - (vec2<f32>(rc_cut_h(id, 20u), rc_cut_h(id, 21u)) - 0.5) * 0.1;
    let rot = rc_cut_h(id, 22u) * 6.2832;
    let sz = 0.28 + 0.08 * rc_cut_h(id, 23u);
    let tone = rc_cut_h(id, 24u);
    g.m = 9.0;
    g.mc = 0.3;
    g.top = 0.05;
    if (rc_cut_h(id, 25u) < 0.17) {
        return g;
    }
    // Four to six leaves: about half the tips a rosette, unevenly spaced
    // round the tip; the rest a short shoot with its leaves alternate along
    // it, each a little smaller than the last.
    let nl = 4u + u32(rc_cut_h(id, 26u) * 2.99);
    let shoot = rc_cut_h(id, 27u) < 0.5;
    let sd = vec2<f32>(sin(rot), -cos(rot));
    for (var j = 0u; j < nl; j++) {
        let fj = f32(j);
        let a = select(rot + fj * 6.2832 / f32(nl), rot + select(-1.0, 1.0, (j & 1u) == 0u) * (0.75 + 0.4 * rc_cut_h(id, j + 20u)), shoot) + (rc_cut_h(id, j) - 0.5) * 0.8;
        let dir = vec2<f32>(sin(a), -cos(a));
        let ln = sz * (0.85 + 0.4 * rc_cut_h(id, j + 5u)) * select(1.0, 0.95 - 0.06 * fj, shoot);
        // A short stalk: the leaf starts a little out from the tip (or from
        // its node along the shoot).
        let p = q - dir * 0.04 - select(vec2<f32>(0.0), sd * (fj * 0.13 - 0.3), shoot);
        let y = dot(p, dir) / ln;
        let x = dot(p, vec2<f32>(-dir.y, dir.x)) / ln;
        // Long obovate (widest past the middle, about twice as long as
        // wide), four or five round lobes a side, alternate, the sinuses
        // cut more than halfway to the midrib.
        let yc = clamp(y, 0.0, 1.0);
        let env = 0.3 * pow(sin(3.1416 * yc), 0.6) * (0.5 + 0.6 * yc);
        let s = fract(y * (4.0 + 1.0 * rc_cut_h(id, j + 10u)) + select(0.0, 0.5, x < 0.0) + 0.3);
        let qd = 1.0 - min(s, 1.0 - s) * 2.0;
        let lobe = mix(1.0, 0.35 + 0.65 * sqrt(max(1.0 - qd * qd, 0.0)), smoothstep(0.08, 0.2, y));
        let w = max(env * lobe, 1e-3);
        // Distances in the square are half the card's uv.
        let dl = max((abs(x) - w) * ln * 0.7, max(-y, y - 1.0) * ln) * 0.5;
        if (dl < g.m) {
            g.m = dl;
            g.na = dir;
            g.d = vec2<f32>(clamp(x / w, -1.0, 1.0), mix(tone, rc_cut_h(id, j + 15u), 0.6));
        }
    }
    // The shoot the rosette grows from, down towards the card's foot.
    let tw = normalize(vec2<f32>(0.5, 0.95) - (uv - c));
    let along = dot(q, tw);
    g.m = min(g.m, max(abs(dot(q, vec2<f32>(-tw.y, tw.x))) * 0.5 - 0.004, max(-along, along - 0.6) * 0.5));
    return g;
}
