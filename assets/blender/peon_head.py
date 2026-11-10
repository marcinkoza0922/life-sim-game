"""Parametric head for the peon generator: skull, face, eyes, ears and brows as pure geometry.

Everything is a function of the face parameters `P` (slider values, -1..1, missing = 0) with a
topology that never depends on them, so the generator turns each slider into a shape key by
building the head twice and subtracting. Lengths are fractions of `hh`, the chin-to-crown height;
the origin is the head centre at eye level, x is the head's left, -y its front, z up.
"""

import math

from mathutils import Vector

# slider id -> label; the order here is the order of the shape keys
FACE_SLIDERS = [
    ("head_width", "Head width"),
    ("face_length", "Face length"),
    ("jaw", "Jaw"),
    ("chin", "Chin"),
    ("cheeks", "Cheekbones"),
    ("cheek_fat", "Cheeks"),
    ("brow", "Brow ridge"),
    ("eye_size", "Eye size"),
    ("eye_spacing", "Eye spacing"),
    ("eye_tilt", "Eye tilt"),
    ("eye_open", "Eyelids"),
    ("nose_length", "Nose length"),
    ("nose_width", "Nose width"),
    ("nose_bridge", "Nose bridge"),
    ("nose_tip", "Nose tip"),
    ("lips", "Lip fullness"),
    ("mouth_width", "Mouth width"),
    ("ears", "Ear size"),
]


# expression shape keys (the game blends these from mood and what a peon is doing)
EXPRESSIONS = ["x_smile", "x_frown", "x_open", "x_blink", "x_brow_up", "x_brow_angry", "x_brow_sad"]


def p(P, k):
    return P.get(k, 0.0)


