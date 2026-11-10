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


def mat(name, color, rough=0.6, metal=0.0, emit=0.0, alpha=1.0):
    """A glTF-exportable material; `color` is sRGB. `alpha` < 1 draws in the transparent pass."""
    key = (name, tuple(color), rough, metal, emit, alpha)
    if key in _mats:
        return _mats[key]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    lin = [c ** 2.2 for c in color]
    b.inputs["Base Color"].default_value = (*lin, 1.0)
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    if alpha < 1.0:
        b.inputs["Alpha"].default_value = alpha
        m.surface_render_method = "BLENDED"
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


# ------------------------------------------------------------------------------------------------
# second batch


def armchair():
    shell = mat("fabric", (0.57, 0.31, 0.23), 0.9)
    cushion = mat("fabric.cushion", (0.63, 0.34, 0.25), 0.95)
    leg = mat("wood.leg", (0.3, 0.21, 0.15), 0.6)
    for x in (0.15, 0.85):
        for z in (0.17, 0.83):
            cyl(0.022, 0.08, (x, 0.04, z), leg)
    box((0.86, 0.3, 0.78), (0.5, 0.23, 0.5), shell, bevel=0.03, segments=3)
    box((0.84, 0.66, 0.2), (0.5, 0.7, 0.14), shell, bevel=0.07, segments=4, rot_x=8)  # wing back
    for x in (0.085, 0.915):
        box((0.16, 0.3, 0.82), (x, 0.52, 0.5), shell, bevel=0.07, segments=4)
        sphere(0.085, (x, 0.66, 0.84), shell, scale=(1.0, 0.8, 0.5))  # rolled arm fronts
    box((0.62, 0.12, 0.6), (0.5, 0.43, 0.56), cushion, bevel=0.045, segments=4)
    box((0.6, 0.38, 0.12), (0.5, 0.66, 0.3), cushion, bevel=0.05, segments=4, rot_x=10)


def tv_basic():
    wood = mat("wood", (0.24, 0.19, 0.16), 0.6)
    black = mat("bezel", (0.05, 0.05, 0.06), 0.3)
    screen = mat("screen", (0.2, 0.35, 0.5), 0.1, emit=1.5)
    # low media console: open shelf between two doors, short legs
    for x in (0.1, 0.9):
        for z in (0.15, 0.45):
            box((0.04, 0.08, 0.04), (x, 0.04, z), black, bevel=0.008)
    box((0.92, 0.04, 0.42), (0.5, 0.48, 0.3), wood, bevel=0.012)
    box((0.92, 0.03, 0.42), (0.5, 0.095, 0.3), wood, bevel=0.008)
    for x in (0.025, 0.975, 0.3, 0.7):
        box((0.03, 0.37, 0.42), (x, 0.28, 0.3), wood, bevel=0.006)
    box((0.92, 0.37, 0.02), (0.5, 0.28, 0.1), wood, bevel=0.0)  # back panel
    for x in (0.16, 0.84):
        box((0.26, 0.33, 0.02), (x, 0.28, 0.51), wood, bevel=0.008)
    box((0.3, 0.06, 0.24), (0.5, 0.17, 0.3), black, bevel=0.01)  # a console in the shelf
    # flat screen on a stand
    box((0.28, 0.02, 0.16), (0.5, 0.51, 0.3), black, bevel=0.006)
    box((0.06, 0.08, 0.03), (0.5, 0.56, 0.28), black, bevel=0.006)
    box((0.95, 0.55, 0.04), (0.5, 0.86, 0.3), black, bevel=0.012)
    box((0.89, 0.49, 0.006), (0.5, 0.86, 0.321), screen, bevel=0.0)


