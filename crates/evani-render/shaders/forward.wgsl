// evani.shader.forward
// Forward pass (`forward`): instanced meshes lit by one directional light
// plus an ambient term, written as linear HDR into `hdr` with depth.
// Per-frame values (camera, light, materials) come from one uniform the
// renderer fills from the render.* params and the look's materials; nothing
// tunable is baked into this source. Slots: `toon_ramp` shapes the diffuse
// term, `fog` gives the fog amount, `rim` adds a rim light (none by
// default).
//
// Grounding: up to 32 blob shadows (soft discs on upward faces near the
// ground under each object, the Low path) are always here. The shadow map
// (`shadow_vis`) comes from a second file appended at build time: the real
// sampler with the `shadows` feature, else a stub that returns 1.0, so a
// game without shadows ships none of it. Shadowed light uses the darkest
// band of the toon ramp: N.L is pushed below zero by the shadow amount.
//
// Shadow tint (`render.shadow_tint`, `render.shadow_tint_strength`): what is
// shadowed (by the map or a blob) leans towards the tint instead of going
// grey: in a gamma-like (square root) space its colour keeps its brightness
// but takes the tint's hue (scaled down if that would leave the range), and
// the strength mixes that in, so a green meadow in shadow turns lavender.
// Strength 0 (the default) mixes in exactly nothing, so a look that never
// sets it renders as before.

struct Material {
    // Linear RGBA base colour.
    base: vec4<f32>,
    // Emissive RGB; w > 0.5 = unlit (base + emissive, no lighting).
    emissive_unlit: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    // Camera position (xyz).
    eye: vec4<f32>,
    // Direction towards the light (xyz, normalised).
    light_dir: vec4<f32>,
    // render.light.color * render.light.intensity (rgb).
    light_color: vec4<f32>,
    // render.ambient (rgb).
    ambient: vec4<f32>,
    // Colour fog fades to (render.clear_color).
    fog_color: vec4<f32>,
    materials: array<Material, 16>,
    // Light view-projection of the shadow map.
    light_vp: mat4x4<f32>,
    // Shadow map: on (x > 0.5), strength, texel size (uv), PCF radius
    // (texels).
    shadow: vec4<f32>,
    // Blob count, blob strength, shadow-map normal offset (m), depth bias.
    blob_info: vec4<f32>,
    // Blob shadows: centre on the ground (xyz) and radius (w).
    blobs: array<vec4<f32>, 32>,
    // Shadow tint: colour (rgb, linear) and strength (w, 0 = off). Always the
    // last member.
    shadow_tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Instance: position (xyz) and material index (w).
    @location(2) inst_pos: vec4<f32>,
    // Instance: rotation quaternion (x, y, z, w).
    @location(3) inst_rot: vec4<f32>,
    // Instance: scale (xyz).
    @location(4) inst_scale: vec4<f32>,
    // Instance: linear RGBA tint.
    @location(5) inst_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) material: u32,
};

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let local = in.pos * in.inst_scale.xyz;
    let world = rotate(in.inst_rot, local) + in.inst_pos.xyz;
    // Non-uniform scale: transform the normal by the inverse scale.
    let n = rotate(in.inst_rot, in.normal / max(abs(in.inst_scale.xyz), vec3<f32>(1e-4)) * sign(in.inst_scale.xyz));
    var o: VsOut;
    o.pos = frame.view_proj * vec4<f32>(world, 1.0);
    // Decals (road lines, painted marks) carry a small depth offset in the
    // instance pad, already signed toward the camera (evani-render
    // depth::toward_camera), so they never z-fight the surface they lie on.
    // 0 for every other instance.
    o.pos.z += in.inst_scale.w * o.pos.w;
    o.world = world;
    o.normal = n;
    o.color = in.inst_color;
    o.material = min(u32(max(in.inst_pos.w, 0.0)), 15u);
    return o;
}

//@slot toon_ramp
//@slot fog
//@slot rim

