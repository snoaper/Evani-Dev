//! The one check every game build and every bake runs (docs/LIVE_API.md
//! 15.6): are this look's limits, baselines, capture moments and stat
//! version exactly what Chris last approved?
//!
//! [`check_record`] is the self-contained core Evani.App mirrors in its own
//! copy (test vectors in docs/LIVE_API.md 15.10); [`verify_look`] gathers
//! its inputs from disk and Evani.App's store.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::guardset;
use crate::record::{Record, key_id, norm_path};
use crate::store::{self, SeqAnswer};

/// Where trust comes from: Evani.App's data folder (never the repo).
#[derive(Debug, Clone, Default)]
pub struct Trust {
    pub data_dir: Option<PathBuf>,
    /// The pinned approvals public key.
    pub pubkey: Option<[u8; 32]>,
    /// Why the key couldn't be read, if it couldn't.
    pub pubkey_error: Option<String>,
    /// Evani.App's project id, when known (the bridge gets it on stdin).
    pub project_id: Option<String>,
    /// Evani.App's seq endpoint port, when it runs.
    pub seq_port: Option<u16>,
}

impl Trust {
    /// Read the pinned key and endpoint from Evani.App's data folder.
    pub fn from_store() -> Self {
        let mut t = Trust {
            data_dir: store::data_dir(),
            ..Trust::default()
        };
        if let Some(dir) = &t.data_dir {
            match store::read_pubkey(dir) {
                Ok(k) => t.pubkey = k,
                Err(e) => t.pubkey_error = Some(e),
            }
            t.seq_port = store::endpoint_port(dir);
        }
        t
    }
}

/// How the rollback guard went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rollback {
    /// Evani.App answered; its current seq.
    Checked { current: u64 },
    /// Evani.App wasn't reachable: checked against the file alone.
    Offline(String),
}

/// A look whose guard set matches its signed record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub look: String,
    pub hash: String,
    pub seq: u64,
    pub key_id: String,
    /// Signed with the public TEST key: worthless outside tests.
    pub test_key: bool,
    pub record_path: PathBuf,
    pub rollback: Rollback,
}

