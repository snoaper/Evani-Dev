// ---- sky cumulus (feature `sky-horizon`, cloud volume on) ---------------------
//
// Fair-weather cumulus as a real volume: a layer of air between a flat base
// and a top, marched along each view ray from the eye. Each cloud is a heap
// whose footprint comes from a seeded 2D field over the layer (separate
// heaps of mixed sizes, clusters and clear stretches) and whose height rises
// from a thin rim to a rounded crown; billows (3D noise at three scales,
// stronger up top) heap the crown into cauliflower lobes, fray the rims into
// wisps and leave the base flat. Lit by the sun through the cloud (three
// probes towards it, multiple scattering in three orders, the outer skin's
// powder), by the sky from above (dimmer towards the base) and the ground
// from below; the air between hides far clouds into the horizon haze. A
// faint high cirrus layer of wind-stretched streaks lies behind them.
//
// Records: 11.z cloud volume (0 off: the flat clouds; 1 on), 11.w the
// field's seed; 12 = [base (km), depth (km), heap size (km), cirrus];
// 13 = [billows, surface softness, base shade, powder]. The clear sky's
// cloud scale is unused here; coverage, density, speed and brightness
// (records 5, 9.w) are.

struct MpCu {
    base: f32,
    depth: f32,
    size: f32,
    thr: f32,
    billow: f32,
    soft: f32,
    off: vec2<f32>,
    evolve: f32,
}

fn mp_cu_on() -> bool {
    return arrayLength(&kit_data) > 13u && kit_data[11].z > 0.5;
}

