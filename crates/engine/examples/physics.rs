//! A tower of boxes knocked over by balls, with a sound on every impact.
//!
//! Space throws a ball where the camera looks; one is also thrown
//! automatically every two seconds. Escape quits.
//!
//! Run from the repository root with: `cargo run --example physics`

use engine::math::{Vec3, Vec4};
use engine::{
    Collider, Config, Context, DirectionalLight, Game, KeyCode, Material, Mesh, MeshData,
    MeshRenderer, Playback, RigidBody, Sound, Transform, WindowConfig,
};

/// Marks things that should be removed once they fall off the floor.
struct Debris;

#[derive(Default)]
struct PhysicsDemo {
    ball: Option<Mesh>,
    thud: Option<Sound>,
    balls_thrown: u32,
    next_throw: f32,
}

impl PhysicsDemo {
    fn throw_ball(&mut self, ctx: &mut Context) {
        let Some(mesh) = self.ball.clone() else {
            return;
        };
        let colors = [(0.9, 0.2, 0.2), (0.2, 0.6, 0.9), (0.95, 0.75, 0.1)];
        let (r, g, b) = colors[self.balls_thrown as usize % colors.len()];
        self.balls_thrown += 1;

        let direction = ctx.camera.forward();
        ctx.world.spawn((
            Transform::from_translation(ctx.camera.position + direction),
            MeshRenderer {
                mesh,
                material: Material {
                    metallic: 1.0,
                    roughness: 0.25,
                    ..Material::colored(r, g, b)
                },
            },
            Collider::sphere(0.35)
                .with_restitution(0.4)
                .with_density(4.0),
            RigidBody {
                // Fast and small: make sure it cannot tunnel through a box.
                continuous_collision: true,
                ..RigidBody::dynamic().with_velocity(direction * 16.0 + Vec3::Y * 2.0)
            },
            Debris,
        ));
    }
}

impl Game for PhysicsDemo {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.25, 0.45, 0.8);
        ctx.camera.position = Vec3::new(5.0, 3.5, 9.0);
        ctx.camera.look_at(Vec3::new(0.0, 1.5, 0.0));
        ctx.world.spawn((
            Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.4)),
            DirectionalLight::default(),
        ));

        let cube = ctx.renderer.create_mesh(&MeshData::cube(1.0));
        self.ball = Some(ctx.renderer.create_mesh(&MeshData::sphere(0.35, 32)));
        match ctx.load_sound("sounds/thud.wav") {
            Ok(sound) => self.thud = Some(sound),
            Err(e) => engine::log::warn!("{e}"),
        }

        // The floor: a unit cube stretched into a slab. The collider is a
        // unit cube too; the Transform's scale applies to both.
        ctx.world.spawn((
            Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(16.0, 1.0, 16.0)),
            MeshRenderer {
                mesh: cube.clone(),
                material: Material::colored(0.4, 0.45, 0.4),
            },
            Collider::cuboid(Vec3::ONE),
        ));

        // A pyramid of boxes, each a dynamic body.
        let levels = 5;
        for level in 0..levels {
            let count = levels - level;
            for i in 0..count {
                let x = (i as f32 - (count - 1) as f32 * 0.5) * 1.05;
                let shade = 0.55 + 0.1 * level as f32;
                ctx.world.spawn((
                    Transform::from_xyz(x, 0.5 + level as f32, 0.0),
                    MeshRenderer {
                        mesh: cube.clone(),
                        material: Material {
                            base_color: Vec4::new(shade, shade * 0.6, 0.3, 1.0),
                            roughness: 0.8,
                            ..Material::default()
                        },
                    },
                    Collider::cuboid(Vec3::ONE),
                    RigidBody::dynamic(),
                    Debris,
                ));
            }
        }
        self.next_throw = 2.0;
    }

    fn fixed_update(&mut self, ctx: &mut Context) {
        // One thud per new contact, a little different each time. Capped so
        // a collapsing tower does not start dozens of sounds in one step.
        if let Some(thud) = &self.thud {
            let started = ctx.physics.collisions().iter().filter(|c| c.started);
            for (i, _) in started.take(3).enumerate() {
                let playback = Playback {
                    volume: 0.6,
                    pitch: 0.8 + 0.2 * i as f32,
                    ..Playback::default()
                };
                ctx.audio.play_with(thud, playback);
            }
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // Lets automated runs stop on their own: PHYSICS_SECONDS=3.
        let elapsed = ctx.time.elapsed().as_secs_f32();
        if let Some(limit) = std::env::var("PHYSICS_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if elapsed >= limit {
                ctx.exit();
            }
        }

        if ctx.input.key_pressed(KeyCode::Space) || elapsed >= self.next_throw {
            self.next_throw = elapsed + 2.0;
            self.throw_ball(ctx);
        }

        // Despawning an entity also removes its body from the simulation.
        let fallen: Vec<_> = ctx
            .world
            .query::<(engine::Entity, &Transform)>()
            .with::<&Debris>()
            .iter()
            .filter(|(_, transform)| transform.translation.y < -20.0)
            .map(|(entity, _)| entity)
            .collect();
        for entity in fallen {
            let _ = ctx.world.despawn(entity);
        }
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Physics".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, PhysicsDemo::default())
}
