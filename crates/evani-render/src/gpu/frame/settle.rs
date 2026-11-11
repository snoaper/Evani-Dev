//! tier-settle on the page (capture builds only): the renderer feeds
//! [`TierSettle`] once a frame and writes `<html data-evani-tier-settled="id">`
//! once the active tier has fully applied, `data-evani-settling` while not.
//! The review's captures and its tier-switch check wait for it.

use std::cell::RefCell;

use evani_core::tier_settle::TierSettle;

thread_local! {
    static SETTLE: RefCell<TierSettle> = RefCell::new(TierSettle::default());
}

/// This frame's tier, render size, whether the sample count is the one the
/// tier asks for, and whether work for it is still under way.
pub(super) fn note(tier: &str, size: [u32; 2], msaa_ok: bool, busy: bool) {
    let changed = SETTLE.with(|s| s.borrow_mut().frame(tier, size, msaa_ok, busy));
    if !changed {
        return;
    }
    let (id, settled) = SETTLE.with(|s| {
        let s = s.borrow();
        (s.tier.clone(), s.settled)
    });
    let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) else {
        return;
    };
    if settled {
        let _ = el.set_attribute("data-evani-tier-settled", &id);
        let _ = el.remove_attribute("data-evani-settling");
    } else {
        let _ = el.set_attribute("data-evani-tier-settled", "");
        let _ = el.set_attribute("data-evani-settling", &id);
    }
}
