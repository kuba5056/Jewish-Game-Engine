use std::collections::HashMap;

use glam::Mat4;
use hecs::{Entity, World};

use crate::transform::{GlobalTransform, Transform};

/// Makes an entity's [`Transform`] relative to another entity.
///
/// If the parent is despawned or has no `Transform`, the child behaves as if
/// it had no parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent(pub Entity);

/// Recomputes the [`GlobalTransform`] of every entity that has a
/// [`Transform`], adding the component where it is missing.
///
/// The engine runs this once per frame after `update`; call it yourself if
/// you need up-to-date world positions earlier.
pub fn propagate_transforms(world: &mut World) {
    let locals: HashMap<Entity, (Mat4, Option<Entity>)> = world
        .query::<(Entity, &Transform, Option<&Parent>)>()
        .iter()
        .map(|(entity, transform, parent)| (entity, (transform.matrix(), parent.map(|p| p.0))))
        .collect();

    let mut globals: HashMap<Entity, Mat4> = HashMap::with_capacity(locals.len());
    let mut chain = Vec::new();
    for &entity in locals.keys() {
        // Walk up until we reach something already resolved or a root, then
        // come back down multiplying, so each entity is computed only once.
        let mut base = Mat4::IDENTITY;
        let mut current = entity;
        loop {
            if let Some(global) = globals.get(&current) {
                base = *global;
                break;
            }
            let Some(&(_, parent)) = locals.get(&current) else {
                break; // Parent is gone or has no Transform: treat as world.
            };
            chain.push(current);
            match parent {
                // A parent already in the chain means a cycle; cut it here
                // instead of looping forever.
                Some(parent) if !chain.contains(&parent) => current = parent,
                _ => break,
            }
        }
        for entity in chain.drain(..).rev() {
            base *= locals[&entity].0;
            globals.insert(entity, base);
        }
    }

    let mut missing = Vec::new();
    for (entity, matrix) in globals {
        match world.get::<&mut GlobalTransform>(entity) {
            Ok(mut global) => global.0 = matrix,
            Err(_) => missing.push((entity, matrix)),
        }
    }
    for (entity, matrix) in missing {
        // The entity came from a query a moment ago, so it still exists.
        let _ = world.insert_one(entity, GlobalTransform(matrix));
    }
}

/// Despawns an entity together with all of its descendants.
pub fn despawn_recursive(world: &mut World, entity: Entity) {
    let mut children: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (child, parent) in world.query::<(Entity, &Parent)>().iter() {
        children.entry(parent.0).or_default().push(child);
    }

    let mut pending = vec![entity];
    while let Some(entity) = pending.pop() {
        // `remove` rather than `get`, so a parent cycle cannot revisit a node.
        if let Some(kids) = children.remove(&entity) {
            pending.extend(kids);
        }
        let _ = world.despawn(entity);
    }
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    fn position(world: &World, entity: Entity) -> Vec3 {
        world.get::<&GlobalTransform>(entity).unwrap().translation()
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn children_follow_their_ancestors() {
        let mut world = World::new();
        // Parent turned 90° about Y: its local +X points along world -Z.
        let root = world.spawn((Transform::from_xyz(10.0, 0.0, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),));
        let child = world.spawn((Transform::from_xyz(2.0, 0.0, 0.0), Parent(root)));
        let grandchild = world.spawn((Transform::from_xyz(0.0, 1.0, 0.0), Parent(child)));

        propagate_transforms(&mut world);
        assert!(close(position(&world, root), Vec3::new(10.0, 0.0, 0.0)));
        assert!(close(position(&world, child), Vec3::new(10.0, 0.0, -2.0)));
        assert!(close(
            position(&world, grandchild),
            Vec3::new(10.0, 1.0, -2.0)
        ));

        // Moving the root moves the whole subtree on the next pass.
        world.get::<&mut Transform>(root).unwrap().translation.y = 5.0;
        propagate_transforms(&mut world);
        assert!(close(
            position(&world, grandchild),
            Vec3::new(10.0, 6.0, -2.0)
        ));
    }

    #[test]
    fn parent_scale_applies_to_children() {
        let mut world = World::new();
        let root = world.spawn((Transform::IDENTITY.with_scale(Vec3::splat(3.0)),));
        let child = world.spawn((Transform::from_xyz(1.0, 0.0, 0.0), Parent(root)));
        propagate_transforms(&mut world);
        assert!(close(position(&world, child), Vec3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn orphan_falls_back_to_world_space() {
        let mut world = World::new();
        let root = world.spawn((Transform::from_xyz(10.0, 0.0, 0.0),));
        let child = world.spawn((Transform::from_xyz(1.0, 0.0, 0.0), Parent(root)));
        world.despawn(root).unwrap();
        propagate_transforms(&mut world);
        assert!(close(position(&world, child), Vec3::new(1.0, 0.0, 0.0)));
    }

    #[test]
    fn parent_cycle_terminates() {
        let mut world = World::new();
        let a = world.spawn((Transform::from_xyz(1.0, 0.0, 0.0),));
        let b = world.spawn((Transform::from_xyz(1.0, 0.0, 0.0), Parent(a)));
        world.insert_one(a, Parent(b)).unwrap();
        propagate_transforms(&mut world);
        assert!(world.get::<&GlobalTransform>(a).is_ok());
        assert!(world.get::<&GlobalTransform>(b).is_ok());
    }

    #[test]
    fn despawn_recursive_removes_the_subtree_only() {
        let mut world = World::new();
        let root = world.spawn((Transform::IDENTITY,));
        let child = world.spawn((Transform::IDENTITY, Parent(root)));
        let grandchild = world.spawn((Transform::IDENTITY, Parent(child)));
        let other = world.spawn((Transform::IDENTITY,));

        despawn_recursive(&mut world, child);
        assert!(world.contains(root));
        assert!(!world.contains(child));
        assert!(!world.contains(grandchild));
        assert!(world.contains(other));
    }
}
