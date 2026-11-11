// evani-grass compute: the trample window, blade generation and culling
// into three LOD buckets, then the indirect counts clamped to the buckets.

struct Blade {
    // root (world) and width (m)
    a: vec4<f32>,
    // control point 1 from the root, blade length (m)
    b: vec4<f32>,
    // control point 2 from the root, facing yaw (rad)
    c: vec4<f32>,
    // tip from the root, occlusion (shorter than the sward: darker)
    d: vec4<f32>,
    // root colour, tip colour (sqrt-encoded rgb8), flags, seed
    e: vec4<u32>,
};

@group(0) @binding(0) var<uniform> sim: Sim;
// Per job: tile x, tile z (i32 bits), patch | first blade << 8,
// full-density count | blades in this job << 16.
@group(0) @binding(1) var<storage, read> jobs: array<vec4<u32>>;
@group(0) @binding(2) var<storage, read_write> blades: array<Blade>;
// Three draw-indirect records: vertex count, instance count, 0, 0.
@group(0) @binding(3) var<storage, read_write> args: array<atomic<u32>, 12>;
// The trample window: per cell the flattening (direction times amount).
@group(0) @binding(4) var<storage, read_write> trample: array<vec2<f32>>;
// Every patch's height grid (CORE_REQUESTS grass-heightfield).
@group(0) @binding(5) var<storage, read> ground: array<f32>;

// The ground under xz: a patch with a height grid puts its blades on it
// (bilinear between samples), a flat one on its ground height.
fn ground_at(pa: Patch, xz: vec2<f32>) -> f32 {
    let n = u32(pa.hf.y);
    if (n < 2u) {
        return pa.info.x;
    }
    let uv = clamp((xz - pa.rect.xy) / max(pa.rect.zw - pa.rect.xy, vec2<f32>(1e-3)), vec2<f32>(0.0), vec2<f32>(1.0)) * f32(n - 1u);
    let c = min(vec2<u32>(uv), vec2<u32>(n - 2u));
    let f = uv - vec2<f32>(c);
    let o = u32(pa.hf.x) + c.y * n + c.x;
    return pa.info.x + mix(mix(ground[o], ground[o + 1u], f.x), mix(ground[o + n], ground[o + n + 1u], f.x), f.y);
}

fn wrap(v: vec2<i32>, n: i32) -> vec2<i32> {
    return ((v % n) + n) % n;
}

