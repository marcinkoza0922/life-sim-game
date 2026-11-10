"""Builds the tiling materials as MaterialCraft packages and exports their maps.

    python3 assets/materialcraft/materials.py [id ...]

For each material this writes `assets/materialcraft/<id>.sbs` (open it in MaterialCraft to tweak),
cooks it headlessly with `materialcraft-cli`, and writes `assets/textures/<id>/{albedo,normal,orm}.png`
(the layout `crates/game/build.rs` embeds). Each graph is four pixel processors (albedo, height,
ORM, plus a Normal node on the height) whose functions are generated from the expressions below,
so everything tiles exactly: the patterns are periodic in the 0..1 texture square, and the
noise lattices wrap.

Texture orientation in the game (see assets/textures/README.md): on walls the image's top is up;
on floors image x runs along world x and image y along world z.
"""

import json
import math
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from fngraph import E, c, pos, sx, sy, v4, floor, frac, sin, absf, mn, mx, mod, lerp, clamp01, hash1, cellhash, graph

HERE = Path(__file__).resolve().parent
TEXTURES = HERE.parent / "textures"
SIZE = 512


# ------------------------------------------------------------------------------------------ helpers


def sstep(a, b, x):
    t = clamp01((x - a) / (b - a))
    return t * t * (3.0 - 2.0 * t)


def vnoise(px, py, nx, ny, seed):
    """Tiling value noise in 0..1 with an nx × ny lattice over the texture square."""
    gx, gy = px * nx, py * ny
    fx, fy = gx - floor(gx), gy - floor(gy)
    ix, iy = mod(floor(gx), nx), mod(floor(gy), ny)  # wrap so shifted lookups still tile
    ux, uy = fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy)
    x1, y1 = mod(ix + 1.0, nx), mod(iy + 1.0, ny)
    h = lambda a, b: hash1(a, b, seed)
    return lerp(lerp(h(ix, iy), h(x1, iy), ux), lerp(h(ix, y1), h(x1, y1), ux), uy)


def fbm(px, py, nx, ny, octaves, seed):
    """Sum of `octaves` value-noise layers doubling in frequency, normalized to 0..1."""
    total, amp, norm = None, 1.0, 0.0
    for o in range(octaves):
        n = vnoise(px, py, nx * 2**o, ny * 2**o, seed + 17.0 * o) * amp
        total = n if total is None else total + n
        norm += amp
        amp *= 0.5
    return total / norm


def grain(px, py, seed):
    """Per-pixel white noise in 0..1."""
    return hash1(floor(px * SIZE), floor(py * SIZE), seed)


def rgb(r, g, b):
    return (c(r), c(g), c(b))


def mix3(a, b, t):
    return tuple(lerp(x, y, t) for x, y in zip(a, b))


def scale3(a, k):
    return tuple(x * k for x in a)


# ---------------------------------------------------------------------------------------- materials


def wall_brick():
    """Running-bond brick, 6 bricks × 20 courses per repeat (1.5 m: ~25 × 7.5 cm with mortar)."""
    cols, rows = 6, 20
    p = pos()
    px, py = sx(p), sy(p)
    row = floor(py * rows)
    X = px * cols + mod(row, 2.0) * 0.5
    fx, fy = frac(X), frac(py * rows)
    blotch = fbm(px, py, 8, 8, 4, 3.0)
    edge_wobble = fbm(px, py, 32, 32, 2, 9.0)
    # distance to the brick's edge in pixels, roughened so mortar lines aren't ruler-straight
    d = mn(mn(fx, 1.0 - fx) * (SIZE / cols), mn(fy, 1.0 - fy) * (SIZE / rows)) + (edge_wobble - 0.5) * 2.0
    face = sstep(1.5, 3.0, d)
    bevel = sstep(1.5, 6.0, d)
    cx = mod(floor(X), float(cols))
    r1, r2 = cellhash(cx, row, 1.0), cellhash(cx, row, 2.0)
    g = grain(px, py, 5.0)

    brick = mix3(rgb(0.50, 0.22, 0.15), rgb(0.66, 0.34, 0.22), r1)
    brick = mix3(brick, rgb(0.36, 0.17, 0.13), sstep(0.8, 0.95, r2))  # the odd dark brick
    brick = scale3(brick, (0.8 + 0.35 * blotch) * (0.93 + 0.14 * g))
    mortar = scale3(rgb(0.70, 0.68, 0.64), 0.88 + 0.2 * blotch + 0.08 * g)
    albedo = mix3(mortar, brick, face)

    height = 0.15 + 0.05 * g + bevel * (0.55 + 0.1 * r2 + 0.15 * blotch + 0.04 * g)
    rough = lerp(0.95, 0.78 + 0.12 * blotch, face)
    ao = lerp(0.6, 1.0, sstep(0.0, 6.0, d))
    return albedo, height, (ao, rough, c(0.0)), 4.0


