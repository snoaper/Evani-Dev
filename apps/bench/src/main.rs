//! Entry points. In the browser the bench runs every tier (or the ones named
//! in `?tier=`) and publishes `window.__bench`. Natively it runs the same
//! content headless: CPU time only, no GPU.

#[cfg(target_arch = "wasm32")]
fn main() {
    use evani_bench::{BenchConfig, build_app};
    let query = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let cfg = BenchConfig::browser().with_query(&query);
    evani_core::web::run(build_app(cfg));
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use evani_bench::{BenchConfig, BenchRun, build_app, format_table};
    // `cargo run -p evani-bench -- --frames 120 --tier low` mirrors the
    // page's query string.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut query = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(key) = a.strip_prefix("--")
            && let Some(v) = it.next()
        {
            query.push_str(&format!("&{key}={v}"));
        }
    }
    let cfg = BenchConfig::native().with_query(&query);
    let mut app = build_app(cfg);
    // Each iteration is one 60 Hz frame; the run ends by itself.
    let mut guard = 0u32;
    while !app.world.res::<BenchRun>().is_done() && guard < 1_000_000 {
        app.run_headless(1, 1000.0 / 60.0);
        guard += 1;
    }
    let report = app.world.res::<BenchRun>().report().clone();
    println!("Evani.Dev bench (native, CPU only: frame ms is the engine's CPU time, no GPU)");
    print!("{}", format_table(&report.results));
    if let Some(e) = &report.error {
        eprintln!("bench error: {e}");
    }
    if report.error.is_some() || !report.done || report.results.iter().any(|r| !r.pass) {
        std::process::exit(1);
    }
}
