//! Particle effects: a campfire with smoke, a fountain, and bursts of sparks.
//!
//! Space sets off a burst; Escape quits.
//!
//! Run from the repository root with: `cargo run --example particles`

use engine::math::{Vec3, Vec4};
use engine::{
    Config, Context, DirectionalLight, Game, KeyCode, MaterialDesc, ParticleEmitter, PointLight,
    Transform, Visual, WindowConfig,
};

#[derive(Default)]
struct ParticlesDemo {
    next_burst: f32,
    bursts: u32,
}

impl ParticlesDemo {
    fn burst(&mut self, ctx: &mut Context) {
        // Alternates between three spots above the ground.
        let x = (self.bursts % 3) as f32 * 2.0 - 2.0;
        self.bursts += 1;
        // A one-off effect: the entity despawns itself when the sparks die.
        ctx.world.spawn((
            Transform::from_xyz(x, 2.5, -2.0),
            ParticleEmitter::burst(150),
        ));
    }
}

impl Game for ParticlesDemo {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.03, 0.04, 0.08);
        ctx.renderer.ambient_sky = Vec3::new(0.05, 0.06, 0.1);
        ctx.camera.position = Vec3::new(0.0, 2.5, 8.0);
        ctx.camera.look_at(Vec3::new(0.0, 1.5, 0.0));

        // Dim moonlight, so the effects stand out.
        ctx.world.spawn((
            Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.6)),
            DirectionalLight {
                color: Vec3::new(0.6, 0.7, 1.0),
                intensity: 0.5,
            },
        ));
        ctx.world.spawn((
            Transform::IDENTITY,
            Visual::Plane {
                size: 40.0,
                uv_repeat: 1.0,
                material: MaterialDesc::colored(0.3, 0.32, 0.3),
            },
        ));

        // Campfire: logs, flames, smoke above them, and a light so the fire
        // actually illuminates its surroundings.
        let fire = Vec3::new(-2.5, 0.0, 0.0);
        ctx.world.spawn((
            Transform::from_translation(fire + Vec3::Y * 0.1).with_scale(Vec3::new(0.9, 0.2, 0.3)),
            Visual::Cube {
                size: 1.0,
                material: MaterialDesc::colored(0.2, 0.12, 0.06),
            },
        ));
        ctx.world.spawn((
            Transform::from_translation(fire + Vec3::Y * 0.25),
            ParticleEmitter::fire(),
            PointLight {
                color: Vec3::new(1.0, 0.5, 0.2),
                intensity: 5.0,
                range: 8.0,
            },
        ));
        ctx.world.spawn((
            Transform::from_translation(fire + Vec3::Y * 1.0),
            ParticleEmitter::smoke(),
        ));

        // Fountain: a custom emitter. Fast, narrow, pulled back by gravity.
        ctx.world.spawn((
            Transform::from_xyz(2.5, 0.1, 0.0),
            ParticleEmitter {
                rate: 150.0,
                lifetime: (1.2, 1.6),
                speed: (6.0, 7.0),
                spread: 8.0,
                gravity: Vec3::new(0.0, -9.81, 0.0),
                size: (0.12, 0.06),
                color_start: Vec4::new(0.5, 0.8, 1.6, 0.8),
                color_end: Vec4::new(0.3, 0.5, 1.2, 0.0),
                additive: true,
                ..ParticleEmitter::default()
            },
        ));
        self.next_burst = 1.0;
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        let elapsed = ctx.time.elapsed().as_secs_f32();
        // Lets automated runs stop on their own: PARTICLES_SECONDS=3.
        if let Some(limit) = std::env::var("PARTICLES_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if elapsed >= limit {
                ctx.exit();
            }
        }

        if ctx.input.key_pressed(KeyCode::Space) || elapsed >= self.next_burst {
            self.next_burst = elapsed + 1.5;
            self.burst(ctx);
        }
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Particles".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, ParticlesDemo::default())
}
