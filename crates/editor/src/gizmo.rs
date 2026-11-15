//! The move handle: three axis arrows on the selected entity that can be
//! dragged with the mouse.

use engine::math::{Mat4, Quat, Vec3};
use engine::{Material, Mesh, Renderer};

pub const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

/// Handle length as a fraction of its distance from the camera, so it looks
/// the same size on screen wherever the entity is.
const LENGTH_PER_DISTANCE: f32 = 0.16;
/// How close (as a fraction of the handle length) the mouse ray must pass to
/// an arrow to grab it.
const GRAB_RADIUS: f32 = 0.12;

/// Where a mouse ray comes closest to an axis line.
#[derive(Debug, Clone, Copy)]
pub struct AxisHit {
    /// Position along the axis from its origin, in world units.
    pub along: f32,
    /// How far the ray passes from the axis at that point.
    pub gap: f32,
}

/// The point on the line `origin + s * axis` nearest to a ray, or `None` if
/// the two are parallel. `axis` and `ray_direction` must be unit length.
pub fn closest_on_axis(
    origin: Vec3,
    axis: Vec3,
    ray_origin: Vec3,
    ray_direction: Vec3,
) -> Option<AxisHit> {
    let w = origin - ray_origin;
    let b = axis.dot(ray_direction);
    let denominator = 1.0 - b * b;
    if denominator < 1e-5 {
        return None;
    }
    let (c, e) = (axis.dot(w), ray_direction.dot(w));
    let along = (b * e - c) / denominator;
    let on_ray = ((e - b * c) / denominator).max(0.0);
    let gap = (origin + axis * along).distance(ray_origin + ray_direction * on_ray);
    Some(AxisHit { along, gap })
}

/// Length of the handle's arrows for an entity at `position`.
pub fn length(position: Vec3, camera: Vec3) -> f32 {
    (position.distance(camera) * LENGTH_PER_DISTANCE).max(0.05)
}

/// Which arrow (0 = X, 1 = Y, 2 = Z) of a handle at `position` the mouse ray
/// points at, if any: the one it passes closest to.
pub fn hovered_axis(
    position: Vec3,
    length: f32,
    ray_origin: Vec3,
    ray_direction: Vec3,
) -> Option<usize> {
    AXES.iter()
        .enumerate()
        .filter_map(|(index, &axis)| {
            let hit = closest_on_axis(position, axis, ray_origin, ray_direction)?;
            let on_arrow = (0.0..=length).contains(&hit.along);
            (on_arrow && hit.gap < length * GRAB_RADIUS).then_some((index, hit.gap))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Draws the handle. `unit_cube` is a 1x1x1 cube mesh; `active` is the arrow
/// being hovered or dragged, drawn brighter.
pub fn draw(
    renderer: &mut Renderer,
    unit_cube: &Mesh,
    position: Vec3,
    length: f32,
    active: Option<usize>,
) {
    let colors = [
        Vec3::new(1.0, 0.02, 0.02),
        Vec3::new(0.03, 0.8, 0.03),
        Vec3::new(0.03, 0.12, 1.0),
    ];
    let thickness = length * 0.035;
    for (index, (&axis, color)) in AXES.iter().zip(colors).enumerate() {
        // Kept below the tone mapper's shoulder so the colours stay
        // saturated; the active arrow is the bright one.
        let brightness = if active == Some(index) { 2.5 } else { 0.9 };
        let material = Material {
            base_color: Vec3::ZERO.extend(1.0),
            emissive: color * brightness,
            ..Material::default()
        };
        // A unit cube stretched along its X axis, then turned onto `axis`
        // and moved so that it starts at the entity.
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::new(length, thickness, thickness),
            Quat::from_rotation_arc(Vec3::X, axis),
            position + axis * length * 0.5,
        );
        renderer.draw_without_shadow(unit_cube, &material, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_point_on_an_axis() {
        // A ray pointing down at the X axis, crossing it at x = 3 and 0.5 off.
        let hit =
            closest_on_axis(Vec3::ZERO, Vec3::X, Vec3::new(3.0, 5.0, 0.5), Vec3::NEG_Y).unwrap();
        assert!((hit.along - 3.0).abs() < 1e-4);
        assert!((hit.gap - 0.5).abs() < 1e-4);
        // Parallel lines have no single closest point.
        assert!(closest_on_axis(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X).is_none());
    }

    #[test]
    fn hover_picks_the_arrow_under_the_ray() {
        let (position, length) = (Vec3::new(1.0, 0.0, 0.0), 2.0);
        let down = Vec3::NEG_Y;
        // Above the middle of the X arrow.
        assert_eq!(
            hovered_axis(position, length, Vec3::new(2.0, 5.0, 0.0), down),
            Some(0)
        );
        // Above the middle of the Z arrow.
        assert_eq!(
            hovered_axis(position, length, Vec3::new(1.0, 5.0, 1.0), down),
            Some(2)
        );
        // Past the end of the arrow, or well to the side: nothing.
        assert_eq!(
            hovered_axis(position, length, Vec3::new(4.0, 5.0, 0.0), down),
            None
        );
        assert_eq!(
            hovered_axis(position, length, Vec3::new(2.0, 5.0, 1.0), down),
            None
        );
        // The Y arrow, seen from the side.
        let side = hovered_axis(position, length, Vec3::new(1.0, 1.0, 5.0), Vec3::NEG_Z);
        assert_eq!(side, Some(1));
    }

    #[test]
    fn dragging_along_an_axis_tracks_the_mouse() {
        // Grab at x = 1, move the mouse ray to cross at x = 4: a drag of 3.
        let start =
            closest_on_axis(Vec3::ZERO, Vec3::X, Vec3::new(1.0, 5.0, 0.2), Vec3::NEG_Y).unwrap();
        let now =
            closest_on_axis(Vec3::ZERO, Vec3::X, Vec3::new(4.0, 5.0, -0.3), Vec3::NEG_Y).unwrap();
        assert!((now.along - start.along - 3.0).abs() < 1e-4);
    }

    #[test]
    fn handle_size_follows_camera_distance() {
        let near = length(Vec3::ZERO, Vec3::new(0.0, 0.0, 5.0));
        let far = length(Vec3::ZERO, Vec3::new(0.0, 0.0, 50.0));
        assert!((far / near - 10.0).abs() < 1e-4);
    }
}
