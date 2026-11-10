"""Detailed hands and feet for the peon body, and the skin tone/thinness vertex colours (see
make_peons.py).

The body's metaball is voxel-remeshed at ~1.8 cm (adult), far too coarse for fingers or toes, so
the hands and feet are their own metaballs remeshed several times finer and welded on: the body
welded on a few cm up the forearm and shin once the body is skinned, keyed and dressed (garments
come from the smooth body, so none straddles a seam). The body's metaball carries smooth stub
hands and feet and each piece the same forearm/shin elements, so their cross-sections agree at
the weld: both are cut there, the piece's open loop is fitted to the body's and the two are
bridged; the piece takes its weights and shape-key offsets from the smooth body it replaced. The hand is one bone, so the fingers are posed
in a relaxed curl. Shoes are made from a separate smooth foot (`shoe_last`), so they show no toes.

Skin colour variation lives in two colour attributes, exported as COLOR_0/COLOR_1 (the game reads
them for skin, see hearth_render::SkinnedVertex): `tone` (rgb multiplies the tint after the pale amount mixes it
towards a pale tone: palms, soles, nails; redder knuckles, knees, elbows) and `thin` (red: how thin
the flesh is, for light shining through fingers and ears; green: the pale mix, as glTF colours lose
their alpha on export).
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Quaternion, Vector
from mathutils.kdtree import KDTree

from peon_body import R_FOOT, Meta, link, select_only


def frame_quat(x, y, z):
    """Rotation taking local x, y, z to the given (orthonormal) axes; flips x for a right-handed frame."""
    if x.cross(y).dot(z) < 0:
        x = -x
    return Matrix((x, y, z)).transposed().to_quaternion()


def rot_towards(v, to, deg):
    """v turned by deg towards `to` (in their common plane)."""
    ax = v.cross(to)
    if ax.length < 1e-8:
        return v.copy()
    return Quaternion(ax.normalized(), math.radians(deg)) @ v


# ------------------------------------------------------------------------------------------------
# hand


def hand_frame(d, wr, tip, side):
    """Unit axes of a hand hanging in the A-pose: `a` down the fingers, `f` forward (the thumb's
    side), `pn` out of the palm (towards the thigh)."""
    a = (tip - wr).normalized()
    f = Vector((0, -1, 0))
    f = (f - a * f.dot(a)).normalized()
    pn = a.cross(f)
    if pn.x * side > 0:
        pn = -pn
    return a, f, pn


def hand_metaball(d, arm_points, side, spots, m=None):
    """The hand plus the forearm elements that reach the wrist; fills `spots` with nail and knuckle
    centres for the tone paint. Given the body's metaball `m`, adds just the stub of the hand near
    the wrist (wrist, palm, thumb base), so the body's surface at the wrist is the piece's."""
    H, ar = d.H, d.arm_r
    sh, el, wr, tip = arm_points(d, side)
    a, f, pn = hand_frame(d, wr, tip, side)
    stub = m is not None
    if not stub:
        m = Meta(f"hand_mb.{side}")
        m.mb.resolution = H * 0.0016
        m.mb.render_resolution = H * 0.0016
        fa = (wr - el).normalized()
        # forearm, exactly as the body has it
        m.cap(el, wr, ar * 0.68, (0.3, 0))
        m.ell(el.lerp(wr, 0.3), ar * 0.92, ar * 0.85, (wr - el).length * 0.32, Vector((0, 0, 1)).rotation_difference(fa))
    L = (tip - wr).length
    P = L * 0.47  # palm length
    W = H * 0.043  # knuckle width
    T = H * 0.0145  # palm thickness
    q = frame_quat(pn, f, a)
    # wrist: flattening the round forearm into the palm
    m.ell(wr + a * H * 0.004, T * 0.62, W * 0.42, H * 0.012, q)
    # palm, a little thicker at the heel and the thenar (thumb) pad, cupped by the hypothenar
    m.ell(wr + a * P * 0.52, T * 0.5, W * 0.5, P * 0.5, q)
    m.ell(wr + a * P * 0.34 + f * W * 0.24 + pn * T * 0.22, T * 0.42, W * 0.24, P * 0.32, q)  # thenar
    m.ell(wr + a * P * 0.42 - f * W * 0.3 + pn * T * 0.12, T * 0.36, W * 0.17, P * 0.36, q)  # hypothenar
    # thumb base, from the heel of the hand into the thenar
    base = wr + a * P * 0.12 + f * W * 0.36 + pn * T * 0.1
    tdir = rot_towards(rot_towards(a, f, 24), pn, 30)
    mc = base + tdir * H * 0.03
    m.cap(base, mc, H * 0.0085, (0, 0.3))
    if stub:
        return m
    # fingers: index (forward) to little; knuckle arch, spread, three phalanges curling to the palm
    lf = L - P
    fingers = [  # across, knuckle along, length, radius, spread, curl per joint
        (0.36, 0.97, 0.9, 0.0057, 5.0, (8, 14, 10)),
        (0.12, 1.0, 1.0, 0.006, 1.0, (10, 16, 11)),
        (-0.12, 0.97, 0.93, 0.0056, -3.0, (12, 18, 12)),
        (-0.35, 0.89, 0.74, 0.0049, -8.0, (15, 20, 13)),
    ]
    for across, along, length, r, spread, curl in fingers:
        r *= H
        k = wr + a * P * along + f * W * across
        dirv = rot_towards(a, f, spread)
        segs = (0.45, 0.31, 0.24)
        p0 = k - dirv * r * 0.6
        rads = (r, r * 0.9, r * 0.8)
        for i, (sl, bend) in enumerate(zip(segs, curl)):
            dirv = rot_towards(dirv, pn, bend)
            p1 = p0 + dirv * lf * length * sl
            m.cap(p0, p1, rads[i], (0, 0.25 if i < 2 else 0))
            if i == 2:
                spots["nail"].append((p0.lerp(p1, 0.62) - pn * rads[i] * 0.7, -pn, rads[i] * 1.15))
            p0 = p1
        m.ell(k - pn * r * 0.25, r * 0.85, r * 1.05, r * 1.1, q)  # knuckle
        spots["knuckle"].append((k - pn * r * 0.9, -pn, r * 1.3))
    # thumb: two phalanges on from the base, forward and across the palm
    tr = H * 0.0068
    p0 = mc
    for i, (sl, bend) in enumerate(((0.019, 10), (0.017, 12))):
        tdir = rot_towards(tdir, pn, bend)
        p1 = p0 + tdir * H * sl
        rr = tr * (1.0 if i == 0 else 0.9)
        m.cap(p0, p1, rr, (0, 0.25 if i == 0 else 0))
        if i == 1:
            # the thumbnail faces outwards, away from the palm and the fingers
            out = (-pn * 0.4 + f * 0.9).normalized()
            out = (out - tdir * out.dot(tdir)).normalized()
            spots["nail"].append((p0.lerp(p1, 0.6) + out * rr * 0.7, out, rr * 0.95))
        p0 = p1
    spots["palm"].append((wr + a * P * 0.5 + pn * T * 0.5, pn, W * 0.6, a, lf * 1.05 + P * 0.6))
    return m


# ------------------------------------------------------------------------------------------------
# foot


def foot_metaball(d, leg_points, side, spots, m=None):
    """The foot plus the shin/ankle elements around the shoe hem. Given the body's metaball `m`,
    adds the foot without its toes (the body's surface at the shoe hem is then the piece's)."""
    H, tr = d.H, d.thigh_r
    hip, knee, ankle, toe = leg_points(d, side)
    stub = m is not None
    if not stub:
        m = Meta(f"foot_mb.{side}")
        m.mb.resolution = H * 0.0018
        m.mb.render_resolution = H * 0.0018
        # shin and calf, as on the body
        m.cap(knee, ankle, tr * 0.5, (0.4, 0))
        m.ell(knee.lerp(ankle, 0.3) + Vector((side * tr * 0.03, tr * 0.14, 0)), tr * 0.64, tr * 0.64, (knee - ankle).length * 0.27)
    med = Vector((-side, 0, 0))  # towards the other foot (the big toe's side)
    fwd = Vector((0, -1, 0))
    up = Vector((0, 0, 1))
    q = frame_quat(med, fwd, up)
    az = d.ankle_z
    heel = Vector((ankle.x, ankle.y + tr * 0.32, az * 0.5))
    tip_y = toe.y - H * 0.004
    ball = Vector((ankle.x + side * tr * 0.02, tip_y + H * 0.04, az * 0.3))
    # ankle bones, heel, the arch (high on the inside), ball of the foot
    m.cap(ankle + up * az * 0.25, Vector((ankle.x, ankle.y - tr * 0.05, az * 0.55)), tr * 0.42)
    for s, dz in ((1, 0.15), (-1, -0.05)):  # the inner ankle bone sits higher
        m.ell(ankle + med * s * tr * 0.33 + up * az * dz, tr * 0.14, tr * 0.16, tr * 0.18, q)
    m.ell(heel, tr * 0.33, tr * 0.38, az * 0.5, q)
    mid = heel.lerp(ball, 0.5)
    m.ell(mid - med * tr * 0.06 + up * az * 0.06, tr * 0.36, (ball - heel).length * 0.42, az * 0.42, q)
    m.ell(mid - med * tr * 0.22 - up * az * 0.12, tr * 0.16, (ball - heel).length * 0.3, az * 0.22, q)  # outer edge touches down
    m.ell(ball + up * az * 0.06 - fwd * H * 0.006, tr * 0.6, tr * 0.42, az * 0.4, q)
    m.cap(ankle + up * az * 0.1, ball + up * az * 0.2, tr * 0.32)  # instep
    if stub:
        return m
    # toes, big (inside) to little: short, plump, side by side, each a little further back
    toes = [  # across (+ inside, in foot widths), back from the tip, length, radius
        (0.34, 0.0, 0.021, 0.0074),
        (0.12, 0.003, 0.016, 0.0053),
        (-0.06, 0.007, 0.015, 0.005),
        (-0.22, 0.012, 0.013, 0.0047),
        (-0.37, 0.019, 0.011, 0.0043),
    ]
    width = H * 0.056
    for across, back, length, r in toes:
        r *= H
        end = Vector((ankle.x - side * width * across, tip_y + H * back + r, r * 0.95))
        start = end - fwd * H * length + up * r * 1.0
        m.cap(start, end, r, (0, 0))
        m.ell(end - fwd * r * 0.1, r * 1.0, r * 0.95, r * 0.9, q)  # pad
        spots["nail"].append((end - fwd * r * 0.25 + up * r * 0.85, up, r * 1.05))
    spots["sole"].append(heel.lerp(ball, 0.5))
    return m


# ------------------------------------------------------------------------------------------------
# welding


def remesh_metaball(m, voxel):
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(m.obj.evaluated_get(dg))
    bpy.data.objects.remove(m.obj)
    obj = link(bpy.data.objects.new(m.mb.name + "_mesh", me))
    select_only(obj)
    rm = obj.modifiers.new("remesh", "REMESH")
    rm.mode = "VOXEL"
    rm.voxel_size = voxel
    bpy.ops.object.modifier_apply(modifier="remesh")
    sm = obj.modifiers.new("smooth", "CORRECTIVE_SMOOTH")
    sm.iterations = 3
    sm.use_only_smooth = True
    sm.smooth_type = "LENGTH_WEIGHTED"
    bpy.ops.object.modifier_apply(modifier="smooth")
    return obj


def snap_loop(moving, target, co, no, band=(), width=1.0):
    """Scales the `moving` loop radially (in the cut plane, about the target loop's centre) onto
    the `target` loop's polygon, so the bridge between them has no step; `band` vertices (the
    piece's, past the plane) get the same radial scale fading out over `width`."""
    tv = list({v for e in target for v in e.verts})
    centre = sum((v.co for v in tv), Vector()) / len(tv)

    def radial(p):
        r = p - centre
        return r - no * r.dot(no)

    def lookup(dirs, dn):
        best = sorted(dirs, key=lambda t: -t[0].dot(dn))[:2]
        w = [max(t[0].dot(dn), 0.0) ** 8 + 1e-9 for t in best]
        return sum(t[1] * wi for t, wi in zip(best, w)) / sum(w)

    tdirs = [(radial(v.co).normalized(), radial(v.co).length) for v in tv]
    mv = list({v for e in moving for v in e.verts})
    scales = [(radial(v.co).normalized(), lookup(tdirs, radial(v.co).normalized()) / max(radial(v.co).length, 1e-6)) for v in mv]
    for v in list(band) + mv:
        r = radial(v.co)
        if r.length < 1e-9:
            continue
        t = 1.0 - min(1.0, max(0.0, (v.co - co).dot(no)) / width)
        k = 1.0 + (lookup(scales, r.normalized()) - 1.0) * t * t * (3 - 2 * t)
        v.co = centre + r * k + no * (v.co - centre).dot(no)


def keep_largest(bm):
    """Deletes every connected part but the largest."""
    seen, parts = set(), []
    for f in bm.faces:
        if f in seen:
            continue
        stack, comp = [f], []
        seen.add(f)
        while stack:
            g = stack.pop()
            comp.append(g)
            for e in g.edges:
                for h in e.link_faces:
                    if h not in seen:
                        seen.add(h)
                        stack.append(h)
        parts.append(comp)
    parts.sort(key=len, reverse=True)
    rest = [f for comp in parts[1:] for f in comp]
    if rest:
        bmesh.ops.delete(bm, geom=rest, context="FACES")


def weld_planes(d, arm_points, leg_points):
    """Where each piece joins the body: a few cm up the forearm and shin, where both are smooth and
    made of the same metaball elements, so their cross-sections agree (the coarse body can't follow
    the wrist and ankle closely enough to weld there). [(point, normal towards the piece)] in
    add_extremities' order."""
    out = []
    for side in (1, -1):
        sh, el, wr, tip = arm_points(d, side)
        fa = (wr - el).normalized()
        out.append((wr - fa * d.H * 0.035, fa))
        out.append((Vector((0, 0, d.ankle_z + d.H * 0.065)), Vector((0, 0, -1))))
    return out


def add_stubs(m, d, arm_points, leg_points):
    """Adds the hand and foot stubs to the body's metaball `m`: smooth hands and feet whose fields
    match the pieces' around the weld planes."""
    for side in (1, -1):
        hand_metaball(d, arm_points, side, None, m)
        foot_metaball(d, leg_points, side, None, m)


def add_extremities(body, rig, d, hems, arm_points, leg_points):
    """Swaps the body's smooth stub hands and feet for detailed ones, after it's been skinned and
    keyed and the clothes made from it (so no garment straddles a seam). Each piece is cut at its
    weld plane (and at the hem it crosses), the body likewise, the piece's open loop is fitted to
    the body's (blending over a band) and the two are bridged; the piece's faces get their skin
    regions, and its vertices the weights and shape-key offsets of the body they replaced.
    Returns the paint spots (nails, knuckles, palms, soles) and `seam_src` for smooth_seams."""
    from peon_body import bone_segments, region_of

    spots = {"nail": [], "knuckle": [], "palm": [], "sole": []}
    pieces = []
    for side, sfx in ((1, "L"), (-1, "R")):
        hand = remesh_metaball(hand_metaball(d, arm_points, side, spots), d.limb * 0.035)
        pieces.append((hand, side, False, hems.arms[sfx]["wrist"]))
        foot = remesh_metaball(foot_metaball(d, leg_points, side, spots), d.limb * 0.038)
        pieces.append((foot, side, True, (Vector((0, 0, hems.shoe)), Vector((0, 0, 1)))))
    # the smooth body, as the source of weights and shape keys for the pieces
    pre = link(bpy.data.objects.new("pre_weld", body.data.copy()))
    for g in body.vertex_groups:
        pre.vertex_groups.new(name=g.name)
    for v in body.data.vertices:
        for ge in v.groups:
            pre.vertex_groups[ge.group].add([v.index], ge.weight, "REPLACE")
    segs = bone_segments(rig)
    tmp = bpy.data.materials.get("piece_tmp") or bpy.data.materials.new("piece_tmp")
    sources = []
    for (piece, side, is_foot, hem), (co, no) in zip(pieces, weld_planes(d, arm_points, leg_points)):
        # an uncut copy: its surface runs smoothly through the seam, so it lends the seam its normals
        sources.append(link(bpy.data.objects.new("seam_src", piece.data.copy())))
        if is_foot:
            pick = lambda p, side=side: abs(p.x - side * d.hip_x) < d.hip_x * 0.9 and p.z < d.knee_z
        else:
            pick = lambda p, c=co: (p - c).length < d.H * 0.16
        # the piece: keep past the plane (its main island), cut along the hem
        bm = bmesh.new()
        bm.from_mesh(piece.data)
        bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=co, plane_no=no, clear_inner=True)
        keep_largest(bm)
        bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=hem[0], plane_no=hem[1])
        bm.to_mesh(piece.data)
        bm.free()
        # the body: cut at the plane, drop what's past it
        me = body.data
        bm = bmesh.new()
        bm.from_mesh(me)
        near = [f for f in bm.faces if pick(f.calc_center_median())]
        geom = list({e for f in near for e in f.edges}) + near + list({v for f in near for v in f.verts})
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=co, plane_no=no)
        gone = [f for f in bm.faces if pick(f.calc_center_median()) and (f.calc_center_median() - co).dot(no) > 0]
        bmesh.ops.delete(bm, geom=gone, context="FACES")
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
        bm.to_mesh(me)
        bm.free()
        # join, the piece's faces marked with a temporary material
        piece.data.materials.clear()
        piece.data.materials.append(tmp)
        select_only(body, piece)
        bpy.context.view_layer.objects.active = body
        bpy.ops.object.join()
        me = body.data
        names = [m.name for m in me.materials]
        ti = names.index("piece_tmp")
        bm = bmesh.new()
        bm.from_mesh(me)
        loops = [e for e in bm.edges if e.is_boundary and all(abs((v.co - co).dot(no)) < d.H * 0.002 and pick(v.co) for v in e.verts)]
        piece_loop = [e for e in loops if e.link_faces[0].material_index == ti]
        body_loop = [e for e in loops if e.link_faces[0].material_index != ti]
        assert piece_loop and body_loop, f"weld: no seam loops ({len(piece_loop)}, {len(body_loop)})"
        loop_verts = {v for e in piece_loop for v in e.verts}
        band = [v for v in bm.verts if v not in loop_verts and v.link_faces and v.link_faces[0].material_index == ti and 0 < (v.co - co).dot(no) < d.H * 0.02]
        snap_loop(piece_loop, body_loop, co, no, band, d.H * 0.02)
        res = bmesh.ops.bridge_loops(bm, edges=body_loop + piece_loop)
        for f in list(res["faces"]) + [f for f in bm.faces if f.material_index == ti]:
            f.material_index = names.index(f"skin.r{region_of(f.calc_center_median(), segs, hems, f.normal)}")
            f.smooth = True
        bm.to_mesh(me)
        bm.free()
        me.materials.pop(index=me.materials.find("piece_tmp"))
    rebind(body, pre)
    bpy.data.objects.remove(pre)
    select_only(*sources)
    bpy.ops.object.join()
    spots["seam_src"] = bpy.context.view_layer.objects.active
    return spots


