# evani-host

A reference game server for Evani.Dev multiplayer, for any host that runs its own server (and for our tests). Node 22+, no dependencies; a dev and test tool, never part of a game's download.

```sh
node tools/evani-host/server.mjs --referee referee.wasm --port 8111 --static <shipped game folder> --laps 3
# open: http://127.0.0.1:8111/index.html?evani-server=ws://127.0.0.1:8111/
#       (&evani-role=spectator for a watcher)
```

- Runs the game's shipped headless referee (`evani-referee/1`) as-is, stepping it every 50 ms (`step(3)`).
- Talks the general web mode of `evani-net/1` (`crates/evani-net/web/port.js`): JSON over one WebSocket, every message under 4 KB, bytes as base64 split into parts at 3,000 characters. A page with no `evani-server` (or one that can't connect) is the single-player game.
- A closed socket is a leave (the AI takes the kart); a new socket is a new join. No accounts, tokens or idle kick.
- `GET /stats`: bytes and messages per peer, the largest message each way, the referee's tick and hash.
- `browser-run.mjs`: the browser proof (two players, a spectator and a page with a dead address, in headless, muted Chrome).

The page's server address can also come from page config: set `globalThis.EVANI_SERVER = "wss://..."` (and `EVANI_ROLE`) before the game's script loads.
