"""Body mesh, skin regions, body-shape keys and clothing for the peon generator (see make_peons.py).

The body is a skin-modifier mesh grown from a stick figure that follows the rig. Its faces are cut
along the hems clothes use (neckline, sleeves, belt, shorts, shoes) and every skin face gets the
material `skin.rN` for the region N it lies in, so the game can hide skin a garment covers
(no poke-through) by region. Garments are copies of the body's region faces pushed out along the
normals, so they share its skinning and shape keys.
"""

import math

import bmesh
import bpy
from mathutils import Vector
from mathutils.kdtree import KDTree

# skin regions (bit numbers of the game's hide mask); 0 = always drawn (head, neck, hands)
R_CHEST, R_BELLY, R_SHOULDER, R_UPPER_ARM, R_FOREARM, R_PELVIS, R_THIGH, R_KNEE, R_SHIN, R_FOOT, R_YOKE, R_STRAP = range(1, 13)
# R_CHEST runs up to the neck between the straps' outer edges; R_YOKE is the top of the torso
# beside them, out to the armhole (bare in a tank top); R_STRAP is the part of the straps above the
# neckline (sleeveless tops reach up over the shoulder there); R_SHOULDER is the cap of the arm

# body shape keys: id -> label (the order of the keys); slim/heavy are driven by `build`
BODY_SLIDERS = [
    ("frame", "Frame"),
    ("muscle", "Muscle"),
    ("shoulders", "Shoulders"),
    ("bust", "Chest"),
    ("waist", "Waist"),
    ("hips", "Hips"),
    ("belly", "Belly"),
]

# garments: name -> (palette material, regions it covers, thickness as a fraction of H)
GARMENTS = {
    "top_tee": ("shirt", {R_CHEST, R_STRAP, R_YOKE, R_BELLY, R_SHOULDER}, 0.0045),
    "top_long": ("shirt", {R_CHEST, R_STRAP, R_YOKE, R_BELLY, R_SHOULDER, R_UPPER_ARM, R_FOREARM}, 0.0045),
    "top_tank": ("shirt", {R_CHEST, R_STRAP, R_BELLY}, 0.004),
    "top_dress": ("shirt", {R_CHEST, R_STRAP, R_BELLY, R_PELVIS}, 0.0045),
    "bottom_trousers": ("pants", {R_PELVIS, R_THIGH, R_KNEE, R_SHIN}, 0.005),
    "bottom_shorts": ("pants", {R_PELVIS, R_THIGH}, 0.005),
    "bottom_skirt": ("pants", {R_PELVIS}, 0.005),
    "shoes": ("shoes", {R_FOOT}, 0.007),
}
SKIRTS = {"top_dress": 0.78, "bottom_skirt": 0.55}  # hem as a fraction of the leg down to the knee... and past


def link(obj):
    bpy.context.scene.collection.objects.link(obj)
    return obj


def select_only(*objs):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]


# ------------------------------------------------------------------------------------------------
# landmarks


def torso_z(d, f):
    return d.legs + d.torso * f


class Hems:
    """Where garments end, shared by the region cuts and the skirt builder."""

    def __init__(self, d, arm_points, leg_points):
        self.strap_x = d.width * 0.3  # outer edge of a tank top's straps
        self.strap_in = d.width * 0.19  # inner edge: between the two the neckline doesn't cut
        self.collar = d.neck_z  # tops of straps and shoulders: above this is the neck
        self.armhole_z = torso_z(d, 0.8)  # where a tank top's armhole crosses the strap's edge
        self.neckline = d.neck_z - d.torso * 0.045
        self.neck_normal = Vector((0, -0.3, 1)).normalized()  # a little lower in front: a crew neck
        self.chest = torso_z(d, 0.52)
        self.belt = torso_z(d, 0.14)
        self.shoe = d.ankle_z + d.H * 0.032
        self.hand_reach = d.H * 0.14
        self.arms = {}
        self.legs = {}
        for side, sfx in ((1, "L"), (-1, "R")):
            sh, el, wr, tip = arm_points(d, side)
            ax = (el - sh).normalized()
            self.arms[sfx] = dict(
                # torso/arm boundary: a plane through the armpit leaning out
                # armhole: one plane through the armpit, leaning out like the A-posed arm, so the
                # flank stays torso and the top of the shoulder is the sleeve's cap
                shoulder=(Vector((side * d.shoulder_x * 0.8, 0, d.shoulder_z)), Vector((side * 0.85, 0, 0.5)).normalized()),
                sleeve=(sh.lerp(el, 0.42), ax),
                elbow=(el, ax),
                wrist=(wr + (wr - el).normalized() * d.H * 0.004, (wr - el).normalized()),
            )
            hip, knee, ankle, toe = leg_points(d, side)
            lx = (knee - hip).normalized()
            self.legs[sfx] = dict(shorts=(hip.lerp(knee, 0.42), lx), knee=(knee + lx * d.H * 0.02, lx))

    def armhole(self, sfx):
        """Lower edge of the yoke: a plane rising from under the arm to the strap (a tank top's
        curved armhole)."""
        s = 1 if sfx == "L" else -1
        return Vector((s * self.strap_x, 0, self.armhole_z)), Vector((s * 0.6, 0, 0.8)).normalized()


