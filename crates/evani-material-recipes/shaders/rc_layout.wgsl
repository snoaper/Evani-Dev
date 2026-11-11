// evani.shader.recipes.layout
// The pattern layouts recipes share (batch 3 build plan 3.1, package B3-P1):
// bonds, herringbone, basket weave and chequer, hex, shingle courses, panels,
// studs (nail heads, rivets), thin lines and stripes, and painted motifs.
// Appended once after rc_shared when a compiled recipe calls an rc_lay_
// function (feature `layout`, which those recipes' features turn on), so a
// game with no patterned recipe downloads none of it. Written fresh for
// Evani.Dev; src/layout.rs mirrors the cell maths on the CPU for its tests
// (every cell function covers the plane with no gap or overlap, keeps a
// unit's id across the unit, and its edge distance is continuous across
// joints).
//
// Lengths are in the caller's units: metres for world-space recipes
// (masonry, stone), uv for uv ones (wood, metal, paint, ceramic). The caller
// fades what it draws from a cell to its average under 1.5 px (rc_aa,
// rc_aa_world, rc_fade), as every recipe does.

// One unit of a layout, at the point asked about:
//   e: the distance to the unit's nearest edge (joint, grout, seam, lead);
//   id: the unit's own number in [0, 1) (its tone, its pattern offset);
//   uv: where the point is inside the unit, 0..1 along its length (x) and
//       across it (y); for shingles y is the lap: 0 at the unit's free
//       edge, 1 where the course above covers it;
//   k: which way the unit lies (0 along x, 1 along y), or the chequer's
//      colour (0 or 1).
struct LayCell {
    e: f32,
    id: f32,
    uv: vec2<f32>,
    k: f32,
};

// x mod 2 for whole numbers (0 or 1, also below zero).
fn rc_lay_par(x: f32) -> f32 {
    return x - 2.0 * floor(x * 0.5);
}

// p turned by 45 degrees (diamonds, herringbone rows along an axis).
fn rc_lay_rot45(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(p.x + p.y, p.y - p.x) * 0.70710678;
}

// Courses of units u.x long and u.y high, rows along x. Mode m: 0 running
// bond (every other row half a unit on), 3 a random offset per row (setts),
// 4 random lengths and offsets (ashlar), anything else stack bond. Masonry's
// bonds (batch 1, P5), moved here unchanged: the row hash takes seed, the
// unit's id seed + 1.
fn rc_lay_bond(p: vec2<f32>, u: vec2<f32>, m: u32, seed: u32) -> LayCell {
    let row = floor(p.y / u.y);
    let rh = rc_u01(rc_hash2(vec2<i32>(i32(row), 0), seed));
    var len = u.x;
    var off = 0.0;
    if (m == 0u) {
        off = 0.5 * u.x * step(0.5, fract(row * 0.5));
    } else if (m == 3u) {
        off = rh * u.x;
    } else if (m == 4u) {
        len = u.x * (0.7 + 0.6 * rh);
        off = rh * 5.0 * u.x;
    }
    let x = p.x + off;
    let seg = floor(x / len);
    let lx = x - seg * len;
    let ly = p.y - row * u.y;
    return LayCell(min(min(ly, u.y - ly), min(lx, len - lx)), rc_u01(rc_hash2(vec2<i32>(i32(seg), i32(row)), seed + 1u)), vec2<f32>(lx / len, ly / u.y), 0.0);
}

// One strip m of the herringbone (q in unit widths, units n wide): a
// staircase of units along x (k 0) and along y (k 1), one step per (1, 1).
// e is below 0 when q is in neither.
fn rc_lay_hb(q: vec2<f32>, n: f32, m: f32, seed: u32) -> LayCell {
    let r = q - m * vec2<f32>(n, -n);
    let ky = floor(r.y);
    let kx = floor(r.x);
    let h = r.x >= ky && r.x < ky + n;
    let k = select(kx, ky, h);
    let a = select(vec2<f32>(r.y - k - 1.0, r.x - k), r - vec2<f32>(k), h);
    let ok = h || (r.y >= kx + 1.0 && r.y < kx + n + 1.0);
    let id = rc_u01(rc_hash3(vec3<i32>(i32(k), i32(m), select(0, 1, h)), seed));
    return LayCell(select(-1.0, min(min(a.x, n - a.x), min(a.y, 1.0 - a.y)), ok), id, vec2<f32>(a.x / n, a.y), select(1.0, 0.0, h));
}

