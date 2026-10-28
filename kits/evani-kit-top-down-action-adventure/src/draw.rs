//! The `Frame` system: Mossgate Keep in pixel art. The kit's frames
//! ([`crate::art`], `tda.*`) go into the renderer's sprite art once; every
//! frame after that draws them on the art grid: autotiled walls, floor,
//! pits, switches, gates, doors, the room's props, pickups, enemies, the
//! hero and his sword, contact shadows, sparks, dust, floating text, the HUD
//! strip and the banners (3x5 [`crate::font`]). Colours come from the active
//! look's ramps, so the kit never names a look. With the `lights2d` feature
//! the scene also pushes its torch, magic, bolt and hero lights.
//!
//! One room fills the view (256 x 176 art pixels) under a HUD strip of
//! `hud_height_px`. The scale (device pixels per art pixel) is a whole
//! number with `render.pixel_art` on, so the grid stays crisp; the letterbox
//! around the view is tiled with the dark backdrop frame.

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::time::Time;
use evani_render::{ArtSource, RenderWorld, Sprite, SpriteArt, SpriteSheet};

use crate::art::{self, Built, Cells, Colors, F};
use crate::font;
use crate::game::{EState, Game, Phase};
use crate::level::{Cell, EnemyKind, PickupKind, PropKind, ROOM_H, ROOM_PX_H, ROOM_PX_W, ROOM_W, TILE};
use crate::{TopDown, TopDownKit, Tune};

/// Sprite layers (lower draw first).
pub mod layer {
    pub const BACK: i16 = 0;
    pub const TILES: i16 = 1;
    /// Flat on the floor: wall shade, pit lips, cracks, moss, dust.
    pub const DECAL: i16 = 2;
    pub const PROPS: i16 = 3;
    pub const SHADOW: i16 = 4;
    pub const ITEMS: i16 = 5;
    pub const ACTORS: i16 = 6;
    pub const BLADE: i16 = 7;
    /// Unlit from here up (sparks, bolts, arcs, text).
    pub const FX: i16 = 8;
    pub const HUD: i16 = 10;
    pub const BANNER: i16 = 11;
}

/// A dust puff behind the walking hero.
#[derive(Debug, Clone, Copy)]
struct Puff {
    pos: [f32; 2],
    room: u16,
    born: u64,
}

/// Ticks a dust frame shows.
const PUFF_FRAME: u64 = 6;
/// Ticks between dust puffs while the hero walks.
const PUFF_EVERY: u64 = 12;
/// HUD hearts in a row.
const HEARTS_PER_ROW: i32 = 9;

/// Draw-side state kept between frames.
#[derive(Debug, Default)]
pub struct DrawState {
    sheet_ready: bool,
    /// The kit's sheet went into the sprite art (or there was none to add to).
    added: bool,
    /// Frame sizes and fallback tints (the pixels went to the sprite art).
    built: Option<Built>,
    colors: Option<Colors>,
    cells: Option<Cells>,
    /// The sprite-art revision the cells were resolved for.
    art_rev: u64,
    puffs: Vec<Puff>,
    last_tick: u64,
    /// The last frame's mapping from art pixels to the canvas.
    pub screen: Screen,
}

/// How art pixels map to the canvas.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Screen {
    /// Device pixels per art pixel.
    pub scale: f32,
    /// Device pixels where the view's top-left sits.
    pub origin: [f32; 2],
    /// CSS pixels per device pixel.
    pub css_per_px: [f32; 2],
    /// The view in art pixels.
    pub view: [f32; 2],
}

impl Screen {
    /// Fit a `view` of art pixels into `target` device pixels, centred.
    pub fn fit(target: [f32; 2], css: [f32; 2], view: [f32; 2], pixel_art: bool) -> Self {
        let target = target.map(|v| v.max(1.0));
        let fit = (target[0] / view[0]).min(target[1] / view[1]);
        let scale = if pixel_art && fit >= 1.0 { fit.floor() } else { fit.max(0.1) };
        let origin = [((target[0] - view[0] * scale) * 0.5).floor(), ((target[1] - view[1] * scale) * 0.5).floor()];
        Self { scale, origin, css_per_px: [css[0] / target[0], css[1] / target[1]], view }
    }

    /// An art-pixel point (with `shake`) in CSS pixels, for lights.
    pub fn css(&self, p: [f32; 2], shake: [f32; 2]) -> [f32; 2] {
        [
            (self.origin[0] + (p[0] + shake[0]) * self.scale) * self.css_per_px[0],
            (self.origin[1] + (p[1] + shake[1]) * self.scale) * self.css_per_px[1],
        ]
    }
}

