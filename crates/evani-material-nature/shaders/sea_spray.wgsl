// evani.shader.nature_sea_spray
// The splash of the field sea's breaking crests (sea_field_break.wgsl): soft
// puffs thrown up from the fold line of each breaking cell near the camera,
// flying on the wind and falling back. They come from the vertex index alone
// (no buffers): cells around the camera, a few puffs a cell; a puff with no
// break behind it folds to nothing and costs a vertex. Added to the picture
// behind the mesh's own depth, so a nearer wave hides a far splash. Appended
// to it: sea_common.wgsl, sea_field_bands.wgsl, sea_field_break.wgsl, the
// classic table.

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Sea;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var field_tex: texture_2d<f32>;

struct SprayOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) col: vec3<f32>,
    @location(2) seed: vec2<f32>,
};

// Cells across the grid round the camera, and puffs a cell, per level (Low
// has none); src/sea/field.rs `SPRAY`.
fn sp_grid(level: u32) -> vec2<u32> {
    let g = array<vec2<u32>, 4>(vec2<u32>(0u, 0u), vec2<u32>(12u, 10u), vec2<u32>(20u, 28u), vec2<u32>(20u, 48u));
    return g[min(level, 3u)];
}

fn sp_none() -> SprayOut {
    var o: SprayOut;
    o.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    o.uv = vec2<f32>(0.0);
    o.col = vec3<f32>(0.0);
    o.seed = vec2<f32>(0.0);
    return o;
}