impl Verified {
    /// One plain line for build output.
    pub fn line(&self) -> String {
        let mut s = format!(
            "look guard: {} matches Chris's approval (seq {}, limits {}…, key {})",
            self.look,
            self.seq,
            &self.hash[..12],
            self.key_id
        );
        if let Rollback::Offline(why) = &self.rollback {
            s.push_str(&format!(
                "; Evani.App not reachable ({why}), so this offline check used the record file alone and could not rule out an older approval being put back"
            ));
        }
        if self.test_key {
            s.push_str("; WARNING: signed with the public TEST key, so it proves nothing");
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No guard folder and no record: this look isn't under the guard yet.
    Unguarded,
    Verified(Verified),
}

/// Why a look failed the check: a code and one plain sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}

fn fail(code: &'static str, message: String) -> Failure {
    Failure { code, message }
}

/// Is `pubkey` the public TEST key (testkey.rs)?
pub fn is_test_key(pubkey: &[u8; 32]) -> bool {
    let test = ed25519_dalek::SigningKey::from_bytes(b"EVANI-LOOK-GUARD-TEST-KEY-NEVER!").verifying_key().to_bytes();
    *pubkey == test
}

/// The core check. In order: the record is for this look and this project,
/// its signature is Evani.App's, its hash is the guard hash now on disk,
/// and (when Evani.App answered) it is not older than Evani.App's latest.
pub fn check_record(
    record: &Record,
    pubkey: &[u8; 32],
    look: &str,
    guard_hash: &str,
    root: &Path,
    current_seq: Option<Option<u64>>,
) -> Result<(), Failure> {
    if record.look != look {
        return Err(fail("record_invalid", format!("The approval record names look {}, not {look}.", record.look)));
    }
    if norm_path(&record.project_path) != norm_path(&root.display().to_string()) {
        return Err(fail(
            "record_invalid",
            format!("The approval record of {look} is for project {}, not {}.", record.project_path, root.display()),
        ));
    }
    if record.verify(pubkey).is_err() {
        return Err(fail("bad_signature", format!("The approval record of {look} isn't signed by Evani.App's key.")));
    }
    if record.hash != guard_hash {
        return Err(fail(
            "hash_mismatch",
            format!(
                "The limits, baseline captures or capture moments of {look} changed since Chris last approved them (approved {}…, now {}…).",
                &record.hash[..12],
                &guard_hash[..12.min(guard_hash.len())]
            ),
        ));
    }
    match current_seq {
        Some(None) => Err(fail(
            "rollback",
            format!("Evani.App has no approval for {look}, but an approval record file is there."),
        )),
        Some(Some(cur)) if record.seq < cur => Err(fail(
            "rollback",
            format!(
                "The approval record of {look} is seq {}, but Evani.App's latest is seq {cur}: an older approval was put back.",
                record.seq
            ),
        )),
        _ => Ok(()),
    }
}

/// Check one look of the project at `root` (every build and every bake).
pub fn verify_look(root: &Path, look: &str, trust: &Trust) -> Result<Verdict, Failure> {
    let guard = guardset::guard_hash(root, look).map_err(|e| fail("guard_incomplete", format!("{e}.")))?;
    let found = match &trust.data_dir {
        Some(dir) => store::find_record(dir, look, root, trust.project_id.as_deref())
            .map_err(|e| fail("record_invalid", format!("The approval record of {look} can't be read: {e}.")))?,
        None => None,
    };
    let (guard, found) = match (guard, found) {
        (None, None) => return Ok(Verdict::Unguarded),
        (None, Some(_)) => {
            return Err(fail(
                "guard_missing",
                format!("{look} has an approval record in Evani.App, but its guard folder (looks/{look}/guard) is gone."),
            ));
        }
        (Some(_), None) => {
            return Err(fail(
                "record_missing",
                format!("{look} has limits but no approval record from Evani.App, so they are unapproved. Approve them in Evani.App."),
            ));
        }
        (Some(g), Some(f)) => (g, f),
    };
    let Some(pubkey) = trust.pubkey else {
        let why = trust.pubkey_error.clone().unwrap_or_else(|| "no pubkey file".into());
        return Err(fail(
            "no_pubkey",
            format!("Evani.App's approvals key isn't pinned in its data folder ({why}), so {look}'s approval can't be checked."),
        ));
    };
    let project_id = trust.project_id.clone().unwrap_or_else(|| found.record.project_id.clone());
    let answer = match trust.seq_port {
        Some(port) => store::query_seq(port, &project_id, look, Duration::from_millis(800)),
        None => SeqAnswer::Offline("Evani.App's seq endpoint is not running".into()),
    };
    let (current, rollback) = match answer {
        SeqAnswer::Seq(s) => (Some(s), Rollback::Checked { current: s.unwrap_or(0) }),
        SeqAnswer::Offline(why) => (None, Rollback::Offline(why)),
    };
    check_record(&found.record, &pubkey, look, &guard.hash, root, current)?;
    Ok(Verdict::Verified(Verified {
        look: look.to_string(),
        hash: guard.hash,
        seq: found.record.seq,
        key_id: key_id(&pubkey),
        test_key: is_test_key(&pubkey),
        record_path: found.path,
        rollback,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkey;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-lg-verify-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn guard(root: &Path) -> String {
        let g = guardset::guard_dir(root, "evani-look-basic");
        std::fs::create_dir_all(g.join("baseline")).unwrap();
        std::fs::write(g.join("moments.json"), "{\"m\":1}\n").unwrap();
        std::fs::write(g.join("limits.json"), "{\"l\":1}\n").unwrap();
        std::fs::write(g.join("baseline/a.png"), [1, 2, 3]).unwrap();
        guardset::guard_hash(root, "evani-look-basic").unwrap().unwrap().hash
    }

    fn signed(root: &Path, hash: &str, seq: u64) -> Record {
        let mut r = Record {
            project_id: "proj".into(),
            project_path: root.display().to_string(),
            look: "evani-look-basic".into(),
            seq,
            approved_at: "2026-10-14T15:00:00Z".into(),
            hash: hash.into(),
            sig: [0; 64],
        };
        r.sig = testkey::sign(r.canonical().as_bytes());
        r
    }

    fn store_with(dir: &Path, r: &Record) {
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        std::fs::write(dir.join("proj/evani-look-basic.json"), r.to_json().to_string()).unwrap();
    }

    #[test]
    fn the_whole_check() {
        let root = temp("root");
        let store = temp("store");
        let trust = Trust {
            data_dir: Some(store.clone()),
            pubkey: Some(testkey::public()),
            ..Trust::default()
        };
        // Nothing at all: unguarded.
        assert_eq!(verify_look(&root, "evani-look-basic", &trust).unwrap(), Verdict::Unguarded);
        // Guard folder without a record: fails.
        let hash = guard(&root);
        assert_eq!(verify_look(&root, "evani-look-basic", &trust).unwrap_err().code, "record_missing");
        // Signed record of the right hash: passes (offline).
        store_with(&store, &signed(&root, &hash, 1));
        let Verdict::Verified(v) = verify_look(&root, "evani-look-basic", &trust).unwrap() else {
            panic!("expected verified")
        };
        assert_eq!(v.seq, 1);
        assert!(v.test_key);
        assert!(matches!(v.rollback, Rollback::Offline(_)));
        assert!(v.line().contains("offline check") && v.line().contains("TEST key"));
        // Limits changed on disk: fails.
        let g = guardset::guard_dir(&root, "evani-look-basic");
        std::fs::write(g.join("limits.json"), "{\"l\":2}\n").unwrap();
        assert_eq!(verify_look(&root, "evani-look-basic", &trust).unwrap_err().code, "hash_mismatch");
        std::fs::write(g.join("limits.json"), "{\"l\":1}\n").unwrap();
        // A re-baseline (new png) fails too.
        std::fs::write(g.join("baseline/b.png"), [9]).unwrap();
        assert_eq!(verify_look(&root, "evani-look-basic", &trust).unwrap_err().code, "hash_mismatch");
        std::fs::remove_file(g.join("baseline/b.png")).unwrap();
        // Forged record (other key): fails.
        let mut forged = signed(&root, &hash, 2);
        forged.sig = ed25519_dalek::Signer::sign(&ed25519_dalek::SigningKey::from_bytes(&[3; 32]), forged.canonical().as_bytes()).to_bytes();
        store_with(&store, &forged);
        assert_eq!(verify_look(&root, "evani-look-basic", &trust).unwrap_err().code, "bad_signature");
        store_with(&store, &signed(&root, &hash, 1));
        // No pinned key: fails closed.
        let no_key = Trust { pubkey: None, ..trust.clone() };
        assert_eq!(verify_look(&root, "evani-look-basic", &no_key).unwrap_err().code, "no_pubkey");
        // Guard folder deleted while a record exists: fails.
        std::fs::remove_dir_all(&g).unwrap();
        assert_eq!(verify_look(&root, "evani-look-basic", &trust).unwrap_err().code, "guard_missing");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn rollback_guard() {
        let root = Path::new("C:/p");
        let r = signed(root, &"ab".repeat(32), 2);
        let pk = testkey::public();
        let h = "ab".repeat(32);
        assert!(check_record(&r, &pk, "evani-look-basic", &h, root, None).is_ok());
        assert!(check_record(&r, &pk, "evani-look-basic", &h, root, Some(Some(2))).is_ok());
        assert_eq!(check_record(&r, &pk, "evani-look-basic", &h, root, Some(Some(3))).unwrap_err().code, "rollback");
        assert_eq!(check_record(&r, &pk, "evani-look-basic", &h, root, Some(None)).unwrap_err().code, "rollback");
        assert_eq!(check_record(&r, &pk, "evani-look-toon", &h, root, None).unwrap_err().code, "record_invalid");
        assert_eq!(check_record(&r, &pk, "evani-look-basic", &h, Path::new("C:/q"), None).unwrap_err().code, "record_invalid");
    }

    /// The test vector of docs/LIVE_API.md 15.10: Evani.App's copy of the
    /// check must give the same results.
    #[test]
    fn documented_test_vector() {
        let pk = testkey::public();
        assert!(is_test_key(&pk));
        assert_eq!(crate::hex::encode(&pk), TEST_PUBKEY_HEX);
        let r = Record {
            project_id: "evani-dev".into(),
            project_path: "C:\\dev\\evani-dev".into(),
            look: "evani-look-basic".into(),
            seq: 1,
            approved_at: "2026-10-14T15:00:00Z".into(),
            hash: VECTOR_HASH.into(),
            sig: crate::hex::decode_n::<64>(VECTOR_SIG).unwrap(),
        };
        // The signature is deterministic (Ed25519), so the vector is fixed.
        assert_eq!(crate::hex::encode(&testkey::sign(r.canonical().as_bytes())), VECTOR_SIG);
        let root = Path::new("C:\\dev\\evani-dev");
        assert!(check_record(&r, &pk, "evani-look-basic", VECTOR_HASH, root, None).is_ok());
        let mut bad = r.clone();
        bad.seq = 2;
        assert_eq!(check_record(&bad, &pk, "evani-look-basic", VECTOR_HASH, root, None).unwrap_err().code, "bad_signature");
    }

    pub const TEST_PUBKEY_HEX: &str = "5f5dc4101d95c54733f938ebc02f8e0cdfac2510075d72d33ca98a8861d00c69";
    pub const VECTOR_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    pub const VECTOR_SIG: &str = "ef215c59990c8046b84795d1bed89dc6f685c52e49c63e29192ac58e1c856412383b7bf82069be0037f263dcfc69a66048896cc152e841b54e4fed3029a1dc0a";
}
