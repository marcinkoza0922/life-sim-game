//! Hearth renderer: instanced PBR-lite meshes on wgpu with a shadowed sun, point lights,
//! MSAA, ACES tone mapping, a transparent pass, and offscreen capture to RGBA images.
//!
//! The renderer knows nothing about the game. Each frame the game builds a [`Frame`]: a camera,
//! lighting, and a flat list of [`Draw`]s (mesh + transform + material), plus [`SkinnedDraw`]s for
//! characters (GPU skinning with morph targets and a per-draw colour palette; see [`model`]).

pub mod model;
pub mod shapes;

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

pub use wgpu;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SAMPLES: u32 = 4;
pub const SHADOW_SIZE: u32 = 2048;
pub const MAX_POINT_LIGHTS: usize = 16;
const MAX_OCCLUDERS: usize = 128;
// Four 32-bit masks per cell. Kept in the globals uniform (under the 64 KiB limit).
const OCCLUDER_GRID: [usize; 3] = [16, 8, 16];
const OCCLUDER_CELLS: usize = OCCLUDER_GRID[0] * OCCLUDER_GRID[1] * OCCLUDER_GRID[2];
// Trace length (3 m) plus the normal offset (0.045 m), with rounding margin.
const OCCLUDER_REACH: f32 = 3.05;
/// Morph targets per skinned vertex.
pub const MAX_MORPHS: usize = 2;
/// Palette entries per skinned draw; vertex `slot` indexes it (slot 0 is conventionally white).
pub const PALETTE_SLOTS: usize = 8;

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
        for t in self.indices.as_chunks_mut::<3>().0 {
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

/// A vertex of a skinned mesh. Position deltas for up to [`MAX_MORPHS`] morph targets ride along;
/// normals are not morphed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SkinnedVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// sRGB, multiplied by the draw's palette entry for `slot`.
    pub color: [f32; 4],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
    pub morph: [[f32; 3]; MAX_MORPHS],
    pub slot: u32,
}

#[derive(Clone, Debug, Default)]
pub struct SkinnedMeshData {
    pub vertices: Vec<SkinnedVertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SkinnedMeshId(pub u32);

/// A set of skinning matrices added to a [`Frame`] with [`Frame::add_pose`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoseId(u32);

/// A skinned mesh instance: joints come from a pose shared by every mesh of the same character.
#[derive(Clone, Copy, Debug)]
pub struct SkinnedDraw {
    pub mesh: SkinnedMeshId,
    pub pose: PoseId,
    pub model: Mat4,
    /// sRGB tints indexed by vertex slot.
    pub palette: [[f32; 4]; PALETTE_SLOTS],
    pub morph: [f32; MAX_MORPHS],
    /// Multiplies the whole mesh; alpha < 1 goes through the transparent pass.
    pub color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub emissive: f32,
    pub flags: u32,
}

impl SkinnedDraw {
    pub fn new(mesh: SkinnedMeshId, pose: PoseId, model: Mat4, palette: [[f32; 4]; PALETTE_SLOTS]) -> Self {
        SkinnedDraw { mesh, pose, model, palette, morph: [0.0; MAX_MORPHS], color: [1.0; 4], roughness: 0.7, metallic: 0.0, emissive: 0.0, flags: 0 }
    }
}

/// Built-in primitive meshes, always present, plus the materials registered with
/// [`Renderer::set_materials`] by name.
#[derive(Clone, Debug)]
pub struct Builtins {
    pub cube: MeshId,
    pub cylinder: MeshId,
    pub sphere: MeshId,
    pub disc: MeshId,
    pub wedge: MeshId,
    pub materials: HashMap<String, MaterialId>,
    /// Static models registered with [`Renderer::add_model`], by name.
    pub models: HashMap<String, Vec<ModelPart>>,
}

/// One drawable piece of a registered static model (see [`model::StaticPart`]).
#[derive(Clone, Debug)]
pub struct ModelPart {
    pub mesh: MeshId,
    /// Texture material id to look up with [`Builtins::material`] (may not exist).
    pub material: String,
    pub color: [f32; 4],
    pub rough: f32,
    pub metal: f32,
    pub emissive: f32,
}

impl Builtins {
    pub fn material(&self, name: &str) -> Option<MaterialId> {
        self.materials.get(name).copied()
    }
}

/// Edge length of every material map layer; larger or smaller images are resized on upload.
pub const MATERIAL_SIZE: u32 = 512;

/// A textured surface: a layer in the renderer's material arrays. [`MaterialId::NONE`] (layer 0)
/// is flat: white albedo, flat normal, and the draw's own roughness/metallic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialId(pub u32);

impl MaterialId {
    pub const NONE: MaterialId = MaterialId(0);
}

/// Source maps for one material. Textures are applied by triplanar box mapping in the draw's
/// scaled object space (no mesh UVs needed, density stays constant on stretched primitives).
#[derive(Clone, Debug)]
pub struct MaterialMaps {
    /// sRGB base colour; multiplied by the draw's colour.
    pub albedo: image::RgbaImage,
    /// Tangent-space normal map, OpenGL convention (green = +V, up the image). Flat if absent.
    pub normal: Option<image::RgbaImage>,
    /// Linear R = ambient occlusion, G = roughness, B = metallic (glTF packing). When absent,
    /// AO is 1 and the draw's roughness/metallic are used.
    pub orm: Option<image::RgbaImage>,
    /// World meters covered by one repeat of the texture.
    pub scale: f32,
}

pub mod flags {
    /// No lighting: output the colour as-is.
    pub const UNLIT: u32 = 1;
    /// Does not receive shadows.
    pub const NO_SHADOW_RECEIVE: u32 = 2;
    /// Does not cast shadows.
    pub const NO_SHADOW_CAST: u32 = 4;
    /// Project the material in world space instead of the draw's object space, so tiled pieces
    /// (floor tiles, wall segments) continue one texture across their seams.
    pub const WORLD_TEXTURE: u32 = 8;
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
    pub material: MaterialId,
}

impl Draw {
    pub fn new(mesh: MeshId, model: Mat4, color: [f32; 3]) -> Self {
        Draw { mesh, model, color: [color[0], color[1], color[2], 1.0], roughness: 0.6, metallic: 0.0, emissive: 0.0, flags: 0, material: MaterialId::NONE }
    }
    /// Textures the draw; `None` leaves it flat.
    pub fn material(mut self, m: Option<MaterialId>) -> Self {
        self.material = m.unwrap_or_default();
        self
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
    pub lighting: LightingOptions,
    /// Center of the region receiving detailed indirect lighting.
    pub indirect_center: Vec3,
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
    pub skinned: Vec<SkinnedDraw>,
    /// Skinning matrices of every pose, back to back.
    pub joints: Vec<Mat4>,
}

/// Hemisphere sample count for ambient occlusion. Bounce lighting always uses eight rays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AoQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl AoQuality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self { Self::Low => "Low", Self::Medium => "Medium", Self::High => "High" }
    }

