"""Generates the rigged peon models, one glTF per life stage, into assets/models/.

Run headless from the repo root (about three minutes for all six):

    blender -b --factory-startup -P assets/blender/make_peons.py [-- adult teen ...]

Set PEON_PREVIEW=out.png (with PEON_SHOW=mesh,names and PEON_KEYS=key=value,...) to render a
dressed body front and three-quarter in Blender instead of exporting.

Every stage file holds the same skeleton (bone names) and these skinned meshes:
- `body`: a metaball body (peon_body.py) with detailed hands and feet welded on
  (peon_extremities.py), its faces tagged `skin.rN` by the skin region N a garment can cover, with
  shape keys `slim`/`heavy` (build) and the body sliders (peon_body.BODY_SLIDERS), and the skin
  colour attributes `tone`/`thin` (COLOR_0/COLOR_1; the head has them too);
- `head`: skull, face, eyes, lids, lips, ears and brows (peon_head.py), rigid on the head bone,
  with a shape key per face slider (peon_head.FACE_SLIDERS);
- garments `top_*`, `bottom_*`, `shoes` (peon_body.GARMENTS): copies of the body's region faces
  pushed out, so they share its skinning and body keys;
- hair styles `hair.<id>` and facial hair `facial.<id>` (peon_hair.py), shells over the head with
  the face keys.
The slider and style ids match the catalogues in crates/sim/src/genetics.rs. Every slider's key is
made by building the geometry with that slider at 1 and subtracting, so the game blends them
linearly (weights -1..1). Keys that move nothing are dropped (glTF stores targets densely).

Plus the animation clips the game maps poses to: idle, walk, sit, lie, use, dance, exercise; walking
bodies (toddler and up) also get the interaction clips the data names (`anim:` and `seat: (anim:)`
in objects.ron, socials.ron) — standing, seated (origin on the seat, like `sit`), on the floor and
lying (like `lie`) — and demeanor variants (idle_sad, walk_sad, idle_proud, walk_proud,
idle_nervous). Infants get their own versions of the clips they can play (sleep, play_floor, laugh,
drink, drowsy, kick_back, and idle_sad as crying), built on their sitting base. The game falls back
to the pose clip for any clip a body lacks.

Materials are named after the palette slot the game tints them with (skin, hair, shirt, pants,
shoes, iris; `skin.lips` and `skin.rN` tint with skin); `eye_white` and `pupil` keep their own colour.
Blender faces -Y, so the exported model faces glTF +Z, which is the game's forward.
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from peon_body import BODY_SLIDERS, Hems, add_body_keys, cut_regions, make_body, make_clothes, transfer_keys, weight_body  # noqa: E402
from peon_extremities import add_extremities, add_stubs, copy_weights, paint_body, paint_head, shoe_last, smooth_seams  # noqa: E402
from peon_hair import FACIAL_HAIR, hair_meshes  # noqa: E402
from peon_head import EXPRESSIONS, FACE_SLIDERS, Face, build_head  # noqa: E402

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "models")
FPS = 30

# Proportions as fractions of the stage height H. Heights match sim::data::LifeStage::height().
STAGES = {
    "infant": dict(H=0.55, head=0.36, legs=0.27, width=0.36, depth=0.25, limb=0.10, foot=0.13),
    "toddler": dict(H=0.85, head=0.30, legs=0.32, width=0.29, depth=0.18, limb=0.08, foot=0.13),
    "child": dict(H=1.25, head=0.225, legs=0.46, width=0.235, depth=0.135, limb=0.064, foot=0.13),
    "teen": dict(H=1.60, head=0.185, legs=0.49, width=0.225, depth=0.125, limb=0.058, foot=0.135),
    "adult": dict(H=1.75, head=0.17, legs=0.5, width=0.23, depth=0.125, limb=0.058, foot=0.135),
    "elder": dict(H=1.68, head=0.17, legs=0.48, width=0.235, depth=0.13, limb=0.058, foot=0.135, hunch=True),
}

PALETTE = {
    # tinted per peon by the game (white base so the tint is the colour)
    "skin": (1, 1, 1, 1),
    "hair": (1, 1, 1, 1),
    "shirt": (1, 1, 1, 1),
    "pants": (1, 1, 1, 1),
    "shoes": (1, 1, 1, 1),
    "iris": (1, 1, 1, 1),
    # fixed colours (sRGB)
    "eye_white": (0.95, 0.95, 0.93, 1),
    "pupil": (0.06, 0.05, 0.07, 1),
    "eye_shade": (0.7, 0.66, 0.66, 1),  # the white under the upper lid's shadow
    "glint": (1, 1, 1, 1),  # emissive catchlight
    "lash": (0.08, 0.06, 0.06, 1),
    "skin.lips": (0.86, 0.6, 0.6, 1),  # multiplies the skin tint
    "mouth": (0.3, 0.09, 0.1, 1),
    "teeth": (0.94, 0.93, 0.88, 1),
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
        bsdf.inputs["Roughness"].default_value = 0.3 if name in ("eye_white", "eye_shade", "iris", "pupil") else 0.6 if name == "skin.lips" else 0.75
        if name == "glint":
            bsdf.inputs["Emission Color"].default_value = (1, 1, 1, 1)
            bsdf.inputs["Emission Strength"].default_value = 1.0


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
        self.hip_x = self.width * 0.2
        self.ankle_z = H * 0.045
        self.knee_z = self.ankle_z + (self.legs - self.ankle_z) * 0.5
        self.shoulder_z = self.legs + self.torso * 0.92
        self.shoulder_x = self.width * 0.5 - self.limb * 0.2
        self.arm_len = self.torso * 0.92
        self.neck_z = self.legs + self.torso
        self.head_c = H - self.head_h * 0.5
        self.thigh_r = self.limb * 0.72
        self.arm_r = self.limb * 0.45


def arm_points(d, side):
    """Shoulder, elbow, wrist, fingertip for side = +1 (left, +X) or -1."""
    dirv = Vector((side * math.sin(ARM_SPREAD), 0, -math.cos(ARM_SPREAD)))
    sh = Vector((side * d.shoulder_x, 0, d.shoulder_z))
    el = sh + dirv * d.arm_len * 0.5
    wr = sh + dirv * d.arm_len
    tip = wr + dirv * d.H * 0.095
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
# body, head, hair, clothes (geometry lives in peon_body.py, peon_head.py and peon_hair.py)


# how child-like the face is per stage (big cranium, small face, soft features)
CHILDISH = {"infant": 1.0, "toddler": 1.0, "child": 0.8, "teen": 0.3, "adult": 0.0, "elder": 0.0}


# the head's shape keys: the face sliders, then the expressions
FACE_KEYS = [k for k, _ in FACE_SLIDERS] + EXPRESSIONS


def head_frame(d):
    """Head height (chin to crown) and its centre at eye level, in model units."""
    hh = d.head_h * 0.8
    return hh, Vector((0, -d.H * 0.008, d.H - 0.49 * hh))


def orient_outward(me, ref):
    """Makes every connected piece face away from `ref` (the head centre): the game culls back
    faces, so inward-wound parts would render hollow."""
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    seen = set()
    for f0 in bm.faces:
        if f0.index in seen:
            continue
        island, stack = [], [f0]
        seen.add(f0.index)
        while stack:
            f = stack.pop()
            island.append(f)
            for e in f.edges:
                for g in e.link_faces:
                    if g.index not in seen:
                        seen.add(g.index)
                        stack.append(g)
        out = sum(f.calc_area() * f.normal.dot(f.calc_center_median() - ref) for f in island)
        if out < 0:
            bmesh.ops.reverse_faces(bm, faces=island)
    bm.to_mesh(me)
    bm.free()


def mesh_object(name, verts, faces, mats, ref=None):
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(v) for v in verts], [], faces)
    if ref is not None:
        orient_outward(me, Vector(ref))
    order = []
    for m in mats:
        if m not in order:
            order.append(m)
    for m in order:
        me.materials.append(bpy.data.materials[m])
    for poly, m in zip(me.polygons, mats):
        poly.material_index = order.index(m)
        poly.use_smooth = True
    return link(bpy.data.objects.new(name, me))


def add_param_keys(obj, variants, eps=1e-5):
    """variants: [(key name, [model-space verts])] with obj's topology. Keys that move no vertex
    more than eps are left out (glTF stores every target densely)."""
    if obj.data.shape_keys is None:
        obj.shape_key_add(name="Basis")
    for name, verts in variants:
        if all((Vector(v) - b.co).length < eps for v, b in zip(verts, obj.data.vertices)):
            continue
        sk = obj.shape_key_add(name=name, from_mix=False)
        for i, v in enumerate(verts):
            sk.data[i].co = v


def make_head(d, stage):
    """The head (skull, face, eyes, lids, lips, ears, brows) as one rigid mesh with a shape key
    per face slider."""
    hh, c = head_frame(d)
    ch = CHILDISH[stage]

    ears = set()

    def build(P):
        verts, faces, mats = [], [], []
        for name, v, f, m in build_head(hh, c, P, ch):
            base = len(verts)
            if name == "ear":
                ears.update(range(base, base + len(v)))
            verts += v
            faces += [tuple(i + base for i in fc) for fc in f]
            mats += m
        return verts, faces, mats

    verts, faces, mats = build({})
    obj = mesh_object("head", verts, faces, mats, ref=c)
    add_param_keys(obj, [(k, build({k: 1.0})[0]) for k in FACE_KEYS])
    g = obj.vertex_groups.new(name="head")
    g.add(list(range(len(verts))), 1.0, "REPLACE")
    paint_head(obj, hh, c, ears)
    return obj


def make_hair(d, stage, rig):
    """Every hair and facial-hair style as its own mesh (`hair.<id>`, `facial.<id>`), rigid on
    the head bone, with the face slider keys."""
    hh, c = head_frame(d)
    ch = CHILDISH[stage]
    to_model = lambda vs: [c + v * hh for v in vs]
    ref = Face({}, ch)
    base = hair_meshes(ref)
    variants = {k: hair_meshes(Face({k: 1.0}, ch), ref) for k in FACE_KEYS}
    facial = {k for k, _ in FACIAL_HAIR}
    out = []
    for style, (verts, faces) in base.items():
        name = ("facial." if style in facial else "hair.") + style
        obj = mesh_object(name, to_model(verts), faces, ["hair"] * len(faces), ref=c)
        add_param_keys(obj, [(k, to_model(variants[k][style][0])) for k in FACE_KEYS])
        rigid_to_head(obj, rig)
        out.append(obj)
    return out


def rigid_to_head(obj, rig, group=True):
    if group:
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
        # a continuous gait sampled every other frame: knee flex at heel strike, a high knee on
        # the swing, the body lowest at contact and highest passing, arms lagging the legs with
        # the elbow following through (the game adds the hip/shoulder twist on top)
        swing, knee, arm = (24, 52, 20) if not d.hunch else (16, 38, 11)
        for f in range(0, 31, 2):
            a = 2 * math.pi * f / 30

            def leg(ph):
                th = swing * math.cos(ph)
                sw = (ph % (2 * math.pi)) > math.pi  # swinging forward
                kn = 6 + (knee * math.sin(ph - math.pi) ** 1.4 if sw else 12 * max(0.0, math.sin(2 * ph)))
                return th, kn

            (tl, kl), (tr, kr) = leg(a), leg(a + math.pi)
            lag = 0.35
            al, ar_ = -arm * math.cos(a - lag), arm * math.cos(a - lag)
            el = (12 + 18 * max(0.0, -math.cos(a - lag - 0.3)), 12 + 18 * max(0.0, math.cos(a - lag - 0.3)))
            base_pose(p, arm_down=23, arm_fwd=(al, ar_), elbow=el)
            legs(p, (tl, tr), (kl, kr), foot=(-kl * 0.3 + 6 * math.cos(a), -kr * 0.3 - 6 * math.cos(a)))
            p.move("hips", (0, 0, -H * 0.011 * math.cos(2 * a) - H * 0.01))
            p.rot("chest", qx(-3) @ (qx(-10) if d.hunch else Quaternion()))
            p.rot("head", qx(2 + 1.5 * math.cos(2 * a)))
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

    if infant:
        make_infant_clips(rig, p)
    else:
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


def seated(p):
    """On a chair: the `sit` clip's legs and hips (the game puts the origin on the seat surface)."""
    d = p.d
    legs(p, (88, 88), (88, 88), foot=(-5, -5), spread=4)
    p.move("hips", (0, d.thigh_r * 0.2, -(d.legs - d.thigh_r * 1.05)))


