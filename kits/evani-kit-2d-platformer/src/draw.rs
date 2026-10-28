//! The `Frame` system: the 2D camera, the art (built lazily per look into
//! the [`SpriteSheet`] resource), every sprite, the kit's pixel HUD, the
//! HUD resource and the sounds.
//!
//! Screen mapping: the renderer's target size in device pixels gives the
//! scale (device pixels per art pixel). With `render.pixel_art` on it is a
//! whole number: the smallest that shows at most `view_height_max_px` rows,
//! `ceil(target_h / view_height_max_px)`, or `floor(target_h /
//! view_height_px)` when that would show fewer than `view_height_px` rows;
//! no more than fits 256 art pixels across (a portrait phone still sees
//! ahead) and at least 1. A view wider than `view_width_max_px` is framed
//! (centred, bars at the sides), and one taller than `view_height_max_px`
//! (a phone held upright) is framed with bars above and below. The camera and every sprite snap to whole
//! art pixels, and each axis maps by its own CSS-to-device ratio, so every
//! art pixel is exactly `scale` device pixels square. Off, the scale is
//! fractional and nothing snaps.
//!
//! Layers, back to front: sky, far, mid and near planes (only while the
//! `parallax` effect is on; a flat sky without it), castle walls,
//! decorations (with the flags, gate and torches), tiles, moving
//! platforms, coins, drop shadows, enemies, the hero, water, pops, HUD.
//! While the game is on its title screen the attract demo is drawn instead
//! (silent).

use evani_core::audio::AudioMixer;
use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::time::Time;
use evani_render::{RenderWorld, Sprite, SpriteSheet};

use crate::art::{Art, ArtData, Colors, DECOS, Frame, SHADOW_MAX_W, SHADOW_MIN_W, TileArt, pose};
use crate::font;
use crate::game::{EnemyKind, Game, Phase, PopKind};
use crate::hud::PlatformerHud;
use crate::level::{TILE, cell};
use crate::sfx::{Sfx, Sounds};
use crate::{ART_JSON, Platformer, PlatformerKit, Tune};

/// Sprite layers.
pub mod layer {
    pub const SKY: i16 = 0;
    pub const FAR: i16 = 10;
    pub const MID: i16 = 20;
    pub const NEAR: i16 = 30;
    pub const CASTLE: i16 = 40;
    pub const DECO: i16 = 50;
    pub const TILES: i16 = 60;
    pub const PLATFORMS: i16 = 65;
    pub const COINS: i16 = 70;
    pub const SHADOWS: i16 = 75;
    pub const ENEMIES: i16 = 80;
    pub const HERO: i16 = 90;
    pub const WATER: i16 = 95;
    pub const POPS: i16 = 100;
    /// The "LEVEL CLEAR!" banner (under the HUD, outside its layers).
    pub const BANNER: i16 = 150;
    pub const HUD: i16 = 200;
    pub const BARS: i16 = 250;
}

/// Narrowest view (art pixels) the whole-pixel scale keeps.
pub const MIN_VIEW_W: f32 = 256.0;
/// The bars beside a framed view.
pub const BAR_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// How much of the world a screen shows, in art pixels (from [`Tune`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewRule {
    /// Fewest rows (a screen shorter than this many device pixels shows less).
    pub min_h: f32,
    /// Most rows the whole-pixel scale allows.
    pub max_h: f32,
    /// Most columns; a wider screen is framed with bars.
    pub max_w: f32,
}

impl ViewRule {
    pub fn of(t: &Tune) -> Self {
        Self { min_h: t.view_height_px, max_h: t.view_height_max_px, max_w: t.view_width_max_px }
    }
}

/// Per-frame state of the draw system (resource).
#[derive(Default)]
pub struct DrawState {
    data: Option<ArtData>,
    /// Look the art was built for (name, palette count).
    look_key: Option<(String, usize)>,
    pub art: Option<Art>,
    revision: u64,
    sounds: Option<Sounds>,
    /// The last frame's screen mapping (tests and tools read it).
    pub screen: Screen,
}

/// How art pixels map to the canvas this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Screen {
    /// Device pixels per art pixel.
    pub scale: f32,
    /// CSS pixels per device pixel, per axis (the canvas's CSS size over
    /// its backing size; the sprite pass works in CSS pixels).
    pub css_per_px: [f32; 2],
    /// The canvas's size in device pixels.
    pub target: [f32; 2],
    /// The view's top-left on the canvas in whole device pixels (non-zero
    /// when a wide screen frames the view).
    pub origin: [f32; 2],
    /// View size in art pixels.
    pub view: [f32; 2],
    /// Top-left of the view in world art pixels.
    pub cam: [f32; 2],
    /// Snap to whole art pixels.
    pub snap: bool,
}

impl Screen {
    /// Fit the view into a canvas of `target` device pixels whose CSS size
    /// is `css`.
    ///
    /// Pixel art: the smallest whole scale that shows at most `max_h` rows,
    /// `s = ceil(h / max_h)`, unless that shows fewer than `min_h` rows, then
    /// `s = floor(h / min_h)`; never so big that fewer than 256 columns
    /// show, never below 1. A view wider than `max_w` is centred on whole
    /// device pixels with bars at the sides, and one taller than `max_h`
    /// (a portrait screen, where the 256 columns set the scale) with bars
    /// above and below. Smooth looks: `h / min_h`, fractional.
    pub fn fit(target: [f32; 2], css: [f32; 2], rule: ViewRule, pixel_art: bool) -> Self {
        let target = target.map(|v| v.max(1.0));
        let (min_h, max_h) = (rule.min_h.max(1.0), rule.max_h.max(1.0));
        let scale = if pixel_art {
            let mut s = (target[1] / max_h).ceil();
            if target[1] / s < min_h {
                s = (target[1] / min_h).floor();
            }
            s.min((target[0] / MIN_VIEW_W).floor()).max(1.0)
        } else {
            (target[1] / min_h).min(target[0] / MIN_VIEW_W).max(0.25)
        };
        let mut view = [target[0] / scale, target[1] / scale];
        let mut origin = [0.0; 2];
        let max_w = rule.max_w.max(MIN_VIEW_W);
        if view[0] > max_w {
            view[0] = max_w;
            origin[0] = ((target[0] - max_w * scale) * 0.5).floor();
        }
        // A tall screen (a phone held upright): the 256-column floor keeps
        // the scale small, so the view would show far more than `max_h`
        // rows, a thin strip of play under a tall sky. It is framed instead:
        // `max_h` rows, centred on whole device pixels with bars above and
        // below (the touch controls and the pause button sit in the bars).
        if view[1] > max_h {
            view[1] = max_h;
            origin[1] = ((target[1] - max_h * scale) * 0.5).floor();
        }
        let css_per_px = [0, 1].map(|i| if css[i] > 0.0 { css[i] / target[i] } else { 1.0 });
        Self { scale, css_per_px, target, origin, view, cam: [0.0; 2], snap: pixel_art }
    }

    /// Snap to the art grid with halves rounded up (`floor(v + 0.5)`), the
    /// same way at every position: Rust's `round` sends halves away from
    /// zero, so -1.5 and 510.5 (one strip width apart) went to -2 and 511
    /// and left a one-pixel gap between two copies of a parallax strip.
    fn px(&self, v: f32) -> f32 {
        if self.snap { (v + 0.5).floor() } else { v }
    }

    /// A sprite covering device-pixel rect `(x, y, w, h)` of the canvas.
    #[allow(clippy::too_many_arguments)]
    fn sprite_device(&self, x: f32, y: f32, w: f32, h: f32, cell: u16, color: [f32; 4], layer: i16) -> Sprite {
        let [kx, ky] = self.css_per_px;
        Sprite {
            pos: [(x + w.abs() * 0.5) * kx, (y + h * 0.5) * ky],
            size: [w * kx, h * ky],
            rotation: 0.0,
            cell,
            color,
            layer,
        }
    }

    /// A sprite covering screen art-pixel rect `(x, y, w, h)`.
    #[allow(clippy::too_many_arguments)]
    fn sprite_screen(&self, x: f32, y: f32, w: f32, h: f32, cell: u16, color: [f32; 4], layer: i16) -> Sprite {
        let (x, y, s) = (self.px(x), self.px(y), self.scale);
        self.sprite_device(self.origin[0] + x * s, self.origin[1] + y * s, w * s, h * s, cell, color, layer)
    }