# ------------------------------------------------------------------------------------------------
# body mesh


MB_STIFF = 10.0
MB_SCALE = 1 / 0.78  # a lone metaball element's surface sits at 0.78 x its radius (stiffness 10)


class Meta:
    """Builds a metaball from primitives given by their surface sizes."""

    def __init__(self, name):
        self.mb = bpy.data.metaballs.new(name)
        self.mb.threshold = 0.6
        self.obj = link(bpy.data.objects.new(name, self.mb))

    def ell(self, c, rx, ry, rz, rot=None):
        e = self.mb.elements.new()
        e.type = "ELLIPSOID"
        r = max(rx, ry, rz)
        e.radius = r * MB_SCALE
        e.size_x, e.size_y, e.size_z = rx / r, ry / r, rz / r
        e.co = Vector(c)
        if rot is not None:
            e.rotation = rot
        e.stiffness = MB_STIFF

    def cap(self, a, b, r, trim=(0.0, 0.0)):
        """Capsule from a to b; trim shortens either end by that many radii (so chained capsules
        don't double up their fields into a bulge at the shared joint)."""
        a, b = Vector(a), Vector(b)
        dirv = (b - a).normalized()
        a, b = a + dirv * r * trim[0], b - dirv * r * trim[1]
        e = self.mb.elements.new()
        e.type = "CAPSULE"
        e.radius = r * MB_SCALE
        e.co = (a + b) * 0.5
        e.size_x = (b - a).length * 0.5  # absolute half-length
        e.rotation = Vector((1, 0, 0)).rotation_difference((b - a).normalized())
        e.stiffness = MB_STIFF


def make_body(d, arm_points, leg_points, stubs=None):
    """Metaball body (organic blends at the shoulders, hips and armpits), voxel-remeshed into an
    even quad mesh. The hands and feet are finer meshes welded on afterwards (peon_extremities.py);
    `stubs(m)` adds their elements near the weld so the shapes agree there."""
    w, dp, H = d.width, d.depth, d.H
    T = d.torso
    tz = lambda f: torso_z(d, f)
    m = Meta("body_mb")
    m.mb.resolution = H * 0.006
    m.mb.render_resolution = H * 0.006
    # torso masses
    m.ell((0, 0.008 * H, tz(0.13)), w * 0.31, dp * 0.44, T * 0.15)  # pelvis
    for sx in (1, -1):
        m.ell((sx * w * 0.13, dp * 0.16, tz(0.05)), w * 0.15, dp * 0.28, T * 0.13)  # glutes
    m.ell((0, 0.0, tz(0.38)), w * 0.29, dp * 0.41, T * 0.26)  # abdomen
    m.ell((0, 0.0, tz(0.64)), w * 0.35, dp * 0.47, T * 0.25)  # ribcage
    for sx in (1, -1):
        m.ell((sx * w * 0.14, -dp * 0.15, tz(0.75)), w * 0.16, dp * 0.22, T * 0.1)  # pecs
    m.cap((-d.shoulder_x * 0.7, 0.012 * H, d.shoulder_z - T * 0.02), (d.shoulder_x * 0.7, 0.012 * H, d.shoulder_z - T * 0.02), dp * 0.3)  # shoulder girdle
    for sx in (1, -1):
        m.cap((0, 0.02 * H, d.neck_z - T * 0.02), (sx * d.shoulder_x * 0.75, 0.015 * H, d.shoulder_z - T * 0.01), d.limb * 0.5)  # traps
    m.cap((0, 0.008 * H, d.neck_z - T * 0.05), (0, 0.002 * H, d.neck_z + d.head_h * 0.32), d.limb * 0.6)  # neck
    ar, tr = d.arm_r, d.thigh_r
    for side in (1, -1):
        sh, el, wr, tip = arm_points(d, side)
        ax = (el - sh).normalized()
        m.ell(sh + ax * ar * 0.5 + Vector((side * ar * 0.1, 0, 0)), ar * 1.25, ar * 1.25, ar * 1.5,
              Vector((0, 0, 1)).rotation_difference(-ax))  # deltoid
        m.cap(sh, el, ar * 0.98, (0, 0.5))
        m.ell(sh.lerp(el, 0.55) + Vector((0, -ar * 0.25, 0)), ar * 0.7, ar * 0.75, ar * 1.6, Vector((0, 0, 1)).rotation_difference(-ax))  # biceps
        fa = (wr - el).normalized()
        m.cap(el, wr, ar * 0.68, (0.3, 0))
        m.ell(el.lerp(wr, 0.3), ar * 0.92, ar * 0.85, (wr - el).length * 0.32, Vector((0, 0, 1)).rotation_difference(fa))  # forearm mass
        # the hand is its own finer mesh (peon_extremities.py)
        hip, knee, ankle, toe = leg_points(d, side)
        m.cap(hip, knee, tr * 0.78, (0, 0.4))
        m.ell(hip.lerp(knee, 0.3) + Vector((side * tr * 0.08, -tr * 0.05, 0)), tr * 0.95, tr * 0.95, (knee - hip).length * 0.42)  # thigh mass
        m.cap(knee, ankle, tr * 0.5, (0.4, 0))
        m.ell(knee.lerp(ankle, 0.3) + Vector((side * tr * 0.03, tr * 0.14, 0)), tr * 0.64, tr * 0.64, (knee - ankle).length * 0.27)  # calf
    if stubs:
        stubs(m)
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(m.obj.evaluated_get(dg))
    me.name = "body"
    bpy.data.objects.remove(m.obj)
    obj = link(bpy.data.objects.new("body", me))
    select_only(obj)
    rm = obj.modifiers.new("remesh", "REMESH")
    rm.mode = "VOXEL"
    rm.voxel_size = H * 0.0105
    bpy.ops.object.modifier_apply(modifier="remesh")
    sm = obj.modifiers.new("smooth", "CORRECTIVE_SMOOTH")
    sm.iterations = 4
    sm.use_only_smooth = True
    sm.smooth_type = "LENGTH_WEIGHTED"
    bpy.ops.object.modifier_apply(modifier="smooth")
    # flatten the soles onto the floor
    for v in me.vertices:
        if v.co.z < d.H * 0.004:
            v.co.z = d.H * 0.004 * 0.5 + max(0.0, v.co.z) * 0.5
    bpy.ops.object.shade_smooth()
    return obj


