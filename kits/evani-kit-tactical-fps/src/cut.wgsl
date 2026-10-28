// tactical-fps: concrete holes cut by the kit material (src/cut.rs fills in
// the constants below and the `kit_data` records; layout in its module docs).
// Runs in the forward pass and the shadow pass alike, so it reads nothing but
// `kit_data`.

const CUT_HB: f32 = $HB;
const CUT_HN: i32 = $HN;
const CUT_NU: i32 = $NU;
const CUT_NV: i32 = $NV;
const CUT_NY: i32 = $NY;
const CUT_PER: i32 = $PER;
const CUT_BUCKET0: i32 = $BUCKET0;
const CUT_COVER0: i32 = $COVER0;
const CUT_ENTRY0: i32 = $ENTRY0;
const CUT_LOOP: i32 = $LOOP;
const CUT_TAN30: f32 = $TAN30;
const CUT_S60: f32 = $S60;
const CUT_JAG: f32 = $JAG;
const CUT_RIM: f32 = $RIM;

// The broken outline: 0..1 by direction from the centre, a smooth sum of
// three waves seeded by the centre, so no two holes break alike.
fn cut_jag(d: vec3<f32>, c: vec3<f32>) -> f32 {
    let s = dot(c, vec3<f32>(1.37, 2.11, 0.73));
    let w = 0.5 * sin(dot(d, vec3<f32>(2.9, -1.3, 2.2)) * 1.7 + s)
        + 0.3 * sin(dot(d, vec3<f32>(-1.1, 3.4, 1.9)) * 2.3 + s * 1.9)
        + 0.2 * sin(dot(d, vec3<f32>(3.7, 1.6, -2.8)) * 3.1 + s * 2.7);
    return 0.5 + 0.5 * w;
}

// A level-frame coordinate's bucket (holes.rs `hbi`).
fn cut_bucket(v: f32) -> i32 {
    return i32(clamp(floor(v / CUT_HB + f32(CUT_HN / 2)), 0.0, f32(CUT_HN - 1)));
}

// A world point in the turning ring's frame (arena.rs `to_level`, level 1).
fn cut_ring(p: vec3<f32>, ca: f32, sa: f32) -> vec3<f32> {
    return vec3<f32>(p.x * ca + p.z * sa, p.y, -p.x * sa + p.z * ca);
}

// The smallest distance to a hole at level-frame point `q` of level `l`, in
// units of that hole's broken radius (below 1: inside the cut).
fn cut_t(l: i32, q: vec3<f32>) -> f32 {
    let bk = (l * CUT_HN + cut_bucket(q.x)) * CUT_HN + cut_bucket(q.z);
    let rec = kit_data[CUT_BUCKET0 + bk / 2];
    var se = rec.xy;
    if ((bk & 1) == 1) {
        se = rec.zw;
    }
    let st = i32(se.x);
    let n = min(i32(se.y), CUT_LOOP);
    var best = 99.0;
    for (var i = 0; i < n; i += 1) {
        let h = kit_data[CUT_ENTRY0 + st + i];
        let d = q - h.xyz;
        let len = length(d);
        let rc = h.w * (1.0 + CUT_JAG * cut_jag(d / max(len, 1e-6), h.xyz));
        best = min(best, len / rc);
    }
    return best;
}

// Solid concrete at level-frame point `q` of level `l`: the floor slab, or a
// cover cell still standing (arena.rs `probe_level`, holes aside).
fn cut_solid(l: i32, q: vec3<f32>) -> bool {
    let fy = kit_data[1];
    let g = kit_data[2];
    var base = fy.x;
    if (l == 1) {
        base = fy.y;
    }
    if (l == 2) {
        base = fy.z;
    }
    let ly = q.y - base;
    var k = i32(round(atan2(q.z, q.x) / CUT_S60));
    k = ((k % 6) + 6) % 6;
    let a = f32(k) * CUT_S60;
    let ck = cos(a);
    let sk = sin(a);
    let u = q.x * ck + q.z * sk;
    let v = -q.x * sk + q.z * ck;
    if (u < g.x || u > g.y) {
        return false;
    }
    if (ly < 0.0) {
        return ly >= -fy.w && abs(v) <= u * CUT_TAN30 - g.z;
    }
    let iu = i32(floor(u - g.x));
    let iv = i32(floor(v + f32(CUT_NV / 2)));
    let iy = i32(floor(ly));
    if (iu < 0 || iv < 0 || iy < 0 || iu >= CUT_NU || iv >= CUT_NV || iy >= CUT_NY) {
        return false;
    }
    let bit = (l * 6 + k) * CUT_PER + (iu * CUT_NV + iv) * CUT_NY + iy;
    var rec = kit_data[CUT_COVER0 + bit / 96];
    let w = u32(rec[(bit % 96) / 24]);
    return ((w >> u32(bit % 24)) & 1u) == 1u;
}

fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
    if (arrayLength(&kit_data) < u32(CUT_ENTRY0)) {
        return vec4<f32>(1.0);
    }
    let h = kit_data[0];
    let m = f32(material);
    let conc = m == h.z;
    let wall = m == h.w;
    if (!conc && !wall) {
        return vec4<f32>(1.0);
    }
    // The level from a point just inside the surface (arena.rs `level_of`).
    let fy = kit_data[1];
    let q0 = p - n * 0.01;
    var l = 0;
    if (q0.y >= fy.y - fy.w) {
        l = 1;
    }
    if (q0.y >= fy.z - fy.w) {
        l = 2;
    }
    var lp = p;
    var q = q0;
    if (l == 1) {
        lp = cut_ring(p, h.x, h.y);
        q = cut_ring(q0, h.x, h.y);
    }
    if (conc) {
        // Concrete faces: gone inside a hole, darker towards its rim.
        let t = cut_t(l, lp);
        if (t < 1.0) {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(vec3<f32>(mix(0.42, 1.0, smoothstep(1.0, CUT_RIM, t))), 1.0);
    }
    // Crater walls (inside-out spheres): only inside solid concrete, and not
    // where another hole has cut them away.
    if (!cut_solid(l, q) || cut_t(l, lp) < 1.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(1.0);
}