def wall_plaster():
    """Smooth painted plaster: near-white (walls tint it) with trowel blotches and a fine stipple."""
    p = pos()
    px, py = sx(p), sy(p)
    trowel = fbm(px, py, 4, 4, 4, 21.0)
    stipple = fbm(px, py, 96, 96, 2, 33.0)
    g = grain(px, py, 7.0)
    tone = 0.9 + 0.06 * trowel + 0.03 * stipple + 0.015 * g
    albedo = (tone, tone, tone * 0.985)
    height = 0.5 * trowel + 0.35 * stipple + 0.15 * g
    rough = 0.82 + 0.1 * stipple
    ao = 0.92 + 0.08 * stipple
    return albedo, height, (ao, rough, c(0.0)), 0.7


def floor_wood():
    """Oak planks along image x: 10 rows × 2 planks per repeat (2 m: 1 m × 20 cm boards)."""
    rows, per_row = 10, 2
    p = pos()
    px, py = sx(p), sy(p)
    row = floor(py * rows)
    X = px * per_row + hash1(row, 0.0, 3.0)  # stagger the joints row by row
    fx, fy = frac(X), frac(py * rows)
    plank = mod(floor(X), float(per_row))
    # plain hash1: these feed the grain lookups, and MaterialCraft's evaluator doesn't share
    # sub-expressions, so cellhash here blows the execution budget (staggered joints hide the correlation)
    r1, r2 = hash1(plank, row, 4.0), hash1(plank, row, 6.0)
    d = mn(mn(fx, 1.0 - fx) * (SIZE / per_row), mn(fy, 1.0 - fy) * (SIZE / rows))
    open_ = sstep(0.6, 1.8, d)
    # grain: noise stretched along the board, offset per plank so neighbours don't match
    ox, oy = px + r1 * 0.37, py + r2 * 0.29
    streak = fbm(ox, oy, 2, 48, 3, 41.0)
    fine = fbm(ox, oy, 8, 192, 2, 53.0)
    figure = sin((oy * 60.0 + streak * 5.0) * (2.0 * math.pi)) * 0.5 + 0.5
    # one grey tone mapped through a gradient (gap → dark → light oak): evaluating the grain once
    # instead of per colour channel keeps the function inside MaterialCraft's execution budget
    tone = open_ * (0.25 + 0.75 * clamp01(0.35 * r1 + 0.5 * sstep(0.25, 0.75, streak) + 0.15 * figure * fine))
    albedo = Gradient(tone, [(0.0, (0.14, 0.09, 0.06)), (0.25, (0.34, 0.22, 0.13)), (0.6, (0.50, 0.35, 0.21)), (1.0, (0.66, 0.49, 0.32))])
    height = open_ * (0.8 + 0.04 * streak + 0.03 * fine) + 0.05
    rough = lerp(0.9, 0.42 + 0.14 * fine + 0.08 * (1.0 - streak), open_)
    ao = lerp(0.5, 1.0, sstep(0.0, 3.0, d))
    return albedo, height, (ao, rough, c(0.0)), 2.5


