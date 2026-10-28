//! Pending look-guard requests and their approval (docs/LIVE_API.md 15.7).
//!
//! The AI (MCP, `/ctl`, the HTTP command door) can only *ask*: propose new
//! limits or a new baseline, or request a flagged bake. Each request gets a
//! random id, the hash it is about, and an expiry (5 minutes). Only an
//! approval that passes every check below answers it, and only once:
//!
//! 1. approvals are on: the engine got a secret and Evani.App's public key
//!    on its first stdin line, and that key equals the one pinned in
//!    Evani.App's data folder;
//! 2. the `Authorization: Evani-Approve <hex>` header is
//!    HMAC-SHA256(secret, `approvalId|hash|expiry`) (transport check);
//! 3. the body's Ed25519 signature over the `evani-look-approve/1` form is
//!    made with the pinned key (the real check: only Evani.App holds the
//!    private key);
//! 4. it is for this engine session, names a pending request, matches its
//!    kind, look, hash and expiry, is not expired and was never used;
//! 5. for limits: the look's guard hash is still the one the proposal was
//!    made against (a stale proposal can't overwrite newer limits).
//!
//! An approved bake becomes a one-time *grant*. Chris approves the bake
//! hash (look, guard hash, captures, bundle, flagged stats); the grant is
//! then keyed on [`bind_key`] (guard hash + bundle hash) so the retried bake
//! finds it before capturing again, and [`Approvals::take_grant`] consumes
//! it. A different bundle or a changed guard set gets no grant.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::hex;
use crate::hmac::{ct_eq, hmac_sha256};
use crate::record::{Approval, verify_sig};

/// How long a request (and a bake grant) stays valid.
pub const TTL_MS: u64 = 5 * 60 * 1000;
/// At most this many requests wait at once; the oldest goes first.
pub const MAX_PENDING: usize = 16;

/// Random hex from the OS generator.
pub fn random_hex(bytes: usize) -> Result<String, String> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| format!("OS random generator failed: {e}"))?;
    Ok(hex::encode(&b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Limits,
    Bake,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Limits => "limits",
            Kind::Bake => "bake",
        }
    }
}

/// What a bake grant is keyed on: the look's guard hash and the bake
/// bundle's hash. Re-captures may differ by a pixel; the bundle may not.
pub fn bind_key(guard_hash: &str, bundle_hash: &str) -> String {
    crate::sha256_hex(format!("evani-look-bind/1\nguard={guard_hash}\nbundle={bundle_hash}\n").as_bytes())
}

/// A request as the AI side asks for it (see [`Approvals::propose`]).
#[derive(Debug, Clone)]
pub struct Proposal {
    pub kind: Kind,
    pub look: String,
    /// Limits: the proposed guard hash. Bake: the bake hash.
    pub hash: String,
    /// Limits: the guard hash proposed against. Bake: the guard hash checked.
    pub old_hash: Option<String>,
    /// Bake only: [`bind_key`] of the guard and bundle.
    pub bind: Option<String>,
    pub rows: Vec<Value>,
    pub files: Vec<(String, Vec<u8>)>,
    /// Set by the server from the door the command came in on.
    pub origin: String,
    pub note: String,
}

/// One request waiting for Chris.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: String,
    pub kind: Kind,
    pub look: String,
    /// Limits: the proposed guard hash. Bake: the bake hash.
    pub hash: String,
    /// Limits: the guard hash the proposal was made against (`None`: the
    /// look had no guard set yet). Bake: the guard hash checked.
    pub old_hash: Option<String>,
    /// Bake only: what the grant is keyed on ([`bind_key`]).
    pub bind: Option<String>,
    pub created_ms: u64,
    pub expiry_ms: u64,
    /// Where the request came from, set by the server (`mcp`, `ctl`, `http`).
    pub origin: String,
    pub note: String,
    /// Old-vs-new rows (limits) or breaches (bake), for the card.
    pub rows: Vec<Value>,
    /// Limits: the files to write when approved (relative to the guard folder).
    pub files: Vec<(String, Vec<u8>)>,
}