def floor_sit(p, spread=10.0, knee=10.0):
    """On the floor (origin on the floor): legs out in front, knees raised by `knee` degrees. The
    thigh rises above level by `knee` and the shin falls back by as much, so the heel stays at hip
    height (on the floor) for equal-length thigh and shin."""
    d = p.d
    legs(p, (90 + knee, 90 + knee), (2 * knee, 2 * knee), foot=(-30 + knee, -30 + knee), spread=spread)
    p.move("hips", (0, d.thigh_r * 0.1, -(d.legs - d.thigh_r * 1.1)))


def lying(p, knee=(4.0, 4.0)):
    """On the back like the `lie` clip: body centred on the origin, head towards +Y, face up."""
    d = p.d
    legs(p, (3 + knee[0] * 0.5, 3 + knee[1] * 0.5), knee, foot=(-20, -20))
    p.rot("hips", qx(-90))
    span = d.torso + d.head_h - d.legs
    p.move("hips", (0, -span * 0.5, -d.legs + d.depth * 0.5 + 0.015))


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

    # seated, floor and lying interactions ------------------------------------------------------

    # watching TV with the remote in the lap: settled back, a channel flick (k = 1) now and then,
    # a chuckle (k = 2)
    def watch_upper(p, k):
        click, chuckle = max(0.0, 1 - abs(k - 1)), max(0.0, 1 - abs(k - 2))
        # (no hip offset here: `seated`/`floor_sit` own the hips' location)
        arm(p, "R", fwd=12 + 10 * click, inward=18, elbow=64 + 16 * click, wrist=-14 * click)
        arm(p, "L", fwd=8, inward=22, elbow=58)
        torso(p, lean=-6 + 12 * chuckle, head=2 - 10 * chuckle, head_tilt=3)

    watch_keys = [(0, 0), (70, 0), (76, 1), (82, 0), (150, 0), (156, 2), (162, 0.3), (168, 2), (178, 0), (240, 0)]

    def watch(p, k):
        seated(p)
        watch_upper(p, k)
    clip(rig, p, "watch", 240, watch_keys, watch)

    def watch_floor(p, k):
        floor_sit(p, spread=12, knee=16)
        watch_upper(p, k)
    clip(rig, p, "watch_floor", 240, watch_keys, watch_floor)

    # chess: chin on the left hand, pondering; the right hand reaches out to move a piece (k = 1)
    def chess_upper(p, k, standing):
        arm(p, "L", fwd=46, inward=34, elbow=138)
        arm(p, "R", fwd=34 + 26 * k, inward=16 - 6 * k, elbow=70 - 40 * k, wrist=12 * k)
        torso(p, lean=(24 if standing else 14) + 8 * k, head=(26 if standing else 18) + 6 * k, head_tilt=6 * (1 - k))

    chess_keys = [(0, 0), (80, 0), (95, 1), (105, 1), (120, 0), (200, 0)]

    def sit_chess(p, k):
        seated(p)
        chess_upper(p, k, False)
    clip(rig, p, "sit_chess", 200, chess_keys, sit_chess)
    clip(rig, p, "chess", 200, chess_keys, lambda p, k: chess_upper(p, k, True))

    # on the toilet, scrolling the phone
    def sit_phone(p, k):
        seated(p)
        arm(p, "R", fwd=34, inward=18, elbow=92, wrist=-6 * k)
        arm(p, "L", fwd=14, inward=20, elbow=40)
        torso(p, lean=16, head=30, head_tilt=4 * k)
    clip(rig, p, "sit_phone", 40, [(0, 0), (6, 1), (12, 0), (30, 0), (34, 1), (40, 0)], sit_phone)

    # paranoid armchair vigil: gripping the arms, head snapping round to the door and back
    def sit_lookout(p, k):
        seated(p)
        arm(p, "R", down=30, fwd=24, inward=-6, elbow=40)
        arm(p, "L", down=30, fwd=24, inward=-6, elbow=40)
        torso(p, lean=8, twist=8 * k, head=-2, head_turn=40 * k)
    clip(rig, p, "sit_lookout", 150, [(0, 0), (40, 0), (46, 1), (80, 1), (88, 0), (110, -0.6), (130, -0.6), (150, 0)], sit_lookout)

    # playing on the floor: legs splayed, banging a block about in front
    def play_floor(p, k):
        floor_sit(p, spread=28, knee=6)
        arm(p, "R", fwd=46 + 22 * k, inward=12, elbow=50 - 20 * k, wrist=10 * k)
        arm(p, "L", fwd=40 - 8 * k, inward=18, elbow=56)
        torso(p, lean=18 + 6 * k, head=26, head_tilt=6 * (k - 0.5))
    clip(rig, p, "play_floor", 30, [(0, 0), (8, 1), (15, 0), (23, 1), (30, 0)], play_floor)

    # asleep: head rolled to one side, a hand on the chest, a knee drifting up, slow deep breaths
    def sleep(p, k):
        lying(p, knee=(30, 4))
        arm(p, "R", down=22, fwd=6, elbow=20, bend_in=80)  # hand over the belly
        arm(p, "L", down=12, fwd=2, elbow=12)
        torso(p, lean=-2 * k, head=-4, head_turn=-28)
    clip(rig, p, "sleep", 180, [(0, 0), (90, 1), (180, 0)], sleep)

    # having a diaper changed: on the back, legs pedalling, arms waving
    def kick_back(p, k):
        lying(p)
        legs(p, (40 + 30 * k, 70 - 30 * k), (70 - 40 * k, 30 + 40 * k), foot=(-10, -10), spread=10)
        arm(p, "R", down=0, fwd=40 + 20 * k, elbow=40)
        arm(p, "L", down=0, fwd=60 - 20 * k, elbow=40)
        torso(p, head=-6, head_turn=10 * k)
    clip(rig, p, "kick_back", 30, [(0, -1), (15, 1), (30, -1)], kick_back)

    # bath: reclined against the tub end, knees up, waving the duck about and humming
    def bathe(p, k):
        lying(p, knee=(70, 64))
        arm(p, "R", down=16, fwd=14 + 10 * k, inward=12, elbow=70 + 10 * k, wrist=-10 * k)
        arm(p, "L", down=22, fwd=4, elbow=20, bend_in=60)
        torso(p, lean=34, head=10, head_tilt=6 * k)
    clip(rig, p, "bathe", 60, [(0, 0), (30, 1), (60, 0)], bathe)

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
# infant clips (on the sitting base; the game plays them for crib sleep, toys and baby care)