    /// A sprite's rect `[x, y, w, h]` back in screen art pixels (tests and
    /// tools check the grid with it).
    pub fn art_rect(&self, sp: &Sprite) -> [f32; 4] {
        let s = self.scale;
        let (w, h) = (sp.size[0].abs() / self.css_per_px[0] / s, sp.size[1] / self.css_per_px[1] / s);
        let x = (sp.pos[0] / self.css_per_px[0] - self.origin[0]) / s - w * 0.5;
        let y = (sp.pos[1] / self.css_per_px[1] - self.origin[1]) / s - h * 0.5;
        [x, y, w, h]
    }

    /// The bars beside, above and below a framed view, in whole device
    /// pixels (none when the view fills the canvas).
    pub fn bars(&self) -> Vec<[f32; 4]> {
        let right = self.origin[0] + self.view[0] * self.scale;
        let bottom = self.origin[1] + self.view[1] * self.scale;
        let [tw, th] = self.target;
        [[0.0, 0.0, self.origin[0], th], [right, 0.0, tw - right, th], [0.0, 0.0, tw, self.origin[1]], [0.0, bottom, tw, th - bottom]]
            .into_iter()
            .filter(|b| b[2] >= 0.5 && b[3] >= 0.5)
            .collect()
    }

    /// A sprite with its top-left at world `(x, y)`.
    #[allow(clippy::too_many_arguments)]
    fn sprite_world(&self, x: f32, y: f32, w: f32, h: f32, cell: u16, color: [f32; 4], layer: i16) -> Sprite {
        self.sprite_screen(x - self.cam[0], y - self.cam[1], w, h, cell, color, layer)
    }

    /// Is world rect `(x, y, w, h)` on screen?
    fn visible(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        x + w > self.cam[0] - 2.0 && x < self.cam[0] + self.view[0] + 2.0 && y + h > self.cam[1] - 2.0 && y < self.cam[1] + self.view[1] + 2.0
    }
}

const WHITE: [f32; 4] = [1.0; 4];

/// A smooth, cheap noise in -1..1 for shake.
fn wobble(t: f32, seed: f32) -> f32 {
    (t * 1.0 + seed * 1.7).sin() * 0.6 + (t * 2.3 + seed * 4.1).sin() * 0.4
}

/// What the frame draws, gathered before borrowing the render world.
struct Look {
    parallax: Option<[f32; 4]>,
    shake: Option<(f32, f32)>,
}

/// The `Frame` system.
pub fn draw_system(world: &mut World) {
    let Some(kit) = world.try_res::<PlatformerKit>().map(|k| *k) else {
        return;
    };
    let (alpha, time) = {
        let tm = world.res::<Time>();
        (tm.alpha, tm.render_seconds())
    };
    let (t, pixel_art, fx) = {
        let reg = world.res::<ParamRegistry>();
        let status = world.try_res::<EffectsStatus>();
        let on = |n: &str| status.as_ref().is_some_and(|s| s.is_on(n));
        let parallax = on("parallax").then(|| {
            [
                reg.f32_or("fx.parallax.far", 0.15),
                reg.f32_or("fx.parallax.mid", 0.4),
                reg.f32_or("fx.parallax.near", 0.7),
                reg.f32_or("fx.parallax.vertical", 0.3),
            ]
        });
        let shake = on("shake").then(|| (reg.f32_or("fx.shake.amount", 0.35), reg.f32_or("fx.shake.frequency", 18.0)));
        (kit.tune.read(&reg), reg.bool_or("render.pixel_art"), Look { parallax, shake })
    };
    let mut st = world.res_mut::<DrawState>();
    // The art, once per look.
    {
        let look = world.res::<LookData>();
        let key = Some((look.name.clone(), look.palettes.len()));
        if st.art.is_none() || st.look_key != key {
            if st.data.is_none() {
                match ArtData::from_json(ART_JSON) {
                    Ok(d) => st.data = Some(d),
                    Err(_) => return,
                }
            }
            st.revision += 1;
            let revision = st.revision;
            let Some(data) = st.data.as_ref() else {
                return;
            };
            let p = world.res::<Platformer>();
            let colors = Colors::new(data, Some(&look));
            let mut art = Art::build(data, &p.game.level, &colors, revision);
            *world.res_mut::<SpriteSheet>() = core::mem::take(&mut art.sheet);
            st.art = Some(art);
            st.look_key = key;
        }
    }
    if st.sounds.is_none()
        && let Some(mut mixer) = world.try_res_mut::<AudioMixer>()
    {
        st.sounds = Some(Sounds::register(&mut mixer));
    }
    let mut p = world.res_mut::<Platformer>();
    // Sounds from the player's game only (the attract demo is silent).
    let events = core::mem::take(&mut p.game.events);
    if let (Some(sounds), Some(mut mixer)) = (st.sounds.as_ref(), world.try_res_mut::<AudioMixer>()) {
        for e in events {
            sounds.play(&mut mixer, Sfx::of(e), 1.0);
        }
    }
    world.res_mut::<PlatformerHud>().update(&p.game);
    let attract = p.game.phase == Phase::Title;
    // A held capture moment animates by the game's own tick (clouds, coin
    // spin, water, torches, shake), so every capture of it is identical.
    #[cfg(feature = "showcase")]
    let time = if !attract && p.show == Some(crate::Show::Still) { p.game.tick as f64 / 60.0 } else { time };
    let alpha = if attract {
        let a = &mut p.attract;
        a.seek(time)
    } else {
        alpha
    };
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else {
        return;
    };
    // The HUD is drawn in the canvas on the art grid (hud-layout.md); the
    // review's captures check reads it from this layer.
    rw.hud_layers = Some([layer::HUD, layer::HUD]);
    let css = if rw.viewport_css[0] > 0.0 && rw.viewport_css[1] > 0.0 { rw.viewport_css } else { [1280.0, 720.0] };
    let target = if rw.target_px[0] > 0 && rw.target_px[1] > 0 {
        [rw.target_px[0] as f32, rw.target_px[1] as f32]
    } else {
        css
    };
    let mut screen = Screen::fit(target, css, ViewRule::of(&t), pixel_art);
    let g = if attract { &p.attract.game } else { &p.game };
    let Some(art) = st.art.as_ref() else {
        return;
    };
    screen.cam = camera(g, &screen, &t, alpha);
    // Shake in whole art pixels.
    if let Some((amount, freq)) = fx.shake
        && g.trauma > 0.0
    {
        let s = g.trauma * g.trauma * amount * TILE;
        let ft = (time % 3600.0) as f32 * freq;
        screen.cam[0] += (s * wobble(ft, 1.0)).round();
        screen.cam[1] += (s * wobble(ft, 2.0)).round();
    }
    let tf = (time % 3600.0) as f32;
    let d = Drawer { s: screen, art, g, t: &t, alpha, tf };
    d.background(fx.parallax, &mut rw);
    d.level(&mut rw);
    d.things(&mut rw);
    d.actors(&mut rw);
    d.pops(&mut rw);
    // The attract demo shows its HUD as well (an arcade attract mode).
    d.hud(&mut rw);
    if !attract && g.phase == Phase::Won {
        d.banner(&mut rw);
    }
    // A framed view's bars cover whatever runs past its sides.
    for [x, y, w, h] in screen.bars() {
        rw.draw_sprite(screen.sprite_device(x, y, w, h, art.white.cell, BAR_COLOR, layer::BARS));
    }
    drop(rw);
    drop(p);
    st.screen = screen;
}

/// The view's top-left in world art pixels: the game camera's focus
/// (interpolated) centred in x and clamped to the level's sides; its ground
/// line placed at `cam_ground_frac` of the view height. The view may rise
/// above the level's top (sky) but never shows below its bottom.
pub fn camera(g: &Game, s: &Screen, t: &Tune, alpha: f32) -> [f32; 2] {
    let c = &g.cam;
    let f = [c.prev[0] + (c.focus[0] - c.prev[0]) * alpha, c.prev[1] + (c.focus[1] - c.prev[1]) * alpha];
    let (lw, lh) = (g.level.width_px(), g.level.height_px());
    let (vw, vh) = (s.view[0], s.view[1]);
    // num::clamp: f32::clamp's panic text would ship float formatting.
    let x = if vw >= lw { (lw - vw) * 0.5 } else { evani_core::num::clamp(f[0] - vw * 0.5, 0.0, lw - vw) };
    let y = (f[1] - vh * t.cam_ground_frac).min(lh - vh);
    [s.px(x), s.px(y)]
}

