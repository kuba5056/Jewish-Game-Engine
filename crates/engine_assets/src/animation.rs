use glam::{Mat4, Quat, Vec3};

/// A skeleton binding: which nodes act as joints for a skinned mesh.
#[derive(Debug, Clone)]
pub struct SkinData {
    /// Indices into `ModelData::nodes`. Vertex joint indices refer to
    /// positions in this list.
    pub joints: Vec<usize>,
    /// Per joint, the transform from mesh space to that joint's space in
    /// the pose the mesh was modelled in.
    pub inverse_bind: Vec<Mat4>,
}

/// One named animation: a set of channels that each drive one property of
/// one node over time.
#[derive(Debug, Clone)]
pub struct AnimationData {
    pub name: String,
    /// Length in seconds: the time of the last keyframe of any channel.
    pub duration: f32,
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone)]
pub struct Channel {
    /// Index into `ModelData::nodes`.
    pub node: usize,
    /// Keyframe times in seconds, ascending. Same length as the values.
    pub times: Vec<f32>,
    pub values: Keyframes,
    /// Hold each value until the next keyframe instead of blending.
    pub step: bool,
}

#[derive(Debug, Clone)]
pub enum Keyframes {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

/// The value of one animated property at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sample {
    Translation(Vec3),
    Rotation(Quat),
    Scale(Vec3),
}

impl Channel {
    /// The channel's value at `time` seconds. Before the first keyframe and
    /// after the last one the nearest keyframe's value holds. `None` if the
    /// channel has no keyframes.
    pub fn sample(&self, time: f32) -> Option<Sample> {
        let last = self.times.len().checked_sub(1)?;
        // The keyframe at or before `time`, and how far towards the next.
        let after = self.times.partition_point(|&t| t <= time);
        let (from, to, blend) = if after == 0 {
            (0, 0, 0.0)
        } else if after > last {
            (last, last, 0.0)
        } else {
            let (t0, t1) = (self.times[after - 1], self.times[after]);
            let blend = if self.step || t1 <= t0 {
                0.0
            } else {
                (time - t0) / (t1 - t0)
            };
            (after - 1, after, blend)
        };
        Some(match &self.values {
            Keyframes::Translation(v) => Sample::Translation(v.get(from)?.lerp(*v.get(to)?, blend)),
            Keyframes::Scale(v) => Sample::Scale(v.get(from)?.lerp(*v.get(to)?, blend)),
            // Slerp takes the short way round and keeps the result normalised.
            Keyframes::Rotation(v) => Sample::Rotation(v.get(from)?.slerp(*v.get(to)?, blend)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translation_channel(step: bool) -> Channel {
        Channel {
            node: 0,
            times: vec![1.0, 2.0, 4.0],
            values: Keyframes::Translation(vec![Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 30.0]),
            step,
        }
    }

    fn x(sample: Option<Sample>) -> f32 {
        match sample {
            Some(Sample::Translation(v)) => v.x,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn blends_linearly_between_keyframes() {
        let channel = translation_channel(false);
        assert_eq!(x(channel.sample(1.0)), 0.0);
        assert_eq!(x(channel.sample(1.5)), 5.0);
        assert_eq!(x(channel.sample(2.0)), 10.0);
        assert_eq!(x(channel.sample(3.0)), 20.0);
    }

    #[test]
    fn holds_the_end_values_outside_the_keyframes() {
        let channel = translation_channel(false);
        assert_eq!(x(channel.sample(-5.0)), 0.0);
        assert_eq!(x(channel.sample(0.5)), 0.0);
        assert_eq!(x(channel.sample(4.0)), 30.0);
        assert_eq!(x(channel.sample(99.0)), 30.0);
    }

    #[test]
    fn step_channels_do_not_blend() {
        let channel = translation_channel(true);
        assert_eq!(x(channel.sample(1.9)), 0.0);
        assert_eq!(x(channel.sample(2.0)), 10.0);
        assert_eq!(x(channel.sample(3.9)), 10.0);
    }

    #[test]
    fn rotations_take_the_short_way_and_stay_unit_length() {
        let channel = Channel {
            node: 0,
            times: vec![0.0, 1.0],
            values: Keyframes::Rotation(vec![
                Quat::IDENTITY,
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ]),
            step: false,
        };
        let Some(Sample::Rotation(half)) = channel.sample(0.5) else {
            panic!("expected a rotation");
        };
        assert!((half.length() - 1.0).abs() < 1e-5);
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        assert!(half.angle_between(expected) < 1e-4);
    }

    #[test]
    fn empty_and_mismatched_channels_yield_nothing() {
        let empty = Channel {
            node: 0,
            times: vec![],
            values: Keyframes::Scale(vec![]),
            step: false,
        };
        assert_eq!(empty.sample(0.0), None);
        // More times than values: sampling past the values is not a panic.
        let short = Channel {
            node: 0,
            times: vec![0.0, 1.0],
            values: Keyframes::Scale(vec![Vec3::ONE]),
            step: false,
        };
        assert_eq!(short.sample(2.0), None);
    }
}
