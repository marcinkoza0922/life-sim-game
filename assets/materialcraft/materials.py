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

from fngraph import E, c, pos, sx, sy, v4, floor, frac, sin, mn, mx, mod, lerp, clamp01, hash1, cellhash, graph

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


MATERIALS = {"wall_brick": wall_brick, "wall_plaster": wall_plaster, "floor_wood": floor_wood}


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
