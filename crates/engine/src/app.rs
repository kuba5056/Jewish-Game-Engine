use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_assets::AssetError;
use engine_audio::{Audio, Sound};
use engine_core::Time;
use engine_physics::Physics;
use engine_platform::winit::application::ApplicationHandler;
use engine_platform::winit::dpi::LogicalSize;
use engine_platform::winit::event::{DeviceEvent, DeviceId, WindowEvent};
use engine_platform::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use engine_platform::winit::window::{CursorGrabMode, Window, WindowId};
use engine_platform::{Input, KeyCode, WindowConfig};
use engine_render::{Camera, RenderError, Renderer, Texture};
use engine_scene::World;
use engine_ui::{egui, Ui};

use crate::assets::{Assets, Model};
use crate::debug::DebugUi;
use crate::scene::draw_world;

/// Engine start-up settings.
#[derive(Debug, Clone)]
pub struct Config {
    pub window: WindowConfig,
    /// Simulation rate for [`Game::fixed_update`], in steps per second.
    pub fixed_hz: u32,
    /// Directory that asset paths are relative to. A relative directory is
    /// looked for in the working directory, then next to the executable.
    pub asset_dir: PathBuf,
    /// Reload textures and models when their files change on disk.
    /// On by default in debug builds only.
    pub hot_reload: bool,
    /// Key that shows and hides the debug overlay (statistics, entity list,
    /// inspector); `None` disables it.
    pub debug_overlay_key: Option<KeyCode>,
    /// Start with the debug overlay visible.
    pub show_debug_overlay: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            fixed_hz: 60,
            asset_dir: PathBuf::from("assets"),
            hot_reload: cfg!(debug_assertions),
            debug_overlay_key: Some(KeyCode::F3),
            show_debug_overlay: false,
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
    /// The physics simulation of entities that have a `Collider`. It is
    /// stepped automatically, once per fixed step, before `fixed_update`.
    pub physics: Physics,
    /// Sound playback.
    pub audio: Audio,
    /// Queue draws here during `update`; they are shown at the end of the frame.
    pub renderer: Renderer,
    /// The camera the frame is rendered from.
    pub camera: Camera,
    window: Arc<Window>,
    pub(crate) assets: Assets,
    ui: Ui,
    debug: Option<DebugUi>,
    debug_overlay_key: Option<KeyCode>,
    exit_requested: bool,
    systems: Vec<System>,
    fixed_systems: Vec<System>,
}

impl Context {
    /// Loads a texture (PNG or JPEG) from the asset directory. Loading the
    /// same path again returns the already-loaded texture.
    pub fn load_texture(&mut self, path: impl AsRef<Path>) -> Result<Texture, AssetError> {
        self.assets.texture(&self.renderer, path.as_ref())
    }

    /// Loads a glTF model (`.gltf` or `.glb`) from the asset directory.
    /// Loading the same path again returns the already-loaded model.
    pub fn load_model(&mut self, path: impl AsRef<Path>) -> Result<Model, AssetError> {
        self.assets.model(&self.renderer, path.as_ref())
    }

    /// Loads a sound (WAV, OGG, MP3 or FLAC) from the asset directory, for
    /// `ctx.audio.play`. Loading the same path again reuses the decoded sound.
    pub fn load_sound(&mut self, path: impl AsRef<Path>) -> Result<Sound, AssetError> {
        self.assets.sound(path.as_ref())
    }

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

    /// The context for building user interface with `egui`: menus, HUD,
    /// tools. Use it during `update` (or in systems), every frame the
    /// interface should be visible:
    ///
    /// ```no_run
    /// # fn update(ctx: &mut engine::Context) {
    /// engine::egui::Window::new("Menu").show(&ctx.ui(), |ui| {
    ///     if ui.button("Quit").clicked() {
    ///         ctx.exit();
    ///     }
    /// });
    /// # }
    /// ```
    ///
    /// Clicks and key presses the interface uses are hidden from `ctx.input`.
    pub fn ui(&self) -> egui::Context {
        self.ui.context().clone()
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

    /// Called at the fixed simulation rate, right after each physics step;
    /// use `ctx.time.fixed_delta_secs()`. Apply impulses and move kinematic
    /// bodies here.
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

        ctx.assets.maintain(&ctx.renderer, ctx.time.delta_secs());
        ctx.ui.begin_frame(&ctx.window);

        while ctx.time.step_fixed() {
            // Stepping first lets fixed_update react to this step's
            // collisions and see up-to-date positions.
            ctx.physics
                .step(&mut ctx.world, ctx.time.fixed_delta_secs());
            self.game.fixed_update(ctx);
            run_systems(ctx, |ctx| &ctx.fixed_systems);
        }
        self.game.update(ctx);
        run_systems(ctx, |ctx| &ctx.systems);
        ctx.physics.clear_collisions();

        ctx.realize_visuals();
        engine_scene::propagate_transforms(&mut ctx.world);
        draw_world(&ctx.world, &mut ctx.renderer);

        if ctx
            .debug_overlay_key
            .is_some_and(|key| ctx.input.key_pressed(key))
        {
            if let Some(debug) = &mut ctx.debug {
                debug.open = !debug.open;
            }
        }
        // Taken out while it draws, as it needs the whole context.
        if let Some(mut debug) = ctx.debug.take() {
            debug.draw(ctx);
            ctx.debug = Some(debug);
        }
        ctx.input.end_frame();

        ctx.ui.end_frame(&ctx.window, &ctx.renderer);
        if !ctx.renderer.render(&ctx.camera, Some(&mut ctx.ui)) {
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

        let ui = Ui::new(&window, &renderer);

        let mut ctx = Context {
            time: Time::new(self.config.fixed_hz),
            input: Input::default(),
            world: World::new(),
            physics: Physics::new(),
            audio: Audio::new(),
            renderer,
            camera: Camera::default(),
            ui,
            debug: Some(DebugUi::new(self.config.show_debug_overlay)),
            debug_overlay_key: self.config.debug_overlay_key,
            window,
            assets: Assets::new(&self.config.asset_dir, self.config.hot_reload),
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
        // Presses the interface used (a click on a button, typing in a text
        // field) must not also reach the game. Everything else still does,
        // releases included, so a key cannot get stuck down.
        let used_by_ui = ctx.ui.on_window_event(&ctx.window, &event);
        let is_press = matches!(
            &event,
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed()
        ) || matches!(
            &event,
            WindowEvent::MouseInput { state, .. } if state.is_pressed()
        ) || matches!(&event, WindowEvent::MouseWheel { .. });
        if !(used_by_ui && is_press) {
            ctx.input.handle_window_event(&event);
        }
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