impl Pending {
    /// The event payload for Evani.App's card (`look_guard.approval_requested`).
    pub fn event(&self, session: &str) -> Value {
        json!({
            "approvalId": self.id,
            "kind": self.kind.as_str(),
            "look": self.look,
            "hash": self.hash,
            "oldHash": self.old_hash,
            "expiry": self.expiry_ms,
            "session": session,
            "origin": self.origin,
            "note": self.note,
            "rows": self.rows,
            "looser": self.rows.iter().any(|r| r["looser"].as_bool() == Some(true)),
            "files": self.files.iter().map(|(p, b)| json!({ "path": p, "bytes": b.len(), "sha256": crate::sha256_hex(b) })).collect::<Vec<_>>(),
        })
    }

    /// The signed approval this request expects.
    pub fn expected(&self, session: &str) -> Approval {
        Approval {
            approval_id: self.id.clone(),
            kind: self.kind.as_str().into(),
            look: self.look.clone(),
            hash: self.hash.clone(),
            expiry: self.expiry_ms,
            session: session.into(),
        }
    }
}

/// A one-time permission to bake a flagged result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub approval_id: String,
    pub look: String,
    /// The bake hash Chris approved.
    pub hash: String,
    /// The [`bind_key`] the retried bake must match.
    pub bind: String,
    pub expiry_ms: u64,
}

/// The keys approvals are checked with. Held in memory only.
pub struct Keys {
    secret: Vec<u8>,
    pubkey: [u8; 32],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret.
        f.debug_struct("Keys").field("key_id", &crate::record::key_id(&self.pubkey)).finish_non_exhaustive()
    }
}

impl Keys {
    /// From the first stdin line's `approve_secret` (≥ 32 bytes of hex) and
    /// `approvals_pubkey`, checked against the key `pinned` in Evani.App's
    /// data folder: the stdin key only switches approvals on if it is the
    /// pinned one, so whoever starts the engine can't bring their own key.
    pub fn new(secret_hex: &str, stdin_pubkey_hex: &str, pinned: Option<[u8; 32]>) -> Result<Self, String> {
        let secret = hex::decode(secret_hex.trim()).filter(|s| s.len() >= 32).ok_or("the approve secret must be at least 32 bytes of hex")?;
        let pubkey = hex::decode_n::<32>(stdin_pubkey_hex.trim()).ok_or("the approvals public key must be 64 hex chars")?;
        let Some(pinned) = pinned else {
            return Err("Evani.App's approvals key isn't pinned in its data folder".into());
        };
        if !ct_eq(&pubkey, &pinned) {
            return Err("the approvals key the engine was started with is not the one Evani.App pinned".into());
        }
        Ok(Keys { secret, pubkey })
    }

    pub fn key_id(&self) -> String {
        crate::record::key_id(&self.pubkey)
    }

    pub fn pubkey(&self) -> [u8; 32] {
        self.pubkey
    }

    /// The header MAC for one approval.
    pub fn mac(&self, approval_id: &str, hash: &str, expiry: u64) -> String {
        hex::encode(&hmac_sha256(&self.secret, format!("{approval_id}|{hash}|{expiry}").as_bytes()))
    }
}

/// Why an approval was refused (code and plain sentence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

/// The body of `POST /look-guard/v1/approve`.
#[derive(Debug, Clone)]
pub struct ApproveBody {
    pub approval: Approval,
    pub sig: [u8; 64],
}

impl ApproveBody {
    pub fn parse(v: &Value) -> Result<Self, Refusal> {
        let s = |k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| refuse("bad_request", format!("approve body has no {k}")));
        let approval = Approval {
            approval_id: s("approvalId")?,
            kind: s("kind")?,
            look: s("look")?,
            hash: s("hash")?,
            expiry: v["expiry"].as_u64().ok_or_else(|| refuse("bad_request", "approve body expiry must be a whole number (unix ms)"))?,
            session: s("session")?,
        };
        approval.check_shape().map_err(|e| refuse("bad_request", e))?;
        let sig = hex::decode_n::<64>(&s("sig")?).ok_or_else(|| refuse("bad_request", "sig must be 128 hex chars"))?;
        Ok(ApproveBody { approval, sig })
    }
}

/// The engine's request book. One per bridge process.
#[derive(Debug)]
pub struct Approvals {
    pub session: String,
    keys: Result<Keys, String>,
    pending: BTreeMap<String, Pending>,
    used: BTreeSet<String>,
    grants: Vec<Grant>,
}

impl Approvals {
    /// `keys`: `Err(why)` when approvals are off (no stdin line, key
    /// mismatch, …). Requests are still recorded so the AI sees them.
    pub fn new(session: String, keys: Result<Keys, String>) -> Self {
        Approvals { session, keys, pending: BTreeMap::new(), used: BTreeSet::new(), grants: Vec::new() }
    }