const WHITE: [f32; 4] = [1.0; 4];

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

/// A triangle wave in -1..1 with period 4 (matches the game's).
fn tri(t: f32) -> f32 {
    let m = t - 4.0 * (t / 4.0).floor();
    (m - 2.0).abs() - 1.0
}

/// The art and colours a frame draws with.
struct Art<'a> {
    col: &'a Colors,
    cells: &'a Cells,
}

struct Ctx<'a> {
    rw: &'a mut RenderWorld,
    s: Screen,
    /// Whole-pixel shake of the room, art px.
    shake: [f32; 2],
    /// The plain white cell (rectangles, text).
    white: u16,
}

impl Ctx<'_> {
    /// A quad of `cell` (top-left and size in art pixels), snapped to whole
    /// device pixels so neighbours never gap.
    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, cell: u16, color: [f32; 4], layer: i16) {
        let sh = if layer > layer::BACK && layer < layer::HUD { self.shake } else { [0.0; 2] };
        let (sc, o) = (self.s.scale, self.s.origin);
        let x0 = (o[0] + (x + sh[0]) * sc).round();
        let x1 = (o[0] + (x + sh[0] + w) * sc).round().max(x0 + 1.0);
        let y0 = (o[1] + (y + sh[1]) * sc).round();
        let y1 = (o[1] + (y + sh[1] + h) * sc).round().max(y0 + 1.0);
        let c = self.s.css_per_px;
        self.rw.draw_sprite(Sprite {
            pos: [(x0 + x1) * 0.5 * c[0], (y0 + y1) * 0.5 * c[1]],
            size: [(x1 - x0) * c[0], (y1 - y0) * c[1]],
            rotation: 0.0,
            cell,
            color,
            layer,
        });
    }

    /// A plain rectangle.
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], layer: i16) {
        self.quad(x, y, w, h, self.white, color, layer);
    }

    /// Frame `f` at its art size, top-left on a whole art pixel.
    fn frame(&mut self, f: F, x: f32, y: f32, color: [f32; 4], layer: i16) {
        self.quad(x.round(), y.round(), f.w, f.h, f.cell, mul(f.tint, color), layer);
    }

    /// Frame `f` centred on (`cx`, `cy`).
    fn frame_c(&mut self, f: F, cx: f32, cy: f32, color: [f32; 4], layer: i16) {
        self.frame(f, cx - (f.w * 0.5).floor(), cy - (f.h * 0.5).floor(), color, layer);
    }

    /// Frame `f`'s white hit flash, top-left at (`x`, `y`).
    fn flash(&mut self, f: F, x: f32, y: f32, layer: i16) {
        self.quad(x.round(), y.round(), f.w, f.h, f.flash, WHITE, layer);
    }

    /// Text with its top-left at (`x`, `y`), `px` art pixels per font pixel.
    fn text(&mut self, text: &str, x: f32, y: f32, px: f32, color: [f32; 4], layer: i16) {
        let mut runs = Vec::new();
        font::runs(text, |c, r, l| runs.push((c, r, l)));
        for (c, r, l) in runs {
            self.rect(x + c as f32 * px, y + r as f32 * px, l as f32 * px, px, color, layer);
        }
    }

    /// Text centred on `cx`.
    fn text_c(&mut self, text: &str, cx: f32, y: f32, px: f32, color: [f32; 4], layer: i16) {
        let w = font::width(text) as f32 * px;
        self.text(text, (cx - w * 0.5).round(), y, px, color, layer);
    }
}

