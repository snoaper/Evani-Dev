//! `<root>/.evani/live/session.json` (docs/LIVE_API.md 2.4): how MCP and
//! scripts find the running bridge and its token. Rewritten at every start,
//! deleted on clean exit. `.evani/` is git-ignored.
//!
//! A bridge started with `--session <file>` (the review's and the test
//! scripts', under `.evani/live/review/<port>/`) writes and removes that
//! file instead, and never touches the Evani.dev tab's.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::proto::PROTOCOL;

pub fn path(root: &Path) -> PathBuf {
    root.join(".evani").join("live").join("session.json")
}

/// Several live sessions in one folder (LIVE_API.md 2.6): every bridge on
/// the default session file also writes `.evani/live/sessions/<port>.json`;
/// `session.json` is a copy of the most recently started one.
pub fn sessions_dir(root: &Path) -> PathBuf {
    root.join(".evani").join("live").join("sessions")
}

pub fn port_path(root: &Path, port: u16) -> PathBuf {
    sessions_dir(root).join(format!("{port}.json"))
}

pub struct Session {
    pub ws: String,
    pub token: String,
    pub pid: u32,
}

/// One session in [`list`]: what `live_sessions` shows.
#[derive(Debug, Clone)]
pub struct Info {
    pub file: PathBuf,
    /// `live` (sessions/), `model` (the Model tab's) or `review`.
    pub kind: &'static str,
    pub port: u16,
    pub game: String,
    pub pid: u32,
    pub started: String,
    pub started_ms: u64,
    /// Something answers on its port (a crashed bridge leaves a stale file).
    pub alive: bool,
}

/// Port of a `ws://127.0.0.1:<port>` address.
fn ws_port(ws: &str) -> Option<u16> {
    ws.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

/// Does anything listen on `127.0.0.1:<port>`? (A cheap liveness check
/// without a process API: a bridge that died leaves its file but not its port.)
pub fn port_answers(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250)).is_ok()
}

fn info_at(p: &Path, kind: &'static str) -> Option<Info> {
    let text = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v["protocol"].as_str() != Some(PROTOCOL) {
        return None;
    }
    let port = ws_port(v["ws"].as_str()?)?;
    let started_ms = v["started_ms"].as_u64().unwrap_or_else(|| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64)
    });
    Some(Info {
        file: p.to_path_buf(),
        kind,
        port,
        game: v["game"].as_str().unwrap_or("").to_string(),
        pid: v["pid"].as_u64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        started: v["started"].as_str().unwrap_or("").to_string(),
        started_ms,
        alive: port_answers(port),
    })
}

/// Every session file under `.evani/live/`: `sessions/*.json`, the Model
/// tab's `model/session.json` and the review bridges' `review/*/session.json`,
/// newest first.
pub fn list(root: &Path) -> Vec<Info> {
    let live = root.join(".evani").join("live");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(sessions_dir(root)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                out.extend(info_at(&p, "live"));
            }
        }
    }
    out.extend(info_at(&live.join("model").join("session.json"), "model"));
    if let Ok(rd) = std::fs::read_dir(live.join("review")) {
        for e in rd.flatten() {
            out.extend(info_at(&e.path().join("session.json"), "review"));
        }
    }
    out.sort_by(|a, b| b.started_ms.cmp(&a.started_ms).then(a.port.cmp(&b.port)));
    out
}

/// The session file a selector names (LIVE_API.md 2.6). `None`, `""` or
/// `latest`: `EVANI_LIVE_SESSION` if set, else `session.json` (the most
/// recent). A port (`8098`), a game name (its newest live session) or a
/// session file path (relative to the root) pick one.
pub fn resolve(root: &Path, sel: Option<&str>) -> Result<PathBuf, String> {
    let env = std::env::var("EVANI_LIVE_SESSION").ok();
    let sel = match sel.map(str::trim) {
        Some(s) if !s.is_empty() && s != "latest" => s.to_string(),
        _ => match env.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() && s != "latest" => s.to_string(),
            _ => return Ok(path(root)),
        },
    };
    resolve_in(root, &sel, &list(root))
}

