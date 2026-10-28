# Playbook: base-building RTS

There is no `docs/playbooks/base-building-rts.md` yet. These are the genre items this kit holds itself to, each with its reason and how the kit meets it; the lessons step (KIT_PROCESS 10) turns them into the playbook.

| Item | Why | How this kit meets it |
|---|---|---|
| Selection is instant and visible | The player must always know what they command | Click or tap selects, drag draws a box; rings under every selected unit the same frame |
| Orders confirm on the ground | A missed order is the worst RTS feeling | A coloured marker at the order target (move, attack, gather) for 0.6 s |
| Groups move without stacking | Clumps block chokes and look broken | One flow field per destination shared by the group, plus separation between units |
| Fog of war has three states | Scouting is a core decision | Unexplored (dark), explored (dim), visible (full) per grid cell |
| Economy loop is short and clear | New players learn by watching workers | Gather a load, walk to the hub, bank it; the counter ticks up on deposit |
| Build and train costs are shown before spending | Players plan by cost | Hints show costs; a refused order says "Not enough crystal" |
| AI attacks in waves the player can see coming | Surprise wipes feel unfair | The AI masses at its base, then sends the whole wave across a ford |
| Win and lose are unmistakable | The match must end cleanly | Destroying a command hub ends the match with a banner |
| Lockstep-safe simulation | RTS multiplayer depends on it | Fixed 60 Hz, seeded Pcg32, orders are the only input, entity order sorted by id |
| Phone play | Touch players need every order | Tap select, drag box, pan stick, zoom buttons, command buttons, pause |
| Clarity over spectacle (KIT_PROCESS "Choosing effects") | Strategy needs readability | No depth of field, motion blur or heavy grain; effects come with the art pass |
| Hundreds of units (ROADMAP notes) | The genre's scale | Flow fields cost one grid pass per destination, not per unit; draw is instanced built-in meshes |
