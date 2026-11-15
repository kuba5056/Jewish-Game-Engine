//! Playing a model's animations and binding skinned meshes to their skeletons.

use std::sync::Arc;

use engine_assets::{AnimationData, Sample};
use engine_render::glam::Mat4;
use engine_scene::{Entity, GlobalTransform, Transform, World};

/// Binds a mesh to a skeleton: the entity's mesh is deformed by the joint
/// entities instead of being placed by its own `Transform`.
///
/// Added by `Model::spawn` to the entities of skinned meshes.
#[derive(Debug, Clone)]
pub struct Skin {
    /// The joint entities, in the order vertex joint indices refer to.
    pub joints: Vec<Entity>,
    /// Per joint, the transform from mesh space to joint space at rest.
    pub inverse_bind: Arc<[Mat4]>,
}

impl Skin {
    /// The matrices the renderer deforms the mesh with: each joint's current
    /// world transform times its inverse bind matrix. A joint that no longer
    /// exists leaves its vertices where they were modelled.
    pub(crate) fn joint_matrices(&self, world: &World) -> Vec<Mat4> {
        self.joints
            .iter()
            .zip(self.inverse_bind.iter())
            .map(
                |(&joint, inverse_bind)| match world.get::<&GlobalTransform>(joint) {
                    Ok(global) => global.0 * *inverse_bind,
                    Err(_) => Mat4::IDENTITY,
                },
            )
            .collect()
    }
}

/// Plays a model's animations. `Model::spawn` puts one on the root entity of
/// every model that has animations.
///
/// ```no_run
/// # use engine::{AnimationPlayer, Context, Entity};
/// # fn start(ctx: &mut Context, robot: Entity) {
/// if let Ok(mut player) = ctx.world.get::<&mut AnimationPlayer>(robot) {
///     player.play("Walk");
/// }
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct AnimationPlayer {
    clips: Arc<[AnimationData]>,
    /// The entity created for each of the model's nodes.
    nodes: Vec<Entity>,
    current: Option<usize>,
    time: f32,
    /// Playback rate: 1 is normal, 2 is double speed, negative runs backwards.
    pub speed: f32,
    /// Start over at the end instead of holding the last pose.
    pub looping: bool,
    pub paused: bool,
}

impl AnimationPlayer {
    pub(crate) fn new(clips: Arc<[AnimationData]>, nodes: Vec<Entity>) -> Self {
        Self {
            clips,
            nodes,
            current: None,
            time: 0.0,
            speed: 1.0,
            looping: true,
            paused: false,
        }
    }

    /// Names of the animations the model has.
    pub fn clip_names(&self) -> impl Iterator<Item = &str> {
        self.clips.iter().map(|clip| clip.name.as_str())
    }

    /// Starts the named animation from its beginning. Returns `false`, and
    /// changes nothing, if the model has no animation of that name.
    pub fn play(&mut self, name: &str) -> bool {
        match self.clips.iter().position(|clip| clip.name == name) {
            Some(index) => {
                self.current = Some(index);
                self.time = 0.0;
                self.paused = false;
                true
            }
            None => false,
        }
    }

    /// Stops animating; the model keeps the pose it is in.
    pub fn stop(&mut self) {
        self.current = None;
    }

    /// Name of the animation that is playing, if any.
    pub fn current(&self) -> Option<&str> {
        Some(self.clips.get(self.current?)?.name.as_str())
    }

    /// Position in the current animation, in seconds.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// Whether a non-looping animation has reached its end.
    pub fn is_finished(&self) -> bool {
        match self.current.and_then(|index| self.clips.get(index)) {
            Some(clip) => !self.looping && self.time >= clip.duration,
            None => true,
        }
    }

    /// Moves the playback position by `dt` seconds of real time.
    fn advance(&mut self, dt: f32) {
        let Some(clip) = self.current.and_then(|index| self.clips.get(index)) else {
            return;
        };
        if self.paused {
            return;
        }
        self.time += dt * self.speed;
        if self.looping && clip.duration > 0.0 {
            // rem_euclid also wraps negative times when playing backwards.
            self.time = self.time.rem_euclid(clip.duration);
        } else {
            self.time = self.time.clamp(0.0, clip.duration);
        }
    }

    /// What the current animation sets each node's properties to right now.
    fn pose(&self, out: &mut Vec<(Entity, Sample)>) {
        let Some(clip) = self.current.and_then(|index| self.clips.get(index)) else {
            return;
        };
        for channel in &clip.channels {
            if let (Some(&entity), Some(sample)) =
                (self.nodes.get(channel.node), channel.sample(self.time))
            {
                out.push((entity, sample));
            }
        }
    }
}

