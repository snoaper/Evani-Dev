# The quadruped generator's glb attribute fix-up (original work). Plain
# Python, no Blender needed:
#   python tools/models/quadruped/glbfix.py <model.glb> [--dump]
# Blender writes an INT8 attribute as float and a byte colour as unsigned
# short; the studio's contract (docs/studio/CONTRACTS.md) is one unsigned
# byte per vertex for `_EVANI_PART` and four unorm8 for `_EVANI_COAT`.
import json, struct, sys


def log(tag, data):
    print("QUAD_%s %s" % (tag, json.dumps(data)))


def fix_glb_attributes(path):
    """Blender writes an INT8 attribute as float and a byte colour as
    unsigned short; the studio's contract is one unsigned byte per vertex
    for `_EVANI_PART` and four unorm8 for `_EVANI_COAT`. Rewrite those
    accessors in place (new buffer views at the end of the binary chunk)."""
    with open(path, "rb") as f:
        data = f.read()
    magic, ver, total = struct.unpack_from("<III", data, 0)
    jl, jt = struct.unpack_from("<II", data, 12)
    js = json.loads(data[20:20 + jl])
    bl, bt = struct.unpack_from("<II", data, 20 + jl)
    binc = bytearray(data[28 + jl:28 + jl + bl])
    done = {}

    def read(acc_i):
        acc = js["accessors"][acc_i]
        bv = js["bufferViews"][acc["bufferView"]]
        comp = {5126: ("f", 4), 5123: ("H", 2), 5121: ("B", 1)}[acc["componentType"]]
        n = {"SCALAR": 1, "VEC4": 4}[acc["type"]]
        stride = bv.get("byteStride", comp[1] * n)
        off = bv.get("byteOffset", 0) + acc.get("byteOffset", 0)
        out = []
        for i in range(acc["count"]):
            out.append(struct.unpack_from("<%d%s" % (n, comp[0]), binc, off + i * stride))
        return acc, out

    for mesh in js.get("meshes", []):
        for prim in mesh["primitives"]:
            for name in ("_EVANI_PART", "_EVANI_COAT"):
                ai = prim["attributes"].get(name)
                if ai is None:
                    continue
                if ai in done:
                    prim["attributes"][name] = done[ai]
                    continue
                acc, vals = read(ai)
                if name == "_EVANI_PART":
                    raw = bytes(int(round(v[0])) & 0xFF for v in vals)
                    width, typ, norm = 1, "SCALAR", False
                else:
                    scale = 65535.0 if acc["componentType"] == 5123 else (255.0 if acc["componentType"] == 5121 else 1.0)
                    raw = bytes(min(255, max(0, int(round(c / scale * 255.0)))) for v in vals for c in v)
                    width, typ, norm = 4, "VEC4", True
                while len(binc) % 4:
                    binc.append(0)
                off = len(binc)
                binc.extend(raw)
                stride = 4 if width == 1 else 4
                if width == 1:
                    # vertex attributes need 4-byte aligned strides: pad each byte to 4
                    padded = bytearray()
                    for bt_ in raw:
                        padded.extend((bt_, 0, 0, 0))
                    binc[off:] = padded
                js["bufferViews"].append({"buffer": 0, "byteOffset": off, "byteLength": len(binc) - off, "byteStride": stride, "target": 34962})
                new = {"bufferView": len(js["bufferViews"]) - 1, "componentType": 5121, "count": acc["count"], "type": typ}
                if norm:
                    new["normalized"] = True
                if name == "_EVANI_PART":
                    new["min"] = [min(v[0] for v in vals)]
                    new["max"] = [max(v[0] for v in vals)]
                js["accessors"].append(new)
                done[ai] = len(js["accessors"]) - 1
                prim["attributes"][name] = done[ai]
    while len(binc) % 4:
        binc.append(0)
    js["buffers"][0]["byteLength"] = len(binc)
    jb = json.dumps(js, separators=(",", ":")).encode()
    while len(jb) % 4:
        jb += b" "
    out = struct.pack("<III", magic, ver, 12 + 8 + len(jb) + 8 + len(binc)) + struct.pack("<II", len(jb), 0x4E4F534A) + jb + struct.pack("<II", len(binc), 0x004E4942) + bytes(binc)
    with open(path, "wb") as f:
        f.write(out)
    log("GLB_ATTRIBUTES", {"rewritten": len(done), "bytes": len(out)})


def dump(path):
    """Every primitive's attributes and the `_EVANI_PART` values it holds."""
    with open(path, "rb") as f:
        data = f.read()
    jl = struct.unpack_from("<I", data, 12)[0]
    js = json.loads(data[20:20 + jl])
    for m in js.get("meshes", []):
        for p in m["primitives"]:
            attrs = {k: (js["accessors"][v]["componentType"], js["accessors"][v]["type"], js["accessors"][v].get("normalized", False)) for k, v in p["attributes"].items()}
            print("PRIM", m.get("name"), json.dumps(attrs))


if __name__ == "__main__":
    if "--dump" in sys.argv:
        dump(sys.argv[1])
    else:
        fix_glb_attributes(sys.argv[1])
