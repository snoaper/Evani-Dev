//! evani-live-bridge: Evani.Dev's live server (docs/LIVE_API.md).
//!
//! * `serve`: the "live server" the Evani.dev tab looks for. Binds
//!   127.0.0.1:P, answers `/live/status` at once, builds the game with the
//!   `live` module in the background, serves it at `/` with the dev token, and
//!   routes commands between controllers (`/ctl`, `POST /live/v1/cmd`) and
//!   the page (`/engine`). News for the tab goes out on `/evani-dev`.
//! * `mcp`: an MCP server on stdio; each tool is one command sent over `/ctl`
//!   with the token from `.evani/live/session.json`.
//!
//! By default `serve` builds with the pinned dx for live code swapping
//! (code.rs, LIVE_API.md 10.4): a saved Rust change swaps into the running
//! page with no reload. Without dx (or with `--no-hot`) it builds with Trunk.
//!
//! A native dev tool: never part of a game download.

mod auth;
mod bake;
mod baked;
mod code;
mod convert;
mod game;
mod html;
mod hub;
mod lookguard;
mod mcp;
mod models;
mod page;
mod proto;
mod server;
mod session;
mod tools;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::json;

const USAGE: &str = "usage:
  evani-live-bridge serve [--root <engine root>] [--port <P>] [--approve-port <A>] [--game <name|apps/name> | --app-dir <folder>] [--session <file>] [--models <folder>] [--any-port] [--quiet] [--no-build] [--no-hot]
  evani-live-bridge mcp   [--root <engine root>]

defaults: --root . --port 8096 --approve-port 0 (any free port) --game sandbox --session <root>/.evani/live/sessions/<port>.json
  and <root>/.evani/live/session.json (the most recent session)
--session: where this bridge writes (and on exit removes) its session file; review and test
  bridges give their own, so they never overwrite or delete the Evani.dev tab's
--any-port: if --port is taken, use the next free port from 8098 (`cargo xtask live` and `dev` pass it
  unless given --port); without it a taken port is an error, as the Evani.dev tab expects
--no-hot (or EVANI_LIVE_HOT=0): build with Trunk, no live code swapping
--app-dir <folder>: a game kept outside the engine folder (its app folder, the one with its
  evani.toml), found and guarded as `cargo xtask build --app-dir` does (it must build against
  this --root). The bridge builds it with Trunk in its own workspace (no live code swapping or
  page hot-replace), serves only files inside that build, and answers the look guard, model
  and convert commands with `not available for --app-dir yet`. Its session file defaults to
  <game workspace>/.evani/live/sessions/<port>.json (never the engine's tab session)
--models <folder>: model mode for the Model tab's page (docs/EVANI_MODEL_TAB.md 4.2): the
  /model/v1/ routes, the workspace watcher and the model.* commands over that Evani.Model
  workspace (relative to --root, e.g. .evani/model)

serve reads one line from stdin when Evani.App starts it (the look guard's
approval keys, docs/LIVE_API.md 15.6); without it, look approvals stay off.";

struct Args {
    mode: String,
    root: PathBuf,
    port: u16,
    /// `--any-port`: a taken `port` moves on to a free one (2.6).
    any_port: bool,
    /// The look guard's approve listener (15.7); 0: any free port.
    approve_port: u16,
    game: String,
    quiet: bool,
    no_build: bool,
    /// Live code swapping with dx (on unless `--no-hot` / `EVANI_LIVE_HOT=0`).
    hot: bool,
    /// `--session <file>`: the session file of this bridge, instead of
    /// `<root>/.evani/live/session.json` (the Evani.dev tab's).
    session: Option<PathBuf>,
    /// `--models <folder>`: model mode over that Evani.Model workspace.
    models: Option<PathBuf>,
    /// `--app-dir <folder>`: a game outside the engine folder.
    app_dir: Option<PathBuf>,
    /// `--game` was given (it may not be combined with `--app-dir`).
    game_given: bool,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut a = Args {
        mode: String::new(),
        root: PathBuf::from("."),
        port: 8096,
        any_port: false,
        approve_port: 0,
        game: "sandbox".into(),
        quiet: false,
        no_build: false,
        hot: true,
        session: None,
        models: None,
        app_dir: None,
        game_given: false,
    };
    let mut it = raw.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--root" => a.root = PathBuf::from(value("--root")?),
            "--port" => {
                let v = value("--port")?;
                a.port = v.parse().map_err(|_| format!("bad --port {v}"))?;
            }
            "--approve-port" => {
                let v = value("--approve-port")?;
                a.approve_port = v.parse().map_err(|_| format!("bad --approve-port {v}"))?;
            }
            "--game" => {
                a.game = value("--game")?;
                a.game_given = true;
            }
            "--app-dir" => a.app_dir = Some(PathBuf::from(value("--app-dir")?)),
            "--session" => a.session = Some(PathBuf::from(value("--session")?)),
            "--models" => a.models = Some(PathBuf::from(value("--models")?)),
            "--any-port" => a.any_port = true,
            "--quiet" => a.quiet = true,
            "--no-build" => a.no_build = true,
            "--no-hot" => a.hot = false,
            "-h" | "--help" => return Err(USAGE.into()),
            m if !m.starts_with('-') && a.mode.is_empty() => a.mode = m.to_string(),
            other => eprintln!("evani-live-bridge: ignoring unknown argument {other}"),
        }
    }
    if a.mode.is_empty() {
        return Err(USAGE.into());
    }
    if a.app_dir.is_some() && a.game_given {
        return Err("--game and --app-dir: give one (--app-dir is the game's folder)".into());
    }
    if a.app_dir.is_some() && a.models.is_some() {
        return Err(format!("--models {}", hub::OUTSIDE_LATER));
    }
    Ok(a)
}

