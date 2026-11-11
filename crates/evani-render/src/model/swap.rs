//! A model slot's copies on the GPU side (bake-keeps-old-copy, 2026-11-02).
//!
//! A model uploaded again (a kit's [`super::Models::replace_glb`], an
//! edited style) on a tier that reads the sun cones (Lite, Low) used to take
//! its slot hidden until its cones were baked: material-view's three dense
//! styled spheres vanished for seconds on Low, and the scene drew no
//! stand-in, since the model stayed `Ready`. Now the new copy waits behind
//! the one on screen and takes the slot once its cones are in (or at once
//! when a shadow map is back, so the cones are not read). Generic over the
//! copy so a native test drives it; the browser's copy is `GpuModel`.

/// One drawable copy of a model.
pub trait SlotCopy: Sized {
    /// Drawn (and the model `Ready`).
    fn shown(&self) -> bool;
    fn set_shown(&mut self);
    /// Its sun cones are in (or it needs none).
    fn baked(&self) -> bool;
    /// The copy waiting to replace this one.
    fn next(&self) -> Option<&Self>;
    fn set_next(&mut self, next: Option<Box<Self>>);
    fn take_next(&mut self) -> Option<Box<Self>>;
    /// The copy whose cones bake: the waiting one, else this one.
    fn baking(&mut self) -> &mut Self;
}

/// Put a new upload in its slot: behind the copy on screen while the new
/// one waits for its cones, else in its place.
pub fn keep_old<T: SlotCopy>(slot: &mut Option<T>, m: T) {
    match slot {
        Some(old) if !m.shown() && old.shown() => old.set_next(Some(Box::new(m))),
        _ => *slot = Some(m),
    }
}

/// The waiting copy takes the slot once its cones are in, or at once when
/// the frame has a shadow map (`cones_read` false: they are not read).
pub fn promote<T: SlotCopy>(slot: &mut Option<T>, cones_read: bool) {
    let Some(old) = slot else { return };
    if old.next().is_some_and(|n| n.baked() || !cones_read)
        && let Some(mut n) = old.take_next()
    {
        n.set_shown();
        *slot = Some(*n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy: its name, shown, cones in, and the one waiting behind it.
    struct C(&'static str, bool, bool, Option<Box<C>>);

    impl SlotCopy for C {
        fn shown(&self) -> bool {
            self.1
        }
        fn set_shown(&mut self) {
            self.1 = true;
        }
        fn baked(&self) -> bool {
            self.2
        }
        fn next(&self) -> Option<&Self> {
            self.3.as_deref()
        }
        fn set_next(&mut self, next: Option<Box<Self>>) {
            self.3 = next;
        }
        fn take_next(&mut self) -> Option<Box<Self>> {
            self.3.take()
        }
        fn baking(&mut self) -> &mut Self {
            match self.3 {
                Some(ref mut n) => n,
                None => self,
            }
        }
    }

    /// What a frame draws from the slot: the shown copy's name, if any.
    fn drawn(slot: &Option<C>) -> Option<&'static str> {
        slot.as_ref().filter(|c| c.shown()).map(|c| c.0)
    }

    #[test]
    fn a_rebuilt_model_keeps_its_old_copy_on_screen_until_the_new_cones_are_in() {
        let mut slot = Some(C("old", true, true, None));
        // The rebuild lands on Low (cones read): the new copy is hidden.
        keep_old(&mut slot, C("new", false, false, None));
        assert_eq!(drawn(&slot), Some("old"), "the model never drops while its new copy bakes");
        // The bake works on the waiting copy, not the one on screen.
        if let Some(s) = slot.as_mut() {
            assert_eq!(s.baking().0, "new");
        }
        for _ in 0..5 {
            promote(&mut slot, true);
            assert_eq!(drawn(&slot), Some("old"));
        }
        // Its cones are in: it takes the slot, shown.
        if let Some(s) = slot.as_mut() {
            s.baking().2 = true;
        }
        promote(&mut slot, true);
        assert_eq!(drawn(&slot), Some("new"));
        assert!(slot.as_ref().is_some_and(|s| s.next().is_none()));
    }

    #[test]
    fn a_shadow_map_or_a_first_upload_needs_no_wait() {
        // A shown upload (a shadow-map tier) replaces the old copy at once.
        let mut slot = Some(C("old", true, true, None));
        keep_old(&mut slot, C("new", true, false, None));
        assert_eq!(drawn(&slot), Some("new"));
        // A waiting copy goes in as soon as a frame has a shadow map.
        let mut slot = Some(C("old", true, true, None));
        keep_old(&mut slot, C("new", false, false, None));
        promote(&mut slot, false);
        assert_eq!(drawn(&slot), Some("new"));
        // A first upload has nothing to keep: it waits hidden (the scene's
        // placeholder draws, the model is still Loading).
        let mut slot = None;
        keep_old(&mut slot, C("first", false, false, None));
        assert_eq!(drawn(&slot), None);
        // A newer rebuild replaces the one still waiting.
        let mut slot = Some(C("old", true, true, None));
        keep_old(&mut slot, C("new", false, false, None));
        keep_old(&mut slot, C("newer", false, false, None));
        assert_eq!(slot.as_ref().and_then(|s| s.next()).map(|n| n.0), Some("newer"));
        assert_eq!(drawn(&slot), Some("old"));
    }
}

#[cfg(test)]
mod budget_tests {
    use crate::model::sun::{BAKE_MS, FRAME_UNITS, START_RATE, frame_units, next_rate};

    #[test]
    fn the_bake_budget_follows_the_device_and_never_passes_frame_units() {
        // A shipped desktop build: FRAME_UNITS in about BAKE_MS, as before.
        assert_eq!(frame_units(START_RATE), FRAME_UNITS);
        // A device ten times slower (a dev build, a phone): its frames settle
        // near BAKE_MS of bake each, not ten times that.
        let mut rate = START_RATE;
        for _ in 0..30 {
            let u = frame_units(rate);
            let ms = u as f64 / (START_RATE / 10.0);
            rate = next_rate(rate, u, ms);
        }
        let ms = frame_units(rate) as f64 / (START_RATE / 10.0);
        assert!((ms - BAKE_MS).abs() < 0.5, "a slow device's bake frame takes {ms:.1} ms");
        // A fast one never takes more than FRAME_UNITS; a frame too short to
        // time keeps the rate; the slowest still moves on.
        assert_eq!(frame_units(next_rate(START_RATE, FRAME_UNITS, 0.5)), FRAME_UNITS);
        assert_eq!(next_rate(123.0, FRAME_UNITS, 0.0), 123.0);
        assert_eq!(frame_units(0.0), FRAME_UNITS / 64);
    }
}