# ------------------------------------------------------------------------------------------------
# regions


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


def side_x(sfx):
    return 1 if sfx == "L" else -1


def side_of(p):
    return "L" if p.x > 0 else "R"


def region_of(c, segs, hems, n=None):
    """Skin region of a point on the body (n: its face normal, if known)."""
    sfx = side_of(c)
    arm = hems.arms[sfx]
    wco, wno = arm["wrist"]
    if (c - wco).dot(wno) > 0 and (c - wco).length < hems.hand_reach:
        return 0  # the hand (fingertips can lie nearer the thigh bone than the hand's)
    co, no = arm["shoulder"]
    bn, _ = nearest_bone(c, segs)
    is_arm = bn.startswith(("upper_arm", "forearm", "hand"))
    is_leg = bn.startswith(("thigh", "shin", "foot"))
    past = (c - co).dot(no) > 0 and c.z > hems.belt and bn not in ("neck", "head") and not is_leg
    # the inside of the hanging arm lies within the armhole plane: it faces the body, where the
    # torso's flank (also nearest the arm bones under the armpit) faces away from it
    inner = is_arm and n is not None and n.x * (1 if c.x > 0 else -1) < -0.1
    if past or inner or (is_arm and c.z <= hems.belt):
        if (c - arm["wrist"][0]).dot(arm["wrist"][1]) > 0:
            return 0
        if (c - arm["elbow"][0]).dot(arm["elbow"][1]) > 0:
            return R_FOREARM
        if (c - arm["sleeve"][0]).dot(arm["sleeve"][1]) > 0:
            return R_UPPER_ARM
        return R_SHOULDER
    if bn.startswith(("thigh", "shin", "foot")) and c.z < hems.belt:
        leg = hems.legs[sfx]
        if c.z < hems.shoe:
            return R_FOOT
        if (c - leg["knee"][0]).dot(leg["knee"][1]) > 0:
            return R_SHIN
        if (c - leg["shorts"][0]).dot(leg["shorts"][1]) > 0:
            return R_KNEE
        return R_THIGH
    if (c - Vector((0, 0, hems.neckline))).dot(hems.neck_normal) > 0 or bn in ("neck", "head"):
        if hems.chest < c.z and hems.strap_in < abs(c.x) < hems.strap_x and bn != "head":
            return R_STRAP  # straps run on over the top of the shoulder, which rises above the collar
        if hems.chest < c.z < hems.collar and abs(c.x) > hems.strap_in:
            return R_YOKE
        return 0
    if c.z < hems.belt:
        return R_PELVIS
    if c.z < hems.chest:
        return R_BELLY
    return R_YOKE if abs(c.x) > hems.strap_x and (c - hems.armhole(sfx)[0]).dot(hems.armhole(sfx)[1]) > 0 else R_CHEST


SNAP = 0.3  # crossing-edge fraction within which a vertex is slid onto a cut plane


