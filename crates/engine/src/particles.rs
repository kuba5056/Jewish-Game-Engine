//! Particle effects: emitters that spawn, move and fade short-lived sprites.

use engine_render::glam::{Vec3, Vec4};
use engine_render::{ParticleBlend, Renderer};
use engine_scene::{Entity, GlobalTransform, World};
use serde::{Deserialize, Serialize};

/// Emits particles from the entity's position: fire, smoke, sparks, dust.
///
/// Particles live in world space, so a moving emitter leaves a trail. Start
/// from a preset ([`fire`](Self::fire), [`smoke`](Self::smoke),
/// [`burst`](Self::burst)) or [`Default`] and adjust the fields:
///
/// ```
/// # use engine::ParticleEmitter;
/// # use engine::math::Vec3;
/// let fountain = ParticleEmitter {
///     rate: 80.0,
///     speed: (5.0, 7.0),
///     gravity: Vec3::new(0.0, -9.81, 0.0),
///     ..ParticleEmitter::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleEmitter {
    /// Particles emitted per second while `emitting`.
    pub rate: f32,
    /// Particles to emit all at once on the next frame; reset to 0 after.
    pub burst: u32,
    /// Whether the continuous `rate` emission is on.
    pub emitting: bool,
    /// How long a particle lives, in seconds: (shortest, longest).
    pub lifetime: (f32, f32),
    /// Speed a particle starts with: (slowest, fastest).
    pub speed: (f32, f32),
    /// The middle of the cone particles are emitted into, in the emitter's
    /// own space (so rotating the entity aims it).
    pub direction: Vec3,
    /// Half-angle of that cone in degrees: 0 is a jet, 180 is all directions.
    pub spread: f32,
    /// Particles appear at a random point within this distance of the emitter.
    pub radius: f32,
    /// Constant acceleration: downwards for sparks, upwards for hot smoke.
    pub gravity: Vec3,
    /// Air resistance: the fraction of its speed a particle loses per second.
    pub drag: f32,
    /// Diameter at birth and at death.
    pub size: (f32, f32),
    /// Colour at birth (linear RGBA; values above 1 glow).
    pub color_start: Vec4,
    /// Colour at death. Fade alpha to 0 to avoid particles popping out.
    pub color_end: Vec4,
    /// Add light instead of covering what is behind: fire and sparks.
    pub additive: bool,
    /// The most particles alive at once; emission pauses at the limit.
    pub max_particles: usize,
    /// Despawn the entity once it has stopped emitting and its last
    /// particle is gone: for one-off effects such as an explosion.
    pub despawn_when_done: bool,
}

/// An emitter's live particles. Kept apart from [`ParticleEmitter`] so that
/// the emitter stays plain settings that can be compared, copied and saved;
/// the engine adds this next to every emitter.
#[derive(Debug, Default)]
pub(crate) struct ParticleState {
    particles: Vec<Particle>,
    /// Fraction of a particle carried over between frames, so low rates
    /// still emit steadily.
    carry: f32,
    random: Random,
}

#[derive(Debug, Clone, Copy)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    lifetime: f32,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            rate: 20.0,
            burst: 0,
            emitting: true,
            lifetime: (1.0, 1.5),
            speed: (1.0, 2.0),
            direction: Vec3::Y,
            spread: 20.0,
            radius: 0.0,
            gravity: Vec3::ZERO,
            drag: 0.0,
            size: (0.2, 0.05),
            color_start: Vec4::ONE,
            color_end: Vec4::new(1.0, 1.0, 1.0, 0.0),
            additive: false,
            max_particles: 2000,
            despawn_when_done: false,
        }
    }
}

