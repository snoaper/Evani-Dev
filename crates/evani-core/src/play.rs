//! Play mode: the normal game ([`PlayMode::Play`], the default) or free roam
//! ([`PlayMode::FreeRoam`]).
//!
//! Free roam is the same world with no goals, timer, lives, fail state or
//! game over: the player moves around freely. The Evani.app site opens it
//! with `?mode=free` on the preview page and `#mode=free` on the demo frame;
//! on the web [`crate::App::new`] reads both. Natively (tests, tools) set it
//! with [`crate::App::with_play_mode`] before the kit is added.
//!
//! Every playground (a game built for the Evani.app library,
//! docs/PLAYGROUNDS.md) must support free roam (Chris's rule, DECISIONS.md
//! 2026-10-15): its kit returns true from `KitPlugin::free_roam`, drops its
//! goals and fail state while [`PlayMode::FreeRoam`], reports fail events in
//! [`PlayOutcomes`], and has a test that calls [`free_roam_test`].
//! `cargo xtask check` and `cargo xtask build <id> --ship` enforce it.

/// How the game is played. World resource, fixed before the kit is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    /// The game as designed: goals, timer, lives, game over.
    #[default]
    Play,
    /// The same world with no goals, timer, lives, fail state or game over.
    FreeRoam,
}

impl PlayMode {
    /// The mode a page address asks for: `mode=free` in the query
    /// (`location.search`, e.g. `?mode=free`) or the fragment
    /// (`location.hash`, e.g. `#mode=free`) is free roam; anything else is
    /// play. Each part may hold other `&`-separated pairs.
    pub fn from_url(search: &str, hash: &str) -> Self {
        let asks = |part: &str| {
            part.split(['?', '#', '&'])
                .any(|pair| pair == "mode=free")
        };
        if asks(search) || asks(hash) {
            Self::FreeRoam
        } else {
            Self::Play
        }
    }

    pub fn is_free_roam(self) -> bool {
        self == Self::FreeRoam
    }

    /// `play` or `free roam`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::FreeRoam => "free roam",
        }
    }
}

/// Fail events the kit reports (each call counts one). A free-roam run must
/// end with none of them ([`free_roam_test`]). World resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayOutcomes {
    pub game_overs: u32,
    pub timers_expired: u32,
    pub lives_lost: u32,
}

impl PlayOutcomes {
    /// The game ended (lost, won-and-over, crashed out).
    pub fn game_over(&mut self) {
        self.game_overs += 1;
    }

    /// A countdown or time limit ran out.
    pub fn timer_expired(&mut self) {
        self.timers_expired += 1;
    }

    /// The player lost a life (or a retry, a heart, a respawn penalty).
    pub fn life_lost(&mut self) {
        self.lives_lost += 1;
    }

    /// Any fail event at all.
    pub fn any(&self) -> bool {
        self.game_overs + self.timers_expired + self.lives_lost > 0
    }
}

/// The mode the current page asks for (wasm; [`PlayMode::Play`] when there
/// is no window).
#[cfg(all(target_arch = "wasm32", not(evani_headless)))]
pub(crate) fn from_page() -> PlayMode {
    // One call (`location.href`) keeps the JS glue small. The whole address
    // works as one part: the path never splits into a bare `mode=free`.
    let href = web_sys::window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    PlayMode::from_url(&href, "")
}

/// One scripted key change for [`free_roam_test`]: `key` (a
/// `KeyboardEvent.code` such as `Space`, `KeyW`, `ArrowLeft`) goes down or up
/// at `at` seconds of simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptKey {
    pub at: f64,
    pub key: &'static str,
    pub down: bool,
}

impl ScriptKey {
    pub fn down(at: f64, key: &'static str) -> Self {
        Self { at, key, down: true }
    }

    pub fn up(at: f64, key: &'static str) -> Self {
        Self {
            at,
            key,
            down: false,
        }
    }
}

/// What a passing [`free_roam_test`] ran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreeRoamReport {
    /// Frames of 1/60 s that ran.
    pub frames: u32,
    /// Simulated seconds at the end (`Time::sim_seconds`).
    pub sim_seconds: f64,
    /// Scripted key changes fed in.
    pub keys: usize,
}

