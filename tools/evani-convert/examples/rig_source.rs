//! Writes `tests/fixtures/models/rig.src.glb`, the skinned, animated fixture
//! the clip baker (S3-7) is tested on (`evani_convert::fixture::rig_glb`),
//! or with `--static` its static copy `rig.static.glb` (the tube without
//! its skin, `evani_convert::fixture::rig_static_glb`; GPU skinning, S3-9).
//!
//! `cargo run -p evani-convert --example rig_source -- tests/fixtures/models/rig.src.glb`
//! `cargo run -p evani-convert --example rig_source -- --static tests/fixtures/models/rig.static.glb`

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let still = args.iter().any(|a| a == "--static");
    let out = args.iter().find(|a| *a != "--static").ok_or("usage: rig_source [--static] <out.glb>")?;
    let b = if still { evani_convert::fixture::rig_static_glb()? } else { evani_convert::fixture::rig_glb()? };
    std::fs::write(out, &b).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out} ({} bytes)", b.len());
    Ok(())
}
