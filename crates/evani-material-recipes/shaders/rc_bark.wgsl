// evani.shader.rc.bark
// Bark (recipe 5, batch 1 build plan section 3, package P5). Written fresh
// for Evani.Dev: furrows are the F2-F1 ridges of a cellular noise stretched
// along u, plates the same noise at its own size with narrow fissures,
// peeling strips the contour of a slow noise. Uv space, so it follows a
// trunk: u runs around it (one turn is one uv unit), v along it, and the
// furrows run along v. Every pattern is periodic in u with period 1 (P6): the
// noises are sampled on a circle of circumference u * scale (3D value noise),
// the cellular cells wrap at a whole number of cells around, so a trunk's uv
// seam (u = 0 and u = 1 on the same line) shows no cut and no mirror line.
//   numbers: na = rough, furrow depth, furrow scale (1-50 per uv), plate
//            size (0.02-1 uv); nb = stretch (0.5-4, along v), peel, cord twist,
//            far detail (0: furrows under about 15 px fade to their average;
//            1: narrow jagged fissures between tall plates, some fused
//            shut, rough faces at three scales, drawn down to about 4 px, with a fine grain over the
//            plates: the procedural photoreal oak's)
//   colours: A bark, B furrow, C patch (lichen, inner bark, paper; the eye
//            marks of aspen; the glowing seams of burnt bark)
//   variants: 0 furrowed, 1 plated, 2 smooth, 3 peeling; batch 2: 4 fibrous
//             (strands around the trunk, helical with the cord twist),
//             5 ringed (palm), 6 banded (cherry: glossy, pale bands, dashes)
//   extras: parallax (bit 0), marks (bit 1: sparse eye marks), embers (bit 2:
//           the furrows glow in colour C, no clock, patches off)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was flat furrow stripes); low draws the
// ridges, fissures, patches, peel and birch marks, anti-aliased (furrows
// under 1.5 px fade to their average); medium adds the relief as normals
// within 25 m; high adds 2-step parallax within 8 m; ultra parallaxes
// everywhere.

// Value noise at u (around the trunk, `fu` features per turn) and y (along it,
// in noise units): periodic in u with period 1, stretched a little in contrast
// (3D value noise sits nearer the middle than the 2D noise it replaces).
fn rc_bark_n(u: f32, fu: f32, y: f32, seed: u32) -> f32 {
    let a = u * 6.2831853;
    return clamp(0.5 + (rc_vnoise3(vec3<f32>(vec2<f32>(cos(a), sin(a)) * (fu * 0.15915494), y), seed) - 0.5) * 1.3, 0.0, 1.0);
}

// Three octaves of it.
fn rc_bark_fbm(u: f32, fu: f32, y: f32, seed: u32) -> f32 {
    return rc_bark_n(u, fu, y, seed) * 0.57 + rc_bark_n(u, fu * 2.0, y * 2.0, seed + 101u) * 0.29 + rc_bark_n(u, fu * 4.0, y * 4.0, seed + 202u) * 0.14;
}

// Cellular distances (F1, F2, F2-F1, cell id) whose cells wrap after `per`
// cells in x.
fn rc_bark_vor(p: vec2<f32>, per: i32, seed: u32) -> vec4<f32> {
    let fl = floor(p);
    let fr = p - fl;
    let ic = vec2<i32>(fl);
    var f1 = 8.0;
    var f2 = 8.0;
    var id = 0u;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let h = rc_hash2(vec2<i32>(((ic.x + x) % per + per) % per, ic.y + y), seed);
            let d = length(vec2<f32>(f32(x), f32(y)) + vec2<f32>(rc_u01(h), rc_u01(rc_pcg(h))) - fr);
            if (d < f1) {
                f2 = f1;
                f1 = d;
                id = h;
            } else if (d < f2) {
                f2 = d;
            }
        }
    }
    return vec4<f32>(f1, f2, f2 - f1, rc_u01(id));
}

