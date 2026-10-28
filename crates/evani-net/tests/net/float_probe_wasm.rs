// The wasm32 side of the float probe: `rustc --target wasm32-unknown-unknown
// --crate-type cdylib` compiles this file alone (no cargo, no dependencies,
// no imports). Not part of the test crate.

#[path = "float_kernel.rs"]
mod float_kernel;

#[no_mangle]
pub extern "C" fn evani_probe(family: u32, iters: u32) -> u64 {
    float_kernel::run(family, iters)
}

#[no_mangle]
pub extern "C" fn evani_probe_families() -> u32 {
    float_kernel::FAMILIES.len() as u32
}
