//! Dev-only asset focus (kit feature `live`, which the app turns on with its
//! `live` module, so shipped builds never carry it). The kit lists its
//! assets for `focus.list` and, while one is focused (`focus.set`, or the
//! page's `?focus=item_box`), draws only that asset over the track and
//! scenery (or over the sky with `backdrop: false`) and reports where it is;
//! evani-core turns the camera round it. The game is frozen meanwhile but
//! render time keeps running, so the item box still spins, bobs and cycles
//! its colours, and its `kit.kart_racer.item_box_*` tunables show live.

use evani_core::ecs::World;
use evani_core::focus::{FocusAsset, FocusAssets, FocusFrame, FocusState};
use evani_render::RenderWorld;

use crate::Tune;
use crate::draw::{BoxSeat, ItemBoxLook, Mat, draw_bomb, draw_goo, draw_item_box, draw_kart, draw_seeker, lerp3};
use crate::race::RaceView;

/// The kit's name as `add_kit` records it ([`crate::KartRacerPlugin`]).
pub const OWNER: &str = "kart-racer";
/// The file that draws every asset here (what Evani.App sends with feedback).
const DRAW: &str = "kits/evani-kit-kart-racer/src/draw.rs";

/// The kit's focusable assets.
pub fn register(assets: &mut FocusAssets) {
    assets.add(
        FocusAsset::new("item_box", "Item box")
            .kind("item")
            .params(["kit.kart_racer.item_box_", "kit.kart_racer.item_respawn_s"])
            .path(DRAW).radius(1.4),
    );
    assets.add(FocusAsset::new("kart", "Kart with driver").kind("vehicle").params(["kit.kart_racer.drift_slip_deg"]).path(DRAW).radius(1.9));
    assets.add(FocusAsset::new("goo", "Goo Blob").kind("item").params(["kit.kart_racer.goo_"]).path(DRAW).radius(1.1));
    assets.add(FocusAsset::new("bomb", "Boing Bomb").kind("item").params(["kit.kart_racer.bomb_"]).path(DRAW).radius(0.8));
    assets.add(FocusAsset::new("seeker", "Seeker Star").kind("item").params(["kit.kart_racer.seeker_"]).path(DRAW).radius(0.9));
}

/// What the draw system needs from the focus state this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Focused {
    pub id: String,
    pub backdrop: bool,
    pub instance: Option<u32>,
}

/// The focused asset when it is one of this kit's.
pub fn focused(world: &World) -> Option<Focused> {
    let st = world.try_res::<FocusState>()?;
    let assets = world.try_res::<FocusAssets>()?;
    let id = st.active_for(&assets, OWNER)?;
    Some(Focused {
        id: id.to_string(),
        backdrop: st.backdrop,
        instance: st.instance,
    })
}

/// Item box `instance`, or the middle box of the first row.
pub fn box_index(race: &RaceView, instance: Option<u32>) -> Option<usize> {
    let i = instance.map_or((race.track.item_boxes_per_row.max(1) / 2) as usize, |n| n as usize);
    (i < race.boxes.len()).then_some(i)
}

/// The yaw (degrees, 0 = +Z) looking back down the track from a point at
/// distance `s`: the turntable's yaw 0 then views from behind, as a racer
/// comes up to it.
fn behind_yaw(race: &RaceView, s: f32, lateral: f32) -> f32 {
    let a = race.track.point_at(s - 1.0, lateral);
    let b = race.track.point_at(s + 1.0, lateral);
    (a[0] - b[0]).atan2(a[2] - b[2]).to_degrees()
}

/// Draw the focused asset alone and say where it is. `None` when the race
/// has no such asset (the normal view then draws).
#[allow(clippy::too_many_arguments)]
pub fn draw(f: &Focused, race: &RaceView, t: &Tune, look: &ItemBoxLook, seats: &[Option<BoxSeat>], mats: &[Mat; 16], alpha: f32, time: f32, shadows: bool, rw: &mut RenderWorld) -> Option<FocusFrame> {
    // Items sit on the road by the focused (or the middle) item box.
    let road = |race: &RaceView| -> Option<([f32; 3], f32)> {
        let i = box_index(race, None)?;
        let b = race.boxes[i].pos;
        let loc = race.track.locate(b, 0);
        let mut p = race.track.point_at(loc.s + 6.0, loc.lateral);
        p[1] = b[1] - 1.0;
        Some((p, behind_yaw(race, loc.s + 6.0, loc.lateral)))
    };
    match f.id.as_str() {
        "item_box" => {
            let i = box_index(race, f.instance)?;
            let mut b = race.boxes[i];
            b.hidden = 0.0;
            draw_item_box(&b, i, look, seats.get(i).copied().flatten().as_ref(), mats, time, shadows, rw);
            let loc = race.track.locate(b.pos, 0);
            // Centre on the resting box; the radius follows its size, so a
            // bigger box stays framed.
            let lift = look.lift;
            Some(FocusFrame {
                center: [b.pos[0], b.pos[1] + lift, b.pos[2]],
                radius: (look.size * 0.8 + look.bob_m).max(0.2),
                front_yaw_deg: behind_yaw(race, loc.s, loc.lateral),
            })
        }
        "kart" => {
            let i = f.instance.map_or_else(|| race.player_kart(0).unwrap_or(0), |n| n as usize);
            let k = race.karts.get(i)?;
            let spec = &race.roster[k.spec];
            let pos = lerp3(k.prev_pos, k.pos, alpha);
            let yaw = k.prev_yaw + crate::kart::wrap_angle(k.yaw - k.prev_yaw) * alpha;
            let vyaw = yaw + k.slip + k.spin_angle;
            draw_kart(k, (spec.color, spec.suit), pos, vyaw, t.max_speed, mats, rw);
            Some(FocusFrame {
                center: [pos[0], pos[1] + 0.9, pos[2]],
                radius: 1.9,
                // Yaw 0 looks at the kart's nose.
                front_yaw_deg: vyaw.to_degrees(),
            })
        }
        "goo" => {
            let (p, yaw) = road(race)?;
            draw_goo(p, mats, rw);
            Some(FocusFrame { center: [p[0], p[1] + 0.2, p[2]], radius: 1.1, front_yaw_deg: yaw })
        }
        "bomb" => {
            let (mut p, yaw) = road(race)?;
            p[1] += 0.6;
            draw_bomb(p, mats, time, rw);
            Some(FocusFrame { center: [p[0], p[1] + 0.2, p[2]], radius: 0.8, front_yaw_deg: yaw })
        }
        "seeker" => {
            let (mut p, yaw) = road(race)?;
            p[1] += 1.0;
            draw_seeker(p, mats, time, rw);
            Some(FocusFrame { center: p, radius: 0.9, front_yaw_deg: yaw })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_listed_with_their_tunables() {
        let mut list = FocusAssets::default();
        register(&mut list);
        let ids: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["item_box", "kart", "goo", "bomb", "seeker"]);
        let b = list.get("item_box").unwrap();
        assert!(b.tunes("kit.kart_racer.item_box_glyph_glow"));
        assert!(b.tunes("kit.kart_racer.item_box_glyph_color"));
        assert!(b.tunes("kit.kart_racer.item_box_color_1"));
        assert!(!b.tunes("kit.kart_racer.max_speed"));
    }
}
