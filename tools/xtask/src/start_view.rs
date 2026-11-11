//! A game's start view as a picture, and how much two of them differ
//! (`upgrade-engine`'s before-and-after pictures, docs/ENGINE_PACK.md 4.4).
//!
//! [`capture`] serves a ship build on a free localhost port and lets
//! `tools/bench-runner/run.mjs` (raw DevTools protocol over Node's own
//! WebSocket, no npm packages) open it in headless, muted Chrome or Edge
//! with its own fresh profile, pointer and keyboard locks stubbed, at a
//! fixed size and render tier, and save one screenshot `shot_ms` after the
//! page starts. [`compare`] counts the pixels whose colour moved by more
//! than [`TOLERANCE`] in any channel and paints them red on a dimmed copy of
//! the second picture.

use std::path::Path;
use std::process::{Command, Stdio};

/// A pixel counts as changed when a colour channel moved by more than this
/// (of 255): small enough to see a different look, large enough to ignore
/// dithering, filtering and a frame of animation drift.
pub const TOLERANCE: u8 = 32;
/// The capture size (the engine's review and netlog size).
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;
/// The render tier both pictures use (saved in the page's storage, so the
/// engine's automatic tier choice can't make the two differ).
pub const TIER: &str = "medium";

/// An image as 8-bit RGBA.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub px: Vec<u8>,
}

pub fn read_png(p: &Path) -> Result<Rgba, String> {
    let f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut d = png::Decoder::new(std::io::BufReader::new(f));
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info().map_err(|e| format!("{}: {e}", p.display()))?;
    let mut buf = vec![0; r.output_buffer_size().ok_or_else(|| format!("{}: too large", p.display()))?];
    let info = r.next_frame(&mut buf).map_err(|e| format!("{}: {e}", p.display()))?;
    let raw = &buf[..info.line_size * info.height as usize];
    let px: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => raw.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::Grayscale => raw.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => raw.as_chunks::<2>().0.iter().flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Indexed => return Err(format!("{}: a palette image was not expanded", p.display())),
    };
    Ok(Rgba { width: info.width, height: info.height, px })
}

/// Write 8-bit RGB pixels as a PNG.
pub fn write_png(p: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<(), String> {
    let f = std::fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| format!("{}: {e}", p.display()))?;
    w.write_image_data(rgb).map_err(|e| format!("{}: {e}", p.display()))
}

/// How two start views differ.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub width: u32,
    pub height: u32,
    /// Both pictures have the same size (otherwise every pixel counts as changed).
    pub same_size: bool,
    pub changed: u64,
    pub total: u64,
    /// Mean colour change over every channel of every pixel, 0..1.
    pub mean_abs: f64,
}

impl Diff {
    pub fn changed_percent(&self) -> f64 {
        if self.total == 0 { 0.0 } else { 100.0 * self.changed as f64 / self.total as f64 }
    }
}

/// Compare `before` with `after`: the counts, and a picture of `after`
/// dimmed with every changed pixel red (RGB, `after`'s size).
pub fn compare(before: &Rgba, after: &Rgba) -> (Diff, Vec<u8>) {
    let total = u64::from(after.width) * u64::from(after.height);
    if before.width != after.width || before.height != after.height {
        let img = after.px.as_chunks::<4>().0.iter().flat_map(|_| [255u8, 0, 0]).collect();
        return (Diff { width: after.width, height: after.height, same_size: false, changed: total, total, mean_abs: 1.0 }, img);
    }
    let (mut changed, mut sum) = (0u64, 0u64);
    let mut img = Vec::with_capacity(after.px.len() / 4 * 3);
    for (a, b) in before.px.as_chunks::<4>().0.iter().zip(after.px.as_chunks::<4>().0.iter()) {
        let d: Vec<u8> = (0..3).map(|i| a[i].abs_diff(b[i])).collect();
        sum += d.iter().map(|&v| u64::from(v)).sum::<u64>();
        if d.iter().any(|&v| v > TOLERANCE) {
            changed += 1;
            img.extend([255, 0, 0]);
        } else {
            let lum = (u32::from(b[0]) * 3 + u32::from(b[1]) * 6 + u32::from(b[2])) / 10;
            let g = (lum / 3 + 30) as u8;
            img.extend([g, g, g]);
        }
    }
    let mean_abs = if total == 0 { 0.0 } else { sum as f64 / (total as f64 * 3.0 * 255.0) };
    (Diff { width: after.width, height: after.height, same_size: true, changed, total, mean_abs }, img)
}

/// Options for [`capture`].
#[derive(Debug, Clone)]
pub struct ShotOpts {
    /// When the picture is taken, in ms after the page starts loading.
    pub shot_ms: u32,
    /// A Chrome or Edge to use (`--chrome`); else the bench runner's finder.
    pub chrome: Option<String>,
}

