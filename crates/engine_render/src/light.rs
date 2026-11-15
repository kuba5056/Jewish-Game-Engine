use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};

/// How many point lights can shine in one frame; further ones are ignored.
pub const MAX_POINT_LIGHTS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Sun {
    /// Direction the light travels, normalised.
    pub direction: Vec3,
    /// Colour times intensity.
    pub radiance: Vec3,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PointLight {
    pub position: Vec3,
    /// Colour times intensity.
    pub radiance: Vec3,
    pub range: f32,
}

/// World-to-shadow-map transform for a sun shining along `direction`: an
/// orthographic box of half-width `radius` around `focus`.
///
/// The box moves in whole shadow-map texels, so shadow edges stay put while
/// the camera (and with it the focus) moves, instead of shimmering.
pub(crate) fn shadow_view_proj(direction: Vec3, focus: Vec3, radius: f32, resolution: u32) -> Mat4 {
    // Any up vector works as long as it is not parallel to the light.
    let up = if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let light_view = view::look_to_mat4(Vec3::ZERO, direction, up);

    let texel = 2.0 * radius / resolution as f32;
    let centre = light_view.transform_point3(focus);
    let x = (centre.x / texel).floor() * texel;
    let y = (centre.y / texel).floor() * texel;
    // The light looks down -Z. Reach well back towards the light so tall or
    // distant objects outside the box still cast shadows into it.
    let depth = radius * 4.0;
    let near = -(centre.z + depth);
    let far = -(centre.z - radius);
    proj::directx::orthographic(x - radius, x + radius, y - radius, y + radius, near, far)
        * light_view
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESOLUTION: u32 = 2048;

    fn project(matrix: Mat4, point: Vec3) -> Vec3 {
        matrix.project_point3(point)
    }

    fn inside(ndc: Vec3) -> bool {
        ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z)
    }

    #[test]
    fn box_covers_the_area_around_the_focus() {
        let direction = Vec3::new(-0.4, -1.0, -0.3).normalize();
        let focus = Vec3::new(5.0, 0.0, -3.0);
        let matrix = shadow_view_proj(direction, focus, 20.0, RESOLUTION);

        let centre = project(matrix, focus);
        // The focus sits within one texel of the map centre.
        assert!(centre.x.abs() < 2.0 / RESOLUTION as f32 * 1.01);
        assert!(centre.y.abs() < 2.0 / RESOLUTION as f32 * 1.01);
        assert!(inside(centre));

        // Ground points around the focus, and a caster high above it, fit.
        for offset in [Vec3::X * 12.0, Vec3::Z * -12.0, Vec3::Y * 30.0] {
            assert!(inside(project(matrix, focus + offset)), "{offset}");
        }
        // Something far to the side does not.
        assert!(!inside(project(matrix, focus + Vec3::X * 60.0)));
    }

    #[test]
    fn points_nearer_the_light_have_smaller_depth() {
        let direction = Vec3::NEG_Y;
        let matrix = shadow_view_proj(direction, Vec3::ZERO, 10.0, RESOLUTION);
        let high = project(matrix, Vec3::new(1.0, 5.0, 1.0));
        let low = project(matrix, Vec3::new(1.0, 0.0, 1.0));
        assert!(high.z < low.z);
    }

    #[test]
    fn straight_down_light_is_handled() {
        let matrix = shadow_view_proj(Vec3::NEG_Y, Vec3::ZERO, 10.0, RESOLUTION);
        assert!(matrix.is_finite());
        assert!(inside(project(matrix, Vec3::new(3.0, 0.0, -3.0))));
    }

    #[test]
    fn small_focus_moves_do_not_shift_the_map() {
        let direction = Vec3::new(0.3, -1.0, 0.2).normalize();
        let a = shadow_view_proj(direction, Vec3::new(0.001, 0.0, 0.001), 20.0, RESOLUTION);
        let b = shadow_view_proj(direction, Vec3::new(0.004, 0.0, 0.003), 20.0, RESOLUTION);
        let point = Vec3::new(2.0, 0.0, 2.0);
        let (pa, pb) = (project(a, point), project(b, point));
        assert!((pa.x - pb.x).abs() < 1e-6 && (pa.y - pb.y).abs() < 1e-6);
    }
}