def floor_tile():
    """Glazed ceramic: 4 × 4 tiles per repeat (1 m: 25 cm tiles), alternating two greys like the
    flat checker, with grey grout and a soft bevel."""
    n = 4
    p = pos()
    px, py = sx(p), sy(p)
    X, Y = px * n, py * n
    fx, fy = frac(X), frac(Y)
    ix, iy = floor(X), floor(Y)
    d = mn(mn(fx, 1.0 - fx), mn(fy, 1.0 - fy)) * (SIZE / n)  # pixels to the tile's edge
    face = sstep(1.5, 2.8, d)
    bevel = sstep(1.5, 7.0, d)
    checker = mod(ix + iy, 2.0)
    r1 = cellhash(ix, iy, 1.0)
    mottle = fbm(px, py, 8, 8, 3, 61.0)
    g = grain(px, py, 63.0)
    tile = 0.80 + 0.08 * checker + 0.03 * (r1 - 0.5) + 0.04 * (mottle - 0.5)
    grout = 0.50 + 0.08 * mottle + 0.04 * g
    tone = lerp(grout, tile, face)
    albedo = Gradient(tone, [(0.0, (0.0, 0.0, 0.0)), (1.0, (1.0, 0.995, 0.97))])
    height = 0.1 + 0.03 * g * (1.0 - face) + bevel * (0.8 + 0.01 * mottle)
    rough = lerp(0.92, 0.16 + 0.08 * mottle, face)
    ao = lerp(0.55, 1.0, sstep(0.0, 5.0, d))
    return albedo, height, (ao, rough, c(0.0)), 3.0


def floor_carpet():
    """Dusty-blue loop pile: fine fibre noise over tufts, with soft wear patches (1 m repeat)."""
    p = pos()
    px, py = sx(p), sy(p)
    pile = fbm(px, py, 128, 128, 2, 71.0)
    tuft = vnoise(px, py, 48, 48, 77.0)
    wear = fbm(px, py, 4, 4, 2, 79.0)
    g = grain(px, py, 73.0)
    h = 0.4 * pile + 0.3 * tuft + 0.3 * g
    tone = clamp01(h * (0.85 + 0.3 * wear))
    albedo = Gradient(tone, [(0.0, (0.22, 0.25, 0.33)), (0.5, (0.43, 0.48, 0.58)), (1.0, (0.58, 0.63, 0.73))])
    ao = 0.7 + 0.3 * h
    return albedo, h, (ao, c(0.96), c(0.0)), 2.0


def floor_concrete():
    """Sealed concrete slab: cloudy trowel blotches, light aggregate flecks and tiny pits (2 m repeat)."""
    p = pos()
    px, py = sx(p), sy(p)
    blot = fbm(px, py, 6, 6, 4, 81.0)
    agg = sstep(0.82, 0.95, vnoise(px, py, 192, 192, 87.0))
    pits = sstep(0.985, 1.0, grain(px, py, 89.0))
    g = grain(px, py, 83.0)
    tone = clamp01(0.45 + 0.3 * (blot - 0.5) + 0.12 * agg + 0.06 * g - 0.35 * pits)
    albedo = Gradient(tone, [(0.0, (0.16, 0.15, 0.15)), (0.5, (0.35, 0.33, 0.32)), (1.0, (0.52, 0.50, 0.48))])
    height = 0.5 + 0.15 * blot + 0.08 * agg + 0.04 * g - 0.4 * pits
    rough = 0.58 + 0.18 * blot + 0.08 * g
    ao = 1.0 - 0.45 * pits
    return albedo, height, (ao, rough, c(0.0)), 1.5


