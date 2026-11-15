use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use engine_platform::winit::window::Window;
use glam::{Mat3, Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::mesh::{Mesh, MeshData, Vertex};
use crate::texture::{mip_chain, Texture};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Why the renderer could not start or a resource could not be created.
#[derive(Debug)]
pub enum RenderError {
    /// The window cannot be drawn to.
    Surface(String),
    /// No usable graphics adapter (GPU and driver) was found.
    NoAdapter(String),
    /// The graphics device could not be opened.
    Device(String),
    /// An image file could not be decoded.
    Image(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Surface(e) => write!(f, "cannot draw to the window: {e}"),
            RenderError::NoAdapter(e) => write!(f, "no usable graphics adapter: {e}"),
            RenderError::Device(e) => write!(f, "cannot open the graphics device: {e}"),
            RenderError::Image(e) => write!(f, "cannot decode image: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
    ambient: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    /// Inverse-transpose of the model's 3x3 part, so normals stay
    /// perpendicular under non-uniform scale.
    normal: [[f32; 4]; 3],
    color: [f32; 4],
}

impl Instance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
        10 => Float32x4,
    ];
}

struct DrawCommand {
    mesh: Mesh,
    texture: Texture,
}

/// Draws the scene to the window.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    white: Texture,

    /// Draws queued for the current frame; `instances[i]` belongs to `commands[i]`.
    commands: Vec<DrawCommand>,
    instances: Vec<Instance>,

    /// Background colour (linear RGB).
    pub clear_color: Vec3,
    /// Direction the directional light travels in.
    pub light_direction: Vec3,
    /// Directional light colour (linear RGB).
    pub light_color: Vec3,
    /// Light that reaches every surface regardless of direction.
    pub ambient_color: Vec3,
}

impl Renderer {
    /// Sets up rendering to `window`. Blocks while the GPU is initialised.
    pub fn new(window: Arc<Window>) -> Result<Self, RenderError> {
        pollster::block_on(Self::new_async(window))
    }