def cut_regions(body, rig, hems):
    """Cuts edge loops along every hem so region boundaries are straight, then assigns each face
    the material `skin.rN`."""
    me = body.data
    segs = bone_segments(rig)
    bm = bmesh.new()
    bm.from_mesh(me)

    def cut(pred, co, no):
        faces = [f for f in bm.faces if pred(f.calc_center_median())]
        edges = list({e for f in faces for e in f.edges})
        # a vertex just beside the plane would leave a sliver row that shades as a line along the
        # hem: slide it along its crossing edge onto the plane first, so the cut passes through it
        co, no = Vector(co), Vector(no).normalized()
        moved = set()
        for e in edges:
            a, b = e.verts
            da, db = (a.co - co).dot(no), (b.co - co).dot(no)
            if da * db >= 0 or a in moved or b in moved:
                continue
            t = da / (da - db)
            if t < SNAP:
                a.co = a.co.lerp(b.co, t)
                moved.add(a)
            elif t > 1 - SNAP:
                b.co = b.co.lerp(a.co, 1 - t)
                moved.add(b)
        geom = edges + faces + list({v for f in faces for v in f.verts})
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=co, plane_no=no, dist=1e-6)

    is_arm = lambda c: nearest_bone(c, segs)[0].startswith(("upper_arm", "forearm", "hand"))
    in_hand = lambda c: any((c - w[0]).dot(w[1]) > 0 and (c - w[0]).length < hems.hand_reach for w in (a["wrist"] for a in hems.arms.values()))
    is_leg = lambda c: nearest_bone(c, segs)[0].startswith(("thigh", "shin", "foot")) and not in_hand(c)
    torso = lambda c: not is_arm(c) and nearest_bone(c, segs)[0] not in ("head",)
    cut(torso, (0, 0, hems.neckline), hems.neck_normal)
    cut(torso, (0, 0, hems.collar), (0, 0, 1))
    cut(lambda c: torso(c) and not is_leg(c), (0, 0, hems.chest), (0, 0, 1))
    cut(lambda c: not is_arm(c), (0, 0, hems.belt), (0, 0, 1))
    cut(is_leg, (0, 0, hems.shoe), (0, 0, 1))
    for sfx, arm in hems.arms.items():
        mine = lambda c, sfx=sfx: side_of(c) == sfx
        cut(lambda c, m=mine: m(c) and c.z > hems.belt - 0.02 and not is_leg(c), *arm["shoulder"])
        # the top of the shoulder lies nearest the arm and neck bones, so the strap and armhole cuts
        # take everything inboard of the armhole plane (else their edges step where they cross it)
        co, no = arm["shoulder"]
        trunk = lambda c, m=mine, co=co, no=no: m(c) and c.z > hems.chest - 0.02 and (c - co).dot(no) < 0.01 and nearest_bone(c, segs)[0] != "head"
        for x in (hems.strap_x, hems.strap_in):
            cut(trunk, (side_x(sfx) * x, 0, 0), (side_x(sfx), 0, 0))
        cut(lambda c, t=trunk: t(c) and abs(c.x) > hems.strap_x - 0.01, *hems.armhole(sfx))
        for k in ("sleeve", "elbow", "wrist"):
            cut(lambda c, m=mine: m(c) and is_arm(c), *arm[k])
        for k in ("shorts", "knee"):
            cut(lambda c, m=mine: m(c) and is_leg(c), *hems.legs[sfx][k])

    names = [f"skin.r{i}" for i in range(R_STRAP + 1)]
    for n in names:
        if n not in bpy.data.materials:
            m = bpy.data.materials["skin"].copy()
            m.name = n
        me.materials.append(bpy.data.materials[n])
    for f in bm.faces:
        f.material_index = region_of(f.calc_center_median(), segs, hems, f.normal)
        f.smooth = True
    bm.to_mesh(me)
    bm.free()


# ------------------------------------------------------------------------------------------------
# skinning


def weight_body(body, rig):
    """Bone-heat automatic weights; any vertex left unweighted gets its nearest bone."""
    select_only(rig)
    body.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    segs = bone_segments(rig)
    groups = {g.index: g.name for g in body.vertex_groups}
    for vtx in body.data.vertices:
        if sum(g.weight for g in vtx.groups if g.group in groups) < 1e-4:
            bn, _ = nearest_bone(vtx.co, segs)
            body.vertex_groups[bn].add([vtx.index], 1.0, "REPLACE")


def vertex_bone_weights(obj):
    names = {g.index: g.name for g in obj.vertex_groups}
    out = []
    for vtx in obj.data.vertices:
        ws = {names[g.group]: g.weight for g in vtx.groups if g.weight > 0}
        tot = sum(ws.values()) or 1.0
        out.append({k: w / tot for k, w in ws.items()})
    return out


# ------------------------------------------------------------------------------------------------
# body shape keys

# radial offsets in fractions of H per bone group: (slim, heavy)
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


def g1(x, s):
    return math.exp(-((x / s) ** 2))


