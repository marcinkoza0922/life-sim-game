struct PointLight {
    pos_radius: vec4<f32>,
    color_intensity: vec4<f32>,
};

struct Globals {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    sun_dir: vec4<f32>,      // xyz: direction towards the sun, w: intensity
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    ground_color: vec4<f32>,
    fog: vec4<f32>,          // rgb colour, a: density
    counts: vec4<f32>,       // x: point light count, y: shadow texel size, z: exposure
    points: array<PointLight, 16>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_tex: texture_depth_2d;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
// Material layers (0 = flat); see `MaterialMaps`.
@group(0) @binding(3) var mat_albedo: texture_2d_array<f32>;
@group(0) @binding(4) var mat_normal: texture_2d_array<f32>;
@group(0) @binding(5) var mat_orm: texture_2d_array<f32>;
@group(0) @binding(6) var mat_samp: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) icolor: vec4<f32>,
    @location(8) params: vec4<f32>,  // roughness, metallic, emissive, flags
    @location(9) tex: vec4<f32>,     // material layer, 1 / texture scale
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) light_clip: vec4<f32>,
    // triplanar mapping: position in scaled object space (rotation and translation removed), in
    // texture repeats, and the model rotation that takes that space back to world
    @location(5) tex_pos: vec3<f32>,
    @location(6) @interpolate(flat) layer: u32,
    @location(7) @interpolate(flat) r0: vec3<f32>,
    @location(8) @interpolate(flat) r1: vec3<f32>,
    @location(9) @interpolate(flat) r2: vec3<f32>,
    // skin (flag SKIN): how thin the flesh is, for light shining through
    @location(10) thin: f32,
};

const FLAG_SKIN: u32 = 16u;

// tex: layer, 1 / scale, world-space flag
fn set_triplanar(o: ptr<function, VsOut>, m: mat4x4<f32>, pos: vec3<f32>, tex: vec4<f32>) {
    (*o).layer = u32(tex.x + 0.5);
    if (tex.z > 0.5) {
        (*o).tex_pos = (m * vec4<f32>(pos, 1.0)).xyz * tex.y;
        (*o).r0 = vec3<f32>(1.0, 0.0, 0.0);
        (*o).r1 = vec3<f32>(0.0, 1.0, 0.0);
        (*o).r2 = vec3<f32>(0.0, 0.0, 1.0);
        return;
    }
    let s = vec3<f32>(length(m[0].xyz), length(m[1].xyz), length(m[2].xyz));
    let inv = 1.0 / max(s, vec3<f32>(1e-6));
    (*o).tex_pos = pos * s * tex.y;
    (*o).r0 = m[0].xyz * inv.x;
    (*o).r1 = m[1].xyz * inv.y;
    (*o).r2 = m[2].xyz * inv.z;
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let world = m * vec4<f32>(v.pos, 1.0);
    var o: VsOut;
    o.clip = g.view_proj * world;
    o.world = world.xyz;
    o.normal = normalize((m * vec4<f32>(v.normal, 0.0)).xyz);
    // material colours are authored in sRGB; light in linear space
    let c = v.color * v.icolor;
    o.color = vec4<f32>(pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(2.2)), c.a);
    o.params = v.params;
    o.light_clip = g.light_view_proj * world;
    set_triplanar(&o, m, v.pos, v.tex);
    return o;
}

@vertex
fn vs_shadow(v: VsIn) -> @builtin(position) vec4<f32> {
    let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    return g.light_view_proj * m * vec4<f32>(v.pos, 1.0);
}

// ---------------------------------------------------------------------------------------- skinned

// Joint matrices (4 vec4 columns each), per-draw palettes and morph weights, indexed by the
// instance's `extra`.
@group(1) @binding(0) var<storage, read> skin: array<vec4<f32>>;
// Sparse morph deltas of every uploaded skinned mesh: xyz = position delta, w = target index.
// A vertex's entries are `morphs[morph.x .. morph.x + morph.y]`.
@group(1) @binding(1) var<storage, read> morphs: array<vec4<f32>>;

