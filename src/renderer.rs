use crate::{animation::Pose, format::Model};
use anyhow::{Context, Result};
use eframe::{egui, egui_wgpu, wgpu};
use glam::{Mat4, Vec3};
use std::path::Path;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}
pub struct SceneCamera {
    pub view_projection: Mat4,
    pub light_direction: Vec3,
    pub light_color: Vec3,
}
pub fn orbit_projection(size: [u32; 2], yaw: f32, pitch: f32, distance: f32, focus: Vec3) -> Mat4 {
    let eye = focus
        + distance
            * Vec3::new(
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                -yaw.cos() * pitch.cos(),
            );
    let mirror = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
    Mat4::perspective_rh(
        45f32.to_radians(),
        size[0].max(1) as f32 / size[1].max(1) as f32,
        0.1,
        10000.0,
    ) * Mat4::look_at_rh(
        mirror.transform_point3(eye),
        mirror.transform_point3(focus),
        Vec3::Y,
    ) * mirror
}
pub struct Renderer {
    state: egui_wgpu::RenderState,
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    white: wgpu::BindGroup,
    materials: Vec<wgpu::BindGroup>,
    vertex: Option<wgpu::Buffer>,
    index: Option<wgpu::Buffer>,
    output: wgpu::Texture,
    depth: wgpu::Texture,
    pub texture_id: egui::TextureId,
    size: [u32; 2],
    pub warnings: Vec<String>,
}
fn target(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewport target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: if format == wgpu::TextureFormat::Rgba8UnormSrgb {
            &[wgpu::TextureFormat::Rgba8Unorm]
        } else {
            &[]
        },
    })
}
impl Renderer {
    pub fn new(state: egui_wgpu::RenderState) -> Self {
        let device = &state.device;
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PMD toon shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()),
        });
        let pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:Some("PMD mesh"),layout:Some(&layout),vertex:wgpu::VertexState { module:&shader,entry_point:Some("vs_main"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:48,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x4]}] },
            fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState { format:wgpu::TextureFormat::Rgba8UnormSrgb,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL })]}),
            primitive:wgpu::PrimitiveState { cull_mode:None,..Default::default() },
            depth_stencil:Some(wgpu::DepthStencilState { format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:Some(true),depth_compare:Some(wgpu::CompareFunction::Less),stencil:Default::default(),bias:Default::default() }),multisample:Default::default(),multiview_mask:None,cache:None,
        });
        let size = [800, 600];
        let output = target(device, size, wgpu::TextureFormat::Rgba8UnormSrgb);
        let depth = target(device, size, wgpu::TextureFormat::Depth32Float);
        let texture_id = state.renderer.write().register_native_texture(
            device,
            &output.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            }),
            wgpu::FilterMode::Linear,
        );
        let white = Self::texture(&state, &texture_layout, &[255; 4], 1, 1);
        Self {
            state,
            pipeline,
            camera_buffer,
            camera_group,
            texture_layout,
            white,
            materials: Vec::new(),
            vertex: None,
            index: None,
            output,
            depth,
            texture_id,
            size,
            warnings: Vec::new(),
        }
    }
    fn texture(
        state: &egui_wgpu::RenderState,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        width: u32,
        height: u32,
    ) -> wgpu::BindGroup {
        let texture = state.device.create_texture_with_data(
            &state.queue,
            &wgpu::TextureDescriptor {
                label: Some("material texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytes,
        );
        let sampler = state.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        state.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    }
    pub fn load(&mut self, model: &Model, path: &Path) -> Result<()> {
        self.load_scene(&[(model, path)])
    }
    pub fn load_scene(&mut self, models: &[(&Model, &Path)]) -> Result<()> {
        self.load_scene_with_textures(models, |path| std::fs::read(path).ok())
    }
    pub fn load_scene_with_textures(
        &mut self,
        models: &[(&Model, &Path)],
        mut loader: impl FnMut(&Path) -> Option<Vec<u8>>,
    ) -> Result<()> {
        let count: usize = models.iter().map(|(model, _)| model.indices.len()).sum();
        anyhow::ensure!(
            (count as u64).saturating_mul(48) <= self.state.device.limits().max_buffer_size,
            "Scene exceeds GPU vertex buffer limit"
        );
        self.warnings.clear();
        self.materials.clear();
        for &(model, path) in models {
            for material in &model.materials {
                let diffuse = material.texture.split('*').find(|s| {
                    !s.is_empty()
                        && !s.to_lowercase().ends_with(".sph")
                        && !s.to_lowercase().ends_with(".spa")
                });
                let group = if let Some(name) = diffuse {
                    let image_path = path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(name.replace('\\', "/"));
                    let decoded = loader(&image_path)
                        .context("texture file not supplied")
                        .and_then(|bytes| {
                            if let Ok(format) = image::ImageFormat::from_path(&image_path) {
                                Ok(image::load_from_memory_with_format(&bytes, format)?)
                            } else {
                                Ok(image::load_from_memory(&bytes)?)
                            }
                        })
                        .with_context(|| format!("texture {}", image_path.display()));
                    match decoded {
                        Ok(mut img) => {
                            let limit = self.state.device.limits().max_texture_dimension_2d;
                            if img.width() > limit || img.height() > limit {
                                self.warnings.push(format!(
                                    "Resized texture {} to GPU limit {limit}",
                                    image_path.display()
                                ));
                                img = img.thumbnail(limit, limit);
                            }
                            let img = img.to_rgba8();
                            Self::texture(
                                &self.state,
                                &self.texture_layout,
                                &img,
                                img.width(),
                                img.height(),
                            )
                        }
                        Err(e) => {
                            self.warnings.push(format!("{e}"));
                            Self::texture(&self.state, &self.texture_layout, &[255; 4], 1, 1)
                        }
                    }
                } else {
                    Self::texture(&self.state, &self.texture_layout, &[255; 4], 1, 1)
                };
                self.materials.push(group);
            }
        }
        let total: usize = models.iter().map(|(m, _)| m.indices.len()).sum();
        let count = total.max(1);
        self.vertex = Some(self.state.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skinned vertices"),
            size: (count * 48) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        let data: Vec<u32> = if total == 0 {
            vec![0]
        } else {
            (0..total as u32).collect()
        };
        self.index = Some(self.state.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("indices"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::INDEX,
            },
        ));
        Ok(())
    }
    pub fn draw_scene(
        &mut self,
        models: &[(&Model, &Pose)],
        size: [u32; 2],
        yaw: f32,
        pitch: f32,
        distance: f32,
        focus: Vec3,
    ) {
        let camera = SceneCamera {
            view_projection: orbit_projection(size, yaw, pitch, distance, focus),
            light_direction: Vec3::new(-0.4, 0.8, -0.6),
            light_color: Vec3::ONE,
        };
        self.draw_scene_camera(models, size, &camera);
    }
    pub fn draw_scene_camera(
        &mut self,
        models: &[(&Model, &Pose)],
        size: [u32; 2],
        camera: &SceneCamera,
    ) {
        let device = &self.state.device;
        let size = size.map(|s| s.clamp(1, 4096));
        if size != self.size {
            self.size = size;
            self.output = target(device, size, wgpu::TextureFormat::Rgba8UnormSrgb);
            self.depth = target(device, size, wgpu::TextureFormat::Depth32Float);
            self.state
                .renderer
                .write()
                .update_egui_texture_from_wgpu_texture(
                    device,
                    &self.output.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(wgpu::TextureFormat::Rgba8Unorm),
                        ..Default::default()
                    }),
                    wgpu::FilterMode::Linear,
                    self.texture_id,
                );
        }
        let mut uniform = [0f32; 24];
        uniform[..16].copy_from_slice(&camera.view_projection.to_cols_array());
        uniform[16..19].copy_from_slice(&camera.light_direction.to_array());
        uniform[20..23].copy_from_slice(&camera.light_color.to_array());
        self.state
            .queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&uniform));
        if let Some(buffer) = self.vertex.as_ref() {
            let mut vertices = Vec::new();
            for &(model, pose) in models {
                let skinned = pose.skin(model);
                // Expand per material so a shared source vertex can have different material colors.
                for material in &model.materials {
                    for &index in &model.indices
                        [material.start as usize..(material.start + material.count) as usize]
                    {
                        let v = &model.vertices[index as usize];
                        let (p, n) = skinned[index as usize];
                        vertices.push(GpuVertex {
                            position: p.into(),
                            normal: n.into(),
                            uv: v.uv.into(),
                            color: material.diffuse,
                        });
                    }
                }
            }
            if !vertices.is_empty() {
                self.state
                    .queue
                    .write_buffer(buffer, 0, bytemuck::cast_slice(&vertices));
            }
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = self.output.create_view(&Default::default());
            let depth = self.depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.012,
                            g: 0.016,
                            b: 0.023,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_group, &[]);
            pass.set_bind_group(1, &self.white, &[]);
            if let (Some(v), Some(i)) = (self.vertex.as_ref(), self.index.as_ref()) {
                pass.set_vertex_buffer(0, v.slice(..));
                pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                let mut index = 0;
                let mut offset = 0;
                for &(model, _) in models {
                    for material in &model.materials {
                        pass.set_bind_group(1, &self.materials[index], &[]);
                        pass.draw_indexed(
                            offset + material.start..offset + material.start + material.count,
                            0,
                            0..1,
                        );
                        index += 1;
                    }
                    offset += model.indices.len() as u32;
                }
            }
        }
        self.state.queue.submit(Some(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    #[test]
    #[ignore = "requires a GPU and original PMD assets; writes reference/preview.png"]
    fn renders_original_miku_on_gpu() {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("GPU adapter");
        println!("GPU: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("smoke test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
            ..Default::default()
        }))
        .unwrap();
        let ui_renderer = egui_wgpu::Renderer::new(
            &device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            egui_wgpu::RendererOptions::default(),
        );
        let state = egui_wgpu::RenderState {
            adapter,
            available_adapters: vec![],
            device,
            queue,
            surface_config: egui_wgpu::SurfaceConfig::LOW_LATENCY,
            target_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            renderer: Arc::new(egui::epaint::mutex::RwLock::new(ui_renderer)),
        };
        let path = Path::new("reference/original/UserFile/Model/Miku_Hatsune.pmd");
        let model = Model::parse(&std::fs::read(path).unwrap()).unwrap();
        let pose = Pose::sample(&model, &Default::default(), 0.0);
        let mut renderer = Renderer::new(state);
        renderer.load(&model, path).unwrap();
        assert!(renderer.warnings.is_empty(), "{:?}", renderer.warnings);
        renderer.draw_scene(
            &[(&model, &pose)],
            [512, 512],
            0.0,
            0.12,
            38.0,
            Vec3::new(0.0, 10.0, 0.0),
        );
        let device = &renderer.state.device;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 512 * 512 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &renderer.output,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512 * 4),
                    rows_per_image: Some(512),
                },
            },
            wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 1,
            },
        );
        renderer.state.queue.submit(Some(encoder.finish()));
        let (tx, rx) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let data = buffer.slice(..).get_mapped_range();
        let background = data[..4].to_vec();
        let foreground = data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p.as_slice() != background.as_slice())
            .count();
        assert!(
            foreground > 5000,
            "model did not render ({foreground} foreground pixels)"
        );
        image::save_buffer(
            "reference/preview.png",
            &data,
            512,
            512,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
