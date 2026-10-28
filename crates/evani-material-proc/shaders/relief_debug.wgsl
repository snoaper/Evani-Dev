    // Debug views (feature `relief-debug`, proof builds only): the layer
    // masks as colour. 1: puddle mask (red), water level (green), upward
    // face (blue). 2: cavity (red), moss mask (green), moss noise (blue).
    // 3: roughness.
    let dbg = u32(kit_data[MP_RELIEF + 3u].y);
    if (dbg == 1u) {
        return vec3<f32>(pm, clamp(wl, 0.0, 1.0) * 0.6, upm * 0.3);
    }
    if (dbg == 2u) {
        return vec3<f32>(cav, mm, mn * 0.5);
    }
    if (dbg == 3u) {
        return vec3<f32>(rough);
    }