impl ParticleEmitter {
    /// Flames: bright, rising, short-lived, glowing.
    pub fn fire() -> Self {
        Self {
            rate: 60.0,
            lifetime: (0.5, 0.9),
            speed: (1.0, 1.8),
            spread: 14.0,
            radius: 0.15,
            gravity: Vec3::new(0.0, 1.5, 0.0),
            size: (0.45, 0.1),
            color_start: Vec4::new(4.0, 1.6, 0.3, 0.9),
            color_end: Vec4::new(1.5, 0.1, 0.0, 0.0),
            additive: true,
            ..Self::default()
        }
    }

    /// Smoke: grey puffs that rise slowly, grow and thin out.
    pub fn smoke() -> Self {
        Self {
            rate: 12.0,
            lifetime: (2.5, 3.5),
            speed: (0.5, 0.9),
            spread: 12.0,
            radius: 0.1,
            gravity: Vec3::new(0.0, 0.3, 0.0),
            drag: 0.3,
            size: (0.4, 1.6),
            color_start: Vec4::new(0.25, 0.25, 0.27, 0.55),
            color_end: Vec4::new(0.4, 0.4, 0.42, 0.0),
            ..Self::default()
        }
    }

    /// A one-off spray of `count` glowing sparks in every direction that
    /// fall under gravity. The entity despawns itself afterwards.
    pub fn burst(count: u32) -> Self {
        Self {
            rate: 0.0,
            burst: count,
            emitting: false,
            lifetime: (0.4, 0.9),
            speed: (2.0, 6.0),
            spread: 180.0,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            drag: 1.0,
            size: (0.14, 0.02),
            color_start: Vec4::new(4.0, 3.0, 1.0, 1.0),
            color_end: Vec4::new(3.0, 0.6, 0.1, 0.0),
            additive: true,
            despawn_when_done: true,
            ..Self::default()
        }
    }
}

impl ParticleState {
    /// Whether nothing is left to show: not emitting, no burst pending and
    /// no particle alive.
    fn is_done(&self, emitter: &ParticleEmitter) -> bool {
        let emitting = emitter.emitting && emitter.rate > 0.0;
        !emitting && emitter.burst == 0 && self.particles.is_empty()
    }

    /// Ages and moves the live particles, then emits new ones at the
    /// emitter's world transform.
    fn update(&mut self, emitter: &mut ParticleEmitter, dt: f32, transform: &GlobalTransform) {
        let (gravity, damping) = (emitter.gravity, (1.0 - emitter.drag * dt).max(0.0));
        self.particles.retain_mut(|particle| {
            particle.age += dt;
            particle.velocity = (particle.velocity + gravity * dt) * damping;
            particle.position += particle.velocity * dt;
            particle.age < particle.lifetime
        });

        let mut count = std::mem::take(&mut emitter.burst) as usize;
        if emitter.emitting && emitter.rate > 0.0 {
            self.carry += emitter.rate * dt;
            count += self.carry as usize;
            self.carry = self.carry.fract();
        }
        let room = emitter.max_particles.saturating_sub(self.particles.len());
        let origin = transform.translation();
        let axis = transform
            .0
            .transform_vector3(emitter.direction)
            .normalize_or(Vec3::Y);
        for _ in 0..count.min(room) {
            let direction = self.random.in_cone(axis, emitter.spread.to_radians());
            let offset = self.random.in_sphere() * emitter.radius;
            self.particles.push(Particle {
                position: origin + offset,
                velocity: direction * self.random.between(emitter.speed),
                age: 0.0,
                lifetime: self.random.between(emitter.lifetime).max(1e-3),
            });
        }
    }

    fn draw(&self, emitter: &ParticleEmitter, renderer: &mut Renderer) {
        let blend = if emitter.additive {
            ParticleBlend::Additive
        } else {
            ParticleBlend::Alpha
        };
        for particle in &self.particles {
            let t = particle.age / particle.lifetime;
            let size = emitter.size.0 + (emitter.size.1 - emitter.size.0) * t;
            let color = emitter.color_start.lerp(emitter.color_end, t);
            renderer.draw_particle(particle.position, size, color, blend);
        }
    }
}

