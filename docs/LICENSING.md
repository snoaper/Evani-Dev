# Licensing

Evani.Dev is open source under the **MIT licence** (decided by Chris on 2026-10-28; see DECISIONS.md). The full text is the `LICENSE` file at the repository root: Copyright (c) 2026 Chris Pearson.

This is a plain-language summary, not legal advice; the licence text governs.

## What the licence covers

Everything in this repository that does not carry its own notice:

- the Evani.Dev engine and its crates (core, render, fx and the rest of the workspace);
- Evani.Model (the model format, converter and runtime);
- the tools (`cargo xtask`, the diagnostic tools such as geocheck and the look guard, the live tools and their server, the AI guidance files);
- the example game kits, looks and apps.

Every crate's `Cargo.toml` says `license = "MIT"`.

## What MIT lets you do

Use, copy, modify, merge, publish, distribute, sublicense and sell the code, for any purpose, as long as the copyright and permission notice travels with every copy or substantial portion of it. The software is provided as is, without warranty.

## Your games

Games and content you make with Evani.Dev are yours; these terms do not claim them.

- `cargo xtask new-game <id>` gives a new game a plain MIT `LICENSE` ("Copyright (c) the authors of the game `<id>`") and `license = "MIT"` in its kit and app crates. You may change the game's own terms.
- A shipped game includes the compiled engine runtime (the `.wasm` file and its JavaScript loader). MIT asks for the engine's notice to travel with it, so `cargo xtask build <id> --ship` writes `ENGINE-LICENSE.txt` (one line saying what it covers, then the root `LICENSE` verbatim) into the ship folder and fails if it is missing or incomplete. `cargo xtask package-check <folder>` and `cargo xtask publish <id>` run the same check.
- A package README carries the licence line `Game code: MIT. Includes the Evani.Dev runtime, also MIT (see ENGINE-LICENSE.txt).` `cargo xtask publish` replaces an older line that starts with `Game code: MIT.` with this one.
- Packages built before 2026-10-28 carry an older engine notice; rebuilding them with `cargo xtask build <id> --ship` or `cargo xtask publish <id>` writes the MIT notice.

## Third-party code

Code and data from other projects keep their own licences, listed with their full notices in THIRD_PARTY_NOTICES.md (for example the three.js sky dome, MIT; meshoptimizer's decoder, MIT; the bundled fonts, SIL OFL). The Blender add-on in `tools/blender-addon/` is a separate GPL-3.0-or-later program with its own `LICENSE`; engine and game crates never include or name it.

This is a plain-language summary, not legal advice; the licence text governs.