    async fn new_async(window: Arc<Window>) -> Result<Self, RenderError> {
        let size = window.inner_size();

        // `from_env` honours WGPU_BACKEND etc., handy for forcing a backend
        // when a driver misbehaves.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window)
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        log::info!("graphics: {} ({:?})", info.name, info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("engine device"),
                ..Default::default()
            })
            .await
            .map_err(|e| RenderError::Device(e.to_string()))?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| RenderError::Surface("adapter cannot present to it".to_owned()))?;
        // An sRGB target makes the GPU convert our linear colours for display.
        let formats = surface.get_capabilities(&adapter).formats;
        if let Some(srgb) = formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, &config);

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
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
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture layout"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("default sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &Vertex::ATTRIBUTES,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &Instance::ATTRIBUTES,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(config.format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let instance_capacity = 256;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);
        let white = upload_texture(&device, &queue, &texture_layout, &sampler, 1, 1, &[255; 4]);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            pipeline,
            globals_buffer,
            globals_bind_group,
            texture_layout,
            sampler,
            instance_buffer,
            instance_capacity,
            white,
            commands: Vec::new(),
            instances: Vec::new(),
            clear_color: Vec3::new(0.02, 0.02, 0.03),
            light_direction: Vec3::new(-0.4, -1.0, -0.3),
            light_color: Vec3::splat(1.0),
            ambient_color: Vec3::splat(0.15),
        })
    }

    /// Uploads mesh geometry to the GPU.
    pub fn create_mesh(&self, data: &MeshData) -> Mesh {
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertices"),
                contents: bytemuck::cast_slice(&data.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh indices"),
                contents: bytemuck::cast_slice(&data.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Mesh {
            vertex_buffer,
            index_buffer,
            index_count: data.indices.len() as u32,
        }
    }

    /// Creates a texture from raw pixels: `width * height` RGBA values, 8 bits
    /// per channel, sRGB, rows top to bottom.
    ///
    /// # Panics
    /// If `rgba` is not exactly `width * height * 4` bytes or a side is zero.
    pub fn create_texture(&self, width: u32, height: u32, rgba: &[u8]) -> Texture {
        assert!(width > 0 && height > 0, "texture size must be non-zero");
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "pixel data does not match {width}x{height} RGBA"
        );
        upload_texture(
            &self.device,
            &self.queue,
            &self.texture_layout,
            &self.sampler,
            width,
            height,
            rgba,
        )
    }

    /// Decodes an image file held in memory (PNG or JPEG) into a texture.
    pub fn load_texture(&self, file_bytes: &[u8]) -> Result<Texture, RenderError> {
        let image = image::load_from_memory(file_bytes)
            .map_err(|e| RenderError::Image(e.to_string()))?
            .into_rgba8();
        Ok(self.create_texture(image.width(), image.height(), &image))
    }

    /// A plain white texture, for drawing meshes in a flat colour.
    pub fn white_texture(&self) -> Texture {
        self.white.clone()
    }

    /// Queues a mesh to be drawn this frame at `transform` (model-to-world).
    pub fn draw(&mut self, mesh: &Mesh, texture: &Texture, transform: Mat4) {
        self.draw_tinted(mesh, texture, transform, Vec4::ONE);
    }

    /// Like [`draw`](Self::draw), with the texture multiplied by `color` (linear RGBA).
    pub fn draw_tinted(&mut self, mesh: &Mesh, texture: &Texture, transform: Mat4, color: Vec4) {
        let normal = Mat3::from_mat4(transform).inverse().transpose();
        self.instances.push(Instance {
            model: transform.to_cols_array_2d(),
            normal: [
                normal.x_axis.extend(0.0).to_array(),
                normal.y_axis.extend(0.0).to_array(),
                normal.z_axis.extend(0.0).to_array(),
            ],
            color: color.to_array(),
        });
        self.commands.push(DrawCommand {
            mesh: mesh.clone(),
            texture: texture.clone(),
        });
    }

    /// Current drawable size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Call when the window's drawable size changes.
    pub fn resize(&mut self, width: u32, height: u32) {
        // A minimised window reports 0x0, which cannot be configured.
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.reconfigure();
    }

    fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
    }

    /// Renders everything queued since the last call and presents it.
    ///
    /// Returns `false` if no frame was shown (window hidden or being resized);
    /// the caller should then pace itself, as vsync did not block.
    pub fn render(&mut self, camera: &Camera) -> bool {
        let presented = self.render_queued(camera);
        self.commands.clear();
        self.instances.clear();
        presented
    }

    fn render_queued(&mut self, camera: &Camera) -> bool {
        use wgpu::CurrentSurfaceTexture as Current;
        let (frame, suboptimal) = match self.surface.get_current_texture() {
            Current::Success(frame) => (frame, false),
            Current::Suboptimal(frame) => (frame, true),
            Current::Outdated | Current::Lost => {
                self.reconfigure();
                return false;
            }
            Current::Timeout | Current::Occluded => return false,
            Current::Validation => {
                log::error!("could not acquire a frame to draw to");
                return false;
            }
        };

        let aspect = self.config.width as f32 / self.config.height as f32;
        let globals = Globals {
            view_proj: camera.view_projection(aspect).to_cols_array_2d(),
            light_dir: self
                .light_direction
                .normalize_or_zero()
                .extend(0.0)
                .to_array(),
            light_color: self.light_color.extend(1.0).to_array(),
            ambient: self.ambient_color.extend(1.0).to_array(),
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        self.queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.clear_color.x as f64,
                            g: self.clear_color.y as f64,
                            b: self.clear_color.z as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            for (i, command) in self.commands.iter().enumerate() {
                let i = i as u32;
                pass.set_bind_group(1, &command.texture.bind_group, &[]);
                pass.set_vertex_buffer(0, command.mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(
                    command.mesh.index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                pass.draw_indexed(0..command.mesh.index_count, 0, i..i + 1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);

        if suboptimal {
            // Still shown, but the surface no longer matches the window exactly.
            self.reconfigure();
        }
        true
    }
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: (capacity * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Texture {
    let levels = mip_chain(width, height, rgba);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, pixels)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Texture {
        bind_group,
        width,
        height,
    }
}
