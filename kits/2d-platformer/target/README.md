# Target pack: Fennel's Leap (kit 002, `2d-platformer`)

What the finished kit must look and play like, written before judging captures, so a reviewer (or the critic) can hold every capture against it. Started in the fix after Chris's review of the first build; the process tooling may add fields later.

| File | What it fixes |
|---|---|
| [reference-prompt.md](reference-prompt.md) | The reference image prompt and what a capture is compared on |
| [look-bible.md](look-bible.md) | Hex palette (the 15 ramps of `art.json`), sky and layer tints, pixel rules, 16 theme props |
| [moments.md](moments.md) | The seven signature moments, their tier and what each must show |
| [hud-layout.md](hud-layout.md) | HUD layout at 1920 × 1080 and on a portrait phone, in art pixels |

Capture rule for this look: integer-scaled from the internal resolution. At 1920 × 1080 the view is 480 × 270 art pixels at 4 device pixels per art pixel (the whole-pixel scale rule in DECISIONS.md, 2026-10-15: the smallest whole scale that shows 224 to 288 rows; 2560 × 1440 is 512 × 288 at 5); every capture is labelled with its tier and engine commit (`captures.json`).
