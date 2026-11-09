"""Tiny expression builder for MaterialCraft pixel-processor function graphs.

Write a function with operators on `E` values, then `graph(expr)` returns the
`{"t": "function", "v": {nodes, output}}` parameter value for a `pixel_processor`
node's `function` param. Scalar math only (plus vector2/vector4 construction and
`samplelum`), which is all the material patterns here need.

    python3 materials.py      # prints the functions used by wall_brick/floor_wood/wall_plaster
"""

import json


class E:
    def __init__(self, op, inputs=(), value=None):
        self.op, self.inputs, self.value = op, list(inputs), value

    def __add__(self, o): return E("add", [self, lift(o)])
    def __radd__(self, o): return E("add", [lift(o), self])
    def __sub__(self, o): return E("sub", [self, lift(o)])
    def __rsub__(self, o): return E("sub", [lift(o), self])
    def __mul__(self, o): return E("mul", [self, lift(o)])
    def __rmul__(self, o): return E("mul", [lift(o), self])
    def __truediv__(self, o): return E("div", [self, lift(o)])
    def __rtruediv__(self, o): return E("div", [lift(o), self])


def lift(v):
    return v if isinstance(v, E) else c(v)


def c(x): return E("const_float1", value={"t": "float", "v": float(x)})
def pos(): return E("get_float2", value={"t": "text", "v": "$pos"})
def sx(v): return E("swizzle1", [v], {"t": "int", "v": 0})
def sy(v): return E("swizzle1", [v], {"t": "int", "v": 1})
def v2(a, b): return E("vector2", [lift(a), lift(b)])
def v4(a, b, cc, d): return E("vector4", [v2(a, b), v2(cc, d)])
def floor(a): return E("floor", [lift(a)])
def frac(a):
    a = lift(a)
    return a - floor(a)
def sin(a): return E("sin", [lift(a)])
def absf(a): return E("abs", [lift(a)])
def mn(a, b): return E("min", [lift(a), lift(b)])
def mx(a, b): return E("max", [lift(a), lift(b)])
def mod(a, b): return E("mod", [lift(a), lift(b)])
def lerp(a, b, t): return E("lerp", [lift(a), lift(b), lift(t)])
def clamp01(a): return mn(mx(a, 0.0), 1.0)
def samplelum(p, image=0): return E("samplelum", [p], {"t": "int", "v": image})


def hash1(a, b, seed=0.0):
    """Pseudo-random 0..1 from two integer-valued scalars."""
    return frac(sin(a * 12.9898 + b * 78.233 + seed) * 43758.5453)


def cellhash(a, b, seed=0.0):
    """Like `hash1` but decorrelated for small neighbouring integers (one value per brick or plank:
    `hash1` alone lines those up in diagonal streaks). Twice the cost, so not for noise lattices."""
    return hash1(hash1(a, b, seed) * 157.31 + a, b * 1.618, seed + 3.7)


def graph(expr):
    nodes, ids = [], {}

    def visit(e):
        if id(e) in ids:
            return ids[id(e)]
        ins = [visit(i) for i in e.inputs]
        nid = len(nodes) + 1
        n = {"id": nid, "op": e.op, "inputs": ins, "pos": [140.0 * nid, 0.0], "type_code": 0}
        if e.value is not None:
            n["value"] = e.value
        nodes.append(n)
        ids[id(e)] = nid
        return nid

    out = visit(expr)
    return {"t": "function", "v": {"nodes": nodes, "output": out}}


def dumps(expr):
    return json.dumps(graph(expr), separators=(",", ":"))
