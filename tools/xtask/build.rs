//! Gives the program the folder it was compiled in, as `EVANI_COMPILED_DIR`.
//! A pack build (`EVANI_PACK_BUILD=1`, set only by `engine-pack`) gets a fixed
//! name instead, so the program's bytes do not depend on the build folder
//! (docs/ENGINE_PACK.md, "Build recipe"). A pack build's folder is gone or
//! elsewhere on a user's PC anyway.
fn main() {
    println!("cargo:rerun-if-env-changed=EVANI_PACK_BUILD");
    println!("cargo:rerun-if-changed=build.rs");
    let dir = if std::env::var_os("EVANI_PACK_BUILD").is_some_and(|v| v == "1") {
        "/evani-pack/src/tools/xtask".to_string()
    } else {
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()
    };
    println!("cargo:rustc-env=EVANI_COMPILED_DIR={dir}");
}
