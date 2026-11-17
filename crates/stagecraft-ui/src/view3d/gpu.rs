//! wgpu rendering of the 3D view through an `egui_wgpu` paint callback.
//!
//! `prepare` renders the scene into an offscreen 4× MSAA colour + depth target of the
//! view's size (reversed depth); `paint` copies the resolved image into egui's pass
//! with a full-screen triangle. So the scene needs no depth buffer or sample count
//! from egui, and the same code runs in eframe and in egui_kittest's headless renderer.
//!
//! [`install`] puts the pipelines into the renderer's callback resources once; without
//! them the view stays empty (tests that do not render).

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

use super::scene::{Buffers, Vertex};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

/// Shader uniforms; mirrors `struct U` in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub view_dir: [f32; 4],
    pub light: [f32; 4],
    pub planes: [[f32; 4]; 6],
    pub colors: [[f32; 4]; 16],
    pub selected: [f32; 4],
    pub hatch: [f32; 4],
    pub grid: [f32; 4],
    pub grid_minor: [f32; 4],
    pub grid_major: [f32; 4],
    pub grid_center: [f32; 4],
    pub counts: [u32; 4],
}

/// One frame of the 3D view, handed to the paint callback.
pub struct Frame3d {
    pub uniforms: Uniforms,
    pub size_px: [u32; 2],
    /// Linear RGBA.
    pub clear: [f64; 4],
    pub mesh: Buffers,
    pub caps: Buffers,
}

struct GpuBuffers {
    version: u64,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    count: u32,
}

impl GpuBuffers {
    fn new() -> Self {
        Self { version: u64::MAX, vertices: None, indices: None, count: 0 }
    }

    fn upload(&mut self, device: &wgpu::Device, b: &Buffers) {
        if self.version == b.version {
            return;
        }
        self.version = b.version;
        self.count = b.indices.len() as u32;
        if b.indices.is_empty() {
            self.vertices = None;
            self.indices = None;
            return;
        }
        self.vertices = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stagecraft-3d vertices"),
            contents: bytemuck::cast_slice(&b.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }));
        self.indices = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stagecraft-3d indices"),
            contents: bytemuck::cast_slice(&b.indices),
            usage: wgpu::BufferUsages::INDEX,
        }));
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(v), Some(i)) = (&self.vertices, &self.indices) {
            pass.set_vertex_buffer(0, v.slice(..));
            pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.count, 0, 0..1);
        }
    }
}

struct Target {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    resolve: wgpu::TextureView,
    depth: wgpu::TextureView,
    blit_group: wgpu::BindGroup,
}

pub struct Gpu3d {
    mesh_pipeline: wgpu::RenderPipeline,
    grid_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    mesh: GpuBuffers,
    caps: GpuBuffers,
    target: Option<Target>,
}

/// Set up the 3D renderer for a wgpu render state (eframe's or a test's).
pub fn install(rs: &egui_wgpu::RenderState) {
    let gpu = Gpu3d::new(&rs.device, rs.target_format);
    rs.renderer.write().callback_resources.insert(gpu);
}

impl Gpu3d {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stagecraft-3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let blit = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stagecraft-3d blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stagecraft-3d uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stagecraft-3d uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stagecraft-3d uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stagecraft-3d"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });
        let depth = |write: bool| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let multisample = wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false };
        let primitive = wgpu::PrimitiveState { cull_mode: None, ..Default::default() };
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stagecraft-3d mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mesh"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32, 3 => Uint32],
                })],
                compilation_options: Default::default(),
            },
            primitive,
            depth_stencil: Some(depth(true)),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_mesh"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stagecraft-3d grid"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grid"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive,
            depth_stencil: Some(depth(false)),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_grid"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stagecraft-3d blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stagecraft-3d blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        // egui's own pass: one sample, no depth, egui's colour format.
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stagecraft-3d blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit,
                entry_point: Some("vs_blit"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit,
                entry_point: Some(if target_format.is_srgb() { "fs_blit_linear" } else { "fs_blit_gamma" }),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("stagecraft-3d blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            mesh_pipeline,
            grid_pipeline,
            blit_pipeline,
            uniform_buffer,
            uniform_group,
            blit_layout,
            sampler,
            mesh: GpuBuffers::new(),
            caps: GpuBuffers::new(),
            target: None,
        }
    }

    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let tex = |label: &str, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let msaa = tex("stagecraft-3d msaa", COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve = tex(
            "stagecraft-3d resolve",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = tex("stagecraft-3d depth", DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stagecraft-3d blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&resolve) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.target = Some(Target { size, msaa, resolve, depth, blit_group });
    }
}

impl CallbackTrait for Frame3d {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = resources.get_mut::<Gpu3d>() else { return Vec::new() };
        if self.size_px[0] == 0 || self.size_px[1] == 0 {
            return Vec::new();
        }
        let size = self.size_px.map(|s| s.min(device.limits().max_texture_dimension_2d));
        gpu.ensure_target(device, size);
        gpu.mesh.upload(device, &self.mesh);
        gpu.caps.upload(device, &self.caps);
        queue.write_buffer(&gpu.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));
        let target = gpu.target.as_ref().expect("target");
        let [r, g, b, a] = self.clear;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stagecraft-3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.msaa,
                resolve_target: Some(&target.resolve),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                    store: wgpu::StoreOp::Discard,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &gpu.uniform_group, &[]);
        pass.set_pipeline(&gpu.mesh_pipeline);
        gpu.mesh.draw(&mut pass);
        gpu.caps.draw(&mut pass);
        pass.set_pipeline(&gpu.grid_pipeline);
        pass.draw(0..6, 0..1);
        drop(pass);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(gpu) = resources.get::<Gpu3d>() else { return };
        let Some(target) = &gpu.target else { return };
        pass.set_pipeline(&gpu.blit_pipeline);
        pass.set_bind_group(0, &target.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
