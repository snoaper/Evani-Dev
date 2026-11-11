//! A tier switch must never rebuild a shader. A pipeline built at runtime is
//! compiled by the browser's GPU process, and while it compiles the page
//! draws nothing: the material viewer froze for 30 to 53 s on every tier it
//! had not shown before (2026-10-25), because the character materials wrote
//! the tier's quality into a shader slot body. Per-tier values belong in
//! uniforms or data; [`tier_switch_test`] is the headless guard any game can
//! run, and `cargo xtask review` measures the same thing in a real browser
//! (the tier-switch freeze check).

use crate::App;
use crate::slots::{ShaderSlots, SlotRebuilds};
use crate::tiers::Tiers;

/// Every slot's current body, by name.
fn bodies(app: &App) -> Vec<(String, String)> {
    app.world.try_res::<ShaderSlots>().map_or_else(Vec::new, |s| {
        s.iter().map(|(_, d)| (d.signature.name.to_string(), d.body.clone())).collect()
    })
}

/// The slot names of the rebuilds queued since the last call (taken, so the
/// next step starts clean).
fn queued(app: &App) -> Vec<String> {
    let Some(mut r) = app.world.try_res_mut::<SlotRebuilds>() else {
        return Vec::new();
    };
    let reqs = r.take_pending();
    drop(r);
    let slots = app.world.try_res::<ShaderSlots>();
    reqs.iter()
        .map(|q| slots.as_ref().and_then(|s| s.get(q.slot)).map_or_else(|| format!("#{}", q.slot.0), |d| d.signature.name.to_string()))
        .collect()
}

/// Run `app` (built, not started yet or started) for `frames` frames, then
/// switch to every selectable tier and back, `frames` frames each, and fail
/// if a switch queued a shader slot rebuild or changed a slot body. The
/// start-up's own rebuilds don't count. The tier is put back afterwards.
pub fn tier_switch_test(app: &mut App, frames: u32) -> Result<(), String> {
    const DT_MS: f64 = 1000.0 / 60.0;
    let (ids, first) = match app.world.try_res::<Tiers>() {
        Some(t) => (t.presets().iter().filter(|p| !p.stereo).map(|p| p.id.clone()).collect::<Vec<_>>(), t.active().id.clone()),
        None => return Err("the app has no tiers".into()),
    };
    app.run_headless(frames.max(1), DT_MS);
    let _ = queued(app);
    let before = bodies(app);
    let mut order: Vec<String> = ids.iter().filter(|id| **id != first).cloned().collect();
    order.push(first.clone());
    let mut failures = Vec::new();
    for id in &order {
        if app.world.res_mut::<Tiers>().try_set_active(id).is_err() {
            continue;
        }
        app.run_headless(frames.max(1), DT_MS);
        let q = queued(app);
        if !q.is_empty() {
            failures.push(format!("switching to {id} queued a shader rebuild of {}", q.join(", ")));
        }
        let now = bodies(app);
        let changed: Vec<&str> = now.iter().zip(&before).filter(|(a, b)| a != b).map(|(a, _)| a.0.as_str()).collect();
        if !changed.is_empty() {
            failures.push(format!("switching to {id} changed the body of slot {}", changed.join(", ")));
        }
    }
    let _ = app.world.res_mut::<Tiers>().try_set_active(&first);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}; a tier change must not rebuild a shader (the page freezes while the browser compiles it): put per-tier values in a uniform or data, not in a slot body",
            failures.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Stage, SystemDesc};
    use crate::plugin::{KitPlugin, PluginContext, PluginError};
    use crate::slots::SlotSignature;

    const SIG: SlotSignature = SlotSignature { name: "tier_probe", params: &[], ret: "f32", description: "test", default_body: "    return 0.0;" };

    /// A kit that writes the tier into a slot body (the bug) or not.
    struct Probe {
        bad: bool,
    }

    impl KitPlugin for Probe {
        fn name(&self) -> &'static str {
            "probe"
        }
        fn build(&self, ctx: &mut PluginContext) -> Result<(), PluginError> {
            ctx.world.res_mut::<ShaderSlots>().declare(SIG);
            let bad = self.bad;
            let mut last = usize::MAX;
            ctx.system(SystemDesc::new("probe.tier", Stage::Update, move |world| {
                let i = world.res::<Tiers>().active_index();
                if !bad || i == last {
                    return;
                }
                last = i;
                let slots = world.res::<ShaderSlots>();
                let _ = world.try_res_mut::<SlotRebuilds>().map(|mut r| r.request_detached(&slots, "tier_probe", &format!("    return {i}.0;"), 0.0));
            }));
            Ok(())
        }
    }

    #[test]
    fn a_tier_written_into_a_slot_fails_and_data_passes() {
        let mut bad = App::new("t").with_kit(Probe { bad: true });
        bad.start();
        let e = tier_switch_test(&mut bad, 2).unwrap_err();
        assert!(e.contains("queued a shader rebuild of tier_probe"), "{e}");
        let mut good = App::new("t").with_kit(Probe { bad: false });
        good.start();
        assert_eq!(tier_switch_test(&mut good, 2), Ok(()));
    }
}
