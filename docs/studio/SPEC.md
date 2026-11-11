The library picker is not working as i expected : here is my expectation

# Evani Materials — Asset & Style Studio Redesign

Redesign the existing Evani Materials viewer into a fully integrated **Asset & Style Studio**. This is a fundamental interaction and workflow redesign, not simply a visual reskin.

## Core expectation

The user must be able to select an asset, an art style and a material, and immediately see the correct combined result in the 3D viewport.

**Example:** Selecting Horse and Voxel must display a recognizable 3D voxel horse. It must NOT display a voxel sphere, a default demonstration shape, or a smooth horse with only a pixelated shader.

Selection order must not matter.

Horse + Voxel + Bay Coat must produce the same result regardless of which item the user selects first.

## 1. Redesign the interface

Build a professional, modern, desktop-first interface with the following hierarchy.

**Top — Art Style Selector**
- Preserve all existing art styles, including Toon, Cosy, Cel, Anime, Painterly, Low-poly, Voxel, Pixel, Retro 3D and others.
- Clearly show the active style.
- Changing styles must immediately update the selected asset.

**Left — Asset Browser**
- Searchable asset library with categories such as Animals, People, Buildings, Plants, Vehicles and Props.
- Use visual thumbnails wherever possible.
- Selecting an animal must load the actual corresponding geometry.
- Preserve the existing material catalogue, categories, counts and search capabilities.

**Center — Interactive 3D Preview**
- Large 3D rendering viewport.
- Orbit, pan, zoom and reset camera.
- Automatic camera framing based on the selected asset.
- Studio, daylight, sunset and night lighting presets.
- Display the current Asset + Style + Material combination clearly.
- Support single asset preview and an explicit comparison mode.

**Right — Contextual Inspector**
- Material settings appropriate to the current style.
- Colour, roughness, metallic, bump, glow and other supported properties.
- Style-specific controls, such as voxel resolution or toon shading bands.
- Material presets and saved combinations.
- Reset, save and export actions.

**Bottom — Variations**
- Show selectable previews of the same asset using different compatible materials.
- Switching a variation must keep the same asset and art style.
- Display clearly when a variation is not compatible.

## 2. Separate asset, style and material responsibilities

An Asset determines WHAT object is being rendered.

A Style determines HOW its geometry and surface appearance are represented.

A Material determines WHAT its surfaces are made from and how they respond to lighting.

These must be independent state properties that combine through a single rendering pipeline.

Do not hard-code the currently selected demonstration sphere as the preview object.

## 3. Automatic matching

When a user clicks an asset-specific material, automatically choose a suitable preview asset if one has not already been explicitly selected.

Examples:

- Horse coat, bay → preview on a horse.
- Dog fur → preview on a dog.
- Brick wall → preview on a wall.
- Roof tiles → preview on a roof.
- Grass → preview on appropriate terrain.

An explicitly selected compatible asset should remain selected.

Never silently replace a user's deliberate asset choice without a clear reason.

## 4. Implement genuine style transformations

Art styles must affect the actual appearance of the asset, not just its shader parameters.

For example:

**Voxel Horse:** Recognizable horse constructed from visible cubic volumes, with identifiable head, body, legs, mane and tail.

**Low-poly Horse:** Recognizable horse with intentionally simplified polygonal geometry and faceted surfaces.

**Toon Horse:** Recognizable horse with smooth stylized forms, defined highlights and controlled shading.

**Pixel Horse:** Pixel-art treatment appropriate to the available rendering technology.

Each style should preserve the asset's identity while changing the representation according to the style definition.

Use actual style-specific meshes, generated geometry or an appropriate conversion pipeline.

If a required asset or conversion is unavailable, show a clearly identified unavailable state. Do not substitute unrelated sample geometry and imply the feature is working.

## 5. Preserve the user's workflow

Every selection should be persistent until deliberately changed.

Switching from Voxel to Toon must retain the horse and its selected coat material wherever compatible.

Switching from Horse to Cow must preserve the selected style and select a sensible compatible material when necessary.

Returning to a previously visited combination should restore its settings.

Avoid unnecessary confirmation dialogs, page transitions or resets.

## 6. Performance and rendering

Design for responsive browser-based 3D rendering, including WebGPU where supported.

Reuse geometry and materials where appropriate, cache generated style variants, and provide progressive loading for expensive conversions.

Use quality settings without changing the underlying selected asset.

Keep UI interactions responsive and provide clear loading states.

## 7. Acceptance tests

The implementation must demonstrate these exact scenarios:

1. Select Voxel, then Horse. A voxel horse appears.
2. Select Horse, then Voxel. The same type of voxel horse appears.
3. Select Horse coat, bay, then Voxel. A correctly coloured voxel horse appears.
4. Change Voxel to Toon. The horse remains, and its visual style changes.
5. Change Horse to Cow. A cow appears with the currently selected style.
6. Change the material. The asset remains unchanged while its material updates.
7. Change lighting. Only the lighting changes.
8. Switch between previously selected combinations. Their settings are preserved.

## 8. Definition of done

This work is complete only when the actual connected 3D viewport behaves as described.

A polished UI with decorative thumbnails, nonfunctional buttons, or a sphere that never changes shape does not meet the requirement.

Inspect the existing code and rendering architecture before implementation. Reuse the existing catalogue, materials, style definitions and engine functionality wherever possible.

Build the real interaction model and verify it through working 3D previews, not mock data.

**The guiding principle: The asset is the subject, the style is the artistic interpretation, and the material is the surface treatment. The viewport must always reflect their combined result.**