def rebind(body, pre):
    """Gives body vertices that weren't on the smooth body `pre` (the pieces, the seam) the weights
    and shape-key offsets of the nearest point of `pre`'s surface (interpolated across its face, so
    they vary smoothly)."""
    from mathutils.bvhtree import BVHTree
    from mathutils.interpolate import poly_3d_calc

    pm = pre.data
    tree = BVHTree.FromPolygons([v.co for v in pm.vertices], [p.vertices[:] for p in pm.polygons])
    kd = KDTree(len(pm.vertices))
    for v in pm.vertices:
        kd.insert(v.co, v.index)
    kd.balance()
    names = {g.index: g.name for g in pre.vertex_groups}
    keys = body.data.shape_keys.key_blocks if body.data.shape_keys else []
    pkeys = pm.shape_keys.key_blocks if pm.shape_keys else []
    for v in body.data.vertices:
        if kd.find(v.co)[2] < 1e-7:
            continue  # an original body vertex
        loc, _, fi, _ = tree.find_nearest(v.co)
        ids = pm.polygons[fi].vertices[:]
        ws = poly_3d_calc([pm.vertices[i].co for i in ids], loc)
        acc = {}
        delta = [Vector() for _ in keys]
        for i, w in zip(ids, ws):
            for ge in pm.vertices[i].groups:
                acc[names[ge.group]] = acc.get(names[ge.group], 0.0) + ge.weight * w
            for j in range(1, len(pkeys)):
                delta[j] += (pkeys[j].data[i].co - pkeys[0].data[i].co) * w
        for g in list(v.groups):
            body.vertex_groups[g.group].remove([v.index])
        for n, w in acc.items():
            if w > 1e-3:
                body.vertex_groups[n].add([v.index], w, "REPLACE")
        if keys:
            keys[0].data[v.index].co = v.co
            for j in range(1, len(keys)):
                keys[j].data[v.index].co = v.co + delta[j]


