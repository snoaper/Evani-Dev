// evani.shader.trails_screen
// Ribbons and trails in screen space (CSS pixels, y down), drawn between
// the sprite layers. Joined after `trails_draw.wgsl` in one module; built
// into 2D games, and into 3D ones only with evani-render's `trails-screen`.

@vertex
fn vs_screen(in: VsIn) -> VsOut {
    if (in.a_key != u.key) {
        return culled();
    }
    let g = shape(in);
    var side = g.edge.xy;
    if (dot(side, side) == 0.0) {
        let d = g.dir.xy;
        let l = max(length(d), 1e-6);
        side = vec2<f32>(-d.y, d.x) / l * g.w * 0.5;
    }
    let k = thin(length(side) * 2.0);
    let p = g.pos.xy + side * g.side * k;
    let vp = max(u.viewport, vec2<f32>(1.0));
    var o: VsOut;
    o.pos = vec4<f32>(p.x / vp.x * 2.0 - 1.0, 1.0 - p.y / vp.y * 2.0, 0.0, 1.0);
    o.v = g.side;
    o.color = g.color / sqrt(k);
    o.soft = u.soft / k;
    return o;
}