def body_key_offset(key, d, co, n, ws, segs):
    """Offset of a body vertex for one shape key at full weight (model units)."""
    H = d.H
    out = Vector()
    front = max(0.0, -n.y)
    side = abs(n.x)
    w = lambda *names: sum(ws.get(f"{b}.L", 0) + ws.get(f"{b}.R", 0) + ws.get(b, 0) for b in names)
    tz = (co.z - d.legs) / d.torso  # 0 at the crotch, 1 at the neck
    if key in ("slim", "heavy"):
        i = 0 if key == "slim" else 1
        off = sum(wt * BUILD[bn.split(".")[0]][i] for bn, wt in ws.items())
        out += n * off * H
        if key == "heavy":
            out += n * H * 0.022 * front * w("spine", "hips") * g1(tz - 0.32, 0.25)
    elif key == "muscle":
        out += n * H * 0.007 * w("upper_arm") * (0.6 + 0.4 * front)
        out += n * H * 0.004 * w("forearm")
        out += n * H * 0.006 * w("thigh")
        out += n * H * 0.005 * w("shin") * (0.4 + max(0.0, n.y))
        out += n * H * 0.006 * w("neck")
        out += n * H * 0.010 * w("chest") * (front * g1(tz - 0.74, 0.12) + 0.6 * side * g1(tz - 0.65, 0.2))
    elif key == "frame":
        # masculine (+) / feminine (-): broader chest and shoulders against hips and waist
        # the game moves the shoulder and hip joints by frame (the arms and legs follow their
        # bones), so the key only reshapes the torso: broad rib cage and lats against a wide pelvis
        # (a key is linear, so this is half the masculine-feminine difference: an hourglass at -1,
        # a V-taper at +1)
        out += Vector((math.copysign(1, co.x), 0, 0)) * H * 0.018 * w("chest", "spine") * g1(tz - 0.76, 0.2) * min(1.0, abs(co.x) / (d.width * 0.3))
        outer = max(0.0, n.x * math.copysign(1, co.x))  # the hips' outside, not the inner thighs
        out -= n * H * 0.026 * outer * w("hips", "thigh") * g1(tz - 0.06, 0.15)
        out -= n * H * 0.013 * max(0.0, n.y) * w("hips", "thigh") * g1(tz - 0.04, 0.13)  # flatter seat
        out += n * H * 0.012 * outer * w("spine", "hips") * g1(tz - 0.42, 0.1)  # waist
        out += n * H * 0.005 * w("neck")
        out += n * H * 0.003 * w("upper_arm", "forearm")
    elif key == "shoulders":
        k = w("chest", "upper_arm") * g1(tz - 0.87, 0.12) * min(1.0, abs(co.x) / (d.width * 0.3))
        out += Vector((math.copysign(1, co.x), 0, 0)) * H * 0.015 * k
    elif key == "bust":
        for sx in (1, -1):
            bx, bz = sx * d.width * 0.2, torso_z(d, 0.69)
            g = g1(co.x - bx, d.width * 0.14) * g1(co.z - bz, d.width * 0.14) * front * w("chest", "spine")
            out += Vector((0, -1, -0.25)) * H * 0.04 * g
    elif key == "waist":
        out += n * H * 0.016 * (1 - front * 0.5) * w("spine", "hips", "chest") * g1(tz - 0.38, 0.12)
    elif key == "hips":
        out += n * H * 0.016 * max(side, max(0.0, n.y)) * w("hips", "thigh") * g1(tz - 0.06, 0.16)
    elif key == "belly":
        out += n * H * 0.035 * front * w("spine", "hips") * g1(tz - 0.3, 0.18) * g1(co.x, d.width * 0.3)
    return out


def add_body_keys(body, d, smooth=8):
    """slim, heavy and the body sliders as shape keys; each key's offsets are smoothed over the
    mesh so they don't step where the bone weights change."""
    weights = vertex_bone_weights(body)
    body.shape_key_add(name="Basis")
    me = body.data
    nbrs = [[] for _ in me.vertices]
    for e in me.edges:
        a, b = e.vertices
        nbrs[a].append(b)
        nbrs[b].append(a)
    for key in ["slim", "heavy"] + [k for k, _ in BODY_SLIDERS]:
        off = [body_key_offset(key, d, v.co, v.normal, ws, None) for v, ws in zip(me.vertices, weights)]
        for _ in range(smooth):
            off = [o * 0.5 + sum((off[j] for j in nb), Vector()) * (0.5 / len(nb)) if nb else o for o, nb in zip(off, nbrs)]
        sk = body.shape_key_add(name=key, from_mix=False)
        for v, o in zip(me.vertices, off):
            sk.data[v.index].co = v.co + o


# ------------------------------------------------------------------------------------------------
# clothing


# extra room per region as a fraction of H, on top of a garment's thickness: cloth stands off the
# belly, the sleeves and the lower legs but sits close at the chest, shoulders and waistband
EASE = {R_CHEST: 0.001, R_STRAP: 0.0, R_YOKE: 0.0005, R_SHOULDER: 0.0015, R_BELLY: 0.005, R_UPPER_ARM: 0.004,
        R_FOREARM: 0.005, R_PELVIS: 0.002, R_THIGH: 0.004, R_KNEE: 0.006, R_SHIN: 0.008}
