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


def shell(verts, faces, nrm, keep, thick, lift=0.004, move=None, solid=False, wall=True):
    """Shell over the kept faces: the outer surface at `thick` (per-vertex function) and a wall
    down to the scalp along its open edges. `solid` adds the inner surface too (for hair that hangs
    away from the head, seen from below); otherwise the head fills the inside. `move` remaps base
    points (long hair). `wall=False` leaves the rim open (stubble: a wall only 1 mm tall still
    bends the smooth normals at the edge into a bright line)."""
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
    open_edges = [(a, b) for (a, b) in edge_count if (b, a) not in edge_count] if wall else []
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


WHORL = Vector((0.0, 0.45, 0.89)).normalized()  # the crown's whorl, up and to the back


def locks(b, count=16, whorl=WHORL, wave=0.18, seed=0.0, fine=0.3):
    """Clumped-hair pattern at a scalp point: 0 in the grooves between locks, 1 along their spines.
    Locks radiate from the crown `whorl` and wave a little along their length; the profile is
    rounded on top with a sharp groove (sqrt of |sin|), so they read as sculpted clumps, not ridges.
    A second, finer layer (weight `fine`) splits the big locks unevenly."""
    p = b.normalized()
    e1 = whorl.cross(Vector((1.0, 0.0, 0.0))).normalized()
    e2 = whorl.cross(e1)
    s = math.acos(max(-1.0, min(1.0, p.dot(whorl))))
    phi = math.atan2(p.dot(e2), p.dot(e1)) + wave * math.sin(s * 4.0 + seed)
    big = math.sqrt(abs(math.sin(phi * count / 2)))
    small = math.sqrt(abs(math.sin(phi * count * 0.85 + s * 3.0 + seed + 1.3)))
    fade = smoothstep(0.08, 0.45, s)  # the whorl itself is smooth
    return (1.0 - fade) * 0.7 + fade * ((1.0 - fine) * big + fine * small)


def curl_centres(count, seed=0.0):
    """`count` points spread evenly over the unit sphere (a Fibonacci spiral), each nudged off
    the spiral by a hash so the curls don't line up in rows."""
    out = []
    golden = math.pi * (3.0 - math.sqrt(5.0))
    for i in range(count):
        h1 = math.sin(i * 12.9898 + seed * 78.233) * 43758.5453
        h2 = math.sin(i * 39.3468 + seed * 11.135) * 24634.6345
        j1, j2 = h1 - math.floor(h1) - 0.5, h2 - math.floor(h2) - 0.5
        z = 1.0 - 2.0 * (i + 0.5 + 0.6 * j1) / count
        r = math.sqrt(max(0.0, 1.0 - z * z))
        a = golden * i + 1.2 * j2 * golden
        out.append(Vector((r * math.cos(a), r * math.sin(a), z)))
    return out