/// A rect on screen in art pixels: top-left and size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl TextBox {
    /// Do `self` and `o` come closer than `gap` pixels?
    pub fn hits(&self, o: &TextBox, gap: f32) -> bool {
        self.x < o.x + o.w + gap && o.x < self.x + self.w + gap && self.y < o.y + o.h + gap && o.y < self.y + self.h + gap
    }
}

/// Clear pixels kept between floating text and the hero or other text.
pub const TEXT_GAP: f32 = 1.0;
/// How far floating text rises over its life (art px).
pub const TEXT_RISE: f32 = 12.0;
/// The hero's drawn box around its feet (outlined 16 px frame, with room
/// for the landing squash and the run bob).
pub const HERO_BOX: [f32; 2] = [16.0, 23.0];
/// HUD font glyph advance (art px).
const HUD_ADV: f32 = 7.0;
/// Mini font: glyph advance and outlined glyph frame size.
const MINI_ADV: f32 = 4.0;
const MINI_FRAME: [f32; 2] = [6.0, 8.0];
/// The HUD label in free roam (a space is an empty advance).
pub const FREE_ROAM_LABEL: &str = "FREE ROAM";

/// The hero's light (glow and burst) is centred this far above its feet.
pub const HERO_LIGHT_UP: f32 = 10.0;
/// Ends of the extra-life glow's three steps (seconds after the 1UP),
/// strongest first; after the last, no glow.
pub const GLOW_STEPS: [f32; 3] = [0.3, 0.45, 0.6];
/// Ends of the burst's three frames (seconds after the 1UP).
pub const BURST_STEPS: [f32; 3] = [0.08, 0.17, 0.27];
/// The glow's opacity (its fade is in the dither, not here).
const GLOW_ALPHA: f32 = 0.85;

/// The hero's box on screen (art px).
pub fn hero_box(g: &Game, cam: [f32; 2], alpha: f32) -> TextBox {
    let p = lerp2(g.hero.prev, g.hero.body.pos, alpha);
    TextBox { x: (p[0] - cam[0] - HERO_BOX[0] * 0.5).round(), y: (p[1] - cam[1] - HERO_BOX[1] + 2.0).round(), w: HERO_BOX[0], h: HERO_BOX[1] }
}

/// Lay out floating text (Chris's review, point 3), in screen space: each
/// box keeps its x and moves only up until it is `TEXT_GAP` clear of the
/// hero and of every box placed before it (oldest first, so a newer pop
/// stacks above the older ones). Whole pixels in, whole pixels out.
pub fn layout_text(boxes: &[TextBox], hero: Option<TextBox>) -> Vec<TextBox> {
    let mut placed: Vec<TextBox> = Vec::with_capacity(boxes.len());
    for b in boxes {
        let mut b = *b;
        // Moving up past an obstacle never brings it back, so this ends
        // after at most one move per obstacle.
        for _ in 0..=boxes.len() + 1 {
            let hit = hero.iter().chain(placed.iter()).find(|o| b.hits(o, TEXT_GAP)).copied();
            match hit {
                Some(o) => b.y = o.y - TEXT_GAP - b.h,
                None => break,
            }
        }
        placed.push(b);
    }
    placed
}

/// One floating text to draw: its text, placed box and opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPop {
    pub text: String,
    pub at: TextBox,
    pub alpha: f32,
}

/// The game's floating text this frame on a screen whose top-left is `cam`
/// (art px): eased rise (fast, then settling), full colour for 55 % of the
/// life, then a smooth fade, laid out clear of the hero and of each other.
pub fn text_pops(g: &Game, cam: [f32; 2], view_w: f32, alpha: f32) -> Vec<TextPop> {
    let mut texts = Vec::new();
    let mut boxes = Vec::new();
    for pop in &g.pops {
        let text = match pop.kind {
            PopKind::Score(n) => n.to_string(),
            PopKind::OneUp => "1UP".to_string(),
            _ => continue,
        };
        let u = (pop.t / pop.kind.life()).clamp(0.0, 1.0);
        let rise = TEXT_RISE * (1.0 - (1.0 - u).powi(3));
        let w = (text.len() as f32 - 1.0) * MINI_ADV + MINI_FRAME[0];
        let h = MINI_FRAME[1];
        let x = evani_core::num::clamp((pop.pos[0] - cam[0] - w * 0.5).round(), 1.0, (view_w - w - 1.0).max(1.0));
        let y = (pop.pos[1] - cam[1] - h * 0.5 - rise).round();
        let f = ((u - 0.55) / 0.45).clamp(0.0, 1.0);
        texts.push((text, 1.0 - f * f * (3.0 - 2.0 * f)));
        boxes.push(TextBox { x, y, w, h });
    }
    let hero = g.hero.alive().then(|| hero_box(g, cam, alpha));
    layout_text(&boxes, hero).into_iter().zip(texts).map(|(at, (text, alpha))| TextPop { text, at, alpha }).collect()
}

/// The top edge, in whole world art pixels, of a sprite `h` pixels tall
/// centred on `y`. An odd height can't centre on a whole-pixel point, so the
/// spare half pixel goes above: a coin at its tile's centre then keeps
/// 2 px of clear air over the ground of the tile below (13 px coin in a
/// 16 px tile: rows 1-13, ground at 16).
pub fn centred_top(h: f32, y: f32) -> f32 {
    (y - h * 0.5).floor()
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

struct Drawer<'a> {
    s: Screen,
    art: &'a Art,
    g: &'a Game,
    t: &'a Tune,
    alpha: f32,
    tf: f32,
}

