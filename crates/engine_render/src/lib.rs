//! Forward renderer on `wgpu` (Vulkan on Linux, DirectX 12 or Vulkan on Windows).
//!
//! Each frame the game queues meshes with [`Renderer::draw`] and lights with
//! [`Renderer::add_sun`] / [`Renderer::add_point_light`]; the engine then
//! renders them from the active [`Camera`].
//!
//! A frame is three passes: the sun's shadow map, the scene with physically
//! based shading into an HDR target (4x antialiased), and tone mapping to the
//! window.

mod camera;
mod light;
mod material;
mod mesh;
mod renderer;
mod texture;

pub use camera::Camera;
pub use glam;
pub use light::MAX_POINT_LIGHTS;
pub use material::Material;
pub use mesh::{Mesh, MeshData, Vertex};
pub use renderer::{Overlay, RenderError, Renderer};
pub use texture::{Texture, WeakTexture};
pub use wgpu;
