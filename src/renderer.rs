use crate::{animation::Pose, format::Model};
use anyhow::Result;
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
    edge_scale: f32,
}
pub struct SceneCamera {
    pub view_projection: Mat4,
    pub light_direction: Vec3,
    pub light_color: Vec3,
    pub view: Mat4,
    pub eye: Vec3,
}
impl SceneCamera {
    pub fn orbit(size: [u32; 2], yaw: f32, pitch: f32, distance: f32, focus: Vec3) -> Self {
        let eye = focus
            + distance
                * Vec3::new(
                    yaw.sin() * pitch.cos(),
                    pitch.sin(),
                    -yaw.cos() * pitch.cos(),
                );
        Self {
            view_projection: orbit_projection(size, yaw, pitch, distance, focus),
            view: Mat4::look_at_lh(eye, focus, Vec3::Y),
            eye,
            light_direction: Vec3::new(-0.4, 0.8, -0.6),
            light_color: Vec3::ONE,
        }
    }
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
    outline_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    white: wgpu::BindGroup,
    materials: Vec<wgpu::BindGroup>,
    vertex: Option<wgpu::Buffer>,
    index: Option<wgpu::Buffer>,
    output: wgpu::Texture,
    multisample: wgpu::Texture,
    depth: wgpu::Texture,
    pub texture_id: egui::TextureId,
    size: [u32; 2],
    pub warnings: Vec<String>,
}
fn target(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    target_multisample(device, size, format, 1)
}
fn target_multisample(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewport target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: if samples == 1 {
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        },
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
            size: 192,
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
            label: Some("MMD material layout"),
            entries: &[0, 2, 3]
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                })
                .into_iter()
                .chain([1, 4].map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                }))
                .chain([wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }])
                .collect::<Vec<_>>(),
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
        let make_pipeline = |outline| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if outline { "MMD outlines" } else { "MMD mesh" }),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(if outline { "vs_outline" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x4,4=>Float32],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if outline { "fs_outline" } else { "fs_main" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                front_face: if outline { wgpu::FrontFace::Cw } else { wgpu::FrontFace::Ccw },
                cull_mode: if outline { Some(wgpu::Face::Front) } else { None },
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(!outline),
                depth_compare: Some(if outline { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Less }),
                stencil: Default::default(), bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: 4, ..Default::default() },
            multiview_mask: None, cache: None,
        })
        };
        let pipeline = make_pipeline(false);
        let outline_pipeline = make_pipeline(true);
        let size = [800, 600];
        let output = target(device, size, wgpu::TextureFormat::Rgba8UnormSrgb);
        let multisample = target_multisample(device, size, wgpu::TextureFormat::Rgba8UnormSrgb, 4);
        let depth = target_multisample(device, size, wgpu::TextureFormat::Depth32Float, 4);
        let texture_id = state.renderer.write().register_native_texture(
            device,
            &output.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            }),
            wgpu::FilterMode::Linear,
        );
        let white = Self::material_group(
            &state,
            &texture_layout,
            &image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
            &image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
            &image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
            &[0.; 16],
        );
        Self {
            state,
            pipeline,
            outline_pipeline,
            camera_buffer,
            camera_group,
            texture_layout,
            white,
            materials: Vec::new(),
            vertex: None,
            index: None,
            output,
            multisample,
            depth,
            texture_id,
            size,
            warnings: Vec::new(),
        }
    }
    fn texture_view(state: &egui_wgpu::RenderState, image: &image::RgbaImage) -> wgpu::TextureView {
        state
            .device
            .create_texture_with_data(
                &state.queue,
                &wgpu::TextureDescriptor {
                    label: Some("MMD texture"),
                    size: wgpu::Extent3d {
                        width: image.width(),
                        height: image.height(),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                image,
            )
            .create_view(&Default::default())
    }
    fn material_group(
        state: &egui_wgpu::RenderState,
        layout: &wgpu::BindGroupLayout,
        diffuse: &image::RgbaImage,
        sphere: &image::RgbaImage,
        toon: &image::RgbaImage,
        values: &[f32; 16],
    ) -> wgpu::BindGroup {
        let diffuse = Self::texture_view(state, diffuse);
        let sphere = Self::texture_view(state, sphere);
        let toon = Self::texture_view(state, toon);
        let repeat = state.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let clamp = state.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = state
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("MMD material values"),
                contents: bytemuck::cast_slice(values),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        state.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("MMD material"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&repeat),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&sphere),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&toon),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&clamp),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: uniform.as_entire_binding(),
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
            (count as u64).saturating_mul(std::mem::size_of::<GpuVertex>() as u64)
                <= self.state.device.limits().max_buffer_size,
            "Scene exceeds GPU vertex buffer limit"
        );
        self.warnings.clear();
        self.materials.clear();
        for &(model, path) in models {
            for material in &model.materials {
                let mut read_image = |name: &str, shared: bool| -> Option<image::RgbaImage> {
                    if name.is_empty() {
                        return None;
                    }
                    let parent = path.parent().unwrap_or(Path::new("."));
                    let mut candidates = vec![parent.join(name.replace('\\', "/"))];
                    if shared {
                        candidates.push(parent.join("../../Data").join(name));
                        candidates.push(parent.join("../Data").join(name));
                    }
                    for candidate in &candidates {
                        if let Some(bytes) = loader(candidate) {
                            match image::load_from_memory(&bytes) {
                                Ok(mut img) => {
                                    let limit = self.state.device.limits().max_texture_dimension_2d;
                                    if img.width() > limit || img.height() > limit {
                                        img = img.thumbnail(limit, limit);
                                        self.warnings.push(format!(
                                            "Resized texture {} to GPU limit {limit}",
                                            candidate.display()
                                        ));
                                    }
                                    return Some(img.to_rgba8());
                                }
                                Err(error) => self
                                    .warnings
                                    .push(format!("Texture {}: {error}", candidate.display())),
                            }
                        }
                    }
                    self.warnings
                        .push(format!("Missing texture {name}: using fallback"));
                    None
                };
                let white = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
                let diffuse = read_image(&material.texture, false).unwrap_or_else(|| white.clone());
                let sphere = read_image(&material.sphere_texture, false);
                let toon = read_image(&material.toon_texture, true);
                let mut values = [0.; 16];
                values[..3].copy_from_slice(&material.ambient.to_array());
                values[4..7].copy_from_slice(&material.specular.to_array());
                values[7] = material.shininess.max(0.);
                values[8] = if sphere.is_some() && material.sphere_mode <= 2 {
                    material.sphere_mode as f32
                } else {
                    0.
                };
                values[9] = if toon.is_some() { 1. } else { 0. };
                values[10] = if material.edge_enabled {
                    material.edge_size.max(0.)
                } else {
                    0.
                };
                values[12..16].copy_from_slice(&material.edge_color);
                if material.sphere_mode == 3 {
                    self.warnings.push(
                        "PMX sphere subtexture (extra UV) is retained but not rendered".into(),
                    );
                }
                let group = Self::material_group(
                    &self.state,
                    &self.texture_layout,
                    &diffuse,
                    sphere.as_ref().unwrap_or(&white),
                    toon.as_ref().unwrap_or(&white),
                    &values,
                );
                self.materials.push(group);
            }
        }
        let total: usize = models.iter().map(|(m, _)| m.indices.len()).sum();
        let count = total.max(1);
        self.vertex = Some(self.state.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skinned vertices"),
            size: (count * std::mem::size_of::<GpuVertex>()) as u64,
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
        let camera = SceneCamera::orbit(size, yaw, pitch, distance, focus);
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
            self.multisample =
                target_multisample(device, size, wgpu::TextureFormat::Rgba8UnormSrgb, 4);
            self.depth = target_multisample(device, size, wgpu::TextureFormat::Depth32Float, 4);
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
        let mut uniform = [0f32; 48];
        uniform[..16].copy_from_slice(&camera.view_projection.to_cols_array());
        uniform[16..19].copy_from_slice(&camera.light_direction.to_array());
        uniform[20..23].copy_from_slice(&camera.light_color.to_array());
        uniform[24..40].copy_from_slice(&camera.view.to_cols_array());
        uniform[40..43].copy_from_slice(&camera.eye.to_array());
        uniform[44..46].copy_from_slice(&[size[0] as f32, size[1] as f32]);
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
                            edge_scale: v.edge_scale.max(0.) * model.outline_scale.max(0.),
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
            let resolve = self.output.create_view(&Default::default());
            let view = self.multisample.create_view(&Default::default());
            let depth = self.depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: Some(&resolve),
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
                for &(model, pose) in models {
                    for material in &model.materials {
                        pass.set_bind_group(1, &self.materials[index], &[]);
                        if pose.visible {
                            pass.draw_indexed(
                                offset + material.start..offset + material.start + material.count,
                                0,
                                0..1,
                            );
                        }
                        index += 1;
                    }
                    offset += model.indices.len() as u32;
                }
                pass.set_pipeline(&self.outline_pipeline);
                let mut index = 0;
                let mut offset = 0;
                for &(model, pose) in models {
                    for material in &model.materials {
                        if pose.visible
                            && material.edge_enabled
                            && material.edge_size > 0.
                            && material.edge_color[3] > 0.
                        {
                            pass.set_bind_group(1, &self.materials[index], &[]);
                            pass.draw_indexed(
                                offset + material.start..offset + material.start + material.count,
                                0,
                                0..1,
                            );
                        }
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
        fn read_output(renderer: &Renderer) -> Vec<u8> {
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
            buffer.slice(..).get_mapped_range().to_vec()
        }
        let data = read_output(&renderer);
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
        let mut hidden = pose.clone();
        hidden.visible = false;
        renderer.draw_scene(
            &[(&model, &hidden)],
            [512, 512],
            0.,
            0.12,
            38.,
            Vec3::new(0., 10., 0.),
        );
        let hidden_output = read_output(&renderer);
        assert!(
            hidden_output
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel.as_slice() == background.as_slice()),
            "hidden model still rendered"
        );
        // Keep material and index offsets for hidden models before visible models.
        renderer
            .load_scene(&[(&model, path), (&model, path)])
            .unwrap();
        renderer.draw_scene(
            &[(&model, &hidden), (&model, &pose)],
            [512, 512],
            0.,
            0.12,
            38.,
            Vec3::new(0., 10., 0.),
        );
        assert_eq!(read_output(&renderer), data);
        // A bright edge color separates outline coverage from the shaded mesh.
        let mut edged = model.clone();
        for material in &mut edged.materials {
            material.edge_enabled = true;
            material.edge_size = 3.;
            material.edge_color = [1., 0., 0., 1.];
        }
        for vertex in &mut edged.vertices {
            vertex.edge_scale = 1.;
        }
        let red_pixels = |pixels: &[u8]| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] > 200 && p[1] < 20 && p[2] < 20)
                .count()
        };
        renderer.load(&edged, path).unwrap();
        renderer.draw_scene(
            &[(&edged, &pose)],
            [512, 512],
            0.,
            0.12,
            38.,
            Vec3::new(0., 10., 0.),
        );
        let count = red_pixels(&read_output(&renderer));
        assert!(
            count > 100 && count < foreground / 3,
            "unexpected outline coverage: {count} / {foreground}"
        );
        let mut ortho = SceneCamera::orbit([512, 512], 0., 0.12, 38., Vec3::new(0., 10., 0.));
        ortho.view_projection =
            Mat4::orthographic_lh(-15., 15., -15., 15., 0.1, 10000.) * ortho.view;
        renderer.draw_scene_camera(&[(&edged, &pose)], [512, 512], &ortho);
        assert!(
            red_pixels(&read_output(&renderer)) > 100,
            "orthographic outlines missing"
        );
        for material in &mut edged.materials {
            material.edge_color[3] = 0.;
        }
        renderer.load(&edged, path).unwrap();
        renderer.draw_scene_camera(&[(&edged, &pose)], [512, 512], &ortho);
        assert_eq!(
            red_pixels(&read_output(&renderer)),
            0,
            "transparent edge still rendered"
        );
        for material in &mut edged.materials {
            material.edge_color[3] = 1.;
        }
        renderer.load(&edged, path).unwrap();
        edged.outline_scale = 0.;
        renderer.draw_scene_camera(&[(&edged, &pose)], [512, 512], &ortho);
        assert_eq!(
            red_pixels(&read_output(&renderer)),
            0,
            "zero model scale still draws outlines"
        );
        edged.outline_scale = 1.;
        for vertex in &mut edged.vertices {
            vertex.edge_scale = 0.;
        }
        renderer.draw_scene(
            &[(&edged, &pose)],
            [512, 512],
            0.,
            0.12,
            38.,
            Vec3::new(0., 10., 0.),
        );
        assert_eq!(
            red_pixels(&read_output(&renderer)),
            0,
            "zero vertex scale still draws outlines"
        );
        for vertex in &mut edged.vertices {
            vertex.edge_scale = 1.;
        }
        for material in &mut edged.materials {
            material.edge_enabled = false;
        }
        renderer.load(&edged, path).unwrap();
        renderer.draw_scene(
            &[(&edged, &pose)],
            [512, 512],
            0.,
            0.12,
            38.,
            Vec3::new(0., 10., 0.),
        );
        assert_eq!(
            red_pixels(&read_output(&renderer)),
            0,
            "disabled edge still draws outlines"
        );
        // Render constant material probes so the shader's gamma, sphere modes and toon
        // bindings are checked on the GPU rather than inferred from the CPU importer.
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([51, 102, 153, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
        for (mode, toon, expected) in [
            (0, false, [51_u8, 51, 51]),
            (1, false, [10, 20, 31]),
            (2, false, [102, 153, 204]),
            (0, true, [10, 20, 31]),
        ] {
            let mut probe = model.clone();
            for material in &mut probe.materials {
                material.diffuse = [0., 0., 0., 1.];
                material.ambient = Vec3::splat(0.2);
                material.specular = Vec3::ZERO;
                material.texture.clear();
                material.sphere_texture = "probe.png".into();
                material.sphere_mode = mode;
                material.toon_texture = if toon {
                    "probe.png".into()
                } else {
                    String::new()
                };
            }
            renderer
                .load_scene_with_textures(&[(&probe, path)], |_| Some(encoded.get_ref().clone()))
                .unwrap();
            renderer.draw_scene(
                &[(&probe, &pose)],
                [512, 512],
                0.,
                0.12,
                38.,
                Vec3::new(0., 10., 0.),
            );
            let output = read_output(&renderer);
            let matches = output
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| {
                    (0..3).all(|channel| pixel[channel].abs_diff(expected[channel]) <= 1)
                })
                .count();
            assert!(matches>5000,"mode {mode}, toon {toon}: only {matches} pixels matched the expected material color");
        }
    }
}
