use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

use engine_scene::{Entity, Transform, World};
use glam::{Quat, Vec3};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{
    ActiveEvents, ChannelEventCollector, ColliderBuilder, ColliderHandle, CollisionEvent,
    ContactForceEvent, PhysicsWorld, QueryFilter, Ray, RigidBodyBuilder, RigidBodyHandle,
    SoftBodyTearEvent,
};

use crate::components::{BodyKind, Collider, RigidBody, Shape};

// Rapier is built against its own glam version, so values cross over as
// plain arrays.
fn to_rapier(v: Vec3) -> Vector {
    Vector::from_array(v.to_array())
}

fn from_rapier(v: Vector) -> Vec3 {
    Vec3::from_array(v.to_array())
}

fn pose(translation: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(
        to_rapier(translation),
        Rotation::from_array(rotation.to_array()),
    )
}

/// Marks an entity that is in the simulation. Added and maintained by
/// [`Physics::step`]; removing it takes the entity out of the simulation.
#[derive(Debug, Clone, Copy)]
pub struct PhysicsBody {
    handle: RigidBodyHandle,
    kind: BodyKind,
    /// The pose last exchanged with the simulation. A `Transform` that
    /// differs from it was moved by the game, and is pushed into the
    /// simulation before the next step.
    synced_translation: Vec3,
    synced_rotation: Quat,
}

/// Two entities whose colliders started or stopped touching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collision {
    pub a: Entity,
    pub b: Entity,
    /// `true` when contact began, `false` when it ended.
    pub started: bool,
}

impl Collision {
    /// If `entity` is part of this collision, the other entity.
    pub fn other(&self, entity: Entity) -> Option<Entity> {
        if self.a == entity {
            Some(self.b)
        } else if self.b == entity {
            Some(self.a)
        } else {
            None
        }
    }
}

/// What a ray hit.
#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub entity: Entity,
    /// Distance from the ray origin to the hit.
    pub distance: f32,
    pub point: Vec3,
    /// Direction the hit surface faces.
    pub normal: Vec3,
}

