// ---- sky discs ----------------------------------------------------------------
//
// Feature `sky-discs` (src/discs.rs): the sun and the moon at their true
// apparent size, and the low sun's warmth on the sky. Records 14-24:
// [sun radius (rad), limb darkening, glow (times 0.04), glow width (rad)],
// [moon direction (unit, towards it), moon radius (rad); 0 no moon], [moon
// radiance, earthshine, maria, one pixel (rad)], [warmth, warmth height,
// its fall-off away from the sun's bearing, the sun's white core], [warm
// green, warm blue,
// skirt width, skirt share], [the light on the moon (unit, towards it;
// worked out on the CPU with its tilt and phase), the disc's brightness
// (1: the dome's own)], [how many times wider than tall the aureole's skirt
// lies along the horizon (1: round), how far the aureole takes the sun's
// own colour (0: the view line's, 1: the sun's line's), the sky's fine
// grain, the white core's radius (a share of the disc's)], [the share of the
// warmth that lies all round the horizon, the moon's softness (the half
// width in pixels of the tent it is sampled under), how fast its horns fade
// (0: not at all), the height the horizon's haze fades over], [the haze's
// own light (the sky's units), how much of it lies on the skyline], [the aureole's broad lobe (a share of the glow), the
// angle it falls off over (rad), how far the aureole keeps the sun's own
// colour below the sun's height too, the haze's unevenness], [the haze's
// own light towards the sun, the angle off the sun it gives way to the
// haze's light all round over (rad; 0: the same light all round)]. Record 14.x zero:
// off, the dome draws its own sun disc and sky. Angles off a disc's centre come from the cross product (the
// sine), not acos of the dot product, which loses its precision this close
// to 1. The code is kept short: a sky-only scene ships it whole.

fn mp_dk_on() -> bool {
    return arrayLength(&kit_data) > 24u && kit_data[14].x > .0;
}

// The haze's unevenness (1 when off): it does not lie the same all round
// and at every height, so the low sun's warm band reaches a few percent
// (kit_data[23].w) higher here and lower there and its glow is as much
// stronger or weaker, in slow swells some tens of degrees across, fixed to
// the world's bearings: a real evening sky's gradients are never quite
// regular.
fn mp_dk_un(d: vec3f) -> f32 {
    return 1. + select(.0, kit_data[23].w, mp_dk_on()) * mp_sky_noise(d.xz * 2.3 + d.y * vec2f(3., -2.) + vec2f(6.75, 1.75));
}

// The low sun's warmth on the clear sky's scattered light (1 when off): the
// blue and some green gone near the horizon (full along the skyline, a
// bell over the height), gathered round the sun's bearing, a share of it
// (kit_data[21].x) all round the horizon: the whole skyline is warm at
// sunset, most of all under the sun. `u`: the haze's unevenness here.
fn mp_dk_warm(d: vec3f, s: vec3f, u: f32) -> vec3f {
    let a = kit_data[17];
    let y = max(d.y, .0) / a.y / u;
    let k = a.x * exp(-y * y) * mix(exp((dot(normalize(d.xz + 1e-6), normalize(s.xz + 1e-6)) - 1.) * a.z), 1., kit_data[21].x);
    return select(vec3f(1.), 1. - k * vec3f(.0, 1. - kit_data[18].xy), mp_dk_on());
}

// The air along a line at height `y` (the dome's own optical length, in
// zenith airs).
fn mp_dk_air(y: f32) -> f32 {
    let c = clamp(y, .0, 1.);
    return 1. / (c + .15 * pow(93.885 - degrees(acos(c)), -1.253));
}

