// hearth_ui: one instanced quad per shape, shaded by signed distance (rounded boxes, triangles,
// arcs) or sampled from the glyph atlas. Output is premultiplied alpha in gamma space.

struct Globals {
    screen: vec2<f32>, // pixels
    ppp: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Inst {
    @location(0) center: vec2<f32>,
    @location(1) half: vec2<f32>,
    @location(2) rot: f32,
    @location(3) kind: u32,
    @location(4) radius: f32,
    @location(5) border: f32,
    @location(6) fill0: vec4<f32>,
    @location(7) fill1: vec4<f32>,
    @location(8) stroke: vec4<f32>,
    @location(9) params: vec4<f32>,
    @location(10) clip: vec4<f32>,
    @location(11) soft: f32,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) screen: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) kind: u32,
    @location(4) @interpolate(flat) half: vec2<f32>,
    @location(5) @interpolate(flat) shape: vec3<f32>, // radius, border, soft
    @location(6) @interpolate(flat) fill0: vec4<f32>,
    @location(7) @interpolate(flat) fill1: vec4<f32>,
    @location(8) @interpolate(flat) stroke: vec4<f32>,
    @location(9) @interpolate(flat) params: vec4<f32>,
    @location(10) @interpolate(flat) clip: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, i: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    var ext = i.half;
    if (i.kind != 1u) {
        ext = i.half + vec2<f32>(i.soft + 1.5 / g.ppp);
    }
    let local = c * ext;
    let cs = cos(i.rot);
    let sn = sin(i.rot);
    let p = i.center + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let px = p * g.ppp;
    var o: VOut;
    o.pos = vec4<f32>(px.x / g.screen.x * 2.0 - 1.0, 1.0 - px.y / g.screen.y * 2.0, 0.0, 1.0);
    o.local = local;
    o.screen = p;
    o.uv = mix(i.params.xy, i.params.zw, c * 0.5 + vec2<f32>(0.5));
    o.kind = i.kind;
    o.half = i.half;
    o.shape = vec3<f32>(i.radius, i.border, i.soft);
    o.fill0 = i.fill0;
    o.fill1 = i.fill1;
    o.stroke = i.stroke;
    o.params = i.params;
    o.clip = i.clip;
    return o;
}

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Isosceles triangle with its apex at the origin and base at y = q.y (y pointing towards the base).
fn sd_tri(pin: vec2<f32>, q: vec2<f32>) -> f32 {
    let p = vec2<f32>(abs(pin.x), pin.y);
    let a = p - q * clamp(dot(p, q) / dot(q, q), 0.0, 1.0);
    let b = p - q * vec2<f32>(clamp(p.x / q.x, 0.0, 1.0), 1.0);
    let s = -sign(q.y);
    let d = min(vec2<f32>(dot(a, a), s * (p.x * q.y - p.y * q.x)), vec2<f32>(dot(b, b), s * (p.y - q.y)));
    return -sqrt(d.x) * sign(d.y);
}

// Arc symmetric around "up", half aperture `ap`, radius `ra`, half thickness `rb`.
fn sd_arc(pin: vec2<f32>, ap: f32, ra: f32, rb: f32) -> f32 {
    let p = vec2<f32>(abs(pin.x), -pin.y);
    let sc = vec2<f32>(sin(ap), cos(ap));
    var d: f32;
    if (sc.y * p.x > sc.x * p.y) {
        d = length(p - sc * ra);
    } else {
        d = abs(length(p) - ra);
    }
    return d - rb;
}

@fragment
fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    if (v.screen.x < v.clip.x || v.screen.y < v.clip.y || v.screen.x > v.clip.z || v.screen.y > v.clip.w) {
        discard;
    }
    let tex = textureSample(atlas, samp, v.uv).r;
    if (v.kind == 1u) {
        let a = v.fill0.a * tex;
        return vec4<f32>(v.fill0.rgb * a, a);
    }
    let px = 1.0 / g.ppp;
    let radius = v.shape.x;
    let border = v.shape.y;
    let soft = v.shape.z;
    var d: f32;
    if (v.kind == 2u) {
        d = sd_tri(v.local + vec2<f32>(0.0, v.half.y), vec2<f32>(v.half.x, 2.0 * v.half.y)) - radius;
    } else if (v.kind == 3u) {
        d = sd_arc(v.local, v.params.x, v.params.y, v.params.z);
    } else {
        d = sd_round_box(v.local, v.half, radius);
    }
    var cover: f32;
    if (soft > 0.0) {
        cover = 1.0 - smoothstep(-soft, soft, d);
    } else {
        cover = 1.0 - smoothstep(-0.5 * px, 0.5 * px, d);
    }
    let t = clamp(v.local.y / max(v.half.y, 0.001) * 0.5 + 0.5, 0.0, 1.0);
    var col = mix(v.fill0, v.fill1, t);
    if (border > 0.0) {
        let inner = 1.0 - smoothstep(-0.5 * px, 0.5 * px, d + border);
        col = mix(v.stroke, col, inner);
    }
    let a = col.a * cover;
    return vec4<f32>(col.rgb * a, a);
}