/// Updates every emitter by `dt` seconds, queues its particles for drawing
/// and despawns finished one-off effects. Run by the engine each frame,
/// after transforms are up to date.
pub(crate) fn update_particles(world: &mut World, renderer: &mut Renderer, dt: f32) {
    step_emitters(world, dt, |emitter, state| state.draw(emitter, renderer));
}

/// The part of [`update_particles`] that needs no renderer: gives new
/// emitters their state, advances every emitter, calls `each` on it, and
/// despawns finished one-off effects.
fn step_emitters(
    world: &mut World,
    dt: f32,
    mut each: impl FnMut(&ParticleEmitter, &ParticleState),
) {
    let new: Vec<Entity> = world
        .query::<Entity>()
        .with::<&ParticleEmitter>()
        .without::<&ParticleState>()
        .iter()
        .collect();
    for entity in new {
        let _ = world.insert_one(entity, ParticleState::default());
    }

    let mut finished: Vec<Entity> = Vec::new();
    for (entity, transform, emitter, state) in world.query_mut::<(
        Entity,
        &GlobalTransform,
        &mut ParticleEmitter,
        &mut ParticleState,
    )>() {
        state.update(emitter, dt, transform);
        each(emitter, state);
        if emitter.despawn_when_done && state.is_done(emitter) {
            finished.push(entity);
        }
    }
    for entity in finished {
        let _ = world.despawn(entity);
    }
}

/// A small, fast pseudo-random generator (xorshift32). Particle effects
/// need variety, not statistical quality, so this avoids a dependency.
#[derive(Debug, Clone)]
struct Random(u32);

impl Default for Random {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        // A different seed per emitter, so identical emitters differ.
        static NEXT: AtomicU32 = AtomicU32::new(0x9E37_79B9);
        Self(NEXT.fetch_add(0x6D2B_79F5, Ordering::Relaxed) | 1)
    }
}

impl Random {
    /// Uniform in `0..1`.
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }

    fn between(&mut self, (low, high): (f32, f32)) -> f32 {
        low + (high - low) * self.unit()
    }

    /// A uniformly random point inside the unit sphere.
    fn in_sphere(&mut self) -> Vec3 {
        let direction = self.in_cone(Vec3::Y, std::f32::consts::PI);
        direction * self.unit().cbrt()
    }

    /// A unit vector within `half_angle` radians of `axis`, uniformly
    /// distributed over that cap of the sphere.
    fn in_cone(&mut self, axis: Vec3, half_angle: f32) -> Vec3 {
        let cos_limit = half_angle.clamp(0.0, std::f32::consts::PI).cos();
        let cos = 1.0 - self.unit() * (1.0 - cos_limit);
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let around = self.unit() * std::f32::consts::TAU;
        let (side, up) = axis.any_orthonormal_pair();
        (side * around.cos() + up * around.sin()) * sin + axis * cos
    }
}

#[cfg(test)]
mod tests {
    use engine_render::glam::{Mat4, Quat};
    use engine_scene::Transform;

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn at(position: Vec3) -> GlobalTransform {
        GlobalTransform(Mat4::from_translation(position))
    }

    fn run(
        state: &mut ParticleState,
        emitter: &mut ParticleEmitter,
        seconds: f32,
        transform: &GlobalTransform,
    ) {
        for _ in 0..(seconds / DT).round() as u32 {
            state.update(emitter, DT, transform);
        }
    }

    #[test]
    fn emits_at_the_configured_rate_and_expires_particles() {
        let mut state = ParticleState::default();
        let mut emitter = ParticleEmitter {
            rate: 30.0,
            lifetime: (0.5, 0.5),
            ..ParticleEmitter::default()
        };
        // Steady state: rate times lifetime particles alive.
        run(&mut state, &mut emitter, 2.0, &at(Vec3::ZERO));
        let alive = state.particles.len();
        assert!((14..=16).contains(&alive), "{alive}");

        emitter.emitting = false;
        run(&mut state, &mut emitter, 0.6, &at(Vec3::ZERO));
        assert!(state.particles.is_empty());
        assert!(state.is_done(&emitter));
    }

