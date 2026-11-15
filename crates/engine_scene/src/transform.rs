use glam::camera::rh::view;
use glam::{Mat4, Quat, Vec3};

/// Position, rotation and scale of an entity, relative to its parent
/// (or to the world, if it has none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self::from_translation(Vec3::new(x, y, z))
    }

    pub fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    /// Turns the transform so its forward direction (local -Z, the way
    /// cameras and lights face) points along `direction`. Does nothing if
    /// `direction` is zero.
    pub fn looking_to(mut self, direction: Vec3) -> Self {
        if let Some(direction) = direction.try_normalize() {
            // Any up vector works unless it is parallel to the direction.
            let up = if direction.y.abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let view = view::look_to_mat4(Vec3::ZERO, direction, up);
            self.rotation = Quat::from_mat4(&view.inverse());
        }
        self
    }

    /// Like [`looking_to`](Self::looking_to), facing the point `target`.
    pub fn looking_at(self, target: Vec3) -> Self {
        let direction = target - self.translation;
        self.looking_to(direction)
    }

    /// The local-to-parent matrix.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// World-space matrix of an entity, computed from its [`Transform`] and its
/// ancestors'. Written by `propagate_transforms`; do not edit it directly, it
/// is overwritten every frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalTransform(pub Mat4);

impl GlobalTransform {
    /// World-space position of the entity's origin.
    pub fn translation(&self) -> Vec3 {
        self.0.w_axis.truncate()
    }

    /// World-space direction the entity faces (its local -Z axis), normalised.
    pub fn forward(&self) -> Vec3 {
        (-self.0.z_axis.truncate()).normalize_or_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn looking_to_faces_the_direction() {
        for direction in [
            Vec3::X,
            Vec3::NEG_Y,
            Vec3::new(-0.4, -1.0, -0.3).normalize(),
        ] {
            let transform = Transform::from_xyz(1.0, 2.0, 3.0).looking_to(direction);
            let global = GlobalTransform(transform.matrix());
            assert!(close(global.forward(), direction), "{direction}");
        }
    }

    #[test]
    fn looking_at_faces_the_target() {
        let transform = Transform::from_xyz(0.0, 5.0, 0.0).looking_at(Vec3::new(5.0, 0.0, 0.0));
        let forward = GlobalTransform(transform.matrix()).forward();
        assert!(close(forward, Vec3::new(1.0, -1.0, 0.0).normalize()));
    }

    #[test]
    fn scale_does_not_affect_forward() {
        let transform = Transform::IDENTITY.with_scale(Vec3::splat(4.0));
        assert!(close(
            GlobalTransform(transform.matrix()).forward(),
            Vec3::NEG_Z
        ));
    }
}