# open edges that hang free (sleeve, shirt and leg hems) flare out by this much, fading over HEM_FADE;
# necklines and waistbands (edges of the other regions) stay snug
HEM_FLARE = {R_UPPER_ARM: 0.006, R_FOREARM: 0.004, R_SHOULDER: 0.006, R_BELLY: 0.005, R_THIGH: 0.006, R_SHIN: 0.006}
HEM_FADE = 0.06
DRAPE_STEPS = 160  # normal-only smoothing passes: how far cloth bridges hollows (under the bust, calves, rolls)


def garment(body, name, mat, regions, thick, d, drape=True, rig=None):
    """A copy of the body's faces in `regions`, pushed out along the normals by the thickness plus
    some ease, then draped (see `drape`) for every shape key, recoloured with the garment's palette
    material."""
    select_only(body)
    bpy.ops.object.duplicate()
    g = bpy.context.view_layer.objects.active
    g.name = name
    g.data.name = name
    me = g.data
    keep = {i for i, m in enumerate(me.materials) if m.name.startswith("skin.r") and int(m.name[6:]) in regions}
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.material_index not in keep], context="FACES")
    # the body has unwelded seams; cloth is one sheet, so weld them (else they show as ridges)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=3e-4 * d.H)
    bm.to_mesh(me)
    bm.free()
    normals = [v.normal.copy() for v in me.vertices]
    region = [int(m.name[6:]) for m in me.materials]
    ease = garment_ease(me, region, thick, d) if drape else [thick * d.H] * len(me.vertices)
    turned = [e + thick * d.H * 1.2 for e in ease]
    for kb in me.shape_keys.key_blocks:
        co = [kb.data[i].co + n * e for i, (n, e) in enumerate(zip(normals, ease))]
        if drape:
            co = drape_shell(me, [v.co for v in kb.data], co, normals, ease)
            if rig is not None:
                co = hang_tubes(me, region, rig, co, d)
        for i, c in enumerate(co):
            kb.data[i].co = c
    for v, b in zip(me.vertices, me.shape_keys.key_blocks[0].data):
        v.co = b.co
    turn_hems(me, normals, turned)
    me.materials.clear()
    me.materials.append(bpy.data.materials[mat])
    return g


def neighbours(me):
    adj = [set() for _ in me.vertices]
    for e in me.edges:
        a, b = e.vertices
        adj[a].add(b)
        adj[b].add(a)
    return [list(a) for a in adj]


def garment_ease(me, region, thick, d):
    """Per-vertex stand-off from the skin: thickness, the region's ease, and a flare that grows
    towards free-hanging hems, smoothed so region seams don't step."""
    n = len(me.vertices)
    adj = neighbours(me)
    acc, cnt = [0.0] * n, [0] * n
    hem_flare = [0.0] * n
    for p in me.polygons:
        r = region[p.material_index]
        for i in p.vertices:
            acc[i] += EASE.get(r, 0.0)
            cnt[i] += 1
    edge_faces = {}
    for p in me.polygons:
        for k in p.edge_keys:
            edge_faces.setdefault(k, []).append(region[p.material_index])
    for (a, b), rs in edge_faces.items():
        if len(rs) == 1:
            f = HEM_FLARE.get(rs[0], 0.0)
            hem_flare[a] = max(hem_flare[a], f)
            hem_flare[b] = max(hem_flare[b], f)
    # spread the flare inwards by distance along the surface (Dijkstra-ish relaxation)
    dist = [0.0 if f > 0 else math.inf for f in hem_flare]
    src = list(hem_flare)
    fade = HEM_FADE * d.H
    todo = [i for i in range(n) if hem_flare[i] > 0]
    while todo:
        nxt = []
        for i in todo:
            for j in adj[i]:
                dd = dist[i] + (me.vertices[i].co - me.vertices[j].co).length
                if dd < dist[j] and dd < fade:
                    dist[j], src[j] = dd, src[i]
                    nxt.append(j)
        todo = nxt
    e = [acc[i] / max(cnt[i], 1) for i in range(n)]
    for _ in range(40):
        e = [0.5 * e[i] + 0.5 * sum(e[j] for j in adj[i]) / max(len(adj[i]), 1) for i in range(n)]
    out = []
    for i in range(n):
        flare = src[i] * (1 - dist[i] / fade) ** 2 if dist[i] < fade else 0.0
        out.append((thick + e[i] + flare) * d.H)
    return out


def drape_shell(me, skin, co, normals, ease):
    """Cloth bridges hollows instead of following them: smooth the shell along the normals only
    (so hems and vertex spacing stay put), never letting it sink below skin + ease."""
    import numpy as np

    n = len(me.vertices)
    ev = np.array([e.vertices[:] for e in me.edges], dtype=np.int64).reshape(-1, 2)
    deg = np.bincount(ev.ravel(), minlength=n).astype(np.float64)
    deg[deg == 0] = 1
    S = np.array([tuple(c) for c in skin])
    N = np.array([tuple(c) for c in normals])
    lo = np.array(ease)
    off = np.maximum(np.einsum("ij,ij->i", np.array([tuple(c) for c in co]) - S, N), lo)
    for _ in range(DRAPE_STEPS):
        P = S + N * off[:, None]
        acc = np.zeros_like(P)
        np.add.at(acc, ev[:, 0], P[ev[:, 1]])
        np.add.at(acc, ev[:, 1], P[ev[:, 0]])
        mid = acc / deg[:, None]
        off = np.maximum(off + 0.5 * np.einsum("ij,ij->i", mid - P, N), lo)
    return [Vector(p) for p in S + N * off[:, None]]


