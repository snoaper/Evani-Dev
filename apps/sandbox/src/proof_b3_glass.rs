//! Batch 3 B3-P3: `?set=b3-glass`, the see-through pass's proof (feature
//! `recipes-proof`, which turns on evani-render's `material-recipes-refract`).
//! Row 0: the nine window-glass rows as panes (batch 1's clear window, then
//! batch 3's wavy, frosted, three stained, leaded, wired and glass block),
//! standing in front of a busy back wall (brick, painted tiles, a gold ball
//! and a red ball between pane and wall, so distortion, frost and tint read).
//! Row 1: fire, smoke and steam cards and a hologram ball, with a clear pane
//! in front of the flame and one behind the smoke (the far-to-near order), in
//! front of the same wall. `?row=0|1` frames one row close, `?far=<k>` k
//! times further back, `?q=` the quality, `?sun=back` the sun behind the
//! panes (stained glass glows), `?blend=0` and `?refract=0` flip the live
//! switches. Code-built: no new asset.

use core::cell::RefCell;

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::LookData;
use evani_material_recipes::Block;
use evani_render::model::{ModelId, ModelInstance, ModelState, Models};
use evani_render::{MeshId, MeshInstance, RenderWorld};

use super::{card_glb, cube_glb, rewrite, sphere_glb, url_value};

/// Where the proof stands: beside the other grids.
const Z: f32 = -70.0;

/// The glass rows, left to right (id, block hex).
pub const GLASS: [(&str, &str); 9] = [
    ("glass.window.clear", "1800011de4f0f83a3c400000001a000d218000000000004c0000000000000000"),
    ("glass.window.wavy", "1800011de2f0ea3a3c4000000033008c0d4d0000000000ee0000000000000000"),
    ("glass.window.frosted", "1800011deef4f43a3c4000000040b30d216600000000008e0000000000000000"),
    ("glass.stained.red", "1803011db010202a2a2c00000066002614e60000000000340000000000000000"),
    ("glass.stained.blue", "1803011d1c3aa82a2a2c00000066002614e60000000000e50000000000000000"),
    ("glass.stained.amber", "1803011de09a202a2a2c00000059002614cc0000000000080000000000000000"),
    ("glass.leaded", "1804011ddceae034363a00000033004d27660000000000580000000000000000"),
    ("glass.wired", "1805011dd8e8e28a8c900000004d000d624d0000000000e20000000000000000"),
    ("glass.glass_block", "1806011dd0e4e4c8c4bc000000b32699144d0000000000b30000000000000000"),
];

/// Row 1's see-through rows (id, block hex): flame, smoke, steam, hologram.
pub const FIRE: [(&str, &str); 4] = [
    ("fire.flame.wood", "1c000100fff0a0ff6a102a2420666666804d4d00000000130000000000000000"),
    ("fire.smoke", "1c030100c8c8c8ff8a40403c3826264d99330000000000d60000000000000000"),
    ("fire.steam", "1c040100f0f4f8a0b0c0c0c8d01a2633994d0000000000630000000000000000"),
    ("sf.holo", "1d0403b440c0ffc0f0ff000000503333392659cc000000720000000000000000"),
];

/// The busy background (id, block hex, roughness, metalness).
pub const BACK: [(&str, &str, f32, f32); 4] = [
    ("brick.red.common", "070003dda85a42b8b2a48a4a382f0c2a80d99999000000d60000000000000000", 0.85, 0.0),
    ("tile.blue_painted", "170507d5ead8c0f4f2ec2a4a9ad980264b001a000000008b0000000000000000", 0.1, 0.0),
    ("tile.mexican", "170506d5d8a030f2eee21e4aa8cc80001c0026000000008e0000000000000000", 0.12, 0.0),
    ("wood.floor.oak", "040306dfd8a868a870385a3a204d2c59730d13594d00007f0000000000000000", 0.3, 0.0),
];

/// The pane spacing and the two rows' depth.
const STEP: f32 = 1.2;
const ROW_Z: [f32; 2] = [Z, Z + 4.0];

