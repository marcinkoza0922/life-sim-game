"""MPFB2 (MakeHuman) base body and head for the peon generator (make_peons.py with PEON_BASE=mpfb).

Needs the pinned MPFB extension and MakeHuman CC0 system asset pack: install both with
`python3 assets/blender/install_mpfb.py` (pins and hashes live there; `check_pins` refuses to build
with anything else, since the models would silently change). The base mesh, targets and assets are CC0.

`build(stage, H, spread)` makes a human for the life stage, poses MPFB's game_engine rig into the
peon A-pose (upper arms `spread` from vertical), scales it to height H and returns an `MpfbBase`:
- `body`: everything below the head, its faces tagged skin, with vertex groups named after the
  peon bones (MPFB's weights merged: fingers into hand, clavicles into chest, ...);
- `head`: the head (MPFB's own skin above the neck, eyes and eyebrows) with a shape key per face
  slider and expression, built from MPFB targets as (shape with the target) - (shape without);
- `joints`: peon joint positions measured on the posed rig (see make_peons.Dims.from_mpfb).
"""

import math
import os

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree
from mathutils.kdtree import KDTree

from bl_ext.blender_org.mpfb.services.humanservice import HumanService
from bl_ext.blender_org.mpfb.services.locationservice import LocationService
from bl_ext.blender_org.mpfb.services.targetservice import TargetService

# MPFB macro sliders per stage (age: 0 is 1 year, 0.1875 is 11, 0.5 is 25, 1 is 90).
STAGE_MACROS = {
    "infant": dict(age=0.0, gender=0.5),
    "toddler": dict(age=0.03, gender=0.5),
    "child": dict(age=0.12, gender=0.5),
    "teen": dict(age=0.28, gender=0.5),
    "adult": dict(age=0.5, gender=0.5),
    "elder": dict(age=0.93, gender=0.5),
}


def both(name, w=1.0):
    return [(f"l-{name}", w), (f"r-{name}", w)]


# face slider -> MPFB targets (name, weight); the game blends each key with a weight in -1..1
FACE_TARGETS = {
    "head_width": [("head-scale-horiz-incr", 1.0)],
    "face_length": [("head-scale-vert-incr", 1.0)],
    "jaw": [("chin-bones-incr", 1.0), ("chin-width-incr", 0.5)],
    "chin": [("chin-prominent-incr", 1.0)],
    "cheeks": both("cheek-bones-incr"),
    "cheek_fat": both("cheek-volume-incr"),
    "brow": [("eyebrows-trans-forward", 1.0)],
    "eye_size": both("eye-scale-incr"),
    "eye_spacing": both("eye-trans-out"),
    "eye_tilt": both("eye-eyefold-angle-up") + both("eye-corner2-up", 0.6),
    "eye_open": both("eye-height2-incr"),
    "nose_length": [("nose-scale-vert-incr", 1.0)],
    "nose_width": [("nose-scale-horiz-incr", 1.0)],
    "nose_bridge": [("nose-hump-incr", 1.0)],
    "nose_tip": [("nose-point-up", 1.0)],
    "lips": [("mouth-upperlip-volume-incr", 1.0), ("mouth-lowerlip-volume-incr", 1.0)],
    "mouth_width": [("mouth-scale-horiz-incr", 1.0)],
    "ears": both("ear-scale-incr"),
}

# expressions from the same targets pushed past 1 (MPFB ships no face poses); blink and open are
# approximations: MakeHuman's lids and jaw are posed by face bones the game_engine rig lacks
EXPRESSION_TARGETS = {
    "x_smile": [("mouth-angles-up", 1.0), ("mouth-scale-horiz-incr", 0.4), ("mouth-laugh-lines-in", 0.6)] + both("cheek-trans-up", 0.5),
    "x_frown": [("mouth-angles-down", 1.0), ("mouth-lowerlip-ext-down", 0.4)],
    "x_open": [("chin-jaw-drop-incr", 1.0), ("mouth-lowerlip-middle-down", 1.0), ("mouth-upperlip-middle-up", 0.5)],
    "x_blink": both("eye-height2-decr", 1.6) + both("eye-height1-decr", 1.0),
    "x_brow_up": [("eyebrows-trans-up", 1.0)],
    "x_brow_angry": [("eyebrows-angle-down", 1.0), ("eyebrows-trans-down", 0.6)],
    "x_brow_sad": [("eyebrows-angle-up", 1.0)],
}

