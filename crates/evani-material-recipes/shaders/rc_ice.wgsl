// evani.shader.rc.ice
// Ice (recipe 27, batch 1 build plan section 3, package P8). Written fresh for
// Evani.Dev: ice, snow and frost as a surface, no refraction. A blue-white ramp
// by view angle (light at the edge, deep blue where you look through), cracks
// inside the ice (the F2-F1 lines of a cellular noise, a second deeper layer
// that slides with the view at high), a frost cover of fbm, sparkle specks
// that wink with the view, a wrap scatter and an optional ripple for icicles.
// World space: the pattern lies on the plane facing the dominant axis of the
// normal (blended from two planes at high), so ground, slabs and props need no
// uv; a prop that moves swims through its pattern.
//   numbers: na = clarity, crack amount, crack scale (1-50 per m), sparkle;
//            nb = scatter, frost
//   colours: A colour, B deep, C rim (and the cracks and frost)
//   variants: 0 clear, 1 blue, 2 frost (needle streaks), 3 icicle (ripples)
//   extras: inner cracks (bit 0), sparkle (bit 1), scatter (bit 2), ridges
//           (bit 3, batch 2: wind ridges and a shiny crust, snow.crust)
//   powder (Asset Studio column 76, no new number or extra): the frost
//     variant with clarity 0, frost 1 and the inner cracks extra at a crack
//     amount over 0.8 draws fresh powder (rc_ice_p): soft clumps, a matte
//     grain and rare glints, a pattern in space, the crack scale its lumps
//     per metre. No catalogue
//     row has that, so every row draws as it did.
// Levels (q, the glass group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was white with a blue rim); low adds the
// ramp, one crack layer and the frost, anti-aliased (cells under 1.5 px fade
// to their average); medium adds crack and ripple normals within 25 m, the
// scatter and, for snow (frost above 0.3), close-up grain, soft clumps and
// their relief (each fades out once finer than a pixel); high adds the second
// crack layer with 2-step parallax, the sparkle and the snow's own glints:
// grains with a random tilt that flash where their mirror direction meets the
// view, so they follow the sun as you walk; ultra draws as high.

// Wind ridges on one plane (batch 2): a slow directional sine warped by
// noise, 0 in a trough to 1 on a crest.
fn rc_ice_w(b: RecBlock, p: vec2<f32>, cs: f32) -> f32 {
    return 0.5 + 0.5 * sin(6.2832 * (dot(p, vec2<f32>(0.94, 0.34)) * cs * 0.5 + rc_fbm2(p * (cs * 0.15), b.seed + 10u, 2u) * 2.0));
}

// The height on one plane: a groove along the cracks and the grain of frost.
fn rc_ice_g(b: RecBlock, p: vec2<f32>, cs: f32, gk: f32) -> f32 {
    let c = rc_voronoi2(p * cs, b.seed + 1u, 1.0);
    let groove = (1.0 - smoothstep(0.0, 0.08, c.z)) * step(c.w, 0.25 + 0.75 * b.na.y) * b.na.y * select(0.0, 1.0, rc_extra(b, RC_X_ICE, 0u));
    let sn = smoothstep(0.3, 0.7, b.nb.y);
    var h = rc_vnoise2(p * (cs * 3.0), b.seed + 5u) * (0.5 * b.nb.y) - groove + sn * (rc_fbm2(p * (cs * 0.7), b.seed + 8u, 2u) * 3.0 + gk * rc_vnoise2(p * (cs * 5.0), b.seed + 6u) * 2.0);
    if (rc_extra(b, RC_X_ICE, 3u)) {
        h = h + 2.0 * rc_ice_w(b, p, cs);
    }
    return h;
}

// The height at world position w with normal n (two planes blended, as the
// colour is at high), and for icicles rings along the vertical.
fn rc_ice_h(b: RecBlock, w: vec3<f32>, n: vec3<f32>, cs: f32, gk: f32) -> f32 {
    let pr = rc_proj2(w, n);
    var h = mix(rc_ice_g(b, pr.a, cs, gk), rc_ice_g(b, pr.b, cs, gk), pr.w);
    if (b.variant == 3u) {
        h = h + 0.5 * sin(w.y * cs * 12.0 + 3.0 * rc_vnoise2(w.xz * (cs * 0.5), b.seed + 2u));
    }
    return h;
}

