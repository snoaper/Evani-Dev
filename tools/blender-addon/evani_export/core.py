# SPDX-License-Identifier: GPL-3.0-or-later
# Export to Evani.dev: the work behind the button, free of any Blender user
# interface so it can be tested on its own. Only `export_glb` needs `bpy`.
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

# The live bridge's convert endpoint (S3-14, docs/LIVE_API.md 16); the
# session file usually gives the port. Without a running bridge the "send to
# the running game" option answers with a plain message.
BRIDGE_URL = "http://127.0.0.1:8096/live/v1/convert"

_ID_OK = re.compile(r"^[a-z0-9][a-z0-9_-]{0,79}$")


def slug(text):
    """A file name as a model id: lowercase, other characters as `_`."""
    s = re.sub(r"[^a-z0-9_-]", "_", (text or "").lower()).strip("_-")
    return s or "model"


def check_id(model_id):
    """None when the id is fine, else a short plain message."""
    if _ID_OK.match(model_id or ""):
        return None
    return "The model name can only use small letters, numbers, - and _."


def check_project(folder):
    """None when `folder` is an Evani.dev folder, else a short plain message."""
    if not folder or not os.path.isdir(folder):
        return "Pick your Evani.dev folder first."
    if not (os.path.isfile(os.path.join(folder, "Cargo.toml")) and os.path.isdir(os.path.join(folder, "tools", "xtask"))):
        return "That is not an Evani.dev folder. Pick the one that has a 'tools' folder inside."
    return None


def find_cargo():
    override = os.environ.get("EVANI_EXPORT_CARGO")  # a test hook
    if override and os.path.isfile(override):
        return override
    found = shutil.which("cargo")
    if found:
        return found
    home = os.path.expanduser("~")
    for name in ("cargo.exe", "cargo"):
        p = os.path.join(home, ".cargo", "bin", name)
        if os.path.isfile(p):
            return p
    return None


def export_glb(path):
    """Blender's own glTF exporter, extras on (custom properties reach the converter)."""
    import bpy

    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_extras=True,
        export_apply=True,
        export_yup=True,
        use_selection=False,
    )


def convert_command(project, glb, model_id):
    cargo = find_cargo()
    if not cargo:
        return None
    return [cargo, "xtask", "convert", glb, "--id", model_id]


# A converter run that takes longer than this is stopped with a plain message.
CONVERT_TIMEOUT_S = 1800


def start_convert(project, glb, model_id, workdir):
    """Start `cargo xtask convert` at below-normal priority; returns the process.

    Its output goes to files in `workdir` (never a pipe nobody reads, which would
    freeze a chatty run); read them with `read_logs` once it has finished.
    """
    cmd = convert_command(project, glb, model_id)
    if cmd is None:
        raise RuntimeError("Rust (cargo) was not found. Install it from rustup.rs, then try again.")
    flags = getattr(subprocess, "BELOW_NORMAL_PRIORITY_CLASS", 0) | getattr(subprocess, "CREATE_NO_WINDOW", 0)
    out_path = os.path.join(workdir, "convert.out")
    err_path = os.path.join(workdir, "convert.err")
    with open(out_path, "wb") as out, open(err_path, "wb") as err:
        p = subprocess.Popen(cmd, cwd=project, stdout=out, stderr=err, creationflags=flags)
    p.logs = (out_path, err_path)
    p.started = time.monotonic()
    return p


def read_logs(p, limit=400000):
    """The last `limit` bytes of the run's output text and error text."""
    texts = []
    for path in getattr(p, "logs", ()):
        try:
            with open(path, "rb") as f:
                f.seek(0, os.SEEK_END)
                f.seek(max(0, f.tell() - limit))
                texts.append(f.read().decode("utf-8", "replace"))
        except OSError:
            texts.append("")
    while len(texts) < 2:
        texts.append("")
    return texts[0], texts[1]


def timed_out(p):
    return time.monotonic() - getattr(p, "started", time.monotonic()) > CONVERT_TIMEOUT_S


