# Playbook: match-3

What a match-3 game must get right, as a checklist the critic scores (rubric R8) and the builder copies into the kit's target pack (`kits/<id>/target/playbook.md`). Each item says why it matters. Mechanics only: all content is original.

## Board and layout

- [ ] **The board is the hero.** On desktop the board takes most of the height and sits near the centre; side panels are compact and there is no big empty gap between the panel and the board. Why: the board is where the whole game happens; empty space around it makes pieces small for nothing. (Chris's note on the current Match-3: a tall, mostly empty left panel and a big gap before the board. Make the board bigger and the panel compact.)
- [ ] **Nothing overlaps the frame.** Mascots, stars and decorations stay inside their panel and never cross its edges or the board. Why: overlapping art looks like a layout bug. (Chris's note: the mascot and its star overlap the panel's bottom edge.)
- [ ] **Phone layout of its own.** On a phone the board fills the width, the score, moves and goal sit in a compact strip above it, and nothing needs scrolling. Why: most match-3 play is on phones, held upright with one hand.
- [ ] **Every value has its label.** Score, moves and goals show their label ("SCORE") on every layout, phone included. Why: a bare number is ambiguous. (Chris's note: the phone score box lacks the "SCORE" label.)

## Pieces

- [ ] **One piece per cell.** A cell never shows two pieces drawn on top of each other. Why: it is either a rendering bug or a logic bug, and both break trust in the board. (Chris's note: row 4, column 1 seemed to show a ring over a triangle; check for two pieces rendered in one cell.)
- [ ] **Shape and colour both differ.** Each piece type has its own colour and its own silhouette. Why: players with colour blindness, and everyone on a small screen, tell pieces apart by shape.
- [ ] **Specials read at a glance.** Special pieces (line clears, bombs, colour clears) look clearly different from normal ones and show what they will do. Why: the player plans around them.
- [ ] **Empty cells only mid-cascade.** An empty cell may show while pieces fall, but never once the board has settled. Why: a cell that stays empty is a refill bug. (Chris's note: empty cells in a phone capture are fine mid-cascade but a bug if they stay.)

## Input and rules

- [ ] **Swap by drag or two taps.** Both work with mouse and touch, with touch targets at least as big as a fingertip on a phone. Why: players expect both, and small targets cause wrong swaps.
- [ ] **Invalid swaps bounce back.** A swap that makes no match animates there and back with a small shake. Why: the player sees the move was understood but not allowed.
- [ ] **No dead boards.** When no move is possible the board reshuffles with a short notice. Why: a board with no moves is a softlock (the playtest bot must reach a win and a lose).
- [ ] **Hint after idling.** After a few seconds without a move, a valid move gently pulses. Why: it keeps a stuck player playing without solving the board for them.

## Juice

- [ ] **Cascades are clear.** Matches clear, pieces fall with a short ease and a bounce, then new matches resolve one step at a time. Why: the player must see why points were scored.
- [ ] **Popups brief and out of the way.** Praise ("Great!") and score popups last about a second and appear away from where the player is looking: not over the falling pieces or the next likely move. Why: the player watches the cascade to plan the next move. (Chris's note: "Great!" and score popups cover falling pieces.)
- [ ] **Combos build up.** Chain bonuses grow in size and pitch with the chain length and merge into one counter. Why: rising feedback is the reward that makes cascades feel good.
- [ ] **Clear win and lose.** Reaching the goal or running out of moves shows a clear end screen with the result and a replay button. Why: the round needs a readable ending.

## Sound

- [ ] **A sound per action.** Swap, invalid swap, match, cascade step, special and end of round each have their own short sound; cascade steps rise in pitch. Why: the ear follows the chain while the eye plans the next move.