/// The engine root as an absolute path without Windows' `\\?\` prefix (the
/// Evani.dev tab compares `/live/status` `root` with its folder setting).
fn absolute_root(root: &std::path::Path) -> Result<PathBuf, String> {
    std::path::absolute(root).map_err(|e| format!("{}: {e}", root.display()))
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("evani-live-bridge: {e}");
            return ExitCode::FAILURE;
        }
    };
    let res = match args.mode.as_str() {
        "serve" => rt.block_on(serve(args)),
        "mcp" => rt.block_on(async {
            let root = absolute_root(&args.root)?;
            mcp::run(root).await
        }),
        other => Err(format!("unknown mode {other}\n{USAGE}")),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("evani-live-bridge: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(args: Args) -> Result<(), String> {
    let root = absolute_root(&args.root)?;
    let root_text = root.display().to_string();
    let outside = match &args.app_dir {
        Some(dir) => Some(evani_guide::app_dir::resolve(dir, &root)?),
        None => None,
    };
    let game = match &outside {
        Some(o) => game::Game::outside(&root, o),
        None => game::Game::new(&root, &args.game),
    };
    // An outside game's default session lives in its own workspace, so it
    // never takes over the engine's Evani.dev tab session.
    let session_home = game.home().to_path_buf();
    let token = auth::new_token()?;

    // 1. Bind first, so /live/status answers before any build (2.3). With
    //    `--any-port` a taken port moves on to the next free one (2.6).
    let listener = bind_live(args.port, args.any_port).await?;
    let port = listener.local_addr().map_err(|e| format!("listener: {e}"))?.port();
    let session_path = match &args.session {
        Some(p) => {
            let p = std::path::absolute(p).map_err(|e| format!("{}: {e}", p.display()))?;
            session::write_at(&p, &root_text, &game.name, port, &token)?;
            Some(p)
        }
        None => {
            session::write_default(&session_home, &root_text, &game.name, port, &token)?;
            None
        }
    };
    // A second build of the same game gets its own folder (2.6).
    let mut game = game;
    let slot = game.claim(port);
    if !args.quiet && slot.alternate {
        eprintln!("evani-live-bridge: {} is already being served live; this build goes to {}", game.name, game.dist.display());
    }

    let mut hub = hub::Hub::new(hub::HubInfo {
        port,
        root: root_text.clone(),
        game: game.name.clone(),
        session: format!("s-{}", auth::short_id()),
    });
    if let Some(o) = &outside {
        hub = hub.with_outside(o.dir.display().to_string());
    }
    if let Some(folder) = &args.models {
        let dir = models::resolve(&root, folder);
        hub = hub.with_models(models::Workspace::new(dir, folder.display().to_string().replace('\\', "/")));
    }
    let hub = Arc::new(hub);
    // Model mode: look at the workspace every 250 ms (EVANI_MODEL_TAB.md 8.3).
    let models_task = hub.models.clone().map(|ws| tokio::spawn(watch_models(ws, hub.clone())));
    // Page hot-replace (LIVE_API.md 15): watch the app's page files and swap
    // changes into the open page; no wasm rebuild.
    // (Not for a game outside the engine: --app-dir serves its build only.)
    let page = if game.outside { None } else { page::PageHot::new(&game.app_dir, &hub.info.session).map(Arc::new) };
    let app = server::App::new(hub.clone(), token, game.dist.clone(), page.clone());
    let app = Arc::new(if game.outside { app.contained() } else { app });
    let page_task = page.map(|p| tokio::spawn(page::run(p, hub.clone(), args.quiet)));

    // The look guard's approve listener (15.7): loopback only, its own port.
    let approve_listener = tokio::net::TcpListener::bind(("127.0.0.1", args.approve_port))
        .await
        .map_err(|e| format!("cannot listen on 127.0.0.1:{} for look approvals ({e})", args.approve_port))?;
    let approve_port = approve_listener.local_addr().map_err(|e| format!("approve listener: {e}"))?.port();
    hub.lookguard.set_approve_port(approve_port);
    read_stdin_line(hub.clone(), args.quiet);
    if !args.quiet {
        eprintln!(
            "evani-live-bridge: serving {} on http://localhost:{}/?live=1 (session file {})",
            game.name,
            port,
            session_path.as_ref().map_or_else(|| session::port_path(&session_home, port), Clone::clone).display()
        );
    }

    // 2. Build the game in the background: with dx for live code swapping
    //    (10.4), else with Trunk unless a current Trunk build exists.
    let quiet = args.quiet;
    let hot = args.hot && !args.no_build && std::env::var("EVANI_LIVE_HOT").map_or(true, |v| v != "0");
    let plan = if game.outside {
        Err(format!("live code swapping {}", hub::OUTSIDE_LATER))
    } else if hot {
        code::Plan::new(&root, &game)
    } else {
        Err("turned off (--no-hot, --no-build or EVANI_LIVE_HOT=0)".to_string())
    };
    let build_task = match plan {
        Ok(plan) => {
            hub.set_build(hub::BuildState::default());
            let (hub, app) = (hub.clone(), app.clone());
            Some(tokio::spawn(async move {
                if let Err(reason) = code::run(plan, hub.clone(), app.clone(), quiet).await {
                    eprintln!("evani-live-bridge: live code swapping is off ({reason}); building with Trunk instead");
                    hub.set_code_mode(json!({ "mode": "trunk", "reason": format!("dx failed: {reason}") }));
                    app.set_dist(game.dist.clone());
                    trunk_build(game, hub, quiet, true).await;
                }
            }))
        }
        Err(reason) => {
            if !quiet && hot {
                eprintln!("evani-live-bridge: live code swapping is off: {reason}");
            }
            hub.set_code_mode(json!({ "mode": "trunk", "reason": reason }));
            if args.no_build || game.is_current() {
                hub.set_build(hub::BuildState { phase: "idle", error: None, build_ms: 0 });
                None
            } else {
                hub.set_build(hub::BuildState::default());
                let hub = hub.clone();
                Some(tokio::spawn(trunk_build(game, hub, quiet, false)))
            }
        }
    };

    let server = axum::serve(listener, server::router(app));
    let approvals = axum::serve(approve_listener, server::approve_router(hub.clone()));
    let result = tokio::select! {
        r = server => r.map_err(|e| format!("server: {e}")),
        r = approvals => r.map_err(|e| format!("approve listener: {e}")),
        _ = shutdown_signal() => Ok(()),
    };
    // Clean exit: stop a running build (kill_on_drop) and remove the session file.
    if let Some(t) = build_task {
        t.abort();
        let _ = t.await;
    }
    if let Some(t) = page_task {
        t.abort();
    }
    if let Some(t) = models_task {
        t.abort();
    }
    match &session_path {
        Some(p) => session::remove_if_ours_at(p),
        None => session::remove_default(&session_home, port),
    }
    slot.release();
    if !args.quiet {
        eprintln!("evani-live-bridge: stopped");
    }
    result
}

/// The live listener on `port`; with `any_port`, if it is taken, the next
/// free port from [`FALLBACK_PORTS`] (8097 is the Model tab's), then any.
async fn bind_live(port: u16, any_port: bool) -> Result<tokio::net::TcpListener, String> {
    let first = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
    let e = match first {
        Ok(l) => return Ok(l),
        Err(e) => e,
    };
    if !any_port {
        return Err(format!("cannot listen on 127.0.0.1:{port} ({e}); is another live server running? (`cargo xtask live` picks a free port by itself)"));
    }
    for p in FALLBACK_PORTS.filter(|p| *p != port) {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", p)).await {
            eprintln!("evani-live-bridge: port {port} is taken ({e}); using {p}");
            return Ok(l);
        }
    }
    tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e2| format!("cannot listen on 127.0.0.1:{port} ({e}) or any other port ({e2})"))
}