/// The `Frame` system.
pub fn draw_system(world: &mut World) {
    let Some(kit) = world.try_res::<TopDownKit>().map(|k| *k) else {
        return;
    };
    let alpha = world.res::<Time>().alpha;
    let (t, pixel_art) = {
        let reg = world.res::<ParamRegistry>();
        (kit.tune.read(&reg), reg.bool_or("render.pixel_art"))
    };
    let mut guard = world.res_mut::<DrawState>();
    let st: &mut DrawState = &mut guard;
    // One white cell for rectangles, text and frames no sheet has.
    if !st.sheet_ready {
        *world.res_mut::<SpriteSheet>() =
            SpriteSheet { width: 4, height: 4, rgba8: vec![255; 4 * 4 * 4], frames: vec![[0, 0, 4, 4]], revision: 1 };
        st.sheet_ready = true;
    }
    // The kit's pixel art, coloured from the look's ramps, once.
    if !st.added {
        st.added = true;
        let colors = Colors::new(world.try_res::<LookData>().as_deref());
        let mut built = art::build(&colors);
        if let Some(mut sprite_art) = world.try_res_mut::<SpriteArt>()
            && let Err(e) = sprite_art.add(ArtSource::Kit, core::mem::take(&mut built.sheet))
            && let Some(mut log) = world.try_res_mut::<ErrorLog>()
        {
            log.error(format!("top-down art: {e}"));
        }
        st.colors = Some(colors);
        st.built = Some(built);
        st.cells = None;
    }
    // Cells follow the sprite art (a look or a later sheet may add frames).
    {
        let sprite_art = world.try_res::<SpriteArt>();
        let rev = sprite_art.as_ref().map_or(0, |a| a.revision());
        if st.cells.is_none() || rev != st.art_rev {
            if let Some(b) = st.built.as_ref() {
                st.cells = Some(Cells::resolve(sprite_art.as_deref(), b, SpriteSheet::cell(0)));
            }
            st.art_rev = rev;
        }
    }
    let (Some(col), Some(cells)) = (st.colors.as_ref(), st.cells.as_ref()) else {
        return;
    };
    let a = Art { col, cells };
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else {
        return;
    };
    rw.hud_layers = Some([layer::HUD, layer::BANNER]);
    let css = if rw.viewport_css[0] > 0.0 && rw.viewport_css[1] > 0.0 { rw.viewport_css } else { [1280.0, 720.0] };
    let target = if rw.target_px[0] > 0 && rw.target_px[1] > 0 {
        [rw.target_px[0] as f32, rw.target_px[1] as f32]
    } else {
        css
    };
    let hud_h = t.hud_height_px;
    let screen = Screen::fit(target, css, [ROOM_PX_W, ROOM_PX_H + hud_h], pixel_art);
    let p = world.res::<TopDown>();
    let g = &p.game;
    // Shake: whole art pixels from the trauma (a cheap fixed pattern).
    let shake = if g.trauma > 0.0 {
        let k = g.trauma * g.trauma * t.shake_px;
        [(k * tri(g.tick as f32 * 0.9)).round(), (k * tri(g.tick as f32 * 1.3 + 1.0)).round()]
    } else {
        [0.0; 2]
    };
    dust_update(&mut st.puffs, &mut st.last_tick, g, &t);
    let mut c = Ctx { rw: &mut rw, s: screen, shake, white: SpriteSheet::cell(0) };
    backdrop(&mut c, &a, target);
    room(&mut c, &a, g, &t, hud_h);
    props(&mut c, &a, g, &t, hud_h);
    things(&mut c, &a, g, &t, hud_h, alpha, &st.puffs);
    hud(&mut c, &a, g, &t, hud_h);
    banners(&mut c, &a, g, &t, hud_h);
    #[cfg(feature = "lights2d")]
    if let Some(mut l) = world.try_res_mut::<evani_render::Lights2d>() {
        lights(&mut l, &a, g, &t, hud_h, &screen, shake);
    }
    st.screen = screen;
}