def curls(b, centres, size):
    """Coiled-hair pattern at a scalp point: 1 on top of each curl, 0 in the creases between
    them. Cellular: the gap between the nearest and second-nearest curl centre (angle on the
    sphere, over `size`) gives a rounded dome per curl with a sharp crease where two meet."""
    p = b.normalized()
    d1 = d2 = 9.0
    for c in centres:
        d = 1.0 - p.dot(c)  # cheap, monotonic in angle
        if d < d1:
            d1, d2 = d, d1
        elif d < d2:
            d2 = d
    gap = math.sqrt(2.0 * d2) - math.sqrt(2.0 * d1)  # ~ angular distance to the crease
    return math.sqrt(min(1.0, gap / size))


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

    def shell_above(thick, field=None, tips=0.0, count=16, **kw):
        """A style bounded by a hairline: the scalp is clipped exactly along the curve (cut points
        interpolated on the `ref` face, so every slider variant shares the topology), and the shell
        thins to a narrow lip there instead of ending in a staircase of whole faces."""
        g0 = field or (lambda q: q.z - hairline(azimuth(q), **kw))
        # lock tips break up the hairline: the edge reaches further along each lock's spine
        g = (lambda q: g0(q) + tips * (locks(q, count, fine=0.0) ** 3 - 0.4)) if tips else g0
        cv, cn, cf = clip(hv, nrm, hf, [g(q) for q in rv])

        def t(b, n):
            return thick(b, n) * (0.15 + 0.85 * smoothstep(0.0, 0.035, g(b)))

        return shell(cv, cf, cn, [True] * len(cf), t, lift=-0.004)

    clumps = lambda b, amp=0.014, count=16: amp * (locks(b, count) - 0.5)

    def crown(b, top, sides):
        return sides + (top - sides) * smoothstep(-0.05, 0.35, b.z)

    out["short"] = shell_above(lambda b, n: crown(b, 0.05, 0.026) + clumps(b, 0.032, 13), tips=0.035, count=13)
    out["buzz"] = shell_above(lambda b, n: 0.008, front=0.31)
    # side part: volume swept to one side, a groove along the parting
    def side_part(b, n):
        part_x = 0.11
        groove = 0.012 * math.exp(-(((b.x - part_x) / 0.012) ** 2)) * smoothstep(0.1, 0.35, b.z) * smoothstep(0.0, -0.2, b.y)
        sweep = 0.02 * smoothstep(0.2, -0.2, b.x)
        return crown(b, 0.06, 0.025) + sweep - groove + clumps(b, 0.026, 12)
    out["side_part"] = shell_above(side_part, front=0.26, tips=0.03, count=12)
    bun_v, bun_f = ellipsoid(Vector((0, 0.3, 0.42)), (0.15, 0.13, 0.13))
    out["bun"] = merge(shell_above(lambda b, n: 0.024 + clumps(b, 0.012, 22), tips=0.012, count=22), (bun_v, bun_f))
    tail_pts = [Vector((0, 0.43, 0.12)), Vector((0, 0.5, 0.02)), Vector((0, 0.52, -0.15)), Vector((0, 0.5, -0.35)), Vector((0, 0.46, -0.55))]
    tail = tube(tail_pts, [0.065, 0.075, 0.07, 0.055, 0.025])
    tie = tube([Vector((0, 0.43, 0.13)), Vector((0, 0.47, 0.08))], [0.07, 0.07])
    out["ponytail"] = merge(shell_above(lambda b, n: 0.026 + clumps(b, 0.012, 22), tips=0.012, count=22), tail, tie)

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

    def long_hair(fringe, cheek_az, thick, move, count=14):
        """Long styles: the scalp above a fringe line plus everything behind the cheeks below
        z 0.15, clipped along that smooth boundary (lock tips along the fringe) and hung by `move`."""

        def g(q):
            az = azimuth(q)
            top = q.z - hairline(az, front=fringe, temple=fringe - 0.06, ear=0.1, nape=-1)
            top += 0.03 * (locks(q, count, fine=0.0) ** 3 - 0.4) * smoothstep(60, 20, az)
            # below the jaw, start where the curtain hangs fully (half-hung faces fold into shards)
            edge = cheek_az + (72 - cheek_az + 4) * smoothstep(-0.1, -0.35, q.z)
            return max(top, min((az - edge) * 0.006, 0.15 - q.z))

        cv, cn, cf = clip(hv, nrm, hf, [g(q) for q in rv])
        return shell(cv, cf, cn, [True] * len(cf), lambda b, n: thick(b) * (0.2 + 0.8 * smoothstep(0.0, 0.03, g(b))), move=move, solid=True)

    out["long"] = long_hair(0.27, 62, lambda b: 0.042 + clumps(b, 0.02, 14) * smoothstep(-0.6, 0.1, b.z), curtain(0.95, 0.2))
    out["bob"] = long_hair(0.17, 68, lambda b: 0.052 + clumps(b, 0.02, 14) * smoothstep(-0.6, 0.1, b.z), curtain(0.42, 0.25))

    # curly and afro: rounded coils with creases between them, big enough to lump the silhouette;
    # the hairline follows the curls too, so the edge is a row of coil fronts, not a cut line
    curl_c = curl_centres(150, seed=1.0)
    curl_f = curl_centres(420, seed=2.0)
    coil = lambda b: 0.75 * curls(b, curl_c, 0.12) + 0.25 * curls(b, curl_f, 0.07)
    coil_edge = lambda q: 0.03 * (curls(q, curl_c, 0.12) - 0.55)
    curly_line = lambda q: q.z - hairline(azimuth(q), front=0.27) + coil_edge(q)
    out["curly"] = shell_above(lambda b, n: crown(b, 0.1, 0.05) + 0.04 * (coil(b) - 0.5), field=curly_line)
    out["undercut"] = shell_above(lambda b, n: 0.006 + 0.075 * math.exp(-((b.x / 0.13) ** 4)) * smoothstep(0.05, 0.25, b.z) + clumps(b, 0.03, 12) * smoothstep(0.05, 0.25, b.z))
    # receding: a horseshoe around the back and sides, the crown and forehead bare
    def receding(q):
        return min(q.z - hairline(azimuth(q), front=0.9, temple=0.9, ear=0.05), 0.3 - q.z, (azimuth(q) - 70) * 0.006)
    out["receding"] = shell_above(lambda b, n: 0.024 + clumps(b, 0.014, 18), field=receding, tips=0.015, count=18)
    afro_c = curl_centres(70, seed=3.0)
    afro_f = curl_centres(300, seed=4.0)
    afro_coil = lambda b: 0.7 * curls(b, afro_c, 0.18) + 0.3 * curls(b, afro_f, 0.08)
    afro_line = lambda q: q.z - hairline(azimuth(q), front=0.27) + 0.03 * (curls(q, afro_c, 0.18) - 0.55)

    def afro(b, n):
        # rounds off over the forehead and temples instead of jutting out as a cap brim
        front = 0.35 + 0.65 * smoothstep(0.0, 0.16, afro_line(b))
        volume = crown(b, 0.24, 0.14) * (1 - 0.25 * smoothstep(-0.1, -0.4, b.y))
        return volume * front + 0.075 * (afro_coil(b) - 0.5)

    out["afro"] = shell_above(afro, field=afro_line)

    # facial hair: regions of the lower face
    mw, mz = fr.mouth_w, fr.mouth_z
    nose_bot = 0.02 - fr.nose_len

    def clipped(field, thick, ramp=0.025, lip=0.15, lift=-0.004, wall=True):
        """Facial hair bounded by a smooth field (>= 0 inside, roughly in head units from the edge):
        the skin is clipped along its zero line on the `ref` face and the shell tapers to a lip over
        `ramp`, so outlines are curves rather than staircases of whole faces."""
        cv, cn, cf = clip(hv, nrm, hf, [field(q) for q in rv])
        return shell(cv, cf, cn, [True] * len(cf), lambda b, n: thick(b, n) * (lip + (1 - lip) * smoothstep(0.0, ramp, field(b))), lift=lift, wall=wall)

    def mouth_gap(q, grow=1.0):
        """Outside an ellipse around the lips (positive), so beards leave the mouth bare."""
        e = (q.x / (mw * 1.08 * grow)) ** 2 + ((q.z - mz + 0.004) / (0.042 * grow)) ** 2
        return 0.04 * (math.sqrt(e) - 1.0) if q.y < -0.1 else 1.0

    def beard_field(q, cheek=0.0):
        """Jaw, chin and cheeks below a line sweeping from the sideburn down to beside the mouth."""
        az = azimuth(q)
        # cheek line: high at the sideburn, curving down to just above the mouth corner
        top = mz + 0.075 + cheek + (0.06 - mz - 0.075) * smoothstep(22, 82, az) ** 1.6
        sideburn = (100 - az) * 0.004  # stops in front of the ear
        neck = q.z + fr.face_len + 0.22 - 0.06 * smoothstep(0, 60, az)
        return min(top - q.z, sideburn, neck, mouth_gap(q))

    def moustache_edge(b):
        """Upper-lip moustache outline (> 0 inside): an ellipse over the lip, its ends tapering
        down past the corners of the mouth."""
        u = abs(b.x) / (mw * 1.12)
        centre = mz + 0.062 - 0.026 * min(u, 1.5) ** 1.5
        return 1.0 - u * u - ((b.z - centre) / 0.029) ** 2

    def moustache_field(q):
        return min(0.025 * moustache_edge(q), -0.15 - q.y, mouth_gap(q))

    def goatee_field(q):
        chin = min(mz - 0.01 - q.z, 0.09 - abs(q.x) * (1.0 + 0.8 * smoothstep(mz, -fr.face_len, q.z)), -0.15 - q.y, q.z + fr.face_len + 0.08, mouth_gap(q))
        return max(chin, moustache_field(q))

    full = lambda q: max(beard_field(q), moustache_field(q))
    out["stubble"] = clipped(full, lambda b, n: 0.0035, ramp=0.03, lip=0.3, wall=False)
    # The standalone moustache is a shallow shell over a broad hidden margin whose offset crosses
    # the skin along `moustache_edge` (on the neutral face, carried by the head's deformation).
    keep = [c.y < -0.15 and moustache_edge(c) > -4.0 for c in centres]
    offsets = {
        tuple(hv[i]): 0.010 * smoothstep(-0.25, 0.65, moustache_edge(rv[i]))
        - 0.018 * (1.0 - smoothstep(-0.8, 0.0, moustache_edge(rv[i])))
        for i in range(len(hv))
    }
    out["moustache"] = shell(hv, hf, nrm, keep, lambda b, n: offsets[tuple(b)], lift=-0.02)
    chin = Vector((0.0, -0.45, -0.89)).normalized()
    tufts = lambda b, amp, count: amp * (locks(b, count, whorl=chin, wave=0.1) - 0.5)
    out["goatee"] = clipped(goatee_field, lambda b, n: 0.018 + tufts(b, 0.008, 30), ramp=0.02)
    out["short_beard"] = clipped(full, lambda b, n: 0.012 + 0.006 * smoothstep(-0.2, -0.45, b.z) + tufts(b, 0.006, 30), ramp=0.03)
    full_big = lambda q: max(beard_field(q, cheek=0.02), moustache_field(q))
    out["full_beard"] = clipped(full_big, lambda b, n: 0.022 + 0.05 * smoothstep(-0.15, -0.5, b.z) + tufts(b, 0.02, 26), ramp=0.05)
    return out
