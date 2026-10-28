//! The approved-limits record Evani.App signs, and the signed approval it
//! sends for one proposal or one flagged bake (docs/LIVE_API.md 15.5, 15.7).
//!
//! Both are verified with Ed25519 (RFC 8032, strict verification) against
//! the public key Evani.App pins in its own data folder. The signed bytes are
//! a canonical text form, not the JSON, so neither side has to agree on JSON
//! formatting: one `key=value` line per field, in the order below, each
//! ending with one LF, UTF-8, no field may contain CR or LF.
//!
//! Record (`<look id>.json` in Evani.App's store):
//!
//! ```text
//! evani-look-approval/1
//! projectId=<id>
//! projectPath=<path as written in the record>
//! look=<look id>
//! seq=<decimal, no leading zeros>
//! approvedAt=<YYYY-MM-DDTHH:MM:SSZ>
//! hash=<guard hash, 64 lowercase hex>
//! ```
//!
//! Approval (`POST /look-guard/v1/approve` body `sig`):
//!
//! ```text
//! evani-look-approve/1
//! approvalId=<id>
//! kind=<limits|bake>
//! look=<look id>
//! hash=<64 lowercase hex>
//! expiry=<unix ms>
//! session=<the engine's session nonce>
//! ```

use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::{Value, json};

pub const RECORD_FORMAT: &str = "evani-look-approval/1";
pub const APPROVE_FORMAT: &str = "evani-look-approve/1";

/// One approved-limits record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub project_id: String,
    pub project_path: String,
    pub look: String,
    pub seq: u64,
    pub approved_at: String,
    pub hash: String,
    pub sig: [u8; 64],
}

fn clean(field: &str, v: &str) -> Result<(), String> {
    if v.is_empty() || v.len() > 1024 || v.chars().any(char::is_control) {
        return Err(format!("record field {field} is empty, too long or has control characters"));
    }
    Ok(())
}

impl Record {
    /// The exact bytes that are signed.
    pub fn canonical(&self) -> String {
        format!(
            "{RECORD_FORMAT}\nprojectId={}\nprojectPath={}\nlook={}\nseq={}\napprovedAt={}\nhash={}\n",
            self.project_id, self.project_path, self.look, self.seq, self.approved_at, self.hash
        )
    }

    /// Parse a record file. Checks the shape only; [`Record::verify`]
    /// checks the signature.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("record is not JSON: {e}"))?;
        if v["format"].as_str() != Some(RECORD_FORMAT) {
            return Err(format!("record format is not {RECORD_FORMAT}"));
        }
        let s = |k: &str| -> Result<String, String> {
            let t = v[k].as_str().ok_or_else(|| format!("record has no {k}"))?;
            clean(k, t)?;
            Ok(t.to_string())
        };
        let seq = v["seq"].as_u64().ok_or("record seq must be a whole number")?;
        let hash = s("hash")?;
        if !crate::hex::is_sha256(&hash) {
            return Err("record hash must be 64 lowercase hex chars".into());
        }
        let sig = crate::hex::decode_n::<64>(&s("sig")?).ok_or("record sig must be 128 hex chars")?;
        Ok(Self {
            project_id: s("projectId")?,
            project_path: s("projectPath")?,
            look: s("look")?,
            seq,
            approved_at: s("approvedAt")?,
            hash,
            sig,
        })
    }

    /// The record as JSON (what Evani.App writes).
    pub fn to_json(&self) -> Value {
        json!({
            "format": RECORD_FORMAT,
            "projectId": self.project_id,
            "projectPath": self.project_path,
            "look": self.look,
            "seq": self.seq,
            "approvedAt": self.approved_at,
            "hash": self.hash,
            "sig": crate::hex::encode(&self.sig),
        })
    }

    /// Is the signature Evani.App's (made with the key `pubkey`)?
    pub fn verify(&self, pubkey: &[u8; 32]) -> Result<(), String> {
        verify_sig(pubkey, self.canonical().as_bytes(), &self.sig)
    }
}