// One crack plane at depth d (m under the surface): the lines of a cellular
// noise, `p` the plane's coordinates and `vp` the view's in the same plane,
// so a deeper layer shifts against the surface as you move.
fn rc_ice_l(b: RecBlock, p: vec2<f32>, vp: vec2<f32>, d: f32, cs: f32) -> f32 {
    let c = rc_voronoi2((p - vp * d) * cs, b.seed + 1u + u32(d * 40.0), 1.0);
    return (1.0 - smoothstep(0.0, 0.08, c.z)) * step(c.w, 0.25 + 0.75 * b.na.y);
}

fn rc_ice_k(b: RecBlock, f: MatFrame, i: MatIn, hi: bool, cs: f32, d: f32) -> f32 {
    if (hi) {
        let pa = rc_proj2(i.world, i.n);
        let va = rc_proj2(f.v, i.n);
        return mix(rc_ice_l(b, pa.a, va.a, d, cs), rc_ice_l(b, pa.b, va.b, d, cs), pa.w);
    }
    return rc_ice_l(b, rc_proj(i.world, i.n), rc_proj(f.v, i.n), d, cs);
}

// Fresh powder (Asset Studio column 76): on opaque, fully frosted snow (the
// frost variant, clarity 0, frost 1) the inner cracks at their fullest break
// the cover up into loose crystals. No catalogue row has that, so every row
// draws as it did.
fn rc_ice_pm(b: RecBlock) -> bool {
    return b.variant == 2u && b.nb.y > 0.95 && b.na.x < 0.05 && b.na.y > 0.8 && rc_extra(b, RC_X_ICE, 0u);
}

// Smooth noise in space with its slope: x the value (0..1), yzw how fast it
// changes along each axis of p.
fn rc_ice_nd(p: vec3<f32>, seed: u32) -> vec4<f32> {
    let fl = floor(p);
    let i = vec3<i32>(fl);
    let t = p - fl;
    let u = rc_fade5(t);
    let du = 30.0 * t * t * (t - vec3<f32>(1.0)) * (t - vec3<f32>(1.0));
    let a = rc_u01(rc_hash3(i, seed));
    let b = rc_u01(rc_hash3(i + vec3<i32>(1, 0, 0), seed));
    let c = rc_u01(rc_hash3(i + vec3<i32>(0, 1, 0), seed));
    let d = rc_u01(rc_hash3(i + vec3<i32>(1, 1, 0), seed));
    let e = rc_u01(rc_hash3(i + vec3<i32>(0, 0, 1), seed));
    let f = rc_u01(rc_hash3(i + vec3<i32>(1, 0, 1), seed));
    let g = rc_u01(rc_hash3(i + vec3<i32>(0, 1, 1), seed));
    let h = rc_u01(rc_hash3(i + vec3<i32>(1, 1, 1), seed));
    let k1 = b - a;
    let k2 = c - a;
    let k3 = e - a;
    let k4 = a - b - c + d;
    let k5 = a - c - e + g;
    let k6 = a - b - e + f;
    let k7 = b + c - a - d + e - f - g + h;
    return vec4<f32>(
        a + k1 * u.x + k2 * u.y + k3 * u.z + k4 * u.x * u.y + k5 * u.y * u.z + k6 * u.z * u.x + k7 * u.x * u.y * u.z,
        du * vec3<f32>(k1 + k4 * u.y + k6 * u.z + k7 * u.y * u.z, k2 + k5 * u.z + k4 * u.x + k7 * u.z * u.x, k3 + k6 * u.x + k5 * u.y + k7 * u.x * u.y));
}

// The powder's relief at world point w and its slope, and the relief of its
// two largest sizes alone (lo).
struct RcIcePb {
    o: vec4<f32>,
    lo: f32,
}

