//! The dev token and the Host / Origin rules (docs/LIVE_API.md 3, 4).

/// The WebSocket and HTTP doors that have their own Origin rule (4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Door {
    /// `/engine`: the page. Origin must be exactly this bridge's origin.
    Engine,
    /// `/ctl`: native controllers (MCP, scripts). No Origin, or a localhost one.
    Ctl,
    /// `/evani-dev`: the Evani.dev tab's news channel. Same rule as `/ctl`.
    EvaniDev,
    /// `POST /live/v1/cmd`: curl-style clients. No Origin at all.
    HttpCmd,
    /// `/live/v1/convert` (16): the page (its own origin) or a native
    /// client such as the Blender add-on (no Origin). Token always needed.
    Convert,
}

/// 32 random bytes from the OS RNG, as 64 lowercase hex chars.
pub fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("OS random generator failed: {e}"))?;
    Ok(hex(&bytes))
}

/// A short random id (for the session id in `hello`).
pub fn short_id() -> String {
    let mut bytes = [0u8; 2];
    if getrandom::fill(&mut bytes).is_err() {
        bytes = [0x7f, 0x2c];
    }
    hex(&bytes)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}

/// Constant-time comparison: the time taken doesn't depend on where the
/// strings differ (only on their lengths, which aren't secret).
pub fn token_eq(given: &str, expected: &str) -> bool {
    let (a, b) = (given.as_bytes(), expected.as_bytes());
    if a.len() != b.len() || b.is_empty() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The token from an `Authorization: Bearer <token>` header value.
pub fn bearer(header: Option<&str>) -> Option<&str> {
    let h = header?.trim();
    let (scheme, rest) = h.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let t = rest.trim();
    (!t.is_empty()).then_some(t)
}

/// Does an `Authorization` header carry the right token?
pub fn authorized(header: Option<&str>, token: &str) -> bool {
    bearer(header).is_some_and(|t| token_eq(t, token))
}

/// One cookie's value from a `Cookie` header (`a=1; b=2`).
pub fn cookie<'a>(header: Option<&'a str>, name: &str) -> Option<&'a str> {
    header?.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name && !v.is_empty()).then_some(v)
    })
}

/// `Host` must be `localhost:P` or `127.0.0.1:P` (stops DNS rebinding).
pub fn host_ok(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else { return false };
    let Some((name, p)) = host.rsplit_once(':') else {
        return false;
    };
    is_loopback_name(name) && p.parse::<u16>().ok() == Some(port)
}

fn is_loopback_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1"
}

/// The Origin rule for one door (4.4). `origin` is the header value, if any.
pub fn origin_ok(door: Door, origin: Option<&str>, port: u16) -> bool {
    match door {
        Door::HttpCmd => origin.is_none(),
        Door::Engine => origin.is_some_and(|o| {
            o == format!("http://localhost:{port}") || o == format!("http://127.0.0.1:{port}")
        }),
        Door::Ctl | Door::EvaniDev => origin.is_none_or(local_origin),
        Door::Convert => origin.is_none_or(|o| o == format!("http://localhost:{port}") || o == format!("http://127.0.0.1:{port}")),
    }
}