/// One signed approval of a pending request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub approval_id: String,
    pub kind: String,
    pub look: String,
    pub hash: String,
    pub expiry: u64,
    pub session: String,
}

impl Approval {
    pub fn canonical(&self) -> String {
        format!(
            "{APPROVE_FORMAT}\napprovalId={}\nkind={}\nlook={}\nhash={}\nexpiry={}\nsession={}\n",
            self.approval_id, self.kind, self.look, self.hash, self.expiry, self.session
        )
    }

    /// Check the fields are safe to put in the canonical form.
    pub fn check_shape(&self) -> Result<(), String> {
        clean("approvalId", &self.approval_id)?;
        clean("look", &self.look)?;
        clean("session", &self.session)?;
        if self.kind != "limits" && self.kind != "bake" {
            return Err("kind must be limits or bake".into());
        }
        if !crate::hex::is_sha256(&self.hash) {
            return Err("hash must be 64 lowercase hex chars".into());
        }
        Ok(())
    }
}

/// Strict Ed25519 verification of `sig` over `msg`.
pub fn verify_sig(pubkey: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> Result<(), String> {
    let key = VerifyingKey::from_bytes(pubkey).map_err(|_| "the approvals public key is not a valid Ed25519 key".to_string())?;
    let sig = Signature::from_bytes(sig);
    key.verify_strict(msg, &sig)
        .map_err(|_| "the signature is not Evani.App's (bad signature)".to_string())
}

/// A short id for a public key: the first 16 hex chars of its SHA-256.
pub fn key_id(pubkey: &[u8; 32]) -> String {
    crate::sha256_hex(pubkey)[..16].to_string()
}

/// A project path in comparable form: `\` → `/`, no `\\?\` prefix, no
/// trailing `/`, lowercase (Windows paths are case-insensitive).
pub fn norm_path(p: &str) -> String {
    let p = p.strip_prefix(r"\\?\").unwrap_or(p);
    let mut s = p.replace('\\', "/").to_lowercase();
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkey;

    fn rec() -> Record {
        let mut r = Record {
            project_id: "evani-dev".into(),
            project_path: "C:\\dev\\evani-dev".into(),
            look: "evani-look-basic".into(),
            seq: 3,
            approved_at: "2026-10-14T15:00:00Z".into(),
            hash: "ab".repeat(32),
            sig: [0; 64],
        };
        r.sig = testkey::sign(r.canonical().as_bytes());
        r
    }

    #[test]
    fn canonical_form() {
        assert_eq!(
            rec().canonical(),
            format!(
                "evani-look-approval/1\nprojectId=evani-dev\nprojectPath=C:\\dev\\evani-dev\nlook=evani-look-basic\nseq=3\napprovedAt=2026-10-14T15:00:00Z\nhash={}\n",
                "ab".repeat(32)
            )
        );
    }

    #[test]
    fn parse_verify_and_tamper() {
        let r = rec();
        let text = r.to_json().to_string();
        let back = Record::parse(&text).unwrap();
        assert_eq!(back, r);
        back.verify(&testkey::public()).unwrap();
        // Any field change breaks the signature.
        let mut t = back.clone();
        t.seq = 4;
        assert!(t.verify(&testkey::public()).is_err());
        let mut t = back.clone();
        t.hash = "cd".repeat(32);
        assert!(t.verify(&testkey::public()).is_err());
        // A different key never verifies.
        let other = ed25519_dalek::SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes();
        assert!(back.verify(&other).is_err());
        // Shape errors.
        assert!(Record::parse("{}").is_err());
        let mut v = r.to_json();
        v["look"] = json!("a\nb");
        assert!(Record::parse(&v.to_string()).is_err());
        let mut v = r.to_json();
        v["hash"] = json!("AB".repeat(32));
        assert!(Record::parse(&v.to_string()).is_err());
    }

    #[test]
    fn paths_compare_loosely() {
        assert_eq!(norm_path(r"\\?\C:\dev\evani-dev\"), "c:/dev/evani-dev");
        assert_eq!(norm_path("c:/dev/evani-dev"), "c:/dev/evani-dev");
    }
}