// The batch 2 variants at uv: x height, y furrow mask, z band mask (colour C),
// all periodic in u (strands count a whole number around the trunk).
fn rc_bark_x(b: RecBlock, uv: vec2<f32>) -> vec4<f32> {
    let fs = rc_range(b.na.z, 1.0, 50.0);
    let nf = max(round(fs), 2.0);
    let y = uv.y / rc_range(b.nb.x, 0.5, 4.0) * fs;
    let n = rc_bark_n(uv.x, nf * 0.5, y * 0.3, b.seed);
    if (b.variant == 4u) {
        let h = (0.5 + 0.5 * sin(6.2832 * (uv.x * nf + y * b.nb.z * 0.5 + n * 1.5))) * (0.5 + 0.5 * rc_bark_n(uv.x, nf, y * 0.15, b.seed + 5u));
        return vec4<f32>(h, 1.0 - smoothstep(0.1, 0.45, h), 0.0, 0.0);
    }
    if (b.variant == 5u) {
        let r = 0.5 + 0.5 * sin(6.2832 * (y * 0.5 + n * 0.3));
        return vec4<f32>(r * (0.6 + 0.4 * rc_bark_n(uv.x, nf * 3.0, y * 0.2, b.seed + 5u)), 1.0 - smoothstep(0.0, 0.5, r), 0.0, 0.0);
    }
    return vec4<f32>(0.7 + 0.3 * n, smoothstep(0.78, 0.82, rc_bark_n(uv.x, nf * 0.35, y * 2.0, b.seed + 6u)), smoothstep(0.6, 0.65, rc_bark_n(uv.x, nf * 0.15, y * 0.5, b.seed + 7u)), 0.0);
}

// The bark at uv: x ridge height (0 in a furrow, 1 on a ridge), y furrow
// mask, z peel mask, w the plate's id.
fn rc_bark_s(b: RecBlock, uv: vec2<f32>) -> vec4<f32> {
    if (b.variant >= 4u) {
        return rc_bark_x(b, uv);
    }
    let v = b.variant;
    let fs = rc_range(b.na.z, 1.0, 50.0);
    let st = rc_range(b.nb.x, 0.5, 4.0);
    let ps = rc_range(b.na.w, 0.02, 1.0);
    let nf = max(round(fs), 2.0);
    let np = max(round(1.0 / ps), 1.0);
    let y = uv.y / st * fs;
    var p = vec2<f32>(uv.x * nf, y);
    p = p + (vec2<f32>(rc_bark_n(uv.x, nf * 0.7, y * 0.7, b.seed), rc_bark_n(uv.x, nf * 0.7, y * 0.7 + 8.1, b.seed + 17u)) - vec2<f32>(0.5)) * 1.2;
    if (b.nb.w > 0.5) {
        // Far detail (the photoreal oak): narrow fissures between tall plates
        // (cells about four times as tall as wide, their edges jagged by a
        // fine warp), some fused shut so the plates join and break unevenly,
        // and rough faces at three scales.
        let pw = vec2<f32>(uv.x * nf, y * 0.9) + (vec2<f32>(rc_bark_n(uv.x, nf * 4.0, y * 1.2, b.seed + 40u), rc_bark_n(uv.x, nf * 4.0, y * 1.2 + 5.3, b.seed + 41u)) - vec2<f32>(0.5)) * 0.5;
        let cv = rc_bark_vor(pw, i32(nf), b.seed + 1u);
        let fu = (1.0 - smoothstep(0.0, 0.08 + 0.06 * b.na.y, cv.z)) * mix(0.3, 0.8, smoothstep(0.25, 0.55, rc_bark_n(uv.x, nf * 0.6, y * 0.15, b.seed + 42u)));
        let ro = 0.35 + 0.4 * rc_bark_n(uv.x, nf * 5.0, y * 2.0, b.seed + 43u) + 0.25 * rc_bark_n(uv.x, nf * 11.0, y * 4.5, b.seed + 44u);
        return vec4<f32>((1.0 - fu) * ro * (0.8 + 0.2 * cv.w), fu * 0.65, 0.0, cv.w);
    }
    var h = 0.6 + 0.4 * rc_bark_n(uv.x, nf * 0.5, y * 0.5, b.seed + 2u);
    var fur = 0.0;
    var id = 0.0;
    if (v != 2u) {
        // Plates have their own size and narrow fissures; furrows follow the
        // furrow scale and meander.
        let c = rc_bark_vor(select(p, vec2<f32>(uv.x * np, uv.y / st / ps), v == 1u), i32(select(nf, np, v == 1u)), b.seed + 1u);
        let wf = select(0.1 + 0.3 * b.na.y, 0.07 + 0.08 * b.na.y, v == 1u);
        fur = 1.0 - smoothstep(0.0, wf, c.z);
        h = smoothstep(0.0, wf * 2.2, c.z) * (0.75 + 0.25 * c.w);
        id = c.w;
    }
    // Peeling: strips across the trunk lift at the edge of a slow noise.
    let pn = rc_bark_fbm(uv.x, nf * 1.2, y * 0.3, b.seed + 4u);
    let th = 0.78 - 0.4 * b.nb.y;
    let on = step(0.001, b.nb.y);
    let pm = smoothstep(th, th + 0.02, pn) * on;
    let rim = (1.0 - smoothstep(0.0, 0.025, abs(pn - th))) * on;
    h = mix(h, 0.25 + 0.6 * rim, max(pm, rim));
    return vec4<f32>(h, max(fur * (1.0 - pm), rim * 0.5), pm, id);
}

