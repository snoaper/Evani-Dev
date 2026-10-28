//! TEST ONLY. The well-known look-guard test key.
//!
//! Its private seed is public (it is right here), so anything it signs is
//! worthless. It exists for unit tests, the test vectors in
//! docs/LIVE_API.md 15.10 and the end-to-end proof, where a harness plays
//! Evani.App. Nothing real may ever be signed with it, and the engine marks
//! any record or approval made with it as `test_key: true`.

use ed25519_dalek::{Signer, SigningKey};

/// The 32-byte seed: the ASCII text `EVANI-LOOK-GUARD-TEST-KEY-NEVER!`.
pub const TEST_ONLY_SEED: [u8; 32] = *b"EVANI-LOOK-GUARD-TEST-KEY-NEVER!";

fn key() -> SigningKey {
    SigningKey::from_bytes(&TEST_ONLY_SEED)
}

/// The test public key.
pub fn public() -> [u8; 32] {
    key().verifying_key().to_bytes()
}

/// Sign `msg` with the test key.
pub fn sign(msg: &[u8]) -> [u8; 64] {
    key().sign(msg).to_bytes()
}
