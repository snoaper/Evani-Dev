//! `evani-ui`: a box layout for in-canvas menus and HUD panels (S3-13b).
//!
//! A tree of row and column boxes is built each frame with [`Ui::open`],
//! [`Ui::text`] and [`Ui::close`], laid out once with [`Ui::layout`], and
//! read back as a list of rectangles and text runs ([`Ui::commands`]) that
//! the game draws (docs/UI_LAYOUT.md).
//!
//! - Sizes per axis: [`Size::Fit`] (the contents), [`Size::Grow`] (the
//!   contents, plus a weighted share of any free room) or [`Size::Fixed`].
//!   When the contents are wider than the room, fit and grow boxes shrink
//!   toward their smallest size (text: its longest word), then overflow.
//! - Padding, gap between children, and alignment of the children block.
//! - Text wraps through a measure callback, `measure(text, max_width) ->
//!   [width, height]` (`max_width` 0 means one line). With the engine's
//!   MSDF font this is `text::measure_in`, so layout and drawing agree.
//! - A box can scroll on either axis; its children are clipped to it.
//!
//! Fixed arrays, no allocation, no dependency, no randomness or clock: the
//! same tree and the same measure give the same boxes, every time.

/// The most boxes (containers and text runs) one [`Ui`] holds. More are
/// dropped and counted in [`Ui::dropped`].
pub const CAP: usize = 128;

/// "No box": the parent of a root, and the id of a dropped box.
pub const NONE: NodeId = u16::MAX;

/// Far enough to mean "no clip".
const FAR: f32 = 1.0e9;

/// A box in the tree, as returned by [`Ui::open`] and [`Ui::text`].
pub type NodeId = u16;

/// Which way a box lays its children out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dir {
    /// Left to right (the main axis is x).
    #[default]
    Row = 0,
    /// Top to bottom (the main axis is y).
    Column = 1,
}

/// How a box is sized on one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// As big as its contents (plus padding).
    Fit,
    /// Its contents, plus a share of the free room along the parent's main
    /// axis by weight; across it, as big as the parent's inside.
    Grow(f32),
    /// This many pixels.
    Fixed(f32),
}

/// Where a block sits in the room it has (start, middle or end), and the
/// alignment of a line of text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Left or top.
    #[default]
    Start,
    /// Middle.
    Center,
    /// Right or bottom.
    End,
}

impl Align {
    fn k(self) -> f32 {
        match self {
            Align::Start => 0.0,
            Align::Center => 0.5,
            Align::End => 1.0,
        }
    }
}

/// A box's layout and look. Build one from [`Style::ROW`] or
/// [`Style::COLUMN`] with struct update syntax.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// How the children are laid out.
    pub dir: Dir,
    /// Width and height.
    pub size: [Size; 2],
    /// Space inside the box: left, top, right, bottom.
    pub pad: [f32; 4],
    /// Space between two children along the main axis.
    pub gap: f32,
    /// Where the children block sits along x and along y (along the main
    /// axis the block moves as a whole, across it each child moves).
    pub align: [Align; 2],
    /// A background colour (RGBA), if the box draws one.
    pub fill: Option<[f32; 4]>,
    /// Scroll offset in pixels on x and y; `Some` makes the box scroll on
    /// that axis and clip its children to its inside. Clamped to the range.
    pub scroll: [Option<f32>; 2],
}

impl Style {
    /// A row: children left to right; fit size, no padding, no fill.
    pub const ROW: Style = Style {
        dir: Dir::Row,
        size: [Size::Fit, Size::Fit],
        pad: [0.0; 4],
        gap: 0.0,
        align: [Align::Start, Align::Start],
        fill: None,
        scroll: [None, None],
    };
    /// A column: children top to bottom; otherwise as [`Style::ROW`].
    pub const COLUMN: Style = Style { dir: Dir::Column, ..Style::ROW };
}

/// A run of text, one leaf of the tree. The font, size and colour are what
/// the game's renderer needs; layout only reads `size`, `font` and `s`
/// through the measure callback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Text<'a> {
    /// The words. Borrowed, so the tree never copies them.
    pub s: &'a str,
    /// Line height in pixels.
    pub size: f32,
    /// RGBA ink.
    pub color: [f32; 4],
    /// The game's font number (0 for the first it registered).
    pub font: u8,
    /// Alignment of each line inside the run's box.
    pub align: Align,
    /// The run's width: [`Size::Fit`] (one line, shrinking and wrapping when
    /// the room is short), [`Size::Grow`] or [`Size::Fixed`].
    pub width: Size,
}