    /// Switch approvals on (or record why they stay off). The bridge calls
    /// it once, when Evani.App's first stdin line arrives.
    pub fn set_keys(&mut self, keys: Result<Keys, String>) {
        self.keys = keys;
    }

    /// `Ok(key id)` when approvals can be answered, else why not.
    pub fn status(&self) -> Result<String, String> {
        self.keys.as_ref().map(Keys::key_id).map_err(Clone::clone)
    }

    fn expire(&mut self, now_ms: u64) {
        let gone: Vec<String> = self.pending.values().filter(|p| p.expiry_ms < now_ms).map(|p| p.id.clone()).collect();
        for id in gone {
            self.pending.remove(&id);
            self.used.insert(id);
        }
        self.grants.retain(|g| g.expiry_ms >= now_ms);
    }

    /// Record a request; returns it (with its id and expiry).
    pub fn propose(&mut self, req: Proposal, now_ms: u64) -> Result<Pending, String> {
        if req.kind == Kind::Bake && req.bind.is_none() {
            return Err("a bake request needs its bind key".into());
        }
        self.expire(now_ms);
        // One open request per look and kind: a new one replaces the old.
        let same: Vec<String> =
            self.pending.values().filter(|p| p.kind == req.kind && p.look == req.look).map(|p| p.id.clone()).collect();
        for id in same {
            self.pending.remove(&id);
            self.used.insert(id);
        }
        while self.pending.len() >= MAX_PENDING {
            let oldest = self.pending.values().min_by_key(|p| p.created_ms).map(|p| p.id.clone());
            match oldest {
                Some(id) => {
                    self.pending.remove(&id);
                    self.used.insert(id);
                }
                None => break,
            }
        }
        let p = Pending {
            id: format!("la-{}", random_hex(12)?),
            kind: req.kind,
            look: req.look,
            hash: req.hash,
            old_hash: req.old_hash,
            bind: req.bind,
            created_ms: now_ms,
            expiry_ms: now_ms + TTL_MS,
            origin: req.origin,
            note: req.note.chars().take(500).collect(),
            rows: req.rows,
            files: req.files,
        };
        self.pending.insert(p.id.clone(), p.clone());
        Ok(p)
    }

    pub fn pending(&self, now_ms: u64) -> Vec<&Pending> {
        self.pending.values().filter(|p| p.expiry_ms >= now_ms).collect()
    }

    pub fn get(&self, id: &str) -> Option<&Pending> {
        self.pending.get(id)
    }

    /// Check an approval and answer its request. `mac_hex` is the header's
    /// MAC; `guard_hash_now(look)` gives the look's guard hash on disk now
    /// (`None` if it has none). On success the request is removed (and, for
    /// a bake, a grant is added) and returned.
    pub fn approve(
        &mut self,
        body: &ApproveBody,
        mac_hex: &str,
        now_ms: u64,
        guard_hash_now: impl Fn(&str) -> Result<Option<String>, String>,
    ) -> Result<Pending, Refusal> {
        let keys = self.keys.as_ref().map_err(|why| refuse("approvals_disabled", format!("Approvals are off: {why}.")))?;
        let a = &body.approval;
        let want_mac = keys.mac(&a.approval_id, &a.hash, a.expiry);
        if !ct_eq(mac_hex.trim().to_ascii_lowercase().as_bytes(), want_mac.as_bytes()) {
            return Err(refuse("bad_auth", "The approval's header MAC is wrong."));
        }
        if a.session != self.session {
            return Err(refuse("wrong_session", "The approval is for another engine session."));
        }
        verify_sig(&keys.pubkey, a.canonical().as_bytes(), &body.sig).map_err(|e| refuse("bad_signature", e))?;
        if self.used.contains(&a.approval_id) {
            return Err(refuse("already_used", "That approval was already used, replaced or expired."));
        }
        self.expire(now_ms);
        let Some(p) = self.pending.get(&a.approval_id) else {
            return Err(refuse(
                if self.used.contains(&a.approval_id) { "expired" } else { "unknown_approval" },
                "There is no pending request with that id (it may have expired).",
            ));
        };
        if *a != p.expected(&self.session) {
            return Err(refuse("mismatch", "The approval doesn't match the request (kind, look, hash or expiry differ)."));
        }
        if now_ms > p.expiry_ms {
            return Err(refuse("expired", "The request expired before it was approved."));
        }
        if p.kind == Kind::Limits {
            let now = guard_hash_now(&p.look).map_err(|e| refuse("stale", e))?;
            if now != p.old_hash {
                return Err(refuse(
                    "stale",
                    format!("The limits of {} changed since this proposal was made; propose again.", p.look),
                ));
            }
        }
        let p = self.pending.remove(&a.approval_id).ok_or_else(|| refuse("unknown_approval", "gone"))?;
        self.used.insert(p.id.clone());
        if p.kind == Kind::Bake {
            self.grants.push(Grant {
                approval_id: p.id.clone(),
                look: p.look.clone(),
                hash: p.hash.clone(),
                bind: p.bind.clone().unwrap_or_default(),
                expiry_ms: now_ms + TTL_MS,
            });
        }
        Ok(p)
    }