// Light reaching `world` past the blob shadows (1 = none). Discs darken
// upward faces from the ground up to a few centimetres above it, softly
// towards the rim.
fn blob_vis(world: vec3<f32>, n: vec3<f32>) -> f32 {
    // Only upward faces are darkened: walls, undersides and sides skip the
    // loop (and so does everything when there are no blobs).
    let up = clamp(n.y, 0.0, 1.0);
    let count = min(u32(max(frame.blob_info.x, 0.0)), 32u);
    if (up <= 0.0 || count == 0u) {
        return 1.0;
    }
    var occ = 0.0;
    for (var i = 0u; i < count; i = i + 1u) {
        let b = frame.blobs[i];
        let h = world.y - b.y;
        // Outside the height band or the disc: no effect, skip the maths.
        if (h < -0.25 || h > 0.3) {
            continue;
        }
        let r = max(b.w, 1e-3);
        let dxz = world.xz - b.xz;
        let d2 = dot(dxz, dxz);
        if (d2 >= r * r) {
            continue;
        }
        let d = sqrt(d2) / r;
        let o = (1.0 - smoothstep(0.45, 1.0, d)) * (1.0 - smoothstep(0.04, 0.3, h));
        occ = max(occ, o);
    }
    return 1.0 - occ * up * frame.blob_info.y;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Screen-door fade: an instance colour alpha below 1 drops pixels on a
    // 2x2 ordered pattern (0.75 keeps 3 of 4, 0.5 half, 0.25 one, under
    // 0.125 none). Opaque instances (alpha 1, everything by default) skip it.
    // The period of 2 px is invisible to an outline sampling an even pixel
    // offset (the toon look's 1.6 px rounds to 2), so the kept pixels ink
    // only the shape's real edges. The kept pixels are marked in alpha, and
    // the tonemap pass averages the pattern away round them (any 2x2 pattern
    // becomes an even blend of the shape and what lies behind it).
    if (in.color.a < 0.999) {
        let q = vec2<u32>(in.pos.xy) & vec2<u32>(1u);
        let th = select(select(0.125, 0.625, q.x == 1u), select(0.875, 0.375, q.x == 1u), q.y == 1u);
        if (in.color.a < th) {
            discard;
        }
    }
    let m = frame.materials[in.material];
    let base = in.color.rgb * m.base.rgb;
    let dist = length(in.world - frame.eye.xyz);
    var c: vec3<f32>;
    if (m.emissive_unlit.w > 0.5) {
        c = base + m.emissive_unlit.rgb;
    } else {
        let n = normalize(in.normal);
        let ndl = dot(n, frame.light_dir.xyz);
        let vis = shadow_vis(in.world, n) * blob_vis(in.world, n);
        let diffuse = slot_toon_ramp(mix(min(ndl, -0.1), ndl, vis));
        let ndv = dot(n, (frame.eye.xyz - in.world) / max(dist, 1e-4));
        c = base * (frame.ambient.rgb + frame.light_color.rgb * diffuse) + m.emissive_unlit.rgb
            + slot_rim(ndv, ndl, dist, base);
        // Tint the shadowed part (see the header).
        let k = clamp(1.0 - vis, 0.0, 1.0) * clamp(frame.shadow_tint.w, 0.0, 1.0);
        let w = vec3<f32>(0.2126, 0.7152, 0.0722);
        let ts = sqrt(max(frame.shadow_tint.rgb, vec3<f32>(0.0)));
        let hs = ts / max(dot(ts, w), 1e-3);
        let t = hs * dot(sqrt(max(c, vec3<f32>(0.0))), w);
        let tc = t / max(1.0, max(t.r, max(t.g, t.b)));
        c = mix(c, tc * tc, k);
    }
    let f = clamp(slot_fog(dist, in.world.y), 0.0, 1.0);
    // Alpha marks the pixels a screen-door fade kept (0; everything else 1):
    // the tonemap pass blends the 2x2 pattern round them into a smooth fade.
    return vec4<f32>(mix(c, frame.fog_color.rgb, f), select(1.0, 0.0, in.color.a < 0.999));
}
