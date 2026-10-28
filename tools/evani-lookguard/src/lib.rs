//! evani-lookguard: the look guard (docs/LIVE_API.md 15).
//!
//! Before any bake, a look's scripted captures are compared with its
//! baseline (brightness, contrast, saturation, clipped highlights, crushed
//! shadows, bloom coverage); anything outside the look's limits is flagged,
//! and a flagged result can't be baked without Chris's approval.
//!
//! The limits themselves are guarded: a look's limits, baseline captures,
//! capture moments and the stat version are hashed ([`guardset`]), and every
//! game build and every bake checks that hash against the record Evani.App
//! signed when Chris last approved them ([`record`], [`store`], [`verify`]).
//!
//! A native dev tool. Look crates use it only as a build-dependency without
//! default features (the record check alone), so nothing here reaches a
//! game's download.

pub mod guardset;
pub mod hex;
pub mod record;
pub mod store;
pub mod verify;

#[cfg(feature = "full")]
pub mod approvals;
#[cfg(feature = "full")]
pub mod audit;
#[cfg(feature = "full")]
pub mod hmac;
#[cfg(feature = "full")]
pub mod limits;
#[cfg(feature = "full")]
pub mod png;
#[cfg(feature = "full")]
pub mod shots;
#[cfg(feature = "full")]
pub mod stats;

#[cfg(any(test, feature = "test-signer"))]
pub mod testkey;

/// SHA-256 of `bytes` as 64 lowercase hex chars.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(bytes))
}

/// SHA-256 of `bytes`.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

/// Milliseconds since the Unix epoch, now.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// `2026-10-13T14:02:11Z` from Unix milliseconds (Howard Hinnant's
/// civil_from_days; no date crate).
pub fn iso_utc_ms(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
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
