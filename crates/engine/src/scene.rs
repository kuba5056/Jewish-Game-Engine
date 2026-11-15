use engine_render::glam::Vec4;
use engine_render::{Mesh, Renderer, Texture};
use engine_scene::{GlobalTransform, World};

/// Makes an entity visible: the engine draws `mesh` at the entity's
/// `Transform` every frame.
#[derive(Debug, Clone)]
pub struct MeshRenderer {
    pub mesh: Mesh,
    pub texture: Texture,
    /// Multiplied with the texture (linear RGBA); `Vec4::ONE` leaves it as is.
    pub color: Vec4,
}

/// Queues a draw for every entity that has a world transform and a mesh.
pub(crate) fn draw_world(world: &World, renderer: &mut Renderer) {
    for (global, mesh_renderer) in world.query::<(&GlobalTransform, &MeshRenderer)>().iter() {
        renderer.draw_tinted(
            &mesh_renderer.mesh,
            &mesh_renderer.texture,
            global.0,
            mesh_renderer.color,
        );
    }
}
