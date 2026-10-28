//! Building the game for `serve` (docs/LIVE_API.md 2.3): a Trunk dev build
//! with the `live` module, at below-normal priority with capped jobs, into
//! `target/evani-live/<game>/dist`, skipped when a current build exists.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Instant, SystemTime};

use tokio::io::AsyncReadExt;

/// What the build stamp records; a different value forces a rebuild.
const STAMP: &str = "evani-live dev build; features = default + live\n";

pub struct Game {
    pub name: String,
    pub app_dir: PathBuf,
    pub dist: PathBuf,
    /// The engine root.
    root: PathBuf,
    /// Where `target/evani-live` and `target/evani-sizes` are: the engine
    /// root, or (`serve --app-dir`) the game's own workspace.
    home: PathBuf,
    /// `serve --app-dir`: a game outside the engine folder. Its build runs
    /// in its own workspace with the engine's wasm cfg passed explicitly.
    pub outside: bool,
    /// Set by [`Game::claim`] when another live bridge already builds this
    /// game: this bridge's own folder (`target/evani-live/<game>-port<P>`),
    /// with its own cargo and dx target dirs.
    own_dir: Option<PathBuf>,
}

/// What [`Game::claim`] took; [`Slot::release`] gives it back at exit.
pub struct Slot {
    /// This bridge builds into its own folder, not the shared one.
    pub alternate: bool,
    lock: Option<PathBuf>,
}

impl Slot {
    pub fn release(&self) {
        if let Some(lock) = &self.lock
            && read_lock(lock).is_some_and(|(pid, _)| pid == std::process::id())
        {
            let _ = std::fs::remove_file(lock);
        }
    }
}

const LOCK: &str = "live-owner.json";

fn read_lock(p: &Path) -> Option<(u32, u16)> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    Some((u32::try_from(v["pid"].as_u64()?).ok()?, u16::try_from(v["port"].as_u64()?).ok()?))
}

impl Game {
    /// `game` is a name (`sandbox`) or a path (`apps/sandbox`).
    pub fn new(root: &Path, game: &str) -> Self {
        let name = Path::new(game)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(game)
            .to_string();
        let out = root.join("target").join("evani-live").join(&name);
        Self {
            app_dir: root.join("apps").join(&name),
            dist: out.join("dist"),
            root: root.to_path_buf(),
            home: root.to_path_buf(),
            outside: false,
            name,
            own_dir: None,
        }
    }

    /// A game outside the engine at `root` (`serve --app-dir`, resolved by
    /// [`evani_guide::app_dir::resolve`]): built from its app folder, into
    /// `<game workspace>/target/evani-live/<id>`.
    pub fn outside(root: &Path, o: &evani_guide::app_dir::Outside) -> Self {
        Self {
            name: o.id.clone(),
            app_dir: o.dir.clone(),
            dist: o.root.join("target").join("evani-live").join(&o.id).join("dist"),
            root: root.to_path_buf(),
            home: o.root.clone(),
            outside: true,
            own_dir: None,
        }
    }

    /// Where this game's live files and sessions live: the engine root, or
    /// the game's workspace.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Take the game's build folder for the bridge on `port` (LIVE_API.md
    /// 2.6). The first bridge gets `target/evani-live/<game>`, as before; one
    /// that starts while another running bridge holds it builds into
    /// `target/evani-live/<game>-port<P>` instead, so two builds of the same
    /// game never write one dist folder or wait on one cargo lock. A holder
    /// counts as running while something answers on its port.
    pub fn claim(&mut self, port: u16) -> Slot {
        let shared = self.home.join("target").join("evani-live").join(&self.name);
        let lock = shared.join(LOCK);
        let doc = serde_json::json!({ "pid": std::process::id(), "port": port }).to_string();
        let _ = std::fs::create_dir_all(&shared);
        for _ in 0..2 {
            let created = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
                .and_then(|mut f| std::io::Write::write_all(&mut f, doc.as_bytes()));
            if created.is_ok() {
                return Slot { alternate: false, lock: Some(lock) };
            }
            match read_lock(&lock) {
                Some((pid, p)) if pid != std::process::id() && p != port && crate::session::port_answers(p) => break,
                // Left by a bridge that is gone (or unreadable): take it over.
                _ => {
                    let _ = std::fs::remove_file(&lock);
                }
            }
        }
        let own = self.home.join("target").join("evani-live").join(format!("{}-port{port}", self.name));
        // Start from the shared build when it is current (a Trunk bridge
        // then serves at once instead of building the game again).
        let shared_current = self.is_current();
        self.dist = own.join("dist");
        self.own_dir = Some(own);
        if shared_current && !self.is_current() {
            let _ = std::fs::remove_dir_all(&self.dist);
            if copy_dir(&shared.join("dist"), &self.dist).is_err() {
                let _ = std::fs::remove_dir_all(&self.dist);
            }
        }
        Slot { alternate: true, lock: None }
    }