struct SkIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
    @location(5) morph: vec2<u32>,
    @location(7) slot: u32,  // bits 0..8 palette slot, 8..16 skin region, 16..24 flesh thinness
    @location(6) tone: vec4<f32>,  // rgb multiplies after `a` mixes towards PALE_SKIN
    @location(8) m0: vec4<f32>,
    @location(9) m1: vec4<f32>,
    @location(10) m2: vec4<f32>,
    @location(11) m3: vec4<f32>,
    @location(12) icolor: vec4<f32>,
    @location(13) params: vec4<f32>,
    @location(14) extra: vec4<f32>,  // joint base, palette base, morph weight base, hidden regions
    @location(15) skinp: vec4<f32>,  // skin surface: palette slot (-1 none), material layer, 1 / scale
};

fn joint_mat(i: u32) -> mat4x4<f32> {
    return mat4x4<f32>(skin[i], skin[i + 1u], skin[i + 2u], skin[i + 3u]);
}

// model × skinning matrix
fn skinned_model(v: SkIn) -> mat4x4<f32> {
    let base = u32(v.extra.x + 0.5);
    let w = v.weights / max(v.weights.x + v.weights.y + v.weights.z + v.weights.w, 1e-6);
    let sk = joint_mat(base + v.joints.x * 4u) * w.x
        + joint_mat(base + v.joints.y * 4u) * w.y
        + joint_mat(base + v.joints.z * 4u) * w.z
        + joint_mat(base + v.joints.w * 4u) * w.w;
    return mat4x4<f32>(v.m0, v.m1, v.m2, v.m3) * sk;
}

fn morphed(v: SkIn) -> vec3<f32> {
    var p = v.pos;
    let wbase = u32(v.extra.z + 0.5);
    for (var i = v.morph.x; i < v.morph.x + v.morph.y; i++) {
        let e = morphs[i];
        let k = u32(e.w + 0.5);
        p += e.xyz * skin[wbase + k / 4u][k % 4u];
    }
    return p;
}

// Skin regions covered by clothes are collapsed out of the way (outside the clip volume).
fn hidden(v: SkIn) -> bool {
    let region = (v.slot >> 8u) & 255u;
    return region > 0u && ((u32(v.extra.w + 0.5) >> region) & 1u) == 1u;
}

// sRGB tone palms, soles and nails lean towards (vertex tone alpha)
const PALE_SKIN: vec3<f32> = vec3<f32>(0.93, 0.8, 0.74);

@vertex
fn vs_skinned(v: SkIn) -> VsOut {
    let m = skinned_model(v);
    let world = m * vec4<f32>(morphed(v), 1.0);
    var o: VsOut;
    o.clip = g.view_proj * world;
    o.world = world.xyz;
    o.normal = normalize((m * vec4<f32>(v.normal, 0.0)).xyz);
    if (hidden(v)) {
        o.clip = vec4<f32>(0.0, 0.0, -2.0, 1.0);
    }
    let tint = skin[u32(v.extra.y + 0.5) + (v.slot & 255u)];
    var c = v.color * v.icolor * tint;
    // palms, soles and nails lean towards a pale tone whatever the skin colour
    c = vec4<f32>(mix(c.rgb, PALE_SKIN, v.tone.a) * v.tone.rgb, c.a);
    o.color = vec4<f32>(pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(2.2)), c.a);
    o.params = v.params;
    o.light_clip = g.light_view_proj * world;
    o.thin = f32((v.slot >> 16u) & 255u) / 255.0;
    let is_skin = v.skinp.x >= 0.0 && (v.slot & 255u) == u32(v.skinp.x + 0.5);
    if (is_skin) {
        o.params.w = f32(u32(v.params.w + 0.5) | FLAG_SKIN);
        // detail material in bind space (the rest-pose mesh), rotated along with the skinning
        o.layer = u32(v.skinp.y + 0.5);
        o.tex_pos = v.pos * v.skinp.z;
        let r = mat3x3<f32>(normalize(m[0].xyz), normalize(m[1].xyz), normalize(m[2].xyz));
        o.r0 = r[0];
        o.r1 = r[1];
        o.r2 = r[2];
    } else {
        set_triplanar(&o, mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0)), vec3<f32>(0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0));
    }
    return o;
}

