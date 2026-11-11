//! Rebuild on change for a game outside the engine folder (`serve --app-dir`,
//! docs/LIVE_API.md 10.5). Such a game is built with Trunk (live code
//! swapping with dx is in-tree only), and before this module only once, at
//! start. Now its source files are watched and the game is rebuilt when they
//! change, and also when the page is reloaded and the build is out of date.
//!
//! No new dependency and no busy loop: one stat-walk of the sources a second
//! ([`POLL`]), a rebuild only after the newest modification time has held
//! still for one whole poll (a debounce: a save-all or a formatter's burst of
//! writes is one build), and one build at a time. The comparison is against
//! the newest source time seen *before* the build started, so a file saved
//! while a build runs makes the next build happen too.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;

use crate::hub::{BuildState, Hub};

/// How often the sources are looked at.
pub const POLL: Duration = Duration::from_millis(1000);

/// The newest source: its modification time and the file that has it.
pub type Newest = Option<(SystemTime, PathBuf)>;

/// What a rebuild does: the Trunk build, answering its time in ms.
pub type BuildFuture = Pin<Box<dyn Future<Output = Result<u64, String>> + Send>>;

/// When to rebuild, with no clock and no files: fed the newest source time
/// each poll.
#[derive(Debug, Default)]
pub struct Tracker {
    /// The newest source time the served build was started from.
    built_from: Option<SystemTime>,
    /// The newest time seen while waiting for the changes to settle.
    seen: Option<SystemTime>,
    /// Polls in a row that saw `seen` unchanged.
    quiet: u32,
    /// A build exists that a page may have loaded.
    served: bool,
}

impl Tracker {
    /// `built_from`: the newest source time of the build that already exists
    /// (`None`: there is none, so the first poll builds).
    pub fn new(built_from: Option<SystemTime>) -> Self {
        Self { built_from, served: built_from.is_some(), ..Self::default() }
    }

    /// The sources are newer than the build.
    pub fn stale(&self, newest: Option<SystemTime>) -> bool {
        newest.is_some_and(|n| self.built_from.is_none_or(|b| n > b))
    }

    /// One look at the sources: `true` means build now. `force` (a page
    /// reload, the first build) skips the wait for the changes to settle.
    pub fn poll(&mut self, newest: Option<SystemTime>, force: bool) -> bool {
        if !self.stale(newest) {
            self.seen = None;
            self.quiet = 0;
            return false;
        }
        if force {
            return true;
        }
        if self.seen == newest {
            self.quiet += 1;
        } else {
            self.seen = newest;
            self.quiet = 0;
        }
        self.quiet >= 1
    }

    /// A build from sources as new as `from` finished (or failed: a broken
    /// build waits for the next edit, it is not retried every second).
    pub fn done(&mut self, from: Option<SystemTime>) {
        self.built_from = from;
        self.seen = None;
        self.quiet = 0;
    }
}

pub struct Rebuilder {
    probe: Arc<dyn Fn() -> Newest + Send + Sync>,
    build: Box<dyn Fn() -> BuildFuture + Send + Sync>,
    hub: Arc<Hub>,
    quiet: bool,
    /// Held for the whole of a build: one at a time, and a check that finds
    /// it taken knows a build is running.
    tracker: Arc<tokio::sync::Mutex<Tracker>>,
}

impl Rebuilder {
    /// `probe`: the newest modification time of everything that goes into the
    /// game. `current`: a build of those sources already exists.
    pub fn new(
        hub: Arc<Hub>,
        probe: Arc<dyn Fn() -> Newest + Send + Sync>,
        build: Box<dyn Fn() -> BuildFuture + Send + Sync>,
        current: bool,
        quiet: bool,
    ) -> Arc<Self> {
        let built_from = if current { probe().map(|n| n.0) } else { None };
        if current {
            hub.set_build(BuildState { phase: "idle", error: None, build_ms: 0 });
        }
        Arc::new(Self { probe, build, hub, quiet, tracker: Arc::new(tokio::sync::Mutex::new(Tracker::new(built_from))) })
    }

    /// Look at the sources and, if they are newer than the build (and, unless
    /// `force`, have stopped changing), start a build in the background.
    /// `true`: a build was started. The hub shows `building` before this
    /// answers, so a page request right after gets the holding page.
    pub async fn start_if_stale(self: &Arc<Self>, force: bool) -> bool {
        let Ok(mut tracker) = self.tracker.clone().try_lock_owned() else {
            return false; // a build is running
        };
        let probe = self.probe.clone();
        let Ok(found) = tokio::task::spawn_blocking(move || probe()).await else {
            return false;
        };
        let newest = found.as_ref().map(|n| n.0);
        if !tracker.poll(newest, force) {
            return false;
        }
        if !self.quiet {
            match &found {
                Some((_, file)) => eprintln!("evani-live-bridge: rebuilding ({} changed)", file.display()),
                None => eprintln!("evani-live-bridge: rebuilding"),
            }
        }
        self.hub.set_build(BuildState::default());
        let me = self.clone();
        tokio::spawn(async move {
            let reload = tracker.served;
            let state = match (me.build)().await {
                Ok(ms) => {
                    if !me.quiet {
                        eprintln!("evani-live-bridge: game rebuilt in {:.1} s", ms as f64 / 1000.0);
                    }
                    BuildState { phase: "idle", error: None, build_ms: ms }
                }
                Err(e) => {
                    eprintln!("evani-live-bridge: {e}");
                    me.hub.dev_news(&json!({ "type": "live_status", "at": crate::hub::now_ms(), "state": "error", "message": e }));
                    BuildState { phase: "error", error: Some(e), build_ms: 0 }
                }
            };
            let built = state.phase == "idle";
            tracker.done(newest);
            tracker.served |= built;
            me.hub.set_build(state);
            // A page that is open runs the old build: it reloads into the new one.
            if built && reload {
                me.hub.code_reload("The game's source changed and it was rebuilt; the page reloads with the new build.");
            }
        });
        true
    }

