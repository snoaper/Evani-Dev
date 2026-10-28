# Signature moments

Five fixed points on the sandbox's 60-second orbit. Each is registered in `apps/sandbox/src/scene.rs` (`scene::moments`) for `capture.frame { moment }`: it sets the sim clock to its orbit time, and the scene's own camera frames it. Every spinner's pose is a pure function of sim time too, so a moment draws the same frame in every run.

Bloom is a tier choice: Low leaves it out (`assets/tiers/low.json`), so on Low the glowing pieces read as flat, bright warm-orange colour with no halo, and every bloom line below applies only on tiers that turn bloom on (Medium and up, Ultra in the review). Shadows are the same: the shadow map is on from Medium up; on Low every spinner stands on a dark shadow disc instead.

## overview

Orbit time 0 s. In frame: the whole 20 × 20 grid with a margin on every side, the green ground running to the horizon, a band of light-blue sky along the top edge, the stone terraces and columns of the middle distance, the HUD bottom left. Passes when every row and column of the grid is visible, every spinner has a shadow on the ground under it, and the glowing pieces bloom (Low: they read as flat warm orange).

## diagonal

Orbit time 7.5 s. In frame: the grid seen along its diagonal, glowing orange pieces scattered through it (not in lines). Passes when the bloom halos are visible but do not wash out the blue spinners (Low: no halos; the glowing pieces are flat warm orange and the blue spinners stay clear).

## quarter_turn

Orbit time 15 s. In frame: the grid a quarter turn round (the square grid looks much the same from each side), the horizon with a clear band of sky above it, drifting sprites over it. Passes when the horizon is a clean line of ground meeting sky (no end of the world) and the sprites draw over the 3D scene.

## far_side

Orbit time 30 s. In frame: the far side of the grid, the ground to the horizon with sky above it, the columns and hills behind. Passes when the ground fills everything below the horizon with no gap, a band of sky shows above it, and the vignette darkens the corners.

## three_quarter

Orbit time 45 s. In frame: the grid three quarters round, just before the orbit closes. Passes when bevelled cubes and ringed orbs are told apart by shape, every one rests on the ground with its shadow, and all are lit from the same side as in the other moments.
