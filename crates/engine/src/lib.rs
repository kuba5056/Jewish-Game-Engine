//! The engine facade: implement [`Game`], hand it to [`run`].
//!
//! ```no_run
//! use engine::math::Vec3;
//! use engine::{
//!     Config, Context, DirectionalLight, Game, KeyCode, Material, MeshData, MeshRenderer,
//!     Transform,
//! };
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn init(&mut self, ctx: &mut Context) {
//!         // Anything with a Transform and a MeshRenderer is drawn every frame.
//!         let cube = MeshRenderer {
//!             mesh: ctx.renderer.create_mesh(&MeshData::cube(1.0)),
//!             material: Material::colored(0.8, 0.2, 0.2),
//!         };
//!         ctx.world.spawn((Transform::IDENTITY, cube));
//!         // A sun, shining down and to the side.
//!         let sun = Transform::IDENTITY.looking_to(Vec3::new(-0.4, -1.0, -0.3));
//!         ctx.world.spawn((sun, DirectionalLight::default()));
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

mod animation;
mod app;
mod assets;
mod debug;
mod scene;
mod scene_file;

pub use animation::{AnimationPlayer, Skin};
pub use app::{run, Config, Context, Error, Game, System};
pub use assets::{Model, ModelPart};
pub use engine_assets::AssetError;
pub use engine_audio::{Audio, Playback, Sound, SoundHandle};
pub use engine_core::Time;
pub use engine_physics::{BodyKind, Collider, Collision, Physics, RayHit, RigidBody, Shape};
pub use engine_platform::{Input, KeyCode, MouseButton, WindowConfig};
pub use engine_render::glam as math;
pub use engine_render::{
    Camera, Material, Mesh, MeshData, RenderError, Renderer, Texture, Vertex, MAX_POINT_LIGHTS,
};
pub use engine_scene::hecs as ecs;
pub use engine_scene::{
    despawn_recursive, find_by_name, propagate_transforms, Entity, GlobalTransform, Name, Parent,
    Transform, World,
};
pub use engine_ui::egui;
pub use log;
pub use scene::{DirectionalLight, MeshRenderer, PointLight};
pub use scene_file::{MaterialDesc, SceneEntity, SceneFile, Visual};