    /// The cargo target dir for this game's Trunk build: shared by every
    /// game, or the bridge's own folder after an alternate [`Game::claim`].
    fn cargo_dir(&self) -> PathBuf {
        match &self.own_dir {
            Some(d) => d.join("cargo"),
            None => self.home.join("target").join("evani-live").join("cargo"),
        }
    }

    /// dx's target dir (code.rs), split the same way.
    pub fn dx_dir(&self) -> PathBuf {
        match &self.own_dir {
            Some(d) => d.join("dx"),
            None => self.home.join("target").join("evani-live").join("dx"),
        }
    }

    fn stamp_path(&self) -> PathBuf {
        self.dist.join(".evani-live-build")
    }

    /// The ship build's gzip total from the last `cargo xtask size-report`
    /// (`target/evani-sizes/size-report.json`), shown in the dev build's
    /// overlay next to its own (much bigger) download.
    pub(crate) fn published_gz(&self) -> Option<u64> {
        let path = self.home.join("target").join("evani-sizes").join("size-report.json");
        let text = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.as_array()?
            .iter()
            .find(|a| a.get("app").and_then(|n| n.as_str()) == Some(self.name.as_str()))?
            .get("gz")?
            .as_u64()
    }

    /// What the stamp says for a build made now.
    fn stamp_text(&self) -> String {
        match self.published_gz() {
            Some(gz) => format!("{STAMP}published_gz = {gz}\n"),
            None => STAMP.to_string(),
        }
    }

    /// A build exists, was made with the live module, and is newer than
    /// every source file that goes into it.
    pub fn is_current(&self) -> bool {
        let stamp = self.stamp_path();
        let Ok(text) = std::fs::read_to_string(&stamp) else {
            return false;
        };
        if text != self.stamp_text() || !self.dist.join("index.html").is_file() {
            return false;
        }
        let Some(built) = mtime(&stamp) else { return false };
        let newest = newest_source(&self.root, &self.app_dir).max(if self.outside { newest_in_workspace(&self.home) } else { None });
        newest.is_none_or(|src| src <= built)
    }

    /// Run `trunk build` (dev profile, default features + `live`).
    /// Returns the build time in ms, or the tail of Trunk's output.
    pub async fn build(&self, quiet: bool) -> Result<u64, String> {
        let started = Instant::now();
        let _ = std::fs::remove_file(self.stamp_path());
        let mut cmd = tokio::process::Command::new("trunk");
        cmd.current_dir(&self.app_dir)
            .arg("build")
            .arg("--features")
            .arg("live")
            .arg("--dist")
            .arg(&self.dist)
            // Its own cargo target dir, so a game build never holds the lock
            // that `cargo run -p evani-live-bridge -- mcp` needs.
            .env("CARGO_TARGET_DIR", self.cargo_dir())
            // Trunk reads NO_COLOR as a boolean flag and refuses "1" (Evani.App sets it).
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
            cmd.env("CARGO_BUILD_JOBS", "4");
        }
        if self.outside {
            // The game's workspace doesn't read the engine's .cargo/config.toml.
            cmd.env(evani_guide::app_dir::WASM_RUSTFLAGS_ENV, outside_rustflags(std::env::var(evani_guide::app_dir::WASM_RUSTFLAGS_ENV).ok().as_deref()));
        }
        let stamp = self.stamp_text();
        match self.published_gz() {
            Some(gz) => cmd.env("EVANI_PUBLISHED_GZ", gz.to_string()),
            None => cmd.env_remove("EVANI_PUBLISHED_GZ"),
        };
        below_normal(&mut cmd);
        if !quiet {
            eprintln!("evani-live-bridge: building {} (trunk build --features live)…", self.name);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot run trunk ({e}); install it with `cargo install trunk --locked`"))?;
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let read_out = async {
            if let Some(s) = out.as_mut() {
                let _ = s.read_to_end(&mut o).await;
            }
        };
        let read_err = async {
            if let Some(s) = err.as_mut() {
                let _ = s.read_to_end(&mut e).await;
            }
        };
        let (_, _, status) = tokio::join!(read_out, read_err, child.wait());
        let status = status.map_err(|e| format!("trunk: {e}"))?;
        if !status.success() {
            let mut log = String::from_utf8_lossy(&o).into_owned();
            log.push_str(&String::from_utf8_lossy(&e));
            let tail: Vec<&str> = log.lines().rev().take(30).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            return Err(format!("The game didn't build:\n{}", tail.join("\n")));
        }
        std::fs::write(self.stamp_path(), stamp).map_err(|e| format!("build stamp: {e}"))?;
        Ok(started.elapsed().as_millis() as u64)
    }
}

