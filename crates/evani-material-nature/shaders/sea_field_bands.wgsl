// The field sea's wave textures: how the atlas is laid out (shared by the
// wave-texture draw, sea_waves.wgsl, and the field sea, sea_field.wgsl;
// src/sea/field.rs mirrors every number).
//
// FB bands of waves, each 2.83x (F_RHO) shorter than the one before,
// starting at the longest wave the sea makes (u.swell.w, metres). Band b's
// waves lie on a periodic tile of F_TILE times its longest wave (times a
// small factor of its own, F_JIT, so the bands' tiles never line up into a
// lattice), FN texels across, with a one-texel border holding the other
// side's texels so a filtered read wraps. Tile A (column 0): the sideways
// squeeze (x), the height, the squeeze (z), the folding tensor's xz. Tile
// B (column 1): the slope at the rest point (x, z), the tensor's xx and zz.
// Tile C (column 2, High and Ultra): the folding a moment ago, the height's
// curvature, the height a moment ago. A pixel sums the bands' slopes and tensors it keeps and works
// out the whole surface's slope and folding from them, so short waves
// steepen where long waves bunch them up. Row F_INFO_Y: per band (x = b)
// its slope variances (x, z), height variance and the slopes' covariance (x
// with z: with the variances, how its slope splits along and across a
// view); x = FB: the same over every band.

const FB: u32 = 8u;
const FN: u32 = 128u;
const FT: u32 = 130u;
const F_RHO: f32 = 2.8284271;
const F_TILE: f32 = 8.0;
const F_INFO_Y: u32 = 1040u;
const F_W: f32 = 390.0;
const F_H: f32 = 1041.0;

fn f_jit(b: u32) -> f32 {
    let j = array<f32, 8>(1.0, 1.09, 0.93, 1.13, 0.97, 1.06, 0.91, 1.11);
    return j[min(b, 7u)];
}

// Band b's longest wave (m).
fn fband_hi(b: u32) -> f32 {
    return u.swell.w / pow(F_RHO, f32(b));
}

// Band b's tile length (m).
fn ftile(b: u32) -> f32 {
    return F_TILE * fband_hi(b) * f_jit(b);
}

// Where a world point lies in band b's tile of kind k (0 A, 1 B, 2 C), as an
// atlas uv for a filtered read (relative to the sea's centre, so the
// numbers stay small).
fn fuv(p: vec2<f32>, b: u32, k: u32) -> vec2<f32> {
    let s = fract((p - u.centre.xz) / ftile(b)) * f32(FN);
    let px = vec2<f32>(f32(k * FT), f32(b * FT)) + 1.5 + s;
    return px / vec2<f32>(F_W, F_H);
}

// How much of the sideways squeeze a wave of k / k_p = x gets: the long waves
// round their crests (full squeeze there makes cones on the horizon and folds
// the mesh over itself), the chop keeps its sharp crest lines. src/sea/field.rs
// `chop_of`.
fn f_chop(x: f32) -> f32 {
    return mix(0.55, 1.0, smoothstep(1.5, 6.0, x));
}

// How much band b shows in the light at a point, from the next longer band's
// height there in its rms: short waves ride steep on the longer waves' crests
// and lie down in their troughs (hydrodynamic modulation), which breaks up long
// regular ridges and makes wave groups. Only in the light: in the height it
// skews the sea, and its tallest crests stand well over the spectrum's (measured
// against Ultra-Water: tools/nature/uw-probe.mjs). 1 for the longest band.
fn f_group(b: u32, hprev: f32, rprev: f32) -> f32 {
    if (b == 0u) {
        return 1.0;
    }
    return clamp(1.0 + 0.15 * hprev / max(rprev, 1e-3), 0.3, 2.0) / 1.011;
}

// The bands each level makes: Ultra and High all eight, Medium seven, Low
// six (only their light: Low's sea stays flat).
fn fbands(level: u32) -> u32 {
    let n = array<u32, 4>(6u, 7u, 8u, 8u);
    return n[min(level, 3u)];
}