def smooth_seams(objs, src, d, arm_points, leg_points):
    """Bridging the body's coarse loop to a piece's fine one leaves little kinks in the shading;
    takes the normals around each seam from the uncut piece (`src`) instead, fading out over a
    couple of cm. Done last (custom normals don't survive later mesh edits)."""
    planes = weld_planes(d, arm_points, leg_points)
    for obj in objs:
        vg = obj.vertex_groups.new(name="seam")
        any_w = False
        for v in obj.data.vertices:
            w = 0.0
            for co, no in planes:
                dist = abs((v.co - co).dot(no))
                radial = (v.co - co) - no * (v.co - co).dot(no)
                if radial.length < d.H * 0.06:
                    w = max(w, smooth01((d.H * 0.03 - dist) / (d.H * 0.015)))
            if w > 0:
                vg.add([v.index], w, "REPLACE")
                any_w = True
        if any_w:
            select_only(obj)
            mod = obj.modifiers.new("seam", "DATA_TRANSFER")
            mod.object = src
            mod.use_loop_data = True
            mod.data_types_loops = {"CUSTOM_NORMAL"}
            mod.loop_mapping = "POLYINTERP_NEAREST"
            mod.vertex_group = "seam"
            mod.mix_mode = "REPLACE"
            # the armature modifier comes first in the stack; transfer onto the rest shape
            bpy.ops.object.modifier_move_to_index(modifier="seam", index=0)
            bpy.ops.object.modifier_apply(modifier="seam")
        obj.vertex_groups.remove(obj.vertex_groups["seam"])
    bpy.data.objects.remove(src)