/// `http(s)://localhost` or `http(s)://127.0.0.1`, any port, nothing after.
fn local_origin(o: &str) -> bool {
    let Some(rest) = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")) else {
        return false;
    };
    let (name, port) = match rest.rsplit_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (rest, None),
    };
    is_loopback_name(name)
        && port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_64_hex_and_differ() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
        assert_eq!(short_id().len(), 4);
    }

    #[test]
    fn token_comparison() {
        let t = "ab".repeat(32);
        assert!(token_eq(&t, &t));
        assert!(!token_eq(&"ab".repeat(31), &t));
        assert!(!token_eq(&format!("{}ac", "ab".repeat(31)), &t));
        assert!(!token_eq("", ""), "an empty expected token never matches");
        assert!(!token_eq("", &t));
    }

    #[test]
    fn bearer_header() {
        let t = "f".repeat(64);
        assert_eq!(bearer(Some(&format!("Bearer {t}"))), Some(t.as_str()));
        assert_eq!(bearer(Some(&format!("bearer   {t} "))), Some(t.as_str()));
        assert_eq!(bearer(Some(&format!("Basic {t}"))), None);
        assert_eq!(bearer(Some("Bearer")), None);
        assert_eq!(bearer(Some("Bearer  ")), None);
        assert_eq!(bearer(None), None);
        assert!(authorized(Some(&format!("Bearer {t}")), &t));
        assert!(!authorized(Some("Bearer nope"), &t));
        assert!(!authorized(None, &t));
    }

    #[test]
    fn host_rule() {
        assert!(host_ok(Some("localhost:8096"), 8096));
        assert!(host_ok(Some("LOCALHOST:8096"), 8096));
        assert!(host_ok(Some("127.0.0.1:8096"), 8096));
        assert!(!host_ok(Some("localhost:8095"), 8096));
        assert!(!host_ok(Some("localhost"), 8096));
        assert!(!host_ok(Some("evil.example:8096"), 8096));
        assert!(!host_ok(Some("localhost.evil.example:8096"), 8096));
        assert!(!host_ok(Some("0.0.0.0:8096"), 8096));
        assert!(!host_ok(Some("[::1]:8096"), 8096));
        assert!(!host_ok(None, 8096));
    }

    #[test]
    fn origin_rules() {
        use Door::*;
        let p = 8096;
        // /engine: exactly our own origin.
        assert!(origin_ok(Engine, Some("http://localhost:8096"), p));
        assert!(origin_ok(Engine, Some("http://127.0.0.1:8096"), p));
        assert!(!origin_ok(Engine, None, p));
        assert!(!origin_ok(Engine, Some("http://localhost:5173"), p));
        assert!(!origin_ok(Engine, Some("https://localhost:8096"), p));
        assert!(!origin_ok(Engine, Some("http://evil.example"), p));
        // /ctl and /evani-dev: none, or localhost with any port.
        for d in [Ctl, EvaniDev] {
            assert!(origin_ok(d, None, p));
            assert!(origin_ok(d, Some("http://localhost:5173"), p));
            assert!(origin_ok(d, Some("https://127.0.0.1"), p));
            assert!(origin_ok(d, Some("http://localhost"), p));
            assert!(!origin_ok(d, Some("http://evil.example"), p));
            assert!(!origin_ok(d, Some("http://localhost.evil.example"), p));
            assert!(!origin_ok(d, Some("http://localhost:80/x"), p));
            assert!(!origin_ok(d, Some("file://"), p));
            assert!(!origin_ok(d, Some("null"), p));
        }
        // POST /live/v1/cmd: no Origin at all.
        assert!(origin_ok(HttpCmd, None, p));
        assert!(!origin_ok(HttpCmd, Some("http://localhost:8096"), p));
        // /live/v1/convert: none, or exactly our own origin.
        assert!(origin_ok(Convert, None, p));
        assert!(origin_ok(Convert, Some("http://localhost:8096"), p));
        assert!(origin_ok(Convert, Some("http://127.0.0.1:8096"), p));
        assert!(!origin_ok(Convert, Some("http://localhost:5173"), p));
        assert!(!origin_ok(Convert, Some("http://evil.example"), p));
        assert!(!origin_ok(Convert, Some("null"), p));
    }

    #[test]
    fn cookie_token() {
        let t = "f".repeat(64);
        let jar = format!("a=1; evani_live_8096={t}; b=2");
        assert_eq!(cookie(Some(&jar), "evani_live_8096"), Some(t.as_str()));
        assert_eq!(cookie(Some(&jar), "evani_live_8097"), None);
        assert_eq!(cookie(Some("xevani_live_8096=1"), "evani_live_8096"), None);
        assert_eq!(cookie(None, "evani_live_8096"), None);
    }
}
