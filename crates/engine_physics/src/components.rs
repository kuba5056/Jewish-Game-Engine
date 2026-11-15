use glam::Vec3;
use serde::{Deserialize, Serialize};

/// How a body takes part in the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BodyKind {
    /// Moved by gravity, collisions and impulses.
    #[default]
    Dynamic,
    /// Moved only by changing its `Transform` (platforms, doors). It pushes
    /// dynamic bodies but nothing pushes it.
    Kinematic,
    /// Never moves.
    Fixed,
}

/// Makes an entity with a [`Collider`] move under physics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigidBody {
    pub kind: BodyKind,
    /// Velocity the body starts with, in units per second.
    pub linear_velocity: Vec3,
    /// Slows movement over time, like air resistance. 0 is none.
    pub linear_damping: f32,
    /// Slows spinning over time. 0 is none.
    pub angular_damping: f32,
    /// Multiplies gravity for this body; 0 makes it float.
    pub gravity_scale: f32,
    /// Stops the body from tipping over or spinning, e.g. for characters.
    pub lock_rotation: bool,
    /// Extra checks so a small, fast body (a bullet) cannot pass through
    /// thin walls between two steps. Costs performance; off by default.
    pub continuous_collision: bool,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            kind: BodyKind::Dynamic,
            linear_velocity: Vec3::ZERO,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
            lock_rotation: false,
            continuous_collision: false,
        }
    }
}

impl RigidBody {
    pub fn dynamic() -> Self {
        Self::default()
    }

    pub fn kinematic() -> Self {
        Self {
            kind: BodyKind::Kinematic,
            ..Self::default()
        }
    }

    pub fn with_velocity(mut self, linear_velocity: Vec3) -> Self {
        self.linear_velocity = linear_velocity;
        self
    }
}

/// A collision shape, centred on the entity's position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// A box; `half_extents` is half its size along each axis.
    Box {
        half_extents: Vec3,
    },
    Sphere {
        radius: f32,
    },
    /// An upright (Y axis) cylinder with rounded ends: total height is
    /// `2 * (half_height + radius)`.
    Capsule {
        half_height: f32,
        radius: f32,
    },
}

/// Gives an entity a solid shape. Without a [`RigidBody`] the entity is a
/// static obstacle.
///
/// The entity's `Transform` scale is applied to the shape once, when the
/// entity first enters the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collider {
    pub shape: Shape,
    /// 0 is ice, 1 is rubber on concrete.
    pub friction: f32,
    /// Bounciness: 0 does not bounce, 1 bounces back to the same height.
    pub restitution: f32,
    /// Mass per unit of volume; with the shape's size it sets the body's mass.
    pub density: f32,
    /// A sensor detects overlaps (reported as collisions) without being
    /// solid: a trigger zone or pickup.
    pub sensor: bool,
}

/// A 1x1x1 box, which the entity's `Transform` scale stretches to size.
impl Default for Collider {
    fn default() -> Self {
        Self::cuboid(Vec3::ONE)
    }
}

impl Collider {
    fn new(shape: Shape) -> Self {
        Self {
            shape,
            friction: 0.5,
            restitution: 0.0,
            density: 1.0,
            sensor: false,
        }
    }

    /// A box of the given full size.
    pub fn cuboid(size: Vec3) -> Self {
        Self::new(Shape::Box {
            half_extents: size * 0.5,
        })
    }

    pub fn sphere(radius: f32) -> Self {
        Self::new(Shape::Sphere { radius })
    }

    /// An upright capsule; see [`Shape::Capsule`].
    pub fn capsule(half_height: f32, radius: f32) -> Self {
        Self::new(Shape::Capsule {
            half_height,
            radius,
        })
    }

    pub fn with_friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }

    pub fn with_restitution(mut self, restitution: f32) -> Self {
        self.restitution = restitution;
        self
    }

    pub fn with_density(mut self, density: f32) -> Self {
        self.density = density;
        self
    }

    pub fn as_sensor(mut self) -> Self {
        self.sensor = true;
        self
    }
}