# sleeves and trouser legs hang as tubes: around the limb's bone chain, cloth keeps the radius of the
# widest bulge nearby, tapering by TUBE_SLOPE per unit of length (and TUBE_TURN per unit of arc), so
# it bridges the gaps between muscles and joints instead of wrapping each one
TUBE_REGIONS = {R_SHOULDER: "arm", R_UPPER_ARM: "arm", R_FOREARM: "arm", R_THIGH: "leg", R_KNEE: "leg", R_SHIN: "leg"}
TUBE_SLOPE = 0.12
TUBE_TURN = 0.5
TUBE_ROOT = 0.03  # fraction of H over which the effect fades in from the shoulder / hip


def hang_tubes(me, region, rig, co, d):
    import numpy as np

    vreg = {}
    for p in me.polygons:
        r = TUBE_REGIONS.get(region[p.material_index])
        if r:
            for i in p.vertices:
                vreg[i] = r
    bones = rig.data.bones
    push = {}
    for sfx in ("L", "R"):
        chains = {
            "arm": [bones[f"upper_arm.{sfx}"].head_local, bones[f"forearm.{sfx}"].head_local, bones[f"forearm.{sfx}"].tail_local],
            "leg": [bones[f"thigh.{sfx}"].head_local, bones[f"shin.{sfx}"].head_local, bones[f"shin.{sfx}"].tail_local],
        }
        for limb, pts in chains.items():
            idx = [i for i, r in vreg.items() if r == limb and (co[i].x > 0) == (sfx == "L")]
            if not idx:
                continue
            P = np.array([tuple(co[i]) for i in idx])
            A = [np.array(tuple(p)) for p in pts]
            # closest point on the two-segment chain, arc-length parameter and a frame per vertex
            best = np.full(len(idx), np.inf)
            T = np.zeros(len(idx))
            C = np.zeros_like(P)
            D = np.zeros_like(P)
            base = 0.0
            for a, b in zip(A, A[1:]):
                ab = b - a
                L = np.linalg.norm(ab)
                u = np.clip((P - a) @ ab / (L * L), 0, 1)
                q = a + u[:, None] * ab
                dd = np.linalg.norm(P - q, axis=1)
                m = dd < best
                best[m], T[m], C[m], D[m] = dd[m], base + u[m] * L, q[m], ab / L
                base += L
            R = P - C
            # radial part only (clamped ends of the chain would otherwise tilt the tube)
            R -= D * np.einsum("ij,ij->i", R, D)[:, None]
            r = np.linalg.norm(R, axis=1)
            ref = np.array((0.0, -1.0, 0.0))
            e1 = ref - D * (D @ ref)[:, None]
            e1 /= np.linalg.norm(e1, axis=1)[:, None]
            e2 = np.cross(D, e1)
            th = np.arctan2(np.einsum("ij,ij->i", R, e2), np.einsum("ij,ij->i", R, e1))
            rb = r.mean()
            env = r.copy()
            tau = 0.0015 * d.H  # a soft maximum, so the envelope has no creases where its peak switches
            for s in range(0, len(idx), 1024):
                dt = np.abs(T[s : s + 1024, None] - T[None, :])
                da = np.abs(th[s : s + 1024, None] - th[None, :])
                da = np.minimum(da, 2 * np.pi - da)
                x = (r[None, :] - TUBE_SLOPE * dt - TUBE_TURN * rb * da) / tau
                m = x.max(axis=1)
                env[s : s + 1024] = tau * (m + np.log(np.exp(x - m[:, None]).sum(axis=1)))
            w = np.clip(T / (TUBE_ROOT * d.H), 0, 1)
            w = w * w * (3 - 2 * w)
            nr = r + w * (np.maximum(env, r) - r)
            dirs = R / np.maximum(r, 1e-9)[:, None]
            for k, i in enumerate(idx):
                push[i] = (Vector(dirs[k]), float(nr[k] - r[k]))
    out = [c.copy() for c in co]
    for i, (dv, a) in push.items():
        out[i] = co[i] + dv * a
    return out