// Herringbone: units w wide and n * w long (n a whole number, 2 or more) at
// right angles, in zigzag rows along the (1, 1) diagonal (turn p with
// rc_lay_rot45 for rows along x). Two strips can hold a point; one does.
fn rc_lay_herring(p: vec2<f32>, w: f32, n: f32, seed: u32) -> LayCell {
    let q = p / w;
    let m = floor((q.x - q.y + n + 1.0) / (2.0 * n));
    var c = rc_lay_hb(q, n, m, seed);
    if (c.e < 0.0) {
        c = rc_lay_hb(q, n, m - 1.0, seed);
    }
    c.e = max(c.e, 0.0) * w;
    return c;
}

// Basket weave: square blocks of n units w wide, the units of each block
// along x or along y in turn (a chequer of directions). With n = 1 it is the
// chequer: square tiles w wide, k their colour (0 or 1).
fn rc_lay_basket(p: vec2<f32>, w: f32, n: f32, seed: u32) -> LayCell {
    let s = w * n;
    let bc = floor(p / s);
    let k = rc_lay_par(bc.x + bc.y);
    let l = p - bc * s;
    let a = select(l, l.yx, k > 0.5);
    let row = floor(a.y / w);
    let ly = a.y - row * w;
    return LayCell(min(min(a.x, s - a.x), min(ly, w - ly)), rc_u01(rc_hash3(vec3<i32>(vec2<i32>(bc), i32(row)), seed)), vec2<f32>(a.x / s, ly / w), k);
}

// Hexagons s apart (the core's rc_hex, pointy-top): e to the nearest side.
fn rc_lay_hex(p: vec2<f32>, s: f32, seed: u32) -> LayCell {
    let h = rc_hex(p / s);
    let a = abs(h.xy);
    return LayCell((0.5 - max(a.x, dot(a, vec2<f32>(0.5, 0.8660254)))) * s, rc_u01(rc_hash2(vec2<i32>(floor(h.zw * 2.0)), seed)), h.xy + vec2<f32>(0.5), 0.0);
}

// Course r of a shingle layout at x: its free (bottom) edge, raised up to
// j of a course by the unit's own jitter (x), the position along the unit
// (y), and the unit's id (z).
fn rc_lay_sb(r: f32, x: f32, u: vec2<f32>, j: f32, seed: u32) -> vec3<f32> {
    let xo = x + 0.5 * u.x * rc_lay_par(r);
    let s = floor(xo / u.x);
    let h = rc_hash2(vec2<i32>(i32(s), i32(r)), seed);
    return vec3<f32>((r - j * rc_u01(h)) * u.y, xo - s * u.x, rc_u01(rc_pcg(h)));
}

// Shingle courses (shingles, shakes, slates, roof tiles, clapboard): units
// u.x wide in courses u.y high, each course half a unit on and lying over
// the one below, its free edge shortened up to j (0..1) of a course per
// unit. e is to the unit's sides and its free edge; uv.y is the lap (0 at
// the free edge, 1 under the next course's edge, where its shadow falls).
fn rc_lay_shingle(p: vec2<f32>, u: vec2<f32>, j: f32, seed: u32) -> LayCell {
    var r = floor(p.y / u.y) + 1.0;
    var a = rc_lay_sb(r, p.x, u, j, seed);
    if (a.x > p.y) {
        r = r - 1.0;
        a = rc_lay_sb(r, p.x, u, j, seed);
    }
    let t = rc_lay_sb(r + 1.0, p.x, u, j, seed).x;
    return LayCell(min(min(a.y, u.x - a.y), p.y - a.x), a.z, vec2<f32>(a.y / u.x, (p.y - a.x) / max(t - a.x, 1e-6)), 0.0);
}

