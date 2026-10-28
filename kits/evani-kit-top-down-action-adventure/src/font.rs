//! A tiny 3x5 pixel font for the HUD, banners and floating text: capitals, digits
//! and `! + - : .`. Each glyph is five rows of three bits (bit 2 is the
//! left column). No texture: [`runs`] hands the draw code horizontal runs to
//! fill as rectangles.

/// Width of one character cell in font pixels (3 wide plus 1 gap).
pub const ADVANCE: i32 = 4;
/// Height of a glyph in font pixels.
pub const HEIGHT: i32 = 5;

/// The five rows of a glyph (unknown characters are blank).
pub fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [6, 1, 2, 4, 7],
        '3' => [6, 1, 2, 1, 6],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 6, 1, 6],
        '6' => [3, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 6],
        '!' => [2, 2, 2, 0, 2],
        '+' => [0, 2, 7, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        ':' => [0, 2, 0, 2, 0],
        '.' => [0, 0, 0, 0, 2],
        _ => [0; 5],
    }
}

/// Width of `text` in font pixels (without the last gap).
pub fn width(text: &str) -> i32 {
    let n = text.chars().count() as i32;
    if n == 0 { 0 } else { n * ADVANCE - 1 }
}

/// Call `f(col, row, len)` for every horizontal run of lit pixels of `text`
/// (font pixels from the text's top-left).
pub fn runs(text: &str, mut f: impl FnMut(i32, i32, i32)) {
    for (i, c) in text.chars().enumerate() {
        let x0 = i as i32 * ADVANCE;
        for (row, bits) in glyph(c).iter().enumerate() {
            let mut col = 0;
            while col < 3 {
                if bits & (4 >> col) != 0 {
                    let start = col;
                    while col < 3 && bits & (4 >> col) != 0 {
                        col += 1;
                    }
                    f(x0 + start, row as i32, col - start);
                } else {
                    col += 1;
                }
            }
        }
    }
}