// The powder's relief at world point w: clumps of every size, smooth noise in
// space at up to four sizes, each a little under half the last (a fifth of a
// lump, 5 mm at 40 lumps a metre, down to half a millimetre) and turned
// against it, so no size and no lattice shows: no cells, no outlines. o.x
// the relief (about -0.9..0.9, in fifths of a lump), o.yzw its slope along
// each world axis (per metre); kb says how much of each size survives the
// anti-alias fade.
fn rc_ice_pb(b: RecBlock, w: vec3<f32>, kb: vec4<f32>, sizes: u32) -> RcIcePb {
    let cs = rc_range(b.na.z, 1.0, 50.0) * 5.0;
    let r = mat3x3<f32>(vec3<f32>(0.0, -0.8, -0.6), vec3<f32>(0.8, 0.36, -0.48), vec3<f32>(0.6, -0.48, 0.64));
    var kv = kb;
    var p = w * cs + vec3<f32>(3.1, 0.0, 7.7);
    var m = mat3x3<f32>(vec3<f32>(cs, 0.0, 0.0), vec3<f32>(0.0, cs, 0.0), vec3<f32>(0.0, 0.0, cs));
    var a = 1.0;
    var o = vec4<f32>(0.0);
    var lo = 0.0;
    for (var k = 0u; k < sizes; k = k + 1u) {
        let nz = rc_ice_nd(p, b.seed + 11u + k);
        o = o + (a * kv[k]) * vec4<f32>(nz.x - 0.5, nz.yzw * m);
        if (k == 1u) {
            lo = o.x;
        }
        p = r * p * 2.13 + vec3<f32>(1.7, 9.2, 5.3);
        m = r * m * 2.13;
        a = a * 0.5;
    }
    return RcIcePb(o, lo);
}

// The powder's crystals: one in each cell of w's lattice, a soft round grain
// somewhere in its middle (the surface cuts each at its own height, so they
// show at every size up to the cell's, with matte snow between them). xyz
// the crystal's own tilt (-0.5..0.5 each way), w how far inside the grain
// the point lies (0 outside it to 1 at its heart).
fn rc_ice_c(w: vec3<f32>, seed: u32) -> vec4<f32> {
    let c = floor(w);
    let h = rc_hash3(vec3<i32>(c), seed);
    let r = vec3<f32>(rc_u01(h), rc_u01(rc_pcg(h)), rc_u01(rc_pcg(h + 1u))) - vec3<f32>(0.5);
    return vec4<f32>(r, 1.0 - smoothstep(0.2, 0.48, length(w - c - vec3<f32>(0.5) - r * 0.4)));
}

