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
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) light_clip: vec4<f32>,
};

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
    return o;
}

@vertex
fn vs_shadow(v: VsIn) -> @builtin(position) vec4<f32> {
    let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    return g.light_view_proj * m * vec4<f32>(v.pos, 1.0);
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

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let flags = u32(i.params.w + 0.5);
    let albedo = i.color.rgb;
    if ((flags & 1u) != 0u) {
        // unlit (selection rings, overlays)
        return vec4<f32>(albedo, i.color.a);
    }
    var n = normalize(i.normal);
    if (!front) {
        n = -n;
    }
    let v = normalize(g.eye.xyz - i.world);
    let l = normalize(g.sun_dir.xyz);
    let rough = clamp(i.params.x, 0.04, 1.0);
    let metal = clamp(i.params.y, 0.0, 1.0);
    let ndotl = max(dot(n, l), 0.0);
    var sh = 1.0;
    if ((flags & 2u) == 0u) {
        sh = shadow_factor(i.light_clip, ndotl);
    }
    var color = shade(n, v, l, albedo, rough, metal) * g.sun_color.rgb * g.sun_dir.w * sh;
    // hemispheric ambient
    let hemi = mix(g.ground_color.rgb, g.sky_color.rgb, n.y * 0.5 + 0.5);
    color += hemi * albedo * (1.0 - metal * 0.6);
    // point lights
    let count = i32(g.counts.x);
    for (var k = 0; k < count; k++) {
        let pl = g.points[k];
        let d = pl.pos_radius.xyz - i.world;
        let dist = length(d);
        let r = pl.pos_radius.w;
        if (dist < r) {
            let att = pow(clamp(1.0 - dist / r, 0.0, 1.0), 2.0) / (1.0 + dist * dist);
            color += shade(n, v, d / dist, albedo, rough, metal) * pl.color_intensity.rgb * pl.color_intensity.w * att * 4.0;
        }
    }
    color += albedo * i.params.z;
    // fog
    let fd = length(g.eye.xyz - i.world);
    let fog = 1.0 - exp(-fd * g.fog.a);
    color = mix(color, g.fog.rgb, clamp(fog, 0.0, 1.0));
    return vec4<f32>(aces(color * g.counts.z), i.color.a);
}