fn mp_cu_hash(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

// 3D value noise, 0..1.
fn mp_cu_noise(x: vec3<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mp_cu_hash(i), mp_cu_hash(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(mp_cu_hash(i + vec3<f32>(0.0, 1.0, 0.0)), mp_cu_hash(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(mp_cu_hash(i + vec3<f32>(0.0, 0.0, 1.0)), mp_cu_hash(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(mp_cu_hash(i + vec3<f32>(0.0, 1.0, 1.0)), mp_cu_hash(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

// The heaps' footprint at `xz` (km): above 0 inside one, higher towards its
// middle (three octaves turned against each other, plus a broad field of
// clusters and clear stretches).
fn mp_cu_cover(xz: vec2<f32>, c: MpCu) -> f32 {
    let q = (xz + c.off) / c.size;
    let r1 = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    let r2 = mat2x2<f32>(-0.6, 0.8, -0.8, -0.6);
    let v = mp_sky_noise(q) * 0.6 + mp_sky_noise(r1 * q * 2.13 + 5.2) * 0.27 + mp_sky_noise(r2 * q * 4.37 + 1.7) * 0.13;
    let region = mp_sky_noise(q * 0.21 + 11.3) * 0.3;
    return v + region - c.thr;
}

// Density (0..1) at `p` (km, the eye at the origin). `detail` 0 is the sun
// probes' view: only the big billows.
fn mp_cu_density(p: vec3<f32>, c: MpCu, detail: f32) -> f32 {
    return mp_cu_density2(p, c, detail, 0.0).x;
}

// The density (x) and, in y, the density of the cloud grown by `pad` (in
// layer depths): above 0 near a surface, where the march must take fine
// steps (a thin crest between two long steps is otherwise caught by some
// pixels and missed by their neighbours: grain along the outlines).
fn mp_cu_density2(p: vec3<f32>, c: MpCu, detail: f32, pad: f32) -> vec2<f32> {
    let hy = (p.y - c.base) / c.depth;
    if (hy < -0.15 - pad || hy > 1.4) {
        return vec2<f32>(0.0);
    }
    let cov = mp_cu_cover(p.xz, c);
    // The heap's height: a thin rim rising to a rounded crown.
    let top = 0.85 * pow(clamp(cov / 0.3, 0.0, 1.0), 0.55);
    if (cov < -0.08 - pad * 0.3 || top - hy < -0.45 * c.billow - 0.05 - pad) {
        return vec2<f32>(0.0);
    }
    let pq = p / (c.depth * 0.3) + vec3<f32>(c.evolve, 0.0, 0.0);
    let n1 = mp_cu_noise(pq);
    var disp = (n1 - 0.5) * 0.55;
    var rag = (n1 - 0.5) * 0.12;
    if (detail > 0.0) {
        // Billows: round bumps with creases between (folded noise), and a
        // finer fray at the rims and the base.
        let n2 = mp_cu_noise(pq * 2.63 + vec3<f32>(3.1, 1.7, 5.3));
        let n3 = mp_cu_noise(pq * 5.37 + vec3<f32>(7.7, 2.9, 0.6));
        disp = disp + (0.5 - abs(n2 * 2.0 - 1.0)) * 0.32 + (n3 - 0.5) * 0.08 * detail;
        rag = rag + (n2 - 0.5) * 0.08 + (n3 - 0.5) * 0.05;
    }
    let up = mix(0.4, 1.0, clamp(hy * 1.6, 0.0, 1.0));
    let s = top - hy + disp * c.billow * up;
    // A flat base, a little ragged and lumpy seen from below.
    let base = smoothstep(0.0, 0.05, hy + rag);
    let d = clamp(s / c.soft, 0.0, 1.0) * base * smoothstep(-0.08, 0.04, cov);
    if (pad <= 0.0) {
        return vec2<f32>(d, d);
    }
    let near = clamp((s + pad) / c.soft, 0.0, 1.0) * smoothstep(0.0, 0.05, hy + rag + pad) * smoothstep(-0.08 - pad * 0.3, 0.04, cov);
    return vec2<f32>(d, near);
}

fn mp_cu_hg(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / pow(max(1.0 + g2 - 2.0 * g * mu, 1e-4), 1.5);
}

// The sky with the cumulus over it, looking along `dir` (the clear sky's
// colour `col`, its scattered light `lin`, the view's extinction `fex`, the
// sun's intensity `sun_e`).
fn mp_cumulus(dir: vec3<f32>, sun: vec3<f32>, col_in: vec3<f32>, lin: vec3<f32>, fex: vec3<f32>, sun_e: f32, time: f32, r5: vec4<f32>) -> vec3<f32> {
    var col = col_in;
    if (dir.y < 0.004) {
        return col;
    }
    let rc = kit_data[12];
    let lk = kit_data[13];
    let seed = kit_data[11].w;
    var c: MpCu;
    c.base = max(rc.x, 0.1);
    c.depth = max(rc.y, 0.05);
    c.size = max(rc.z, 0.1);
    c.thr = mix(0.55, -0.3, clamp(r5.z, 0.0, 1.0));
    c.billow = lk.x;
    c.soft = max(lk.y, 0.01);
    // The wind carries the field along +x (cloud speed 2e-5: 10 m/s); the
    // billows turn over more slowly.
    c.off = vec2<f32>(seed * 7.31, seed * -3.17) + vec2<f32>(time * r5.y * 500.0, 0.0);
    c.evolve = time * r5.y * 60.0;

    let day = smoothstep(-0.08, 0.3, sun.y);
    // Sunlight as it reaches the clouds: dimmed along the sun's own path
    // through the air (not the view's), kept nearly neutral.
    let r4 = kit_data[4];
    let beta_r = MP_TOTAL_RAYLEIGH * r4.y;
    let beta_m = 0.434 * (0.2 * r4.x * 10e-18) * MP_MIE_CONST * r4.z;
    let zs = acos(clamp(sun.y, 0.0, 1.0));
    let inv_s = 1.0 / (cos(zs) + 0.15 * pow(93.885 - zs * 180.0 / MP_PI, -1.253));
    let fex_s = exp(-(beta_r * 8.4e3 + beta_m * 1.25e3) * inv_s);
    let fs_l = dot(fex_s, vec3<f32>(0.2126, 0.7152, 0.0722));
    let sun_col = sun_e * mix(vec3<f32>(fs_l), fex_s, 0.5) * 0.22 * 0.04;
    let sky_amb = lin * 0.04;
    let amb_grey = vec3<f32>(dot(sky_amb, vec3<f32>(0.2126, 0.7152, 0.0722)));
    let amb = (mix(amb_grey, sky_amb, 0.45) * 1.5 + vec3<f32>(0.05, 0.055, 0.065)) * day;
    let gain = select(1.0, kit_data[9].w, arrayLength(&kit_data) > 11u);

    // Cirrus: faint streaks far up, stretched along the wind.
    if (rc.w > 0.0) {
        let tc = 8.0 / dir.y;
        let pc = dir.xz * tc + c.off * 1.6;
        let rw = mat2x2<f32>(0.94, 0.34, -0.34, 0.94);
        let qc = (rw * pc) * vec2<f32>(0.09, 0.42);
        let w = mp_sky_noise(qc) * 0.55 + mp_sky_noise(qc * 2.3 + vec2<f32>(4.1, 1.3)) * 0.3 + mp_sky_noise(qc * vec2<f32>(5.1, 9.7) + 2.2) * 0.15;
        let cpatch = smoothstep(0.1, 0.5, mp_sky_noise(pc * 0.035 + vec2<f32>(9.0, 3.0)) + rc.w * 0.6 - 0.3);
        let a = smoothstep(0.05, 0.55, w) * cpatch * rc.w * 0.45 * exp(-tc / 70.0) * smoothstep(0.02, 0.12, dir.y);
        let cc = (amb * 1.2 + sun_col * 0.9) * gain;
        col = mix(col, cc, a);
    }

    // The march through the layer.
    let t0 = c.base / dir.y;
    if (t0 > 80.0) {
        return col;
    }
    let t1 = min((c.base + c.depth * 1.4) / dir.y, t0 + 14.0);
    let n = 64;
    let dt = (t1 - t0) / f32(n);
    let j = mp_cu_hash(vec3<f32>(dir.x * 4096.0, dir.y * 4096.0, dir.z * 4096.0));
    var t = t0 + dt * j;
    let l = normalize(vec3<f32>(sun.x, max(sun.y, 0.08), sun.z));
    let mu = dot(dir, l);
    let sigma = max(r5.w, 0.01) * 60.0;
    let g = 0.6;
    var tr = 1.0;
    var acc = vec3<f32>(0.0);
    var th = 0.0;
    // Long steps through clear air; near a cloud (inside it grown by a
    // tenth of the layer) the ray backs up and takes eighth steps until it
    // has been clear of it for a while, so crests and outlines are sampled
    // finely: a cloud's skin is so dense that one sample in a thin edge
    // turns a pixel nearly solid while its neighbour misses it (grain).
    var fine = 0;
    for (var i = 0; i < n * 3; i = i + 1) {
        if (t > t1) {
            break;
        }
        let p = dir * t;
        let dd = mp_cu_density2(p, c, 1.0, 0.1);
        let d = dd.x;
        if (dd.y > 0.002 && fine == 0) {
            fine = 8;
            t = t - dt * 0.875;
            continue;
        }
        let h = select(dt, dt * 0.125, fine > 0);
        if (dd.y <= 0.002 && fine > 0) {
            fine = fine - 1;
        }
        if (dd.y > 0.002) {
            fine = 8;
        }
        if (d > 0.002) {
            let hy = clamp((p.y - c.base) / c.depth, 0.0, 1.0);
            // Probes towards the sun at distances varied per pixel (fixed
            // ones draw rings where the cloud's thickness crosses them).
            let pj = 0.7 + 0.6 * fract(j * 7.13 + f32(i) * 0.618);
            let d1 = mp_cu_density(p + l * (0.035 * pj), c, 0.0);
            let d2 = mp_cu_density(p + l * (0.11 * pj), c, 0.0);
            let d3 = mp_cu_density(p + l * (0.3 * pj), c, 0.0);
            let od = (d1 * 0.035 + d2 * 0.075 + d3 * 0.19) * pj * sigma;
            let pw = 1.0 - lk.w * exp(-od * 1.5);
            // Three orders of scattering: each less dimmed, a little weaker
            // and less forward (shadowed sides stay grey-white, not dark).
            var sun_l = 0.0;
            var a = 1.0;
            var b = 1.0;
            var cc = 1.0;
            for (var k = 0; k < 3; k = k + 1) {
                let ph = mix(mp_cu_hg(mu, g * cc), mp_cu_hg(mu, -0.2 * cc), 0.3);
                sun_l = sun_l + b * ph * exp(-a * od);
                a = a * 0.18;
                b = b * 0.6;
                cc = cc * 0.5;
            }
            // Sky light from above, dimmer towards the base; the sunlit
            // ground's bounce from below.
            let skyl = amb * mix(lk.z, 1.0, hy) + sun_col * 0.08 * (1.0 - hy);
            let lc = (sun_col * sun_l * pw * 0.55 + skyl) * gain;
            let ext = exp(-d * sigma * h);
            let w = tr * (1.0 - ext);
            acc = acc + w * lc;
            th = th + w * t;
            tr = tr * ext;
            if (tr < 0.01) {
                break;
            }
        }
        t = t + h;
    }
    let alpha = 1.0 - tr;
    if (alpha < 1e-4) {
        return col;
    }
    // The air between: far clouds melt into the horizon's haze.
    // (The clouds' haze record, 8.w: 0.3 hides half a cloud 14 km away;
    // the fade band, 8.xy, thins them out over the last degrees.)
    let hzr = kit_data[8];
    let dist = th / alpha;
    let haze = exp(-dist * max(hzr.w, 0.0) * 0.165) * smoothstep(hzr.x, hzr.y, dir.y);
    return mix(col, acc / alpha, alpha * haze);
}
