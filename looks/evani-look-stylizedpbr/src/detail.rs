//! Biplanar detail (feature `biplanar`): the baked tiling fabric weave the
//! shader lays over flagged materials. R = brightness and G = thread height,
//! both centred on 0.5 so a flat area changes nothing.

/// Bake a plain weave: `size` x `size` RGBA8 with `threads` threads per
/// side each way. Over/under alternates like a real plain weave: the thread
/// on top is rounded (bright along its crown, darker at its edges), the gaps
/// between threads are low and dark, and a light per-thread variation keeps
/// it from looking printed. Same inputs, same bytes; tiles seamlessly.
pub fn weave_texture(size: u32, threads: u32, seed: u32) -> Vec<u8> {
    let size = size.max(8);
    let threads = threads.clamp(1, size / 4);
    let cell = size as f32 / threads as f32;
    let vary = |i: u32, axis: u32| -> f32 {
        let mut h = i.wrapping_mul(0x9e3d_5a27) ^ axis.wrapping_mul(0x6c8e_9cf5) ^ seed.wrapping_mul(0x2545_f491);
        h ^= h >> 15;
        h = h.wrapping_mul(0x7f4a_7c15);
        h ^= h >> 13;
        ((h & 0xff) as f32 / 255.0 - 0.5) * 0.12
    };
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = ((x as f32 + 0.5) / cell, (y as f32 + 0.5) / cell);
            let (ix, iy) = (fx.floor() as u32, fy.floor() as u32);
            let (ux, uy) = (fx - fx.floor(), fy - fy.floor());
            // Across-thread profile: 1 on the crown, 0 in the gap.
            let across_x = (1.0 - (2.0 * uy - 1.0).powi(2)).max(0.0).sqrt();
            let across_y = (1.0 - (2.0 * ux - 1.0).powi(2)).max(0.0).sqrt();
            // Which thread is on top in this cell.
            let warp_on_top = (ix + iy) % 2 == 0;
            let (top, top_vary, bottom) = if warp_on_top { (across_y, vary(ix, 0), across_x) } else { (across_x, vary(iy, 1), across_y) };
            // Along the top thread it dips at the cell ends where it passes under.
            let along = if warp_on_top { uy } else { ux };
            let dip = 1.0 - 0.35 * (2.0 * along - 1.0).powi(4);
            let height = (0.1 + 0.55 * top * dip).max(0.25 * bottom + 0.05);
            let bright = (0.32 + 0.36 * top * dip + top_vary).clamp(0.0, 1.0);
            out.extend_from_slice(&[(bright * 255.0).round() as u8, (height * 255.0).round() as u8, 128, 255]);
        }
    }
    out
}