    /// Consume the grant for exactly this look and [`bind_key`], if any.
    pub fn take_grant(&mut self, look: &str, bind: &str, now_ms: u64) -> Option<Grant> {
        self.expire(now_ms);
        let i = self.grants.iter().position(|g| g.look == look && !g.bind.is_empty() && g.bind == bind)?;
        Some(self.grants.remove(i))
    }

    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkey;

    const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn book() -> Approvals {
        let keys = Keys::new(SECRET, &hex::encode(&testkey::public()), Some(testkey::public()));
        Approvals::new("sess-1".into(), keys)
    }

    fn sign(p: &Pending, session: &str) -> (ApproveBody, String) {
        let a = p.expected(session);
        let sig = testkey::sign(a.canonical().as_bytes());
        let keys = Keys::new(SECRET, &hex::encode(&testkey::public()), Some(testkey::public())).unwrap();
        let mac = keys.mac(&a.approval_id, &a.hash, a.expiry);
        (ApproveBody { approval: a, sig }, mac)
    }

    fn h(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn req(kind: Kind, look: &str, hash: &str, old: Option<String>) -> Proposal {
        Proposal {
            kind,
            look: look.into(),
            hash: hash.into(),
            old_hash: old.clone(),
            bind: (kind == Kind::Bake).then(|| bind_key(&old.unwrap_or_default(), &h('u'))),
            rows: vec![],
            files: vec![],
            origin: "mcp".into(),
            note: String::new(),
        }
    }

    #[test]
    fn keys_must_be_the_pinned_ones() {
        let other = ed25519_dalek::SigningKey::from_bytes(&[9; 32]).verifying_key().to_bytes();
        assert!(Keys::new(SECRET, &hex::encode(&other), Some(testkey::public())).unwrap_err().contains("not the one Evani.App pinned"));
        assert!(Keys::new(SECRET, &hex::encode(&testkey::public()), None).is_err());
        assert!(Keys::new("abcd", &hex::encode(&testkey::public()), Some(testkey::public())).is_err());
        let k = Keys::new(SECRET, &hex::encode(&testkey::public()), Some(testkey::public())).unwrap();
        assert!(!format!("{k:?}").contains("0011"));
    }

    #[test]
    fn a_bake_approval_is_single_use_and_bound_to_its_bundle() {
        let mut b = book();
        let p = b.propose(req(Kind::Bake, "evani-look-basic", &h('a'), Some(h('g'))), 1000).unwrap();
        let bind = bind_key(&h('g'), &h('u'));
        assert_eq!(p.bind.as_deref(), Some(bind.as_str()));
        // No grant before approval.
        assert!(b.take_grant("evani-look-basic", &bind, 2000).is_none());
        let (body, mac) = sign(&p, "sess-1");
        b.approve(&body, &mac, 2000, |_| Ok(None)).unwrap();
        // Replay of the same approval: refused.
        assert_eq!(b.approve(&body, &mac, 2001, |_| Ok(None)).unwrap_err().code, "already_used");
        // The grant only fits its own look, guard set and bundle, and only once.
        assert!(b.take_grant("evani-look-basic", &bind_key(&h('g'), &h('v')), 2002).is_none(), "another bundle");
        assert!(b.take_grant("evani-look-basic", &bind_key(&h('f'), &h('u')), 2002).is_none(), "another guard set");
        assert!(b.take_grant("evani-look-basic", &h('a'), 2002).is_none(), "the bake hash is not the key");
        assert!(b.take_grant("evani-look-toon", &bind, 2002).is_none());
        let g = b.take_grant("evani-look-basic", &bind, 2003).unwrap();
        assert_eq!(g.hash, h('a'));
        assert!(b.take_grant("evani-look-basic", &bind, 2004).is_none());
        // A grant expires.
        let p = b.propose(req(Kind::Bake, "evani-look-basic", &h('a'), Some(h('g'))), 3000).unwrap();
        let (body, mac) = sign(&p, "sess-1");
        b.approve(&body, &mac, 3000, |_| Ok(None)).unwrap();
        assert!(b.take_grant("evani-look-basic", &bind, 3000 + TTL_MS + 1).is_none());
        // A bake request without a bind key is refused.
        let mut r = req(Kind::Bake, "evani-look-basic", &h('a'), None);
        r.bind = None;
        assert!(b.propose(r, 4000).is_err());
    }

    #[test]
    fn every_check_refuses() {
        let mut b = book();
        let p = b.propose(req(Kind::Bake, "evani-look-basic", &h('a'), None), 1000).unwrap();
        let (body, mac) = sign(&p, "sess-1");
        // Wrong MAC (e.g. the AI guessing the secret).
        assert_eq!(b.approve(&body, &"0".repeat(64), 2000, |_| Ok(None)).unwrap_err().code, "bad_auth");
        // Right MAC, but signed with another key (someone who knows the secret).
        let mut forged = body.clone();
        forged.sig = ed25519_dalek::Signer::sign(&ed25519_dalek::SigningKey::from_bytes(&[5; 32]), body.approval.canonical().as_bytes()).to_bytes();
        assert_eq!(b.approve(&forged, &mac, 2000, |_| Ok(None)).unwrap_err().code, "bad_signature");
        // Signed for a different hash than the request: the MAC and sig cover it, so it's a mismatch.
        let mut other = p.clone();
        other.hash = h('c');
        let (ob, om) = sign(&other, "sess-1");
        assert_eq!(b.approve(&ob, &om, 2000, |_| Ok(None)).unwrap_err().code, "mismatch");
        // Another session.
        let (sb, sm) = sign(&p, "sess-0");
        assert_eq!(b.approve(&sb, &sm, 2000, |_| Ok(None)).unwrap_err().code, "wrong_session");
        // Expired.
        assert_eq!(b.approve(&body, &mac, 1000 + TTL_MS + 1, |_| Ok(None)).unwrap_err().code, "expired");
        // Unknown id.
        let mut ghost = p.clone();
        ghost.id = "la-nope".into();
        let (gb, gm) = sign(&ghost, "sess-1");
        assert_eq!(b.approve(&gb, &gm, 2000, |_| Ok(None)).unwrap_err().code, "unknown_approval");
        // Approvals off: nothing can be approved.
        let mut off = Approvals::new("sess-1".into(), Err("no stdin line".into()));
        let p = off.propose(req(Kind::Bake, "evani-look-basic", &h('a'), None), 1000).unwrap();
        let (body, mac) = sign(&p, "sess-1");
        assert_eq!(off.approve(&body, &mac, 2000, |_| Ok(None)).unwrap_err().code, "approvals_disabled");
    }

    #[test]
    fn limits_proposals_go_stale_and_are_replaced() {
        let mut b = book();
        let p1 = b.propose(req(Kind::Limits, "evani-look-basic", &h('n'), Some(h('o'))), 1000).unwrap();
        // A second proposal for the same look replaces the first.
        let p2 = b.propose(req(Kind::Limits, "evani-look-basic", &h('m'), Some(h('o'))), 1100).unwrap();
        let (b1, m1) = sign(&p1, "sess-1");
        assert_eq!(b.approve(&b1, &m1, 1200, |_| Ok(Some(h('o')))).unwrap_err().code, "already_used");
        // The guard changed on disk since: stale.
        let (b2, m2) = sign(&p2, "sess-1");
        assert_eq!(b.approve(&b2, &m2, 1200, |_| Ok(Some(h('x')))).unwrap_err().code, "stale");
        // Still pending after a refused attempt; approves when the guard is as proposed against.
        let ok = b.approve(&b2, &m2, 1300, |_| Ok(Some(h('o')))).unwrap();
        assert_eq!(ok.hash, h('m'));
        assert!(b.grants().is_empty());
        assert!(b.pending(1300).is_empty());
    }
}
