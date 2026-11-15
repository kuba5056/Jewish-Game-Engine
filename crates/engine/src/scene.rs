use engine_render::glam::Vec3;
use engine_render::{Material, Mesh, Renderer};
use engine_scene::{GlobalTransform, World};

/// Makes an entity visible: the engine draws `mesh` with `material` at the
/// entity's `Transform` every frame.
#[derive(Debug, Clone)]
pub struct MeshRenderer {
    pub mesh: Mesh,
    pub material: Material,
}

/// A distant light such as the sun: parallel rays that cast shadows.
///
/// It shines along the entity's forward direction (its local -Z axis); the
/// entity's position does not matter. Aim it with
/// `Transform::IDENTITY.looking_to(direction)`. Only one is used per frame.
#[derive(Debug, Clone, Copy)]
pub struct DirectionalLight {
    /// Linear RGB.
    pub color: Vec3,
    /// 3 is a bright daylight key light at the default exposure.
    pub intensity: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            color: Vec3::new(1.0, 0.96, 0.9),
            intensity: 3.0,
        }
    }
}

/// A light shining in all directions from the entity's position, like a
/// bulb. It casts no shadows. At most `MAX_POINT_LIGHTS` are used per frame.
#[derive(Debug, Clone, Copy)]
pub struct PointLight {
    /// Linear RGB.
    pub color: Vec3,
    /// Brightness at one unit of distance; it falls off with the square of
    /// the distance.
    pub intensity: f32,
    /// Distance at which the light fades out completely.
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 10.0,
            range: 10.0,
        }
    }
}

/// Queues every visible entity and every light in the world for this frame.
pub(crate) fn draw_world(world: &World, renderer: &mut Renderer) {
    for (global, mesh_renderer) in world.query::<(&GlobalTransform, &MeshRenderer)>().iter() {
        renderer.draw(&mesh_renderer.mesh, &mesh_renderer.material, global.0);
    }
    for (global, light) in world
        .query::<(&GlobalTransform, &DirectionalLight)>()
        .iter()
    {
        renderer.add_sun(global.forward(), light.color, light.intensity);
    }
    for (global, light) in world.query::<(&GlobalTransform, &PointLight)>().iter() {
        renderer.add_point_light(
            global.translation(),
            light.color,
            light.intensity,
            light.range,
        );
    }
}