def make_infant_clips(rig, p):
    d, H = p.d, p.d.H

    def sit(p, bob=0.0):
        """`sitting_infant`'s legs and hips (the origin on the floor), arms left free."""
        legs(p, (80, 80), (20, 20), spread=22)
        p.move("hips", (0, 0, -(d.legs - d.thigh_r * 1.1) + bob))

    # laughing (cooed at, peekaboo): bouncing on the bottom, arms flapping, head thrown back
    def laugh(p, k):
        sit(p, bob=H * 0.025 * k)
        arm(p, "R", down=10 - 30 * k, fwd=40 + 20 * k, elbow=30)
        arm(p, "L", down=10 - 30 * k, fwd=40 + 20 * k, elbow=30)
        torso(p, lean=-4 * k, head=-14 * k)
    clip(rig, p, "laugh", 20, [(0, 0), (5, 1), (10, 0), (15, 1), (20, 0)], laugh)

    # bottle feeding: both hands up at the mouth, head tipped back, sucking away
    def drink(p, k):
        sit(p)
        arm(p, "R", fwd=60, inward=34, elbow=95)
        arm(p, "L", fwd=60, inward=34, elbow=95)
        torso(p, lean=-8, head=-16 + 4 * k)
    clip(rig, p, "drink", 24, [(0, 0), (12, 1), (24, 0)], drink)

    # being rocked to sleep: slumping, the head nodding down and jerking back up
    def drowsy(p, k):
        sit(p)
        arm(p, "R", fwd=20, inward=20, elbow=40 - 10 * k)
        arm(p, "L", fwd=20, inward=20, elbow=40 - 10 * k)
        torso(p, lean=10 + 10 * k, head=10 + 30 * k, head_tilt=8 * k)
    clip(rig, p, "drowsy", 120, [(0, 0), (80, 1), (90, 1), (96, 0.1), (120, 0)], drowsy)

    # toys: whacking a block on the floor, head bobbing along
    def play_floor(p, k):
        sit(p)
        arm(p, "R", down=10, fwd=55 + 40 * k, inward=10, elbow=30)
        arm(p, "L", down=10, fwd=50 - 20 * k, inward=10, elbow=30)
        torso(p, lean=12 + 6 * k, head=20 - 6 * k)
    clip(rig, p, "play_floor", 16, [(0, 0), (8, 1), (16, 0)], play_floor)

    # crib: on the back, arms up beside the head, knees flopped out, quick shallow breaths
    def sleep(p, k):
        lying(p)
        legs(p, (30, 30), (50, 50), foot=(-10, -10), spread=30)
        arm(p, "R", down=-70, elbow=10, bend_in=70)
        arm(p, "L", down=-64, elbow=10, bend_in=76)
        torso(p, lean=-3 * k, head_turn=25)
    clip(rig, p, "sleep", 60, [(0, 0), (30, 1), (60, 0)], sleep)

    # crying (bad mood): head back, arms flailing, bouncing with each wail
    def idle_sad(p, k):
        sit(p, bob=H * 0.012 * abs(k))
        arm(p, "R", down=-10, fwd=50 + 30 * k, elbow=40)
        arm(p, "L", down=-10, fwd=50 - 30 * k, elbow=40)
        torso(p, lean=-6, head=-22, head_tilt=8 * k)
    clip(rig, p, "idle_sad", 24, [(0, 0), (6, 1), (12, 0), (18, -1), (24, 0)], idle_sad)

    # diaper change: on the back, legs pedalling (like the walking bodies' clip)
    def kick_back(p, k):
        lying(p)
        legs(p, (40 + 30 * k, 70 - 30 * k), (70 - 40 * k, 30 + 40 * k), foot=(-10, -10), spread=16)
        arm(p, "R", down=0, fwd=40 + 20 * k, elbow=40)
        arm(p, "L", down=0, fwd=60 - 20 * k, elbow=40)
        torso(p, head=-6, head_turn=10 * k)
    clip(rig, p, "kick_back", 24, [(0, -1), (12, 1), (24, -1)], kick_back)


