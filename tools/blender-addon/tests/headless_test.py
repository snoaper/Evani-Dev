# SPDX-License-Identifier: GPL-3.0-or-later
# Headless test of the "Export to Evani.dev" add-on. Run by
# `cargo xtask blender-addon --test` as:
#   blender --background --factory-startup --python headless_test.py -- <addon.zip> <engine root>
# It installs the built zip, makes a small model in the script, presses the
# button (the operator) and checks the converted tier files and report appear.
import os
import shutil
import sys

import bpy

zip_path, root = sys.argv[sys.argv.index("--") + 1:][:2]
MODEL_ID = "blender_addon_probe"
out_dir = os.path.join(root, "assets", "models", MODEL_ID)


def say(msg):
    print("evani-addon-test: " + msg)
    sys.stdout.flush()


def fail(msg):
    say("FAIL " + msg)
    shutil.rmtree(out_dir, ignore_errors=True)
    sys.exit(1)


# 1. Install the built zip the way a person would, then check it is on.
repos = [r for r in bpy.context.preferences.extensions.repos if r.use_remote_url is False]
user_repo = next((r for r in repos if r.module == "user_default"), None)
if user_repo is None:
    user_repo = bpy.context.preferences.extensions.repos.new(name="Evani test", module="evani_test_repo", custom_directory=os.path.join(os.path.dirname(zip_path), "repo"))
    os.makedirs(user_repo.custom_directory, exist_ok=True)
res = bpy.ops.extensions.package_install_files(filepath=zip_path, repo=user_repo.module, enable_on_install=True)
if "FINISHED" not in res:
    fail("the zip did not install: %s" % (res,))
if not hasattr(bpy.ops.evani, "export"):
    fail("installed, but the Export to Evani.dev button (evani.export) is missing")
say("installed and enabled")
pkg = [k for k in bpy.context.preferences.addons.keys() if k.endswith("evani_export")]
if not pkg:
    fail("the add-on is not in the enabled list")
mod = sys.modules[pkg[0]]
core = mod.core

# 2. A small model made here: a cube with a material and a custom property.
for o in list(bpy.data.objects):
    bpy.data.objects.remove(o)
bpy.ops.mesh.primitive_uv_sphere_add(segments=16, ring_count=8, radius=0.5)
ob = bpy.context.active_object
ob.name = "probe"
mat = bpy.data.materials.new("probe_mat")
ob.data.materials.append(mat)
ob["evani"] = {"class": MODEL_ID}

# 3. A wrong folder is refused with a plain message, nothing converted.
try:  # an error report with CANCELLED raises in a script
    res = bpy.ops.evani.export(project_dir=os.path.join(root, "no-such-folder"), model_id=MODEL_ID, wait=True)
except RuntimeError as e:
    res = {"CANCELLED"}
if "CANCELLED" not in res or "Evani.dev folder" not in mod.STATE["result"]["line"]:
    fail("a wrong folder was not refused plainly: %s %s" % (res, mod.STATE["result"]))
say("wrong folder refused: " + mod.STATE["result"]["line"])

# 4. The bridge option with no game running says so and does nothing harmful.
core.BRIDGE_URL = "http://127.0.0.1:8319/live/v1/convert"  # nothing listens here
res = bpy.ops.evani.export(project_dir=root, model_id=MODEL_ID, use_bridge=True, wait=True)
line = mod.STATE["result"]["line"]
if "FINISHED" not in res or "isn't running" not in line or os.path.isdir(out_dir):
    fail("bridge option misbehaved: %s %s" % (res, line))
say("bridge option: " + line)

# 4b. A chatty converter that fails (a stand-in for cargo, Windows only): the run
# must finish with a plain message (its output is not left in a full pipe) and
# must not show files an earlier export left behind.
import tempfile

fake_dir = tempfile.mkdtemp(prefix="evani_fake_cargo_")


def fake_cargo(name, body):
    path = os.path.join(fake_dir, name)
    with open(path, "w") as f:
        f.write("@echo off\r\n" + body + "\r\n")
    return path


if os.name == "nt":
    os.makedirs(out_dir, exist_ok=True)
    for stale in (MODEL_ID + ".low.glb", MODEL_ID + ".report.json"):
        with open(os.path.join(out_dir, stale), "w") as f:
            f.write("{}")
    os.environ["EVANI_EXPORT_CARGO"] = fake_cargo("chatty.cmd", "for /L %%i in (1,1,6000) do echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx 1>&2\r\nexit /b 2")
    res = bpy.ops.evani.export(project_dir=root, model_id=MODEL_ID, wait=True)
    r = mod.STATE["result"]
    if r["ok"] or r["files"] or r["where"] or "Could not convert" not in r["line"]:
        fail("a failed chatty run showed %s" % (r,))
    shutil.rmtree(out_dir, ignore_errors=True)
    say("chatty failing converter finished, no stale files: " + r["line"][:60])
    del os.environ["EVANI_EXPORT_CARGO"]

# 5. The real thing.
res = bpy.ops.evani.export(project_dir=root, model_id=MODEL_ID, wait=True)
r = mod.STATE["result"]
say("result: " + r["line"])
if "FINISHED" not in res or not r["ok"]:
    fail("export did not pass: %s %s" % (res, r))
files = sorted(os.listdir(out_dir)) if os.path.isdir(out_dir) else []
tiers = [f for f in files if f.endswith(".glb")]
if not tiers:
    fail("no tier .glb files in %s: %s" % (out_dir, files))
for need in (MODEL_ID + ".report.md", MODEL_ID + ".report.json"):
    if need not in files:
        fail("missing " + need)
if "Passed" not in r["line"]:
    fail("the summary line does not say Passed: " + r["line"])
say("files: " + ", ".join(files))

# 6. Turned off in the middle of a run: the child process ends and the temp
# folder goes (Windows only, with a stand-in that would run for a while).
if os.name == "nt":
    os.environ["EVANI_EXPORT_CARGO"] = fake_cargo("slow.cmd", "ping -n 60 127.0.0.1 >nul\r\nexit /b 0")
    res = bpy.ops.evani.export(project_dir=root, model_id=MODEL_ID)
    proc, tmp_dir = mod._proc.get("p"), mod._proc.get("tmp")
    if "FINISHED" not in res or proc is None or proc.poll() is not None or not os.path.isdir(tmp_dir):
        fail("the slow run did not start as expected")
    mod.unregister()
    if proc.poll() is None or os.path.exists(tmp_dir) or mod._proc:
        fail("disabling the add-on did not stop the run and remove its temp folder")
    say("disabled mid-run: process ended, temp folder removed")
    del os.environ["EVANI_EXPORT_CARGO"]
shutil.rmtree(fake_dir, ignore_errors=True)
shutil.rmtree(out_dir, ignore_errors=True)
say("PASS")
