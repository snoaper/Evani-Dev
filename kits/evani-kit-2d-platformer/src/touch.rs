//! On-screen controls for phones and tablets (README "Touch controls").
//!
//! The layer only sets actions the keys already drive: left and right are
//! the two sides of the `move` axis (A/D, arrows, pad stick), Jump is the
//! `jump` button (held like Space, so a held thumb jumps higher and an early
//! release cuts it), Run is the `run` button (Shift) and the pause button
//! is one press of the `pause` action (Esc). Nothing in the sim knows about
//! touch. The layer is up only while the hero can be steered
//! ([`controls_on`]): the title, pause, settings and results screens are
//! tapped on the page.
//!
//! Drop-through (down + jump on a plank) has no button: no part of the
//! level needs it, and keys keep it.

use evani_core::touch::{Anchor, Button, ControlSize, TouchLayout};

use crate::game::Phase;

/// Control order in [`layout`] (the tests press them by index).
pub const LEFT: usize = 0;
pub const RIGHT: usize = 1;
pub const RUN: usize = 2;
pub const JUMP: usize = 3;
pub const PAUSE: usize = 4;

/// The pause button's drop below the top edge, in `vmin` (the README's
/// `pause_at`). The HUD (lives, coins, score, time or the FREE ROAM label)
/// owns the top 24 art rows, so the button sits under it, at the right edge:
/// the test `the_pause_button_sits_under_the_hud_on_every_screen` measures
/// that on phones and tablets.
pub const PAUSE_DOWN_VMIN: f32 = 10.0;
/// Its distance from the right edge, in `vmin` (the page adds the safe area).
pub const PAUSE_IN_VMIN: f32 = 3.0;

/// The kit's touch layout: walk with the left thumb, Jump (large) and Run
/// under the right, pause under the HUD at the top right.
pub fn layout() -> TouchLayout {
    TouchLayout::new()
        .button(Button::hold("left", "move", "\u{25C0}").value(-1.0).primary().at(Anchor::BottomLeft, 3.0, 4.0))
        .button(Button::hold("right", "move", "\u{25B6}").primary().at(Anchor::BottomLeft, 23.0, 4.0))
        .button(Button::hold("run", "run", "Run").size(ControlSize::Normal).at(Anchor::BottomRight, 24.0, 3.0))
        .button(Button::hold("jump", "jump", "Jump").primary().at(Anchor::BottomRight, 3.0, 5.0))
        .pause_at("pause", Anchor::TopRight, PAUSE_IN_VMIN, PAUSE_DOWN_VMIN)
}

