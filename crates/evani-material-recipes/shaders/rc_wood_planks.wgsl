// Wood: the planks stand-in.
// The stand-in for wood's layouts (shaders/rc_wood_layouts.wgsl) when the
// part `recipe-wood-layouts` is off: the same three functions, drawing every
// layout as planks (variant 3 exactly as with the part) and cork as flat
// colour A. Written fresh for Evani.Dev.

fn rc_wood_u(b: RecBlock, q: vec2<f32>, pw: f32) -> vec4<f32> {
    return rc_wood_plank(b, q, pw, 7.0);
}

fn rc_wood_form(b: RecBlock, u: vec4<f32>, pw: f32) -> vec2<f32> {
    return vec2<f32>(1.0, smoothstep(0.0, pw * 0.06, u.z));
}

// Marquetry draws as flat-sawn colour A.
fn rc_wood_mq(b: RecBlock, uv: vec2<f32>, pw: f32) -> vec4<f32> {
    return vec4<f32>(uv - vec2<f32>(0.5), 0.0, 0.0);
}

fn rc_wood_cork(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    return rc_out(i, b.col_a, b.na.x, 0.0);
}