/// This frame's 2D lights: flickering warm torches, the cool Dawn Shard,
/// wand bolts and a latched switch, the Warden's core (stronger as it
/// weakens) and a soft light on the hero. Radii are art px; the renderer
/// keeps the strongest lights the tier allows.
#[cfg(feature = "lights2d")]
fn lights(l: &mut evani_render::Lights2d, a: &Art, g: &Game, t: &Tune, top: f32, s: &Screen, shake: [f32; 2]) {
    use evani_render::Light2d;
    let lc = a.col.light;
    // The unlit shade leans toward the look's ambient colour (brightest
    // channel kept at 1, so `fx.lights2d.ambient` still sets the level).
    let amb = lc[0];
    let top_ch = amb[0].max(amb[1]).max(amb[2]).max(1e-4);
    let k = t.ambient_tint.clamp(0.0, 1.0);
    l.ambient_color = amb.map(|v| 1.0 + (v / top_ch - 1.0) * k);
    l.unlit_from = layer::FX;
    let px = s.scale * s.css_per_px[0];
    let mut push = |p: [f32; 2], rgb: [f32; 3], radius: f32, intensity: f32| {
        if radius > 0.0 && intensity > 0.0 {
            l.push(Light2d::new(s.css([p[0], top + p[1]], shake), rgb, radius * px, intensity));
        }
    };
    let r = g.room();
    let pr = g.player.room;
    let cool = lc[3];
    if let Some(room) = g.level.rooms.get(r) {
        for (i, p) in room.props.iter().enumerate().filter(|(_, p)| p.kind == PropKind::Torch) {
            let f = (art::hash(i as i32, (g.tick / 5) as i32, 31) % 100) as f32 / 100.0;
            let c = [p.tx as f32 * TILE + TILE * 0.5, p.ty as f32 * TILE + TILE * 0.5 + 2.0];
            push(c, lc[2], t.torch_light_radius * (0.95 + 0.1 * f), t.torch_light_intensity * (0.85 + 0.15 * f));
        }
        if room.has_switch && g.latched(r) {
            for ty in 0..ROOM_H as i32 {
                for tx in 0..ROOM_W as i32 {
                    if g.level.cell(r, tx, ty) == Cell::Switch {
                        push([tx as f32 * TILE + TILE * 0.5, ty as f32 * TILE + TILE * 0.5], cool, 40.0, 0.6);
                    }
                }
            }
        }
    }
    let pulse = 0.5 + 0.5 * (g.tick as f32 * 0.08).sin();
    for p in g.pickups.iter().filter(|p| p.room == pr && p.active && p.kind == PickupKind::Crystal) {
        push(p.pos, cool, 64.0, 0.8 + 0.4 * pulse);
    }
    for sh in g.shots.iter().filter(|sh| sh.room == pr && sh.friendly) {
        push(sh.pos, cool, 32.0, 0.8);
    }
    let core = a.col.lin(art::RED, 3, 1.0);
    for e in g.enemies.iter().filter(|e| e.room == pr && e.kind == EnemyKind::Warden && e.hp > 0) {
        let weak = 1.0 - (e.hp as f32 / g.warden_max_hp.max(1) as f32).clamp(0.0, 1.0);
        push(e.pos, [core[0], core[1], core[2]], 56.0, 0.3 + 0.7 * weak);
    }
    if g.phase != Phase::Title {
        push(g.player.pos, lc[4], t.hero_light_radius, t.hero_light_intensity);
    }
}

/// The dark canvas and the backdrop frame tiled over the letterbox only.
fn backdrop(c: &mut Ctx, a: &Art, target: [f32; 2]) {
    let s = c.s;
    let full = Screen { scale: 1.0, origin: [0.0, 0.0], css_per_px: s.css_per_px, view: target };
    c.s = full;
    c.rect(0.0, 0.0, target[0], target[1], a.col.lin(art::INK, 0, 1.0), layer::BACK);
    c.s = s;
    // The canvas in art pixels, tiles on the room's grid.
    let x0 = (-s.origin[0] / s.scale / TILE).floor() as i32;
    let y0 = (-s.origin[1] / s.scale / TILE).floor() as i32;
    let x1 = ((target[0] - s.origin[0]) / s.scale / TILE).ceil() as i32;
    let y1 = ((target[1] - s.origin[1]) / s.scale / TILE).ceil() as i32;
    if (x1 - x0) * (y1 - y0) > 4096 {
        return;
    }
    let (vw, vh) = (s.view[0], s.view[1]);
    for ty in y0..y1 {
        for tx in x0..x1 {
            let (x, y) = (tx as f32 * TILE, ty as f32 * TILE);
            if x >= 0.0 && y >= 0.0 && x + TILE <= vw && y + TILE <= vh {
                continue;
            }
            c.frame(a.cells.backdrop, x, y, WHITE, layer::BACK);
        }
    }
}