# MPFB game_engine bone -> peon bone (for _l/_r the peon suffix .L/.R)
BONE_MAP = {
    "pelvis": "hips", "spine_01": "spine", "spine_02": "spine", "spine_03": "chest",
    "neck_01": "neck", "head": "head",
    "clavicle": "chest", "upperarm": "upper_arm", "lowerarm": "forearm", "hand": "hand",
    "thumb": "hand", "index": "hand", "middle": "hand", "ring": "hand", "pinky": "hand",
    "thigh": "thigh", "calf": "shin", "foot": "foot", "ball": "foot",
}


def peon_bone(name):
    side = {"_l": ".L", "_r": ".R"}.get(name[-2:], "")
    stem = name[:-2] if side else name
    key = stem.split("_0")[0] if stem.split("_0")[0] in BONE_MAP else stem
    return BONE_MAP[key] + side if key in BONE_MAP else None


class MpfbBase:
    def __init__(self, body, head, joints, ears):
        self.body, self.head, self.joints, self.ears = body, head, joints, ears


def asset(sub, name):
    return os.path.join(LocationService.get_user_data(sub), name, f"{name}.mhclo")


def evaluated(obj):
    """World-space vertex positions of obj after its modifiers (pose, helper mask)."""
    dg = bpy.context.evaluated_depsgraph_get()
    ev = obj.evaluated_get(dg)
    me = ev.to_mesh()
    out = [obj.matrix_world @ v.co for v in me.vertices]
    ev.to_mesh_clear()
    return out


def pose_a(rig, spread):
    """Rotate MPFB's upper arms so they hang `spread` radians out from vertical, forearms and
    hands in line, matching the rest pose the peon clips are authored for."""
    for sfx, side in (("_l", 1), ("_r", -1)):
        for name in ("upperarm", "lowerarm", "hand"):
            pb = rig.pose.bones[name + sfx]
            bpy.context.view_layer.update()
            cur = (rig.matrix_world @ pb.tail - rig.matrix_world @ pb.head).normalized()
            want = Vector((side * math.sin(spread), 0, -math.cos(spread)))
            rot = cur.rotation_difference(want)
            m = pb.matrix.copy()
            loc = m.translation.copy()
            m = rot.to_matrix().to_4x4() @ m
            m.translation = loc
            pb.matrix = m
    bpy.context.view_layer.update()


def check_pins():
    """Fails unless the installed MPFB and asset pack are the ones install_mpfb.py pins."""
    import json
    import tomllib

    import install_mpfb as pin

    import bl_ext.blender_org.mpfb as mpfb_pkg

    root = os.path.dirname(mpfb_pkg.__file__)
    with open(os.path.join(root, "blender_manifest.toml"), "rb") as f:
        version = tomllib.load(f)["version"]
    stamp = os.path.join(LocationService.get_user_data(), pin.STAMP)
    got = json.load(open(stamp)) if os.path.exists(stamp) else {}
    want = {"mpfb": pin.MPFB_VERSION, "assets_sha256": pin.ASSETS_SHA256}
    if version != pin.MPFB_VERSION or got != want:
        raise SystemExit(f"MPFB {version} with asset stamp {got or 'missing'}, expected {want}: "
                         "run python3 assets/blender/install_mpfb.py")


