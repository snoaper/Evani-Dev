//! Seeds per piece, not per run (docs/MODELGEN_PLAN.md 3.2).
//!
//! Every piece has a stable path id (`lid/plank_3`); its random stream is
//! `Pcg32::new(model_seed, hash(path))`, and each varied property draws from
//! its own keyed stream (`hash(path + "#" + key)`). Adding a plank, a part or
//! a varied property therefore never reshuffles any other piece's values.

use crate::rng::Pcg32;

/// FNV-1a, 64-bit: a fixed, platform-free hash of a path.
pub fn path_hash(path: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The random stream of one piece.
pub fn stream(seed: u64, path: &str) -> Pcg32 {
    Pcg32::new(seed, path_hash(path))
}

/// Seeded variation for a model: one value per (piece path, property key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vary {
    pub seed: u64,
}

impl Vary {
    pub fn new(seed: u64) -> Self {
        Vary { seed }
    }
    /// Uniform in [0, 1) for the piece's property `key`.
    pub fn unit(&self, path: &str, key: &str) -> f64 {
        let mut k = String::with_capacity(path.len() + key.len() + 1);
        k.push_str(path);
        k.push('#');
        k.push_str(key);
        stream(self.seed, &k).next_f64()
    }
    /// Uniform in [-amount, amount).
    pub fn signed(&self, path: &str, key: &str, amount: f64) -> f64 {
        (self.unit(path, key) * 2.0 - 1.0) * amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_stable_per_piece_and_key() {
        let v = Vary::new(7);
        let a = v.unit("lid/plank_3", "tone");
        assert_eq!(a.to_bits(), Vary::new(7).unit("lid/plank_3", "tone").to_bits());
        assert_ne!(a, v.unit("lid/plank_4", "tone"));
        assert_ne!(a, v.unit("lid/plank_3", "tilt"));
        assert_ne!(a, Vary::new(8).unit("lid/plank_3", "tone"));
        // Pinned: FNV-1a of the empty string and a known path.
        assert_eq!(path_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(path_hash("a"), 0xaf63_dc4c_8601_ec8c);
        let s = v.signed("x", "k", 0.5);
        assert!((-0.5..0.5).contains(&s));
    }
}