def stop(p):
    """End the run, and the tools it started, if it is still going."""
    if p.poll() is None:
        if os.name == "nt":
            subprocess.run(["taskkill", "/F", "/T", "/PID", str(p.pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        else:
            p.kill()
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            pass


def timeout_result():
    return {"ok": False, "level": "error", "line": "Stopped: the conversion took too long (over {} minutes).".format(CONVERT_TIMEOUT_S // 60), "where": "", "files": [], "problems": []}


def summarize(project, model_id, code, out_text, err_text):
    """Turn the finished converter run into a result dict for the panel."""
    out_dir = os.path.join(project, "assets", "models", model_id)
    files = []
    trusted = code in (0, 1)  # any other failure may leave an earlier export in the folder
    if trusted and os.path.isdir(out_dir):
        files = sorted(f for f in os.listdir(out_dir) if f.endswith(".glb") or f.endswith(".report.md") or f.endswith(".report.json"))
    problems = [l[len("ERROR:"):].strip() for l in err_text.splitlines() if l.startswith("ERROR:")]
    tiers = []
    package = None
    report = os.path.join(out_dir, model_id + ".report.json")
    if trusted and os.path.isfile(report):
        try:
            with open(report, "r", encoding="utf-8") as f:
                data = json.load(f)
            tiers = [t.get("tier") for t in data.get("tiers", []) if t.get("tier")]
            package = data.get("package")
        except (OSError, ValueError):
            pass
    if code == 0:
        line = "Passed: {} tier file{}".format(len(tiers), "" if len(tiers) == 1 else "s")
        if package and package.get("raw") is not None and package.get("cap"):
            line += ", package {:.0f} KB of {:.0f} KB allowed".format(package["raw"] / 1024.0, package["cap"] / 1024.0)
        line += "."
        return {"ok": True, "level": "ok", "line": line, "where": out_dir, "files": files, "problems": []}
    if code == 1:
        n = len(problems)
        line = "Converted, but the checks found {} problem{}. Read the report.".format(n or "some", "" if n == 1 else "s")
        return {"ok": False, "level": "warn", "line": line, "where": out_dir, "files": files, "problems": problems[:5]}
    last = [l for l in (err_text or out_text).splitlines() if l.strip()]
    msg = last[-1].strip() if last else "The converter did not run."
    return {"ok": False, "level": "error", "line": "Could not convert: " + msg[:200], "where": "", "files": [], "problems": []}


def send_to_bridge(glb, model_id, project):
    """Option: post the model to the running game's live server (S3-14,
    docs/LIVE_API.md 16). It converts there and loads into the game's split
    view. Harmless when no game is running."""
    not_ready = {"ok": False, "level": "warn", "line": "The live game isn't running, so nothing was sent. Open the game in the Evani.dev tab and try again, or turn the option off to make the files instead.", "where": "", "files": [], "problems": []}
    token = ""
    url = BRIDGE_URL
    session = os.path.join(project, ".evani", "live", "session.json")
    try:
        with open(session, "r", encoding="utf-8") as f:
            s = json.load(f)
        token = str(s.get("token", ""))
        http = str(s.get("http", ""))
        if http.startswith("http://127.0.0.1:") or http.startswith("http://localhost:"):
            url = http.rstrip("/") + "/live/v1/convert"
    except (OSError, ValueError, AttributeError):
        pass
    try:
        with open(glb, "rb") as f:
            body = f.read()
        req = urllib.request.Request(url + "?id=" + model_id, data=body, method="POST")
        req.add_header("Content-Type", "model/gltf-binary")
        if token:
            req.add_header("Authorization", "Bearer " + token)
        with urllib.request.urlopen(req, timeout=10) as r:
            if 200 <= r.status < 300:
                return {"ok": True, "level": "ok", "line": "Sent to the running game. It converts there and shows up in the game when it's done.", "where": "", "files": [], "problems": []}
    except urllib.error.HTTPError as e:
        if e.code == 409:
            return {"ok": False, "level": "warn", "line": "The game is converting another model. Try again in a moment.", "where": "", "files": [], "problems": []}
        if e.code == 413:
            return {"ok": False, "level": "error", "line": "The model is too big to send to the game.", "where": "", "files": [], "problems": []}
        if e.code not in (404, 405, 501):
            return {"ok": False, "level": "error", "line": "The running game refused the model (code {}).".format(e.code), "where": "", "files": [], "problems": []}
    except (urllib.error.URLError, OSError, ValueError):
        pass
    return not_ready


def make_temp_dir():
    return tempfile.mkdtemp(prefix="evani_export_")


def run_blocking(project, glb, model_id, workdir, timeout=CONVERT_TIMEOUT_S):
    """Convert and wait (headless tests, scripts)."""
    p = start_convert(project, glb, model_id, workdir)
    try:
        p.wait(timeout)
    except subprocess.TimeoutExpired:
        stop(p)
        return timeout_result()
    out, err = read_logs(p)
    return summarize(project, model_id, p.returncode, out, err)


