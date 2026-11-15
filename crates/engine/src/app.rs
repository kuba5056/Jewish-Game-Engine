use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_core::Time;
use engine_platform::winit::application::ApplicationHandler;
use engine_platform::winit::dpi::LogicalSize;
use engine_platform::winit::event::{DeviceEvent, DeviceId, WindowEvent};
use engine_platform::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use engine_platform::winit::window::{CursorGrabMode, Window, WindowId};
use engine_platform::{Input, WindowConfig};
use engine_render::{Camera, RenderError, Renderer};
use engine_scene::World;

use crate::scene::draw_world;

/// Engine start-up settings.
#[derive(Debug, Clone)]
pub struct Config {
    pub window: WindowConfig,
    /// Simulation rate for [`Game::fixed_update`], in steps per second.
    pub fixed_hz: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            fixed_hz: 60,
        }
    }
}

/// Why the engine could not start or stopped abnormally.
#[derive(Debug)]
pub enum Error {
    /// The platform event loop failed (e.g. no display server available).
    EventLoop(String),
    /// The main window could not be created.
    Window(String),
    /// Graphics could not be initialised.
    Render(RenderError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EventLoop(e) => write!(f, "event loop error: {e}"),
            Error::Window(e) => write!(f, "could not create window: {e}"),
            Error::Render(e) => write!(f, "could not start rendering: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// A reusable piece of game logic: a plain function run by the engine every
/// frame or every fixed step. Register with [`Context::add_system`].
pub type System = fn(&mut Context);

/// Engine state handed to the game every callback.
pub struct Context {
    pub time: Time,
    pub input: Input,
    /// All entities and their components.
    pub world: World,
    /// Queue draws here during `update`; they are shown at the end of the frame.
    pub renderer: Renderer,
    /// The camera the frame is rendered from.
    pub camera: Camera,
    window: Arc<Window>,
    exit_requested: bool,
    systems: Vec<System>,
    fixed_systems: Vec<System>,
}

impl Context {
    /// Runs `system` once per frame, after [`Game::update`], in the order added.
    pub fn add_system(&mut self, system: System) {
        self.systems.push(system);
    }

    /// Runs `system` once per fixed step, after [`Game::fixed_update`], in the
    /// order added.
    pub fn add_fixed_system(&mut self, system: System) {
        self.fixed_systems.push(system);
    }

    /// The main window.
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Current drawable size in physical pixels.
    pub fn window_size(&self) -> (u32, u32) {
        let size = self.window.inner_size();
        (size.width, size.height)
    }

    /// Hides the cursor and keeps it inside the window, for mouse-look.
    /// `input.mouse_delta()` keeps reporting movement while captured.
    pub fn set_cursor_captured(&mut self, captured: bool) {
        if captured {
            // Locking is unsupported on some platforms (e.g. X11); confining
            // the cursor to the window is the next best thing.
            let grabbed = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(e) = grabbed {
                log::warn!("could not capture the cursor: {e}");
            }
        } else if let Err(e) = self.window.set_cursor_grab(CursorGrabMode::None) {
            log::warn!("could not release the cursor: {e}");
        }
        self.window.set_cursor_visible(!captured);
    }

    /// Asks the engine to shut down after the current frame.
    pub fn exit(&mut self) {
        self.exit_requested = true;
    }
}

/// A game running on the engine. All methods are optional.
pub trait Game {
    /// Called once, after the window exists and before the first frame.
    fn init(&mut self, _ctx: &mut Context) {}

    /// Called at the fixed simulation rate; use `ctx.time.fixed_delta_secs()`.
    fn fixed_update(&mut self, _ctx: &mut Context) {}

    /// Called once per frame; use `ctx.time.delta_secs()`. Entities with a
    /// `MeshRenderer` are drawn automatically; for anything else, draw here
    /// with `ctx.renderer.draw(..)`.
    fn update(&mut self, _ctx: &mut Context) {}

    /// Called when the window's drawable size changes (physical pixels).
    fn resized(&mut self, _ctx: &mut Context, _width: u32, _height: u32) {}

    /// Called once before shutdown.
    fn shutdown(&mut self, _ctx: &mut Context) {}
}

/// Opens the window and runs `game` until it exits or the window is closed.
///
/// Logging goes through the `log` crate; set `RUST_LOG=debug` for more output.
pub fn run(config: Config, game: impl Game) -> Result<(), Error> {
    // Ignore the error: the host application may have installed its own logger.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    let event_loop = EventLoop::new().map_err(|e| Error::EventLoop(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut runner = Runner {
        config,
        game,
        ctx: None,
        last_frame: Instant::now(),
        error: None,
    };
    event_loop
        .run_app(&mut runner)
        .map_err(|e| Error::EventLoop(e.to_string()))?;

    match runner.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct Runner<G: Game> {
    config: Config,
    game: G,
    /// `None` until the platform lets us create the window.
    ctx: Option<Context>,
    last_frame: Instant,
    error: Option<Error>,
}

impl<G: Game> Runner<G> {
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some(ctx) = self.ctx.as_mut() else { return };

        let now = Instant::now();
        ctx.time.advance(now - self.last_frame);
        self.last_frame = now;

        while ctx.time.step_fixed() {
            self.game.fixed_update(ctx);
            run_systems(ctx, |ctx| &ctx.fixed_systems);
        }
        self.game.update(ctx);
        run_systems(ctx, |ctx| &ctx.systems);
        ctx.input.end_frame();

        engine_scene::propagate_transforms(&mut ctx.world);
        draw_world(&ctx.world, &mut ctx.renderer);

        if !ctx.renderer.render(&ctx.camera) {
            // Nothing was presented (e.g. minimised), so vsync did not pace
            // us; sleep instead of spinning a CPU core.
            std::thread::sleep(Duration::from_millis(16));
        }

        if ctx.exit_requested {
            event_loop.exit();
        }
    }
}

/// Runs every system in a list. Indexing afresh each step (instead of
/// iterating) lets a system register further systems while the list runs.
fn run_systems(ctx: &mut Context, list: fn(&Context) -> &Vec<System>) {
    let mut i = 0;
    while let Some(&system) = list(ctx).get(i) {
        system(ctx);
        i += 1;
    }
}

impl<G: Game> ApplicationHandler for Runner<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.ctx.is_some() {
            return;
        }
        let cfg = &self.config.window;
        let attributes = Window::default_attributes()
            .with_title(cfg.title.clone())
            .with_inner_size(LogicalSize::new(cfg.width, cfg.height))
            .with_resizable(cfg.resizable);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                self.error = Some(Error::Window(e.to_string()));
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        log::info!("window created: {}x{} px", size.width, size.height);

        let renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(e) => {
                self.error = Some(Error::Render(e));
                event_loop.exit();
                return;
            }
        };

        let mut ctx = Context {
            time: Time::new(self.config.fixed_hz),
            input: Input::default(),
            world: World::new(),
            renderer,
            camera: Camera::default(),
            window,
            exit_requested: false,
            systems: Vec::new(),
            fixed_systems: Vec::new(),
        };
        self.game.init(&mut ctx);
        self.last_frame = Instant::now();
        self.ctx = Some(ctx);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(ctx) = self.ctx.as_mut() else { return };
        ctx.input.handle_window_event(&event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                ctx.renderer.resize(size.width, size.height);
                self.game.resized(ctx, size.width, size.height);
            }
            // Driving frames from redraw requests keeps the game animating
            // while the window is being dragged or resized on Windows.
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (Some(ctx), DeviceEvent::MouseMotion { delta }) = (self.ctx.as_mut(), event) {
            ctx.input.add_mouse_motion(delta.0 as f32, delta.1 as f32);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ctx) = self.ctx.as_ref() {
            ctx.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ctx) = self.ctx.as_mut() {
            self.game.shutdown(ctx);
            log::info!(
                "shut down after {} frames ({:.1} s)",
                ctx.time.frame(),
                ctx.time.elapsed().as_secs_f32()
            );
        }
    }
}
