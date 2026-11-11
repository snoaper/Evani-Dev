// evani-grass draws, inside the forward pass: the ground carpet under each
// patch, then the blades of each LOD bucket (one indirect draw each), with
// the scene's light and the game's fog slot (spliced in after this file).

struct View {
    view_proj: mat4x4<f32>,
    // eye (world), segments of this bucket's blades
    eye: vec4<f32>,
    // direction towards the sun, translucency
    sun: vec4<f32>,
    // sun colour times intensity, specular strength
    sun_color: vec4<f32>,
    // ambient, wetness
    ambient: vec4<f32>,
    // fog colour, max distance
    fog: vec4<f32>,
    // rounding, root occlusion, wet darkening, wet gloss
    misc: vec4<f32>,
    // the ground round the patches, carpet on
    ground: vec4<f32>,
    // time, bucket, full-density distance, pixels per metre at 1 m
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> sim: Sim;

struct BladeIn {
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
    @location(2) c: vec4<f32>,
    @location(3) d: vec4<f32>,
    @location(4) e: vec4<u32>,
};

struct BladeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    // the blade's face normal (towards its facing)
    @location(1) n: vec3<f32>,
    // across the blade
    @location(2) side: vec3<f32>,
    // x across (-1..1; a tiller's fan: metres from its middle), y along
    // (0..1)
    @location(3) uv: vec2<f32>,
    // root colour, tip colour, flags
    @location(4) @interpolate(flat) cols: vec3<u32>,
    // ear start (0: none), occlusion, length, kind
    @location(5) @interpolate(flat) info: vec4<f32>,
    // the tuft's shade on this blade (-0.5 its sunny flank .. 1 its lee;
    // 0 in an even sward), 1 + its rosette's kind (0..3) for a weed's leaf
    @location(6) @interpolate(flat) lit: vec2<f32>,
    // the blade's own two random numbers
    @location(7) @interpolate(flat) rb: vec2<f32>,
    // a fine sward's light and rank growth where the blade stands
    // ([`sward_far`]); the breadth of a strip leaves are cut out of (m: over
    // 0 a tiller's fan at its top, under 0 a weed's leaf, 0 a plain blade)
    // and where along it its last row lies
    @location(8) @interpolate(flat) far: vec4<f32>,
};

// A weed's leaf, cut out of its strip (the strip is the leaf's full breadth
// to its last row, then draws in to the tip): the share of the breadth the
// leaf fills at `t` along it (0 root, 1 tip): a thin stalk widening into the
// blade, round-shouldered, widest two fifths along, drawn in to a point.
// `segs` are the strip's segments, `r` the leaf's own random number.
fn leaf_outline(t: f32, segs: f32, r: f32) -> f32 {
    let bq = (t - 0.42) / 0.58;
    var shape = mix(0.1 + 0.08 * r, 1.0, smoothstep(0.04, 0.4, t));
    if (t > 0.42) {
        shape = sqrt(clamp(1.0 - bq * bq, 0.0, 1.0));
    }
    let t1 = leaf_row(max(segs - 1.0, 0.0) / max(segs, 1.0));
    return min(shape, 0.97 * (1.0 - t) / max(1.0 - t1, 1e-3));
}

// Where along a leaf its strip's row `q` (0 root .. 1 tip) lies: the rows
// crowd towards the tip, so the strip is the leaf's full breadth nearly to
// its end and the tip is cut round.
fn leaf_row(q: f32) -> f32 {
    return 1.0 - pow(1.0 - q, 2.2);
}

// A fine sward's light and rank growth at `xz`, as a pixel sees it that
// covers `ax` of the ground along the line of sight (m, a vector) and
// `across` m across it: x how far its rises stand in their own lee (over 0)
// or face the sun (under 0), y how rank it grows (-0.5 grazed low and pale
// .. 0.5 rank and deep). Each of the six sizes of rise ([`rank_oct`]) is
// read at four points along the pixel's depth and shows while the pixels
// resolve it across the line of sight and are not many times deeper than it:
// far off, where a pixel is a finger wide and a stride deep, a hand-wide rise
// is a dash, and there is always a size the pixels resolve, on to the
// horizon. The blades (their vertex stage) and the carpet read the same.
fn sward_far(xz: vec2<f32>, ax: vec2<f32>, across: f32) -> vec2<f32> {
    let l = view.sun.xyz;
    let lh = l.xz / max(length(l.xz), 1e-4);
    let deep = length(ax);
    var slope = 0.0;
    var rank = 0.0;
    var f = RANK_F;
    for (var o = 0; o < 6; o += 1) {
        let vis = (1.0 - smoothstep(0.2, 0.5, across * f)) * (1.0 - smoothstep(2.0, 5.0, deep * f));
        if (vis > 0.002) {
            var a = 0.0;
            var b = 0.0;
            for (var i = 0; i < 4; i += 1) {
                let q = (xz + ax * (f32(i) * 0.25 - 0.375)) * f;
                let v = rank_oct(q, o);
                a += v;
                b += rank_oct(q + lh * 0.35, o) - v;
            }
            rank += (a * 0.25 - 0.5) * vis * select(0.8, 0.55, o < 2);
            slope += b * 0.25 * vis * select(0.2, 0.7 - 0.15 * f32(o), o < 2);
        }
        f *= 0.25;
    }
    return vec2<f32>(slope, rank);
}

// A fine sward's colour `c` under that light and growth (`t` from
// [`sward_far`], `k` the kind's `fine`): paler and yellower where it is
// grazed low, deeper where it grows rank, darker in a rise's lee.
fn far_tone(c: vec3<f32>, t: vec2<f32>, k: f32) -> vec3<f32> {
    return c * mix(vec3<f32>(1.0), mix(vec3<f32>(1.1, 1.06, 0.94), vec3<f32>(0.84, 0.93, 0.9), smoothstep(-0.3, 0.3, t.y)) * clamp(1.0 - 1.8 * t.x, 0.6, 1.4), k);
}

// The shade of the tuft nearest `xz` (a kind's `tuft`, `amount`): up to 1 on
// its side away from the sun, as far as its shadow reaches (longer under a
// low sun); down to -0.5 on the flank that faces the sun.
fn tuft_shade(xz: vec2<f32>, amount: f32, fine: f32) -> f32 {
    let l = view.sun.xyz;
    let ll = length(l.xz);
    let lh = l.xz / max(ll, 1e-4);
    let c = vec2<i32>(floor(xz / TUFT_CELL));
    var lee = 0.0;
    var sunny = 0.0;
    // Every tuft near enough to reach here (a smooth field: no seams
    // between one tuft's ground and the next).
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            var tu = tuft_cell(c + vec2<i32>(i, j), xz);
            if (fine > 0.001) {
                // (A fine sward: tufts of every size, none in some cells.)
                tu = ftuft_cell(c + vec2<i32>(i, j), xz);
            }
            let rad = tuft_radius(tu.z);
            let toward = dot(tu.xy, lh);
            let across = length(tu.xy - lh * toward);
            let reach = min(rad * (0.9 + 0.5 * tu.z * ll / max(l.y, 0.12)), TUFT_CELL * 0.9);
            lee = max(lee, tu.z * smoothstep(rad * 0.15, rad * 0.7, toward) * (1.0 - smoothstep(rad * 0.4, rad * 0.9, across)) * (1.0 - smoothstep(reach * 0.55, reach, toward)));
            sunny = max(sunny, tu.z * smoothstep(rad * 0.1, rad * 0.7, -toward) * (1.0 - smoothstep(rad * 0.5, rad, length(tu.xy))));
        }
    }
    return (lee - 0.5 * sunny * (1.0 - lee)) * amount * step(1e-4, ll);
}