impl Drawer<'_> {
    /// Draw `f` with its bottom centre at world `(x, y)`; `flip` mirrors it.
    #[allow(clippy::too_many_arguments)]
    fn at_feet(&self, rw: &mut RenderWorld, f: Frame, x: f32, y: f32, flip: bool, color: [f32; 4], layer: i16) {
        let (w, h) = (f.w, f.h);
        let x0 = self.s.px(x - w * 0.5);
        if !self.s.visible(x0, y - h, w, h) {
            return;
        }
        let ww = if flip { -w } else { w };
        rw.draw_sprite(self.s.sprite_world(x0, y - h, ww, h, f.cell, color, layer));
    }

    /// Draw `f` centred on world `(x, y)` (top edge from [`centred_top`]).
    #[allow(clippy::too_many_arguments)]
    fn centred(&self, rw: &mut RenderWorld, f: Frame, x: f32, y: f32, flip: bool, color: [f32; 4], layer: i16) {
        self.at_feet(rw, f, x, centred_top(f.h, y) + f.h, flip, color, layer);
    }

    #[allow(clippy::too_many_arguments)]
    fn fill(&self, rw: &mut RenderWorld, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], layer: i16) {
        if w > 0.0 && h > 0.0 {
            rw.draw_sprite(self.s.sprite_screen(x, y, w, h, self.art.white.cell, color, layer));
        }
    }

    /// Sky bands and the three parallax planes (or a flat sky).
    fn background(&self, parallax: Option<[f32; 4]>, rw: &mut RenderWorld) {
        let a = self.art;
        // Whole art pixels: the last, partly shown row or column is filled
        // too (the canvas edge cuts it), so every fill stays on the grid.
        let (vw, vh) = (self.s.view[0].ceil(), self.s.view[1].ceil());
        let Some([far, mid, near, vertical]) = parallax else {
            let c = a.sky.get(a.sky.len() / 2).copied().unwrap_or([0.5, 0.7, 1.0, 1.0]);
            self.fill(rw, 0.0, 0.0, vw, vh, c, layer::SKY);
            return;
        };
        // Planes sit relative to the camera resting on the start's ground
        // (that ground at `cam_ground_frac`): their bases at fixed offsets
        // from the ground line, the far hills well above it, the near
        // bushes just under it (critic round 2, item 1: the view no longer
        // rests on the level's bottom, so the backdrop follows the framing).
        let ground = self.t.cam_ground_frac;
        let rest_y = self.g.level.start[1] - vh * ground;
        let dy = self.s.cam[1] - rest_y;
        let bottoms = [vh * (ground - 0.18), vh * (ground - 0.06), vh * (ground + 0.04)];
        let factors = [far, mid, near];
        let layers = [layer::FAR, layer::MID, layer::NEAR];
        let plane_y = |i: usize| bottoms[i] - dy * factors[i] * vertical;
        // Sky: bands from the top down to the far plane's top.
        let horizon = (plane_y(0) - a.planes[0].h * 0.5).max(8.0);
        let n = a.sky.len().max(1);
        let band = (horizon / n as f32).ceil();
        // Each band edge is an ordered dither (look bible: "a six-band
        // dithered blue sky"; critic round 2, item 4): the upper band runs
        // on half the dither's rows past its edge and the lower band's
        // colour is dithered over that, from 1 in 16 pixels to 15 in 16.
        let dh = crate::art::SKY_DITHER_H as f32;
        let half = if band >= dh + 2.0 { dh * 0.5 } else { 0.0 };
        let edge = |i: usize| self.s.px(i as f32 * band);
        for (i, c) in a.sky.iter().enumerate() {
            let y = if i == 0 { 0.0 } else { edge(i) + half };
            let h = if i + 1 == n { vh - y } else { edge(i + 1) + half - y };
            self.fill(rw, 0.0, y, vw, h, *c, layer::SKY);
            if i > 0 && half > 0.0 {
                let f = a.sky_dither;
                let mut x = 0.0;
                while x < vw {
                    rw.draw_sprite(self.s.sprite_screen(x, edge(i) - half, f.w, f.h, f.cell, *c, layer::SKY + 1));
                    x += f.w;
                }
            }
        }
        // Clouds drift in the far plane.
        let cam_x = self.s.cam[0];
        let period = vw + 96.0;
        for i in 0..self.g.level.clouds {
            let f = a.clouds[(i % 2) as usize];
            let h = crate::art::hash(i as i32, 7, 11);
            let base = (h % 997) as f32 / 997.0 * period;
            let x = (base - cam_x * far * 0.6 - self.tf * (3.0 + (h % 5) as f32)).rem_euclid(period) - 48.0;
            let y = 6.0 + ((h >> 10) % 40) as f32 - dy * far * vertical * 0.5;
            rw.draw_sprite(self.s.sprite_screen(x, y, f.w, f.h, f.cell, WHITE, layer::FAR));
        }
        for i in 0..3 {
            let f = a.planes[i];
            // The near plane's mist and valley haze hang below its bushes' base.
            let hang = if i == 2 { (crate::art::MIST + crate::art::HAZE) as f32 } else { 0.0 };
            let bottom = plane_y(i) + hang;
            let top = self.s.px(bottom - f.h);
            // The plane's scroll, snapped once, so every copy sits a whole
            // strip width from the last: snapping each copy on its own could
            // round two copies apart (in f32, 512 - 25.5000019 is 486.5).
            let scroll = self.s.px(cam_x * factors[i]);
            let off = scroll.rem_euclid(f.w);
            let mut x = -off;
            while x < vw {
                rw.draw_sprite(self.s.sprite_screen(x, top, f.w, f.h, f.cell, WHITE, layers[i]));
                x += f.w;
            }
            if i == 0 {
                // The far castle: once every two strips, so a wide view
                // shows it once and it is far off screen at the goal.
                let c = a.far_castle;
                let period = f.w * 2.0;
                // From the same snapped scroll, so the castle keeps its
                // place on the far hills.
                let mut cx = a.far_castle_at[0] - scroll.rem_euclid(period);
                if cx < -c.w {
                    cx += period;
                }
                if cx < vw {
                    rw.draw_sprite(self.s.sprite_screen(cx, top + a.far_castle_at[1], c.w, c.h, c.cell, WHITE, layer::FAR + 1));
                }
            }
            let fy = top + f.h;
            self.fill(rw, 0.0, fy, vw, vh - fy, a.plane_fill[i], layers[i]);
        }
    }

    /// Tiles on screen: walls behind, solids, water (cycled, in front).
    fn level(&self, rw: &mut RenderWorld) {
        let s = &self.s;
        let (tx0, tx1) = (cell(s.cam[0]), cell(s.cam[0] + s.view[0]));
        let (ty0, ty1) = (cell(s.cam[1]).max(0), cell(s.cam[1] + s.view[1]));
        let frame = (self.tf * self.t.cycle_fps.max(0.0)) as u16 % 4;
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let (cell, layer) = match self.art.tile(tx, ty) {
                    TileArt::None => continue,
                    TileArt::Still(c) => (c, layer::TILES),
                    TileArt::Back(c) => (c, layer::CASTLE),
                    TileArt::Cycle(c) => (c + frame, layer::WATER),
                };
                let (x, y) = (tx as f32 * TILE, ty as f32 * TILE);
                rw.draw_sprite(s.sprite_world(x, y, TILE, TILE, cell, WHITE, layer));
            }
        }
    }

    /// Decorations, flags, the gate, torches, platforms and coins.
    fn things(&self, rw: &mut RenderWorld) {
        let a = self.art;
        let l = &self.g.level;
        for d in &l.decorations {
            if let Some(i) = DECOS.iter().position(|k| *k == d.kind) {
                // Tucked one pixel into the ground.
                self.at_feet(rw, a.deco[i], d.pos[0], d.pos[1] + 2.0, false, WHITE, layer::DECO);
            }
        }
        for (i, f) in l.checkpoints.iter().enumerate() {
            let up = self.g.flags.get(i).copied().unwrap_or(false);
            self.at_feet(rw, a.flag[usize::from(up)], f[0] + 3.0, f[1] + 1.0, false, WHITE, layer::DECO);
        }
        self.at_feet(rw, a.gate, l.goal[0], l.goal[1], false, WHITE, layer::DECO);
        let frame = (self.tf * self.t.cycle_fps.max(0.0)) as usize % 4;
        for (i, p) in l.torches.iter().enumerate() {
            self.at_feet(rw, a.torch, p[0], p[1], false, WHITE, layer::DECO);
            let fl = a.flame[(frame + i) % 4];
            self.at_feet(rw, fl, p[0], p[1] - a.torch.h + 3.0, false, WHITE, layer::DECO + 1);
        }
        // Moving platforms.
        for (i, d) in l.platforms.iter().enumerate() {
            let (Some(now), Some(was)) = (self.g.plats.get(i), self.g.plats_prev.get(i)) else {
                continue;
            };
            let p = lerp2(*was, *now, self.alpha);
            let n = (d.width_px / TILE).round().max(1.0) as usize;
            let x0 = self.s.px(p[0]);
            for k in 0..n {
                let part = if k == 0 {
                    0
                } else if k + 1 == n {
                    2
                } else {
                    1
                };
                let f = a.platform[part];
                let x = x0 + k as f32 * TILE;
                if self.s.visible(x, p[1], f.w, f.h) {
                    rw.draw_sprite(self.s.sprite_world(x, p[1], f.w, f.h, f.cell, WHITE, layer::PLATFORMS));
                }
            }
        }
        // Coins spin: face, three-quarter, edge, three-quarter (mirrored).
        for (i, c) in l.coins.iter().enumerate() {
            if self.g.coins_taken.get(i).copied().unwrap_or(true) {
                continue;
            }
            let ph = (self.tf * 7.0 + c[0] * 0.03) as usize % 4;
            let (f, flip) = match ph {
                0 => (a.coin[0], false),
                1 => (a.coin[1], false),
                2 => (a.coin[2], false),
                _ => (a.coin[1], true),
            };
            self.centred(rw, f, c[0], c[1], flip, WHITE, layer::COINS);
        }
    }

    /// The ground under `(x, feet)` within 96 px (tiles and platforms).
    fn ground_below(&self, x: f32, hw: f32, feet: f32) -> Option<f32> {
        let l = &self.g.level;
        let mut y = l.surface(x - hw, x + hw, x, feet - 2.0, feet + 96.0, Some(feet)).map(|s| s.y);
        for (i, d) in l.platforms.iter().enumerate() {
            let Some(p) = self.g.plats.get(i) else {
                continue;
            };
            if x + hw > p[0] && x - hw < p[0] + d.width_px && p[1] >= feet - 2.0 && p[1] <= feet + 96.0 && y.is_none_or(|g| p[1] < g) {
                y = Some(p[1]);
            }
        }
        y
    }

    /// A drop shadow on the ground under a body, smaller and fainter the
    /// higher it is.
    fn shadow(&self, rw: &mut RenderWorld, x: f32, hw: f32, feet: f32) {
        let Some(gy) = self.ground_below(x, hw, feet) else {
            return;
        };
        let k = 1.0 - ((gy - feet).max(0.0) / 96.0).min(1.0);
        if k <= 0.05 {
            return;
        }
        // Whole widths only, each its own baked frame drawn 1:1.
        let w = ((2.0 * (hw + 2.0) * (0.45 + 0.55 * k)).round() as u32).clamp(SHADOW_MIN_W, SHADOW_MAX_W);
        let Some(f) = self.art.shadow.get((w - SHADOW_MIN_W) as usize).copied() else {
            return;
        };
        let (w, h) = (f.w, f.h);
        let mut c = self.art.shadow_color;
        c[3] = self.t.shadow_alpha * (0.35 + 0.65 * k);
        let x0 = self.s.px(x - w * 0.5);
        // One row above the surface, the rest on its top (grass or stone),
        // where the light ground shows the shadow best.
        let y0 = self.s.px(gy - 1.0);
        if self.s.visible(x0, y0, w, h) {
            rw.draw_sprite(self.s.sprite_world(x0, y0, w, h, f.cell, c, layer::SHADOWS));
        }
    }

    fn actors(&self, rw: &mut RenderWorld) {
        let a = self.art;
        for e in &self.g.enemies {
            if e.gone || e.squashed > 0.5 {
                continue;
            }
            let p = lerp2(e.prev, e.body.pos, self.alpha);
            let frames = match e.kind {
                EnemyKind::Beetle => a.beetle,
                EnemyKind::Hopper => a.hopper,
            };
            let f = if e.squashed >= 0.0 {
                frames[2]
            } else {
                match e.kind {
                    EnemyKind::Beetle => frames[(e.age * 6.0) as usize % 2],
                    EnemyKind::Hopper => frames[usize::from(!e.body.grounded)],
                }
            };
            let mut c = WHITE;
            if e.squashed >= 0.0 {
                c[3] = (1.0 - (e.squashed - 0.3).max(0.0) * 5.0).clamp(0.0, 1.0);
            } else {
                self.shadow(rw, p[0], e.body.hw, p[1]);
            }
            self.at_feet(rw, f, p[0], p[1] + 1.0, e.dir < 0.0, c, layer::ENEMIES);
        }
        let h = &self.g.hero;
        let p = lerp2(h.prev, h.body.pos, self.alpha);
        let won_walk = self.g.phase == Phase::Won;
        let pose = if !h.alive() {
            pose::HURT
        } else if !h.body.grounded && !won_walk {
            if h.body.vel[1] < 0.0 { pose::JUMP } else { pose::FALL }
        } else if h.body.vel[0].abs() > 8.0 {
            pose::RUN0 + (h.stride / 7.0) as usize % 4
        } else {
            pose::IDLE0 + (self.tf * 1.6) as usize % 2
        };
        let blink = h.invuln > 0.0 && (h.invuln * 14.0) as i32 % 2 == 1;
        if h.alive() {
            self.shadow(rw, p[0], h.body.hw, p[1]);
        }
        if h.alive() && !blink {
            self.hero_light(rw, [p[0], p[1] - HERO_LIGHT_UP]);
        }
        if !blink {
            // Squash for a moment on landing (a baked frame, drawn 1:1).
            let f = if h.alive() && h.body.grounded && h.landed_t < 0.08 { a.hero_land[pose] } else { a.hero[pose] };
            self.at_feet(rw, f, p[0], p[1] + 1.0, h.facing < 0.0, WHITE, layer::HERO);
        }
    }

    /// The light on the hero, drawn from game state only (visual, nothing
    /// in the simulation): when an extra life lands (the youngest "1UP"
    /// pop) a burst of rays flies out and a glow behind Fennel fades by its
    /// dither; at the level clear a steady soft glow lights Fennel in the
    /// dark gate. `c` is the hero's middle in world art px.
    fn hero_light(&self, rw: &mut RenderWorld, c: [f32; 2]) {
        let a = self.art;
        let one_up = self.g.pops.iter().filter(|p| p.kind == PopKind::OneUp).map(|p| p.t).reduce(f32::min);
        let glow = match one_up {
            Some(t) => GLOW_STEPS.iter().position(|&end| t < end),
            None if self.g.phase == Phase::Won => Some(1),
            None => None,
        };
        let mut c_glow = WHITE;
        c_glow[3] = GLOW_ALPHA;
        if let Some(f) = glow.and_then(|i| a.glow.get(i)) {
            self.centred(rw, *f, c[0], c[1], false, c_glow, layer::HERO - 1);
        }
        if let Some(f) = one_up.and_then(|t| BURST_STEPS.iter().position(|&end| t < end)).and_then(|i| a.burst.get(i)) {
            self.centred(rw, *f, c[0], c[1], false, WHITE, layer::POPS);
        }
    }

    /// Dust, sparkles, puffs, score pops and "1UP".
    fn pops(&self, rw: &mut RenderWorld) {
        let a = self.art;
        for pop in &self.g.pops {
            let life = pop.kind.life();
            let u = (pop.t / life).clamp(0.0, 0.999);
            let [x, y] = pop.pos;
            match pop.kind {
                PopKind::Dust => {
                    let f = a.dust[(u * 3.0) as usize];
                    self.at_feet(rw, f, x - 5.0, y + 1.0, true, WHITE, layer::POPS);
                    self.at_feet(rw, f, x + 5.0, y + 1.0, false, WHITE, layer::POPS);
                }
                PopKind::Sparkle => self.centred(rw, a.sparkle[(pop.t * 16.0) as usize % 2], x, y, false, WHITE, layer::POPS),
                PopKind::Puff => self.centred(rw, a.puff[(u * 2.0) as usize], x, y - 4.0, false, WHITE, layer::POPS),
                PopKind::Score(_) | PopKind::OneUp => {}
            }
        }
        // Floating text, laid out in screen space on the art pixel grid.
        for p in text_pops(self.g, self.s.cam, self.s.view[0], self.alpha) {
            let mut c = WHITE;
            c[3] = p.alpha;
            for (i, ch) in p.text.chars().enumerate() {
                if let Some(gi) = font::mini_index(ch) {
                    let f = a.mini[gi];
                    rw.draw_sprite(self.s.sprite_screen(p.at.x + i as f32 * MINI_ADV, p.at.y, f.w, f.h, f.cell, c, layer::POPS + 1));
                }
            }
        }
    }

    /// The pixel HUD across the top: lives, coins, score, time, on the same
    /// pixel grid as the art (Chris's review, point 6): one HUD pixel is one
    /// art pixel, at whole art-pixel positions. It needs about 190 art
    /// pixels across; the narrowest view (a portrait phone) is 256.
    fn hud(&self, rw: &mut RenderWorld) {
        let a = self.art;
        let g = self.g;
        let hs = Screen { cam: [0.0; 2], ..self.s };
        let adv = HUD_ADV;
        let glyph = |rw: &mut RenderWorld, ch: char, x: f32, y: f32, c: [f32; 4]| {
            if let Some(i) = font::hud_index(ch) {
                let f = a.glyphs[i];
                rw.draw_sprite(hs.sprite_screen(x, y, f.w, f.h, f.cell, c, layer::HUD));
            }
        };
        let text = |rw: &mut RenderWorld, t: &str, x: f32, y: f32, c: [f32; 4]| {
            for (i, ch) in t.chars().enumerate() {
                glyph(rw, ch, x + i as f32 * adv, y, c);
            }
            t.len() as f32 * adv
        };
        let icon = |rw: &mut RenderWorld, f: Frame, x: f32, y: f32| {
            rw.draw_sprite(hs.sprite_screen(x, y, f.w, f.h, f.cell, WHITE, layer::HUD));
            f.w + 1.0
        };
        let (y, m) = (6.0, 8.0);
        let mut x = m;
        // Free roam has no lives: the HUD starts with the coins.
        if !g.free_roam {
            x += icon(rw, a.icon_head, x, y);
            x += text(rw, &format!("x{}", g.lives.clamp(0, 99)), x, y + 1.0, WHITE);
            x += 10.0;
        }
        x += icon(rw, a.icon_coin, x, y);
        text(rw, &format!("x{:02}", g.coins.min(999)), x, y + 1.0, WHITE);
        let w = hs.view[0];
        let score = format!("{:06}", g.score.min(999_999));
        text(rw, &score, (w - score.len() as f32 * adv) * 0.5, y + 1.0, WHITE);
        if g.free_roam {
            // No timer: a small label in its place, flush right.
            let label = FREE_ROAM_LABEL;
            let lw = (label.len() as f32 - 1.0) * MINI_ADV + MINI_FRAME[0];
            let lx = w - m - lw;
            for (i, ch) in label.chars().enumerate() {
                if let Some(gi) = font::mini_index(ch) {
                    let f = a.mini[gi];
                    rw.draw_sprite(hs.sprite_screen(lx + i as f32 * MINI_ADV, y + 1.0, f.w, f.h, f.cell, WHITE, layer::HUD));
                }
            }
            return;
        }
        let left = g.time_left.max(0.0).ceil() as u32;
        let hurry = g.phase == Phase::Playing && g.time_left < 30.0;
        let flash = hurry && (self.tf * 4.0) as i32 % 2 == 0;
        let c = if flash { [1.0, 0.35, 0.3, 1.0] } else { WHITE };
        let time = format!("{:03}", left.min(999));
        let tw = time.len() as f32 * adv;
        let x = w - m - tw - a.icon_clock.w - 1.0;
        let x = x + icon(rw, a.icon_clock, x, y);
        text(rw, &time, x, y + 1.0, c);
    }

    /// "LEVEL CLEAR!" in the HUD glyphs on an ink panel, centred across
    /// the view at [`BANNER_Y`] of its height: the level-clear state shows
    /// in the canvas itself (and in every capture), before and under the
    /// results panel.
    fn banner(&self, rw: &mut RenderWorld) {
        let a = self.art;
        let hs = Screen { cam: [0.0; 2], ..self.s };
        let r = banner_rect(&hs);
        let [panel, border] = a.banner_colors;
        rw.draw_sprite(hs.sprite_screen(r.x, r.y, r.w, r.h, a.white.cell, border, layer::BANNER));
        rw.draw_sprite(hs.sprite_screen(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, a.white.cell, panel, layer::BANNER));
        let (x0, y0) = (r.x + BANNER_PAD[0], r.y + BANNER_PAD[1]);
        for (i, ch) in font::LEVEL_CLEAR.chars().enumerate() {
            if let Some(gi) = font::hud_index(ch) {
                let f = a.glyphs[gi];
                rw.draw_sprite(hs.sprite_screen(x0 + i as f32 * HUD_ADV, y0, f.w, f.h, f.cell, WHITE, layer::BANNER + 1));
            }
        }
    }
}