// The haze along the horizon (the colour as it is when off): the lowest
// degree or two of the sky seen through the thick air and dust lying on the
// land, a veil with a dim grey-blue light of its own (kit_data[22].w of the
// way to it on the skyline, fading out in a bell over the height
// kit_data[21].w: nearly whole for the first half degree, a third left at
// that height), so a sunset's orange dulls to a dusky band just over the
// land instead of deepening all the way down. Towards the sun the dust is
// lit from behind: there the veil's light is kit_data[24].xyz (a rose red
// under a setting sun), giving way to the grey all round over the angle
// kit_data[24].w off the sun (the angle from the arc tangent of the cross
// and dot products, exact near the sun and all the way round).
fn mp_dk_haze(c: vec3f, d: vec3f, s: vec3f) -> vec3f {
    let h = kit_data[22];
    let q = kit_data[24];
    let y = max(d.y, .0) / kit_data[21].w;
    return mix(c, mix(h.xyz, q.xyz, exp(-atan2(length(cross(d, s)), dot(d, s)) / max(q.w, 1e-6))), select(.0, h.w * exp(-y * y), mp_dk_on()));
}

// The sky's fine grain (1 when off): each colour a hair brighter or darker
// from one point of the sky to the next, by under one level of an 8-bit
// screen, so its slow gradients show no bands (and have a photograph's
// grain instead of a drawn smoothness).
fn mp_dk_grain(d: vec3f) -> vec3f {
    let p = d * 913.;
    return 1. + select(.0, kit_data[20].z, mp_dk_on()) * (fract(sin(vec3f(dot(p, vec3f(12.99, 78.23, 37.72)), dot(p, vec3f(39.35, 11.14, 83.16)), dot(p, vec3f(73.16, 52.24, 9.15)))) * 43758.5) - .5);
}

// How much of a tent of half width 1 lies below `t` (0 at -1, 1 at 1).
fn mp_dk_cdf(t: f32) -> f32 {
    let c = clamp(t, -1., 1.);
    return .5 + c - .5 * c * abs(c);
}