def wood():
    """Furniture wood: continuous grain along image x, nearly neutral because parts tint it with
    their colour (1 m repeat). Kept light so tinted parts land close to their flat colour."""
    p = pos()
    px, py = sx(p), sy(p)
    streak = fbm(px, py, 2, 32, 3, 91.0)
    fine = fbm(px, py, 4, 192, 2, 93.0)
    figure = sin((py * 48.0 + streak * 1.2) * (2.0 * math.pi)) * 0.5 + 0.5
    tone = clamp01(0.4 * sstep(0.2, 0.8, streak) + 0.25 * figure * figure + 0.35 * fine)
    albedo = Gradient(tone, [(0.0, (0.66, 0.62, 0.58)), (0.5, (0.84, 0.81, 0.77)), (1.0, (0.97, 0.95, 0.92))])
    height = 0.5 + 0.12 * streak + 0.1 * fine - 0.08 * (1.0 - figure)
    rough = 0.48 + 0.14 * fine + 0.12 * (1.0 - streak)
    return albedo, height, (c(1.0), rough, c(0.0)), 0.8


def fabric():
    """Upholstery plain weave, 64 × 64 threads per repeat (0.5 m: ~8 mm threads), neutral so parts
    tint it, with a per-thread shade so it reads as woven yarn."""
    n = 64
    p = pos()
    px, py = sx(p), sy(p)
    X, Y = px * n, py * n
    fx, fy = frac(X), frac(Y)
    ix, iy = floor(X), floor(Y)
    over = mod(ix + iy, 2.0)  # 1: the warp (running along y) is on top in this cell
    ax, ay = sin(fx * math.pi), sin(fy * math.pi)  # cross-section bumps
    warp = ax * (0.55 + 0.45 * ay)
    weft = ay * (0.55 + 0.45 * ax)
    h = lerp(weft, warp, over)
    shade = lerp(hash1(iy, 1.0, 101.0), hash1(ix, 2.0, 103.0), over)
    g = grain(px, py, 107.0)
    tone = clamp01(0.25 + 0.55 * h + 0.12 * shade + 0.08 * g)
    albedo = Gradient(tone, [(0.0, (0.55, 0.55, 0.55)), (1.0, (1.0, 1.0, 1.0))])
    height = 0.8 * h + 0.1 * g
    ao = 0.55 + 0.45 * h
    return albedo, height, (ao, c(0.93), c(0.0)), 2.5


def floor_marble():
    """Polished marble slabs, 2 × 2 per repeat (1.2 m: 60 cm slabs): white with soft grey cloud and
    turbulent diagonal veins whose phase shifts per slab, hairline grout."""
    n = 2
    p = pos()
    px, py = sx(p), sy(p)
    X, Y = px * n, py * n
    fx, fy = frac(X), frac(Y)
    ix, iy = floor(X), floor(Y)
    d = mn(mn(fx, 1.0 - fx), mn(fy, 1.0 - fy)) * (SIZE / n)
    face = sstep(0.8, 1.8, d)
    # integer x/y multipliers keep the vein field periodic over the square
    warp = fbm(px, py, 3, 3, 4, 111.0)
    vein = clamp01(1.0 - absf(sin((px * 2.0 + py * 3.0 + warp * 1.7 + hash1(ix, iy, 113.0)) * (2.0 * math.pi))) * 6.0)
    thread = clamp01(1.0 - absf(sin((px * 5.0 - py * 2.0 + vnoise(px, py, 5, 5, 119.0) * 2.5) * (2.0 * math.pi))) * 14.0)
    cloud = vnoise(px, py, 6, 6, 117.0)
    tone = lerp(0.5, clamp01(0.86 + 0.1 * cloud - 0.45 * vein - 0.15 * thread), face)
    albedo = Gradient(tone, [(0.0, (0.30, 0.31, 0.33)), (0.5, (0.62, 0.62, 0.63)), (1.0, (0.95, 0.94, 0.92))])
    height = 0.2 + face * 0.6
    rough = lerp(0.9, 0.1 + 0.06 * cloud, face)
    ao = lerp(0.6, 1.0, sstep(0.0, 3.0, d))
    return albedo, height, (ao, rough, c(0.0)), 2.0