def build(stage, H, spread):
    check_pins()
    info = TargetService.get_default_macro_info_dict()
    info.update(STAGE_MACROS[stage])
    base = HumanService.create_human(macro_detail_dict=info)
    rig = HumanService.add_builtin_rig(base, "game_engine")
    eyes = HumanService.add_mhclo_asset(os.path.join(LocationService.get_user_data("eyes"), "low-poly", "low-poly.mhclo"),
                                        base, asset_type="eyes", subdiv_levels=0)
    brows = HumanService.add_mhclo_asset(asset("eyebrows", "eyebrow001"), base, asset_type="eyebrows", subdiv_levels=0)
    # a skin only for its material slots: they mark the lips and ears faces
    skin = os.path.join(LocationService.get_user_data("skins"), "young_caucasian_male", "young_caucasian_male.mhmat")
    HumanService.set_character_skin(skin, base, skin_type="MAKESKIN")
    pose_a(rig, spread)

    # keep each evaluated vertex's source index, so weights and keys can be read back by index
    src = base.data.attributes.new("src", "INT", "POINT")
    src.data.foreach_set("value", list(range(len(base.data.vertices))))

    keys = {}
    for slider, targets in list(FACE_TARGETS.items()) + list(EXPRESSION_TARGETS.items()):
        for tname, _w in targets:
            if tname not in keys:
                kb = TargetService.load_target(base, TargetService.target_full_path(tname), weight=0.0, name=tname)
                keys[tname] = kb if kb is not None else base.data.shape_keys.key_blocks[-1]

    def snapshot():
        HumanService.refit(base)
        return [evaluated(o) for o in (base, eyes, brows)]

    rest = snapshot()
    deltas = {}
    for slider, targets in list(FACE_TARGETS.items()) + list(EXPRESSION_TARGETS.items()):
        for tname, w in targets:
            keys[tname].value = w  # MPFB target values range -1..1 but deltas are linear: clamp-free
            keys[tname].slider_max = max(keys[tname].slider_max, w)
        shot = snapshot()
        deltas[slider] = [[b - a for a, b in zip(r, s)] for r, s in zip(rest, shot)]
        for tname, _w in targets:
            keys[tname].value = 0.0
    HumanService.refit(base)

    # bake the evaluated base into a plain mesh, scaled to the stage height
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(base.evaluated_get(dg), preserve_all_data_layers=True, depsgraph=dg)
    top = max(v.z for v in rest[0])
    s = H / top
    S = Matrix.Scale(s, 4)
    me.transform(S @ base.matrix_world)
    srcs = [0] * len(me.vertices)
    me.attributes["src"].data.foreach_get("value", srcs)

    # peon bone weights per source vertex, from MPFB's groups
    names = {g.index: g.name for g in base.vertex_groups}
    weights = {}
    for v in base.data.vertices:
        ws = {}
        for g in v.groups:
            pb = peon_bone(names[g.group])
            if pb and g.weight > 1e-4:
                ws[pb] = ws.get(pb, 0.0) + g.weight
        tot = sum(ws.values()) or 1.0
        weights[v.index] = {k: w / tot for k, w in ws.items()}

    joints = measure(rig, s)

    whole = bpy.data.objects.new("mpfb_whole", me)
    bpy.context.collection.objects.link(whole)
    # mostly-head vertices (earlobes, jaw) go with the head; the head keeps its real weights, so the
    # seam's shared vertices skin identically on both meshes
    head_vert = [weights[srcs[i]].get("head", 0.0) > 0.5 for i in range(len(me.vertices))]
    mats = [m.name.split(".")[-1] if m else "" for m in me.materials]

    # split: faces entirely on the head bone become the head, the rest the body
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    head_faces = [f for f in bm.faces if all(head_vert[v.index] for v in f.verts)]
    body = split_off(whole, bm, head_faces, "body")
    head = whole
    head.name = head.data.name = "head"

    # --- body: peon materials and weights
    body.data.materials.clear()  # cut_regions assigns the skin.rN regions
    body_src = attr(body, "src")
    body.vertex_groups.clear()
    for bone in sorted({b for ws in weights.values() for b in ws}):
        body.vertex_groups.new(name=bone)
    for vi, si in enumerate(body_src):
        for bone, w in weights[si].items():
            body.vertex_groups[bone].add([vi], w, "REPLACE")

    # --- head: skin + lips, then eyes and brows joined in; keys from the deltas
    ears = set()
    head_src = attr(head, "src")
    head.vertex_groups.clear()
    # chin: the lowest point of the face's midline
    hj = joints["head"]
    mid = [v.co.copy() for v in head.data.vertices if abs(v.co.x) < 0.01 * H and v.co.y < hj.y - 0.03 * H]
    joints["chin"] = min(mid, key=lambda c: c.z)
    for poly in head.data.polygons:
        if mats[poly.material_index] == "ears":
            ears.update(poly.vertices)
    new_mats = [material("skin.lips" if n == "lips" else "skin") for n in mats]
    for i, m in enumerate(new_mats):
        head.data.materials[i] = m
    n_head = len(head.data.vertices)
    eye_obj = bake_proxy(eyes, S, "eyes")
    paint_eyes(eye_obj, joints)
    brow_obj = bake_proxy(brows, S, "brow")
    brow_obj.data.materials.clear()
    brow_obj.data.materials.append(material("hair"))
    n_eyes = len(eye_obj.data.vertices)
    join(head, [eye_obj, brow_obj])

    head.shape_key_add(name="Basis")
    for slider, (dbody, deye, dbrow) in deltas.items():
        offs = [s * dbody[si] for si in head_src] + [s * d for d in deye] + [s * d for d in dbrow]
        if max(o.length for o in offs) < 1e-5:
            continue
        kb = head.shape_key_add(name=slider, from_mix=False)
        for v, o in zip(kb.data, offs):
            v.co = v.co + o
    groups = {}
    for vi, si in enumerate(head_src):
        for bone, w in weights[si].items():
            if bone not in groups:
                groups[bone] = head.vertex_groups.new(name=bone)
            groups[bone].add([vi], w, "REPLACE")
    if "head" not in groups:
        groups["head"] = head.vertex_groups.new(name="head")
    groups["head"].add(list(range(len(head_src), len(head.data.vertices))), 1.0, "REPLACE")  # eyes, brows
    for o in (body, head):
        o.data.attributes.remove(o.data.attributes["src"])
        for p in o.data.polygons:
            p.use_smooth = True

    # MPFB's scene objects are no longer needed
    for o in [base, rig, eyes, brows]:
        bpy.data.objects.remove(o)
    head_info = (n_head, n_eyes)
    out = MpfbBase(body, head, joints, ears)
    out.head_parts = head_info
    return out


