# SPDX-License-Identifier: GPL-3.0-or-later
# Export to Evani.dev: one button that exports the model with Blender's own
# glTF exporter and runs the Evani.dev converter on it. This folder is
# separate from the engine (GPL-3.0-or-later, see LICENSE); the engine and the
# games never include or link it.
import os
import shutil

import bpy
from bpy.props import BoolProperty, StringProperty

from . import core

bl_info = {
    "name": "Export to Evani.dev",
    "author": "Evani.Dev",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "location": "File > Export, and the Evani.dev tab in the 3D view sidebar",
    "description": "Send the model to Evani.dev with one button",
    "category": "Import-Export",
}

# What the panel shows: the last run and a live progress line.
STATE = {"busy": False, "progress": "", "result": None}
_proc = {}
_fallback_prefs = {"project_dir": ""}


def _prefs(context=None):
    context = context or bpy.context
    addon = context.preferences.addons.get(__package__)
    return addon.preferences if addon else None


def get_project_dir(context=None):
    p = _prefs(context)
    return p.project_dir if p else _fallback_prefs["project_dir"]


def set_project_dir(value, context=None):
    p = _prefs(context)
    if p:
        p.project_dir = value
    _fallback_prefs["project_dir"] = value


def _redraw():
    wm = bpy.context.window_manager
    for win in wm.windows:
        for area in win.screen.areas:
            area.tag_redraw()


def _finish(result):
    STATE["busy"] = False
    STATE["progress"] = ""
    STATE["result"] = result
    try:
        bpy.context.window_manager.progress_end()
        for win in bpy.context.window_manager.windows:
            win.workspace.status_text_set(None)
    except Exception:
        pass
    _redraw()
    print("Evani.dev:", result["line"])


def _poll():
    p = _proc.get("p")
    if p is None:
        return None
    if p.poll() is None and core.timed_out(p):
        core.stop(p)
        _cleanup()
        _finish(core.timeout_result())
        return None
    if p.poll() is None:
        _proc["ticks"] = _proc.get("ticks", 0) + 1
        STATE["progress"] = "Converting for Evani.dev... {}s".format(int(_proc["ticks"] * 0.5))
        try:
            bpy.context.window_manager.progress_update(min(_proc["ticks"], 90))
            for win in bpy.context.window_manager.windows:
                win.workspace.status_text_set(STATE["progress"])
        except Exception:
            pass
        _redraw()
        return 0.5
    out, err = core.read_logs(p)
    result = core.summarize(_proc["project"], _proc["id"], p.returncode, out, err)
    _cleanup()
    _finish(result)
    return None


def _cleanup():
    p = _proc.get("p")
    if p is not None:
        core.stop(p)
    tmp = _proc.get("tmp")
    if tmp:
        shutil.rmtree(tmp, ignore_errors=True)
    _proc.clear()


class EvaniExportPreferences(bpy.types.AddonPreferences):
    bl_idname = __package__

    project_dir: StringProperty(
        name="Evani.dev folder",
        description="Your Evani.dev folder (the one with the 'tools' folder inside). Converted models go into its assets",
        subtype="DIR_PATH",
    )

    def draw(self, context):
        self.layout.prop(self, "project_dir")