    fn rays(self) -> f32 {
        match self { Self::Low => 2.0, Self::Medium => 4.0, Self::High => 8.0 }
    }
}

/// Optional world-space lighting. Indirect light uses nearby box proxies rather than triangle rays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightingOptions {
    pub ambient_occlusion: bool,
    pub ao_quality: AoQuality,
    pub indirect_light: bool,
    pub shadows: bool,
    pub soft_shadows: bool,
}

impl Default for LightingOptions {
    fn default() -> Self {
        Self { ambient_occlusion: true, ao_quality: AoQuality::Medium, indirect_light: false, shadows: true, soft_shadows: true }
    }
}

impl Frame {
    pub fn new(camera: Camera) -> Frame {
        Frame {
            lighting: LightingOptions::default(),
            indirect_center: Vec3::ZERO,
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
            skinned: Vec::new(),
            joints: Vec::new(),
        }
    }

    /// Adds skinning matrices (joint global × inverse bind) for [`SkinnedDraw`]s to reference.
    pub fn add_pose(&mut self, matrices: &[Mat4]) -> PoseId {
        push_pose(&mut self.joints, matrices)
    }
}

/// [`Frame::add_pose`] on the `joints` field alone, for callers holding other fields borrowed.
pub fn push_pose(joints: &mut Vec<Mat4>, matrices: &[Mat4]) -> PoseId {
    let id = PoseId(joints.len() as u32);
    joints.extend_from_slice(matrices);
    id
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
    effects: [f32; 4],
    occluders: [GpuOccluder; MAX_OCCLUDERS],
    // xyz: grid minimum/inverse cell size; w: AO rays/reserved.
    grid_min: [f32; 4],
    grid_inv_cell: [f32; 4],
    occluder_cells: [[u32; 4]; OCCLUDER_CELLS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuOccluder {
    sphere: [f32; 4],
    world_to_box: [[f32; 4]; 4],
    color: [f32; 4],
}

/// Conservative point-query grid: every cell touched by a proxy's AABB expanded
/// by the ray reach stores its bit. No per-cell capacity limit or dropped proxies.
struct OccluderGrid {
    min: Vec3,
    inv_cell: Vec3,
    cells: [[u32; 4]; OCCLUDER_CELLS],
}

impl OccluderGrid {
    fn new(proxies: &[GpuOccluder]) -> Self {
        let mut grid = Self { min: Vec3::ZERO, inv_cell: Vec3::ONE, cells: [[0; 4]; OCCLUDER_CELLS] };
        if proxies.is_empty() { return grid; }
        let bounds: Vec<_> = proxies.iter().map(|p| {
            let model = Mat4::from_cols_array_2d(&p.world_to_box).inverse();
            let center = model.w_axis.truncate();
            // Absolute row sums give the world AABB, including rotated/nonuniform boxes.
            let extent = (model.x_axis.truncate().abs() + model.y_axis.truncate().abs()
                + model.z_axis.truncate().abs()) * 0.5 + Vec3::splat(OCCLUDER_REACH);
            (center - extent, center + extent)
        }).collect();
        let (lo, hi) = bounds.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), (a, b)| (lo.min(*a), hi.max(*b)));
        grid.min = lo;
        grid.inv_cell = Vec3::new(OCCLUDER_GRID[0] as f32, OCCLUDER_GRID[1] as f32, OCCLUDER_GRID[2] as f32)
            / (hi - lo).max(Vec3::splat(0.001));
        let [nx, ny, nz] = OCCLUDER_GRID.map(|v| v as u32);
        for (i, (a, b)) in bounds.iter().enumerate() {
            let start = ((*a - lo) * grid.inv_cell).floor().as_uvec3();
            let end = ((*b - lo) * grid.inv_cell).floor().as_uvec3();
            for z in start.z.min(nz - 1)..=end.z.min(nz - 1) {
                for y in start.y.min(ny - 1)..=end.y.min(ny - 1) {
                    for x in start.x.min(nx - 1)..=end.x.min(nx - 1) {
                        grid.cells[(x + nx * (y + ny * z)) as usize][i / 32] |= 1 << (i % 32);
                    }
                }
            }
        }
        grid
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    params: [f32; 4],
    /// material layer, 1 / texture scale
    tex: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkInstanceRaw {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    params: [f32; 4],
    /// joint base, palette base (vec4 indices into the skin buffer), morph weights
    extra: [f32; 4],
}

struct GpuMesh {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    count: u32,
    bounds: (Vec3, Vec3),
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
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
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
    sk_meshes: Vec<Option<GpuMesh>>,
    globals_buf: wgpu::Buffer,
    main_bgl: wgpu::BindGroupLayout,
    main_bg: wgpu::BindGroup,
    shadow_bg: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    material_sampler: wgpu::Sampler,
    /// 1 / scale per material layer (layer 0 = flat).
    material_inv_scale: Vec<f32>,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    instances: wgpu::Buffer,
    instance_cap: u64,
    sk_opaque: wgpu::RenderPipeline,
    sk_transparent: wgpu::RenderPipeline,
    sk_shadow: wgpu::RenderPipeline,
    sk_instances: wgpu::Buffer,
    sk_instance_cap: u64,
    skin_bgl: wgpu::BindGroupLayout,
    skin_buf: wgpu::Buffer,
    skin_cap: u64,
    skin_bg: wgpu::BindGroup,
    targets: Option<Targets>,
    pub stats: RenderStats,
    profile_queries: Option<wgpu::QuerySet>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub draws: usize,
    pub batches: usize,
    pub meshes: usize,
    pub skinned: usize,
}

impl Renderer {
    pub fn new(gpu: Arc<Gpu>, format: wgpu::TextureFormat) -> Renderer {
        Self::with_shader(gpu, format, include_str!("shader.wgsl"))
    }

    fn with_shader(gpu: Arc<Gpu>, format: wgpu::TextureFormat, source: &str) -> Renderer {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hearth shader"), source: wgpu::ShaderSource::Wgsl(source.into()),
        });

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
                material_tex_entry(3),
                material_tex_entry(4),
                material_tex_entry(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let material_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow bgl"), entries: &[uniform_entry] });
        let main_bg = main_bind_group(device, &main_bgl, &globals_buf, &shadow_view, &shadow_sampler, &MaterialArrays::new(device, &gpu.queue, &[]), &material_sampler);
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
                attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4],
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

        let skin_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let sk_layouts = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SkinnedVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Uint16x4, 4 => Float32x4, 5 => Float32x3, 6 => Float32x3, 7 => Uint32],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SkInstanceRaw>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4],
            }),
        ];
        let sk_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned layout"),
            bind_group_layouts: &[Some(&main_bgl), Some(&skin_bgl)],
            immediate_size: 0,
        });
        let sk_shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned shadow layout"),
            bind_group_layouts: &[Some(&shadow_bgl), Some(&skin_bgl)],
            immediate_size: 0,
        });

        let make_with = |label: &str, layout: &wgpu::PipelineLayout, vs: &str, buffers: &[Option<wgpu::VertexBufferLayout>], blend: Option<wgpu::BlendState>, depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
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
        let opaque = make_with("opaque", &main_layout, "vs_main", &vertex_layouts, None, true);
        let transparent = make_with("transparent", &main_layout, "vs_main", &vertex_layouts, Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let sk_opaque = make_with("skinned opaque", &sk_layout, "vs_skinned", &sk_layouts, None, true);
        let sk_transparent = make_with("skinned transparent", &sk_layout, "vs_skinned", &sk_layouts, Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let make_shadow = |label: &str, layout: &wgpu::PipelineLayout, vs: &str, buffers: &[Option<wgpu::VertexBufferLayout>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
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
            })
        };
        let shadow = make_shadow("shadow", &shadow_layout, "vs_shadow", &vertex_layouts);
        let sk_shadow = make_shadow("skinned shadow", &sk_shadow_layout, "vs_shadow_skinned", &sk_layouts);

        let instance_cap = 4096;
        let instances = vertex_buffer(device, "instances", instance_cap * std::mem::size_of::<InstanceRaw>() as u64);
        let sk_instance_cap = 64;
        let sk_instances = vertex_buffer(device, "skinned instances", sk_instance_cap * std::mem::size_of::<SkInstanceRaw>() as u64);
        let skin_cap = 4096; // vec4s
        let skin_buf = storage_buffer(device, skin_cap * 16);
        let skin_bg = skin_bind_group(device, &skin_bgl, &skin_buf);

        let mut r = Renderer {
            gpu: gpu.clone(),
            format,
            builtins: Builtins { cube: MeshId(0), cylinder: MeshId(0), sphere: MeshId(0), disc: MeshId(0), wedge: MeshId(0), materials: HashMap::new(), models: HashMap::new() },
            meshes: Vec::new(),
            sk_meshes: Vec::new(),
            globals_buf,
            main_bgl,
            main_bg,
            shadow_bg,
            shadow_view,
            shadow_sampler,
            material_sampler,
            material_inv_scale: vec![1.0],
            opaque,
            transparent,
            shadow,
            instances,
            instance_cap,
            sk_opaque,
            sk_transparent,
            sk_shadow,
            sk_instances,
            sk_instance_cap,
            skin_bgl,
            skin_buf,
            skin_cap,
            skin_bg,
            targets: None,
            stats: RenderStats::default(),
            profile_queries: None,
        };
        r.builtins = Builtins {
            cube: r.upload_mesh(&shapes::cube()),
            cylinder: r.upload_mesh(&shapes::cylinder(20)),
            sphere: r.upload_mesh(&shapes::sphere(12, 20)),
            disc: r.upload_mesh(&shapes::disc(32)),
            wedge: r.upload_mesh(&shapes::wedge()),
            materials: HashMap::new(),
            models: HashMap::new(),
        };
        r
    }

    /// Replaces every registered material (ids from earlier calls become invalid) and makes them
    /// available by name through [`Builtins::material`].
    pub fn set_materials(&mut self, mats: &[(String, MaterialMaps)]) {
        let gpu = self.gpu.clone();
        let maps: Vec<&MaterialMaps> = mats.iter().map(|(_, m)| m).collect();
        let arrays = MaterialArrays::new(&gpu.device, &gpu.queue, &maps);
        self.main_bg = main_bind_group(&gpu.device, &self.main_bgl, &self.globals_buf, &self.shadow_view, &self.shadow_sampler, &arrays, &self.material_sampler);
        self.material_inv_scale = std::iter::once(1.0).chain(maps.iter().map(|m| 1.0 / m.scale.max(1e-3))).collect();
        self.builtins.materials = mats.iter().enumerate().map(|(i, (name, _))| (name.clone(), MaterialId(i as u32 + 1))).collect();
    }

    /// Uploads a static model's parts and registers them under `name` in [`Builtins::models`].
    pub fn add_model(&mut self, name: &str, parts: &[model::StaticPart]) {
        let parts = parts
            .iter()
            .map(|p| ModelPart { mesh: self.upload_mesh(&p.mesh), material: p.material.clone(), color: p.color, rough: p.rough, metal: p.metal, emissive: p.emissive })
            .collect();
        self.builtins.models.insert(name.to_string(), parts);
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
        self.meshes[id.0 as usize] = Some(GpuMesh { vb, ib, count: m.indices.len() as u32, bounds: mesh_bounds(m.vertices.iter().map(|v| Vec3::from(v.pos))) });
    }

    pub fn upload_skinned_mesh(&mut self, m: &SkinnedMeshData) -> SkinnedMeshId {
        let id = SkinnedMeshId(self.sk_meshes.len() as u32);
        let mesh = (!m.indices.is_empty()).then(|| {
            let device = &self.gpu.device;
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("skinned vb"),
                contents: bytemuck::cast_slice(&m.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("skinned ib"),
                contents: bytemuck::cast_slice(&m.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            GpuMesh { vb, ib, count: m.indices.len() as u32, bounds: mesh_bounds(m.vertices.iter().map(|v| Vec3::from(v.pos))) }
        });
        self.sk_meshes.push(mesh);
        id
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
        let mut occluders = [GpuOccluder::zeroed(); MAX_OCCLUDERS];
        let mut candidates = Vec::new();
        if frame.lighting.ambient_occlusion || frame.lighting.indirect_light {
            for d in &frame.draws {
                if d.color[3] < 0.999 || d.flags & (flags::UNLIT | flags::NO_SHADOW_CAST) != 0 {
                    continue;
                }
                let Some(mesh) = self.meshes.get(d.mesh.0 as usize).and_then(Option::as_ref) else { continue };
                let (lo, hi) = mesh.bounds;
                let size = hi - lo;
                if size.min_element() < 0.001 || d.model.determinant().abs() < 1e-8 { continue; }
                let model = d.model * Mat4::from_translation((lo + hi) * 0.5) * Mat4::from_scale(size);
                let center = model.w_axis.truncate();
                // Prefer geometry near the viewed area. Bounded cost independent of lot size.
                let distance = (center - frame.indirect_center).length_squared();
                candidates.push((distance, GpuOccluder {
                    sphere: center.extend((model.x_axis.truncate().length() + model.y_axis.truncate().length() + model.z_axis.truncate().length()) * 0.5).to_array(),
                    world_to_box: model.inverse().to_cols_array_2d(),
                    color: srgb_to_linear(Vec3::from_slice(&d.color[..3])).extend(d.emissive).to_array(),
                }));
            }
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let occluder_count = candidates.len().min(MAX_OCCLUDERS);
        for (dst, (_, src)) in occluders.iter_mut().zip(candidates) { *dst = src; }
        let grid = OccluderGrid::new(&occluders[..occluder_count]);
        let globals = Globals {
            view_proj: frame.camera.view_proj().to_cols_array_2d(),
            light_view_proj: Self::light_view_proj(frame).to_cols_array_2d(),
            eye: frame.camera.eye.extend(1.0).to_array(),
            sun_dir: frame.sun_dir.normalize().extend(frame.sun_intensity).to_array(),
            sun_color: frame.sun_color.extend(1.0).to_array(),
            sky: frame.sky.extend(1.0).to_array(),
            ground: frame.ground.extend(1.0).to_array(),
            fog: srgb_to_linear(frame.fog_color).extend(frame.fog_density).to_array(),
            counts: Vec4::new(frame.lights.len().min(MAX_POINT_LIGHTS) as f32, 1.0 / SHADOW_SIZE as f32, frame.exposure, occluder_count as f32).to_array(),
            points,
            effects: [f32::from(frame.lighting.ambient_occlusion), f32::from(frame.lighting.indirect_light),
                f32::from(frame.lighting.shadows), f32::from(frame.lighting.soft_shadows)],
            occluders,
            grid_min: grid.min.extend(frame.lighting.ao_quality.rays()).to_array(),
            grid_inv_cell: grid.inv_cell.extend(0.0).to_array(),
            occluder_cells: grid.cells,
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
            .map(|d| {
                let layer = if (d.material.0 as usize) < self.material_inv_scale.len() { d.material.0 } else { 0 };
                InstanceRaw {
                    model: d.model.to_cols_array_2d(),
                    color: d.color,
                    params: [d.roughness, d.metallic, d.emissive, (d.flags & 3) as f32],
                    tex: [layer as f32, self.material_inv_scale[layer as usize], (d.flags & flags::WORLD_TEXTURE != 0) as u32 as f32, 0.0],
                }
            })
            .collect();
        let need = raw.len() as u64;
        if need > self.instance_cap {
            self.instance_cap = need.next_power_of_two();
            self.instances = vertex_buffer(device, "instances", self.instance_cap * std::mem::size_of::<InstanceRaw>() as u64);
        }
        if !raw.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&raw));
        }

        // skinned: opaque grouped by mesh, then transparent back-to-front; joints + palettes go
        // into one storage buffer of vec4s
        let mut sk_order: Vec<&SkinnedDraw> = frame.skinned.iter().filter(|d| self.sk_meshes.get(d.mesh.0 as usize).is_some_and(|m| m.is_some())).collect();
        sk_order.sort_by(|a, b| {
            let (ta, tb) = (a.color[3] < 0.999, b.color[3] < 0.999);
            ta.cmp(&tb).then_with(|| {
                if ta {
                    let da = (a.model.w_axis.truncate() - eye).length_squared();
                    let db = (b.model.w_axis.truncate() - eye).length_squared();
                    db.total_cmp(&da)
                } else {
                    a.mesh.cmp(&b.mesh)
                }
            })
        });
        let mut skin_data: Vec<[f32; 4]> = Vec::with_capacity(frame.joints.len() * 4 + sk_order.len() * PALETTE_SLOTS);
        for m in &frame.joints {
            skin_data.extend(m.to_cols_array_2d());
        }
        let mut sk_raw = Vec::with_capacity(sk_order.len());
        for d in &sk_order {
            let palette_base = skin_data.len();
            skin_data.extend(d.palette);
            sk_raw.push(SkInstanceRaw {
                model: d.model.to_cols_array_2d(),
                color: d.color,
                params: [d.roughness, d.metallic, d.emissive, (d.flags & 3) as f32],
                extra: [(d.pose.0 * 4) as f32, palette_base as f32, d.morph[0], d.morph[1]],
            });
        }
        if skin_data.is_empty() {
            skin_data.push([0.0; 4]);
        }
        if skin_data.len() as u64 > self.skin_cap {
            self.skin_cap = (skin_data.len() as u64).next_power_of_two();
            self.skin_buf = storage_buffer(device, self.skin_cap * 16);
            self.skin_bg = skin_bind_group(device, &self.skin_bgl, &self.skin_buf);
        }
        queue.write_buffer(&self.skin_buf, 0, bytemuck::cast_slice(&skin_data));
        if sk_raw.len() as u64 > self.sk_instance_cap {
            self.sk_instance_cap = (sk_raw.len() as u64).next_power_of_two();
            self.sk_instances = vertex_buffer(device, "skinned instances", self.sk_instance_cap * std::mem::size_of::<SkInstanceRaw>() as u64);
        }
        if !sk_raw.is_empty() {
            queue.write_buffer(&self.sk_instances, 0, bytemuck::cast_slice(&sk_raw));
        }
        // mesh, start, end, transparent, casts
        let mut sk_batches: Vec<(SkinnedMeshId, u32, u32, bool, bool)> = Vec::new();
        for (i, d) in sk_order.iter().enumerate() {
            let tr = d.color[3] < 0.999;
            let casts = d.flags & flags::NO_SHADOW_CAST == 0 && d.flags & flags::UNLIT == 0 && !tr;
            match sk_batches.last_mut() {
                Some(b) if b.0 == d.mesh && b.3 == tr && b.4 == casts && !tr => b.2 = i as u32 + 1,
                _ => sk_batches.push((d.mesh, i as u32, i as u32 + 1, tr, casts)),
            }
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
        self.stats = RenderStats { draws: raw.len() + sk_raw.len(), batches: batches.len() + sk_batches.len(), meshes: self.meshes.len() + self.sk_meshes.len(), skinned: sk_raw.len() };

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        if frame.lighting.shadows {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: self.profile_queries.as_ref().map(|q| wgpu::RenderPassTimestampWrites {
                    query_set: q, beginning_of_pass_write_index: Some(0), end_of_pass_write_index: Some(1),
                }),
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
            if sk_batches.iter().any(|b| b.4) {
                pass.set_pipeline(&self.sk_shadow);
                pass.set_bind_group(1, &self.skin_bg, &[]);
                pass.set_vertex_buffer(1, self.sk_instances.slice(..));
                for (mesh, s, e, _, casts) in &sk_batches {
                    if let (true, Some(m)) = (*casts, &self.sk_meshes[mesh.0 as usize]) {
                        pass.set_vertex_buffer(0, m.vb.slice(..));
                        pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..m.count, 0, *s..*e);
                    }
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
                timestamp_writes: self.profile_queries.as_ref().map(|q| wgpu::RenderPassTimestampWrites {
                    query_set: q, beginning_of_pass_write_index: Some(2), end_of_pass_write_index: Some(3),
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.main_bg, &[]);
            pass.set_bind_group(1, &self.skin_bg, &[]);
            // static opaque, skinned opaque, static transparent, skinned transparent
            for transparent_pass in [false, true] {
                pass.set_pipeline(if transparent_pass { &self.transparent } else { &self.opaque });
                pass.set_vertex_buffer(1, self.instances.slice(..));
                for (mesh, s, e, tr, _) in &batches {
                    if *tr != transparent_pass {
                        continue;
                    }
                    if let Some(m) = &self.meshes[mesh.0 as usize] {
                        pass.set_vertex_buffer(0, m.vb.slice(..));
                        pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..m.count, 0, *s..*e);
                    }
                }
                if !sk_batches.iter().any(|b| b.3 == transparent_pass) {
                    continue;
                }
                pass.set_pipeline(if transparent_pass { &self.sk_transparent } else { &self.sk_opaque });
                pass.set_vertex_buffer(1, self.sk_instances.slice(..));
                for (mesh, s, e, tr, _) in &sk_batches {
                    if *tr != transparent_pass {
                        continue;
                    }
                    if let Some(m) = &self.sk_meshes[mesh.0 as usize] {
                        pass.set_vertex_buffer(0, m.vb.slice(..));
                        pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..m.count, 0, *s..*e);
                    }
                }
            }
        }
        queue.submit([enc.finish()]);
    }

    /// Measures one frame with GPU pass timestamps. Synchronous, for offline profiling only.
    /// Returns shadow and main pass milliseconds; excludes CPU work and image readback.
    pub fn profile_frame(&mut self, frame: &Frame, target: &wgpu::TextureView, width: u32, height: u32) -> anyhow::Result<[f64; 2]> {
        anyhow::ensure!(self.gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY), "adapter does not support GPU timestamps");
        self.profile_queries = Some(self.gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("profile timestamps"), ty: wgpu::QueryType::Timestamp, count: 4,
        }));
        self.render(frame, target, width, height);
        let queries = self.profile_queries.take().unwrap();
        let device = &self.gpu.device;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("profile resolve"), size: 32,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("profile readback"), size: 32,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        let first = if frame.lighting.shadows { 0 } else { 2 };
        enc.resolve_query_set(&queries, first..4, &resolve, 0);
        enc.copy_buffer_to_buffer(&resolve, 0, &read, 0, 32);
        self.gpu.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..).map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = read.slice(..).get_mapped_range()?;
        let ticks: &[u64] = bytemuck::cast_slice(&data);
        let scale = self.gpu.queue.get_timestamp_period() as f64 / 1_000_000.0;
        let times = if frame.lighting.shadows {
            [(ticks[1] - ticks[0]) as f64 * scale, (ticks[3] - ticks[2]) as f64 * scale]
        } else { [0.0, (ticks[1] - ticks[0]) as f64 * scale] };
        drop(data);
        read.unmap();
        Ok(times)
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
            for p in r.as_chunks::<4>().0 {
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

fn mesh_bounds(vertices: impl Iterator<Item = Vec3>) -> (Vec3, Vec3) {
    vertices.fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)))
}

