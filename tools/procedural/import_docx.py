"""Import the procedural asset list (a .docx) into catalog/procedural/library.json.

Usage (run with python -I; the .docx is untrusted data, unpacked elsewhere):
    python -I tools/procedural/import_docx.py <unpacked-docx-dir> catalog/procedural/library.json

The .docx is a zip: unpack word/document.xml and word/_rels/document.xml.rels
into <unpacked-docx-dir> first. Fields already recorded per asset in the
output file (status, path, cargo, wraps, item, notes, inspired_by, flags)
are kept, so re-importing never loses work. Then run
tools/procedural/gen_library.py.
"""
import json
import os
import re
import sys
import xml.etree.ElementTree as ET

W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
R = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'

# Short ids for the document's 24 categories, in its order.
CATEGORY_IDS = ["geometry", "terrain", "rocks", "vegetation", "trees", "water", "sky", "roads",
                "buildings", "architecture", "furniture", "props", "industrial", "city", "vehicles",
                "aircraft", "boats", "humans", "clothing", "animals", "weapons", "agriculture",
                "textures", "effects"]

# Licences the document states, by source (matched on the link).
STATED = [
    ("askalice.github.io/threejs-worldgen", "Apache-2.0"),
    ("github.com/askalice/threejs-worldgen", "Apache-2.0"),
    ("github.com/SkyeShark/SeedThree", "MIT"),
    ("github.com/IceCreamYou/THREE.Terrain", "MIT"),
    ("github.com/majidmanzarpour/threejs-procedural-animals", "MIT"),
    ("github.com/jonathandasilvasantos/2026-human-body-generator", "MIT"),
    ("github.com/danielsobrado/drusniel-procedural-textures", "Apache-2.0"),
]
# Licences that are well known but not stated in the document.
KNOWN = [("threejs.org", "MIT")]

# Document text that is not a category note.
SKIP = ("GitHub", "GitHub+3", "askalice.github.io", "Assets mapped to sources", "Categories covered")
CLOSING = ("What this means", "The key opportunity", "I'd start", "For every source", "The end goal")


def clean(url):
    return re.sub(r'[?&]utm_source=chatgpt\.com$', '', url or '')


def slug(name):
    s = name.lower().replace('&', ' and ')
    return re.sub(r'[^a-z0-9]+', '-', s).strip('-')


def paragraphs(d):
    rels = {r.get('Id'): r.get('Target') for r in ET.parse(os.path.join(d, 'word/_rels/document.xml.rels')).getroot()}
    body = ET.parse(os.path.join(d, 'word/document.xml')).getroot().find(W + 'body')
    for p in body.iter(W + 'p'):
        text, links = '', []
        for ch in p:
            if ch.tag == W + 'r':
                text += ''.join(t.text or '' for t in ch.iter(W + 't'))
            elif ch.tag == W + 'hyperlink':
                t = ''.join(x.text or '' for x in ch.iter(W + 't'))
                links.append((t, clean(rels.get(ch.get(R + 'id'), ''))))
                text += t
        listed = p.find(W + 'pPr/' + W + 'numPr') is not None
        if text.strip():
            yield text.strip(), links, listed


def main(src, out):
    old = {}
    if os.path.exists(out):
        with open(out, encoding='utf-8') as f:
            for c in json.load(f)['categories']:
                for a in c['assets']:
                    old[a['feature']] = a
    cats, cur = [], None
    for text, links, listed in paragraphs(src):
        m = re.match(r'^(\d+)\.\s+(.+)$', text)
        if m and not listed:
            n = int(m.group(1))
            cid = CATEGORY_IDS[n - 1]
            cur = {"id": cid, "number": n, "name": m.group(2), "feature": cid, "notes": [], "assets": []}
            cats.append(cur)
            continue
        if cur is None:
            continue
        if listed:
            parts = [p.strip() for p in text.split(' — ')]
            name = parts[0]
            kind = parts[-1] if len(parts) >= 3 else ''
            label, link = links[0] if links else ('', '')
            a_slug = slug(name)
            feature = f"{cur['id']}-{a_slug}"
            lic = next((l for k, l in STATED if k in link), None)
            known = None if lic else next((l for k, l in KNOWN if k in link), None)
            flags = []
            if not link:
                flags.append("no-link")
            if not kind:
                flags.append("unclear")
            asset = {
                "id": a_slug, "name": name, "feature": feature,
                "param_prefix": f"procedural.{cur['id']}.{a_slug.replace('-', '_')}",
                "source": {"label": label, "url": link, "type": kind},
                "licence": lic, "licence_known": known,
                "status": "placeholder", "path": "cpu",
                # Style coverage: the styles built natively; every other
                # style of the 24 falls back to Cinematic Realism (WK24).
                "styles": {"native": ["WK24"], "fallback": "WK24"},
            }
            if flags:
                asset["flags"] = flags
            prev = old.get(feature)
            if prev:
                for k in ("status", "path", "cargo", "wraps", "item", "notes", "inspired_by", "flags", "styles"):
                    if k in prev:
                        asset[k] = prev[k]
            cur["assets"].append(asset)
        elif text not in SKIP and not text.startswith(CLOSING) and not re.match(r'^\d+$', text):
            cur["notes"].append(text)
    total = sum(len(c["assets"]) for c in cats)
    doc = {
        "library": "Evani.Dev procedural library",
        "version": 1,
        "about": "Every procedural asset the library plans, by category, with the source each one is modelled on (reading material for maths and structure only: no code is copied). Imported from the owner's list (\"Each asset has a clickable source reference next to it\", 2026-11-03) by tools/procedural/import_docx.py; tools/procedural/gen_library.py writes crates/evani-procedural-lib's Cargo features and placeholder table from this file. status: todo (not built), placeholder (a simple stand-in made from its name), done (a real generator or a wrapped engine module). path: cpu (a mesh made on the CPU) or gpu (an engine module that draws itself). styles: the art styles (WK01..WK24, the material library's style bar) an asset builds natively; every other style falls back to Cinematic Realism (WK24), the style every asset is built for first. A game lists categories or assets in evani.toml: [modules] procedural = [\"terrain\", \"geometry-cube\"]. docs/PROCEDURAL.md.",
        "licence_note": "licence is what the source document states; licence_known is a well-known licence the document does not state (three.js is MIT). Only MIT sources are named as inspiration for a built asset (inspired_by).",
        "count": total,
        "categories": cats,
    }
    with open(out, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write('\n')
    print(f"{len(cats)} categories, {total} assets -> {out}")
    for c in cats:
        print(f"  {c['number']:2} {c['id']:13} {len(c['assets']):3}  {c['name']}")


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
