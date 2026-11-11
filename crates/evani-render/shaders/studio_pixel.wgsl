// evani.shader.studio_pixel
// The studio's pixel pass (feature `studio-passes`, pass `studio.apply`,
// mode `pixel`): the tonemapped image redrawn on a coarse grid of art
// pixels, `px` screen pixels each, with a small palette and an outline.
//
// * Grid: every screen pixel finds its art cell (shifted by `offset`, the
//   camera's remainder below one art pixel, so the grid moves with the
//   scene instead of the scene crawling through it). One cell = one colour.
// * Colour: four samples in the cell; the frontmost one (smallest view
//   distance) wins, and the samples within a few percent of its distance
//   are averaged. A thin leg covering only part of a cell still owns it, so
//   thin parts never break up, and an edge cell never blends with what is
//   behind it (no halo).
// * Palette: luma and two chroma axes (YCoCg) cut by `levels`, with a
//   small ordered dither per art pixel; hue ramps stay hue ramps. 0 = off.
// * Outline: a cell nearer than a neighbouring cell by more than a few
//   percent is on the near side of a depth edge: the silhouette against the
//   ground or sky gets the ink colour, an inner edge (a near leg over a far
//   one, the neck over the body) a darker shade of its own colour, the way
//   pixel artists line inner shapes.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

struct Pixel {
    // Target size in pixels; `1 / distance = inv.x + inv.y * depth`.
    size: vec2<f32>,
    inv: vec2<f32>,
    // Screen pixels per art pixel, palette steps (0 = none), outline (0..1),
    // ordered-dither amount (0..1).
    px: f32,
    levels: f32,
    outline: f32,
    dither: f32,
    // The camera's remainder below one art pixel, in screen pixels.
    offset: vec2<f32>,
    pad0: vec2<f32>,
    // Ink colour of the silhouette line.
    ink: vec3<f32>,
    pad1: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Pixel;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

const SKY: f32 = 1e6;

fn texel(p: vec2<f32>) -> vec2<i32> {
    return clamp(vec2<i32>(floor(p)), vec2<i32>(0), vec2<i32>(u.size) - vec2<i32>(1));
}

// View distance at a screen point (SKY where nothing was drawn).
fn dist(p: vec2<f32>) -> f32 {
    let d = textureLoad(depth_tex, texel(p), 0);
    if (d <= 0.0) {
        return SKY;
    }
    return 1.0 / max(u.inv.x + u.inv.y * d, 1e-9);
}

fn origin(cell: vec2<f32>) -> vec2<f32> {
    return cell * u.px - u.offset;
}

// The frontmost distance of a cell (its four sample spots).
fn cell_dist(cell: vec2<f32>) -> f32 {
    let o = origin(cell);
    let a = u.px * 0.25;
    let b = u.px * 0.75;
    return min(min(dist(o + vec2<f32>(a, a)), dist(o + vec2<f32>(b, a))), min(dist(o + vec2<f32>(a, b)), dist(o + vec2<f32>(b, b))));
}

// A cell's colour: the mean of the samples at its frontmost distance.
fn cell_colour(cell: vec2<f32>, front: f32) -> vec3<f32> {
    let o = origin(cell);
    var sum = vec3<f32>(0.0);
    var n = 0.0;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let s = o + vec2<f32>(0.25 + 0.5 * f32(k & 1u), 0.25 + 0.5 * f32(k >> 1u)) * u.px;
        if (dist(s) <= front * 1.04 + 0.01) {
            sum = sum + textureLoad(src, texel(s), 0).rgb;
            n = n + 1.0;
        }
    }
    return sum / max(n, 1.0);
}

// 4x4 Bayer threshold in -0.5..0.5 for an art pixel.
fn bayer(c: vec2<i32>) -> f32 {
    let x = u32(c.x & 3);
    let y = u32(c.y & 3);
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[y * 4u + x] + 0.5) / 16.0 - 0.5;
}

fn quantise(c: vec3<f32>, cell: vec2<f32>) -> vec3<f32> {
    if (u.levels < 2.0) {
        return c;
    }
    let t = bayer(vec2<i32>(cell)) * u.dither;
    // YCoCg: `2 levels - 1` luma steps over 0..1; the chroma axes (which
    // real colours keep within about +-0.3) `2 levels` steps over -0.5..0.5.
    let y = dot(c, vec3<f32>(0.25, 0.5, 0.25));
    let co = 0.5 * (c.r - c.b);
    let cg = -0.25 * c.r + 0.5 * c.g - 0.25 * c.b;
    let ly = 2.0 * u.levels - 2.0;
    let lc = 2.0 * u.levels;
    let qy = clamp(floor(y * ly + 0.5 + t), 0.0, ly) / ly;
    let qo = floor((co + 0.5) * lc + 0.5 + t * 0.5) / lc - 0.5;
    let qg = floor((cg + 0.5) * lc + 0.5 + t * 0.5) / lc - 0.5;
    return clamp(vec3<f32>(qy + qo - qg, qy + qg, qy - qo - qg), vec3<f32>(0.0), vec3<f32>(1.0));
}

// How far behind `front` a neighbour must be to make an edge.
fn gap(front: f32) -> f32 {
    return front * 0.035 + 0.02;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = max(u.px, 1.0);
    let cell = floor((in.pos.xy + u.offset) / px);
    let front = cell_dist(cell);
    var c = cell_colour(cell, front);
    c = quantise(c, cell);
    if (u.outline > 0.0 && front < SKY) {
        var behind = 0.0;
        var sky = false;
        let n = array<vec2<f32>, 4>(vec2<f32>(1.0, 0.0), vec2<f32>(-1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(0.0, -1.0));
        for (var k = 0u; k < 4u; k = k + 1u) {
            let d = cell_dist(cell + n[k]);
            behind = max(behind, d - front);
            sky = sky || d > front * 1.6;
        }
        if (behind > gap(front)) {
            let line = select(c * 0.55, mix(c * 0.3, u.ink, 0.6), sky);
            c = mix(c, line, u.outline);
        }
    }
    return vec4<f32>(c, 1.0);
}
