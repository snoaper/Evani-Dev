//! `evani-reqcheck`: the prebuilt requirements checker in the engine pack
//! (`<pack>/tools/bin/`). See the library for what it checks.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(evani_reqcheck::cli(&args));
}
