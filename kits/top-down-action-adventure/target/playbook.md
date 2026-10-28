# Playbook: top-down action-adventure

No `docs/playbooks/top-down-action-adventure.md` exists yet (the lessons loop, KIT_PROCESS step 10, creates it after Chris's review). This is the genre checklist this kit is built and judged against, each item with its reason and how the kit meets it.

## Camera and rooms

- [ ] **One room per screen, whole room visible.** The camera shows the full room at once and cuts or scrolls to the next on a door. Why: puzzles and fights are planned to a screen; seeing the whole room lets the player plan. Kit: the view is the 16x11-tile room; stepping through a doorway switches room.
- [ ] **Doors read as doors.** Open doorways, locked doors, gates and walls look different from each other. Why: the player must know where to go and what blocks them without text. Kit: gaps in the wall for open doors; a brass plate on locked doors; bars on the gate.
- [ ] **Entering a room is safe.** On entering a room the player gets a short grace (no enemy touches for about half a second) and enemies start a few tiles away. Why: taking damage the moment you walk in feels unfair. Kit: `enter_grace_s` and enemy placement away from doorways.

## Combat feel

- [ ] **Sword is instant and readable.** The attack starts on the press, shows an arc, and the hit lands in the first frames. Why: a delayed swing feels broken. Kit: hitbox from the first tick of the swing, an arc drawn for `sword_time`.
- [ ] **Hits have feedback.** Hit-stop, flash, knockback and a spark on every hit, both ways. Why: the player needs to know the hit landed. Kit: `hit_stop_s`, `knockback`, a white flash and sparks in the art pass.
- [ ] **Damage is fair.** After a hit the hero is invulnerable and blinks for about a second, and enemies telegraph (the husk pauses, the spitter winds up). Why: stacked hits feel cheap. Kit: `hurt_invuln_s`, wind-up times in the data.
- [ ] **Enemies teach one at a time.** Each new enemy first appears alone in a safe room. Why: the level teaches without text. Kit: gloop first, then flit, then the husk, then the spitter, then the Warden.

## Items and progression

- [ ] **Keys and locks are legible.** The key count is on the HUD; a locked door says "needs a key" visually; a used key is consumed. Why: the loop of find-key, open-door is the core of the genre.
- [ ] **A new item opens something.** The bolt wand is needed to reach the first key: its bolts cross the pit moat and press the vault's switch. Why: progression through tools, not only keys.
- [ ] **Health is findable.** Hearts drop from enemies and pots and a heart container is a reward. Why: so a hurt player can recover without restarting.
- [ ] **Map of visited rooms.** The HUD shows the rooms seen so far. Why: backtracking for keys needs a map.

## Controls

- [ ] **Eight-way movement, four-way facing.** The hero moves in any direction at the same speed (diagonals normalised) and faces the last dominant direction. Why: it matches the grid-aligned rooms and the attack direction.
- [ ] **Touch controls mirror the keys.** Pad, attack, item and pause all set the same actions the keys do. Why: the game plays on phones with no separate path.

## Not applicable in the greybox

- Pushable block puzzles and dialogue: out of the greybox (the vault uses a floor switch and a gate instead); the art pass or a later pass adds them if Chris wants them.