def bookshelf():
    wood = mat("wood", (0.5, 0.35, 0.23), 0.7)
    for x in (0.035, 0.965):
        box((0.04, 1.9, 0.35), (x, 0.95, 0.2), wood, bevel=0.008)
    box((0.95, 1.86, 0.02), (0.5, 0.95, 0.035), wood, bevel=0.0)  # back
    box((0.98, 0.05, 0.37), (0.5, 1.915, 0.2), wood, bevel=0.012)  # crown
    box((0.9, 0.08, 0.33), (0.5, 0.04, 0.21), wood, bevel=0.006)  # plinth
    shelves = [0.08, 0.5, 0.92, 1.34, 1.76]
    for y in shelves[1:]:
        box((0.89, 0.025, 0.32), (0.5, y, 0.21), wood, bevel=0.004)
    # rows of books: deterministic varied heights, widths, colours and the odd lean
    colours = [(0.6, 0.2, 0.2), (0.2, 0.35, 0.5), (0.3, 0.5, 0.3), (0.75, 0.62, 0.3), (0.35, 0.25, 0.45), (0.85, 0.82, 0.74), (0.15, 0.15, 0.18)]
    seed = 7
    for row, y in enumerate(shelves[:-1]):
        x = 0.07
        while True:
            seed = (seed * 1103515245 + 12345) % 2**31
            w = 0.03 + (seed % 5) * 0.008
            h = 0.22 + (seed // 7 % 6) * 0.025
            # a leaning book pivots on its bottom-right corner, so its top overhangs to the right
            lean = math.radians(12) if seed % 11 == 0 else 0.0
            span = w + h * math.sin(lean)
            if x + span > 0.93 or (row == 2 and 0.55 < x + span and x < 0.75):
                if x + span > 0.93:
                    break
                x += 0.03
                continue
            c = colours[seed // 3 % len(colours)]
            # centre relative to the pivot, rotated (positive Blender y-rotation tips the top to +x)
            cx = x + w - w / 2 * math.cos(lean) + h / 2 * math.sin(lean)
            cy = y + 0.0125 + w / 2 * math.sin(lean) + h / 2 * math.cos(lean)
            b = box((w, h, 0.22), (cx, cy, 0.22), mat(f"book{seed // 3 % len(colours)}", c, 0.8), bevel=0.004)
            if lean:
                b.rotation_euler = (0, lean, 0)
            x += span + 0.004
    # a little vase on the gap
    cyl(0.04, 0.16, (0.65, 0.92 + 0.0125 + 0.08, 0.22), mat("vase", (0.25, 0.55, 0.6), 0.2))


def computer_desk():
    top = mat("desk", (0.85, 0.85, 0.83), 0.4)
    side = mat("desk.side", (0.75, 0.75, 0.73), 0.5)
    black = mat("bezel", (0.06, 0.06, 0.07), 0.3)
    screen = mat("screen", (0.3, 0.45, 0.65), 0.1, emit=1.2)
    box((0.96, 0.035, 0.6), (0.5, 0.74, 0.3), top, bevel=0.01)
    box((0.04, 0.72, 0.56), (0.05, 0.36, 0.3), side, bevel=0.006)
    # drawer pedestal on the right
    box((0.36, 0.72, 0.56), (0.76, 0.36, 0.3), side, bevel=0.006)
    knob = m_("metal.chrome", CHROME)
    for y in (0.6, 0.38, 0.16):
        box((0.33, 0.2, 0.015), (0.76, y, 0.585), top, bevel=0.005)
        box((0.1, 0.015, 0.02), (0.76, y + 0.06, 0.6), knob, bevel=0.004)
    box((0.5, 0.18, 0.015), (0.3, 0.62, 0.05), side, bevel=0.0)  # modesty panel
    # monitor on a stand, keyboard, mouse, a mug
    box((0.18, 0.012, 0.12), (0.5, 0.764, 0.13), black, bevel=0.004)
    box((0.04, 0.12, 0.025), (0.5, 0.82, 0.12), black, bevel=0.005)
    box((0.52, 0.32, 0.03), (0.5, 0.98, 0.15), black, bevel=0.01)
    box((0.48, 0.28, 0.005), (0.5, 0.985, 0.166), screen, bevel=0.0)
    box((0.4, 0.018, 0.13), (0.5, 0.767, 0.4), mat("keyboard", (0.15, 0.15, 0.16), 0.6), bevel=0.006)
    sphere(0.03, (0.78, 0.768, 0.42), mat("keyboard", (0.15, 0.15, 0.16), 0.6), scale=(0.75, 0.45, 1.1))
    cyl(0.035, 0.09, (0.13, 0.802, 0.38), mat("mug", (0.85, 0.4, 0.25), 0.3))


def shower_basic():
    tray = m_("porcelain", PORCELAIN)
    tile = mat("wall_tile", (0.85, 0.9, 0.92), 0.2)
    glass = mat("glass", (0.75, 0.88, 0.95), 0.05, alpha=0.3)
    chrome = m_("metal.chrome", CHROME)
    box((0.96, 0.08, 0.96), (0.5, 0.04, 0.5), tray, bevel=0.02)
    cyl(0.04, 0.004, (0.5, 0.082, 0.5), mat("drain", (0.15, 0.15, 0.16), 0.4, 1.0))
    # tiled back wall, glass side and front panel with a door gap
    box((0.96, 2.0, 0.03), (0.5, 1.0, 0.015), tile, bevel=0.0)
    box((0.012, 1.9, 0.92), (0.025, 1.03, 0.5), glass, bevel=0.0)
    box((0.45, 1.9, 0.012), (0.255, 1.03, 0.955), glass, bevel=0.0)
    box((0.03, 1.92, 0.03), (0.035, 1.04, 0.955), chrome, bevel=0.008)  # frame post
    box((0.47, 0.025, 0.025), (0.255, 1.99, 0.955), chrome, bevel=0.006)
    box((0.025, 0.025, 0.93), (0.035, 1.99, 0.5), chrome, bevel=0.006)
    # riser pipe, head and mixer
    cyl(0.012, 1.0, (0.5, 1.45, 0.05), chrome)
    cyl(0.01, 0.15, (0.5, 1.95, 0.12), chrome, axis="z")
    cyl(0.09, 0.02, (0.5, 1.94, 0.2), chrome, verts=32)
    cyl(0.035, 0.05, (0.5, 1.05, 0.05), chrome, axis="z")
    box((0.08, 0.015, 0.02), (0.5, 1.05, 0.085), chrome, bevel=0.005)


def bathtub_basic():
    p = m_("porcelain", PORCELAIN)
    water = mat("water", (0.6, 0.78, 0.88), 0.05, alpha=0.75)
    chrome = m_("metal.chrome", CHROME)
    # walls of the tub around a hollow, a rolled rim, and the floor of the basin
    box((0.86, 0.2, 1.84), (0.5, 0.1, 1.0), p, bevel=0.04, segments=3)
    for (sx, sz, x, z) in ((0.86, 0.1, 0.5, 0.13), (0.86, 0.1, 0.5, 1.87), (0.08, 1.84, 0.11, 1.0), (0.08, 1.84, 0.89, 1.0)):
        box((sx, 0.56, sz), (x, 0.28, z), p, bevel=0.035, segments=3)
    box((0.7, 0.03, 1.64), (0.5, 0.48, 1.0), water, bevel=0.0)
    # claw feet
    for x in (0.14, 0.86):
        for z in (0.2, 1.8):
            sphere(0.05, (x, 0.04, z), chrome, scale=(1, 0.8, 1))
    # tap at the foot end
    cyl(0.02, 0.12, (0.5, 0.62, 1.88), chrome)
    cyl(0.016, 0.12, (0.5, 0.67, 1.83), chrome, axis="z")
    for x in (0.42, 0.58):
        cyl(0.025, 0.03, (x, 0.59, 1.88), chrome)


def crib():
    wood = mat("wood", (1.0, 0.93, 0.78), 0.7)
    sheet = mat("fabric", (1.0, 1.0, 0.97), 0.9)
    blanket = mat("fabric.blanket", (0.72, 0.85, 0.95), 0.9)
    # corner posts with knobs, top and bottom rails, spindles on all four sides
    for x in (0.07, 0.93):
        for z in (0.07, 0.93):
            box((0.05, 0.95, 0.05), (x, 0.475, z), wood, bevel=0.01)
            sphere(0.035, (x, 0.97, z), wood)
    for y in (0.3, 0.9):
        for z in (0.07, 0.93):
            box((0.82, 0.04, 0.03), (0.5, y, z), wood, bevel=0.008)
        for x in (0.07, 0.93):
            box((0.03, 0.04, 0.82), (x, y, 0.5), wood, bevel=0.008)
    n = 8
    for i in range(1, n):
        t = 0.07 + 0.86 * i / n
        for z in (0.07, 0.93):
            cyl(0.011, 0.58, (t, 0.6, z), wood, verts=10)
        for x in (0.07, 0.93):
            cyl(0.011, 0.58, (x, 0.6, t), wood, verts=10)
    # head and foot panels a bit higher, mattress, folded blanket, a star mobile
    box((0.86, 0.1, 0.84), (0.5, 0.36, 0.5), sheet, bevel=0.035, segments=3)
    box((0.8, 0.04, 0.35), (0.5, 0.43, 0.7), blanket, bevel=0.018, segments=3)
    box((0.016, 0.45, 0.016), (0.88, 1.15, 0.12), wood, bevel=0.004)
    box((0.016, 0.016, 0.38), (0.88, 1.37, 0.31), wood, bevel=0.004)
    for i, c in enumerate(((1.0, 0.8, 0.3), (0.9, 0.45, 0.5), (0.5, 0.75, 0.55))):
        sphere(0.035, (0.88, 1.2, 0.25 + i * 0.1), mat(f"toy{i}", c, 0.5))
        cyl(0.003, 0.15, (0.88, 1.29, 0.25 + i * 0.1), mat("string", (0.9, 0.9, 0.9), 0.9))


def floor_lamp():
    metal = mat("metal", (0.2, 0.2, 0.2), 0.3, 1.0)
    shade = mat("fabric", (1.0, 1.0, 0.86), 0.9, emit=0.8)
    cyl(0.16, 0.03, (0.5, 0.015, 0.5), metal, bevel=0.01, verts=32)
    cyl(0.045, 0.03, (0.5, 0.045, 0.5), metal)
    cyl(0.013, 1.42, (0.5, 0.74, 0.5), metal, verts=12)
    cyl(0.02, 0.04, (0.5, 1.44, 0.5), metal)
    # tapered drum shade (open cone) and a glowing bulb inside
    bpy.ops.mesh.primitive_cone_add(vertices=32, radius1=0.22, radius2=0.16, depth=0.3, end_fill_type="NOTHING", location=g2b(0.5, 1.55, 0.5))
    o = bpy.context.active_object
    sol = o.modifiers.new("solid", "SOLIDIFY")
    sol.thickness = 0.008
    for poly in o.data.polygons:
        poly.use_smooth = True
    o.data.materials.append(shade)
    sphere(0.05, (0.5, 1.5, 0.5), mat("bulb", (1.0, 0.95, 0.8), 0.3, emit=3.0))


# ------------------------------------------------------------------------------------------------
# third batch


def plant_potted():
    pot = mat("pot", (0.7, 0.4, 0.28), 0.8)
    soil = mat("soil", (0.22, 0.15, 0.1), 1.0)
    leaf = mat("leaf", (0.25, 0.55, 0.25), 0.7)
    leaf2 = mat("leaf.dark", (0.18, 0.42, 0.2), 0.7)
    # tapered terracotta pot with a rim, soil
    bpy.ops.mesh.primitive_cone_add(vertices=32, radius1=0.13, radius2=0.18, depth=0.32, location=g2b(0.5, 0.16, 0.5))
    o = bpy.context.active_object
    _place(o, pot, 0.01, 2)
    cyl(0.195, 0.05, (0.5, 0.315, 0.5), pot, verts=32)
    cyl(0.17, 0.01, (0.5, 0.336, 0.5), soil, bevel=0.0, verts=32)
    # fronds: flattened leaves fanning out and arching from the centre
    n = 11
    for i in range(n):
        a = 2 * math.pi * i / n + (0.3 if i % 2 else 0)
        tilt = 35 + (i % 3) * 12
        length = 0.38 + (i % 4) * 0.05
        bpy.ops.mesh.primitive_uv_sphere_add(segments=12, ring_count=6, radius=1.0)
        f = bpy.context.active_object
        f.scale = (0.05, length / 2, 0.012)
        f.rotation_euler = (math.radians(tilt), 0, a)
        r = math.sin(math.radians(tilt)) * length / 2
        x, z = 0.5 + r * math.sin(a), 0.5 + r * math.cos(a)
        f.location = g2b(x, 0.34 + math.cos(math.radians(tilt)) * length / 2, z)
        f.rotation_euler = (math.radians(-tilt), 0, a)
        for p in f.data.polygons:
            p.use_smooth = True
        f.data.materials.append(leaf if i % 2 else leaf2)
    sphere(0.12, (0.5, 0.42, 0.5), leaf2, scale=(1, 0.8, 1))


def painting():
    wood = mat("wood", (0.36, 0.31, 0.26), 0.6)
    canvas = mat("canvas", (0.92, 0.9, 0.84), 0.9)
    # easel: two front legs, a back leg, a ledge
    for x in (0.25, 0.75):
        box((0.04, 1.75, 0.04), (x, 0.885, 0.46), wood, bevel=0.008, rot_x=6)
    box((0.04, 1.6, 0.04), (0.5, 0.78, 0.25), wood, bevel=0.008, rot_x=-18)
    box((0.7, 0.04, 0.16), (0.5, 0.93, 0.5), wood, bevel=0.008)
    # framed canvas with abstract blobs of a feeling
    box((0.9, 0.7, 0.05), (0.5, 1.3, 0.53), wood, bevel=0.012)
    box((0.8, 0.6, 0.02), (0.5, 1.3, 0.56), canvas, bevel=0.0)
    for (x, y, rx, ry, c) in ((0.35, 1.4, 0.13, 0.1, (0.85, 0.5, 0.2)), (0.62, 1.22, 0.16, 0.12, (0.25, 0.4, 0.7)),
                             (0.45, 1.18, 0.08, 0.08, (0.8, 0.2, 0.3)), (0.68, 1.44, 0.07, 0.09, (0.95, 0.8, 0.25))):
        cyl(1.0, 0.006, (x, y, 0.571), mat(f"paint{c}", c, 0.5), axis="z", bevel=0.0).scale = (rx, 1, ry)


def trash_can():
    steel = mat("metal", (0.45, 0.47, 0.5), 0.4, 0.8)
    dark = mat("pedal", (0.2, 0.2, 0.22), 0.5, 0.5)
    cyl(0.2, 0.6, (0.5, 0.3, 0.5), steel, bevel=0.01, verts=32)
    for y in (0.08, 0.52):
        cyl(0.205, 0.02, (0.5, y, 0.5), steel, bevel=0.004, verts=32)  # ribs
    cyl(0.215, 0.04, (0.5, 0.62, 0.5), steel, bevel=0.012, verts=32)  # lid
    sphere(0.17, (0.5, 0.64, 0.5), steel, scale=(1, 0.18, 1))  # domed top
    box((0.08, 0.02, 0.025), (0.5, 0.67, 0.5), dark, bevel=0.006)  # lid handle
    box((0.12, 0.025, 0.08), (0.5, 0.03, 0.73), dark, bevel=0.006)  # pedal
    box((0.03, 0.03, 0.05), (0.32, 0.6, 0.29), dark, bevel=0.006)  # hinge


def mirror():
    wood = mat("wood", (0.42, 0.31, 0.23), 0.6)
    glass = mat("mirror", (0.85, 0.9, 0.95), 0.02, 1.0)
    # free-standing cheval mirror: two posts on feet, framed glass between them
    for x in (0.17, 0.83):
        box((0.05, 1.7, 0.05), (x, 0.85, 0.1), wood, bevel=0.01)
        box((0.07, 0.05, 0.4), (x, 0.025, 0.2), wood, bevel=0.012)
        sphere(0.035, (x, 1.72, 0.1), wood)
        cyl(0.02, 0.03, (x + (0.035 if x < 0.5 else -0.035), 1.05, 0.13), m_("metal.chrome", CHROME), axis="x")
    box((0.6, 1.5, 0.05), (0.5, 0.97, 0.13), wood, bevel=0.015, segments=3)
    box((0.52, 1.42, 0.01), (0.5, 0.97, 0.16), glass, bevel=0.0)


def toy_box():
    red = mat("toy_box", (0.9, 0.35, 0.3), 0.6)
    trim = mat("toy_box.trim", (0.98, 0.85, 0.3), 0.5)
    box((0.8, 0.45, 0.5), (0.5, 0.25, 0.3), red, bevel=0.03, segments=3)
    for y in (0.06, 0.44):
        box((0.82, 0.04, 0.52), (0.5, y, 0.3), trim, bevel=0.012)
    # open lid leaning back, letter blocks on the front, toys spilling out
    box((0.8, 0.04, 0.5), (0.5, 0.7, 0.03), red, bevel=0.012, rot_x=80)
    for (x, c) in ((0.3, (0.3, 0.5, 0.9)), (0.5, (0.4, 0.75, 0.35)), (0.7, (0.95, 0.85, 0.2))):
        box((0.12, 0.12, 0.012), (x, 0.25, 0.556), mat(f"block{x}", c, 0.5), bevel=0.01)
    sphere(0.1, (0.35, 0.53, 0.3), mat("ball", (0.3, 0.5, 0.9), 0.4))
    box((0.16, 0.16, 0.16), (0.62, 0.54, 0.32), mat("cube", (0.95, 0.85, 0.2), 0.5), bevel=0.02)
    sphere(0.07, (0.8, 0.07, 0.7), mat("ball.red", (0.9, 0.2, 0.25), 0.4))
    box((0.1, 0.1, 0.1), (0.2, 0.05, 0.72), mat("block.green", (0.4, 0.75, 0.35), 0.5), bevel=0.012)


def exercise_bike():
    frame = mat("frame", (0.2, 0.2, 0.22), 0.4, 0.6)
    black = mat("rubber", (0.1, 0.1, 0.1), 0.8)
    wheel = mat("flywheel", (0.8, 0.2, 0.2), 0.3, 0.5)
    screen = mat("screen", (0.3, 0.8, 0.5), 0.2, emit=0.7)
    # base: two stabiliser feet joined by a rail
    for z in (0.15, 0.85):
        box((0.45, 0.06, 0.08), (0.5, 0.03, z), black, bevel=0.015)
    box((0.08, 0.06, 0.75), (0.5, 0.06, 0.5), frame, bevel=0.01)
    # seat post (rider at the back), saddle; front column with console and bars
    box((0.06, 0.8, 0.06), (0.5, 0.45, 0.3), frame, bevel=0.01, rot_x=10)
    box((0.25, 0.06, 0.3), (0.5, 0.88, 0.25), black, bevel=0.025, segments=3)
    box((0.07, 0.95, 0.07), (0.5, 0.5, 0.78), frame, bevel=0.01, rot_x=-12)
    box((0.2, 0.12, 0.05), (0.5, 1.0, 0.84), frame, bevel=0.012)
    box((0.15, 0.08, 0.006), (0.5, 1.0, 0.818), screen, bevel=0.0)
    cyl(0.015, 0.5, (0.5, 0.97, 0.76), frame, axis="x")
    for x in (0.27, 0.73):
        cyl(0.02, 0.12, (x, 0.97, 0.76), black, axis="x")
    # flywheel with a hub cover, crank and pedals
    cyl(0.22, 0.05, (0.5, 0.35, 0.58), wheel, axis="x", bevel=0.01, verts=32)
    cyl(0.07, 0.07, (0.5, 0.35, 0.58), frame, axis="x")
    cyl(0.04, 0.2, (0.5, 0.33, 0.4), frame, axis="x")
    for (x, dy) in ((0.38, 0.09), (0.62, -0.09)):
        box((0.02, 0.2, 0.03), (x, 0.33 + dy / 2, 0.4), frame, bevel=0.005)
        box((0.1, 0.025, 0.06), (x + (-0.05 if x < 0.5 else 0.05), 0.33 + dy, 0.4), black, bevel=0.006)


def chess_table():
    wood = mat("wood", (0.54, 0.37, 0.26), 0.5)
    dark = mat("wood.dark", (0.3, 0.2, 0.14), 0.5)
    cyl(0.35, 0.04, (0.5, 0.72, 0.5), wood, bevel=0.012, verts=40)
    cyl(0.045, 0.62, (0.5, 0.39, 0.5), dark, verts=16)
    cyl(0.22, 0.04, (0.5, 0.02, 0.5), dark, bevel=0.012, verts=32)
    # 8x8 board inlay with a frame, a few pieces mid-game
    box((0.42, 0.012, 0.42), (0.5, 0.745, 0.5), dark, bevel=0.004)
    light = mat("square.light", (0.92, 0.88, 0.78), 0.4)
    black = mat("square.dark", (0.2, 0.15, 0.12), 0.4)
    sq = 0.045
    for i in range(8):
        for j in range(8):
            box((sq, 0.004, sq), (0.5 + (i - 3.5) * sq, 0.752, 0.5 + (j - 3.5) * sq), light if (i + j) % 2 else black, bevel=0.0)
    white_p = mat("piece.white", (0.95, 0.93, 0.88), 0.35)
    black_p = mat("piece.black", (0.08, 0.08, 0.09), 0.3)
    for (i, j, m, h) in ((1, 1, white_p, 0.04), (3, 1, white_p, 0.06), (4, 2, white_p, 0.04), (2, 6, black_p, 0.04), (4, 6, black_p, 0.07), (5, 5, black_p, 0.04)):
        x, z = 0.5 + (i - 3.5) * sq, 0.5 + (j - 3.5) * sq
        cyl(0.014, h, (x, 0.754 + h / 2, z), m, verts=12)
        sphere(0.012, (x, 0.754 + h + 0.008, z), m)
    # toppled king
    cyl(0.012, 0.06, (0.75, 0.76, 0.6), black_p, axis="x", verts=12)


def rug():
    red = mat("fabric", (0.63, 0.23, 0.21), 1.0)
    gold = mat("fabric.gold", (0.86, 0.69, 0.4), 1.0)
    navy = mat("fabric.navy", (0.2, 0.25, 0.45), 1.0)
    box((1.9, 0.01, 1.9), (1.0, 0.005, 1.0), red, bevel=0.004)
    box((1.6, 0.012, 1.6), (1.0, 0.006, 1.0), gold, bevel=0.0)
    box((1.4, 0.014, 1.4), (1.0, 0.007, 1.0), red, bevel=0.0)
    # diamond medallion and corner motifs
    bpy.ops.mesh.primitive_cylinder_add(vertices=4, radius=0.45, depth=0.016, location=g2b(1.0, 0.008, 1.0))
    _place(bpy.context.active_object, navy, 0, 1)
    for (x, z) in ((0.45, 0.45), (1.55, 0.45), (0.45, 1.55), (1.55, 1.55)):
        bpy.ops.mesh.primitive_cylinder_add(vertices=4, radius=0.12, depth=0.016, location=g2b(x, 0.008, z))
        _place(bpy.context.active_object, navy, 0, 1)
    # fringe on the short ends
    for z in (0.03, 1.97):
        for k in range(19):
            box((0.02, 0.006, 0.06), (0.1 + k * 0.1, 0.003, z), gold, bevel=0.0)


def end_table():
    wood = mat("wood", (0.6, 0.43, 0.29), 0.5)
    dark = mat("wood.dark", (0.45, 0.31, 0.21), 0.5)
    box((0.52, 0.04, 0.52), (0.5, 0.55, 0.5), wood, bevel=0.012)
    for (x, z) in ((0.28, 0.28), (0.72, 0.28), (0.28, 0.72), (0.72, 0.72)):
        box((0.04, 0.53, 0.04), (x, 0.265, z), dark, bevel=0.006)
    box((0.44, 0.03, 0.44), (0.5, 0.15, 0.5), wood, bevel=0.008)
    box((0.42, 0.1, 0.42), (0.5, 0.47, 0.5), dark, bevel=0.008)
    box((0.08, 0.02, 0.012), (0.5, 0.47, 0.716), mat("knob", (0.8, 0.65, 0.3), 0.3, 0.8), bevel=0.004)
    # table lamp: base, stem, glowing shade
    cyl(0.07, 0.05, (0.5, 0.595, 0.45), mat("ceramic", (0.3, 0.5, 0.55), 0.3), bevel=0.015)
    cyl(0.01, 0.15, (0.5, 0.69, 0.45), mat("brass", (0.8, 0.65, 0.3), 0.3, 1.0))
    bpy.ops.mesh.primitive_cone_add(vertices=24, radius1=0.11, radius2=0.07, depth=0.13, location=g2b(0.5, 0.79, 0.45))
    _place(bpy.context.active_object, mat("shade", (0.95, 0.88, 0.7), 0.8, emit=0.8), 0, 1)
    box((0.12, 0.03, 0.09), (0.66, 0.585, 0.62), mat("book", (0.3, 0.45, 0.3), 0.7), bevel=0.005)


def kids_desk():
    blue = mat("desk", (0.35, 0.6, 0.85), 0.5)
    side = mat("desk.side", (0.3, 0.5, 0.75), 0.5)
    box((0.9, 0.04, 0.55), (0.5, 0.6, 0.3), blue, bevel=0.015, segments=3)
    for x in (0.08, 0.92):
        box((0.04, 0.58, 0.5), (x, 0.29, 0.3), side, bevel=0.01)
    box((0.8, 0.25, 0.02), (0.5, 0.45, 0.06), side, bevel=0.006)
    # drawer cube on the right with a yellow handle
    box((0.25, 0.2, 0.45), (0.78, 0.48, 0.3), side, bevel=0.008)
    box((0.08, 0.02, 0.012), (0.78, 0.48, 0.53), mat("handle", (0.98, 0.8, 0.25), 0.4), bevel=0.004)
    # open notebook, pencil cup with pencils, gum under the top
    box((0.3, 0.012, 0.22), (0.4, 0.626, 0.35), mat("paper", (0.95, 0.95, 0.9), 0.8), bevel=0.003)
    box((0.004, 0.014, 0.22), (0.4, 0.628, 0.35), mat("spine", (0.85, 0.3, 0.3), 0.6), bevel=0.0)
    cyl(0.04, 0.12, (0.75, 0.68, 0.2), mat("cup", (0.9, 0.5, 0.2), 0.5))
    for (dx, c) in ((-0.015, (0.95, 0.85, 0.2)), (0.012, (0.3, 0.6, 0.9)), (0.0, (0.9, 0.3, 0.3))):
        cyl(0.006, 0.16, (0.75 + dx, 0.78, 0.2 + dx), mat(f"pencil{c[0]}", c, 0.5), verts=6)
    sphere(0.025, (0.3, 0.57, 0.4), mat("gum", (0.95, 0.55, 0.7), 0.6), scale=(1, 0.5, 1))


def tombstone():
    stone = mat("stone", (0.6, 0.6, 0.62), 0.85)
    base = mat("stone.dark", (0.45, 0.45, 0.47), 0.9)
    box((0.6, 0.08, 0.35), (0.5, 0.04, 0.5), base, bevel=0.015)
    box((0.5, 0.7, 0.14), (0.5, 0.43, 0.5), stone, bevel=0.012)
    cyl(0.25, 0.14, (0.5, 0.78, 0.5), stone, axis="z", bevel=0.012, verts=32)
    # carved plaque and RIP lettering bars
    engr = mat("engraving", (0.35, 0.35, 0.37), 0.9)
    box((0.32, 0.22, 0.01), (0.5, 0.55, 0.572), engr, bevel=0.0)
    box((0.04, 0.12, 0.012), (0.5, 0.86, 0.572), engr, bevel=0.0)
    box((0.1, 0.035, 0.012), (0.5, 0.88, 0.572), engr, bevel=0.0)
    # flowers at the foot, grass tufts
    stem = mat("stem", (0.25, 0.5, 0.2), 0.7)
    for (x, c) in ((0.3, (0.85, 0.25, 0.4)), (0.36, (0.95, 0.85, 0.3)), (0.25, (0.9, 0.9, 0.95))):
        cyl(0.006, 0.12, (x, 0.06, 0.72), stem, verts=6)
        sphere(0.035, (x, 0.13, 0.72), mat(f"flower{x}", c, 0.5))
    for (x, z) in ((0.22, 0.4), (0.8, 0.62), (0.72, 0.38)):
        sphere(0.05, (x, 0.02, z), mat("grass", (0.3, 0.55, 0.25), 0.9), scale=(1, 0.5, 1))


def treadmill():
    body = mat("body", (0.15, 0.15, 0.17), 0.5, 0.3)
    belt = mat("belt", (0.08, 0.08, 0.08), 1.0)
    post = mat("post", (0.25, 0.25, 0.28), 0.35, 0.6)
    # deck with side rails and a motor hood at the front (-z), rollers at both ends
    box((0.7, 0.1, 1.7), (0.5, 0.06, 1.05), body, bevel=0.02)
    box((0.55, 0.02, 1.5), (0.5, 0.12, 1.05), belt, bevel=0.0)
    for x in (0.17, 0.83):
        box((0.08, 0.03, 1.55), (x, 0.12, 1.07), mat("rail", (0.3, 0.3, 0.33), 0.5), bevel=0.008)
    box((0.7, 0.18, 0.25), (0.5, 0.12, 0.2), mat("hood", (0.2, 0.2, 0.22), 0.4, 0.3), bevel=0.04, segments=3)
    for z in (0.32, 1.85):
        cyl(0.04, 0.6, (0.5, 0.08, z), post, axis="x")
    # uprights, console and handrails
    for x in (0.17, 0.83):
        box((0.06, 1.1, 0.06), (x, 0.65, 0.18), post, bevel=0.01, rot_x=-8)
        cyl(0.018, 0.4, (x, 1.0, 0.4), mat("grip", (0.1, 0.1, 0.1), 0.8), axis="z")
    box((0.72, 0.22, 0.12), (0.5, 1.22, 0.14), body, bevel=0.03, segments=3, rot_x=-25)
    box((0.3, 0.12, 0.01), (0.5, 1.24, 0.21), mat("screen", (0.3, 0.8, 0.5), 0.2, emit=0.7), bevel=0.0, rot_x=-25)
    for x in (0.32, 0.68):
        cyl(0.02, 0.01, (x, 1.2, 0.2), mat("button", (0.9, 0.3, 0.2), 0.4), axis="z", verts=12)


def weight_bench():
    pad = mat("pad", (0.6, 0.12, 0.12), 0.55)
    steel = mat("steel", (0.2, 0.2, 0.22), 0.35, 0.7)
    bar = mat("metal", (0.7, 0.7, 0.72), 0.3, 1.0)
    plate = mat("plate", (0.1, 0.1, 0.1), 0.6, 0.2)
    box((0.35, 0.08, 1.2), (0.5, 0.45, 1.0), pad, bevel=0.03, segments=3)
    box((0.12, 0.04, 1.1), (0.5, 0.39, 1.0), steel, bevel=0.006)
    # legs with T-feet
    for z in (0.55, 1.45):
        box((0.06, 0.38, 0.06), (0.5, 0.2, z), steel, bevel=0.008)
        box((0.45, 0.04, 0.08), (0.5, 0.02, z), steel, bevel=0.01)
    # rack uprights with J-hooks, base, barbell with plates and collars
    for x in (0.12, 0.88):
        box((0.06, 1.0, 0.06), (x, 0.5, 0.3), steel, bevel=0.01)
        box((0.08, 0.04, 0.5), (x, 0.02, 0.3), steel, bevel=0.01)
        box((0.05, 0.08, 0.05), (x, 0.93, 0.34), steel, bevel=0.008)
    cyl(0.015, 0.98, (0.5, 0.97, 0.3), bar, axis="x")
    for s in (-1, 1):
        cyl(0.15, 0.04, (0.5 + s * 0.44, 0.97, 0.3), plate, axis="x", bevel=0.008, verts=32)
        cyl(0.1, 0.035, (0.5 + s * 0.475, 0.97, 0.3), plate, axis="x", bevel=0.008, verts=28)
        cyl(0.03, 0.03, (0.5 + s * 0.4, 0.97, 0.3), bar, axis="x", verts=16)
    # dumbbell on the floor, towel on the bench end
    cyl(0.01, 0.2, (0.85, 0.06, 1.7), bar, axis="x")
    for dx in (-0.08, 0.08):
        cyl(0.05, 0.04, (0.85 + dx, 0.06, 1.7), plate, axis="x", verts=6)
    box((0.3, 0.02, 0.2), (0.5, 0.5, 1.5), mat("towel", (0.95, 0.95, 0.95), 0.9), bevel=0.008)


def espresso_bar():
    wood = mat("wood", (0.36, 0.25, 0.18), 0.6)
    top = mat("top", (0.85, 0.85, 0.82), 0.3)
    steel = mat("metal", (0.75, 0.75, 0.78), 0.22, 1.0)
    black = mat("black", (0.1, 0.1, 0.11), 0.4)
    box((0.95, 0.88, 0.6), (0.5, 0.44, 0.35), wood, bevel=0.015)
    for x in (0.27, 0.73):
        box((0.4, 0.7, 0.01), (x, 0.47, 0.655), mat("wood.panel", (0.42, 0.3, 0.21), 0.6), bevel=0.004)
    box((0.95, 0.06, 0.03), (0.5, 0.03, 0.66), black, bevel=0.005)
    box((1.0, 0.04, 0.65), (0.5, 0.9, 0.35), top, bevel=0.01)
    # machine: body, top cup tray, group heads, steam wand, gauge, drip tray
    box((0.5, 0.36, 0.33), (0.5, 1.1, 0.24), steel, bevel=0.03, segments=3)
    box((0.46, 0.03, 0.3), (0.5, 1.3, 0.24), black, bevel=0.006)
    for x in (0.4, 0.6):
        cyl(0.04, 0.06, (x, 1.03, 0.43), black, verts=16)
        box((0.025, 0.02, 0.12), (x, 1.05, 0.52), black, bevel=0.005)
    cyl(0.008, 0.18, (0.72, 1.0, 0.43), steel, verts=8)
    cyl(0.04, 0.01, (0.5, 1.2, 0.408), mat("gauge", (0.95, 0.95, 0.9), 0.3), axis="z", verts=20)
    box((0.42, 0.02, 0.12), (0.5, 0.93, 0.47), black, bevel=0.004)
    cup = mat("cup", (0.95, 0.93, 0.88), 0.3)
    for x in (0.4, 0.6):
        cyl(0.03, 0.06, (x, 0.97, 0.47), cup, verts=16)
    for (x, y) in ((0.42, 1.34), (0.5, 1.34), (0.58, 1.34)):
        cyl(0.03, 0.06, (x, y, 0.2), cup, verts=16)
    # bean grinder with a hopper
    box((0.12, 0.2, 0.14), (0.15, 1.02, 0.3), black, bevel=0.01)
    bpy.ops.mesh.primitive_cone_add(vertices=16, radius1=0.04, radius2=0.07, depth=0.12, location=g2b(0.15, 1.18, 0.3))
    _place(bpy.context.active_object, mat("hopper", (0.45, 0.3, 0.2), 0.2, alpha=0.7), 0, 1)


def vending_machine():
    red = mat("shell", (0.7, 0.12, 0.1), 0.35, 0.2)
    dark = mat("dark", (0.15, 0.15, 0.16), 0.4, 0.5)
    box((0.9, 1.8, 0.7), (0.5, 0.9, 0.4), red, bevel=0.03, segments=3)
    box((0.86, 0.15, 0.02), (0.5, 1.68, 0.76), mat("sign", (0.98, 0.9, 0.5), 0.3, emit=0.8), bevel=0.0)
    # window into lit shelves of snacks
    box((0.6, 1.15, 0.05), (0.38, 1.0, 0.73), mat("interior", (0.85, 0.88, 0.9), 0.5, emit=0.4), bevel=0.0)
    cols = ((0.9, 0.3, 0.2), (0.95, 0.8, 0.2), (0.3, 0.6, 0.9), (0.4, 0.8, 0.35), (0.8, 0.4, 0.8))
    for r in range(5):
        y = 0.52 + r * 0.22
        box((0.58, 0.012, 0.06), (0.38, y, 0.74), steel_m := mat("shelf", (0.7, 0.7, 0.72), 0.3, 1.0), bevel=0.0)
        for c in range(4):
            box((0.1, 0.14, 0.03), (0.15 + c * 0.15, y + 0.08, 0.74), mat(f"snack{(r + c) % 5}", cols[(r + c) % 5], 0.5), bevel=0.006)
    box((0.62, 1.17, 0.015), (0.38, 1.0, 0.765), mat("glass", (0.7, 0.85, 0.9), 0.05, alpha=0.3), bevel=0.0)
    # keypad panel, coin slot, dispensing flap; a snack stuck mid-fall
    box((0.18, 0.5, 0.02), (0.82, 1.25, 0.76), dark, bevel=0.006)
    for r in range(4):
        for c in range(3):
            box((0.035, 0.035, 0.01), (0.77 + c * 0.05, 1.38 - r * 0.06, 0.772), mat("key", (0.85, 0.85, 0.85), 0.4), bevel=0.003)
    box((0.08, 0.06, 0.02), (0.82, 1.1, 0.772), mat("display", (0.3, 0.9, 0.4), 0.2, emit=0.8), bevel=0.0)
    box((0.02, 0.06, 0.01), (0.82, 1.02, 0.772), mat("slot", (0.05, 0.05, 0.05), 0.5), bevel=0.0)
    box((0.55, 0.18, 0.04), (0.38, 0.3, 0.75), mat("flap", (0.08, 0.08, 0.09), 0.3), bevel=0.01)
    box((0.1, 0.14, 0.03), (0.45, 0.68, 0.74), mat("stuck", (0.95, 0.8, 0.2), 0.5), bevel=0.006, rot_x=25)
    for x in (0.1, 0.9):
        box((0.08, 0.04, 0.6), (x, 0.02, 0.4), dark, bevel=0.008)


def wedding_cake():
    wood = mat("wood", (0.55, 0.36, 0.22), 0.6)
    cloth = mat("fabric", (0.97, 0.96, 0.94), 0.9)
    box((0.7, 0.04, 0.7), (0.5, 0.7, 0.5), wood, bevel=0.012)
    for (x, z) in ((0.2, 0.2), (0.8, 0.2), (0.2, 0.8), (0.8, 0.8)):
        box((0.05, 0.68, 0.05), (x, 0.34, z), wood, bevel=0.008)
    box((0.72, 0.015, 0.72), (0.5, 0.727, 0.5), cloth, bevel=0.004)
    for z in (0.135, 0.865):
        box((0.72, 0.15, 0.01), (0.5, 0.66, z), cloth, bevel=0.0)
    # cake stand, three leaning tiers with piped trim, strawberry and a little topper
    cyl(0.04, 0.06, (0.5, 0.765, 0.5), mat("silver", (0.8, 0.8, 0.82), 0.2, 1.0))
    cyl(0.28, 0.012, (0.5, 0.8, 0.5), mat("silver", (0.8, 0.8, 0.82), 0.2, 1.0), verts=32)
    tiers = ((0.25, 0.17, 0.5, (0.97, 0.95, 0.9)), (0.18, 0.15, 0.515, (0.97, 0.93, 0.92)), (0.11, 0.13, 0.535, (0.98, 0.9, 0.92)))
    y = 0.806
    trim = mat("trim", (0.95, 0.75, 0.8), 0.6)
    for (r, h, x, c) in tiers:
        cyl(r, h, (x, y + h / 2, 0.5), mat(f"tier{r}", c, 0.7), bevel=0.02, verts=32)
        bpy.ops.mesh.primitive_torus_add(major_radius=r, minor_radius=0.012, location=g2b(x, y + 0.01, 0.5))
        _place(bpy.context.active_object, trim, 0, 1)
        y += h
    sphere(0.035, (0.535, y + 0.03, 0.5), mat("berry", (0.85, 0.2, 0.3), 0.4))
    for (dx, c) in ((-0.03, (0.1, 0.1, 0.1)), (0.03, (0.97, 0.97, 0.97))):
        cyl(0.012, 0.08, (0.535 + dx, y + 0.04, 0.45), mat(f"topper{c[0]}", c, 0.5), verts=10)
        sphere(0.012, (0.535 + dx, y + 0.095, 0.45), mat("skin", (0.9, 0.75, 0.6), 0.6))


# Coordinated walnut / sage entryway collection.
def entry_palette():
    return (mat("wood", (0.48, 0.29, 0.18)),
            mat("sage", (0.48, 0.61, 0.48), 0.7),
            mat("brass", (0.77, 0.59, 0.29), 0.3, 0.8),
            mat("fabric", (0.88, 0.83, 0.7), 0.95))


def cabinet_feet(width, wood):
    for x in (0.16, width - 0.16):
        for z in (0.2, 0.7):
            cyl(0.035, 0.22, (x, 0.11, z), wood, verts=12)


def sideboard():
    wood, sage, brass, cream = entry_palette()
    cabinet_feet(2, wood)
    box((1.86, 0.58, 0.64), (1, 0.5, 0.44), wood, 0.025)
    box((1.94, 0.05, 0.72), (1, 0.815, 0.44), wood, 0.02)
    for x in (0.39, 0.99, 1.59):
        box((0.57, 0.51, 0.035), (x, 0.51, 0.777), sage, 0.012)
        cyl(0.018, 0.035, (x + 0.17, 0.55, 0.81), brass, axis="z", verts=12)
    # Small ceramic vase and a stack of linen-bound books.
    cyl(0.095, 0.19, (0.32, 0.935, 0.42), cream)
    cyl(0.05, 0.09, (0.32, 1.065, 0.42), cream)
    for i, m in enumerate((sage, cream, wood)):
        box((0.36 - i * 0.025, 0.045, 0.27), (1.53, 0.865 + i * 0.045, 0.45), m, 0.005)


def coffee_table():
    wood, sage, brass, cream = entry_palette()
    for x in (0.18, 1.82):
        for z in (0.2, 0.8):
            cyl(0.035, 0.42, (x, 0.21, z), wood, verts=12)
    box((1.85, 0.04, 0.72), (1, 0.19, 0.5), sage, 0.015)
    box((1.96, 0.065, 0.9), (1, 0.455, 0.5), wood, 0.03, 3)
    box((0.34, 0.035, 0.25), (0.62, 0.505, 0.48), sage, 0.006)
    box((0.29, 0.025, 0.22), (0.64, 0.535, 0.48), cream, 0.004)
    cyl(0.12, 0.018, (1.4, 0.497, 0.5), brass)
    cyl(0.065, 0.09, (1.4, 0.55, 0.5), cream)
    cyl(0.052, 0.004, (1.4, 0.597, 0.5), mat("tea", (0.25, 0.12, 0.05)))


def shoe_bench():
    wood, sage, brass, cream = entry_palette()
    cabinet_feet(2, wood)
    box((1.85, 0.04, 0.66), (1, 0.18, 0.45), wood)
    for x in (0.1, 1, 1.9):
        box((0.045, 0.28, 0.66), (x, 0.34, 0.45), wood)
    box((1.94, 0.05, 0.72), (1, 0.49, 0.45), wood, 0.02)
    for x in (0.54, 1.46):
        box((0.86, 0.12, 0.66), (x, 0.575, 0.45), sage, 0.045, 4)
    for x in (0.35, 0.58, 1.3, 1.53):
        sphere(0.1, (x, 0.255, 0.53), cream, (0.8, 0.6, 1.6))
        box((0.16, 0.025, 0.31), (x, 0.205, 0.53), wood, 0.01)


def coat_rack():
    wood, sage, brass, cream = entry_palette()
    cyl(0.29, 0.05, (0.5, 0.025, 0.5), wood, verts=32)
    cyl(0.045, 1.68, (0.5, 0.88, 0.5), wood, verts=16)
    sphere(0.065, (0.5, 1.76, 0.5), brass)
    for x, z in ((0.24, 0.5), (0.76, 0.5), (0.5, 0.24), (0.5, 0.76)):
        box((abs(x - 0.5) + 0.04, 0.035, abs(z - 0.5) + 0.04),
            ((x + 0.5) / 2, 1.49, (z + 0.5) / 2), brass)
        cyl(0.02, 0.12, (x, 1.54, z), brass, verts=12)
        sphere(0.035, (x, 1.61, z), wood)
    # A draped scarf with contrasting hem.
    box((0.17, 0.63, 0.045), (0.74, 1.22, 0.52), sage, 0.018)
    box((0.17, 0.035, 0.05), (0.74, 0.94, 0.52), cream, 0.005)


def console_radio():
    wood, sage, brass, cream = entry_palette()
    cabinet_feet(1, wood)
    box((0.86, 0.63, 0.58), (0.5, 0.535, 0.44), wood, 0.04, 3)
    box((0.73, 0.44, 0.025), (0.5, 0.5, 0.744), cream, 0.018)
    for i in range(13):
        box((0.012, 0.37, 0.012), (0.17 + i * 0.038, 0.5, 0.763), wood, 0.002)
    box((0.13, 0.07, 0.02), (0.78, 0.61, 0.767), sage, 0.005)
    for y in (0.4, 0.51):
        cyl(0.035, 0.03, (0.78, y, 0.78), brass, axis="z", verts=16)
    box((0.84, 0.03, 0.56), (0.5, 0.865, 0.44), wood, 0.015)


BUILDERS = {
    f.__name__: f
    for f in (
        fridge_basic, stove_basic, counter_basic, kitchen_sink, toilet_basic, bed_single, bed_double, sofa_basic, dining_chair, dining_table,
        armchair, tv_basic, bookshelf, computer_desk, shower_basic, bathtub_basic, crib, floor_lamp,
        plant_potted, painting, trash_can, mirror, toy_box, exercise_bike,
        chess_table, rug, end_table, kids_desk, tombstone, treadmill, weight_bench, espresso_bar, vending_machine,
        wedding_cake, sideboard, coffee_table, shoe_bench, coat_rack, console_radio,
    )
}


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
