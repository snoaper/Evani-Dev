# Signature moments

Each moment is a capture the kit can stage with `capture.frame {moment}` (wired in the art pass). Every capture shows the full HUD for its state (`hud-layout.md`).

## base_overview

- In frame: the player's command hub, a foundry and a bastion, four or more workers on the crystal field and walking back to the hub.
- Pass: team colour reads on every building; every worker's carry state is visible (crate or no crate); fog dims the unexplored edge.

## drag_select

- In frame: a selection box being dragged over a group of six or more units; selected units show their rings.
- Pass: the box outline is clear against the ground; rings sit on the ground under each unit; unselected units have none.

## group_move

- In frame: a group of ten or more units flowing along the flow field around a basalt outcrop towards a move marker.
- Pass: units spread around the obstacle without stacking; the marker is visible at the destination.

## fog_reveal

- In frame: a scout at the edge of the explored area, the circle of vision showing the map in full, explored ground dimmed behind it, black fog ahead.
- Pass: three fog states are distinct (visible, explored, unexplored).

## river_assault

- In frame: the rival's attack wave crossing a ford into the player's base, a bastion firing, health bars over damaged units.
- Pass: both team colours are distinct; shots read; health bars legible.

## hq_falls

- In frame: the rival's command hub at zero health with the "Victory" banner (or the player's with "Defeat").
- Pass: banner legible at 1080p and on a phone; the rest of the HUD stays visible.
