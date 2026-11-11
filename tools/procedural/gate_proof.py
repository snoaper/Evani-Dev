"""The procedural library's gating proof (docs/PROCEDURAL.md "Proof of gating").

Builds examples/proc-field to release wasm (the engine's ship profile: opt
z, LTO, stripped) three ways, no procedural asset, one asset, one category,
then reports each wasm's raw and gzip size and which assets' marker strings
(their display names, compiled in only with their code) are inside.

    python -I tools/procedural/gate_proof.py <target-dir> <out.json>
    (GATE_ONLY=none,"one batch-C" builds only those variants)

Prints progress for every build so a long run is never silent.
"""
import gzip
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
EX = os.path.join(ROOT, 'examples', 'proc-field')
VARIANTS = [
    ("none", []),
    ("one asset: terrain-rolling-hills", ["procedural", "procedural-terrain-rolling-hills"]),
    ("one category: geometry (15 assets)", ["procedural", "procedural-geometry"]),
    ("one batch-A asset: rocks-medium-rocks", ["procedural", "procedural-rocks-medium-rocks"]),
    ("one batch-A category: trees (15 basic assets)", ["procedural", "procedural-trees"]),
    ("one batch-B asset: buildings-castles", ["procedural", "procedural-buildings-castles"]),
    ("one batch-B category: city (15 basic assets)", ["procedural", "procedural-city"]),
    ("one batch-C asset: vehicles-sedans", ["procedural", "procedural-vehicles-sedans"]),
    ("one batch-C category: animals (15 basic assets)", ["procedural", "procedural-animals"]),
]
# GATE_ONLY=none,one batch-C: build only the variants whose label starts so.
if os.environ.get("GATE_ONLY"):
    VARIANTS = [v for v in VARIANTS if any(v[0].startswith(k) for k in os.environ["GATE_ONLY"].split(","))]
# Marker strings: asset names (in an asset's AssetInfo, so in the wasm only
# with that asset's code) and the module id.
lib = json.load(open(os.path.join(ROOT, 'catalog', 'procedural', 'library.json'), encoding='utf-8'))
NAMES = {a['feature']: a['name'] for c in lib['categories'] for a in c['assets']}
OTHER = ["evani.module.procedural", "procedural.terrain", "Size of the stand-in"]


def main(target, out):
    rows = []
    for label, feats in VARIANTS:
        t0 = time.time()
        print(f"[gate] building {label} ...", flush=True)
        cmd = ["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "-p", "evani-proc-field", "--no-default-features"]
        if feats:
            cmd += ["--features", ",".join(feats)]
        env = dict(os.environ, CARGO_TARGET_DIR=target)
        p = subprocess.Popen(cmd, cwd=EX, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        for line in p.stdout:
            line = line.rstrip()
            if line.strip().startswith(("Compiling", "Finished", "error", "warning: unused")) or "error" in line:
                print("[gate]   " + line.strip()[:160], flush=True)
        if p.wait() != 0:
            sys.exit(f"build failed: {label}")
        wasm = os.path.join(target, "wasm32-unknown-unknown", "release", "evani-proc-field.wasm")
        data = open(wasm, "rb").read()
        found = sorted(f for f, n in NAMES.items() if len(n) > 4 and n.encode() in data)
        rows.append({"build": label, "features": feats, "wasm_bytes": len(data), "wasm_gzip_bytes": len(gzip.compress(data, 9)),
                     "assets_found_by_name": found, "markers": {m: (m.encode() in data) for m in OTHER}, "seconds": round(time.time() - t0)})
        print(f"[gate] {label}: {len(data)} B wasm, {rows[-1]['wasm_gzip_bytes']} B gzip, assets in it: {len(found)} {found[:16]}", flush=True)
    base = rows[0]
    for r in rows:
        r["added_bytes"] = r["wasm_bytes"] - base["wasm_bytes"]
        r["added_gzip_bytes"] = r["wasm_gzip_bytes"] - base["wasm_gzip_bytes"]
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(rows, f, indent=1)
        f.write("\n")
    for r in rows:
        print(f"[gate] {r['build']:40} {r['wasm_bytes']:9} B  +{r['added_bytes']:7} B  gzip {r['wasm_gzip_bytes']:8} B  +{r['added_gzip_bytes']:6} B  assets {len(r['assets_found_by_name'])}", flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