// The sea's height at p (a crest's top; the bands that shape it).
fn sp_height(p: vec2<f32>) -> f32 {
    var h = 0.0;
    for (var b = 0u; b < 6u; b = b + 1u) {
        h = h + textureSampleLevel(field_tex, samp, fuv(p, b, 0u), 0.0).y;
    }
    return h;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> SprayOut {
    let level = u32(quality());
    let g = sp_grid(level);
    let chance = brk_chance();
    if (g.x == 0u || chance <= 0.0) {
        return sp_none();
    }
    let quad = vi / 6u;
    let cnr = vi % 6u;
    let k = i32(quad % g.y);
    let cell = quad / g.y;
    let pr = brk_params();
    let t = u.tl * 300.0;
    let we = brk_wind(u.eye.xz);
    let j = i32(floor(we.y / pr.cy)) + i32(cell / g.x) - i32(g.x / 2u);
    let i = i32(floor(brk_x(we.x, j, t, pr) / pr.cx)) + i32(cell % g.x) - i32(g.x / 2u);
    let n = i32(floor(t / pr.cyc));
    let rel = brk_rel(i, j, n, t, pr, chance);
    if (rel < 0.0 || rel > pr.ev) {
        return sp_none();
    }
    let s0 = vec3<i32>(i * 31 + k, j * 17 - k, n);
    let h1 = hash3(s0);
    let h2 = hash3(s0 + vec3<i32>(5, 11, 3));
    let h3 = hash3(s0 + vec3<i32>(19, 2, 7));
    let h4 = hash3(s0 + vec3<i32>(41, 23, 13));
    // Thrown as the crest goes over, a little before and after the fold's end.
    let age = rel - pr.tf * (0.6 + 0.7 * h1);
    let life = 0.9 + 1.3 * h2;
    if (age < 0.0 || age > life) {
        return sp_none();
    }
    // Where: of a few looks over the cell, in the moving frame and world again
    // now, the one where a lip is folding hardest: spray only comes off a lip.
    let r = vec2<f32>(cos(u.wind.z), sin(u.wind.z));
    let shift = brk_shift(j, pr);
    var score = 0.0;
    var bx = 0.0;
    var wy = 0.0;
    var hm = 0.0;
    var bp = vec2<f32>(0.0);
    for (var q = 0; q < 6; q = q + 1) {
        let hq = hash3(s0 + vec3<i32>(61 + q * 3, 37 - q, 29 + q));
        let hr = hash3(s0 + vec3<i32>(7 - q, 53 + q * 5, 31));
        let wx = (f32(i) + 0.1 + 0.8 * hq) * pr.cx + pr.c * t - shift;
        let yy = (f32(j) + 0.1 + 0.8 * hr) * pr.cy;
        let pw = u.centre.xz + r * wx + vec2<f32>(-r.y, r.x) * yy;
        // (Off the top of a wave that's going over: near its crest.)
        let wv = brk_wave(pw, pr, fbands(level));
        let near = max(cos(wv.x), 0.0);
        let sc = brk_tall(wv.z) * near * near * brk_at(pw, chance, pr).w;
        if (sc > score) {
            score = sc;
            bx = wx;
            wy = yy;
            hm = wv.y;
            bp = pw;
        }
    }
    if (score < 0.12) {
        return sp_none();
    }
    let tall = score;
    let best = sp_height(bp);
    let toss = sqrt(max(hm, 1.0));
    let vup = (1.0 + 2.6 * h4) * toss;
    let fwd = 0.7 * pr.c + 3.0 * (h2 - 0.5);
    // (The crest has moved on since: the puff started behind where it is now.)
    let along = bx + brk_lip(hm, pr) - pr.c * age + fwd * age;
    let across = wy + 2.5 * (h1 - 0.5) * age;
    let base = u.centre.xz + r * along + vec2<f32>(-r.y, r.x) * across;
    let wp = vec3<f32>(base.x, u.centre.y + best + vup * age - 3.8 * age * age, base.y);
    let d = wp - u.eye;
    let f = normalize(u.fwd);
    let s = dot(d, f);
    if (s < 0.5) {
        return sp_none();
    }
    let scale = sqrt(pr.lm / 60.0);
    // At least a pixel and a half wide, so a far splash reads, not sparkles.
    let rad = max((0.22 + 0.7 * age) * scale * (0.5 + 1.0 * h3), 1.5 * s * u.px);
    let cx = array<f32, 6>(-1.0, 1.0, -1.0, -1.0, 1.0, 1.0);
    let cy = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let ru = normalize(u.right);
    let uu = normalize(u.up);
    let q3 = wp + ru * (cx[cnr] * rad) + uu * (cy[cnr] * rad);
    let dq = q3 - u.eye;
    var o: SprayOut;
    o.pos = vec4<f32>(dot(dq, u.right) / dot(u.right, u.right), dot(dq, u.up) / dot(u.up, u.up), 0.05, dot(dq, f));
    o.uv = vec2<f32>(cx[cnr], cy[cnr]);
    let fade = smoothstep(0.0, 0.15, age) * (1.0 - age / life) * (1.0 - age / life);
    let amb = sky(vec3<f32>(0.0, 1.0, 0.0)) * 0.55 + sky(vec3<f32>(0.0, 0.1, 0.0)) * 0.25;
    let lit = amb * 1.3 + u.sun_col * max(u.sun.y, 0.0) * (0.5 + 0.8 * pow(clamp(dot(u.sun, normalize(d)), 0.0, 1.0), 3.0));
    // Thicker puffs for a thin scatter, so the sum over a cell stays alike.
    let density = 28.0 / f32(g.y);
    o.col = lit * tall * fade * clamp(0.42 * density * brk_spray(), 0.0, 1.2);
    o.seed = vec2<f32>(h1, h4) * 37.0;
    return o;
}

@fragment
fn fs_main(in: SprayOut) -> @location(0) vec4<f32> {
    let d = dot(in.uv, in.uv);
    if (d >= 1.0) {
        discard;
    }
    // Torn, not a round ball.
    let torn = smoothstep(0.25, 0.7, vnoise(in.uv * 2.3 + in.seed).x + 0.5 * (1.0 - d) - 0.2);
    let soft = (1.0 - d) * (1.0 - d) * torn;
    return vec4<f32>(in.col * soft, 1.0);
}
