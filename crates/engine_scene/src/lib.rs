//! Entities, components and the transform hierarchy, built on the `hecs` ECS.
//!
//! An entity is positioned by its [`Transform`], relative to its [`Parent`] if
//! it has one. [`propagate_transforms`] turns those into world-space
//! [`GlobalTransform`]s, which is what rendering and physics read.

mod hierarchy;
mod transform;

pub use hecs;
pub use hecs::{Entity, World};
pub use hierarchy::{despawn_recursive, propagate_transforms, Parent};
pub use transform::{GlobalTransform, Transform};
