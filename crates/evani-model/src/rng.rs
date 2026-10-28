//! Seeded randomness and fixed-tile parallel work, both deterministic.
//!
//! `Pcg32` follows the published PCG description (a 64-bit linear
//! congruential state with a permuted 32-bit output: xorshift of the high
//! bits, then a rotation chosen by the top bits). `map_tiles` splits work into
//! tiles that threads take by index and merges the results in tile order, so
//! the thread count never changes a result (docs/model_research/18).

#[derive(Clone, Debug)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const MULT: u64 = 6_364_136_223_846_793_005;

impl Pcg32 {
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut r = Pcg32 { state: 0, inc: (stream << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let x = (((old >> 18) ^ old) >> 27) as u32;
        x.rotate_right((old >> 59) as u32)
    }
    /// Uniform in [0, 1) with 32 bits of resolution.
    pub fn next_f64(&mut self) -> f64 {
        self.next_u32() as f64 * (1.0 / 4_294_967_296.0)
    }
    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
}

/// Run `f(tile)` for every tile in `0..tiles` on up to `threads` threads and
/// return the results in tile order.
pub fn map_tiles<T: Send, F: Fn(usize) -> T + Sync>(tiles: usize, threads: usize, f: F) -> Vec<T> {
    let threads = threads.clamp(1, tiles.max(1));
    if threads == 1 {
        return (0..tiles).map(&f).collect();
    }
    let mut parts: Vec<Vec<(usize, T)>> = Vec::with_capacity(threads);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let f = &f;
                s.spawn(move || (t..tiles).step_by(threads).map(|i| (i, f(i))).collect::<Vec<_>>())
            })
            .collect();
        for h in handles {
            // A panicking worker re-raises its panic here, as a serial run would.
            match h.join() {
                Ok(v) => parts.push(v),
                Err(e) => std::panic::resume_unwind(e),
            }
        }
    });
    let mut slots: Vec<Option<T>> = (0..tiles).map(|_| None).collect();
    for part in parts {
        for (i, v) in part {
            slots[i] = Some(v);
        }
    }
    slots.into_iter().flatten().collect()
}

/// Threads to use by default: the machine's, capped at 8 (shared PCs).
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcg_is_repeatable_and_streams_differ() {
        let mut a = Pcg32::new(42, 54);
        let mut b = Pcg32::new(42, 54);
        let mut c = Pcg32::new(42, 55);
        let va: Vec<u32> = (0..8).map(|_| a.next_u32()).collect();
        let vb: Vec<u32> = (0..8).map(|_| b.next_u32()).collect();
        let vc: Vec<u32> = (0..8).map(|_| c.next_u32()).collect();
        assert_eq!(va, vb);
        assert_ne!(va, vc);
    }

    #[test]
    fn tiles_merge_in_order_for_any_thread_count() {
        let want: Vec<u64> = (0..37).map(|i| (i as u64) * 31 + 7).collect();
        for t in [1, 2, 4, 16] {
            assert_eq!(map_tiles(37, t, |i| (i as u64) * 31 + 7), want);
        }
    }
}