/// The banner's top as a fraction of the view height, in the sky between
/// the HUD and the castle: at the goal the castle's towers rise to about
/// 144 px above the ground line (row 194 of 270 at 1080p), so their tops sit
/// near row 50, and a banner at 28 % covered them (critic round 3 note).
pub const BANNER_Y: f32 = 0.1;
/// The banner's top is never above this row: the HUD owns the top 20 art
/// rows (hud-layout.md), plus a 4-row gap.
pub const BANNER_MIN_Y: f32 = 24.0;
/// Panel padding around the banner text (art px, x and y).
const BANNER_PAD: [f32; 2] = [6.0, 4.0];

/// The banner panel on screen (art px, whole pixels).
pub fn banner_rect(s: &Screen) -> TextBox {
    let n = font::LEVEL_CLEAR.chars().count() as f32;
    // A glyph frame is 9 x 10 (6 x 7, outlined, with a drop shadow).
    let text_w = (n - 1.0) * HUD_ADV + 9.0;
    let (w, h) = (text_w + BANNER_PAD[0] * 2.0, 10.0 + BANNER_PAD[1] * 2.0);
    TextBox { x: ((s.view[0] - w) * 0.5).round(), y: (s.view[1] * BANNER_Y).round().max(BANNER_MIN_Y), w, h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformerPlugin;
    use crate::game::Request;
    use evani_core::App;

    fn app(look: bool) -> App {
        let a = App::new("t").with_module(evani_render::RenderModule::new());
        let a = if look { a.with_look(evani_look_pixel16::Pixel16Look) } else { a.with_look(evani_look_basic::BasicLook) };
        a.with_kit(PlatformerPlugin)
    }

    fn fit(target: [f32; 2], dpr: f32, pixel_art: bool) -> Screen {
        Screen::fit(target, target.map(|v| v / dpr), ViewRule::of(&Tune::default()), pixel_art)
    }

    #[test]
    fn the_level_clear_banner_fits_every_view_below_the_hud() {
        for target in [[1920.0, 1080.0], [2532.0, 1170.0], [1170.0, 2532.0], [1280.0, 720.0], [1055.0, 487.0]] {
            let s = fit(target, 1.0, true);
            let r = banner_rect(&s);
            assert!(r.x >= 0.0 && r.x + r.w <= s.view[0], "{target:?}: {r:?} in {:?}", s.view);
            assert!(r.y >= 20.0 && r.y + r.h < s.view[1] * 0.5, "{target:?}: {r:?}");
            assert_eq!((r.x.fract(), r.y.fract()), (0.0, 0.0));
        }
    }

    #[test]
    fn whole_pixel_scale() {
        // (device size, scale, view in art px, framed?) for the review sizes.
        for (target, scale, view, framed) in [
            ([1280.0, 720.0], 3.0, [1280.0 / 3.0, 240.0], false),
            ([1920.0, 1080.0], 4.0, [480.0, 270.0], false),
            ([2560.0, 1440.0], 5.0, [512.0, 288.0], false),
            // Ultrawide: 640 art px across, framed.
            ([3440.0, 1440.0], 5.0, [640.0, 288.0], true),
            // 3440 x 1440 at 125 % Windows scaling, browser fullscreen.
            ([3439.0, 1389.0], 5.0, [640.0, 1389.0 / 5.0], true),
            // Phones at dpr 3, landscape and portrait (256 across). Upright,
            // the 256 columns set the scale and the view is framed to 288
            // rows with bars above and below.
            ([2532.0, 1170.0], 5.0, [506.4, 234.0], false),
            ([1170.0, 2532.0], 4.0, [292.5, 288.0], true),
            ([1080.0, 2340.0], 4.0, [270.0, 288.0], true),
            // A tablet held upright at dpr 1.
            ([800.0, 1280.0], 3.0, [800.0 / 3.0, 288.0], true),
        ] {
            let s = fit(target, 1.0, true);
            assert_eq!(s.scale, scale, "{target:?}");
            assert!((s.view[0] - view[0]).abs() < 1e-3 && (s.view[1] - view[1]).abs() < 1e-3, "{target:?}: {:?}", s.view);
            assert!(s.view[1] >= 224.0 && s.view[1] <= 288.0, "{target:?}");
            assert_eq!(!s.bars().is_empty(), framed, "{target:?}");
            // The framed view starts on a whole device pixel; on each axis
            // the bars on that axis and the view fill the canvas exactly.
            assert_eq!((s.origin[0].fract(), s.origin[1].fract()), (0.0, 0.0));
            for i in 0..2 {
                let side: f32 = s.bars().iter().filter(|b| b[2 + (1 - i)] >= target[1 - i] - 1e-3).map(|b| b[2 + i]).sum();
                let covered = side + s.view[i] * s.scale;
                assert!((covered - target[i]).abs() < 1e-2, "{target:?} axis {i}: {covered}");
            }
            // Centred: the two bars on an axis differ by at most a pixel.
            let far = [target[0] - s.origin[0] - s.view[0] * s.scale, target[1] - s.origin[1] - s.view[1] * s.scale];
            assert!((far[0] - s.origin[0]).abs() <= 1.0 && (far[1] - s.origin[1]).abs() <= 1.0, "{target:?}: {:?} {far:?}", s.origin);
        }
        // A tiny window still gets one device pixel per art pixel.
        assert_eq!(fit([300.0, 150.0], 1.0, true).scale, 1.0);
        // Smooth looks get a fractional scale and the same framing.
        assert!((fit([1920.0, 1080.0], 1.0, false).scale - 1080.0 / 224.0).abs() < 1e-4);
    }

    /// Each axis maps by its own CSS-to-device ratio, so a backing store
    /// that is not exactly css x dpr on one axis still lands every art
    /// pixel on exactly `scale` device pixels.
    #[test]
    fn each_axis_maps_to_whole_device_pixels() {
        let s = Screen::fit([3439.0, 1389.0], [2751.2, 1111.2], ViewRule::of(&Tune::default()), true);
        let sp = s.sprite_screen(17.0, 9.0, 16.0, 16.0, 0, WHITE, 0);
        let dev = |i: usize, v: f32| v / s.css_per_px[i];
        let x0 = dev(0, sp.pos[0] - sp.size[0] * 0.5);
        let y0 = dev(1, sp.pos[1] - sp.size[1] * 0.5);
        assert!((x0 - (s.origin[0] + 17.0 * 5.0)).abs() < 1e-2 && (y0 - 45.0).abs() < 1e-2, "{x0} {y0}");
        assert!((dev(0, sp.size[0]) - 80.0).abs() < 1e-2 && (dev(1, sp.size[1]) - 80.0).abs() < 1e-2);
        assert_eq!(s.art_rect(&sp).map(|v| (v * 1000.0).round() / 1000.0), [17.0, 9.0, 16.0, 16.0]);
    }

    fn sprites_after(a: &mut App, frames: u32) -> Vec<Sprite> {
        a.world.res_mut::<RenderWorld>().viewport_css = [1280.0, 720.0];
        // The native renderer counts and clears the draws every frame, so
        // run the frames, then draw once more and look.
        a.run_headless(frames, 1000.0 / 60.0);
        a.world.res_mut::<RenderWorld>().clear();
        draw_system(&mut a.world);
        a.world.res::<RenderWorld>().sprites.clone()
    }

    #[test]
    fn title_draws_the_attract_demo_with_parallax_shadows_and_art() {
        let mut a = app(true);
        a.start();
        let sprites = sprites_after(&mut a, 3);
        let sheet = a.world.res::<SpriteSheet>();
        assert!(sheet.is_valid() && sheet.width > 0, "no sheet");
        drop(sheet);
        let has = |l: i16| sprites.iter().any(|s| s.layer == l);
        for l in [layer::SKY, layer::TILES, layer::HERO, layer::SHADOWS] {
            assert!(has(l), "layer {l}");
        }
        // pixel16 turns pixel art on: whole-pixel scale.
        let st = a.world.res::<DrawState>();
        assert!(st.screen.snap);
        assert_eq!(st.screen.scale, 3.0);
        // The demo shows its HUD too (the captures check it reads).
        assert!(has(layer::HUD));
    }

    #[test]
    fn playing_draws_the_hud_and_plays_sounds_from_events() {
        let mut a = app(true);
        a.start();
        a.world.res_mut::<Platformer>().game.request(Request::Start);
        let sprites = sprites_after(&mut a, 3);
        assert!(sprites.iter().filter(|s| s.layer == layer::HUD).count() >= 10);
        // The review sees the canvas HUD: across the top, inside the canvas.
        let ch = a.world.res::<RenderWorld>().canvas_hud();
        let Some(ch) = ch else { panic!("the kit declares its HUD layer") };
        assert!(ch.sprites >= 10, "{ch:?}");
        let [x, y, w, h] = ch.rect_css;
        assert!(x >= 0.0 && y >= 0.0 && w > 600.0 && h > 20.0 && y + h < 720.0 * 0.2, "{ch:?}");
        let hud = a.world.res::<PlatformerHud>();
        assert_eq!(hud.phase, Phase::Playing);
        assert_eq!(hud.level_name, "Bramble Hills");
        drop(hud);
        // Events are drained every frame.
        assert!(a.world.res::<Platformer>().game.events.is_empty());
    }

    /// Play to a busy moment (coins, a stomp, pops) on a `css` screen at
    /// `dpr`, then draw once; the sprites and the screen mapping.
    fn frame_at(css: [f32; 2], dpr: f32) -> (Vec<Sprite>, Screen) {
        let mut a = app(true);
        a.start();
        {
            let mut rw = a.world.res_mut::<RenderWorld>();
            rw.viewport_css = css;
            rw.target_px = [(css[0] * dpr).round() as u32, (css[1] * dpr).round() as u32];
        }
        // Parallax on (as evani-fx would report it), so the planes and
        // clouds are drawn too.
        static PARALLAX: evani_core::effects::EffectInfo = evani_core::effects::EffectInfo {
            name: "parallax",
            cost: 1,
            value: 1.0,
            styles: evani_core::effects::StyleClasses::TWO_D,
            vr_comfort_off: false,
            description: "",
        };
        a.world.insert_resource(EffectsStatus {
            effects: vec![evani_core::effects::EffectStatus {
                info: &PARALLAX,
                enabled: None,
                weight: None,
                passes: &[],
                state: evani_core::effects::EffectState::On,
            }],
        });
        // The stomp moment: enemies, the hero, a score pop, the HUD.
        a.run_headless(2, 1000.0 / 60.0);
        crate::moments::stomp(&a.world).unwrap();
        assert!(a.world.res::<Platformer>().game.pops.iter().any(|p| matches!(p.kind, PopKind::Score(_))));
        a.world.res_mut::<RenderWorld>().clear();
        draw_system(&mut a.world);
        let sprites = a.world.res::<RenderWorld>().sprites.clone();
        let s = a.world.res::<DrawState>().screen;
        (sprites, s)
    }

    /// Point 6: at 1920 x 1080 (and on a dpr-3 phone) every sprite -
    /// parallax planes, tiles, actors, pops and HUD - covers whole art
    /// pixels at whole art-pixel positions.
    #[test]
    fn every_sprite_sits_on_the_art_pixel_grid() {
        for (css, dpr, scale) in [([1920.0, 1080.0], 1.0, 4.0), ([360.0, 780.0], 3.0, 4.0), ([2751.2, 1111.2], 1.25, 5.0)] {
            let (sprites, s) = frame_at(css, dpr);
            assert_eq!(s.scale, scale);
            let whole = |v: f32| (v - v.round()).abs() < 1e-3;
            let mut off = Vec::new();
            for sp in sprites.iter().filter(|sp| sp.layer != layer::BARS) {
                let [x0, y0, w, h] = s.art_rect(sp);
                if !(whole(w) && whole(h) && whole(x0) && whole(y0)) {
                    off.push((sp.layer, x0, y0, w, h));
                }
            }
            assert!(sprites.len() > 100);
            for l in [layer::FAR, layer::MID, layer::NEAR, layer::POPS + 1, layer::HUD, layer::HERO] {
                assert!(sprites.iter().any(|sp| sp.layer == l), "{css:?}: nothing on layer {l}");
            }
            assert!(off.is_empty(), "{css:?}: off the grid (layer, x, y, w, h): {:?}", &off[..off.len().min(8)]);
        }
    }

    /// The thin line through the hills: snapping rounds a half the same way
    /// at every position, so two copies of a strip one width apart stay one
    /// width apart (`round` took -1.5 to -2 but 510.5 to 511).
    #[test]
    fn snapping_keeps_strip_copies_a_whole_width_apart() {
        let s = fit([1920.0, 1080.0], 1.0, true);
        for i in 0..2048 {
            let v = i as f32 * 0.25 - 256.0;
            assert_eq!(s.px(v) + 512.0, s.px(v + 512.0), "at {v}");
        }
        assert_eq!((s.px(-1.5), s.px(510.5)), (-1.0, 511.0));
    }

    /// Geocheck pixel integrity: drop shadows (every width) and the landing
    /// squash are baked frames drawn 1:1, never a sprite scaled when drawn.
    #[test]
    fn shadows_and_the_landing_squash_are_drawn_one_to_one() {
        let mut a = app(true);
        a.start();
        a.world.res_mut::<Platformer>().game.request(Request::Start);
        a.world.res_mut::<RenderWorld>().viewport_css = [1920.0, 1080.0];
        a.run_headless(30, 1000.0 / 60.0);
        {
            let mut p = a.world.res_mut::<Platformer>();
            assert!(p.game.hero.body.grounded, "the hero starts on the ground");
            p.game.hero.landed_t = 0.0;
        }
        a.world.res_mut::<RenderWorld>().clear();
        draw_system(&mut a.world);
        let sprites = a.world.res::<RenderWorld>().sprites.clone();
        let st = a.world.res::<DrawState>();
        let (s, art) = (st.screen, st.art.clone().unwrap_or_default());
        drop(st);
        let size = |cell: u16| {
            art.hero.iter().chain(&art.hero_land).chain(&art.shadow).find(|f| f.cell == cell).map(|f| [f.w, f.h])
        };
        let hero: Vec<&Sprite> = sprites.iter().filter(|sp| sp.layer == layer::HERO).collect();
        assert_eq!(hero.len(), 1);
        assert!(art.hero_land.iter().any(|f| f.cell == hero[0].cell), "the squash frame is drawn just after landing");
        let shadows: Vec<&Sprite> = sprites.iter().filter(|sp| sp.layer == layer::SHADOWS).collect();
        assert!(!shadows.is_empty());
        for sp in hero.into_iter().chain(shadows) {
            let [_, _, w, h] = s.art_rect(sp);
            let f = size(sp.cell).unwrap_or_else(|| panic!("layer {}: cell {} is not a hero or shadow frame", sp.layer, sp.cell));
            assert!((w - f[0]).abs() < 1e-3 && (h - f[1]).abs() < 1e-3, "layer {}: drawn {w} x {h}, frame {f:?}", sp.layer);
        }
        // Every width from the narrowest to the widest has its own frame.
        for (i, f) in art.shadow.iter().enumerate() {
            let w = crate::art::SHADOW_MIN_W + i as u32;
            assert_eq!([f.w, f.h], [w as f32, crate::art::shadow_height(w) as f32]);
        }
        assert_eq!(art.shadow.len() as u32, SHADOW_MAX_W - SHADOW_MIN_W + 1);
    }

    /// Clear air (geocheck): a 13 px coin at its tile's centre keeps 2 px
    /// of air above the ground of the tile below; odd heights round up.
    #[test]
    fn a_centred_coin_keeps_two_pixels_of_air() {
        let (centre, ground) = (9.5 * TILE, 10.0 * TILE);
        let top = centred_top(13.0, centre);
        assert_eq!(top, 145.0);
        assert!(ground - (top + 13.0) >= 2.0);
        // Even heights centre exactly.
        assert_eq!(centred_top(12.0, centre), 146.0);
    }

    /// Point 6/7: the HUD is drawn at the art scale and fits the narrowest
    /// view (portrait phone) with its three groups apart.
    #[test]
    fn hud_is_on_the_art_grid_and_fits_a_portrait_phone() {
        for (css, dpr) in [([1920.0, 1080.0], 1.0), ([360.0, 780.0], 3.0), ([390.0, 844.0], 3.0), ([3440.0, 1440.0], 1.0)] {
            let (sprites, s) = frame_at(css, dpr);
            let mut hud: Vec<[f32; 4]> = sprites
                .iter()
                .filter(|sp| sp.layer == layer::HUD)
                .map(|sp| {
                    let [x, y, w, h] = s.art_rect(sp);
                    [x, y, x + w, y + h]
                })
                .collect();
            assert!(hud.len() >= 10);
            hud.sort_by(|a, b| a[0].total_cmp(&b[0]));
            assert!(hud.iter().all(|r| r[0] >= 4.0 && r[2] <= s.view[0] - 4.0 && r[1] >= 0.0 && r[3] <= 24.0), "{css:?}: HUD off screen");
            // Glyph frames include a drop shadow, so neighbours touch;
            // the groups (lives+coins, score, time) leave gaps of 8+ px.
            let gaps = hud.windows(2).filter(|w| w[1][0] - w[0][2] >= 8.0).count();
            assert!(gaps >= 2, "{css:?}: HUD groups run together");
            let mut groups = vec![hud[0]];
            for r in &hud[1..] {
                let g = groups.last_mut().unwrap();
                if r[0] - g[2] >= 8.0 {
                    groups.push(*r);
                } else {
                    *g = [g[0], g[1].min(r[1]), g[2].max(r[2]), g[3].max(r[3])];
                }
            }
            eprintln!("{css:?} x{dpr}: view {:?} art px, scale {}, HUD groups (x0, y0, x1, y1) {groups:?}", s.view, s.scale);
        }
    }

    /// Free roam: the HUD drops the lives and the timer for a small
    /// "FREE ROAM" label at the right edge, on the art grid and on screen.
    #[test]
    fn free_roam_hud_shows_a_label_instead_of_lives_and_timer() {
        let hud_of = |free: bool, css: [f32; 2], dpr: f32| {
            let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_look(evani_look_pixel16::Pixel16Look);
            if free {
                a = a.with_play_mode(evani_core::play::PlayMode::FreeRoam);
            }
            let mut a = a.with_kit(PlatformerPlugin);
            a.start();
            {
                let mut rw = a.world.res_mut::<RenderWorld>();
                rw.viewport_css = css;
                rw.target_px = [(css[0] * dpr).round() as u32, (css[1] * dpr).round() as u32];
            }
            a.world.res_mut::<Platformer>().game.request(Request::Start);
            a.run_headless(3, 1000.0 / 60.0);
            a.world.res_mut::<RenderWorld>().clear();
            draw_system(&mut a.world);
            let s = a.world.res::<DrawState>().screen;
            let mut hud: Vec<[f32; 4]> = a
                .world
                .res::<RenderWorld>()
                .sprites
                .iter()
                .filter(|sp| sp.layer == layer::HUD)
                .map(|sp| {
                    let [x, y, w, h] = s.art_rect(sp);
                    [x, y, x + w, y + h]
                })
                .collect();
            hud.sort_by(|a, b| a[0].total_cmp(&b[0]));
            (hud, s.view[0])
        };
        for (css, dpr) in [([1920.0, 1080.0], 1.0), ([360.0, 780.0], 3.0), ([3440.0, 1440.0], 1.0)] {
            let (play, _) = hud_of(false, css, dpr);
            let (free, view) = hud_of(true, css, dpr);
            // Play: head, x3, coin, x00, score (6), clock, time (3) = 17.
            // Free roam: coin, x00, score (6), and the 8 letters = 18.
            assert_eq!(play.len(), 17, "{css:?}");
            assert_eq!(free.len(), 18, "{css:?}");
            assert!(free.iter().all(|r| r[0] >= 4.0 && r[2] <= view - 4.0 && r[1] >= 0.0 && r[3] <= 24.0), "{css:?}: HUD off screen {free:?}");
            // The coin is now first at the left margin; the label (the last
            // 8 sprites) sits flush right, clear of the score before it.
            assert!(free[0][0] <= 9.0, "{css:?}: {:?}", free[0]);
            let label = &free[free.len() - 8..];
            let score_end = free[free.len() - 9][2];
            assert!(label[0][0] - score_end >= 8.0, "{css:?}: label {:?} runs into the score ending {score_end}", label[0]);
            assert!((view - 8.0 - label[7][2]).abs() <= 3.0, "{css:?}: label not flush right: {:?} in {view}", label[7]);
        }
    }

    #[test]
    fn runs_with_the_basic_look_too() {
        // Done-when 4: another look, the kit's own colours, smooth scale.
        let mut a = app(false);
        a.start();
        let sprites = sprites_after(&mut a, 2);
        assert!(sprites.iter().any(|s| s.layer == layer::HERO));
        assert!(!a.world.res::<DrawState>().screen.snap);
    }
}
