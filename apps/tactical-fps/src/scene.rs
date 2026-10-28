//! The tactical-fps scene. The drawing itself lives in the tactical-fps kit
//! (`evani_kit_tactical_fps::draw`), so native tools draw exactly what the game
//! does; this module adds the signature moments for `capture.frame`.

/// The path live's bench, compare and capture tools fly.
#[cfg(feature = "live")]
pub use evani_kit_tactical_fps::draw::camera_path;

/// The tactical-fps kit's signature moments (`kits/tactical-fps/target/moments.md`),
/// for `capture.frame { moment }`: fixed points on the orbit. Each one sets
/// the sim clock to its orbit time; the capture then holds it there and the
/// scene's own camera frames it.
#[cfg(feature = "live")]
pub mod moments {
    use evani_core::capture::CaptureMoments;
    use evani_core::ecs::World;
    use evani_core::time::Time;

    /// `(id, label, orbit time in seconds)`; the orbit takes 60 s.
    pub const MOMENTS: [(&str, &str, f64); 5] = [
        (
            "overview",
            "The whole spinner grid from the start of the orbit, sky above",
            0.0,
        ),
        (
            "diagonal",
            "The grid seen along its diagonal, glow pieces blooming",
            7.5,
        ),
        (
            "quarter_turn",
            "A quarter turn round: the grid side-on with the horizon behind",
            15.0,
        ),
        (
            "far_side",
            "Half way round: the far side of the grid, ground to the horizon",
            30.0,
        ),
        (
            "three_quarter",
            "Three quarters round, just before the orbit closes",
            45.0,
        ),
    ];

    fn hold(world: &World, at: f64) -> Result<(), String> {
        let mut t = world.try_res_mut::<Time>().ok_or("no Time resource")?;
        let dt = f64::from(t.fixed_dt).max(1e-6);
        t.tick = (at / dt).round() as u64;
        t.sim_seconds = t.tick as f64 * dt;
        t.alpha = 0.0;
        t.held = None;
        Ok(())
    }

    fn overview(w: &World) -> Result<(), String> {
        hold(w, MOMENTS[0].2)
    }
    fn diagonal(w: &World) -> Result<(), String> {
        hold(w, MOMENTS[1].2)
    }
    fn quarter_turn(w: &World) -> Result<(), String> {
        hold(w, MOMENTS[2].2)
    }
    fn far_side(w: &World) -> Result<(), String> {
        hold(w, MOMENTS[3].2)
    }
    fn three_quarter(w: &World) -> Result<(), String> {
        hold(w, MOMENTS[4].2)
    }

    type Setup = fn(&World) -> Result<(), String>;

    /// Register every moment, in the order the review shows them.
    pub fn register(m: &mut CaptureMoments) -> Result<(), String> {
        let setups: [Setup; 5] =
            [overview, diagonal, quarter_turn, far_side, three_quarter];
        for ((name, label, _), setup) in MOMENTS.iter().zip(setups) {
            m.add(name, label, setup)?;
        }
        Ok(())
    }
}