class EVANI_OT_export(bpy.types.Operator):
    """Make this model ready for Evani.dev"""

    bl_idname = "evani.export"
    bl_label = "Export to Evani.dev"
    bl_options = {"REGISTER"}

    project_dir: StringProperty(name="Evani.dev folder", subtype="DIR_PATH")
    model_id: StringProperty(name="Model name", description="Leave empty to use the file name")
    use_bridge: BoolProperty(
        name="Send to the running game instead",
        description="Sends the model to the game open in the Evani.dev tab, which converts it and shows it there",
        default=False,
    )
    wait: BoolProperty(default=False, options={"SKIP_SAVE", "HIDDEN"})

    def invoke(self, context, event):
        self.project_dir = get_project_dir(context)
        self.model_id = context.scene.evani_model_id
        return context.window_manager.invoke_props_dialog(self, width=420)

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "project_dir")
        col.prop(self, "model_id")
        col.prop(self, "use_bridge")
        if self.use_bridge:
            col.label(text="The model is converted by the running game.", icon="INFO")

    def execute(self, context):
        if STATE["busy"]:
            self.report({"WARNING"}, "An export is already running.")
            return {"CANCELLED"}
        project = bpy.path.abspath(self.project_dir or get_project_dir(context))
        model_id = self.model_id.strip() or context.scene.evani_model_id.strip()
        if not model_id:
            model_id = core.slug(os.path.splitext(os.path.basename(bpy.data.filepath))[0])
        for msg in (core.check_id(model_id), None if self.use_bridge else core.check_project(project)):
            if msg:
                STATE["result"] = {"ok": False, "level": "error", "line": msg, "where": "", "files": [], "problems": []}
                self.report({"ERROR"}, msg)
                return {"CANCELLED"}
        set_project_dir(self.project_dir or project, context)
        context.scene.evani_model_id = self.model_id.strip()

        tmp = core.make_temp_dir()
        glb = os.path.join(tmp, model_id + ".glb")
        try:
            core.export_glb(glb)
        except Exception as e:  # Blender's exporter names its own problem
            shutil.rmtree(tmp, ignore_errors=True)
            msg = "Blender could not export the model: {}".format(str(e).strip().splitlines()[-1] if str(e).strip() else "unknown problem")
            STATE["result"] = {"ok": False, "level": "error", "line": msg, "where": "", "files": [], "problems": []}
            self.report({"ERROR"}, msg)
            return {"CANCELLED"}

        if self.use_bridge:
            result = core.send_to_bridge(glb, model_id, project)
            shutil.rmtree(tmp, ignore_errors=True)
            STATE["result"] = result
            self.report({"INFO"} if result["ok"] else {"WARNING"}, result["line"])
            return {"FINISHED"}

        try:
            if self.wait:
                result = core.run_blocking(project, glb, model_id, tmp)
                shutil.rmtree(tmp, ignore_errors=True)
                STATE["result"] = result
                self.report({"INFO"} if result["ok"] else {"WARNING"}, result["line"])
                return {"FINISHED"}
            _proc.update({"p": core.start_convert(project, glb, model_id, tmp), "project": project, "id": model_id, "tmp": tmp, "ticks": 0})
        except Exception as e:
            shutil.rmtree(tmp, ignore_errors=True)
            msg = str(e)
            STATE["result"] = {"ok": False, "level": "error", "line": msg, "where": "", "files": [], "problems": []}
            self.report({"ERROR"}, msg)
            return {"CANCELLED"}
        STATE["busy"] = True
        STATE["result"] = None
        STATE["progress"] = "Converting for Evani.dev..."
        try:
            context.window_manager.progress_begin(0, 100)
        except Exception:
            pass
        bpy.app.timers.register(_poll, first_interval=0.5)
        self.report({"INFO"}, "Converting for Evani.dev. This can take a minute.")
        return {"FINISHED"}


class EVANI_PT_panel(bpy.types.Panel):
    bl_label = "Evani.dev"
    bl_idname = "EVANI_PT_panel"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Evani.dev"

    def draw(self, context):
        col = self.layout.column()
        p = _prefs(context)
        if p:
            col.prop(p, "project_dir", text="Folder")
        col.prop(context.scene, "evani_model_id", text="Name")
        row = col.row()
        row.scale_y = 1.4
        row.enabled = not STATE["busy"]
        row.operator("evani.export", text="Export to Evani.dev", icon="EXPORT")
        if STATE["busy"]:
            col.label(text=STATE["progress"], icon="TIME")
        r = STATE["result"]
        if r:
            box = col.box()
            icon = {"ok": "CHECKMARK", "warn": "ERROR", "error": "CANCEL"}.get(r["level"], "INFO")
            box.label(text=r["line"][:70], icon=icon)
            if r["where"]:
                box.label(text="Files: " + r["where"])
            for pr in r["problems"]:
                box.label(text=pr[:70])


def _menu_export(self, context):
    self.layout.operator("evani.export", text="Evani.dev (ready for the game)")


_classes = (EvaniExportPreferences, EVANI_OT_export, EVANI_PT_panel)


def register():
    bpy.types.Scene.evani_model_id = StringProperty(name="Model name", description="Leave empty to use the file name", default="")
    for c in _classes:
        bpy.utils.register_class(c)
    bpy.types.TOPBAR_MT_file_export.append(_menu_export)


def unregister():
    # Disabled mid-run: stop the poll, end the child process, remove the temp folder.
    if bpy.app.timers.is_registered(_poll):
        bpy.app.timers.unregister(_poll)
    _cleanup()
    STATE.update({"busy": False, "progress": "", "result": None})
    bpy.types.TOPBAR_MT_file_export.remove(_menu_export)
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
    del bpy.types.Scene.evani_model_id