impl<'a> Text<'a> {
    /// A left-aligned, fit-width run of font 0.
    pub const fn new(s: &'a str, size: f32, color: [f32; 4]) -> Self {
        Text { s, size, color, font: 0, align: Align::Start, width: Size::Fit }
    }
}

/// A rectangle in pixels: top left and size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One thing to draw, in paint order (a box before the boxes inside it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd<'a> {
    /// A filled rectangle (whole pixels, clipped to the scroll boxes above it).
    Rect { rect: Rect, color: [f32; 4], depth: u8 },
    /// A text run. `pos` is the top of the first line, anchored by
    /// `text.align` (left edge, middle or right edge of the run's box);
    /// draw it with `max_width` so it wraps the way layout measured it.
    /// A run that is not wholly inside the scroll boxes above it is left out.
    Text { text: Text<'a>, pos: [f32; 2], max_width: f32, depth: u8 },
}

#[derive(Clone, Copy)]
struct Node<'a> {
    style: Style,
    text: Option<Text<'a>>,
    parent: NodeId,
    /// One past the last node of this box's subtree (the next sibling).
    end: u16,
    depth: u8,
    /// Preferred and smallest size per axis (contents).
    pref: [f32; 2],
    min: [f32; 2],
    /// The result: size, top left, clip `[x0, y0, x1, y1]`, scroll range.
    size: [f32; 2],
    pos: [f32; 2],
    clip: [f32; 4],
    max_scroll: [f32; 2],
}

const EMPTY: Node<'static> = Node {
    style: Style::ROW,
    text: None,
    parent: NONE,
    end: 0,
    depth: 0,
    pref: [0.0; 2],
    min: [0.0; 2],
    size: [0.0; 2],
    pos: [0.0; 2],
    clip: [-FAR, -FAR, FAR, FAR],
    max_scroll: [0.0; 2],
};

/// The box tree of one frame.
pub struct Ui<'a> {
    nodes: [Node<'a>; CAP],
    len: usize,
    /// The box being filled (`NONE` at the top).
    open: NodeId,
    /// Open levels inside a dropped box, so `close` stays paired.
    skip: u16,
    dropped: u16,
    depth: u8,
}