#[cfg(windows)]
pub(crate) fn below_normal(cmd: &mut tokio::process::Command) {
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // cargo and rustc inherit the below-normal class from trunk.
    cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn below_normal(_cmd: &mut tokio::process::Command) {}

/// Copy a build folder, keeping file times (the stamp's decides `is_current`).
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), &dest)?;
            if let Some(t) = mtime(&e.path()) {
                std::fs::File::options().write(true).open(&dest)?.set_modified(t)?;
            }
        }
    }
    Ok(())
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Newest modification time of anything that goes into the game: the
/// workspace manifests, the engine crates, kits, looks and the app itself.
fn newest_source(root: &Path, app_dir: &Path) -> Option<SystemTime> {
    let mut newest = None;
    for f in [root.join("Cargo.toml"), root.join("Cargo.lock")] {
        newest = newest.max(mtime(&f));
    }
    for dir in [root.join("crates"), root.join("kits"), root.join("looks"), app_dir.to_path_buf()] {
        walk(&dir, &mut newest);
    }
    newest
}

/// An outside game's own workspace: its manifests and every source file in
/// it (its app, kit and anything else it builds from).
fn newest_in_workspace(ws: &Path) -> Option<SystemTime> {
    let mut newest = None;
    walk(ws, &mut newest);
    newest
}

/// The wasm rustflags for an outside game's build: what the caller set,
/// plus the engine's cfg (once).
fn outside_rustflags(set: Option<&str>) -> String {
    let cfg = evani_guide::app_dir::WEB_SYS_CFG;
    match set.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.split_whitespace().any(|f| f == cfg) => s.to_string(),
        Some(s) => format!("{s} {cfg}"),
        None => cfg.to_string(),
    }
}

