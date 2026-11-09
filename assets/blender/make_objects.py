"""Generates catalog object models into assets/models/objects/<object id>.glb.

Run headless from the repo root (all objects, or just the ids given):

    blender -b --factory-startup -P assets/blender/make_objects.py [-- fridge_basic sofa_basic ...]

Each model replaces the greybox `parts` of the object with the same id in assets/data/objects.ron
(the parts still define picking). Shapes are written in the game's object space: x across the
footprint width, z across its depth with the front at +z, y up, origin at the back-left floor
corner, so a 1x1 object spans 0..1 on x and z. Keep seats, mattresses and counter tops at the
heights the RON slots expect (`use_at`).

Materials: a name that is a materials.ron id (`wood`, `fabric`, `metal`, `stone`), optionally with
a `.variant` suffix, is textured by the game with the colour as a tint; any other name is a flat
colour. Colours here are sRGB like the RON parts; roughness/metallic/emission export as glTF PBR.
"""

import math
import os
import sys

import bpy

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "models", "objects")


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


_mats = {}


def mat(name, color, rough=0.6, metal=0.0, emit=0.0):
    """A glTF-exportable material; `color` is sRGB."""
    key = (name, tuple(color), rough, metal, emit)
    if key in _mats:
        return _mats[key]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    lin = [c ** 2.2 for c in color]
    b.inputs["Base Color"].default_value = (*lin, 1.0)
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    if emit > 0:
        b.inputs["Emission Color"].default_value = (*lin, 1.0)
        b.inputs["Emission Strength"].default_value = emit
    _mats[key] = m
    return m


def _place(obj, m, bevel, segments):
    obj.data.materials.append(m)
    if bevel > 0:
        mod = obj.modifiers.new("bevel", "BEVEL")
        mod.width = bevel
        mod.segments = segments
        mod.limit_method = "ANGLE"
        mod.harden_normals = True
        for p in obj.data.polygons:
            p.use_smooth = True
    return obj


def g2b(x, y, z):
    """Game object space -> Blender (the glTF exporter maps Blender (x, y, z) to (x, z, -y))."""
    return (x, -z, y)


def box(size, at, m, bevel=0.01, segments=2, rot_x=0.0):
    """Axis-aligned box: `size` and centre `at` in game space; `rot_x` tilts it about x (deg,
    positive leans the top towards -z / the back)."""
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=g2b(*at))
    o = bpy.context.active_object
    o.scale = (size[0], size[2], size[1])
    o.rotation_euler = (math.radians(rot_x), 0, 0)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return _place(o, m, min(bevel, min(size) * 0.45), segments)


def cyl(radius, height, at, m, axis="y", bevel=0.005, verts=24):
    """Cylinder centred at `at`, along game axis `axis`."""
    rot = {"y": (0, 0, 0), "x": (0, math.pi / 2, 0), "z": (math.pi / 2, 0, 0)}[axis]
    bpy.ops.mesh.primitive_cylinder_add(vertices=verts, radius=radius, depth=height, location=g2b(*at), rotation=rot)
    o = bpy.context.active_object
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    return _place(o, m, min(bevel, radius * 0.4, height * 0.4), 2)


def sphere(radius, at, m, scale=(1, 1, 1)):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=20, ring_count=10, radius=radius, location=g2b(*at))
    o = bpy.context.active_object
    o.scale = (scale[0], scale[2], scale[1])
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    for p in o.data.polygons:
        p.use_smooth = True
    o.data.materials.append(m)
    return o


def export(oid):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, f"{oid}.glb")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_yup=True,
        export_apply=True,
        export_materials="EXPORT",
        export_texcoords=False,
        export_normals=True,
        export_animations=False,
        export_skins=False,
        export_morph=False,
    )
    print(f"wrote {path}")


# ------------------------------------------------------------------------------------------------
# palette (sRGB)

WHITE_ENAMEL = dict(color=(0.93, 0.94, 0.95), rough=0.25)
CHROME = dict(color=(0.75, 0.76, 0.78), rough=0.18, metal=1.0)
BLACK_GLASS = dict(color=(0.06, 0.06, 0.07), rough=0.08)
PORCELAIN = dict(color=(0.96, 0.96, 0.97), rough=0.12)


def m_(name, d):
    return mat(name, **d)


# ------------------------------------------------------------------------------------------------
# kitchen