// Width along a blade (t 0 root, 1 tip): a plain blade tapers to a point;
// a wheat stalk stays thin, then the ear swells, holds and tapers into the
// awns.
fn width_at(t: f32, w: f32, ear0: f32, earw: f32, fine: f32) -> f32 {
    if (ear0 <= 0.0) {
        // (A fine sward's blade keeps its breadth to half way, then draws in
        // to a point.)
        return w * mix(pow(max(1.0 - t, 0.0), 0.55), (0.7 + 0.3 * smoothstep(0.0, 0.35, t)) * sqrt(max(1.0 - t, 0.0)), fine);
    }
    if (t < ear0) {
        return w * (1.0 - 0.25 * t);
    }
    let q = (t - ear0) / max(1.0 - ear0, 1e-3);
    return mix(w * 0.75, earw, smoothstep(0.0, 0.2, q)) * (1.0 - 0.94 * smoothstep(0.55, 1.0, q));
}

@vertex
fn vs_blade(in: BladeIn) -> BladeOut {
    let segs = max(u32(view.eye.w), 1u);
    let last = 2u * segs;
    let v = min(in.vi, last);
    let row = v / 2u;
    var u = select(-1.0, 1.0, (v & 1u) == 1u);
    let ear0 = f32(in.e.z & 255u) / 255.0;
    let earw = in.a.w * f32((in.e.z >> 8u) & 255u) * 0.25;
    // Rows packed towards the tip, where the blade bends most; a wheat
    // stalk keeps half its rows for the ear.
    var t = pow(f32(row) / f32(segs), 0.75);
    // (A weed's wide leaf keeps its own facing.)
    let leaf = ((in.e.z >> 18u) & 1u) == 1u;
    // (A fine sward's tiller: its leaves are cut out of its strip, a fan;
    // with one segment it is drawn as a single blade.)
    let tiller = ((in.e.z >> 21u) & 1u) == 1u;
    let fan = tiller && segs >= 2u;
    if (leaf || fan) {
        t = leaf_row(f32(row) / f32(segs));
    }
    if (ear0 > 0.0 && segs >= 3u) {
        let ne = clamp(segs / 2u, 1u, 4u);
        let ns = segs - ne;
        if (row <= ns) {
            t = ear0 * pow(f32(row) / f32(ns), 0.9);
        } else {
            t = ear0 + (1.0 - ear0) * f32(row - ns) / f32(ne);
        }
    }
    if (v == last) {
        t = 1.0;
        u = 0.0;
    }
    let p0 = in.a.xyz;
    let p1 = p0 + in.b.xyz;
    let p2 = p0 + in.c.xyz;
    let p3 = p0 + in.d.xyz;
    let mt = 1.0 - t;
    var pos = mt * mt * mt * p0 + 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t * p3;
    let dp = 3.0 * mt * mt * (p1 - p0) + 6.0 * mt * t * (p2 - p1) + 3.0 * t * t * (p3 - p2);
    let tan = normalize(dp + vec3<f32>(0.0, 1e-6, 0.0));
    let kb = sim.kinds[(in.e.z >> 24u) & 3u];
    let fine = kb.look.x;
    let rb = vec2<f32>(unit_f(hash_u(in.e.w ^ 912367u)), unit_f(hash_u(in.e.w ^ 40291u)));
    // A fine sward's blades twist along their length, each its own way.
    let yaw = in.c.w + (rb.x - 0.5) * 3.2 * fine * t * select(1.0, 0.0, leaf) * select(1.0, 0.25, fan);
    var side = vec3<f32>(cos(yaw), 0.0, -sin(yaw));
    // Blades seen edge-on turn part way towards the eye, so they never thin
    // to nothing (they still read as flat strips from the side).
    let to_eye = view.eye.xyz - pos;
    let vdir = to_eye / max(length(to_eye), 1e-4);
    let n0 = normalize(cross(side, tan) + vec3<f32>(1e-6, 0.0, 0.0));
    let edge = 1.0 - abs(dot(n0, vdir));
    let want = cross(tan, vdir);
    let wl = length(want);
    if (wl > 1e-4 && !leaf) {
        let wn = want / wl;
        let ws = select(wn, -wn, dot(wn, side) < 0.0);
        side = normalize(mix(side, ws, edge * edge * 0.55));
    }
    let n = normalize(cross(side, tan) + vec3<f32>(1e-6, 0.0, 0.0));
    // Never thinner than about a pixel at the root: thinner blades shimmer
    // (far blades are already fewer and wider, and fade to the sward's
    // colour).
    // (A fine sward's far blades go thinner still and melt into its
    // carpet.)
    let wmin = mix(0.85, 0.7, fine) * length(view.eye.xyz - p0) / max(view.extra.w, 1.0);
    var w = width_at(t, max(select(in.a.w, min(in.a.w, kb.shape.z * 1.3), tiller), wmin), ear0, max(earw, wmin * 1.3), fine);
    var strip = 0.0;
    if (leaf) {
        w = max(in.a.w, wmin * 1.6);
        strip = -w;
    }
    if (fan) {
        strip = max(in.a.w, wmin * 3.0);
        w = strip * (0.12 + 0.88 * t);
    }
    pos += side * (u * 0.5 * w);
    pos.y = max(pos.y, p0.y + 0.002 * t);
    // A tussocky sward: the blade stands in its tuft's shade or on its
    // sunny flank (the blade's own kind: bits 24 and 25).
    let tuft = kb.more.z;
    var shade = 0.0;
    if (tuft > 0.001) {
        shade = tuft_shade(p0.xz, tuft, fine);
    }
    var o: BladeOut;
    o.lit = vec2<f32>(shade, select(0.0, 1.0 + f32((in.e.z >> 19u) & 3u), leaf));
    o.rb = rb;
    o.far = vec4<f32>(0.0, 0.0, strip, leaf_row(f32(segs - 1u) / f32(segs)));
    var kind = f32(min(in.e.z >> 24u, 3u));
    if (fine > 0.001) {
        // (A fine sward's blades are shaded as their own kind, dried tip or
        // not.)
        kind = f32((in.e.z >> 24u) & 3u);
        let te = p0 - view.eye.xyz;
        let d0 = max(length(te), 1e-3);
        let across = d0 / max(view.extra.w, 1.0);
        let sf = sward_far(p0.xz, te.xz / max(length(te.xz), 1e-3) * (across / clamp(-te.y / d0, 0.03, 1.0)), across);
        o.far = vec4<f32>(sf, o.far.zw);
    }
    o.pos = view.view_proj * vec4<f32>(pos, 1.0);
    o.world = pos;
    o.n = n;
    o.side = side;
    o.uv = vec2<f32>(select(u, u * 0.5 * w, fan), t);
    o.cols = in.e.xyz;
    o.info = vec4<f32>(ear0, in.d.w, in.b.w, kind);
    return o;
}

fn fogged(c: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let dist = length(world - view.eye.xyz);
    let f = clamp(slot_fog(dist, world.y), 0.0, 1.0);
    return mix(c, view.fog.rgb, f);
}

// The sward of a kind seen from far off: its blades' colour from above,
// sun-bleached patches, and the gust waves sweeping it (the same field
// the blades bend in: brighter where the wind lays it over).
fn sward_albedo(kd: Kind, xz: vec2<f32>) -> vec3<f32> {
    let dry = clamp(kd.tip.w * dry_at(xz) * 1.5, 0.0, 1.0);
    let wdir = sim.wind.xy;
    let gn = vnoise(vec2<f32>(dot(xz, wdir) - view.extra.x * sim.wind2.x, dot(xz, vec2<f32>(-wdir.y, wdir.x)) * 0.55) / max(sim.wind2.y, 0.5));
    let wave = smoothstep(0.3, 0.85, gn) * min(sim.wind.w, 1.5);
    return mix(mix(kd.root.rgb, kd.tip.rgb, 0.72), kd.dry.rgb, dry * 0.8) * (0.9 + 0.14 * wave);
}