fn room(c: &mut Ctx, a: &Art, g: &Game, t: &Tune, top: f32) {
    let r = g.room();
    let lv = &g.level;
    let latched = g.latched(r);
    let wall = |tx: i32, ty: i32| lv.cell(r, tx, ty) == Cell::Wall;
    let pit = |tx: i32, ty: i32| lv.cell(r, tx, ty) == Cell::Pit;
    let shade = a.col.lin(art::INK, 0, t.shadow_alpha);
    for ty in 0..ROOM_H as i32 {
        for tx in 0..ROOM_W as i32 {
            let (x, y) = (tx as f32 * TILE, top + ty as f32 * TILE);
            let cell = lv.cell(r, tx, ty);
            match cell {
                Cell::Wall => {
                    // Autotile: a bit per open neighbour (N 1, E 2, S 4, W 8).
                    let m = usize::from(!wall(tx, ty - 1))
                        | usize::from(!wall(tx + 1, ty)) << 1
                        | usize::from(!wall(tx, ty + 1)) << 2
                        | usize::from(!wall(tx - 1, ty)) << 3;
                    c.frame(a.cells.wall[m], x, y, WHITE, layer::TILES);
                    continue;
                }
                Cell::Pit => {
                    c.frame(a.cells.pit, x, y, WHITE, layer::TILES);
                    if !pit(tx, ty - 1) {
                        c.frame(a.cells.pit_lip, x, y, WHITE, layer::DECAL);
                    }
                    continue;
                }
                _ => {}
            }
            let checker = ((tx + ty) & 1) as usize;
            let detail = usize::from(art::hash(tx, ty, r as u32 + 7).is_multiple_of(5));
            c.frame(a.cells.floor[checker + 2 * detail], x, y, WHITE, layer::TILES);
            // Shade under the walls to the north and west.
            let north = wall(tx, ty - 1);
            if north {
                c.rect(x, y, TILE, 3.0, shade, layer::DECAL);
            }
            if wall(tx - 1, ty) {
                let dy = if north { 3.0 } else { 0.0 };
                c.rect(x, y + dy, 3.0, TILE - dy, shade, layer::DECAL);
            }
            match cell {
                Cell::Switch => c.frame(a.cells.switch[usize::from(latched)], x, y, WHITE, layer::DECAL),
                Cell::Gate if !latched => c.frame(a.cells.gate, x, y, WHITE, layer::PROPS),
                Cell::Door(l) if !g.doors.get(l).is_some_and(|d| d.open) => {
                    // N and S doors span tiles 7 and 8, W and E doors tiles 4 and 5.
                    let f = if ty == 0 || ty == ROOM_H as i32 - 1 {
                        a.cells.door_h[usize::from(tx > 7)]
                    } else {
                        a.cells.door_v[usize::from(ty > 4)]
                    };
                    c.frame(f, x, y, WHITE, layer::PROPS);
                }
                _ => {}
            }
        }
    }
}

/// A contact shadow centred on (`cx`, `cy`): size 0 small, 1 medium, 2 large.
fn shadow(c: &mut Ctx, a: &Art, t: &Tune, cx: f32, cy: f32, size: usize) {
    let f = a.cells.shadow[size.min(2)];
    c.frame_c(f, cx, cy, a.col.lin(art::INK, 0, t.shadow_alpha), layer::SHADOW);
}

fn props(c: &mut Ctx, a: &Art, g: &Game, t: &Tune, top: f32) {
    let Some(room) = g.level.rooms.get(g.room()) else {
        return;
    };
    let cl = a.cells;
    for (i, p) in room.props.iter().enumerate() {
        let (cx, cy) = (p.tx as f32 * TILE + TILE * 0.5, top + p.ty as f32 * TILE + TILE * 0.5);
        let (f, lay) = match p.kind {
            PropKind::Torch => (cl.torch[(g.tick / 6 + i as u64) as usize % 3], layer::PROPS),
            PropKind::Crack => (cl.crack, layer::DECAL),
            PropKind::Moss => (cl.moss, layer::DECAL),
            PropKind::Puddle => (cl.puddle, layer::DECAL),
            PropKind::Cobweb => (cl.cobweb[usize::from(p.tx > 8)], layer::DECAL),
            PropKind::Pot => (cl.pot, layer::PROPS),
            PropKind::Bones => (cl.bones, layer::PROPS),
            PropKind::Rubble => (cl.rubble, layer::PROPS),
            PropKind::Banner => (cl.banner, layer::PROPS),
            PropKind::Rack => (cl.rack, layer::PROPS),
            PropKind::Pillar => (cl.pillar, layer::PROPS),
            PropKind::Helmet => (cl.helmet, layer::PROPS),
            PropKind::Pedestal => (cl.pedestal, layer::PROPS),
        };
        if matches!(p.kind, PropKind::Pot | PropKind::Pillar | PropKind::Pedestal) {
            let sf = cl.shadow[1];
            c.frame_c(sf, cx, cy + (f.h * 0.5).floor(), a.col.lin(art::INK, 0, t.shadow_alpha), layer::DECAL);
        }
        c.frame_c(f, cx, cy, WHITE, lay);
    }
}