def attr(obj, name):
    vals = [0] * len(obj.data.vertices)
    obj.data.attributes[name].data.foreach_get("value", vals)
    return vals


def material(name):
    return bpy.data.materials.get(name) or bpy.data.materials.new(name)


def split_off(obj, bm, keep_faces, name):
    """Splits obj: `keep_faces` stay in obj, every other face moves to a new object `name`."""
    other = bm.copy()
    keep = {f.index for f in keep_faces}
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.index not in keep], context="FACES")
    other.faces.ensure_lookup_table()
    bmesh.ops.delete(other, geom=[f for f in other.faces if f.index in keep], context="FACES")
    for b in (bm, other):
        bmesh.ops.delete(b, geom=[v for v in b.verts if not v.link_faces], context="VERTS")
    bm.to_mesh(obj.data)
    me = obj.data.copy()
    other.to_mesh(me)
    me.name = name
    new = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(new)
    return new


def bake_proxy(obj, S, name):
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(obj.evaluated_get(dg), depsgraph=dg)
    me.transform(S @ obj.matrix_world)
    me.name = name
    new = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(new)
    return new


def paint_eyes(eyes, joints):
    """eye_white / iris / pupil by angle from each eyeball's forward axis (the game has no UVs)."""
    me = eyes.data
    me.materials.clear()
    for n in ("eye_white", "iris", "pupil"):
        me.materials.append(material(n))
    bm = bmesh.new()
    bm.from_mesh(me)
    for side in (1, -1):
        verts = [v for v in bm.verts if v.co.x * side > 0]
        c = sum((v.co for v in verts), Vector()) / max(len(verts), 1)
        r = max((v.co - c).length for v in verts)
        for f in bm.faces:
            p = f.calc_center_median()
            if (p.x > 0) != (side > 0):
                continue
            ang = (p - c).normalized().dot(Vector((0, -1, 0)))
            f.material_index = 2 if ang > 0.965 else 1 if ang > 0.86 else 0
    bm.to_mesh(me)
    bm.free()