/// Run `app` (built with `.with_play_mode(PlayMode::FreeRoam)` before
/// `.with_kit(..)`) for `seconds` of 60 Hz frames, feeding `script` as key
/// events at their times, and check free roam holds: the mode is still free
/// roam (the kit says it supports it), no game over, timer expiry or lost
/// life was reported in [`PlayOutcomes`], and no error was logged. Call it
/// as `free_roam_test(&mut app, 120.0, &script)` in a `#[test]`, then add the
/// kit's own checks on `app` (the player still in the world, still moving).
#[cfg(not(all(target_arch = "wasm32", not(evani_headless))))]
pub fn free_roam_test(
    app: &mut crate::App,
    seconds: f64,
    script: &[ScriptKey],
) -> Result<FreeRoamReport, String> {
    use crate::input::{Input, KeyCode, RawEvent};
    use crate::overlay::{ErrorLog, Severity};
    use crate::plugin::BuildInfo;
    use crate::time::Time;

    const DT_MS: f64 = 1000.0 / 60.0;
    let errors = |app: &crate::App| -> Option<String> {
        let log = app.world.res::<ErrorLog>();
        (log.error_count() > 0).then(|| {
            let list: Vec<&str> = log
                .entries()
                .filter(|e| e.severity == Severity::Error)
                .map(|e| e.message.as_str())
                .collect();
            format!("{} error(s) logged: {}", log.error_count(), list.join("; "))
        })
    };

    let Some(kit) = app.world.res::<BuildInfo>().kit.clone() else {
        return Err("no kit: build the app with `.with_kit(..)`".into());
    };
    if !app.play_mode().is_free_roam() {
        return Err(format!(
            "the game runs in play mode, not free roam: build it with `.with_play_mode(PlayMode::FreeRoam)` before `.with_kit(..)`, and return true from `KitPlugin::free_roam` in {kit}"
        ));
    }
    if !(seconds.is_finite() && seconds > 0.0) {
        return Err(format!("run for a positive number of seconds, not {seconds}"));
    }
    let mut keys = Vec::with_capacity(script.len());
    for k in script {
        let code = KeyCode::new(k.key).ok_or_else(|| format!("`{}` is not a key code", k.key))?;
        keys.push((k.at, code, k.down));
    }
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));

    let frames = (seconds * 60.0).ceil() as u32;
    let mut next = 0;
    for frame in 0..frames {
        let now = f64::from(frame) / 60.0;
        while let Some(&(at, code, down)) = keys.get(next) {
            if at > now {
                break;
            }
            app.world.res_mut::<Input>().handle(&RawEvent::Key { code, down });
            next += 1;
        }
        app.run_headless(1, DT_MS);
        let outcomes = *app.world.res::<PlayOutcomes>();
        if outcomes.any() {
            return Err(format!(
                "free roam failed at {:.2} s: {outcomes:?} (free roam has no game over, timer or lives)",
                app.world.res::<Time>().sim_seconds
            ));
        }
        if let Some(e) = errors(app) {
            return Err(format!("free roam at {now:.2} s: {e}"));
        }
    }
    if !app.play_mode().is_free_roam() {
        return Err("the play mode changed during the run; it is fixed at start".into());
    }
    Ok(FreeRoamReport {
        frames,
        sim_seconds: app.world.res::<Time>().sim_seconds,
        keys: next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Stage, SystemDesc, World};
    use crate::input::{ActionDef, Input};
    use crate::overlay::ErrorLog;
    use crate::plugin::{KitPlugin, PluginContext, PluginError};
    use crate::App;

    #[test]
    fn reads_the_address() {
        assert_eq!(PlayMode::from_url("", ""), PlayMode::Play);
        assert_eq!(PlayMode::from_url("?mode=free", ""), PlayMode::FreeRoam);
        assert_eq!(PlayMode::from_url("", "#mode=free"), PlayMode::FreeRoam);
        assert_eq!(PlayMode::from_url("?a=1&mode=free&b=2", ""), PlayMode::FreeRoam);
        assert_eq!(PlayMode::from_url("", "#x&mode=free"), PlayMode::FreeRoam);
        // The web reads the whole address as one part.
        for href in ["http://h/p/index.html?mode=free", "https://h/p?a=1#mode=free", "http://h/?x=2&mode=free#top"] {
            assert_eq!(PlayMode::from_url(href, ""), PlayMode::FreeRoam, "{href}");
        }
        for href in ["http://h/mode=free", "http://h/p?mode=play#x", "http://h/p#mode=free2"] {
            assert_eq!(PlayMode::from_url(href, ""), PlayMode::Play, "{href}");
        }
        for other in ["?mode=play", "?mode=freeroam", "?xmode=free", "?mode=FREE", "?mode", "mode"] {
            assert_eq!(PlayMode::from_url(other, other), PlayMode::Play, "{other}");
        }
        assert_eq!(PlayMode::default(), PlayMode::Play);
    }

    /// A kit with a 2 s timer and a jump on Space that costs a life, both of
    /// which free roam drops.
    struct Timed {
        roams: bool,
    }

    #[derive(Default)]
    struct Clock(f64);

    fn tick(world: &mut World) {
        let free = world.res::<PlayMode>().is_free_roam();
        let dt = f64::from(world.res::<crate::time::Time>().fixed_dt);
        let t = {
            let mut c = world.res_mut::<Clock>();
            c.0 += dt;
            c.0
        };
        let jump = world
            .res::<Input>()
            .action_id("jump")
            .is_some_and(|id| world.res::<Input>().just_pressed(id));
        if free {
            return;
        }
        if t >= 2.0 && t - dt < 2.0 {
            let mut o = world.res_mut::<PlayOutcomes>();
            o.timer_expired();
            o.game_over();
        }
        if jump {
            world.res_mut::<PlayOutcomes>().life_lost();
        }
    }

    impl KitPlugin for Timed {
        fn name(&self) -> &'static str {
            "timed"
        }
        fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
            kit.action(ActionDef::button("jump", &["key:Space"])?);
            kit.resource(Clock::default());
            kit.system(SystemDesc::new("timed.tick", Stage::Update, tick));
            Ok(())
        }
        fn free_roam(&self) -> bool {
            self.roams
        }
    }

    fn script() -> Vec<ScriptKey> {
        vec![ScriptKey::down(0.5, "Space"), ScriptKey::up(0.6, "Space")]
    }

    #[test]
    fn free_roam_runs_without_fail_events() {
        let mut app = App::new("t")
            .with_play_mode(PlayMode::FreeRoam)
            .with_kit(Timed { roams: true });
        assert_eq!(*app.world.res::<PlayMode>(), PlayMode::FreeRoam);
        let r = free_roam_test(&mut app, 5.0, &script()).unwrap();
        assert_eq!(r.frames, 300);
        assert_eq!(r.keys, 2);
        assert!((r.sim_seconds - 5.0).abs() < 0.05, "{r:?}");
    }

    #[test]
    fn play_mode_reports_what_free_roam_drops() {
        let mut app = App::new("t").with_kit(Timed { roams: true });
        assert_eq!(app.play_mode(), PlayMode::Play);
        app.run_headless(30, 1000.0 / 60.0);
        let code = crate::input::KeyCode::new("Space").unwrap();
        app.world
            .res_mut::<Input>()
            .handle(&crate::input::RawEvent::Key { code, down: true });
        app.run_headless(150, 1000.0 / 60.0);
        let o = *app.world.res::<PlayOutcomes>();
        assert_eq!((o.game_overs, o.timers_expired, o.lives_lost), (1, 1, 1));
        assert!(o.any());
        // And the helper refuses a game that isn't in free roam.
        let e = free_roam_test(&mut App::new("t").with_kit(Timed { roams: true }), 1.0, &[]).unwrap_err();
        assert!(e.contains("with_play_mode"), "{e}");
    }

    #[test]
    fn a_kit_without_free_roam_plays_normally() {
        let mut app = App::new("t")
            .with_play_mode(PlayMode::FreeRoam)
            .with_kit(Timed { roams: false });
        assert_eq!(app.play_mode(), PlayMode::Play);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let e = free_roam_test(&mut app, 1.0, &[]).unwrap_err();
        assert!(e.contains("KitPlugin::free_roam"), "{e}");
    }

    /// A kit that says it roams but still runs its timer is caught.
    #[test]
    fn the_helper_catches_a_fail_event() {
        struct Liar;
        impl KitPlugin for Liar {
            fn name(&self) -> &'static str {
                "liar"
            }
            fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
                kit.system(SystemDesc::new("liar.over", Stage::Update, |w: &mut World| {
                    if w.res::<crate::time::Time>().sim_seconds > 1.0 {
                        w.res_mut::<PlayOutcomes>().game_over();
                    }
                }));
                Ok(())
            }
            fn free_roam(&self) -> bool {
                true
            }
        }
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(Liar);
        let e = free_roam_test(&mut app, 3.0, &[]).unwrap_err();
        assert!(e.contains("game_overs: 1"), "{e}");
    }

    #[test]
    fn errors_and_bad_input_fail_the_run() {
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(Timed { roams: true });
        app.error("boom");
        let e = free_roam_test(&mut app, 1.0, &[]).unwrap_err();
        assert!(e.contains("boom"), "{e}");
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(Timed { roams: true });
        assert!(free_roam_test(&mut app, 1.0, &[ScriptKey::down(0.0, "")]).is_err());
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(Timed { roams: true });
        assert!(free_roam_test(&mut app, 0.0, &[]).is_err());
        assert!(free_roam_test(&mut App::new("t").with_play_mode(PlayMode::FreeRoam), 1.0, &[]).unwrap_err().contains("no kit"));
    }

    #[test]
    fn the_mode_is_fixed_once_a_kit_is_built() {
        let mut app = App::new("t").with_kit(Timed { roams: true });
        app.set_play_mode(PlayMode::FreeRoam);
        assert_eq!(app.play_mode(), PlayMode::Play);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 1);
    }

    #[test]
    fn start_says_free_roam() {
        let mut app = App::new("t").with_play_mode(PlayMode::FreeRoam).with_kit(Timed { roams: true });
        app.start();
        let log = app.world.res::<ErrorLog>();
        assert!(log.entries().any(|e| e.message == "play mode: free roam"));
        drop(log);
        let mut app = App::new("t").with_kit(Timed { roams: true });
        app.start();
        assert!(!app.world.res::<ErrorLog>().entries().any(|e| e.message.starts_with("play mode")));
    }
}