fn resolve_in(root: &Path, sel: &str, all: &[Info]) -> Result<PathBuf, String> {
    let known = || {
        let names: Vec<String> = all
            .iter()
            .filter(|i| i.alive)
            .map(|i| format!("{} ({}, {})", i.port, i.game, i.kind))
            .collect();
        if names.is_empty() { "none are running".to_string() } else { format!("running: {}", names.join(", ")) }
    };
    if let Ok(port) = sel.parse::<u16>() {
        return all
            .iter()
            .find(|i| i.port == port)
            .map(|i| i.file.clone())
            .ok_or_else(|| format!("no live session on port {port}; {}", known()));
    }
    if sel.ends_with(".json") || sel.contains('/') || sel.contains('\\') {
        return Ok(root.join(sel));
    }
    let game = Path::new(sel).file_name().and_then(|n| n.to_str()).unwrap_or(sel);
    all.iter()
        .filter(|i| i.game == game && i.alive)
        .find(|i| i.kind == "live")
        .or_else(|| all.iter().find(|i| i.game == game && i.alive))
        .map(|i| i.file.clone())
        .ok_or_else(|| format!("no live session for game {game}; {}", known()))
}

/// A bridge on the default session file: write its own `sessions/<port>.json`
/// and make `session.json` point at it (it is now the most recent).
pub fn write_default(root: &Path, root_text: &str, game: &str, port: u16, token: &str) -> Result<(), String> {
    write_at(&port_path(root, port), root_text, game, port, token)?;
    write_at(&path(root), root_text, game, port, token)
}

/// Clean exit of a [`write_default`] bridge: remove its `sessions/<port>.json`;
/// if `session.json` is still ours, hand it to the newest session still
/// running (else remove it), so MCP's default keeps working.
pub fn remove_default(root: &Path, port: u16) {
    remove_if_ours_at(&port_path(root, port));
    let main = path(root);
    if !read_at(&main).is_ok_and(|s| s.pid == std::process::id()) {
        return;
    }
    let next = list(root).into_iter().find(|i| i.kind == "live" && i.alive && i.pid != std::process::id());
    match next.and_then(|i| std::fs::read(&i.file).ok()) {
        Some(bytes) => {
            let _ = std::fs::write(&main, bytes);
        }
        None => {
            let _ = std::fs::remove_file(&main);
        }
    }
}

/// Write the session file at `p` (the tab's is [`path`]).
pub fn write_at(p: &Path, root_text: &str, game: &str, port: u16, token: &str) -> Result<(), String> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let doc = json!({
        "protocol": PROTOCOL,
        "pid": std::process::id(),
        "root": root_text,
        "game": game,
        "http": format!("http://127.0.0.1:{port}"),
        "ws": format!("ws://127.0.0.1:{port}"),
        "token": token,
        "started": iso_utc(SystemTime::now()),
        "started_ms": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
    });
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))
}

/// Delete the session file at `p`, but only if it is still ours (another
/// bridge may have started since and rewritten it).
pub fn remove_if_ours_at(p: &Path) {
    if read_at(p).is_ok_and(|s| s.pid == std::process::id()) {
        let _ = std::fs::remove_file(p);
    }
}

/// The tab's session file (the one MCP reads).
#[cfg(test)]
pub fn read(root: &Path) -> Result<Session, String> {
    read_at(&path(root))
}

pub fn read_at(p: &Path) -> Result<Session, String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Session, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("session.json: {e}"))?;
    let s = |k: &str| {
        v[k].as_str()
            .map(String::from)
            .ok_or_else(|| format!("session.json: missing {k}"))
    };
    if v["protocol"].as_str() != Some(PROTOCOL) {
        return Err(format!("session.json: protocol is not {PROTOCOL}"));
    }
    Ok(Session {
        ws: s("ws")?,
        token: s("token")?,
        pid: v["pid"].as_u64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
    })
}