impl Default for Ui<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Ui<'a> {
    /// An empty tree.
    pub fn new() -> Self {
        Ui { nodes: [EMPTY; CAP], len: 0, open: NONE, skip: 0, dropped: 0, depth: 0 }
    }

    /// How many boxes did not fit in [`CAP`] (they, and what is inside a
    /// dropped box, are left out).
    pub fn dropped(&self) -> usize {
        usize::from(self.dropped)
    }

    /// How many boxes the tree holds.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Is the tree empty?
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, style: Style, text: Option<Text<'a>>) -> Option<usize> {
        let i = self.len;
        if self.skip > 0 || i >= CAP {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        let slot = self.nodes.get_mut(i)?;
        *slot = Node { style, text, parent: self.open, end: i as u16 + 1, depth: self.depth, ..EMPTY };
        self.len = i + 1;
        Some(i)
    }

    /// Start a box inside the current one; the boxes added until the
    /// matching [`Ui::close`] go inside it.
    pub fn open(&mut self, style: Style) -> NodeId {
        match self.push(style, None) {
            Some(i) => {
                self.open = i as u16;
                self.depth = self.depth.saturating_add(1);
                i as u16
            }
            None => {
                self.skip = self.skip.saturating_add(1);
                NONE
            }
        }
    }

    /// Finish the box [`Ui::open`] started.
    pub fn close(&mut self) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        let end = self.len as u16;
        let Some(n) = self.nodes.get_mut(usize::from(self.open)) else {
            return;
        };
        n.end = end;
        self.open = n.parent;
        self.depth = self.depth.saturating_sub(1);
    }

    /// Add a run of text inside the current box.
    pub fn text(&mut self, text: Text<'a>) -> NodeId {
        let style = Style { size: [text.width, Size::Fit], ..Style::ROW };
        self.push(style, Some(text)).map_or(NONE, |i| i as u16)
    }

    /// Lay the tree out in a `avail` pixel area whose top left is `origin`.
    /// Each top-level box with a grow size fills the area. `measure(text,
    /// max_width)` returns the `[width, height]` of the text wrapped at
    /// `max_width` (0: not wrapped).
    pub fn layout<M: FnMut(&Text<'_>, f32) -> [f32; 2]>(&mut self, origin: [f32; 2], avail: [f32; 2], mut measure: M) {
        while self.open != NONE || self.skip > 0 {
            self.close();
        }
        for a in 0..2 {
            self.fit(a, &mut measure);
            self.fill(a, avail);
        }
        self.place(origin);
    }

    /// Bottom up: every box's preferred and smallest size on axis `a`.
    fn fit<M: FnMut(&Text<'_>, f32) -> [f32; 2]>(&mut self, a: usize, m: &mut M) {
        for i in (0..self.len).rev() {
            let n = self.nodes[i];
            let pads = n.style.pad[a] + n.style.pad[a + 2];
            let (mut pref, mut min);
            if let Some(t) = &n.text {
                if a == 0 {
                    pref = m(t, 0.0)[0];
                    min = pref;
                    if t.s.contains(' ') {
                        min = t.s.split(' ').fold(0.0, |w, s| f32::max(w, m(&Text { s, ..*t }, 0.0)[0]));
                    }
                } else {
                    pref = m(t, n.size[0])[1];
                    min = pref;
                }
            } else {
                let main = n.style.dir as usize == a;
                let (mut sp, mut sm, mut count) = (0.0f32, 0.0f32, 0u16);
                let mut j = i + 1;
                while j < usize::from(n.end) {
                    let c = &self.nodes[j];
                    if main {
                        sp += c.pref[a];
                        sm += c.min[a];
                    } else {
                        sp = sp.max(c.pref[a]);
                        sm = sm.max(c.min[a]);
                    }
                    count += 1;
                    j = usize::from(c.end);
                }
                let gaps = if main { n.style.gap * f32::from(count.saturating_sub(1)) } else { 0.0 };
                pref = sp + gaps + pads;
                min = sm + gaps + pads;
                if n.style.scroll[a].is_some() {
                    min = pads;
                }
            }
            if let Size::Fixed(px) = n.style.size[a] {
                (pref, min) = (px, px);
            }
            self.nodes[i].pref[a] = pref;
            self.nodes[i].min[a] = min;
        }
    }

    /// Top down: every box's size on axis `a`, given by its parent.
    fn fill(&mut self, a: usize, avail: [f32; 2]) {
        for i in 0..self.len {
            let n = self.nodes[i];
            if n.parent == NONE {
                self.nodes[i].size[a] = match n.style.size[a] {
                    Size::Fixed(px) => px,
                    Size::Grow(_) => avail[a],
                    Size::Fit => n.pref[a],
                };
            }
            let inner = (self.nodes[i].size[a] - n.style.pad[a] - n.style.pad[a + 2]).max(0.0);
            let main = n.style.dir as usize == a;
            // Along the main axis: the contents, the weights, the room to shrink.
            let (mut base, mut weights, mut room, mut count) = (0.0f32, 0.0f32, 0.0f32, 0u16);
            let mut j = i + 1;
            while j < usize::from(n.end) {
                let c = &self.nodes[j];
                let b = if let Size::Fixed(px) = c.style.size[a] { px } else { c.pref[a] };
                base += b;
                match c.style.size[a] {
                    Size::Grow(w) => {
                        weights += w.max(0.0);
                        room += (b - c.min[a]).max(0.0);
                    }
                    Size::Fit => room += (b - c.min[a]).max(0.0),
                    Size::Fixed(_) => {}
                }
                count += 1;
                j = usize::from(c.end);
            }
            let free = inner - base - n.style.gap * f32::from(count.saturating_sub(1));
            let shrink = (-free).min(room).max(0.0);
            let mut j = i + 1;
            while j < usize::from(n.end) {
                let c = &self.nodes[j];
                let (pref, min, style) = (c.pref[a], c.min[a], c.style.size[a]);
                let size = if !main {
                    match style {
                        Size::Fixed(px) => px,
                        Size::Grow(_) => inner,
                        Size::Fit => pref.min(inner).max(min),
                    }
                } else {
                    match style {
                        Size::Fixed(px) => px,
                        Size::Grow(w) if free > 0.0 && weights > 0.0 => pref + free * w.max(0.0) / weights,
                        _ if shrink > 0.0 => pref - shrink * (pref - min).max(0.0) / room,
                        _ => pref,
                    }
                };
                let end = usize::from(c.end);
                self.nodes[j].size[a] = size;
                j = end;
            }
        }
    }

    /// Top down: every box's position, clip and scroll range.
    fn place(&mut self, origin: [f32; 2]) {
        for i in 0..self.len {
            if self.nodes[i].parent == NONE {
                self.nodes[i].pos = origin;
                self.nodes[i].clip = [-FAR, -FAR, FAR, FAR];
            }
            let n = self.nodes[i];
            let (m, c) = (n.style.dir as usize, 1 - n.style.dir as usize);
            let inner = [n.size[0] - n.style.pad[0] - n.style.pad[2], n.size[1] - n.style.pad[1] - n.style.pad[3]];
            let at = [n.pos[0] + n.style.pad[0], n.pos[1] + n.style.pad[1]];
            let mut clip = n.clip;
            for a in 0..2 {
                if n.style.scroll[a].is_some() {
                    clip[a] = clip[a].max(at[a]);
                    clip[a + 2] = clip[a + 2].min(at[a] + inner[a]);
                }
            }
            let (mut total, mut widest, mut count) = (0.0f32, 0.0f32, 0u16);
            let mut j = i + 1;
            while j < usize::from(n.end) {
                let ch = &self.nodes[j];
                total += ch.size[m];
                widest = widest.max(ch.size[c]);
                count += 1;
                j = usize::from(ch.end);
            }
            total += n.style.gap * f32::from(count.saturating_sub(1));
            let mut range = [0.0f32; 2];
            range[m] = (total - inner[m]).max(0.0);
            range[c] = (widest - inner[c]).max(0.0);
            self.nodes[i].max_scroll = range;
            let off = |a: usize| n.style.scroll[a].map_or(0.0, |v| v.max(0.0).min(range[a]));
            let mut cursor = at[m] + (inner[m] - total).max(0.0) * n.style.align[m].k() - off(m);
            let mut j = i + 1;
            while j < usize::from(n.end) {
                let ch = &mut self.nodes[j];
                ch.pos[m] = cursor;
                ch.pos[c] = at[c] + (inner[c] - ch.size[c]).max(0.0) * n.style.align[c].k() - off(c);
                ch.clip = clip;
                cursor += ch.size[m] + n.style.gap;
                j = usize::from(ch.end);
            }
        }
    }

    /// The laid-out rectangle of a box (not clipped, not rounded), `None`
    /// for a dropped or unknown id.
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        let n = self.nodes.get(usize::from(id)).filter(|_| usize::from(id) < self.len)?;
        Some(Rect { x: n.pos[0], y: n.pos[1], w: n.size[0], h: n.size[1] })
    }

    /// How far a scroll box can scroll on x and y (0 when its contents fit).
    pub fn max_scroll(&self, id: NodeId) -> [f32; 2] {
        self.nodes.get(usize::from(id)).filter(|_| usize::from(id) < self.len).map_or([0.0; 2], |n| n.max_scroll)
    }

    /// What to draw, in paint order. Rectangles are on whole pixels and cut
    /// to the scroll boxes above them; text runs outside them are left out.
    pub fn commands(&self) -> impl Iterator<Item = Cmd<'a>> + '_ {
        self.nodes.iter().take(self.len).filter_map(command)
    }
}

fn command<'a>(n: &Node<'a>) -> Option<Cmd<'a>> {
    let (x0, y0) = (n.pos[0], n.pos[1]);
    let (x1, y1) = (x0 + n.size[0], y0 + n.size[1]);
    if let Some(text) = n.text {
        let inside = x0 >= n.clip[0] - 0.5 && y0 >= n.clip[1] - 0.5 && x1 <= n.clip[2] + 0.5 && y1 <= n.clip[3] + 0.5;
        let anchor = x0 + n.size[0] * text.align.k();
        return (inside && n.size[0] > 0.0).then_some(Cmd::Text { text, pos: [anchor, y0], max_width: n.size[0], depth: n.depth });
    }
    let color = n.style.fill?;
    let (cx0, cy0) = (x0.max(n.clip[0]).round(), y0.max(n.clip[1]).round());
    let (cx1, cy1) = (x1.min(n.clip[2]).round(), y1.min(n.clip[3]).round());
    (cx1 > cx0 && cy1 > cy0).then_some(Cmd::Rect { rect: Rect { x: cx0, y: cy0, w: cx1 - cx0, h: cy1 - cy0 }, color, depth: n.depth })
}

#[cfg(test)]
mod tests;
