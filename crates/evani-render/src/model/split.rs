//! The before/after split view of drop-to-convert (S3-14, docs/LIVE_API.md
//! 16). Dev builds only (`hooks`): the live page draws the original and the
//! converted model in the same place and the forward pass clips each to its
//! side of the line, so the frame shows both halves at once.

use super::{ModelId, Models};

/// A split view: `left` draws left of the line at `at` (0..1 across the
/// view), `right` draws right of it. Every other model draws whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub at: f32,
    pub left: ModelId,
    pub right: ModelId,
}

impl Split {
    /// The scissor `(x, width)` of `model` in a view `x0..x0 + w` pixels
    /// wide: `None` when it draws whole, `Some((_, 0))` when its side is empty.
    pub fn columns(&self, model: u32, x0: u32, w: u32) -> Option<(u32, u32)> {
        let at = if self.at.is_finite() { self.at.clamp(0.0, 1.0) } else { 0.5 };
        let cut = ((at * w as f32).round() as u32).min(w);
        if model == self.left.0 {
            Some((x0, cut))
        } else if model == self.right.0 {
            Some((x0 + cut, w - cut))
        } else {
            None
        }
    }
}

impl Models {
    /// A model's bounding sphere `(centre, radius)`, once it is read.
    pub fn sphere(&self, id: ModelId) -> Option<([f32; 3], f32)> {
        self.entries.get(id.0 as usize).filter(|e| e.sphere.1 > 0.0).map(|e| e.sphere)
    }

    /// Native tests stand in for the browser's fetch and GPU upload with
    /// this: the file of a fetched model arrived and is read; a good one
    /// counts as uploaded (`Ready`), a bad one `Failed`.
    pub fn arrived(&mut self, id: ModelId, glb: &[u8]) {
        self.fetch.retain(|f| *f != id);
        self.loaded(id, glb);
        if self.pending.iter().any(|(p, _)| *p == id) {
            self.pending.retain(|(p, _)| *p != id);
            self.set_state(id, super::ModelState::Ready);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_line() {
        let s = Split { at: 0.25, left: ModelId(1), right: ModelId(2) };
        assert_eq!(s.columns(1, 10, 400), Some((10, 100)));
        assert_eq!(s.columns(2, 10, 400), Some((110, 300)));
        assert_eq!(s.columns(0, 10, 400), None);
        let all_left = Split { at: 1.0, ..s };
        assert_eq!(all_left.columns(2, 0, 400), Some((400, 0)));
        let bad = Split { at: f32::NAN, ..s };
        assert_eq!(bad.columns(1, 0, 400), Some((0, 200)));
    }
}