/// Node: `EVANI_NODE` (Evani.App's own Node), else `node` on the PATH.
pub fn node() -> std::ffi::OsString {
    std::env::var_os("EVANI_NODE").filter(|v| !v.is_empty()).unwrap_or_else(|| "node".into())
}

/// The start view of the ship build in `dist` (game `id`) as a PNG at `out`.
pub fn capture(engine_root: &Path, dist: &Path, id: &str, out: &Path, o: &ShotOpts) -> Result<(), String> {
    if !dist.join("index.html").is_file() {
        return Err(format!("{}: no ship build here (no index.html)", dist.display()));
    }
    let runner = engine_root.join("tools").join("bench-runner").join("run.mjs");
    if !runner.is_file() {
        return Err(format!("{} is missing (the browser runner)", runner.display()));
    }
    let shots = out.with_extension("shots");
    let _ = std::fs::remove_dir_all(&shots);
    std::fs::create_dir_all(&shots).map_err(|e| format!("{}: {e}", shots.display()))?;
    let server = crate::bench::Server::start_in(dist.to_path_buf(), 8250..=8259)?;
    let url = format!("http://127.0.0.1:{}/", server.port);
    let job = serde_json::json!([{ "tier": TIER, "label": "start", "width": WIDTH, "height": HEIGHT, "storage": { format!("evani:{id}:render.tier"): TIER } }]);
    let mut cmd = Command::new(node());
    cmd.arg(&runner)
        .args(["--url", &url, "--jobs", &job.to_string()])
        .args(["--page-ms", &(o.shot_ms + 500).to_string(), "--shots-at", &o.shot_ms.to_string()])
        .arg("--shots")
        .arg(&shots)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(c) = &o.chrome {
        cmd.args(["--chrome", c]);
    }
    let st = cmd.status().map_err(|e| format!("could not start Node for the browser runner: {e} (set EVANI_NODE to Evani.App's node)"))?;
    drop(server);
    let shot = shots.join(format!("start-{}ms.png", o.shot_ms));
    let result = if !st.success() {
        Err(format!("the browser runner failed ({st}); it needs Chrome or Edge with WebGPU"))
    } else if !shot.is_file() {
        Err("the browser runner took no picture".to_string())
    } else {
        std::fs::rename(&shot, out).map_err(|e| format!("{}: {e}", out.display()))
    };
    let _ = std::fs::remove_dir_all(&shots);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Rgba {
        let px = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| {
            let c = f(x, y);
            [c[0], c[1], c[2], 255]
        });
        Rgba { width: w, height: h, px: px.collect() }
    }

    #[test]
    fn small_changes_are_ignored_and_large_ones_counted() {
        let a = img(10, 10, |x, _| [100, 120, (x * 10) as u8]);
        // Noise within the tolerance: nothing changed.
        let b = img(10, 10, |x, _| [110, 100, (x * 10) as u8 + 5]);
        let (d, pic) = compare(&a, &b);
        assert_eq!((d.changed, d.total, d.same_size), (0, 100, true));
        assert!(d.mean_abs > 0.0 && d.mean_abs < 0.1);
        assert_eq!(pic.len(), 300);
        // The left 3 columns turn white: 30 %.
        let c = img(10, 10, |x, _| if x < 3 { [255, 255, 255] } else { [100, 120, (x * 10) as u8] });
        let (d, pic) = compare(&a, &c);
        assert_eq!(d.changed, 30);
        assert!((d.changed_percent() - 30.0).abs() < 1e-9);
        assert_eq!(&pic[..3], &[255, 0, 0], "a changed pixel is red");
        assert_ne!(&pic[3 * 5..3 * 5 + 3], &[255, 0, 0], "an unchanged one is not");
        // Another size: everything counts as changed.
        let (d, _) = compare(&a, &img(5, 5, |_, _| [0, 0, 0]));
        assert_eq!((d.same_size, d.changed_percent()), (false, 100.0));
    }

    #[test]
    fn pictures_round_trip_through_png() {
        let d = std::env::temp_dir().join(format!("evani-start-view-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let a = img(7, 3, |x, y| [(x * 30) as u8, (y * 80) as u8, 9]);
        let rgb: Vec<u8> = a.px.as_chunks::<4>().0.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
        write_png(&d.join("a.png"), 7, 3, &rgb).unwrap();
        assert_eq!(read_png(&d.join("a.png")).unwrap(), a);
        assert!(read_png(&d.join("none.png")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn capture_needs_a_ship_build() {
        let e = capture(Path::new("."), Path::new("no-such-dist"), "x", Path::new("x.png"), &ShotOpts { shot_ms: 100, chrome: None }).unwrap_err();
        assert!(e.contains("no ship build"), "{e}");
    }
}
