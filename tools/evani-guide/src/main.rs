//! `evani-guide`: the same commands as `cargo xtask init-ai | doctor | check
//! | new-game | ai-eval`, as a small binary that builds in seconds (the
//! Claude Code Stop hook runs `check --hook` through it).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(evani_guide::cli::run(&args));
}
