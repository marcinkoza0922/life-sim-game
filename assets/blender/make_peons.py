"""Generates the rigged peon base meshes, one glTF per life stage, into assets/models/.

Run headless from the repo root:

    blender -b --factory-startup -P assets/blender/make_peons.py

Every stage file holds the same skeleton (bone names), a body mesh with `slim`/`heavy` shape keys,
four hair meshes (`hair0`..`hair3`, rigid on the head bone), and the animation clips the game maps
poses to: idle, walk, sit, lie, use, dance, exercise; walking bodies (toddler and up) also get the
interaction clips the data names (`anim:` in objects.ron / socials.ron) and demeanor variants
(idle_sad, walk_sad, idle_proud, walk_proud, idle_nervous). The game falls back to the pose clip
for any clip a body lacks.

Materials are named after the palette slot the game tints them with (skin, hair, shirt, pants,
shoes); `eye_white`, `pupil` and `mouth` keep their own colour. Blender faces -Y, so the exported
model faces glTF +Z, which is the game's forward.
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "models")
FPS = 30

# Proportions as fractions of the stage height H. Heights match sim::data::LifeStage::height().
STAGES = {
    "infant": dict(H=0.55, head=0.36, legs=0.27, width=0.36, depth=0.25, limb=0.10, foot=0.13),
    "toddler": dict(H=0.85, head=0.30, legs=0.32, width=0.29, depth=0.18, limb=0.08, foot=0.13),
    "child": dict(H=1.25, head=0.235, legs=0.45, width=0.235, depth=0.135, limb=0.064, foot=0.13),
    "teen": dict(H=1.60, head=0.195, legs=0.47, width=0.225, depth=0.125, limb=0.058, foot=0.135),
    "adult": dict(H=1.75, head=0.18, legs=0.47, width=0.23, depth=0.125, limb=0.058, foot=0.135),
    "elder": dict(H=1.68, head=0.18, legs=0.46, width=0.235, depth=0.13, limb=0.058, foot=0.135, hunch=True),
}

PALETTE = {
    # tinted per peon by the game (white base so the tint is the colour)
    "skin": (1, 1, 1, 1),
    "hair": (1, 1, 1, 1),
    "shirt": (1, 1, 1, 1),
    "pants": (1, 1, 1, 1),
    "shoes": (1, 1, 1, 1),
    # fixed colours (sRGB)
    "eye_white": (0.95, 0.95, 0.93, 1),
    "pupil": (0.06, 0.05, 0.07, 1),
    "mouth": (0.55, 0.22, 0.24, 1),
}

ARM_SPREAD = math.radians(32)  # A-pose: upper arms angle out from vertical by this much


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = FPS
    for name, srgb in PALETTE.items():
        col = tuple(c**2.2 for c in srgb[:3]) + (srgb[3],)  # glTF base colours are linear
        m = bpy.data.materials.new(name)
        m.diffuse_color = col
        m.use_nodes = True
        bsdf = m.node_tree.nodes.get("Principled BSDF")
        bsdf.inputs["Base Color"].default_value = col
        bsdf.inputs["Roughness"].default_value = 0.4 if name in ("eye_white", "pupil") else 0.75


def link(obj):
    bpy.context.scene.collection.objects.link(obj)
    return obj


def select_only(*objs):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]


# ------------------------------------------------------------------------------------------------
# skeleton


class Dims:
    def __init__(self, p):
        H = self.H = p["H"]
        self.head_h = H * p["head"]
        self.legs = H * p["legs"]
        self.torso = H - self.head_h - self.legs
        self.width = H * p["width"]
        self.depth = H * p["depth"]
        self.limb = H * p["limb"]
        self.foot_len = H * p["foot"]
        self.hunch = p.get("hunch", False)
        self.hip_x = self.width * 0.22
        self.ankle_z = H * 0.045
        self.knee_z = self.ankle_z + (self.legs - self.ankle_z) * 0.5
        self.shoulder_z = self.legs + self.torso * 0.86
        self.shoulder_x = self.width * 0.5 - self.limb * 0.2
        self.arm_len = self.torso * 0.92
        self.neck_z = self.legs + self.torso
        self.head_c = H - self.head_h * 0.5
        self.thigh_r = self.limb * 0.62
        self.arm_r = self.limb * 0.45


def arm_points(d, side):
    """Shoulder, elbow, wrist, fingertip for side = +1 (left, +X) or -1."""
    dirv = Vector((side * math.sin(ARM_SPREAD), 0, -math.cos(ARM_SPREAD)))
    sh = Vector((side * d.shoulder_x, 0, d.shoulder_z))
    el = sh + dirv * d.arm_len * 0.5
    wr = sh + dirv * d.arm_len
    tip = wr + dirv * d.H * 0.085
    return sh, el, wr, tip


def leg_points(d, side):
    hip = Vector((side * d.hip_x, 0, d.legs))
    knee = Vector((side * d.hip_x, -d.limb * 0.05, d.knee_z))
    ankle = Vector((side * d.hip_x, 0, d.ankle_z))
    toe = Vector((side * d.hip_x, -d.foot_len * 0.85, d.H * 0.012))
    return hip, knee, ankle, toe


def make_armature(d):
    arm_data = bpy.data.armatures.new("rig")
    rig = link(bpy.data.objects.new("rig", arm_data))
    select_only(rig)
    bpy.ops.object.mode_set(mode="EDIT")
    eb = arm_data.edit_bones

    def bone(name, head, tail, parent=None, connect=False):
        b = eb.new(name)
        b.head, b.tail = head, tail
        b.roll = 0.0
        if parent:
            b.parent = eb[parent]
            b.use_connect = connect
        return b

    z0 = d.legs
    bone("hips", (0, 0, z0), (0, 0, z0 + d.torso * 0.3))
    bone("spine", (0, 0, z0 + d.torso * 0.3), (0, 0, z0 + d.torso * 0.62), "hips", True)
    bone("chest", (0, 0, z0 + d.torso * 0.62), (0, 0, d.neck_z), "spine", True)
    bone("neck", (0, 0, d.neck_z), (0, 0, d.neck_z + d.head_h * 0.22), "chest", True)
    bone("head", (0, 0, d.neck_z + d.head_h * 0.22), (0, 0, d.H), "neck", True)
    for side, sfx in ((1, "L"), (-1, "R")):
        sh, el, wr, tip = arm_points(d, side)
        bone(f"upper_arm.{sfx}", sh, el, "chest")
        bone(f"forearm.{sfx}", el, wr, f"upper_arm.{sfx}", True)
        bone(f"hand.{sfx}", wr, tip, f"forearm.{sfx}", True)
        hip, knee, ankle, toe = leg_points(d, side)
        bone(f"thigh.{sfx}", hip, knee, "hips")
        bone(f"shin.{sfx}", knee, ankle, f"thigh.{sfx}", True)
        bone(f"foot.{sfx}", ankle, toe, f"shin.{sfx}", True)
    bpy.ops.object.mode_set(mode="OBJECT")
    for pb in rig.pose.bones:
        pb.rotation_mode = "QUATERNION"
    return rig


# ------------------------------------------------------------------------------------------------
# body


def make_body_skin(d):
    """Skin-modifier body: one connected quad mesh from a stick figure."""
    verts, radii, edges = [], [], []

    def v(p, rx, ry=None):
        verts.append(tuple(p))
        radii.append((rx, ry if ry is not None else rx))
        return len(verts) - 1

    w, dp = d.width, d.depth
    pelvis = v((0, 0, d.legs + d.torso * 0.02), w * 0.40, dp * 0.44)
    waist = v((0, 0, d.legs + d.torso * 0.38), w * 0.34, dp * 0.38)
    chest = v((0, 0, d.legs + d.torso * 0.72), w * 0.43, dp * 0.5)
    neck0 = v((0, 0, d.neck_z - d.torso * 0.02), d.limb * 0.55)
    neck1 = v((0, 0, d.neck_z + d.head_h * 0.3), d.limb * 0.48)
    edges += [(pelvis, waist), (waist, chest), (chest, neck0), (neck0, neck1)]
    for side in (1, -1):
        sh, el, wr, tip = arm_points(d, side)
        s = v(sh, d.arm_r * 1.25)
        e = v(el, d.arm_r * 0.95)
        wv = v(wr, d.arm_r * 0.75)
        hand = v(wr.lerp(tip, 0.55), d.arm_r * 0.95, d.arm_r * 0.6)
        t = v(tip, d.arm_r * 0.55, d.arm_r * 0.4)
        edges += [(chest, s), (s, e), (e, wv), (wv, hand), (hand, t)]
        hip, knee, ankle, toe = leg_points(d, side)
        h = v(hip, d.thigh_r * 1.15)
        k = v(knee, d.thigh_r * 0.78)
        a = v(ankle, d.thigh_r * 0.58)
        ball = v(ankle.lerp(toe, 0.55) * Vector((1, 1, 0)) + Vector((0, 0, d.ankle_z * 0.5)), d.thigh_r * 0.55, d.thigh_r * 0.42)
        t = v(toe * Vector((1, 1, 0)) + Vector((0, 0, d.ankle_z * 0.42)), d.thigh_r * 0.45, d.thigh_r * 0.36)
        edges += [(pelvis, h), (h, k), (k, a), (a, ball), (ball, t)]

    me = bpy.data.meshes.new("body")
    me.from_pydata(verts, edges, [])
    obj = link(bpy.data.objects.new("body", me))
    select_only(obj)
    mod = obj.modifiers.new("skin", "SKIN")
    mod.use_smooth_shade = True
    mod.branch_smoothing = 0.6
    for i, (rx, ry) in enumerate(radii):
        sv = me.skin_vertices[0].data[i]
        sv.radius = (rx, ry)
        sv.use_root = i == pelvis
    sub = obj.modifiers.new("sub", "SUBSURF")
    sub.levels = 2
    sub.render_levels = 2
    bpy.ops.object.modifier_apply(modifier="skin")
    bpy.ops.object.modifier_apply(modifier="sub")
    bpy.ops.object.shade_smooth()
    return obj


def segment_dist(p, a, b):
    ab = b - a
    t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-9)))
    return (p - (a + ab * t)).length, t


def bone_segments(rig):
    return {b.name: (b.head_local.copy(), b.tail_local.copy()) for b in rig.data.bones}


def nearest_bone(p, segs, names=None):
    best, bt, bn = 1e9, 0.0, None
    for n, (a, b) in segs.items():
        if names and n not in names:
            continue
        dd, t = segment_dist(p, a, b)
        if dd < best:
            best, bt, bn = dd, t, n
    return bn, bt


def assign_body_materials(body, rig, d):
    """Clothing by region. Edge loops are cut exactly at the belt, sleeve and shoe lines first, so
    the colour boundaries are straight instead of following the subdivided faces."""
    me = body.data
    for name in ("skin", "shirt", "pants", "shoes"):
        me.materials.append(bpy.data.materials[name])
    idx = {m.name: i for i, m in enumerate(me.materials)}
    segs = bone_segments(rig)
    belt = d.legs + d.torso * 0.16
    shoe_top = d.ankle_z + d.H * 0.035
    sleeves = {}
    for side, sfx in ((1, "L"), (-1, "R")):
        sh, el, _, _ = arm_points(d, side)
        sleeves[sfx] = (sh.lerp(el, 0.75), (el - sh).normalized())

    bm = bmesh.new()
    bm.from_mesh(me)

    def region(f):
        bn, _ = nearest_bone(f.calc_center_median(), segs)
        return bn

    def cut(pred, co, no):
        faces = [f for f in bm.faces if pred(region(f))]
        geom = list({e for f in faces for e in f.edges}) + faces + list({v for f in faces for v in f.verts})
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=co, plane_no=no)

    is_arm = lambda bn: bn.startswith(("upper_arm", "forearm", "hand"))
    cut(lambda bn: not is_arm(bn) and bn not in ("head", "neck"), (0, 0, belt), (0, 0, 1))
    cut(lambda bn: bn.startswith(("shin", "foot")), (0, 0, shoe_top), (0, 0, 1))
    for sfx, (co, no) in sleeves.items():
        cut(lambda bn, sfx=sfx: bn in (f"upper_arm.{sfx}", f"forearm.{sfx}"), co, no)

    for f in bm.faces:
        c = f.calc_center_median()
        bn = region(f)
        if is_arm(bn):
            co, no = sleeves[bn[-1]]
            m = "shirt" if bn.startswith("upper_arm") and (c - co).dot(no) < 0 else "skin"
        elif bn in ("head", "neck"):
            m = "skin"
        elif c.z < shoe_top:
            m = "shoes"
        elif c.z < belt:
            m = "pants"
        else:
            m = "shirt"
        f.material_index = idx[m]
        f.smooth = True
    bm.to_mesh(me)
    bm.free()


def weight_body(body, rig):
    """Bone-heat automatic weights; any vertex left unweighted gets its nearest bone."""
    select_only(rig)
    body.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    segs = bone_segments(rig)
    groups = {g.index: g.name for g in body.vertex_groups}
    missing = 0
    for vtx in body.data.vertices:
        if sum(g.weight for g in vtx.groups if g.group in groups) < 1e-4:
            bn, _ = nearest_bone(vtx.co, segs)
            body.vertex_groups[bn].add([vtx.index], 1.0, "REPLACE")
            missing += 1
    if missing:
        print(f"  {missing} vertices had no heat weights; used nearest bone")


def vertex_bone_weights(body):
    names = {g.index: g.name for g in body.vertex_groups}
    out = []
    for vtx in body.data.vertices:
        ws = {names[g.group]: g.weight for g in vtx.groups if g.weight > 0}
        tot = sum(ws.values()) or 1.0
        out.append({k: w / tot for k, w in ws.items()})
    return out


# body-shape offsets along the normal, in fractions of H, per bone: (slim, heavy)
BUILD = {
    "hips": (-0.010, 0.032),
    "spine": (-0.014, 0.045),
    "chest": (-0.010, 0.030),
    "neck": (-0.004, 0.012),
    "upper_arm": (-0.006, 0.016),
    "forearm": (-0.004, 0.010),
    "hand": (-0.001, 0.004),
    "thigh": (-0.009, 0.026),
    "shin": (-0.004, 0.012),
    "foot": (0.0, 0.002),
    "head": (0.0, 0.0),
}


def add_build_keys(body, d):
    weights = vertex_bone_weights(body)
    body.shape_key_add(name="Basis")
    me = body.data
    for key_i, key_name in ((0, "slim"), (1, "heavy")):
        sk = body.shape_key_add(name=key_name, from_mix=False)
        for vtx, ws in zip(me.vertices, weights):
            off = 0.0
            for bn, w in ws.items():
                off += w * BUILD[bn.split(".")[0]][key_i]
            n = vtx.normal
            if key_i == 1 and ws.get("spine", 0) + ws.get("hips", 0) > 0.3 and n.y < 0:
                off += d.H * 0.02 * (-n.y) * (ws.get("spine", 0) + ws.get("hips", 0) * 0.5)  # belly
            sk.data[vtx.index].co = vtx.co + n * off * d.H


# ------------------------------------------------------------------------------------------------
# head, face, hair (rigid on the head bone)


def uv_sphere(name, center, radii, mat, segs=24, rings=14):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segs, v_segments=rings, radius=1.0)
    for vert in bm.verts:
        vert.co = Vector((vert.co.x * radii[0], vert.co.y * radii[1], vert.co.z * radii[2])) + Vector(center)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    for p in me.polygons:
        p.use_smooth = True
    me.materials.append(bpy.data.materials[mat])
    return link(bpy.data.objects.new(name, me))


def make_head_parts(d):
    hh = d.head_h
    c = Vector((0, 0, d.head_c))
    parts = [uv_sphere("head", c, (hh * 0.43, hh * 0.46, hh * 0.5), "skin", 32, 20)]
    for side in (1, -1):
        eye = c + Vector((side * hh * 0.17, -hh * 0.385, hh * 0.06))
        parts.append(uv_sphere("eye", eye, (hh * 0.085, hh * 0.05, hh * 0.1), "eye_white", 16, 10))
        parts.append(uv_sphere("pupil", eye + Vector((0, -hh * 0.04, -hh * 0.005)), (hh * 0.042, hh * 0.022, hh * 0.05), "pupil", 12, 8))
        parts.append(uv_sphere("ear", c + Vector((side * hh * 0.42, hh * 0.02, 0)), (hh * 0.06, hh * 0.1, hh * 0.13), "skin", 12, 8))
    parts.append(uv_sphere("nose", c + Vector((0, -hh * 0.45, -hh * 0.06)), (hh * 0.06, hh * 0.07, hh * 0.08), "skin", 12, 8))
    parts.append(uv_sphere("mouth", c + Vector((0, -hh * 0.405, -hh * 0.22)), (hh * 0.12, hh * 0.03, hh * 0.025), "mouth", 12, 8))
    return parts


def hair_cap(name, d, scale, cut_front_z, cut_back_z):
    """A shell over the skull; faces below the cut lines (front: brow, back: nape) removed."""
    hh = d.head_h
    c = Vector((0, 0, d.head_c))
    obj = uv_sphere(name, c, (hh * 0.43 * scale, hh * 0.46 * scale, hh * 0.5 * scale), "hair", 32, 20)
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    kill = []
    for f in bm.faces:
        fc = f.calc_center_median() - c
        front = -fc.y / (hh * 0.46)
        limit = cut_front_z if front > 0.2 else cut_back_z
        if fc.z < limit * hh:
            kill.append(f)
    bmesh.ops.delete(bm, geom=kill, context="FACES")
    # thicken into a solid shell so it reads from inside the head too
    bm.to_mesh(obj.data)
    bm.free()
    m = obj.modifiers.new("solid", "SOLIDIFY")
    m.thickness = hh * 0.03
    select_only(obj)
    bpy.ops.object.modifier_apply(modifier="solid")
    return obj


def make_hair(d):
    hh = d.head_h
    c = Vector((0, 0, d.head_c))
    styles = []
    # 0: short crop
    styles.append([hair_cap("hair0", d, 1.07, 0.18, -0.12)])
    # 1: crop + top bun
    styles.append([hair_cap("hair1", d, 1.07, 0.2, -0.05), uv_sphere("bun", c + Vector((0, hh * 0.12, hh * 0.55)), (hh * 0.2,) * 3, "hair", 16, 10)])
    # 2: long hair falling to the shoulders
    long_back = uv_sphere("long", c + Vector((0, hh * 0.16, -hh * 0.32)), (hh * 0.46, hh * 0.28, hh * 0.5), "hair", 20, 12)
    styles.append([hair_cap("hair2", d, 1.08, 0.12, -0.6), long_back])
    # 3: mostly bald with a heroic fringe at the back
    styles.append([hair_cap("hair3", d, 1.05, 0.9, -0.2)])
    for s in styles:
        bm = bmesh.new()
        # trim fringe style: keep only a horseshoe below the crown
        if s[0].name == "hair3":
            bm.from_mesh(s[0].data)
            kill = [f for f in bm.faces if (f.calc_center_median() - c).z > hh * 0.22 or (f.calc_center_median() - c).y < -hh * 0.05]
            bmesh.ops.delete(bm, geom=kill, context="FACES")
            bm.to_mesh(s[0].data)
        bm.free()
    out = []
    for s in styles:
        select_only(*s)
        bpy.context.view_layer.objects.active = s[0]
        bpy.ops.object.join()
        out.append(s[0])
    return out


def rigid_to_head(obj, rig):
    g = obj.vertex_groups.new(name="head")
    g.add([v.index for v in obj.data.vertices], 1.0, "REPLACE")
    obj.parent = rig
    mod = obj.modifiers.new("rig", "ARMATURE")
    mod.object = rig


# ------------------------------------------------------------------------------------------------
# animation


class Poser:
    """Sets pose-bone rotations from rotations expressed in armature rest space."""

    def __init__(self, rig, d):
        self.rig, self.d = rig, d
        self.rest = {b.name: b.matrix_local.to_3x3() for b in rig.data.bones}

    def clear(self):
        for pb in self.rig.pose.bones:
            pb.rotation_quaternion = Quaternion()
            pb.location = Vector()

    def rot(self, bone, q):
        b = self.rest[bone]
        local = b.inverted() @ q.to_matrix() @ b
        pb = self.rig.pose.bones[bone]
        pb.rotation_quaternion = local.to_quaternion()

    def move(self, bone, delta):
        """Translate a bone by an armature-space delta."""
        self.rig.pose.bones[bone].location = self.rest[bone].inverted() @ Vector(delta)

    def key(self, frame):
        for pb in self.rig.pose.bones:
            pb.keyframe_insert("rotation_quaternion", frame=frame)
            pb.keyframe_insert("location", frame=frame)


def qx(deg):
    """Rotation about armature X. Negative swings a hanging limb forward (towards -Y)."""
    return Quaternion((1, 0, 0), math.radians(deg))


def qy(deg):
    return Quaternion((0, 1, 0), math.radians(deg))


def qz(deg):
    return Quaternion((0, 0, 1), math.radians(deg))


def base_pose(p, arm_down=26.0, arm_fwd=(0.0, 0.0), elbow=(8.0, 8.0)):
    """Shared starting point: arms lowered from the A-pose, elder hunch."""
    p.clear()
    for i, (sfx, side) in enumerate((("L", 1), ("R", -1))):
        p.rot(f"upper_arm.{sfx}", qx(-arm_fwd[i]) @ qy(side * arm_down))
        p.rot(f"forearm.{sfx}", qx(-elbow[i]))
    if p.d.hunch:
        p.rot("spine", qx(-8))
        p.rot("chest", qx(-10))
        p.rot("neck", qx(12))


def legs(p, thigh, shin, foot=(0.0, 0.0), spread=0.0):
    """thigh: forward swing (deg), shin: knee bend (deg, positive bends back)."""
    for i, (sfx, side) in enumerate((("L", 1), ("R", -1))):
        p.rot(f"thigh.{sfx}", qx(-thigh[i]) @ qy(-side * spread))
        p.rot(f"shin.{sfx}", qx(shin[i]))
        p.rot(f"foot.{sfx}", qx(foot[i]))


def new_action(rig, name):
    act = bpy.data.actions.new(name)
    rig.animation_data_create()
    rig.animation_data.action = act
    return act


def finish_action(rig, act, frames):
    act.use_frame_range = True
    act.frame_range = (0, frames)
    track = rig.animation_data.nla_tracks.new()
    track.name = act.name
    strip = track.strips.new(act.name, 0, act)
    strip.mute = False
    rig.animation_data.action = None


def sitting_infant(p, bob=0.0):
    d = p.d
    base_pose(p, arm_down=18, arm_fwd=(30, 30), elbow=(30, 30))
    legs(p, (80, 80), (20, 20), spread=22)
    p.move("hips", (0, 0, -(d.legs - d.thigh_r * 1.1) + bob))


def make_clips(rig, d, infant):
    p = Poser(rig, d)
    H = d.H

    # idle: breathing, a little weight shift
    act = new_action(rig, "idle")
    for f, k in ((0, 0.0), (30, 1.0), (60, 0.0)):
        if infant:
            sitting_infant(p)
            p.rot("head", qz(10 * k - 5))
        else:
            base_pose(p, arm_down=26 - 2 * k)
            p.rot("chest", qx(-2 * k) @ (qx(-10) if d.hunch else Quaternion()))
            p.rot("head", qz(4 * k - 2))
            p.move("hips", (0, 0, -H * 0.003 * k))
        p.key(f)
    finish_action(rig, act, 60)

    # walk: one full stride cycle
    act = new_action(rig, "walk")
    if infant:
        for f, k in ((0, 0.0), (8, 1.0), (15, 0.0), (23, 1.0), (30, 0.0)):
            sitting_infant(p, bob=H * 0.04 * k)
            p.rot("hips", qz(6 * (1 if f in (8,) else -1) * k))
            p.key(f)
    else:
        swing, knee, arm = (24, 45, 22) if not d.hunch else (16, 35, 12)
        keys = [
            (0, (swing, -swing), (4, 12), (-arm, arm), 0.0),
            (8, (0, -8), (8, knee), (0, 0), 0.025),
            (15, (-swing, swing), (12, 4), (arm, -arm), 0.0),
            (23, (-8, 0), (knee, 8), (0, 0), 0.025),
            (30, (swing, -swing), (4, 12), (-arm, arm), 0.0),
        ]
        for f, th, sh, ar, up in keys:
            base_pose(p, arm_down=24, arm_fwd=ar, elbow=(14, 14))
            legs(p, th, sh, foot=(-sh[0] * 0.25, -sh[1] * 0.25))
            p.move("hips", (0, 0, H * up * 0.5 - H * 0.008))
            p.rot("hips", qz(-th[0] * 0.15))
            p.rot("chest", qz(th[0] * 0.25) @ (qx(-10) if d.hunch else Quaternion()))
            p.key(f)
    finish_action(rig, act, 30)

    # sit: hips at the seat (the game puts the peon's origin on the seat surface)
    act = new_action(rig, "sit")
    for f, k in ((0, 0.0), (45, 1.0), (90, 0.0)):
        if infant:
            sitting_infant(p)
        else:
            base_pose(p, arm_down=27, arm_fwd=(22 + 3 * k, 22), elbow=(48, 48))
            legs(p, (88, 88), (88, 88), foot=(-5, -5), spread=4)
            p.move("hips", (0, d.thigh_r * 0.2, -(d.legs - d.thigh_r * 1.05)))
            p.rot("chest", qx(-2 * k) @ (qx(-10) if d.hunch else Quaternion()))
        p.key(f)
    finish_action(rig, act, 90)

    # lie: on the back, body centred on the origin, head towards +Y (game -Z), face up
    act = new_action(rig, "lie")
    for f, k in ((0, 0.0), (60, 1.0), (120, 0.0)):
        base_pose(p, arm_down=20, elbow=(10, 10))
        legs(p, (3, 3), (4, 4), foot=(-20, -20))
        p.rot("hips", qx(-90))
        span = d.torso + d.head_h - d.legs
        p.move("hips", (0, -span * 0.5, -d.legs + d.depth * 0.5 + 0.015))
        p.rot("chest", qx(1.5 * k))
        p.key(f)
    finish_action(rig, act, 120)

    # use: hands busy in front (computers, stoves, sinks)
    act = new_action(rig, "use")
    for f, k in ((0, 0.0), (15, 1.0), (30, 0.0), (45, -1.0), (60, 0.0)):
        if infant:
            sitting_infant(p)
            p.rot("upper_arm.L", qx(-50 - 10 * k) @ qy(18))
            p.rot("upper_arm.R", qx(-50 + 10 * k) @ qy(-18))
        else:
            base_pose(p, arm_down=25, arm_fwd=(40 + 6 * k, 40 - 6 * k), elbow=(62, 62))
            p.rot("head", qx(12))
            p.rot("chest", qx(-4) @ (qx(-10) if d.hunch else Quaternion()))
        p.key(f)
    finish_action(rig, act, 60)

    # dance: hip sway, arms up
    act = new_action(rig, "dance")
    for f, k in ((0, 0.0), (8, 1.0), (15, 0.0), (23, -1.0), (30, 0.0)):
        if infant:
            sitting_infant(p, bob=H * 0.03 * abs(k))
            p.rot("upper_arm.L", qx(-120 - 20 * k) @ qy(30))
            p.rot("upper_arm.R", qx(-120 + 20 * k) @ qy(-30))
        else:
            base_pose(p, arm_down=-20, arm_fwd=(120 + 25 * k, 120 - 25 * k), elbow=(30, 30))
            legs(p, (12 * abs(k), 12 * abs(k)), (22 * abs(k), 22 * abs(k)))
            p.move("hips", (H * 0.02 * k, 0, -H * 0.02 * abs(k)))
            p.rot("hips", qz(14 * k))
            p.rot("chest", qy(-8 * k))
            p.rot("head", qy(10 * k))
        p.key(f)
    finish_action(rig, act, 30)

    # exercise: jumping jacks
    act = new_action(rig, "exercise")
    for f, k in ((0, 0.0), (8, 1.0), (15, 0.0)):
        if infant:
            sitting_infant(p, bob=H * 0.05 * k)
            p.rot("upper_arm.L", qy(-90 * k + 18))
            p.rot("upper_arm.R", qy(90 * k - 18))
        else:
            base_pose(p, arm_down=26 - 150 * k, elbow=(5, 5))
            legs(p, (0, 0), (6 * (1 - k), 6 * (1 - k)), spread=14 * k)
            p.move("hips", (0, 0, H * 0.06 * math.sin(k * math.pi / 2)))
        p.key(f)
    finish_action(rig, act, 15)

    if not infant:
        make_activity_clips(rig, p)
    p.clear()


# ------------------------------------------------------------------------------------------------
# interaction & demeanor clips (toddler and up)


def arm(p, sfx, fwd=0.0, down=26.0, inward=0.0, elbow=8.0, wrist=0.0, bend_in=0.0):
    """One arm, angles in degrees. From the A-pose: lower by `down` (26 = hanging; negative raises
    it out sideways), raise forward by `fwd`, swing towards the midline by `inward`; bend the elbow
    forward by `elbow` (or towards the body's midline by `bend_in`, for hands on hips), flex the
    wrist forward by `wrist`."""
    side = 1 if sfx == "L" else -1
    # the elbow and wrist hinge about the axis perpendicular to the A-posed bone (not world X, or
    # the forearm keeps the A-pose's outward angle and swings away from the face)
    spread = math.degrees(ARM_SPREAD)
    hinge = lambda deg: qy(-side * spread) @ qx(-deg) @ qy(side * spread)
    p.rot(f"upper_arm.{sfx}", qz(-side * inward) @ qx(-fwd) @ qy(side * down))
    p.rot(f"forearm.{sfx}", qy(side * bend_in) @ hinge(elbow))
    p.rot(f"hand.{sfx}", hinge(wrist))


def torso(p, lean=0.0, twist=0.0, tilt=0.0, head=0.0, head_turn=0.0, head_tilt=0.0):
    """Chest lean (positive = forward), twist about the vertical, side tilt; head nod (positive =
    down), turn and tilt. Keeps the elder hunch."""
    hunch = qx(-10) if p.d.hunch else Quaternion()
    p.rot("chest", qz(twist) @ qy(tilt) @ qx(lean) @ hunch)
    p.rot("head", qz(head_turn) @ qy(head_tilt) @ qx(head))


def clip(rig, p, name, frames, keys, pose):
    """Keys a looping clip: `pose(p, k)` for each (frame, k); the last key should match the first."""
    act = new_action(rig, name)
    for f, k in keys:
        base_pose(p)
        pose(p, k)
        p.key(f)
    finish_action(rig, act, frames)


def walk_clip(rig, p, name, swing, knee, arm_swing, lean=0.0, head=0.0, bounce=0.025, sway=0.15):
    d, H = p.d, p.d.H
    act = new_action(rig, name)
    keys = [
        (0, (swing, -swing), (4, 12), (-arm_swing, arm_swing), 0.0),
        (8, (0, -8), (8, knee), (0, 0), bounce),
        (15, (-swing, swing), (12, 4), (arm_swing, -arm_swing), 0.0),
        (23, (-8, 0), (knee, 8), (0, 0), bounce),
        (30, (swing, -swing), (4, 12), (-arm_swing, arm_swing), 0.0),
    ]
    for f, th, sh, ar, up in keys:
        base_pose(p, arm_down=24, arm_fwd=ar, elbow=(14, 14))
        legs(p, th, sh, foot=(-sh[0] * 0.25, -sh[1] * 0.25))
        p.move("hips", (0, 0, H * up * 0.5 - H * 0.008))
        p.rot("hips", qz(-th[0] * sway))
        torso(p, lean=lean, twist=th[0] * 0.25, head=head)
        p.key(f)
    finish_action(rig, act, 30)


def crouch(p, depth=1.0):
    """Squat with feet kept in place: thighs forward, knees bent, hips lowered and moved back."""
    d = p.d
    th, sh = 70 * depth, 110 * depth
    legs(p, (th, th), (sh, sh), foot=(th - sh, th - sh), spread=6)
    L = d.legs - d.ankle_z
    drop = L * (1 - 0.5 * (math.cos(math.radians(th)) + math.cos(math.radians(sh - th))))
    back = 0.5 * L * (math.sin(math.radians(th)) - math.sin(math.radians(sh - th)))
    p.move("hips", (0, back, -drop))


def make_activity_clips(rig, p):
    d, H = p.d, p.d.H
    wave = lambda k: math.sin(k * math.tau)  # k in 0..1 around a loop

    # eating from the hand (snack, jar): food at the chest, then to the mouth
    def eat(p, k):
        arm(p, "R", fwd=35 + 15 * k, inward=20 + 22 * k, elbow=95 + 50 * k)
        arm(p, "L", fwd=12, elbow=20)
        torso(p, head=6 - 10 * k)
    clip(rig, p, "eat", 60, [(0, 0), (15, 1), (25, 1), (40, 0), (60, 0)], eat)

    # chopping at a counter: forearms level, right hand chops
    def chop(p, k):
        arm(p, "R", fwd=18, inward=12, elbow=72 + 14 * k, wrist=-10 * k)
        arm(p, "L", fwd=18, inward=14, elbow=78)
        torso(p, lean=8, head=18)
    clip(rig, p, "chop", 12, [(0, 0), (6, 1), (12, 0)], chop)

    # stirring a pot: right hand circles, left rests
    def stir(p, k):
        arm(p, "R", fwd=22 + 8 * math.cos(k * math.tau), inward=8 + 8 * wave(k), elbow=80)
        arm(p, "L", fwd=10, elbow=35)
        torso(p, lean=6, head=16)
    clip(rig, p, "stir", 40, [(f, f / 40) for f in range(0, 41, 10)], stir)

    # scrubbing (hands, dishes, toilets): both hands small alternating circles, low
    def scrub(p, k):
        c, s_ = math.cos(k * math.tau), wave(k)
        arm(p, "R", fwd=16 + 6 * c, inward=30 + 6 * s_, elbow=68)
        arm(p, "L", fwd=16 - 6 * c, inward=30 - 6 * s_, elbow=68)
        torso(p, lean=12, head=18)
    clip(rig, p, "scrub", 24, [(f, f / 24) for f in range(0, 25, 6)], scrub)

    # typing at a desk: hands low and forward, fingers (forearms) tapping
    def type_(p, k):
        arm(p, "R", fwd=12, inward=26, elbow=62 + 4 * k, wrist=-8)
        arm(p, "L", fwd=12, inward=26, elbow=66 - 4 * k, wrist=-8)
        torso(p, lean=10, head=6)
    clip(rig, p, "type", 8, [(0, 0), (4, 1), (8, 0)], type_)

    # writing (homework, doodling): right hand scribbles, left holds the page
    def write(p, k):
        arm(p, "R", fwd=10 + 3 * wave(k), inward=26 + 5 * math.cos(k * math.tau), elbow=70, wrist=-15)
        arm(p, "L", fwd=10, inward=32, elbow=66, wrist=-10)
        torso(p, lean=16, head=18)
    clip(rig, p, "write", 20, [(f, f / 20) for f in range(0, 21, 5)], write)

    # phone at the chest, head down, thumb (wrist) twitching
    def phone(p, k):
        arm(p, "R", fwd=26, inward=18, elbow=84, wrist=-6 * k)
        arm(p, "L", fwd=5, elbow=12)
        torso(p, head=26, head_tilt=4 * k)
    clip(rig, p, "phone", 40, [(0, 0), (6, 1), (12, 0), (30, 0), (34, 1), (40, 0)], phone)

    # phone call: hand at the ear, the other gesturing
    def phone_call(p, k):
        arm(p, "R", fwd=22, down=12, inward=8, elbow=150)
        arm(p, "L", fwd=20 + 18 * k, inward=10, elbow=60 + 20 * k)
        torso(p, head=4, head_tilt=-8, head_turn=6 * k)
    clip(rig, p, "phone_call", 90, [(0, 0), (20, 1), (45, 0), (65, 1), (90, 0)], phone_call)

    # selfie: arm out high, head tilted, pout and re-pose
    def selfie(p, k):
        arm(p, "R", fwd=115, inward=12, elbow=18, wrist=10)
        arm(p, "L", down=-8, fwd=-12, elbow=10, bend_in=80)  # hand on hip
        torso(p, lean=-4, tilt=4 * k, head=-6, head_tilt=10 * k - 5, head_turn=-10)
    clip(rig, p, "selfie", 60, [(0, 1), (20, 1), (28, -1), (52, -1), (60, 1)], selfie)

    # reading a book held in both hands, turning a page now and then
    def read(p, k):
        arm(p, "R", fwd=26, inward=16, elbow=78)
        arm(p, "L", fwd=26, inward=16 + 14 * k, elbow=78 - 8 * k)
        torso(p, head=26)
    clip(rig, p, "read", 90, [(0, 0), (60, 0), (68, 1), (76, 0), (90, 0)], read)

    # oration (read aloud, monologues, speeches): one arm flung to the heavens, then to the heart
    def orate(p, k):
        arm(p, "R", fwd=50, inward=20, elbow=60)
        if k > 0:
            arm(p, "L", down=-30 * k, fwd=150 * k, elbow=15)
        else:
            arm(p, "L", fwd=30, inward=-60 * k, elbow=125)
        torso(p, lean=-6 * k, head=-12 * k, head_turn=-10 * k)
    clip(rig, p, "orate", 120, [(0, 0), (20, 1), (50, 1), (70, -1), (100, -1), (120, 0)], orate)

    # talking: easy alternating hand gestures and nods
    def talk(p, k):
        arm(p, "R", fwd=22 + 16 * k, inward=12, elbow=70 + 15 * k, wrist=-10 * k)
        arm(p, "L", fwd=22 + 16 * (1 - k), inward=12, elbow=70 + 15 * (1 - k))
        torso(p, head=4 * k - 2, head_tilt=4 * (k - 0.5), twist=3 * (k - 0.5))
    clip(rig, p, "talk", 60, [(0, 0), (15, 1), (30, 0.3), (45, 1), (60, 0)], talk)

    # laughing: hands on the belly, chest bobbing, head thrown back
    def laugh(p, k):
        arm(p, "R", fwd=14, inward=36, elbow=80)
        arm(p, "L", fwd=14, inward=36, elbow=80)
        torso(p, lean=6 - 12 * k, head=-10 - 8 * k)
        p.move("hips", (0, 0, -H * 0.01 * k))
    clip(rig, p, "laugh", 24, [(0, 0), (6, 1), (12, 0.2), (18, 1), (24, 0)], laugh)

    # arguing: leaning in, fists shaking
    def argue(p, k):
        arm(p, "R", fwd=55 + 15 * k, inward=12, elbow=95 - 15 * k)
        arm(p, "L", fwd=55 - 15 * k, inward=12, elbow=95 + 15 * k)
        torso(p, lean=12, head=6, head_turn=6 * (2 * k - 1))
    clip(rig, p, "argue", 16, [(0, 0), (8, 1), (16, 0)], argue)

    # hug (or an enthusiastic air hug): arms out and wrapping, gentle sway
    def hug(p, k):
        arm(p, "R", fwd=78, inward=6, elbow=30)
        arm(p, "L", fwd=80, inward=6, elbow=26)
        torso(p, lean=8, tilt=4 * k, head=4, head_tilt=-8)
    clip(rig, p, "hug", 60, [(0, -1), (30, 1), (60, -1)], hug)

    # mourning: bowed, a hand to the eyes, shoulders shaking
    def mourn(p, k):
        arm(p, "R", fwd=48, inward=34, elbow=140)
        arm(p, "L", fwd=6, elbow=10)
        torso(p, lean=14 + 3 * k, head=30)
    clip(rig, p, "mourn", 40, [(0, 0), (6, 1), (12, 0), (18, 1), (40, 0)], mourn)

    # shower: both hands scrubbing the hair
    def shower(p, k):
        arm(p, "R", down=-10, fwd=40 + 10 * k, inward=28, elbow=150)
        arm(p, "L", down=-10, fwd=40 + 10 * (1 - k), inward=28, elbow=150)
        torso(p, head=-14, head_tilt=6 * (k - 0.5))
    clip(rig, p, "shower", 20, [(0, 0), (10, 1), (20, 0)], shower)

    # watering a plant: can held out, tipped
    def water(p, k):
        arm(p, "R", fwd=42, inward=10, elbow=30, wrist=20 + 15 * k)
        arm(p, "L", fwd=8, elbow=15)
        torso(p, lean=8, head=22)
    clip(rig, p, "water", 60, [(0, 0), (30, 1), (60, 0)], water)

    # kicking something over: wind up, kick, recover
    def kick(p, k):
        arm(p, "R", down=6 * k, fwd=-10 * k, elbow=20)
        arm(p, "L", down=6 * k, fwd=30 * k, elbow=20)
        th = 70 * k if k > 0 else -25 * -k
        legs(p, (0, th), (0, 30 if k < 0 else 5))
        torso(p, lean=-14 * max(k, 0) + 6 * max(-k, 0), head=10)
    clip(rig, p, "kick", 40, [(0, 0), (8, -1), (14, 1), (20, 0.4), (30, 0), (40, 0)], kick)

    # fixing (repair, unclog): crouched, right hand working
    def fix(p, k):
        crouch(p)
        arm(p, "R", fwd=55 + 18 * k, inward=18, elbow=55 - 15 * k)
        arm(p, "L", fwd=35, inward=10, elbow=40)
        torso(p, lean=30, head=10)
    clip(rig, p, "fix", 16, [(0, 0), (8, 1), (16, 0)], fix)

    # cradling (rocking a baby, bottle feeding): arms cupped at the waist, swaying
    def cradle(p, k):
        arm(p, "R", fwd=30, inward=26, elbow=88)
        arm(p, "L", fwd=24, inward=30, elbow=80)
        p.rot("hips", qz(6 * k))
        torso(p, twist=-4 * k, head=24, head_tilt=4 * k)
    clip(rig, p, "cradle", 90, [(0, -1), (45, 1), (90, -1)], cradle)

    # peekaboo: hands over the face... then BOO
    def peekaboo(p, k):
        arm(p, "R", down=26 - 40 * (1 - k), fwd=50 + 20 * (1 - k), inward=40 * k, elbow=150 - 70 * (1 - k))
        arm(p, "L", down=26 - 40 * (1 - k), fwd=50 + 20 * (1 - k), inward=40 * k, elbow=150 - 70 * (1 - k))
        torso(p, lean=14, head=6 - 14 * (1 - k))
    clip(rig, p, "peekaboo", 70, [(0, 1), (35, 1), (40, 0), (58, 0), (64, 1), (70, 1)], peekaboo)

    # admiring / pondering: hand on the chin, other arm across, head tilting
    def admire(p, k):
        arm(p, "R", fwd=40, inward=30, elbow=140)
        arm(p, "L", fwd=28, inward=58, elbow=92)
        torso(p, lean=-2, head=-4, head_tilt=8 * k)
    clip(rig, p, "admire", 120, [(0, -1), (60, 1), (120, -1)], admire)

    # demeanor variants -----------------------------------------------------------------------
    def idle_sad(p, k):
        arm(p, "R", down=30, fwd=6, elbow=4)
        arm(p, "L", down=30, fwd=6, elbow=4)
        torso(p, lean=14 + 2 * k, head=22)
        p.move("hips", (0, 0, -H * 0.004 * k))
    clip(rig, p, "idle_sad", 90, [(0, 0), (45, 1), (90, 0)], idle_sad)

    def idle_proud(p, k):
        for sfx in ("R", "L"):
            arm(p, sfx, down=-10, fwd=-14, elbow=10, bend_in=85)  # hands on hips
        torso(p, lean=-6, head=-10, head_turn=12 * k)
    clip(rig, p, "idle_proud", 120, [(0, -1), (50, -1), (60, 1), (110, 1), (120, -1)], idle_proud)

    def idle_nervous(p, k):
        arm(p, "R", down=30, fwd=14, inward=14, elbow=40)
        arm(p, "L", down=30, fwd=14, inward=14, elbow=40)
        torso(p, lean=6, twist=10 * k, head=2, head_turn=35 * k)
        p.move("hips", (0, 0, -H * 0.01))
    clip(rig, p, "idle_nervous", 120, [(0, 0), (12, -1), (35, -1), (45, 0), (62, 1), (85, 1), (95, 0), (120, 0)], idle_nervous)

    walk_clip(rig, p, "walk_sad", 16, 32, 8, lean=12, head=18, bounce=0.012, sway=0.1)
    walk_clip(rig, p, "walk_proud", 28, 50, 30, lean=-6, head=-8, bounce=0.035, sway=0.3)


# ------------------------------------------------------------------------------------------------


def build_stage(stage, params):
    reset()
    d = Dims(params)
    rig = make_armature(d)
    body = make_body_skin(d)
    assign_body_materials(body, rig, d)
    weight_body(body, rig)
    add_build_keys(body, d)
    # face parts join the body (they get the Basis position in every shape key)
    parts = make_head_parts(d)
    for prt in parts:
        g = prt.vertex_groups.new(name="head")
        g.add([v.index for v in prt.data.vertices], 1.0, "REPLACE")
    select_only(body, *parts)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()
    for h in make_hair(d):
        rigid_to_head(h, rig)
    make_clips(rig, d, infant=(stage == "infant"))

    os.makedirs(OUT, exist_ok=True)
    path = os.path.abspath(os.path.join(OUT, f"peon_{stage}.glb"))
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_frame_step=1,
        export_morph=True,
        export_morph_normal=False,
        export_skins=True,
        export_all_influences=False,
        export_def_bones=False,
        export_yup=True,
        export_apply=False,
        export_texcoords=False,
        export_normals=True,
        export_materials="EXPORT",
    )
    print(f"wrote {path}: body {len(body.data.vertices)} verts")


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    only = set(argv)
    for stage, params in STAGES.items():
        if only and stage not in only:
            continue
        build_stage(stage, params)


main()