def smoothstep(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def gauss(dx, sx, dz, sz, power=2.0):
    return math.exp(-(abs(dx / sx) ** power) - (abs(dz / sz) ** power))


def warp(s, k):
    """Maps 0..1 onto 0..1 with extra density around s = 0 (and 1)."""
    return s - k * math.sin(2 * math.pi * s) / (2 * math.pi)


class Face:
    """Face landmarks in head units, derived from the sliders and the child flag."""

    def __init__(self, P, child=False):
        self.P = P
        self.child = child
        c = float(child)
        self.c = c
        self.rx = 0.385 * (1 + 0.06 * p(P, "head_width")) + 0.02 * c
        self.ry = 0.45 + 0.01 * c
        self.rz_top = 0.47 + 0.02 * c
        # the lower face is shorter on children (big cranium, small face)
        self.face_len = (0.5 - 0.07 * c) * (1 + 0.07 * p(P, "face_length"))
        self.eye_x = (0.172 - 0.005 * c) * (1 + 0.08 * p(P, "eye_spacing"))
        self.eye_r = (0.073 + 0.008 * c) * (1 + 0.12 * p(P, "eye_size"))
        self.eye_tilt = math.radians(5 * p(P, "eye_tilt"))
        self.nose_len = self.face_len * (0.32 + 0.05 * p(P, "nose_length")) - 0.02 * c
        self.mouth_z = -self.face_len * 0.56
        # a hint of a smile at rest: a pleasant default face rather than a blank or startled one
        self.smile, self.frown = 0.18 + p(P, "x_smile"), p(P, "x_frown")
        self.mouth_w0 = (0.105 - 0.01 * c) * (1 + 0.15 * p(P, "mouth_width"))
        self.mouth_w = self.mouth_w0 * (1 + 0.18 * self.smile - 0.06 * self.frown)
        self.chin_z = -self.face_len
        self.jaw = 0.08 * p(P, "x_open")  # how far the chin drops

    def mouth_at(self, ax):
        """z of the mouth line at |x|: the corners rise with a smile and sink with a frown."""
        u = min(ax / self.mouth_w, 1.4)
        return self.mouth_z + (0.034 * self.smile - 0.03 * self.frown) * u * u - 0.006 * self.smile

    def jaw_drop(self, ax, y, z):
        """Downward shift of a point below the mouth line when the mouth opens: the middle of the
        lower lip moves at once, towards the corners only deeper points do (a lens-shaped gap)."""
        if self.jaw == 0.0:
            return 0.0
        mz = self.mouth_at(ax)
        u = min(ax / self.mouth_w, 1.0)
        lens = 1 - u * u * (3 - 2 * u)
        s = max(lens * smoothstep(mz - 0.0002, mz - 0.002, z), smoothstep(mz - 0.01, mz - 0.07, z))
        return -self.jaw * s * smoothstep(0.34, 0.2, ax) * smoothstep(-0.02, -0.25, y)


def skull_point(f, d):
    """Head surface point for unit direction d, before facial features."""
    P = f.P
    x = f.rx * d.x
    y = (f.ry if d.y < 0 else f.ry + 0.03) * d.y
    z = (f.rz_top * d.z + 0.02) if d.z > 0 else d.z * (f.face_len + 0.02)
    if d.z < 0:
        # squarer lower face: superellipse instead of an ellipse below the eyes
        sz = min(abs(d.z), 0.999)
        sq = (1 - sz**3.2) ** (1 / 3.2) / math.sqrt(1 - sz * sz)
        x *= sq
        if d.y < 0:
            y *= 1 + (sq - 1) * 0.7
    # flatten the sides of the skull a little (heads are boxier than eggs)
    x *= 1 + 0.05 * (1 - abs(d.x)) * abs(d.x) * 4 * smoothstep(-0.1, 0.3, d.z)
    # lower face: the jaw narrows towards the chin, the back of the head tucks into the neck
    t = smoothstep(0.02, f.chin_z, z)
    jaw = 0.38 - 0.12 * p(P, "jaw") + 0.04 * f.c
    x *= 1 - t * t * jaw - t * 0.06
    if y > 0:
        y *= 1 - t * 0.55
    else:
        # flatten the face plane: the front of the lower face stays forward
        y = y * (1 - 0.1 * t) - 0.02 * t
    # chin
    chin = 0.035 + 0.025 * p(P, "chin") - 0.01 * f.c
    zc = (z - f.chin_z * 0.9) / (f.face_len * 0.12)
    front = smoothstep(-0.05, -0.3, y)
    y -= chin * math.exp(-zc * zc - (x / 0.11) ** 2) * front
    # squared-off jaw angle
    ja = 0.03 * (p(P, "jaw") + 0.3) * (1 - f.c)
    x += math.copysign(ja, x) * math.exp(-((z - f.chin_z * 0.75) / 0.08) ** 2 - ((abs(x) - 0.2) / 0.08) ** 2)
    return Vector((x, y, z))


def face_offset(f, q, jaw=True):
    """Facial features as an offset (along -y mostly) at skull point q; `jaw` includes the
    open-mouth drop."""
    P = f.P
    x, y, z = q
    ax = abs(x)
    front = smoothstep(-0.12, -0.32, y)
    if front <= 0.0:
        return Vector()
    dy = 0.0
    dx = 0.0
    # brow ridge (stronger on adults), tapering towards the temples
    br = (0.022 + 0.014 * p(P, "brow")) * (1 - 0.7 * f.c)
    dy -= br * gauss(ax - 0.12, 0.17, z - 0.085, 0.045)
    # eye sockets: almond hollows the eyeballs show through
    ex, er = f.eye_x, f.eye_r
    zt = z + math.tan(f.eye_tilt) * (ax - ex)  # tilt: outer corners up for positive
    open_ = 1 + 0.25 * p(P, "eye_open")
    sock = gauss(ax - ex + er * 0.05, er * 1.5, zt - 0.006 - 0.0015 * (ax - ex) / er, er * 0.5 * open_, 2.8)
    dy += 0.05 * sock
    # upper lid crease shadow and the cushion below the brow
    dy -= 0.008 * gauss(ax - ex, er * 1.3, zt - er * 1.05, er * 0.3)
    # soft cheek under the eye
    dy -= 0.008 * gauss(ax - ex, er * 1.3, zt + er * 1.3, er * 0.5)
    # cheekbones and cheek fat
    cb = 0.025 + 0.015 * p(P, "cheeks")
    dy -= cb * gauss(ax - 0.22, 0.08, z + 0.09, 0.06)
    dx += cb * 0.6 * gauss(ax - 0.27, 0.06, z + 0.07, 0.07) * (1 if x > 0 else -1)
    # a real smile bunches the cheeks up under the eyes (they rise and round out)
    squint = max(0.0, p(P, "x_smile"))
    apple = squint * gauss(ax - ex * 1.08, er * 1.7, zt + er * 1.75, er * 0.85)
    dy -= 0.016 * apple
    dz = 0.014 * apple
    cf = 0.012 + 0.02 * p(P, "cheek_fat") + 0.02 * f.c + 0.014 * f.smile
    dy -= cf * gauss(ax - 0.18, 0.08, z - f.mouth_z * 0.7, 0.08)
    dx += cf * 0.5 * gauss(ax - 0.22, 0.08, z - f.mouth_z * 0.7, 0.09) * (1 if x > 0 else -1)
    # nose: bridge growing from between the eyes to the tip, wings, nostrils tucked under
    nl = f.nose_len
    top = 0.02
    tip_z = top - nl
    nw = (0.034 + 0.008 * p(P, "nose_width")) * (1 - 0.1 * f.c)
    proj = (0.085 + 0.015 * p(P, "nose_length")) * (1 - 0.35 * f.c)
    u = (top - z) / nl  # 0 at the bridge top, 1 at the tip
    if -0.4 < u < 1.35:
        prof = smoothstep(-0.3, 1.0, u) ** 1.2
        bridge = 0.012 + 0.01 * p(P, "nose_bridge")
        prof = max(prof, bridge / proj * gauss(0, 1, u - 0.15, 0.35))
        under = 1 - smoothstep(1.0, 1.25, u)
        width = nw * (0.55 + 0.45 * smoothstep(0.3, 1.0, u)) * (1 + 0.2 * p(P, "nose_bridge") * (1 - u))
        dy -= proj * prof * under * math.exp(-((ax / width) ** 2.4))
    tip = 0.012 + 0.01 * p(P, "nose_tip")
    dy -= tip * gauss(ax, nw * 0.7, z - tip_z - 0.01, 0.025)
    dy -= 0.02 * gauss(ax - nw * 1.1, nw * 0.45, z - tip_z, 0.022)  # alae
    dy += 0.01 * gauss(ax - nw * 0.5, nw * 0.25, z - tip_z + 0.02, 0.008)  # nostrils
    # nasolabial fold
    dy += (0.006 + 0.007 * f.smile) * gauss(ax - nw * 1.9, 0.012, z - (tip_z + f.mouth_z) * 0.5, 0.05)
    # mouth: upper and lower lip, philtrum groove, mouth line, chin dimple
    mw = f.mouth_w
    lf = 1 + 0.35 * p(P, "lips")
    mz = f.mouth_at(ax)
    lipw = math.exp(-((ax / mw) ** 4))
    dy -= 0.024 * lf * lipw * gauss(0, 1, z - mz - 0.016 * lf, 0.014 * lf)
    dy -= 0.028 * lf * lipw * gauss(0, 1, z - mz + 0.02 * lf, 0.017 * lf)
    dy -= 0.025 * math.exp(-((ax / (mw * 1.3)) ** 2)) * gauss(0, 1, z - mz, 0.06)  # muzzle
    dy += 0.012 * lipw * gauss(0, 1, z - mz, 0.004)
    dy += 0.004 * gauss(ax, 0.01, z - (mz + tip_z) * 0.5, 0.025)  # philtrum
    dy += 0.01 * gauss(ax, 0.06, z - (mz + f.chin_z) * 0.5 - 0.01, 0.015)  # mentolabial sulcus
    return Vector((dx * front, dy * front, dz * front + (f.jaw_drop(ax, y, z) if jaw else 0.0)))


def lip_region(f, q):
    ax = abs(q.x)
    lf = 1 + 0.35 * p(f.P, "lips")
    dz = q.z - f.mouth_z
    # upper lip: a cupid's bow; lower lip: a fuller crescent
    w = f.mouth_w * (1 - (dz / (0.04 * lf)) ** 2 * 0.5)
    if dz >= 0:
        top = 0.026 * lf * (1 - (ax / f.mouth_w) ** 2 * 0.8) + 0.004 * math.exp(-((ax - 0.02) / 0.012) ** 2) - 0.004 * math.exp(-(ax / 0.008) ** 2)
        return ax < w and dz < top and q.y < -0.2
    bot = 0.036 * lf * (1 - (ax / f.mouth_w) ** 2) ** 0.7 if ax < f.mouth_w else 0
    return ax < w and -dz < bot and q.y < -0.2


def head_mesh(f, segs=80, rings=60):
    """Skull + face as a UV sphere grid (poles on z), denser at the front and around the face."""
    verts, faces, mats = [], [], []
    for r in range(1, rings):
        s = r / rings
        # polar angle, denser around the face (~100 degrees from the top)
        pol = math.pi * s
        pol += 0.18 * math.sin(2 * (pol - math.radians(100))) * -0.5
        for k in range(segs):
            a = 2 * math.pi * warp(k / segs, 0.55) - math.pi / 2  # k = 0 faces -y
            d = Vector((math.sin(pol) * math.cos(a), math.sin(pol) * math.sin(a), math.cos(pol)))
            q = skull_point(f, d)
            verts.append(q + face_offset(f, q))
    top = len(verts)
    verts.append(skull_point(f, Vector((0, 0, 1))))
    bot = len(verts)
    verts.append(skull_point(f, Vector((0, 0.3, -1)).normalized()))
    ix = lambda r, k: (r - 1) * segs + k % segs
    for r in range(1, rings - 1):
        for k in range(segs):
            faces.append((ix(r, k), ix(r, k + 1), ix(r + 1, k + 1), ix(r + 1, k)))
    for k in range(segs):
        faces.append((top, ix(1, k + 1), ix(1, k)))
        faces.append((bot, ix(rings - 1, k), ix(rings - 1, k + 1)))
    for fc in faces:
        c = sum((verts[i] for i in fc), Vector()) / len(fc)
        mats.append("skin")
    return verts, faces, mats


def surface_y(f, x, z):
    """Front skin depth at (x, z), searching the skull + face along -y (no eyes yet)."""
    best = None
    # find direction whose skull point has this x/z (Newton-free: scan)
    lo, hi = -0.6, 0.0
    for _ in range(40):
        mid = (lo + hi) * 0.5
        q = Vector((x, mid, z))
        # inside test against the deformed surface: compare radius along the ray from the centre
        d = q.normalized()
        s = skull_point(f, d)
        s = s + face_offset(f, s, jaw=False)
        if q.length < s.length:
            hi = mid
        else:
            lo = mid
        best = mid
    return best


def eye_mesh(f, side, segs=24, rings=16):
    """Eyeball with the iris and pupil as caps facing forward, sunk into its socket."""
    er = f.eye_r
    cx = side * f.eye_x
    cz = 0.003
    sy = surface_y(f, cx, cz + er * 1.6)  # skin above the socket
    cy = sy + er * 1.0
    verts, faces, mats = [], [], []
    iris_a, pupil_a = 0.52, 0.24
    for r in range(rings + 1):
        th = math.pi * r / rings  # 0 = front pole
        for k in range(segs):
            a = 2 * math.pi * k / segs
            # front-facing pole along -y; the cornea bulges a little over the iris
            bulge = 1 + 0.06 * max(0.0, 1 - th / iris_a) ** 1.5
            verts.append(Vector((math.sin(th) * math.cos(a) * er, -math.cos(th) * er * bulge, math.sin(th) * math.sin(a) * er)))
    for r in range(rings):
        for k in range(segs):
            i0 = r * segs + k
            i1 = r * segs + (k + 1) % segs
            faces.append((i0, i1, i1 + segs, i0 + segs))
            th = math.pi * (r + 0.5) / rings
            up = math.sin(th) * math.sin(2 * math.pi * (k + 0.5) / segs)  # height on the ball
            # the upper lid shades the top of the white, so the eye sits in its socket
            mats.append("pupil" if th < pupil_a else "iris" if th < iris_a else "eye_shade" if up > 0.42 else "eye_white")
    # catchlight: a small bright disc on the cornea, up and to the peon's right on both eyes (one
    # light source), so the eyes read as wet and alive even in shade
    gb = len(verts)
    gth, ga, gr = 0.3, math.radians(125), 0.13
    centre = Vector((math.sin(gth) * math.cos(ga), -math.cos(gth), math.sin(gth) * math.sin(ga)))
    t1 = centre.cross(Vector((0, 0, 1))).normalized()
    t2 = t1.cross(centre).normalized()
    verts.append(centre * er * 1.075)
    for k in range(10):
        a = 2 * math.pi * k / 10
        verts.append((centre + (t1 * math.cos(a) + t2 * math.sin(a)) * gr) .normalized() * er * 1.075)
    for k in range(10):
        faces.append((gb, gb + 1 + (k + 1) % 10, gb + 1 + k))
        mats.append("glint")
    # look very slightly inward and down, like a relaxed gaze at a few metres
    from mathutils import Euler

    rot = Euler((math.radians(2), 0, math.radians(side * 4)), "XYZ").to_matrix()
    verts = [rot @ v + Vector((cx, cy, cz)) for v in verts]
    return verts, faces, mats


def eye_centre(f, side):
    er = f.eye_r
    cx, cz = side * f.eye_x, 0.003
    return Vector((cx, surface_y(f, cx, cz + er * 1.6) + er * 1.0, cz))


def lid_mesh(f, side, segs=20, rows=6):
    """Upper and lower eyelids: shells just outside the eyeball, from the lid line back into the
    socket, with a rounded rim. Lid lines are angles above/below the gaze, tilted with the eye."""
    er = f.eye_r
    c = eye_centre(f, side)
    open_ = p(f.P, "eye_open")
    # a real (Duchenne) smile squints: the cheeks push the lower lid up and the upper lid drops a
    # little; only the expression does this, the resting hint of a smile keeps the eyes open
    squint = max(0.0, p(f.P, "x_smile"))
    up_line = math.radians(19 + 8 * open_ - 6 * f.c - 6 * squint)  # relaxed: the lid covers the top of the iris
    lo_line = math.radians(-30 - 4 * open_ + 7 * f.smile + 7 * squint)  # a smile lifts the lower lid
    blink = p(f.P, "x_blink")
    up_line += (lo_line + math.radians(1) - up_line) * blink
    verts, faces, mats = [], [], []
    for which, line, far in (("up", up_line, math.radians(80)), ("lo", lo_line, math.radians(-75))):
        base = len(verts)
        for k in range(segs + 1):
            u = k / segs  # across the eye, inner (nose) corner to outer
            az = math.radians(-62 + 124 * u)  # yaw from the gaze
            # lid edge: an arch, highest a little inside of centre (upper), flatter below
            if which == "up":
                edge = line * (0.35 + 0.65 * math.sin(math.pi * (u * 0.92 + 0.04)) ** 0.7)
            else:
                edge = line * (0.35 + 0.65 * math.sin(math.pi * (u * 0.92 + 0.08)) ** 1.0)
                # smiling eyes: the cheek bows the lower lid up into a crescent
                edge += math.radians(12) * squint * math.sin(math.pi * (u * 0.92 + 0.08)) ** 1.5
            edge += f.eye_tilt * (u - 0.5) * 2 * (1 if side > 0 else 1)
            for j in range(rows + 1):
                t = (j / rows) ** 2  # rows bunch at the lid edge: the first one is the lash line
                el = edge + (far - edge) * t
                rad = er * (1.06 + 0.05 * math.sin(math.pi * min(1.0, t * 3)) * (1 - t))
                if j == 0:
                    rad = er * 1.02
                d = Vector((math.sin(az) * math.cos(el) * side, -math.cos(az) * math.cos(el), math.sin(el)))
                verts.append(c + d * rad)
        for k in range(segs):
            for j in range(rows):
                a = base + k * (rows + 1) + j
                fc = (a, a + 1, a + rows + 2, a + rows + 1)
                if (which == "up") != (side > 0):
                    fc = tuple(reversed(fc))
                faces.append(fc)
                mats.append("pupil" if which == "up" and j == 0 else "skin")
    # upper lashes: a dark fringe from the lash line, sweeping out and up, longest at the outer
    # corner (a soft wing), which frames the eye at any distance
    lash = er * 0.36 * (1 - 0.5 * f.c) * (1 - blink * 0.6)
    lb = len(verts)
    for k in range(segs + 1):
        u = k / segs
        root = verts[k * (rows + 1)]  # upper lid, lash line
        out = (root - c).normalized()
        sweep = (out * 0.55 + Vector((0, -0.25, 1.0))).normalized()
        sweep.x += side * 0.5 * u * u
        ln = lash * (0.35 + 0.65 * max(0.0, math.sin(math.pi * min(0.97, 0.15 + u * 0.82))) ** 0.5) * (0.6 + 0.6 * u)
        verts.append(root + out * er * 0.01)
        verts.append(root + sweep.normalized() * ln)
    for k in range(segs):
        a = lb + 2 * k
        fc = (a, a + 2, a + 3, a + 1)
        faces.append(fc if side > 0 else tuple(reversed(fc)))
        mats.append("lash")
    return verts, faces, mats


def lips_mesh(f, nu=22, nv=6):
    """Lips as a shell just over the sculpted mouth: smooth outline, tinted lip colour. Plus the
    inside of the mouth (teeth above, dark below): a strip along the mouth line that has no height
    until the jaw drops."""
    lf = 1 + 0.35 * p(f.P, "lips")
    mw = f.mouth_w
    verts, faces, mats = [], [], []

    def opening(ax):
        """How much of the jaw drop the lower lip takes at |x|: all of it mid-mouth, none at the
        corners (so the open mouth is lens-shaped)."""
        u = min(ax / mw, 1.0)
        return 1 - u * u * (3 - 2 * u)

    def on_skin(x, z, lift, lower=False):
        y = surface_y(f, x, z) - lift
        drop = -f.jaw * opening(abs(x)) if lower else f.jaw_drop(abs(x), y, z)
        return Vector((x, y, z + drop))

    for which in ("up", "lo"):
        base = len(verts)
        for i in range(nu + 1):
            x = -mw + 2 * mw * i / nu
            ax = abs(x)
            mz = f.mouth_at(ax)
            if which == "up":
                h = 0.03 * lf * max(0.0, 1 - (ax / mw) ** 2) ** 0.8
                h += 0.004 * math.exp(-((ax - 0.022) / 0.012) ** 2) - 0.004 * math.exp(-(ax / 0.009) ** 2)
                z0, z1 = mz + 0.001, mz + 0.001 + max(h, 0.0)
            else:
                h = 0.038 * lf * max(0.0, 1 - (ax / mw) ** 2) ** 0.6
                z0, z1 = mz - 0.001 - max(h, 0.0), mz - 0.001
            for j in range(nv + 1):
                verts.append(on_skin(x, z0 + (z1 - z0) * j / nv, 0.0012, lower=which == "lo"))
        for i in range(nu):
            for j in range(nv):
                a = base + i * (nv + 1) + j
                faces.append((a, a + 1, a + nv + 2, a + nv + 1))
                mats.append("skin.lips")
    # inside of the mouth: rows from the lower lip's edge up to the upper lip's
    base = len(verts)
    for i in range(nu + 1):
        x = -mw + 2 * mw * i / nu
        mz = f.mouth_at(abs(x))
        top = on_skin(x, mz + 0.001, 0.0016)
        bot = on_skin(x, mz - 0.001, 0.0016, lower=True)
        # in front of the skin the open jaw drags into the gap: as far forward as the lips reach
        front = min(surface_y(f, x, mz + dz * 0.004) for dz in range(-8, 9)) - 0.0015
        for t in (0.0, 0.62, 1.0):
            v = bot.lerp(top, t)
            v.y = front
            verts.append(v)
    for i in range(nu):
        for j in range(2):
            a = base + i * 3 + j
            faces.append((a, a + 1, a + 4, a + 3))
            mats.append("mouth" if j == 0 else "teeth")
    return verts, faces, mats


def ear_mesh(f, side, segs=16, rings=10):
    s = 1 + 0.15 * p(f.P, "ears")
    h, w, t = 0.17 * s, 0.09 * s, 0.045 * s
    # attach behind the jaw hinge, from brow to nose-bottom height
    cz = -0.05 + 0.01 * f.c
    sx = f.rx * (1 - 0.05) + 0.0
    cy = 0.02
    verts, faces, mats = [], [], []
    for r in range(rings + 1):
        th = math.pi * r / rings
        for k in range(segs):
            a = 2 * math.pi * k / segs
            u, v = math.sin(th) * math.cos(a), math.sin(th) * math.sin(a)
            n = math.cos(th)  # +1 = outward side
            rim = (u * u + v * v) ** 0.5
            # cup: the outer face dips in inside the rim; lobe thicker at the bottom
            cup = 0.6 * max(0.0, 1 - rim * 1.25) if n > 0 else 0.0
            ww = w * (1 - 0.25 * max(0.0, -v))  # narrower lobe
            verts.append(Vector((n * t * (1 - cup) * 0.5 + t * 0.4, u * ww, v * h)))
    for r in range(rings):
        for k in range(segs):
            i0 = r * segs + k
            i1 = r * segs + (k + 1) % segs
            faces.append((i0, i1, i1 + segs, i0 + segs))
            mats.append("skin")
    from mathutils import Matrix

    # flare out from the head and lean back
    m = Matrix.Rotation(math.radians(-side * 24), 3, "Z") @ Matrix.Rotation(math.radians(-12), 3, "X")
    verts = [m @ Vector((v.x * side, v.y, v.z)) + Vector((side * sx, cy, cz)) for v in verts]
    if side < 0:
        faces = [tuple(reversed(fc)) for fc in faces]
    return verts, faces, mats


def brow_mesh(f, side, n=14):
    """A tapered arch above the eye, lifted off the skin (tinted with the hair colour)."""
    P = f.P
    ex, er = f.eye_x, f.eye_r
    verts, faces, mats = [], [], []
    thick = 0.028 * (1 - 0.3 * f.c)
    for i in range(n + 1):
        u = i / n  # 0 = inner end
        x = ex - er * 1.25 + u * er * 2.6
        z = er * 1.35 + 0.012 + math.sin(u * math.pi * 0.85) * 0.022 - u * 0.006 + math.tan(f.eye_tilt) * (x - ex) * 0.5
        # expressions: raised, knitted (inner ends down) or worried (inner ends up)
        inner = (1 - u) ** 1.5
        z += 0.024 * p(P, "x_brow_up") + inner * (0.024 * p(P, "x_brow_sad") - 0.022 * p(P, "x_brow_angry"))
        z += u * (0.008 * p(P, "x_brow_angry") - 0.012 * p(P, "x_brow_sad"))
        w = thick * (1.0 - 0.6 * u) * (0.7 + 0.3 * min(1.0, u * 4))
        for k, dz in enumerate((-w * 0.5, 0.0, w * 0.5)):
            y = surface_y(f, x, z + dz) - 0.004 - (0.003 if k == 1 else 0.0)
            verts.append(Vector((side * x, y, z + dz)))
    for i in range(n):
        for k in range(2):
            a = i * 3 + k
            fc = (a, a + 3, a + 4, a + 1)
            faces.append(fc if side > 0 else tuple(reversed(fc)))
            mats.append("hair")
    return verts, faces, mats


def build_head(hh, center, P, child=False):
    """[(name, verts, faces, mats)] in model space for a head of height hh centred at `center`."""
    f = Face(P, child)
    parts = [("head", *head_mesh(f)), ("lips", *lips_mesh(f))]
    for side in (1, -1):
        parts.append(("eye", *eye_mesh(f, side)))
        parts.append(("lid", *lid_mesh(f, side)))
        parts.append(("ear", *ear_mesh(f, side)))
        parts.append(("brow", *brow_mesh(f, side)))
    out = []
    for name, verts, faces, mats in parts:
        out.append((name, [tuple(center + v * hh) for v in verts], faces, mats))
    return out
