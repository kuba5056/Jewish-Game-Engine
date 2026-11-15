//! The engine facade: implement [`Game`], hand it to [`run`].
//!
//! ```no_run
//! use engine::{Config, Context, Game, KeyCode};
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn update(&mut self, ctx: &mut Context) {
//!         if ctx.input.key_pressed(KeyCode::Escape) {
//!             ctx.exit();
//!         }
//!     }
//! }
//!
//! fn main() -> Result<(), engine::Error> {
//!     engine::run(Config::default(), MyGame)
//! }
//! ```

mod app;

pub use app::{run, Config, Context, Error, Game};
pub use engine_core::Time;
pub use engine_platform::{Input, KeyCode, MouseButton, WindowConfig};
pub use engine_render::glam as math;
pub use engine_render::{Camera, Mesh, MeshData, RenderError, Renderer, Texture, Vertex};
pub use log;
