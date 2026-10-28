//! Evani.App's look-approvals folder, read-only from the engine's side
//! (docs/LIVE_API.md 15.5, 15.6).
//!
//! ```text
//! %APPDATA%\Evani.App\look-approvals\
//!   pubkey                         64 hex chars: the pinned Ed25519 public key
//!   endpoint.json                  {"port": N} while Evani.App runs (seq endpoint)
//!   <project id>\<look id>.json    the signed record of each look
//! ```
//!
//! (Elsewhere than Windows: `$XDG_CONFIG_HOME/Evani.App/look-approvals`, or
//! `~/.config/Evani.App/look-approvals`.) The engine never writes here.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::record::{Record, norm_path};

/// The look-approvals folder, if the environment names a home for it.
pub fn data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("Evani.App").join("look-approvals"))
}

/// The pinned public key (`pubkey`: 64 hex chars, surrounding whitespace
/// allowed). `Ok(None)` if the file doesn't exist.
pub fn read_pubkey(dir: &Path) -> Result<Option<[u8; 32]>, String> {
    let p = dir.join("pubkey");
    match std::fs::read_to_string(&p) {
        Ok(t) => crate::hex::decode_n::<32>(t.trim())
            .map(Some)
            .ok_or_else(|| format!("{} is not 64 hex chars", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// The port of Evani.App's seq endpoint, from `endpoint.json`.
pub fn endpoint_port(dir: &Path) -> Option<u16> {
    let t = std::fs::read_to_string(dir.join("endpoint.json")).ok()?;
    let v: Value = serde_json::from_str(&t).ok()?;
    v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p != 0)
}

/// A record found for this project and look, with where it came from.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub record: Record,
}

/// Find the look's record. With a project id: `<dir>/<id>/<look>.json`.
/// Without: every `<dir>/*/<look>.json` whose `projectPath` is this
/// project's root; the highest `seq` wins. A file that can't be parsed is
/// an error only when it is the one asked for by id.
pub fn find_record(dir: &Path, look: &str, root: &Path, project_id: Option<&str>) -> Result<Option<Found>, String> {
    if let Some(id) = project_id {
        if id.is_empty() || id.contains(['/', '\\', ':']) || id.contains("..") {
            return Err(format!("bad project id {id:?}"));
        }
        let p = dir.join(id).join(format!("{look}.json"));
        return match std::fs::read_to_string(&p) {
            Ok(t) => Record::parse(&t)
                .map(|record| Some(Found { path: p.clone(), record }))
                .map_err(|e| format!("{}: {e}", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", p.display())),
        };
    }
    let want = norm_path(&root.display().to_string());
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut best: Option<Found> = None;
    for entry in rd.flatten() {
        let p = entry.path().join(format!("{look}.json"));
        let Ok(t) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(record) = Record::parse(&t) else {
            continue;
        };
        if norm_path(&record.project_path) != want {
            continue;
        }
        if best.as_ref().is_none_or(|b| record.seq > b.record.seq) {
            best = Some(Found { path: p, record });
        }
    }
    Ok(best)
}

/// Is there any record for this look and project (used to notice a guard
/// folder that was deleted)?
pub fn has_record(dir: &Path, look: &str, root: &Path, project_id: Option<&str>) -> bool {
    find_record(dir, look, root, project_id).is_ok_and(|f| f.is_some())
        || project_id.is_some_and(|id| dir.join(id).join(format!("{look}.json")).exists())
}

/// What Evani.App's seq endpoint said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeqAnswer {
    /// The current approved seq (`None`: Evani.App has no record for it).
    Seq(Option<u64>),
    /// Evani.App isn't reachable; the check fell back to the file alone.
    Offline(String),
}

/// Percent-encode a query value (unreserved chars kept).
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The request line path of the seq query.
pub fn seq_path(project_id: &str, look: &str) -> String {
    format!("/look-approvals/v1/seq?project={}&look={}", pct(project_id), pct(look))
}

/// Parse the seq endpoint's raw HTTP response.
pub fn parse_seq_response(raw: &str, look: &str) -> SeqAnswer {
    let Some((head, body)) = raw.split_once("\r\n\r\n") else {
        return SeqAnswer::Offline("no HTTP response".into());
    };
    let status = head.split_whitespace().nth(1).unwrap_or("");
    match status {
        "200" => {
            // Node's http server (Evani.App) sends chunked bodies unless it sets Content-Length.
            let chunked = head.lines().any(|l| {
                let l = l.to_ascii_lowercase();
                l.starts_with("transfer-encoding:") && l.contains("chunked")
            });
            let body = if chunked { dechunk(body) } else { Some(body.to_string()) };
            // A body we can't read is "offline", never "no approval".
            let Some(v) = body.and_then(|b| serde_json::from_str::<Value>(b.trim()).ok()) else {
                return SeqAnswer::Offline("the seq endpoint's answer isn't JSON".into());
            };
            if v["look"].as_str().is_some_and(|l| l != look) {
                return SeqAnswer::Offline("the seq endpoint answered for another look".into());
            }
            match &v["seq"] {
                Value::Null => SeqAnswer::Seq(None),
                s => s
                    .as_u64()
                    .map_or_else(|| SeqAnswer::Offline("the seq endpoint sent a bad seq".into()), |n| SeqAnswer::Seq(Some(n))),
            }
        }
        "404" => SeqAnswer::Seq(None),
        other => SeqAnswer::Offline(format!("the seq endpoint answered HTTP {other}")),
    }
}

/// A chunked HTTP body put back together; `None` when it is cut short or malformed.
fn dechunk(mut rest: &str) -> Option<String> {
    let mut out = String::new();
    loop {
        let (line, after) = rest.split_once("\r\n")?;
        let size = usize::from_str_radix(line.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        out.push_str(after.get(..size)?);
        rest = after.get(size..)?.strip_prefix("\r\n")?;
    }
}

/// Ask Evani.App (127.0.0.1:`port`) for the current seq of a look. One short
/// blocking request; never waits more than `timeout` per step.
pub fn query_seq(port: u16, project_id: &str, look: &str, timeout: Duration) -> SeqAnswer {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(s) => s,
        Err(e) => return SeqAnswer::Offline(format!("Evani.App is not answering on port {port} ({e})")),
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        seq_path(project_id, look)
    );
    if let Err(e) = s.write_all(req.as_bytes()) {
        return SeqAnswer::Offline(format!("seq request failed: {e}"));
    }
    let mut buf = Vec::new();
    let _ = s.take(64 * 1024).read_to_end(&mut buf);
    parse_seq_response(&String::from_utf8_lossy(&buf), look)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkey;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-lg-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_record(dir: &Path, id: &str, path: &str, seq: u64) {
        let mut r = Record {
            project_id: id.into(),
            project_path: path.into(),
            look: "evani-look-basic".into(),
            seq,
            approved_at: "2026-10-14T15:00:00Z".into(),
            hash: "ab".repeat(32),
            sig: [0; 64],
        };
        r.sig = testkey::sign(r.canonical().as_bytes());
        std::fs::create_dir_all(dir.join(id)).unwrap();
        std::fs::write(dir.join(id).join("evani-look-basic.json"), r.to_json().to_string()).unwrap();
    }

    #[test]
    fn finds_by_id_or_by_project_path() {
        let d = temp("find");
        write_record(&d, "p1", "C:\\Proj\\A", 2);
        write_record(&d, "p2", "C:\\Proj\\B", 9);
        let root = Path::new("c:/proj/a/");
        let f = find_record(&d, "evani-look-basic", root, None).unwrap().unwrap();
        assert_eq!(f.record.seq, 2);
        let f = find_record(&d, "evani-look-basic", root, Some("p2")).unwrap().unwrap();
        assert_eq!(f.record.seq, 9);
        assert!(find_record(&d, "evani-look-toon", root, None).unwrap().is_none());
        assert!(find_record(&d, "evani-look-basic", Path::new("C:/other"), None).unwrap().is_none());
        assert!(find_record(&d, "evani-look-basic", root, Some("../x")).is_err());
        assert!(has_record(&d, "evani-look-basic", root, None));
        std::fs::write(d.join("pubkey"), format!("{}\n", crate::hex::encode(&testkey::public()))).unwrap();
        assert_eq!(read_pubkey(&d).unwrap(), Some(testkey::public()));
        std::fs::write(d.join("endpoint.json"), r#"{"port":8123}"#).unwrap();
        assert_eq!(endpoint_port(&d), Some(8123));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(read_pubkey(&d).unwrap(), None);
    }

    #[test]
    fn seq_responses() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"project\":\"p\",\"look\":\"evani-look-basic\",\"seq\":4}";
        assert_eq!(parse_seq_response(ok, "evani-look-basic"), SeqAnswer::Seq(Some(4)));
        let none = "HTTP/1.1 200 OK\r\n\r\n{\"seq\":null}";
        assert_eq!(parse_seq_response(none, "evani-look-basic"), SeqAnswer::Seq(None));
        assert_eq!(parse_seq_response("HTTP/1.1 404 Not Found\r\n\r\n", "x"), SeqAnswer::Seq(None));
        assert!(matches!(parse_seq_response("HTTP/1.1 500 X\r\n\r\n", "x"), SeqAnswer::Offline(_)));
        assert!(matches!(parse_seq_response(ok, "evani-look-toon"), SeqAnswer::Offline(_)));
        // Node's default: a chunked body (no Content-Length).
        let chunked = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n\
                       1b\r\n{\"look\":\"evani-look-basic\",\r\n8\r\n\"seq\":3}\r\n0\r\n\r\n";
        assert_eq!(parse_seq_response(chunked, "evani-look-basic"), SeqAnswer::Seq(Some(3)));
        let cut = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1a\r\n{\"look\":";
        assert!(matches!(parse_seq_response(cut, "evani-look-basic"), SeqAnswer::Offline(_)));
        // A 200 we can't read is offline, never "no approval" (which would fail as a rollback).
        assert!(matches!(parse_seq_response("HTTP/1.1 200 OK\r\n\r\n<html>", "evani-look-basic"), SeqAnswer::Offline(_)));
        assert_eq!(seq_path("my proj", "evani-look-basic"), "/look-approvals/v1/seq?project=my%20proj&look=evani-look-basic");
        // Nobody listening: offline, quickly.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(matches!(query_seq(port, "p", "evani-look-basic", Duration::from_millis(300)), SeqAnswer::Offline(_)));
    }

    #[test]
    fn seq_query_over_a_socket() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"project\":\"p\",\"look\":\"evani-look-basic\",\"seq\":7}").unwrap();
            req
        });
        let a = query_seq(port, "p", "evani-look-basic", Duration::from_secs(2));
        assert_eq!(a, SeqAnswer::Seq(Some(7)));
        let req = t.join().unwrap();
        assert!(req.starts_with("GET /look-approvals/v1/seq?project=p&look=evani-look-basic HTTP/1.1\r\n"), "{req}");
    }
}
