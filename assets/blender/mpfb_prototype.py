"""Prototype: MPFB2 (MakeHuman) base bodies as a replacement for the metaball peon body.

Needs the MPFB extension (`blender --online-mode -b -c extension install mpfb --enable`).
Builds a lineup of humans from MPFB macro sliders, rigs each with MPFB's game_engine skeleton and
renders the lineup, or exports one rigged body as glTF:

    blender -b -P assets/blender/mpfb_prototype.py -- render out/mpfb_lineup.png
    blender -b -P assets/blender/mpfb_prototype.py -- export out/mpfb_adult.glb

The mesh is MakeHuman's CC0 base mesh. Skins, eyes, brows, hair and clothes come from MakeHuman's CC0
system asset pack (makehuman_system_assets_cc0.zip from files.makehumancommunity.org), unzipped
into MPFB's user data directory (~/.config/blender/<ver>/extensions/.user/blender_org/mpfb/data).
"""

import math
import os
import sys

import bpy
from mathutils import Vector

from bl_ext.blender_org.mpfb.services.humanservice import HumanService
from bl_ext.blender_org.mpfb.services.locationservice import LocationService

# (label, macro sliders). Values are MPFB's 0..1 ranges; age 0.5 is 25 years, 0.1875 is ~11.
LINEUP = [
    ("woman", dict(gender=0.0, age=0.5, weight=0.5, muscle=0.5, height=0.45),
     dict(skin="young_caucasian_female", hair="bob02", clothes=["female_casualsuit01", "shoes02"])),
    ("man", dict(gender=1.0, age=0.5, weight=0.55, muscle=0.6, height=0.6),
     dict(skin="young_african_male", hair="short02", clothes=["male_casualsuit03", "shoes01"])),
    ("heavy", dict(gender=0.8, age=0.7, weight=0.95, muscle=0.4, height=0.5),
     dict(skin="middleage_asian_male", hair="short04", clothes=["male_worksuit01", "shoes03"])),
    ("elder", dict(gender=0.0, age=0.95, weight=0.6, muscle=0.3, height=0.4),
     dict(skin="old_caucasian_female", hair="ponytail01", clothes=["female_elegantsuit01", "shoes05"])),
    ("teen", dict(gender=1.0, age=0.3, weight=0.45, muscle=0.45, height=0.5),
     dict(skin="young_asian_male", hair="short03", clothes=["male_casualsuit05", "shoes06"])),
    ("child", dict(gender=0.3, age=0.15, weight=0.5, muscle=0.5, height=0.5),
     dict(skin="young_african_female", hair="afro01", clothes=["female_sportsuit01", "shoes04"])),
]
RACE = dict(asian=0.33, caucasian=0.34, african=0.33)


def asset(sub, name, ext):
    return os.path.join(LocationService.get_user_data(sub), name, f"{name}.{ext}")


def make_human(macros, look=None):
    info = dict(gender=0.5, age=0.5, muscle=0.5, weight=0.5, proportions=0.5, height=0.5,
                cupsize=0.5, firmness=0.5, race=dict(RACE))
    info.update(macros)
    body = HumanService.create_human(macro_detail_dict=info)
    rig = HumanService.add_builtin_rig(body, "game_engine")
    if look:
        HumanService.set_character_skin(asset("skins", look["skin"], "mhmat"), body, skin_type="MAKESKIN")
        parts = [("eyes", os.path.join(LocationService.get_user_data("eyes"), "low-poly", "low-poly.mhclo")),
                 ("eyebrows", asset("eyebrows", "eyebrow001", "mhclo")),
                 ("eyelashes", asset("eyelashes", "eyelashes01", "mhclo")),
                 ("hair", asset("hair", look["hair"], "mhclo"))]
        parts += [("Clothes", asset("clothes", c, "mhclo")) for c in look["clothes"]]
        for kind, path in parts:
            HumanService.add_mhclo_asset(path, body, asset_type=kind, subdiv_levels=0)
    return body, rig


def clear_scene():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj)


def render(path):
    clear_scene()
    x = 0.0
    for _label, macros, look in LINEUP:
        _body, rig = make_human(macros, look)
        rig.location.x = x
        x += 1.0
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x, scene.render.resolution_y = 1800, 900
    scene.render.filepath = path
    cx = (x - 1.0) / 2
    bpy.ops.object.camera_add(location=(cx + 1.6, -9.0, 1.5))
    cam = bpy.context.object
    cam.data.lens = 50
    cam.rotation_euler = (Vector((cx, 0, 0.9)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam
    bpy.ops.object.light_add(type="SUN", rotation=(math.radians(50), 0, math.radians(30)))
    bpy.context.object.data.energy = 3
    world = bpy.data.worlds.new("w")
    world.color = (0.35, 0.35, 0.4)
    scene.world = world
    bpy.ops.render.render(write_still=True)


def export(path):
    clear_scene()
    make_human(*LINEUP[1][1:])
    bpy.ops.export_scene.gltf(filepath=path, export_apply=True, export_animations=False)


if __name__ == "__main__":
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else ["render", "out/mpfb_lineup.png"]
    {"render": render, "export": export}[args[0]](args[1])
