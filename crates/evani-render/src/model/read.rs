//! Streamed model files read over frames (model-read-spread, 2026-11-02).
//!
//! A fetched `.glb` used to be decoded and prepared in the frame it arrived:
//! the material viewer's 850 KB bust (`char-head.high.glb`) held that frame
//! for 300 ms on the first visit to High, past the tier switch's 250 ms
//! limit. Now each arrived file waits here and every frame spends at most
//! [`BUDGET_MS`] on reads, in steps: the JSON, one meshopt view, one
//! primitive, the end checks. At least one step runs a frame, so a read
//! always moves on. While a tier's file is read, its handle keeps drawing
//! the previous tier's model ([`super::Models::resolve`]), so a switch shows
//! no gap. Files added from memory ([`super::Models::add_glb`]) and native
//! runs are still read in one go (their callers expect the parts at once).

use evani_gltf::{Error, Gltf, Reading};

use super::steps::Steps;
use super::{ModelId, Models};

/// Milliseconds a frame gives the model reads (one step always runs).
pub const BUDGET_MS: f64 = 4.0;

enum Stage {
    Bytes(Vec<u8>),
    Views(Box<Reading>),
    Parts(Box<Gltf>, Box<Steps>),
}

/// The files waiting to be read, oldest first.
#[derive(Default)]
pub struct Reads {
    queue: Vec<(ModelId, Stage)>,
}

/// Where a step left a read.
enum Next {
    More(Stage),
    Done(Box<super::Prepared>),
}

/// One step of a read: the next stage, or the end (the parts or why not).
fn advance(s: Stage) -> Result<Next, Error> {
    Ok(Next::More(match s {
        Stage::Bytes(b) => Stage::Views(Box::new(Gltf::begin_glb(&b)?)),
        Stage::Views(mut r) => {
            if !r.step()? {
                return Ok(Next::More(Stage::Views(r)));
            }
            let g = r.finish()?;
            let steps = Steps::new(&g);
            Stage::Parts(Box::new(g), Box::new(steps))
        }
        Stage::Parts(g, mut s) => {
            if !s.step(&g)? {
                return Ok(Next::More(Stage::Parts(g, s)));
            }
            return Ok(Next::Done(Box::new(s.finish(&g)?)));
        }
    }))
}

impl Reads {
    /// File `id` arrived (a newer copy replaces one still being read).
    pub fn push(&mut self, id: ModelId, bytes: Vec<u8>) {
        self.queue.retain(|(at, _)| *at != id);
        self.queue.push((id, Stage::Bytes(bytes)));
    }

    /// Files still being read.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Read on until `budget_ms` of `now()` (ms) is spent; at least one
    /// step runs when anything waits. Finished files go to `models` (parts
    /// to upload, or the reason they failed). Returns the steps run.
    pub fn run(&mut self, models: &mut Models, now: &dyn Fn() -> f64, budget_ms: f64) -> u32 {
        let start = now();
        let mut steps = 0;
        while !self.queue.is_empty() && (steps == 0 || now() - start < budget_ms) {
            let (id, stage) = self.queue.remove(0);
            steps += 1;
            match advance(stage) {
                Ok(Next::More(next)) => self.queue.insert(0, (id, next)),
                Ok(Next::Done(p)) => models.accept(id, Ok(*p)),
                Err(e) => models.accept(id, Err(e)),
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelState, prepare};
    use std::cell::Cell;

    fn bust() -> Vec<u8> {
        std::fs::read(format!("{}/../../apps/material-view/assets/models/char-head.high.glb", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn a_streamed_file_is_read_a_step_a_frame_and_matches_the_one_go_read() {
        let b = bust();
        let mut models = Models::default();
        let id = models.load("models/char-head.high.glb");
        let mut reads = Reads::default();
        reads.push(id, b.clone());
        // Every look at the clock is 5 ms later: each frame gets one step.
        let t = Cell::new(0.0);
        let slow = || {
            let v = t.get();
            t.set(v + 5.0);
            v
        };
        let mut frames = 0;
        while !reads.is_empty() {
            assert_eq!(reads.run(&mut models, &slow, BUDGET_MS), 1, "one step a frame once the budget is spent");
            frames += 1;
            assert!(frames < 1000);
            assert!(models.pending.is_empty() || reads.is_empty(), "the parts come out only at the end");
        }
        assert!(frames > 10, "the bust is read over {frames} frames, not spread");
        let (got, p) = models.pending.pop().unwrap();
        assert_eq!(got, id);
        let one = prepare(&Gltf::from_glb(&b).unwrap()).unwrap();
        assert!(p.parts.len() == one.parts.len() && p.parts.iter().zip(&one.parts).all(|(a, b)| a.vertices == b.vertices && a.indices == b.indices));
        assert_eq!(models.triangles(id), one.triangles);
        // A clock that stands still reads it all in one frame.
        reads.push(id, b);
        reads.run(&mut models, &|| 0.0, BUDGET_MS);
        assert!(reads.is_empty());
        // A broken file fails its model (the error log says why), never panics.
        reads.push(id, vec![1, 2, 3]);
        reads.run(&mut models, &|| 0.0, BUDGET_MS);
        assert!(matches!(models.state(id), Some(ModelState::Failed(_))));
    }
}