// Fresh powder (round 3): loose clumps of every size from 5 mm down to half
// a millimetre (the crack scale is the lumps per metre; a clump is round on
// top, the gaps between clumps are small cool pockets, never an outline),
// a slight relief with its own shade towards a low light, and a crisp grain of
// crystals of two sizes under a millimetre, each with its own tilt: it turns
// the light a little, and where its mirror direction comes near the view it
// shows as a pinpoint of light, most of them faint and a few bright (the
// sparkle number). Light goes through the powder (the scatter number): it
// wraps round a form and the shade side glows cool. Its value changes only
// with its form: no painted patches. Lite and low draw its averages; medium
// adds the clumps, the grain and the scatter; high adds the pinpoints.
fn rc_ice_p(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let cs = rc_range(b.na.z, 1.0, 50.0) * 5.0;
    let ng = normalize(i.g);
    let l = frame.light_dir.xyz;
    var n = i.n;
    var s = 0.5;
    var fine = 0.0;
    var shade = 0.0;
    var ga = vec4<f32>(0.0);
    var gb = vec4<f32>(0.0);
    if (q >= 2u) {
        let kb = vec4<f32>(rc_aa_world(f, 1.0 / cs), rc_aa_world(f, 0.47 / cs), rc_aa_world(f, 0.22 / cs), rc_aa_world(f, 0.103 / cs));
        let pb = rc_ice_pb(b, i.world, kb, 4u);
        s = clamp(0.5 + 0.62 * pb.o.x, 0.0, 1.0);
        fine = pb.o.x - pb.lo;
        let near = rc_near(f);
        // A clump's top is broad and round, a pocket narrow and steep: the
        // relief's slope, more of it the lower the point lies. Powder is
        // soft: the relief is slight (more of it reads as plaster).
        let amp = 0.27;
        let g = pb.o.yzw * (1.24 * (1.0 - s) * amp * near / cs);
        n = normalize(n - (g - ng * dot(g, ng)));
        // Its own shade: one look towards the light over the two largest
        // sizes; a clump standing over the line to the light shades the point.
        let lt = l - ng * dot(l, ng);
        let ll = max(length(lt), 1e-4);
        let look = rc_ice_pb(b, i.world + lt * (0.5 / (ll * cs)), kb, 2u);
        shade = near * smoothstep(0.0, 0.2, amp * (look.lo - pb.lo) - 0.5 * max(dot(l, ng), 0.0) / ll);
        // The crystals, of two sizes.
        ga = rc_ice_c(i.world * 1600.0, b.seed + 21u);
        gb = rc_ice_c(i.world.zxy * 3400.0 + vec3<f32>(5.3, 0.0, 0.0), b.seed + 22u);
        ga.w = ga.w * rc_aa_world(f, 0.000625) * near;
        gb.w = gb.w * rc_aa_world(f, 0.000294) * near;
        n = normalize(n + (ga.xyz * ga.w + gb.xyz * (0.7 * gb.w)) * 0.5);
    }
    // The powder's own white; cool in the pockets between clumps, in its own
    // shade and on the faces turned from the light (the light there has come
    // through the snow), a change of hue far more than of value: a hollow
    // is blue-white, never grey; a clump's top and a crystal a touch whiter.
    let pore = (1.0 - smoothstep(-0.13, -0.03, fine)) * (1.0 - 0.5 * smoothstep(0.4, 0.7, s));
    let away = 1.0 - smoothstep(-0.25, 0.45, dot(n, l));
    let cool = clamp(0.7 * pore + 0.5 * shade + 0.8 * away + 0.3 * (1.0 - smoothstep(0.2, 0.5, s)), 0.0, 1.0);
    var col = mix(b.col_a, b.col_b, 0.27 * cool);
    col = mix(col, b.col_c, 0.5 * smoothstep(0.5, 0.9, s));
    col = col * (1.0 - 0.03 * pore) * (1.0 - 0.06 * shade) * (0.985 + 0.03 * smoothstep(0.25, 0.75, s)) * (1.0 + 0.09 * (ga.w + 0.6 * gb.w));
    var o = rc_out(i, col, 0.9, 0.0);
    o.n = n;
    o.occ = i.occ * (1.0 - 0.12 * pore);
    if (q >= 2u && rc_extra(b, RC_X_ICE, 2u)) {
        // Scatter: light goes through loose snow, so it wraps round a form
        // (no hard line where the form turns from the light) and the shade
        // side glows a cool white of the light's own.
        o.n = normalize(o.n + l * (0.45 * b.nb.x));
        o.sheen = vec4<f32>(b.col_b * (0.45 * b.nb.x), 0.6);
    }
    if (q >= 3u && rc_extra(b, RC_X_ICE, 1u)) {
        // Pinpoints: a crystal shows where its mirror direction comes near
        // the view (the half vector of light and eye): many faintly, the few
        // that meet it brightly; fewer down in a pocket.
        let hv = normalize(l + f.v);
        let fa = pow(max(dot(normalize(i.n + ga.xyz * 1.6), hv), 0.0), 40.0) * ga.w;
        let fb = pow(max(dot(normalize(i.n + gb.xyz * 1.6), hv), 0.0), 40.0) * gb.w;
        o.glint = vec3<f32>((0.3 + b.na.w) * 0.62 * (fa + 0.6 * fb) * (0.35 + 0.65 * smoothstep(0.2, 0.6, s)));
    }
    // Powder never shines: its normal stays off the view's horizon.
    o.n = normalize(o.n + f.v * max(0.25 - dot(o.n, f.v), 0.0));
    return o;
}

