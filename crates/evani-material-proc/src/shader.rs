//! The kit-material WGSL, composed from its parts with the module's
//! material slots filled in: the surfaces (`shaders/material.wgsl`), the sky
//! (`shaders/sky.wgsl`, feature `sky` only) and the entry
//! (`shaders/entry.wgsl`).

/// The surfaces part (`$WORLD_SLOT`, `$DECAL_SLOT` and `$SKY_SLOT` become
/// `u32` literals).
pub const MATERIAL: &str = include_str!("../shaders/material.wgsl");
/// The sky dome part (three.js Sky.js, MIT; see the file's notice).
#[cfg(feature = "sky")]
pub const SKY: &str = include_str!("../shaders/sky.wgsl");
/// `kit_material` itself (`$SKY_CALL` becomes the sky colour call).
pub const ENTRY: &str = include_str!("../shaders/entry.wgsl");

/// A slot number that matches no material (the renderer has 16): decals
/// stay unpainted when the look has no decal slot, and no sky is drawn
/// without a sky slot.
pub const NO_SLOT: u32 = 255;

/// The kit code for these slots. Without feature `sky` the sky slot is
/// ignored (the code carries no sky).
pub fn wgsl(world_slot: u32, decal_slot: u32, sky_slot: u32) -> String {
    #[cfg(feature = "sky")]
    let (sky_part, sky_call, sky_slot) = (SKY, "mp_sky(n)", sky_slot);
    #[cfg(not(feature = "sky"))]
    let (sky_part, sky_call, sky_slot) = ("", "vec3<f32>(1.0)", {
        let _ = sky_slot;
        NO_SLOT
    });
    let mut s = String::with_capacity(MATERIAL.len() + sky_part.len() + ENTRY.len() + 2);
    s.push_str(MATERIAL);
    s.push('\n');
    s.push_str(sky_part);
    s.push('\n');
    s.push_str(&ENTRY.replace("$SKY_CALL", sky_call));
    s.replace("$WORLD_SLOT", &world_slot.to_string())
        .replace("$DECAL_SLOT", &decal_slot.to_string())
        .replace("$SKY_SLOT", &sky_slot.to_string())
}
