//! Skeletal and node animation loaded from glTF.
//!
//! The green stalks are one skinned mesh each, bent by a chain of four
//! joints; the robot's head turns by animating a single node.
//! Keys 1 and 2 switch the stalks between their two animations, Space
//! pauses, Escape quits.
//!
//! Run from the repository root with: `cargo run --example animation`

use engine::math::Vec3;
use engine::{
    AnimationPlayer, Config, Context, DirectionalLight, Entity, Game, KeyCode, MaterialDesc,
    Transform, Visual, WindowConfig,
};

#[derive(Default)]
struct AnimationDemo {
    stalks: Vec<Entity>,
}

impl AnimationDemo {
    fn play_on_stalks(&self, ctx: &mut Context, clip: &str) {
        for &stalk in &self.stalks {
            if let Ok(mut player) = ctx.world.get::<&mut AnimationPlayer>(stalk) {
                player.play(clip);
            }
        }
    }
}

impl Game for AnimationDemo {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.25, 0.45, 0.8);
        ctx.camera.position = Vec3::new(0.0, 3.2, 9.0);
        ctx.camera.look_at(Vec3::new(0.0, 1.8, 0.0));
        ctx.world.spawn((
            Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.6)),
            DirectionalLight::default(),
        ));
        ctx.world.spawn((
            Transform::IDENTITY,
            Visual::Plane {
                size: 40.0,
                uv_repeat: 1.0,
                material: MaterialDesc::colored(0.35, 0.4, 0.35),
            },
        ));

        let (stalk, robot) = match (
            ctx.load_model("models/stalk.gltf"),
            ctx.load_model("models/robot.gltf"),
        ) {
            (Ok(stalk), Ok(robot)) => (stalk, robot),
            (Err(e), _) | (_, Err(e)) => {
                engine::log::error!("{e}");
                ctx.exit();
                return;
            }
        };

        // Three instances of one model, each with its own animation state.
        for (i, x) in [-3.0, -1.5, 3.0].into_iter().enumerate() {
            let root = stalk.spawn(&mut ctx.world, Transform::from_xyz(x, 0.0, 0.0));
            if let Ok(mut player) = ctx.world.get::<&mut AnimationPlayer>(root) {
                player.play("Sway");
                player.speed = 0.7 + 0.3 * i as f32;
            }
            self.stalks.push(root);
        }

        let robot = robot.spawn(&mut ctx.world, Transform::from_xyz(0.8, 0.0, 0.5));
        if let Ok(mut player) = ctx.world.get::<&mut AnimationPlayer>(robot) {
            player.play("LookAround");
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // Lets automated runs stop on their own: ANIMATION_SECONDS=3.
        if let Some(limit) = std::env::var("ANIMATION_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if ctx.time.elapsed().as_secs_f32() >= limit {
                ctx.exit();
            }
        }

        if ctx.input.key_pressed(KeyCode::Digit1) {
            self.play_on_stalks(ctx, "Sway");
        }
        if ctx.input.key_pressed(KeyCode::Digit2) {
            self.play_on_stalks(ctx, "Twist");
        }
        if ctx.input.key_pressed(KeyCode::Space) {
            for player in ctx.world.query_mut::<&mut AnimationPlayer>() {
                player.paused = !player.paused;
            }
        }
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Animation".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, AnimationDemo::default())
}
