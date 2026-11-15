//! The engine facade: implement [`Game`], hand it to [`run`].
//!
//! ```no_run
//! use engine::math::Vec4;
//! use engine::{Config, Context, Game, KeyCode, MeshData, MeshRenderer, Transform};
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn init(&mut self, ctx: &mut Context) {
//!         // Anything with a Transform and a MeshRenderer is drawn every frame.
//!         let cube = MeshRenderer {
//!             mesh: ctx.renderer.create_mesh(&MeshData::cube(1.0)),
//!             texture: ctx.renderer.white_texture(),
//!             color: Vec4::ONE,
//!         };
//!         ctx.world.spawn((Transform::IDENTITY, cube));
//!     }
//!
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
mod scene;

pub use app::{run, Config, Context, Error, Game, System};
pub use engine_core::Time;
pub use engine_platform::{Input, KeyCode, MouseButton, WindowConfig};
pub use engine_render::glam as math;
pub use engine_render::{Camera, Mesh, MeshData, RenderError, Renderer, Texture, Vertex};
pub use engine_scene::hecs as ecs;
pub use engine_scene::{
    despawn_recursive, propagate_transforms, Entity, GlobalTransform, Parent, Transform, World,
};
pub use log;
pub use scene::MeshRenderer;
