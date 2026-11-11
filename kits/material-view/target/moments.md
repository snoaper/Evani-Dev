# Signature moments

Each moment is a `capture.frame {moment}` id (kit
`kits/evani-kit-material-view/src/moments.rs`, registered by the app in
`live` builds). The setup opens the scene on its own default material, sets
the light preset, stops the turntable at its start and puts the scene's home
camera back, so a moment is the same frame whatever was open before; every
streamed model is asked for when the page starts, so no moment catches a
placeholder. The showcase takes each as a full-page shot (canvas plus the
DOM HUD) at 1920x1080 on Low and Ultra. One more moment, `spin` (the shader
ball on the turntable), starts the run video; it is not a target-pack moment.

In every moment: the subject sits on the floor or its stand with a contact
shadow, the floor runs to the horizon (no edge of the world, no sky below
the horizon), the light comes from the upper right, the HUD is legible and
covers no part of the subject, and no debug overlay shows.

## styles

- In frame: the art styles scene with the kit's own look (style 5, Painterly 3D: stone, metal, leather, fabric, grass), every material side by side on spheres, the selection disc under the first.
- HUD: the style tabs with style 5 selected, its header (name, a few original words about the style, "Look: stylizedpbr"), the material and shape chips; the library drawer and the tuning panel open on desktop.
- Pass: five distinct finishes read at a glance (matte stone, bright metal, warm leather, soft fabric, grass green); rim light on each sphere's shadow side; shadows on the sand.

## ball

- In frame: the shader ball under clear-coat paint, close enough that the coat's sharp highlight shows over the rougher red base.
- HUD: "Clear-coat paint" in the HUD; the panel shows the clear-coat strength and quality.
- Pass: a crisp second highlight over a softer base; no dark dot at the top of the ball (critic round 1); the ball rests on the floor with its shadow.

## cushion

- In frame: the velvet pouf, its sheen bright at the grazing edges and darker facing the camera.
- HUD: "Velvet"; the panel shows sheen and weave.
- Pass: the sheen reads as cloth (bright rim, soft middle), the weave shows on the top, the pouf rests on the floor.

## head

- In frame: the whole bust on its stone plinth from the front, plinth and shadow clear of the bar: skin with soft scattering at the nose and ears, eyes with a catchlight, the hair.
- HUD: "Soft skin"; the panel shows the skin's wrap, warmth and detail.
- Pass: the head stands on its plinth (never floating), the plinth is whole and in the open view at every size, the skin is warm and soft, both eyes show a catchlight.

## head_close

- In frame: the face close, from the crown of the hair to the chin (the plinth is out of frame on purpose): skin, wet eyes with a catchlight, brows, the hairline.
- HUD: "Soft skin"; the panel shows the skin's wrap, warmth and detail.
- Pass: the face holds up at close range on Low and Ultra: a soft hairline, whole brows, lips blended into the skin. This is the review's hero close-up.

## ground

- In frame: the ground patch under the sky: cobbles in front, the plank bench and the brick pillar, the tiled plinth behind.
- HUD: "Cobbles"; the panel shows the relief settings.
- Pass: the cobbles' relief and gaps read, the bricks and planks keep their pattern, the bench and the pillar cast shadows.

## pool

- In frame: the garden pool with clear water in its walled basin, the bottle on its table and the window frame, the hedge and leaves behind, on the lawn.
- HUD: "Clear pool water"; the panel shows the water's colour, clarity and waves.
- Pass: the water sits inside the basin just below the wall top (never floating over an open edge), the bed shows through it, the glass reads as glass.

## lamp

- In frame: the lamp corner under the night lamp preset: the lamp's shade glowing warm, the back-lit leaves, the pool in the dark.
- HUD: "Lamp glow" with "Night lamp".
- Pass: the shade glows and lights its surroundings, the scene is dark but readable, bloom only on the lamp.