# ------------------------------------------------------------------------------------------------


def preview(rig, body, clothes, hair, path):
    """Renders a dressed body front and three-quarter (PEON_PREVIEW=out.png; PEON_SHOW=comma list of
    mesh names to show, PEON_KEYS=key=value,... shape key values) instead of exporting."""
    show = set(os.environ.get("PEON_SHOW", "top_tee,bottom_trousers,shoes,hair.short").split(","))
    keys = dict(kv.split("=") for kv in os.environ.get("PEON_KEYS", "").split(",") if kv)
    for o in clothes + hair:
        o.hide_render = o.name not in show and o.name != "head"
    for o in [body] + clothes + hair:
        if o.data.shape_keys:
            for kb in o.data.shape_keys.key_blocks[1:]:
                kb.slider_min = -1
                kb.value = float(keys.get(kb.name, 0))
    tints = {"skin": (0.87, 0.67, 0.5), "hair": (0.3, 0.18, 0.1), "shirt": (0.3, 0.5, 0.75), "pants": (0.25, 0.25, 0.3), "shoes": (0.9, 0.9, 0.9), "iris": (0.3, 0.45, 0.6)}
    for m in bpy.data.materials:
        base = m.name.split(".")[0]
        if base in tints and m.node_tree:
            col = [c**2.2 for c in tints[base]]
            if m.name == "skin.lips":
                col = [a * b for a, b in zip(col, (0.86**2.2, 0.6**2.2, 0.6**2.2))]
            m.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (*col, 1)
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_EEVEE"
    w = bpy.data.worlds.new("w")
    sc.world = w
    w.use_nodes = True
    w.node_tree.nodes["Background"].inputs[0].default_value = (0.5, 0.55, 0.6, 1)
    ld = bpy.data.lights.new("sun", "SUN")
    ld.energy = 3
    lo = link(bpy.data.objects.new("sun", ld))
    lo.rotation_euler = (math.radians(50), 0, math.radians(-30))
    H = rig.dimensions.z
    zoom_on = os.environ.get("PEON_ZOOM", "")
    zoom = zoom_on == "head"
    if zoom_on in ("hand", "foot"):
        # close-up of the left hand / foot: front, side, back and from below
        sh, el, wr, tip = arm_points(Dims(STAGES[os.environ.get("PEON_STAGE", "adult")]), 1)
        target = wr.lerp(tip, float(os.environ.get("PEON_ZOOM_AT", 0.5))) if zoom_on == "hand" else Vector((rig.data.bones["foot.L"].head_local.x, -0.05 * H, 0.04 * H))
        for i, (yaw, pitch) in enumerate(((0, 0), (90, 0), (180, 0), (-90, 0), (30, 45))):
            cd = bpy.data.cameras.new("c")
            cd.type = "ORTHO"
            cd.ortho_scale = H * float(os.environ.get("PEON_ZOOM_SCALE", 0.14 if zoom_on == "hand" else 0.17))
            co = link(bpy.data.objects.new("c", cd))
            a, b = math.radians(yaw), math.radians(pitch)
            dirv = Vector((math.sin(a) * math.cos(b), -math.cos(a) * math.cos(b), math.sin(b)))
            co.location = target + dirv * 3
            co.rotation_euler = dirv.to_track_quat("Z", "Y").to_euler()
            sc.camera = co
            sc.render.resolution_x = sc.render.resolution_y = 500
            sc.render.filepath = path.replace(".png", f"_{zoom_on}{i}.png")
            bpy.ops.render.render(write_still=True)
        return
    for yaw, label in ((0, "front"), (40, "side")):
        cd = bpy.data.cameras.new("c")
        cd.type = "ORTHO"
        cd.ortho_scale = H * (0.3 if zoom else 1.1)
        co = link(bpy.data.objects.new("c", cd))
        a = math.radians(yaw)
        co.location = (math.sin(a) * 5, -math.cos(a) * 5, H * (0.9 if zoom else 0.5))
        co.rotation_euler = (math.radians(90), 0, a)
        sc.camera = co
        sc.render.resolution_x, sc.render.resolution_y = 500, 700 if not zoom else 500
        sc.render.filepath = path.replace(".png", f"_{label}.png")
        bpy.ops.render.render(write_still=True)


