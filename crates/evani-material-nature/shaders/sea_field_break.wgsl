// The field sea's breaking crests (`material.nature.water.breaking`), shared
// by the mesh (sea_field.wgsl) and the spray (sea_spray.wgsl); src/sea/field.rs
// mirrors it.
//
// Not every wave breaks. The sea is divided into cells that travel downwind
// with the waves (long as a breaking wave, wide as a few of its crests); each
// cell, each cycle, breaks with a chance that rises with the wind, drawn from
// a seeded hash, so the same moment always looks the same and everything loops
// with the sea. A breaking cell's crest leans forward (its top moves ahead of
// the water below, so the mesh truly overhangs), falls, throws spray and
// leaves a patch of foam that stays behind as the wave moves on and fades.
// Only the part of a cell where a tall crest actually stands is touched.

struct BrkP {
    // The breaking wave's length (m), the cell's size along and across the
    // wind, the cells' speed, then the seconds to fold over, to collapse and
    // to settle, the whole event and the cycle; the cells in a loop.
    lm: f32,
    cx: f32,
    cy: f32,
    c: f32,
    tf: f32,
    tc: f32,
    tr: f32,
    ev: f32,
    cyc: f32,
    nx: i32,
};

// The chance a cell breaks in a cycle, worked out from the wind and the
// breaking setting (src/sea/field.rs `brk_chance`), shares a word with spray
// (and, in dev builds, the foam view's switch above them).
fn brk_chance() -> f32 {
    return f32((u32(max(u.spray, 0.0) + 0.5) >> 8u) & 255u) * 0.01;
}

fn brk_spray() -> f32 {
    return f32(u32(max(u.spray, 0.0) + 0.5) & 255u) * 0.01;
}

fn brk_params() -> BrkP {
    var p: BrkP;
    let lp = TAU / max(u.wind.x, 1e-5);
    p.lm = clamp(0.33 * lp, 10.0, 130.0);
    p.cx = 0.6 * p.lm;
    p.cy = 0.9 * p.lm;
    let c0 = sqrt(GRAV * p.lm / TAU);
    // A whole number of cells a loop (the sea repeats every 300 s).
    let nx = max(round(c0 * 300.0 / p.cx), 1.0);
    p.nx = i32(nx);
    p.c = nx * p.cx / 300.0;
    let per = p.lm / c0;
    p.tf = 0.5 * per;
    p.tc = 0.3 * per;
    p.tr = 0.35 * per;
    p.ev = p.tf + p.tc + p.tr;
    p.cyc = 300.0 / max(round(300.0 / (3.0 * p.ev)), 1.0);
    return p;
}

// World (x, z) in the wind's frame (along, across), from the sea's centre.
fn brk_wind(p: vec2<f32>) -> vec2<f32> {
    let q = p - u.centre.xz;
    let r = vec2<f32>(cos(u.wind.z), sin(u.wind.z));
    return vec2<f32>(dot(q, r), dot(q, vec2<f32>(-r.y, r.x)));
}

// A row's own shift along the wind, so the cells don't line up in columns.
fn brk_shift(j: i32, pr: BrkP) -> f32 {
    return pr.cx * hash2(vec2<i32>(j, 7));
}

// A point along the wind in the cells' moving frame at loop time t.
fn brk_x(wx: f32, j: i32, t: f32, pr: BrkP) -> f32 {
    return wx - pr.c * t + brk_shift(j, pr);
}

// Seconds since cell (i, j)'s event of cycle n began, or -1 when it has no
// event then. i wraps with the loop.
fn brk_rel(i: i32, j: i32, n: i32, t: f32, pr: BrkP, chance: f32) -> f32 {
    let ii = ((i % pr.nx) + pr.nx) % pr.nx;
    let ncyc = max(i32(round(300.0 / pr.cyc)), 1);
    let nn = ((n % ncyc) + ncyc) % ncyc;
    if (hash3(vec3<i32>(ii, j, nn)) >= chance) {
        return -1.0;
    }
    let start = (pr.cyc - pr.ev) * hash3(vec3<i32>(ii + 1009, j, nn));
    return t - f32(n) * pr.cyc - start;
}

