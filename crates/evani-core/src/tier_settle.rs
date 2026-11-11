//! tier-settle (2026-11-02): when a tier switch has fully applied, for
//! captures and tools ([`TierSettle`]). Plain data, in every build; only the
//! renderer's capture builds feed it (see `tier_switch` for the
//! headless no-shader-rebuild guard).

/// When a tier switch has fully applied (tier-settle, 2026-11-02): the frame
/// drawn with the new tier's size, sample count and shadow state, and none
/// of its work (model files, uploads, sun cones, pipeline builds) left.
/// Captures and tools wait for it so they never judge a frame still drawn
/// with the previous tier's state (material-view's Low shadows looked
/// "fixed" or "broken" across five critic rounds depending on how soon a
/// Low capture followed an Ultra one). The renderer's capture builds feed it
/// once a frame and write `<html data-evani-tier-settled="id">` once settled,
/// `data-evani-settling` while not; ship builds never call it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TierSettle {
    /// The tier last drawn.
    pub tier: String,
    /// The switch to `tier` has fully applied.
    pub settled: bool,
    stable: u32,
    last: Option<(String, [u32; 2], bool)>,
}

impl TierSettle {
    /// Quiet frames in a row (same tier, size and sample count, nothing
    /// under way) before a tier counts as settled.
    pub const FRAMES: u32 = 3;

    /// One frame: the tier drawn, the render size, whether the sample
    /// count is the one the tier asks for (`msaa_ok`; false while its 4x
    /// pipelines build) and whether work for the tier is still under way
    /// (`busy`). Returns whether `settled` or `tier` changed.
    pub fn frame(&mut self, tier: &str, size: [u32; 2], msaa_ok: bool, busy: bool) -> bool {
        let before = (self.tier.clone(), self.settled);
        let key = (tier.to_string(), size, msaa_ok);
        if self.last.as_ref() != Some(&key) {
            self.stable = 0;
            self.last = Some(key);
        }
        self.stable = if msaa_ok && !busy { self.stable.saturating_add(1) } else { 0 };
        self.settled = self.stable >= Self::FRAMES;
        self.tier = tier.to_string();
        before != (self.tier.clone(), self.settled)
    }
}

#[cfg(test)]
mod tests {
    use super::TierSettle;

    #[test]
    fn a_tier_settles_after_quiet_frames_and_a_switch_or_work_resets_it() {
        let mut s = TierSettle::default();
        for _ in 0..TierSettle::FRAMES - 1 {
            s.frame("ultra", [1280, 720], true, false);
        }
        assert!(!s.settled);
        assert!(s.frame("ultra", [1280, 720], true, false));
        assert!(s.settled && s.tier == "ultra");
        // The switch: a new tier (and its size) starts over.
        assert!(s.frame("low", [960, 540], true, false));
        assert!(!s.settled && s.tier == "low");
        // A model still reading, or the 4x pipelines still building, holds it.
        for _ in 0..10 {
            s.frame("low", [960, 540], true, true);
        }
        assert!(!s.settled);
        for _ in 0..10 {
            s.frame("low", [960, 540], false, false);
        }
        assert!(!s.settled);
        for _ in 0..TierSettle::FRAMES {
            s.frame("low", [960, 540], true, false);
        }
        assert!(s.settled);
        // A resize on the same tier (render scale) starts over too.
        s.frame("low", [800, 450], true, false);
        assert!(!s.settled);
    }
}
