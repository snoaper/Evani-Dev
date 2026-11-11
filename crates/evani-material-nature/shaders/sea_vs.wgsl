// Body of the spectral sea shader (appended after `sea.wgsl`, which
// declares the bindings and `fs_main`, and before the generated component
// table `sea_table.wgsl`).
//
// The sea is a height field h(x, z, t): a sum of wave components drawn from
// a wind sea and a swell (JONSWAP with directional spreading, in normalised
// units: src/sea/spectrum.rs), each with a sharp-crested profile
// exp(s (sin - 1)) that flattens troughs and sharpens crests as choppiness
// rises, plus two shore breaker trains on a straight beach. Each pixel
// marches its view ray through the slab the waves can reach, brackets the
// first crossing and bisects onto it, so every water pixel lies on the wave
// surface: there is no flat fallback plane anywhere, and the waves shrink
// over shallow water so a trough never reaches the bed (src/sea/mod.rs
// `no_bottoming_out` tests it).
//
// Shading ideas from Ultra-Water (Humpalumps, MIT), Tessendorf's notes and
// the Atlas GDC 2019 talk: slope variance roughness for the components too
// fine for the pixel (LEAN style) plus the Cox-Munk roughness the sum lacks,
// a GGX sun with Smith visibility and a roughness-aware Fresnel, Poisson
// sun glitter far out, a height-weighted back-lit crest glow, per-channel
// absorption with in-scattering, foam where the surface folds (a Jacobian
// that also remembers where crests broke a moment ago), eroded into lace.
// Our own code, ideas only.

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