fn walk(dir: &Path, newest: &mut Option<SystemTime>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "target" | "dist" | "node_modules") {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, newest),
            Ok(_) => *newest = (*newest).max(mtime(&p)),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outside_game_builds_in_its_own_workspace() {
        let o = evani_guide::app_dir::Outside {
            id: "sea".into(),
            dir: PathBuf::from("C:/g/app"),
            root: PathBuf::from("C:/g"),
            kit_dir: PathBuf::from("C:/g/kit"),
        };
        let mut g = Game::outside(Path::new("C:/e"), &o);
        assert!(g.outside);
        assert_eq!((g.name.as_str(), g.app_dir.as_path(), g.home()), ("sea", Path::new("C:/g/app"), Path::new("C:/g")));
        assert_eq!(g.dist, Path::new("C:/g/target/evani-live/sea/dist"));
        assert_eq!(g.cargo_dir(), Path::new("C:/g/target/evani-live/cargo"));
        g.own_dir = Some(PathBuf::from("C:/g/target/evani-live/sea-port9"));
        assert_eq!(g.dx_dir(), Path::new("C:/g/target/evani-live/sea-port9/dx"));
        assert_eq!(outside_rustflags(None), "--cfg=web_sys_unstable_apis");
        assert_eq!(outside_rustflags(Some("-C opt-level=1")), "-C opt-level=1 --cfg=web_sys_unstable_apis");
        assert_eq!(outside_rustflags(Some("--cfg=web_sys_unstable_apis")), "--cfg=web_sys_unstable_apis");
        // A game in apps/ is unchanged.
        let inside = Game::new(Path::new("C:/e"), "sandbox");
        assert!(!inside.outside && inside.home() == Path::new("C:/e"));
    }

    #[test]
    fn names_and_paths() {
        let root = Path::new("C:/r");
        let g = Game::new(root, "apps/sandbox");
        assert_eq!(g.name, "sandbox");
        assert_eq!(g.app_dir, root.join("apps").join("sandbox"));
        assert!(g.dist.ends_with(Path::new("target/evani-live/sandbox/dist")));
        assert_eq!(Game::new(root, "sandbox").name, "sandbox");
    }

    /// Two bridges on one game: the second builds into its own folder (with
    /// the first's current build copied in); a stale holder is taken over.
    #[test]
    fn a_second_bridge_of_one_game_gets_its_own_folder() {
        let dir = std::env::temp_dir().join(format!("evani-live-claim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let live = dir.join("target").join("evani-live");
        // The first bridge, another process on a port that answers.
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let held_port = held.local_addr().unwrap().port();
        std::fs::create_dir_all(live.join("sandbox").join("dist")).unwrap();
        std::fs::write(live.join("sandbox").join(LOCK), format!(r#"{{"pid":1,"port":{held_port}}}"#)).unwrap();
        let first = Game::new(&dir, "sandbox");
        std::fs::write(first.dist.join("index.html"), "<html>").unwrap();
        std::fs::write(first.stamp_path(), STAMP).unwrap();
        let mut g = Game::new(&dir, "apps/sandbox");
        let slot = g.claim(8123);
        assert!(slot.alternate);
        assert!(g.dist.ends_with(Path::new("target/evani-live/sandbox-port8123/dist")));
        assert!(g.dx_dir().ends_with(Path::new("target/evani-live/sandbox-port8123/dx")));
        assert!(g.cargo_dir().ends_with(Path::new("target/evani-live/sandbox-port8123/cargo")));
        assert!(g.is_current(), "the first bridge's current build is copied in");
        slot.release();
        assert!(live.join("sandbox").join(LOCK).exists(), "not ours to release");
        // The first bridge goes away: the next one takes the shared folder.
        drop(held);
        let mut g = Game::new(&dir, "sandbox");
        let slot = g.claim(8124);
        assert!(!slot.alternate);
        assert!(g.dist.ends_with(Path::new("target/evani-live/sandbox/dist")));
        assert!(g.dx_dir().ends_with(Path::new("target/evani-live/dx")));
        assert_eq!(read_lock(&live.join("sandbox").join(LOCK)), Some((std::process::id(), 8124)));
        slot.release();
        assert!(!live.join("sandbox").join(LOCK).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_build_is_not_current() {
        let dir = std::env::temp_dir().join(format!("evani-live-game-{}", std::process::id()));
        let g = Game::new(&dir, "sandbox");
        assert!(!g.is_current());
        std::fs::create_dir_all(&g.dist).unwrap();
        std::fs::write(g.dist.join("index.html"), "<html>").unwrap();
        std::fs::write(g.stamp_path(), STAMP).unwrap();
        assert!(g.is_current(), "no sources at all: the build is current");
        std::fs::write(g.stamp_path(), "old").unwrap();
        assert!(!g.is_current());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every game the bridge serves finds its own published size, and a new
    /// report changes the stamp (so the dev build is rebuilt with it).
    #[test]
    fn published_size_is_read_per_game() {
        let dir = std::env::temp_dir().join(format!("evani-live-sizes-{}", std::process::id()));
        let sizes = dir.join("target").join("evani-sizes");
        std::fs::create_dir_all(&sizes).unwrap();
        let kart = Game::new(&dir, "apps/kart-racer");
        assert_eq!(kart.published_gz(), None);
        let before = kart.stamp_text();
        std::fs::write(
            sizes.join("size-report.json"),
            r#"[{"app": "sandbox", "gz": 234190}, {"app": "kart-racer", "gz": 306000}]"#,
        )
        .unwrap();
        assert_eq!(kart.published_gz(), Some(306_000));
        assert_eq!(Game::new(&dir, "sandbox").published_gz(), Some(234_190));
        assert_eq!(Game::new(&dir, "match-3").published_gz(), None);
        assert_ne!(kart.stamp_text(), before);
        assert!(kart.stamp_text().ends_with("published_gz = 306000\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