fn vertex_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

fn storage_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some("skin"), size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

fn material_tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}

fn main_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    shadow_view: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
    mats: &MaterialArrays,
    material_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("main bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(shadow_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(shadow_sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&mats.albedo) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&mats.normal) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&mats.orm) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(material_sampler) },
        ],
    })
}

/// Albedo, normal and ORM texture arrays: layer 0 is the flat material, then one layer per material.
struct MaterialArrays {
    albedo: wgpu::TextureView,
    normal: wgpu::TextureView,
    orm: wgpu::TextureView,
}

impl MaterialArrays {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, maps: &[&MaterialMaps]) -> MaterialArrays {
        let flat = |c: [u8; 4]| image::RgbaImage::from_pixel(1, 1, image::Rgba(c));
        let (white, flat_n, flat_orm) = (flat([255; 4]), flat([128, 128, 255, 255]), flat([255, 255, 0, 255]));
        let layers = |pick: &dyn Fn(&MaterialMaps) -> Option<&image::RgbaImage>, default: &image::RgbaImage| -> Vec<image::RgbaImage> {
            std::iter::once(default.clone()).chain(maps.iter().map(|m| pick(m).unwrap_or(default).clone())).collect()
        };
        MaterialArrays {
            albedo: texture_array(device, queue, "material albedo", wgpu::TextureFormat::Rgba8UnormSrgb, &layers(&|m| Some(&m.albedo), &white)),
            normal: texture_array(device, queue, "material normal", wgpu::TextureFormat::Rgba8Unorm, &layers(&|m| m.normal.as_ref(), &flat_n)),
            orm: texture_array(device, queue, "material orm", wgpu::TextureFormat::Rgba8Unorm, &layers(&|m| m.orm.as_ref(), &flat_orm)),
        }
    }
}