// A sward of upright blades seen from far off, as one surface: the blades
// facing the eye are lit by a low sun when it is behind the eye and glow
// through when the eye looks towards it, the tops by a high sun. The far
// blades hand over to exactly this, and the carpet takes it on beyond
// them, so no line shows where the blades end.
fn sward_light(alb: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let v = normalize(view.eye.xyz - world);
    let l = view.sun.xyz;
    let lc = view.sun_color.rgb;
    let vh = normalize(vec3<f32>(v.x, 0.0, v.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let face = dot(vh, vec3<f32>(l.x, 0.0, l.z));
    let side = clamp((face + 0.3) / 1.3, 0.0, 1.0);
    let diff = mix(side, max(l.y, 0.0), clamp(l.y * 1.2, 0.25, 0.8));
    let trans = pow(max(0.0, dot(-v, l)), 3.0) * 0.5 * view.sun.w + max(0.0, -face) * 0.3 * (1.0 - view.sun.w);
    let sky = view.ambient.rgb * 0.92;
    return alb * (sky * 0.85 + lc * diff * 0.8) + alb * vec3<f32>(1.45, 1.55, 0.5) * lc * trans * 0.6;
}

// ---- a fine sward (a kind's `fine`) -----------------------------------------
// Fibres lying one way in the undergrowth: strokes `wide` m across and about
// `lng` m lng along `d`, over ground `q`; `gx` and `gy` are the ground one
// pixel covers across and down the screen. Returns how much of the pixel
// they cover (x), the stroke's own random number (y) and how well the pixels
// resolve them (z): strokes finer than the pixels across their width fade to
// their average, so the carpet never shimmers and thins smoothly with
// distance.
fn fibre(q: vec2<f32>, d: vec2<f32>, wide: f32, lng: f32, th: f32, gx: vec2<f32>, gy: vec2<f32>) -> vec3<f32> {
    let w = vec2<f32>(-d.y, d.x);
    let fw = max(abs(dot(gx, w)), abs(dot(gy, w))) / wide;
    let fl = max(abs(dot(gx, d)), abs(dot(gy, d))) / lng;
    let see = (1.0 - smoothstep(0.35, 0.95, fw)) * (1.0 - smoothstep(0.6, 1.6, fl));
    let cov = 1.0 - smoothstep(0.1, 0.9, th + 0.06);
    if (see <= 0.002) {
        return vec3<f32>(cov, 0.5, 0.0);
    }
    let p = vec2<f32>(dot(q, w) / wide, dot(q, d) / lng);
    let r = lattice(vec2<i32>(floor(p + 0.5)));
    return vec3<f32>(mix(cov, smoothstep(th, th + 0.12, vnoise(p)), see), mix(0.5, r, see), see);
}

// A fine sward's even colour from far off (its standing leaves, too far to
// tell apart).
fn fine_even(kd: Kind) -> vec3<f32> {
    let lo = mix(kd.root.rgb, kd.tip.rgb, 0.3);
    let dryness = clamp(0.06 + kd.tip.w * 0.8, 0.0, 0.6);
    return mix(mix(lo, kd.tip.rgb, 0.62), kd.dry.rgb, dryness * 0.75) * 0.84;
}

// The standing leaves of a fine sward's undergrowth, drawn in the carpet
// itself (no triangles): every cell of an 8 mm grid roots one short upright
// leaf, and the eye's ray to this ground pixel is walked back towards the
// eye over the ground it passed above, meeting the leaves that stand in its
// way. From overhead they are dots among the matted fibres; from a standing
// eye a dense fine pile, in true parallax as the eye moves; too far to
// tell apart they are their even colour, hiding the ground at a low angle.
// Returns their colour times their cover (rgb) and their cover (a).
fn standing(world: vec3<f32>, gx: vec2<f32>, gy: vec2<f32>, kd: Kind, vy: f32) -> vec4<f32> {
    let xz = world.xz;
    let to = xz - view.eye.xz;
    let d = to / max(length(to), 1e-3);
    let w = vec2<f32>(-d.y, d.x);
    // How far the ray climbs for each metre back towards the eye.
    let rise = vy / sqrt(max(1.0 - vy * vy, 1e-4));
    let h0 = kd.shape.x * 0.65;
    let w0 = max(kd.shape.z * 0.8, 0.001);
    let lo = mix(kd.root.rgb, kd.tip.rgb, 0.3);
    let dryness = clamp(0.06 + kd.tip.w * 0.8, 0.0, 0.6);
    let even = fine_even(kd);
    let cover = 1.0 - exp(-0.8 * (h0 / 0.05) * (w0 / 0.005) / max(rise, 1e-3));
    // The ground one pixel covers across the line of sight: the leaves'
    // edges are softened over it.
    let fa = max(abs(dot(gx, w)), abs(dot(gy, w)));
    // (Too far to tell apart they are their even cover.)
    let see = 1.0 - smoothstep(1.0, 2.5, fa / w0);
    if (see <= 0.002 || rise <= 1e-3) {
        return vec4<f32>(even * cover, cover);
    }
    let g = 0.011;
    // (They lean with the spray they stand in.)
    let sp = dot(spray_dir(xz), w) * 0.7;
    // In a tussocky sward the leaves stand tall and close in a tuft, short
    // and thin between the tufts; none stand on open soil. (Both read at the
    // pixel and at the walk's far end: a leaf takes its share of the two.)
    let tuft = kd.more.z;
    let reach = min(h0 * mix(1.0, 2.25, tuft) / rise, 18.0 * g);
    var tp = vec2<f32>(0.5);
    if (tuft > 0.001) {
        tp = vec2<f32>(ftuft_at(xz).w, ftuft_at(xz - d * reach).w);
    }
    let op = vec2<f32>(open_at(xz, kd.ear.z), open_at(xz - d * reach, kd.ear.z));
    let n = i32(clamp(ceil(reach / g), 1.0, 18.0));
    let stp = reach / f32(n);
    var col = vec3<f32>(0.0);
    var acc = 0.0;
    // From the eye's side to the pixel: the nearest leaf first. (A fixed
    // count with no early way out: steps past the pixel add nothing.)
    for (var i = 0; i < 19; i += 1) {
        let k = f32(n - i);
        let p = xz - d * (stp * max(k, 0.0));
        for (var j = 0; j < 3; j += 1) {
            let cell = vec2<i32>(floor((p + w * (g * f32(j - 1))) / g));
            let h1 = hash_u((u32(cell.x) * 1664525u) ^ hash_u(u32(cell.y) + 3266489917u));
            let h2 = hash_u(h1);
            let h3 = hash_u(h2);
            let rel = (vec2<f32>(cell) + vec2<f32>(unit_f(h1), unit_f(h2))) * g - xz;
            // The leaf's root lies `s` m towards the eye; the ray passes
            // `z` m over it.
            let s = -dot(rel, d);
            let z = s * rise;
            let along = clamp(s / max(reach, 1e-4), 0.0, 1.0);
            let tpl = mix(tp.x, tp.y, along);
            let gone = max(mix(op.x, op.y, along), 0.55 * (1.0 - min(tpl * 1.5, 1.0)) * tuft);
            let hb = h0 * mix(1.0, 0.45 + 1.5 * min(tpl, 1.2), tuft) * (0.3 + 0.7 * unit_f(h3));
            let t = clamp(z / hb, 0.0, 1.0);
            let r4 = f32(h3 & 255u) / 255.0;
            let r5 = f32((h3 >> 8u) & 255u) / 255.0;
            let r6 = f32((h2 >> 3u) & 255u) / 255.0;
            let r7 = f32((h1 >> 22u) & 255u) / 255.0;
            // It leans its own way and bends aside, and draws in to its tip.
            let u = dot(rel, w) + ((r4 - 0.5) * 0.9 + sp) * z + (r5 - 0.5) * 0.9 * z * t;
            let hit = step(0.0, k) * step(0.0, s) * step(z, hb) * (1.0 - step(0.98, acc)) * step(gone, r7);
            let a = clamp((0.5 * w0 * (0.6 + 0.8 * r6) * pow(1.0 - t, 0.6) - abs(u)) / max(fa, 1e-5) + 0.5, 0.0, 1.0) * hit;
            var c = mix(lo, kd.tip.rgb, 0.25 + 0.75 * f32((h1 >> 5u) & 255u) / 255.0);
            c = mix(c, kd.dry.rgb * (0.8 + 0.4 * r5), step(1.0 - dryness * 0.6, f32((h1 >> 14u) & 255u) / 255.0));
            // In shade at the foot (deeper in a tuft); each leaf faces the
            // light its own way.
            c *= (mix(0.3, 0.22, min(tpl, 1.0) * tuft) + 0.9 * t) * (0.55 + 0.9 * r6);
            col += (1.0 - acc) * a * c;
            acc += (1.0 - acc) * a;
        }
    }
    return vec4<f32>(mix(even * cover, col, see), mix(cover, acc, see));
}

// ---- a fine sward from far off ----------------------------------------------
// Past where its blades stand close, a fine sward's tufts and weeds are
// drawn in the carpet itself (no triangles) as what the eye makes of them
// from there: low mounds standing on the ground, each hiding what lies
// behind it, lit on its sunward flank and dark in its lee. The eye's ray to
// a ground pixel is walked back towards the eye over the grid cells it
// passed above, and meets the mound that stands in its way nearest the eye
// (each cell's mound is met exactly: none is stepped over). They are the
// same tufts and rosettes the blades grow in, so as the eye nears the
// blades rise where the mounds were; they are in true parallax as the eye
// moves, they shrink evenly with distance, and too small to tell apart they
// are the sward's even colour.

// What the eye's ray meets.
struct Mound {
    // how far towards the eye from the ground pixel (m; < 0: it meets none)
    s: f32,
    // from the mound's heart to where the ray meets it (ground plan, m)
    rho: vec2<f32>,
    // the mound's radius and height (m)
    size: vec2<f32>,
    // its own random number; 1 for a weed
    id: vec2<f32>,
    // half the ray's run inside it (m on the ground)
    half: f32,
};

// How much of a tuft's or rosette's height reads as a mound (the blades thin
// out towards their tips).
const MOUND_H: f32 = 0.8;

// The eye's ray (to ground point 0, from direction `d` on the ground, `w`
// across it, climbing `rise` for each metre back towards the eye) against a
// mound of radius `r` and height `ht` whose heart lies at `rel`: how far
// back towards the eye it meets the mound, nearest the eye (x; < 0: not at
// all), and half its run inside the mound (y, m on the ground: short where
// it only grazes the mound's outline).
fn mound_hit(rel: vec2<f32>, r: f32, ht: f32, d: vec2<f32>, w: vec2<f32>, rise: f32) -> vec2<f32> {
    let s = -dot(rel, d);
    let u = dot(rel, w);
    let k = 1.0 - (u * u) / (r * r);
    if (k <= 0.0) {
        return vec2<f32>(-1.0, 0.0);
    }
    let a = ht / (r * r);
    let disc = rise * rise - 4.0 * a * (rise * s - ht * k);
    if (disc <= 0.0) {
        return vec2<f32>(-1.0, 0.0);
    }
    let half = sqrt(disc) / (2.0 * a);
    let sp = s + half - rise / (2.0 * a);
    if (sp < 0.0) {
        return vec2<f32>(-1.0, 0.0);
    }
    return vec2<f32>(sp, half);
}

// A mound's light against the flat sward's, where the ray meets it: lit on
// its sunward flank, dark in its lee and at its foot (x), and how far up
// the mound that is (y, 0..1).
fn mound_light(m: Mound, rise: f32, l: vec3<f32>) -> vec2<f32> {
    let r2 = m.size.x * m.size.x;
    let mn = normalize(vec3<f32>(2.0 * m.size.y * m.rho.x / r2, 1.0, 2.0 * m.size.y * m.rho.y / r2));
    let up = clamp(m.s * rise / m.size.y, 0.0, 1.0);
    return vec2<f32>(mix(1.0, clamp(dot(mn, l) / max(l.y, 0.15), 0.0, 2.0), 0.6) * (0.5 + 0.62 * up), up);
}

// The cell (of a grid of `cell` m) the eye's ray to `xz` crosses `i` columns
// back towards the eye (columns along the axis the ray runs nearest).
fn ray_cell(xz: vec2<f32>, d: vec2<f32>, i: i32, cell: f32) -> vec2<i32> {
    let c0 = vec2<i32>(floor(xz / cell));
    if (abs(d.x) >= abs(d.y)) {
        let cm = c0.x - i * select(-1, 1, d.x >= 0.0);
        let s = (xz.x - (f32(cm) + 0.5) * cell) / select(d.x, 1e-4, abs(d.x) < 1e-4);
        return vec2<i32>(cm, i32(floor((xz.y - d.y * s) / cell)));
    }
    let cm = c0.y - i * select(-1, 1, d.y >= 0.0);
    let s = (xz.y - (f32(cm) + 0.5) * cell) / d.y;
    return vec2<i32>(i32(floor((xz.x - d.x * s) / cell)), cm);
}

// The tuft or rosette the eye's ray to `xz` meets nearest the eye.
fn far_mounds(xz: vec2<f32>, d: vec2<f32>, w: vec2<f32>, rise: f32, kd: Kind) -> Mound {
    var best = Mound(-1.0, vec2<f32>(0.0), vec2<f32>(1.0), vec2<f32>(0.0), 0.0);
    let rs = max(rise, 1e-3);
    // (The cells beside a column: across the axis the ray runs nearest.)
    let along_x = abs(d.x) >= abs(d.y);
    let side = vec2<i32>(select(1, 0, along_x), select(0, 1, along_x));
    let run = max(abs(d.x), abs(d.y));
    if (kd.more.z > 0.001) {
        let top = kd.shape.x * tuft_height(1.2, kd.more.z) * MOUND_H;
        let n = i32(clamp(ceil(top / rs * run / TUFT_CELL), 0.0, 17.0)) + 1;
        for (var i = 0; i <= 18; i += 1) {
            if (i > n) {
                break;
            }
            let c = ray_cell(xz, d, i, TUFT_CELL);
            for (var j = -1; j <= 1; j += 1) {
                let q = c + side * j;
                let tu = ftuft_cell(q, xz);
                if (tu.z <= 0.0) {
                    continue;
                }
                let r = tuft_radius(tu.z) * 0.85;
                let ht = kd.shape.x * tuft_height(tu.z, kd.more.z) * MOUND_H;
                let hit = mound_hit(tu.xy, r, ht, d, w, rise);
                if (hit.x > best.s) {
                    let id = unit_f(hash_u((u32(q.x) * 73856093u) ^ hash_u(u32(q.y) + 19349663u)));
                    best = Mound(hit.x, -d * hit.x - tu.xy, vec2<f32>(r, ht), vec2<f32>(id, 0.0), hit.y);
                }
            }
        }
    }
    if (kd.more.w > 0.001) {
        let n = i32(clamp(ceil(0.3 / rs * run / WEED_CELL), 0.0, 17.0)) + 1;
        for (var i = 0; i <= 18; i += 1) {
            if (i > n) {
                break;
            }
            let c = ray_cell(xz, d, i, WEED_CELL);
            for (var j = -1; j <= 1; j += 1) {
                let wc = weed_cell(c + side * j, xz, kd.more.w);
                if (wc.z <= 0.0) {
                    continue;
                }
                // (A rosette: broad leaves lifting to about half its
                // radius.)
                let wh = clamp(wc.z * select(0.55, 1.0, wc.w < 0.25), 0.03, 0.3);
                let hit = mound_hit(wc.xy, wc.z, wh, d, w, rise);
                if (hit.x > best.s) {
                    best = Mound(hit.x, -d * hit.x - wc.xy, vec2<f32>(wc.z, wh), vec2<f32>(wc.w, 1.0), hit.y);
                }
            }
        }
    }
    return best;
}

// Further off still, where a rosette is too low to tell apart, a crowded
// cluster of them (weed_cell's block of plots) is one broad low clump: the
// one the eye's ray to `xz` meets nearest the eye.
fn far_clumps(xz: vec2<f32>, d: vec2<f32>, w: vec2<f32>, rise: f32, kd: Kind) -> Mound {
    var best = Mound(-1.0, vec2<f32>(0.0), vec2<f32>(1.0), vec2<f32>(0.0), 0.0);
    if (kd.more.w <= 0.001) {
        return best;
    }
    let cell = WEED_CELL * 4.0;
    let along_x = abs(d.x) >= abs(d.y);
    let side = vec2<i32>(select(1, 0, along_x), select(0, 1, along_x));
    let run = max(abs(d.x), abs(d.y));
    let n = i32(clamp(ceil(0.2 / max(rise, 1e-3) * run / cell), 0.0, 17.0)) + 1;
    for (var i = 0; i <= 18; i += 1) {
        if (i > n) {
            break;
        }
        let c = ray_cell(xz, d, i, cell);
        for (var j = -1; j <= 1; j += 1) {
            let q = c + side * j;
            let hq = hash_u((u32(q.x) * 1597334677u) ^ hash_u(u32(q.y) + 3812015801u));
            let cu = unit_f(hq);
            // (As crowded as the share of its plots that carry a rosette.)
            let full = clamp(kd.more.w * (0.05 + 2.6 * cu * cu * cu), 0.0, 1.0);
            if (full < 0.45) {
                continue;
            }
            let h2 = hash_u(hq + 7u);
            let h3 = hash_u(h2);
            let o = (vec2<f32>(q) + 0.3 + 0.4 * vec2<f32>(unit_f(h2), unit_f(h3))) * cell - xz;
            let big = unit_f(hash_u(h3));
            let r = cell * (0.1 + 0.3 * full * big);
            let ht = 0.06 + 0.13 * full * big;
            let hit = mound_hit(o, r, ht, d, w, rise);
            if (hit.x > best.s) {
                best = Mound(hit.x, -d * hit.x - o, vec2<f32>(r, ht), vec2<f32>(unit_f(hash_u(h3 + 3u)), 1.0), hit.y);
            }
        }
    }
    return best;
}

// The undergrowth of a fine sward as the eye meets it from `vy` (the sine
// of its height angle over the ground): matted fibres in three sizes
// (bunches a thumb wide, tufts of a few leaves, single leaves), each lying
// three ways and bending, green from the kind's root to tip colour with dead
// straw among them, over the dark thatch that shows between; and the
// standing leaves ([`standing`]) over them. With `gx` and `gy` huge it is
// the sward's even colour from far off.
fn fine_albedo(world: vec3<f32>, gx: vec2<f32>, gy: vec2<f32>, kd: Kind, vy: f32, soil: vec4<f32>) -> vec3<f32> {
    let xz = world.xz;
    // Looking down, the eye sees between the fibres into the thatch; at a
    // low angle they hide it.
    let th = 0.55 + 0.16 * smoothstep(0.0, 0.6, vy);
    let dryness = clamp(0.06 + kd.tip.w * 0.8, 0.0, 0.6);
    let lo = mix(kd.root.rgb, kd.tip.rgb, 0.3);
    let mean = mix(mix(lo, kd.tip.rgb, 0.5), kd.dry.rgb * 0.95, dryness);
    var col = mix(kd.root.rgb, kd.dry.rgb, 0.25) * 0.28;
    var wide = 0.032;
    var lng = 0.26;
    for (var j = 0; j < 3; j += 1) {
        let fj = f32(j);
        let wq = xz * (0.5 / lng) + vec2<f32>(fj * 19.7, 4.1);
        let q = xz + (vec2<f32>(vnoise(wq), vnoise(wq + vec2<f32>(37.0, 11.0))) - 0.5) * (lng * 0.9);
        for (var i = 0; i < 3; i += 1) {
            let a = f32(i) * 1.0472 + fj * 0.41 + 0.2;
            let f = fibre(q + vec2<f32>(f32(i) * 3.7, fj * 5.3), vec2<f32>(cos(a), sin(a)), wide, lng, th, gx, gy);
            var c = mix(lo, kd.tip.rgb, f.y);
            c = mix(c, kd.dry.rgb * (0.7 + 0.5 * fract(f.y * 13.7 + 0.37)), step(1.0 - dryness, fract(f.y * 5.3 + 0.11)));
            // (The thumb-wide bunches are soft shade more than leaves; the
            // undergrowth lies under the blades, in their shade.)
            col = mix(col, mix(mean, c, f.z * (0.45 + 0.275 * fj)) * (0.6 + 0.13 * fj), f.x);
        }
        wide *= 0.33;
        lng *= 0.6;
    }
    // (Open soil, `soil` rgb where its a is 1, lies under the standing
    // leaves: those on the eye's side of an opening hide its near edge.)
    col = mix(col, soil.rgb, soil.a);
    let st = standing(world, gx, gy, kd, vy);
    return col * (1.0 - st.a) + st.rgb;
}

// A sward seen at a low angle (`vy` the sine of the eye's height angle over
// it) turns lighter and paler (a kind's `sheen`, `k`): the eye meets the lit
// tips and the sky's gloss on the leaves, not the shade between them.
fn sheen(c: vec3<f32>, vy: f32, k: f32) -> vec3<f32> {
    let g = 1.0 - smoothstep(0.02, 0.36, vy);
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    return mix(c, vec3<f32>(lum), 0.16 * k * g * g * g) * (1.0 + 0.9 * k * pow(g, 1.5));
}

// The wheat ear: rows of grains in a chevron down the ear, a few awn
// lines running up off them.
fn ear_shade(q: f32, u: f32) -> vec2<f32> {
    let rows = 10.0;
    let g = fract(q * rows - abs(u) * 0.7);
    let grain = smoothstep(0.0, 0.22, g) * (1.0 - smoothstep(0.78, 1.0, g));
    let bulge = 1.0 - 0.35 * abs(u);
    let awn_line = 1.0 - smoothstep(0.0, 0.07, abs(fract(q * rows * 0.5 + abs(u) * 1.6) - 0.5));
    let awn = awn_line * smoothstep(0.55, 0.95, abs(u)) * smoothstep(0.25, 0.6, q);
    return vec2<f32>(mix(0.62, 1.12, grain) * bulge, awn);
}

struct BladeFrag {
    @location(0) color: vec4<f32>,
    // The pixel's samples the blade covers: a leaf cut out of its strip has
    // an edge a pixel soft (all of them for a plain blade).
    @builtin(sample_mask) mask: u32,
};

// `a` of a pixel's four samples (the odd part dithered pixel by pixel).
fn cover_mask(a: f32, p: vec2<f32>) -> u32 {
    let d = fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
    return (1u << u32(clamp(a * 4.0 + d, 0.0, 4.0))) - 1u;
}

@fragment
fn fs_blade(in: BladeOut) -> BladeFrag {
    var t = in.uv.y;
    var ux = in.uv.x;
    var rb = in.rb;
    var cov = 1.0;
    var turn = 0.0;
    let ear0 = in.info.x;
    let kd = sim.kinds[u32(in.info.w)];
    let fine = kd.look.x;
    var root = unpack_rgb(in.cols.x);
    var tip = unpack_rgb(in.cols.y);
    var dry_tip = ((in.cols.z >> 26u) & 1u) == 1u;
    let dist = length(view.eye.xyz - in.world);
    // What one pixel covers here (m).
    let pxm = dist / max(view.extra.w, 1.0);
    if (in.far.z > 0.0) {
        // A fine sward's tiller: four leaves fanning out of its foot, cut
        // out of its strip, each its own length, breadth, sideways curve,
        // green and turn to the light; the middle ones the longest. A leaf
        // is never under about a pixel broad (far off the fan closes up).
        let hw = 0.5 * in.far.z * (0.12 + 0.88 * in.uv.y);
        var bh = 0u;
        cov = 0.0;
        for (var i = 0u; i < 4u; i += 1u) {
            let h1 = hash_u(bitcast<u32>(in.rb.x) ^ (i * 2654435769u + 1013904223u));
            let h2 = hash_u(h1);
            let c0 = (f32(i) - 1.5) * 0.4 + (unit_f(h1) - 0.5) * 0.3;
            let li = (1.0 - (abs(c0) + 0.15) * (1.0 - in.far.w)) * (0.5 + 0.5 * unit_f(h2));
            let q = in.uv.y / li;
            if (q < 1.0) {
                let off = (in.uv.x - (c0 + (f32(h2 & 255u) / 255.0 - 0.5) * 0.5 * in.uv.y * in.uv.y) * hw) / pxm;
                let hp = max(0.5 * kd.shape.z * (0.8 + 1.1 * f32(h1 & 255u) / 255.0) / pxm, 0.45) * (0.7 + 0.3 * smoothstep(0.0, 0.35, q)) * sqrt(1.0 - q);
                let a = clamp(min(hp - abs(off) + 0.5, 2.0 * hp), 0.0, 1.0);
                if (a > cov) {
                    cov = a;
                    t = q;
                    ux = clamp(off / max(hp, 1e-3), -1.0, 1.0);
                    bh = h2;
                }
            }
        }
        if (cov <= 0.02) {
            discard;
        }
        let h3 = hash_u(bh + 77u);
        rb = vec2<f32>(unit_f(h3), unit_f(hash_u(h3)));
        turn = (rb.x - 0.5) * 1.3;
        let dead = step(f32(h3 & 255u) / 255.0, 0.03 + 0.3 * kd.tip.w);
        tip = mix(tip * (0.8 + 0.4 * rb.y), kd.dry.rgb * (0.75 + 0.5 * rb.x), dead);
        root = mix(root * (0.85 + 0.3 * rb.y), kd.dry.rgb * 0.55, dead);
        dry_tip = f32((bh >> 8u) & 255u) / 255.0 < kd.tip.w * 0.45;
    }
    var alb = mix(root, tip, smoothstep(0.0, 0.85, t));
    if (dry_tip) {
        alb = mix(alb, kd.dry.rgb, smoothstep(0.7, 0.97, t) * 0.85);
    }
    // A faint lighter midrib and darker edges.
    alb *= 0.9 + 0.14 * (1.0 - abs(ux));
    var rounding = view.misc.x;
    var awn = 0.0;
    var leafy = 0.0;
    if (ear0 > 0.0 && t > ear0) {
        let q = (t - ear0) / max(1.0 - ear0, 1e-3);
        let es = ear_shade(q, ux);
        alb = tip * es.x;
        awn = es.y;
        rounding *= 1.6;
    }
    // With distance the blades lose their deep shade (fk), then hand over
    // to the far sward's shading (fs), so the thinned field never speckles
    // and meets the carpet without a line.
    let reach = max(kd.ear.w, 0.05);
    let dr = dist / reach;
    let fk = smoothstep(view.extra.z * 0.7, view.fog.w, dr);
    let fs = smoothstep(view.fog.w * 0.5, view.fog.w * 0.9, dr);
    if (in.lit.y > 0.5) {
        // A weed's broad leaf: a pale midrib, side veins running out from
        // it, the blade channelled along the rib.
        let ax = abs(ux);
        // Its outline is its own: a wavy margin, bitten here and there
        // (cut out of the strip, its edge a pixel soft).
        let ph = rb.x * 40.0;
        let bite = smoothstep(0.72, 0.95, vnoise(vec2<f32>(t * 7.0 + ph, ux * 1.5 + ph * 0.3)));
        // (A dandelion's leaf is cut into lobes, deepest towards its foot.)
        let lobe = pow(0.5 + 0.5 * sin(t * (24.0 + 8.0 * rb.y) + ph), 2.0) * (1.0 - smoothstep(0.7, 0.92, t)) * select(0.0, 0.55, u32(in.lit.y - 0.5) == 1u);
        cov = clamp((leaf_outline(t, view.eye.w, rb.y) * (0.88 + 0.08 * sin(t * (9.0 + 9.0 * rb.y) + ph) + 0.04 * sin(t * 29.0 + ph * 1.7) - 0.4 * bite) * (1.0 - lobe) - ax) * -0.5 * in.far.z / pxm + 0.5, 0.0, 1.0);
        if (cov <= 0.02) {
            discard;
        }
        let vein = smoothstep(0.8, 1.0, abs(fract(t * 6.0 - ax * 1.4) * 2.0 - 1.0)) * (1.0 - fk);
        alb *= 1.0 + 0.16 * vein;
        alb = mix(alb, alb * 1.35 + vec3<f32>(0.01, 0.015, 0.0), smoothstep(0.14, 0.04, ax) * 0.6);
        // Blotched and browning towards the margin, each leaf differently.
        alb *= 0.8 + 0.4 * vnoise(vec2<f32>(t * 5.0 + ph, ux * 2.0 - ph));
        alb = mix(alb, kd.dry.rgb * 0.55, smoothstep(0.55, 1.0, ax) * rb.y * 0.6);
        rounding = -0.7;
        leafy = 1.0;
    }
    let wet = view.ambient.w;
    let darken = 1.0 - view.misc.z * wet;
    alb *= darken;
    let v = normalize(view.eye.xyz - in.world);
    var n = normalize(in.n);
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    let side = normalize(in.side);
    if (turn != 0.0) {
        n = normalize(n + side * turn);
    }
    let nr = normalize(n + side * (ux * rounding));
    let l = view.sun.xyz;
    let lc = view.sun_color.rgb;
    // Wrapped diffuse: thin blades pass light round their edges.
    let ndl = dot(nr, l);
    let diff = clamp((ndl + 0.3) / 1.3, 0.0, 1.0);
    // Deep in the sward the blades shade each other.
    let depth = clamp(in.uv.y * mix(1.0, 1.4, 1.0 - in.info.y), 0.0, 1.0);
    // (A fine sward's blades stand in less shade at the foot: its carpet
    // is the shade between them.)
    let ao = mix(mix(1.0 - view.misc.y * mix(1.0, 0.8, fine), 1.0, smoothstep(0.0, 0.85, depth)) * mix(mix(0.7, 0.8, fine), 1.0, in.info.y), 1.0, fk * 0.85);
    // (In a tussocky sward: less sun in a tuft's lee, more on its sunny
    // flank; 1 in an even sward.)
    let tuft_k = 1.0 - mix(0.7, 0.9, fine) * max(in.lit.x, 0.0) + mix(0.5, 0.8, fine) * max(-in.lit.x, 0.0);
    // (A fine sward's leaves stand deep in each other's shade at the foot,
    // and one in eight in shade to its tip.)
    let sun_ao = mix(mix(mix(0.4, 0.35, fine), 1.0, smoothstep(0.0, mix(0.7, 0.8, fine), depth)), 1.0, fk * 0.85) * tuft_k * mix(1.0, 0.3 + 0.95 * smoothstep(0.12, 0.45, rb.y), fine * (1.0 - fk));
    // Translucency: the sun behind a blade glows through it, most at the
    // thin tips and looking towards the sun; and a bright rim along blades
    // seen edge-on against it.
    // Every tier lets light through a blade lit from behind (cheap, view
    // independent); the glow looking into the sun and the rim are the
    // translucency the tier may leave out (Lite and Low).
    let toward = max(0.0, dot(-v, l));
    let back = max(0.0, -dot(n, l)) * 0.6 + pow(toward, 3.0) * 0.9 * view.sun.w;
    // (In a fine sward each blade lets its own share of light through and
    // has its own gloss.)
    let trans = back * (0.3 + 0.7 * t) * (1.0 - 0.4 * wet) * mix(1.0, 0.4 + 1.2 * rb.x, fine) * (1.0 - 0.6 * leafy);
    let tint = alb * vec3<f32>(1.45, 1.55, 0.5) + vec3<f32>(0.025, 0.035, 0.0) * darken;
    let rim = pow(1.0 - abs(dot(n, v)), 2.0) * pow(toward, 6.0) * 0.3 * view.sun.w;
    // Highlight: soft when dry, a tight wet sheen when soaked.
    let h = normalize(l + v);
    let gloss = view.misc.w * wet;
    let shin = mix(18.0, 160.0, gloss);
    let fres = 0.04 + 0.96 * pow(1.0 - max(dot(nr, v), 0.0), 5.0);
    // (A fine sward's leaves have a dry gloss of their own: the ones turned
    // right glint pale in the sun.)
    let spec = pow(max(dot(nr, h), 0.0), shin) * mix(mix(0.05, 0.42, fine), 0.9, gloss) * (shin + 8.0) / 25.0 * view.sun_color.w * max(ndl, 0.0) * mix(1.0, 0.2 + 2.2 * rb.y * rb.y, fine);
    let sky = view.ambient.rgb * (0.65 + 0.35 * nr.y);
    var c = alb * (sky * ao + lc * diff * sun_ao * ao) + tint * lc * trans * ao + lc * spec * mix(mix(0.04, 0.3, fine), 1.0, fres) * sun_ao;
    c += sky * (fres * gloss * 0.35 + (0.02 + 0.1 * fres) * leafy * fine) * ao;
    c += lc * (awn * 0.06 * (0.3 + diff) + rim * sun_ao);
    let vy = clamp(v.y, 0.0, 1.0);
    if (fs > 0.0) {
        var far_alb = sward_albedo(kd, in.world.xz);
        if (fine > 0.001) {
            // A fine sward's far blades hand over to its carpet's colour.
            far_alb = mottled(fine_albedo(in.world, vec2<f32>(1e3, 0.0), vec2<f32>(0.0, 1e3), kd, vy, vec4<f32>(0.0)), kd.dry.rgb, mottle_at(in.world.xz), kd.look.y);
        }
        c = mix(c, sward_light(far_alb * darken, in.world), fs);
    }
    if (fine > 0.001) {
        c = far_tone(c, in.far.xy, fine);
    }
    if (kd.look.z > 0.001) {
        c = sheen(c, vy, kd.look.z);
    }
    var o: BladeFrag;
    o.color = vec4<f32>(fogged(c, in.world), 1.0);
    o.mask = 0xffffffffu;
    if (cov < 1.0) {
        o.mask = cover_mask(cov, in.pos.xy);
    }
    return o;
}

// ---- ground carpet --------------------------------------------------------
// How much of the light reaches a fine sward's undergrowth under its blades.
const UNDER: f32 = 0.85;

struct GroundOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) pidx: u32,
};