fn rc_ice(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    if (rc_ice_pm(b)) {
        return rc_ice_p(b, f, i, q);
    }
    let clar = b.na.x;
    let cs = rc_range(b.na.z, 1.0, 50.0);
    let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
    let rim = pow(1.0 - ndv, 3.0);
    let ka = rc_aa_world(f, 1.0 / cs);
    let hi = q >= 3u;
    let p = rc_proj(i.world, i.n);
    // Frost: a cover of fbm (needle streaks for the frost variant).
    let fq = p * (cs * 0.7) * select(vec2<f32>(1.0), vec2<f32>(1.0, 0.3), b.variant == 2u);
    let fm = rc_fade(smoothstep(0.35, 0.7, rc_fbm2(fq, b.seed + 3u, 3u)) * b.nb.y, b.nb.y * 0.5, ka);
    var col = mix(b.col_a, b.col_b, smoothstep(0.1, 1.0, ndv) * (0.25 + 0.75 * clar));
    col = mix(col, b.col_c, rim * 0.6 + (1.0 - clar) * 0.5);
    col = mix(col, b.col_b, (1.0 - fm) * b.nb.y * 0.3);
    // Cracks, in the ice: a surface layer, and at high a deeper one.
    let amt = b.na.y * select(0.0, 1.0, rc_extra(b, RC_X_ICE, 0u)) * (1.0 - fm);
    if (amt > 0.0) {
        col = mix(col, b.col_c, rc_fade(rc_ice_k(b, f, i, hi, cs, 0.0), 0.1, ka) * amt * 0.8);
        if (hi) {
            col = mix(col, mix(b.col_a, b.col_c, 0.5), rc_ice_k(b, f, i, true, cs * 0.6, 0.08) * ka * amt * 0.5);
        }
    }
    col = mix(col, b.col_c, fm * 0.9);
    var rg = 0.0;
    if (rc_extra(b, RC_X_ICE, 3u)) {
        // Wind ridges: lighter and shinier on the crests.
        rg = rc_fade(rc_ice_w(b, p, cs), 0.5, ka);
        col = col * (0.88 + 0.24 * rg);
    }
    let sn = smoothstep(0.3, 0.7, b.nb.y);
    let gk = rc_aa_world(f, 0.2 / cs);
    if (q >= 2u && sn > 0.0) {
        // Snow close up: a fine grain and soft clumps, bluer in the hollows.
        let cl = rc_fbm2(p * (cs * 0.7), b.seed + 8u, 2u);
        col = mix(col, b.col_b, (1.0 - smoothstep(0.25, 0.6, cl)) * 0.2 * sn);
        col = col * (1.0 + sn * gk * (rc_vnoise2(p * (cs * 5.0), b.seed + 6u) - 0.5) * 0.5);
    }
    var o = rc_out(i, col, 0.08 + 0.55 * fm + 0.15 * (1.0 - clar) - 0.2 * rg, 0.0);
    if (q >= 2u) {
        if (rc_extra(b, RC_X_ICE, 2u)) {
            // Wrap scatter: light bleeds through thin, grazing edges.
            o.sheen = vec4<f32>(b.col_a * (1.4 * b.nb.x), 2.0);
        }
        let gs = max(max(max(amt, select(0.0, 0.6, b.variant == 3u)), 0.7 * b.nb.y), select(0.0, 0.7, rc_extra(b, RC_X_ICE, 3u)));
        let near = rc_near(f) * ka * gs;
        if (near > 0.0) {
            let e = 0.12 / cs;
            let t = rc_tw(i.n);
            let h0 = rc_ice_h(b, i.world, i.n, cs, gk);
            let g = vec2<f32>(rc_ice_h(b, i.world + t[0] * e, i.n, cs, gk), rc_ice_h(b, i.world + t[1] * e, i.n, cs, gk)) - vec2<f32>(h0);
            o.n = rc_bump(i.n, t, g * (0.6 * near));
        }
    }
    if (hi && rc_extra(b, RC_X_ICE, 1u)) {
        // Sparkle: specks that wink for a narrow band of view angles.
        let sc = rc_voronoi2(p * 60.0, b.seed + 4u, 1.0);
        let wink = 1.0 - smoothstep(0.0, 0.15, abs(ndv - fract(sc.w * 13.7)));
        o.glint = vec3<f32>(b.na.w * 4.0 * (1.0 - smoothstep(0.05, 0.2, sc.x)) * step(sc.w, 0.4) * wink * rc_aa_world(f, 0.016));
        if (sn > 0.0) {
            // Snow glints: a grain with a random tilt flashes where its mirror
            // direction meets the view (the half vector of light and eye).
            let wc = floor(i.world * 160.0);
            let h = rc_hash3(vec3<i32>(wc), b.seed + 9u);
            let r = vec3<f32>(rc_u01(h), rc_u01(rc_pcg(h)), rc_u01(rc_pcg(h + 1u))) - vec3<f32>(0.5);
            let hv = normalize(frame.light_dir.xyz + f.v);
            let flash = pow(max(dot(normalize(i.n + r * 1.6), hv), 0.0), 220.0);
            let pf = fract(i.world * 160.0) - vec3<f32>(0.5) - r * 0.5;
            o.glint = o.glint + vec3<f32>(sn * (0.4 + b.na.w) * 3.0 * flash * (1.0 - smoothstep(0.1, 0.5, length(pf))) * rc_aa_world(f, 0.006));
        }
    }
    return o;
}
