"""Find and repair vertex normals that point against every triangle using them.

A vertex normal that points against all of its own triangles shades that
corner as if lit from behind (a dark spot). `crates/evani-gltf/tests/normals.rs`
fails a shipped model with one; this tool shows them and repairs them in
place: each such normal becomes the area-weighted normal of its triangles,
and its tangent is made orthogonal to it again (handedness kept). Nothing
else in the file changes, so the file keeps its size and layout.

Mirrored nodes follow the glTF rule (a negative determinant turns the front
face clockwise), as the engine draws them (model.wgsl `mirror`).

Reads float or KHR_mesh_quantization normals (f32, i8 or i16 normalized).

    python tools/models/fix_flipped_normals.py <file.glb> [...]          # report
    python tools/models/fix_flipped_normals.py --fix <file.glb> [...]    # repair

Exit code 1 in report mode when any file has a flipped normal.
"""

import json
import math
import struct
import sys

CT = {5126: ("f", 4), 5123: ("H", 2), 5125: ("I", 4), 5121: ("B", 1), 5122: ("h", 2), 5120: ("b", 1)}
NC = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
NORM_MAX = {5122: 32767, 5120: 127, 5123: 65535, 5121: 255}


def load(path):
    b = bytearray(open(path, "rb").read())
    if b[:4] != b"glTF":
        raise SystemExit(f"{path}: not a .glb")
    jl = struct.unpack_from("<I", b, 12)[0]
    js = json.loads(bytes(b[20 : 20 + jl]))
    bin_off = 20 + jl + 8
    return b, js, bin_off


class Accessor:
    def __init__(self, js, i, bin_off):
        a = js["accessors"][i]
        bv = js["bufferViews"][a["bufferView"]]
        self.ct = a["componentType"]
        self.fmt, size = CT[self.ct]
        self.n = NC[a["type"]]
        self.count = a["count"]
        self.norm = a.get("normalized", False)
        self.off = bin_off + bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        self.stride = bv.get("byteStride", size * self.n)

    def get(self, b, k):
        v = list(struct.unpack_from("<" + self.fmt * self.n, b, self.off + k * self.stride))
        if self.norm:
            m = NORM_MAX[self.ct]
            v = [max(x / m, -1.0) for x in v]
        return v

    def put(self, b, k, v):
        if self.ct == 5126:
            vals = v
        elif self.norm and self.ct in (5120, 5122):
            m = NORM_MAX[self.ct]
            vals = [max(-m, min(m, round(x * m))) for x in v]
        else:
            raise SystemExit("only float or signed normalized normals can be written")
        struct.pack_into("<" + self.fmt * self.n, b, self.off + k * self.stride, *vals)


def node_matrix(node):
    if "matrix" in node:
        m = node["matrix"]
        return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
    t = node.get("translation", [0, 0, 0])
    x, y, z, w = node.get("rotation", [0, 0, 0, 1])
    s = node.get("scale", [1, 1, 1])
    r = [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ]
    return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]] for i in range(3)] + [[0, 0, 0, 1]]


def mmul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def det3(m):
    return (
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    )


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def unit(a):
    l = math.sqrt(dot(a, a))
    return [x / l for x in a] if l > 0 else a


def mesh_nodes(js):
    """(node index, world matrix) of every node with a mesh in the scene."""
    out = []
    ident = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]

    def walk(i, parent):
        m = mmul(parent, node_matrix(js["nodes"][i]))
        if "mesh" in js["nodes"][i]:
            out.append((i, m))
        for c in js["nodes"][i].get("children", []):
            walk(c, m)

    scenes = js.get("scenes") or [{"nodes": list(range(len(js["nodes"])))}]
    for r in scenes[js.get("scene", 0)]["nodes"]:
        walk(r, ident)
    return out


def scan(path, fix):
    b, js, bin_off = load(path)
    found = []
    done = set()
    for ni, _m in mesh_nodes(js):
        # Tested in the part's own space, before the node's transform: a
        # mirrored node turns its triangles and its normals together, so its
        # parts need no sign change here (in world space they would).
        node = js["nodes"][ni]
        for pi, prim in enumerate(js["meshes"][node["mesh"]]["primitives"]):
            at = prim.get("attributes", {})
            if "POSITION" not in at or "NORMAL" not in at or prim.get("mode", 4) != 4:
                continue
            pa = Accessor(js, at["POSITION"], bin_off)
            na = Accessor(js, at["NORMAL"], bin_off)
            ta = Accessor(js, at["TANGENT"], bin_off) if "TANGENT" in at else None
            p = [pa.get(b, k) for k in range(pa.count)]
            if "indices" in prim:
                ia = Accessor(js, prim["indices"], bin_off)
                idx = [int(ia.get(b, k)[0]) for k in range(ia.count)]
            else:
                idx = list(range(pa.count))
            faces = {}
            for t in range(0, len(idx) - 2, 3):
                a, c, d = idx[t : t + 3]
                f = cross(sub(p[c], p[a]), sub(p[d], p[a]))
                if dot(f, f) < 1e-20:
                    continue
                for v in (a, c, d):
                    faces.setdefault(v, []).append(f)
            for v, fs in faces.items():
                n = na.get(b, v)
                if dot(n, n) < 1e-12:
                    continue
                nl = math.sqrt(dot(n, n))
                if not all(dot(n, f) / (nl * math.sqrt(dot(f, f))) < -0.5 for f in fs):
                    continue
                key = (na.off, v)
                if key in done:
                    continue
                done.add(key)
                want = unit([sum(f[k] for f in fs) for k in range(3)])
                found.append((node.get("name", f"node {ni}"), pi, v, [round(x, 2) for x in n], [round(x, 2) for x in want]))
                if fix:
                    na.put(b, v, want)
                    if ta is not None:
                        t4 = ta.get(b, v)
                        t3 = t4[:3]
                        t3 = unit(sub(t3, [x * dot(t3, want) for x in want]))
                        ta.put(b, v, t3 + [1.0 if t4[3] >= 0 else -1.0])
    if fix and found:
        open(path, "wb").write(bytes(b))
    return found


def main(argv):
    fix = "--fix" in argv
    files = [a for a in argv if a != "--fix"]
    if not files:
        print(__doc__)
        return 2
    bad = 0
    for f in files:
        try:
            found = scan(f, fix)
        except (struct.error, KeyError, IndexError) as e:
            # Compressed (EXT_meshopt_compression) or external buffers: the
            # engine's own reader checks those (crates/evani-gltf/tests/normals.rs).
            print(f"{f}: skipped, buffers this tool can't read ({type(e).__name__})")
            continue
        bad += len(found)
        verb = "repaired" if fix else "flipped"
        print(f"{f}: {len(found)} {verb}")
        for name, pi, v, n, want in found:
            print(f"  {name} primitive {pi} vertex {v}: normal {n} -> faces {want}")
    return 1 if bad and not fix else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