// ---- trample window -------------------------------------------------------
// The window is toroidal: a world cell always lives at (cell mod n), so when
// the window scrolls only the cells that come in are cleared, and nothing
// pops. Each frame every cell relaxes (half-life), then the characters'
// capsules (swept from last frame) press it flat, away from their path and
// along their motion.
@compute @workgroup_size(8, 8)
fn cs_trample(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(sim.trample.w);
    if (id.x >= n || id.y >= n) {
        return;
    }
    let ni = i32(n);
    let w = vec2<i32>(id.xy);
    let o = vec2<i32>(sim.trample.xy);
    let po = vec2<i32>(sim.trample2.xy);
    let cell = o + wrap(w - o, ni);
    let was = po + wrap(w - po, ni);
    let at = id.y * n + id.x;
    var v = trample[at];
    if (any(cell != was)) {
        v = vec2<f32>(0.0);
    }
    v *= sim.trample2.z;
    let p = (vec2<f32>(cell) + 0.5) * sim.trample.z;
    let count = min(u32(sim.stamps_n.x), 16u);
    for (var s = 0u; s < count; s += 1u) {
        let s0 = sim.stamps[s * 2u];
        let s1 = sim.stamps[s * 2u + 1u];
        let ab = s0.zw - s0.xy;
        let t = clamp(dot(p - s0.xy, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
        let off = p - (s0.xy + ab * t);
        let d = length(off);
        let r = s1.x;
        if (d >= r * 1.2) {
            continue;
        }
        // A full press across most of the footprint, fading at its rim.
        let amt = (1.0 - smoothstep(r * 0.75, r * 1.2, d)) * s1.y;
        let away = select(vec2<f32>(0.0), off / max(d, 1e-4), d > 1e-3);
        var dir = away * 0.7 + s1.zw * 0.8;
        if (dot(dir, dir) < 1e-6) {
            dir = vec2<f32>(1.0, 0.0);
        }
        dir = normalize(dir);
        let cur = length(v);
        let m = min(max(cur, amt), 1.0);
        let nd = v + dir * amt * 2.0;
        v = select(dir, normalize(nd), dot(nd, nd) > 1e-8) * m;
    }
    trample[at] = v;
}

fn trample_cell(c: vec2<i32>) -> vec2<f32> {
    let n = i32(sim.trample.w);
    let rel = c - vec2<i32>(sim.trample.xy);
    if (n <= 0 || rel.x < 0 || rel.y < 0 || rel.x >= n || rel.y >= n) {
        return vec2<f32>(0.0);
    }
    let w = wrap(c, n);
    return trample[u32(w.y * n + w.x)];
}

fn trample_at(xz: vec2<f32>) -> vec2<f32> {
    if (sim.trample.w < 0.5) {
        return vec2<f32>(0.0);
    }
    let g = xz / sim.trample.z - 0.5;
    let i = vec2<i32>(floor(g));
    let f = g - floor(g);
    let a = trample_cell(i);
    let b = trample_cell(i + vec2<i32>(1, 0));
    let c = trample_cell(i + vec2<i32>(0, 1));
    let d = trample_cell(i + vec2<i32>(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn pack_rgb(c: vec3<f32>) -> u32 {
    let v = vec3<u32>(clamp(sqrt(max(c, vec3<f32>(0.0))), vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + 0.5);
    return v.x | (v.y << 8u) | (v.z << 16u);
}

// ---- blades ---------------------------------------------------------------
// One thread per candidate blade of a tile (64 per job). Everything about a
// blade comes from a hash of its tile, patch and index, so a blade is the
// same every frame and on every device; the blades of a tile are a random
// order, so any prefix of them is an even thinning (LOD keeps a prefix).
@compute @workgroup_size(64)
fn cs_blades(@builtin(workgroup_id) wid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let j = wid.y * nwg.x + wid.x;
    if (j >= sim.caps.w) {
        return;
    }
    let job = jobs[j];
    if (lid >= (job.w >> 16u)) {
        return;
    }
    let pi = job.z & 255u;
    let i = (job.z >> 8u) + lid;
    let full = max(job.w & 65535u, 1u);
    let pa = sim.patches[pi];
    let kd = sim.kinds[min(u32(pa.info.z), 3u)];
    let tile = sim.lod.w;
    var s = hash_u((job.x * 2654435769u) ^ hash_u(job.y ^ hash_u(i * 31u + pi * 2246822519u)));
    let xz = (vec2<f32>(f32(bitcast<i32>(job.x)), f32(bitcast<i32>(job.y))) + vec2<f32>(rnd(&s), rnd(&s))) * tile;
    let rc = pa.rect;
    let edge = min(min(xz.x - rc.x, rc.z - xz.x), min(xz.y - rc.y, rc.w - xz.y));
    if (edge < 0.0) {
        return;
    }
    let e = smoothstep(0.0, max(pa.info.y, 1e-3), edge);
    var root = vec3<f32>(xz.x, ground_at(pa, xz), xz.y);
    let d = distance(root, sim.eye.xyz);
    // Short kinds are drawn less far (`reach`); LOD by the scaled distance.
    let dr = d / max(kd.ear.w, 0.05);
    let far_d = sim.lod.z;
    if (dr >= far_d) {
        return;
    }
    var bk = 2u;
    if (d < sim.lod2.x) {
        bk = 0u;
    } else if (d < sim.lod.y) {
        bk = 1u;
    }
    let keep = keep_at(dr, sim.lod.x, far_d) * sim.scales[bk];
    // A fine sward (the kind's `fine`; its own random numbers too, so every
    // other kind's blades are as they were): its soil lies open in
    // irregular openings, the sward thinning towards their rims, and it
    // stands thick in its tufts and thin between them.
    let fine = kd.look.x;
    var cover = cover_at(xz, kd.ear.z);
    var rim = 0.0;
    var ftu = vec4<f32>(0.0);
    var crowd = 1.0;
    if (fine > 0.001) {
        let op = open_rim(xz, kd.ear.z);
        cover = 1.0 - op.x;
        rim = op.y;
        if (kd.more.z > 0.001) {
            ftu = ftuft_at(xz);
            crowd = mix(1.0, 0.6 + 1.0 * min(ftu.w * 1.5, 1.0), kd.more.z);
        }
        crowd *= 1.0 - 0.4 * rim;
    }
    let thresh = keep * mix(0.15, 1.0, e) * cover * crowd * step(0.02, e);
    let rank = (f32(i) + 0.5) / f32(full);
    if (rank >= thresh) {
        return;
    }
    // The last quarter of the kept share grows in as the camera nears, and
    // the last fifth of the distance shrinks away: no blade pops.
    let thin = clamp((thresh - rank) / max(thresh * 0.25, 1e-5), 0.0, 1.0);
    let far = 1.0 - smoothstep(far_d * 0.78, far_d, dr);
    let cl = kd.dry.w;
    let cn = vnoise(xz * 0.9 + vec2<f32>(3.3, 9.1));
    let hn = vnoise(xz * 0.22 + vec2<f32>(31.0, 17.0));
    // A few stalks stand above the leaves carrying a small seed head
    // (meadow); in wheat every stalk carries its ear.
    // Broadleaf weeds (the kind's `weeds`): inside a rosette a few of the
    // candidates are its leaves (their own random numbers, so the grass
    // round them is the same with and without weeds); the grass under the
    // leaves is shaded out and stays short.
    var leaf = false;
    var weed = vec4<f32>(0.0);
    var ws = hash_u(s ^ 2654435769u);
    var fr = hash_u(s ^ 1597334677u);
    if (kd.more.w > 0.001) {
        weed = weed_at(xz, kd.more.w);
        if (weed.z > 0.0) {
            let inside = max(kd.shape.w * max(pa.info.w, 0.05) * 3.1416 * weed.z * weed.z, 1.0);
            // The first candidates in the tile's order, so the leaves are
            // the last of the rosette to thin out with distance; each
            // rosette has its own number of leaves, half a dozen to a score.
            leaf = rank < (9.0 + 12.0 * weed.w) / inside;
        }
    }
    let head_r = rnd(&s);
    let head = !leaf && head_r < kd.more.x;
    var len = kd.shape.x * (1.0 + kd.shape.y * (rnd(&s) * 2.0 - 1.0)) * select(1.0, 1.2, head);
    len *= mix(1.0, 0.5 + 0.95 * (hn * 0.6 + cn * 0.4), cl);
    // Tussocks (the kind's `tuft`): long blades in a tuft's heart fanning
    // out from it, the grazed sward short between the tufts.
    var tuft_out = vec2<f32>(0.0);
    if (kd.more.z > 0.001 && fine > 0.001) {
        // (A fine sward: tufts of every size, none in the grazed gaps.)
        len *= tuft_height(ftu.w, kd.more.z);
        if (ftu.z > 0.0) {
            let td = length(ftu.xy);
            tuft_out = -ftu.xy / max(td, 1e-4) * (min(ftu.w, 1.0) * smoothstep(0.0, tuft_radius(ftu.z) * 0.5, td) * kd.more.z);
        }
    } else if (kd.more.z > 0.001) {
        let tu = tuft_at(xz);
        let td = length(tu.xy);
        let rad = tuft_radius(tu.z);
        let prof = 1.0 - smoothstep(rad * 0.15, rad, td);
        len *= mix(1.0, 0.42 + 1.25 * prof * tu.z, kd.more.z);
        tuft_out = -tu.xy / max(td, 1e-4) * (prof * smoothstep(0.0, rad * 0.5, td) * kd.more.z);
    }
    if (weed.z > 0.0) {
        len *= mix(mix(0.4, 0.2, fine), 1.0, smoothstep(weed.z * mix(0.6, 0.5, fine), weed.z, length(weed.xy)));
    }
    // A fine sward stands taller where it grows rank, over a hand's breadth
    // and over a stride, and each blade has its own length: most near the
    // sward's, a few long ones over it.
    if (fine > 0.001) {
        let fl = rnd(&fr);
        len *= mix(1.0, (0.72 + 0.56 * (rank_oct(xz * RANK_F, 0) * 0.6 + rank_oct(xz * (RANK_F * 0.25), 1) * 0.4)) * (0.62 + 0.9 * fl * fl), fine);
    }
    // The sward's patches (the kind's `mottle`): taller, coarser growth in
    // its clumps, shorter in its dry patches.
    var mo = vec3<f32>(0.5, 0.0, 0.0);
    if (kd.look.y > 0.001) {
        mo = mottle_at(xz);
        len *= 1.0 + (mix(0.55, 0.4, fine) * mo.y - 0.25 * mo.z) * kd.look.y;
    }
    len *= mix(0.5, 1.0, e) * mix(0.55, 1.0, cover) * (1.0 - 0.4 * rim);
    len *= mix(0.35, 1.0, thin) * mix(0.45, 1.0, far);
    len = max(len, 0.008);
    if (fine > 0.001) {
        len = min(len, kd.shape.x * 2.6);
    }
    // (A fine sward's far blades hardly widen: its carpet carries the
    // cover, and its blades run from thread-thin to a few broad ones.)
    let boost = min(inverseSqrt(max(keep, 1e-4)), mix(sim.lod2.y, 1.3, fine));
    let fw = rnd(&fr);
    var bw = kd.shape.z * (0.7 + 0.6 * rnd(&s)) * select(1.0, 0.4, head) * boost * mix(0.35, 1.0, thin) * mix(0.3, 1.0, far) * mix(1.0, (0.55 + 1.1 * fw * fw) * (1.0 + 0.35 * min(ftu.w, 1.0) * kd.more.z), fine);
    if (sim.stamps_n.z > 0.5) {
        let cen = root + vec3<f32>(0.0, len * 0.5, 0.0);
        let rad = len * 0.8 + 0.05;
        for (var p = 0u; p < 6u; p += 1u) {
            let pl = sim.planes[p];
            if (dot(pl.xyz, cen) + pl.w < -rad) {
                return;
            }
        }
    }
    // Facing, with clumps that share a direction.
    let cdir = cn * 12.566;
    var yaw = mix(rnd(&s) * 6.2832, cdir, cl * 0.55);
    var face = vec2<f32>(sin(yaw), cos(yaw));
    // Bends as 2D vectors on the ground: `phi` tilts the root, `theta`
    // arcs the blade; the blade is a circular arc of its own length.
    var phi = face * kd.bend.x * (0.2 + 0.9 * rnd(&s));
    let fc = rnd(&fr);
    var theta = face * kd.bend.y * (0.35 + 0.9 * rnd(&s)) * select(1.0, 0.3, head) * mix(1.0, 0.2 + 1.9 * fc * fc, fine);
    if (head) {
        phi *= 0.4;
    }
    phi += tuft_out * 0.6;
    let lie = rnd(&s);
    let lie_a = rnd(&s) * 6.2832;
    let lie_k = rnd(&s);
    if (lie < kd.bend.w) {
        phi += vec2<f32>(sin(lie_a), cos(lie_a)) * (1.05 + 0.4 * lie_k);
        theta *= 0.5;
        len *= 0.85;
    }
    // A fine sward's blades hold themselves every way: most lie in sprays,
    // leaning the way their neighbours lean and arcing on over, each a
    // little aside, so they cross; some stand with their own lean and curl,
    // some stand and fold over to hang their tips, some lie matted on the
    // undergrowth (more of them at an opening's rim, over the soil).
    if (fine > 0.001 && !head && !leaf) {
        let pr = rnd(&fr);
        let pa = rnd(&fr) * 6.2832;
        let pk = rnd(&fr);
        let pd = vec2<f32>(sin(pa), cos(pa));
        let matted = (0.07 + 0.3 * rim) * fine;
        if (pr < matted) {
            phi = pd * (1.0 + 0.4 * pk);
            theta = pd * (0.1 + 0.3 * pk);
            len *= 0.85;
        } else if (pr < matted + 0.12 * fine) {
            phi = phi * 0.5 + pd * 0.15;
            theta = normalize(theta + pd * 0.5 + vec2<f32>(1e-4, 0.0)) * (1.3 + 1.0 * pk);
        } else if (pr < matted + 0.72 * fine) {
            let sd = normalize(spray_dir(xz) + pd * 0.6);
            phi = sd * (0.35 + 0.7 * pk) + tuft_out * 0.5;
            theta = sd * (0.25 + 1.25 * fc * fc) * kd.bend.y;
        }
    }
    if (leaf) {
        // A leaf of the rosette: rooted at its heart, pointing outwards,
        // wide, leaning low and arcing over towards the ground.
        let wl = length(weed.xy);
        let out = -weed.xy / max(wl, 1e-4);
        let rxz = xz + weed.xy * 0.9;
        root = vec3<f32>(rxz.x, ground_at(pa, rxz), rxz.y);
        yaw = atan2(out.x, out.y);
        face = out;
        // Each leaf its own length, breadth and lift: the outer ones lie
        // on the sward, the inner ones stand; a rosette is of one kind (its
        // random number's quarter): long leaves that stand (dock), lobed
        // ones (dandelion), broad ones (plantain), very broad ones (burdock).
        let wk = u32(weed.w * 4.0);
        len = max(weed.z * (0.55 + 0.7 * rnd(&ws)) * mix(0.5, 1.0, thin) * mix(0.6, 1.0, far), 0.02);
        let lw = rnd(&ws);
        var broad = 0.3;
        if (wk >= 2u) {
            broad = select(0.5, 0.62, wk == 3u);
        }
        bw = len * broad * (0.8 + 0.4 * lw) * min(boost, 1.6);
        let lift = rnd(&ws);
        phi = out * (select(0.62, 0.22, wk == 0u) + 0.62 * lift);
        theta = out * (0.25 + 0.6 * rnd(&ws));
        if (wk == 0u) {
            len *= 1.35;
        }
    }
    // Wind: a slow sway, gust waves travelling downwind and a flutter;
    // the bend angle grows with the blade's length, so the tip moves with
    // the square of the height.
    let wdir = sim.wind.xy;
    let wside = vec2<f32>(-wdir.y, wdir.x);
    let along = dot(xz, wdir);
    let t = sim.eye.w;
    let gn = vnoise(vec2<f32>(along - t * sim.wind2.x, dot(xz, wside) * 0.55) / max(sim.wind2.y, 0.5));
    let gust = smoothstep(0.3, 0.85, gn);
    let ph = rnd(&s);
    let sway = sin(6.2832 * (t * sim.wind2.w + along * 0.045) + ph * 1.3);
    let push = sim.wind.z * (0.6 + 0.4 * sway) + sim.wind.w * gust;
    let stiff = kd.bend.z;
    let wet = sim.lod2.z;
    let soft = (len / 0.5 + 0.3) / (1.0 + 2.5 * stiff) * (1.0 - 0.2 * wet);
    let fl = sin(t * (5.5 + 5.0 * ph) + ph * 40.0) * sim.wind2.z * (0.35 + gust);
    theta += (wdir * push * 0.5 + wside * fl * 0.12) * soft;
    // Soaked blades droop over along their bend.
    let bd0 = theta + phi + face * 0.01;
    theta += normalize(bd0) * wet * sim.lod2.w * (1.0 - 0.6 * stiff) * clamp(len / max(kd.shape.x, 0.01), 0.3, 1.3);
    // Trampled: the root lies over along the press, the arc relaxes.
    let tr = trample_at(xz);
    let ta = min(length(tr), 1.0);
    let tk = rnd(&s);
    if (ta > 0.002) {
        let k = smoothstep(0.0, 1.0, ta) * (0.8 + 0.2 * tk);
        let td = tr / max(ta, 1e-4);
        phi = mix(phi, td * 1.42, k);
        theta = theta * (1.0 - 0.8 * k) + td * 0.2 * k;
    }
    let bv = phi + theta;
    let bl = length(bv);
    let bdir = select(face, bv / max(bl, 1e-5), bl > 1e-4);
    // A fine sward's blade is a tiller: a few leaves fanning out of one
    // foot, drawn in one strip as broad at its top as the fan spreads (the
    // draw cuts the leaves out of it). It bends over flatwise, the way it
    // leans, each a little turned.
    let tiller = fine > 0.001 && !head && !leaf;
    if (tiller) {
        bw = len * (0.5 + 0.5 * rnd(&fr));
        yaw = atan2(bdir.x, bdir.y) + (rnd(&fr) - 0.5) * 0.9;
    }
    let a0 = clamp(dot(phi, bdir), 0.0, 1.5);
    let ab = clamp(dot(theta, bdir), 0.0, 2.7 - a0);
    let b3 = vec3<f32>(bdir.x, 0.0, bdir.y);
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let t0 = sin(a0) * b3 + cos(a0) * up;
    let t1 = sin(a0 + ab) * b3 + cos(a0 + ab) * up;
    var p1 = t0 * (len / 3.0);
    var p2 = t0 * (len * 2.0 / 3.0);
    var p3 = t0 * len;
    if (ab > 1e-3) {
        let r = len / ab;
        p3 = r * ((cos(a0) - cos(a0 + ab)) * b3 + (sin(a0 + ab) - sin(a0)) * up);
        let hk = 4.0 / 3.0 * tan(ab * 0.25) * r;
        p1 = t0 * hk;
        p2 = p3 - t1 * hk;
    }
    // Long leaves sag over at the tip under their own weight (more when
    // wet); stalks and ears stand stiffer.
    let sag = kd.more.y * (0.4 + 0.9 * rnd(&s)) * clamp(len / 0.5, 0.3, 2.0) * (1.0 + wet * 0.8) * select(1.0, 0.15, head) * (1.0 - 0.6 * ta);
    p3 += (b3 * 0.4 - up) * len * sag * 0.32;
    p2 += (b3 * 0.2 - up * 0.3) * len * sag * 0.22;
    // A fine sward's blades turn aside out of their bend, each its own
    // way: tips point every way, some blades in an S.
    if (fine > 0.001 && !leaf) {
        let sd = vec3<f32>(bdir.y, 0.0, -bdir.x);
        let k1 = (rnd(&fr) - 0.5) * fine;
        let k2 = (rnd(&fr) - 0.5) * fine;
        p2 += sd * (len * k1 * 0.3);
        p3 += sd * (len * (k1 * 0.15 + k2 * 0.4));
    }
    p3.y = max(p3.y, len * 0.04);
    // Wheat: the heavy ear nods over (more when wet).
    var ear = kd.ear.x;
    var earw_m = kd.ear.y;
    if (head) {
        ear = 0.17;
        earw_m = bw * 2.6;
    }
    let nod = (0.45 + 0.8 * rnd(&s)) * (1.0 + wet * 1.2) * (1.0 - 0.7 * ta);
    if (ear > 0.0) {
        p3 += (b3 * 0.55 - up * 0.8) * len * ear * 0.6 * nod;
        p2 += (b3 * 0.3 - up * 0.2) * len * ear * 0.3 * nod;
    }
    // Colour: per-blade brightness and hue, sun-bleached patches.
    let vr = kd.root.w;
    let br = 1.0 + vr * (rnd(&s) - 0.5) * 0.9;
    let hue = (rnd(&s) - 0.5) * vr;
    let dry = clamp(kd.tip.w * dry_at(xz) * 1.5 + (rnd(&s) - 0.72) * kd.tip.w * 0.7, 0.0, 1.0);
    // Each blade has an age: fresh and yellow-green, mature and a deeper
    // blue-green, or old and olive; seed heads ripen towards the dry colour.
    let age = rnd(&s);
    var agec = vec3<f32>(1.0);
    if (age < 0.3) {
        agec = vec3<f32>(1.1, 1.12, 0.8);
    } else if (age < 0.75) {
        agec = vec3<f32>(0.78, 0.94, 1.08);
    } else {
        agec = vec3<f32>(0.82, 0.74, 0.52);
    }
    agec = mix(vec3<f32>(1.0), agec, clamp(vr * 1.2, 0.0, 1.0));
    var tipc = mix(kd.tip.rgb * agec, kd.dry.rgb, dry) * br * vec3<f32>(1.0 + hue * 0.7, 1.0, 1.0 - hue * 0.9) * (1.0 + 0.1 * ta);
    if (head) {
        tipc = mix(tipc, kd.dry.rgb * br, 0.65);
    }
    var rootc = mix(kd.root.rgb * mix(vec3<f32>(1.0), agec, 0.5), kd.dry.rgb * 0.45, dry * 0.4) * br * (1.0 - 0.1 * ta);
    // A fine sward: its patches' colours; a share of blades dead and straw
    // coloured from root to tip; roots nearer the tips' colour (no dark
    // foot under each blade).
    if (fine > 0.001) {
        if (kd.look.y > 0.001) {
            tipc = mottled(tipc, kd.dry.rgb, mo, kd.look.y);
            rootc = mottled(rootc, kd.dry.rgb * 0.6, mo, kd.look.y);
        }
        let straw = rnd(&fr);
        if (straw < (0.04 + 0.5 * kd.tip.w + 0.12 * mo.z * kd.look.y) * fine) {
            tipc = kd.dry.rgb * (0.75 + 0.5 * rnd(&fr));
            rootc = tipc * 0.7;
        }
        rootc = mix(rootc, tipc * 0.62, 0.55 * fine);
    }
    // Some blades have dried, bleached tips.
    let dry_tip = (rnd(&s) < kd.tip.w * 0.45) && !leaf;
    // Shorter than the sward around it: deeper in shade.
    var occ = clamp(len / max(kd.shape.x, 0.01), 0.25, 1.0);
    if (leaf) {
        // Dock and plantain: a deeper, bluer green than the grass, on a
        // paler stalk; the leaves lie over the sward, out of its shade.
        // Each leaf its own shade; an old one yellows.
        let lb = mix(0.65 + 0.6 * rnd(&ws), 0.6 + 0.45 * rnd(&ws), fine);
        let old = smoothstep(0.72, 0.95, rnd(&ws));
        tipc = mix(kd.tip.rgb * vec3<f32>(0.26, 0.4, 0.3), kd.dry.rgb * 0.7, old * 0.7) * lb;
        rootc = mix(kd.tip.rgb * vec3<f32>(0.22, 0.33, 0.25), kd.dry.rgb * 0.55, old * 0.5) * lb;
        occ = 1.0;
    }
    // Flags: ear start (share of the length, 8 bits), ear width as a
    // multiple of the stalk's (quarter steps), the bucket, a weed's leaf
    // (bit 18) and its rosette's kind (bits 19 and 20), a fine sward's tiller
    // (bit 21), the kind, a dried tip.
    let ear0 = select(0u, u32(clamp(1.0 - ear, 0.0, 1.0) * 255.0), ear > 0.0);
    let earw = u32(clamp(earw_m / max(bw, 1e-4) * 4.0, 0.0, 255.0));
    let flags = ear0 | (earw << 8u) | (bk << 16u) | (select(0u, 1u | (min(u32(weed.w * 4.0), 3u) << 1u), leaf) << 18u) | (select(0u, 1u, tiller) << 21u) | (min(u32(pa.info.z), 3u) << 24u) | (select(0u, 1u, dry_tip) << 26u);
    let slot = atomicAdd(&args[bk * 4u + 1u], 1u);
    if (slot >= sim.caps[bk]) {
        return;
    }
    blades[sim.bases[bk] + slot] = Blade(
        vec4<f32>(root, bw),
        vec4<f32>(p1, len),
        vec4<f32>(p2, yaw),
        vec4<f32>(p3, occ),
        vec4<u32>(pack_rgb(rootc), pack_rgb(tipc), flags, s),
    );
}

// ---- clamp the counts to the buckets --------------------------------------
@compute @workgroup_size(1)
fn cs_finish() {
    for (var b = 0u; b < 3u; b += 1u) {
        let n = atomicLoad(&args[b * 4u + 1u]);
        atomicStore(&args[b * 4u + 1u], min(n, sim.caps[b]));
    }
}
