//! Forward renderer on `wgpu` (Vulkan on Linux, DirectX 12 or Vulkan on Windows).
//!
//! Each frame the game queues draws with [`Renderer::draw`]; the engine then
//! renders them from the active [`Camera`].

mod camera;
mod mesh;
mod renderer;
mod texture;

pub use camera::Camera;
pub use glam;
pub use mesh::{Mesh, MeshData, Vertex};
pub use renderer::{RenderError, Renderer};
pub use texture::{Texture, WeakTexture};