def floor_stone():
    """Sandstone pavers in staggered rows, 3 per row × 5 rows per repeat (2 m: ~67 × 40 cm), with
    wide mortar joints, chipped edges and a bumpy cleft face."""
    cols, rows = 3, 5
    p = pos()
    px, py = sx(p), sy(p)
    row = floor(py * rows)
    X = px * cols + hash1(row, 0.0, 121.0)
    fx, fy = frac(X), frac(py * rows)
    cx = mod(floor(X), float(cols))
    chip = vnoise(px, py, 40, 40, 123.0)
    d = mn(mn(fx, 1.0 - fx) * (SIZE / cols), mn(fy, 1.0 - fy) * (SIZE / rows)) + (chip - 0.5) * 5.0
    face = sstep(3.0, 4.5, d)
    r1 = hash1(cx, row, 125.0)
    cleft = fbm(px, py, 10, 10, 3, 127.0)
    g = grain(px, py, 129.0)
    tone = lerp(0.18 + 0.1 * g, clamp01(0.35 + 0.45 * r1 + 0.25 * (cleft - 0.5) + 0.08 * g), face)
    albedo = Gradient(tone, [(0.0, (0.30, 0.29, 0.27)), (0.35, (0.55, 0.50, 0.43)), (0.7, (0.72, 0.65, 0.54)), (1.0, (0.82, 0.77, 0.68))])
    height = 0.1 + 0.05 * g + sstep(3.0, 14.0, d) * (0.5 + 0.3 * cleft)
    rough = lerp(0.97, 0.8 + 0.1 * cleft, face)
    ao = lerp(0.5, 1.0, sstep(2.0, 10.0, d))
    return albedo, height, (ao, rough, c(0.0)), 4.0


def floor_parquet():
    """Basketweave parquet: 4 × 4 squares per repeat (1 m: 25 cm), each three slats that alternate
    direction like a checkerboard, honey oak with per-slat shade."""
    n = 4
    p = pos()
    px, py = sx(p), sy(p)
    X, Y = px * n, py * n
    fx, fy = frac(X), frac(Y)
    ix, iy = floor(X), floor(Y)
    chk = mod(ix + iy, 2.0)  # 0: slats run along x, 1: along y
    across = lerp(fy, fx, chk) * 3.0
    along = lerp(fx, fy, chk)
    a = frac(across)
    si = floor(across)
    d = mn(mn(a, 1.0 - a) * (SIZE / n / 3), mn(along, 1.0 - along) * (SIZE / n))
    open_ = sstep(0.5, 1.5, d)
    r1 = hash1(ix * 3.0 + si, iy, 131.0)
    # grain stretched along the slat: one noise per orientation, picked by the checker
    streak = lerp(fbm(px, py, 4, 96, 2, 133.0), fbm(px, py, 96, 4, 2, 137.0), chk)
    tone = open_ * (0.25 + 0.75 * clamp01(0.45 * r1 + 0.55 * streak))
    albedo = Gradient(tone, [(0.0, (0.13, 0.08, 0.05)), (0.25, (0.40, 0.26, 0.13)), (0.6, (0.60, 0.42, 0.23)), (1.0, (0.76, 0.58, 0.36))])
    height = open_ * 0.8 + 0.1
    rough = lerp(0.9, 0.38 + 0.12 * r1, open_)
    ao = lerp(0.5, 1.0, sstep(0.0, 3.0, d))
    return albedo, height, (ao, rough, c(0.0)), 2.5


