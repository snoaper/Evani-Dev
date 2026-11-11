// evani.shader.rc.glow
// Glow (recipe 29, batch 1 build plan section 3, package P9). Written fresh
// for Evani.Dev: emission from colour A, with a pulse and a hologram variant.
// The surface is dark and lit as usual; the light is `o.emit`, times the
// texture and vertex colour. All motion reads rc_time() (sim time from the
// fixed step), never a clock.
//   numbers: na = intensity (0-8), pulse speed (0-10), pulse amount,
//            scanline density (0-400 per m for the hologram, per uv for a
//            screen); nb = scanline speed (0-10), flicker, rim
//   colours: A colour, B rim and scanline
//   variants: 0 lamp (brighter face-on), 1 neon (white-hot core), 2 screen
//   (pixel rows), 3 rune (glowing cell lines), 4 hologram (rim, scanlines
//   crawling up, flicker, dither gaps, see-through alpha), 5 energy (ridged
//   swirls)
//   extras: pulse (bit 0), hologram (bit 1: scanlines on screens, and the rim,
//   flicker and dither gaps of the hologram variant)
// Levels (q, the detail group; glow has none of its own): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the plain
// emissive); low adds the pulse, the patterns and the scanlines; medium adds
// the hologram rim and flicker; high adds the dither gaps and a second,
// finer scanline layer; ultra draws as high.

fn rc_glow(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let it = rc_range(b.na.x, 0.0, 8.0);
    let tint = i.tex.rgb * i.vcol;
    var o = rc_out(i, b.col_a * 0.12, 0.5, 0.0);
    var e = b.col_a * it;
    let t = rc_time();
    let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
    var m = 1.0;
    if (b.variant == 0u) {
        m = 0.55 + 0.45 * ndv;
    } else if (b.variant == 1u) {
        e = mix(e, vec3<f32>(it), smoothstep(0.7, 1.0, ndv) * 0.5);
    } else if (b.variant == 3u) {
        let c = rc_voronoi2(i.uv * 8.0, b.seed, 1.0);
        m = 0.08 + 1.4 * (1.0 - smoothstep(0.0, 0.1, c.z));
    } else if (b.variant == 5u) {
        let r = 1.0 - abs(2.0 * rc_fbm2(i.uv * 4.0 + vec2<f32>(t * 0.3, -t * 0.2), b.seed, 3u) - 1.0);
        m = 0.25 + 1.3 * r * r * r;
    }
    if (rc_extra(b, RC_X_GLOW, 0u)) {
        m = m * (1.0 - b.na.z * (0.5 - 0.5 * sin(t * rc_range(b.na.y, 0.0, 10.0) * 6.2832)));
    }
    let dens = rc_range(b.na.w, 0.0, 400.0);
    let sp = rc_range(b.nb.x, 0.0, 10.0);
    if (rc_extra(b, RC_X_GLOW, 1u) && dens > 0.0) {
        if (b.variant == 2u) {
            m = m * (0.72 + 0.28 * step(0.5, fract(i.uv.y * dens)));
        } else if (b.variant == 4u) {
            let k = rc_aa_world(f, 1.0 / dens);
            m = m * mix(0.8, 0.5 + 0.5 * step(0.5, fract(i.world.y * dens - t * sp)), k);
            if (q >= 3u) {
                m = m * (0.85 + 0.15 * step(0.5, fract(i.world.y * dens * 0.37 + t * sp * 0.5)));
            }
        }
    }
    var a = i.base.a;
    if (b.variant == 4u && rc_extra(b, RC_X_GLOW, 1u)) {
        let rim = pow(1.0 - ndv, 2.0);
        e = mix(e, b.col_b * it, rim);
        m = m * (0.4 + rim * 1.6);
        if (q >= 2u) {
            m = m * (1.0 - b.nb.y * step(0.7, rc_u01(rc_pcg(u32(t * 24.0) + b.seed))));
        }
        if (q >= 3u) {
            m = m * (1.0 - 0.85 * step(rc_u01(rc_hash3(vec3<i32>(floor(i.world * 80.0)), u32(t * 12.0) + b.seed)), 0.1));
        }
        a = a * clamp(0.2 + m * 0.6, 0.0, 1.0);
    }
    o.base.a = a;
    o.emit = e * (m * tint);
    return o;
}
