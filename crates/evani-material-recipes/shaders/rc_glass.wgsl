// evani.shader.rc.glass
// Glass (recipe 24, batch 3 build plan 3.3; drawn with B3-P3's see-through
// pass). Written fresh for Evani.Dev: window, bottle and crystal glass,
// stained glass that glows when the sun is behind it, leaded diamond panes,
// wired safety glass and glass blocks, for any uv surface (pattern space is
// uv times pane_scale). The recipe writes a straight colour and its alpha:
// the see-through pass premultiplies and blends it (crate::blend: OVER, or
// REFRACT for frosted, wavy and block glass, which also bend and blur what is
// behind); drawn opaque (blend off), it is a tinted gloss.
//   numbers: na = thickness, frost, waviness, pane scale (1-40 per uv);
//            nb.x = backlight, nb.y = swirl (P8-4: a coloured ribbon inside,
//            colour B, its width; 0 none; every tier)
//   colours: A tint (what comes through; also the body's colour), B came (the
//            lead lines, the wire, a block's joints)
//   variants: 0 window, 1 bottle (darker towards the edges), 2 crystal (facet
//   sparkle), 3 stained (deep colour, seeds and streaks), 4 leaded (diamond
//   panes in lead), 5 wired (a square wire grid inside), 6 block (square units
//   with lumpy faces and a thick edge)
//   extras: backlight (bit 0: the sun's glow through the pane)
// Levels (q, the glass group): every level draws the same panes, lines,
// tint, frost and waviness (the same look on every device; lite's streaks
// come from two sines instead of noise taps); medium adds the
// stained glass's seeds and the crystal's sparkle; high adds a second, finer
// ripple. Lines fade to their share of colour under 1.5 px (rc_aa), so a far
// leaded window reads as an even tint and never shimmers.

// The wave height at p (pattern space): long streaks along v, as drawn
// crown and cylinder glass have.
fn rc_glass_h(p: vec2<f32>, seed: u32, fine: bool) -> f32 {
    var h = rc_vnoise2(vec2<f32>(p.x * 3.0, p.y * 0.8), seed);
    if (fine) {
        h = h + 0.35 * rc_vnoise2(p * 9.0, seed + 5u);
    }
    return h;
}