def shoe_last(d, leg_points, hems):
    """The smooth foot shoes are made from (so they don't show toes), cut at the shoe hem, faces
    tagged with the foot region's material."""
    H, tr = d.H, d.thigh_r
    m = Meta("shoe_last_mb")
    m.mb.resolution = H * 0.006
    m.mb.render_resolution = H * 0.006
    for side in (1, -1):
        hip, knee, ankle, toe = leg_points(d, side)
        m.cap(knee, ankle, tr * 0.5, (0.4, 0))
        # roomy enough for the detailed foot: heel, toes and sole all inside
        heel = Vector((ankle.x, ankle.y + tr * 0.5, d.ankle_z * 0.5))
        toe_p = Vector((toe.x - side * tr * 0.08, toe.y - H * 0.012, d.ankle_z * 0.42))
        m.ell(heel.lerp(toe_p, 0.5), tr * 0.62, (toe_p - heel).length * 0.55, d.ankle_z * 0.68,
              Vector((0, 1, 0)).rotation_difference((toe_p - heel).normalized()))
        m.cap(ankle, heel.lerp(toe_p, 0.35), tr * 0.5)
    obj = remesh_metaball(m, H * 0.0085)
    obj.name = obj.data.name = "shoe_last"
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=(0, 0, hems.shoe), plane_no=(0, 0, 1), clear_outer=True)
    for v in bm.verts:
        if v.co.z < H * 0.004:
            v.co.z = H * 0.002 + max(0.0, v.co.z) * 0.5
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.materials.append(bpy.data.materials[f"skin.r{R_FOOT}"])
    for p in obj.data.polygons:
        p.use_smooth = True
    return obj


