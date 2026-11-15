use engine_render::glam::Vec3;
use engine_render::{Material, Mesh, Renderer};
use engine_scene::{Entity, GlobalTransform, World};
use serde::{Deserialize, Serialize};

use crate::animation::Skin;

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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
    for (global, mesh_renderer, skin) in world
        .query::<(&GlobalTransform, &MeshRenderer, Option<&Skin>)>()
        .iter()
    {
        let (mesh, material) = (&mesh_renderer.mesh, &mesh_renderer.material);
        match skin {
            Some(skin) => renderer.draw_skinned(mesh, material, &skin.joint_matrices(world)),
            None => renderer.draw(mesh, material, global.0),
        }
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

/// Finds the visible entity a ray hits first, e.g. the one under the mouse
/// (see `Camera::screen_ray`). Returns it with the distance along the ray.
///
/// Tests each mesh's bounding box, placed by the entity's transform, so it
/// is approximate for shapes that do not fill their box. `direction` need
/// not be normalised, but distances are in units of its length.
pub fn pick(world: &World, origin: Vec3, direction: Vec3) -> Option<(Entity, f32)> {
    world
        .query::<(Entity, &GlobalTransform, &MeshRenderer)>()
        .iter()
        .filter_map(|(entity, global, renderer)| {
            let (min, max) = renderer.mesh.bounds();
            ray_box(global, origin, direction, min, max).map(|distance| (entity, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Distance along a world-space ray to a box given in the local space of
/// `transform`, or `None` if the ray misses it or it lies behind the origin.
fn ray_box(
    transform: &GlobalTransform,
    origin: Vec3,
    direction: Vec3,
    min: Vec3,
    max: Vec3,
) -> Option<f32> {
    let to_local = transform.0.inverse();
    if !to_local.is_finite() {
        return None; // A zero scale flattens the box to nothing.
    }
    // In the box's own space it is axis-aligned, and the ray parameter is
    // unchanged because the transform is linear.
    let o = to_local.transform_point3(origin);
    let d = to_local.transform_vector3(direction);
    // A flat mesh such as a plane still needs some thickness to be hit.
    let pad = Vec3::splat(1e-3);
    let (min, max) = (min - pad, max + pad);

    let (mut near, mut far) = (0.0_f32, f32::INFINITY);
    for axis in 0..3 {
        if d[axis].abs() < 1e-9 {
            if o[axis] < min[axis] || o[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = (
            (min[axis] - o[axis]) / d[axis],
            (max[axis] - o[axis]) / d[axis],
        );
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some(near)
}

#[cfg(test)]
mod tests {
    use engine_render::glam::{Mat4, Quat};
    use engine_scene::Transform;

    use super::*;

    fn unit_box(transform: Transform) -> GlobalTransform {
        GlobalTransform(transform.matrix())
    }

    fn hit(transform: Transform, origin: Vec3, direction: Vec3) -> Option<f32> {
        ray_box(
            &unit_box(transform),
            origin,
            direction,
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
        )
    }

    #[test]
    fn ray_hits_the_near_face() {
        let distance = hit(Transform::IDENTITY, Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z).unwrap();
        assert!((distance - 4.5).abs() < 1e-3);
        assert!(hit(Transform::IDENTITY, Vec3::new(2.0, 0.0, 5.0), Vec3::NEG_Z).is_none());
        // Behind the origin does not count.
        assert!(hit(Transform::IDENTITY, Vec3::new(0.0, 0.0, 5.0), Vec3::Z).is_none());
    }

    #[test]
    fn box_follows_translation_rotation_and_scale() {
        let moved = Transform::from_xyz(10.0, 0.0, 0.0).with_scale(Vec3::new(1.0, 6.0, 1.0));
        // Tall box: a ray 2 units up still hits; the unscaled box would not.
        let distance = hit(moved, Vec3::new(10.0, 2.0, 5.0), Vec3::NEG_Z).unwrap();
        assert!((distance - 4.5).abs() < 1e-3);

        // Turned 45 degrees about Y, the box's corner faces the ray.
        let turned =
            Transform::IDENTITY.with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4));
        let distance = hit(turned, Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z).unwrap();
        // (The box is padded by a millimetre, hence the looser tolerance.)
        assert!((distance - (5.0 - 0.5_f32.hypot(0.5))).abs() < 0.01);
    }

    #[test]
    fn flat_and_degenerate_boxes() {
        // A plane (zero height) is hit from above.
        let plane = GlobalTransform(Mat4::IDENTITY);
        let (min, max) = (Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, 5.0));
        let distance = ray_box(&plane, Vec3::new(1.0, 3.0, 1.0), Vec3::NEG_Y, min, max).unwrap();
        assert!((distance - 3.0).abs() < 0.01);
        // Zero scale cannot be hit and must not produce NaN.
        assert!(hit(
            Transform::IDENTITY.with_scale(Vec3::ZERO),
            Vec3::Z * 5.0,
            Vec3::NEG_Z
        )
        .is_none());
    }
}
