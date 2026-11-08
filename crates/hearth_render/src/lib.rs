//! Hearth renderer: instanced PBR-lite meshes on wgpu with a shadowed sun, point lights,
//! MSAA, ACES tone mapping, a transparent pass, and offscreen capture to RGBA images.
//!
//! The renderer knows nothing about the game. Each frame the game builds a [`Frame`]: a camera,
//! lighting, and a flat list of [`Draw`]s (mesh + transform + material).

pub mod shapes;

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

pub use wgpu;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SAMPLES: u32 = 4;
pub const SHADOW_SIZE: u32 = 2048;
pub const MAX_POINT_LIGHTS: usize = 16;

// ------------------------------------------------------------------------------------------------
// CPU-side data

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Makes every triangle counter-clockwise when seen from the side its vertex normals face.
    pub fn fix_winding(&mut self) {
        for t in self.indices.chunks_exact_mut(3) {
            let p = |i: u32| Vec3::from(self.vertices[i as usize].pos);
            let n = |i: u32| Vec3::from(self.vertices[i as usize].normal);
            let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
            if face.dot(n(t[0]) + n(t[1]) + n(t[2])) < 0.0 {
                t.swap(1, 2);
            }
        }
    }

    /// Appends another mesh transformed by `m` and tinted by `color`.
    pub fn append(&mut self, other: &MeshData, m: Mat4, color: [f32; 4]) {
        let base = self.vertices.len() as u32;
        let nm = m.inverse().transpose();
        for v in &other.vertices {
            let p = m.transform_point3(Vec3::from(v.pos));
            let n = nm.transform_vector3(Vec3::from(v.normal)).normalize_or_zero();
            let c = [v.color[0] * color[0], v.color[1] * color[1], v.color[2] * color[2], v.color[3] * color[3]];
            self.vertices.push(Vertex { pos: p.to_array(), normal: n.to_array(), color: c });
        }
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshId(pub u32);

/// Built-in primitive meshes, always present.
#[derive(Clone, Copy, Debug)]
pub struct Builtins {
    pub cube: MeshId,
    pub cylinder: MeshId,
    pub sphere: MeshId,
    pub disc: MeshId,
    pub wedge: MeshId,
}

pub mod flags {
    /// No lighting: output the colour as-is.
    pub const UNLIT: u32 = 1;
    /// Does not receive shadows.
    pub const NO_SHADOW_RECEIVE: u32 = 2;
    /// Does not cast shadows.
    pub const NO_SHADOW_CAST: u32 = 4;
}

#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub mesh: MeshId,
    pub model: Mat4,
    /// Linear RGB + alpha. Alpha < 1 goes through the transparent pass.
    pub color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub emissive: f32,
    pub flags: u32,
}