def turn_hems(me, normals, off):
    """Folds every open edge of a garment shell back into the body (a turned hem), so no gap
    between cloth and skin shows along necklines, sleeves and hems. `off` is each vertex's
    stand-off from the skin."""
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    rim = [e for e in bm.edges if e.is_boundary]
    if not rim:
        bm.free()
        return
    # each new vertex sits on its source: match them by position
    old = {tuple(round(c, 7) for c in v.co): normals[v.index] * off[v.index] for e in rim for v in e.verts}
    res = bmesh.ops.extrude_edge_only(bm, edges=rim)
    new = [g for g in res["geom"] if isinstance(g, bmesh.types.BMVert)]
    layers = list(bm.verts.layers.shape.values())
    for v in new:
        n = old.get(tuple(round(c, 7) for c in v.co))
        if n is None:
            continue
        push = -n * 1.0
        v.co += push
        for L in layers:
            v[L] = v[L] + push
    bm.to_mesh(me)
    bm.free()


def transfer_keys(obj, src, k=4):
    """Gives obj shape keys named like src's, each vertex moving like its nearest src vertices."""
    sm = src.data
    tree = KDTree(len(sm.vertices))
    for v in sm.vertices:
        tree.insert(v.co, v.index)
    tree.balance()
    near = []
    for v in obj.data.vertices:
        hits = tree.find_n(v.co, k)
        ws = [(i, 1.0 / max(dd, 1e-4)) for _, i, dd in hits]
        tot = sum(w for _, w in ws)
        near.append([(i, w / tot) for i, w in ws])
    if obj.data.shape_keys is None:
        obj.shape_key_add(name="Basis")
    basis = sm.shape_keys.key_blocks[0]
    for kb in sm.shape_keys.key_blocks[1:]:
        sk = obj.shape_key_add(name=kb.name, from_mix=False)
        for v, nb in zip(obj.data.vertices, near):
            delta = sum(((kb.data[i].co - basis.data[i].co) * w for i, w in nb), Vector())
            sk.data[v.index].co = v.co + delta


def skirt(body, name, mat, hems, d, hem_frac, rig):
    """A flared tube from the belt to the hem, skinned to the hips and (lower down) the thighs."""
    segs, rings = 40, 10
    H = d.H
    top_z = hems.belt + d.torso * 0.02
    bot_z = d.legs - (d.legs - d.knee_z) * hem_frac * 2.0
    # sample the body's girth at the belt to fit the waistband
    me = body.data
    verts, faces = [], []
    for r in range(rings + 1):
        t = r / rings
        z = top_z + (bot_z - top_z) * t
        for k in range(segs):
            a = 2 * math.pi * k / segs
            rx = d.width * (0.37 + 0.09 * t + 0.03 * t * t) + 0.004 * H
            ry = d.depth * (0.52 + 0.25 * t) + 0.004 * H
            verts.append(Vector((math.cos(a) * rx, math.sin(a) * ry, z)))
    for r in range(rings):
        for k in range(segs):
            i0 = r * segs + k
            i1 = r * segs + (k + 1) % segs
            faces.append((i0, i0 + segs, i1 + segs, i1))
    m = bpy.data.meshes.new(name)
    m.from_pydata([tuple(v) for v in verts], [], faces)
    m.materials.append(bpy.data.materials[mat])
    for p in m.polygons:
        p.use_smooth = True
    obj = link(bpy.data.objects.new(name, m))
    select_only(obj)
    sol = obj.modifiers.new("solid", "SOLIDIFY")
    sol.thickness = 0.004 * H
    sol.offset = -1
    bpy.ops.object.modifier_apply(modifier="solid")
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)  # the game culls back faces
    bm.to_mesh(obj.data)
    bm.free()
    # skinning: hips at the waistband, blending to the nearer thigh towards the hem
    for n in ("hips", "thigh.L", "thigh.R"):
        obj.vertex_groups.new(name=n)
    for v in obj.data.vertices:
        t = max(0.0, min(1.0, (top_z - v.co.z) / max(top_z - bot_z, 1e-6)))
        lean = max(0.0, min(1.0, 0.5 + v.co.x / (d.width * 0.6)))
        leg = 0.55 * t * t
        obj.vertex_groups["hips"].add([v.index], 1 - leg, "REPLACE")
        obj.vertex_groups["thigh.L"].add([v.index], leg * lean, "REPLACE")
        obj.vertex_groups["thigh.R"].add([v.index], leg * (1 - lean), "REPLACE")
    obj.parent = rig
    mod = obj.modifiers.new("rig", "ARMATURE")
    mod.object = rig
    return obj


def make_clothes(body, rig, d, hems, sources=None):
    """Every garment; `sources` maps a garment to another mesh to build it from (the shoes come
    from the smooth foot the detailed one replaced), skinned and keyed like the body."""
    out = []
    for name, (mat, regions, thick) in GARMENTS.items():
        g = garment((sources or {}).get(name, body), name, mat, regions, thick, d, drape=name != "shoes", rig=rig if name != "shoes" else None)
        if name in SKIRTS:
            s = skirt(body, name + "_skirt", mat, hems, d, SKIRTS[name], rig)
            transfer_keys(s, body)
            select_only(g, s)
            bpy.context.view_layer.objects.active = g
            bpy.ops.object.join()
        out.append(g)
    return out
