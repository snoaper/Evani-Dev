// Organic: the plain stand-in.
// The stand-in for organic's food part (shaders/rc_organic_food.wgsl) when
// the part `recipe-organic-food` is off: the same two functions, drawing
// flesh without its grain (colour, mottle and fat flecks), kernels as a
// plain mottled colour A and pastry as a plain crust in colour A. Written fresh for Evani.Dev.

// No meat seams without the food part: rc_organic's own flecks draw the fat.
fn rc_organic_mx(b: RecBlock) -> bool {
    return false;
}

fn rc_organic_fh(b: RecBlock, uv: vec2<f32>, cs: f32) -> f32 {
    return 0.0;
}

fn rc_organic_f(b: RecBlock, uv: vec2<f32>, c: ptr<function, vec3<f32>>, r: ptr<function, f32>, cs: f32, ka: f32) {}
