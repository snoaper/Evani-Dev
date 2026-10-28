//! Seeded PCG32 (XSH RR 64/32), as in the reference `pcg32_random_r`.
//! Same seed and stream give the same sequence on every platform.

const MULT: u64 = 6_364_136_223_846_793_005;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Seed with an initial state and a stream selector (any value).
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut r = Self {
            state: 0,
            inc: (stream << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    /// Seed with the default stream.
    pub fn seeded(seed: u64) -> Self {
        Self::new(seed, 0xda3e_39cb_94b9_5bdb)
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    pub fn next_u64(&mut self) -> u64 {
        (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32())
    }

    /// Uniform in `[0, 1)` with 24 bits of precision.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[0, bound)` without modulo bias. `bound == 0` returns 0.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % bound;
            }
        }
    }

    /// Uniform integer in `[lo, hi]` (inclusive). Swaps if reversed.
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
        if span > u64::from(u32::MAX) {
            return self.next_u32() as i32;
        }
        (i64::from(lo) + i64::from(self.below(span as u32))) as i32
    }

    /// Uniform float in `[lo, hi)`.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Derive an independent generator (e.g. per subsystem) deterministically.
    pub fn fork(&mut self, stream: u64) -> Self {
        Self::new(self.next_u64(), stream)
    }
}

impl Default for Pcg32 {
    fn default() -> Self {
        Self::seeded(0x853c_49e6_748f_ea9b)
    }
}

// Checkpoint access, kept after every other method so that adding it moved
// no line above (panic locations carry line numbers: a moved line changes a
// game's bytes; see DECISIONS.md "Multiplayer", byte proof).
impl Pcg32 {
    /// The generator's whole state `(state, inc)`, for checkpoints (a
    /// multiplayer referee's save); [`Pcg32::from_raw`] restores it.
    pub fn to_raw(&self) -> (u64, u64) {
        (self.state, self.inc)
    }

    /// A generator from [`Pcg32::to_raw`]'s state (`inc` is forced odd).
    pub fn from_raw(state: u64, inc: u64) -> Self {
        Self { state, inc: inc | 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pcg_reference_vectors() {
        // pcg32-demo from the reference C implementation: seed 42, stream 54.
        let mut r = Pcg32::new(42, 54);
        let got: Vec<u32> = (0..6).map(|_| r.next_u32()).collect();
        assert_eq!(
            got,
            vec![0xa15c02b7, 0x7b47f409, 0xba1d3330, 0x83d2f293, 0xbfa4784b, 0xcbed606e]
        );
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Pcg32::seeded(1234);
        let mut b = Pcg32::seeded(1234);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let mut c = Pcg32::seeded(1235);
        let mut a = Pcg32::seeded(1234);
        let same = (0..100).filter(|_| a.next_u32() == c.next_u32()).count();
        assert!(same < 3);
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut r = Pcg32::seeded(7);
        for _ in 0..10_000 {
            let f = r.next_f32();
            assert!((0.0..1.0).contains(&f));
            let i = r.range_i32(-3, 3);
            assert!((-3..=3).contains(&i));
            assert!(r.below(10) < 10);
            let x = r.range_f32(2.0, 4.0);
            assert!((2.0..4.0).contains(&x));
        }
        assert_eq!(r.below(0), 0);
        let _ = r.range_i32(i32::MIN, i32::MAX);
    }

    #[test]
    fn shuffle_is_deterministic_permutation() {
        let mut a = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut b = a;
        Pcg32::seeded(99).shuffle(&mut a);
        Pcg32::seeded(99).shuffle(&mut b);
        assert_eq!(a, b);
        let mut sorted = a;
        sorted.sort();
        assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn serde_round_trip_resumes_sequence() {
        let mut r = Pcg32::seeded(5);
        r.next_u32();
        let json = serde_json::to_string(&r).unwrap();
        let mut back: Pcg32 = serde_json::from_str(&json).unwrap();
        assert_eq!(r.next_u32(), back.next_u32());
    }
}