@vertex
fn vs_shadow_skinned(v: SkIn) -> @builtin(position) vec4<f32> {
    if (hidden(v)) {
        return vec4<f32>(0.0, 0.0, -2.0, 1.0);
    }
    return g.light_view_proj * skinned_model(v) * vec4<f32>(morphed(v), 1.0);
}

const PI: f32 = 3.14159265;

fn shadow_factor(lc: vec4<f32>, ndotl: f32) -> f32 {
    let p = lc.xyz / lc.w;
    let uv = vec2<f32>(p.x * 0.5 + 0.5, -p.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || p.z > 1.0) {
        return 1.0;
    }
    let bias = max(0.0025 * (1.0 - ndotl), 0.0006);
    let t = g.counts.y;
    var sum = 0.0;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            sum += textureSampleCompareLevel(shadow_tex, shadow_samp, uv + vec2<f32>(f32(x), f32(y)) * t, p.z - bias);
        }
    }
    return sum / 9.0;
}

fn ggx(n: vec3<f32>, h: vec3<f32>, rough: f32) -> f32 {
    let a = rough * rough;
    let a2 = a * a;
    let nh = max(dot(n, h), 0.0);
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn shade(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, rough: f32, metal: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let nl = max(dot(n, l), 0.0);
    let nv = max(dot(n, v), 0.001);
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let f = f0 + (1.0 - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let gv = nv / (nv * (1.0 - k) + k);
    let gl = nl / (nl * (1.0 - k) + k);
    let spec = ggx(n, h, rough) * gv * gl * f / max(4.0 * nv * nl, 0.001);
    let kd = (1.0 - f) * (1.0 - metal);
    return (kd * albedo / PI + spec) * nl;
}

// Skin: diffuse wrapped further per channel (red light scatters deepest under the skin, so the
// terminator goes soft and warm), shadows bleeding red at their edges, and light shining through
// thin flesh (ears, fingers) from behind. Specular stays the plain GGX lobe.
fn shade_skin(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, rough: f32, sh: f32, thin: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let nl_raw = dot(n, l);
    let nl = max(nl_raw, 0.0);
    let nv = max(dot(n, v), 0.001);
    let wrap = vec3<f32>(0.42, 0.2, 0.13);
    let diff = max((vec3<f32>(nl_raw) + wrap) / ((1.0 + wrap) * (1.0 + wrap)), vec3<f32>(0.0));
    let shc = pow(vec3<f32>(sh), vec3<f32>(0.55, 0.95, 1.1));
    let f = 0.028 + 0.972 * pow(1.0 - max(dot(h, v), 0.0), 5.0);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let gv = nv / (nv * (1.0 - k) + k);
    let gl = nl / (nl * (1.0 - k) + k);
    let spec = ggx(n, h, rough) * gv * gl * f / max(4.0 * nv * nl, 0.001) * nl;
    let back = pow(clamp(dot(v, -normalize(l + n * 0.35)), 0.0, 1.0), 3.0);
    let trans = back * (0.06 + 0.6 * thin) * vec3<f32>(1.0, 0.32, 0.18) * sqrt(max(albedo, vec3<f32>(0.0)));
    return (albedo / PI * diff * (1.0 - f) + trans) * shc + spec * sh;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

struct Surface {
    albedo: vec3<f32>,
    // object-space normal (scaled object space, see VsOut)
    normal: vec3<f32>,
    orm: vec3<f32>,
};

// Box-projected material sample. Planes map (u, v) so textures stand upright on vertical faces:
// X faces use (z, -y), Y faces (x, z), Z faces (x, -y). Normal-map tangents follow the same axes
// (green = up the image), added to the geometric normal per plane and blended (UDN).
fn triplanar(p: vec3<f32>, n: vec3<f32>, layer: u32) -> Surface {
    var w = pow(abs(n), vec3<f32>(4.0));
    w = w / max(w.x + w.y + w.z, 1e-5);
    let ux = vec2<f32>(p.z, -p.y);
    let uy = vec2<f32>(p.x, p.z);
    let uz = vec2<f32>(p.x, -p.y);
    var s: Surface;
    s.albedo = textureSample(mat_albedo, mat_samp, ux, layer).rgb * w.x
        + textureSample(mat_albedo, mat_samp, uy, layer).rgb * w.y
        + textureSample(mat_albedo, mat_samp, uz, layer).rgb * w.z;
    s.orm = textureSample(mat_orm, mat_samp, ux, layer).rgb * w.x
        + textureSample(mat_orm, mat_samp, uy, layer).rgb * w.y
        + textureSample(mat_orm, mat_samp, uz, layer).rgb * w.z;
    let tx = textureSample(mat_normal, mat_samp, ux, layer).xy * 2.0 - 1.0;
    let ty = textureSample(mat_normal, mat_samp, uy, layer).xy * 2.0 - 1.0;
    let tz = textureSample(mat_normal, mat_samp, uz, layer).xy * 2.0 - 1.0;
    // world direction of +u and of "up the image" (-v) on each plane
    let dx = vec3<f32>(0.0, tx.y, tx.x);
    let dy = vec3<f32>(ty.x, 0.0, -ty.y);
    let dz = vec3<f32>(tz.x, tz.y, 0.0);
    let bump = select(0.0, 1.0, layer > 0u);
    s.normal = normalize(n + (dx * w.x + dy * w.y + dz * w.z) * bump);
    return s;
}

@fragment
fn fs_main(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let flags = u32(i.params.w + 0.5);
    var n = normalize(i.normal);
    if (!front) {
        n = -n;
    }
    // sample before any early return (implicit-derivative sampling needs uniform control flow)
    let rot = mat3x3<f32>(i.r0, i.r1, i.r2);
    let surf = triplanar(i.tex_pos, transpose(rot) * n, i.layer);
    let albedo = i.color.rgb * surf.albedo;
    if ((flags & 1u) != 0u) {
        // unlit (selection rings, overlays)
        return vec4<f32>(albedo, i.color.a);
    }
    let textured = i.layer > 0u;
    n = normalize(rot * surf.normal);
    let v = normalize(g.eye.xyz - i.world);
    let l = normalize(g.sun_dir.xyz);
    let rough = clamp(select(i.params.x, surf.orm.g, textured), 0.04, 1.0);
    let metal = clamp(select(i.params.y, surf.orm.b, textured), 0.0, 1.0);
    let ao = surf.orm.r;
    let ndotl = max(dot(n, l), 0.0);
    var sh = 1.0;
    if ((flags & 2u) == 0u) {
        sh = shadow_factor(i.light_clip, ndotl);
    }
    let is_skin = (flags & FLAG_SKIN) != 0u;
    var color: vec3<f32>;
    if (is_skin) {
        color = shade_skin(n, v, l, albedo, rough, sh, i.thin) * g.sun_color.rgb * g.sun_dir.w;
    } else {
        color = shade(n, v, l, albedo, rough, metal) * g.sun_color.rgb * g.sun_dir.w * sh;
    }
    // hemispheric ambient
    let hemi = mix(g.ground_color.rgb, g.sky_color.rgb, n.y * 0.5 + 0.5);
    color += hemi * albedo * (1.0 - metal * 0.6) * ao;
    // point lights
    let count = i32(g.counts.x);
    for (var k = 0; k < count; k++) {
        let pl = g.points[k];
        let d = pl.pos_radius.xyz - i.world;
        let dist = length(d);
        let r = pl.pos_radius.w;
        if (dist < r) {
            let att = pow(clamp(1.0 - dist / r, 0.0, 1.0), 2.0) / (1.0 + dist * dist);
            var lit: vec3<f32>;
            if (is_skin) {
                lit = shade_skin(n, v, d / dist, albedo, rough, 1.0, i.thin);
            } else {
                lit = shade(n, v, d / dist, albedo, rough, metal);
            }
            color += lit * pl.color_intensity.rgb * pl.color_intensity.w * att * 4.0;
        }
    }
    color += albedo * i.params.z;
    // fog
    let fd = length(g.eye.xyz - i.world);
    let fog = 1.0 - exp(-fd * g.fog.a);
    color = mix(color, g.fog.rgb, clamp(fog, 0.0, 1.0));
    return vec4<f32>(aces(color * g.counts.z), i.color.a);
}
