//! A look's guard set and its hash (docs/LIVE_API.md 15.4).
//!
//! Everything that decides what "passes" for a look counts as its limits and
//! needs Chris's approval to change: the limit values, the baseline
//! captures, the capture moments and the stat code version. They live in
//! `looks/<look>/guard/`:
//!
//! ```text
//! looks/evani-look-basic/guard/
//!   moments.json          which game, tier, width and moments are captured
//!   limits.json           baseline stats per moment, tolerances, bands
//!   baseline/<moment>.png         the baseline capture
//!   baseline/<moment>.nobloom.png the same moment with bloom off
//! ```
//!
//! The hash recipe (Evani.App reproduces it exactly):
//!
//! 1. Take `moments.json`, `limits.json` and every `baseline/*.png` (other
//!    files are ignored). JSON files are normalised by deleting every CR
//!    byte (0x0D), so a CRLF checkout hashes the same; PNGs are hashed as is.
//! 2. Sort them by relative path (`/`-separated), comparing bytes.
//! 3. Build the manifest, every line ending with one LF:
//!    `evani-look-guard/1`, `look=<look id>`, `stat_version=<n>`,
//!    `capture_version=<n>`, then one `file <relative path> <sha256 hex>`
//!    line per file.
//! 4. The guard hash is the SHA-256 of the manifest's UTF-8 bytes, as 64
//!    lowercase hex chars.

use std::path::{Path, PathBuf};

/// First line of the manifest.
pub const GUARD_FORMAT: &str = "evani-look-guard/1";
/// Version of the stat code (`stats.rs`). Changing a stat's maths bumps it,
/// which changes every look's guard hash, so it needs approval too.
pub const STAT_VERSION: u32 = 1;
/// Version of how captures are taken (the bridge's capture sequence:
/// bloom-on then bloom-off per moment, fixed tier, adaptive quality off).
pub const CAPTURE_VERSION: u32 = 1;

/// The folder of a look's guard set.
pub fn guard_dir(root: &Path, look: &str) -> PathBuf {
    root.join("looks").join(look).join("guard")
}

/// `evani-look-<name>`, lowercase letters, digits and `-` only.
pub fn is_look_id(s: &str) -> bool {
    s.strip_prefix("evani-look-").is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    })
}

/// A moment name usable as a file name: `[a-z0-9_-]`, 1-64 chars.
pub fn is_moment_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

/// The guard hash and the manifest it was computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardHash {
    pub hash: String,
    pub manifest: String,
}

/// Is this relative path part of the guard set?
pub fn counts(rel: &str) -> bool {
    if rel == "moments.json" || rel == "limits.json" {
        return true;
    }
    rel.strip_prefix("baseline/")
        .is_some_and(|f| !f.contains('/') && f.ends_with(".png") && f.len() > 4)
}

/// The guard hash of a set of files (relative path, contents). Files that
/// don't count are ignored. Pure: used for the files on disk and for a
/// proposal's staged files alike.
pub fn hash_files(look: &str, files: &[(String, Vec<u8>)]) -> Result<GuardHash, String> {
    let mut entries: Vec<(&str, String)> = Vec::new();
    for (rel, bytes) in files {
        if !counts(rel) {
            continue;
        }
        let digest = if rel.ends_with(".json") {
            let clean: Vec<u8> = bytes.iter().copied().filter(|b| *b != b'\r').collect();
            crate::sha256_hex(&clean)
        } else {
            crate::sha256_hex(bytes)
        };
        entries.push((rel.as_str(), digest));
    }
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for need in ["moments.json", "limits.json"] {
        if !entries.iter().any(|(r, _)| *r == need) {
            return Err(format!("the guard set of {look} has no {need}"));
        }
    }
    if !entries.iter().any(|(r, _)| r.starts_with("baseline/")) {
        return Err(format!("the guard set of {look} has no baseline captures"));
    }
    let mut manifest = format!(
        "{GUARD_FORMAT}\nlook={look}\nstat_version={STAT_VERSION}\ncapture_version={CAPTURE_VERSION}\n"
    );
    for (rel, digest) in &entries {
        manifest.push_str(&format!("file {rel} {digest}\n"));
    }
    Ok(GuardHash {
        hash: crate::sha256_hex(manifest.as_bytes()),
        manifest,
    })
}

