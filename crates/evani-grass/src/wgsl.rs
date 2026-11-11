//! The grass WGSL as it ships: each file without what the compiler never
//! reads (comments, indentation, line ends, spaces beside operators),
//! squeezed at compile time, so the sources stay readable and only the short
//! form reaches the download. The same rule as evani-render's trails
//! shaders.

const fn squeezed<const N: usize>(src: &[u8]) -> ([u8; N], usize) {
    const fn word(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_'
    }
    let mut out = [0u8; N];
    let (mut i, mut n) = (0, 0);
    let mut space = false;
    while i < src.len() {
        let c = src[i];
        i += 1;
        if c == b'/' && i < src.len() && src[i] == b'/' {
            while i < src.len() && src[i] != b'\n' {
                i += 1;
            }
        } else if matches!(c, b' ' | b'\n' | b'\r' | b'\t') {
            space = true;
        } else {
            // A space only where two words would run together, or two signs
            // would read as one (`x - -y`).
            if space && n > 0 && ((word(out[n - 1]) && word(c)) || out[n - 1] == c) {
                out[n] = b' ';
                n += 1;
            }
            space = false;
            out[n] = c;
            n += 1;
        }
    }
    (out, n)
}

/// The first `M` bytes of `a`.
const fn head<const M: usize, const N: usize>(a: &[u8; N]) -> [u8; M] {
    let mut out = [0u8; M];
    let mut i = 0;
    while i < M {
        out[i] = a[i];
        i += 1;
    }
    out
}

/// Bytes as text (a compile error if they are not UTF-8).
const fn text(bytes: &'static [u8]) -> &'static str {
    match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => panic!("WGSL must be UTF-8"),
    }
}

const COMMON_SRC: &[u8] = include_bytes!("../shaders/grass_common.wgsl");
const CS_SRC: &[u8] = include_bytes!("../shaders/grass_cs.wgsl");
const DRAW_SRC: &[u8] = include_bytes!("../shaders/grass_draw.wgsl");
static COMMON_FULL: ([u8; COMMON_SRC.len()], usize) = squeezed(COMMON_SRC);
static CS_FULL: ([u8; CS_SRC.len()], usize) = squeezed(CS_SRC);
static DRAW_FULL: ([u8; DRAW_SRC.len()], usize) = squeezed(DRAW_SRC);
static COMMON_SHORT: [u8; COMMON_FULL.1] = head(&COMMON_FULL.0);
static CS_SHORT: [u8; CS_FULL.1] = head(&CS_FULL.0);
static DRAW_SHORT: [u8; DRAW_FULL.1] = head(&DRAW_FULL.0);

/// The structs and noise both modules share.
pub const COMMON: &str = text(&COMMON_SHORT);
/// The compute passes (after [`COMMON`]).
pub const CS: &str = text(&CS_SHORT);
/// The draws (after [`COMMON`]; the fog slot goes after them).
pub const DRAW: &str = text(&DRAW_SHORT);

/// The compute module.
pub fn cs_module() -> String {
    [COMMON, "\n", CS].concat()
}

/// The draw module with the game's fog slot function.
pub fn draw_module(fog: &str) -> String {
    [COMMON, "\n", DRAW, "\n", fog].concat()
}