def fridge_basic():
    body = m_("enamel", WHITE_ENAMEL)
    seal = mat("seal", (0.25, 0.25, 0.27), 0.7)
    handle = m_("metal.chrome", CHROME)
    box((0.78, 1.8, 0.66), (0.5, 0.9, 0.38), body, bevel=0.04, segments=3)
    # freezer and fridge doors, split by a dark seal
    box((0.76, 1.03, 0.04), (0.5, 0.5 + 0.02, 0.72), body, bevel=0.02)
    box((0.76, 0.72, 0.04), (0.5, 1.43, 0.72), body, bevel=0.02)
    box((0.77, 0.015, 0.03), (0.5, 1.055, 0.72), seal, bevel=0.0)
    box((0.04, 0.42, 0.04), (0.84, 1.3, 0.77), handle, bevel=0.012)
    box((0.04, 0.32, 0.04), (0.84, 0.75, 0.77), handle, bevel=0.012)
    for x in (0.18, 0.82):
        box((0.06, 0.04, 0.06), (x, 0.02, 0.62), seal, bevel=0.01)


def stove_basic():
    body = m_("enamel", WHITE_ENAMEL)
    top = m_("cooktop", BLACK_GLASS)
    ring = mat("burner", (0.18, 0.18, 0.19), 0.5)
    haunted = mat("burner.haunted", (0.35, 0.2, 0.28), 0.5, emit=0.0)
    knob = m_("metal.chrome", CHROME)
    box((0.8, 0.88, 0.65), (0.5, 0.44, 0.38), body, bevel=0.025)
    box((0.79, 0.025, 0.64), (0.5, 0.89, 0.38), top, bevel=0.008)
    for (x, z, m) in ((0.3, 0.25, ring), (0.7, 0.25, ring), (0.3, 0.52, ring), (0.7, 0.52, haunted)):
        cyl(0.1, 0.012, (x, 0.906, z), m, bevel=0.003)
        cyl(0.055, 0.016, (x, 0.908, z), ring, bevel=0.003)
    # backsplash panel with knobs
    box((0.8, 0.14, 0.06), (0.5, 0.97, 0.07), body, bevel=0.02)
    for i in range(4):
        cyl(0.022, 0.03, (0.2 + i * 0.2, 0.97, 0.115), knob, axis="z")
    # oven door: window, handle bar
    box((0.74, 0.62, 0.03), (0.5, 0.39, 0.715), body, bevel=0.015)
    box((0.56, 0.3, 0.012), (0.5, 0.4, 0.733), m_("oven_glass", BLACK_GLASS), bevel=0.01)
    cyl(0.014, 0.62, (0.5, 0.66, 0.77), knob, axis="x")
    for x in (0.2, 0.8):
        box((0.025, 0.025, 0.04), (x, 0.66, 0.745), knob, bevel=0.005)


def _counter_body(wood, stone, sink=False):
    # cabinet with a toe kick, two doors and a stone top
    box((0.98, 0.1, 0.56), (0.5, 0.05, 0.31), mat("toe_kick", (0.2, 0.17, 0.15), 0.8), bevel=0.0)
    if sink:
        # hollow above 0.72 so the basin shows: carcass, then front/back/side panels
        box((1.0, 0.62, 0.62), (0.5, 0.41, 0.31), wood, bevel=0.01)
        box((1.0, 0.16, 0.03), (0.5, 0.8, 0.605), wood, bevel=0.006)
        box((1.0, 0.16, 0.03), (0.5, 0.8, 0.015), wood, bevel=0.006)
        for x in (0.015, 0.985):
            box((0.03, 0.16, 0.62), (x, 0.8, 0.31), wood, bevel=0.006)
    else:
        box((1.0, 0.78, 0.62), (0.5, 0.49, 0.31), wood, bevel=0.01)
    knob = m_("metal.chrome", CHROME)
    for x in (0.255, 0.745):
        box((0.47, 0.68, 0.025), (x, 0.47, 0.63), wood, bevel=0.012)
        box((0.39, 0.6, 0.012), (x, 0.47, 0.643), wood, bevel=0.006)  # raised panel
        cyl(0.015, 0.03, (x + (0.18 if x < 0.5 else -0.18), 0.7, 0.66), knob, axis="z")
    if not sink:
        box((1.0, 0.04, 0.68), (0.5, 0.9, 0.34), stone, bevel=0.01)


def counter_basic():
    _counter_body(mat("wood", (0.66, 0.52, 0.39), 0.7), mat("stone", (0.82, 0.8, 0.76), 0.25))


