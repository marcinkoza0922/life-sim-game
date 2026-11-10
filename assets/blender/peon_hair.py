"""Hair styles and facial hair for the peon generator, built as shells over the parametric head
(peon_head.py) so they fit every face: each style is a thickness field over the skull, kept
above a hairline, with long styles hanging a curtain below the ears. Pure geometry in head units,
with a topology independent of the face sliders (so they get the same shape keys as the head).
"""

import math

import bmesh
from mathutils import Vector

from peon_head import Face, head_mesh, smoothstep

# hair styles in the order of `Appearance::hair_style` (index 0 is the first); "bald" has no mesh
HAIR_STYLES = [
    ("short", "Short"),
    ("buzz", "Buzz cut"),
    ("side_part", "Side part"),
    ("bun", "Bun"),
    ("ponytail", "Ponytail"),
    ("long", "Long"),
    ("bob", "Bob"),
    ("curly", "Curly"),
    ("undercut", "Undercut"),
    ("receding", "Receding"),
    ("afro", "Afro"),
    ("bald", "Bald"),
]
FACIAL_HAIR = [
    ("none", "None"),
    ("stubble", "Stubble"),
    ("moustache", "Moustache"),
    ("goatee", "Goatee"),
    ("short_beard", "Short beard"),
    ("full_beard", "Full beard"),
]


def normals(verts, faces):
    bm = bmesh.new()
    vs = [bm.verts.new(v) for v in verts]
    for fc in faces:
        try:
            bm.faces.new([vs[i] for i in fc])
        except ValueError:
            pass
    bm.normal_update()
    out = [v.normal.copy() for v in vs]
    bm.free()
    if sum(n.dot(v) for n, v in zip(out, verts)) < 0:  # wound inwards: flip
        out = [-n for n in out]
    return out


def azimuth(q):
    """Degrees around the head from the front (0) to the back (180), either side."""
    return math.degrees(math.atan2(abs(q.x), -q.y))


def hairline(az, front=0.29, temple=0.2, ear=0.05, nape=-0.24):
    """z of the hairline at an azimuth: forehead, temples, over the ear, down to the nape."""
    if az < 45:
        return front + (temple - front) * smoothstep(0, 45, az)
    if az < 82:
        return temple + (ear - temple) * smoothstep(45, 82, az)
    if az < 115:
        # arch over the ear
        return ear + 0.13 * math.sin(math.pi * (az - 82) / 33)
    return ear + (nape - ear) * smoothstep(115, 160, az)


def shell(verts, faces, nrm, keep, thick, lift=0.004, move=None, solid=False):
    """Shell over the kept faces: the outer surface at `thick` (per-vertex function) and a wall
    down to the scalp along its open edges. `solid` adds the inner surface too (for hair that hangs
    away from the head, seen from below); otherwise the head fills the inside. `move` remaps base
    points (long hair)."""
    used = sorted({i for fi, fc in enumerate(faces) if keep[fi] for i in fc})
    remap = {old: new for new, old in enumerate(used)}
    moved = [move(verts[old], nrm[old]) if move else (verts[old], nrm[old]) for old in used]
    out_v = [base + nn * thick(verts[old], nn) for old, (base, nn) in zip(used, moved)]
    out_f = []
    edge_count = {}
    for fi, fc in enumerate(faces):
        if not keep[fi]:
            continue
        f2 = [remap[i] for i in fc]
        out_f.append(tuple(f2))
        for a, b in zip(f2, f2[1:] + f2[:1]):
            edge_count[(a, b)] = edge_count.get((a, b), 0) + 1
    open_edges = [(a, b) for (a, b) in edge_count if (b, a) not in edge_count]
    # inner copies: every vertex when solid, else just the rim
    inner = {}
    for i in range(len(used)) if solid else sorted({v for e in open_edges for v in e}):
        base, nn = moved[i]
        inner[i] = len(out_v)
        out_v.append(base + nn * lift)
    if solid:
        for fi, fc in enumerate(faces):
            if keep[fi]:
                out_f.append(tuple(reversed([inner[remap[i]] for i in fc])))
    for a, b in open_edges:
        out_f.append((b, a, inner[a], inner[b]))
    return out_v, out_f