// A breaking event at rest point p now: (fold 0..1, collapse 0..1, settle
// 1..0, the cell's window), all 0 with no event here.
fn brk_at(p: vec2<f32>, chance: f32, pr: BrkP) -> vec4<f32> {
    if (chance <= 0.0) {
        return vec4<f32>(0.0);
    }
    let t = u.tl * 300.0;
    let w = brk_wind(p);
    let j = i32(floor(w.y / pr.cy));
    let x = brk_x(w.x, j, t, pr);
    let i = i32(floor(x / pr.cx));
    let n = i32(floor(t / pr.cyc));
    let rel = brk_rel(i, j, n, t, pr, chance);
    if (rel < 0.0 || rel > pr.ev) {
        return vec4<f32>(0.0);
    }
    let fx = x / pr.cx - floor(x / pr.cx);
    let fy = w.y / pr.cy - floor(w.y / pr.cy);
    // A soft, irregular patch of the cell, not a box.
    let rr = length(vec2<f32>(fx - 0.5, fy - 0.5) * 2.0) + 0.45 * (vnoise(w / (0.22 * pr.lm)).x - 0.5);
    let win = 1.0 - smoothstep(0.45, 0.95, rr);
    return vec4<f32>(smoothstep(0.0, pr.tf, rel), smoothstep(pr.tf, pr.tf + pr.tc, rel), 1.0 - smoothstep(pr.tf + pr.tc, pr.ev, rel), win);
}

// The wave about the breaking length at rest point p, found from its height
// and its slope along the wind (the two turn into each other along a wave):
// (radians from its crest, positive ahead of it downwind; how tall it stands
// here, m; that height in those waves' rms). Longer swell is left out: a lip
// comes off the wave that breaks, whatever it rides on.
fn brk_wave(p: vec2<f32>, pr: BrkP, nb: u32) -> vec3<f32> {
    let r = vec2<f32>(cos(u.wind.z), sin(u.wind.z));
    var h = 0.0;
    var g = 0.0;
    var sg = 0.0;
    for (var b = 0u; b < nb; b = b + 1u) {
        let hi = fband_hi(b);
        let lo = hi / F_RHO;
        if (lo > 1.5 * pr.lm || hi < 0.5 * pr.lm) {
            continue;
        }
        h = h + textureSampleLevel(field_tex, samp, fuv(p, b, 0u), 0.0).y;
        g = g + dot(textureSampleLevel(field_tex, samp, fuv(p, b, 1u), 0.0).xy, r) * sqrt(hi * lo) / TAU;
        sg = sg + textureLoad(field_tex, vec2<i32>(i32(b), i32(F_INFO_Y)), 0).z;
    }
    let env = sqrt(h * h + g * g);
    return vec3<f32>(atan2(-g, h), env, env / max(sqrt(sg), 1e-3));
}

// How strongly a wave this far over its waves' rms goes over.
fn brk_tall(rel: f32) -> f32 {
    return smoothstep(1.2, 2.0, rel);
}

// The lip's length (m) on a wave standing env m: about its height, never long
// against the breaking length. src/sea/field.rs `brk_fold`.
fn brk_lip(env: f32, pr: BrkP) -> f32 {
    return min(1.5 * env, 0.12 * pr.lm);
}

// A breaking wave's shape at a point xf m ahead of its crest (rest position),
// on a wave standing env m, at strength s and moment ev: how far the point goes
// downwind, and how far it goes up. The water from behind the crest up to it
// goes forward together; ahead of the crest, within less than the lip's own
// length, the push drops to nothing, so the top ends up over water that started
// ahead of it: a real overhang. That underside runs back and down into a hollow
// face, which rejoins the wave close ahead (shallow: a steep face, never a
// pit in front of the crest). Then the lip falls and the crest
// collapses. `lean` shortens it (Medium).
fn brk_fold(xf: f32, env: f32, s: f32, ev: vec4<f32>, pr: BrkP, lean: f32) -> vec2<f32> {
    // (The lip's ends round off along the crest, not a plate's straight edges.)
    let l = brk_lip(env, pr) * lean * pow(s, 0.7) * ev.x * (1.0 + 0.5 * ev.y) * ev.z;
    let xa = 0.01 * pr.lm;
    let w = max(0.45 * l, 0.02 * pr.lm);
    let u1 = clamp((xf - xa) / w, 0.0, 1.0);
    let drop = u1 * u1 * (3.0 - 2.0 * u1);
    let go = l * smoothstep(-0.2 * pr.lm, 0.0, xf) * (1.0 - drop);
    // The top curves down to the tip; the underside goes on down into the
    // hollow, which fills again ahead.
    let top = smoothstep(-0.6 * l - 1e-3, 0.0, xf - xa);
    let hollow = l * (0.2 * top + 0.55 * drop) * (1.0 - smoothstep(xa + w, xa + w + 0.12 * pr.lm, xf));
    // First the crest stands up, then it falls away.
    let cr = xf / (0.16 * pr.lm);
    let cf = (xf - 0.06 * pr.lm) / (0.2 * pr.lm);
    let rise = 0.35 * env * s * ev.x * (1.0 - ev.y) * exp(-cr * cr);
    let fall = 0.8 * env * s * ev.y * lean * exp(-cf * cf);
    return vec2<f32>(go, (rise - hollow - fall) * ev.z);
}