impl Draw {
    pub fn new(mesh: MeshId, model: Mat4, color: [f32; 3]) -> Self {
        Draw { mesh, model, color: [color[0], color[1], color[2], 1.0], roughness: 0.6, metallic: 0.0, emissive: 0.0, flags: 0 }
    }
    pub fn rough(mut self, r: f32) -> Self {
        self.roughness = r;
        self
    }
    pub fn metal(mut self, m: f32) -> Self {
        self.metallic = m;
        self
    }
    pub fn emissive(mut self, e: f32) -> Self {
        self.emissive = e;
        self
    }
    pub fn alpha(mut self, a: f32) -> Self {
        self.color[3] = a;
        self
    }
    pub fn flags(mut self, f: u32) -> Self {
        self.flags |= f;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub pos: Vec3,
    pub radius: f32,
    pub color: Vec3,
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub view: Mat4,
    pub proj: Mat4,
    pub eye: Vec3,
}

impl Camera {
    /// Orbit camera: `yaw` around +Y (0 looks toward -Z), `pitch` above the horizon, both in radians.
    pub fn orbit(target: Vec3, yaw: f32, pitch: f32, dist: f32, aspect: f32) -> Camera {
        let dir = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
        let eye = target + dir * dist;
        Camera {
            view: Mat4::look_at_rh(eye, target, Vec3::Y),
            proj: Mat4::perspective_rh(40f32.to_radians(), aspect.max(0.01), 0.1, 400.0),
            eye,
        }
    }
    pub fn view_proj(&self) -> Mat4 {
        self.proj * self.view
    }
    /// World-space ray through normalized device coords (x, y in -1..1).
    pub fn ray(&self, ndc_x: f32, ndc_y: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj().inverse();
        let near = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        (near, (far - near).normalize())
    }
    /// World → screen pixels; None when behind the camera.
    pub fn to_screen(&self, p: Vec3, w: f32, h: f32) -> Option<(f32, f32)> {
        let c = self.view_proj() * p.extend(1.0);
        if c.w <= 0.0 {
            return None;
        }
        let n = c.truncate() / c.w;
        Some(((n.x * 0.5 + 0.5) * w, (1.0 - (n.y * 0.5 + 0.5)) * h))
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub camera: Camera,
    /// Direction *towards* the sun.
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    pub sun_intensity: f32,
    pub sky: Vec3,
    pub ground: Vec3,
    pub clear: Vec3,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub exposure: f32,
    /// Area the shadow map must cover.
    pub shadow_center: Vec3,
    pub shadow_radius: f32,
    pub lights: Vec<PointLight>,
    pub draws: Vec<Draw>,
}

impl Frame {
    pub fn new(camera: Camera) -> Frame {
        Frame {
            camera,
            sun_dir: Vec3::new(0.4, 0.8, 0.3).normalize(),
            sun_color: Vec3::new(1.0, 0.95, 0.88),
            sun_intensity: 3.0,
            sky: Vec3::new(0.35, 0.42, 0.55),
            ground: Vec3::new(0.18, 0.16, 0.13),
            clear: Vec3::new(0.55, 0.7, 0.88),
            fog_color: Vec3::new(0.62, 0.74, 0.88),
            fog_density: 0.004,
            exposure: 1.0,
            shadow_center: Vec3::ZERO,
            shadow_radius: 30.0,
            lights: Vec::new(),
            draws: Vec::new(),
        }
    }
}

/// Converts an sRGB-authored colour to linear.
pub fn srgb_to_linear(c: Vec3) -> Vec3 {
    Vec3::new(c.x.max(0.0).powf(2.2), c.y.max(0.0).powf(2.2), c.z.max(0.0).powf(2.2))
}

// ------------------------------------------------------------------------------------------------
// GPU-side

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPointLight {
    pos_radius: [f32; 4],
    color_intensity: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    fog: [f32; 4],
    counts: [f32; 4],
    points: [GpuPointLight; MAX_POINT_LIGHTS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    params: [f32; 4],
}

struct GpuMesh {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    count: u32,
}

/// Device + queue, shared with UI renderers.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Creates a device without a window (for offscreen rendering and tools).
    pub fn headless() -> anyhow::Result<Arc<Gpu>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        Self::with_instance(instance, None)
    }

    pub fn with_instance(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> anyhow::Result<Arc<Gpu>> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))?;
        log::info!("GPU adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("hearth device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))?;
        Ok(Arc::new(Gpu { instance, adapter, device, queue }))
    }
}

struct Targets {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
}

pub struct Renderer {
    pub gpu: Arc<Gpu>,
    pub format: wgpu::TextureFormat,
    pub builtins: Builtins,
    meshes: Vec<Option<GpuMesh>>,
    globals_buf: wgpu::Buffer,
    main_bg: wgpu::BindGroup,
    shadow_bg: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    instances: wgpu::Buffer,
    instance_cap: u64,
    targets: Option<Targets>,
    pub stats: RenderStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub draws: usize,
    pub batches: usize,
    pub meshes: usize,
}