def clip(verts, nrm, faces, field):
    """The part of a mesh where `field` (per vertex) is >= 0, with faces cut along its zero line.
    Returns (verts, normals, faces); cut points are shared between neighbouring faces."""
    field = [v if v != 0 else 1e-7 for v in field]
    vs, ns, cache, out = list(verts), list(nrm), {}, []

    def cut(a, b):
        key = (min(a, b), max(a, b))
        if key not in cache:
            t = field[a] / (field[a] - field[b])
            vs.append(verts[a].lerp(verts[b], t))
            ns.append(nrm[a].lerp(nrm[b], t).normalized())
            cache[key] = len(vs) - 1
        return cache[key]

    for fc in faces:
        poly = []
        for a, b in zip(fc, fc[1:] + fc[:1]):
            if field[a] > 0:
                poly.append(a)
            if (field[a] > 0) != (field[b] > 0):
                poly.append(cut(a, b))
        if len(poly) >= 3:
            out.append(tuple(poly))
    return vs, ns, out


def ellipsoid(center, radii, segs=16, rings=10):
    verts, faces = [], []
    for r in range(1, rings):
        th = math.pi * r / rings
        for k in range(segs):
            a = 2 * math.pi * k / segs
            verts.append(center + Vector((math.sin(th) * math.cos(a) * radii[0], math.sin(th) * math.sin(a) * radii[1], math.cos(th) * radii[2])))
    top, bot = len(verts), len(verts) + 1
    verts += [center + Vector((0, 0, radii[2])), center - Vector((0, 0, radii[2]))]
    for r in range(rings - 2):
        for k in range(segs):
            i0, i1 = r * segs + k, r * segs + (k + 1) % segs
            faces.append((i0, i1, i1 + segs, i0 + segs))
    for k in range(segs):
        faces.append((top, (k + 1) % segs, k))
        last = (rings - 2) * segs
        faces.append((bot, last + k, last + (k + 1) % segs))
    return verts, faces


def tube(points, radii, segs=12):
    """A closed tube along points (list of Vector) with radii; capped ends."""
    verts, faces = [], []
    for i, (pt, r) in enumerate(zip(points, radii)):
        nxt = points[min(i + 1, len(points) - 1)] - points[max(i - 1, 0)]
        t = nxt.normalized()
        a = t.cross(Vector((1, 0, 0))).normalized()
        b = t.cross(a)
        for k in range(segs):
            ang = 2 * math.pi * k / segs
            verts.append(pt + (a * math.cos(ang) + b * math.sin(ang)) * r)
    for i in range(len(points) - 1):
        for k in range(segs):
            i0, i1 = i * segs + k, i * segs + (k + 1) % segs
            faces.append((i0, i1, i1 + segs, i0 + segs))
    faces.append(tuple(reversed(range(segs))))
    last = (len(points) - 1) * segs
    faces.append(tuple(range(last, last + segs)))
    return verts, faces


def merge(*parts):
    verts, faces = [], []
    for v, f in parts:
        base = len(verts)
        verts += v
        faces += [tuple(i + base for i in fc) for fc in f]
    return verts, faces


