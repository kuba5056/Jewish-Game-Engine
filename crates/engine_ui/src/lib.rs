//! Immediate-mode user interface on `egui`, drawn over the rendered frame.
//!
//! Between [`Ui::begin_frame`] and [`Ui::end_frame`], anything may build
//! interface through [`Ui::context`]; the result is painted as a renderer
//! [`Overlay`].

use engine_platform::winit::event::WindowEvent;
use engine_platform::winit::window::Window;
use engine_render::{wgpu, Overlay, Renderer};

pub use egui;

pub struct Ui {
    context: egui::Context,
    input: egui_winit::State,
    painter: egui_wgpu::Renderer,
    /// Tessellated output of the last finished frame.
    triangles: Vec<egui::ClippedPrimitive>,
    /// Textures egui stopped using; freed once the frame that may still
    /// reference them has been painted.
    stale_textures: Vec<egui::TextureId>,
    pixels_per_point: f32,
    size: [u32; 2],
}

impl Ui {
    pub fn new(window: &Window, renderer: &Renderer) -> Self {
        let context = egui::Context::default();
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(renderer.device().limits().max_texture_dimension_2d as usize),
        );
        let painter = egui_wgpu::Renderer::new(
            renderer.device(),
            renderer.surface_format(),
            egui_wgpu::RendererOptions::default(),
        );
        Self {
            context,
            input,
            painter,
            triangles: Vec::new(),
            stale_textures: Vec::new(),
            pixels_per_point: 1.0,
            size: [1, 1],
        }
    }

    /// The egui context to build interface with. Cheap to clone.
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Feeds a window event to the interface. Returns `true` if the
    /// interface used it (a click on a button, typing into a text field) and
    /// the game should not react to it as well.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.input.on_window_event(window, event).consumed
    }

    /// Starts collecting interface for a new frame.
    pub fn begin_frame(&mut self, window: &Window) {
        let input = self.input.take_egui_input(window);
        self.context.begin_pass(input);
    }

    /// Finishes the frame started by [`begin_frame`](Self::begin_frame) and
    /// gets it ready to paint.
    pub fn end_frame(&mut self, window: &Window, renderer: &Renderer) {
        let output = self.context.end_pass();
        self.input
            .handle_platform_output(window, output.platform_output);
        self.pixels_per_point = output.pixels_per_point;
        self.triangles = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        // Drained rather than iterated: egui insists every delta is consumed.
        let mut textures = output.textures_delta;
        for (id, deltas) in textures.set.drain() {
            for delta in &deltas {
                self.painter
                    .update_texture(renderer.device(), renderer.queue(), id, delta);
            }
        }
        self.stale_textures.extend(textures.free.drain());
    }

    fn screen(&self) -> egui_wgpu::ScreenDescriptor {
        egui_wgpu::ScreenDescriptor {
            size_in_pixels: self.size,
            pixels_per_point: self.pixels_per_point,
        }
    }
}

impl Overlay for Ui {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) -> Vec<wgpu::CommandBuffer> {
        self.size = size;
        let screen = self.screen();
        self.painter
            .update_buffers(device, queue, encoder, &self.triangles, &screen)
    }

    fn paint(&mut self, pass: &mut wgpu::RenderPass<'static>) {
        self.painter.render(pass, &self.triangles, &self.screen());
        for id in self.stale_textures.drain(..) {
            self.painter.free_texture(&id);
        }
    }
}