/// A full mip chain of `MATERIAL_SIZE` layers, each image resized to fit.
fn texture_array(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, format: wgpu::TextureFormat, images: &[image::RgbaImage]) -> wgpu::TextureView {
    let size = MATERIAL_SIZE;
    let mips = size.ilog2() + 1;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: images.len() as u32 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, img) in images.iter().enumerate() {
        for mip in 0..mips {
            let s = size >> mip;
            let level = image::imageops::resize(img, s, s, image::imageops::FilterType::Triangle);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: mip, origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 }, aspect: wgpu::TextureAspect::All },
                &level,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(s * 4), rows_per_image: Some(s) },
                wgpu::Extent3d { width: s, height: s, depth_or_array_layers: 1 },
            );
        }
    }
    tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })
}

fn skin_bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, buf: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("skin bg"), layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_grid_conservatively_covers_transformed_proxies_and_dense_cells() {
        assert!(std::mem::size_of::<Globals>() <= 65536);
        let models = [
            Mat4::from_scale_rotation_translation(Vec3::new(0.1, 8.0, 1.0), glam::Quat::from_rotation_y(0.7), Vec3::new(-12.0, 1.0, -7.0)),
            Mat4::from_scale_rotation_translation(Vec3::new(-9.0, 0.2, 5.0), glam::Quat::from_rotation_z(0.4), Vec3::new(8.0, -4.0, 11.0)),
        ];
        let proxies: Vec<_> = (0..128).map(|i| GpuOccluder {
            world_to_box: models[i % 2].inverse().to_cols_array_2d(), ..GpuOccluder::zeroed()
        }).collect();
        let grid = OccluderGrid::new(&proxies);
        // Every point within ray reach of a transformed box must retain that proxy,
        // including all mask words and the high bit of each word.
        for (i, model) in models.iter().enumerate() {
            for x in [-0.5, 0.0, 0.5] {
                for y in [-0.5, 0.0, 0.5] {
                    for z in [-0.5, 0.0, 0.5] {
                        let surface = model.transform_point3(Vec3::new(x, y, z));
                        for direction in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
                            let point = surface + direction * 3.045;
                            let cell = ((point - grid.min) * grid.inv_cell).floor().as_uvec3();
                            assert!(cell.x < 16 && cell.y < 8 && cell.z < 16);
                            let bits = grid.cells[(cell.x + 16 * (cell.y + 8 * cell.z)) as usize];
                            for index in (i..128).step_by(2) {
                                assert_ne!(bits[index / 32] & (1 << (index % 32)), 0);
                            }
                        }
                    }
                }
            }
        }
        assert!(OccluderGrid::new(&[]).cells.iter().all(|c| *c == [0; 4]));
    }

    #[test]
    fn spatial_tracing_matches_full_proxy_scan() {
        let Ok(gpu) = Gpu::headless() else { eprintln!("Skipping spatial capture: no GPU"); return; };
        let source = include_str!("shader.wgsl");
        let start = source.find("    // A point lookup").unwrap();
        let end = source[start..].find("    let bounce_enabled").unwrap() + start;
        let reference = format!("{}    let candidates = vec4<u32>(0xffffffffu);\n{}", &source[..start], &source[end..])
            .replace("let k = i32(word * 32u + bit);", "let k = i32(word * 32u + bit); if (k >= i32(g.counts.w)) { continue; }");
        let mut optimized = Renderer::new(gpu.clone(), wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut full_scan = Renderer::with_shader(gpu, wgpu::TextureFormat::Rgba8UnormSrgb, &reference);
        let mut frame = Frame::new(Camera::orbit(Vec3::new(0.0, 0.4, 0.0), 0.4, 0.65, 9.0, 1.0));
        frame.fog_density = 0.0;
        frame.draws.push(Draw::new(optimized.builtins.cube,
            Mat4::from_scale_rotation_translation(Vec3::new(12.0, 0.2, 12.0), glam::Quat::IDENTITY, Vec3::new(0.0, -0.2, 0.0)), [0.8; 3]));
        for i in 0..127 {
            let x = (i % 12) as f32 - 5.5;
            let z = (i / 12) as f32 - 5.0;
            frame.draws.push(Draw::new(optimized.builtins.cube,
                Mat4::from_scale_rotation_translation(Vec3::new(0.3, 0.5 + (i % 3) as f32 * 0.3, 0.7),
                    glam::Quat::from_rotation_y(i as f32 * 0.4), Vec3::new(x, 0.3, z)), [0.7, 0.2, 0.1]));
        }
        for bounce in [false, true] {
            frame.lighting.indirect_light = bounce;
            for quality in AoQuality::ALL {
                frame.lighting.ao_quality = quality;
                let actual = optimized.capture(&frame, 128, 128);
                let expected = full_scan.capture(&frame, 128, 128);
                assert!(actual == expected, "spatial pruning changed pixels: {quality:?}, bounce={bounce}");
            }
        }
    }

    /// Renders one cube filling most of the view; returns the RGBA pixels. Skips (None) without a GPU.
    fn render_cube(material: Option<(&str, MaterialMaps)>) -> Option<Vec<u8>> {
        let gpu = Gpu::headless().ok()?;
        let mut r = Renderer::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut mat = None;
        if let Some((name, maps)) = material {
            r.set_materials(&[(name.to_string(), maps)]);
            mat = r.builtins.material(name);
            assert_eq!(mat, Some(MaterialId(1)));
        }
        let mut f = Frame::new(Camera::orbit(Vec3::ZERO, 0.6, 0.5, 4.0, 1.0));
        f.draws.push(Draw::new(r.builtins.cube, Mat4::from_scale(Vec3::new(2.0, 1.0, 2.0)), [1.0; 3]).material(mat));
        Some(r.capture(&f, 64, 64))
    }

    #[test]
    fn indirect_lighting_darkens_contacts_and_bounces_color() {
        let Ok(gpu) = Gpu::headless() else { eprintln!("Skipping lighting capture: no GPU"); return; };
        let mut renderer = Renderer::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut f = Frame::new(Camera::orbit(Vec3::new(0.0, 0.4, 0.0), 0.2, 0.65, 7.0, 1.0));
        f.sun_intensity = 0.0;
        f.sky = Vec3::splat(0.5);
        f.ground = Vec3::splat(0.25);
        f.fog_density = 0.0;
        f.draws.push(Draw::new(renderer.builtins.cube, Mat4::from_scale_rotation_translation(
            Vec3::new(6.0, 0.2, 6.0), glam::Quat::IDENTITY, Vec3::new(0.0, -0.15, 0.0)), [0.8; 3]));
        f.draws.push(Draw::new(renderer.builtins.cube, Mat4::from_scale_rotation_translation(
            Vec3::new(5.0, 2.5, 0.2), glam::Quat::IDENTITY, Vec3::new(0.0, 1.1, -1.0)), [0.9, 0.15, 0.1]));
        f.lighting = LightingOptions { ambient_occlusion: false, indirect_light: false, shadows: false, soft_shadows: false, ..Default::default() };
        let baseline = renderer.capture(&f, 128, 128);
        f.lighting.ambient_occlusion = true;
        let ao = renderer.capture(&f, 128, 128);
        f.lighting.indirect_light = true;
        let bounce = renderer.capture(&f, 128, 128);
        let darker = baseline.as_chunks::<4>().0.iter().zip(ao.as_chunks::<4>().0)
            .filter(|(a, b)| a[0] > b[0].saturating_add(5)).count();
        let red_bounce = bounce.as_chunks::<4>().0.iter().zip(ao.as_chunks::<4>().0)
            .filter(|(a, b)| a[0] > b[0].saturating_add(2) && a[0].saturating_sub(b[0]) > a[2].saturating_sub(b[2])).count();
        assert!(darker > 100, "AO should darken contact pixels: {darker}");
        assert!(red_bounce > 50, "red wall should bounce red light: {red_bounce}");
        f.lighting.ambient_occlusion = false;
        f.lighting.indirect_light = false;
        assert_eq!(renderer.capture(&f, 128, 128), baseline, "effects must fully disable");
        if let Ok(dir) = std::env::var("HEARTH_TEST_OUT") {
            for (name, pixels) in [("baseline", baseline), ("ao", ao), ("bounce", bounce)] {
                image::RgbaImage::from_raw(128, 128, pixels).unwrap().save(format!("{dir}/lighting_{name}.png")).unwrap();
            }
        }
    }

    /// 2×2 black/white checker per repeat, one repeat per meter.
    fn checker() -> MaterialMaps {
        let albedo = image::RgbaImage::from_fn(64, 64, |x, y| if (x / 32 + y / 32) % 2 == 0 { image::Rgba([255; 4]) } else { image::Rgba([10, 10, 10, 255]) });
        MaterialMaps { albedo, normal: None, orm: Some(image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 200, 0, 255]))), scale: 1.0 }
    }

    /// Brightness of the middle of a wall (+Z face) lit from above, with a constant normal map.
    fn lit_wall(normal: Option<[u8; 4]>) -> Option<u32> {
        let gpu = Gpu::headless().ok()?;
        let mut r = Renderer::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let albedo = image::RgbaImage::from_pixel(4, 4, image::Rgba([180, 180, 180, 255]));
        let normal = normal.map(|n| image::RgbaImage::from_pixel(4, 4, image::Rgba(n)));
        r.set_materials(&[("m".into(), MaterialMaps { albedo, normal, orm: None, scale: 1.0 })]);
        let mut f = Frame::new(Camera::orbit(Vec3::ZERO, 0.0, 0.0, 4.0, 1.0));
        f.sun_dir = Vec3::new(0.0, 0.8, 0.6).normalize();
        f.sky = Vec3::ZERO;
        f.ground = Vec3::ZERO;
        f.fog_density = 0.0;
        f.draws.push(Draw::new(r.builtins.cube, Mat4::from_scale(Vec3::new(2.0, 2.0, 0.2)), [1.0; 3]).material(r.builtins.material("m")));
        let px = r.capture(&f, 32, 32);
        let i = (16 * 32 + 16) * 4;
        Some(px[i] as u32)
    }

    #[test]
    fn normal_map_green_tilts_walls_upward() {
        let Some(flat) = lit_wall(None) else { return };
        // green up = toward +V in OpenGL convention = up the image = world +Y on a wall
        let up = lit_wall(Some([128, 230, 180, 255])).unwrap();
        let down = lit_wall(Some([128, 26, 180, 255])).unwrap();
        assert!(up > flat + 10 && down + 10 < flat, "flat {flat}, tilted up {up}, tilted down {down}");
    }

    #[test]
    fn materials_texture_draws_and_flat_draws_stay_flat() {
        let Some(flat) = render_cube(None) else { return };
        let textured = render_cube(Some(("checker", checker()))).unwrap();
        if let Ok(dir) = std::env::var("HEARTH_TEST_OUT") {
            for (name, px) in [("flat", &flat), ("checker", &textured)] {
                image::RgbaImage::from_raw(64, 64, px.clone()).unwrap().save(format!("{dir}/material_{name}.png")).unwrap();
            }
        }
        // the flat cube's faces are uniform; the textured one has dark checker squares
        let dark = |px: &[u8]| px.as_chunks::<4>().0.iter().filter(|p| p[0] < 40).count();
        assert!(dark(&textured) > dark(&flat) + 300, "dark pixels: flat {} textured {}", dark(&flat), dark(&textured));
        // registering materials doesn't change untextured draws
        let gpu = Gpu::headless().unwrap();
        let mut r = Renderer::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        r.set_materials(&[("checker".into(), checker())]);
        let mut f = Frame::new(Camera::orbit(Vec3::ZERO, 0.6, 0.5, 4.0, 1.0));
        f.draws.push(Draw::new(r.builtins.cube, Mat4::from_scale(Vec3::new(2.0, 1.0, 2.0)), [1.0; 3]));
        assert_eq!(r.capture(&f, 64, 64), flat);
    }
}
