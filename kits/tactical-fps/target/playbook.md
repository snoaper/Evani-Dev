# Playbook items: Hexagon (tactical-fps kit)

There is no `docs/playbooks/tactical-fps.md` yet, so these items come from the genre's systems in `catalog/genres.json` and the plan (docs/HEXAGON_PLAN.md section 6). [x] met, [~] partly met, [ ] not met yet, [n/a] a deliberate difference.

## Shooting

- [x] **Hitscan weapons.** Pistol and sniper are traced rays (Hexagon's `trace`): the nearest soldier hit cylinder, concrete or metal within range.
- [x] **Recoil patterns and movement spread.** Added to pistol and sniper (the genre's first system; HEXAGON_PLAN section 2): each shot adds recoil that climbs and decays, and moving or being in the air widens the spread. Seeded, so deterministic.
- [x] **Projectiles with splash.** Rockets fly straight, grenades arc and bounce on a fuse; both blast with falloff, knockback, half self-damage and no team damage.

## Objective and rounds

- [n/a] **Plant and defuse objective.** Deliberate difference, approved by Chris (2026-10-15): Hexagon's core hold stands in for it. A team wins by holding the enemy core zone for 8 s with no defender inside; the hold decays at half speed while nobody holds it.
- [n/a] **Round economy with a buy menu.** Deliberate difference, approved by Chris (2026-10-15): Hexagon has a fixed four-weapon loadout and a single match (first to 25 kills, the core hold, or the most kills when the 15 minutes run out). No money and no buy menu.
- [x] **Clear match flow.** Title, a 6 s warm-up at the bases, the match, 9 s of results, then back to the title.

## Players

- [x] **Bots for offline play.** Four a side; attackers route level by level to the enemy core, defenders hold their base; they scan, aim with an error that settles, pick a weapon by range, sidestep craters and blast their way out when stuck.
- [~] **Server-authoritative netcode with lag compensation.** The simulation is a deterministic fixed step, ready to run as the authority; the evani-net loopback, prediction and lag compensation are kit piece K6b.

## World

- [x] **Destructible cover.** Concrete takes holes from every weapon; cover with too little left under it is removed at once. The tipping fall animation, landing craters and floor slabs that drop away are later pieces.
- [x] **No world edge.** The outer metal wall closes every view; the chasm below kills.
