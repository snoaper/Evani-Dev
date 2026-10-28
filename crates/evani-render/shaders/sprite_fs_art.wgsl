// Sprite art (feature `sprite-art`): the atlas has mips. A pixel-art frame
// has its uv moved by PIXEL_U (2.0) on u; it reads level 0 on the texel
// centre (crisp, never a mip), every other frame filters normally.
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let pixel = in.uv.x >= 2.0;
    let uv = select(in.uv, in.uv - vec2<f32>(2.0, 0.0), pixel);
    let soft = textureSample(atlas, samp, uv);
    let size = vec2<f32>(textureDimensions(atlas, 0));
    let crisp = textureSampleLevel(atlas, samp, (floor(uv * size) + 0.5) / size, 0.0);
    return select(soft, crisp, pixel) * in.color;
}