/// Where `--any-port` looks after the asked port: above the Model tab's 8097,
/// below the review bridges' 8180.
const FALLBACK_PORTS: std::ops::RangeInclusive<u16> = 8098..=8139;

/// Model mode's workspace watcher: one `model_version` per new complete
/// version folder, one `model_status` per new or changed request file.
async fn watch_models(ws: models::Workspace, hub: Arc<hub::Hub>) {
    let mut watcher = models::Watcher::default();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(models::WATCH_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let ws = ws.clone();
        let Ok((w, events)) = tokio::task::spawn_blocking(move || {
            let events = watcher.step(&ws);
            (watcher, events)
        })
        .await
        else {
            return;
        };
        watcher = w;
        for (event, data) in events {
            hub.model_event(event, data);
        }
    }
}

/// Build with Trunk (2.3) unless `skip_if_current` and a current build exists.
async fn trunk_build(game: game::Game, hub: Arc<hub::Hub>, quiet: bool, skip_if_current: bool) {
    if skip_if_current && game.is_current() {
        hub.set_build(hub::BuildState { phase: "idle", error: None, build_ms: 0 });
        return;
    }
    hub.set_build(hub::BuildState::default());
    let state = match game.build(quiet).await {
        Ok(ms) => {
            if !quiet {
                eprintln!("evani-live-bridge: game built in {:.1} s", ms as f64 / 1000.0);
            }
            // No news for the tab here: the holding page reloads itself.
            hub::BuildState { phase: "idle", error: None, build_ms: ms }
        }
        Err(e) => {
            eprintln!("evani-live-bridge: {e}");
            hub.dev_news(&json!({ "type": "live_status", "at": hub::now_ms(), "state": "error", "message": e }));
            hub::BuildState { phase: "error", error: Some(e), build_ms: 0 }
        }
    };
    hub.set_build(state);
}