def wall_tile():
    """Glossy subway tile in running bond, 6 × 12 per repeat (0.9 m: 15 × 7.5 cm), near-white so
    walls tint it, with rounded edges and pale grout."""
    cols, rows = 6, 12
    p = pos()
    px, py = sx(p), sy(p)
    row = floor(py * rows)
    X = px * cols + mod(row, 2.0) * 0.5
    fx, fy = frac(X), frac(py * rows)
    d = mn(mn(fx, 1.0 - fx) * (SIZE / cols), mn(fy, 1.0 - fy) * (SIZE / rows))
    face = sstep(1.0, 2.0, d)
    r1 = hash1(mod(floor(X), float(cols)), row, 141.0)
    ripple = vnoise(px, py, 24, 48, 143.0)
    tone = lerp(0.72, 0.93 + 0.04 * r1 + 0.03 * ripple, face)
    albedo = Gradient(tone, [(0.0, (0.0, 0.0, 0.0)), (1.0, (1.0, 1.0, 0.99))])
    height = 0.1 + sstep(1.0, 8.0, d) * (0.8 + 0.03 * ripple)
    rough = lerp(0.9, 0.06 + 0.04 * ripple, face)
    ao = lerp(0.55, 1.0, sstep(0.0, 5.0, d))
    return albedo, height, (ao, rough, c(0.0)), 4.0


def wall_wallpaper():
    """Striped wallpaper: 8 alternating stripes per repeat (1 m: 12.5 cm) with pinstripes at the
    seams, on a fibrous matte paper; neutral so walls tint it."""
    n = 8
    p = pos()
    px, py = sx(p), sy(p)
    X = px * n
    fx = frac(X)
    band = mod(floor(X), 2.0)
    edge = mn(fx, 1.0 - fx) * (SIZE / n)  # pixels to the stripe seam
    pin = 1.0 - sstep(1.5, 2.5, absf(edge - 5.0))
    fibre = fbm(px, py, 64, 160, 2, 151.0)
    g = grain(px, py, 153.0)
    tone = clamp01(0.94 - 0.13 * band - 0.22 * pin + 0.05 * (fibre - 0.5) + 0.02 * g)
    albedo = Gradient(tone, [(0.0, (0.0, 0.0, 0.0)), (1.0, (1.0, 0.99, 0.97))])
    height = 0.5 + 0.3 * fibre + 0.1 * g + 0.1 * pin
    ao = 0.95 + 0.05 * fibre
    return albedo, height, (ao, c(0.88), c(0.0)), 0.6


def wall_panel():
    """Painted beadboard: 10 vertical boards per repeat (1 m: 10 cm) with a V-groove and bead at
    each joint and faint wood grain under the semi-gloss paint; neutral so walls tint it."""
    n = 10
    p = pos()
    px, py = sx(p), sy(p)
    fx = frac(px * n)
    e = mn(fx, 1.0 - fx) * (SIZE / n)  # pixels to the joint
    groove = sstep(0.5, 3.0, e)
    bead = 1.0 - sstep(0.0, 3.0, absf(fx * (SIZE / n) - 9.0))  # half-round bead beside the joint
    under = fbm(px, py, 40, 3, 2, 161.0)
    tone = 0.62 + 0.3 * groove + 0.04 * bead + 0.03 * under
    albedo = Gradient(tone, [(0.0, (0.0, 0.0, 0.0)), (1.0, (1.0, 1.0, 1.0))])
    height = 0.1 + 0.7 * groove + 0.15 * bead + 0.04 * under
    rough = 0.45 + 0.08 * under
    ao = lerp(0.6, 1.0, sstep(0.0, 4.0, e))
    return albedo, height, (ao, rough, c(0.0)), 3.0


def metal():
    """Brushed metal: fine streaks along image x, light and neutral so parts tint it (metallic in
    the ORM, so the part's colour sets the reflectance)."""
    p = pos()
    px, py = sx(p), sy(p)
    brush = fbm(px, py, 4, 256, 2, 171.0)
    g = grain(px, py, 173.0)
    smudge = vnoise(px, py, 3, 3, 177.0)
    tone = clamp01(0.55 * brush + 0.25 * g + 0.2 * smudge)
    albedo = Gradient(tone, [(0.0, (0.86, 0.86, 0.86)), (1.0, (1.0, 1.0, 1.0))])
    height = 0.5 + 0.2 * brush + 0.05 * g
    rough = 0.26 + 0.12 * brush + 0.03 * smudge
    return albedo, height, (c(1.0), rough, c(1.0)), 0.4