// For the light: (the lip, strongest at its tip, the water that started at the
// crest; the white water the collapse spills down the wave's front, further as
// it goes on).
fn brk_white(xf: f32, env: f32, s: f32, s_wide: f32, ev: vec4<f32>, pr: BrkP) -> vec2<f32> {
    let l = brk_lip(env, pr) * pow(s, 0.7) * ev.x * (1.0 + 0.5 * ev.y) * ev.z;
    let d = (xf - 0.01 * pr.lm) / (0.35 * l + 0.01 * pr.lm);
    let reach = (0.03 + 0.16 * ev.y) * pr.lm;
    let spill = smoothstep(-0.04 * pr.lm, 0.0, xf) * (1.0 - smoothstep(0.45 * reach, reach, xf));
    return vec2<f32>(s * ev.x * ev.z * exp(-d * d), s_wide * ev.y * ev.z * spill);
}

// The foam a break leaves at p, only where a crest truly went over: each event
// of the cells ahead of p (this cycle and the last) whose collapse is under
// `life` seconds old. The cells move with the breaking waves, so the crest that
// broke is the one in its cell now; the foam lies along it where it stood when
// it fell (the crest's travel since, back from where it stands now), only if
// it stood tall then (as for the white water: `brk_wave`, at the height's own
// travel since, at the group's speed), a
// band a little down its face, as long as the cell is wide: (young, under half
// its life; old, fading). (It used to lay a round patch at the cell's middle
// whatever stood there: foam with no cause.)
fn brk_foam_src(p: vec2<f32>, chance: f32, pr: BrkP, life: f32, nb: u32) -> vec2<f32> {
    var o = vec2<f32>(0.0);
    if (chance <= 0.0) {
        return o;
    }
    let t = u.tl * 300.0;
    let w = brk_wind(p);
    let j = i32(floor(w.y / pr.cy));
    let x = brk_x(w.x, j, t, pr);
    let i = i32(floor(x / pr.cx));
    let n = i32(floor(t / pr.cyc));
    let dy = w.y - (f32(j) + 0.5) * pr.cy;
    let r = vec2<f32>(cos(u.wind.z), sin(u.wind.z));
    for (var di = 0; di < 3; di = di + 1) {
        for (var dn = 0; dn < 2; dn = dn + 1) {
            let rel = brk_rel(i + di, j, n - dn, t, pr, chance);
            let age = rel - pr.tf - 0.5 * pr.tc;
            if (rel < 0.0 || age < 0.0 || age > life) {
                continue;
            }
            let k = 1.0 - age / life;
            let fade = k * k * smoothstep(0.0, 0.1 * pr.ev, age);
            // The cell's centre now (along the wind from the sea's centre), and
            // the wave there.
            let along = (f32(i + di) + 0.5) * pr.cx + pr.c * t - brk_shift(j, pr);
            let cw = u.centre.xz + r * along + vec2<f32>(-r.y, r.x) * ((f32(j) + 0.5) * pr.cy);
            let wv = brk_wave(cw, pr, nb);
            // Where its crest fell: the crest is wv.x of a wave behind the
            // centre now, and has run pr.c * age since.
            let fell = along - wv.x * pr.lm / TAU - pr.c * age;
            // How tall it stood then: a wave's height travels at its group's
            // speed, half its crests', so that height is now half the crests'
            // run on from where it fell.
            let ga = fell + 0.5 * pr.c * age;
            let gw = u.centre.xz + r * ga + vec2<f32>(-r.y, r.x) * ((f32(j) + 0.5) * pr.cy);
            // (At its birth the break's own test, as the white water's; later the
            // wave that broke only needs to stand somewhat above its sea: the
            // group's height spreads as it travels.)
            let a = age / life;
            let tall = smoothstep(0.9 - 0.5 * a, 1.7 - 0.7 * a, brk_wave(gw, pr, nb).z);
            let da = (w.x - fell - 0.05 * pr.lm) / (0.09 * pr.lm);
            let hb = dy / (0.38 * pr.cy);
            let caused = tall * exp(-da * da - hb * hb) * fade;
            if (age < 0.5 * life) {
                o.x = max(o.x, caused);
            } else {
                o.y = max(o.y, caused);
            }
        }
    }
    return o;
}
