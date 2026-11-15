use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use engine_platform::winit::window::Window;
use glam::{Mat3, Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::light::{shadow_view_proj, PointLight, Sun, MAX_POINT_LIGHTS};
use crate::material::Material;
use crate::mesh::{Mesh, MeshData, MeshGpu, Vertex};
use crate::texture::{mip_chain, Texture, TextureGpu};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The scene is lit in linear floating point so bright lights do not clip
/// before tone mapping.
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Antialiasing samples per pixel in the main pass.
const SAMPLES: u32 = 4;
const SHADOW_RESOLUTION: u32 = 2048;

/// Why the renderer could not start or a resource could not be created.
#[derive(Debug)]
pub enum RenderError {
    /// The window cannot be drawn to.
    Surface(String),
    /// No usable graphics adapter (GPU and driver) was found.
    NoAdapter(String),
    /// The graphics device could not be opened.
    Device(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Surface(e) => write!(f, "cannot draw to the window: {e}"),
            RenderError::NoAdapter(e) => write!(f, "no usable graphics adapter: {e}"),
            RenderError::Device(e) => write!(f, "cannot open the graphics device: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    camera_position: [f32; 4],
    camera_right: [f32; 4],
    camera_up: [f32; 4],
    ambient_sky: [f32; 4],
    ambient_ground: [f32; 4],
    sun_direction: [f32; 4],
    sun_color: [f32; 4],
    shadow: [f32; 4],
    point_count: [u32; 4],
    /// Per light: position xyz + range, then colour rgb + unused.
    points: [[f32; 8]; MAX_POINT_LIGHTS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    /// Inverse-transpose of the model's 3x3 part, so normals stay
    /// perpendicular under non-uniform scale.
    normal: [[f32; 4]; 3],
    base_color: [f32; 4],
    /// x: metallic, y: roughness, z: 0 for a rigid mesh, otherwise one more
    /// than the index of its first joint matrix this frame.
    surface: [f32; 4],
    emissive: [f32; 4],
}

impl Instance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
        10 => Float32x4, 11 => Float32x4, 12 => Float32x4,
    ];
}

/// Something drawn on top of the finished frame, such as a user interface.
pub trait Overlay {
    /// Called before any pass, to upload what [`paint`](Self::paint) needs.
    /// `size` is the drawable size in pixels. Returned command buffers are
    /// submitted ahead of the frame's own.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) -> Vec<wgpu::CommandBuffer>;

    /// Draws into the window's colour target, after tone mapping.
    fn paint(&mut self, pass: &mut wgpu::RenderPass<'static>);
}

/// How a particle's colour combines with what is behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleBlend {
    /// Covers what is behind it according to its alpha: smoke, dust.
    Alpha,
    /// Adds its light to what is behind it: fire, sparks, glows.
    Additive,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleInstance {
    /// xyz: world position of the centre, w: diameter.
    position_size: [f32; 4],
    color: [f32; 4],
}

impl ParticleInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
}

struct DrawCommand {
    mesh: Mesh,
    texture: Texture,
}

/// Render targets that depend on the window size.
struct Targets {
    /// Multisampled colour; resolved into `hdr` at the end of the main pass.
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    hdr: wgpu::TextureView,
    /// Binds `hdr` for the tone-mapping pass.
    tonemap_bind_group: wgpu::BindGroup,
}

/// Draws the scene to the window.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    targets: Targets,

    mesh_pipeline: wgpu::RenderPipeline,
    alpha_particle_pipeline: wgpu::RenderPipeline,
    additive_particle_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    tonemap_pipeline: wgpu::RenderPipeline,

    globals_buffer: wgpu::Buffer,
    globals_layout: wgpu::BindGroupLayout,
    globals_bind_group: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_buffer: wgpu::Buffer,
    shadow_layout: wgpu::BindGroupLayout,
    shadow_bind_group: wgpu::BindGroup,
    /// Joint matrices of this frame's skinned meshes, on the GPU.
    joint_buffer: wgpu::Buffer,
    joint_capacity: usize,
    tonemap_layout: wgpu::BindGroupLayout,
    tonemap_buffer: wgpu::Buffer,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    particle_buffer: wgpu::Buffer,
    particle_capacity: usize,
    white: Texture,

    /// Queued for the current frame; `instances[i]` belongs to `commands[i]`.
    commands: Vec<DrawCommand>,
    instances: Vec<Instance>,
    joints: Vec<[[f32; 4]; 4]>,
    alpha_particles: Vec<ParticleInstance>,
    additive_particles: Vec<ParticleInstance>,
    sun: Option<Sun>,
    point_lights: Vec<PointLight>,

    /// Background colour (linear RGB, before tone mapping).
    pub clear_color: Vec3,
    /// Ambient light arriving from above (linear RGB).
    pub ambient_sky: Vec3,
    /// Ambient light arriving from below, e.g. bounced off the ground.
    pub ambient_ground: Vec3,
    /// Brightness multiplier applied before tone mapping.
    pub exposure: f32,
    /// Whether the sun casts shadows.
    pub shadows: bool,
    /// Shadows are drawn within roughly this distance of the camera. Smaller
    /// values give sharper shadows over a smaller area.
    pub shadow_distance: f32,
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
        // Prefer a target without automatic sRGB conversion and encode in the
        // tone-mapping shader instead: overlays such as egui blend in display
        // space and expect exactly that kind of target.
        let formats = surface.get_capabilities(&adapter).formats;
        if let Some(plain) = formats.iter().copied().find(|f| !f.is_srgb()) {
            config.format = plain;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let joints_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform_buffer = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let vertex_buffers = [
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
        ];

        // --- Shadow pass -------------------------------------------------
        let shadow_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d {
                    width: SHADOW_RESOLUTION,
                    height: SHADOW_RESOLUTION,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let shadow_buffer = uniform_buffer("shadow uniforms", 64);
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX),
                joints_entry(1),
            ],
        });
        let shadow_shader = device.create_shader_module(wgpu::include_wgsl!("shadow.wgsl"));
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow pipeline layout"),
                bind_group_layouts: &[Some(&shadow_layout)],
                immediate_size: 0,
            });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
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
                // Pushes stored depths away from the light, more so on
                // surfaces at a grazing angle to it, so a surface does not
                // shadow itself through limited depth precision.
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        // --- Main pass ---------------------------------------------------
        let globals_buffer = uniform_buffer("globals", std::mem::size_of::<Globals>() as u64);
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
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
                joints_entry(3),
            ],
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let joint_capacity = 64;
        let joint_buffer = create_joint_buffer(&device, joint_capacity);
        let (globals_bind_group, shadow_bind_group) = create_frame_bind_groups(
            &device,
            &globals_layout,
            &globals_buffer,
            &shadow_view,
            &shadow_sampler,
            &shadow_layout,
            &shadow_buffer,
            &joint_buffer,
        );

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

        let mesh_shader = device.create_shader_module(wgpu::include_wgsl!("mesh.wgsl"));
        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh pipeline"),
            layout: Some(&mesh_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
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
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Particles (drawn in the main pass, after the meshes) --------
        let particle_shader = device.create_shader_module(wgpu::include_wgsl!("particles.wgsl"));
        let particle_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("particle pipeline layout"),
                bind_group_layouts: &[Some(&globals_layout)],
                immediate_size: 0,
            });
        let particle_pipeline = |label, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&particle_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &particle_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ParticleInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &ParticleInstance::ATTRIBUTES,
                    })],
                },
                primitive: Default::default(),
                // Hidden behind solid objects, but particles do not hide one
                // another: they are see-through, so they leave depth alone.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &particle_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let alpha_particle_pipeline =
            particle_pipeline("alpha particles", wgpu::BlendState::ALPHA_BLENDING);
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let additive_particle_pipeline = particle_pipeline(
            "additive particles",
            wgpu::BlendState {
                color: additive,
                alpha: additive,
            },
        );

        // --- Tone-mapping pass -------------------------------------------
        let tonemap_buffer = uniform_buffer("tonemap uniforms", 16);
        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let tonemap_shader = device.create_shader_module(wgpu::include_wgsl!("tonemap.wgsl"));
        let tonemap_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tonemap pipeline layout"),
                bind_group_layouts: &[Some(&tonemap_layout)],
                immediate_size: 0,
            });
        let tonemap_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap pipeline"),
            layout: Some(&tonemap_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &tonemap_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &tonemap_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(config.format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let targets = create_targets(&device, &config, &tonemap_layout, &tonemap_buffer);
        let instance_capacity = 256;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);
        let particle_capacity = 1024;
        let particle_buffer = create_particle_buffer(&device, particle_capacity);
        let white = Texture::new(upload_texture(
            &device,
            &queue,
            &texture_layout,
            &sampler,
            1,
            1,
            &[255; 4],
        ));

        Ok(Self {
            surface,
            device,
            queue,
            config,
            targets,
            mesh_pipeline,
            alpha_particle_pipeline,
            additive_particle_pipeline,
            shadow_pipeline,
            tonemap_pipeline,
            globals_buffer,
            globals_layout,
            globals_bind_group,
            shadow_view,
            shadow_sampler,
            shadow_buffer,
            shadow_layout,
            shadow_bind_group,
            joint_buffer,
            joint_capacity,
            tonemap_layout,
            tonemap_buffer,
            texture_layout,
            sampler,
            instance_buffer,
            instance_capacity,
            particle_buffer,
            particle_capacity,
            white,
            commands: Vec::new(),
            instances: Vec::new(),
            joints: Vec::new(),
            alpha_particles: Vec::new(),
            additive_particles: Vec::new(),
            sun: None,
            point_lights: Vec::new(),
            clear_color: Vec3::new(0.02, 0.02, 0.03),
            ambient_sky: Vec3::new(0.18, 0.24, 0.34),
            ambient_ground: Vec3::new(0.07, 0.06, 0.05),
            exposure: 1.0,
            shadows: true,
            shadow_distance: 25.0,
        })
    }

    /// Uploads mesh geometry to the GPU.
    pub fn create_mesh(&self, data: &MeshData) -> Mesh {
        Mesh::new(upload_mesh(&self.device, data))
    }

    /// Replaces the geometry of an existing mesh; every clone of it changes.
    pub fn update_mesh(&self, mesh: &Mesh, data: &MeshData) {
        *mesh.gpu() = upload_mesh(&self.device, data);
    }

    /// Creates a texture from raw pixels: `width * height` RGBA values, 8 bits
    /// per channel, sRGB, rows top to bottom.
    ///
    /// # Panics
    /// If `rgba` is not exactly `width * height * 4` bytes or a side is zero.
    pub fn create_texture(&self, width: u32, height: u32, rgba: &[u8]) -> Texture {
        Texture::new(self.upload_texture(width, height, rgba))
    }

    /// Replaces the pixels of an existing texture (the size may differ);
    /// every clone of it changes. Panics like [`create_texture`](Self::create_texture).
    pub fn update_texture(&self, texture: &Texture, width: u32, height: u32, rgba: &[u8]) {
        *texture.gpu() = self.upload_texture(width, height, rgba);
    }

    fn upload_texture(&self, width: u32, height: u32, rgba: &[u8]) -> TextureGpu {
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

    /// The graphics device, for code that draws with `wgpu` directly.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue commands and uploads for [`device`](Self::device) go through.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Pixel format of the window's colour target.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Number of meshes queued for the current frame so far.
    pub fn queued_draws(&self) -> usize {
        self.commands.len()
    }

    /// A plain white texture.
    pub fn white_texture(&self) -> Texture {
        self.white.clone()
    }

    /// Queues a mesh to be drawn this frame at `transform` (model-to-world).
    pub fn draw(&mut self, mesh: &Mesh, material: &Material, transform: Mat4) {
        self.queue_draw(mesh, material, transform, 0.0);
    }

    /// Queues a skinned mesh: one whose vertices follow a skeleton.
    ///
    /// `joint_matrices` holds, for each joint of the mesh's skin, the joint's
    /// current world transform times its inverse bind matrix. They place the
    /// vertices in the world by themselves, so there is no model transform.
    pub fn draw_skinned(&mut self, mesh: &Mesh, material: &Material, joint_matrices: &[Mat4]) {
        if joint_matrices.is_empty() {
            return;
        }
        // Zero is reserved for "not skinned", hence the offset by one.
        let first = self.joints.len() as f32 + 1.0;
        self.joints
            .extend(joint_matrices.iter().map(Mat4::to_cols_array_2d));
        self.queue_draw(mesh, material, Mat4::IDENTITY, first);
    }

    fn queue_draw(&mut self, mesh: &Mesh, material: &Material, transform: Mat4, skin: f32) {
        let normal = Mat3::from_mat4(transform).inverse().transpose();
        self.instances.push(Instance {
            model: transform.to_cols_array_2d(),
            normal: [
                normal.x_axis.extend(0.0).to_array(),
                normal.y_axis.extend(0.0).to_array(),
                normal.z_axis.extend(0.0).to_array(),
            ],
            base_color: material.base_color.to_array(),
            surface: [material.metallic, material.roughness, skin, 0.0],
            emissive: material.emissive.extend(0.0).to_array(),
        });
        self.commands.push(DrawCommand {
            mesh: mesh.clone(),
            texture: material
                .texture
                .clone()
                .unwrap_or_else(|| self.white.clone()),
        });
    }

    /// Queues one particle for this frame: a soft disc that always faces the
    /// camera, `size` units across, centred on `position`.
    ///
    /// Particles are not lit; `color` is linear RGBA, and values above 1 glow.
    pub fn draw_particle(&mut self, position: Vec3, size: f32, color: Vec4, blend: ParticleBlend) {
        let instance = ParticleInstance {
            position_size: position.extend(size).to_array(),
            color: color.to_array(),
        };
        match blend {
            ParticleBlend::Alpha => self.alpha_particles.push(instance),
            ParticleBlend::Additive => self.additive_particles.push(instance),
        }
    }

    /// Number of particles queued for the current frame so far.
    pub fn queued_particles(&self) -> usize {
        self.alpha_particles.len() + self.additive_particles.len()
    }

    /// Lights this frame with a distant light such as the sun: parallel rays
    /// travelling along `direction`, casting shadows.
    ///
    /// `intensity` 3 is a bright daylight key light at the default exposure.
    /// Only one sun is supported; calls after the first in a frame are ignored.
    pub fn add_sun(&mut self, direction: Vec3, color: Vec3, intensity: f32) {
        let Some(direction) = direction.try_normalize() else {
            return;
        };
        if self.sun.is_none() {
            self.sun = Some(Sun {
                direction,
                radiance: color * intensity,
            });
        }
    }

    /// Lights this frame with a light shining in all directions from
    /// `position`. Its brightness falls off with distance and reaches zero at
    /// `range`. Point lights cast no shadows.
    ///
    /// At most [`MAX_POINT_LIGHTS`] are used per frame; more are ignored.
    pub fn add_point_light(&mut self, position: Vec3, color: Vec3, intensity: f32, range: f32) {
        if self.point_lights.len() < MAX_POINT_LIGHTS && range > 0.0 {
            self.point_lights.push(PointLight {
                position,
                radiance: color * intensity,
                range,
            });
        }
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
        self.targets = create_targets(
            &self.device,
            &self.config,
            &self.tonemap_layout,
            &self.tonemap_buffer,
        );
    }

    /// Renders everything queued since the last call, then `overlay` on top,
    /// and presents the frame.
    ///
    /// Returns `false` if no frame was shown (window hidden or being resized);
    /// the caller should then pace itself, as vsync did not block.
    pub fn render(&mut self, camera: &Camera, overlay: Option<&mut dyn Overlay>) -> bool {
        let presented = self.render_queued(camera, overlay);
        self.commands.clear();
        self.instances.clear();
        self.joints.clear();
        self.alpha_particles.clear();
        self.additive_particles.clear();
        self.sun = None;
        self.point_lights.clear();
        presented
    }

    fn render_queued(&mut self, camera: &Camera, mut overlay: Option<&mut dyn Overlay>) -> bool {
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

        // --- Per-frame data ----------------------------------------------
        let shadow_sun = self
            .sun
            .filter(|_| self.shadows && self.shadow_distance > 0.0);
        let radius = self.shadow_distance;
        let light_view_proj = shadow_sun.map_or(Mat4::IDENTITY, |sun| {
            // Centre the shadowed area ahead of the camera, where it looks.
            let focus = camera.position + camera.forward() * radius * 0.5;
            shadow_view_proj(sun.direction, focus, radius, SHADOW_RESOLUTION)
        });
        let shadow_texel_world = 2.0 * radius / SHADOW_RESOLUTION as f32;

        let mut points = [[0.0; 8]; MAX_POINT_LIGHTS];
        for (slot, light) in points.iter_mut().zip(&self.point_lights) {
            let [x, y, z] = light.position.to_array();
            let [r, g, b] = light.radiance.to_array();
            *slot = [x, y, z, light.range, r, g, b, 0.0];
        }
        let aspect = self.config.width as f32 / self.config.height as f32;
        let globals = Globals {
            view_proj: camera.view_projection(aspect).to_cols_array_2d(),
            light_view_proj: light_view_proj.to_cols_array_2d(),
            camera_position: camera.position.extend(1.0).to_array(),
            camera_right: camera.right().extend(0.0).to_array(),
            camera_up: camera.up().extend(0.0).to_array(),
            ambient_sky: self.ambient_sky.extend(0.0).to_array(),
            ambient_ground: self.ambient_ground.extend(0.0).to_array(),
            sun_direction: self
                .sun
                .map_or([0.0; 4], |sun| sun.direction.extend(1.0).to_array()),
            sun_color: self
                .sun
                .map_or([0.0; 4], |sun| sun.radiance.extend(0.0).to_array()),
            shadow: [
                1.0 / SHADOW_RESOLUTION as f32,
                shadow_texel_world * 2.0,
                if shadow_sun.is_some() { 1.0 } else { 0.0 },
                0.0,
            ],
            point_count: [self.point_lights.len() as u32, 0, 0, 0],
            points,
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.queue.write_buffer(
            &self.shadow_buffer,
            0,
            bytemuck::bytes_of(&light_view_proj.to_cols_array_2d()),
        );
        self.queue.write_buffer(
            &self.tonemap_buffer,
            0,
            bytemuck::bytes_of(&[
                self.exposure,
                // Whether the shader must sRGB-encode (the target does not).
                if self.config.format.is_srgb() {
                    0.0
                } else {
                    1.0
                },
                0.0,
                0.0,
            ]),
        );

        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        self.queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );

        if self.joints.len() > self.joint_capacity {
            self.joint_capacity = self.joints.len().next_power_of_two();
            self.joint_buffer = create_joint_buffer(&self.device, self.joint_capacity);
            // The bind groups refer to the old buffer.
            (self.globals_bind_group, self.shadow_bind_group) = create_frame_bind_groups(
                &self.device,
                &self.globals_layout,
                &self.globals_buffer,
                &self.shadow_view,
                &self.shadow_sampler,
                &self.shadow_layout,
                &self.shadow_buffer,
                &self.joint_buffer,
            );
        }
        self.queue
            .write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(&self.joints));

        // Alpha-blended particles only look right drawn far to near, each
        // over the ones behind it. Additive ones sum up in any order.
        let distance = |p: &ParticleInstance| {
            let [x, y, z, _] = p.position_size;
            Vec3::new(x, y, z).distance_squared(camera.position)
        };
        self.alpha_particles
            .sort_by(|a, b| distance(b).total_cmp(&distance(a)));
        let particle_count = self.alpha_particles.len() + self.additive_particles.len();
        if particle_count > self.particle_capacity {
            self.particle_capacity = particle_count.next_power_of_two();
            self.particle_buffer = create_particle_buffer(&self.device, self.particle_capacity);
        }
        let additive_offset = std::mem::size_of_val(self.alpha_particles.as_slice()) as u64;
        self.queue.write_buffer(
            &self.particle_buffer,
            0,
            bytemuck::cast_slice(&self.alpha_particles),
        );
        self.queue.write_buffer(
            &self.particle_buffer,
            additive_offset,
            bytemuck::cast_slice(&self.additive_particles),
        );

        // Look up what each handle points at now, so resources replaced since
        // the draw was queued (hot reload) show their new contents.
        struct Resolved {
            vertices: wgpu::Buffer,
            indices: wgpu::Buffer,
            index_count: u32,
            texture: wgpu::BindGroup,
        }
        let resolved: Vec<Resolved> = self
            .commands
            .iter()
            .map(|command| {
                let mesh = command.mesh.gpu();
                Resolved {
                    vertices: mesh.vertex_buffer.clone(),
                    indices: mesh.index_buffer.clone(),
                    index_count: mesh.index_count,
                    texture: command.texture.gpu().bind_group.clone(),
                }
            })
            .collect();
        let draw_all = |pass: &mut wgpu::RenderPass, with_textures: bool| {
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            for (i, draw) in resolved.iter().enumerate() {
                let i = i as u32;
                if with_textures {
                    pass.set_bind_group(1, &draw.texture, &[]);
                }
                pass.set_vertex_buffer(0, draw.vertices.slice(..));
                pass.set_index_buffer(draw.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..draw.index_count, 0, i..i + 1);
            }
        };

        let mut encoder = self.device.create_command_encoder(&Default::default());
        let overlay_commands = match overlay.as_deref_mut() {
            Some(overlay) => overlay.prepare(
                &self.device,
                &self.queue,
                &mut encoder,
                [self.config.width, self.config.height],
            ),
            None => Vec::new(),
        };

        // --- Pass 1: scene depth as seen from the sun --------------------
        if shadow_sun.is_some() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_bind_group, &[]);
            draw_all(&mut pass, false);
        }

        // --- Pass 2: lit scene into the HDR target -----------------------
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.msaa,
                    depth_slice: None,
                    resolve_target: Some(&self.targets.hdr),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.clear_color.x as f64,
                            g: self.clear_color.y as f64,
                            b: self.clear_color.z as f64,
                            a: 1.0,
                        }),
                        // Only the resolved image is needed afterwards.
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            draw_all(&mut pass, true);

            // Particles go last, over the finished opaque scene.
            let alpha = self.alpha_particles.len() as u32;
            let additive = self.additive_particles.len() as u32;
            if alpha + additive > 0 {
                pass.set_vertex_buffer(0, self.particle_buffer.slice(..));
                if alpha > 0 {
                    pass.set_pipeline(&self.alpha_particle_pipeline);
                    pass.draw(0..6, 0..alpha);
                }
                if additive > 0 {
                    pass.set_pipeline(&self.additive_particle_pipeline);
                    pass.draw(0..6, alpha..alpha + additive);
                }
            }
        }

        // --- Pass 3: tone map to the window, then the overlay ------------
        let view = frame.texture.create_view(&Default::default());
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tonemap pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            // Overlays take a pass that is not tied to the encoder's borrow;
            // it is still dropped before the encoder is finished below.
            let mut pass = pass.forget_lifetime();
            pass.set_pipeline(&self.tonemap_pipeline);
            pass.set_bind_group(0, &self.targets.tonemap_bind_group, &[]);
            pass.draw(0..3, 0..1);
            if let Some(overlay) = overlay {
                overlay.paint(&mut pass);
            }
        }

        self.queue
            .submit(overlay_commands.into_iter().chain([encoder.finish()]));
        self.queue.present(frame);

        if suboptimal {
            // Still shown, but the surface no longer matches the window exactly.
            self.reconfigure();
        }
        true
    }
}