/// Read the guard set's files from `dir` (`moments.json`, `limits.json`,
/// `baseline/*.png`).
pub fn read_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    for name in ["moments.json", "limits.json"] {
        let p = dir.join(name);
        if p.is_file() {
            out.push((name.to_string(), std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?));
        }
    }
    let base = dir.join("baseline");
    if base.is_dir() {
        let rd = std::fs::read_dir(&base).map_err(|e| format!("{}: {e}", base.display()))?;
        for entry in rd {
            let p = entry.map_err(|e| e.to_string())?.path();
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let rel = format!("baseline/{name}");
            if p.is_file() && counts(&rel) {
                out.push((rel, std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?));
            }
        }
    }
    Ok(out)
}

/// The guard hash of a look on disk; `Ok(None)` if it has no guard folder.
pub fn guard_hash(root: &Path, look: &str) -> Result<Option<GuardHash>, String> {
    let dir = guard_dir(root, look);
    if !dir.exists() {
        return Ok(None);
    }
    let files = read_files(&dir)?;
    hash_files(look, &files).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> Vec<(String, Vec<u8>)> {
        vec![
            ("moments.json".into(), b"{\"a\":1}\n".to_vec()),
            ("limits.json".into(), b"{\"b\":2}\n".to_vec()),
            ("baseline/m1.png".into(), vec![1, 2, 3]),
            ("baseline/m1.nobloom.png".into(), vec![4, 5]),
            ("notes.txt".into(), b"ignored".to_vec()),
        ]
    }

    #[test]
    fn manifest_shape_and_order() {
        let h = hash_files("evani-look-basic", &set()).unwrap();
        let lines: Vec<&str> = h.manifest.lines().collect();
        assert_eq!(lines[0], "evani-look-guard/1");
        assert_eq!(lines[1], "look=evani-look-basic");
        assert_eq!(lines[2], "stat_version=1");
        assert_eq!(lines[3], "capture_version=1");
        assert!(lines[4].starts_with("file baseline/m1.nobloom.png "));
        assert!(lines[5].starts_with("file baseline/m1.png "));
        assert!(lines[6].starts_with("file limits.json "));
        assert!(lines[7].starts_with("file moments.json "));
        assert_eq!(lines.len(), 8);
        assert!(h.manifest.ends_with('\n'));
        assert!(crate::hex::is_sha256(&h.hash));
        assert_eq!(h.hash, crate::sha256_hex(h.manifest.as_bytes()));
    }

    #[test]
    fn crlf_and_order_do_not_matter_but_content_does() {
        let a = hash_files("evani-look-basic", &set()).unwrap();
        let mut s = set();
        s.reverse();
        let li = s.iter().position(|(r, _)| r == "limits.json").unwrap();
        s[li].1 = b"{\"b\":2}\r\n".to_vec(); // limits.json with CRLF
        let b = hash_files("evani-look-basic", &s).unwrap();
        assert_eq!(a.hash, b.hash);
        let mut c = set();
        c[2].1 = vec![1, 2, 4];
        assert_ne!(hash_files("evani-look-basic", &c).unwrap().hash, a.hash);
        assert_ne!(hash_files("evani-look-toon", &set()).unwrap().hash, a.hash);
    }

    #[test]
    fn incomplete_sets_fail() {
        let mut s = set();
        s.retain(|(r, _)| r != "limits.json");
        assert!(hash_files("evani-look-basic", &s).is_err());
        let mut s = set();
        s.retain(|(r, _)| !r.starts_with("baseline/"));
        assert!(hash_files("evani-look-basic", &s).is_err());
    }

    #[test]
    fn names() {
        assert!(is_look_id("evani-look-basic"));
        assert!(!is_look_id("evani-look-"));
        assert!(!is_look_id("evani-kit-example"));
        assert!(!is_look_id("evani-look-../x"));
        assert!(is_moment_name("orbit-20"));
        assert!(!is_moment_name("Orbit"));
        assert!(!is_moment_name("../x"));
        assert!(counts("baseline/a.png"));
        assert!(!counts("baseline/x/a.png"));
        assert!(!counts("baseline/.png"));
    }
}