    /// Wait until no build is running (tests).
    #[cfg(test)]
    pub async fn settled(&self) {
        let _ = self.tracker.lock().await;
    }

    /// The watch loop: the first look builds at once, then one look per
    /// [`POLL`], for as long as the bridge runs.
    pub async fn watch(self: Arc<Self>) {
        let mut force = true;
        loop {
            self.start_if_stale(force).await;
            force = false;
            tokio::time::sleep(POLL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::hub::HubInfo;

    fn t(secs: u64) -> Option<SystemTime> {
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[test]
    fn tracker_waits_for_a_change_to_settle() {
        let mut k = Tracker::new(t(10));
        assert!(!k.poll(t(10), false), "nothing newer than the build");
        assert!(!k.poll(t(11), false), "a change is seen, not built yet");
        assert!(!k.poll(t(12), false), "still changing (a burst of saves)");
        assert!(k.poll(t(12), false), "held still for a poll: build");
        k.done(t(12));
        assert!(!k.poll(t(12), false));
    }

    #[test]
    fn tracker_force_skips_the_wait_and_a_missing_build_builds() {
        let mut k = Tracker::new(t(10));
        assert!(k.poll(t(11), true), "a reload builds at once");
        let mut none = Tracker::new(None);
        assert!(none.poll(t(1), true), "no build yet: the first look builds");
        assert!(!Tracker::new(None).poll(None, true), "no sources, nothing to build");
        // A file saved during a build: the next look is stale again.
        none.done(t(1));
        assert!(none.stale(t(2)));
    }

    fn rebuilder(sources: &Arc<std::sync::Mutex<Option<SystemTime>>>, builds: &Arc<AtomicUsize>, fail: bool, current: bool) -> (Arc<Hub>, Arc<Rebuilder>) {
        let hub = Arc::new(Hub::new(HubInfo { port: 0, root: "C:\\e".into(), game: "g".into(), session: "s".into() }));
        let s = sources.clone();
        let probe: Arc<dyn Fn() -> Newest + Send + Sync> = Arc::new(move || s.lock().unwrap().map(|t| (t, PathBuf::from("src/main.rs"))));
        let b = builds.clone();
        let build: Box<dyn Fn() -> BuildFuture + Send + Sync> = Box::new(move || {
            let b = b.clone();
            Box::pin(async move {
                b.fetch_add(1, Ordering::SeqCst);
                if fail { Err("The game didn't build:\nerror[E0425]".to_string()) } else { Ok(7) }
            })
        });
        (hub.clone(), Rebuilder::new(hub, probe, build, current, true))
    }

    #[tokio::test]
    async fn a_changed_source_is_rebuilt_once_and_the_page_reloads() {
        let sources = Arc::new(std::sync::Mutex::new(t(10)));
        let builds = Arc::new(AtomicUsize::new(0));
        let (hub, r) = rebuilder(&sources, &builds, false, true);
        assert_eq!(hub.build().phase, "idle", "a current build is served as it is");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        hub.attach_engine(tx);

        assert!(!r.start_if_stale(false).await, "unchanged sources: no build");
        *sources.lock().unwrap() = t(20);
        assert!(!r.start_if_stale(false).await, "first sight of the edit: wait for it to settle");
        assert!(r.start_if_stale(false).await, "unchanged for a poll: rebuild");
        assert_eq!(hub.build().phase, "building", "a page request now gets the holding page");
        r.settled().await;
        assert_eq!((builds.load(Ordering::SeqCst), hub.build().phase, hub.build().build_ms), (1, "idle", 7));
        let sent = rx.try_recv().expect("the open page is told to reload");
        assert!(sent.contains("code_reload"), "{sent}");
        assert!(!r.start_if_stale(false).await && !r.start_if_stale(true).await, "built: nothing more to do");
        assert_eq!(builds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_reload_rebuilds_an_out_of_date_game_without_waiting() {
        let sources = Arc::new(std::sync::Mutex::new(t(10)));
        let builds = Arc::new(AtomicUsize::new(0));
        let (hub, r) = rebuilder(&sources, &builds, false, true);
        *sources.lock().unwrap() = t(30);
        assert!(r.start_if_stale(true).await, "force: no settle wait");
        r.settled().await;
        assert_eq!((builds.load(Ordering::SeqCst), hub.build().phase), (1, "idle"));
    }

    #[tokio::test]
    async fn the_first_build_and_a_broken_build() {
        let sources = Arc::new(std::sync::Mutex::new(t(10)));
        let builds = Arc::new(AtomicUsize::new(0));
        let (hub, r) = rebuilder(&sources, &builds, true, false);
        assert_eq!(hub.build().phase, "building", "no build yet");
        assert!(r.start_if_stale(true).await, "the first look builds at once");
        r.settled().await;
        let b = hub.build();
        assert_eq!(b.phase, "error");
        assert!(b.error.is_some_and(|e| e.contains("E0425")));
        // A broken build is not retried until the source changes again.
        assert!(!r.start_if_stale(false).await && !r.start_if_stale(true).await);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        *sources.lock().unwrap() = t(11);
        assert!(!r.start_if_stale(false).await);
        assert!(r.start_if_stale(false).await);
        r.settled().await;
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }
}
