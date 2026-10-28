//! The kit's pixel glyphs: bold 6 × 7 HUD digits (with `x`), and 3 × 5 mini
//! digits (with `U` and `P`) for score pops and "1UP". Built into the sprite
//! sheet with a top-lit fill, an outline and a drop shadow (see
//! [`crate::art`]), so they read over any background.

/// HUD glyph order: `0`..`9`, then `x`, then the letters of the
/// "LEVEL CLEAR!" banner.
pub const HUD_CHARS: &str = "0123456789xLEVCAR!";

/// The banner shown in the canvas once the level is cleared.
pub const LEVEL_CLEAR: &str = "LEVEL CLEAR!";

/// Bold HUD glyphs, 6 × 7, `#` = ink.
pub const HUD: [[&str; 7]; 18] = [
    [".####.", "##..##", "##.###", "######", "###.##", "##..##", ".####."],
    ["..##..", ".###..", "..##..", "..##..", "..##..", "..##..", "######"],
    [".####.", "##..##", "....##", "..###.", ".##...", "##....", "######"],
    [".####.", "##..##", "....##", "..###.", "....##", "##..##", ".####."],
    ["...###", "..####", ".##.##", "##..##", "######", "....##", "....##"],
    ["######", "##....", "#####.", "....##", "....##", "##..##", ".####."],
    [".####.", "##....", "#####.", "##..##", "##..##", "##..##", ".####."],
    ["######", "....##", "...##.", "..##..", "..##..", "..##..", "..##.."],
    [".####.", "##..##", "##..##", ".####.", "##..##", "##..##", ".####."],
    [".####.", "##..##", "##..##", ".#####", "....##", "...##.", ".###.."],
    ["......", "......", "##..##", ".####.", "..##..", ".####.", "##..##"],
    ["##....", "##....", "##....", "##....", "##....", "##....", "######"],
    ["######", "##....", "##....", "#####.", "##....", "##....", "######"],
    ["##..##", "##..##", "##..##", "##..##", ".####.", ".####.", "..##.."],
    [".####.", "##..##", "##....", "##....", "##....", "##..##", ".####."],
    [".####.", "##..##", "##..##", "######", "##..##", "##..##", "##..##"],
    ["#####.", "##..##", "##..##", "#####.", "##.##.", "##..##", "##..##"],
    ["..##..", "..##..", "..##..", "..##..", "..##..", "......", "..##.."],
];

/// Mini glyph order: `0`..`9`, then `U`, `P` (1UP), then `F`, `R`, `E`,
/// `O`, `A`, `M` (the "FREE ROAM" label).
pub const MINI_CHARS: &str = "0123456789UPFREOAM";

/// Mini glyphs, 3 × 5.
pub const MINI: [[&str; 5]; 18] = [
    ["###", "#.#", "#.#", "#.#", "###"],
    [".#.", "##.", ".#.", ".#.", "###"],
    ["###", "..#", "###", "#..", "###"],
    ["###", "..#", ".##", "..#", "###"],
    ["#.#", "#.#", "###", "..#", "..#"],
    ["###", "#..", "###", "..#", "###"],
    ["###", "#..", "###", "#.#", "###"],
    ["###", "..#", "..#", ".#.", ".#."],
    ["###", "#.#", "###", "#.#", "###"],
    ["###", "#.#", "###", "..#", "###"],
    ["#.#", "#.#", "#.#", "#.#", "###"],
    ["##.", "#.#", "##.", "#..", "#.."],
    ["###", "#..", "##.", "#..", "#.."],
    ["##.", "#.#", "##.", "#.#", "#.#"],
    ["###", "#..", "##.", "#..", "###"],
    ["###", "#.#", "#.#", "#.#", "###"],
    [".#.", "#.#", "###", "#.#", "#.#"],
    ["#.#", "###", "###", "#.#", "#.#"],
];

/// Index of `c` in [`HUD_CHARS`].
pub fn hud_index(c: char) -> Option<usize> {
    HUD_CHARS.chars().position(|x| x == c)
}

/// Index of `c` in [`MINI_CHARS`].
pub fn mini_index(c: char) -> Option<usize> {
    MINI_CHARS.chars().position(|x| x == c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_rectangular() {
        for g in HUD {
            assert!(g.iter().all(|r| r.len() == 6));
        }
        for g in MINI {
            assert!(g.iter().all(|r| r.len() == 3));
        }
        assert_eq!(HUD.len(), HUD_CHARS.len());
        assert_eq!(MINI.len(), MINI_CHARS.len());
        assert_eq!(hud_index('x'), Some(10));
        // The banner has a glyph for every letter (a space is an empty advance).
        assert!(LEVEL_CLEAR.chars().all(|c| c == ' ' || hud_index(c).is_some()));
        assert_eq!(mini_index('P'), Some(11));
        // The free-roam label has a glyph for every letter.
        assert!("FREEROAM".chars().all(|c| mini_index(c).is_some()));
    }
}