# ------------------------------------------------------------------------------------------------
# skin paint


def smooth01(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def blob(p, c, r):
    """1 at c falling to 0 at distance r."""
    return smooth01(1.0 - (p - c).length / r)


def set_skin_colors(obj, tone_of, thin_of):
    """Writes the `tone` (COLOR_0: rgb multiplier) and `thin` (COLOR_1: red thinness, green the
    pale mix; the exporter drops alpha) point colour attributes."""
    me = obj.data
    for name in ("tone", "thin"):
        if name in me.color_attributes:
            me.color_attributes.remove(me.color_attributes[name])
    tone = me.color_attributes.new("tone", "FLOAT_COLOR", "POINT")
    thin = me.color_attributes.new("thin", "FLOAT_COLOR", "POINT")
    for v in me.vertices:
        r, g, b, pale = tone_of(v.index, v.co, v.normal)
        tone.data[v.index].color = (r, g, b, 1.0)
        t = thin_of(v.index, v.co, v.normal)
        thin.data[v.index].color = (t, pale, 0.0, 1.0)
    me.color_attributes.active_color = tone
    me.color_attributes.render_color_index = 0


def paint_body(body, d, spots, arm_points, leg_points):
    """Pale palms, soles and nails; redder knuckles, elbows, knees, fingertips; thin fingers."""
    joints = []
    hands, feet = [], []
    for side in (1, -1):
        sh, el, wr, tip = arm_points(d, side)
        joints.append((el + Vector((0, d.arm_r * 0.6, 0)), d.arm_r * 1.1, 0.6))  # elbow (back)
        hands.append((wr, tip))
        hip, knee, ankle, toe = leg_points(d, side)
        joints.append((knee + Vector((0, -d.thigh_r * 0.6, 0)), d.thigh_r * 1.0, 0.5))  # kneecap
        feet.append(ankle)
    # how much each vertex is hand / foot (by its skinning), so palm and sole paint can't spill
    # onto a thigh the hand hangs next to
    names = {g.index: g.name for g in body.vertex_groups}
    handw, footw = [], []
    for v in body.data.vertices:
        ws = {names[g.group].split(".")[0]: g.weight for g in v.groups}
        handw.append(ws.get("hand", 0.0))
        footw.append(ws.get("foot", 0.0))

    def tone_of(i, p, n):
        r, g, b, pale = 1.0, 1.0, 1.0, 0.0
        red = 0.0
        for c, rad, k in joints:
            red += k * blob(p, c, rad)
        for c, out, rad in spots["knuckle"]:
            red += 0.7 * blob(p, c, rad) * smooth01(n.dot(out) * 2)
        # hands and feet run a little warmer overall, fingertips more
        for wr, tip in hands:
            red += 0.25 * smooth01(1.0 - (p - wr).length / ((tip - wr).length * 1.3))
            red += 0.35 * blob(p, tip, d.H * 0.025)
        for c, pn, rad, a, length in spots["palm"]:
            # the palm and the palm side of the fingers
            along = (p - (c - a * length * 0.45)).dot(a)
            off = (p - c) - a * (p - c).dot(a)
            inside = 0.0 <= along <= length * 1.1 and off.length < rad * 1.2 and (p - c).dot(pn) > -rad * 0.35
            if inside and handw[i] > 0.3:
                fade = smooth01(along / (length * 0.2)) * smooth01((handw[i] - 0.3) / 0.4)
                pale = max(pale, 0.4 * fade * smooth01(n.dot(pn) * 2.5 - 0.2))
        for c in spots["sole"]:
            if abs(p.x - c.x) < d.thigh_r * 0.8 and p.z < d.ankle_z * 0.25 and footw[i] > 0.3:
                pale = max(pale, 0.35 * smooth01((d.ankle_z * 0.25 - p.z) / (d.ankle_z * 0.15)) * smooth01(-n.z * 1.5))
        for c, out, rad in spots["nail"]:
            if handw[i] + footw[i] < 0.3:
                break
            k = blob(p, c, rad) * smooth01(n.dot(out) * 2.5)
            if k > 0:
                pale = max(pale, 0.3 * smooth01(k * 2.0))
                red -= 0.5 * smooth01(k * 2.5)
        red = max(0.0, min(1.0, red))
        return (r - 0.02 * red, g - 0.1 * red, b - 0.1 * red, pale)

    def thin_of(_i, p, n):
        t = 0.1
        for wr, tip in hands:
            L = (tip - wr).length
            t = max(t, 0.85 * smooth01(((p - wr).dot((tip - wr) / L) - L * 0.45) / (L * 0.3)))
        for a in feet:
            if p.z < d.ankle_z * 0.7 and (a - p).y > d.foot_len * 0.45:
                t = max(t, 0.45)  # toes
        return t

    set_skin_colors(body, tone_of, thin_of)


def copy_weights(obj, src, rig, k=4):
    """Skins obj like its nearest src vertices (inverse-distance blend of their weights)."""
    sm = src.data
    tree = KDTree(len(sm.vertices))
    for v in sm.vertices:
        tree.insert(v.co, v.index)
    tree.balance()
    names = {g.index: g.name for g in src.vertex_groups}
    for g in src.vertex_groups:
        if g.name not in obj.vertex_groups:
            obj.vertex_groups.new(name=g.name)
    for v in obj.data.vertices:
        acc = {}
        hits = tree.find_n(v.co, k)
        tot = 0.0
        for _, i, dd in hits:
            w = 1.0 / max(dd, 1e-4)
            tot += w
            for ge in sm.vertices[i].groups:
                acc[names[ge.group]] = acc.get(names[ge.group], 0.0) + ge.weight * w
        for n, w in acc.items():
            if w / tot > 1e-3:
                obj.vertex_groups[n].add([v.index], w / tot, "REPLACE")
    obj.parent = rig
    mod = obj.modifiers.new("rig", "ARMATURE")
    mod.object = rig


def paint_head(head, hh, centre, ears):
    """Ears thin and pink, a warm nose tip and cheeks. `ears` holds the ear meshes' vertex indices;
    hh and centre are the head frame (head_frame in make_peons.py)."""
    def local(p):
        return (p - centre) / hh

    nose = Vector((0, -0.53, -0.17))
    cheeks = [Vector((sx * 0.21, -0.37, -0.2)) for sx in (1, -1)]

    def tone_of(i, p, n):
        q = local(p)
        red = 0.55 if i in ears else 0.0
        red += 0.3 * blob(q, nose, 0.12)
        red += sum(0.3 * blob(q, c, 0.14) for c in cheeks)
        red = min(red, 1.0)
        return (1.0 - 0.02 * red, 1.0 - 0.1 * red, 1.0 - 0.1 * red, 0.0)

    def thin_of(i, p, n):
        return 1.0 if i in ears else 0.1 + 0.3 * blob(local(p), nose, 0.1)

    set_skin_colors(head, tone_of, thin_of)