def stone():
    """Polished granite for countertops: speckles of dark and light mineral over a mid ground,
    near-neutral so parts tint it (0.6 m repeat)."""
    p = pos()
    px, py = sx(p), sy(p)
    dark = sstep(0.62, 0.75, vnoise(px, py, 96, 96, 181.0))
    light = sstep(0.7, 0.8, vnoise(px, py, 160, 160, 183.0))
    cloud = vnoise(px, py, 8, 8, 187.0)
    tone = clamp01(0.68 + 0.1 * cloud - 0.45 * dark + 0.25 * light)
    albedo = Gradient(tone, [(0.0, (0.42, 0.41, 0.40)), (0.6, (0.80, 0.79, 0.77)), (1.0, (1.0, 1.0, 0.99))])
    height = 0.5 + 0.05 * cloud
    rough = 0.14 + 0.06 * cloud
    return albedo, height, (c(1.0), rough, c(0.0)), 0.3


def skin():
    """Human skin detail for peons (8 cm repeat, box-projected in the body's bind space): scattered
    pores, the criss-cross microrelief furrows and faint mottling. Near-white so the skin tint is
    the colour; the game lights it with subsurface scattering."""
    n = 40  # pore cells per repeat: ~2 mm apart
    p = pos()
    px, py = sx(p), sy(p)
    X, Y = px * n, py * n
    ix, iy = floor(X), floor(Y)
    # one pore per cell, jittered in its middle half (so no neighbour lookups), varied size
    ox = 0.25 + 0.5 * hash1(ix, iy, 211.0)
    oy = 0.25 + 0.5 * hash1(ix, iy, 223.0)
    dx, dy = frac(X) - ox, frac(Y) - oy
    r = 0.09 + 0.1 * hash1(ix, iy, 227.0)
    pore = 1.0 - sstep(0.0, r * r, dx * dx + dy * dy)
    # microrelief: two families of shallow furrows crossing at an angle, wobbled by noise
    wob = vnoise(px, py, 12, 12, 229.0)
    f1 = absf(sin(math.pi * (px * 22.0 + py * 14.0 + 1.4 * wob)))
    f2 = absf(sin(math.pi * (px * 16.0 - py * 24.0 + 1.4 * wob)))
    furrow = 1.0 - mn(f1, f2)
    furrow = furrow * furrow * furrow * furrow
    mottle = vnoise(px, py, 6, 6, 233.0)
    g = grain(px, py, 239.0)
    height = clamp01(0.62 - 0.4 * pore - 0.12 * furrow + 0.08 * g)
    tone = 0.95 + 0.05 * mottle - 0.04 * pore - 0.02 * furrow
    albedo = (tone, tone * (0.985 + 0.015 * mottle), tone * (0.975 + 0.025 * mottle))
    rough = 0.52 + 0.14 * pore + 0.08 * furrow + 0.06 * (1.0 - mottle)
    ao = 1.0 - 0.25 * pore - 0.1 * furrow
    return albedo, height, (ao, rough, c(0.0)), 0.32


MATERIALS = {
    "skin": skin,
    "wall_brick": wall_brick,
    "wall_plaster": wall_plaster,
    "wall_tile": wall_tile,
    "wall_wallpaper": wall_wallpaper,
    "wall_panel": wall_panel,
    "floor_wood": floor_wood,
    "floor_tile": floor_tile,
    "floor_carpet": floor_carpet,
    "floor_concrete": floor_concrete,
    "floor_marble": floor_marble,
    "floor_stone": floor_stone,
    "floor_parquet": floor_parquet,
    "wood": wood,
    "fabric": fabric,
    "metal": metal,
    "stone": stone,
}


# ------------------------------------------------------------------------------------------ building


def lift(x):
    return x if isinstance(x, E) else c(x)


