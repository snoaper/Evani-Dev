//! PNG in and out (RGBA8), through the `png` crate.

use std::io::Cursor;
use std::path::Path;

/// Decode a PNG to `(width, height, RGBA8)`; any colour type or bit depth.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| format!("png: {e}"))?;
    let size = reader.output_buffer_size().ok_or("png: image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("png: palette not expanded".into()),
    };
    if rgba.len() != n * 4 {
        return Err(format!("png: {} bytes for {w} x {h}", rgba.len()));
    }
    Ok((w, h, rgba))
}

/// Encode RGBA8 as PNG bytes.
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(|e| format!("png: {e}"))?;
        wr.write_image_data(rgba).map_err(|e| format!("png: {e}"))?;
    }
    Ok(out)
}

/// Write RGBA8 as a PNG file.
pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let bytes = encode_png(w, h, rgba)?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_round_trip() {
        let px: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 10) as u8).collect();
        let bytes = super::encode_png(3, 2, &px).unwrap();
        let (w, h, back) = super::decode_png(&bytes).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(back, px);
    }
}