def kitchen_sink():
    stone = mat("stone", (0.82, 0.8, 0.76), 0.25)
    _counter_body(mat("wood", (0.66, 0.52, 0.39), 0.7), stone, sink=True)
    steel = mat("metal", (0.72, 0.74, 0.77), 0.2, 1.0)
    # top in four strips around a basin
    box((1.0, 0.04, 0.12), (0.5, 0.9, 0.06), stone, bevel=0.008)
    box((1.0, 0.04, 0.16), (0.5, 0.9, 0.6), stone, bevel=0.008)
    for x in (0.125, 0.875):
        box((0.25, 0.04, 0.4), (x, 0.9, 0.32), stone, bevel=0.004)
    # basin: rim, floor and walls
    for (sx, sz, x, z) in ((0.5, 0.03, 0.5, 0.135), (0.5, 0.03, 0.5, 0.505), (0.03, 0.4, 0.265, 0.32), (0.03, 0.4, 0.735, 0.32)):
        box((sx, 0.012, sz), (x, 0.922, z), steel, bevel=0.004)  # rim
    box((0.46, 0.01, 0.36), (0.5, 0.74, 0.32), steel, bevel=0.0)
    for (sx, sz, x, z) in ((0.46, 0.01, 0.5, 0.14), (0.46, 0.01, 0.5, 0.5), (0.01, 0.36, 0.27, 0.32), (0.01, 0.36, 0.73, 0.32)):
        box((sx, 0.17, sz), (x, 0.83, z), steel, bevel=0.0)
    cyl(0.025, 0.004, (0.5, 0.747, 0.32), mat("drain", (0.15, 0.15, 0.16), 0.4, 1.0))
    # gooseneck tap
    chrome = m_("metal.chrome", CHROME)
    cyl(0.025, 0.2, (0.5, 1.02, 0.07), chrome)
    cyl(0.018, 0.16, (0.5, 1.12, 0.14), chrome, axis="z")
    cyl(0.02, 0.05, (0.5, 1.1, 0.215), chrome)
    cyl(0.012, 0.08, (0.6, 0.95, 0.07), chrome, axis="x")


# ------------------------------------------------------------------------------------------------
# bathroom


def toilet_basic():
    p = m_("porcelain", PORCELAIN)
    chrome = m_("metal.chrome", CHROME)
    # pedestal, bowl, seat and lid
    box((0.26, 0.3, 0.34), (0.5, 0.15, 0.38), p, bevel=0.06, segments=4)
    sphere(0.23, (0.5, 0.33, 0.47), p, scale=(0.9, 0.42, 1.15))
    seat = mat("seat", (0.97, 0.97, 0.98), 0.3)
    box((0.42, 0.03, 0.46), (0.5, 0.425, 0.46), seat, bevel=0.012)
    box((0.4, 0.025, 0.44), (0.5, 0.445, 0.45), seat, bevel=0.012)
    # tank with lid and flush button
    box((0.44, 0.38, 0.17), (0.5, 0.6, 0.11), p, bevel=0.03, segments=3)
    box((0.46, 0.03, 0.19), (0.5, 0.805, 0.11), p, bevel=0.012)
    cyl(0.025, 0.012, (0.5, 0.825, 0.11), chrome)


# ------------------------------------------------------------------------------------------------
# beds and seating


def _bed(width, frame_color, head_color, blanket_color, head_h):
    wood = mat("wood", frame_color, 0.7)
    head = mat("wood.head", head_color, 0.7)
    sheet = mat("fabric", (1.0, 1.0, 1.0), 0.9)
    blanket = mat("fabric.blanket", blanket_color, 0.9)
    cx = width / 2
    w = width - 0.05
    # frame rails and legs
    box((w, 0.2, 1.92), (cx, 0.25, 1.0), wood, bevel=0.015)
    for x in (0.06, width - 0.06):
        box((0.07, 0.15, 0.07), (x, 0.075, 1.92), wood, bevel=0.01)
    # headboard with a top rail, footboard
    box((w + 0.02, head_h, 0.07), (cx, head_h / 2, 0.035), head, bevel=0.02, segments=3)
    box((w + 0.06, 0.06, 0.1), (cx, head_h + 0.01, 0.04), head, bevel=0.02, segments=3)
    box((w, 0.42, 0.05), (cx, 0.21, 1.955), head, bevel=0.015)
    # mattress, blanket folded back over it, pillows
    box((w - 0.07, 0.16, 1.85), (cx, 0.43, 1.0), sheet, bevel=0.05, segments=4)
    box((w - 0.04, 0.07, 1.25), (cx, 0.525, 1.3), blanket, bevel=0.03, segments=3)
    box((w - 0.04, 0.05, 0.16), (cx, 0.545, 0.62), sheet, bevel=0.024, segments=3)  # turned-down sheet
    pillows = 1 if width < 1.5 else 2
    for i in range(pillows):
        px = cx if pillows == 1 else (0.5 + i)
        box((0.6, 0.11, 0.32), (px, 0.565, 0.26), sheet, bevel=0.05, segments=4)