/// Evani.App's first stdin line (15.6), read on a plain thread so a console
/// or an idle pipe never blocks the server. The line holds the approve
/// secret: it stays in memory only, never in a log, file or env var, and the
/// read buffer is zeroed afterwards.
fn read_stdin_line(hub: Arc<hub::Hub>, quiet: bool) {
    use std::io::{BufRead, Read};
    let spawned = std::thread::Builder::new().name("look-guard-stdin".into()).spawn(move || {
        let mut buf = String::new();
        let n = std::io::stdin().lock().take(lookguard::MAX_STDIN_LINE).read_line(&mut buf);
        let msg = match n {
            Ok(0) => None, // no stdin (EOF): approvals stay off
            Ok(_) => Some(hub.lookguard.on_stdin_line(&buf)),
            Err(e) => Some(format!("look guard: approvals off: stdin can't be read ({e})")),
        };
        let mut bytes = std::mem::take(&mut buf).into_bytes();
        bytes.fill(0);
        drop(bytes);
        if let Some(m) = msg
            && !quiet
        {
            eprintln!("evani-live-bridge: {m}");
        }
    });
    if let Err(e) = spawned {
        eprintln!("evani-live-bridge: look guard: approvals off: {e}");
    }
}

async fn shutdown_signal() {
    #[cfg(windows)]
    {
        let close = async {
            match tokio::signal::windows::ctrl_close() {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        let brk = async {
            match tokio::signal::windows::ctrl_break() {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = close => {}
            _ = brk => {}
        }
    }
    #[cfg(not(windows))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Result<Args, String> {
        parse_args(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn cli() {
        let a = args("serve --root C:/e --port 8096 --quiet --game apps/sandbox").unwrap();
        assert!(a.app_dir.is_none());
        let o = args("serve --root C:/e --app-dir ../game/app").unwrap();
        assert_eq!(o.app_dir.as_deref(), Some(std::path::Path::new("../game/app")));
        assert!(args("serve --app-dir ../game/app --game sandbox").err().unwrap().contains("give one"));
        assert!(args("serve --app-dir ../game/app --models .evani/model").err().unwrap().contains("not available for --app-dir yet"));
        assert_eq!(a.mode, "serve");
        assert_eq!(a.port, 8096);
        assert!(a.quiet);
        assert!(a.hot, "live code swapping is on by default");
        assert!(!args("serve --no-hot").unwrap().hot);
        assert_eq!(a.game, "apps/sandbox");
        assert_eq!(a.session, None, "the tab's session file unless told otherwise");
        let a = args("serve --port 8180 --session C:/e/.evani/live/review/8180/session.json").unwrap();
        assert_eq!(a.session, Some(PathBuf::from("C:/e/.evani/live/review/8180/session.json")));
        assert!(args("serve --session").is_err());
        assert!(!a.any_port, "a taken port is an error unless asked (the tab's launch)");
        assert!(args("serve --any-port").unwrap().any_port);
        assert_eq!(a.models, None, "model mode is off unless asked for");
        let a = args("serve --port 8097 --game apps/model-view --models .evani/model").unwrap();
        assert_eq!(a.models, Some(PathBuf::from(".evani/model")));
        assert!(args("serve --models").is_err());
        let a = args("mcp --unknown").unwrap();
        assert_eq!((a.mode.as_str(), a.port), ("mcp", 8096));
        assert!(args("").is_err());
        assert!(args("serve --port nope").is_err());
        assert!(args("serve --port").is_err());
        assert_eq!(args("serve").unwrap().approve_port, 0);
        assert_eq!(args("serve --approve-port 8099").unwrap().approve_port, 8099);
        assert!(args("serve --approve-port x").is_err());
    }
}
