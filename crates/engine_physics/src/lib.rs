//! Rigid-body physics on `rapier3d`, driven by components.
//!
//! Give an entity a `Transform` and a [`Collider`] and it becomes a solid,
//! immovable obstacle. Add a [`RigidBody`] and it moves: falling, colliding
//! and coming to rest. [`Physics::step`] creates the simulation objects for
//! new entities, removes them for despawned ones, and writes the simulated
//! positions back into each `Transform`.
//!
//! Physics works in world space, so entities with a body should not have a
//! `Parent`.

mod components;
mod world;

pub use components::{BodyKind, Collider, RigidBody, Shape};
pub use world::{Collision, Physics, PhysicsBody, RayHit};
