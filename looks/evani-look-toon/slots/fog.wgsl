    // evani-look-toon: light distance haze, thinner higher up; the far ground fades fully into the sky
    let d = max(distance - 60.0, 0.0);
    let haze = 1.0 - exp(-d * 0.0025);
    let thin = clamp(1.0 - max(height, 0.0) / 120.0, 0.35, 1.0);
    let far = smoothstep(420.0, 900.0, distance) * (1.0 - smoothstep(60.0, 160.0, height));
    return max(min(haze * thin, 0.45), far);