impl Renderer {
    pub fn new(gpu: Arc<Gpu>, format: wgpu::TextureFormat) -> Renderer {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let main_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("main bgl"),
            entries: &[
                uniform_entry,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow bgl"), entries: &[uniform_entry] });
        let main_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("main bg"),
            layout: &main_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_sampler) },
            ],
        });
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow bg"),
            layout: &shadow_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });

        let vertex_layouts = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<InstanceRaw>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4],
            }),
        ];

        let main_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("main layout"),
            bind_group_layouts: &[Some(&main_bgl)],
            immediate_size: 0,
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow layout"),
            bind_group_layouts: &[Some(&shadow_bgl)],
            immediate_size: 0,
        });

        let make = |label: &str, blend: Option<wgpu::BlendState>, depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&main_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &vertex_layouts,
                },
                primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = make("opaque", None, true);
        let transparent = make("transparent", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &vertex_layouts },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        let instance_cap = 4096;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: instance_cap * std::mem::size_of::<InstanceRaw>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut r = Renderer {
            gpu: gpu.clone(),
            format,
            builtins: Builtins { cube: MeshId(0), cylinder: MeshId(0), sphere: MeshId(0), disc: MeshId(0), wedge: MeshId(0) },
            meshes: Vec::new(),
            globals_buf,
            main_bg,
            shadow_bg,
            shadow_view,
            opaque,
            transparent,
            shadow,
            instances,
            instance_cap,
            targets: None,
            stats: RenderStats::default(),
        };
        r.builtins = Builtins {
            cube: r.upload_mesh(&shapes::cube()),
            cylinder: r.upload_mesh(&shapes::cylinder(20)),
            sphere: r.upload_mesh(&shapes::sphere(12, 20)),
            disc: r.upload_mesh(&shapes::disc(32)),
            wedge: r.upload_mesh(&shapes::wedge()),
        };
        r
    }

    pub fn upload_mesh(&mut self, m: &MeshData) -> MeshId {
        let id = MeshId(self.meshes.len() as u32);
        self.meshes.push(None);
        self.update_mesh(id, m);
        id
    }

    /// Replaces a mesh's geometry (e.g. walls after a build edit).
    pub fn update_mesh(&mut self, id: MeshId, m: &MeshData) {
        let device = &self.gpu.device;
        if m.indices.is_empty() {
            self.meshes[id.0 as usize] = None;
            return;
        }
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vb"),
            contents: bytemuck::cast_slice(&m.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ib"),
            contents: bytemuck::cast_slice(&m.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes[id.0 as usize] = Some(GpuMesh { vb, ib, count: m.indices.len() as u32 });
    }

    fn ensure_targets(&mut self, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == (w, h)) {
            return;
        }
        let device = &self.gpu.device;
        let mk = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        self.targets = Some(Targets { size: (w, h), msaa: mk(self.format, "msaa color"), depth: mk(DEPTH_FORMAT, "depth") });
    }

    fn light_view_proj(frame: &Frame) -> Mat4 {
        let r = frame.shadow_radius;
        let dir = frame.sun_dir.normalize();
        let eye = frame.shadow_center + dir * r * 2.0;
        let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
        let view = Mat4::look_at_rh(eye, frame.shadow_center, up);
        let proj = Mat4::orthographic_rh(-r, r, -r, r, 0.1, r * 4.0);
        proj * view
    }

    /// Renders `frame` into `target` (a view of a texture in `self.format`, single-sampled).
    pub fn render(&mut self, frame: &Frame, target: &wgpu::TextureView, width: u32, height: u32) {
        self.ensure_targets(width, height);
        let gpu = self.gpu.clone();
        let (device, queue) = (&gpu.device, &gpu.queue);

        // globals
        let mut points = [GpuPointLight::zeroed(); MAX_POINT_LIGHTS];
        for (i, l) in frame.lights.iter().take(MAX_POINT_LIGHTS).enumerate() {
            points[i] = GpuPointLight { pos_radius: l.pos.extend(l.radius).to_array(), color_intensity: l.color.extend(l.intensity).to_array() };
        }
        let globals = Globals {
            view_proj: frame.camera.view_proj().to_cols_array_2d(),
            light_view_proj: Self::light_view_proj(frame).to_cols_array_2d(),
            eye: frame.camera.eye.extend(1.0).to_array(),
            sun_dir: frame.sun_dir.normalize().extend(frame.sun_intensity).to_array(),
            sun_color: frame.sun_color.extend(1.0).to_array(),
            sky: frame.sky.extend(1.0).to_array(),
            ground: frame.ground.extend(1.0).to_array(),
            fog: srgb_to_linear(frame.fog_color).extend(frame.fog_density).to_array(),
            counts: Vec4::new(frame.lights.len().min(MAX_POINT_LIGHTS) as f32, 1.0 / SHADOW_SIZE as f32, frame.exposure, 0.0).to_array(),
            points,
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));

        // sort draws: opaque by mesh, then transparent back-to-front
        let mut opaque: Vec<&Draw> = frame.draws.iter().filter(|d| d.color[3] >= 0.999).collect();
        let mut transparent: Vec<&Draw> = frame.draws.iter().filter(|d| d.color[3] < 0.999).collect();
        opaque.sort_by_key(|d| d.mesh);
        let eye = frame.camera.eye;
        transparent.sort_by(|a, b| {
            let da = (a.model.w_axis.truncate() - eye).length_squared();
            let db = (b.model.w_axis.truncate() - eye).length_squared();
            db.total_cmp(&da)
        });
        let raw: Vec<InstanceRaw> = opaque
            .iter()
            .chain(transparent.iter())
            .map(|d| InstanceRaw {
                model: d.model.to_cols_array_2d(),
                color: d.color,
                params: [d.roughness, d.metallic, d.emissive, (d.flags & 3) as f32],
            })
            .collect();
        let need = raw.len() as u64;
        if need > self.instance_cap {
            self.instance_cap = need.next_power_of_two();
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: self.instance_cap * std::mem::size_of::<InstanceRaw>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !raw.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&raw));
        }

        // batches of consecutive identical meshes
        let mut batches: Vec<(MeshId, u32, u32, bool, bool)> = Vec::new(); // mesh, start, end, transparent, casts
        for (i, d) in opaque.iter().chain(transparent.iter()).enumerate() {
            let tr = i >= opaque.len();
            let casts = d.flags & flags::NO_SHADOW_CAST == 0 && d.flags & flags::UNLIT == 0 && !tr;
            match batches.last_mut() {
                Some(b) if b.0 == d.mesh && b.3 == tr && b.4 == casts && !tr => b.2 = i as u32 + 1,
                _ => batches.push((d.mesh, i as u32, i as u32 + 1, tr, casts)),
            }
        }
        self.stats = RenderStats { draws: raw.len(), batches: batches.len(), meshes: self.meshes.len() };

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow);
            pass.set_bind_group(0, &self.shadow_bg, &[]);
            pass.set_vertex_buffer(1, self.instances.slice(..));
            for (mesh, s, e, _, casts) in &batches {
                if !casts {
                    continue;
                }
                if let Some(m) = &self.meshes[mesh.0 as usize] {
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, *s..*e);
                }
            }
        }
        {
            let t = self.targets.as_ref().unwrap();
            let c = srgb_to_linear(frame.clear);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa,
                    depth_slice: None,
                    resolve_target: Some(target),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: c.x as f64, g: c.y as f64, b: c.z as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.main_bg, &[]);
            pass.set_vertex_buffer(1, self.instances.slice(..));
            let mut cur_tr = None;
            for (mesh, s, e, tr, _) in &batches {
                if cur_tr != Some(*tr) {
                    pass.set_pipeline(if *tr { &self.transparent } else { &self.opaque });
                    cur_tr = Some(*tr);
                }
                if let Some(m) = &self.meshes[mesh.0 as usize] {
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, *s..*e);
                }
            }
        }
        queue.submit([enc.finish()]);
    }

    /// Creates a texture suitable for offscreen rendering + readback.
    pub fn offscreen_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[self.format.remove_srgb_suffix()],
        })
    }

    /// Renders offscreen and returns tightly packed RGBA8 (sRGB) pixels.
    pub fn capture(&mut self, frame: &Frame, width: u32, height: u32) -> Vec<u8> {
        let tex = self.offscreen_texture(width, height);
        let view = tex.create_view(&Default::default());
        self.render(frame, &view, width, height);
        self.readback(&tex, width, height)
    }

    /// Copies a texture (in `self.format`) back to the CPU as RGBA8.
    pub fn readback(&self, tex: &wgpu::Texture, width: u32, height: u32) -> Vec<u8> {
        let device = &self.gpu.device;
        let row = (width * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.gpu.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = slice.get_mapped_range().expect("mapped");
        let bgra = matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let r = &data[(y * row) as usize..(y * row + width * 4) as usize];
            for p in r.chunks_exact(4) {
                if bgra {
                    out.extend_from_slice(&[p[2], p[1], p[0], 255]);
                } else {
                    out.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
        }
        drop(data);
        buf.unmap();
        out
    }

    /// Convenience: capture and save a PNG.
    pub fn save_png(&mut self, frame: &Frame, width: u32, height: u32, path: &std::path::Path) -> anyhow::Result<()> {
        let px = self.capture(frame, width, height);
        let img = image::RgbaImage::from_raw(width, height, px).ok_or_else(|| anyhow::anyhow!("bad image size"))?;
        img.save(path)?;
        Ok(())
    }
}
