use glam::camera::rh::{proj, view};
use glam::{EulerRot, Mat4, Quat, Vec3};

/// A perspective camera. Right-handed, Y up, looking down its local -Z axis.
#[derive(Debug, Clone)]
pub struct Camera {
    pub position: Vec3,
    pub rotation: Quat,
    /// Vertical field of view, in degrees.
    pub fov_y_degrees: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 5.0),
            rotation: Quat::IDENTITY,
            fov_y_degrees: 60.0,
            near: 0.1,
            far: 1000.0,
        }
    }
}

impl Camera {
    /// Turns the camera to face `target`, keeping world Y as up. Does
    /// nothing if `target` is where the camera already is.
    pub fn look_at(&mut self, target: Vec3) {
        let Some(direction) = (target - self.position).try_normalize() else {
            return;
        };
        // Looking straight up or down leaves "up" undefined; any horizontal
        // axis will do there.
        let up = if direction.y.abs() > 0.999 {
            Vec3::NEG_Z
        } else {
            Vec3::Y
        };
        let view = view::look_to_mat4(self.position, direction, up);
        self.rotation = Quat::from_mat4(&view.inverse());
    }

    /// Sets the orientation from first-person angles, in radians: `yaw` turns
    /// left for positive values, `pitch` looks up for positive values.
    pub fn set_yaw_pitch(&mut self, yaw: f32, pitch: f32) {
        self.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }

    /// Unit vector the camera is looking along.
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// Unit vector to the camera's right.
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// Unit vector pointing up from the camera.
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// World-to-camera transform.
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::from_rotation_translation(self.rotation, self.position).inverse()
    }

    /// Camera-to-clip transform for a viewport with the given width/height ratio.
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        // wgpu shares DirectX's clip space on every backend: Y up, depth 0..1.
        proj::directx::perspective(self.fov_y_degrees.to_radians(), aspect, self.near, self.far)
    }

    /// The ray through a pixel: where it starts (the camera) and the unit
    /// direction it travels in, for picking what is under the mouse.
    ///
    /// `pixel` is measured from the top-left corner of a viewport of
    /// `width` by `height` pixels.
    pub fn screen_ray(&self, pixel: (f32, f32), width: f32, height: f32) -> (Vec3, Vec3) {
        // Pixel to normalised device coordinates: x right, y up, -1..1.
        let x = 2.0 * pixel.0 / width.max(1.0) - 1.0;
        let y = 1.0 - 2.0 * pixel.1 / height.max(1.0);
        let half_height = (self.fov_y_degrees.to_radians() * 0.5).tan();
        let half_width = half_height * width / height.max(1.0);
        let direction =
            self.forward() + self.right() * (x * half_width) + self.up() * (y * half_height);
        (self.position, direction.normalize())
    }

    /// World-to-clip transform.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn default_looks_down_negative_z() {
        let camera = Camera::default();
        assert!(close(camera.forward(), Vec3::NEG_Z));
        assert!(close(camera.right(), Vec3::X));
        assert!(close(camera.up(), Vec3::Y));
    }

    #[test]
    fn look_at_faces_the_target() {
        let mut camera = Camera {
            position: Vec3::new(3.0, 4.0, 5.0),
            ..Camera::default()
        };
        camera.look_at(Vec3::ZERO);
        assert!(close(camera.forward(), (-camera.position).normalize()));
    }

    #[test]
    fn look_at_handles_straight_down_and_a_zero_distance() {
        let mut camera = Camera {
            position: Vec3::new(0.0, 10.0, 0.0),
            ..Camera::default()
        };
        camera.look_at(Vec3::ZERO);
        assert!(close(camera.forward(), Vec3::NEG_Y));
        assert!(camera.view_matrix().is_finite());

        let before = camera.rotation;
        camera.look_at(camera.position);
        assert_eq!(camera.rotation, before);
    }

    #[test]
    fn screen_ray_matches_the_projection() {
        let mut camera = Camera {
            position: Vec3::new(2.0, 3.0, 8.0),
            ..Camera::default()
        };
        camera.look_at(Vec3::new(0.0, 1.0, 0.0));
        let (width, height) = (1280.0, 720.0);

        // The centre pixel looks straight ahead.
        let (origin, direction) = camera.screen_ray((640.0, 360.0), width, height);
        assert_eq!(origin, camera.position);
        assert!(close(direction, camera.forward()));

        // A point along any pixel's ray projects back onto that pixel.
        for pixel in [(100.0, 50.0), (1200.0, 700.0), (640.0, 10.0)] {
            let (origin, direction) = camera.screen_ray(pixel, width, height);
            let ndc = camera
                .view_projection(width / height)
                .project_point3(origin + direction * 7.0);
            let back = ((ndc.x + 1.0) * 0.5 * width, (1.0 - ndc.y) * 0.5 * height);
            assert!((back.0 - pixel.0).abs() < 0.01 && (back.1 - pixel.1).abs() < 0.01);
        }
    }

    #[test]
    fn yaw_and_pitch_directions() {
        let mut camera = Camera::default();
        camera.set_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0);
        assert!(close(camera.forward(), Vec3::NEG_X)); // positive yaw turns left
        camera.set_yaw_pitch(0.0, std::f32::consts::FRAC_PI_2);
        assert!(close(camera.forward(), Vec3::Y)); // positive pitch looks up
    }

    #[test]
    fn point_in_front_projects_to_screen_centre() {
        let camera = Camera::default();
        let clip = camera.view_projection(16.0 / 9.0) * Vec3::ZERO.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1e-5 && ndc.y.abs() < 1e-5);
        assert!(ndc.z > 0.0 && ndc.z < 1.0);
    }
}
