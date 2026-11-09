//! wgpu backend: uploads the glyph and image atlases and draws the frame's instances in one call.

use crate::images::IMAGE_ATLAS_SIZE;
use crate::paint::Instance;
use crate::text::ATLAS_SIZE;
use crate::Ui;

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    atlas: wgpu::Texture,
    images: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: u64,
    count: u32,
}

impl Renderer {
    /// `format` is the (non-sRGB) colour target: the UI blends in gamma space.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("hearth_ui"), source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()) });
        let globals = device.create_buffer(&wgpu::BufferDescriptor { label: Some("ui globals"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui glyph atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let images = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui image atlas"),
            size: wgpu::Extent3d { width: IMAGE_ATLAS_SIZE, height: IMAGE_ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // gamma-space values, like every other UI colour
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler =device.create_sampler(&wgpu::SamplerDescriptor { label: Some("ui sampler"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let view = atlas.create_view(&Default::default());
        let images_view = images.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&images_view) },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ui layout"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32, 3 => Uint32, 4 => Float32, 5 => Float32,
            6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32
        ];
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hearth_ui"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Instance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &attrs })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 4096;
        let instances = Self::instance_buffer(device, capacity);
        Renderer { pipeline, globals, atlas, images, bind_group, instances, capacity, count: 0 }
    }

    fn instance_buffer(device: &wgpu::Device, n: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui instances"),
            size: n * std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Uploads the atlas changes and this frame's shapes (call after [`Ui::end_frame`]).
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ui: &mut Ui, size_px: [u32; 2]) {
        let ppp = ui.painter.ppp;
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[size_px[0] as f32, size_px[1] as f32, ppp, 0.0]));
        if let Some((y0, y1)) = ui.fonts.atlas.dirty.take() {
            let y1 = y1.min(ATLAS_SIZE);
            if y1 > y0 {
                let start = (y0 * ATLAS_SIZE) as usize;
                let end = (y1 * ATLAS_SIZE) as usize;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &self.atlas, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
                    &ui.fonts.atlas.pixels[start..end],
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ATLAS_SIZE), rows_per_image: Some(y1 - y0) },
                    wgpu::Extent3d { width: ATLAS_SIZE, height: y1 - y0, depth_or_array_layers: 1 },
                );
            }
        }
        if let Some((y0, y1)) = ui.images.dirty.take() {
            let y1 = y1.min(IMAGE_ATLAS_SIZE);
            if y1 > y0 && !ui.images.pixels.is_empty() {
                let row = (IMAGE_ATLAS_SIZE * 4) as usize;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &self.images, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
                    &ui.images.pixels[y0 as usize * row..y1 as usize * row],
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(IMAGE_ATLAS_SIZE * 4), rows_per_image: Some(y1 - y0) },
                    wgpu::Extent3d { width: IMAGE_ATLAS_SIZE, height: y1 - y0, depth_or_array_layers: 1 },
                );
            }
        }
        let inst = ui.instances();
        if inst.len() as u64 > self.capacity {
            self.capacity = (inst.len() as u64).next_power_of_two();
            self.instances = Self::instance_buffer(device, self.capacity);
        }
        if !inst.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(inst));
        }
        self.count = inst.len() as u32;
    }

    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..self.count);
    }
}