fn create_targets(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    tonemap_layout: &wgpu::BindGroupLayout,
    tonemap_buffer: &wgpu::Buffer,
) -> Targets {
    let texture = |label, format, sample_count, usage| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
    let msaa = texture("msaa colour", HDR_FORMAT, SAMPLES, attachment);
    let depth = texture("depth", DEPTH_FORMAT, SAMPLES, attachment);
    let hdr = texture(
        "hdr colour",
        HDR_FORMAT,
        1,
        attachment | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let tonemap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tonemap"),
        layout: tonemap_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: tonemap_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&hdr),
            },
        ],
    });
    Targets {
        msaa,
        depth,
        hdr,
        tonemap_bind_group,
    }
}

fn create_particle_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particles"),
        size: (capacity * std::mem::size_of::<ParticleInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_joint_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("joint matrices"),
        size: (capacity * std::mem::size_of::<[[f32; 4]; 4]>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The bind groups shared by every draw in the main pass and in the shadow
/// pass. Both include the joint buffer, so they are rebuilt when it grows.
#[allow(clippy::too_many_arguments)]
fn create_frame_bind_groups(
    device: &wgpu::Device,
    globals_layout: &wgpu::BindGroupLayout,
    globals_buffer: &wgpu::Buffer,
    shadow_view: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
    shadow_layout: &wgpu::BindGroupLayout,
    shadow_buffer: &wgpu::Buffer,
    joint_buffer: &wgpu::Buffer,
) -> (wgpu::BindGroup, wgpu::BindGroup) {
    let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout: globals_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: joint_buffer.as_entire_binding(),
            },
        ],
    });
    let shadow = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow uniforms"),
        layout: shadow_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shadow_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: joint_buffer.as_entire_binding(),
            },
        ],
    });
    (globals, shadow)
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: (capacity * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn upload_mesh(device: &wgpu::Device, data: &MeshData) -> MeshGpu {
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mesh vertices"),
        contents: bytemuck::cast_slice(&data.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mesh indices"),
        contents: bytemuck::cast_slice(&data.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    MeshGpu {
        vertex_buffer,
        index_buffer,
        index_count: data.indices.len() as u32,
    }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> TextureGpu {
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
    TextureGpu {
        bind_group,
        width,
        height,
    }
}
