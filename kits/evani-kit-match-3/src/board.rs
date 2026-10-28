//! The board: cells, pieces, matching, special pieces, chain reactions,
//! gravity, refills, move search, shuffles and generation with no starting
//! matches. Pure logic with no engine types, so every rule is unit-tested
//! natively; randomness comes from the caller's seeded [`Pcg32`].
//!
//! Cells are indexed `y * w + x` (row 0 at the top). Masked-out cells are
//! holes: never hold a piece; pieces fall past them.

use evani_core::rng::Pcg32;

/// Most colours a level can use.
pub const MAX_COLORS: u8 = 6;
/// The colour of a Prism (it has none).
pub const NO_COLOR: u8 = u8::MAX;

/// What a piece is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Normal,
    /// Clears its row when fired.
    StreakH,
    /// Clears its column when fired.
    StreakV,
    /// Clears the square around it (radius from the tunables).
    Bomb,
    /// Clears one colour.
    Prism,
}

impl Kind {
    pub fn is_special(self) -> bool {
        self != Kind::Normal
    }

    pub fn is_streak(self) -> bool {
        matches!(self, Kind::StreakH | Kind::StreakV)
    }
}

/// One piece: also the ECS component on the piece's entity (its visual
/// state is the entity's [`crate::ecs::Motion`]; the board logic never
/// reads that).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// Unique per board (stable while the piece moves; the piece's entity
    /// follows it by this id, see [`crate::ecs::Grid::sync`]).
    pub id: u32,
    /// `0..colors`, or [`NO_COLOR`] for a Prism.
    pub color: u8,
    pub kind: Kind,
}

impl Piece {
    pub fn matches(&self, other: &Piece) -> bool {
        self.color != NO_COLOR && self.color == other.color
    }
}

/// A level's board shape: playable cells and frosting layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub w: usize,
    pub h: usize,
    pub mask: Vec<bool>,
    pub frost: Vec<u8>,
}

impl Shape {
    /// Rows of `#` (cell), `.` (hole), `f` (one frosting layer), `F` (two).
    pub fn parse(rows: &[String]) -> Result<Self, String> {
        let h = rows.len();
        let w = rows.first().map_or(0, |r| r.chars().count());
        if !(3..=12).contains(&w) || !(3..=12).contains(&h) {
            return Err(format!("board must be 3 to 12 cells each way, got {w}x{h}"));
        }
        let mut mask = Vec::with_capacity(w * h);
        let mut frost = Vec::with_capacity(w * h);
        for (y, r) in rows.iter().enumerate() {
            if r.chars().count() != w {
                return Err(format!("row {y} has {} cells, expected {w}", r.chars().count()));
            }
            for c in r.chars() {
                let (m, f) = match c {
                    '#' => (true, 0),
                    '.' => (false, 0),
                    'f' => (true, 1),
                    'F' => (true, 2),
                    other => return Err(format!("row {y}: unknown cell '{other}' (use # . f F)")),
                };
                mask.push(m);
                frost.push(f);
            }
        }
        if mask.iter().filter(|m| **m).count() < 9 {
            return Err("fewer than 9 playable cells".into());
        }
        Ok(Self { w, h, mask, frost })
    }

    /// A full `w` x `h` board.
    pub fn full(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            mask: vec![true; w * h],
            frost: vec![0; w * h],
        }
    }
}

/// Tunables the board rules read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// Pop Bomb radius in cells (1 = 3x3).
    pub bomb_radius: i32,
    /// Generation attempts before the fixed fallback pattern.
    pub gen_attempts: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            bomb_radius: 1,
            gen_attempts: 50,
        }
    }
}

/// One straight run of 3+ same-coloured pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub cells: Vec<usize>,
    pub horizontal: bool,
    pub color: u8,
}

/// Runs of one colour that share cells: one match.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Every cell, sorted, no repeats.
    pub cells: Vec<usize>,
    pub color: u8,
    pub runs: Vec<Run>,
}

impl Group {
    /// The special this match makes, if any: 5+ in a line → Prism, crossed
    /// lines → Bomb, 4 in a line → Streak along that line.
    pub fn special(&self) -> Option<Kind> {
        let longest = self.runs.iter().max_by_key(|r| r.cells.len())?;
        let crossed = self.runs.iter().any(|r| r.horizontal) && self.runs.iter().any(|r| !r.horizontal);
        if longest.cells.len() >= 5 {
            Some(Kind::Prism)
        } else if crossed {
            Some(Kind::Bomb)
        } else if longest.cells.len() == 4 {
            Some(if longest.horizontal { Kind::StreakH } else { Kind::StreakV })
        } else {
            None
        }
    }

    /// Where the special appears: a preferred cell (the swapped pair) if
    /// the match has one, else where lines cross, else the middle of the
    /// longest line.
    pub fn special_cell(&self, prefer: &[usize]) -> usize {
        if let Some(&c) = prefer.iter().find(|c| self.cells.contains(c)) {
            return c;
        }
        let h: Vec<&Run> = self.runs.iter().filter(|r| r.horizontal).collect();
        let v: Vec<&Run> = self.runs.iter().filter(|r| !r.horizontal).collect();
        for a in &h {
            for b in &v {
                if let Some(&c) = a.cells.iter().find(|c| b.cells.contains(c)) {
                    return c;
                }
            }
        }
        let longest = self.runs.iter().max_by_key(|r| r.cells.len());
        longest.map_or(self.cells[0], |r| r.cells[r.cells.len() / 2])
    }
}

