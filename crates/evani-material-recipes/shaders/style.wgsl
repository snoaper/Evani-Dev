// evani.shader.recipes.style
// The style detail step (feature `style`, dev viewers only; src/style.rs).
// A non-realistic art style draws a shape as flat colours (toon bands, cel,
// pixel cubes, ink and paper, neon bodies) made from the material's main
// colour. This step keeps the material's own pattern inside those colours:
// mat_surface evaluates the recipe (and its condition layers) as usual, turns
// its patterned colour into a factor against the material's main colour,
// simplifies it the style's way and hands the lit stage its own values back
// untouched (the style's flat colour stays flat); mat_extra then multiplies
// the finished colour by that factor. Shared by every recipe: no per-material
// code. Off (word 0) it is the plain recipe stage.
//
// The style word is rc_frame.kd.w (reserved in the plain stage):
//   bits 0-1  mode: 0 off, 1 tone (bands, cel, pixel, voxel, lit styles),
//             2 dither (the pattern's darker parts drawn in ink),
//             3 neon (the tone, and the pattern's contour lines glow);
//   bits 2-4  step: the pattern posterized in steps of n/4 stops (0 = smooth);
//   bits 5-11 strength: n/63.5 (0 to 2; 1 = the pattern as it is);
//   bits 12-13 hue: n/3, how much of the pattern's own hue is kept (0 = its
//             brightness only, for grey styles);
//   bits 14-31 cell, n/10000 m: tone and neon evaluate the pattern once per
//             cell (cells on a grid from the origin, centres at (k + 0.5) *
//             cell: a voxel style's cubes, its model placed to match),
//             dither lays its 4 x 4 ordered pattern on cells this size.

var<private> rc_styled: bool = false;
var<private> rc_sd: vec3<f32> = vec3<f32>(1.0);
var<private> rc_se: f32 = 0.0;

struct RecStyle {
    mode: u32,
    step: f32,
    strength: f32,
    hue: f32,
    cell: f32,
};

fn rc_style() -> RecStyle {
    let w = rc_frame.kd.w;
    var s: RecStyle;
    s.mode = w & 3u;
    s.step = f32((w >> 2u) & 7u) * 0.25;
    s.strength = f32((w >> 5u) & 127u) / 63.5;
    s.hue = f32((w >> 12u) & 3u) / 3.0;
    s.cell = f32(w >> 14u) * 0.0001;
    return s;
}

fn rc_style_luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The material's main colour, as a style's flat colours are made from it:
// colour A two thirds, colour B one third.
fn rc_style_tint(b: RecBlock) -> vec3<f32> {
    return b.col_a * 0.67 + b.col_b * 0.33;
}

// The pattern's brightness against the main colour, in stops.
fn rc_style_stops(a: vec3<f32>, t: vec3<f32>) -> f32 {
    return clamp(log2(max(rc_style_luma(a), 1e-4) / max(rc_style_luma(t), 1e-3)), -3.0, 1.5);
}

// Stops posterized to the style's step (unchanged at step 0).
fn rc_style_q(e: f32, step: f32) -> f32 {
    if (step <= 0.0) {
        return e;
    }
    return round(e / step) * step;
}

// A 4 x 4 ordered-dither (Bayer) threshold, 0 to 1, from bit tricks.
fn rc_style_bayer(c: vec2<i32>) -> f32 {
    let x = u32(c.x) & 3u;
    let y = u32(c.y) & 3u;
    let z = x ^ y;
    let v = ((z & 1u) << 3u) | ((y & 1u) << 2u) | (z & 2u) | ((y & 2u) >> 1u);
    return (f32(v) + 0.5) / 16.0;
}

// mat_surface's last step with the style word read: the plain recipe and
// layers when it is 0; else the pattern kept for mat_extra and the lit
// stage's own values returned. The recipe runs from exactly one call site and
// no loop: the browser's shader compilers inline it whole at every call, and
// a second call (or an unrolled loop) multiplied the pipeline build time ten
// times (32 s to 374 s on D3D12 with all 21 recipes).
fn rc_style_surface(b: RecBlock, f: MatFrame, i: MatIn, o: MatOut) -> MatOut {
    let s = rc_style();
    var si = i;
    if (s.mode != 0u && s.mode != 2u && s.cell > 0.0) {
        // One sample per cell: nudged inside along the face, then to the
        // centre (a cube's faces lie on the cell walls).
        si.world = (floor((i.world - i.g * (0.25 * s.cell)) / s.cell) + vec3<f32>(0.5)) * s.cell;
    }
    let r = mat_conditions(b, f, si, rc_dispatch(b, f, si, o));
    if (s.mode == 0u) {
        return r;
    }
    let t = rc_style_tint(b);
    let a0 = max(r.base.rgb + r.emit, vec3<f32>(0.0));
    let e = rc_style_stops(a0, t);
    let q = rc_style_q(e, s.step);
    rc_styled = true;
    if (s.mode == 2u) {
        // Engraving, against the material's first colour (its ground: bare
        // iron, not iron mixed with rust): where the pattern is clearly
        // darker (a step or more: grain lines, cracks, stripes) it is solid
        // ink, so thin lines stay lines; a little darker, or much brighter
        // (rust on iron, a pale belly), gets sparse ordered dots (4 x 4 cells
        // of the given size); paper elsewhere. The shadow side is ink already.
        let x = rc_style_q(rc_style_stops(a0, b.col_a), s.step) * s.strength;
        let k = max(s.step, 0.25);
        let dots = x < -0.5 * k || x >= 2.0 * k;
        let dark = select(select(0.0, 0.3, dots), 1.0, x <= -k);
        let p = rc_proj(i.world, i.g) / max(s.cell, 1e-4);
        let ink = rc_style_bayer(vec2<i32>(floor(p))) < dark;
        rc_sd = vec3<f32>(select(1.0, 0.015, ink));
        return mat_identity(i);
    }
    let lt = max(rc_style_luma(t), 1e-3);
    let la = max(rc_style_luma(a0), 1e-4);
    let hue = clamp((a0 / la) / max(t / lt, vec3<f32>(0.05)), vec3<f32>(0.0), vec3<f32>(3.0));
    rc_sd = exp2(q * s.strength) * mix(vec3<f32>(1.0), hue, s.hue * min(s.strength, 1.0));
    if (s.mode == 3u) {
        // Neon: the pattern's strong edges glow: one contour where its
        // brightness crosses two steps (default one stop) darker, or four
        // brighter, than the material's first colour (stripe rims, scale and crack
        // outlines, rust spots), not every small change (fur hairs, a metal's
        // fine bright speckle). No screen
        // derivatives after the discard and no second sample (see above), so
        // the line is thin where the pattern changes fast.
        // Measured against the first colour, as the dither: against the
        // A-B mix, bare iron sat right on the dark contour and glittered.
        let ea = rc_style_stops(a0, b.col_a);
        let k = 2.0 * max(s.step, 0.25);
        let d = select(abs(ea + k), abs(ea - 2.0 * k), ea > 0.0);
        rc_se = (1.0 - smoothstep(0.05, 0.14, d)) * 0.8 * min(s.strength, 1.5);
    }
    return mat_identity(i);
}

// mat_extra's last step: the styled colour (the lit stage's flat colour
// times the pattern, plus neon's edge glow in its own hue), or c as it is.
fn rc_style_post(c0: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
    if (!rc_styled) {
        return c;
    }
    let top = max(c0.r, max(c0.g, c0.b));
    return c0 * rc_sd + (c0 / max(top, 1e-3)) * rc_se;
}
