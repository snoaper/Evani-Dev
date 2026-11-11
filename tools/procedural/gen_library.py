"""Write evani-procedural-lib's Cargo features and placeholder table from
catalog/procedural/library.json (the manifest).

Usage (from the workspace root):
    python -I tools/procedural/gen_library.py

Writes, between their BEGIN/END generated markers:
  * crates/evani-procedural-lib/Cargo.toml: one feature per category (all
    its assets) and one per asset (its category's internal `cat-<id>`
    feature plus the manifest's `cargo` entries);
  * crates/evani-procedural-lib/src/generated.rs: the categories, and a
    placeholder registration for every asset whose status is not `done` or `basic`;
  * catalog/procedural/references.json: the realism pass's reference
    pictures, scores and notes (each asset's `realism` entry), which the
    material viewer's Procedural tab shows next to the live render (dev data:
    only the viewer reads it, never a game).
An asset marked `done` or `basic` must be registered by hand in its category module
(the crate's tests check that the registry and the manifest agree).
"""
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MANIFEST = os.path.join(ROOT, 'catalog', 'procedural', 'library.json')
CARGO = os.path.join(ROOT, 'crates', 'evani-procedural-lib', 'Cargo.toml')
GENERATED = os.path.join(ROOT, 'crates', 'evani-procedural-lib', 'src', 'generated.rs')
REFERENCES = os.path.join(ROOT, 'catalog', 'procedural', 'references.json')

# A placeholder's colour per category (linear RGB) and its default shape.
CATEGORY = {
    "geometry": ([0.55, 0.55, 0.6], "Block"), "terrain": ([0.22, 0.3, 0.12], "Slab"),
    "rocks": ([0.32, 0.3, 0.28], "Blob"), "vegetation": ([0.12, 0.3, 0.06], "Blob"),
    "trees": ([0.1, 0.25, 0.06], "Tree"), "water": ([0.05, 0.18, 0.3], "Slab"),
    "sky": ([0.6, 0.7, 0.85], "Blob"), "roads": ([0.18, 0.17, 0.16], "Slab"),
    "buildings": ([0.6, 0.45, 0.32], "Block"), "architecture": ([0.62, 0.58, 0.5], "Block"),
    "furniture": ([0.4, 0.24, 0.12], "Block"), "props": ([0.45, 0.32, 0.18], "Block"),
    "industrial": ([0.5, 0.42, 0.1], "Block"), "city": ([0.35, 0.36, 0.38], "Post"),
    "vehicles": ([0.55, 0.08, 0.06], "Long"), "aircraft": ([0.7, 0.72, 0.75], "Long"),
    "boats": ([0.75, 0.7, 0.6], "Long"), "humans": ([0.7, 0.48, 0.36], "Post"),
    "clothing": ([0.2, 0.25, 0.45], "Block"), "animals": ([0.45, 0.3, 0.18], "Long"),
    "weapons": ([0.4, 0.4, 0.42], "Long"), "agriculture": ([0.6, 0.5, 0.15], "Blob"),
    "textures": ([0.5, 0.5, 0.5], "Ball"), "effects": ([0.9, 0.45, 0.1], "Scatter"),
}

# Name keywords to a shape (first match wins), before the category default.
KEYWORDS = [
    ("Ring", ["torus", "ring", "disc", "roundabout", "wheel"]),
    ("Tree", ["tree", "sapling", "palm", "orchard", "forest"]),
    ("Cone", ["cone", "pyramid", "mountain", "roof", "crystal", "stalag", "spear", "rocket", "shuttle"]),
    ("Ball", ["sphere", "eye", "head", "fruit", "pumpkin", "bubble", "droplet", "moon", "balloon"]),
    ("Scatter", ["particle", "spark", "ember", "fragment", "rain", "snow", "hail", "insect", "broken", "scattered", "grain", "pebble", "scree", "explosion"]),
    ("Post", ["cylinder", "capsule", "pole", "light", "barrel", "tank", "silo", "column", "chimney", "pipe", "hydrant", "bollard", "can", "bottle", "lantern", "stump", "pot", "tower", "meter", "valve", "pump", "motor", "stool", "body"]),
    ("Slab", ["plane", "terrain", "road", "floor", "pavement", "track", "boardwalk", "grass", "bed", "crop", "dune", "beach", "lake", "pond", "puddle", "decal", "footprint", "papers", "kerb", "foundation", "marsh", "pool", "channel", "grate", "cover", "shoreline", "valley", "plateau", "flat"]),
    ("Long", ["sword", "knife", "rifle", "gun", "cannon", "bridge", "bench", "sofa", "conveyor", "crane", "beam", "train", "belt", "arrow", "bow", "fish", "shark", "whale", "trail", "wedge", "ramp", "spline", "tube", "fallen", "log"]),
    ("Blob", ["rock", "stone", "boulder", "bush", "shrub", "cloud", "smoke", "dust", "mist", "fog", "steam", "hay", "moss", "rubble", "pile", "ore", "sack", "bread", "potato", "vegetable", "fire", "crater", "island", "cave"]),
]