def join(target, others):
    bpy.ops.object.select_all(action="DESELECT")
    for o in [target] + others:
        o.select_set(True)
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.join()


def measure(rig, s):
    """Peon joint positions (scaled) from the posed MPFB rig."""
    bpy.context.view_layer.update()
    mw = rig.matrix_world

    def h(b):
        return s * (mw @ rig.pose.bones[b].head)

    def t(b):
        return s * (mw @ rig.pose.bones[b].tail)

    j = {
        "hips": h("pelvis"), "spine": h("spine_01"), "chest": h("spine_03"),
        "neck": h("neck_01"), "head": h("head"), "head_top": t("head"),
    }
    for sfx, side in (("_l", 1), ("_r", -1)):
        j[("arm", side)] = (h("upperarm" + sfx), h("lowerarm" + sfx), h("hand" + sfx), t("middle_03" + sfx))
        j[("leg", side)] = (h("thigh" + sfx), h("calf" + sfx), h("foot" + sfx), t("ball" + sfx))
    return j


def fit_hair(objs, mpfb_head, peon_head, centre):
    """Moves hair shells built over the parametric peon head onto the MPFB head: each vertex keeps
    its height above the scalp along the ray from the head centre (vertices whose ray misses a head
    take the shift of the nearest one that hit), and its face keys become those of the nearest MPFB
    skin vertex."""
    def bvh(obj, skin_only):
        me = obj.data
        mats = [m.name if m else "" for m in me.materials]
        polys = [p for p in me.polygons if not skin_only or mats[p.material_index].startswith("skin")]
        return BVHTree.FromPolygons([v.co for v in me.vertices], [tuple(p.vertices) for p in polys])

    old, new = bvh(peon_head, False), bvh(mpfb_head, True)
    me = mpfb_head.data
    mats = [m.name if m else "" for m in me.materials]
    skin = sorted({i for p in me.polygons if mats[p.material_index].startswith("skin") for i in p.vertices})
    kd = KDTree(len(skin))
    for i in skin:
        kd.insert(me.vertices[i].co, i)
    kd.balance()
    keys = me.shape_keys.key_blocks
    for obj in objs:
        hm = obj.data
        rest = [v.co.copy() for v in hm.vertices]
        shift = [None] * len(rest)
        for i, p in enumerate(rest):
            d = (p - centre).normalized()
            a, b = old.ray_cast(centre, d), new.ray_cast(centre, d)
            if a[0] is not None and b[0] is not None:
                shift[i] = d * ((b[0] - centre).length - (a[0] - centre).length)
        hits = [i for i, s in enumerate(shift) if s is not None]
        if os.environ.get("PEON_DEBUG"):
            ls = sorted(s.length * (1 if s.dot((rest[i] - centre)) > 0 else -1) for i, s in enumerate(shift) if s is not None)
            print("RES fit", obj.name, len(ls), "/", len(rest), "shift min/med/max", ls[0] if ls else None, ls[len(ls)//2] if ls else None, ls[-1] if ls else None)
        if not hits:
            continue
        hk = KDTree(len(hits))
        for i in hits:
            hk.insert(rest[i], i)
        hk.balance()
        moved = [p + (shift[i] if shift[i] is not None else shift[hk.find(p)[1]]) for i, p in enumerate(rest)]
        if hm.shape_keys:
            obj.shape_key_clear()
        for v, p in zip(hm.vertices, moved):
            v.co = p
        near = [kd.find(p)[1] for p in moved]
        obj.shape_key_add(name="Basis")
        for kb in keys[1:]:
            offs = [kb.data[j].co - keys[0].data[j].co for j in near]
            if max(o.length for o in offs) < 1e-5:
                continue  # glTF stores targets densely; the exporter writes empty ones without data
            k = obj.shape_key_add(name=kb.name, from_mix=False)
            for v, o in zip(k.data, offs):
                v.co = v.co + o