def hair_meshes(f, ref=None):
    """{style: (verts, faces)} in head units for every style but bald, plus facial hair. Which
    faces each style keeps is decided on the `ref` face (the neutral one), so every slider
    variant has the same topology."""
    hv, hf, _ = head_mesh(f)
    nrm = normals(hv, hf)
    rv = head_mesh(ref)[0] if ref else hv
    centres = [sum((rv[i] for i in fc), Vector()) / len(fc) for fc in hf]
    fr = ref or f
    out = {}

    def shell_above(thick, field=None, **kw):
        """A style bounded by a hairline: the scalp is clipped exactly along the curve (cut points
        interpolated on the `ref` face, so every slider variant shares the topology), and the shell
        thins to a narrow lip there instead of ending in a staircase of whole faces."""
        g = field or (lambda q: q.z - hairline(azimuth(q), **kw))
        cv, cn, cf = clip(hv, nrm, hf, [g(q) for q in rv])

        def t(b, n):
            return thick(b, n) * (0.15 + 0.85 * smoothstep(0.0, 0.035, g(b)))

        return shell(cv, cf, cn, [True] * len(cf), t, lift=-0.004)

    strands = lambda b, amp=0.004, freq=38: amp * math.sin(math.atan2(b.x, -b.y) * freq + b.z * 20)

    def crown(b, top, sides):
        return sides + (top - sides) * smoothstep(-0.05, 0.35, b.z)

    out["short"] = shell_above(lambda b, n: crown(b, 0.045, 0.022) + strands(b))
    out["buzz"] = shell_above(lambda b, n: 0.008, front=0.31)
    # side part: volume swept to one side, a groove along the parting
    def side_part(b, n):
        part_x = 0.11
        groove = 0.012 * math.exp(-(((b.x - part_x) / 0.012) ** 2)) * smoothstep(0.1, 0.35, b.z) * smoothstep(0.0, -0.2, b.y)
        sweep = 0.02 * smoothstep(0.2, -0.2, b.x)
        return crown(b, 0.06, 0.025) + sweep - groove + strands(b, 0.003, 44)
    out["side_part"] = shell_above(side_part, front=0.26)
    bun_v, bun_f = ellipsoid(Vector((0, 0.3, 0.42)), (0.15, 0.13, 0.13))
    out["bun"] = merge(shell_above(lambda b, n: 0.022 + strands(b, 0.003)), (bun_v, bun_f))
    tail_pts = [Vector((0, 0.43, 0.12)), Vector((0, 0.5, 0.02)), Vector((0, 0.52, -0.15)), Vector((0, 0.5, -0.35)), Vector((0, 0.46, -0.55))]
    tail = tube(tail_pts, [0.065, 0.075, 0.07, 0.055, 0.025])
    tie = tube([Vector((0, 0.43, 0.13)), Vector((0, 0.47, 0.08))], [0.07, 0.07])
    out["ponytail"] = merge(shell_above(lambda b, n: 0.024 + strands(b, 0.003)), tail, tie)

    def curtain(length, flare, ear_z=0.0):
        """Below ear level, behind the cheeks: hang straight down to `length` below the ear line."""

        def move(b, n):
            az = azimuth(b)
            wgt = smoothstep(52, 72, az) * smoothstep(ear_z + 0.08, ear_z - 0.04, b.z)
            if wgt <= 0.0:
                return b, n
            # how far below the ear line on the sphere (0..1 at the bottom pole)
            u = max(0.0, min(1.0, (ear_z - b.z) / (f.face_len + ear_z)))
            ang = math.atan2(b.y, b.x)
            rx, ry = f.rx * 1.04, f.ry + 0.03 * smoothstep(-0.1, 0.2, math.sin(ang))
            k = 1 + flare * u
            hang = Vector((math.cos(ang) * rx * k, math.sin(ang) * ry * k, min(b.z, ear_z - u * length)))
            hn = Vector((math.cos(ang) * ry, math.sin(ang) * rx, 0)).normalized()
            return b.lerp(hang, wgt), n.lerp(hn, wgt).normalized()

        return move

    def long_keep(fringe, cheek_az=62):
        keep = []
        for c in centres:
            az = azimuth(c)
            if c.z > hairline(az, front=fringe, temple=fringe - 0.06, ear=0.1, nape=-1):
                keep.append(True)
            else:
                keep.append(az > cheek_az and c.z < 0.15)
        return keep

    soft = lambda b, amp: amp * math.sin(math.atan2(b.x, -b.y) * 30) * smoothstep(-0.1, 0.1, b.z)
    out["long"] = shell(hv, hf, nrm, long_keep(0.27), lambda b, n: 0.04 + soft(b, 0.004), move=curtain(0.95, 0.2), solid=True)
    out["bob"] = shell(hv, hf, nrm, long_keep(0.17, 68), lambda b, n: 0.05 + soft(b, 0.004), move=curtain(0.42, 0.25), solid=True)

    def curly(b, n):
        bumps = 0.018 * (math.sin(b.x * 60) * math.sin(b.y * 57) * math.sin(b.z * 63))
        return crown(b, 0.11, 0.06) + bumps
    out["curly"] = shell_above(curly, front=0.27)
    out["undercut"] = shell_above(lambda b, n: 0.006 + 0.075 * math.exp(-((b.x / 0.13) ** 4)) * smoothstep(0.05, 0.25, b.z) + strands(b, 0.003) * smoothstep(0.05, 0.25, b.z))
    # receding: a horseshoe around the back and sides, the crown and forehead bare
    def receding(q):
        return min(q.z - hairline(azimuth(q), front=0.9, temple=0.9, ear=0.05), 0.3 - q.z, (azimuth(q) - 70) * 0.006)
    out["receding"] = shell_above(lambda b, n: 0.022 + strands(b, 0.003), field=receding)
    out["afro"] = shell_above(front=0.27, thick=lambda b, n: crown(b, 0.24, 0.14) * (1 - 0.25 * smoothstep(-0.1, -0.4, b.y)) + 0.01 * math.sin(b.x * 70) * math.sin(b.z * 70))

    # facial hair: regions of the lower face
    mw, mz = fr.mouth_w, fr.mouth_z
    nose_bot = 0.02 - fr.nose_len

    def beard_area(c, lip_gap=1.0):
        az = azimuth(c)
        if az > 105:
            return False
        jawline = -0.02 - 0.12 * smoothstep(60, 20, az)  # cheeks stay bare above this
        if c.z > jawline and az < 85:
            return False
        if az >= 85 and c.z > 0.03:
            return False
        mouth = abs(c.x) < mw * 1.05 and -0.045 * lip_gap < c.z - mz < 0.035
        return not mouth and c.z > -fr.face_len - 0.25 and not (c.z > nose_bot - 0.01 and az < 85)

    def moustache_area(c):
        """Across the upper lip from corner to corner, drooping past the corners of the mouth."""
        ax = abs(c.x)
        if c.y > -0.15 or ax > mw * 1.3 or c.z > nose_bot - 0.004:
            return False
        bottom = mz + 0.02 - 0.05 * smoothstep(mw * 0.75, mw * 1.25, ax)
        return c.z > bottom

    def goatee_area(c):
        return abs(c.x) < 0.085 and c.z < mz - 0.035 and c.y < -0.15 and c.z > -fr.face_len - 0.1

    beard = [beard_area(c) or moustache_area(c) for c in centres]
    out["stubble"] = shell(hv, hf, nrm, beard, lambda b, n: 0.0035, lift=0.0015)
    # Define the outline on the neutral face, then carry it with the head's
    # deformation. Whole-face selection alone makes a rectangular, stepped patch.
    # A broad hidden margin lets the shallow shell cross the skin along a tapered
    # curve instead; using reference coordinates keeps every morph's topology fixed.
    def moustache_edge(b):
        u = abs(b.x) / (mw * 1.12)
        centre = mz + 0.062 - 0.026 * min(u, 1.5) ** 1.5
        return 1.0 - u * u - ((b.z - centre) / 0.029) ** 2

    keep = [c.y < -0.15 and moustache_edge(c) > -4.0 for c in centres]
    offsets = {
        tuple(hv[i]): 0.010 * smoothstep(-0.25, 0.65, moustache_edge(rv[i]))
        - 0.018 * (1.0 - smoothstep(-0.8, 0.0, moustache_edge(rv[i])))
        for i in range(len(hv))
    }
    out["moustache"] = shell(hv, hf, nrm, keep, lambda b, n: offsets[tuple(b)], lift=-0.02)
    out["goatee"] = shell(hv, hf, nrm, [goatee_area(c) or moustache_area(c) for c in centres], lambda b, n: 0.02 + strands(b, 0.002, 90))
    out["short_beard"] = shell(hv, hf, nrm, beard, lambda b, n: 0.012 + 0.006 * smoothstep(-0.2, -0.45, b.z))
    out["full_beard"] = shell(hv, hf, nrm, beard, lambda b, n: 0.022 + 0.05 * smoothstep(-0.15, -0.5, b.z) + strands(b, 0.004, 60))
    return out