    #[test]
    fn low_rates_still_emit() {
        let mut state = ParticleState::default();
        let mut emitter = ParticleEmitter {
            rate: 2.0, // Far less than one per frame.
            lifetime: (10.0, 10.0),
            ..ParticleEmitter::default()
        };
        run(&mut state, &mut emitter, 3.0, &at(Vec3::ZERO));
        let alive = state.particles.len();
        assert!((5..=6).contains(&alive), "{alive}");
    }

    #[test]
    fn burst_emits_once_and_respects_the_limit() {
        let mut state = ParticleState::default();
        let mut emitter = ParticleEmitter {
            max_particles: 40,
            ..ParticleEmitter::burst(100)
        };
        assert!(!state.is_done(&emitter));
        state.update(&mut emitter, DT, &at(Vec3::ZERO));
        assert_eq!(state.particles.len(), 40);
        assert_eq!(emitter.burst, 0);
        state.update(&mut emitter, DT, &at(Vec3::ZERO));
        assert_eq!(state.particles.len(), 40, "a burst must not repeat");
    }

    #[test]
    fn particles_start_at_the_emitter_and_follow_gravity() {
        let mut state = ParticleState::default();
        let mut emitter = ParticleEmitter {
            rate: 0.0,
            burst: 1,
            speed: (0.0, 0.0),
            lifetime: (5.0, 5.0),
            gravity: Vec3::new(0.0, -10.0, 0.0),
            ..ParticleEmitter::default()
        };
        let origin = Vec3::new(3.0, 20.0, -1.0);
        state.update(&mut emitter, DT, &at(origin));
        assert_eq!(state.particles[0].position, origin);

        // Moving the emitter does not drag existing particles along.
        run(&mut state, &mut emitter, 1.0, &at(Vec3::ZERO));
        let fallen = state.particles[0].position;
        assert_eq!((fallen.x, fallen.z), (origin.x, origin.z));
        // About g*t²/2 = 5 below the start.
        assert!((origin.y - fallen.y - 5.0).abs() < 0.2, "{fallen}");
    }

    #[test]
    fn emission_cone_follows_the_entity_rotation() {
        let mut state = ParticleState::default();
        let mut emitter = ParticleEmitter {
            burst: 200,
            spread: 15.0,
            direction: Vec3::Y,
            ..ParticleEmitter::default()
        };
        // Rotated so the emitter's up axis points along world +X.
        let rotation = Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2);
        let transform = GlobalTransform(Transform::IDENTITY.with_rotation(rotation).matrix());
        state.update(&mut emitter, 0.0, &transform);
        assert_eq!(state.particles.len(), 200);
        let min_cos = 15.0_f32.to_radians().cos() - 1e-4;
        for particle in &state.particles {
            assert!(particle.velocity.normalize().dot(Vec3::X) >= min_cos);
        }
    }

    #[test]
    fn random_values_stay_in_range_and_vary() {
        let mut random = Random::default();
        let values: Vec<f32> = (0..1000).map(|_| random.unit()).collect();
        assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
        for _ in 0..200 {
            assert!(random.in_sphere().length() <= 1.0 + 1e-5);
            assert!((random.in_cone(Vec3::X, 3.0).length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn finished_one_off_effects_despawn_themselves() {
        let mut world = World::new();
        let effect = world.spawn((at(Vec3::ZERO), ParticleEmitter::burst(10)));
        let fire = world.spawn((at(Vec3::ZERO), ParticleEmitter::fire()));
        let mut seen = 0;
        for _ in 0..120 {
            step_emitters(&mut world, DT, |_, state| seen += state.particles.len());
        }
        assert!(seen > 0);
        assert!(!world.contains(effect));
        assert!(world.contains(fire));
    }
}
