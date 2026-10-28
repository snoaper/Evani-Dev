//! The match HUD in the page: the kit fills [`MatchHud`] (phase banner,
//! score, timer, health, weapon, "Paused"); this writes it into the page's
//! `#tf-match` element and sets the body's `data-phase`, `data-paused` and
//! `data-alive` so the page's CSS shows the crosshair and the pause screen
//! (index.html). The DOM is touched only when something changed.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_kit_tactical_fps::match_rules::{Match, Phase};
use evani_kit_tactical_fps::{MatchHud, Paused};

/// What the page shows this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudView {
    /// The kit's HUD text without the "Paused" line (the pause screen says it).
    pub text: String,
    /// `title`, `pre`, `play` or `results` (the body's `data-phase`).
    pub phase: &'static str,
    /// The match is paused.
    pub paused: bool,
    /// The local player is alive (the crosshair shows only then).
    pub alive: bool,
}

/// This frame's [`HudView`], or `None` before the match exists.
pub fn view(world: &World) -> Option<HudView> {
    let game = world.try_res::<Match>()?;
    let hud = world.try_res::<MatchHud>()?;
    let paused = world.try_res::<Paused>().is_some_and(|p| p.0);
    let text = hud.text.strip_prefix("Paused\n").unwrap_or(&hud.text).to_string();
    let phase = match game.phase {
        Phase::Title => "title",
        Phase::Pre => "pre",
        Phase::Play => "play",
        Phase::Results => "results",
    };
    let alive = game.soldiers.first().is_some_and(|s| s.alive());
    Some(HudView { text, phase, paused, alive })
}

/// The system: after the kit's HUD (order 10), once a frame.
pub fn system() -> SystemDesc {
    SystemDesc::new("tactical-fps.matchhud", Stage::Frame, frame).order(10)
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static SEEN: core::cell::RefCell<Option<HudView>> = const { core::cell::RefCell::new(None) };
}

fn frame(world: &mut World) {
    let Some(_now) = view(world) else {
        return;
    };
    #[cfg(target_arch = "wasm32")]
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        if seen.as_ref() == Some(&_now) {
            return;
        }
        if dom::write(&_now, seen.as_ref()) {
            *seen = Some(_now);
        }
    });
}

#[cfg(target_arch = "wasm32")]
mod dom {
    use super::HudView;

    /// Write what changed since `before`; false when the page is not ready.
    pub fn write(now: &HudView, before: Option<&HudView>) -> bool {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return false;
        };
        let (Some(body), Some(text)) = (doc.body(), doc.get_element_by_id("tf-match")) else {
            return false;
        };
        if before.is_none_or(|b| b.text != now.text) {
            text.set_text_content(Some(&now.text));
        }
        let flag = |b: bool| if b { "1" } else { "0" };
        if before.is_none_or(|b| b.phase != now.phase) {
            let _ = body.set_attribute("data-phase", now.phase);
        }
        if before.is_none_or(|b| b.paused != now.paused) {
            let _ = body.set_attribute("data-paused", flag(now.paused));
        }
        if before.is_none_or(|b| b.alive != now.alive) {
            let _ = body.set_attribute("data-alive", flag(now.alive));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_follows_the_match() {
        let mut app = crate::build_app();
        app.run_headless(5, 1000.0 / 60.0);
        let v = view(&app.world).expect("the match exists");
        assert_eq!(v.phase, "title");
        assert!(v.text.contains("HEXAGON"), "{}", v.text);
        assert!(!v.paused);
        app.world.res_mut::<Paused>().0 = true;
        app.world.res_mut::<MatchHud>().text = "Paused\nHealth 100".into();
        let v = view(&app.world).expect("the match exists");
        assert!(v.paused);
        assert_eq!(v.text, "Health 100", "the pause screen carries the word");
    }
}
