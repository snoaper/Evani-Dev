//! Cache keys: 64-bit FNV-1a (the engine's content hash, evani-core
//! `assets::content_hash`) over a canonical byte stream. Every field is
//! length-prefixed and every float is quantised to 1/10,000, so the same
//! inputs give the same key on every machine and `-0.0` keys as `0.0`.
//!
//! Three keys, one per cost:
//! - [`GeometryKey`]: what the expensive geometry stage is cached by (memory
//!   LRU and OPFS): the engine version, the generator's version, the asset id,
//!   its source model's content hash, the style's geometry stage and the
//!   style's geometry controls. Never the material, the light or the quality.
//! - [`VariantKey`]: the model the scene draws: the geometry key, the region
//!   bindings (materials), the surface stage, the surface controls and the
//!   material edits. Never the light or the quality.
//! - [`PassKey`]: the screen pass's settings. Changing it never rebuilds.

use std::fmt;

/// FNV-1a 64 offset basis.
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 prime.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 64-bit FNV-1a of `bytes` (same values as evani-core's content hash).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// A canonical byte stream for a key.
#[derive(Debug, Clone, Default)]
pub struct KeyWriter {
    buf: Vec<u8>,
}

impl KeyWriter {
    /// A stream that starts with its domain (`geometry`, `variant`, `pass`),
    /// so two kinds of key never collide on the same fields.
    pub fn new(domain: &str) -> Self {
        let mut w = Self::default();
        w.str(domain);
        w
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.buf.extend((s.len() as u64).to_le_bytes());
        self.buf.extend(s.as_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend(v.to_le_bytes());
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.buf.push(u8::from(v));
        self
    }

    /// A float, quantised to 1/10,000 (a non-finite value keys as one fixed
    /// value).
    pub fn f32(&mut self, v: f32) -> &mut Self {
        let q = if v.is_finite() { (f64::from(v) * 10_000.0).round() as i64 } else { i64::MIN };
        self.buf.extend(q.to_le_bytes());
        self
    }

    pub fn finish(&self) -> u64 {
        fnv1a(&self.buf)
    }
}

macro_rules! key {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            /// 16 hex digits, the form stream.json and OPFS file names use.
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:016x}", self.0)
            }
        }
    };
}

key!(
    /// The geometry stage's output (see the module docs).
    GeometryKey
);
key!(
    /// The drawn model (see the module docs).
    VariantKey
);
key!(
    /// The screen pass's settings (see the module docs).
    PassKey
);