class Gradient:
    """Albedo as a grey `tone` expression coloured by a Gradient Map node: [(pos, (r, g, b)), ...] in sRGB."""

    def __init__(self, tone, keys):
        self.tone, self.keys = tone, keys


def steps(albedo, height, orm, normal_intensity):
    pp = lambda color, expr: {"id": "graph.node.add", "params": {"def": "pixel_processor", "params": {"color_mode": color, "function": graph(expr)}}}
    out = lambda ident, usage: {"id": "graph.node.add", "params": {"def": "output", "params": {"identifier": ident, "usage": usage}}}
    conn = lambda a, b: {"id": "graph.connect", "params": {"from": a, "to": b}}
    one = c(1.0)
    s = [
        pp(False, lift(height)),  # 1
        {"id": "graph.node.add", "params": {"def": "normal", "params": {"intensity": normal_intensity, "opengl": True}}},  # 2
        pp(True, v4(*map(lift, orm), one)),  # 3
        out("albedo", 0),  # 4 base colour
        out("normal", 1),  # 5
        out("orm", 9),  # 6 any
        out("height", 4),  # 7
        conn(1, 2),
        conn(2, 5),
        conn(3, 6),
        conn(1, 7),
    ]
    if isinstance(albedo, Gradient):
        # graph colours are linear (the base colour output is sRGB-encoded on export)
        keys = [{"pos": p, "color": [*(x**2.2 for x in rgb_), 1.0], "midpoint": 0.5} for p, rgb_ in albedo.keys]
        s += [
            pp(False, lift(albedo.tone)),  # 8
            {"id": "graph.node.add", "params": {"def": "gradient_map", "params": {"color_mode": True, "gradient": {"t": "gradient", "v": {"keys": keys}}}}},  # 9
            conn(8, 9),
            conn(9, 4),
        ]
    else:
        linear = [E("pow", [lift(ch), c(2.2)]) for ch in albedo]  # authored in sRGB; graph colours are linear
        s += [pp(True, v4(*linear, one)), conn(8, 4)]  # 8
    return s


def cli(*args):
    r = subprocess.run(["materialcraft-cli", *map(str, args)], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"materialcraft-cli {' '.join(map(str, args))} failed:\n{r.stdout}{r.stderr}")
    return r.stdout


def build(name, preview_dir=None):
    sbs = HERE / f"{name}.sbs"
    cli("new", "empty", sbs, "--size", SIZE, "--name", name)
    with tempfile.TemporaryDirectory() as tmp:
        script = Path(tmp) / "build.json"
        script.write_text(json.dumps({"open": str(sbs), "steps": steps(*MATERIALS[name]()), "save": str(sbs)}))
        result = json.loads(cli("batch", script))
        if result.get("failed"):
            sys.exit(f"{name}: {result['failed']}")
        # the export writes magenta for nodes that failed to cook, so check first
        errors = [f"node {n['id']} ({n['def']}): {n['error']}" for n in json.loads(cli("cook", sbs, "--full"))["nodes"] if n["error"]]
        if errors:
            sys.exit(f"{name}: cook failed:\n  " + "\n  ".join(errors))
        cli("export", sbs, tmp, "--depth", 8, "--pattern", "$(identifier)")
        dest = TEXTURES / name
        dest.mkdir(parents=True, exist_ok=True)
        for m in ["albedo", "normal", "orm"]:
            shutil.copy(Path(tmp) / f"{m}.png", dest / f"{m}.png")
        if preview_dir:
            shutil.copy(Path(tmp) / "height.png", Path(preview_dir) / f"{name}_height.png")
    print(f"{name}: {sbs.relative_to(HERE.parent.parent)} -> {dest.relative_to(HERE.parent.parent)}/")


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--preview=")]
    preview = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--preview=")), None)
    for name in args or MATERIALS:
        if name not in MATERIALS:
            sys.exit(f"unknown material {name}; have {', '.join(MATERIALS)}")
        build(name, preview)