/// One piece removed by a resolution step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cleared {
    pub cell: usize,
    pub piece: Piece,
    /// Chain delay in cell steps (0 = at once).
    pub delay: f32,
    /// Removed by a blast rather than a line match.
    pub blasted: bool,
}

/// A special that went off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fired {
    pub cell: usize,
    pub kind: Kind,
    /// Its colour (the target colour for a Prism).
    pub color: u8,
    pub delay: f32,
    /// A combo of two specials (bigger area).
    pub combo: Option<Kind>,
}

/// A special piece made by a match.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Created {
    pub cell: usize,
    pub piece: Piece,
}

/// Everything one resolution step did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolution {
    pub groups: Vec<Group>,
    pub cleared: Vec<Cleared>,
    pub fired: Vec<Fired>,
    pub created: Vec<Created>,
    /// Frosting layers melted.
    pub frost_melted: u32,
}

impl Resolution {
    pub fn is_empty(&self) -> bool {
        self.cleared.is_empty() && self.created.is_empty()
    }

    /// Longest chain delay (cell steps).
    pub fn max_delay(&self) -> f32 {
        self.cleared.iter().map(|c| c.delay).fold(0.0, f32::max)
    }
}

/// A piece that moved down (or arrived) during gravity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fall {
    pub cell: usize,
    /// Rows it drops (visual distance).
    pub rows: f32,
    /// Newly spawned from above the board.
    pub new: bool,
}

/// A possible swap and how good it looks right away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub a: usize,
    pub b: usize,
    /// Specials made x 100 + combo bonus + pieces matched.
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Board {
    pub w: usize,
    pub h: usize,
    pub mask: Vec<bool>,
    pub cells: Vec<Option<Piece>>,
    pub frost: Vec<u8>,
    pub colors: u8,
    next_id: u32,
}

impl Board {
    /// An empty board of this shape (fill it with [`Board::generate`]).
    pub fn empty(shape: &Shape, colors: u8) -> Self {
        Self {
            w: shape.w,
            h: shape.h,
            mask: shape.mask.clone(),
            cells: vec![None; shape.w * shape.h],
            frost: shape.frost.clone(),
            colors: colors.clamp(3, MAX_COLORS),
            next_id: 1,
        }
    }

    /// Build a board from rows of digits (`0`-`5` colours, `.` hole, `P`
    /// prism, `h`/`v` horizontal/vertical streak of colour 0, `b` bomb of
    /// colour 0). For tests and the fallback pattern.
    pub fn from_rows(rows: &[&str], colors: u8) -> Self {
        let h = rows.len();
        let w = rows.first().map_or(0, |r| r.len());
        let mut b = Self::empty(&Shape::full(w, h), colors);
        for (y, r) in rows.iter().enumerate() {
            for (x, ch) in r.chars().enumerate() {
                let i = y * w + x;
                let (color, kind) = match ch {
                    '.' => {
                        b.mask[i] = false;
                        continue;
                    }
                    'P' => (NO_COLOR, Kind::Prism),
                    'h' => (0, Kind::StreakH),
                    'v' => (0, Kind::StreakV),
                    'b' => (0, Kind::Bomb),
                    d => (d.to_digit(10).unwrap_or(0) as u8, Kind::Normal),
                };
                b.cells[i] = Some(b.make(color, kind));
            }
        }
        b
    }

    pub fn make(&mut self, color: u8, kind: Kind) -> Piece {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        Piece {
            id,
            color: if kind == Kind::Prism { NO_COLOR } else { color },
            kind,
        }
    }