fn rc_glass(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let th = b.na.x;
    let frost = b.na.y;
    let wav = b.na.z;
    let sc = rc_range(b.na.w, 1.0, 40.0);
    let p = i.uv * sc;
    let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
    let tint = b.col_a * i.tex.rgb * i.vcol;
    var body = tint * mix(0.12, 0.6, frost);
    var a = 0.05 + 0.08 * th + 0.35 * frost;
    var rough = 0.03 + 0.5 * frost;
    var metal = 0.0;
    var n = i.n;
    var wv = wav;
    var line = 0.0;
    var line_col = b.col_b;
    let px = f.uv_px * sc;
    if (b.variant == 1u) {
        a = mix(0.35, 0.8, (1.0 - ndv) * th + 0.2);
        body = tint * 0.5;
    } else if (b.variant == 3u) {
        a = 0.55 + 0.25 * th;
        body = tint * 0.7;
        if (q >= 2u) {
            let s = rc_vnoise2(p * 2.0, b.seed);
            body = body * (0.8 + 0.4 * s);
            a = a * (0.9 + 0.2 * s);
        }
    } else if (b.variant == 4u) {
        let c = rc_lay_basket(rc_lay_rot45(p), 1.0, 1.0, b.seed);
        line = rc_fade(1.0 - smoothstep(0.045 - px, 0.045 + px, c.e), 0.18, rc_aa(f, 0.09 / sc));
        body = body * (0.85 + 0.3 * c.id);
        wv = wv + 0.25 * (1.0 - smoothstep(0.0, 0.25, c.e));
    } else if (b.variant == 5u) {
        let g = abs(fract(p + 0.5) - 0.5);
        let e = min(g.x, g.y);
        let k = rc_aa(f, 0.05 / sc);
        line = rc_fade(1.0 - smoothstep(0.022 - px, 0.022 + px, e), 0.09, k);
        metal = 0.7 * line;
    } else if (b.variant == 6u) {
        let c = rc_lay_basket(p, 1.0, 1.0, b.seed);
        let k = rc_aa(f, 0.06 / sc);
        line = rc_fade(1.0 - smoothstep(0.03 - px, 0.03 + px, c.e), 0.12, k);
        // The thick edge: darker and less clear towards each unit's sides.
        let edge = 1.0 - smoothstep(0.03, 0.2, c.e);
        a = a + 0.35 * edge + 0.1;
        body = body * (1.0 - 0.4 * edge);
        wv = max(wv, 0.6);
    }
    if (b.nb.y > 0.0) {
        // Swirl: a ribbon of colour B twisted inside (a marble's), three
        // vanes round u turning along v, wider by the swirl number; fainter
        // towards the rim, where the eye looks through more glass.
        let r = (1.0 - smoothstep(b.nb.y * 0.2, b.nb.y * 0.2 + px * 3.0 + 0.02, abs(fract(i.uv.x * 3.0 + i.uv.y * 1.5) - 0.5))) * ndv;
        body = mix(body, b.col_b, r);
        a = mix(a, 0.95, r);
    }
    // Waviness and the block's lumps bend the normal (the refraction offset
    // reads it too), within 25 m.
    if (wv > 0.0) {
        let t = rc_tuv(f, i.n);
        var d: vec2<f32>;
        if (q == 0u) {
            // Lite: the same streaks from two sines, their slope exact (no
            // noise taps).
            let ph = p.x * 3.0 + sin(p.y * 0.8 + f32(b.seed)) * 1.7;
            d = vec2<f32>(cos(ph) * 1.5, cos(p.y * 0.8 + f32(b.seed)) * 1.7 * cos(ph) * 0.4);
        } else {
            let e = 0.04;
            let fine = q >= 3u;
            let h0 = rc_glass_h(p, b.seed, fine);
            d = vec2<f32>(rc_glass_h(p + vec2<f32>(e, 0.0), b.seed, fine) - h0, rc_glass_h(p + vec2<f32>(0.0, e), b.seed, fine) - h0) / e;
        }
        n = rc_bump(i.n, t, d * (wv * 0.08 * rc_near(f)));
    }
    if (b.variant == 2u && q >= 2u) {
        rough = 0.02;
    }
    // Less comes through at a grazing angle (Schlick), so a pane seen edge-on
    // reads as glass, not a hole.
    let fr = pow(1.0 - ndv, 5.0);
    a = clamp(a + (1.0 - a) * fr * 0.6, 0.0, 1.0);
    var o = MatOut(vec4<f32>(body, a * i.base.a), rough, metal, n, i.occ, vec3<f32>(0.0), vec4<f32>(0.0), vec3<f32>(0.0));
    // The sun behind the pane shines through its colour.
    if (rc_extra(b, RC_X_GLASS, 0u)) {
        let back = max(-dot(i.g, frame.light_dir.xyz), 0.0);
        o.emit = tint * frame.light_color.rgb * (back * back * rc_range(b.nb.x, 0.0, 1.0) * (1.0 - a * 0.5) * (1.0 - line) * 0.6);
    }
    if (b.variant == 2u && q >= 2u) {
        let s = rc_voronoi2(i.uv * 40.0, b.seed, 1.0);
        o.glint = vec3<f32>(step(0.97, s.w) * (1.0 - smoothstep(0.0, 0.2, s.x)) * 2.0);
    }
    if (line > 0.0) {
        o.base = mix(o.base, vec4<f32>(line_col * i.tex.rgb, 1.0), line);
        o.rough = mix(rough, 0.55, line);
        o.n = normalize(mix(n, i.n, line));
        o.emit = o.emit * (1.0 - line);
    }
    return o;
}