/// Whether the layer is up: only while a run is being played. Menus (title,
/// pause, settings, results) are the page's, tapped.
pub fn controls_on(phase: Phase) -> bool {
    phase == Phase::Playing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{DrawState, draw_system, layer};
    use crate::game::Request;
    use crate::{Platformer, PlatformerPlugin};
    use evani_core::App;
    use evani_core::input::{Binding, Input, KeyCode, RawEvent};
    use evani_core::play::PlayMode;
    use evani_core::touch::{Control, TouchControls};
    use evani_render::RenderWorld;

    const FRAME_MS: f64 = 1000.0 / 60.0;

    fn started(mode: PlayMode) -> App {
        let mut a = App::new("touchtest").with_play_mode(mode).with_kit(PlatformerPlugin);
        a.start();
        if mode == PlayMode::Play {
            a.world.res_mut::<Platformer>().game.request(Request::Start);
        }
        a.run_headless(2, FRAME_MS);
        a
    }

    fn with_touch<R>(a: &App, f: impl FnOnce(&mut TouchControls, &mut Input) -> R) -> R {
        let mut t = a.world.res_mut::<TouchControls>();
        let mut i = a.world.res_mut::<Input>();
        f(&mut t, &mut i)
    }

    fn press(a: &App, control: usize, pointer: i32) {
        with_touch(a, |t, i| {
            t.on_touch(i);
            t.press(i, control, pointer, [0.0; 2], [0.0; 2], 0.0);
        });
    }

    fn release(a: &App, pointer: i32) {
        with_touch(a, |t, i| t.release(i, pointer));
    }

    fn key(a: &App, code: &str, down: bool) {
        a.world.res_mut::<Input>().handle(&RawEvent::Key { code: KeyCode::new(code).unwrap(), down });
    }

    fn hero(a: &App) -> ([f32; 2], [f32; 2]) {
        let p = a.world.res::<Platformer>();
        (p.game.hero.body.pos, p.game.hero.body.vel)
    }

    /// Every control names an action the kit declares with key bindings, so
    /// the keys drive each one first; touch is only another way to set it.
    #[test]
    fn every_touch_control_drives_an_action_the_keys_already_bind() {
        let a = started(PlayMode::Play);
        let input = a.world.res::<Input>();
        let t = a.world.res::<TouchControls>();
        assert_eq!(t.layout().controls.len(), 5);
        let mut seen = Vec::new();
        for c in &t.layout().controls {
            let Control::Button(b) = c else { panic!("`{}` is not a button", c.id()) };
            let def = input
                .actions()
                .iter()
                .find(|d| d.name == b.action)
                .unwrap_or_else(|| panic!("`{}`: no action `{}`", b.id, b.action));
            let keys = def.bindings.iter().filter(|x| matches!(x, Binding::Key(_) | Binding::KeyAxis { .. })).count();
            assert!(keys >= 1, "`{}` drives `{}`, which has no key binding", b.id, b.action);
            seen.push((b.id.as_str(), b.action.as_str(), b.value));
        }
        assert_eq!(
            seen,
            [("left", "move", -1.0), ("right", "move", 1.0), ("run", "run", 1.0), ("jump", "jump", 1.0), ("pause", "pause", 1.0)]
        );
        assert!(t.layout().hold_while_shown.is_empty(), "nothing is held for the player");
        // Control order is what these tests (and the screenshots) press by.
        for (i, id) in ["left", "right", "run", "jump", "pause"].iter().enumerate() {
            assert_eq!(t.layout().controls[i].id(), *id);
        }
    }

    /// A thumb holding right and Jump for the same frames as D and Space
    /// ends in exactly the same place with the same speed, jump height
    /// included (hold for higher, release to cut).
    #[test]
    fn touch_walks_and_jumps_exactly_like_the_keys() {
        // Frames the jump stays held; a short tap cuts the jump.
        for (hold_jump, label) in [(40, "held jump"), (6, "tapped jump")] {
            let mut keys = started(PlayMode::Play);
            let mut touch = started(PlayMode::Play);
            for frame in 0..160 {
                if frame == 10 {
                    key(&keys, "KeyD", true);
                    press(&touch, RIGHT, 1);
                }
                if frame == 30 {
                    key(&keys, "Space", true);
                    press(&touch, JUMP, 2);
                }
                if frame == 30 + hold_jump {
                    key(&keys, "Space", false);
                    release(&touch, 2);
                }
                if frame == 120 {
                    key(&keys, "KeyD", false);
                    release(&touch, 1);
                }
                keys.run_headless(1, FRAME_MS);
                touch.run_headless(1, FRAME_MS);
                assert_eq!(hero(&keys), hero(&touch), "{label}: frame {frame}");
            }
            assert!(hero(&touch).0[0] > 0.0);
        }
        // The two jumps really differ in height, so the test would notice a
        // touch jump that ignored the release.
        let peak = |hold_jump: usize| {
            let mut a = started(PlayMode::Play);
            let start = hero(&a).0[1];
            let mut top = start;
            for frame in 0..90 {
                if frame == 5 {
                    press(&a, JUMP, 2);
                }
                if frame == 5 + hold_jump {
                    release(&a, 2);
                }
                a.run_headless(1, FRAME_MS);
                top = top.min(hero(&a).0[1]);
            }
            start - top
        };
        let (full, short) = (peak(40), peak(4));
        assert!(full > short + 8.0, "full {full} px, short {short} px");
    }

    /// Run is Shift: held, the top speed is the run speed. Left walks left.
    #[test]
    fn run_and_left_are_the_keys_run_and_left() {
        // The hero's horizontal speed after 40 frames of holding some controls.
        let speed = |held: &[usize]| {
            let mut a = started(PlayMode::Play);
            for (n, c) in held.iter().enumerate() {
                press(&a, *c, n as i32 + 1);
            }
            a.run_headless(40, FRAME_MS);
            hero(&a).1[0]
        };
        let (walk, run) = (speed(&[RIGHT]), speed(&[RIGHT, RUN]));
        assert!(run > walk + 20.0, "walk {walk}, run {run}");
        // Left, away from the wall at the start of the level.
        let mut a = started(PlayMode::Play);
        press(&a, RIGHT, 1);
        a.run_headless(60, FRAME_MS);
        release(&a, 1);
        press(&a, LEFT, 2);
        a.run_headless(20, FRAME_MS);
        let left = hero(&a).1[0];
        assert!(left < -50.0, "left walks left: {left}");
        assert_eq!(speed(&[]), 0.0);
    }

    /// The pause button is one press of the `pause` action, like Esc, and
    /// the layer follows the screens.
    #[test]
    fn the_pause_button_pauses_and_the_layer_follows_the_screens() {
        let mut a = App::new("touchtest").with_kit(PlatformerPlugin);
        a.start();
        with_touch(&a, |t, i| t.on_touch(i));
        a.run_headless(3, FRAME_MS);
        // Title (the attract demo behind it): the page's Play button is tapped.
        assert!(!evani_core::touch::active(&a.world), "off on the title");
        a.world.res_mut::<Platformer>().game.request(Request::Start);
        a.run_headless(3, FRAME_MS);
        assert_eq!(a.world.res::<Platformer>().game.phase, Phase::Playing);
        assert!(evani_core::touch::active(&a.world), "up while playing");
        press(&a, PAUSE, 4);
        release(&a, 4);
        a.run_headless(3, FRAME_MS);
        assert_eq!(a.world.res::<Platformer>().game.phase, Phase::Paused, "the pause button pauses");
        assert!(!evani_core::touch::active(&a.world), "off in the pause menu");
        a.world.res_mut::<Platformer>().game.request(Request::TogglePause);
        a.run_headless(3, FRAME_MS);
        assert_eq!(a.world.res::<Platformer>().game.phase, Phase::Playing);
        assert!(evani_core::touch::active(&a.world), "back after resume");
        // A held button is let go with the layer: no stuck walking after a pause.
        press(&a, RIGHT, 5);
        a.world.res_mut::<Platformer>().game.request(Request::TogglePause);
        a.run_headless(3, FRAME_MS);
        assert!(!evani_core::touch::active(&a.world));
        let walk = a.world.res::<Input>().action_id("move").unwrap();
        assert_eq!(a.world.res::<Input>().value(walk), 0.0);
    }

    #[test]
    fn only_a_run_in_progress_shows_the_controls() {
        for (phase, on) in [(Phase::Title, false), (Phase::Playing, true), (Phase::Paused, false), (Phase::Won, false), (Phase::Lost, false)] {
            assert_eq!(controls_on(phase), on, "{phase:?}");
        }
    }

    /// Free roam has the layer too, from the first frame.
    #[test]
    fn free_roam_has_the_controls() {
        let mut a = started(PlayMode::FreeRoam);
        with_touch(&a, |t, i| t.on_touch(i));
        a.run_headless(2, FRAME_MS);
        assert!(evani_core::touch::active(&a.world));
    }

    /// The HUD's top and lowest rows (CSS px from the page's top) on a
    /// screen, in play or free roam, and the screen mapping.
    fn hud_rows_css(css: [f32; 2], dpr: f32, free: bool) -> ([f32; 2], crate::draw::Screen) {
        let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_look(evani_look_pixel16::Pixel16Look);
        if free {
            a = a.with_play_mode(PlayMode::FreeRoam);
        }
        let mut a = a.with_kit(PlatformerPlugin);
        a.start();
        {
            let mut rw = a.world.res_mut::<RenderWorld>();
            rw.viewport_css = css;
            rw.target_px = [(css[0] * dpr).round() as u32, (css[1] * dpr).round() as u32];
        }
        a.world.res_mut::<Platformer>().game.request(Request::Start);
        a.run_headless(3, FRAME_MS);
        a.world.res_mut::<RenderWorld>().clear();
        draw_system(&mut a.world);
        let s = a.world.res::<DrawState>().screen;
        let rows: Vec<[f32; 2]> = a
            .world
            .res::<RenderWorld>()
            .sprites
            .iter()
            .filter(|sp| sp.layer == layer::HUD)
            .map(|sp| {
                let [_, y, _, h] = s.art_rect(sp);
                [y, y + h]
            })
            .collect();
        let top_art = rows.iter().map(|r| r[0]).fold(f32::MAX, f32::min);
        let bottom_art = rows.iter().map(|r| r[1]).fold(0.0_f32, f32::max);
        assert!(bottom_art > 8.0, "{css:?}: no HUD drawn");
        // Art rows to CSS px from the page's top: the view's top (a framed
        // portrait view starts below a bar) plus device px per art px, times
        // CSS px per device px.
        let to_css = |art: f32| (s.origin[1] + art * s.scale) * s.css_per_px[1];
        ([to_css(top_art), to_css(bottom_art)], s)
    }

    /// The pause button's size on the page (`ControlSize::Small`,
    /// `clamp(48px, 9vmin, 56px)`).
    fn pause_size(vmin: f32) -> f32 {
        (9.0 * vmin).clamp(48.0, 56.0)
    }

    /// The page puts the pause button `max(8 px, safe-area inset) +
    /// PAUSE_DOWN_VMIN` below the top edge (crates/evani-core/src/web/touch.rs;
    /// an inset only pushes it lower). On every phone and tablet shape it must
    /// stay clear of the HUD, with a gap: under it on a landscape screen, in
    /// the bar above the framed view on an upright one.
    #[test]
    fn the_pause_button_sits_under_the_hud_on_every_screen() {
        let screens: [([f32; 2], f32, &str); 7] = [
            ([844.0, 390.0], 3.0, "phone landscape"),
            ([390.0, 844.0], 3.0, "phone portrait"),
            ([360.0, 780.0], 3.0, "narrow phone portrait"),
            ([667.0, 375.0], 2.0, "small phone landscape"),
            ([1280.0, 800.0], 1.0, "tablet landscape"),
            ([800.0, 1280.0], 1.0, "tablet portrait"),
            ([1194.0, 834.0], 2.0, "large tablet landscape"),
        ];
        for (css, dpr, name) in screens {
            let vmin = css[0].min(css[1]) / 100.0;
            let top = 8.0 + PAUSE_DOWN_VMIN * vmin;
            let bottom = top + pause_size(vmin);
            for free in [false, true] {
                let ([hud_top, hud], s) = hud_rows_css(css, dpr, free);
                eprintln!("{name} {css:?}: HUD {hud_top:.1}..{hud:.1} CSS px, pause {top:.1}..{bottom:.1} (free roam {free})");
                let under = top >= hud + 3.0;
                let above = s.origin[1] > 0.0 && bottom + 3.0 <= hud_top;
                assert!(under || above, "{name} {css:?} (free roam {free}): pause {top:.1}..{bottom:.1} px, HUD {hud_top:.1}..{hud:.1} px");
            }
        }
    }

    /// Held upright, the framed view ends above the movement buttons and
    /// Jump, so no thumb control covers play. The page puts a button
    /// `max(8 px, inset) + y vmin` above the bottom edge (the same rule as
    /// the pause button's drop).
    #[test]
    fn upright_the_controls_sit_in_the_bar_below_the_view() {
        for (css, dpr) in [([390.0, 844.0], 3.0), ([360.0, 780.0], 3.0), ([800.0, 1280.0], 1.0)] {
            let (_, s) = hud_rows_css(css, dpr, false);
            assert!(s.origin[1] > 0.0, "{css:?}: not framed");
            let view_bottom = (s.origin[1] + s.view[1] * s.scale) * s.css_per_px[1];
            let vmin = css[0].min(css[1]) / 100.0;
            // Primary buttons: clamp(64px, 18vmin, 88px); Run: clamp(48px, 13vmin, 72px).
            let primary = (18.0 * vmin).clamp(64.0, 88.0);
            let normal = (13.0 * vmin).clamp(48.0, 72.0);
            for (y, size) in [(4.0, primary), (5.0, primary), (3.0, normal)] {
                let control_top = css[1] - 8.0 - y * vmin - size;
                assert!(control_top >= view_bottom + 8.0, "{css:?}: control top {control_top:.1}, view ends at {view_bottom:.1}");
            }
        }
    }
}
