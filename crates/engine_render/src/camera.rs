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
    /// Turns the camera to face `target`, keeping world Y as up.
    pub fn look_at(&mut self, target: Vec3) {
        let view = view::look_at_mat4(self.position, target, Vec3::Y);
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