/// Advances every [`AnimationPlayer`] by `dt` seconds and writes the
/// resulting poses into the animated entities' `Transform`s.
pub(crate) fn animate(world: &mut World, dt: f32) {
    let mut poses = Vec::new();
    for player in world.query_mut::<&mut AnimationPlayer>() {
        player.advance(dt);
        player.pose(&mut poses);
    }
    for (entity, sample) in poses {
        // A node entity may have been despawned; its channels then do nothing.
        if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
            match sample {
                Sample::Translation(value) => transform.translation = value,
                Sample::Rotation(value) => transform.rotation = value,
                Sample::Scale(value) => transform.scale = value,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use engine_assets::{Channel, Keyframes};
    use engine_render::glam::Vec3;

    use super::*;

    /// One clip, "Slide": node 0 moves from x = 0 to x = 10 over two seconds.
    fn player(world: &mut World) -> (Entity, Entity) {
        let node = world.spawn((Transform::IDENTITY,));
        let clip = AnimationData {
            name: "Slide".to_owned(),
            duration: 2.0,
            channels: vec![Channel {
                node: 0,
                times: vec![0.0, 2.0],
                values: Keyframes::Translation(vec![Vec3::ZERO, Vec3::X * 10.0]),
                step: false,
            }],
        };
        let root = world.spawn((AnimationPlayer::new(Arc::from(vec![clip]), vec![node]),));
        (root, node)
    }

    fn x(world: &World, node: Entity) -> f32 {
        world.get::<&Transform>(node).unwrap().translation.x
    }

    #[test]
    fn nothing_moves_until_a_clip_is_played() {
        let mut world = World::new();
        let (root, node) = player(&mut world);
        animate(&mut world, 1.0);
        assert_eq!(x(&world, node), 0.0);

        let mut player = world.get::<&mut AnimationPlayer>(root).unwrap();
        assert!(!player.play("Missing"));
        assert_eq!(player.current(), None);
        assert!(player.play("Slide"));
        assert_eq!(player.current(), Some("Slide"));
        assert_eq!(player.clip_names().collect::<Vec<_>>(), ["Slide"]);
    }

    #[test]
    fn looping_clip_wraps_around() {
        let mut world = World::new();
        let (root, node) = player(&mut world);
        world
            .get::<&mut AnimationPlayer>(root)
            .unwrap()
            .play("Slide");

        animate(&mut world, 0.5);
        assert!((x(&world, node) - 2.5).abs() < 1e-4);
        animate(&mut world, 2.0); // 2.5 s into a 2 s loop is 0.5 s
        assert!((x(&world, node) - 2.5).abs() < 1e-4);
        assert!(!world.get::<&AnimationPlayer>(root).unwrap().is_finished());
    }

    #[test]
    fn one_shot_clip_holds_its_last_pose() {
        let mut world = World::new();
        let (root, node) = player(&mut world);
        {
            let mut player = world.get::<&mut AnimationPlayer>(root).unwrap();
            player.play("Slide");
            player.looping = false;
        }
        animate(&mut world, 5.0);
        assert_eq!(x(&world, node), 10.0);
        assert!(world.get::<&AnimationPlayer>(root).unwrap().is_finished());
    }

    #[test]
    fn speed_and_pause_control_playback() {
        let mut world = World::new();
        let (root, node) = player(&mut world);
        {
            let mut player = world.get::<&mut AnimationPlayer>(root).unwrap();
            player.play("Slide");
            player.speed = 2.0;
        }
        animate(&mut world, 0.5);
        assert!((x(&world, node) - 5.0).abs() < 1e-4);

        world.get::<&mut AnimationPlayer>(root).unwrap().paused = true;
        animate(&mut world, 0.5);
        assert!((x(&world, node) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn skin_at_rest_leaves_the_mesh_in_place() {
        let mut world = World::new();
        // A joint resting two units up; its inverse bind matrix undoes that.
        let joint = world.spawn((Transform::from_xyz(0.0, 2.0, 0.0),));
        engine_scene::propagate_transforms(&mut world);
        let skin = Skin {
            joints: vec![joint],
            inverse_bind: Arc::from(vec![Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0))]),
        };
        assert!(skin.joint_matrices(&world)[0].abs_diff_eq(Mat4::IDENTITY, 1e-6));

        // Moving the joint moves the vertices bound to it by the same amount.
        world.get::<&mut Transform>(joint).unwrap().translation.x = 3.0;
        engine_scene::propagate_transforms(&mut world);
        let moved = skin.joint_matrices(&world)[0].transform_point3(Vec3::new(0.0, 2.5, 0.0));
        assert!(moved.abs_diff_eq(Vec3::new(3.0, 2.5, 0.0), 1e-6));
    }
}
