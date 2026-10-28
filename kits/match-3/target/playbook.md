# Playbook items (from `docs/playbooks/match-3.md`)

[x] met, [~] partly met (what is missing), [ ] not met. Each line says how this kit meets the item and where to see it.

## Board and layout

- [x] **The board is the hero.** Desktop 1920x1080: 96 px cells (768 px board, 71 % of the height), the panel only as tall as its content (420 x 458 px: 13.6 HUD units plus 50 px, a line fitted to the measured content, which needs 454 px with the banner showing) and a fixed 81 px gap; panel and board are centred as one group (`layout.rs`, test `desktop_board_is_the_hero_with_a_compact_panel_beside_it`). The cell is capped at 96 px because the 64 px atlas shapes soften above it (SPEC section 6); a bigger board needs larger atlas art (core request). Taste on the exact proportions is the critic's.
- [x] **Nothing overlaps the frame.** Puff and his star have their own room under the panel (reach 1.6 x his size, `MASCOT_REACH`), never over the panel or the board (tests `puff_stays_within_his_reach`, the layout tests; geocheck `exclusive` group "screen": 0 issues). The ring over a triangle Chris saw was a Pop Bomb's marker drawn on top of its piece, which read as a second piece in the cell (critic round 1, must-fix 1); the marker is now a thin halo ring behind the piece, just outside its outline, so the triangle keeps its whole shape. Pieces dropping in stay inside the board's top edge (test `entering_pieces_never_cross_the_board_top`), and blast rings are clipped to the board.
- [x] **Phone layout of its own.** Portrait puts a compact HUD strip on top, the board full width under it (42 px cells at 390x844) and Puff under the board; nothing scrolls (`#ui` is fixed, `overflow: hidden`).
- [x] **Every value has its label.** MOVES and SCORE carry their labels on desktop and on the phone (the phone SCORE label is shown, compact at 0.45 u); goals show the piece's shape icon and have/need.

## Pieces

- [x] **One piece per cell.** The board holds one piece per cell by construction; the geocheck's `exclusive` group "pieces" finds no two pieces overlapping in any snapshot (desktop and phone, title, board, cascade, specials, won, lost). Pop Bomb halos (behind their piece) and blast rings are effects, not pieces.
- [x] **Shape and colour both differ.** Cherry circle, toffee square, lemon star, mint diamond, berry triangle, grape ring; the review dump's colours are all at least 20 apart (item 20).
- [x] **Specials read at a glance.** Streak: white band with arrows along its line; Pop Bomb: a thin halo ring behind the piece with a glowing fuse spark, the piece's own shape left whole; Prism: dark orb with six orbiting colour beads and a hot core (`draw.rs` `piece`).
- [x] **Empty cells only mid-cascade.** Pieces dropping in fade in at the board's top edge; at rest every playable cell is filled (the geocheck dump asserts it on every settled snapshot, and the lose moment shows a full board).

## Input and rules

- [x] **Swap by drag or two taps.** Drag more than 0.35 of a cell or tap two neighbours, mouse, pen and touch on one code path; keyboard and pad cursor too. Phone cells are 42 px.
- [x] **Invalid swaps bounce back.** The piece slides `invalid_slide` (0.45) of the way, bounces back with a head-shake and costs no move; its own sound.
- [x] **No dead boards.** With no valid move the board reshuffles (pieces fly to new cells over 0.6 s, with the shuffle sound), so no softlock (the playtest bots reach a win and a loss), and a "Shuffle!" callout shows in the callout slot off the board (with or without juice: it explains why every piece moved). The kit's `shuffle` capture moment stages a board with no move left and shoots the reshuffle with its callout (test `every_moment_stages_its_shot`).
- [x] **Hint after idling.** After `hint_delay_s` (5 s) the best move's two pieces wobble towards each other.

## Juice

- [x] **Cascades are clear.** Pop and spin over 0.24 s, pieces fall under gravity with a stretch and land with a squash, and each cascade round resolves after the previous has landed.
- [x] **Popups brief and out of the way.** Score texts live only until the pieces start to fall and merge when close; combo callouts last 0.8 s and sit off the board (under the panel on desktop, between the HUD and the board on a phone). The geocheck popups check (UI text rule) reports 0 issues over 422 sampled popups.
- [x] **Combos build up.** Callouts climb Nice!, Great!, Superb!, Fantastic!, Unstoppable!, the big ones larger and pink; pop sounds rise in pitch per cascade; score texts merge into one.
- [x] **Clear win and lose.** Win: Sugar Rush, then "Level complete!" with 1 to 3 stars, score, Next level, Play again, Title screen. Lose: "Out of moves", what was missing, Play again, Title screen; Puff is sad.

## Sound

- [x] **A sound per action.** Synthesised at start (no downloads): select, swap, invalid, pops pitched up per cascade, streak, bomb, prism, land, shuffle, star, win, lose.