    pub fn len(&self) -> usize {
        self.w * self.h
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn xy(&self, i: usize) -> (i32, i32) {
        ((i % self.w) as i32, (i / self.w) as i32)
    }

    pub fn at(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        let i = y as usize * self.w + x as usize;
        self.mask[i].then_some(i)
    }

    pub fn piece(&self, i: usize) -> Option<&Piece> {
        self.cells.get(i).and_then(|c| c.as_ref())
    }

    pub fn adjacent(&self, a: usize, b: usize) -> bool {
        let (ax, ay) = self.xy(a);
        let (bx, by) = self.xy(b);
        (ax - bx).abs() + (ay - by).abs() == 1
    }

    /// Frosting layers left on the board.
    pub fn frost_left(&self) -> u32 {
        self.frost.iter().map(|&f| u32::from(f)).sum()
    }

    fn same(&self, a: Option<usize>, color: u8) -> bool {
        a.and_then(|i| self.piece(i)).is_some_and(|p| p.color == color && color != NO_COLOR)
    }

    /// Every straight run of 3+ (horizontal runs first, scan order).
    pub fn runs(&self) -> Vec<Run> {
        let mut out = Vec::new();
        for horizontal in [true, false] {
            let (outer, inner) = if horizontal { (self.h, self.w) } else { (self.w, self.h) };
            for o in 0..outer as i32 {
                let mut k = 0i32;
                while k < inner as i32 {
                    let cell = |k: i32| if horizontal { self.at(k, o) } else { self.at(o, k) };
                    let Some(start) = cell(k) else {
                        k += 1;
                        continue;
                    };
                    let Some(color) = self.piece(start).map(|p| p.color).filter(|&c| c != NO_COLOR) else {
                        k += 1;
                        continue;
                    };
                    let mut end = k + 1;
                    while end < inner as i32 && self.same(cell(end), color) {
                        end += 1;
                    }
                    if end - k >= 3 {
                        let cells = (k..end).filter_map(cell).collect();
                        out.push(Run { cells, horizontal, color });
                    }
                    k = end;
                }
            }
        }
        out
    }

    /// Runs merged into match groups (runs of one colour sharing a cell).
    pub fn groups(&self) -> Vec<Group> {
        let runs = self.runs();
        let mut owner: Vec<usize> = (0..runs.len()).collect();
        fn root(o: &mut [usize], mut i: usize) -> usize {
            while o[i] != i {
                o[i] = o[o[i]];
                i = o[i];
            }
            i
        }
        for i in 0..runs.len() {
            for j in i + 1..runs.len() {
                if runs[i].color == runs[j].color && runs[i].cells.iter().any(|c| runs[j].cells.contains(c)) {
                    let (a, b) = (root(&mut owner, i), root(&mut owner, j));
                    owner[b.max(a)] = a.min(b);
                }
            }
        }
        let mut groups: Vec<(usize, Group)> = Vec::new();
        for (i, r) in runs.into_iter().enumerate() {
            let g = root(&mut owner, i);
            match groups.iter_mut().find(|(k, _)| *k == g) {
                Some((_, grp)) => {
                    grp.cells.extend(&r.cells);
                    grp.runs.push(r);
                }
                None => groups.push((
                    g,
                    Group {
                        cells: r.cells.clone(),
                        color: r.color,
                        runs: vec![r],
                    },
                )),
            }
        }
        groups
            .into_iter()
            .map(|(_, mut g)| {
                g.cells.sort_unstable();
                g.cells.dedup();
                g
            })
            .collect()
    }

    /// Does the piece at `i` sit in a line of three?
    pub fn match_at(&self, i: usize) -> bool {
        let Some(color) = self.piece(i).map(|p| p.color).filter(|&c| c != NO_COLOR) else {
            return false;
        };
        let (x, y) = self.xy(i);
        let count = |dx: i32, dy: i32| {
            let mut n = 0;
            let (mut cx, mut cy) = (x + dx, y + dy);
            while self.same(self.at(cx, cy), color) {
                n += 1;
                cx += dx;
                cy += dy;
            }
            n
        };
        count(-1, 0) + count(1, 0) >= 2 || count(0, -1) + count(0, 1) >= 2
    }

    pub fn swap(&mut self, a: usize, b: usize) {
        self.cells.swap(a, b);
    }

    /// Is swapping `a` and `b` a legal move?
    pub fn valid_swap(&self, a: usize, b: usize) -> bool {
        if a == b || !self.adjacent(a, b) {
            return false;
        }
        let (Some(pa), Some(pb)) = (self.piece(a), self.piece(b)) else {
            return false;
        };
        if pa.kind == Kind::Prism || pb.kind == Kind::Prism || (pa.kind.is_special() && pb.kind.is_special()) {
            return true;
        }
        let mut t = self.clone();
        t.swap(a, b);
        t.match_at(a) || t.match_at(b)
    }

    /// Every legal move with its immediate value, in scan order.
    pub fn moves(&self) -> Vec<Move> {
        let mut out = Vec::new();
        for i in 0..self.len() {
            let (x, y) = self.xy(i);
            for (dx, dy) in [(1, 0), (0, 1)] {
                let Some(j) = self.at(x + dx, y + dy) else {
                    continue;
                };
                if self.valid_swap(i, j) {
                    out.push(Move {
                        a: i,
                        b: j,
                        value: self.move_value(i, j),
                    });
                }
            }
        }
        out
    }

    pub fn has_move(&self) -> bool {
        (0..self.len()).any(|i| {
            let (x, y) = self.xy(i);
            [(1, 0), (0, 1)]
                .iter()
                .any(|&(dx, dy)| self.at(x + dx, y + dy).is_some_and(|j| self.valid_swap(i, j)))
        })
    }

    fn move_value(&self, a: usize, b: usize) -> i32 {
        let (Some(pa), Some(pb)) = (self.piece(a), self.piece(b)) else {
            return 0;
        };
        let combo = |k: Kind| match k {
            Kind::Normal => 0,
            Kind::StreakH | Kind::StreakV => 150,
            Kind::Bomb => 200,
            Kind::Prism => 400,
        };
        if pa.kind.is_special() && pb.kind.is_special() {
            return 300 + combo(pa.kind) + combo(pb.kind);
        }
        if pa.kind == Kind::Prism || pb.kind == Kind::Prism {
            return 250;
        }
        let mut t = self.clone();
        t.swap(a, b);
        t.groups()
            .iter()
            .filter(|g| g.cells.contains(&a) || g.cells.contains(&b))
            .map(|g| {
                let fires: i32 = g
                    .cells
                    .iter()
                    .filter_map(|&c| t.piece(c))
                    .map(|p| combo(p.kind) / 3)
                    .sum();
                g.special().map_or(0, |k| 60 + combo(k) / 2) + g.cells.len() as i32 + fires
            })
            .sum()
    }

    /// The best move (highest value, first in scan order on ties).
    pub fn best_move(&self) -> Option<Move> {
        let mut best: Option<Move> = None;
        for m in self.moves() {
            if best.is_none_or(|b| m.value > b.value) {
                best = Some(m);
            }
        }
        best
    }

    /// The weakest move (lowest value, first on ties): for the losing test bot.
    pub fn worst_move(&self) -> Option<Move> {
        let mut worst: Option<Move> = None;
        for m in self.moves() {
            if worst.is_none_or(|b| m.value < b.value) {
                worst = Some(m);
            }
        }
        worst
    }

    /// The most common colour on the board (lowest index on ties).
    pub fn common_color(&self) -> u8 {
        let mut n = [0u32; MAX_COLORS as usize];
        for p in self.cells.iter().flatten() {
            if let Some(c) = n.get_mut(p.color as usize) {
                *c += 1;
            }
        }
        let mut best = 0usize;
        for (i, &c) in n.iter().enumerate() {
            if c > n[best] {
                best = i;
            }
        }
        best as u8
    }

    /// Fill every playable cell: no line of three anywhere, and at least
    /// one legal move. Tries `rules.gen_attempts` times, then falls back
    /// to a repeating pattern that always has moves.
    pub fn generate(&mut self, rng: &mut Pcg32, rules: &Rules) {
        for _ in 0..rules.gen_attempts.max(1) {
            self.fill_no_matches(rng);
            if self.has_move() {
                return;
            }
        }
        self.fallback_fill();
    }

    fn fill_no_matches(&mut self, rng: &mut Pcg32) {
        for i in 0..self.len() {
            self.cells[i] = None;
        }
        for i in 0..self.len() {
            if !self.mask[i] {
                continue;
            }
            let (x, y) = self.xy(i);
            let mut c = rng.below(u32::from(self.colors)) as u8;
            for _ in 0..self.colors {
                let left = self.same(self.at(x - 1, y), c) && self.same(self.at(x - 2, y), c);
                let up = self.same(self.at(x, y - 1), c) && self.same(self.at(x, y - 2), c);
                if !left && !up {
                    break;
                }
                c = (c + 1) % self.colors;
            }
            self.cells[i] = Some(self.make(c, Kind::Normal));
        }
    }

    /// Pairs of colours, shifted by one each row (`0 0 1 1 2 2` over
    /// `1 1 2 2 0 0`): no line of three (pairs across, neighbours differ
    /// down), and moving the piece above a pair's right neighbour down
    /// completes a line, so any full board of 3x2 or more has a move.
    fn fallback_fill(&mut self) {
        for i in 0..self.len() {
            self.cells[i] = None;
            if self.mask[i] {
                let (x, y) = self.xy(i);
                let c = ((x / 2 + y) as u32 % u32::from(self.colors)) as u8;
                self.cells[i] = Some(self.make(c, Kind::Normal));
            }
        }
    }

    /// Re-arrange the pieces (specials included) until there is no match
    /// and at least one move. Returns where each piece came from
    /// (`(to, from)` cell pairs) for the fly animation.
    pub fn shuffle(&mut self, rng: &mut Pcg32) -> Vec<(usize, usize)> {
        let playable: Vec<usize> = (0..self.len()).filter(|&i| self.mask[i] && self.cells[i].is_some()).collect();
        let original: Vec<Option<Piece>> = self.cells.clone();
        let mut order = playable.clone();
        for attempt in 0..200 {
            rng.shuffle(&mut order);
            for (k, &to) in playable.iter().enumerate() {
                self.cells[to] = original[order[k]];
            }
            if self.groups().is_empty() && self.has_move() {
                return playable.iter().zip(&order).map(|(&to, &from)| (to, from)).collect();
            }
            if attempt == 199 {
                break;
            }
        }
        // No arrangement worked (tiny boards): recolour the normal pieces.
        let ids: Vec<(usize, u32, Kind)> = playable
            .iter()
            .filter_map(|&i| self.cells[i].map(|p| (i, p.id, p.kind)))
            .collect();
        self.fallback_fill();
        for (i, id, kind) in ids {
            if let Some(p) = self.cells[i].as_mut() {
                p.id = id;
                if kind.is_special() {
                    p.kind = kind;
                    if kind == Kind::Prism {
                        p.color = NO_COLOR;
                    }
                }
            }
        }
        playable.iter().map(|&i| (i, i)).collect()
    }

    /// Cells a fired special clears, with chain delays (cell steps) from
    /// `delay`. `color` is the Prism's target colour.
    fn blast_area(&self, cell: usize, kind: Kind, color: u8, combo: Option<Kind>, rules: &Rules) -> Vec<(usize, f32)> {
        let (cx, cy) = self.xy(cell);
        let mut out = Vec::new();
        let mut push = |x: i32, y: i32| {
            if let Some(i) = self.at(x, y) {
                let d = ((x - cx).abs()).max((y - cy).abs()) as f32;
                out.push((i, d));
            }
        };
        let w = self.w as i32;
        let h = self.h as i32;
        let r = rules.bomb_radius.max(0);
        let row = |push: &mut dyn FnMut(i32, i32), y: i32| (0..w).for_each(|x| push(x, y));
        let col = |push: &mut dyn FnMut(i32, i32), x: i32| (0..h).for_each(|y| push(x, y));
        match (kind, combo) {
            (Kind::Prism, Some(Kind::Prism)) => {
                for y in 0..h {
                    for x in 0..w {
                        push(x, y);
                    }
                }
            }
            (Kind::Prism, _) => {
                for i in 0..self.len() {
                    if self.piece(i).is_some_and(|p| p.color == color && color != NO_COLOR) {
                        let (x, y) = self.xy(i);
                        push(x, y);
                    }
                }
            }
            (k, Some(c)) if k.is_streak() && c.is_streak() => {
                row(&mut push, cy);
                col(&mut push, cx);
            }
            (k, Some(c)) if (k.is_streak() && c == Kind::Bomb) || (k == Kind::Bomb && c.is_streak()) => {
                for d in -1..=1 {
                    row(&mut push, cy + d);
                    col(&mut push, cx + d);
                }
            }
            (Kind::Bomb, Some(Kind::Bomb)) => {
                for y in cy - r - 1..=cy + r + 1 {
                    for x in cx - r - 1..=cx + r + 1 {
                        push(x, y);
                    }
                }
            }
            (Kind::StreakH, _) => row(&mut push, cy),
            (Kind::StreakV, _) => col(&mut push, cx),
            (Kind::Bomb, _) => {
                for y in cy - r..=cy + r {
                    for x in cx - r..=cx + r {
                        push(x, y);
                    }
                }
            }
            (Kind::Normal, _) => {}
        }
        out
    }

    /// Clear the queued cells, firing every special among them (chain
    /// reactions), then melt frosting and place `create`d specials.
    fn clear_queue(
        &mut self,
        mut queue: Vec<(usize, f32, bool)>,
        mut fired: Vec<Fired>,
        create: Vec<Created>,
        consumed: &[usize],
        rules: &Rules,
    ) -> Resolution {
        let n = self.len();
        let mut when: Vec<Option<(f32, bool)>> = vec![None; n];
        let mut head = 0;
        // Breadth-first by insertion; a cell keeps its earliest delay.
        while head < queue.len() {
            let (cell, delay, blasted) = queue[head];
            head += 1;
            if cell >= n || !self.mask[cell] {
                continue;
            }
            match when[cell] {
                Some((d, _)) if d <= delay => continue,
                _ => {}
            }
            let first = when[cell].is_none();
            when[cell] = Some((delay, blasted));
            if !first || consumed.contains(&cell) {
                continue;
            }
            let Some(p) = self.piece(cell).copied() else {
                continue;
            };
            if p.kind.is_special() {
                let color = if p.kind == Kind::Prism { self.common_color() } else { p.color };
                fired.push(Fired {
                    cell,
                    kind: p.kind,
                    color,
                    delay,
                    combo: None,
                });
                for (c, d) in self.blast_area(cell, p.kind, color, None, rules) {
                    queue.push((c, delay + d.max(1.0), true));
                }
            }
        }
        let mut res = Resolution {
            fired,
            ..Resolution::default()
        };
        for (cell, w) in when.iter().enumerate() {
            let Some((delay, blasted)) = *w else {
                continue;
            };
            if let Some(piece) = self.cells[cell].take() {
                res.cleared.push(Cleared {
                    cell,
                    piece,
                    delay,
                    blasted,
                });
                if self.frost[cell] > 0 {
                    self.frost[cell] -= 1;
                    res.frost_melted += 1;
                }
            }
        }
        for c in create {
            if self.mask[c.cell] {
                self.cells[c.cell] = Some(c.piece);
                res.created.push(c);
            }
        }
        res
    }

    /// Resolve the current line matches: clear them, fire specials, make
    /// new specials (at a `prefer`red cell when one is in the match).
    pub fn resolve_matches(&mut self, prefer: &[usize], rules: &Rules) -> Resolution {
        let groups = self.groups();
        if groups.is_empty() {
            return Resolution::default();
        }
        let mut queue = Vec::new();
        let mut create = Vec::new();
        for g in &groups {
            for &c in &g.cells {
                queue.push((c, 0.0, false));
            }
            if let Some(kind) = g.special() {
                let cell = g.special_cell(prefer);
                let piece = self.make(g.color, kind);
                create.push(Created { cell, piece });
            }
        }
        let mut res = self.clear_queue(queue, Vec::new(), create, &[], rules);
        res.groups = groups;
        res
    }

    /// The swap of `a` (moved piece, now at `b`... see below) when it is a
    /// special combo or involves a Prism. Call after [`Board::swap`]: `a`
    /// and `b` are the two cells, `target` the cell the player moved onto.
    /// Returns `None` for a plain swap (resolve it with line matches).
    pub fn resolve_swap_combo(&mut self, a: usize, b: usize, rules: &Rules) -> Option<Resolution> {
        let pa = self.piece(a).copied()?;
        let pb = self.piece(b).copied()?;
        let special_pair = pa.kind.is_special() && pb.kind.is_special();
        let prism = pa.kind == Kind::Prism || pb.kind == Kind::Prism;
        if !special_pair && !prism {
            return None;
        }
        // The combo fires at `b` (where the player moved the first piece).
        let (src, other, cell) = if pa.kind == Kind::Prism || (!prism && pa.kind.is_special()) {
            (pa, pb, b)
        } else {
            (pb, pa, b)
        };
        let mut queue: Vec<(usize, f32, bool)> = vec![(a, 0.0, true), (b, 0.0, true)];
        let mut fired = Vec::new();
        let consumed = [a, b];
        if src.kind == Kind::Prism && other.kind == Kind::Prism {
            fired.push(Fired {
                cell,
                kind: Kind::Prism,
                color: NO_COLOR,
                delay: 0.0,
                combo: Some(Kind::Prism),
            });
            for (c, d) in self.blast_area(cell, Kind::Prism, NO_COLOR, Some(Kind::Prism), rules) {
                queue.push((c, d, true));
            }
        } else if src.kind == Kind::Prism && other.kind.is_special() {
            // Every piece of the other's colour becomes that special, then fires.
            let color = other.color;
            fired.push(Fired {
                cell,
                kind: Kind::Prism,
                color,
                delay: 0.0,
                combo: Some(other.kind),
            });
            let (cx, cy) = self.xy(cell);
            for i in 0..self.len() {
                if i == a || i == b {
                    continue;
                }
                let (x, y) = self.xy(i);
                let kind = match other.kind {
                    k if k.is_streak() => {
                        if (x + y) % 2 == 0 {
                            Kind::StreakH
                        } else {
                            Kind::StreakV
                        }
                    }
                    k => k,
                };
                if let Some(p) = self.cells[i].as_mut()
                    && p.color == color
                {
                    p.kind = kind;
                    let d = ((x - cx).abs()).max((y - cy).abs()) as f32;
                    queue.push((i, 1.0 + d * 0.5, true));
                }
            }
        } else if src.kind == Kind::Prism {
            let color = other.color;
            fired.push(Fired {
                cell,
                kind: Kind::Prism,
                color,
                delay: 0.0,
                combo: None,
            });
            for (c, d) in self.blast_area(cell, Kind::Prism, color, None, rules) {
                queue.push((c, d * 0.5, true));
            }
        } else {
            // Two specials (no Prism): a bigger blast at `cell`.
            let (k1, k2) = (src.kind, other.kind);
            let (kind, combo) = if k1 == Kind::Bomb || k2 == Kind::Bomb {
                (Kind::Bomb, if k1 == Kind::Bomb { k2 } else { k1 })
            } else {
                (k1, k2)
            };
            fired.push(Fired {
                cell,
                kind,
                color: src.color,
                delay: 0.0,
                combo: Some(combo),
            });
            for (c, d) in self.blast_area(cell, kind, src.color, Some(combo), rules) {
                queue.push((c, d, true));
            }
        }
        Some(self.clear_queue(queue, fired, Vec::new(), &consumed, rules))
    }

    /// Fire the special at `cell` on its own (Sugar Rush), as if blasted.
    pub fn fire(&mut self, cell: usize, rules: &Rules) -> Resolution {
        self.clear_queue(vec![(cell, 0.0, true)], Vec::new(), Vec::new(), &[], rules)
    }

    /// Drop pieces into the gaps and refill from the top with `rng`.
    pub fn gravity(&mut self, rng: &mut Pcg32) -> Vec<Fall> {
        let mut falls = Vec::new();
        for x in 0..self.w {
            let rows: Vec<usize> = (0..self.h).filter(|&y| self.mask[y * self.w + x]).collect();
            // Bottom-up: existing pieces keep their order.
            let mut pieces: Vec<(usize, Piece)> = Vec::new();
            for &y in rows.iter().rev() {
                if let Some(p) = self.cells[y * self.w + x].take() {
                    pieces.push((y, p));
                }
            }
            let mut k = 0;
            for &y in rows.iter().rev() {
                let i = y * self.w + x;
                if let Some((from, p)) = pieces.get(k).copied() {
                    self.cells[i] = Some(p);
                    if from != y {
                        falls.push(Fall {
                            cell: i,
                            rows: (y - from) as f32,
                            new: false,
                        });
                    }
                    k += 1;
                } else {
                    // New piece: the first lands lowest and starts just above row 0.
                    let n_new = k + 1 - pieces.len();
                    let c = rng.below(u32::from(self.colors)) as u8;
                    self.cells[i] = Some(self.make(c, Kind::Normal));
                    falls.push(Fall {
                        cell: i,
                        rows: y as f32 + n_new as f32,
                        new: true,
                    });
                    k += 1;
                }
            }
        }
        falls
    }

    /// Pieces per colour on the board.
    pub fn color_counts(&self) -> [u32; MAX_COLORS as usize] {
        let mut n = [0u32; MAX_COLORS as usize];
        for p in self.cells.iter().flatten() {
            if let Some(c) = n.get_mut(p.color as usize) {
                *c += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules::default()
    }

    fn colors_of(b: &Board) -> Vec<String> {
        (0..b.h)
            .map(|y| {
                (0..b.w)
                    .map(|x| match b.cells[y * b.w + x] {
                        None if !b.mask[y * b.w + x] => '.',
                        None => '_',
                        Some(p) => match p.kind {
                            Kind::Prism => 'P',
                            Kind::StreakH => 'h',
                            Kind::StreakV => 'v',
                            Kind::Bomb => 'b',
                            Kind::Normal => char::from(b'0' + p.color),
                        },
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn shape_parses_holes_and_frosting() {
        let rows: Vec<String> = ["#f#", "#.F", "###", "###"].iter().map(|s| s.to_string()).collect();
        let s = Shape::parse(&rows).unwrap();
        assert_eq!((s.w, s.h), (3, 4));
        assert!(!s.mask[4]);
        assert_eq!(s.frost[1], 1);
        assert_eq!(s.frost[5], 2);
        assert!(Shape::parse(&["##".into(), "##".into()]).is_err());
        assert!(Shape::parse(&["#x#".into(), "###".into(), "###".into()]).is_err());
    }

    #[test]
    fn generated_boards_have_no_match_and_a_move() {
        for seed in 0..200u64 {
            let mut rng = Pcg32::seeded(seed);
            let colors = 3 + (seed % 4) as u8;
            let mut b = Board::empty(&Shape::full(8, 8), colors);
            b.generate(&mut rng, &rules());
            assert!(b.groups().is_empty(), "seed {seed}: {:?}", colors_of(&b));
            assert!(b.has_move(), "seed {seed}");
            assert!(b.cells.iter().all(|c| c.is_some()));
        }
        // Holes stay empty.
        let rows: Vec<String> = ["..####..", "########", "########", "########", "########", "########", "########", "..####.."]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let shape = Shape::parse(&rows).unwrap();
        let mut b = Board::empty(&shape, 5);
        b.generate(&mut Pcg32::seeded(9), &rules());
        assert!(b.cells[0].is_none() && b.cells[2].is_some());
        assert!(b.groups().is_empty());
    }

    #[test]
    fn same_seed_same_board() {
        let make = |s| {
            let mut b = Board::empty(&Shape::full(8, 8), 5);
            b.generate(&mut Pcg32::seeded(s), &rules());
            colors_of(&b)
        };
        assert_eq!(make(42), make(42));
        assert_ne!(make(42), make(43));
    }

    #[test]
    fn fallback_pattern_has_moves_and_no_matches() {
        for colors in 3..=6u8 {
            for (w, h) in [(3, 3), (5, 4), (8, 8), (9, 9)] {
                let mut b = Board::empty(&Shape::full(w, h), colors);
                b.fallback_fill();
                assert!(b.groups().is_empty(), "{colors} {w}x{h}");
                assert!(b.has_move(), "{colors} {w}x{h}: {:?}", colors_of(&b));
            }
        }
    }

    #[test]
    fn swap_validation() {
        let b = Board::from_rows(&["0012", "3401", "2323"], 5);
        // Moving the 0 at (2,1) up to (2,0) makes 0 0 0 in row 0.
        assert!(b.valid_swap(6, 2));
        assert!(b.valid_swap(2, 6));
        // Not adjacent / no match.
        assert!(!b.valid_swap(0, 5));
        assert!(!b.valid_swap(0, 4));
        assert!(!b.valid_swap(3, 7));
        // A Prism swaps with anything; two specials always swap.
        let p = Board::from_rows(&["P123", "4501", "2323"], 6);
        assert!(p.valid_swap(0, 1));
        let s = Board::from_rows(&["hb23", "4501", "2323"], 6);
        assert!(s.valid_swap(0, 1));
    }

    #[test]
    fn groups_and_specials() {
        // Row of 5 -> Prism.
        let b = Board::from_rows(&["00000", "12341", "23412"], 5);
        let g = b.groups();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].special(), Some(Kind::Prism));
        // Row of 4 -> horizontal streak; column of 4 -> vertical.
        let b = Board::from_rows(&["00001", "12341", "23412"], 5);
        assert_eq!(b.groups()[0].special(), Some(Kind::StreakH));
        let b = Board::from_rows(&["0123", "0231", "0312", "0123"], 5);
        assert_eq!(b.groups()[0].special(), Some(Kind::StreakV));
        // L shape -> bomb at the corner.
        let b = Board::from_rows(&["0001", "0231", "0312"], 5);
        let g = b.groups();
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!(g[0].cells, vec![0, 1, 2, 4, 8]);
        assert_eq!(g[0].special(), Some(Kind::Bomb));
        assert_eq!(g[0].special_cell(&[]), 0);
        assert_eq!(g[0].special_cell(&[8]), 8);
        // Plain three: nothing.
        let b = Board::from_rows(&["0001", "1231", "2312"], 5);
        assert_eq!(b.groups()[0].special(), None);
        // Two separate groups of one colour stay separate.
        let b = Board::from_rows(&["000100", "123123", "231000"], 5);
        assert_eq!(b.groups().len(), 2);
    }

    #[test]
    fn match_clears_and_makes_a_streak() {
        let mut b = Board::from_rows(&["00012", "12304", "23412"], 5);
        // Move the 0 at (3,1) up to (3,0): 0 0 0 0 in row 0.
        b.swap(3, 8);
        let r = b.resolve_matches(&[3, 8], &rules());
        assert_eq!(r.cleared.len(), 4);
        assert_eq!(r.created.len(), 1);
        assert_eq!(r.created[0].cell, 3);
        assert_eq!(r.created[0].piece.kind, Kind::StreakH);
        assert_eq!(b.piece(3).unwrap().kind, Kind::StreakH);
        assert!(b.piece(0).is_none());
    }

    #[test]
    fn specials_fire_in_chains() {
        // A vertical streak in a matched row clears its column, which holds a bomb.
        let mut b = Board::from_rows(&["01234", "12b40", "00v00", "34123", "41234"], 5);
        // Row 2 = 0 0 v(0) 0 0 -> one group of 5 (v counts as colour 0).
        let r = b.resolve_matches(&[], &rules());
        let fired: Vec<Kind> = r.fired.iter().map(|f| f.kind).collect();
        assert!(fired.contains(&Kind::StreakV), "{fired:?}");
        assert!(fired.contains(&Kind::Bomb), "{fired:?}");
        // The whole column 2 is gone, and the bomb's 3x3 around (2,1).
        for y in 0..5 {
            assert!(r.cleared.iter().any(|c| c.cell == y * 5 + 2), "col cell {y}");
        }
        for c in [1, 3, 6, 8] {
            assert!(r.cleared.iter().any(|x| x.cell == c), "bomb area {c}");
        }
        // Blasted cells come later than the line itself.
        let late = r.cleared.iter().filter(|c| c.blasted).map(|c| c.delay).fold(0.0, f32::max);
        assert!(late >= 1.0);
        // The 5-line made a Prism in the middle.
        assert_eq!(r.created[0].piece.kind, Kind::Prism);
    }

    #[test]
    fn prism_clears_a_colour_and_prism_pair_clears_all() {
        let mut b = Board::from_rows(&["P1012", "21302", "01231"], 4);
        b.swap(0, 1);
        let r = b.resolve_swap_combo(0, 1, &rules()).unwrap();
        // Colour 1 everywhere, plus the prism itself.
        let ones = r.cleared.iter().filter(|c| c.piece.color == 1).count();
        assert_eq!(ones, 5);
        assert!(r.cleared.iter().any(|c| c.piece.kind == Kind::Prism));
        assert!(b.cells.iter().flatten().all(|p| p.color != 1));
        let mut b = Board::from_rows(&["PP12", "2130", "0123"], 4);
        let r = b.resolve_swap_combo(0, 1, &rules()).unwrap();
        assert_eq!(r.cleared.len(), 12);
        assert!(b.cells.iter().all(|c| c.is_none()));
    }

    #[test]
    fn special_pairs_make_bigger_blasts() {
        // Streak + streak: a cross.
        let mut b = Board::from_rows(&["01230", "12301", "2hv12", "30123", "01230"], 4);
        let r = b.resolve_swap_combo(11, 12, &rules()).unwrap();
        assert_eq!(r.cleared.len(), 9, "row 2 and column 2");
        // Bomb + bomb: 5x5.
        let mut b = Board::from_rows(&["01230", "12301", "2bb12", "30123", "01230"], 4);
        let r = b.resolve_swap_combo(11, 12, &rules()).unwrap();
        assert_eq!(r.cleared.len(), 25);
        // A plain swap is not a combo.
        let mut b = Board::from_rows(&["0123", "1230"], 4);
        assert!(b.resolve_swap_combo(0, 1, &rules()).is_none());
    }

    #[test]
    fn gravity_drops_and_refills_through_holes() {
        let mut b = Board::from_rows(&["01", "1.", "23"], 4);
        // Clear the bottom-left and the top-right.
        b.cells[4] = None;
        b.cells[1] = None;
        let falls = b.gravity(&mut Pcg32::seeded(1));
        assert!(b.cells.iter().enumerate().all(|(i, c)| c.is_some() == b.mask[i]));
        // Column 0: 0 and 1 drop one row; one new piece arrives at the top.
        assert_eq!(b.piece(4).unwrap().color, 1);
        assert_eq!(b.piece(2).unwrap().color, 0);
        let new: Vec<&Fall> = falls.iter().filter(|f| f.new).collect();
        assert_eq!(new.len(), 2);
        // Column 1 (hole in the middle): the new piece lands at row 0.
        assert!(falls.iter().any(|f| f.cell == 1 && f.new && f.rows == 1.0));
        assert!(falls.iter().any(|f| f.cell == 2 && !f.new && f.rows == 1.0));
    }

    #[test]
    fn best_move_prefers_specials_and_is_deterministic() {
        // One move makes four in a row, others only three.
        let b = Board::from_rows(&["00102", "12010", "21321"], 4);
        let m = b.best_move().unwrap();
        let mut t = b.clone();
        t.swap(m.a, m.b);
        let g = t.groups();
        assert!(g.iter().any(|g| g.special().is_some()), "{m:?}");
        assert_eq!(b.best_move(), b.best_move());
        let w = b.worst_move().unwrap();
        assert!(w.value <= m.value);
    }

    #[test]
    fn shuffle_finds_a_playable_arrangement() {
        // No moves: a checkerboard of two colours plus stripes.
        let mut b = Board::from_rows(&["0101", "2323", "0101", "2323"], 4);
        assert!(!b.has_move());
        let moved = b.shuffle(&mut Pcg32::seeded(3));
        assert!(b.groups().is_empty());
        assert!(b.has_move(), "{:?}", colors_of(&b));
        assert_eq!(moved.len(), 16);
        // Same pieces, rearranged.
        let mut before = [0u32; 6];
        for c in ["0101", "2323", "0101", "2323"].concat().chars() {
            before[c.to_digit(10).unwrap() as usize] += 1;
        }
        assert_eq!(b.color_counts(), before);
    }

    #[test]
    fn frosting_melts_under_cleared_pieces() {
        let rows: Vec<String> = ["#F#", "###", "###"].iter().map(|s| s.to_string()).collect();
        let shape = Shape::parse(&rows).unwrap();
        let mut b = Board::empty(&shape, 4);
        for (i, c) in [0u8, 0, 0, 1, 2, 3, 2, 3, 1].iter().enumerate() {
            b.cells[i] = Some(b.make(*c, Kind::Normal));
        }
        assert_eq!(b.frost_left(), 2);
        let r = b.resolve_matches(&[], &rules());
        assert_eq!(r.frost_melted, 1);
        assert_eq!(b.frost_left(), 1);
    }
}