#[derive(Default)]
struct State {
    /// Glass panes, fire row, back wall pieces (in that order).
    models: Vec<ModelId>,
    frames: u32,
    row: Option<usize>,
    far: f32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn block(hex: &str) -> Option<Block> {
    Block::from_hex(hex).ok()
}

/// The camera: the whole scene, or row `r` close (k times further back).
fn camera(row: Option<usize>, far: f32) -> CameraPose {
    let (z, half) = match row {
        Some(r) => (ROW_Z[r.min(1)], if r == 0 { 6.6 } else { 3.6 }),
        None => (Z + 2.0, 8.0),
    };
    let d = half * 1.25 * far;
    CameraPose { eye: [0.0, 1.0 + d * 0.06, z + d], target: [0.0, 0.85, z], up: [0.0, 1.0, 0.0], fov_y_deg: 42.0 }
}

/// The `?set=b3-glass` frame (called by the recipes proof's system).
pub fn frame(world: &mut World) {
    STATE.with_borrow_mut(|st| run(world, st));
}

fn run(world: &mut World, st: &mut State) {
    if st.frames == 0 {
        st.row = url_value("row").and_then(|r| r.parse().ok()).filter(|&r: &usize| r < 2);
        st.far = url_value("far").and_then(|k| k.parse().ok()).filter(|k: &f32| *k > 0.0).unwrap_or(1.0);
        let mut reg = world.res_mut::<ParamRegistry>();
        let sun = if url_value("sun").is_some_and(|s| s == "back") { [-0.2, 0.45, -0.87] } else { super::SUN };
        let _ = reg.set_by_name("render.light.direction", ParamValue::Vec3(sun));
        if url_value("blend").is_some_and(|b| b == "0") {
            let _ = reg.set_by_name("material.recipe.blend.enabled", ParamValue::Bool(false));
        }
        if url_value("refract").is_some_and(|b| b == "0") {
            let _ = reg.set_by_name("material.recipe.blend.refraction", ParamValue::F32(0.0));
        }
    }
    if st.frames == 4
        && let Some(q) = super::quality_from_url()
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        for name in ["material.quality.conditions", "material.quality.detail", "material.quality.glass", "material.quality.reflections"] {
            let _ = reg.set_by_name(name, ParamValue::Enum(q));
        }
    }
    st.frames = st.frames.saturating_add(1);
    if let Some(mut models) = world.try_res_mut::<Models>() {
        if st.models.is_empty() {
            let (card, cube, ball) = (card_glb(), cube_glb(), sphere_glb());
            for (id, hex) in GLASS {
                if let Some(glb) = block(hex).and_then(|b| rewrite(&card, id, &b, 0.05, 0.0)) {
                    st.models.push(models.add_glb(&format!("models/b3g-{id}.glb"), &glb));
                }
            }
            for (id, hex) in FIRE {
                let shape = if id == "sf.holo" { &ball } else { &card };
                if let Some(glb) = block(hex).and_then(|b| rewrite(shape, id, &b, 0.5, 0.0)) {
                    st.models.push(models.add_glb(&format!("models/b3g-{id}.glb"), &glb));
                }
            }
            for (id, hex, rough, metal) in BACK {
                if let Some(glb) = block(hex).and_then(|b| rewrite(&cube, id, &b, rough, metal)) {
                    st.models.push(models.add_glb(&format!("models/b3g-{id}.glb"), &glb));
                }
            }
            // Two plain balls between the panes and the wall: gold and red.
            for (id, hex) in [("metal.gold.polished", "010005bff4c45afce6a48a6a301aff007f0f4d00240000020000000000000000"), ("plastic.glossy.red", "03000195c81e1ee0e0e00000001f33004c2d0000000000a30000000000000000")] {
                if let Some(glb) = block(hex).and_then(|b| rewrite(&ball, id, &b, 0.3, 0.0)) {
                    st.models.push(models.add_glb(&format!("models/b3g-{id}.glb"), &glb));
                }
            }
        }
        let ready = st.models.iter().filter(|&&m| matches!(models.state(m), Some(ModelState::Ready))).count();
        let n = st.models.len();
        let at = |i: usize| st.models.get(i).copied();
        let show = |r: usize| st.row.is_none_or(|x| x == r);
        // Row 0: the panes.
        for (k, _) in GLASS.iter().enumerate() {
            if let (Some(m), true) = (at(k), show(0)) {
                let x = (k as f32 - 4.0) * STEP;
                models.draw(ModelInstance::new(m, [x, 0.05, ROW_Z[0]]));
            }
        }
        // Row 1: flame, smoke, steam cards and the hologram; a clear pane in
        // front of the flame and one behind the smoke.
        let g = GLASS.len();
        for k in 0..FIRE.len() {
            if let (Some(m), true) = (at(g + k), show(1)) {
                let x = (k as f32 - 1.5) * 1.4;
                let s = if k == 3 { [0.9; 3] } else { [1.0; 3] };
                models.draw(ModelInstance::new(m, [x, 0.05, ROW_Z[1]]).scale(s));
            }
        }
        if let (Some(clear), true) = (at(0), show(1)) {
            models.draw(ModelInstance::new(clear, [-2.1, 0.05, ROW_Z[1] + 0.6]));
            models.draw(ModelInstance::new(clear, [-0.7, 0.05, ROW_Z[1] - 0.6]));
        }
        // The wall behind each row: brick, with painted tile and oak strips,
        // and the two balls between the panes and the wall.
        let w = g + FIRE.len();
        for (r, &rz) in ROW_Z.iter().enumerate() {
            if !show(r) {
                continue;
            }
            let back = rz - 1.6;
            if let Some(m) = at(w) {
                models.draw(ModelInstance::new(m, [0.0, 0.0, back]).scale([14.0, 3.6, 0.3]));
            }
            for (j, piece) in [w + 1, w + 2, w + 3].into_iter().enumerate() {
                if let Some(m) = at(piece) {
                    for i in 0..4 {
                        let x = (i as f32 * 3.0 - 4.5) + j as f32 * 0.9;
                        models.draw(ModelInstance::new(m, [x, 0.25 + j as f32 * 0.55, back + 0.2]).scale([0.8, 0.5, 0.2]));
                    }
                }
            }
            for (j, ball) in [w + 4, w + 5].into_iter().enumerate() {
                if let Some(m) = at(ball) {
                    for i in 0..3 {
                        let x = (i as f32 * 3.6 - 3.6) + j as f32 * 1.6;
                        models.draw(ModelInstance::new(m, [x, 0.0, rz - 0.8]).scale([0.5; 3]));
                    }
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if st.frames % 10 == 0
            && let Some(d) = web_sys::window().and_then(|w| w.document())
        {
            d.set_title(&format!("b3-glass frames={} drawn={ready}/{n}", st.frames));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (ready, n);
    }
    let ground = world.try_res::<LookData>().map_or(0, |l| RenderWorld::material(&l, "ground"));
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = camera(st.row, st.far);
        rw.sprites.clear();
        rw.meshes.clear();
        rw.blobs.clear();
        let mut plane = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, Z + 2.0]);
        plane.scale = [24.0, 1.0, 24.0];
        plane.material = ground;
        rw.draw_mesh(plane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every block is what the catalogue encodes for its row, every glass row
    /// is ready and see-through, and the four row-1 rows blend as they should
    /// (flame and hologram add, smoke and steam over).
    #[test]
    fn the_blocks_are_the_catalogue_rows() {
        use evani_material_recipes::blend::{ADD, OVER, mode};
        use evani_material_recipes::catalog::Catalog;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
        let cat = Catalog::load_dir(&dir).unwrap();
        let mut wrong = Vec::new();
        for &(id, hex) in GLASS.iter().chain(FIRE.iter()) {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(row.status, "ready", "{id}");
            let want = cat.encode(row).unwrap().to_hex();
            if hex != want {
                wrong.push(format!("(\"{id}\", \"{want}\"),"));
            }
        }
        for &(id, hex, _, _) in &BACK {
            let row = cat.row(id).unwrap();
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
        }
        assert!(wrong.is_empty(), "blocks to update:\n{}", wrong.join("\n"));
        for (id, hex) in GLASS {
            assert_ne!(block(hex).map(|b| mode(&b)), Some(0), "{id} is see-through");
        }
        let modes: Vec<u8> = FIRE.iter().filter_map(|(_, h)| block(h).map(|b| mode(&b))).collect();
        assert_eq!(modes, [ADD, OVER, OVER, ADD]);
    }
}