def build_stage(stage, params):
    reset()
    d = Dims(params)
    rig = make_armature(d)
    hems = Hems(d, arm_points, leg_points)
    body = make_body(d, arm_points, leg_points, lambda m: add_stubs(m, d, arm_points, leg_points))
    cut_regions(body, rig, hems)
    weight_body(body, rig)
    add_body_keys(body, d)
    last = shoe_last(d, leg_points, hems)
    copy_weights(last, body, rig)
    transfer_keys(last, body)
    clothes = make_clothes(body, rig, d, hems, sources={"shoes": last})
    bpy.data.objects.remove(last)
    spots = add_extremities(body, rig, d, hems, arm_points, leg_points)
    paint_body(body, d, spots, arm_points, leg_points)
    smooth_seams([body], spots.pop("seam_src"), d, arm_points, leg_points)
    # the head stays its own mesh: glTF stores morph targets densely, so face keys on the body
    # (or body keys on the head) would cost a delta for every vertex
    head = make_head(d, stage)
    rigid_to_head(head, rig, group=False)
    hair = make_hair(d, stage, rig)
    if os.environ.get("PEON_PREVIEW"):
        preview(rig, body, clothes + [head], hair, os.environ["PEON_PREVIEW"])
        return
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
        export_vertex_color="ACTIVE",
        export_all_vertex_colors=True,
        export_active_vertex_color_when_no_material=True,
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