/// Spawn and age the hero's dust puffs, once per sim tick.
fn dust_update(puffs: &mut Vec<Puff>, last: &mut u64, g: &Game, t: &Tune) {
    if g.tick == *last {
        return;
    }
    *last = g.tick;
    let pl = &g.player;
    let moving = (pl.pos[0] - pl.prev[0]).abs() + (pl.pos[1] - pl.prev[1]).abs() > 0.01;
    if moving && g.phase == Phase::Playing && g.tick.is_multiple_of(PUFF_EVERY) {
        puffs.push(Puff { pos: [pl.pos[0], pl.pos[1] + t.hero_h * 0.5], room: pl.room, born: g.tick });
    }
    let (tick, room) = (g.tick, pl.room);
    puffs.retain(|p| p.room == room && p.born <= tick && tick - p.born < PUFF_FRAME * 3);
}

fn things(c: &mut Ctx, a: &Art, g: &Game, t: &Tune, top: f32, alpha: f32, puffs: &[Puff]) {
    let r = g.player.room;
    let cl = a.cells;
    let bob = tri(g.tick as f32 * 0.05);
    for p in puffs {
        let k = ((g.tick - p.born) / PUFF_FRAME).min(2) as usize;
        c.frame_c(cl.dust[k], p.pos[0], top + p.pos[1], WHITE, layer::DECAL);
    }
    // Pickups.
    for p in g.pickups.iter().filter(|p| p.room == r && p.active) {
        let f = match p.kind {
            PickupKind::Heart => cl.heart,
            PickupKind::Container => cl.container,
            PickupKind::Key => cl.key,
            PickupKind::Gem => cl.gem,
            PickupKind::Wand => cl.wand,
            PickupKind::Crystal => cl.shard,
        };
        let (x, y) = (p.pos[0], top + p.pos[1]);
        shadow(c, a, t, x, y + (f.h * 0.5).floor() + 2.0, 0);
        if p.kind == PickupKind::Crystal {
            let pulse = 0.5 + 0.5 * (g.tick as f32 * 0.08).sin();
            c.frame_c(cl.glow, x, y + bob.round(), a.col.lin(art::GLOW, 3, 0.3 + 0.3 * pulse), layer::SHADOW);
        }
        c.frame_c(f, x, y + bob.round(), WHITE, layer::ITEMS);
    }
    // Enemies.
    for (i, e) in g.enemies.iter().enumerate().filter(|(_, e)| e.room == r) {
        let (x, y) = (e.pos[0].round(), top + e.pos[1].round());
        let h = e.kind.half();
        let size = if h <= 5.0 {
            0
        } else if h <= 7.0 {
            1
        } else {
            2
        };
        shadow(c, a, t, x, y + h, size);
        let step = (g.tick / 16 + i as u64) as usize % 2;
        let f = match e.kind {
            EnemyKind::Slime => cl.slime[step],
            EnemyKind::Bat => cl.bat[step],
            EnemyKind::Husk => cl.husk[step],
            EnemyKind::Spitter => cl.spitter[step],
            EnemyKind::Warden => {
                let bright = e.hp * 2 <= g.warden_max_hp || e.state == EState::WindUp;
                cl.warden[usize::from(bright)]
            }
        };
        // Fliers by their middle, walkers by their feet.
        let (fx, fy) = if e.kind.flies() {
            (x - (f.w * 0.5).floor(), y - 3.0 + bob.round() - (f.h * 0.5).floor())
        } else {
            (x - (f.w * 0.5).floor(), y + h + 1.0 - f.h)
        };
        let warn = e.state == EState::WindUp && e.kind != EnemyKind::Warden && (g.tick / 3).is_multiple_of(2);
        if e.flash > 0.0 || warn {
            c.flash(f, fx, fy, layer::ACTORS);
        } else {
            c.frame(f, fx, fy, WHITE, layer::ACTORS);
        }
        if e.flash > 0.0 {
            let k = ((0.15 - e.flash) / 0.05).clamp(0.0, 2.0) as usize;
            c.frame_c(cl.spark[k], x, fy + (f.h * 0.5).floor(), WHITE, layer::FX);
        }
    }
    // Shots: wand bolts glow (unlit), spat seeds are lit.
    for s in g.shots.iter().filter(|s| s.room == r) {
        let (x, y) = (s.pos[0].round(), top + s.pos[1].round());
        if s.friendly {
            c.frame_c(cl.bolt, x, y, WHITE, layer::FX);
        } else {
            c.frame_c(cl.seed, x, y, WHITE, layer::ACTORS);
        }
    }
    // The hero.
    let pl = &g.player;
    let d = [pl.pos[0] - pl.prev[0], pl.pos[1] - pl.prev[1]];
    let pos = if d[0].abs() + d[1].abs() > 32.0 { pl.pos } else { [pl.prev[0] + d[0] * alpha, pl.prev[1] + d[1] * alpha] };
    let moving = d[0].abs() + d[1].abs() > 0.01;
    let blink = pl.invuln > 0.0 && (g.tick / 4).is_multiple_of(2);
    if g.phase != Phase::Title {
        let (x, y) = (pos[0].round(), top + pos[1].round());
        let fi = pl.facing as usize;
        let f = cl.hero[fi][if moving { ((g.tick / 8) % 2) as usize } else { 0 }];
        let feet = y + t.hero_h * 0.5 + 1.0;
        shadow(c, a, t, x, feet - 1.0, 1);
        if !blink {
            c.frame(f, x - (f.w * 0.5).floor(), feet - f.h, WHITE, layer::ACTORS);
        }
        // Sword and a fading arc along the facing while swinging.
        if pl.swing > 0.0 {
            let dir = pl.facing.dir();
            let k = (pl.swing / t.sword_time_s.max(0.01)).clamp(0.0, 1.0);
            let off = t.hero_h * 0.5 + t.sword_reach * 0.5;
            c.frame_c(cl.arc[fi], x + dir[0] * off, y + dir[1] * off, a.col.lin(art::WHITE, 3, 0.55 * k), layer::FX);
            let sw = cl.sword[fi];
            let so = t.hero_h * 0.5 + (sw.w.max(sw.h) * 0.5).floor() - 1.0;
            c.frame_c(sw, x + dir[0] * so, y + dir[1] * so, WHITE, layer::BLADE);
        }
    }
    // Floating text, with a 1 px ink drop for the floor behind it.
    for p in &g.pops {
        let rise = p.t * 16.0;
        let k = (1.0 - p.t / t.pop_life_s).clamp(0.0, 1.0);
        let (x, y) = (p.pos[0], top + p.pos[1] - 8.0 - rise);
        c.text_c(p.kind.text(), x, y.round() + 1.0, 1.0, a.col.lin(art::INK, 0, k), layer::FX);
        c.text_c(p.kind.text(), x, y.round(), 1.0, a.col.lin(art::BRASS, 3, k), layer::FX);
    }
}