# Statuses with a real generator (no placeholder): `done` (built or wrapped)
# and `basic` (batch A's simplest real version, "placeholder->basic").
BUILT = ('done', 'basic')


def hint(cat, name):
    n = name.lower()
    for shape, words in KEYWORDS:
        if any(w in n for w in words):
            return shape
    return CATEGORY[cat][1]


def rust_str(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'


def replace_block(text, begin, end, body):
    pat = re.compile(re.escape(begin) + r'.*?' + re.escape(end), re.S)
    if not pat.search(text):
        sys.exit(f"markers {begin!r} .. {end!r} not found")
    return pat.sub(lambda _: begin + '\n' + body + end, text)


def main():
    with open(MANIFEST, encoding='utf-8') as f:
        lib = json.load(f)
    feats = []
    for c in lib['categories']:
        ids = [a['feature'] for a in c['assets']]
        feats.append(f"# {c['number']}. {c['name']} ({len(ids)} assets)")
        feats.append(f"{c['feature']} = [{', '.join(json.dumps(i) for i in ids)}]")
        feats.append(f"cat-{c['id']} = []")
        for a in c['assets']:
            extra = [f"cat-{c['id']}"] + a.get('cargo', [])
            feats.append(f"{a['feature']} = [{', '.join(json.dumps(x) for x in extra)}]")
        feats.append('')
    with open(CARGO, encoding='utf-8') as f:
        cargo = f.read()
    cargo = replace_block(cargo, '# BEGIN generated features (tools/procedural/gen_library.py)', '# END generated features', '\n'.join(feats))
    with open(CARGO, 'w', encoding='utf-8', newline='\n') as f:
        f.write(cargo)

    out = ["// Written by tools/procedural/gen_library.py from catalog/procedural/library.json.",
           "// Do not edit: change the manifest and run the script again.",
           "",
           "// A build whose listed assets are all built has no placeholder to add.",
           "#[allow(unused_imports)]",
           "use evani_procedural::{Category, Hint, Placeholder, Registry};",
           "",
           "/// Every category this build carries (one with at least one asset on).",
           "pub fn categories(_r: &mut Registry) {"]
    for c in lib['categories']:
        out.append(f'    #[cfg(feature = "cat-{c["id"]}")]')
        out.append(f'    _r.add_category(Category {{ id: {rust_str(c["id"])}, name: {rust_str(c["name"])} }});')
    out.append("}")
    out.append("")
    out.append("/// A placeholder for every listed asset not built yet (status not `done`).")
    out.append("pub fn placeholders(_r: &mut Registry) {")
    n = 0
    for c in lib['categories']:
        col = CATEGORY[c['id']][0]
        for a in c['assets']:
            if a.get('status') in BUILT:
                continue
            n += 1
            out.append(f'    #[cfg(feature = "{a["feature"]}")]')
            out.append(f'    _r.add(Box::new(Placeholder::new({rust_str(c["id"])}, {rust_str(a["id"])}, {rust_str(a["name"])}, Hint::{hint(c["id"], a["name"])}, [{col[0]!r}, {col[1]!r}, {col[2]!r}])));')
    out.append("}")
    out.append("")
    with open(GENERATED, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(out))
    refs = {}
    for c in lib['categories']:
        for a in c['assets']:
            r = a.get('realism')
            if r:
                refs[a['feature']] = {k: r[k] for k in ('reference', 'score', 'status', 'note', 'camera', 'studio') if k in r}
    with open(REFERENCES, 'w', encoding='utf-8', newline='\n') as f:
        json.dump({"about": "Realism pass references, written by tools/procedural/gen_library.py from library.json (each asset's realism entry). Dev data for the material viewer only.", "assets": refs}, f, indent=1, ensure_ascii=False)
        f.write('\n')
    done = sum(1 for c in lib['categories'] for a in c['assets'] if a.get('status') in BUILT)
    print(f"{len(lib['categories'])} categories, {n} placeholders, {done} done -> Cargo.toml, src/generated.rs")


if __name__ == '__main__':
    main()