// The sun disc (limb-darkened, through the air along this line) and its
// aureole, then the sunlit moon, as light added to the clear sky (the
// dome's units, before its brightness). `sc` is the sun's light reaching
// the eye along `d` (sun_e * fex), `un` the haze's unevenness there.
fn mp_discs(d: vec3f, s: vec3f, sc: vec3f, fex: vec3f, on: f32, un: f32) -> vec3f {
    let rs = kit_data[14];
    let rm = kit_data[15];
    let lk = kit_data[16];
    let sk = kit_data[18];
    let n = kit_data[20];
    let q = kit_data[23];
    var o = vec3f();
    if dot(d, s) > .0 {
        let a = length(cross(d, s));
        let x = a / rs.x;
        // The disc: limb darkening by the cosine of the photosphere's
        // slope (u = rs.y), the rim a little redder (its light leaves
        // through more of the sun's own atmosphere), a white core out to
        // n.w of the radius, gone within an eighth of the radius either
        // side of that (a low sun's middle burns out to a clean white disc
        // in a photograph, smaller than the sun: its rim, dimmed and
        // reddened by the thick air, stays at the glare's orange;
        // kit_data[17].w, 0 none); its outline smoothed over
        // two pixels or at least 0.15 of its radius, so a disc no brighter
        // than a few times white (kit_data[19].w) runs from its white
        // middle through yellow into the orange glare, as a lens draws a
        // burnt-out sun.
        let e = max(lk.w / rs.x, .15);
        let u = max(1. - x * x, .0);
        o = 760. * kit_data[19].w * (min(sc, vec3f(80.)) + sc.x * kit_data[17].w * (1. - smoothstep(n.w - .12, n.w + .12, x))) * (1. - rs.y * (1. - sqrt(u)) * vec3f(.85, 1., 1.25)) * (1. - smoothstep(1. - e, 1. + e, x)) * on;
        // The aureole: the haze's forward glow, a tight core and a wider
        // skirt. The skirt lies along the horizon (n.x times wider than
        // tall: the haze that makes it is thickest along the skyline, so
        // a setting sun's glow is a flattened oval, not a column over it).
        // Its light came down the sun's own line through the air, so with
        // n.y it keeps the low sun's deep orange above the sun too instead
        // of the paler colour of the shorter view line (c: the view
        // line's air raised to the sun line's). A third, broad lobe
        // (q: a share of the glow, falling off over tens of
        // degrees) is the haze's faint brightening of the whole sky on
        // the sun's side (c: the view
        // line's air raised to the sun line's). Like the sky's own light
        // it is warmed further this low. Below the sun's height the view
        // line's air is the longer one: with q.z the glow keeps
        // the sun's own colour there too (the view line's air lowered to
        // the sun line's) instead of sinking to a dark red on the skyline.
        let g = asin(min(a, 1.));
        let v = asin(d.y) - asin(s.y);
        let w = sqrt(max(g * g - v * v, .0) / max(n.x * n.x, 1.) + v * v);
        let ar = mp_dk_air(s.y) / mp_dk_air(d.y) - 1.;
        let c = sc * pow(max(fex, vec3f(1e-9)), vec3f(select(q.z, n.y, ar > .0) * ar));
        o += rs.z * c * ((1. - sk.w) * exp(-g / rs.w) + sk.w * exp(-w / (rs.w * sk.z)) + q.x * exp(-g / q.y)) * mp_dk_warm(d, s, un) * un;
    }
    let m = rm.xyz;
    let t = d - m * dot(d, m);
    // This pixel on the moon's disc, in a frame on the sky round it; r is
    // the half width of the tent the moon is seen under, in the disc's
    // units (b.y pixels: 1.3 a crisp line, 2 a phone camera's soft one).
    let e1 = normalize(vec3f(-m.z, .0, m.x) + 1e-5);
    let e2 = cross(e1, m);
    let p = vec2f(dot(t, e1), dot(t, e2)) / rm.w;
    let b = kit_data[21];
    let r = b.y * lk.w / rm.w;
    if rm.w <= .0 || dot(d, m) <= .0 || length(p) > 1. + 1.5 * r {
        return o;
    }
    // The light on the moon: c the cosine of its angle from the moon, lp
    // where it comes from on the disc. Along a line towards the light the
    // lit strip runs from the terminator (c w) to the limb (w, the disc's
    // half chord there), so its share of the tent is exact (mp_dk_cdf): a
    // crescent far thinner than a pixel is one smooth tapering arc to its
    // horns, with no beads or gaps, and a fuller moon's terminator is as
    // soft. Six such lines side by side, under the same tent. Each is
    // shaded at the middle of what the tent sees of the strip:
    // Lommel-Seeliger (the regolith's flat face, bright to its edge) with
    // a little Lambert, broad dark maria from noise fixed to the face (it
    // always shows us one side), earthshine on the dark side. A thin
    // crescent's horns fade before they reach the ends of the limb (b.z:
    // the grazing sunlight there is lost in the shadows of the moon's own
    // relief, Danjon's shortened cusps): dimmed by the half chord raised
    // to b.z, less for a fuller moon (none from half on).
    let l = kit_data[19].xyz;
    let c = dot(m, l);
    let lp = normalize(vec2f(dot(l, e1), dot(l, e2)) + 1e-6);
    let lq = vec2f(-lp.y, lp.x);
    let a = dot(p, lp);
    var f = .0;
    for (var i = 0; i < 6; i++) {
        let k = (f32(i) + .5) / 3. - 1.;
        let y = dot(p, lq) + k * r;
        let w = sqrt(max(1. - y * y, .0));
        let x = .5 * (max(c * w, a - r) + min(w, a + r));
        let z = sqrt(max(1. - x * x - y * y, .0));
        let v = max(x * sqrt(max(1. - c * c, .0)) - z * c, .0);
        let n1 = mp_dk_cdf((c * w - a) / r);
        f += (1. - abs(k)) * ((1.6 * v / (v + max(z, .05)) + .4 * v) * (mp_dk_cdf((w - a) / r) - n1) * pow(max(w, 1e-4), b.z * max(c, .0)) + lk.y * (n1 - mp_dk_cdf((-w - a) / r))) * (1. - lk.z * smoothstep(.0, .5, mp_sky_noise((lp * x + lq * y) * 2.2 + 5.)));
    }
    return o + f / 3. * lk.x * fex;
}