fn hud(c: &mut Ctx, a: &Art, g: &Game, t: &Tune, h: f32) {
    let (col, cl) = (a.col, a.cells);
    c.rect(0.0, 0.0, ROOM_PX_W, h, col.lin(art::INK, 0, 1.0), layer::HUD);
    c.rect(0.0, h - 1.0, ROOM_PX_W, 1.0, col.lin(art::INK, 2, 1.0), layer::HUD);
    let pl = &g.player;
    // Hearts: two health per heart; the heart just hit flashes its old fill.
    let n = (pl.max_hp + 1) / 2;
    let flash = pl.invuln > 0.0 && (g.tick / 4).is_multiple_of(2);
    for i in 0..n {
        let (cx, row) = (i % HEARTS_PER_ROW, i / HEARTS_PER_ROW);
        let mut fill = (pl.hp - i * 2).clamp(0, 2);
        if flash && i == pl.hp / 2 && fill < 2 {
            fill += 1;
        }
        let (x, y) = (t.hud_hearts_x + cx as f32 * t.hud_heart_step, t.hud_hearts_y + row as f32 * t.hud_heart_step);
        c.frame(cl.hud_heart[fill as usize], x, y, WHITE, layer::HUD);
    }
    // Keys and gems.
    let num = col.lin(art::WHITE, 3, 1.0);
    let iy = t.hud_icons_y;
    c.frame(cl.hud_key, t.hud_keys_x, iy, WHITE, layer::HUD);
    c.text(&pl.keys.to_string(), t.hud_keys_x + cl.hud_key.w + 2.0, iy + 2.0, 1.0, num, layer::HUD);
    c.frame(cl.hud_gem, t.hud_gems_x, iy, WHITE, layer::HUD);
    c.text(&pl.gems.to_string(), t.hud_gems_x + cl.hud_gem.w + 2.0, iy + 2.0, 1.0, num, layer::HUD);
    // The item slot: the wand once found.
    let (sx, sy) = (t.hud_slot_x, t.hud_slot_y);
    c.rect(sx, sy, 18.0, 18.0, col.lin(art::BRASS, 0, 1.0), layer::HUD);
    c.rect(sx + 1.0, sy + 1.0, 16.0, 16.0, col.lin(art::INK, 1, 1.0), layer::HUD);
    if pl.has_wand {
        c.frame_c(cl.wand, sx + 9.0, sy + 9.0, WHITE, layer::HUD);
    }
    if g.free_roam {
        c.text_c("FREE ROAM", ROOM_PX_W * 0.5, t.hud_label_y, 1.0, col.lin(art::GLOW, 3, 1.0), layer::HUD);
    } else if let Some(w) = g.enemies.iter().find(|e| e.kind == EnemyKind::Warden && e.room == pl.room && e.hp > 0) {
        // The Warden's health: a 2 px bar along the bottom of the strip.
        let frac = (w.hp as f32 / g.warden_max_hp.max(1) as f32).clamp(0.0, 1.0);
        let (bx, bw) = (t.hud_hearts_x, ROOM_PX_W - 2.0 * t.hud_hearts_x);
        c.rect(bx, h - 3.0, bw, 2.0, col.lin(art::RED, 0, 1.0), layer::HUD);
        c.rect(bx, h - 3.0, (bw * frac).round(), 2.0, col.lin(art::RED, 2, 1.0), layer::HUD);
    }
    // The room map: one square per visited room by its grid place; the
    // current room bright, the boss room red.
    let (mut x0, mut y0) = (i32::MAX, i32::MAX);
    for r in &g.level.rooms {
        x0 = x0.min(r.gx);
        y0 = y0.min(r.gy);
    }
    let (cw, ch) = (9.0, 6.0);
    for (i, r) in g.level.rooms.iter().enumerate() {
        if !g.visited.get(i).copied().unwrap_or(false) {
            continue;
        }
        let boss = r.enemies.iter().any(|e| e.kind == EnemyKind::Warden);
        let fill = if i == g.room() {
            col.lin(art::BRASS, 3, 1.0)
        } else if boss {
            col.lin(art::RED, 2, 1.0)
        } else {
            col.lin(art::FLOOR, 1, 1.0)
        };
        let (x, y) = (t.hud_map_x + (r.gx - x0) as f32 * cw, t.hud_map_y + (r.gy - y0) as f32 * ch);
        c.rect(x, y, cw - 1.0, ch - 1.0, fill, layer::HUD);
    }
}

