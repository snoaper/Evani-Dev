# Rows that draw after the batch 3-5 join (matcat-engine)

Counted from `catalog/materials/materials.json` on matcat-engine after the
joins of b3-patterns, b4-p2, b5-p2 and b5-p1 (status `ready`: every one
encodes to its 32-byte block and carries real numbers; the catalogue tests
hold both).

**429 of 1,054 rows draw** (624 planned, 1 alias). Before the joins
(36c616bf): 312.

| Join | Drawn after | Gain |
|---|---|---|
| before (36c616bf) | 312 | |
| b3-patterns (batch 3, 90/90; plus wood.plank.deck, cloth.denim.indigo, cloth.lace.white, wall.paper.damask, paper.newsprint in batch 1) | 401 | +89 |
| b4-p2 (leather.embossed_scale, leather.cracked_old) | 403 | +2 |
| b5-p2 (hide bristles and plates, keratin hoof, scutes, banded) | 415 | +12 |
| b5-p1 (16 painted fur coats; 2 were drawn already and were re-pointed) | 429 | +14 |

By batch (drawn / rows): 1: 101/106, 2: 97/98, 3: 90/90, 4: 5/120,
5: 22/95, 6: 11/74, 7: 31/96, 8: 17/129, 9: 4/106, 10: 15/80, 11: 36/60.

By category: animals 81/283, people 12/94, clothing 18/104,
weapons_tools 11/62, buildings 102/102, land 77/90, plants 42/98,
household 29/120, elements 21/41, vehicles 18/30, fantasy_scifi 18/30.

By recipe: fur 29/153, hair 4/34, keratin 15/40, hide 10/29, eye 1/13,
thin 9/51, feather 13/42, scales 20/28, stone 28/34, skin 6/50, metal 24/76,
paint 20/29, cloth 11/73, plastic 13/21, leather 7/18, glass 11/29,
wood 21/54, foliage 38/60, ceramic 14/25, ground 32/45, masonry 21/21,
plaster 12/12, bark 15/16, gem 11/13, ice 12/13, organic 10/23, glow 10/15,
water 3/25, fire 9/12.

Not joined yet: b5-region and b5-fur (still being built).