@vertex
fn vs_ground(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> GroundOut {
    let pa = sim.patches[ii];
    let fx = select(0.0, 1.0, vi == 1u || vi == 3u || vi == 4u);
    let fz = select(0.0, 1.0, vi == 2u || vi == 4u || vi == 5u);
    let p = vec3<f32>(mix(pa.rect.x, pa.rect.z, fx), pa.info.x + 0.003, mix(pa.rect.y, pa.rect.w, fz));
    var o: GroundOut;
    o.pos = view.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    o.pidx = ii;
    return o;
}

// The carpet under a patch: near, the dark undergrowth and soil between
// the blades; past the blades' reach, the far sward itself.
@fragment
fn fs_ground(in: GroundOut) -> @location(0) vec4<f32> {
    let pa = sim.patches[in.pidx];
    let kd = sim.kinds[min(u32(pa.info.z), 3u)];
    let xz = in.world.xz;
    // The ground a pixel covers (m): tufts and weeds fade into the even
    // sward before they are finer than the pixels.
    let gx = dpdx(xz);
    let gy = dpdy(xz);
    let px = max(length(gx), length(gy));
    let rc = pa.rect;
    let edge = min(min(xz.x - rc.x, rc.z - xz.x), min(xz.y - rc.y, rc.w - xz.y));
    let e = smoothstep(0.0, max(pa.info.y, 1e-3) * 1.2, edge);
    // A tussocky sward: the ground lies in a tuft's shade or on its sunny
    // flank; under a weed's rosette it is the weed's deeper green.
    var tuft_k = 1.0;
    var weedy = 0.0;
    let tuft_see = 1.0 - smoothstep(TUFT_CELL * 0.3, TUFT_CELL * 1.1, mix(px, min(px, 6.0 * min(length(gx), length(gy))), step(0.001, kd.look.x)));
    if (kd.more.z > 0.001 && tuft_see > 0.0) {
        let sh = tuft_shade(xz, kd.more.z, kd.look.x) * tuft_see;
        tuft_k = 1.0 - 0.7 * max(sh, 0.0) + 0.5 * max(-sh, 0.0);
    }
    let weed_see = 1.0 - smoothstep(0.07, 0.28, px);
    if (kd.more.w > 0.001 && weed_see > 0.0) {
        let wd = weed_at(xz, kd.more.w);
        if (wd.z > 0.0) {
            weedy = (1.0 - smoothstep(wd.z * 0.5, wd.z * 0.9, length(wd.xy))) * weed_see;
        }
    }
    let weedc = kd.tip.rgb * mix(vec3<f32>(0.3, 0.48, 0.55) * 0.75, vec3<f32>(0.34, 0.48, 0.45) * 0.8, step(0.001, kd.look.x));
    let cover = cover_at(xz, kd.ear.z) * e * min(kd.shape.w, 1.0);
    let dist = length(in.world - view.eye.xyz);
    let dr = dist / max(kd.ear.w, 0.05);
    let farness = smoothstep(view.fog.w * 0.5, view.fog.w * 0.9, dr);
    let n1 = vnoise(xz * 2.3) * 0.6 + vnoise(xz * 9.0) * 0.4;
    let wet = view.ambient.w;
    // Muddy soil is darker and wetter even without rain; in it, puddles.
    let muddy = max(wet, kd.soil.w);
    let puddle = smoothstep(0.64, 0.7, vnoise(xz * 0.32 + vec2<f32>(4.0, 9.0)) * 0.8 + n1 * 0.2) * smoothstep(0.45, 0.95, muddy) * (1.0 - cover);
    let soil = mix(view.ground.rgb, kd.soil.rgb * (1.0 - 0.3 * kd.soil.w), e) * (0.75 + 0.5 * n1) * (1.0 - 0.55 * puddle);
    let darken = 1.0 - view.misc.z * wet * 0.9;
    // Near: lit as flat ground, darkened under the blades.
    // Under a tussocky sward the ground is matted, grazed grass with its
    // own fine grain (root to tip colour), not the deep shade under a
    // lawn; the grain fades before it is finer than the pixels.
    let grain = mix(0.5, vnoise(xz * 41.0) * 0.55 + vnoise(xz * 97.0 + vec2<f32>(7.0, 3.0)) * 0.45, 1.0 - smoothstep(0.006, 0.03, px));
    let matted = mix(kd.root.rgb, kd.tip.rgb, 0.2 + 0.65 * grain);
    let under = mix(mix(mix(kd.root.rgb, kd.tip.rgb, 0.3) * 0.75, matted, kd.more.z), weedc, weedy);
    let alb_near = mix(soil, under, cover) * darken;
    let n = vec3<f32>(0.0, 1.0, 0.0);
    let l = view.sun.xyz;
    let lc = view.sun_color.rgb;
    let v = normalize(view.eye.xyz - in.world);
    let ndl = max(dot(n, l), 0.0);
    let occl = 1.0 - view.misc.y * 0.6 * cover;
    let near = alb_near * (view.ambient.rgb + lc * ndl * tuft_k) * occl;
    // Far: the sward over the soil where the patch thins out.
    let far = sward_light(mix(soil, mix(sward_albedo(kd, xz), weedc, weedy), cover) * darken, in.world);
    var c = mix(near, far, farness);
    var open_soil = 1.0 - cover;
    if (kd.look.x > 0.001) {
        // A fine sward: its matted undergrowth near (the fibres and the
        // standing leaves fade to their even colour by themselves) and its
        // tufts and weeds as mounds far off, in its patches' colours (each
        // size of patch fading before it is finer than the pixels), each
        // tuft's lee and each rosette's leaves shading the ground; its soil
        // lies open in irregular openings, a little below the sward.
        let vy = clamp(v.y, 0.0, 1.0);
        let to = xz - view.eye.xz;
        let dh = to / max(length(to), 1e-3);
        let wh = vec2<f32>(-dh.y, dh.x);
        let cosv = sqrt(max(1.0 - vy * vy, 1e-4));
        let rise = vy / cosv;
        // What one pixel covers across the line of sight (m), at this
        // distance and on the ground.
        let pxv = dist / max(view.extra.w, 1.0);
        let fa = max(abs(dot(gx, wh)), abs(dot(gy, wh)));
        let even = fine_even(kd);
        // The mounds: from where the standing leaves are too fine to tell
        // apart, each until it is too low to tell apart; then the clumps.
        var gp = xz;
        var mound = 0.0;
        var mound_k = 1.0;
        var mound_c = vec3<f32>(1.0);
        // (Looking steeply down, from the air, the sward is its flat plan
        // with each tuft's lee on it: no mounds.)
        let m_in = smoothstep(1.0, 2.5, fa / max(kd.shape.z * 0.8, 0.001)) * (1.0 - smoothstep(0.2, 0.42, vy)) * step(0.001, kd.more.z);
        if (m_in > 0.002) {
            let mh = far_mounds(xz, dh, wh, rise, kd);
            let ch = far_clumps(xz, dh, wh, rise, kd);
            var ma = 0.0;
            var ca = 0.0;
            var mk = vec2<f32>(1.0, 0.0);
            var ck = vec2<f32>(1.0, 0.0);
            var mc = vec3<f32>(1.0);
            let weed_c = mix(vec3<f32>(1.0), weedc / max(even, vec3<f32>(1e-3)) * 1.35, 0.8);
            if (mh.s >= 0.0) {
                mk = mound_light(mh, rise, l);
                // Its leaves show as upright strokes, and its outline is
                // ragged with their tips, until they are finer than the
                // pixels.
                let ac = dot(xz - dh * mh.s, wh);
                let stroke = mix(0.5, vnoise(vec2<f32>(ac * 46.0, mk.y * 5.0 + mh.id.x * 40.0)), 1.0 - smoothstep(0.012, 0.04, pxv));
                // (And lumpy: no two outlines alike.)
                let lump = vnoise(vec2<f32>(ac * 9.0 + mh.id.x * 40.0, mh.id.x * 17.0));
                ma = smoothstep(0.0, 1.0, mh.half / (0.45 * mh.size.x) - 0.9 * stroke - 0.7 * lump * lump) * (1.0 - smoothstep(0.3, 0.8, pxv / max(mh.size.y * cosv, 1e-4)));
                // Each tuft its own green (a few gone to straw); a rosette
                // the deeper, bluer green of its leaves.
                mc = mix(vec3<f32>(1.07, 1.03, 0.9), vec3<f32>(0.9, 0.97, 1.0), mh.id.x) * (0.78 + 0.44 * stroke);
                mc = mix(mc, kd.dry.rgb / max(even, vec3<f32>(1e-3)) * 0.8, step(0.86, fract(mh.id.x * 7.3)) * 0.5);
                mc = mix(mc, weed_c * (0.7 + 0.6 * stroke), mh.id.y * 0.7);
            }
            if (ch.s >= 0.0) {
                ck = mound_light(ch, rise, l);
                let lump = vnoise((xz - dh * ch.s) * 2.6 + vec2<f32>(ch.id.x * 30.0, 4.0));
                ca = smoothstep(0.0, 1.0, ch.half / (0.5 * ch.size.x) - 0.8 * lump) * smoothstep(0.25, 0.7, pxv / (0.09 * cosv)) * (1.0 - smoothstep(0.3, 0.8, pxv / max(ch.size.y * cosv, 1e-4)));
            }
            // (The nearer of the two over the other.)
            let clump_first = ch.s > mh.s;
            let a1 = select(ma, ca, clump_first);
            let a2 = select(ca, ma, clump_first) * (1.0 - a1);
            mound = (a1 + a2) * m_in;
            if (a1 + a2 > 1e-4) {
                let wm = select(a1, a2, clump_first) / (a1 + a2);
                mound_k = mix(ck.x, mk.x, wm);
                mound_c = mix(mix(vec3<f32>(1.0), weed_c, 0.45), mc, wm);
                gp = xz - dh * mix(ch.s, mh.s, wm);
            }
        }
        let mo = mottle_px(gp, px);
        // The soil's openings: seen while they are wider than the pixels.
        // Crumbed and crusted, darker and damper in its pits, in the shade
        // of the sward's edge on the sun's side and dark all round under
        // it; dead straw and a few green leaves lie across it.
        let th = open_th(kd.ear.z);
        let on = open_n(xz);
        var open = 0.0;
        var earth = soil;
        if (kd.ear.z > 0.001) {
            open = smoothstep(th, th + 0.045, on) * (1.0 - smoothstep(0.06, 0.4, px)) * e;
        }
        if (open > 0.002) {
            let lh = l.xz / max(length(l.xz), 1e-4);
            let crumb = mix(0.5, vnoise(xz * 37.0) * 0.5 + vnoise(xz * 11.0 + vec2<f32>(3.0, 8.0)) * 0.5, 1.0 - smoothstep(0.02, 0.1, px));
            let clod = mix(0.5, vnoise(xz * 95.0 + vec2<f32>(5.0, 1.0)), 1.0 - smoothstep(0.004, 0.02, px));
            let pit = smoothstep(0.6, 0.82, vnoise(xz * 23.0 + vec2<f32>(11.0, 5.0))) * (1.0 - smoothstep(0.02, 0.1, px));
            let crust = mix(kd.soil.rgb * vec3<f32>(0.62, 0.58, 0.56), kd.soil.rgb * vec3<f32>(1.3, 1.24, 1.16), smoothstep(0.25, 0.75, crumb * 0.7 + clod * 0.3));
            let sunward = smoothstep(th, th + 0.045, open_n(xz + lh * (0.03 * length(l.xz) / max(l.y, 0.15))));
            let under = 0.7 + 0.3 * smoothstep(th + 0.01, th + 0.07, on);
            earth = crust * (1.0 - 0.5 * pit) * (0.75 + 0.5 * clod) * under * mix(0.5, 1.0, sunward);
            let wq = xz * 5.0 + vec2<f32>(3.3, 8.8);
            let lq = xz + (vec2<f32>(vnoise(wq), vnoise(wq + vec2<f32>(37.0, 11.0))) - 0.5) * 0.11;
            var lit_a = 0.0;
            var lit_r = 0.0;
            for (var i = 0; i < 3; i += 1) {
                let an = f32(i) * 1.9 + 0.5;
                let f = fibre(lq + vec2<f32>(f32(i) * 3.7, 5.3), vec2<f32>(cos(an), sin(an)), 0.0045, 0.075, 0.78, gx, gy);
                if (f.x * f.z > lit_a) {
                    lit_a = f.x * f.z;
                    lit_r = f.y;
                }
            }
            earth = mix(earth, mix(kd.dry.rgb * 0.95, kd.tip.rgb * 0.9, step(0.72, lit_r)) * mix(0.55, 1.0, sunward), lit_a * 0.8);
        }
        // The sward: near, its undergrowth, darker under each tuft and in
        // its lee; far, its mounds.
        var base = even;
        if (mound < 0.998) {
            base = fine_albedo(in.world, gx, gy, kd, vy, vec4<f32>(earth, open));
        }
        var sw = mottled(mix(base, even * mound_c, mound), kd.dry.rgb, mo, kd.look.y);
        sw *= mix(clamp(1.0 + (tuft_k - 1.0) * 1.3, 0.22, 1.9) * (1.0 - 0.4 * weedy), mound_k, mound);
        // Near, where the blades stand over it, the undergrowth lies in
        // their shade (the open soil does not); and the sward's light and
        // rank growth, as the blades read them.
        sw *= mix(mix(UNDER, 1.0, smoothstep(view.extra.z * 0.8, view.fog.w * 0.75, dr)), 1.0, open * (1.0 - mound));
        sw = far_tone(sw, sward_far(gp, dh * (pxv / max(vy, 0.03)), pxv), kd.look.x);
        open_soil = open * (1.0 - mound);
        c = sheen(sward_light(sw * darken, in.world), vy, kd.look.z);
    }
    // Wet bare soil turns to shining mud, puddles to mirrors of the sky.
    let mud = open_soil * muddy;
    let h = normalize(l + v);
    let gloss = max(view.misc.w * mud, puddle);
    let fres = 0.03 + 0.97 * pow(1.0 - max(dot(n, v), 0.0), 5.0);
    let spec = pow(max(dot(n, h), 0.0), mix(12.0, 300.0, gloss)) * mix(0.02, 1.6, gloss) * view.sun_color.w * ndl;
    c += lc * spec + (view.ambient.rgb * 0.5 + view.fog.rgb * 0.5) * fres * gloss * (0.5 + 0.5 * puddle);
    return vec4<f32>(fogged(c, in.world), 1.0);
}