fn banners(c: &mut Ctx, a: &Art, g: &Game, t: &Tune, top: f32) {
    let cx = ROOM_PX_W * 0.5;
    let mid = top + ROOM_PX_H * 0.5;
    let blink = (g.tick / 30).is_multiple_of(2);
    let lines: Option<(&str, bool)> = match g.phase {
        Phase::Title => Some(("MOSSGATE KEEP", true)),
        Phase::Paused => Some(("PAUSED", false)),
        Phase::Won => Some(("YOU WIN", g.end_wait <= 0.0)),
        Phase::Lost => Some(("GAME OVER", g.end_wait <= 0.0)),
        Phase::Playing => None,
    };
    let Some((title, prompt)) = lines else {
        return;
    };
    let col = a.col;
    c.rect(0.0, top, ROOM_PX_W, ROOM_PX_H, col.lin(art::INK, 0, 0.6), layer::BANNER);
    // The see-through dim lets floor cracks show between the letters; a
    // near-opaque band behind each line keeps the lettering clean.
    let band = col.lin(art::INK, 0, t.banner_band_alpha);
    let glyph_h = font::HEIGHT as f32;
    let band_row = |c: &mut Ctx, text: &str, y: f32, px: f32| {
        let w = font::width(text) as f32 * px + 4.0 * px;
        c.rect((cx - w * 0.5).round(), y - 2.0 * px, w.round(), (glyph_h + 4.0) * px, band, layer::BANNER);
    };
    band_row(c, title, mid - 16.0, 3.0);
    c.text_c(title, cx, mid - 15.0, 3.0, col.lin(art::INK, 0, 1.0), layer::BANNER);
    c.text_c(title, cx, mid - 16.0, 3.0, col.lin(art::BRASS, 2, 1.0), layer::BANNER);
    if prompt && blink {
        band_row(c, "PRESS ATTACK", mid + 8.0, 2.0);
        c.text_c("PRESS ATTACK", cx, mid + 9.0, 2.0, col.lin(art::INK, 0, 1.0), layer::BANNER);
        c.text_c("PRESS ATTACK", cx, mid + 8.0, 2.0, col.lin(art::WHITE, 3, 1.0), layer::BANNER);
    }
}
