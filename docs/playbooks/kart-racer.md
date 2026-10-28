# Playbook: kart racer

What a kart racer must get right, as a checklist the critic scores (rubric R8) and the builder copies into the kit's target pack (`kits/<id>/target/playbook.md`). Each item says why it matters. Mechanics only: all content is original.

## Start

- [ ] **Countdown lights 3-2-1-GO.** Three lights or numbers, then GO, about a second apart, with the karts held until GO. Why: every racer starts on the same beat and the player knows exactly when to go.
- [ ] **Start boost.** Pressing accelerate in a short window around GO gives a small boost; pressing too early gives a stall or spin. Why: it rewards attention from the first second and makes the start a skill.

## Reading the race

- [ ] **Distinct rival colours.** Every kart has its own colour, far enough from the others to tell apart at a glance (colour distance of at least 20). Why: the player tracks rivals by colour at speed.
- [ ] **None near the player's colour.** No rival is close to the player's colour (distance of at least 40). Why: the player must never mistake a rival for themselves.
- [ ] **The player is marked.** An arrow, outline, name tag or minimap dot marks the player's kart in every view, including split-screen. Why: in a pack of karts, the player must find themselves instantly.
- [ ] **HUD always visible.** Position, lap, item slot and speed are on screen and readable, including during the countdown, on the results screen when relevant and on a phone. Why: these are the race's state; hiding them in the countdown leaves the player guessing at the start.

## World

- [ ] **No visible world edge.** No camera shows the edge of the ground or an empty void: the race cameras, the title-screen orbit, menus, replays and the results screen. Why: one glimpse of the end of the world breaks the illusion (the review's world-edge check renders every scripted camera).
- [ ] **Middle distance filled.** Between the track and the backdrop there are trees, hills, buildings, crowds or water, not an empty plane. Why: the middle distance gives the sense of speed and place.
- [ ] **Track edges read at speed.** Kerbs, barriers or colour changes show where the track ends, well ahead of the kart. Why: players steer by the edges they see ahead.
- [ ] **Grounded karts with contact shadows.** Every kart, prop and pickup sits on the ground and has a contact shadow under it. A pickup that hovers on purpose (a floating item box reads from far away) is tagged as intentional in the review dump with its reason, and still has a contact shadow under it. Why: without one, objects look pasted on and the player cannot judge jumps or height.

## Driving feel

- [ ] **Drift and boost.** Holding a drift through a corner builds a visible charge (sparks changing colour) that releases as a boost. Why: it is the genre's core skill and its main reward loop.
- [ ] **Items with readable threat.** Each item has a distinct icon and a warning when one is coming at the player. Why: a hit that could not be seen coming feels unfair.
- [ ] **Catch-up without cheating.** Back-of-the-pack karts get better items; rivals never teleport or rubber-band visibly. Why: races stay close without the player feeling robbed.

## Performance

- [ ] **Low never costs more than higher tiers.** Each tier up costs more GPU time than the one below. Why: a Low tier that is slower than High means a broken setting, and players on weak devices get the worst of both (checked by item 11).

## Sound

- [ ] **Engine pitch follows speed.** The player's engine sound rises in pitch with speed and drops when braking or off the throttle. Why: speed is heard before it is read off the speedometer.
- [ ] **Distinct countdown beeps.** The 3, 2, 1 beeps share one pitch and GO is higher and longer. Why: players time the start boost by ear.
- [ ] **Drift and boost cues.** A tyre squeal while drifting that changes as the charge level rises, and a burst when the boost fires. Why: the player can time the boost without looking at the kart.
- [ ] **Surface sounds.** Road, grass, dirt and water each sound different under the wheels. Why: going off the track is heard at once.
- [ ] **Warning sound for threats.** An incoming item or hazard plays a cue that rises as it nears. Why: it warns of what is behind the camera.
- [ ] **Mix puts the player first.** The player's kart is loudest; rivals' engines are quieter and placed by direction; music ducks under important cues and can lift in tempo on the final lap. Why: a flat mix of eight engines is noise.
- [ ] **Small and controllable.** Short loops or synthesised sounds to keep the download small; separate music and effects volume in settings; sound stops when the tab is hidden. Why: the download budget and the player's control both matter.