def bed_single():
    _bed(1.0, (0.54, 0.4, 0.29), (0.48, 0.35, 0.26), (0.4, 0.57, 0.8), 0.8)


def bed_double():
    _bed(2.0, (0.48, 0.33, 0.23), (0.43, 0.3, 0.21), (0.69, 0.29, 0.34), 1.0)


def sofa_basic():
    shell = mat("fabric", (0.38, 0.45, 0.56), 0.9)
    cushion = mat("fabric.cushion", (0.46, 0.53, 0.64), 0.95)
    leg = mat("wood.leg", (0.3, 0.21, 0.15), 0.6)
    for x in (0.12, 1.88):
        for z in (0.14, 0.82):
            cyl(0.025, 0.08, (x, 0.04, z), leg)
    box((1.9, 0.3, 0.82), (1.0, 0.23, 0.48), shell, bevel=0.03, segments=3)  # base
    box((1.88, 0.55, 0.22), (1.0, 0.64, 0.15), shell, bevel=0.06, segments=4, rot_x=8)  # back
    for x in (0.1, 1.9):
        box((0.2, 0.3, 0.86), (x, 0.52, 0.48), shell, bevel=0.07, segments=4)  # arms
    for x in (0.6, 1.4):
        box((0.79, 0.12, 0.62), (x, 0.43, 0.56), cushion, bevel=0.045, segments=4)  # seat
        box((0.78, 0.36, 0.13), (x, 0.66, 0.29), cushion, bevel=0.05, segments=4, rot_x=10)  # back


def dining_chair():
    wood = mat("wood", (0.72, 0.56, 0.39), 0.6)
    legs = mat("wood.leg", (0.6, 0.46, 0.32), 0.6)
    box((0.45, 0.04, 0.44), (0.5, 0.45, 0.5), wood, bevel=0.012)
    for x in (0.3, 0.7):
        box((0.035, 0.43, 0.035), (x, 0.215, 0.69), legs, bevel=0.008)
        # back legs run up into the back posts
        box((0.035, 0.95, 0.035), (x, 0.475, 0.3), legs, bevel=0.008)
    box((0.4, 0.025, 0.025), (0.5, 0.18, 0.69), legs, bevel=0.005)  # stretchers
    box((0.4, 0.025, 0.025), (0.5, 0.18, 0.3), legs, bevel=0.005)
    for y in (0.62, 0.78, 0.92):
        box((0.38, 0.06 if y > 0.9 else 0.035, 0.02), (0.5, y, 0.3), wood, bevel=0.008)


def dining_table():
    wood = mat("wood", (0.66, 0.47, 0.31), 0.5)
    legs = mat("wood.leg", (0.54, 0.37, 0.26), 0.6)
    box((1.82, 0.05, 0.92), (1.0, 0.75, 0.5), wood, bevel=0.015, segments=3)
    box((1.6, 0.08, 0.72), (1.0, 0.685, 0.5), legs, bevel=0.005)  # apron
    for x in (0.18, 1.82):
        for z in (0.15, 0.85):
            cyl(0.035, 0.72, (x, 0.36, z), legs, verts=16)
            cyl(0.042, 0.06, (x, 0.03, z), legs, verts=16)


BUILDERS = {f.__name__: f for f in (fridge_basic, stove_basic, counter_basic, kitchen_sink, toilet_basic, bed_single, bed_double, sofa_basic, dining_chair, dining_table)}


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    ids = argv or list(BUILDERS)
    for oid in ids:
        if oid not in BUILDERS:
            sys.exit(f"unknown object id {oid}; have {', '.join(BUILDERS)}")
        reset()
        _mats.clear()
        BUILDERS[oid]()
        export(oid)


main()