/// `2026-10-13T14:02:11Z` without a date crate (days-from-civil, inverted).
pub fn iso_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(UNIX_EPOCH + Duration::from_secs(1_791_986_531)), "2026-10-14T14:02:11Z");
        assert_eq!(iso_utc(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn write_read_remove() {
        let dir = std::env::temp_dir().join(format!("evani-live-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let token = "ab".repeat(32);
        let p = path(&dir);
        assert!(p.ends_with(".evani/live/session.json") || p.ends_with(".evani\\live\\session.json"));
        write_at(&p, "C:\\x", "sandbox", 8123, &token).unwrap();
        let s = read(&dir).unwrap();
        assert_eq!(s.token, token);
        assert_eq!(s.ws, "ws://127.0.0.1:8123");
        assert_eq!(s.pid, std::process::id());
        remove_if_ours_at(&p);
        assert!(read(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(parse(r#"{"protocol":"evani-live/2","http":"a","ws":"b","token":"c"}"#).is_err());
    }

    /// Two sessions in one folder: each has its own file, `session.json`
    /// follows the newest, and the newest leaving hands it back.
    #[test]
    fn several_sessions_share_a_folder() {
        let dir = std::env::temp_dir().join(format!("evani-live-multi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // An older session of another process, on a port that answers.
        let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let other_port = other.local_addr().unwrap().port();
        let older = format!(
            r#"{{"protocol":"{PROTOCOL}","pid":1,"game":"kart-racer","http":"http://127.0.0.1:{other_port}","ws":"ws://127.0.0.1:{other_port}","token":"{}","started_ms":1}}"#,
            "cd".repeat(32)
        );
        std::fs::create_dir_all(sessions_dir(&dir)).unwrap();
        std::fs::write(port_path(&dir, other_port), &older).unwrap();
        std::fs::write(path(&dir), &older).unwrap();
        // This process starts a second one: its own file, and the latest.
        let mine = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = mine.local_addr().unwrap().port();
        write_default(&dir, "C:\\x", "sandbox", port, &"ab".repeat(32)).unwrap();
        assert_eq!(read(&dir).unwrap().ws, format!("ws://127.0.0.1:{port}"));
        let all = list(&dir);
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].port, all[0].game.as_str(), all[0].alive), (port, "sandbox", true));
        assert_eq!((all[1].port, all[1].game.as_str()), (other_port, "kart-racer"));
        // Selectors: port, game, file, latest.
        assert_eq!(resolve_in(&dir, &other_port.to_string(), &all).unwrap(), port_path(&dir, other_port));
        assert_eq!(resolve_in(&dir, "kart-racer", &all).unwrap(), port_path(&dir, other_port));
        assert_eq!(resolve_in(&dir, "apps/sandbox", &all).unwrap(), dir.join("apps/sandbox"), "a path is a file");
        assert_eq!(resolve_in(&dir, "sandbox", &all).unwrap(), port_path(&dir, port));
        let e = resolve_in(&dir, "match-3", &all).unwrap_err();
        assert!(e.contains("no live session for game match-3") && e.contains("kart-racer"), "{e}");
        assert!(resolve_in(&dir, "1", &all).unwrap_err().contains("port 1"));
        assert_eq!(resolve(&dir, Some("latest")).unwrap(), path(&dir));
        // This one stops: its file goes, session.json is the older one again.
        remove_default(&dir, port);
        assert!(!port_path(&dir, port).exists());
        assert_eq!(std::fs::read_to_string(path(&dir)).unwrap(), older);
        // The last one stops (its port closes): session.json goes too.
        write_default(&dir, "C:\\x", "sandbox", port, &"ab".repeat(32)).unwrap();
        drop(other);
        remove_default(&dir, port);
        assert!(!path(&dir).exists(), "no running session left to point at");
        drop(mine);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bridge_with_its_own_session_file_leaves_the_tabs_alone() {
        let dir = std::env::temp_dir().join(format!("evani-live-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The Evani.dev tab's bridge (another process) on 8096.
        let tab = path(&dir);
        std::fs::create_dir_all(tab.parent().unwrap()).unwrap();
        let tab_text = format!(
            r#"{{"protocol":"{PROTOCOL}","pid":1,"http":"http://127.0.0.1:8096","ws":"ws://127.0.0.1:8096","token":"{}"}}"#,
            "cd".repeat(32)
        );
        std::fs::write(&tab, &tab_text).unwrap();
        // A review bridge on 8180 with its own file: written, read, removed.
        let own = dir.join(".evani").join("live").join("review").join("8180").join("session.json");
        write_at(&own, "C:\\x", "kart-racer", 8180, &"ef".repeat(32)).unwrap();
        assert_eq!(read_at(&own).unwrap().ws, "ws://127.0.0.1:8180");
        remove_if_ours_at(&own);
        assert!(!own.exists());
        assert_eq!(std::fs::read_to_string(&tab).unwrap(), tab_text, "the tab's session file is untouched");
        // Removing at the tab's path is a no-op too: it is not this process's.
        remove_if_ours_at(&tab);
        assert!(tab.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