/// The physics simulation.
pub struct Physics {
    world: PhysicsWorld,
    bodies: HashMap<Entity, RigidBodyHandle>,
    event_collector: ChannelEventCollector,
    collision_events: Receiver<CollisionEvent>,
    // Kept only so the collector's senders stay connected.
    _force_events: Receiver<ContactForceEvent>,
    _tear_events: Receiver<SoftBodyTearEvent>,
    collisions: Vec<Collision>,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    /// An empty simulation with Earth gravity along -Y.
    pub fn new() -> Self {
        let (collision_sender, collision_events) = channel();
        let (force_sender, force_events) = channel();
        let (tear_sender, tear_events) = channel();
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, -9.81, 0.0);
        Self {
            world,
            bodies: HashMap::new(),
            event_collector: ChannelEventCollector::new(
                collision_sender,
                force_sender,
                tear_sender,
            ),
            collision_events,
            _force_events: force_events,
            _tear_events: tear_events,
            collisions: Vec::new(),
        }
    }

    /// Acceleration applied to every dynamic body, in units per second squared.
    pub fn gravity(&self) -> Vec3 {
        from_rapier(self.world.gravity)
    }

    pub fn set_gravity(&mut self, gravity: Vec3) {
        self.world.gravity = to_rapier(gravity);
    }

    /// Advances the simulation by `dt` seconds and syncs it with `world`.
    ///
    /// The engine calls this once per fixed step, before `fixed_update`.
    pub fn step(&mut self, world: &mut World, dt: f32) {
        self.remove_stale(world);
        self.add_new(world);
        self.push_moved(world);

        self.world.integration_parameters.dt = dt;
        self.world.step_with_events(&(), &self.event_collector);

        self.pull_simulated(world);
        self.collect_collisions();
    }

    /// Drops bodies whose entity was despawned or lost its `PhysicsBody`.
    fn remove_stale(&mut self, world: &World) {
        let physics = &mut self.world;
        self.bodies.retain(|&entity, &mut handle| {
            let alive = world
                .get::<&PhysicsBody>(entity)
                .is_ok_and(|body| body.handle == handle);
            if !alive {
                physics.remove_body_with_colliders(handle, true);
            }
            alive
        });
    }

    /// Creates bodies for entities that gained a `Collider`.
    fn add_new(&mut self, world: &mut World) {
        let new: Vec<(Entity, Transform, Collider, Option<RigidBody>)> = world
            .query::<(Entity, &Transform, &Collider, Option<&RigidBody>)>()
            .without::<&PhysicsBody>()
            .iter()
            .map(|(entity, transform, collider, body)| {
                (entity, *transform, *collider, body.copied())
            })
            .collect();

        for (entity, transform, collider, body) in new {
            let kind = body.map_or(BodyKind::Fixed, |body| body.kind);
            let mut builder = match kind {
                BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
                BodyKind::Kinematic => RigidBodyBuilder::kinematic_position_based(),
                BodyKind::Fixed => RigidBodyBuilder::fixed(),
            }
            .pose(pose(transform.translation, transform.rotation));
            if let Some(body) = body {
                builder = builder
                    .linvel(to_rapier(body.linear_velocity))
                    .linear_damping(body.linear_damping)
                    .angular_damping(body.angular_damping)
                    .gravity_scale(body.gravity_scale)
                    .ccd_enabled(body.continuous_collision);
                if body.lock_rotation {
                    builder = builder.lock_rotations();
                }
            }

            let scale = transform.scale.abs();
            let shape = match collider.shape {
                Shape::Box { half_extents } => {
                    let h = half_extents * scale;
                    ColliderBuilder::cuboid(h.x, h.y, h.z)
                }
                // Spheres and capsules cannot be squashed; use the largest
                // relevant scale so the shape covers the visible mesh.
                Shape::Sphere { radius } => ColliderBuilder::ball(radius * scale.max_element()),
                Shape::Capsule {
                    half_height,
                    radius,
                } => {
                    ColliderBuilder::capsule_y(half_height * scale.y, radius * scale.x.max(scale.z))
                }
            }
            .friction(collider.friction)
            .restitution(collider.restitution)
            .density(collider.density)
            .sensor(collider.sensor)
            .active_events(ActiveEvents::COLLISION_EVENTS)
            // Lets a collider found by a query or event be traced to its entity.
            .user_data(entity.to_bits().get() as u128);

            let (handle, _) = self.world.insert(builder, shape);
            self.bodies.insert(entity, handle);
            let marker = PhysicsBody {
                handle,
                kind,
                synced_translation: transform.translation,
                synced_rotation: transform.rotation,
            };
            // The entity came from a query a moment ago, so it still exists.
            let _ = world.insert_one(entity, marker);
        }
    }

    /// Applies `Transform`s the game changed since the last step.
    fn push_moved(&mut self, world: &World) {
        for (transform, body) in world.query::<(&Transform, &PhysicsBody)>().iter() {
            let moved = transform.translation != body.synced_translation
                || transform.rotation != body.synced_rotation;
            if !moved {
                continue;
            }
            let Some(rigid_body) = self.world.bodies.get_mut(body.handle) else {
                continue;
            };
            let target = pose(transform.translation, transform.rotation);
            if body.kind == BodyKind::Kinematic {
                // Moving (rather than teleporting) lets it push what it touches.
                rigid_body.set_next_kinematic_position(target);
            } else {
                rigid_body.set_position(target, true);
            }
        }
    }

    /// Copies simulated poses into each `Transform`.
    fn pull_simulated(&mut self, world: &mut World) {
        for (transform, body) in world.query_mut::<(&mut Transform, &mut PhysicsBody)>() {
            let Some(rigid_body) = self.world.bodies.get(body.handle) else {
                continue;
            };
            transform.translation = from_rapier(rigid_body.translation());
            transform.rotation = Quat::from_array(rigid_body.rotation().to_array());
            body.synced_translation = transform.translation;
            body.synced_rotation = transform.rotation;
        }
    }

    fn collect_collisions(&mut self) {
        while let Ok(event) = self.collision_events.try_recv() {
            let (a, b, started) = match event {
                CollisionEvent::Started(a, b, _) => (a, b, true),
                CollisionEvent::Stopped(a, b, _) => (a, b, false),
            };
            // A collider removed during the step no longer maps to an entity.
            if let (Some(a), Some(b)) = (self.entity_of(a), self.entity_of(b)) {
                self.collisions.push(Collision { a, b, started });
            }
        }
    }

    fn entity_of(&self, collider: ColliderHandle) -> Option<Entity> {
        let data = self.world.colliders.get(collider)?.user_data;
        Entity::from_bits(data as u64)
    }

    /// Contacts that began or ended since [`clear_collisions`](Self::clear_collisions)
    /// was last called; the engine clears them at the end of every frame.
    pub fn collisions(&self) -> &[Collision] {
        &self.collisions
    }

    pub fn clear_collisions(&mut self) {
        self.collisions.clear();
    }

    /// The body's velocity, or `None` if the entity is not in the simulation
    /// (yet: bodies are created on the first step after the entity is spawned).
    pub fn linear_velocity(&self, entity: Entity) -> Option<Vec3> {
        let body = self.world.bodies.get(*self.bodies.get(&entity)?)?;
        Some(from_rapier(body.linvel()))
    }

    /// Sets the body's velocity. Returns `false` if the entity is not in the
    /// simulation.
    pub fn set_linear_velocity(&mut self, entity: Entity, velocity: Vec3) -> bool {
        self.with_body(entity, |body| body.set_linvel(to_rapier(velocity), true))
    }

    /// Spin as an axis scaled by radians per second.
    pub fn angular_velocity(&self, entity: Entity) -> Option<Vec3> {
        let body = self.world.bodies.get(*self.bodies.get(&entity)?)?;
        Some(from_rapier(body.angvel()))
    }

    pub fn set_angular_velocity(&mut self, entity: Entity, velocity: Vec3) -> bool {
        self.with_body(entity, |body| body.set_angvel(to_rapier(velocity), true))
    }

    /// Gives a dynamic body an instant push through its centre: its velocity
    /// changes by `impulse / mass`. Returns `false` if the entity is not in
    /// the simulation.
    pub fn apply_impulse(&mut self, entity: Entity, impulse: Vec3) -> bool {
        self.with_body(entity, |body| body.apply_impulse(to_rapier(impulse), true))
    }

    fn with_body(
        &mut self,
        entity: Entity,
        f: impl FnOnce(&mut rapier3d::prelude::RigidBody),
    ) -> bool {
        let body = self
            .bodies
            .get(&entity)
            .and_then(|&handle| self.world.bodies.get_mut(handle));
        match body {
            Some(body) => {
                f(body);
                true
            }
            None => false,
        }
    }

    /// Finds the first collider along a ray, up to `max_distance` away.
    ///
    /// `ignore` leaves one entity out, typically the one casting the ray.
    /// Reflects the simulation as of the last step.
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        ignore: Option<Entity>,
    ) -> Option<RayHit> {
        let direction = direction.try_normalize()?;
        let mut filter = QueryFilter::default();
        if let Some(&handle) = ignore.and_then(|entity| self.bodies.get(&entity)) {
            filter = filter.exclude_rigid_body(handle);
        }
        let ray = Ray::new(to_rapier(origin), to_rapier(direction));
        let (collider, hit) =
            self.world
                .cast_ray_and_get_normal(&ray, max_distance, true, filter)?;
        Some(RayHit {
            entity: self.entity_of(collider)?,
            distance: hit.time_of_impact,
            point: origin + direction * hit.time_of_impact,
            normal: from_rapier(hit.normal),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn floor(world: &mut World) -> Entity {
        // Top surface at y = 0.
        world.spawn((
            Transform::from_xyz(0.0, -0.5, 0.0),
            Collider::cuboid(Vec3::new(50.0, 1.0, 50.0)),
        ))
    }

    fn run(physics: &mut Physics, world: &mut World, seconds: f32) {
        for _ in 0..(seconds / DT) as u32 {
            physics.step(world, DT);
        }
    }

    fn position(world: &World, entity: Entity) -> Vec3 {
        world.get::<&Transform>(entity).unwrap().translation
    }

    #[test]
    fn box_falls_and_rests_on_the_floor() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        floor(&mut world);
        let cube = world.spawn((
            Transform::from_xyz(0.0, 3.0, 0.0),
            Collider::cuboid(Vec3::ONE),
            RigidBody::dynamic(),
        ));

        run(&mut physics, &mut world, 0.25);
        let falling = position(&world, cube).y;
        assert!(falling < 3.0 && falling > 0.5, "{falling}");

        run(&mut physics, &mut world, 3.0);
        let resting = position(&world, cube);
        assert!((resting.y - 0.5).abs() < 0.02, "{resting}");
        assert!(physics.linear_velocity(cube).unwrap().length() < 0.01);
    }

    #[test]
    fn collider_without_a_body_stays_put() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        let wall = world.spawn((
            Transform::from_xyz(0.0, 5.0, 0.0),
            Collider::cuboid(Vec3::ONE),
        ));
        run(&mut physics, &mut world, 1.0);
        assert_eq!(position(&world, wall), Vec3::new(0.0, 5.0, 0.0));
    }

    #[test]
    fn collisions_are_reported_with_their_entities() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        let ground = floor(&mut world);
        let ball = world.spawn((
            Transform::from_xyz(0.0, 1.0, 0.0),
            Collider::sphere(0.5),
            RigidBody::dynamic(),
        ));
        run(&mut physics, &mut world, 1.0);

        let started: Vec<_> = physics.collisions().iter().filter(|c| c.started).collect();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].other(ball), Some(ground));
        assert_eq!(started[0].other(ground), Some(ball));

        physics.clear_collisions();
        assert!(physics.collisions().is_empty());
    }

    #[test]
    fn sensor_reports_overlap_without_blocking() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        let zone = world.spawn((
            Transform::from_xyz(0.0, 2.0, 0.0),
            Collider::cuboid(Vec3::splat(2.0)).as_sensor(),
        ));
        let ball = world.spawn((
            Transform::from_xyz(0.0, 5.0, 0.0),
            Collider::sphere(0.25),
            RigidBody::dynamic(),
        ));
        run(&mut physics, &mut world, 2.0);
        assert!(
            position(&world, ball).y < 0.0,
            "the sensor must not stop the ball"
        );
        let entered = physics
            .collisions()
            .iter()
            .any(|c| c.started && c.other(zone) == Some(ball));
        let left = physics
            .collisions()
            .iter()
            .any(|c| !c.started && c.other(zone) == Some(ball));
        assert!(entered && left);
    }

    #[test]
    fn raycast_finds_the_nearest_collider() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        let ground = floor(&mut world);
        let cube = world.spawn((
            Transform::from_xyz(0.0, 0.5, 0.0),
            Collider::cuboid(Vec3::ONE),
        ));
        physics.step(&mut world, DT);

        let down =
            |x: f32, ignore| physics.raycast(Vec3::new(x, 10.0, 0.0), Vec3::NEG_Y, 100.0, ignore);
        let hit = down(0.0, None).unwrap();
        assert_eq!(hit.entity, cube);
        assert!((hit.distance - 9.0).abs() < 1e-3);
        assert!((hit.point - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-3);
        assert!((hit.normal - Vec3::Y).length() < 1e-3);

        assert_eq!(down(5.0, None).unwrap().entity, ground);
        assert_eq!(down(0.0, Some(cube)).unwrap().entity, ground);
        assert!(physics
            .raycast(Vec3::new(0.0, 10.0, 0.0), Vec3::Y, 100.0, None)
            .is_none());
        assert!(physics
            .raycast(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y, 5.0, None)
            .is_none());
    }

    #[test]
    fn despawned_entities_leave_the_simulation() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        let cube = world.spawn((
            Transform::from_xyz(0.0, 0.5, 0.0),
            Collider::cuboid(Vec3::ONE),
        ));
        physics.step(&mut world, DT);
        assert!(physics
            .raycast(Vec3::Y * 10.0, Vec3::NEG_Y, 100.0, None)
            .is_some());

        world.despawn(cube).unwrap();
        physics.step(&mut world, DT);
        assert!(physics
            .raycast(Vec3::Y * 10.0, Vec3::NEG_Y, 100.0, None)
            .is_none());
    }

    #[test]
    fn editing_the_transform_teleports_the_body() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        floor(&mut world);
        let cube = world.spawn((
            Transform::from_xyz(0.0, 0.5, 0.0),
            Collider::cuboid(Vec3::ONE),
            RigidBody::dynamic(),
        ));
        run(&mut physics, &mut world, 0.5);

        world.get::<&mut Transform>(cube).unwrap().translation = Vec3::new(8.0, 0.5, 0.0);
        run(&mut physics, &mut world, 0.5);
        let moved = position(&world, cube);
        assert!(
            (moved - Vec3::new(8.0, 0.5, 0.0)).length() < 0.05,
            "{moved}"
        );
    }

    #[test]
    fn impulses_and_initial_velocity_move_bodies() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        physics.set_gravity(Vec3::ZERO);
        let thrown = world.spawn((
            Transform::IDENTITY,
            Collider::sphere(0.5),
            RigidBody::dynamic().with_velocity(Vec3::X * 2.0),
        ));
        let pushed = world.spawn((
            Transform::from_xyz(0.0, 10.0, 0.0),
            Collider::sphere(0.5),
            RigidBody::dynamic(),
        ));
        assert!(
            !physics.apply_impulse(pushed, Vec3::Z),
            "not in the simulation yet"
        );
        physics.step(&mut world, DT);
        assert!(physics.apply_impulse(pushed, Vec3::Z));

        run(&mut physics, &mut world, 1.0);
        assert!((position(&world, thrown).x - 2.0).abs() < 0.05);
        assert!(position(&world, pushed).z > 0.1);
    }

    #[test]
    fn kinematic_body_pushes_dynamic_ones() {
        let (mut physics, mut world) = (Physics::new(), World::new());
        physics.set_gravity(Vec3::ZERO);
        let pusher = world.spawn((
            Transform::IDENTITY,
            Collider::cuboid(Vec3::ONE),
            RigidBody::kinematic(),
        ));
        let ball = world.spawn((
            Transform::from_xyz(1.5, 0.0, 0.0),
            Collider::sphere(0.5),
            RigidBody::dynamic(),
        ));
        for _ in 0..60 {
            world.get::<&mut Transform>(pusher).unwrap().translation.x += 2.0 * DT;
            physics.step(&mut world, DT);
        }
        assert!((position(&world, pusher).x - 2.0).abs() < 0.05);
        assert!(position(&world, ball).x > 3.0, "{}", position(&world, ball));
    }
}
