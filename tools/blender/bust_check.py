"""Mesh check for the bust master (tools/blender/bust_build.py), per part,
with the geometry check's own rules (tools/evani-geocheck/src/meshcheck.rs):
positions welded on a 0.01 mm grid; zero-area (or welded-corner) triangles,
duplicates, open, non-manifold and flipped edges, triangles wound against
their vertex normals, and crossing triangle pairs that share no corner.

    blender -b --factory-startup -t 2 --python tools/blender/bust_check.py -- <bust.glb>

Reads the uncompressed master the build writes (not the converter's tiers).
Prints one line per part and `BUST_CHECK clean` or `BUST_CHECK issues`.
"""

import json
import struct
import sys
from collections import Counter

import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def read_glb(path):
    b = open(path, "rb").read()
    jl = struct.unpack_from("<I", b, 12)[0]
    js = json.loads(b[20:20 + jl])
    bo = 20 + jl + 8
    binb = b[bo:]

    def acc(i):
        a = js["accessors"][i]
        v = js["bufferViews"][a["bufferView"]]
        n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
        dt = {5126: np.float32, 5123: np.uint16, 5125: np.uint32}[a["componentType"]]
        arr = np.frombuffer(binb, dtype=dt, count=a["count"] * n, offset=v.get("byteOffset", 0))
        return arr.reshape(-1, n) if n > 1 else arr

    parts = []
    for m in js["meshes"]:
        p = m["primitives"][0]
        parts.append((m["name"], acc(p["attributes"]["POSITION"]).astype(np.float64),
                      acc(p["attributes"]["NORMAL"]).astype(np.float64), acc(p["indices"]).astype(np.int64).reshape(-1, 3)))
    return parts


def check(name, P, N, F):
    K = np.round(P * 1e5).astype(np.int64)
    kid = {}
    vid = np.array([kid.setdefault(tuple(k), len(kid)) for k in K])
    T = vid[F]
    a, b, c = P[F[:, 0]], P[F[:, 1]], P[F[:, 2]]
    n = np.cross(b - a, c - a)
    area = np.linalg.norm(n, axis=1)
    degen = (area < 1e-12) | (T[:, 0] == T[:, 1]) | (T[:, 1] == T[:, 2]) | (T[:, 0] == T[:, 2])
    ok = ~degen
    srt = np.sort(T[ok], axis=1)
    dup = len(srt) - len({tuple(r) for r in srt})
    vn = N[F[:, 0]] + N[F[:, 1]] + N[F[:, 2]]
    inv = ok & (np.linalg.norm(vn, axis=1) > 1e-9) & ((vn * n).sum(1) < 0)
    directed = Counter()
    for t in T[ok]:
        for x, y in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            directed[(x, y)] += 1
    und = {}
    for (x, y), m in directed.items():
        lo, hi, fwd = (x, y, True) if x <= y else (y, x, False)
        e = und.setdefault((lo, hi), [0, 0])
        e[0 if fwd else 1] += m
    open_n = sum(1 for f, bb in und.values() if f + bb == 1)
    nonman = sum(1 for f, bb in und.values() if f + bb > 2)
    flipped = sum(1 for f, bb in und.values() if f + bb == 2 and (f == 2 or bb == 2))
    # Crossing pairs that share no welded corner.
    good = np.nonzero(ok)[0]
    verts = [Vector(p) for p in P]
    tree = BVHTree.FromPolygons(verts, [list(F[i]) for i in good], epsilon=0.0)
    pairs = tree.overlap(tree)
    cross = 0
    where = []
    for i, j in pairs:
        if i >= j:
            continue
        ti, tj = T[good[i]], T[good[j]]
        if set(ti) & set(tj):
            continue
        cross += 1
        if len(where) < 4:
            where.append(tuple(np.round(P[F[good[i]]].mean(0), 4)))
    if inv.any():
        print("BUST_CHECK   inverted at", [tuple(np.round(P[F[i]].mean(0), 4)) for i in np.nonzero(inv)[0][:6]])
    print(f"BUST_CHECK {name:10s} tris {len(F):6d} degenerate {int(degen.sum())} duplicate {dup} open {open_n} "
          f"nonmanifold {nonman} flipped {flipped} inverted {int(inv.sum())} crossing {cross} {where}", flush=True)
    return int(degen.sum()) + dup + open_n + nonman + flipped + int(inv.sum()) + cross


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:]
    total = 0
    for part in read_glb(argv[0]):
        total += check(*part)
    print("BUST_CHECK", "clean" if total == 0 else "issues")
