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

/// A human-readable label for an entity, e.g. the node name from a model file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name(pub String);

/// Every entity whose [`Name`] is exactly `name`, in a stable order.
pub fn find_by_name(world: &World, name: &str) -> Vec<Entity> {
    let mut found: Vec<Entity> = world
        .query::<(Entity, &Name)>()
        .iter()
        .filter(|(_, entity_name)| entity_name.0 == name)
        .map(|(entity, _)| entity)
        .collect();
    found.sort_by_key(|entity| entity.to_bits());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_by_name_matches_exactly() {
        let mut world = World::new();
        let a = world.spawn((Name("Cell".to_owned()),));
        let b = world.spawn((Name("Cell".to_owned()),));
        world.spawn((Name("Cellar".to_owned()),));
        world.spawn((Transform::IDENTITY,));
        assert_eq!(find_by_name(&world, "Cell"), [a, b]);
        assert!(find_by_name(&world, "Missing").is_empty());
    }
}