// A panel's height at q inside a unit of size u: 0 on the frame (fw wide),
// rising over a bevel bw to 1 on the field (raised; 1 minus it is sunk).
fn rc_lay_panel(q: vec2<f32>, u: vec2<f32>, fw: f32, bw: f32) -> f32 {
    let d = min(q, u - q);
    return smoothstep(fw, fw + bw, min(d.x, d.y));
}

// Studs (nail heads, rivets) on a lattice of pitch t, at (i + 0.5) * t: a
// dome of radius r, 1 at a stud's centre, 0 off it.
fn rc_lay_studs(q: vec2<f32>, t: vec2<f32>, r: f32) -> f32 {
    let d = q - (floor(q / t) + vec2<f32>(0.5)) * t;
    return max(0.0, 1.0 - dot(d, d) / (r * r));
}

// A row of studs round the edge of a unit of size u (a riveted plate): a
// stud every t along each side, i in from it, radius r.
fn rc_lay_rim(q: vec2<f32>, u: vec2<f32>, i: f32, t: f32, r: f32) -> f32 {
    let d = min(q, u - q);
    let a = select(vec2<f32>(d.y, q.x), vec2<f32>(d.x, q.y), d.x < d.y);
    let s = vec2<f32>(a.x - i, a.y - (floor(a.y / t) + 0.5) * t);
    return max(0.0, 1.0 - dot(s, s) / (r * r));
}

// Riveted plates (metal, painted steel): plates 2/s by 1/s in a running
// bond, a rivet every sixth of a plate's height round each, half a pitch in
// (so the two rows meeting at a corner share its rivet). x the rivet dome,
// y the distance to the plate's seam, z the plate's id.
fn rc_lay_rivets(q: vec2<f32>, s: f32, seed: u32) -> vec3<f32> {
    let u = vec2<f32>(2.0, 1.0) / s;
    let c = rc_lay_bond(q, u, 0u, seed);
    return vec3<f32>(rc_lay_rim(c.uv * u, u, u.y / 12.0, u.y / 6.0, u.y * 0.03), c.e, c.id);
}

// The distance from x to the centre line of the nearest stripe of a set t
// apart (seams, battens, lines of a grid: rc_lay_grid).
fn rc_lay_stripe(x: f32, t: f32) -> f32 {
    return abs(fract(x / t + 0.5) - 0.5) * t;
}

// Thin crossing lines t apart (leaded panes, wire, mesh; diamonds through
// rc_lay_rot45): the distance to the nearest line.
fn rc_lay_grid(p: vec2<f32>, t: vec2<f32>) -> f32 {
    return min(rc_lay_stripe(p.x, t.x), rc_lay_stripe(p.y, t.y));
}

// A painted motif in a square tile, q from -1 to 1 across it: a rosette
// (round petals) or a star (thin points) with 4 or 8 arms, picked and sized
// by the material's seed, so every tile of a material carries the same one,
// painted with a wobble w that differs per tile (id, the tile's own number).
// Signed distances, below 0 inside: x the petals, y the centre disc, z the
// quarter circles in the corners (with the next tiles' they make circles).
fn rc_lay_motif(q: vec2<f32>, seed: u32, w: f32, id: f32) -> vec3<f32> {
    let h = rc_pcg(seed);
    let n = f32(4u + 4u * (h & 1u));
    let wob = (rc_vnoise2(q * 3.0 + vec2<f32>(id * 97.0), seed + 9u) - 0.5) * w;
    let r = length(q) + wob;
    let arm = pow(abs(cos(atan2(q.y, q.x) * n * 0.5)), select(1.0, 6.0, (h & 2u) > 0u));
    return vec3<f32>(r - (0.5 + 0.3 * rc_u01(h)) * (0.45 + 0.55 * arm), r - 0.16, length(abs(q) - vec2<f32>(1.0)) - 0.42 - wob);
}
