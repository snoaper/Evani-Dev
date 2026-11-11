// Written by tools/procedural/gen_library.py from catalog/procedural/library.json.
// Do not edit: change the manifest and run the script again.

// A build whose listed assets are all built has no placeholder to add.
#[allow(unused_imports)]
use evani_procedural::{Category, Hint, Placeholder, Registry};

/// Every category this build carries (one with at least one asset on).
pub fn categories(_r: &mut Registry) {
    #[cfg(feature = "cat-geometry")]
    _r.add_category(Category { id: "geometry", name: "Core geometry and primitives" });
    #[cfg(feature = "cat-terrain")]
    _r.add_category(Category { id: "terrain", name: "Terrain and landforms" });
    #[cfg(feature = "cat-rocks")]
    _r.add_category(Category { id: "rocks", name: "Rocks, minerals and geology" });
    #[cfg(feature = "cat-vegetation")]
    _r.add_category(Category { id: "vegetation", name: "Grass, plants and ground vegetation" });
    #[cfg(feature = "cat-trees")]
    _r.add_category(Category { id: "trees", name: "Trees and forests" });
    #[cfg(feature = "cat-water")]
    _r.add_category(Category { id: "water", name: "Water and coastal assets" });
    #[cfg(feature = "cat-sky")]
    _r.add_category(Category { id: "sky", name: "Sky, weather and atmosphere" });
    #[cfg(feature = "cat-roads")]
    _r.add_category(Category { id: "roads", name: "Roads, paths and transport infrastructure" });
    #[cfg(feature = "cat-buildings")]
    _r.add_category(Category { id: "buildings", name: "Buildings and structures" });
    #[cfg(feature = "cat-architecture")]
    _r.add_category(Category { id: "architecture", name: "Architectural components" });
    #[cfg(feature = "cat-furniture")]
    _r.add_category(Category { id: "furniture", name: "Interior furniture" });
    #[cfg(feature = "cat-props")]
    _r.add_category(Category { id: "props", name: "Environmental props and clutter" });
    #[cfg(feature = "cat-industrial")]
    _r.add_category(Category { id: "industrial", name: "Industrial and factory assets" });
    #[cfg(feature = "cat-city")]
    _r.add_category(Category { id: "city", name: "City and public infrastructure" });
    #[cfg(feature = "cat-vehicles")]
    _r.add_category(Category { id: "vehicles", name: "Ground vehicles" });
    #[cfg(feature = "cat-aircraft")]
    _r.add_category(Category { id: "aircraft", name: "Aircraft and spacecraft" });
    #[cfg(feature = "cat-boats")]
    _r.add_category(Category { id: "boats", name: "Boats and marine vessels" });
    #[cfg(feature = "cat-humans")]
    _r.add_category(Category { id: "humans", name: "Humans and character anatomy" });
    #[cfg(feature = "cat-clothing")]
    _r.add_category(Category { id: "clothing", name: "Clothing and character equipment" });
    #[cfg(feature = "cat-animals")]
    _r.add_category(Category { id: "animals", name: "Animals and creatures" });
    #[cfg(feature = "cat-weapons")]
    _r.add_category(Category { id: "weapons", name: "Weapons, tools and combat equipment" });
    #[cfg(feature = "cat-agriculture")]
    _r.add_category(Category { id: "agriculture", name: "Agriculture, crops and food" });
    #[cfg(feature = "cat-textures")]
    _r.add_category(Category { id: "textures", name: "Procedural textures and materials" });
    #[cfg(feature = "cat-effects")]
    _r.add_category(Category { id: "effects", name: "Particles, effects and destruction" });
}

/// A placeholder for every listed asset not built yet (status not `done`).
pub fn placeholders(_r: &mut Registry) {
}