// The uv shift of a point d metres below the surface, seen along the view:
// the view's tangent part in the screen basis, mapped through the uv
// derivatives.
fn rc_bark_off(f: MatFrame, ng: vec3<f32>, d: f32) -> vec2<f32> {
    let dn = dot(f.v, ng);
    let vt = (f.v - ng * dn) / max(dn, 0.3) * d;
    let a = dot(f.dpx, f.dpx);
    let c = dot(f.dpx, f.dpy);
    let e = dot(f.dpy, f.dpy);
    let r = dot(vt, f.dpx);
    let s = dot(vt, f.dpy);
    let det = max(a * e - c * c, 1e-20);
    return f.duvx * ((r * e - s * c) / det) + f.duvy * ((s * a - r * c) / det);
}

fn rc_bark(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var uv = i.uv;
    let fs = rc_range(b.na.z, 1.0, 50.0);
    let st = rc_range(b.nb.x, 0.5, 4.0);
    let nf = max(round(fs), 2.0);
    let kf = rc_aa(f, mix(0.1, 0.35, b.nb.w) / fs);
    if (q >= 3u && rc_extra(b, RC_X_BARK, 0u) && (q == 4u || f.dist < 8.0)) {
        // Parallax, two steps: the view slides into the furrows.
        let d = 0.015 * b.na.y * kf;
        let h1 = rc_bark_s(b, uv).x;
        let h2 = rc_bark_s(b, uv - rc_bark_off(f, normalize(i.g), d * (1.0 - h1))).x;
        uv = uv - rc_bark_off(f, normalize(i.g), d * (1.0 - h2));
    }
    let s = rc_bark_s(b, uv);
    let fm = rc_fade(s.y, 0.25, kf);
    let v = b.variant;
    // Patches (lichen, grey birch), plate tints.
    let pt = smoothstep(0.7, 0.8, rc_bark_fbm(uv.x, nf * 0.4, uv.y / st * fs * 0.4, b.seed + 3u));
    let em = rc_extra(b, RC_X_BARK, 2u);
    var c = mix(b.col_a * (0.8 + 0.35 * s.x), b.col_b, fm);
    c = mix(c, b.col_c, max(pt * 0.5, select(0.0, s.w * 0.35, v == 1u)) * (1.0 - fm) * select(1.0, 0.0, em));
    c = mix(c, b.col_c * 1.1, s.z * (1.0 - s.y));
    if (v == 2u) {
        // Birch: dark lenticel dashes across the trunk.
        c = mix(c, b.col_b, smoothstep(0.8, 0.84, rc_bark_n(uv.x, nf * 0.35, uv.y / st * fs * 2.0, b.seed + 6u)) * kf);
    }
    if (rc_extra(b, RC_X_BARK, 1u)) {
        // Eye marks (aspen): sparse dark cells, wider than tall.
        let n1 = max(round(fs * 0.4), 2.0);
        let m = rc_bark_vor(vec2<f32>(uv.x * n1, uv.y / st * fs * 1.5), i32(n1), b.seed + 9u);
        c = mix(c, b.col_c, (1.0 - smoothstep(0.1, 0.18, m.x)) * step(m.w, 0.3) * kf);
    }
    // Far detail: a fine grain over the plates' faces too.
    c = c * (1.0 + b.nb.w * 0.3 * (rc_bark_n(uv.x, nf * 5.0, uv.y / st * fs * 5.0, b.seed + 8u) - 0.5));
    var o = rc_out(i, c, select(b.na.x, b.na.x * 0.5, v == 6u) + 0.1 * fm, 0.0);
    if (em) {
        // Embers: the furrows glow in colour C, steady (no clock).
        o.emit = b.col_c * (1.5 * fm);
    }
    if (q >= 2u) {
        // Far detail keeps its relief out to 45-80 m, over a wider step (a
        // third of a plate) so it doesn't sparkle.
        let fd = b.nb.w > 0.5;
        let near = select(rc_near(f), 1.0 - smoothstep(45.0, 80.0, f.dist), fd) * kf;
        if (near > 0.0) {
            // Furrow and plate relief; the step is a tenth of a furrow.
            let e = select(0.1, 0.3, fd) / fs;
            let d = vec2<f32>(rc_bark_s(b, uv + vec2<f32>(e, 0.0)).x, rc_bark_s(b, uv + vec2<f32>(0.0, e)).x) - vec2<f32>(s.x);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), d * (8.0 * b.na.y * near));
        }
    }
    return o;
}
