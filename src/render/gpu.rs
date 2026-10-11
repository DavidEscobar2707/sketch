//! wgpu pipeline for canvas batches, plus offscreen rendering for screenshots and PNG export.

use super::mesh::{Batch, Vertex};
use wgpu::util::DeviceExt;

pub const MSAA: u32 = 4;

/// Colors are authored in sRGB and blended in gamma space (like egui), so targets are non-sRGB.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const SHADER: &str = r#"
struct Globals { size_px: vec2<f32>, scale: f32, _pad: f32 };
@group(0) @binding(0) var<uniform> g: Globals;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    let px = pos * g.scale;
    var o: VsOut;
    o.pos = vec4<f32>(px.x / g.size_px.x * 2.0 - 1.0, 1.0 - px.y / g.size_px.y * 2.0, 0.0, 1.0);
    o.color = color;
    return o;
}

@fragment
fn fs(i: VsOut) -> @location(0) vec4<f32> {
    return i.color;
}
"#;

pub struct CanvasRenderer {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    msaa: Option<(wgpu::TextureView, [u32; 2])>,
    format: wgpu::TextureFormat,
}

impl CanvasRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> CanvasRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canvas"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas globals"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: MSAA,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        CanvasRenderer {
            pipeline,
            globals,
            bind_group,
            msaa: None,
            format,
        }
    }

    fn msaa_view(&mut self, device: &wgpu::Device, size: [u32; 2]) -> &wgpu::TextureView {
        if self.msaa.as_ref().is_none_or(|(_, s)| *s != size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("canvas msaa"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: MSAA,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.msaa = Some((tex.create_view(&Default::default()), size));
        }
        &self.msaa.as_ref().unwrap().0
    }

    /// Record a pass that clears `target` to `clear` and draws `batches`.
    /// Batch coordinates are logical px; `scale` converts to physical px.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scale: f32,
        batches: &[Batch],
        clear: [f64; 4],
    ) {
        let globals: [f32; 4] = [size[0] as f32, size[1] as f32, scale, 0.0];
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut ranges = Vec::new();
        for b in batches {
            let base = vertices.len() as u32;
            let start = indices.len() as u32;
            vertices.extend_from_slice(&b.mesh.vertices);
            indices.extend(b.mesh.indices.iter().map(|i| i + base));
            ranges.push((start..indices.len() as u32, b.clip));
        }
        let buffers = (!indices.is_empty()).then(|| {
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("canvas vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("canvas indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            (vb, ib)
        });

        let msaa = self.msaa_view(device, size).clone();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("canvas"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa,
                depth_slice: None,
                resolve_target: Some(target),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear[0],
                        g: clear[1],
                        b: clear[2],
                        a: clear[3],
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some((vb, ib)) = &buffers else { return };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        for (range, clip) in ranges {
            let (x, y, w, h) = match clip {
                Some(c) => {
                    let x0 = (c.x * scale as f64).floor().clamp(0.0, size[0] as f64);
                    let y0 = (c.y * scale as f64).floor().clamp(0.0, size[1] as f64);
                    let x1 = (c.right() * scale as f64).ceil().clamp(0.0, size[0] as f64);
                    let y1 = (c.bottom() * scale as f64)
                        .ceil()
                        .clamp(0.0, size[1] as f64);
                    (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
                }
                None => (0, 0, size[0], size[1]),
            };
            if w == 0 || h == 0 || range.is_empty() {
                continue;
            }
            pass.set_scissor_rect(x, y, w, h);
            pass.draw_indexed(range, 0, 0..1);
        }
    }
}

/// A device without a window, for tests, screenshots and PNG export.
pub struct Headless {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
}

impl Headless {
    pub fn new() -> Result<Headless, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter available: {e}"))?;
        let adapter_info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("paper-rs headless"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("failed to create device: {e}"))?;
        Ok(Headless {
            device,
            queue,
            adapter_info,
        })
    }

    /// Create a render target texture of `size` physical px.
    pub fn target(&self, size: [u32; 2]) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Copy a texture back to the CPU as RGBA8.
    pub fn read(&self, tex: &wgpu::Texture) -> image::RgbaImage {
        let (w, h) = (tex.width(), tex.height());
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let data = slice.get_mapped_range().expect("mapped readback buffer");
        let mut img = image::RgbaImage::new(w, h);
        for y in 0..h {
            let src = &data[(y * row) as usize..(y * row + w * 4) as usize];
            img.as_mut()[(y * w * 4) as usize..((y + 1) * w * 4) as usize].copy_from_slice(src);
        }
        img
    }

    /// Render batches into a new image of `size` physical px.
    pub fn render_batches(
        &self,
        batches: &[Batch],
        size: [u32; 2],
        scale: f32,
        clear: [f64; 4],
    ) -> image::RgbaImage {
        let tex = self.target(size);
        let view = tex.create_view(&Default::default());
        let mut r = CanvasRenderer::new(&self.device, OFFSCREEN_FORMAT);
        let mut enc = self.device.create_command_encoder(&Default::default());
        r.render(
            &self.device,
            &self.queue,
            &mut enc,
            &view,
            size,
            scale,
            batches,
            clear,
        );
        self.queue.submit([enc.finish()]);
        self.read(&tex)
    }
}
